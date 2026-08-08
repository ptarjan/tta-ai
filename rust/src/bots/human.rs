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

use crate::bots::pending;
use crate::bots::plan;
use crate::bots::weighted::weights::Weights;
use crate::human_policy;
use crate::moves::Move;
use crate::rng::PyRandom;
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

// ------------------------------------------------------------ real lookahead

/// Root shortlist width for [`choose_with_search`]'s move-ordering prior --
/// a `const`, not a config knob: the sole caller never varies it, matching
/// `plan.rs::QUIESCE_CAP`'s "no knob for a value nothing ever varies"
/// precedent. Same order of magnitude as [`plan::PlanConfig::width`]'s own
/// default (8): the root usually offers more legal moves than any later ply
/// (nothing has been committed to yet this turn), so it earns the
/// same-sized cut.
const ROOT_SHORTLIST: usize = 8;

/// The [`ROOT_SHORTLIST`] legal moves [`human_policy::predict_top1`]'s own
/// linear ranking model likes best, most-liked first -- passthrough,
/// unordered, when `moves` already fits inside one shortlist (nothing to
/// prune, and ranking a list [`plan::beam`] is about to re-sort by ITS OWN
/// score anyway would only cost cycles for no effect).
///
/// Factored out of [`choose_with_search`] so it can be pinned by its own
/// test independent of the search it feeds: `human_policy` is a ranking
/// model over (state, candidate) pairs, not a state-value function (see
/// that module's own doc comment), so this is the ONE place in this file
/// its opinion is consulted directly -- everything downstream
/// ([`plan::pick`]) scores with `weighted::eval::evaluate`, never this
/// vector.
fn root_shortlist(human_weights: &[f64], state: &GameState, idx: u8, moves: &[Move]) -> Vec<Move> {
    if moves.len() <= ROOT_SHORTLIST {
        return moves.to_vec();
    }
    let candidates = human_policy::candidate_features(state, idx, moves);
    let dense: Vec<Vec<f64>> = candidates.iter().map(human_policy::features_to_dense).collect();
    let order = human_policy::rank(human_weights, &dense);
    order.into_iter().take(ROOT_SHORTLIST).map(|i| moves[i]).collect()
}

/// Real lookahead for the human-imitation baseline: [`root_shortlist`]'s
/// move-ordering prior narrows the ROOT's legal moves to the
/// [`ROOT_SHORTLIST`] most human-plausible candidates, then delegates
/// entirely to [`plan::pick`] -- the SAME beam search `BotKind::Plan` runs,
/// completely unmodified -- to choose among that shortlist's resulting
/// lines.
///
/// ## What this bot actually is -- read before citing its win rate
///
/// This is a HYBRID, not a human imitator: which first move gets
/// CONSIDERED at all is human-shaped (`human_policy::predict_top1`'s own
/// ranking model), but which of those survivors wins, and every ply after
/// the first, is scored by [`plan::pick`]'s ordinary beam under `cfg.weights`
/// -- expected to be a real gameplay evaluator vector (e.g. the frozen
/// champion's), NOT `human_weights`. The two vectors are deliberately
/// different arguments: `Seat` (`greedy::Seat`) carries exactly one
/// [`Weights`] slot per seat, already spent -- for this bot -- on the
/// vector [`plan::pick`]'s beam needs at every node, so there is no second
/// slot for a human ranking vector to travel through `make_bots`'s generic
/// per-kind construction; [`Bot::human_plan`](super::greedy::Bot::human_plan)
/// is built by hand for exactly this reason -- see its own doc comment for
/// where each vector comes from in practice. A win-rate win here is
/// evidence a shallow human-shaped move-ordering prior on top of a real
/// state-value search is competitive, not evidence of human-LIKE play;
/// [`HumanBot`] (this module's original, unchanged, no-lookahead bot)
/// remains the one to cite for imitation accuracy (`humanpaired`'s top-1
/// numbers) -- this function does not touch [`HumanBot`] or its `choose` at
/// all.
///
/// # Panics
/// If `moves` is empty (a caller bug, matching every other bot in this
/// port).
#[allow(clippy::too_many_arguments)]
pub fn choose_with_search(
    cfg: &plan::PlanConfig,
    stats: &mut plan::Stats,
    counters: &mut pending::Counters,
    rng: &mut PyRandom,
    human_weights: &[f64],
    state: &GameState,
    moves: &[Move],
) -> Move {
    let filtered = super::filter_resign(moves, false);
    let moves: &[Move] = filtered.as_slice();
    if moves.len() == 1 {
        return moves[0];
    }
    let idx = state.decider();
    let shortlist = root_shortlist(human_weights, state, idx, moves);
    plan::pick(cfg, stats, counters, rng, state, &shortlist)
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

    // -------------------------------------------------- choose_with_search

    /// `root_shortlist` keeps exactly the [`ROOT_SHORTLIST`] highest-scoring
    /// candidates under the human ranking vector -- not just some subset --
    /// on a real position that offers more legal moves than one shortlist
    /// holds. This is the one property a broken or no-op move-ordering
    /// prior (e.g. one that returns `moves` untouched instead of sorting
    /// and truncating) fails: reverting `root_shortlist` to
    /// `moves.to_vec()` unconditionally makes the length assertion below
    /// fail immediately, since this position's move count exceeds
    /// `ROOT_SHORTLIST`.
    /// A real position reached by playing a real game forward a bit --
    /// openings offer few legal moves, but a board with a few built
    /// structures and a fuller card row routinely offers more than
    /// [`ROOT_SHORTLIST`]. Deterministic (`crate::apply::apply` on a fixed
    /// move-index-cycling policy, no RNG choices of our own), so this is
    /// exactly as reproducible as pinning a magic seed, without needing one.
    fn a_position_with_more_than_root_shortlist_legal_moves() -> (GameState, crate::moves::MoveList) {
        for seed in 0..30u64 {
            let mut state = G::new_game(3, seed);
            for step in 0..60u32 {
                let moves = legal::legal_moves(&state);
                if moves.as_slice().len() > ROOT_SHORTLIST {
                    return (state, moves);
                }
                if state.game_over {
                    break;
                }
                let mv = moves.as_slice()[step as usize % moves.as_slice().len()];
                crate::apply::apply(&mut state, mv);
            }
        }
        panic!("no position with more than ROOT_SHORTLIST legal moves found in 30 simulated games");
    }

    #[test]
    fn root_shortlist_keeps_exactly_the_top_ranked_candidates_by_score() {
        let (state, moves) = a_position_with_more_than_root_shortlist_legal_moves();
        let idx = state.decider();
        let moves = moves.as_slice();
        assert!(
            moves.len() > ROOT_SHORTLIST,
            "test needs a position with more than {ROOT_SHORTLIST} legal moves, got {}",
            moves.len()
        );

        let w = human_policy::vector_from_weights(&Weights::default());
        let shortlist = root_shortlist(&w, &state, idx, moves);
        assert_eq!(shortlist.len(), ROOT_SHORTLIST);

        let candidates = human_policy::candidate_features(&state, idx, moves);
        let dense: Vec<Vec<f64>> = candidates.iter().map(human_policy::features_to_dense).collect();
        let score = |d: &[f64]| -> f64 { d.iter().zip(w.iter()).map(|(x, wk)| x * wk).sum() };
        let scored: Vec<(Move, f64)> =
            moves.iter().zip(dense.iter()).map(|(&m, d)| (m, score(d))).collect();

        let kept_min = shortlist
            .iter()
            .map(|m| scored.iter().find(|(mm, _)| mm == m).unwrap().1)
            .fold(f64::INFINITY, f64::min);
        let dropped_max = scored
            .iter()
            .filter(|(m, _)| !shortlist.contains(m))
            .map(|(_, s)| *s)
            .fold(f64::NEG_INFINITY, f64::max);
        assert!(
            kept_min >= dropped_max,
            "kept min score {kept_min} should be >= dropped max score {dropped_max}"
        );
    }

    /// `root_shortlist` is a passthrough, not a filter, when `moves`
    /// already fits inside one shortlist -- every offered move survives,
    /// unlike the "more moves than the shortlist" case above.
    #[test]
    fn root_shortlist_returns_every_move_when_there_are_few_enough() {
        let state = G::new_game(2, 3);
        let idx = state.decider();
        let w = human_policy::vector_from_weights(&Weights::default());
        let moves = [Move::EndTurn];
        let shortlist = root_shortlist(&w, &state, idx, &moves);
        assert_eq!(shortlist, vec![Move::EndTurn]);
    }

    /// A freshly wired `choose_with_search` (default weights on both the
    /// ranking prior and the search's evaluator -- no trained file needed
    /// for this smoke test) always returns a move that was actually
    /// offered, on a real opening position, for every player count. Proves
    /// the wiring end to end: `root_shortlist` -> `plan::pick`'s beam ->
    /// picking a real move -- without depending on any trained weight file
    /// being present in this checkout.
    #[test]
    fn choose_with_search_always_returns_an_offered_move_on_a_real_opening_position() {
        let human_w = human_policy::vector_from_weights(&Weights::default());
        let cfg = plan::PlanConfig {
            weights: Weights::default(),
            width: 3,
            max_nodes: 200,
            ..plan::PlanConfig::default()
        };
        for players in [2, 3, 4] {
            let mut stats = plan::Stats::default();
            let mut counters = pending::Counters::default();
            let mut rng = PyRandom::new(0);
            let state = G::new_game(players, 42);
            let moves = legal::legal_moves(&state);
            let mv = choose_with_search(
                &cfg,
                &mut stats,
                &mut counters,
                &mut rng,
                &human_w,
                &state,
                moves.as_slice(),
            );
            assert!(moves.as_slice().contains(&mv), "{players}p: {mv:?} not among offered moves");
        }
    }

    /// `choose_with_search` never mutates the state it was handed, exactly
    /// like [`HumanBot::choose`] and every other search bot's `pick` in
    /// this crate: [`plan::pick`] only ever touches clones.
    #[test]
    fn choose_with_search_never_mutates_the_real_state() {
        let human_w = human_policy::vector_from_weights(&Weights::default());
        let cfg = plan::PlanConfig {
            weights: Weights::default(),
            width: 3,
            max_nodes: 200,
            ..plan::PlanConfig::default()
        };
        let mut stats = plan::Stats::default();
        let mut counters = pending::Counters::default();
        let mut rng = PyRandom::new(0);
        let state = G::new_game(2, 7);
        let before = state.clone();
        let moves = legal::legal_moves(&state);
        let _ = choose_with_search(
            &cfg,
            &mut stats,
            &mut counters,
            &mut rng,
            &human_w,
            &state,
            moves.as_slice(),
        );
        assert_eq!(state.round, before.round);
        assert_eq!(state.turn, before.turn);
        assert_eq!(state.card_row, before.card_row);
        assert_eq!(state.players[0].resources, before.players[0].resources);
    }

    /// A single-candidate decision is returned directly by
    /// `choose_with_search` too, matching [`HumanBot::choose`]'s identical
    /// short-circuit -- no shortlist, no `plan::pick` call, for a decision
    /// that was never really a choice.
    #[test]
    fn choose_with_search_with_a_single_legal_move_returns_it_directly() {
        let human_w = human_policy::vector_from_weights(&Weights::default());
        let cfg = plan::PlanConfig::default();
        let mut stats = plan::Stats::default();
        let mut counters = pending::Counters::default();
        let mut rng = PyRandom::new(0);
        let state = G::new_game(2, 3);
        let mv = choose_with_search(
            &cfg,
            &mut stats,
            &mut counters,
            &mut rng,
            &human_w,
            &state,
            &[Move::EndTurn],
        );
        assert_eq!(mv, Move::EndTurn);
    }
}
