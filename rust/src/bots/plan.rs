//! `PlanBot`: beam search over whole-turn action *sequences*, scored at one
//! fixed horizon, on a determinized state.
//!
//! Ports `engine/bots/plan.py` (616 lines) -- read that file's own module doc
//! comment first; it is the design rationale for everything below (three
//! defects of `WeightedBot` fixed at once: horizon asymmetry between
//! `end_turn` and every other candidate, one ply of lookahead inside a turn
//! that has several, and hidden information leaking through an
//! undeterminized root) and is restated here only where the Rust shape earns
//! its own note. Everything the search needs is already ported:
//! [`super::weighted::eval::evaluate`], [`super::weighted::rivals::
//! rival_context`], [`super::quiescent::war_value`] (identical helper,
//! reused rather than reimplemented -- Python's own docstring §4 insists on
//! this: "two searches that disagree about one move class do not share a
//! weight vector"), and [`super::pending`] (the shared "not my ordinary
//! turn" policy `NeuralPlanBot` -- not ported, no Rust counterpart exists to
//! need it yet -- used to duplicate).
//!
//! ## Three things this port drops, and why each is safe
//!
//! 1. **The journalled search path** (`_beam_journalled`/`_one_ply_
//!    journalled`/`_replay`, `USE_JOURNAL`). No `journal.rs` exists in this
//!    port -- `bots/mod.rs`'s own module doc comment already closed this for
//!    every earlier search bot: a `GameState` clone is this port's one
//!    search mechanism, so there is nothing to journal against. Only the
//!    copy-shaped `_beam`/`_one_ply` are ported.
//! 2. **Every per-candidate `try: ... except Exception: continue`.** Mirrors
//!    `weighted::eval`'s own documented choice (point 2 of that module's top
//!    doc comment) and `quiescent.rs::pick_one`'s identical one:
//!    [`crate::apply::apply`] panics on an invariant violation rather than
//!    being caught, so a candidate that would have raised in Python instead
//!    stops the program here, loudly, at the point of the actual bug.
//!    `rivals::rival_context`'s own Rust signature cannot raise either (no
//!    `_NO_CTX` fallback dict has anything to port to).
//! 3. **`trial.py`'s pooled `Random(0)` (`_TrialRandom`/`_rng()` in
//!    `plan.py`).** [`crate::apply::apply`] takes no rng parameter at all --
//!    it derives one deterministically from `state.seed`/`state.turn`/
//!    `state.round` ([`crate::game::rng_for`]) instead of a caller-injected
//!    stream, exactly as `bots/mod.rs`'s "`engine/bots/trial.py`: NOT
//!    ported" section already established for every earlier search bot.
//!    Every candidate at a given search node therefore already sees the
//!    identical rng, without a pool to fake that property by hand.
//!
//! `self.rng` is the one constructor field that is NOT dead here (unlike
//! `WeightedBot`'s, which `weighted::eval`'s own doc comment retires for
//! being genuinely unread): `pick`'s pending-decision branch hands it to
//! `pending::prepare_root` as the determinize rng, and it is a real,
//! call-to-call-persistent stream -- unlike the ordinary-turn branch's own
//! `drng`, which is freshly re-seeded from `state.seed`/`state.turn`/`me` on
//! every call and therefore carries no history. [`pick`] takes that stream as
//! an explicit `&mut PyRandom` parameter, owned by the caller (one per bot
//! instance), rather than as a struct field -- the same "counters are
//! owned by the caller, not process-global statics" shape `pending.rs`'s own
//! top doc comment already chose for [`pending::Counters`], for the same
//! reason: two callers (or two tests) must never share mutable rng state.
//!
//! `census.py` (per-decision instrumentation): not ported, matching
//! `quiescent.rs`'s identical omission -- there is no Rust `census.rs` for
//! either bot to call.
//!
//! ## Determinization: [`determinize`]
//!
//! Re-shuffles the two draw decks and the current-events pile -- the fields
//! whose ORDER a player at the table cannot see -- so a trial `apply` inside
//! the search draws a plausible card rather than the true next one. See
//! Python's own extensive comment on `determinize` (not reproduced here) for
//! why exactly these three fields and no others (in particular, why
//! `future_events` is NOT reshuffled), and for the Joan-of-Arc carve-out: a
//! player who has legitimately peeked the top event keeps seeing it on top
//! after the reshuffle, because determinizing must not destroy information
//! the mover genuinely has.

use crate::apply;
use crate::cards::CardId;
use crate::moves::Move;
use crate::rng::{shuffle_cards, PyRandom};
use crate::state::{CardList, GameState, ROW_SIZE};

use super::pending;
use super::quiescent;
use super::weighted::eval;
use super::weighted::rivals::{self, RivalContext};
use super::weighted::weights::Weights;

// ------------------------------------------------------------ determinize

/// Re-shuffle what the mover cannot see: `state.civil_deck`, `state.
/// military_deck`, `state.current_events`. See this module's top doc comment
/// and Python's own (much longer) comment on `determinize` for the full
/// rationale, restated here only for the one piece that needs a Rust-shape
/// note: the peeked-event carve-out reads `state.players[state.decider()]
/// .peeked_event` and must finish BEFORE `state.current_events` is borrowed
/// mutably, since both live on the same `state`.
pub fn determinize(state: &mut GameState, rng: &mut PyRandom) {
    if !state.civil_deck.is_empty() {
        shuffle_cards(rng, state.civil_deck.as_mut_slice());
    }
    if !state.military_deck.is_empty() {
        shuffle_cards(rng, state.military_deck.as_mut_slice());
    }
    if state.current_events.len() > 1 {
        // Read the peeked-event carve-out before taking a mutable borrow of
        // `current_events` below -- both are fields of the same `state`.
        let mover = state.decider();
        let known = state.players[mover as usize].peeked_event;
        let ev = &mut state.current_events;
        // Only pin the top card if it is STILL the true top: a stale note
        // (the real top has since been revealed and replaced) must not pin a
        // card it no longer names.
        let top = if !known.is_none() && ev.as_slice().last() == Some(&known) { ev.pop() } else { None };
        shuffle_cards(rng, ev.as_mut_slice());
        // THE EVENT PILE IS AGE-ORDERED AND THAT ORDER IS PUBLIC --
        // `events::_recycle_future_events`'s own two lines (shuffle, then
        // stable-sort by descending age level), repeated here rather than
        // called, because randomising the pile flat would destroy public
        // information along with the hidden kind. `sort_by_key` is stable,
        // so this only permutes within each age band.
        ev.as_mut_slice().sort_by_key(|&id| std::cmp::Reverse(id.level()));
        if let Some(top) = top {
            ev.push(top);
        }
    }
}

/// `random.Random(state.seed * 7919 + state.turn * 31 + me)`: `pick`'s own
/// per-decision determinize rng, freshly re-seeded every call (unlike the
/// caller-owned, call-to-call-persistent `rng` parameter `pick` uses for the
/// pending-decision branch). Checked arithmetic, not wrapping -- mirrors
/// `economy::deck_rng`/`game::rng_for`'s identical justification: Python's
/// ints are unbounded, so a seed big enough to overflow would draw a
/// DIFFERENT MT19937 stream there than any `i64` could here.
fn plan_rng(state: &GameState, me: u8) -> PyRandom {
    let seed = i64::try_from(state.seed)
        .ok()
        .and_then(|s| s.checked_mul(7919))
        .and_then(|s| s.checked_add(state.turn as i64 * 31))
        .and_then(|s| s.checked_add(me as i64))
        .expect(
            "game seed * 7919 + turn * 31 + me overflows i64; Python's unbounded ints would \
             seed a different MT19937 stream -- widen rng::PyRandom::new rather than wrapping",
        );
    PyRandom::new(seed)
}

// ------------------------------------------------------------- configuration

/// Search-shape knobs, mirroring `PlanBot`'s class attributes. `weights` sits
/// here (unlike `quiescent::QuiescenceConfig`, which is generic over the
/// evaluator via a closure) because this module calls [`eval::evaluate`]
/// directly rather than taking a scorer as a parameter -- `weighted.rs` is
/// fully landed now, so there is a real function to call instead of a
/// closure to inject (house style: no dependency injection for its own
/// sake).
#[derive(Clone, Copy, Debug)]
pub struct PlanConfig {
    pub weights: Weights,
    /// Beam width kept between plies.
    pub width: usize,
    /// Hard cap on sequence length (a turn is ~2-7 moves; 16 is slack).
    pub max_plies: u32,
    /// Hard cap on `apply` calls per root decision.
    pub max_nodes: i64,
    /// How many determinizations to average the search over (1 = one
    /// sample).
    pub samples: u32,
    /// Score an unresolved war of mine through the engine's own
    /// [`quiescent::war_value`] (this module's top doc comment, point 4 of
    /// Python's docstring).
    pub war_lookahead: bool,
    /// The shared "not my ordinary turn" policy -- see [`pending`]'s own top
    /// doc comment. `bot.determinize` is ALSO the gate for this module's own
    /// beam-path determinization (Python's `self.determinize` constructor
    /// field is the identical bot-wide switch `pending.py` reads through
    /// `getattr`), not just the pending-decision path's.
    pub bot: pending::BotConfig,
}

impl Default for PlanConfig {
    fn default() -> Self {
        PlanConfig {
            weights: Weights::default(),
            width: 8,
            max_plies: 16,
            max_nodes: 4000,
            samples: 1,
            war_lookahead: true,
            bot: pending::BotConfig::default(),
        }
    }
}

/// Instrumentation, mirroring `PlanBot`'s `nodes`/`searches`/`wars_priced`
/// counters. Caller-owned (house style: no process-global mutable state) --
/// one per bot instance, passed `&mut` into [`pick`], exactly like
/// `quiescent::Stats`/`pending::Counters`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub nodes: u64,
    pub searches: u64,
    pub wars_priced: u64,
}

/// Pending decisions resolved per [`quiesce`] call before scoring, mirroring
/// `PlanBot._quiesce`'s `cap=12` default -- never overridden by any call site
/// in `plan.py`, so this is a `const` rather than a [`PlanConfig`] field
/// (house style: no knob for a value nothing ever varies).
const QUIESCE_CAP: u32 = 12;

// ------------------------------------------------------------------- search

/// Best move for `state.decider()` among `moves`. Mirrors `PlanBot.choose`/
/// `pick`/`__call__`'s three-method harness-adapter split collapsed into one
/// function, the same shape `weighted::eval::WeightedBot::choose`/
/// `book::BookBot::choose` already use -- move GENERATION
/// ([`crate::legal::legal_moves`]) stays the caller's job.
///
/// `stats`/`counters`/`rng` are the caller-owned mutable state this search
/// touches: `counters` is threaded straight into [`pending::fallback_pick`]/
/// [`pending::prepare_root`] so the shared "not my ordinary turn" wiring is
/// provably taken rather than re-inlined (`pending.rs`'s own top doc comment
/// -- Python enforces this with `tests/test_pending_fallback_is_shared.py`;
/// `tests::pending_branch_routes_through_the_shared_policy` below is this
/// port's equivalent). `rng` is the one field this module's top doc comment
/// explains is genuinely alive.
///
/// # Panics
/// If `moves` is empty (a caller bug, matching every other bot in this
/// port).
pub fn pick(
    cfg: &PlanConfig,
    stats: &mut Stats,
    counters: &mut pending::Counters,
    rng: &mut PyRandom,
    state: &GameState,
    moves: &[Move],
) -> Move {
    if moves.len() == 1 {
        return moves[0];
    }
    let me = state.decider();
    let w = &cfg.weights;
    // Computed once at the root (from `state`, never from a determinized
    // copy) and reused for every candidate/sample below -- `rivals::
    // rival_context`'s own doc comment: recomputing per candidate would be
    // an information leak, not just a missed optimisation.
    let ctx = rivals::rival_context(state, me, None, None);

    // Not my ordinary turn: there is no turn to plan, so price the
    // candidates one ply deep at a common horizon of "now" -- draining a
    // pending decision that is MINE first, exactly as every beam node
    // already does, iff `pending::wants_quiet` says so.
    if pending::not_my_turn(state, me) {
        let root = pending::prepare_root(&cfg.bot, state, counters, determinize, rng);
        return pending::fallback_pick(
            &cfg.bot,
            state,
            counters,
            || one_ply(&root, moves, me, w, &ctx),
            || one_ply_quiet(&root, moves, me, w, &ctx, cfg.war_lookahead, stats),
        );
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
        let best = beam(cfg, stats, &root, moves, me, w, &ctx);
        for (mv, v) in best {
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

/// One node in [`beam`]'s frontier: the running score, the state it reached,
/// and which ROOT candidate this line descends from (`None` only for the
/// synthetic root entry itself).
struct Frontier {
    score: f64,
    state: GameState,
    first: Option<Move>,
}

/// Beam search to the end of `me`'s own turn. Returns `(first_move, best
/// terminal score reachable through it)` pairs -- Python's `{first_move:
/// score}` dict, restated as a `Vec` of pairs rather than a parallel
/// `HashMap` (house style; the number of root candidates is small, so a
/// linear scan in [`update_best`] costs nothing a hash would recover).
fn beam(
    cfg: &PlanConfig,
    stats: &mut Stats,
    root: &GameState,
    moves: &[Move],
    me: u8,
    w: &Weights,
    ctx: &RivalContext,
) -> Vec<(Move, f64)> {
    stats.searches += 1;
    let mut budget = cfg.max_nodes;
    let mut frontier = vec![Frontier { score: 0.0, state: root.clone(), first: None }];
    let mut best: Vec<(Move, f64)> = Vec::new();

    for _ply in 0..cfg.max_plies {
        let mut nxt: Vec<Frontier> = Vec::new();
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
                let first = entry.first.unwrap_or(mv);
                // Resolve decisions owned by anybody, so the position is
                // quiet before it is either scored or expanded -- threading
                // the ROOT's row/counts down, never `t`'s own (which a trial
                // `end_turn` may already have replenished with cards this
                // search must not see).
                quiesce(&mut t, w, Some(&ctx.root_row), Some((&ctx.civil_outlook, &ctx.event_pool)));
                let v = score(&t, me, w, ctx, cfg.war_lookahead, stats);
                if t.game_over || t.current != me {
                    update_best(&mut best, first, v);
                } else {
                    nxt.push(Frontier { score: v, state: t, first: Some(first) });
                }
            }
        }
        if nxt.is_empty() || budget <= 0 {
            break;
        }
        // Stable sort: ties keep the order they were appended in, matching
        // Python's `list.sort`.
        nxt.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
        nxt.truncate(cfg.width);
        frontier = nxt;
    }
    best
}

/// `best[mv] = max(best.get(mv), v)`, first-insertion order preserved.
fn update_best(best: &mut Vec<(Move, f64)>, mv: Move, v: f64) {
    match best.iter_mut().find(|(m, _)| *m == mv) {
        Some(entry) if v > entry.1 => entry.1 = v,
        Some(_) => {}
        None => best.push((mv, v)),
    }
}

/// Evaluate a quiet position, pricing an unresolved war of mine through
/// [`quiescent::war_value`] rather than as pure cost -- Python's `_score`,
/// point 4 of this module's top doc comment. `quiescent::war_value` returns
/// `f64` directly rather than Python's `Optional[float]`: that module's own
/// doc comment establishes the `None` branch is unreachable on a real engine
/// (a war with nothing declared, or a genuine tie, both fall through to a
/// real evaluated float in Python too), so `stats.wars_priced` increments
/// unconditionally whenever this branch is taken, matching what Python's
/// counter actually measures in practice.
fn score(t: &GameState, me: u8, w: &Weights, ctx: &RivalContext, war_lookahead: bool, stats: &mut Stats) -> f64 {
    if war_lookahead && !t.game_over && !t.players[me as usize].war_declared_by_me.is_none() {
        stats.wars_priced += 1;
        return quiescent::war_value(t, me, &|s, i| eval::evaluate(s, i, w, Some(ctx), None));
    }
    eval::evaluate(t, me, w, Some(ctx), None)
}

/// Best move for `idx` at `me`'s turn horizon, by plain 1-ply search: apply
/// each candidate to a clone and score it immediately with [`eval::
/// evaluate`] -- no quiescence, no war lookahead. Mirrors `PlanBot._one_ply`
/// (its journalled twin dropped; see this module's top doc comment).
fn one_ply(state: &GameState, moves: &[Move], me: u8, w: &Weights, ctx: &RivalContext) -> Move {
    let mut best: Option<(Move, f64)> = None;
    for &mv in moves {
        let mut t = state.clone();
        apply::apply(&mut t, mv);
        let v = eval::evaluate(&t, me, w, Some(ctx), None);
        if best.is_none_or(|(_, bv)| v > bv) {
            best = Some((mv, v));
        }
    }
    best.map(|(m, _)| m).unwrap_or(moves[0])
}

/// `one_ply`, but drain the pending stack before scoring -- mirrors
/// `PlanBot._one_ply_quiet`. THE INCONSISTENCY THIS EXISTS TO REMOVE: every
/// node inside [`beam`] is scored `apply -> quiesce -> score`, so at a REAL
/// pending decision of mine the identical position must be priced the same
/// way, or (Python's own measurement, `docs/AGGRESSION_RATE.md`) a defender
/// spends cards on arithmetically hopeless defences and holds off none of
/// the winnable ones, because an undrained position cannot express whether a
/// defence succeeds.
fn one_ply_quiet(
    state: &GameState,
    moves: &[Move],
    me: u8,
    w: &Weights,
    ctx: &RivalContext,
    war_lookahead: bool,
    stats: &mut Stats,
) -> Move {
    let mut best: Option<(Move, f64)> = None;
    for &mv in moves {
        let mut t = state.clone();
        apply::apply(&mut t, mv);
        quiesce(&mut t, w, Some(&ctx.root_row), Some((&ctx.civil_outlook, &ctx.event_pool)));
        let v = score(&t, me, w, ctx, war_lookahead, stats);
        if best.is_none_or(|(_, bv)| v > bv) {
            best = Some((mv, v));
        }
    }
    best.map(|(m, _)| m).unwrap_or(moves[0])
}

/// Drain `st.pending` with plain 1-ply picks for whoever decides, up to
/// [`QUIESCE_CAP`] decisions. Mirrors `PlanBot._quiesce`.
///
/// `root_row`/`root_counts` are the search ROOT's visible card row and
/// counting outlook, threaded down from [`pick`]'s own `ctx` so the opponent
/// picks made in here price the same information the real decider could see
/// -- not information a trial `end_turn` deep in the beam has already
/// changed. They must arrive as parameters: this runs on `st`, which may be
/// several plies from the root by the time it is called.
///
/// Deliberately NOT `quiescent::resolve`: that function shares a node budget
/// across the whole [`pick`] call and supports nested quiescence levels,
/// neither of which `_quiesce` in Python ever uses (no call site overrides
/// `cap`, and the decider's own pick is always a plain [`one_ply`], never a
/// further resolve). Reusing it here would mean threading two parameters
/// (`level`, a shared `nodes_left`) that this bot's own Python never varies,
/// for a generic evaluator closure this module does not need either (it
/// calls [`eval::evaluate`] directly). See `quiescent.rs`'s own top doc
/// comment: it is deliberately generic FOR a caller with no `weighted.rs` to
/// call; that caller now exists, and calls it directly instead.
fn quiesce(
    st: &mut GameState,
    w: &Weights,
    root_row: Option<&CardList<ROW_SIZE>>,
    root_counts: Option<(&Vec<(CardId, f64)>, &(Vec<(CardId, u16)>, f64))>,
) {
    let mut n = 0u32;
    while !st.pending.is_empty() && n < QUIESCE_CAP && !st.game_over {
        n += 1;
        let d = st.decider();
        let mvs = crate::legal::legal_moves(st);
        let mvs = mvs.as_slice();
        if mvs.is_empty() {
            return;
        }
        if mvs.len() == 1 {
            apply::apply(st, mvs[0]);
            continue;
        }
        let dctx =
            rivals::rival_context(st, d, root_row.cloned(), root_counts.map(|(a, b)| (a.clone(), b.clone())));
        let mv = one_ply(st, mvs, d, w, &dctx);
        apply::apply(st, mv);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::CardId;
    use crate::combat;
    use crate::game as G;
    use crate::interact;

    fn war_card(name: &str) -> CardId {
        CardId::by_name(name).unwrap_or_else(|| panic!("no card named {name:?}"))
    }

    // -------------------------------------------------------- determinize

    /// A no-op below the two-card threshold: `current_events` with 0 or 1
    /// entries has nothing to reorder, matching Python's `if len(ev) > 1`
    /// guard exactly (not `>= 1`).
    #[test]
    fn determinize_leaves_a_short_event_pile_untouched() {
        let mut state = G::new_game(2, 1);
        state.current_events = CardList::new();
        state.current_events.push(war_card("War over Territory"));
        let before = state.current_events.clone();
        let mut rng = PyRandom::new(1);
        determinize(&mut state, &mut rng);
        assert_eq!(state.current_events, before);
    }

    /// Reorders exactly the three hidden piles and nothing else -- the
    /// COMPLEMENT half of Python's own `test_search_root_is_determinized.py`
    /// (`HIDDEN_ORDER`'s doc comment): every other list/dict field of
    /// `GameState` is untouched by `determinize`.
    #[test]
    fn determinize_touches_only_the_three_hidden_piles() {
        let state = G::new_game(3, 7);
        let mut after = state.clone();
        let mut rng = PyRandom::new(99);
        determinize(&mut after, &mut rng);
        assert_eq!(after.card_row, state.card_row, "the visible row must not move");
        assert_eq!(after.players[0].hand_civil, state.players[0].hand_civil);
        assert_eq!(after.players[0].hand_military, state.players[0].hand_military);
        assert_eq!(after.scoring_events, state.scoring_events);
        assert_eq!(after.past_events, state.past_events);
        assert_eq!(after.future_events, state.future_events, "future_events is never reshuffled here");
        assert_eq!(after.turn, state.turn);
        assert_eq!(after.round, state.round);
    }

    /// The reshuffled civil deck is a PERMUTATION of the original -- same
    /// multiset of cards, order free to change. Age A's civil deck is 20
    /// cards minus the 13 dealt into the row (`game::new_game`'s own test),
    /// so 7 remain -- a 1-in-5040 chance of the identity permutation, which
    /// this also pins as "something actually moved".
    #[test]
    fn determinize_permutes_the_civil_deck_without_changing_its_contents() {
        let state = G::new_game(2, 3);
        assert!(state.civil_deck.len() > 4, "need a long enough deck for a reorder to be provable");
        let mut after = state.clone();
        let mut rng = PyRandom::new(1);
        determinize(&mut after, &mut rng);
        let mut before_sorted: Vec<CardId> = state.civil_deck.as_slice().to_vec();
        let mut after_sorted: Vec<CardId> = after.civil_deck.as_slice().to_vec();
        before_sorted.sort_by_key(|c| c.0);
        after_sorted.sort_by_key(|c| c.0);
        assert_eq!(before_sorted, after_sorted, "same multiset of cards");
        assert_ne!(state.civil_deck.as_slice(), after.civil_deck.as_slice(), "a real reorder must have happened");
    }

    /// The reshuffled event pile stays sorted by descending age level (public
    /// information the reshuffle must not destroy) -- built directly with a
    /// mixed-age pile rather than relying on a fresh deal to already have
    /// one.
    #[test]
    fn determinize_keeps_the_event_pile_age_banded() {
        let mut state = G::new_game(2, 5);
        state.current_events = CardList::new();
        // `data/*.json` events span Age A..IV; pick a handful of names known
        // to exist across ages via `CARDS` directly rather than hand-naming
        // events (whose exact roster this test should not have to track).
        let mut events: Vec<CardId> = crate::cards::CARDS
            .iter()
            .enumerate()
            .filter(|(_, c)| c.kind == crate::cards::CardType::Event)
            .take(6)
            .map(|(i, _)| CardId(i as u16))
            .collect();
        assert!(events.len() >= 2, "need at least two events to prove a sort happened");
        // Reverse so the pile does NOT start pre-sorted.
        events.reverse();
        for &e in &events {
            state.current_events.push(e);
        }
        let mut rng = PyRandom::new(3);
        determinize(&mut state, &mut rng);
        let levels: Vec<u8> = state.current_events.as_slice().iter().map(|c| c.level()).collect();
        let mut sorted = levels.clone();
        sorted.sort_by(|a, b| b.cmp(a));
        assert_eq!(levels, sorted, "descending age level must survive the reshuffle");
    }

    /// Joan of Arc's peek: when the decider's `peeked_event` is genuinely the
    /// current top card, it MUST still be on top after `determinize` --
    /// destroying it would take away information the mover legitimately has.
    #[test]
    fn determinize_keeps_a_genuinely_peeked_top_event_on_top() {
        let mut state = G::new_game(2, 11);
        state.current_events = CardList::new();
        let events: Vec<CardId> = crate::cards::CARDS
            .iter()
            .enumerate()
            .filter(|(_, c)| c.kind == crate::cards::CardType::Event)
            .take(5)
            .map(|(i, _)| CardId(i as u16))
            .collect();
        for &e in &events {
            state.current_events.push(e);
        }
        let top = *state.current_events.as_slice().last().unwrap();
        state.current = 0;
        state.players[0].peeked_event = top;
        let mut rng = PyRandom::new(4);
        determinize(&mut state, &mut rng);
        assert_eq!(state.current_events.as_slice().last(), Some(&top), "the genuinely peeked card must stay on top");
    }

    /// A STALE peeked note (the top has already moved on) must not pin
    /// anything -- the negative control for the test above.
    #[test]
    fn determinize_ignores_a_stale_peeked_event() {
        let mut state = G::new_game(2, 12);
        state.current_events = CardList::new();
        let events: Vec<CardId> = crate::cards::CARDS
            .iter()
            .enumerate()
            .filter(|(_, c)| c.kind == crate::cards::CardType::Event)
            .take(5)
            .map(|(i, _)| CardId(i as u16))
            .collect();
        for &e in &events {
            state.current_events.push(e);
        }
        // Peek a card that is NOT the current top.
        state.current = 0;
        state.players[0].peeked_event = events[0];
        assert_ne!(state.current_events.as_slice().last(), Some(&events[0]));
        let before_len = state.current_events.len();
        let mut rng = PyRandom::new(5);
        determinize(&mut state, &mut rng);
        assert_eq!(state.current_events.len(), before_len, "no card should be dropped or duplicated");
    }

    // ---------------------------------------------------------------- pick

    #[test]
    fn pick_with_a_single_move_returns_it_directly() {
        let state = G::new_game(2, 1);
        let moves = crate::legal::legal_moves(&state);
        let one = [moves.as_slice()[0]];
        let cfg = PlanConfig { width: 2, max_plies: 2, max_nodes: 50, ..PlanConfig::default() };
        let mut stats = Stats::default();
        let mut counters = pending::Counters::default();
        let mut rng = PyRandom::new(1);
        let picked = pick(&cfg, &mut stats, &mut counters, &mut rng, &state, &one);
        assert_eq!(picked, one[0]);
        assert_eq!(stats, Stats::default(), "the single-move short circuit must not touch stats");
    }

    #[test]
    fn pick_never_mutates_the_real_state() {
        let state = G::new_game(2, 2);
        let before = state.clone();
        let moves = crate::legal::legal_moves(&state);
        let cfg = PlanConfig { width: 2, max_plies: 3, max_nodes: 100, ..PlanConfig::default() };
        let mut stats = Stats::default();
        let mut counters = pending::Counters::default();
        let mut rng = PyRandom::new(1);
        let _ = pick(&cfg, &mut stats, &mut counters, &mut rng, &state, moves.as_slice());
        assert_eq!(state.card_row, before.card_row);
        assert_eq!(state.turn, before.turn);
        assert_eq!(state.civil_deck, before.civil_deck, "search runs on clones only -- the real deck must not move");
    }

    #[test]
    fn pick_always_returns_an_offered_move() {
        for n in [2u8, 3, 4] {
            let state = G::new_game(n, 3);
            let moves = crate::legal::legal_moves(&state);
            let cfg = PlanConfig { width: 3, max_plies: 3, max_nodes: 200, ..PlanConfig::default() };
            let mut stats = Stats::default();
            let mut counters = pending::Counters::default();
            let mut rng = PyRandom::new(1);
            let mv = pick(&cfg, &mut stats, &mut counters, &mut rng, &state, moves.as_slice());
            assert!(moves.as_slice().contains(&mv), "{n}p: {mv:?} was not offered");
        }
    }

    /// A zero node budget degrades to the synthetic root's own empty `best`
    /// (nothing reachable is ever scored), landing on the safety-net
    /// `moves[0]` fallback -- not a crash.
    #[test]
    fn a_zero_node_budget_degrades_gracefully_not_a_crash() {
        let state = G::new_game(2, 9);
        let moves = crate::legal::legal_moves(&state);
        let cfg = PlanConfig { max_nodes: 0, ..PlanConfig::default() };
        let mut stats = Stats::default();
        let mut counters = pending::Counters::default();
        let mut rng = PyRandom::new(1);
        let mv = pick(&cfg, &mut stats, &mut counters, &mut rng, &state, moves.as_slice());
        assert!(moves.as_slice().contains(&mv));
    }

    /// Proves the shared `pending` policy is actually wired through, not
    /// re-inlined -- the Rust equivalent of Python's
    /// `tests/test_pending_fallback_is_shared.py` (see this module's top doc
    /// comment). Built directly with a pending `Defense`, decider = the
    /// attacker's target -- `pick`'s `not_my_turn` branch must fire and must
    /// route through `pending::fallback_pick`/`prepare_root`, provable only
    /// by checking THEIR counters moved.
    #[test]
    fn pending_branch_routes_through_the_shared_policy() {
        use crate::state::{Defense, Pending};
        let mut state = G::new_game(3, 1);
        state.current = 0;
        // A card in the defender's military hand is what makes `Defend`
        // moves legal alongside `DefendDone` -- see
        // `interact::pending_moves`'s `Pending::Defense` arm.
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
        let cfg = PlanConfig::default();
        let mut stats = Stats::default();
        let mut counters = pending::Counters::default();
        let mut rng = PyRandom::new(1);
        let mv = pick(&cfg, &mut stats, &mut counters, &mut rng, &state, moves.as_slice());
        assert!(moves.as_slice().contains(&mv));
        assert_eq!(counters.calls, 1, "fallback_pick must have been called exactly once");
        assert_eq!(counters.roots, 1, "prepare_root must have been called exactly once");
    }

    // ---------------------------------------------------------------- score

    /// Mirrors `quiescent.rs`'s own `war_value_prices_a_resolvable_war_
    /// through_the_engine_itself`: `score` must price a declared, unresolved
    /// war of mine through `quiescent::war_value`, not as pure cost, and must
    /// count it in `stats.wars_priced`.
    #[test]
    fn score_prices_a_declared_war_through_war_value_not_as_pure_cost() {
        let mut state = G::new_game(2, 77);
        let war = war_card("War over Territory");
        state.players[0].war_declared_by_me = war;
        state.players[0].war_target = 1;
        state.players[1].wars_declared_on_me[0] = war;
        let warriors = war_card("Warriors");
        state.players[0].techs.get_mut(warriors).unwrap().workers = 12;

        let w = Weights::default();
        let ctx = rivals::rival_context(&state, 0, None, None);
        let mut stats = Stats::default();
        let looked = score(&state, 0, &w, &ctx, true, &mut stats);
        assert_eq!(stats.wars_priced, 1);

        let mut scratch = state.clone();
        let outcome = combat::resolve_war_outcome(&mut scratch, 0).expect("a 12-worker edge must not be a tie");
        combat::apply_war_spoils(&mut scratch, &outcome);
        interact::settle_war_spoils(&mut scratch);
        let expected = eval::evaluate(&scratch, 0, &w, Some(&ctx), None);
        assert_eq!(looked, expected);

        // Pure cost would have scored the UNRESOLVED position instead --
        // negative control proving the branch actually fired.
        let unresolved = eval::evaluate(&state, 0, &w, Some(&ctx), None);
        assert_ne!(looked, unresolved);
    }

    /// `war_lookahead = false` must fall back to pricing the position as it
    /// stands -- the war costs the military card/actions and nothing is
    /// priced back in.
    #[test]
    fn score_with_war_lookahead_off_prices_the_position_as_it_stands() {
        let mut state = G::new_game(2, 77);
        let war = war_card("War over Territory");
        state.players[0].war_declared_by_me = war;
        state.players[0].war_target = 1;
        state.players[1].wars_declared_on_me[0] = war;

        let w = Weights::default();
        let ctx = rivals::rival_context(&state, 0, None, None);
        let mut stats = Stats::default();
        let looked = score(&state, 0, &w, &ctx, false, &mut stats);
        assert_eq!(stats.wars_priced, 0);
        assert_eq!(looked, eval::evaluate(&state, 0, &w, Some(&ctx), None));
    }

    // -------------------------------------------------------------- quiesce

    /// Mirrors `quiescent.rs`'s own `resolve_drains_a_real_pending_choice_
    /// to_quiet`: a real oversized-hand discard decision must be fully
    /// drained within the default cap.
    #[test]
    fn quiesce_drains_a_real_pending_choice_to_quiet() {
        let mut state = G::new_game(2, 6);
        let extra: Vec<CardId> = crate::cards::CARDS
            .iter()
            .enumerate()
            .filter(|(_, c)| c.kind == crate::cards::CardType::Aggression)
            .take(4)
            .map(|(i, _)| CardId(i as u16))
            .collect();
        for &c in &extra {
            state.players[0].hand_military.push(c);
        }
        let forced = interact::discard_excess_military(&mut state, 0);
        assert!(forced, "the hand was built oversized on purpose");
        assert!(!state.pending.is_empty());
        let w = Weights::default();
        quiesce(&mut state, &w, None, None);
        assert!(state.pending.is_empty(), "a real discard decision must resolve within the default cap");
    }

    #[test]
    fn quiesce_with_nothing_pending_is_a_no_op() {
        let mut state = G::new_game(2, 1);
        assert!(state.pending.is_empty());
        let w = Weights::default();
        quiesce(&mut state, &w, None, None);
        assert!(state.pending.is_empty());
    }
}
