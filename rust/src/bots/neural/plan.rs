//! `NeuralPlanBot`: `PlanBot`'s whole-turn beam with the value net as the leaf.
//!
//! Ports `engine/bots/neural_plan.py` (332 lines) -- read that file's own
//! module doc comment first; it is the design rationale (three deliberate
//! differences from `PlanBot`, all forced by the evaluator) and is restated
//! here only where the Rust shape earns its own note.
//!
//! ## Reused, not restated
//!
//! * [`super::super::plan::determinize`] -- the root reshuffle.
//! * [`super::super::plan::quiesce`] -- draining a pending decision with the
//!   plain LINEAR 1-ply pick, `DEFAULT_WEIGHTS`-scored. Python's own docstring
//!   (point 3) insists `neural_plan.py`'s pending drain stay "exactly as
//!   `PlanBot._quiesce` does" -- calling the SAME function is what makes that
//!   true by construction rather than by two copies staying accidentally in
//!   sync. See "the `root_row` fix" below: this port never had the bug that
//!   made Python's own claim false until 2026-08-05.
//! * [`super::super::plan::plan_rng`]/[`super::super::plan::update_best`] --
//!   identical seed formula / identical "keep the max terminal value per
//!   root candidate" accumulation as `PlanBot`'s own.
//! * [`super::super::pending`] -- the shared "not my ordinary turn" policy.
//! * [`super::super::weighted::rivals::rival_context`] -- the root's counting
//!   outlook (`ctx.root_row`/`ctx.civil_outlook`/`ctx.event_pool`), computed
//!   once and threaded into every `quiesce` call this module makes.
//! * [`crate::combat::resolve_war_outcome`]/[`crate::combat::apply_war_spoils`]/
//!   [`crate::interact::settle_war_spoils`] -- the same three calls
//!   [`super::super::quiescent::war_value`] makes, at the primitive level
//!   rather than through that function itself: `war_value` returns a score
//!   (`f64`), and this module's leaf needs the RESOLVED **state** to encode,
//!   not a score -- see "War lookahead" below for why that is a genuine
//!   difference, not an oversight.
//!
//! ## The `root_row` fix (2026-08-05, both engines)
//!
//! Python's `neural_plan.py::_quiesce` used to call `rival_context(st, d)`
//! with NEITHER `root_row` nor `root_counts`, even though this module's own
//! top doc comment (point 3) claimed it stayed "exactly as `PlanBot._quiesce`
//! does" -- which DOES thread both through, precisely to stop a quiesce-drain
//! deep in the beam from pricing an opponent's pick off a row a trial
//! `end_turn` has already replenished with the real deck's next cards
//! (docs/INFORMATION_AUDIT.md). The claim and the code disagreed; fixed in
//! `neural_plan.py` (`pick`/`_beam`/`_one_ply_neural`/`_quiesce` now thread
//! `root_row`/`root_counts` the same way `plan.py` always has) and in this
//! port by construction: [`pick`] computes `ctx` via `rival_context` once at
//! the root and passes `Some(&ctx.root_row)`/`Some((&ctx.civil_outlook,
//! &ctx.event_pool))` into every [`super::super::plan::quiesce`] call below,
//! exactly like [`super::super::plan::beam`] already does for `PlanBot`.
//!
//! ## War lookahead substitutes the ENCODING, not the score
//!
//! `PlanBot`'s `score` calls [`super::super::quiescent::war_value`], which
//! resolves a declared war on a scratch copy and returns `evaluate(scratch)`.
//! The neural leaf ([`leaf_enc`]) needs the scratch STATE, not a score, so it
//! cannot call `war_value` itself (whose return type is `f64`) -- it inlines
//! the identical three-call resolution `war_value` performs and then
//! [`super::encode::encode`]s the result, so both searches price the move
//! class through the same underlying combat resolution
//! (docs/PLAN_WAR_LOOKAHEAD.md, docs/EVALUATOR_HISTORY.md: "two searches that
//! disagree about one move class do not share an evaluator").
//!
//! ## No `Option<f64>`/`try`-`except` anywhere in the leaf path
//!
//! Python's `_leaf_enc`/`_score_many` return `Optional[float]`/`list[Optional
//! [float]]` because `encode(t, me)` is wrapped in `try/except Exception:
//! return None`, defensively, per candidate. [`crate::apply::apply`] and
//! [`super::encode::encode`] are both total in this port (no `Result`, no
//! panic on a legal input) -- matching every other search bot's identical
//! "no per-candidate exception guard" choice (`weighted::eval`'s own top doc
//! comment, point 2) -- so [`leaf_enc`]/[`score_many`] return a bare `Vec<f64>`
//! with no `Option` to unwrap downstream.
//!
//! ## Batched per PLY, not per candidate -- the one genuine shape difference
//!
//! `PlanBot`'s beam scores each node the instant it is generated (a linear
//! dot product costs ~30us). This module's leaf costs a whole [`super::net::
//! ValueNet::forward`] call, so [`beam`] generates every candidate of one
//! ply first, then scores the WHOLE ply in one [`score_many`] call -- Python's
//! own docstring, point 1: "this changes no decision -- the same nodes get
//! the same scores -- only the wall clock." This is why [`beam`] cannot
//! simply be [`super::super::plan::beam`] with the evaluator swapped, and is
//! the one place this module forks its own copy of the frontier-expansion
//! loop rather than reusing `plan.rs`'s.
//!
//! ## `Stats`-free helpers where two closures would otherwise both need `&mut`
//!
//! [`pending::fallback_pick`] takes two `FnOnce` closures (`plain`/`quiet`),
//! both constructed before the call and each independently needing whatever
//! it captures. `PlanBot`'s own use of this (`plan.rs::pick`) only needs
//! `stats` inside the `quiet` closure, so the borrow is never contended. This
//! bot's plain and quiet branches are the SAME function ([`one_ply_neural`])
//! either way, and both need to count evals/wars-priced -- two closures each
//! capturing `&mut Stats` would be two live mutable borrows of the same
//! value, which does not compile. [`one_ply_neural`]/[`score_many`] are
//! therefore pure functions that RETURN their counts rather than mutating a
//! `&mut Stats` field, and [`pick`] adds the returned deltas to its own
//! `stats` once, after `fallback_pick` resolves which branch actually ran.

use crate::combat;
use crate::interact;
use crate::moves::{Move, MoveList};
use crate::rng::PyRandom;
use crate::state::GameState;

use crate::apply;

use super::super::pending;
use super::super::plan::{determinize, plan_rng, quiesce, update_best, Bank};
use super::super::weighted::rivals::{self, RivalContext};
use super::super::weighted::weights::Weights;
use super::encode::encode;
use super::net::{value_batch, ValueNet};

// ------------------------------------------------------------- configuration

/// Search-shape knobs, mirroring `NeuralPlanBot`'s class attributes.
#[derive(Clone, Copy, Debug)]
pub struct NeuralPlanConfig {
    /// Beam width kept between plies.
    pub width: usize,
    /// Hard cap on sequence length.
    pub max_plies: u32,
    /// Hard cap on `apply` calls per root decision. Python's own comment on
    /// `MAX_NODES` (1200, lower than `PlanBot`'s 4000): a node here costs an
    /// encode + a forward instead of a 30us dot product.
    pub max_nodes: i64,
    /// Determinizations averaged over.
    pub samples: u32,
    pub war_lookahead: bool,
    pub allow_resign: bool,
    /// Weight vector for the plain-LINEAR pending-stack drain (`_quiesce`);
    /// Python's `dict(drain_weights or DEFAULT_WEIGHTS)`.
    pub drain_weights: Weights,
    /// The shared "not my ordinary turn" policy -- see [`pending`]'s own top
    /// doc comment. `bot.determinize` is ALSO the gate for this module's own
    /// beam-path determinization, exactly as `PlanConfig::bot` documents for
    /// `PlanBot`.
    pub bot: pending::BotConfig,
}

impl Default for NeuralPlanConfig {
    fn default() -> Self {
        NeuralPlanConfig {
            width: 8,
            max_plies: 16,
            max_nodes: 1200,
            samples: 1,
            war_lookahead: true,
            allow_resign: false,
            drain_weights: Weights::default(),
            bot: pending::BotConfig::default(),
        }
    }
}

/// Instrumentation, mirroring `NeuralPlanBot`'s `decisions`/`nodes`/`evals`/
/// `searches`/`wars_priced` counters. Caller-owned, like every other bot's
/// `Stats`/`Counters` in this crate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub decisions: u64,
    pub nodes: u64,
    pub evals: u64,
    pub searches: u64,
    pub wars_priced: u64,
}

// ------------------------------------------------------------------- leaf

/// The encoding the net should score for position `t` from `me`'s view.
///
/// Substitutes the war-resolved position when I hold a declared war -- see
/// this module's top doc comment, "War lookahead substitutes the ENCODING".
/// `wars` is incremented, not `stats.wars_priced` directly -- see "`Stats`-
/// free helpers" above for why this and [`score_many`] return counts rather
/// than mutating a `&mut Stats`.
pub(crate) fn leaf_enc(t: &GameState, me: u8, war_lookahead: bool, wars: &mut u64) -> Vec<f64> {
    if war_lookahead && !t.game_over && !t.players[me as usize].war_declared_by_me.is_none() {
        let mut scratch = t.clone();
        if let Some(outcome) = combat::resolve_war_outcome(&mut scratch, me) {
            combat::apply_war_spoils(&mut scratch, &outcome);
        }
        interact::settle_war_spoils(&mut scratch);
        *wars += 1;
        return encode(&scratch, me);
    }
    encode(t, me)
}

/// Batched leaf values for a whole ply/candidate set at once. Returns
/// `(scores aligned with states, wars-priced delta)`.
fn score_many(
    states: &[GameState],
    me: u8,
    war_lookahead: bool,
    net: &ValueNet,
    bank: &mut Bank<Vec<f64>>,
) -> (Vec<f64>, u64) {
    let mut wars = 0u64;
    let encs: Vec<Vec<f64>> = states.iter().map(|t| leaf_enc(t, me, war_lookahead, &mut wars)).collect();
    // The encodings are already built here, so collecting them is a clone
    // rather than a second encode -- and they are what the net was actually
    // asked about, war substitution included, which is the whole property a
    // leaf-distribution training set needs.
    for e in &encs {
        bank.push(|| e.clone());
    }
    (value_batch(net, &encs), wars)
}

// ------------------------------------------------------------------- search

/// Best move for `state.decider()` among `moves`. Mirrors `NeuralPlanBot.
/// choose`/`pick`/`__call__` collapsed into one function, the same shape
/// every other bot in this crate uses -- move GENERATION stays the caller's
/// job.
///
/// # Panics
/// If `moves` is empty (a caller bug, matching every other bot in this
/// port).
pub fn pick(
    cfg: &NeuralPlanConfig,
    net: &ValueNet,
    stats: &mut Stats,
    counters: &mut pending::Counters,
    rng: &mut PyRandom,
    state: &GameState,
    moves: &[Move],
) -> Move {
    pick_collecting(cfg, net, stats, counters, rng, state, moves, &mut Bank::Off)
}

/// [`pick`], plus the ENCODING of every leaf the beam priced appended to
/// `bank` -- the hook `experiments/neural_gen_plan.py` spells as a
/// `_score_many` override on a `NeuralPlanBot` subclass. See
/// [`super::super::plan::Bank`] for why a generator needs these positions
/// specifically and not the pre-move states it could collect for free.
#[allow(clippy::too_many_arguments)]
pub fn pick_collecting(
    cfg: &NeuralPlanConfig,
    net: &ValueNet,
    stats: &mut Stats,
    counters: &mut pending::Counters,
    rng: &mut PyRandom,
    state: &GameState,
    moves: &[Move],
    bank: &mut Bank<Vec<f64>>,
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

    let me = state.decider();
    stats.decisions += 1;
    // Computed once at the root (from `state`, never from a determinized
    // copy) and threaded into every `quiesce` call below -- see this
    // module's top doc comment, "The `root_row` fix".
    let ctx = rivals::rival_context(state, me, None, None);

    if pending::not_my_turn(state, me) {
        let root = pending::prepare_root(&cfg.bot, state, counters, determinize, rng);
        // One bank per closure: `fallback_pick` runs exactly one of them,
        // but both must be constructible at once and they cannot share a
        // `&mut`. Absorbing both afterwards keeps whichever actually ran.
        let (mut plain, mut quiet) = (bank.like(), bank.like());
        let (mv, evals, wars) = pending::fallback_pick(
            &cfg.bot,
            state,
            counters,
            || one_ply_neural(&root, moves, me, net, false, &ctx, cfg, &mut plain),
            || one_ply_neural(&root, moves, me, net, true, &ctx, cfg, &mut quiet),
        );
        bank.absorb(plain);
        bank.absorb(quiet);
        stats.evals += evals;
        stats.wars_priced += wars;
        return mv;
    }

    // `(move, total, samples seen)` -- one collection of triples, not
    // `totals`/`seen` kept in step by index (house style).
    let mut totals: Vec<(Move, f64, u32)> = moves.iter().map(|&m| (m, 0.0, 0u32)).collect();
    let mut drng = plan_rng(state, me);
    for _ in 0..cfg.samples {
        let mut root = state.clone();
        if cfg.bot.determinize {
            determinize(&mut root, &mut drng);
        }
        for (mv, v) in beam(cfg, net, stats, &root, moves, me, &ctx, bank) {
            if let Some(entry) = totals.iter_mut().find(|(m, _, _)| *m == mv) {
                entry.1 += v;
                entry.2 += 1;
            }
        }
    }
    let mut chosen: Option<(f64, Move)> = None;
    for &(mv, total, seen) in &totals {
        if seen == 0 {
            continue;
        }
        let avg = total / seen as f64;
        if chosen.is_none_or(|(bv, _)| avg > bv) {
            chosen = Some((avg, mv));
        }
    }
    chosen.map(|(_, m)| m).unwrap_or(moves[0])
}

/// One node in [`beam`]'s frontier -- mirrors `plan.rs::Frontier`, minus the
/// running `score` field: this module's frontier is re-scored a WHOLE PLY at
/// a time by [`score_many`], not per-candidate, so there is no per-node
/// running score to carry between plies (the value that decides which nodes
/// survive to the next ply IS that ply's freshly computed score, used and
/// discarded immediately in the sort/truncate step below).
struct Frontier {
    state: GameState,
    first: Option<Move>,
}

/// Beam search to the end of `me`'s own turn, scored in one BATCH per ply
/// (this module's top doc comment, "Batched per PLY, not per candidate").
/// Returns `(first_move, best terminal score reachable through it)` pairs.
#[allow(clippy::too_many_arguments)]
fn beam(
    cfg: &NeuralPlanConfig,
    net: &ValueNet,
    stats: &mut Stats,
    root: &GameState,
    moves: &[Move],
    me: u8,
    ctx: &RivalContext,
    bank: &mut Bank<Vec<f64>>,
) -> Vec<(Move, f64)> {
    stats.searches += 1;
    let mut budget = cfg.max_nodes;
    let mut frontier = vec![Frontier { state: root.clone(), first: None }];
    let mut best: Vec<(Move, f64)> = Vec::new();

    for _ply in 0..cfg.max_plies {
        let mut gen_states: Vec<GameState> = Vec::new();
        let mut gen_first: Vec<Move> = Vec::new();
        for entry in &frontier {
            let generated;
            let mvs: &[Move] = match entry.first {
                None => moves,
                Some(_) => {
                    generated = crate::legal::legal_moves(&entry.state);
                    generated.as_slice()
                }
            };
            for &mv in mvs {
                if matches!(mv, Move::Resign) {
                    continue;
                }
                if budget <= 0 {
                    break;
                }
                budget -= 1;
                stats.nodes += 1;
                let mut t = entry.state.clone();
                apply::apply(&mut t, mv);
                // Every node this search generates is quiesced immediately,
                // exactly like `PlanBot`'s own beam -- threading the ROOT's
                // row/counts down, never `t`'s own.
                quiesce(&mut t, &cfg.drain_weights, Some(&ctx.root_row), Some((&ctx.civil_outlook, &ctx.event_pool)));
                let first = entry.first.unwrap_or(mv);
                gen_states.push(t);
                gen_first.push(first);
            }
            if budget <= 0 {
                break;
            }
        }
        if gen_states.is_empty() {
            break;
        }
        let (vals, wars) = score_many(&gen_states, me, cfg.war_lookahead, net, bank);
        stats.evals += gen_states.len() as u64;
        stats.wars_priced += wars;

        let mut nxt: Vec<(f64, GameState, Move)> = Vec::new();
        for ((t, f), v) in gen_states.into_iter().zip(gen_first).zip(vals) {
            if t.game_over || t.current != me {
                update_best(&mut best, f, v);
            } else {
                nxt.push((v, t, f));
            }
        }
        if nxt.is_empty() || budget <= 0 {
            break;
        }
        // Stable sort, matching Python's `list.sort`.
        nxt.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        nxt.truncate(cfg.width);
        frontier = nxt.into_iter().map(|(_, t, f)| Frontier { state: t, first: Some(f) }).collect();
    }
    best
}

/// One ply, batched. `quiet` drains the pending stack per candidate before
/// scoring -- mirrors `NeuralPlanBot._one_ply_neural`. Returns `(move,
/// evals delta, wars-priced delta)`; see this module's top doc comment,
/// "`Stats`-free helpers", for why the counts are returned rather than
/// applied to a `&mut Stats` here.
#[allow(clippy::too_many_arguments)]
fn one_ply_neural(
    state: &GameState,
    moves: &[Move],
    me: u8,
    net: &ValueNet,
    quiet: bool,
    ctx: &RivalContext,
    cfg: &NeuralPlanConfig,
    bank: &mut Bank<Vec<f64>>,
) -> (Move, u64, u64) {
    let mut states: Vec<GameState> = Vec::with_capacity(moves.len());
    for &mv in moves {
        let mut t = state.clone();
        apply::apply(&mut t, mv);
        if quiet {
            quiesce(&mut t, &cfg.drain_weights, Some(&ctx.root_row), Some((&ctx.civil_outlook, &ctx.event_pool)));
        }
        states.push(t);
    }
    if states.is_empty() {
        // Unreachable given a non-empty `moves` and a total `apply` -- kept
        // for the same defensive-fallback reason every other bot in this
        // crate keeps its own `unwrap_or(moves[0])`.
        return (moves[0], 0, 0);
    }
    let (vals, wars) = score_many(&states, me, cfg.war_lookahead, net, bank);
    let evals = states.len() as u64;
    let mut best: Option<(Move, f64)> = None;
    for (&mv, &v) in moves.iter().zip(vals.iter()) {
        if best.is_none_or(|(_, bv)| v > bv) {
            best = Some((mv, v));
        }
    }
    (best.map(|(m, _)| m).unwrap_or(moves[0]), evals, wars)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::CardId;
    use crate::game as G;
    use crate::state::{Defense, Pending};

    fn war_card(name: &str) -> CardId {
        CardId::by_name(name).unwrap_or_else(|| panic!("no card named {name:?}"))
    }

    /// A tiny all-zero net: every position scores exactly `0.0` (see
    /// `bots::neural::bot::tests::flat_net`'s identical reasoning). Exercises
    /// SEARCH MECHANICS, independent of any trained checkpoint.
    fn flat_net() -> ValueNet {
        let in_dim = super::super::encode::ENCODING_DIM;
        ValueNet {
            in_dim,
            hidden: 2,
            stem_w: vec![0.0; 2 * in_dim],
            stem_b: vec![0.0; 2],
            stem_ln_gamma: vec![1.0; 2],
            stem_ln_beta: vec![0.0; 2],
            blocks: vec![],
            head_w: vec![0.0; 2],
            head_b: 0.0,
        }
    }

    #[test]
    fn pick_with_a_single_move_returns_it_directly() {
        let state = G::new_game(2, 1);
        let moves = crate::legal::legal_moves(&state);
        let one = [moves.as_slice()[0]];
        let cfg = NeuralPlanConfig { width: 2, max_plies: 2, max_nodes: 50, ..NeuralPlanConfig::default() };
        let net = flat_net();
        let mut stats = Stats::default();
        let mut counters = pending::Counters::default();
        let mut rng = PyRandom::new(1);
        let picked = pick(&cfg, &net, &mut stats, &mut counters, &mut rng, &state, &one);
        assert_eq!(picked, one[0]);
        assert_eq!(stats, Stats::default(), "the single-move short circuit must not touch stats");
    }

    #[test]
    fn pick_never_mutates_the_real_state() {
        let state = G::new_game(2, 2);
        let before = state.clone();
        let moves = crate::legal::legal_moves(&state);
        let cfg = NeuralPlanConfig { width: 2, max_plies: 3, max_nodes: 100, ..NeuralPlanConfig::default() };
        let net = flat_net();
        let mut stats = Stats::default();
        let mut counters = pending::Counters::default();
        let mut rng = PyRandom::new(1);
        let _ = pick(&cfg, &net, &mut stats, &mut counters, &mut rng, &state, moves.as_slice());
        assert_eq!(state.card_row, before.card_row);
        assert_eq!(state.turn, before.turn);
        assert_eq!(state.civil_deck, before.civil_deck, "search runs on clones only");
    }

    #[test]
    fn pick_always_returns_an_offered_move() {
        for n in [2u8, 3, 4] {
            let state = G::new_game(n, 3);
            let moves = crate::legal::legal_moves(&state);
            let cfg = NeuralPlanConfig { width: 3, max_plies: 3, max_nodes: 200, ..NeuralPlanConfig::default() };
            let net = flat_net();
            let mut stats = Stats::default();
            let mut counters = pending::Counters::default();
            let mut rng = PyRandom::new(1);
            let mv = pick(&cfg, &net, &mut stats, &mut counters, &mut rng, &state, moves.as_slice());
            assert!(moves.as_slice().contains(&mv), "{n}p: {mv:?} was not offered");
        }
    }

    /// A zero node budget degrades to the synthetic root's own empty `best`,
    /// landing on the safety-net `moves[0]` fallback -- not a crash.
    #[test]
    fn a_zero_node_budget_degrades_gracefully_not_a_crash() {
        let state = G::new_game(2, 9);
        let moves = crate::legal::legal_moves(&state);
        let cfg = NeuralPlanConfig { max_nodes: 0, ..NeuralPlanConfig::default() };
        let net = flat_net();
        let mut stats = Stats::default();
        let mut counters = pending::Counters::default();
        let mut rng = PyRandom::new(1);
        let mv = pick(&cfg, &net, &mut stats, &mut counters, &mut rng, &state, moves.as_slice());
        assert!(moves.as_slice().contains(&mv));
    }

    /// Proves the shared `pending` policy is actually wired through, not
    /// re-inlined -- mirrors `plan.rs`'s own identical test.
    #[test]
    fn pending_branch_routes_through_the_shared_policy() {
        let mut state = G::new_game(3, 1);
        state.current = 0;
        state.players[1].hand_military.push(war_card("Phalanx"));
        state.pending.push(Pending::Defense(Defense {
            player: 1,
            attacker: 0,
            card: war_card("Aggression: Raid (I)"),
            atk: 6,
            dfn: 0,
            spent: 0,
            budget: 3,
        }));
        let moves = crate::legal::legal_moves(&state);
        assert!(moves.as_slice().len() > 1, "need a real decision to exercise the branch");
        let cfg = NeuralPlanConfig::default();
        let net = flat_net();
        let mut stats = Stats::default();
        let mut counters = pending::Counters::default();
        let mut rng = PyRandom::new(1);
        let mv = pick(&cfg, &net, &mut stats, &mut counters, &mut rng, &state, moves.as_slice());
        assert!(moves.as_slice().contains(&mv));
        assert_eq!(counters.calls, 1, "fallback_pick must have been called exactly once");
        assert_eq!(counters.roots, 1, "prepare_root must have been called exactly once");
        assert!(stats.evals >= 1, "one_ply_neural must have scored at least one candidate");
    }

    /// `allow_resign = false` (the default) drops `Move::Resign` from the
    /// candidate set whenever a non-resign move is legal too.
    #[test]
    fn resign_is_filtered_out_when_a_live_alternative_exists() {
        let state = G::new_game(2, 4);
        let cfg = NeuralPlanConfig::default();
        let net = flat_net();
        let mut stats = Stats::default();
        let mut counters = pending::Counters::default();
        let mut rng = PyRandom::new(1);
        let picked = pick(&cfg, &net, &mut stats, &mut counters, &mut rng, &state, &[Move::Resign, Move::EndTurn]);
        assert_eq!(picked, Move::EndTurn, "resign must never be chosen over a live alternative");
    }

    /// `score_many` prices a declared war through the same resolve/spoils
    /// primitives `quiescent::war_value` calls, and counts it.
    #[test]
    fn score_many_prices_a_declared_war_through_the_engines_own_resolution() {
        let mut state = G::new_game(2, 77);
        let war = war_card("War over Territory");
        state.players[0].war_declared_by_me = war;
        state.players[0].war_target = 1;
        state.players[1].wars_declared_on_me[0] = war;
        let warriors = war_card("Warriors");
        state.players[0].techs.get_mut(warriors).unwrap().workers = 12;

        let net = flat_net();
        let (_vals, wars) = score_many(std::slice::from_ref(&state), 0, true, &net, &mut Bank::Off);
        assert_eq!(wars, 1);

        let mut scratch = state.clone();
        let outcome = combat::resolve_war_outcome(&mut scratch, 0).expect("a 12-worker edge must not be a tie");
        combat::apply_war_spoils(&mut scratch, &outcome);
        interact::settle_war_spoils(&mut scratch);
        let expected_enc = encode(&scratch, 0);
        let looked_enc = leaf_enc(&state, 0, true, &mut 0u64);
        assert_eq!(looked_enc, expected_enc);

        // war_lookahead off must price the UNRESOLVED position.
        let unresolved_enc = leaf_enc(&state, 0, false, &mut 0u64);
        assert_eq!(unresolved_enc, encode(&state, 0));
        assert_ne!(looked_enc, unresolved_enc);
    }

    /// `quiesce_drains_a_real_pending_choice_to_quiet`-shaped check that this
    /// module's reused `super::super::plan::quiesce` is actually reachable
    /// and functions from here -- a compile-and-run smoke test for the
    /// module-boundary reuse this module's top doc comment claims.
    #[test]
    fn beam_drains_a_real_pending_choice_generated_mid_search() {
        // A game close to a discard-forcing state is hard to construct
        // directly through the beam; instead this pins the weaker but still
        // meaningful property that a full search on an ordinary early state
        // completes and returns a legal move without the reused `quiesce`
        // call panicking or hanging -- the sharpest failure mode a bad
        // module boundary would produce.
        let state = G::new_game(2, 55);
        let moves = crate::legal::legal_moves(&state);
        let cfg = NeuralPlanConfig { width: 4, max_plies: 6, max_nodes: 400, ..NeuralPlanConfig::default() };
        let net = flat_net();
        let mut stats = Stats::default();
        let mut counters = pending::Counters::default();
        let mut rng = PyRandom::new(1);
        let mv = pick(&cfg, &net, &mut stats, &mut counters, &mut rng, &state, moves.as_slice());
        assert!(moves.as_slice().contains(&mv));
        assert!(stats.searches >= 1);
        assert!(stats.nodes >= 1);
    }
}
