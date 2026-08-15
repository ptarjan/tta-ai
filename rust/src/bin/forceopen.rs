//! `forceopen` -- the payoff half of the "canonical openings" measurement:
//! does FORCING the mined human-opening divergences onto the champion
//! actually win it more games, or was the divergence only ever a
//! convention?
//!
//! Reuses `tta::arena`'s seat-paired design and its clustered-interval
//! [`Summary`] wholesale (see that module's own top doc comment for why the
//! pairing and the clustering are load-bearing) -- the only thing new here
//! is HOW one game gets played: seat A's decisions are routed through
//! [`tta::opening_force::pick_with_optional_force`] with a live
//! [`OpeningTracker`], seat B (and A once its opening is complete) plays the
//! SAME champion vector's ordinary unconstrained choice.
//!
//! Both seats read the SAME weight vector (`--champion`, default the frozen
//! 2p champion) -- this experiment is not "vector A vs vector B", it is
//! "the champion forced to open one way vs the champion left alone", so a
//! `--a`/`--b` pair of different files would be answering a different
//! question.
//!
//! ```text
//! forceopen --policy mine-first --games 300 --seed 0 --threads 2
//! ```

use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use tta::arena::{Duel, Summary};
use tta::bots::greedy::{build_bots, BotKind, Search, Seat};
use tta::bots::weighted::eval::load_weights;
use tta::bots::weighted::weights::Weights;
use tta::game::{self, MOVE_CAP};
use tta::opening_force::{pick_with_optional_force, OpeningPolicy, OpeningTracker};

const DEFAULT_CHAMPION: &str = "/private/tmp/rowdig/frozen_champion_2p.json";

fn parse_policy(s: &str) -> Result<OpeningPolicy, String> {
    match s {
        "unforced" => Ok(OpeningPolicy::Unforced),
        "mine-first" => Ok(OpeningPolicy::MineFirst),
        "leader-by-round-three" => Ok(OpeningPolicy::LeaderByRoundThree),
        "mine-first-and-leader" => Ok(OpeningPolicy::MineFirstAndLeader),
        "military-first" => Ok(OpeningPolicy::MilitaryFirst),
        other => Err(format!(
            "unknown --policy {other:?}, want one of: unforced, mine-first, leader-by-round-three, \
             mine-first-and-leader, military-first"
        )),
    }
}

struct Args {
    policy: OpeningPolicy,
    games: usize,
    seed: u64,
    threads: usize,
    champion: String,
}

impl Default for Args {
    fn default() -> Args {
        Args { policy: OpeningPolicy::MineFirst, games: 300, seed: 0, threads: 1, champion: DEFAULT_CHAMPION.to_string() }
    }
}

const USAGE: &str = "\
usage: forceopen --policy POLICY [options]

  --policy POLICY   unforced | mine-first | leader-by-round-three |
                     mine-first-and-leader | military-first  (required)
  --games N         games; rounded DOWN to a whole number of 2p deals (default 300)
  --seed N          base deal seed (default 0)
  --threads N       games in parallel (default 1)
  --champion PATH   weight vector BOTH seats play (default the frozen 2p champion)
  --help

Seat A (rotated through every seat, same as `arena`) is forced under
--policy; every other seat plays the SAME vector's ordinary unconstrained
choice. 2-player only -- see this file's own doc comment.
";

fn parse_args(argv: &[String]) -> Result<Option<Args>, String> {
    let mut a = Args::default();
    let mut policy_given = false;
    let mut it = argv.iter();
    while let Some(flag) = it.next() {
        let mut value = |flag: &str| -> Result<String, String> {
            it.next().cloned().ok_or_else(|| format!("{flag} needs a value"))
        };
        match flag.as_str() {
            "--policy" => {
                a.policy = parse_policy(&value(flag)?)?;
                policy_given = true;
            }
            "--games" => a.games = value(flag)?.parse().map_err(|_| "bad --games".to_string())?,
            "--seed" => a.seed = value(flag)?.parse().map_err(|_| "bad --seed".to_string())?,
            "--threads" => a.threads = value(flag)?.parse().map_err(|_| "bad --threads".to_string())?,
            "--champion" => a.champion = value(flag)?,
            "--help" | "-h" => {
                print!("{USAGE}");
                return Ok(None);
            }
            other => return Err(format!("unknown flag {other}\n\n{USAGE}")),
        }
    }
    if !policy_given {
        return Err(format!("--policy is required\n\n{USAGE}"));
    }
    a.games -= a.games % 2;
    if a.games == 0 {
        return Err("--games must be at least 2 (a whole 2p deal)".to_string());
    }
    if a.threads == 0 {
        return Err("--threads must be at least 1".to_string());
    }
    Ok(Some(a))
}

/// One goal's tally across a whole arm's games: how often it was actually
/// achieved by the end of the forced window, and how the forcing that
/// pursued it fared. Zero-valued for a goal a policy never pursues (e.g.
/// `leader` under `MineFirst`) -- reported but not a claim.
#[derive(Clone, Copy, Debug, Default)]
struct GoalTally {
    achieved: u32,
    forced: u32,
    fallthrough: u32,
}

impl GoalTally {
    fn add(&mut self, achieved: bool, forced: u32, fallthrough: u32) {
        self.achieved += achieved as u32;
        self.forced += forced;
        self.fallthrough += fallthrough;
    }

    /// Fallthrough rate among decisions where the forcing actually had an
    /// opinion (forced + fell through) -- decisions where the goal simply
    /// didn't apply are not in this denominator at all (see
    /// `opening_force`'s own doc comment on why that keeps this meaningful).
    fn fallthrough_rate(self) -> f64 {
        let denom = self.forced + self.fallthrough;
        if denom == 0 {
            0.0
        } else {
            self.fallthrough as f64 / denom as f64
        }
    }
}

/// Play game `index` of the arm: seat A = `index % 2` (mirrors
/// `tta::arena::Match::play_one`'s own seating formula exactly, so this
/// pairs the same way `arena`'s does), forced under `policy`; the other
/// seat plays the same vector unconstrained.
fn play_one(policy: OpeningPolicy, weights: Weights, seed0: u64, index: usize) -> (Duel, GoalTally, GoalTally, GoalTally) {
    let players = 2usize;
    let a_seat = index % players;
    let seed = (seed0.wrapping_add((index / players) as u64)).wrapping_mul(7919).wrapping_add(17);

    let seat = Seat { kind: BotKind::Weighted, weights, search: Search::None };
    let mut bots = build_bots(&[seat, seat], seed as i64);
    let mut tracker = OpeningTracker::new(policy);

    let mut state = game::new_game(players as u8, seed);
    let outcome = game::play_game(&mut state, MOVE_CAP, |s, legal| {
        let actor = s.current as usize;
        if actor == a_seat {
            pick_with_optional_force(&mut bots[actor], Some(&mut tracker), s, legal.as_slice())
        } else {
            pick_with_optional_force(&mut bots[actor], None, s, legal.as_slice())
        }
    });

    let winners = game::winners(&state);
    let share = if winners.contains(&(a_seat as u8)) { 1.0 / winners.len() as f64 } else { 0.0 };
    let culture = |i: usize| state.players[i].culture as f64;
    let best_other =
        (0..players).filter(|i| *i != a_seat).map(culture).fold(f64::NEG_INFINITY, f64::max);

    let duel = Duel {
        share,
        culture_a: culture(a_seat),
        culture_best_other: best_other,
        moves: outcome.moves_played,
        cap_hit: outcome.move_cap_hit,
    };

    let mut mine = GoalTally::default();
    let mut leader = GoalTally::default();
    let mut military = GoalTally::default();
    mine.add(tracker.achieved_mine(), tracker.mine.forced, tracker.mine.fallthrough);
    leader.add(tracker.achieved_leader(), tracker.leader.forced, tracker.leader.fallthrough);
    military.add(tracker.achieved_military(), tracker.military.forced, tracker.military.fallthrough);
    (duel, mine, leader, military)
}

/// One worker thread's share of results: `(index, duel, three goal tallies)`
/// per game it played, still tagged with its original index so the caller
/// can flatten threads back into submission order.
type ThreadDuelResults = Vec<(usize, Duel, GoalTally, GoalTally, GoalTally)>;

fn play_all(args: &Args, weights: Weights) -> (Vec<Duel>, GoalTally, GoalTally, GoalTally) {
    let next = AtomicUsize::new(0);
    let done: Vec<ThreadDuelResults> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..args.threads)
            .map(|_| {
                let next = &next;
                scope.spawn(move || {
                    let mut mine = Vec::new();
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        if index >= args.games {
                            return mine;
                        }
                        let (d, m, l, mil) = play_one(args.policy, weights, args.seed, index);
                        mine.push((index, d, m, l, mil));
                    }
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().expect("worker thread panicked")).collect()
    });

    let mut slots: Vec<Option<(Duel, GoalTally, GoalTally, GoalTally)>> = vec![None; args.games];
    for (index, d, m, l, mil) in done.into_iter().flatten() {
        slots[index] = Some((d, m, l, mil));
    }

    let mut duels = Vec::with_capacity(args.games);
    let mut mine_total = GoalTally::default();
    let mut leader_total = GoalTally::default();
    let mut military_total = GoalTally::default();
    for slot in slots {
        let (d, m, l, mil) = slot.expect("every index was played");
        duels.push(d);
        mine_total.achieved += m.achieved;
        mine_total.forced += m.forced;
        mine_total.fallthrough += m.fallthrough;
        leader_total.achieved += l.achieved;
        leader_total.forced += l.forced;
        leader_total.fallthrough += l.fallthrough;
        military_total.achieved += mil.achieved;
        military_total.forced += mil.forced;
        military_total.fallthrough += mil.fallthrough;
    }
    (duels, mine_total, leader_total, military_total)
}

fn report(args: &Args, s: &Summary, games: usize, mine: GoalTally, leader: GoalTally, military: GoalTally, elapsed: f64) {
    let null = 0.5; // 2p, forced seat vs one unforced defender
    println!("policy       {:?}", args.policy);
    println!("games        {} ({} deals x 2 seats)", games, s.win.n_deals);
    println!("champion     {}", args.champion);
    println!("elapsed      {elapsed:.1}s  ({:.1} games/s)", games as f64 / elapsed);
    if s.cap_hits > 0 {
        println!("WARNING      {} game(s) hit the {MOVE_CAP}-move cap -- that is a bug", s.cap_hits);
    }
    println!();
    println!(
        "win rate     {:.1}%  +/- {:.1}   (null 50.0%)   p = {:.4}",
        100.0 * s.win.mean,
        100.0 * s.win.half,
        s.win.p_against(null),
    );
    println!(
        "             [naive +/- {:.1}, rho {:+.2}, deff {:.2}, {} deals]",
        100.0 * s.win.naive_half,
        s.win.rho,
        s.win.deff,
        s.win.n_deals,
    );
    println!();
    let report_goal = |name: &str, g: GoalTally| {
        println!(
            "{name:<9} achieved {:>5.1}%  ({}/{})   forced-decisions {}   fallthrough-decisions {}   fallthrough-rate {:.1}%",
            100.0 * g.achieved as f64 / games as f64,
            g.achieved,
            games,
            g.forced,
            g.fallthrough,
            100.0 * g.fallthrough_rate(),
        );
    };
    report_goal("mine", mine);
    report_goal("leader", leader);
    report_goal("military", military);
    println!();
    if s.win.beats(null) {
        println!("verdict      the forced seat wins MORE (interval clear above 50%)");
    } else if s.win.hi() < null {
        println!("verdict      the forced seat wins LESS (interval clear below 50%)");
    } else {
        println!("verdict      inconclusive -- the interval straddles 50%");
    }
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_args(&argv) {
        Ok(Some(a)) => a,
        Ok(None) => return ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("forceopen: {e}");
            return ExitCode::FAILURE;
        }
    };
    let weights = match load_weights(std::path::Path::new(&args.champion)) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("forceopen: loading {}: {e}", args.champion);
            return ExitCode::FAILURE;
        }
    };

    let started = Instant::now();
    let (duels, mine, leader, military) = play_all(&args, weights);
    let summary = Summary::of(&duels, 2);
    report(&args, &summary, duels.len(), mine, leader, military, started.elapsed().as_secs_f64());
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `--games` must round down to a whole number of 2p deals, exactly the
    /// same rule `arena`'s own `Match::validate` enforces -- a partial deal
    /// is a seat-biased observation.
    #[test]
    fn games_round_down_to_a_whole_number_of_deals() {
        let parsed = parse_args(&["--policy".into(), "mine-first".into(), "--games".into(), "7".into()])
            .unwrap()
            .unwrap();
        assert_eq!(parsed.games, 6);
    }

    /// `--policy` is not optional: a run with no policy silently defaulting
    /// to something is exactly the class of mistake that would let a
    /// forgotten flag report a meaningless number as this experiment's
    /// headline.
    #[test]
    fn a_run_with_no_policy_flag_is_rejected() {
        assert!(parse_args(&["--games".into(), "300".into()]).is_err());
    }

    #[test]
    fn an_unknown_policy_name_is_rejected() {
        assert!(parse_policy("wonder-first").is_err());
    }

    /// Every name `parse_policy` accepts must round-trip through the SAME
    /// `--policy` flag `parse_args` reads -- pins the two functions against
    /// drifting (a policy `parse_policy` accepts but `USAGE` doesn't
    /// document, or vice versa, is a real footgun for whoever runs this next).
    #[test]
    fn every_documented_policy_name_parses() {
        for name in ["unforced", "mine-first", "leader-by-round-three", "mine-first-and-leader", "military-first"] {
            assert!(parse_policy(name).is_ok(), "{name} should parse");
        }
    }

    /// A `GoalTally` that never fired (a goal the policy under test doesn't
    /// pursue) must report a 0% fallthrough rate, not divide by zero.
    #[test]
    fn fallthrough_rate_of_an_untouched_goal_is_zero_not_nan() {
        let g = GoalTally::default();
        assert_eq!(g.fallthrough_rate(), 0.0);
    }
}
