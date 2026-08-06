//! `NeuralBot`: the existing 1-ply search machinery with a neural evaluator.
//!
//! Ports `engine/bots/neural_bot.py` (98 lines) -- read that file's own
//! module doc comment first: it is byte-for-byte the same search shape as
//! [`super::super::weighted::eval::WeightedBot`] (apply every legal move to a
//! trial copy, evaluate, keep the best) with exactly one thing swapped: the
//! linear `evaluate` scalar is replaced by [`super::net::ValueNet`]'s
//! prediction of the final-culture margin, batched across every candidate of
//! one decision in a single call ([`super::net::value_batch`]) so a (nonexistent
//! here, but real once a trained net exists) GPU sees a batch rather than one
//! state at a time.
//!
//! **Determinization at the root.** [`super::encode::encode`] reads card
//! identity in the row, which makes the `end_turn` information leak
//! (`super::super::plan`'s own module doc comment, point about
//! `end_turn` refilling the row from the real civil deck) live for this bot
//! in a way it never is for the row-blind linear evaluator. So the decks are
//! re-shuffled once at the root before candidates are scored
//! ([`super::super::plan::determinize`]), exactly as `PlanBot`/`NeuralPlanBot`
//! do. `determinize: false` disables it for an A/B that measures the leak.
//!
//! ## Three things this port drops, matching `weighted::eval`'s precedent
//!
//! 1. **The per-candidate `try: ... except Exception: continue`.** Mirrors
//!    `weighted::eval::WeightedBot::choose`'s own documented choice:
//!    [`crate::apply::apply`] panics on an invariant violation rather than
//!    being caught, so a candidate that would have raised in Python instead
//!    stops the program here, loudly. Because every candidate that survives
//!    always contributes a real encoding, `best` is provably `Some` by the
//!    end of [`pick`]'s loop whenever `moves.len() > 1`.
//! 2. **`self.rng.choice(moves)` (the `best is None` fallback).** Point 1
//!    above makes it unreachable, same as `WeightedBot`'s identical fallback.
//! 3. **The injected `NeuralValue` abstraction.** Python injects `value`
//!    (anything with a `.value(list) -> list[float]` method) specifically so
//!    `NeuralBot` never imports torch itself -- `neural_net.py`'s guarded
//!    import is the reason. This crate never imports torch at all (`net.rs`'s
//!    own top doc comment), so there is exactly one implementation of "score
//!    a batch of encodings" ([`super::net::value_batch`]) and [`pick`] calls
//!    it directly -- house style: no dependency injection for its own sake.

use crate::apply;
use crate::moves::{Move, MoveList};
use crate::rng::PyRandom;
use crate::state::GameState;

use super::super::plan::determinize;
use super::encode::encode;
use super::net::{value_batch, ValueNet};

/// Search-shape knobs, mirroring `NeuralBot`'s constructor fields (minus
/// `rng`/`seed`/`name`, dropped for the reasons `weighted::eval`'s own top
/// doc comment gives for `WeightedBot`'s identical fields).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NeuralBotConfig {
    pub determinize: bool,
    /// Added to a candidate's score when the candidate is `Move::EndTurn`.
    /// `0.0` (Python's default) is a real, common choice -- unlike
    /// `weighted::eval`'s tuned `-3.0`, nothing has measured a nonzero value
    /// here yet.
    pub end_turn_bias: f64,
    pub allow_resign: bool,
}

impl Default for NeuralBotConfig {
    fn default() -> Self {
        NeuralBotConfig { determinize: true, end_turn_bias: 0.0, allow_resign: false }
    }
}

/// Instrumentation, mirroring `NeuralBot.decisions`/`.candidates`.
/// Caller-owned (house style: no process-global mutable state), matching
/// every other bot's `Stats`/`Counters` struct in this crate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub decisions: u64,
    pub candidates: u64,
}

/// Best move for `state.decider()` among `moves`, by 1-ply search under
/// [`ValueNet`]. Mirrors `NeuralBot.choose`/`pick`/`__call__` collapsed into
/// one function, the same shape `weighted::eval::WeightedBot::choose`/
/// `bots::plan::pick` already use -- move GENERATION stays the caller's job.
///
/// # Panics
/// If `moves` is empty (a caller bug, matching every other bot in this
/// port).
pub fn pick(
    cfg: &NeuralBotConfig,
    net: &ValueNet,
    stats: &mut Stats,
    rng: &mut PyRandom,
    state: &GameState,
    moves: &[Move],
) -> Move {
    let mut filtered = MoveList::new();
    if !cfg.allow_resign && moves.len() > 1 {
        let has_non_resign = moves.iter().any(|m| !matches!(m, Move::Resign));
        if has_non_resign {
            for &m in moves {
                if !matches!(m, Move::Resign) {
                    filtered.push(m);
                }
            }
        }
    }
    let moves: &[Move] = if filtered.as_slice().is_empty() { moves } else { filtered.as_slice() };
    if moves.len() == 1 {
        return moves[0];
    }

    let idx = state.decider();
    stats.decisions += 1;

    let mut root = state.clone();
    if cfg.determinize {
        determinize(&mut root, rng);
    }

    let mut encs: Vec<Vec<f64>> = Vec::with_capacity(moves.len());
    for &mv in moves {
        let mut trial = root.clone();
        apply::apply(&mut trial, mv);
        encs.push(encode(&trial, idx));
    }
    stats.candidates += encs.len() as u64;

    let scores = value_batch(net, &encs);
    let mut best: Option<(Move, f64)> = None;
    for (&mv, &val) in moves.iter().zip(scores.iter()) {
        let mut v = val;
        if matches!(mv, Move::EndTurn) {
            v += cfg.end_turn_bias;
        }
        if best.is_none_or(|(_, bv)| v > bv) {
            best = Some((mv, v));
        }
    }
    // Unreachable given `moves.len() > 1` and a complete `apply`/`encode`
    // (this module's top doc comment, point 1) -- kept for the same
    // defensive-fallback reason every other bot in this crate keeps its own
    // `unwrap_or(moves[0])`.
    best.map(|(m, _)| m).unwrap_or(moves[0])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game as G;

    /// A tiny net with all-zero weights: every state scores exactly `0.0`
    /// (LayerNorm of a constant zero vector is `beta = 0`, ReLU(0) = 0, the
    /// head reads all-zero input -> `0.0`), so this only exercises the
    /// SEARCH MECHANICS (does it pick a legal move, does it leave the real
    /// state untouched) independently of the net's actual predictions --
    /// there is no trained checkpoint to load (`net.rs`'s own top doc
    /// comment: no torch, no checkpoint I/O).
    fn flat_net(in_dim: usize, hidden: usize) -> ValueNet {
        ValueNet {
            in_dim,
            hidden,
            stem_w: vec![0.0; hidden * in_dim],
            stem_b: vec![0.0; hidden],
            stem_ln_gamma: vec![1.0; hidden],
            stem_ln_beta: vec![0.0; hidden],
            blocks: vec![],
            head_w: vec![0.0; hidden],
            head_b: 0.0,
        }
    }

    #[test]
    fn pick_with_a_single_move_returns_it_directly() {
        let state = G::new_game(2, 1);
        let moves = crate::legal::legal_moves(&state);
        let one = [moves.as_slice()[0]];
        let cfg = NeuralBotConfig::default();
        let net = flat_net(super::super::encode::ENCODING_DIM, 1);
        let mut stats = Stats::default();
        let mut rng = PyRandom::new(1);
        let picked = pick(&cfg, &net, &mut stats, &mut rng, &state, &one);
        assert_eq!(picked, one[0]);
        assert_eq!(stats, Stats::default(), "the single-move short circuit must not touch stats");
    }

    #[test]
    fn pick_never_mutates_the_real_state() {
        let state = G::new_game(2, 2);
        let before = state.clone();
        let moves = crate::legal::legal_moves(&state);
        let cfg = NeuralBotConfig::default();
        let net = flat_net(super::super::encode::ENCODING_DIM, 4);
        let mut stats = Stats::default();
        let mut rng = PyRandom::new(1);
        let _ = pick(&cfg, &net, &mut stats, &mut rng, &state, moves.as_slice());
        assert_eq!(state.card_row, before.card_row);
        assert_eq!(state.turn, before.turn);
        assert_eq!(state.civil_deck, before.civil_deck, "search runs on clones only");
    }

    #[test]
    fn pick_always_returns_an_offered_move() {
        for n in [2u8, 3, 4] {
            let state = G::new_game(n, 3);
            let moves = crate::legal::legal_moves(&state);
            let cfg = NeuralBotConfig::default();
            let net = flat_net(super::super::encode::ENCODING_DIM, 4);
            let mut stats = Stats::default();
            let mut rng = PyRandom::new(1);
            let mv = pick(&cfg, &net, &mut stats, &mut rng, &state, moves.as_slice());
            assert!(moves.as_slice().contains(&mv), "{n}p: {mv:?} was not offered");
        }
    }

    /// `allow_resign = false` (the default) drops `Move::Resign` from the
    /// candidate set whenever a non-resign move is legal too, exactly
    /// mirroring `WeightedBot`'s identical test.
    #[test]
    fn resign_is_filtered_out_when_a_live_alternative_exists() {
        let state = G::new_game(2, 4);
        let cfg = NeuralBotConfig::default();
        let net = flat_net(super::super::encode::ENCODING_DIM, 2);
        let mut stats = Stats::default();
        let mut rng = PyRandom::new(1);
        let picked = pick(&cfg, &net, &mut stats, &mut rng, &state, &[Move::Resign, Move::EndTurn]);
        assert_eq!(picked, Move::EndTurn, "resign must never be chosen over a live alternative");
    }

    #[test]
    fn allow_resign_keeps_a_lone_resign_candidate_eligible() {
        let state = G::new_game(2, 5);
        let cfg = NeuralBotConfig { allow_resign: true, ..NeuralBotConfig::default() };
        let net = flat_net(super::super::encode::ENCODING_DIM, 2);
        let mut stats = Stats::default();
        let mut rng = PyRandom::new(1);
        let picked = pick(&cfg, &net, &mut stats, &mut rng, &state, &[Move::Resign]);
        assert_eq!(picked, Move::Resign);
    }

    /// A net that scores `end_turn`'s encoding strictly lowest, but with a
    /// large `end_turn_bias`, must still pick `end_turn` -- pins that the
    /// bias is applied AFTER the net's raw score, not folded into it.
    #[test]
    fn end_turn_bias_is_added_after_the_net_score() {
        let state = G::new_game(2, 6);
        let moves = crate::legal::legal_moves(&state);
        assert!(moves.as_slice().iter().any(|m| matches!(m, Move::EndTurn)), "round 1 offers end_turn");
        let cfg = NeuralBotConfig { end_turn_bias: 1_000_000.0, ..NeuralBotConfig::default() };
        let net = flat_net(super::super::encode::ENCODING_DIM, 2); // every state scores 0.0
        let mut stats = Stats::default();
        let mut rng = PyRandom::new(1);
        let picked = pick(&cfg, &net, &mut stats, &mut rng, &state, moves.as_slice());
        assert_eq!(picked, Move::EndTurn, "an overwhelming end_turn_bias must win regardless of the flat net");
    }

    #[test]
    fn candidates_and_decisions_are_counted() {
        let state = G::new_game(2, 7);
        let moves = crate::legal::legal_moves(&state);
        let n_moves = moves.as_slice().len();
        assert!(n_moves > 1, "need a real decision to exercise counting");
        let cfg = NeuralBotConfig::default();
        let net = flat_net(super::super::encode::ENCODING_DIM, 2);
        let mut stats = Stats::default();
        let mut rng = PyRandom::new(1);
        let _ = pick(&cfg, &net, &mut stats, &mut rng, &state, moves.as_slice());
        assert_eq!(stats.decisions, 1);
        assert_eq!(stats.candidates as usize, n_moves);
    }
}
