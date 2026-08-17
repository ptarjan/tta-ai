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

use super::neural::policy_order::PolicyOrder;
use super::pending;
use super::quiescent;
use super::weighted::eval;
use super::weighted::rivals::{self, RivalContext};
use super::weighted::weights::Weights;

// ------------------------------------------------------------ determinize

/// Re-shuffle what the mover cannot see: `state.civil_deck`, `state.
/// military_deck`, `state.current_events`. See this module's top doc comment
/// and Python's own (much longer) comment on `determinize` for the full
/// rationale. The `current_events` third is split out as [`determinize_
/// current_events`] -- see that function's own doc comment for why it has a
/// second, narrower caller.
pub fn determinize(state: &mut GameState, rng: &mut PyRandom) {
    if !state.civil_deck.is_empty() {
        shuffle_cards(rng, state.civil_deck.as_mut_slice());
    }
    if !state.military_deck.is_empty() {
        shuffle_cards(rng, state.military_deck.as_mut_slice());
    }
    determinize_current_events(state, rng);
}

/// The `current_events` third of [`determinize`], on its own: re-shuffle the
/// pile the mover cannot see, preserving a genuinely-peeked top card (Joan of
/// Arc) and the pile's public age-descending order. See [`determinize`]'s
/// own doc comment and Python's (much longer) one on `determinize` for the
/// full rationale; restated here only for the Rust-shape note: the
/// peeked-event carve-out reads `state.players[state.decider()].peeked_event`
/// and must finish BEFORE `state.current_events` is borrowed mutably, since
/// both live on the same `state`.
///
/// Split out because it has a second kind of caller besides [`pick`]:
/// [`super::weighted::eval::WeightedBot::choose`] and [`super::quiescent::
/// pick`] each run a 1-ply trial per candidate move with no determinized root
/// of their own (unlike this module's `PlanBot`-shaped search, they never
/// called [`determinize`] at all before this fix -- an information leak,
/// since `Move::PrepareEvent` (`apply::h_prepare_event`) reveals-and-resolves
/// the TRUE top event mid-`apply`, unconditionally). Both close that leak by
/// calling this function once per decision, on a shared root all their
/// per-candidate trials clone from -- never the FULL [`determinize`]:
/// `civil_deck`/`military_deck` can run 50+ cards deep, and nothing either
/// bot's `evaluate` reads by card IDENTITY there (`bots/weighted/horizon.rs`
/// reads `civil_deck.len()`, never a card out of it) the way [`my_event_
/// threat`](super::weighted::events::my_event_threat) and a resolved event's
/// immediate effects read `current_events`'s top card -- so shuffling those
/// two piles too would cost real time for zero change in what either bot's
/// score could possibly depend on.
pub(crate) fn determinize_current_events(state: &mut GameState, rng: &mut PyRandom) {
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
/// pending-decision branch). Checked arithmetic in `i128`, not wrapping --
/// mirrors `economy::deck_rng`/`game::rng_for`'s identical justification
/// (see `game::rng_for`'s doc comment for why `i128`, not `i64`, is the
/// permanent-headroom width for a `u64` seed times a small fixed
/// multiplier): Python's ints are unbounded, so a seed big enough to
/// overflow would draw a DIFFERENT MT19937 stream there than any
/// fixed-width Rust integer could here.
pub(crate) fn plan_rng(state: &GameState, me: u8) -> PyRandom {
    let seed = i128::from(state.seed)
        .checked_mul(7919)
        .and_then(|s| s.checked_add(state.turn as i128 * 31))
        .and_then(|s| s.checked_add(me as i128))
        .expect(
            "game seed * 7919 + turn * 31 + me overflows i128; Python's unbounded ints would \
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
    /// How many of `searches` (calls to [`beam`]) were cut short by
    /// `cfg.max_nodes` -- `budget` reached zero before some frontier node's
    /// candidate list was fully examined, rather than the search finishing
    /// every ply on its own. Purely observational (nothing reads it to
    /// change search behaviour): it exists to answer one question from the
    /// outside -- at a given `max_nodes`, what fraction of real decisions
    /// were actually starved? -- without instrumenting a caller by hand.
    /// See `docs/NEURAL.md`'s policy-head follow-up section for why this
    /// number matters: a move-ordering prior can only matter when the
    /// budget it reorders candidates FOR genuinely runs out.
    pub searches_capped: u64,
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
/// Somewhere for a caller to collect the positions a search actually PRICED,
/// so a training-data generator can learn the leaf evaluator's own input
/// distribution instead of guessing at it.
///
/// This exists because of a measured distribution bug, not for generality.
/// `experiments/neural_rankdata.py` labelled value rows with *pre-move,
/// mid-turn* encodings, and `experiments/plan_teacher_gen.py`'s top doc
/// comment names that as the first of two bugs it closes: "A beam leaf
/// evaluator is asked about *quiet, end-of-my-turn* positions and nothing
/// else." `docs/BOT_ARCHITECTURE.md` 3b: "PlanBot evaluates only at turn
/// boundaries, which is exactly the distribution a boundary-only fit is
/// trained on ... the fitted vector and PlanBot are a matched pair by
/// construction." A net fitted on the wrong distribution is asked, at play
/// time, about states it never saw in training.
///
/// [`Bank::Off`] is the default every ordinary caller uses and costs
/// nothing: [`Bank::push`] takes a CLOSURE, so the clone or encode that
/// builds the collected value never runs at all unless someone is
/// collecting.
pub enum Bank<T> {
    /// Collect nothing. What `pick` uses, so the search is not slowed down
    /// by a facility only the generator wants.
    Off,
    On(Vec<T>),
}

impl<T> Bank<T> {
    pub fn collecting() -> Bank<T> {
        Bank::On(Vec::new())
    }

    /// Append `make()`'s result -- but only when collecting.
    pub fn push(&mut self, make: impl FnOnce() -> T) {
        match self {
            Bank::Off => {}
            Bank::On(v) => v.push(make()),
        }
    }

    /// A second bank in the same MODE as this one. Two closures cannot both
    /// hold `&mut` on one bank even when only one of them will ever run
    /// (`pending::fallback_pick` takes exactly such a pair), so a caller in
    /// that position gives each side its own bank and [`Bank::absorb`]s
    /// afterwards.
    pub fn like(&self) -> Bank<T> {
        match self {
            Bank::Off => Bank::Off,
            Bank::On(_) => Bank::On(Vec::new()),
        }
    }

    /// Move everything `other` collected into this bank.
    pub fn absorb(&mut self, other: Bank<T>) {
        match (self, other) {
            (Bank::On(mine), Bank::On(theirs)) => mine.extend(theirs),
            // Nothing to move, or nowhere to move it to. Both are the
            // honest no-op: a bank that is Off never collected anything,
            // and one that is Off was never asked to keep anything.
            (Bank::On(_), Bank::Off) | (Bank::Off, Bank::On(_)) | (Bank::Off, Bank::Off) => {}
        }
    }

    /// Take what has been collected so far, leaving the bank empty and still
    /// collecting. An empty `Vec` when [`Bank::Off`], which is the honest
    /// answer: nobody asked for anything.
    pub fn take(&mut self) -> Vec<T> {
        match self {
            Bank::Off => Vec::new(),
            Bank::On(v) => std::mem::take(v),
        }
    }
}

pub fn pick(
    cfg: &PlanConfig,
    stats: &mut Stats,
    counters: &mut pending::Counters,
    rng: &mut PyRandom,
    state: &GameState,
    moves: &[Move],
) -> Move {
    pick_collecting(cfg, stats, counters, rng, state, moves, &mut Bank::Off, None)
}

/// [`pick`], plus every position the beam priced appended to `bank`. The
/// teacher-data generator (`bots::neural::rankdata`) is the only caller that
/// passes anything but [`Bank::Off`]; see [`Bank`] for why it needs them.
///
/// `policy`: the move-ordering prior (`docs/NEURAL.md`'s "The policy head"),
/// OFF by default -- every existing caller of [`pick`] passes `None` here
/// (unconditionally, via [`pick`]'s own fixed-arity wrapper above), so the
/// search takes EXACTLY today's code path unless a caller opts in by
/// building a [`PolicyOrder`] and passing it explicitly. See [`beam`]'s own
/// doc comment for what changes, and does not change, when it is `Some`.
// Grouping these into a config struct is a real fix but a larger,
// cross-cutting refactor (every call site would need updating too) --
// out of scope for this lint-gate pass, which must not change behaviour.
// The argument list itself is stable and each parameter is unambiguous
// at every call site.
#[allow(clippy::too_many_arguments)]
pub fn pick_collecting(
    cfg: &PlanConfig,
    stats: &mut Stats,
    counters: &mut pending::Counters,
    rng: &mut PyRandom,
    state: &GameState,
    moves: &[Move],
    bank: &mut Bank<GameState>,
    policy: Option<&mut PolicyOrder>,
) -> Move {
    // `beam` below already refuses to EXPAND a `Move::Resign` candidate (see
    // its own per-move `continue`), so in the ordinary case Resign never wins
    // on score. But nothing here ever checked what `moves[0]` -- the raw,
    // UNfiltered root list -- actually is, and this function falls back to
    // exactly that when nothing scores at all: `chosen.map(...)
    // .unwrap_or(moves[0])`, reached whenever `cfg.max_nodes` starves before
    // a single candidate is scored. Today `legal::politics_moves` happens to
    // push `PolPass` before `Resign`, so `moves[0]` is never actually Resign
    // -- but that ordering is an implementation detail of a function this
    // one does not call and has no contract with; "happens not to" is not a
    // guarantee. Filtering here, at the root, the same way every OTHER
    // search bot in this crate does (`crate::bots::filter_resign`) closes
    // that off by construction instead of by move-generation order.
    let filtered = crate::bots::filter_resign(moves, false);
    let moves: &[Move] = filtered.as_slice();
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
            || one_ply_quiet(&root, moves, me, w, &ctx, cfg.war_lookahead, stats, bank),
        );
    }

    let totals = search_totals(cfg, stats, state, moves, me, w, &ctx, bank, policy);
    best_from_totals(&totals).unwrap_or(moves[0])
}

/// The per-root-candidate accumulation [`pick_collecting`] uses to choose its
/// one winner: `(move, summed terminal score, samples that reached it)`,
/// averaged over `cfg.samples` determinizations of `state`. A collection of
/// triples, not `totals`/`seen` kept in step by index (house style).
///
/// Factored out so [`rank`] can read the SAME numbers `pick_collecting`
/// itself computes to choose a winner, rather than re-running the beam once
/// per candidate to recover them -- re-running would multiply search cost by
/// the branching factor for information this one search call already has.
// Grouping these into a config struct is a real fix but a larger,
// cross-cutting refactor (every call site would need updating too) --
// out of scope for this lint-gate pass, which must not change behaviour.
// The argument list itself is stable and each parameter is unambiguous
// at every call site.
#[allow(clippy::too_many_arguments)]
fn search_totals(
    cfg: &PlanConfig,
    stats: &mut Stats,
    state: &GameState,
    moves: &[Move],
    me: u8,
    w: &Weights,
    ctx: &RivalContext,
    bank: &mut Bank<GameState>,
    mut policy: Option<&mut PolicyOrder>,
) -> Vec<(Move, f64, u32)> {
    let mut totals: Vec<(Move, f64, u32)> = moves.iter().map(|&m| (m, 0.0, 0u32)).collect();
    let mut drng = plan_rng(state, me);
    for _ in 0..cfg.samples {
        let mut root = state.clone();
        if cfg.bot.determinize {
            determinize(&mut root, &mut drng);
        }
        let best = beam(cfg, stats, &root, moves, me, w, ctx, bank, policy.as_deref_mut());
        for (mv, v) in best {
            if let Some(entry) = totals.iter_mut().find(|(m, _, _)| *m == mv) {
                entry.1 += v;
                entry.2 += 1;
            }
        }
    }
    totals
}

/// The single best `(avg score, move)` in `totals`, ignoring any root
/// candidate no sample ever reached (`seen == 0`) -- `pick_collecting`'s own
/// argmax loop, factored out so [`rank`] does not have to restate it.
fn best_from_totals(totals: &[(Move, f64, u32)]) -> Option<Move> {
    let mut chosen: Option<(f64, Move)> = None;
    for &(mv, total, seen) in totals {
        if seen == 0 {
            continue;
        }
        let avg = total / seen as f64;
        if chosen.is_none_or(|(bv, _)| avg > bv) {
            chosen = Some((avg, mv));
        }
    }
    chosen.map(|(_, m)| m)
}

/// [`pick_collecting`]'s search, but returning EVERY root candidate's
/// averaged score, best first, instead of collapsing to one winner -- what a
/// caller that wants to show a human several ranked options (not just a
/// single verdict) needs. Runs the search exactly ONCE per call, same as
/// [`pick_collecting`]: see [`search_totals`]'s own doc comment for why that
/// matters.
///
/// A root candidate is absent from the result -- rather than padded with a
/// guessed score -- whenever [`beam`] never actually recorded a terminal
/// value for it into `best`. Two distinct ways that happens, both real and
/// both common, not just an edge case: `cfg.max_nodes` can starve the ROOT
/// ply itself before every candidate's turn in the loop, OR (far more often)
/// a candidate's whole line can get pruned out of the frontier by `cfg.
/// width` truncation, ply after ply, before any of its descendants ever
/// reaches a terminal position (`t.game_over || t.current != me`) --
/// `update_best` is only ever called from a terminal position, so a
/// truncated-out line contributes nothing at all, no matter how generous
/// `max_nodes` is. A caller that must never drop a legal move from view has
/// to notice the gap and fall back to its own 1-ply score for it
/// (`advisor::session::rank_moves_beam` does exactly this).
///
/// Outside the decider's own ordinary turn (`pending::not_my_turn`, mirroring
/// [`pick_collecting`]'s identical branch) there is no multi-move "turn" to
/// beam-search: every legal move is scored flat, one ply deep (draining the
/// pending stack first when [`pending::wants_quiet`] says so), through the
/// same [`pending::fallback_pick`] policy `pick_collecting` itself routes
/// through -- so `counters` moves exactly as it would for [`pick`] at an
/// identical decision.
// Grouping these into a config struct is a real fix but a larger,
// cross-cutting refactor (every call site would need updating too) --
// out of scope for this lint-gate pass, which must not change behaviour.
// The argument list itself is stable and each parameter is unambiguous
// at every call site.
#[allow(clippy::too_many_arguments)]
pub fn rank(
    cfg: &PlanConfig,
    stats: &mut Stats,
    counters: &mut pending::Counters,
    rng: &mut PyRandom,
    state: &GameState,
    moves: &[Move],
    bank: &mut Bank<GameState>,
    policy: Option<&mut PolicyOrder>,
) -> Vec<(Move, f64)> {
    let filtered = crate::bots::filter_resign(moves, false);
    let moves: &[Move] = filtered.as_slice();
    if moves.is_empty() {
        return Vec::new();
    }
    if moves.len() == 1 {
        return vec![(moves[0], 0.0)];
    }
    let me = state.decider();
    let w = &cfg.weights;
    let ctx = rivals::rival_context(state, me, None, None);

    if pending::not_my_turn(state, me) {
        let root = pending::prepare_root(&cfg.bot, state, counters, determinize, rng);
        return pending::fallback_pick(
            &cfg.bot,
            state,
            counters,
            || one_ply_ranked(&root, moves, me, w, &ctx),
            || one_ply_quiet_ranked(&root, moves, me, w, &ctx, cfg.war_lookahead, stats, bank),
        );
    }

    let totals = search_totals(cfg, stats, state, moves, me, w, &ctx, bank, policy);
    let mut ranked: Vec<(Move, f64)> = totals
        .into_iter()
        .filter(|&(_, _, seen)| seen > 0)
        .map(|(mv, total, seen)| (mv, total / f64::from(seen)))
        .collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    ranked
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
///
/// `policy`, when `Some`, reorders EVERY node's candidate list
/// (`docs/NEURAL.md`'s policy head, most-preferred-first) before this
/// function's own budget/quiesce/score loop below ever looks at it --
/// [`PolicyOrder::order_moves`] permutes in place, dropping nothing, so
/// which candidates get expanded before `budget` runs out changes, but the
/// SET of moves reachable at unlimited budget does not. `policy: None`
/// (every call site today, transitively from [`pick`]) takes the identical
/// branch every one of these `match`es already had -- `mvs = moves` /
/// `mvs = generated.as_slice()`, completely unordered -- so the flag-off
/// path is not just "close to" today's search, it is the same code running
/// the same comparisons in the same order.
// Grouping these into a config struct is a real fix but a larger,
// cross-cutting refactor (every call site would need updating too) --
// out of scope for this lint-gate pass, which must not change behaviour.
// The argument list itself is stable and each parameter is unambiguous
// at every call site.
#[allow(clippy::too_many_arguments)]
fn beam(
    cfg: &PlanConfig,
    stats: &mut Stats,
    root: &GameState,
    moves: &[Move],
    me: u8,
    w: &Weights,
    ctx: &RivalContext,
    bank: &mut Bank<GameState>,
    mut policy: Option<&mut PolicyOrder>,
) -> Vec<(Move, f64)> {
    stats.searches += 1;
    let mut budget = cfg.max_nodes;
    let mut frontier = vec![Frontier { score: 0.0, state: root.clone(), first: None }];
    let mut best: Vec<(Move, f64)> = Vec::new();

    for _ply in 0..cfg.max_plies {
        let mut nxt: Vec<Frontier> = Vec::new();
        for entry in &frontier {
            let mut generated;
            // Only the ROOT ply (`entry.first == None`) can see `moves`
            // directly; a policy-guided root additionally needs its OWN
            // mutable copy to permute, since `moves` is a borrowed `&[Move]`
            // shared across every sample in `pick_collecting`'s outer loop.
            // `MoveList` is a fixed stack array (`moves.rs::MAX_MOVES`), so
            // this copy is not a heap allocation.
            let mut root_ordered;
            let mvs: &[Move] = match entry.first {
                None => match policy.as_deref_mut() {
                    None => moves,
                    Some(p) => {
                        root_ordered = crate::moves::MoveList::new();
                        for &mv in moves {
                            root_ordered.push(mv);
                        }
                        p.order_moves(&entry.state, me, root_ordered.as_mut_slice());
                        root_ordered.as_slice()
                    }
                },
                Some(_) => {
                    generated = crate::legal::legal_moves(&entry.state);
                    if let Some(p) = policy.as_deref_mut() {
                        p.order_moves(&entry.state, me, generated.as_mut_slice());
                    }
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
                // Collected AFTER `quiesce` and before the frontier decides
                // anything: this is the exact position `score` was handed.
                bank.push(|| t.clone());
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
    // `budget <= 0` here means the search stopped because `cfg.max_nodes`
    // ran out, not because the tree itself was exhausted (`nxt.is_empty()`
    // with budget to spare) or `max_plies` was reached with room left --
    // see `Stats::searches_capped`'s own doc comment for why this number is
    // worth reading from outside. The one false positive this admits (the
    // tree finishes on EXACTLY the last unit of budget, by coincidence, with
    // nothing left unexamined) is a single-node-wide sliver of `max_nodes`
    // space and not worth a second flag to exclude.
    if budget <= 0 {
        stats.searches_capped += 1;
    }
    best
}

/// `best[mv] = max(best.get(mv), v)`, first-insertion order preserved.
///
/// `pub(crate)`: also called by [`super::neural::plan::beam`], which needs
/// the identical "keep the max terminal value reached through each root
/// candidate" accumulation Python's own `neural_plan.py::_beam` spells with
/// the same `best.get(f)`/`v > best[f]` shape `plan.py::_beam` uses.
pub(crate) fn update_best(best: &mut Vec<(Move, f64)>, mv: Move, v: f64) {
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
///
/// EXCEPT when the war cannot resolve before the game ends. `apply.rs::
/// h_war`'s own doc comment records that resolution fires at the start of
/// the declarer's NEXT turn (`game.rs::start_turn` ->
/// `combat::resolve_war_outcome`), not immediately -- and `game.rs::
/// advance_turn` ends the game at the wrap into `final_round_end + 1`,
/// before that next turn is ever reached, whenever the war was declared in
/// `t.last_round` (`round >= final_round_end`; `game.rs::set_last_round`'s
/// own doc comment: everyone finishes the round they are in, nobody starts
/// another). A war declared there is real -- `h_war` already paid its cost
/// on `t` (a military action, the card, any pact it broke) -- but it will
/// sit open, unfought, forever: pricing it through `war_value`'s optimistic
/// "resolved right now" would credit spoils this trial can never actually
/// collect. `t.last_round` is what the engine itself already tracks for
/// exactly this question, so this reads it rather than re-deriving "is
/// there time left" from `t.round`/`t.final_round_end` by hand.
fn score(t: &GameState, me: u8, w: &Weights, ctx: &RivalContext, war_lookahead: bool, stats: &mut Stats) -> f64 {
    if war_lookahead && !t.game_over && !t.last_round && !t.players[me as usize].war_declared_by_me.is_none() {
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
    one_ply_ranked(state, moves, me, w, ctx).first().map(|&(m, _)| m).unwrap_or(moves[0])
}

/// [`one_ply`], but returns every candidate's score, best first, instead of
/// only the winner -- [`rank`]'s counterpart to [`one_ply`] at a non-ordinary
/// -turn decision. A stable sort keeps [`one_ply`]'s own tie-break (first
/// candidate seen wins a tie) exactly: `.first()` after a stable descending
/// sort is the first-inserted maximum, identical to the strict `v > bv`
/// running-best this replaces.
fn one_ply_ranked(state: &GameState, moves: &[Move], me: u8, w: &Weights, ctx: &RivalContext) -> Vec<(Move, f64)> {
    let mut ranked: Vec<(Move, f64)> = moves
        .iter()
        .map(|&mv| {
            let mut t = state.clone();
            apply::apply(&mut t, mv);
            (mv, eval::evaluate(&t, me, w, Some(ctx), None))
        })
        .collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    ranked
}

/// `one_ply`, but drain the pending stack before scoring -- mirrors
/// `PlanBot._one_ply_quiet`. THE INCONSISTENCY THIS EXISTS TO REMOVE: every
/// node inside [`beam`] is scored `apply -> quiesce -> score`, so at a REAL
/// pending decision of mine the identical position must be priced the same
/// way, or (Python's own measurement, `docs/AUDIT_HISTORY.md`) a defender
/// spends cards on arithmetically hopeless defences and holds off none of
/// the winnable ones, because an undrained position cannot express whether a
/// defence succeeds.
// Grouping these into a config struct is a real fix but a larger,
// cross-cutting refactor (every call site would need updating too) --
// out of scope for this lint-gate pass, which must not change behaviour.
// The argument list itself is stable and each parameter is unambiguous
// at every call site.
#[allow(clippy::too_many_arguments)]
fn one_ply_quiet(
    state: &GameState,
    moves: &[Move],
    me: u8,
    w: &Weights,
    ctx: &RivalContext,
    war_lookahead: bool,
    stats: &mut Stats,
    bank: &mut Bank<GameState>,
) -> Move {
    one_ply_quiet_ranked(state, moves, me, w, ctx, war_lookahead, stats, bank)
        .first()
        .map(|&(m, _)| m)
        .unwrap_or(moves[0])
}

/// [`one_ply_quiet`], but returns every candidate's score, best first --
/// [`rank`]'s counterpart to [`one_ply_quiet`] at a non-ordinary-turn
/// decision that wants the pending stack drained before scoring. See
/// [`one_ply_ranked`]'s own doc comment for why `.first()` after a stable
/// sort reproduces the original running-best tie-break exactly.
// Grouping these into a config struct is a real fix but a larger,
// cross-cutting refactor (every call site would need updating too) --
// out of scope for this lint-gate pass, which must not change behaviour.
// The argument list itself is stable and each parameter is unambiguous
// at every call site.
#[allow(clippy::too_many_arguments)]
fn one_ply_quiet_ranked(
    state: &GameState,
    moves: &[Move],
    me: u8,
    w: &Weights,
    ctx: &RivalContext,
    war_lookahead: bool,
    stats: &mut Stats,
    bank: &mut Bank<GameState>,
) -> Vec<(Move, f64)> {
    let mut ranked: Vec<(Move, f64)> = Vec::with_capacity(moves.len());
    for &mv in moves {
        let mut t = state.clone();
        apply::apply(&mut t, mv);
        quiesce(&mut t, w, Some(&ctx.root_row), Some((&ctx.civil_outlook, &ctx.event_pool)));
        let v = score(&t, me, w, ctx, war_lookahead, stats);
        bank.push(|| t.clone());
        ranked.push((mv, v));
    }
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    ranked
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
///
/// `pub(crate)`: also called directly by [`super::neural::plan`], which
/// drains its own beam's pending decisions with the SAME plain-LINEAR
/// 1-ply pick this function already implements (`neural_plan.py`'s own
/// `_quiesce` is -- after a 2026-08-05 fix that made it actually true --
/// documented as "exactly as PlanBot._quiesce does"). Calling this function
/// instead of forking a second copy is what makes that "exactly" durable
/// rather than a comment two files can silently drift apart under.
/// Borrowed form of `rivals::RootCounts` (see that alias): `(civil outlook,
/// (event pool, its own total))`, by reference since `quiesce` never needs
/// to own or mutate either.
type RootCountsRef<'a> = (&'a Vec<(CardId, f64)>, &'a (Vec<(CardId, u16)>, f64));

pub(crate) fn quiesce(
    st: &mut GameState,
    w: &Weights,
    root_row: Option<&CardList<ROW_SIZE>>,
    root_counts: Option<RootCountsRef>,
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

    // ---------------------------------------------------------------- rank

    /// [`rank`] must return only offered moves, each at most once, sorted
    /// best-first -- the shape the advisor's beam mode needs to print a
    /// ranked list, not just one winner. It must NOT be assumed to cover
    /// every root candidate: [`rank`]'s own doc comment explains why a
    /// truncated-out line can leave a real legal move absent even at a
    /// generous node budget, so this test only pins the invariants that
    /// always hold.
    #[test]
    fn rank_returns_a_sorted_duplicate_free_subset_of_the_offered_moves() {
        let state = G::new_game(3, 5);
        let moves = crate::legal::legal_moves(&state);
        let cfg = PlanConfig { width: 4, max_plies: 3, max_nodes: 400, ..PlanConfig::default() };
        let mut stats = Stats::default();
        let mut counters = pending::Counters::default();
        let mut rng = PyRandom::new(1);
        let ranked =
            rank(&cfg, &mut stats, &mut counters, &mut rng, &state, moves.as_slice(), &mut Bank::Off, None);
        assert!(!ranked.is_empty());
        assert!(ranked.len() <= moves.as_slice().len());
        for &(mv, _) in &ranked {
            assert!(moves.as_slice().contains(&mv));
            assert_eq!(ranked.iter().filter(|&&(m, _)| m == mv).count(), 1, "{mv:?} appeared more than once");
        }
        for w in ranked.windows(2) {
            assert!(w[0].1 >= w[1].1, "not sorted best-first: {:?}", ranked);
        }
    }

    /// [`rank`]'s own winner (the first entry) must agree with what [`pick`]
    /// returns on the identical (state, config, seed) -- two different views
    /// of the SAME search, not two searches that can disagree.
    #[test]
    fn rank_agrees_with_pick_on_which_move_is_best() {
        let state = G::new_game(3, 5);
        let moves = crate::legal::legal_moves(&state);
        let cfg = PlanConfig { width: 4, max_plies: 3, max_nodes: 400, ..PlanConfig::default() };

        let mut stats_a = Stats::default();
        let mut counters_a = pending::Counters::default();
        let mut rng_a = PyRandom::new(1);
        let picked = pick(&cfg, &mut stats_a, &mut counters_a, &mut rng_a, &state, moves.as_slice());

        let mut stats_b = Stats::default();
        let mut counters_b = pending::Counters::default();
        let mut rng_b = PyRandom::new(1);
        let ranked =
            rank(&cfg, &mut stats_b, &mut counters_b, &mut rng_b, &state, moves.as_slice(), &mut Bank::Off, None);

        assert_eq!(ranked[0].0, picked, "rank's top entry must match pick's own winner");
    }

    /// A single legal move short-circuits to a one-entry list scored `0.0`,
    /// matching [`pick`]'s identical short-circuit (and untouched `Stats`,
    /// since neither ever reaches the search).
    #[test]
    fn rank_with_a_single_move_returns_it_alone_at_zero() {
        let state = G::new_game(2, 1);
        let moves = crate::legal::legal_moves(&state);
        let one = [moves.as_slice()[0]];
        let cfg = PlanConfig::default();
        let mut stats = Stats::default();
        let mut counters = pending::Counters::default();
        let mut rng = PyRandom::new(1);
        let ranked = rank(&cfg, &mut stats, &mut counters, &mut rng, &state, &one, &mut Bank::Off, None);
        assert_eq!(ranked, vec![(one[0], 0.0)]);
        assert_eq!(stats, Stats::default());
    }

    /// At a non-ordinary-turn decision (a real pending `Defense`), [`rank`]
    /// must still return every offered candidate and must route through the
    /// shared [`pending`] policy exactly like [`pick`] does -- the ranking
    /// counterpart of `pending_branch_routes_through_the_shared_policy`.
    #[test]
    fn rank_at_a_pending_decision_routes_through_the_shared_policy_and_ranks_everything() {
        use crate::state::{Defense, Pending};
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
        let cfg = PlanConfig::default();
        let mut stats = Stats::default();
        let mut counters = pending::Counters::default();
        let mut rng = PyRandom::new(1);
        let ranked =
            rank(&cfg, &mut stats, &mut counters, &mut rng, &state, moves.as_slice(), &mut Bank::Off, None);
        assert_eq!(ranked.len(), moves.as_slice().len());
        assert_eq!(counters.calls, 1, "fallback_pick must have been called exactly once");
        assert_eq!(counters.roots, 1, "prepare_root must have been called exactly once");
    }

    // ------------------------------------------------ policy-guided ordering

    /// A [`PolicyOrder`] that scores `Move::EndTurn` strictly BELOW every
    /// other candidate, at any state, deterministically -- built by hand
    /// rather than trained, so these tests do not depend on a checkpoint
    /// file existing in the checkout. Every stem weight is zero except the
    /// `EndTurn` one-hot column, which gets a per-hidden-unit RAMP (`h + 1`,
    /// not a uniform value): a uniform weight makes every hidden unit
    /// identical for a given input, which `LayerNorm` then collapses to a
    /// state-independent constant, destroying the very signal this needs.
    /// `stem_ln_gamma = 1`/`stem_ln_beta = 0` pin `LayerNorm` to a plain
    /// z-score so the result is determined by the ramp alone, not by
    /// whatever `random_policy_net`'s own random init drew. Net effect:
    /// `EndTurn` (whose one-hot column is 1) scores strictly negative;
    /// every other move (whose one-hot column is 0, so its whole input row
    /// through these weights is the zero vector) scores exactly 0.0 and
    /// ties with every other non-`EndTurn` candidate.
    fn end_turn_last_policy() -> crate::bots::neural::policy_order::PolicyOrder {
        use crate::bots::neural::{action, encode, policy_train};
        let hidden = 4;
        let mut net = policy_train::random_policy_net(hidden, 0);
        let end_turn_col = encode::ENCODING_DIM + action::move_kind_slot(&Move::EndTurn);
        for h in 0..hidden {
            for i in 0..net.in_dim {
                net.stem_w[h * net.in_dim + i] = if i == end_turn_col { (h + 1) as f64 } else { 0.0 };
            }
            net.stem_b[h] = 0.0;
            net.stem_ln_gamma[h] = 1.0;
            net.stem_ln_beta[h] = 0.0;
            net.head_w[h] = -1.0;
        }
        net.head_b = 0.0;
        crate::bots::neural::policy_order::PolicyOrder::from_net(net)
    }

    /// Under a node budget too small to expand every root candidate,
    /// [`beam`] must genuinely process a DIFFERENT set of root moves
    /// depending on the policy prior -- proof the wiring actually reaches
    /// the search's own budget loop, not just that `PolicyOrder::
    /// order_moves` sorts correctly in isolation
    /// (`policy_order.rs`'s own tests already cover that in full).
    ///
    /// `G::new_game(3, 5)`'s root decision offers exactly `[EndTurn, Take{0},
    /// Take{1}, Take{2}, Take{3}, Take{4}]` in `legal_moves`' own raw order
    /// (pinned by this test, not assumed -- the first assertion below fails
    /// loudly if a future change to move generation reorders it, rather
    /// than silently making the rest of this test meaningless). A budget of
    /// 5 (one short of all 6) processes `EndTurn` under that raw order, but
    /// [`end_turn_last_policy`] always ranks `EndTurn` last, so the same
    /// budget can never reach it once the policy has reordered the list.
    #[test]
    fn policy_ordering_changes_which_root_candidates_the_search_can_afford_to_process() {
        let state = G::new_game(3, 5);
        let moves = crate::legal::legal_moves(&state);
        let me = state.decider();
        assert_eq!(
            moves.as_slice(),
            [
                Move::EndTurn,
                Move::Take { slot: 0 },
                Move::Take { slot: 1 },
                Move::Take { slot: 2 },
                Move::Take { slot: 3 },
                Move::Take { slot: 4 },
            ],
            "test is pinned to this exact raw root order; update the pin (and re-check the reasoning \
             in this test's own doc comment) if move generation legitimately changed it"
        );
        let budget = (moves.len() - 1) as i64;

        let w = Weights::default();
        let ctx = rivals::rival_context(&state, me, None, None);
        let cfg = PlanConfig { max_plies: 1, max_nodes: budget, ..PlanConfig::default() };

        // No policy: the raw order above puts `EndTurn` first, so a budget
        // of 5 processes it plus 4 of the 5 `Take`s.
        let mut stats_a = Stats::default();
        let mut bank_a = Bank::collecting();
        let _ = beam(&cfg, &mut stats_a, &state, moves.as_slice(), me, &w, &ctx, &mut bank_a, None);
        let end_turn_seen_a = bank_a.take().iter().any(|t| t.current != me);
        assert!(end_turn_seen_a, "the raw order puts EndTurn first, so the unordered run must process it");

        // Policy-ordered: EndTurn is ALWAYS last, so a budget one short of
        // the full root count can never reach it.
        let mut policy = end_turn_last_policy();
        let mut stats_b = Stats::default();
        let mut bank_b = Bank::collecting();
        let _ =
            beam(&cfg, &mut stats_b, &state, moves.as_slice(), me, &w, &ctx, &mut bank_b, Some(&mut policy));
        let end_turn_seen_b = bank_b.take().iter().any(|t| t.current != me);
        assert!(!end_turn_seen_b, "the policy ranks EndTurn last, so a one-short budget must never reach it");

        assert_ne!(
            end_turn_seen_a, end_turn_seen_b,
            "policy ordering had no observable effect on which root moves the budget could reach"
        );
    }

    /// [`Stats::searches_capped`] counts a [`beam`] call as capped exactly
    /// when `max_nodes` genuinely cut it short (a real candidate at the root
    /// was never examined), and NOT when a generous budget lets the same
    /// small tree finish on its own -- reuses the fixed 6-move root from
    /// [`policy_ordering_changes_which_root_candidates_the_search_can_afford_to_process`]
    /// (pinned there) so "one short" (5) vs. "plenty" (100) are known
    /// quantities, not guesses.
    #[test]
    fn searches_capped_counts_only_decisions_max_nodes_actually_cut_short() {
        let state = G::new_game(3, 5);
        let moves = crate::legal::legal_moves(&state);
        let me = state.decider();
        let w = Weights::default();
        let ctx = rivals::rival_context(&state, me, None, None);

        // One short of the 6 root candidates: the 6th is never scored, so
        // this search is genuinely starved.
        let cfg_short = PlanConfig { max_plies: 1, max_nodes: 5, ..PlanConfig::default() };
        let mut stats_short = Stats::default();
        let _ = beam(&cfg_short, &mut stats_short, &state, moves.as_slice(), me, &w, &ctx, &mut Bank::Off, None);
        assert_eq!(stats_short.searches, 1);
        assert_eq!(stats_short.searches_capped, 1, "5 nodes is one short of all 6 root moves -- must be capped");

        // Far more than the 6 root candidates can ever consume at
        // `max_plies: 1`: the tree finishes on its own, budget to spare.
        let cfg_plenty = PlanConfig { max_plies: 1, max_nodes: 100, ..PlanConfig::default() };
        let mut stats_plenty = Stats::default();
        let _ = beam(&cfg_plenty, &mut stats_plenty, &state, moves.as_slice(), me, &w, &ctx, &mut Bank::Off, None);
        assert_eq!(stats_plenty.searches, 1);
        assert_eq!(stats_plenty.searches_capped, 0, "100 nodes is plenty for 6 root moves -- must not be capped");
    }

    /// `Some(policy)` never changes the SET of moves the search could reach
    /// at unlimited budget: with a budget generous enough for the whole
    /// tree, [`pick`]-through-[`pick_collecting`] with a policy loaded
    /// still returns an offered move, for every player count -- ordering
    /// only, nothing pruned, matching this module's calling task's hard
    /// rule 4.
    #[test]
    fn pick_with_a_policy_still_returns_an_offered_move() {
        for n in [2u8, 3, 4] {
            let state = G::new_game(n, 3);
            let moves = crate::legal::legal_moves(&state);
            let cfg = PlanConfig { width: 3, max_plies: 3, max_nodes: 200, ..PlanConfig::default() };
            let mut stats = Stats::default();
            let mut counters = pending::Counters::default();
            let mut rng = PyRandom::new(1);
            let mut policy = end_turn_last_policy();
            let mv = pick_collecting(
                &cfg,
                &mut stats,
                &mut counters,
                &mut rng,
                &state,
                moves.as_slice(),
                &mut Bank::Off,
                Some(&mut policy),
            );
            assert!(moves.as_slice().contains(&mv), "{n}p: {mv:?} was not offered");
        }
    }

    /// [`pick`] (every real caller's entry point) is unaffected by this
    /// whole module's policy-ordering machinery existing: it always passes
    /// `None`, so its output on a fixed (state, seed) pair must match the
    /// exact move + [`Stats`] this repo produced BEFORE `PolicyOrder` was
    /// wired into [`beam`] -- pinned by literally running this exact
    /// scenario against the pre-change code and copying its output here
    /// (see the calling task's own verification step). A `stats.nodes`
    /// match, not just the chosen move, pins the search order itself, not
    /// merely its final answer.
    #[test]
    fn pick_output_on_fixed_positions_matches_the_pre_policy_order_baseline() {
        let cases: [(u8, u64, usize, u32, i64, Move, u64); 4] = [
            (2, 1, 4, 4, 400, Move::Take { slot: 1 }, 10),
            (3, 7, 3, 3, 300, Move::Take { slot: 0 }, 9),
            (4, 42, 5, 4, 500, Move::Take { slot: 2 }, 11),
            (3, 99, 8, 6, 2000, Move::Take { slot: 2 }, 11),
        ];
        for (players, seed, width, max_plies, max_nodes, expected_mv, expected_nodes) in cases {
            let state = G::new_game(players, seed);
            let moves = crate::legal::legal_moves(&state);
            let cfg = PlanConfig { width, max_plies, max_nodes, ..PlanConfig::default() };
            let mut stats = Stats::default();
            let mut counters = pending::Counters::default();
            let mut rng = PyRandom::new(1);
            let mv = pick(&cfg, &mut stats, &mut counters, &mut rng, &state, moves.as_slice());
            assert_eq!(mv, expected_mv, "{players}p seed={seed}: move regressed from the pre-policy baseline");
            assert_eq!(
                stats.nodes, expected_nodes,
                "{players}p seed={seed}: node count regressed from the pre-policy baseline"
            );
        }
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

    /// A war declared in the last round never reaches the declarer's next
    /// turn (`game.rs::advance_turn` ends the game at the wrap into
    /// `final_round_end + 1` instead of starting it) -- see this function's
    /// own doc comment for the full timing argument. `score` must therefore
    /// price it as it actually stands (cost paid, no spoils ever), exactly
    /// like the `war_lookahead = false` case above, NOT through `war_value`'s
    /// "resolved right now" optimism -- built with the identical decisive
    /// strength edge `score_prices_a_declared_war_through_war_value_not_as_
    /// pure_cost` uses, so a passing `war_value` branch would visibly move
    /// the score if it fired here (it must not).
    #[test]
    fn score_does_not_price_a_last_round_war_through_war_value() {
        let mut state = G::new_game(2, 77);
        let war = war_card("War over Territory");
        state.players[0].war_declared_by_me = war;
        state.players[0].war_target = 1;
        state.players[1].wars_declared_on_me[0] = war;
        let warriors = war_card("Warriors");
        state.players[0].techs.get_mut(warriors).unwrap().workers = 12;
        state.last_round = true;

        let w = Weights::default();
        let ctx = rivals::rival_context(&state, 0, None, None);
        let mut stats = Stats::default();
        let looked = score(&state, 0, &w, &ctx, true, &mut stats);
        assert_eq!(stats.wars_priced, 0, "a war that cannot resolve must not count as priced");
        assert_eq!(
            looked,
            eval::evaluate(&state, 0, &w, Some(&ctx), None),
            "an unresolvable war must be scored as it actually stands"
        );

        // Positive control: the SAME position with `last_round` off prices
        // DIFFERENTLY (mirroring `score_prices_a_declared_war_through_war_
        // value_not_as_pure_cost`'s own `assert_ne!` -- combat can cost the
        // victor real losses too, so "resolved" is not guaranteed to score
        // higher, only different), proving `war_value` really did fire here
        // and this position is not simply insensitive to the guard either
        // way.
        let mut not_last = state.clone();
        not_last.last_round = false;
        let mut stats2 = Stats::default();
        let with_lookahead = score(&not_last, 0, &w, &ctx, true, &mut stats2);
        assert_eq!(stats2.wars_priced, 1);
        assert_ne!(with_lookahead, looked, "war_value must actually fire once `last_round` is off");
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
