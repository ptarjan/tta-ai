//! Throughput benchmark: full games, uniformly-random LEGAL moves, single
//! threaded, release build only. This is the number for "how much faster is
//! the Rust port than the Python engine at the same job" -- see
//! `docs/`/the commit message for the measurement writeup.
//!
//! ## Why this duplicates `random_game.rs` instead of reusing it
//!
//! Each file under `tests/` is compiled as its own separate crate with no
//! access to another test file's private items, so `Rng`, `blocked_on`,
//! `action_card_is_blocked` and `play_random` are copied here byte-for-byte
//! from `tests/random_game.rs` as of the commit that added this file. That
//! file's "the move filter" doc comment is the authority on WHAT is skipped
//! and WHY (events.rs unported, several interact.rs decision responses
//! unported, a few structurally-blocked move types); read it before reading
//! this one. If that file's `blocked_on` changes, mirror the change here --
//! the two driving the same move space is what makes this a benchmark of
//! "the game logic the port actually runs" rather than a different game.
//!
//! ## Why this matters for the number
//!
//! A random Python game (`engine.bots.RandomBot`) sees EVERY legal move,
//! including events, pacts, aggression (and its defense), War over
//! Technology, and the ordered-choice action cards. A random Rust game here
//! sees a strictly smaller move space -- whole subsystems (events, pacts,
//! aggression) are never exercised at all. That is real, unclosed scope, not
//! a benchmark artifact: it means a Rust random game is doing LESS WORK per
//! game than an unfiltered Python random game, and a naive comparison of the
//! two would overstate the port's speedup. The counterpart harness,
//! `tools/bench_python_playout.py`, applies the identical filter to Python's
//! move list so both sides play the same restricted game -- that is the
//! comparable number. That script also runs Python's own unfiltered
//! `RandomBot` alongside it, reported separately, so the size of the gap this
//! filter opens up is visible rather than hidden.
//!
//! ## Running it
//!
//! `#[ignore]` by default so a plain `cargo test` stays fast; this is minutes
//! of single-threaded CPU, not milliseconds. MUST be run against a release
//! build -- a debug build has no optimizations and the number is meaningless:
//!
//!   cargo test --release --test bench_playout -- --ignored --nocapture --test-threads=1
//!
//! Game count and starting seed are env-configurable so tuning them doesn't
//! require a fresh `lto = "fat"` rebuild:
//!   TTA_BENCH_GAMES=300 TTA_BENCH_SEED0=0 cargo test --release --test bench_playout -- --ignored --nocapture --test-threads=1

use std::time::Instant;

use tta::cards::{CardId, Special, CARDS};
use tta::game;
use tta::moves::Move;
use tta::state::GameState;

// ------------------------------------------------------------ the driver
// (copied from tests/random_game.rs -- see module doc comment above)

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn blocked_on(state: &GameState, mv: Move) -> Option<&'static str> {
    let me = state.me();
    match mv {
        Move::OfferPact { .. } => Some("interact.rs: a pact offer is the other player's decision"),
        Move::Aggression { .. } => Some("interact.rs: the aggression defense is a decision"),
        Move::War { card, .. } if card.get().base_name == "War over Technology" => {
            Some("interact.rs: War over Technology spoils are a decision")
        }
        Move::PrepareEvent { .. } => Some("events.rs: revealing the current event"),
        Move::PlayAction { card } if action_card_is_blocked(card) => {
            Some("interact.rs / card table: an action card's ordered choice")
        }
        Move::WonderStep { .. }
            if matches!(me.wonder.get().base_name, "Hollywood" | "Internet") =>
        {
            Some("effects.rs: Hollywood/Internet completion culture needs building_output")
        }
        Move::Bid { .. }
        | Move::BidPass
        | Move::Defend { .. }
        | Move::DefendDone
        | Move::SendUnit { .. }
        | Move::SendBonus { .. }
        | Move::SendDone => Some("interact.rs: a response to an open decision"),
        Move::Resign => Some("would end the game early; tested separately"),
        _ => None,
    }
}

// Mirrors `tests/random_game.rs::action_card_is_blocked` -- see this file's
// top doc comment for why it is a byte-for-byte copy rather than a shared
// helper. Kept in sync 2026-08-05: the per-player-count magnitude gap that
// USED to widen this `matches!` (`CulturePerCivilizationWithMoreCulture`/
// `ResourcesForMilitaryUnitsPerStrongerCivilization`) is closed in
// `apply.rs` now, so those two cards are no longer skipped here either.
fn action_card_is_blocked(card: CardId) -> bool {
    card.get()
        .special
        .iter()
        .any(|s| matches!(s, Special::FreeCivilAction(_) | Special::GainFoodOrResources(_)))
}

enum Played {
    Finished(game::Outcome),
    Blocked(Vec<&'static str>),
}

fn play_random(num_players: u8, seed: u64) -> (GameState, Played) {
    let mut state = game::new_game(num_players, seed);
    let mut rng = Rng(seed ^ 0x5EED);
    let mut moves = 0usize;

    loop {
        if state.game_over {
            return (state, Played::Finished(game::Outcome { moves_played: moves, move_cap_hit: false }));
        }
        assert!(
            moves < game::MOVE_CAP,
            "hit the move cap at turn {} round {}: the turn loop is not closing",
            state.turn,
            state.round
        );
        let legal = tta::legal::legal_moves(&state);
        assert!(
            !legal.is_empty(),
            "no legal move at all for player {} in phase {:?} (turn {}, round {})",
            state.current,
            state.phase,
            state.turn,
            state.round
        );
        let playable: Vec<Move> = legal
            .as_slice()
            .iter()
            .copied()
            .filter(|&m| blocked_on(&state, m).is_none())
            .collect();
        if playable.is_empty() {
            let mut why: Vec<&'static str> = legal
                .as_slice()
                .iter()
                .filter_map(|&m| blocked_on(&state, m))
                .collect();
            why.sort_unstable();
            why.dedup();
            return (state, Played::Blocked(why));
        }
        let n = playable.len();
        let mv = playable[rng.below(n)];
        game::step(&mut state, mv);
        moves += 1;
    }
}

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
