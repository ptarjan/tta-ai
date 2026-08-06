//! Throughput benchmark: full games, uniformly-random LEGAL moves, single
//! threaded, release build only. This is the number for "how much faster is
//! the Rust port than the Python engine at the same job" -- see
//! `docs/`/the commit message for the measurement writeup.
//!
//! ## The driver is shared, not copied
//!
//! Each file directly under `tests/` is compiled as its own crate, so `Rng`,
//! `blocked_on`, `action_card_is_blocked` and `play_random` used to be copied
//! here byte-for-byte from `tests/random_game.rs`, with a comment in each
//! asking the next person to mirror their changes into the other. They
//! drifted: `random_game.rs` unblocked events, pacts, aggression, the
//! aggression defense and the colonization responses as those modules landed,
//! and this file went on blocking all of them, so the benchmark was quietly
//! timing a much smaller game than the suite actually plays. They now live
//! once, in [`mod@common`], which both files use. `common::blocked_on`'s doc
//! comment is the authority on WHAT is skipped and WHY.
//!
//! ## Why this matters for the number
//!
//! A random Python game (`engine.bots.RandomBot`) sees EVERY legal move. A
//! random Rust game here sees a slightly smaller move space -- see
//! `common::blocked_on` for the arms that are left. That is real, unclosed
//! scope, not a benchmark artifact: a Rust random game does marginally LESS
//! WORK per game than an unfiltered Python random game, and a naive
//! comparison of the two would overstate the port's speedup. The counterpart
//! harness, `tools/bench_python_playout.py`, applies the identical filter to
//! Python's move list so both sides play the same restricted game -- that is
//! the comparable number. That script also runs Python's own unfiltered
//! `RandomBot` alongside it, reported separately, so the size of the gap this
//! filter opens up is visible rather than hidden. It is the one remaining
//! hand-mirrored copy of the filter, and it goes away with the Python engine.
//!
//! ## Running it
//!
//! `#[ignore]` by default so a plain `cargo test` stays fast; this is minutes
//! of single-threaded CPU, not milliseconds. MUST be run against a release
//! build -- a debug build has no optimizations and the number is meaningless:
//!
//!   cargo test --release --test suite -- bench_playout::bench_random_playouts --ignored --nocapture --test-threads=1
//!
//! Game count and starting seed are env-configurable so tuning them doesn't
//! require a fresh `lto = "fat"` rebuild:
//!   TTA_BENCH_GAMES=300 TTA_BENCH_SEED0=0 cargo test --release --test suite -- bench_playout::bench_random_playouts --ignored --nocapture --test-threads=1

use std::time::Instant;

use tta::cards::CARDS;

use crate::common::{play_random, Played};

// ------------------------------------------------------------- the bench

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

/// `TTA_BENCH_PLAYERS`, e.g. "2,3,4" or just "3" -- lets a caller wrap a
/// SINGLE player count in an external CPU-time profiler (`/usr/bin/time -l`)
/// without that tool's number being diluted by the other two counts' games.
/// Defaults to all three, matching the un-filtered default this file had
/// before the flag existed.
fn env_players() -> Vec<u8> {
    match std::env::var("TTA_BENCH_PLAYERS") {
        Ok(v) => v.split(',').filter_map(|s| s.trim().parse().ok()).collect(),
        Err(_) => vec![2, 3, 4],
    }
}

fn mean_std(xs: &[f64]) -> (f64, f64) {
    let n = xs.len() as f64;
    let mean = xs.iter().sum::<f64>() / n;
    let var = xs.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / n;
    (mean, var.sqrt())
}

#[test]
#[ignore]
fn bench_random_playouts() {
    let n_games = env_usize("TTA_BENCH_GAMES", 200);
    let seed0 = env_usize("TTA_BENCH_SEED0", 0) as u64;

    println!(
        "tta rust random-playout bench: {n_games} games/count, seeds {seed0}..{}",
        seed0 + n_games as u64
    );
    // Reference CARDS so the table is definitely loaded before timing starts
    // (it is a static array, not lazy, but this keeps the intent explicit).
    assert!(!CARDS.is_empty());

    for num_players in env_players() {
        let mut plies = Vec::with_capacity(n_games);
        let mut blocked = 0usize;
        let t0 = Instant::now();
        for i in 0..n_games as u64 {
            let seed = seed0 + i;
            let (_state, played) = play_random(num_players, seed);
            match played {
                Played::Finished(outcome) => plies.push(outcome.moves_played as f64),
                Played::Blocked(why) => {
                    blocked += 1;
                    eprintln!("{num_players}p seed {seed}: BLOCKED {why:?}");
                }
            }
        }
        let elapsed = t0.elapsed();
        let finished = plies.len();
        let total_plies: f64 = plies.iter().sum();
        let (mean_plies, std_plies) = if finished > 0 { mean_std(&plies) } else { (0.0, 0.0) };
        let secs = elapsed.as_secs_f64();
        println!(
            "{num_players}p  games={finished}/{n_games} blocked={blocked}  \
             wall={secs:.3}s  games/s={:.3}  plies/s={:.1}  \
             mean_plies={mean_plies:.1} std_plies={std_plies:.1}",
            finished as f64 / secs,
            total_plies / secs,
        );
    }
}
