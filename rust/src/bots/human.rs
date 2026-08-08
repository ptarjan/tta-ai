//! `HumanBot`: plays via the fitted linear scorer `docs/HUMAN_MODEL.md`'s
//! Stage One baseline model produces ([`crate::human_policy::train`]) --
//! loads a persisted weight vector (`humantrain`'s output, see
//! [`crate::human_policy::load_weights`]) and picks the legal move ranked
//! highest by [`crate::human_policy::predict_top1`], instead of
//! `WeightedBot`'s hand-tuned evaluator.
//!
//! ## Shape: mirrors `GreedyBot`/`WeightedBot::choose`, not a new pattern
//!
//! [`HumanBot::choose`] filters `Move::Resign` first (`super::filter_resign`,
//! the shared guard every other search bot in this module uses -- see
//! `bots/mod.rs`'s own doc comment for why that is not configurable), then
//! scores every remaining candidate and takes the argmax. The scoring itself
//! is delegated entirely to [`crate::human_policy::candidate_features`] +
//! [`crate::human_policy::predict_top1`] -- this file adds no new feature-
//! reading or ranking logic of its own.
//!
//! ## Legality
//!
//! [`HumanBot::choose`] calls [`crate::human_policy::candidate_features`],
//! the SAME function `bin/humandata.rs` uses to build the training dataset
//! -- see that function's own doc comment, and `human_policy.rs`'s
//! module-level legality audit, for the full argument that it reads no
//! rival hand contents or unshuffled deck order, only public board state
//! (`bots::weighted::features::features`, the same encoding `WeightedBot`
//! itself is scored with). This bot adds no new feature-reading code of its
//! own, so that audit carries over unchanged to play time, not just to
//! dataset extraction.
//!
//! ## Weight file, and why it is not `bots::weighted::eval::load_weights`
//!
//! A [`HumanBot`]'s vector is loaded with [`crate::human_policy::
//! load_weights`], never `bots::weighted::eval::load_weights` -- the latter
//! applies `dominance_repair`, a set of gameplay-EVALUATOR monotonicity
//! invariants a vector fit purely to imitate human move CHOICES was never
//! trained to satisfy (see `human_policy::weights_to_text`'s doc comment for
//! the full argument). [`super::greedy::Seat::weights`] carries a
//! [`crate::bots::weighted::weights::Weights`] for every kind (reusing that
//! one `WeightKey`-indexed container rather than adding a parallel one just
//! for this bot) -- for a [`super::greedy::BotKind::Human`] seat, the caller
//! must have populated it via `human_policy::load_weights`, not the champion
//! loader; `bin/selfplay.rs`'s `--weights` handling does this automatically
//! when `--bots` is exactly `human`.

use crate::bots::weighted::weights::Weights;
use crate::human_policy;
use crate::moves::Move;
use crate::state::GameState;

/// A bot that plays the legal move [`crate::human_policy::predict_top1`]
/// ranks highest under a fitted human-imitation weight vector.
pub struct HumanBot {
    /// [`human_policy::feature_dim`]-wide, in `WeightKey::ALL` order --
    /// [`crate::human_policy::vector_from_weights`]'s own output shape, kept
    /// as a plain `Vec<f64>` (not a [`Weights`]) so [`Self::choose`] can hand
    /// it to [`crate::human_policy::predict_top1`] with no per-move
    /// conversion.
    weights: Vec<f64>,
}

impl HumanBot {
    /// Build a `HumanBot` from an already-loaded [`Weights`] (see this
    /// module's top doc comment for how that vector must have been loaded).
    pub fn new(weights: &Weights) -> HumanBot {
        HumanBot { weights: human_policy::vector_from_weights(weights) }
    }

    /// Best move for `state.decider()` among `moves`, by
    /// [`crate::human_policy::predict_top1`] under this bot's fitted vector.
    /// Mirrors [`super::greedy::GreedyBot::choose`]'s shape exactly (filter
    /// resign, single-candidate short-circuit, then score).
    ///
    /// # Panics
    /// If `moves` is empty (a caller bug, matching every other bot in this
    /// port).
    pub fn choose(&self, state: &GameState, moves: &[Move]) -> Move {
        let filtered = super::filter_resign(moves, false);
        let moves: &[Move] = filtered.as_slice();
        if moves.len() == 1 {
            return moves[0];
        }

        let idx = state.decider();
        let candidates = human_policy::candidate_features(state, idx, moves);
        let dense: Vec<Vec<f64>> = candidates.iter().map(human_policy::features_to_dense).collect();
        let best = human_policy::predict_top1(&self.weights, &dense);
        moves[best]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game as G;
    use crate::legal;

    /// A freshly constructed `HumanBot` (all-zero weights -- no trained file
    /// needed for this smoke test) always returns a move that was actually
    /// offered, on a real opening position. Proves the wiring end to end:
    /// legal-move generation -> `candidate_features` -> `predict_top1` ->
    /// picking `moves[best]` -- without depending on a trained weight file
    /// being present in this checkout.
    #[test]
    fn choose_always_returns_an_offered_move_on_a_real_opening_position() {
        let bot = HumanBot::new(&Weights::default());
        for players in [2, 3, 4] {
            let state = G::new_game(players, 42);
            let moves = legal::legal_moves(&state);
            let mv = bot.choose(&state, moves.as_slice());
            assert!(moves.as_slice().contains(&mv), "{players}p: {mv:?} not among offered moves");
        }
    }

    /// `choose` never mutates the state it was handed -- every trial is
    /// scored on a clone (`human_policy::candidate_features`'s own
    /// contract), matching every other bot's `choose` in this crate.
    #[test]
    fn choose_never_mutates_the_real_state() {
        let bot = HumanBot::new(&Weights::default());
        let state = G::new_game(2, 7);
        let before = state.clone();
        let moves = legal::legal_moves(&state);
        let _ = bot.choose(&state, moves.as_slice());
        assert_eq!(state.round, before.round);
        assert_eq!(state.turn, before.turn);
        assert_eq!(state.card_row, before.card_row);
        assert_eq!(state.players[0].resources, before.players[0].resources);
    }

    /// A single-candidate decision is returned directly, without even
    /// building a `candidate_features` call for it -- matching
    /// `GreedyBot::choose`'s identical short-circuit.
    #[test]
    fn choose_with_a_single_legal_move_returns_it_directly() {
        let bot = HumanBot::new(&Weights::default());
        let state = G::new_game(2, 3);
        let mv = bot.choose(&state, &[Move::EndTurn]);
        assert_eq!(mv, Move::EndTurn);
    }
}
