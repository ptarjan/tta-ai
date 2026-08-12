//! `budgetcheck` -- how often does `plan::PlanConfig::max_nodes` actually
//! bind?
//!
//! The follow-up to `docs/NEURAL.md`'s "The policy head" section asks a
//! cheap question before spending a single game on the expensive one: at a
//! chosen node budget, what fraction of REAL self-play decisions are cut
//! short by that budget (`plan::Stats::searches_capped`) rather than
//! finishing their own tree with room to spare? A move-ordering prior can
//! only matter when the answer is well above zero -- reordering candidates
//! nobody was going to examine anyway changes nothing.
//!
//! Plays ordinary `plan`-vs-itself self-play games with every seat's
//! `plan::PlanConfig` built by hand at `width: 2` -- matching
//! `bots::greedy::build_bots`'s own `BotKind::Plan` override, so this
//! measures the exact search `kindmatch`'s `plan` seats run, not a
//! differently-shaped stand-in -- and sums `Stats::searches`/
//! `searches_capped` across every seat and every game. No policy prior is
//! loaded: `Stats::searches_capped`'s own doc comment notes ordering does
//! not change the total node count needed to finish a tree at unlimited
//! budget, only which nodes a STARVED search reaches, so a plain `Bot::Plan`
//! reading is the right proxy for "is this budget binding at all" without
//! needing a checkpoint on disk.
//!
//! ```text
//! cargo run --profile difftest --bin budgetcheck -- \
//!     --max-nodes 4000 --games 60 --players 2 --threads 2 --seed 2026
//! ```

use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use tta::bots::pending;
use tta::bots::plan;
use tta::bots::weighted::weights::Weights;
use tta::game::{self, MOVE_CAP};
use tta::rng::PyRandom;

#[derive(Clone, Debug)]
struct Args {
    max_nodes: i64,
    games: usize,
    players: u8,
    seed: u64,
    threads: usize,
}

impl Default for Args {
    fn default() -> Args {
        Args {
            max_nodes: plan::PlanConfig::default().max_nodes,
            games: 60,
            players: 2,
            seed: 0,
            threads: 1,
        }
    }
}

const USAGE: &str = "\
usage: budgetcheck [options]

  --max-nodes N   plan::PlanConfig::max_nodes for every seat (default 4000)
  --games N       games; rounded down to a whole number of deals (default 60)
  --players N     2, 3 or 4 (default 2)
  --seed N        base deal seed (default 0)
  --threads N     games in parallel (default 1)
  --help
";

fn parse_args(argv: &[String]) -> Result<Option<Args>, String> {
    let mut a = Args::default();
    let mut it = argv.iter();
    while let Some(flag) = it.next() {
        let mut value = |flag: &str| -> Result<String, String> {
            it.next().cloned().ok_or_else(|| format!("{flag} needs a value"))
        };
        match flag.as_str() {
            "--max-nodes" => a.max_nodes = parse_num(&value(flag)?, flag)?,
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
    if a.threads == 0 {
        return Err("--threads must be at least 1".to_string());
    }
    if a.max_nodes <= 0 {
        return Err(format!("--max-nodes must be positive, got {}", a.max_nodes));
    }
    let per_deal = a.players as usize;
    a.games -= a.games % per_deal;
    if a.games == 0 {
        return Err(format!("--games must be at least {per_deal} at {}p", a.players));
    }
    Ok(Some(a))
}

fn parse_num<T: std::str::FromStr>(s: &str, flag: &str) -> Result<T, String> {
    s.parse().map_err(|_| format!("{flag}: {s:?} is not a number"))
}

/// One seat's [`plan::Stats`] tally from a single game -- the two counters
/// [`main`] actually pools, not the whole struct, so a caller of this module
/// cannot accidentally sum `nodes`/`wars_priced` into a total that means
/// nothing here.
struct SeatTally {
    searches: u64,
    capped: u64,
}

/// Play one game with every seat a plain `plan::pick` search at
/// `args.max_nodes` (`width: 2`, matching `build_bots`'s own
/// `BotKind::Plan` override) -- no policy prior, see this module's own top
/// doc comment for why that is the right proxy here. Seeded identically to
/// `kindmatch::play_one`'s deal scheme (`index / players` selects the deal,
/// `seed * 131 + i` seeds each seat), so a caller running both tools at the
/// same `--seed` samples the same deals, even though this binary has no `a`/
/// `b` split of its own to make that matter yet.
fn play_one(args: &Args, index: usize) -> Vec<SeatTally> {
    let players = args.players as usize;
    let seed = (args.seed.wrapping_add((index / players) as u64)).wrapping_mul(7919).wrapping_add(17);

    let mut bots: Vec<(plan::PlanConfig, plan::Stats, pending::Counters, PyRandom)> = (0..players)
        .map(|i| {
            let player_seed = (seed as i64).wrapping_mul(131).wrapping_add(i as i64);
            let cfg = plan::PlanConfig {
                width: 2,
                max_nodes: args.max_nodes,
                weights: Weights::defaults(),
                ..plan::PlanConfig::default()
            };
            (cfg, plan::Stats::default(), pending::Counters::default(), PyRandom::new(player_seed.into()))
        })
        .collect();

    let mut state = game::new_game(args.players, seed);
    let outcome = game::play_game(&mut state, MOVE_CAP, |s, legal| {
        let (cfg, stats, counters, rng) = &mut bots[s.current as usize];
        plan::pick(cfg, stats, counters, rng, s, legal.as_slice())
    });
    if outcome.move_cap_hit {
        eprintln!("budgetcheck: WARNING game at seed {seed} hit the {MOVE_CAP}-move cap");
    }

    bots.into_iter()
        .map(|(_, stats, _, _)| SeatTally { searches: stats.searches, capped: stats.searches_capped })
        .collect()
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_args(&argv) {
        Ok(Some(a)) => a,
        Ok(None) => return ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("budgetcheck: {e}");
            return ExitCode::FAILURE;
        }
    };

    let started = Instant::now();
    let next = AtomicUsize::new(0);
    let done: Vec<Vec<SeatTally>> = std::thread::scope(|scope| {
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
                        mine.extend(play_one(args, index));
                    }
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().expect("worker thread panicked")).collect()
    });
    let elapsed = started.elapsed().as_secs_f64();

    let (searches, capped) = done
        .into_iter()
        .flatten()
        .fold((0u64, 0u64), |(s, c), t| (s + t.searches, c + t.capped));

    println!("games        {} ({:.1} games/s)", args.games, args.games as f64 / elapsed);
    println!("players      {}", args.players);
    println!("max_nodes    {}", args.max_nodes);
    println!("elapsed      {elapsed:.1}s");
    println!();
    println!(
        "searches     {searches}   capped {capped}   ({:.2}% of decisions hit the max_nodes budget)",
        100.0 * capped as f64 / searches.max(1) as f64,
    );
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The un-overridden default must be `plan::PlanConfig`'s OWN default
    /// (4000 today) -- pinned by reading the real constant rather than a
    /// copied literal, so a future change to `PlanConfig::default()` cannot
    /// silently leave this binary measuring a stale budget.
    #[test]
    fn default_max_nodes_matches_plan_configs_own_default() {
        let args = Args::default();
        assert_eq!(args.max_nodes, plan::PlanConfig::default().max_nodes);
    }

    #[test]
    fn parse_args_reads_max_nodes() {
        let argv = vec!["--max-nodes".to_string(), "400".to_string()];
        let args = parse_args(&argv).unwrap().unwrap();
        assert_eq!(args.max_nodes, 400);
    }

    #[test]
    fn parse_args_rejects_a_nonpositive_max_nodes() {
        let argv = vec!["--max-nodes".to_string(), "0".to_string()];
        assert!(parse_args(&argv).is_err(), "a zero node budget can never run a search");
    }

    /// Same `(args, index)` must draw the same deal and the same search
    /// trace every time -- `plan::pick`'s own rng is reseeded from
    /// `state.seed`/`state.turn`/`me` on every call (`plan.rs`'s own top doc
    /// comment), so nothing here is a caller-owned stream that could drift
    /// between two calls in the same process.
    #[test]
    fn play_one_is_deterministic_for_a_fixed_seed_and_index() {
        let args = Args { max_nodes: 200, games: 2, players: 2, seed: 5, threads: 1 };
        let searches_a: u64 = play_one(&args, 0).iter().map(|t| t.searches).sum();
        let searches_b: u64 = play_one(&args, 0).iter().map(|t| t.searches).sum();
        assert_eq!(searches_a, searches_b);
        assert!(searches_a > 0, "a real game must make at least one search");
    }

    /// A budget far too small to expand even the ROOT candidate list of most
    /// decisions must show up as (almost always) capped -- the coarse
    /// negative-space check that this binary's whole point (reading
    /// `Stats::searches_capped`) is actually wired to a real, varying
    /// signal, not a constant.
    #[test]
    fn a_tiny_budget_caps_far_more_often_than_a_generous_one() {
        let tiny = Args { max_nodes: 2, games: 2, players: 2, seed: 5, threads: 1 };
        let generous = Args { max_nodes: 4000, games: 2, players: 2, seed: 5, threads: 1 };
        let (mut tiny_s, mut tiny_c) = (0u64, 0u64);
        for t in play_one(&tiny, 0) {
            tiny_s += t.searches;
            tiny_c += t.capped;
        }
        let (mut gen_s, mut gen_c) = (0u64, 0u64);
        for t in play_one(&generous, 0) {
            gen_s += t.searches;
            gen_c += t.capped;
        }
        assert!(tiny_s > 0 && gen_s > 0, "both runs must make at least one search");
        let tiny_frac = tiny_c as f64 / tiny_s as f64;
        let gen_frac = gen_c as f64 / gen_s as f64;
        assert!(
            tiny_frac > gen_frac,
            "max_nodes=2 ({tiny_frac}) should cap far more often than max_nodes=4000 ({gen_frac})"
        );
    }
}
