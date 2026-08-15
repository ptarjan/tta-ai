//! `orderprobe` -- does [`PolicyOrder`] actually change what [`plan::beam`]
//! DOES, or does it only ever reorder candidates a search was going to
//! finish examining anyway?
//!
//! `docs/NEURAL.md`'s policy-head follow-up ran two plan-vs-plan
//! head-to-heads (policy on one seat, off the other) and got a null both
//! times -- 51.22% +/- 2.40 at `--max-nodes 4000`, 51.08% +/- 2.44 at
//! `--max-nodes 60`. A null there is ambiguous on its own: "the prior adds
//! no playing strength" and "the prior never actually runs" are
//! INDISTINGUISHABLE from a win rate alone. This binary settles which one
//! it is, cheaply, before spending any more games on head-to-heads.
//!
//! At every real decision of an ordinary `plan`-vs-`plan` self-play game
//! (same shape as [`crate::budgetcheck`]'s: `width: 2`, matching
//! `bots::greedy::build_bots`'s own `BotKind::Plan` override), this SEARCHES
//! THE IDENTICAL POSITION TWICE -- once through [`plan::pick_collecting`]
//! with `policy: None` (also the move that actually advances the game, so
//! the trajectory this binary walks is byte-for-byte the same self-play
//! `budgetcheck` would have played at the same seed), once with
//! `policy: Some(&mut policy)` -- and tallies three independent counters:
//!
//!   - `permuted`: [`PolicyOrder::order_moves`] run directly on that
//!     decision's raw legal-move list produced an order different from the
//!     input order. This is the cheapest possible check that the prior is
//!     even LIVE: a checkpoint that failed to load a real signal (e.g. an
//!     all-zero or degenerate net) would show ~0% here regardless of node
//!     budget, no self-play needed to see it -- see this file's own top
//!     doc comment's INTERPRETATION note.
//!   - `different`: the two searches returned a DIFFERENT chosen move.
//!   - `capped`: either search hit `cfg.max_nodes` before finishing its own
//!     tree ([`plan::Stats::searches_capped`]), the same signal
//!     `budgetcheck` reads, given a fresh [`plan::Stats`] per shadow call so
//!     this is a per-DECISION flag, not a whole-game accumulator.
//!
//! Both shadow searches read from an identical snapshot of the driving
//! seat's [`pending::Counters`] and [`PyRandom`] stream (cloned before the
//! real, game-advancing call mutates them) -- so any difference between the
//! two is attributable to the policy prior alone, not to the pending-branch
//! bookkeeping or an RNG stream that drifted between the two calls.
//!
//! ```text
//! cargo run --profile difftest --bin orderprobe -- \
//!     --policy-checkpoint control.ckpt --max-nodes 60 --games 12 \
//!     --players 2 --threads 2 --seed 7
//! ```
//!
//! # INTERPRETATION (state this in every report that cites this binary)
//!
//! - `permuted` ~0%: the prior is inert -- wiring is broken, and BOTH
//!   existing head-to-head nulls are void (neither one ever exercised the
//!   thing it claims to have measured).
//! - `permuted` high, `different` ~0% at a generous budget (e.g. 4000):
//!   CONFIRMS the already-known structural finding -- an unstarved search
//!   reaches the same answer regardless of candidate order -- and the
//!   integration is fine, just inert exactly where it was expected to be.
//! - `different` materially non-zero at a starved budget (e.g. 60): the
//!   mechanism works, and a head-to-head null AT THAT BUDGET is a real
//!   negative result, not an artifact of a prior that never ran.

use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use tta::bots::neural::net::ValueNet;
use tta::bots::neural::policy_order::PolicyOrder;
use tta::bots::neural::policy_train::load_policy_checkpoint;
use tta::bots::pending;
use tta::bots::plan;
use tta::bots::weighted::weights::Weights;
use tta::game::{self, MOVE_CAP};
use tta::moves::Move;
use tta::rng::PyRandom;

#[derive(Clone, Debug)]
struct Args {
    policy_checkpoint: String,
    max_nodes: i64,
    games: usize,
    players: u8,
    seed: u64,
    threads: usize,
}

impl Default for Args {
    fn default() -> Args {
        Args {
            policy_checkpoint: String::new(),
            max_nodes: plan::PlanConfig::default().max_nodes,
            games: 12,
            players: 2,
            seed: 0,
            threads: 1,
        }
    }
}

const USAGE: &str = "\
usage: orderprobe --policy-checkpoint PATH [options]

  --policy-checkpoint PATH  a TTAPOL01 policy checkpoint, loaded once
  --max-nodes N   plan::PlanConfig::max_nodes for every seat (default 4000)
  --games N       games; rounded down to a whole number of deals (default 12)
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
            "--policy-checkpoint" => a.policy_checkpoint = value(flag)?,
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
    if a.policy_checkpoint.is_empty() {
        return Err(format!("--policy-checkpoint is required\n\n{USAGE}"));
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

/// One game's tally of this binary's three counters, plus the number of
/// real decisions (`legal.len() > 1`, the only decisions either the prior
/// or a shadow search could possibly affect) they were counted over.
#[derive(Default)]
struct DecisionTally {
    decisions: u64,
    permuted: u64,
    different: u64,
    capped: u64,
}

impl DecisionTally {
    fn add(&mut self, other: &DecisionTally) {
        self.decisions += other.decisions;
        self.permuted += other.permuted;
        self.different += other.different;
        self.capped += other.capped;
    }
}

/// Play one `plan`-vs-`plan` self-play game (`width: 2`, matching
/// `budgetcheck::play_one` and `bots::greedy::build_bots`'s own
/// `BotKind::Plan` override), and at every real decision, shadow-search the
/// identical position with the policy prior on -- see this file's own top
/// doc comment for the three counters and why the shadow call reads cloned
/// `pending::Counters`/`PyRandom` snapshots rather than the live ones.
///
/// Seeded identically to `budgetcheck::play_one`/`kindmatch::play_one`'s
/// deal scheme, so a caller running this alongside either tool at the same
/// `--seed` samples the same deals.
fn play_one(args: &Args, index: usize, policy_net: &Arc<ValueNet>) -> DecisionTally {
    let players = args.players as usize;
    let seed = (args.seed.wrapping_add((index / players) as u64)).wrapping_mul(7919).wrapping_add(17);

    let mut seats: Vec<(plan::PlanConfig, pending::Counters, PyRandom)> = (0..players)
        .map(|i| {
            let player_seed = (seed as i64).wrapping_mul(131).wrapping_add(i as i64);
            let cfg = plan::PlanConfig {
                width: 2,
                max_nodes: args.max_nodes,
                weights: Weights::defaults(),
                ..plan::PlanConfig::default()
            };
            (cfg, pending::Counters::default(), PyRandom::new(player_seed.into()))
        })
        .collect();

    // One `PolicyOrder` per game, built from the shared `Arc<ValueNet>` --
    // exactly `kindmatch::play_one`'s pattern (`Bot::plan_with_policy`'s own
    // doc comment): the checkpoint loads once in `main`, every game clones
    // only the small in-memory net into its own scratch buffers.
    let mut policy = PolicyOrder::from_net((**policy_net).clone());

    let mut tally = DecisionTally::default();
    let mut state = game::new_game(args.players, seed);
    let outcome = game::play_game(&mut state, MOVE_CAP, |s, legal| {
        let me = s.current as usize;
        let (cfg, counters, rng) = &mut seats[me];
        let legal_slice = legal.as_slice();

        if legal_slice.len() > 1 {
            tally.decisions += 1;

            // `permuted`: run the prior directly on the raw legal-move list
            // and compare -- the cheapest possible "is this even live" check
            // (see this file's own top doc comment), independent of
            // `pick_collecting`'s own internal resign filtering.
            let mut probe: Vec<Move> = legal_slice.to_vec();
            policy.order_moves(s, s.current, &mut probe);
            if probe.as_slice() != legal_slice {
                tally.permuted += 1;
            }

            // Snapshot BEFORE the real, game-advancing call mutates them, so
            // the shadow "on" call below starts from the identical
            // conditions the "off" call saw.
            let counters_snapshot = *counters;
            let rng_snapshot = rng.clone();

            let mut stats_off = plan::Stats::default();
            let mv_off =
                plan::pick_collecting(cfg, &mut stats_off, counters, rng, s, legal_slice, &mut plan::Bank::Off, None);

            let mut stats_on = plan::Stats::default();
            let mut counters_on = counters_snapshot;
            let mut rng_on = rng_snapshot;
            let mv_on = plan::pick_collecting(
                cfg,
                &mut stats_on,
                &mut counters_on,
                &mut rng_on,
                s,
                legal_slice,
                &mut plan::Bank::Off,
                Some(&mut policy),
            );

            if mv_off != mv_on {
                tally.different += 1;
            }
            if stats_off.searches_capped > 0 || stats_on.searches_capped > 0 {
                tally.capped += 1;
            }

            mv_off
        } else {
            let mut stats = plan::Stats::default();
            plan::pick_collecting(cfg, &mut stats, counters, rng, s, legal_slice, &mut plan::Bank::Off, None)
        }
    });
    if outcome.move_cap_hit {
        eprintln!("orderprobe: WARNING game at seed {seed} hit the {MOVE_CAP}-move cap");
    }

    tally
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_args(&argv) {
        Ok(Some(a)) => a,
        Ok(None) => return ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("orderprobe: {e}");
            return ExitCode::FAILURE;
        }
    };

    let policy_net: Arc<ValueNet> = match load_policy_checkpoint(std::path::Path::new(&args.policy_checkpoint)) {
        Ok((net, _meta)) => Arc::new(net),
        Err(e) => {
            eprintln!("orderprobe: {e}");
            return ExitCode::FAILURE;
        }
    };

    let started = Instant::now();
    let next = AtomicUsize::new(0);
    let done: Vec<DecisionTally> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..args.threads)
            .map(|_| {
                let (next, args, policy_net) = (&next, &args, &policy_net);
                scope.spawn(move || {
                    let mut mine = DecisionTally::default();
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        if index >= args.games {
                            return mine;
                        }
                        mine.add(&play_one(args, index, policy_net));
                    }
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().expect("worker thread panicked")).collect()
    });
    let elapsed = started.elapsed().as_secs_f64();

    let mut total = DecisionTally::default();
    for t in &done {
        total.add(t);
    }

    println!("games        {} ({:.1} games/s)", args.games, args.games as f64 / elapsed);
    println!("players      {}", args.players);
    println!("max_nodes    {}", args.max_nodes);
    println!("elapsed      {elapsed:.1}s");
    println!();
    println!("decisions    {}", total.decisions);
    println!(
        "permuted     {}   ({:.2}% -- order_moves changed the root candidate order)",
        total.permuted,
        100.0 * total.permuted as f64 / total.decisions.max(1) as f64,
    );
    println!(
        "different    {}   ({:.2}% -- off vs on search chose a different move)",
        total.different,
        100.0 * total.different as f64 / total.decisions.max(1) as f64,
    );
    println!(
        "capped       {}   ({:.2}% -- either shadow search hit max_nodes)",
        total.capped,
        100.0 * total.capped as f64 / total.decisions.max(1) as f64,
    );
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_max_nodes_matches_plan_configs_own_default() {
        let args = Args::default();
        assert_eq!(args.max_nodes, plan::PlanConfig::default().max_nodes);
    }

    #[test]
    fn parse_args_requires_a_policy_checkpoint() {
        let argv = vec!["--max-nodes".to_string(), "60".to_string()];
        assert!(parse_args(&argv).is_err(), "no --policy-checkpoint was given");
    }

    #[test]
    fn parse_args_reads_max_nodes_and_checkpoint() {
        let argv = vec![
            "--policy-checkpoint".to_string(),
            "foo.ckpt".to_string(),
            "--max-nodes".to_string(),
            "60".to_string(),
        ];
        let args = parse_args(&argv).unwrap().unwrap();
        assert_eq!(args.max_nodes, 60);
        assert_eq!(args.policy_checkpoint, "foo.ckpt");
    }

    #[test]
    fn parse_args_rejects_a_nonpositive_max_nodes() {
        let argv =
            vec!["--policy-checkpoint".to_string(), "foo.ckpt".to_string(), "--max-nodes".to_string(), "0".to_string()];
        assert!(parse_args(&argv).is_err(), "a zero node budget can never run a search");
    }
}
