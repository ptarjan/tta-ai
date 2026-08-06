//! `arena` -- duel two weight vectors and say whether one is actually better.
//!
//! Port of what `experiments/arena.py::duel` does, and the piece the hill
//! climber is a loop around: `selfplay` measures a vector against other
//! KINDS, this measures it against another VECTOR of the same kind.
//!
//! ```text
//! arena --a challenger.json --b experiments/champion_3p.json --games 240 --threads 6
//! ```
//!
//! # The design is seat-paired, and that is the whole point
//!
//! One challenger (A) sits at a table of defenders (B) and is rotated through
//! every seat: game `g` puts A in seat `g % players` and deals seed
//! `seed0 + g / players`. So every deal is played `players` times with the
//! seats swapped, and §1.9's unfair seating order -- one civil action for the
//! first seat, four for the last -- cancels exactly instead of being averaged
//! over and hoped away.
//!
//! That pairing also means the games are NOT independent samples, so the
//! interval has to cluster on the deal rather than on the game. See
//! [`tta::stats`] for why that usually makes the interval NARROWER here, and
//! why the naive number is printed alongside rather than instead.
//!
//! # What it reports, and what it decides on
//!
//! The headline is A's win share against a null of `1 / players`, which is
//! what A would score if the two vectors were interchangeable. `lead` -- A's
//! final culture minus the BEST defender's -- is the secondary number, and is
//! what `docs/LEAGUE_OBJECTIVE.md` has the league train on. Its sign is the
//! game result exactly, because both come from the same maximum over the same
//! score list; the mean-of-defenders margin does NOT have that property (you
//! can beat the average and still come third), so it is not reported.
//!
//! `verdict accept` means the whole confidence interval clears the null, not
//! merely that the point estimate does.

use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use tta::bots::greedy::{build_bots, BotKind, Seat};
use tta::bots::weighted::eval::load_weights;
use tta::bots::weighted::weights::Weights;
use tta::game::{self, MOVE_CAP};
use tta::stats;

// ====================================================================== args

#[derive(Clone, Debug)]
struct Args {
    /// The challenger. Defaults to the built-in vector, so `arena --b x.json`
    /// asks the useful question "is the trained champion better than the
    /// defaults?" with no second file.
    a: Weights,
    b: Weights,
    name_a: String,
    name_b: String,
    /// Both seats play the same KIND; only the vectors differ. A duel across
    /// kinds is `selfplay --bots a,b`, which tallies by kind.
    kind: BotKind,
    games: usize,
    players: u8,
    seed: u64,
    threads: usize,
}

impl Default for Args {
    fn default() -> Args {
        Args {
            a: Weights::default(),
            b: Weights::default(),
            name_a: "defaults".to_string(),
            name_b: "defaults".to_string(),
            kind: BotKind::Weighted,
            games: 60,
            players: 3,
            seed: 0,
            threads: 1,
        }
    }
}

const USAGE: &str = "\
usage: arena [options]

  --a PATH        challenger weights (default: the built-in vector)
  --b PATH        defender weights   (default: the built-in vector)
  --kind KIND     bot kind both sides play (default weighted)
  --games N       games; rounded DOWN to a whole number of deals (default 60)
  --players N     2, 3 or 4 (default 3)
  --seed N        base deal seed (default 0)
  --threads N     games in parallel (default 1)
  --help

Exit status is 0 on a completed run whatever the verdict; it is non-zero only
if the run itself failed.  Read `verdict` for the result.
";

fn parse_args(argv: &[String]) -> Result<Option<Args>, String> {
    let mut a = Args::default();
    let mut it = argv.iter();
    while let Some(flag) = it.next() {
        let mut value = |flag: &str| -> Result<String, String> {
            it.next().cloned().ok_or_else(|| format!("{flag} needs a value"))
        };
        match flag.as_str() {
            "--a" => {
                let p = value(flag)?;
                a.a = load_weights(std::path::Path::new(&p))?;
                a.name_a = short_name(&p);
            }
            "--b" => {
                let p = value(flag)?;
                a.b = load_weights(std::path::Path::new(&p))?;
                a.name_b = short_name(&p);
            }
            "--kind" => a.kind = value(flag)?.parse()?,
            "--games" => a.games = parse_num(&value(flag)?, flag)?,
            "--players" => a.players = parse_num::<u8>(&value(flag)?, flag)?,
            "--seed" => a.seed = parse_num(&value(flag)?, flag)?,
            "--threads" => a.threads = parse_num(&value(flag)?, flag)?,
            "--help" | "-h" => {
                print!("{USAGE}");
                return Ok(None);
            }
            other => return Err(format!("unknown flag {other}\n\n{USAGE}")),
        }
    }

    if !(2..=4).contains(&a.players) {
        return Err(format!("--players must be 2, 3 or 4, got {}", a.players));
    }
    // A partial deal is a seat-biased observation, which is the one thing
    // this design exists to exclude -- so refuse to plan one rather than
    // playing it and having `deal_means` silently drop it later.
    let per_deal = a.players as usize;
    a.games -= a.games % per_deal;
    if a.games == 0 {
        return Err(format!("--games must be at least {per_deal} at {}p", a.players));
    }
    if a.threads == 0 {
        return Err("--threads must be at least 1".to_string());
    }
    Ok(Some(a))
}

fn parse_num<T: std::str::FromStr>(s: &str, flag: &str) -> Result<T, String> {
    s.parse().map_err(|_| format!("{flag}: {s:?} is not a number"))
}

/// A path's file stem, for the report's row labels. Purely cosmetic.
fn short_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

// =================================================================== playing

/// One game's result from A's point of view.
#[derive(Clone, Copy, Debug)]
struct Duel {
    /// 1.0 for a clean win, 1/n for an n-way tie, 0.0 otherwise.
    share: f64,
    culture_a: f64,
    /// The BEST defender's culture, not the mean -- see the module doc.
    culture_best_other: f64,
    moves: usize,
    cap_hit: bool,
}

/// Game `index`: A in seat `index % players`, deal `seed0 + index / players`.
fn play_one(args: &Args, index: usize) -> Duel {
    let players = args.players as usize;
    let seat = index % players;
    // `* 7919 + 17` keeps consecutive deals from being consecutive seeds, so
    // neighbouring deals do not share a prefix of the shuffle.
    let seed = (args.seed.wrapping_add((index / players) as u64))
        .wrapping_mul(7919)
        .wrapping_add(17);

    let seats: Vec<Seat> = (0..players)
        .map(|i| Seat {
            kind: args.kind,
            weights: if i == seat { args.a } else { args.b },
        })
        .collect();
    let mut bots = build_bots(&seats, seed as i64);

    let mut state = game::new_game(args.players, seed);
    let outcome = game::play_game(&mut state, MOVE_CAP, |s, _legal| {
        bots[s.current as usize].pick(s)
    });

    let winners = game::winners(&state);
    let share = if winners.contains(&(seat as u8)) { 1.0 / winners.len() as f64 } else { 0.0 };
    let culture = |i: usize| state.players[i].culture as f64;
    let best_other = (0..players)
        .filter(|i| *i != seat)
        .map(culture)
        .fold(f64::NEG_INFINITY, f64::max);

    Duel {
        share,
        culture_a: culture(seat),
        culture_best_other: best_other,
        moves: outcome.moves_played,
        cap_hit: outcome.move_cap_hit,
    }
}

// ================================================================= reporting

fn report(args: &Args, duels: &[Duel], elapsed: f64) {
    let players = args.players as usize;
    let null = 1.0 / players as f64;

    // `Some` everywhere: a game that hits the move cap is still a completed
    // game with a real winner. `stats::paired` takes `Option` because a
    // future runner may drop a game outright, and the placeholder is what
    // keeps a deal's seats recoverable by index.
    let shares: Vec<Option<f64>> = duels.iter().map(|d| Some(d.share)).collect();
    let leads: Vec<Option<f64>> =
        duels.iter().map(|d| Some(d.culture_a - d.culture_best_other)).collect();
    let win = stats::paired(&shares, players);
    let lead = stats::paired(&leads, players);

    let cap_hits = duels.iter().filter(|d| d.cap_hit).count();
    let mean = |f: fn(&Duel) -> f64| duels.iter().map(f).sum::<f64>() / duels.len() as f64;

    println!("games        {} ({} deals x {} seats)", duels.len(), win.n_deals, players);
    println!("players      {}", args.players);
    println!("kind         {}", args.kind.name());
    println!("A            {}", args.name_a);
    println!("B            {}", args.name_b);
    println!("mean moves   {:.1}", mean(|d| d.moves as f64));
    println!("elapsed      {elapsed:.1}s  ({:.1} games/s)", duels.len() as f64 / elapsed);
    if cap_hits > 0 {
        println!("WARNING      {cap_hits} game(s) hit the {MOVE_CAP}-move cap -- that is a bug");
    }
    println!();

    println!(
        "win rate     {:.1}%  +/- {:.1}   (null {:.1}%)   p = {:.4}",
        100.0 * win.mean,
        100.0 * win.half,
        100.0 * null,
        win.p_against(null),
    );
    println!(
        "             [naive +/- {:.1}, rho {:+.2}, deff {:.2}, {} deals]",
        100.0 * win.naive_half,
        win.rho,
        win.deff,
        win.n_deals,
    );
    println!(
        "culture      A {:.1}   best other {:.1}   lead {:+.1} +/- {:.1}",
        mean(|d| d.culture_a),
        mean(|d| d.culture_best_other),
        lead.mean,
        lead.half,
    );
    println!();

    // The gate: the whole interval clear of the null, not just the point.
    if win.beats(null) {
        println!("verdict      accept -- {} beats {} (interval clear of the null)", args.name_a, args.name_b);
    } else if win.hi() < null {
        println!("verdict      reject -- {} is WORSE than {}", args.name_a, args.name_b);
    } else {
        println!("verdict      inconclusive -- the interval still straddles the null");
    }
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_args(&argv) {
        Ok(Some(a)) => a,
        Ok(None) => return ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("arena: {e}");
            return ExitCode::FAILURE;
        }
    };

    let started = Instant::now();
    let next = AtomicUsize::new(0);

    // Threads claim whole games off a counter and each returns its own
    // `(index, result)` pairs; the indices are what put the list back in task
    // order below. Order is not cosmetic here the way it is in `selfplay`:
    // `stats::paired` recovers a game's deal and seat from its POSITION, so a
    // list in completion order would silently pair the wrong games together.
    let done: Vec<Vec<(usize, Duel)>> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..args.threads)
            .map(|_| {
                let (next, args) = (&next, &args);
                scope.spawn(move || {
                    let mut mine = Vec::new();
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        if index >= args.games {
                            return mine;
                        }
                        mine.push((index, play_one(args, index)));
                    }
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().expect("worker thread panicked")).collect()
    });

    let mut slots: Vec<Option<Duel>> = vec![None; args.games];
    for (index, duel) in done.into_iter().flatten() {
        slots[index] = Some(duel);
    }
    let duels: Vec<Duel> = slots.into_iter().map(|d| d.expect("every index was played")).collect();
    report(&args, &duels, started.elapsed().as_secs_f64());
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(games: usize, players: u8) -> Args {
        Args { games, players, ..Args::default() }
    }

    /// The pairing is the design: over a whole number of deals, A must sit in
    /// every seat the same number of times.
    #[test]
    fn the_challenger_visits_every_seat_equally() {
        for players in [2usize, 3, 4] {
            let mut counts = vec![0usize; players];
            for index in 0..(players * 5) {
                counts[index % players] += 1;
            }
            assert!(counts.iter().all(|c| *c == 5), "{players}p: {counts:?}");
        }
    }

    /// Every game of one deal must be the SAME deal -- if the seed moved with
    /// the seat, there would be nothing to pair.
    #[test]
    fn one_deals_games_all_share_a_seed() {
        let a = args(12, 3);
        let seed_of = |index: usize| {
            (a.seed.wrapping_add((index / 3) as u64)).wrapping_mul(7919).wrapping_add(17)
        };
        assert_eq!(seed_of(0), seed_of(1));
        assert_eq!(seed_of(1), seed_of(2));
        assert_ne!(seed_of(2), seed_of(3));
    }

    /// A partial deal is exactly the seat-biased observation the design
    /// exists to exclude, so `--games` rounds down to whole deals.
    #[test]
    fn games_round_down_to_whole_deals() {
        let parsed = parse_args(&["--games".into(), "10".into(), "--players".into(), "3".into()])
            .unwrap()
            .unwrap();
        assert_eq!(parsed.games, 9);
    }

    #[test]
    fn too_few_games_for_even_one_deal_is_an_error() {
        let e = parse_args(&["--games".into(), "3".into(), "--players".into(), "4".into()]);
        assert!(e.is_err(), "{e:?}");
    }

    #[test]
    fn player_counts_outside_the_base_game_are_rejected() {
        assert!(parse_args(&["--players".into(), "5".into()]).is_err());
        assert!(parse_args(&["--players".into(), "1".into()]).is_err());
    }

    #[test]
    fn an_unknown_kind_is_rejected_before_any_game_is_played() {
        assert!(parse_args(&["--kind".into(), "not_a_bot".into()]).is_err());
    }

    /// Two identical vectors must land on the null, not above it: this is the
    /// test that would catch A being handed an advantage by the harness
    /// itself rather than by its weights.
    #[test]
    fn identical_vectors_split_the_wins_evenly() {
        let a = args(9, 3);
        let duels: Vec<Duel> = (0..a.games).map(|i| play_one(&a, i)).collect();
        let total: f64 = duels.iter().map(|d| d.share).sum();
        // 3 deals, 3 seats each, A is one of three identical bots: A takes
        // exactly one share per deal because the same game is replayed with A
        // in each seat in turn.
        assert!((total - 3.0).abs() < 1e-9, "shares summed to {total}");
    }

    #[test]
    fn the_same_arguments_play_the_same_duel() {
        let a = args(6, 2);
        let first: Vec<f64> = (0..a.games).map(|i| play_one(&a, i).culture_a).collect();
        let again: Vec<f64> = (0..a.games).map(|i| play_one(&a, i).culture_a).collect();
        assert_eq!(first, again);
    }
}
