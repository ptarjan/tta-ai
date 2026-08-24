//! `engine/bots/weighted.py` lines 4104-end: `evaluate` (the linear
//! evaluation entry point), `WeightedBot` (the 1-ply bot built on it), and
//! the dominance guard (`dominance_repair` and its four rule tables).
//!
//! Everything `evaluate` needs is already ported: [`features::features`]
//! (the raw vector), [`cards`] (the identity-aware `hand_potential`/
//! `wonder_potential`/`hand_mil_potential`/`rival_hand_potential`/
//! `tactic_terms`), [`row`] (`row_pressure`/`row_last_copy`), [`events`]
//! (`my_event_threat`), [`horizon`] (`rate_multiplier`/`lateness`) and
//! [`rivals`] (`rival_context`). This module is arithmetic over those
//! answers, exactly as Python's `evaluate` is -- nothing here recomputes a
//! fact a sibling module already owns.
//!
//! ## `DEFAULT_WEIGHTS`/`BASE_WEIGHTS`/`_PHASE_PRIOR`/`PHASE_WEIGHTS`: no port
//!
//! Every number in Python's four module-scope weight tables (`weighted.py`
//! 3548-4140) already landed as [`weights::WeightKey::default_weight`] --
//! `weights.rs`'s own top doc comment explains why a `weight_key_table!`
//! macro invocation replaces the dict literals rather than restating them
//! here a second time. [`weights::Weights::defaults`] IS `DEFAULT_WEIGHTS`.
//!
//! ## Three things `WeightedBot.pick` does in Python that this port does not
//!
//! 1. **`_pick_journalled` / `USE_JOURNAL`.** No `journal.rs` exists in this
//!    port to switch to -- `bots/mod.rs`'s own module doc comment already
//!    closed this for `trial.py` generally: a `GameState` clone is this
//!    port's one search mechanism, so there is nothing to toggle between.
//! 2. **The per-candidate `try: ... except Exception: continue`.** Mirrors
//!    `quiescent.rs::pick_one`'s own documented choice (see that function's
//!    doc comment) and `bots/book.rs`'s "fail-loud" convention throughout
//!    this crate: [`crate::apply::apply`] panics on an invariant violation
//!    rather than being caught, so a candidate that would have raised in
//!    Python instead stops the program here, loudly, at the point of the
//!    actual bug -- never silently narrows the search. Because every
//!    candidate that survives always contributes a real score, `best` is
//!    provably `Some` by the end of [`WeightedBot::choose`]'s loop whenever
//!    `moves.len() > 1` (the only case that reaches the loop at all), which
//!    is what makes the next point safe.
//! 3. **`self.rng.choice(moves)` (the `best is None` fallback) and the
//!    constructor's `rng`/`seed`/`name` fields.** Point 2 above makes the
//!    fallback this `rng` field exists to serve unreachable in this port
//!    (it is ALREADY only a defensive net over a per-candidate exception in
//!    Python, per that function's own comment -- "the engine grows new move
//!    types... an unscorable candidate is skipped, never fatal"). A field
//!    nothing can ever read is exactly what `bots/book.rs`'s own top doc
//!    comment found for `BookBot.rng` and dropped for the identical reason
//!    ("this port carries no rng field at all: `BookBot` is a pure function
//!    of `(state, moves)`, which is the true behaviour, not a smaller one").
//!    `name` (a label read by nothing in this crate -- there is no bot
//!    registry/harness here yet) is dropped for the same "no unread field"
//!    reason `BookBot` already set the precedent for.
//!    [`WeightedBot::choose`] therefore collapses Python's `choose`/`pick`/
//!    `__call__` three-method harness-adapter split into one method, the same
//!    shape `BookBot::choose` already uses -- move GENERATION
//!    (`actions.legal_moves`/[`crate::legal::legal_moves`]) stays the
//!    caller's job, matching `book.rs`'s own doc comment on that split.
//!
//!    None of this is a behaviour change requiring a matching Python fix
//!    (the "fix things as you port" ruling): every reachable decision Python
//!    makes, this makes identically; only unreachable-in-a-complete-port
//!    defensive plumbing is not restated, and that omission was already
//!    decided, with its own citation trail, twice elsewhere in this exact
//!    crate before this module existed.
//!
//! ## `load_weights`/`save_weights`: ported, once the league needed them
//!
//! These were parked as "not ported" while nothing in this crate could ask
//! for a champion by name: they are pure JSON I/O (`weighted.py` 4519-4553)
//! whose only rule-level content is calling [`dominance_repair`] on the way
//! in, and `Cargo.toml`'s `[dependencies]` is deliberately empty. Both facts
//! still hold -- what changed is that the native trainer has to read the
//! champion vector the Python league produced and write the one it accepts,
//! so the functions now have a caller. No dependency was added for them:
//! [`crate::fixtures::parse_json`] is the reader the fixture loader already
//! carries, and [`save_weights`] emits its own text. The engine still parses
//! no JSON on any hot path -- a champion is read once at start-up.
//!
//! Two deliberate differences from Python, both in the "fix things as you
//! port" spirit rather than parity:
//!
//! * An unknown weight name is an ERROR, not a silently-carried extra dict
//!   entry. Python's `w.update(d["weights"])` accepts any key and `evaluate`
//!   then ignores it, so a typo in a hand-edited champion is invisible and
//!   costs you the weight you thought you had set. [`Weights`] has no room
//!   for an unrepresentable key, and the honest thing to do with one is say
//!   so. [`RETIRED_KEYS`] stay silently dropped -- every champion on disk
//!   predates their removal, so they are expected, not a mistake.
//! * [`save_weights`] writes every [`WeightKey`], not just the keys that
//!   happened to be in the file it loaded. A champion written here is a
//!   complete vector, which is what stops "the key was missing so it silently
//!   read as its default" from being a way to lose a trained value.

use crate::apply;
use crate::moves::Move;
use crate::state::GameState;

use super::super::plan;
use super::cards;
use super::events;
use super::features::{self, Features};
use super::horizon;
use super::rivals::{self, RivalContext};
use super::row;
use super::weights::{self, Weights, WeightKey, PHASE_KEYS, RETIRED_KEYS};

// ------------------------------------------------------------- evaluation

/// `evaluate(state, idx, weights, ctx=None, f=None)`.
///
/// `weights` is mandatory here, unlike Python's `weights=None` (which falls
/// back to `DEFAULT_WEIGHTS`): every real call site in this crate -- and
/// every one in Python too, `weighted.py`'s own grep shows -- already has a
/// concrete vector in hand ([`WeightedBot::choose`]'s own `&self.weights`,
/// exactly as `horizon::rate_multiplier`'s `n` parameter dropped Python's
/// `n=None` for the identical reason). `ctx`/`f`, by contrast, real callers
/// DO pass `None` for (a fresh root decision has no context yet; almost no
/// caller precomputes `f`), so both stay `Option`, matching
/// [`features::features`]'s own `ctx`/`w` parameters.
///
/// `ctx`, when `Some`, must be a [`RivalContext`] built for this exact `idx`
/// -- see [`features::features`]'s own doc comment; threaded through to
/// [`row::row_pressure`]/[`row::row_last_copy`] unchanged and otherwise only
/// read by the `f` recomputation below.
///
/// `f`, when `Some`, is used exactly as given -- this function does not
/// re-derive it. When `None`, it is computed with `priced_only = true`
/// (Python's own reasoning: `evaluate` multiplies every coordinate by its
/// weight and skips the zero ones, so a `priced_only` vector is
/// byte-identical to the full one for every coordinate this loop can ever
/// see a nonzero contribution from -- see [`features::features`]'s own doc
/// comment for why that is NOT true of an instrument reading the complete
/// vector, which must never set `priced_only`).
pub fn evaluate(state: &GameState, idx: u8, w: &Weights, ctx: Option<&RivalContext>, f: Option<&Features>) -> f64 {
    let computed_f;
    let f = match f {
        Some(f) => f,
        None => {
            computed_f = features::features(state, idx, ctx, Some(w), true);
            &computed_f
        }
    };

    let mut total = 0.0;
    // `rate_multiplier` is 1.0 unless the vector asks for the horizon; the
    // `hz != 1.0` guards below keep this loop byte-identical to the
    // pre-horizon evaluator when it is.
    let n = horizon::live_count(state);
    let hz = horizon::rate_multiplier(state, w, n);

    // The linear body: every `WeightKey` [`features::features`] can have
    // written, dotted against its weight. Iterating `WeightKey::ALL` rather
    // than Python's `f.items()` (a dict with only the ~60 keys `features()`
    // actually sets) is exactly equivalent: every key `features()` never
    // writes reads back `0.0` from [`Features::get`] (its documented zero
    // default), so its contribution here is `wk * 0.0 == 0.0` regardless of
    // `wk` -- the same nothing Python's dict simply never iterates over.
    //
    // `StrengthRel` is deliberately SKIPPED here (see the STRUCTURAL FIX
    // note on the phase-blended body below) -- it is priced entirely in
    // that loop instead of being both an always-on base term here AND a
    // phase-blended nudge there.
    for &k in WeightKey::ALL {
        // All four `PHASE_KEYS` are skipped here, for two different reasons
        // that both land on "don't double-count":
        //   * `Workers`/`TechLevels`/`HandValue` (T1-A/C/D, PHASECUT.txt,
        //     2026-08-13) no longer carry a separate always-on base term --
        //     `k` itself now IS the early-extreme ("start") coefficient of
        //     the collapsed 2-parameter blend, added below (scaled by
        //     `1 - lateness`) instead of unconditionally here.
        //   * `StrengthRel` keeps its OLD 3-parameter shape (a parallel fix,
        //     commit 578ee9e "earlymil", further round-gates its blend --
        //     see the STRUCTURAL FIX note below) and is priced entirely in
        //     the dedicated block below (its own base term included there),
        //     not here, so it does not end up added twice either.
        if PHASE_KEYS.contains(&k) {
            continue;
        }
        let wk = w.get(k);
        if wk == 0.0 {
            continue;
        }
        let v = f.get(k);
        let scale = if hz != 1.0 && horizon::RATE_KEYS.contains(&k) { hz } else { 1.0 };
        total += wk * v * scale;
    }

    // The phase-blended body, for the four [`PHASE_KEYS`]. Two different
    // formulas live here now, one per group:
    //
    //   * `Workers`/`TechLevels`/`HandValue` (T1-A/C/D collapse, PHASECUT.txt,
    //     2026-08-13): the OLD 3-parameter `w[k] + (1-L)*w[k_early] +
    //     L*w[k_late]` had only 2 real degrees of freedom for 3 raw numbers
    //     -- a proven, exact, data-independent dead direction (moving
    //     `(base,early,late) += t*(1,-1,-1)` changed nothing this ever
    //     computed, at any lateness, on any board). Collapsed to the
    //     equivalent, non-redundant `start*(1-L) + end*L`, where
    //     `start = w.get(k)` (the base key, repurposed to hold the L=0
    //     value) and `end = w.get(k.late())` (repurposed to hold the L=1
    //     value). The phase pair carries the same rate horizon as the base
    //     term -- see [`super::rivals::feature_marginal`], which sums
    //     exactly these three for a card pricer.
    //
    //   * `StrengthRel` -- deliberately EXCLUDED from the collapse above
    //     (PHASECUT.txt's scope note: a parallel fix, STRUCTURAL FIX below,
    //     makes this ONE triple genuinely identifiable via a round-gated
    //     blend, so collapsing it would delete a distinction that fix
    //     depends on) and given its OWN dedicated block right below instead
    //     of running through the generic loop above at all -- unlike the
    //     collapsed three, its base term is no longer added in the generic
    //     `WeightKey::ALL` loop (see that loop's own `PHASE_KEYS.contains`
    //     skip), because the STRUCTURAL FIX changes what the base even
    //     means depending on phase (see immediately below), which the
    //     generic loop's unconditional `wk * v * scale` cannot express.
    //
    // STRUCTURAL FIX (earlymil, 2026-08-13): for the other three PHASE_KEYS
    // an always-on base plus a small phase nudge is correct -- more
    // workers/tech/hand is a genuine every-phase preference, and the phase
    // pair only adjusts its size. For `StrengthRel` it is not:
    // `strength_marginal`'s own arithmetic (mirrored here) showed the BASE
    // term (+17.23 in the frozen 2p champion) applies in full at
    // `lateness == 0`, the literal opening, before any war is remotely
    // relevant -- the tiny `strength_rel_early` nudge (-0.19 in that same
    // champion) can never bring that down anywhere close to zero, because
    // it is only ever added ON TOP of the base, never scaling it. A real
    // round-2 position (`bin/dumpweights.rs`, EARLYMIL.txt step 1) confirmed
    // this dominates the champion's decision to build a military unit
    // before a mine. Confirmed empirically NOT fixable by moving the base
    // weight alone either: zeroing `strength_rel` collapses the opening
    // habit (96.7% -> ~0% military-first over 300 self-play games) but also
    // collapses genuine late-game strength value the SAME base term
    // carries, and loses to the unmodified champion at 7.4% (n=600) -- the
    // base is entangled across BOTH phases through one shared coefficient,
    // which is exactly why 42,000 hill-climb generations never found this:
    // any single-coordinate step that helps the opening also hurts the
    // endgame, so the climb's own win-rate-vs-champion test rejects it
    // before it can travel far enough to help.
    //
    // The fix: stop treating the base as an unconditional, phase-blind
    // term. `strength_rel_early` becomes the WHOLE early-game coefficient
    // (not a delta on the base), and the base folds into the LATE endpoint
    // instead, alongside `strength_rel_late` -- `eff(L) = (1-L) *
    // strength_rel_early + L * (strength_rel + strength_rel_late)`. At
    // `L == 1` this is byte-identical to the old formula (late-game value,
    // and every game already played near the endgame, is untouched). At
    // `L == 0` it is exactly `strength_rel_early` alone -- already small
    // (-0.19) in the trained champion, with no retuning needed for this
    // structural change to take effect. [`linear_features`] mirrors this
    // exactly (`out[StrengthRel] = late * v` instead of the generic `v`),
    // which is what keeps `linear_features_dotted_with_a_weight_vector_
    // reproduces_evaluate_exactly` green. This whole block is additionally
    // gated on [`horizon::combat_unreachable`] rather than blended
    // continuously off lateness -- see the `if`/`else` immediately below for
    // why (STRGATE.txt, superseding the round<=3 literal this comment used
    // to describe before that fix landed).
    let late = horizon::lateness(state);
    let early = 1.0 - late;
    {
        let v = f.get(WeightKey::StrengthRel);
        if v != 0.0 {
            // GATED to [`horizon::combat_unreachable`] -- true while no
            // player can possibly hold an aggression or war card yet (see
            // that function's doc comment for the RULES_SPEC derivation) --
            // rather than blended continuously off `lateness()` for the
            // WHOLE game. An earlier version of this fix used the
            // continuous blend below unconditionally and measured
            // DECISIVELY WORSE than the pre-fix champion (27.7% at n=3000,
            // EARLYMIL.txt) -- diagnosed, not shrugged off: `lateness()`
            // rises gradually across the WHOLE game, so an unconditional
            // blend suppresses `StrengthRel`'s value through roughly the
            // first half of every game, not merely the measured defect's
            // actual window -- e.g. at `lateness == 0.5` (a real mid-game
            // moment, not the opening) the unconditional blend priced this
            // at ~11.3 against the old formula's ~19.9, undermining
            // legitimate mid-game military value the MineFirst-forcing
            // experiment never touched (it restricted only the FIRST
            // build, then handed control straight back to the UNMODIFIED
            // evaluator for the rest of the game). `combat_unreachable`
            // reproduces that scope from an actual rules fact instead of a
            // fitted round number (STRGATE.txt, superseding EARLYMIL.txt's
            // `state.round <= 3`, itself chosen by measuring win rates --
            // a fitted parameter that does not belong in engine code):
            // outside it, `StrengthRel` prices EXACTLY as it did before
            // this fix (see the `else` arm), so nothing changes once any
            // player could plausibly hold a combat-capable card.
            if horizon::combat_unreachable(state) {
                let early_full = w.get(WeightKey::StrengthRel.early());
                let late_full = w.get(WeightKey::StrengthRel) + w.get(WeightKey::StrengthRel.late());
                total += early * early_full * v + late * late_full * v;
            } else {
                total += w.get(WeightKey::StrengthRel) * v;
                total += early * w.get(WeightKey::StrengthRel.early()) * v + late * w.get(WeightKey::StrengthRel.late()) * v;
            }
        }
    }
    for &k in PHASE_KEYS {
        if k == WeightKey::StrengthRel {
            continue;
        }
        let v = f.get(k);
        if v == 0.0 {
            continue;
        }
        let scale = if hz != 1.0 && horizon::RATE_KEYS.contains(&k) { hz } else { 1.0 };
        let vv = v * scale;
        if matches!(k, WeightKey::Workers | WeightKey::TechLevels | WeightKey::HandValue) {
            let start = w.get(k);
            if start != 0.0 {
                total += start * early * vv;
            }
            let end = w.get(k.late());
            if end != 0.0 {
                total += end * late * vv;
            }
        } else {
            let we = w.get(k.early());
            if we != 0.0 {
                total += we * early * vv;
            }
            let wl = w.get(k.late());
            if wl != 0.0 {
                total += wl * late * vv;
            }
        }
    }

    // Identity-aware hand term: what the cards actually in hand would be
    // worth if played. Deliberately NOT folded into `features()` -- priced
    // through `w` itself, so it is not a linear feature and must not pick up
    // the phase multipliers above.
    let hp = w.get(WeightKey::HandPotential);
    if hp != 0.0 {
        total += hp * cards::hand_potential(state, idx, w);
    }
    // Which wonder am I building (and, through the post-move state, which
    // one am I taking).
    let wp = w.get(WeightKey::WonderPotential);
    if wp != 0.0 {
        total += wp * cards::wonder_potential(state, idx, w);
    }
    // ... and the half of that same wonder's value still ahead of me, which is
    // the ONLY term that is nonzero on the move that takes it (see
    // `cards::wonder_promise`). Gated at 0.0 like every other block here, so a
    // champion trained before it existed evaluates exactly as it did.
    let wpr = w.get(WeightKey::WonderPromise);
    if wpr != 0.0 {
        total += wpr * cards::wonder_promise(state, idx, w);
    }
    // The row / rival-hand terms, same shape and reason as `hand_potential`
    // above. Each is skipped entirely at scale 0.0 (the default), so a
    // champion trained before it existed evaluates exactly as it did.
    let hmp = w.get(WeightKey::HandMilPotential);
    if hmp != 0.0 {
        total += hmp * cards::hand_mil_potential(state, idx, w);
    }
    // The tactic deadlock terms. Linear features, unlike the ones above, but
    // gated here for the same reason: both weights default to 0.0.
    let tg = w.get(WeightKey::TacticGain);
    let ts = w.get(WeightKey::TacticShort);
    if tg != 0.0 || ts != 0.0 {
        let (gain, short) = cards::tactic_terms(state, idx);
        if tg != 0.0 {
            total += tg * gain;
        }
        if ts != 0.0 {
            total += ts * short;
        }
    }
    let rhp = w.get(WeightKey::RivalHandPotential);
    if rhp != 0.0 {
        total += rhp * cards::rival_hand_potential(state, idx, w);
    }
    let ru = w.get(WeightKey::RowUrgency);
    let rb = w.get(WeightKey::RowBargainForgone);
    if ru != 0.0 || rb != 0.0 {
        let (urgency, bargain) = row::row_pressure(state, idx, w, ctx);
        if ru != 0.0 {
            total += ru * urgency;
        }
        if rb != 0.0 {
            total += rb * bargain;
        }
    }
    // Card counting, priced through `w` like everything else in this block
    // and therefore eval-only.
    let rlc = w.get(WeightKey::RowLastCopy);
    if rlc != 0.0 {
        total += rlc * row::row_last_copy(state, idx, w, ctx);
    }
    // The events I planted myself, priced through `w` for the same reason
    // `hand_potential` is. Skipped entirely at scale 0.0, the default.
    let met = w.get(WeightKey::MyEventThreat);
    if met != 0.0 {
        total += met * events::my_event_threat(state, idx, w);
    }
    total
}

// ------------------------------------------------------------------- bot

/// 1-ply search under a fully parameterized linear evaluation. See this
/// module's top doc comment for the three Python-only mechanisms this struct
/// does not carry (`rng`/`seed`/`name` fields, the journalled search path,
/// the per-candidate exception guard) and why each omission is safe.
#[derive(Clone, Copy, Debug, PartialEq)]
#[derive(Default)]
pub struct WeightedBot {
    pub weights: Weights,
    /// `("resign",)` (RULES_SPEC 5.11) is legal on almost every turn, and in
    /// a 2p game it is never right -- it hands the win to the opponent
    /// immediately. A value vector fitted by regression has been measured to
    /// resign on turn 3 of 3 games in 12 (`docs/BOT_ARCHITECTURE.md` section
    /// 3b), silently contaminating an n=400 duel with games that ended at
    /// round 2 scored `[0, 0]`. `false` (the default) filters `Move::Resign`
    /// out of the candidate set whenever a non-resign move is legal too --
    /// see [`WeightedBot::choose`].
    pub allow_resign: bool,
}


impl WeightedBot {
    pub fn new(weights: Weights) -> WeightedBot {
        WeightedBot { weights, allow_resign: false }
    }

    /// Best move for `state.decider()` among `moves`, by 1-ply search: apply
    /// each candidate to a clone, score it with [`evaluate`], and take the
    /// argmax (first candidate wins a tie -- `is_none_or` only replaces on
    /// strict `>`, mirroring Python's `val > best_val`).
    ///
    /// `moves` should be exactly what [`crate::legal::legal_moves`] returns
    /// for `state` -- callers generate the move list themselves, the same
    /// split `bots::book`/`bots::quiescent` use throughout this port (move
    /// GENERATION lives in `legal.rs`/`interact.rs`; this method only
    /// SELECTS).
    ///
    /// Scores for whoever actually owns the move: on a pending decision that
    /// is NOT the turn player -- pact accept/refuse is always one of these --
    /// `state.decider()` (not `state.actor()`) is who this maximises for,
    /// exactly as `evaluate`'s own `idx` parameter requires
    /// (`docs/AUDIT_HISTORY.md`).
    ///
    /// # Panics
    /// If `moves` is empty (a caller bug -- a live game's `legal_moves` never
    /// returns one, matching `BookBot::choose`'s identical contract).
    pub fn choose(&self, state: &GameState, moves: &[Move]) -> Move {
        let filtered = super::super::filter_resign(moves, self.allow_resign);
        let moves: &[Move] = filtered.as_slice();
        if moves.len() == 1 {
            return moves[0];
        }

        let idx = state.decider();
        // Computed once at the root and reused for every candidate -- see
        // `rivals::rival_context`'s own doc comment on why that reuse
        // matters (an information leak, not just an optimisation, if
        // recomputed per candidate).
        let ctx = rivals::rival_context(state, idx, None, None);
        let w = &self.weights;
        let end_bias = w.get(WeightKey::EndTurnBias);

        // A 1-ply trial applies exactly one candidate move and scores it
        // immediately, with no later ply for a genuinely random look to
        // average out over -- so if `mv` is `Move::PrepareEvent`, `apply`
        // reveal-and-resolves the TRUE top card of `state.current_events`
        // unconditionally, and this bot would be scoring a card it has not
        // legally seen. Re-shuffle just that pile (preserving a genuinely
        // peeked top card -- Joan of Arc) once here, on a shared root every
        // candidate below clones from, rather than calling the full
        // `plan::determinize`: see `plan::determinize_current_events`'s own
        // doc comment for why `civil_deck`/`military_deck` are deliberately
        // left out of this bot's determinization. `plan::plan_rng` derives
        // the stream from `state` alone (no caller-owned rng field to add to
        // this struct -- this module's own top doc comment already retired
        // `self.rng` for being unread; a determinize-only stream would just
        // be a new unread-outside-this-call field with extra steps).
        let mut root = state.clone();
        plan::determinize_current_events(&mut root, &mut plan::plan_rng(state, idx));

        let mut best: Option<(Move, f64)> = None;
        for &mv in moves {
            let mut trial = root.clone();
            // Every candidate is scored at the SAME point in time -- mid-turn,
            // before this turn's production. `Move::EndTurn` is the only move
            // whose apply arm reaches `economy::end_of_turn` (`apply.rs` ->
            // `game::end_turn` -> `resume_end_turn`), so applying it here would
            // score it against a board that already holds this turn's food and
            // resources while every rival candidate is still charged the full
            // negative `food_gap`/`resource_gap`. Playing a card and THEN
            // ending the turn collects the identical production, but one ply
            // cannot represent that pair, so the comparison would price doing
            // nothing as if it produced. `EndTurn` is therefore scored on the
            // unmoved root, and `end_bias` below is what prices ending a turn.
            if !matches!(mv, Move::EndTurn) {
                apply::apply(&mut trial, mv);
            }
            let mut val = evaluate(&trial, idx, w, Some(&ctx), None);
            if matches!(mv, Move::EndTurn) {
                val += end_bias;
            }
            if best.is_none_or(|(_, bv)| val > bv) {
                best = Some((mv, val));
            }
        }
        // Unreachable given `moves.len() > 1` and a complete `apply`/
        // `evaluate` (see this module's top doc comment, point 2) -- kept
        // for the same defensive-fallback reason `book.rs`/`quiescent.rs`
        // keep their own `unwrap_or(moves[0])`.
        best.map(|(m, _)| m).unwrap_or(moves[0])
    }

    /// Like [`WeightedBot::choose`], but returns every candidate's score
    /// instead of only the argmax -- for a move-agreement analysis
    /// (`docs/REPLAY.md`, `bin/agreement.rs`) that needs the bot's full
    /// preference order over the same legal-move list a human faced, not
    /// just its top pick. Shares every step of `choose`'s own
    /// trial-and-evaluate loop (same `filter_resign`, same shared
    /// `rival_context`/`determinize_current_events` root, same
    /// `end_turn_bias` asymmetry) so the two can never silently diverge on
    /// what "the bot's opinion" means -- deliberately NOT applying `choose`'s
    /// own `moves.len() == 1` short-circuit, so a single-candidate call still
    /// returns a real (if moot) score rather than a placeholder.
    ///
    /// Returned in descending score order; ties keep the ORIGINAL `moves`
    /// ordering (`sort_by` is a stable sort over a vector built in `moves`'
    /// own order), matching `choose`'s own first-candidate-wins tie-break --
    /// so `ranked[0].0 == self.choose(state, moves)` whenever `moves` is
    /// non-empty.
    ///
    /// Returns an empty `Vec` for an empty (post-`filter_resign`) candidate
    /// list, rather than `choose`'s own out-of-bounds panic on that input --
    /// a caller passing a genuine `legal_moves()` output never hits this,
    /// since a live game's legal-move list is never empty.
    pub fn rank_moves(&self, state: &GameState, moves: &[Move]) -> Vec<(Move, f64)> {
        let filtered = super::super::filter_resign(moves, self.allow_resign);
        let moves: &[Move] = filtered.as_slice();

        let idx = state.decider();
        let ctx = rivals::rival_context(state, idx, None, None);
        let w = &self.weights;
        let end_bias = w.get(WeightKey::EndTurnBias);

        let mut root = state.clone();
        plan::determinize_current_events(&mut root, &mut plan::plan_rng(state, idx));

        let mut scored: Vec<(Move, f64)> = Vec::with_capacity(moves.len());
        for &mv in moves {
            let mut trial = root.clone();
            apply::apply(&mut trial, mv);
            let mut val = evaluate(&trial, idx, w, Some(&ctx), None);
            if matches!(mv, Move::EndTurn) {
                val += end_bias;
            }
            scored.push((mv, val));
        }
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored
    }
}

// --------------------------------------------------- agreement-fit features
//
// `bin/agreefit.rs` (the champion-weight-vs-human-choice supervised fit,
// `docs/AGREEMENT_FIT.md`) needs, for every legal move at a human decision
// point, the exact per-`WeightKey` vector `evaluate` above dots against `w`
// -- NOT a second, drifting reimplementation of `evaluate`'s arithmetic, and
// NOT `evaluate`'s own scalar output (which only tells you the score under
// ONE fixed `w`, not what to fit). [`linear_features`]/[`candidate_features`]
// below share every trial-and-evaluate step [`WeightedBot::rank_moves`]
// itself uses (same `filter_resign`, same root/`RivalContext`/
// `determinize_current_events` construction, same `end_turn_bias` handling)
// and read the SAME [`features::features`]/[`cards`]/[`row`]/[`events`]
// calls [`evaluate`] itself calls -- so a champion `w` dotted against this
// vector reproduces [`evaluate`]'s own number exactly (pinned by
// [`tests::linear_features_dotted_with_a_weight_vector_reproduces_evaluate_exactly`]),
// and a *different* `w` dotted against it is a faithful LINEAR
// approximation of what `evaluate` would have scored under that `w` --
// faithful for every coordinate `evaluate`'s own `WeightKey::ALL`/
// `PHASE_KEYS` loops price, which is most of the vector.
//
// One documented, deliberate gap, not hidden: eleven coordinates
// ([`WeightKey::HandPotential`], [`WeightKey::WonderPotential`],
// [`WeightKey::WonderPromise`], [`WeightKey::
// HandMilPotential`], [`WeightKey::RivalHandPotential`], [`WeightKey::
// RowUrgency`], [`WeightKey::RowBargainForgone`], [`WeightKey::RowLastCopy`],
// [`WeightKey::MyEventThreat`], plus [`WeightKey::RateHorizon`]'s own scaling
// of the four [`horizon::RATE_KEYS`]) are NOT linear in `w` in [`evaluate`]
// itself -- each is priced by calling a function that takes the FULL weight
// vector and reprices its own internal sub-terms through it (`cards::
// hand_potential(state, idx, w)` and siblings), so the true `evaluate(state,
// w)` is bilinear in `w` on these eleven dimensions, not expressible as
// `w . f(state)` for any single fixed `f`. [`linear_features`] resolves this
// by freezing those eleven sub-computations at a caller-supplied `freeze`
// vector (the champion, in every real call site) rather than the `w` being
// fit -- the OUTER gate weight (e.g. `hand_potential`'s own coordinate) is
// still fully free and fit, only the INNER sub-pricing inside it is held at
// the champion's numbers. `WeightKey::TacticGain`/`WeightKey::TacticShort`
// are the one exception among the "identity-aware" group that needed no
// freezing: `cards::tactic_terms(state, idx)` -- see [`evaluate`]'s own call
// above -- takes no `w` at all, so both are genuinely `w`-independent
// features already.

/// The raw linear feature vector, one entry per [`WeightKey`], such that
/// `sum_k w.get(k) * out[k as usize]` equals [`evaluate`]'s own total
/// whenever `w == freeze` -- see this section's own doc comment for exactly
/// which eleven coordinates are only equal in that one case (frozen at
/// `freeze`, not recomputed for a different `w`) and why.
///
/// `ctx` must be the SAME [`RivalContext`] `evaluate`'s own caller built for
/// `idx` at the search root -- not recomputed per candidate, exactly as
/// [`WeightedBot::choose`]/[`rank_moves`] themselves require (see their own
/// doc comments).
pub fn linear_features(state: &GameState, idx: u8, ctx: Option<&RivalContext>, freeze: &Weights) -> Vec<f64> {
    let mut out = vec![0.0f64; WeightKey::ALL.len()];

    // The board-read vector `evaluate`'s own `WeightKey::ALL` loop dots
    // against `w` -- `w: None, priced_only: false` so every coordinate is
    // computed regardless of which weight the CHAMPION happens to price at
    // zero (a coordinate the champion never turned on may still be exactly
    // what a fit needs to turn on -- see `features::features`'s own doc
    // comment on why `priced_only` must stay off for a complete read).
    let f = features::features(state, idx, ctx, None, false);
    let hz = horizon::rate_multiplier(state, freeze, horizon::live_count(state));
    for &k in WeightKey::ALL {
        let mut v = f.get(k);
        if v != 0.0 && horizon::RATE_KEYS.contains(&k) {
            v *= hz;
        }
        out[k as usize] = v;
    }

    // The phase-blended body -- `PHASE_KEYS` members are never in
    // `RATE_KEYS` (pinned by `horizon.rs`'s own set, `evaluate`'s `scale`
    // guard is therefore always 1.0 here, matching `evaluate` exactly), so
    // this is genuinely state-only, no `freeze` dependence at all.
    //
    // T1-A/C/D collapse (PHASECUT.txt, 2026-08-13): for `Workers`/
    // `TechLevels`/`HandValue`, `out[k as usize]` was just set to the raw
    // feature value `v` by the generic loop above -- overwritten here to
    // `v * early`, so that `w.get(k) * out[k]` reproduces `start * early *
    // v` (matching `evaluate`'s own new formula) instead of the old
    // `base * v`.
    //
    // STRUCTURAL FIX (earlymil, 2026-08-13): `StrengthRel` is excluded from
    // the collapse above (see PHASECUT.txt) -- `evaluate`'s own matching
    // comment explains why it alone gets a different blend -- `eff(L) =
    // (1-L) * strength_rel_early + L * (strength_rel + strength_rel_late)`
    // instead of the other three PHASE_KEYS' `start*(1-L) + end*L`.
    // `out[StrengthRel]` (written `v` by the generic loop above) is
    // overridden to `late * v` here so that `sum_k w.get(k) * out[k]` over
    // the three StrengthRel-family slots reproduces exactly `evaluate`'s new
    // formula: `w[early] * (early * v) + w[base] * (late * v) + w[late] *
    // (late * v)` == `early * w[early] * v + late * (w[base] + w[late]) *
    // v`, byte-identical to `evaluate`'s own arithmetic above.
    let late = horizon::lateness(state);
    let early = 1.0 - late;
    // Gated identically to `evaluate`'s own [`horizon::combat_unreachable`]
    // check (see that function's doc comment for why the earlier, ungated
    // version of this fix measured decisively worse) -- once combat is
    // reachable, `out[StrengthRel]` stays the generic `v` the loop above
    // already wrote, reproducing the untouched pre-fix formula exactly.
    if horizon::combat_unreachable(state) {
        out[WeightKey::StrengthRel as usize] *= late;
    }
    for &k in PHASE_KEYS {
        let v = f.get(k);
        if matches!(k, WeightKey::Workers | WeightKey::TechLevels | WeightKey::HandValue) {
            out[k as usize] = v * early;
            out[k.late() as usize] = v * late;
        } else {
            out[k.early() as usize] = v * early;
            out[k.late() as usize] = v * late;
        }
    }

    // The eleven identity-aware, `freeze`-priced gates -- see this section's
    // top doc comment.
    out[WeightKey::HandPotential as usize] = cards::hand_potential(state, idx, freeze);
    out[WeightKey::WonderPotential as usize] = cards::wonder_potential(state, idx, freeze);
    out[WeightKey::WonderPromise as usize] = cards::wonder_promise(state, idx, freeze);
    out[WeightKey::HandMilPotential as usize] = cards::hand_mil_potential(state, idx, freeze);
    let (tactic_gain, tactic_short) = cards::tactic_terms(state, idx);
    out[WeightKey::TacticGain as usize] = tactic_gain;
    out[WeightKey::TacticShort as usize] = tactic_short;
    out[WeightKey::RivalHandPotential as usize] = cards::rival_hand_potential(state, idx, freeze);
    let (row_urgency, row_bargain) = row::row_pressure(state, idx, freeze, ctx);
    out[WeightKey::RowUrgency as usize] = row_urgency;
    out[WeightKey::RowBargainForgone as usize] = row_bargain;
    out[WeightKey::RowLastCopy as usize] = row::row_last_copy(state, idx, freeze, ctx);
    out[WeightKey::MyEventThreat as usize] = events::my_event_threat(state, idx, freeze);

    out
}

/// [`linear_features`] for every candidate in `moves`, sharing exactly the
/// root-construction and per-candidate trial loop [`WeightedBot::
/// rank_moves`] uses (same `filter_resign`, same shared root/`ctx`, same
/// `end_turn_bias` indicator folded into the vector at [`WeightKey::
/// EndTurnBias`]'s own slot rather than added back on afterward, so a
/// caller's plain `w . f` already includes it) -- this is the ONE place a
/// caller outside this module should ever build these vectors from, so
/// `bin/agreefit.rs` never re-derives the root/ctx/trial machinery itself
/// (this module's own top doc comment, point 2's "one shared function"
/// requirement).
///
/// Returns `(move, features)` pairs in `moves`' own order (post-
/// `filter_resign`), NOT sorted -- there is no score to sort by until a
/// caller dots a `w` against each entry.
pub fn candidate_features(
    state: &GameState,
    moves: &[Move],
    allow_resign: bool,
    freeze: &Weights,
) -> Vec<(Move, Vec<f64>)> {
    let filtered = super::super::filter_resign(moves, allow_resign);
    let moves: &[Move] = filtered.as_slice();

    let idx = state.decider();
    let ctx = rivals::rival_context(state, idx, None, None);
    let mut root = state.clone();
    plan::determinize_current_events(&mut root, &mut plan::plan_rng(state, idx));

    let mut out = Vec::with_capacity(moves.len());
    for &mv in moves {
        let mut trial = root.clone();
        apply::apply(&mut trial, mv);
        let mut f = linear_features(&trial, idx, Some(&ctx), freeze);
        if matches!(mv, Move::EndTurn) {
            f[WeightKey::EndTurnBias as usize] += 1.0;
        }
        out.push((mv, f));
    }
    out
}

/// `w . f` over the full [`WeightKey`] space -- the linear score a fitted
/// (or champion, or zero) weight vector assigns one [`linear_features`]/
/// [`candidate_features`] output. Not a method on [`Weights`] itself: this
/// vocabulary (a plain `&[f64]` aligned to `WeightKey as usize`) belongs to
/// the agreement-fit experiment, not to the champion-facing `Weights` API.
pub fn dot(w: &Weights, f: &[f64]) -> f64 {
    let mut total = 0.0;
    for (i, &k) in WeightKey::ALL.iter().enumerate() {
        total += w.get(k) * f[i];
    }
    total
}

// -------------------------------------------------------- dominance guard
//
// THE HOLE THIS CLOSES, and which side gets repaired and why: see
// Python's own extensive comment on this section (`weighted.py` 4366-4477),
// not reproduced here. Short version: `hillclimb_league.guard_weights`
// catches a value term whose SIGN is inverted, but exempts the phase
// multipliers and never checked a term's NET weight, and never checked two
// terms against each other -- both holes were open and the league walked
// through both (a champion that rated losing 3 culture as a +0.55 gain; a
// champion that rated being plundered of 4 resources as a +1.27 gain).
// `dominance_repair` closes both, plus a third (a printed per-card benefit
// priced negatively), with the repair applied by RAISING the dominated side
// (or clamping to the boundary), never by lowering what the league already
// measured.

/// Terms whose net weight (`k` plus either phase multiplier) may not go
/// negative, because a pure gain of them cannot hurt under the rules. Empty
/// from 2026-08-04 until 2026-08-13, when [`WeightKey::HandValue`] joined it
/// (SIGNAUDIT.txt) -- and empty again from 2026-08-13 onward, for a
/// DIFFERENT reason (PHASECUT.txt, T1-D): `HandValue`'s three-parameter
/// `{base, early, late}` shape (the shape this composite mechanism existed
/// to police -- "the base alone can be negative as long as the phase
/// partner offsets it") was collapsed to a non-redundant two-parameter
/// `{start, end}` basis, `start*(1-L) + end*L`. That is a CONVEX
/// combination of the two endpoints, so the net is `>= 0` at every lateness
/// in `[0,1]` if and only if BOTH endpoints are `>= 0` individually --
/// exactly what a plain per-key `SignIntent::NonNegative` gate on each
/// already enforces (see `weights::WeightKey::sign_intent`'s `HandValue`/
/// `HandValueLate` arm). The composite constraint this list existed for has
/// nothing left to compose: two independent per-key gates are strictly
/// simpler and not weaker, so this stays a live mechanism (kept, not
/// deleted, in case a FUTURE key needs a genuine cross-key/cross-phase
/// composite the plain per-key gates cannot express) but currently has no
/// members.
///
/// (`culture`/`wonder_progress`, empty here from 2026-08-04 to 2026-08-13,
/// had their phase pair deleted outright instead -- [`PHASE_KEYS`] no
/// longer lists either -- a different reason for the same emptiness; see
/// git history / SIGNAUDIT.txt for that citation trail.)
///
/// Python's own test of the formerly-empty branch drove it with
/// `mock.patch.object(weighted, "NET_NONNEG_PHASE", ("culture",))` -- there
/// is no monkeypatching a `const` in Rust, so that specific test has no
/// direct port here.
pub const NET_NONNEG_PHASE: &[WeightKey] = &[];

/// The per-type "board credit" keys `cards::card_potential`'s generic
/// swap-pricing path can ADD to [`WeightKey::CardBoardCredit`] before scaling
/// a computed board-swap diff (`cards.rs`'s `credit_board = base +
/// board_credit_key(id).map_or(0.0, |k| w.get(k))`) -- today
/// `CardBoardLeader`/`CardBoardBonus`, one per [`cards::board_credit_key`]
/// match arm that returns `Some`. `CardBoardGovernment`/`CardBoardAction`/
/// `CardBoardWonder` used to be three more such arms; all three were
/// RETIRED 2026-08-13 (SIGNAUDIT.txt) for being permanently shadowed by a
/// dedicated board-aware pricing function that intercepts first -- see
/// `weights::RETIRED_KEYS`'s own entry for the full account. Because this
/// list is DERIVED from `board_credit_key` rather than hand-copied (next
/// paragraph), that retirement needed no edit here at all: the function
/// simply stopped returning `Some` for those three `CardType`s and this
/// list shrank from five entries to two automatically.
///
/// Deliberately NOT hand-copied: this calls [`cards::board_credit_key`] over
/// every real card in [`crate::card_table::CARDS`] and collects the distinct
/// `Some` results, so a card type that function starts answering later is
/// picked up automatically by [`dominance_repair`]'s gate below instead of
/// landing outside it the way a hand-retyped list could silently drift --
/// exactly the failure shape `board_credit_key`'s own doc comment names for
/// itself (a card category present in the data but silently absent from a
/// hand-rolled registry), one level up.
///
/// Cached behind a `OnceLock`: [`dominance_repair`] runs on every hillclimb
/// mutation (`bin/climb.rs`'s `mutate` wrapper), not only at load, so this
/// must not re-walk a few hundred cards on every single call.
pub fn card_board_credit_keys() -> &'static [WeightKey] {
    static KEYS: std::sync::OnceLock<Vec<WeightKey>> = std::sync::OnceLock::new();
    KEYS.get_or_init(|| {
        let mut keys: Vec<WeightKey> = crate::card_table::CARDS
            .iter()
            .filter_map(|c| {
                cards::board_credit_key(crate::cards::CardId::by_name(c.name).expect("every card_table entry resolves by its own name"))
            })
            .collect();
        keys.sort_by_key(|k| k.name());
        keys.dedup();
        keys
    })
}

/// `(dominant, dominated)` -- `w[dominant] >= w[dominated]`, repaired by
/// raising the dominant side. A resource in stock dominates the blue token it
/// sits on: spending the resource hands the token back to the bank AND buys
/// what it paid for, so a stocked resource is worth at least a free token
/// whatever either is worth.
///
/// `wonder_potential >= wonder_promise` is the second entry, and it is what
/// keeps PAYING a wonder stage rewarded. The two coordinates split a wonder's
/// printed value by how much of it is paid for: `wonder_promise` scales
/// `(1 - paid_fraction) * feasible`, `wonder_potential` scales
/// `paid_fraction * collect_fraction`, so buying a stage moves value out of
/// the first and into the second. If the promise side were priced HIGHER,
/// that transfer would be a net loss and the evaluator would take wonders it
/// then refuses to build -- the exact pathology `horizon::WonderOutlook::
/// paid_fraction`'s own doc comment says booking the whole value up front
/// causes. Repaired by raising `wonder_potential`, never by lowering the
/// promise: same direction as the entry above.
pub const DOMINATES: &[(WeightKey, WeightKey)] = &[
    (WeightKey::ResourceStock, WeightKey::BlueFree),
    (WeightKey::WonderPotential, WeightKey::WonderPromise),
];

/// THE STRUCTURAL FIX (SIGNAUDIT.txt): every simple per-key sign gate used to
/// live here as EIGHT separate hand-typed `&[WeightKey]` lists (`BENEFIT_
/// GATES`, `SHORTFALL_GATES`, `LOSS_GATES`, `REDUNDANCY_NONNEG_GATES`,
/// `STOCK_NONNEG_GATES`, `WONDER_DEBT_GATES`, `PERISHABLE_GATES`,
/// `WONDER_VALUE_GATES`), fed into two wrapper tables (`NON_POSITIVE_GATES`/
/// `NON_NEGATIVE_GATES`) `dominance_repair` and `bin/climb.rs`'s
/// under-mutation guard both iterated. That is exactly the shape that let
/// `card_board_leader` sit unconstrained for months: a hand-typed list is a
/// classification a NEW `WeightKey` variant can silently miss, with nothing
/// failing until someone measures the damage.
///
/// All eight lists, and the two wrapper tables, are now DERIVED from
/// [`WeightKey::sign_intent`]'s own exhaustive match (`weights.rs`) instead
/// -- [`non_positive_gates`]/[`non_negative_gates`] below simply filter
/// [`WeightKey::ALL`] by that classification. A key's direction and its
/// diagnostic "why" text both live on the `SignIntent` value itself (see
/// that enum's own doc comment), so there is exactly ONE place left to edit
/// when a key's sign intent changes, and adding a 163rd `WeightKey` variant
/// without extending `sign_intent`'s match is a compile error, not a silent
/// gap in one of these two functions.
///
/// [`bin/climb.rs`]'s under-mutation guard calls these same two functions
/// (not a copy), so a key reclassified in `sign_intent` is armed at both
/// load time (`dominance_repair`, called from [`parse_weights`]) and
/// mutation time automatically, exactly as the former table-driven design
/// promised -- confirmed by `bin/climb.rs`'s own guard tests, which drive
/// the mutator hard from a deliberately illegal vector built from these two
/// functions and assert every mutant comes back legal.
pub fn non_positive_gates() -> impl Iterator<Item = (WeightKey, &'static str)> {
    WeightKey::ALL.iter().filter_map(|&k| match k.sign_intent() {
        weights::SignIntent::NonPositive(why) => Some((k, why)),
        weights::SignIntent::NonNegative(_) | weights::SignIntent::Free => None,
    })
}

/// The mirror image of [`non_positive_gates`] -- see that function's own doc
/// comment.
pub fn non_negative_gates() -> impl Iterator<Item = (WeightKey, &'static str)> {
    WeightKey::ALL.iter().filter_map(|&k| match k.sign_intent() {
        weights::SignIntent::NonNegative(why) => Some((k, why)),
        weights::SignIntent::NonPositive(_) | weights::SignIntent::Free => None,
    })
}

/// One rule-level ordering `dominance_repair` had to fix -- Python's
/// `{"weight": ..., "value": ..., "default": ..., "rule": ...}` dict,
/// restated as a struct. Diagnostic only (a hillclimb log entry): nothing in
/// this crate branches on `rule`'s text, so it stays a human-readable
/// message rather than an enum -- there is no closed vocabulary of "which
/// rule fired" for a caller to match on, only a sentence to log.
#[derive(Clone, Debug, PartialEq)]
pub struct Violation {
    pub weight: WeightKey,
    pub value: f64,
    pub default: f64,
    pub rule: String,
}

/// Return `(weights, violations)` with the rule-level orderings restored.
///
/// Pure and idempotent: repairing an already-legal vector returns it
/// unchanged with an empty violation list, so it is safe to apply on every
/// load and again in a trainer's guard.
pub fn dominance_repair(w: &Weights) -> (Weights, Vec<Violation>) {
    let mut out = *w;
    let mut viol = Vec::new();

    for &k in NET_NONNEG_PHASE {
        let base = out.get(k);
        for mk in [k.early(), k.late()] {
            let m = out.get(mk);
            if base + m < -1e-12 {
                viol.push(Violation {
                    weight: mk,
                    value: m,
                    default: mk.default_weight(),
                    rule: format!("{} + {} >= 0", k.name(), mk.name()),
                });
                out.set(mk, -base);
            }
        }
    }

    // `CardBoardCredit`'s per-type offsets ([`card_board_credit_keys`]):
    // the EFFECTIVE multiplier `card_potential` scales a board-swap diff by
    // is `CardBoardCredit + <per-type key>`, not either term alone --
    // [`BENEFIT_GATES`] above only gates `CardBoardCredit` itself, which is
    // useless when the per-type key carries the negative sign instead (the
    // live 2p champion: `card_board_credit = 0.0`, `card_board_leader =
    // -15.003`). A negative EFFECTIVE scale on a diff that is genuinely
    // positive for a helpful leader/government/wonder/action/bonus swap and
    // genuinely negative for a harmful one does not mis-price one card, it
    // inverts the entire ranking of that card TYPE against every other type
    // priced with a sane sign -- exactly what `leadersign` (the investigation
    // this gate was added for) measured: `Hammurabi` priced at -13.28 despite
    // a +0.885 raw board benefit, `Julius Caesar` at +43.63 despite a -2.908
    // raw diff. Same shape as [`NET_NONNEG_PHASE`] above (a base plus a
    // modifier must not net negative), restated for a single fixed base
    // (`CardBoardCredit`) against several per-type modifiers instead of a
    // phase pair. Repaired by raising the per-type key to `-base`, never by
    // lowering `base`: same direction as every other rule in this function.
    let card_board_base = out.get(WeightKey::CardBoardCredit);
    for &k in card_board_credit_keys() {
        let m = out.get(k);
        if card_board_base + m < -1e-12 {
            viol.push(Violation {
                weight: k,
                value: m,
                default: k.default_weight(),
                rule: format!("{} + {} >= 0", WeightKey::CardBoardCredit.name(), k.name()),
            });
            out.set(k, -card_board_base);
        }
    }

    for (k, why) in non_positive_gates() {
        let v = out.get(k);
        if v > 1e-12 {
            viol.push(Violation {
                weight: k,
                value: v,
                default: k.default_weight(),
                rule: format!("{} <= 0 ({why})", k.name()),
            });
            out.set(k, 0.0);
        }
    }

    for (k, why) in non_negative_gates() {
        let v = out.get(k);
        if v < -1e-12 {
            viol.push(Violation {
                weight: k,
                value: v,
                default: k.default_weight(),
                rule: format!("{} >= 0 ({why})", k.name()),
            });
            out.set(k, 0.0);
        }
    }

    // ORDERINGS LAST, and deliberately so: a [`DOMINATES`] repair copies one
    // weight's value onto another, so running it before the sign gates would
    // propagate an illegally-signed operand into a second coordinate and then
    // report the wrong rule for it (a champion with a negative
    // `wonder_potential` and a `wonder_promise` at its 0.0 default would be
    // logged as an ordering violation, when the thing actually wrong with it
    // is the sign). Both dominant sides here are non-negative-gated or
    // ungated, so raising one can never re-open a gate the loops above just
    // closed -- which is what keeps this function idempotent in a single pass.
    for &(hi, lo) in DOMINATES {
        let a = out.get(hi);
        let b = out.get(lo);
        if b > a + 1e-12 {
            viol.push(Violation {
                weight: hi,
                value: a,
                default: hi.default_weight(),
                rule: format!("{} >= {}", hi.name(), lo.name()),
            });
            out.set(hi, b);
        }
    }

    (out, viol)
}

// ------------------------------------------------------------------- io

/// Parse a champion vector out of already-read JSON text.
///
/// Split from [`load_weights`] so that the parsing rules are testable
/// without a file on disk, and so a caller holding a champion in memory
/// (a league that just received one over a pipe) does not have to write it
/// out to read it back.
///
/// Accepts either the wrapper shape the trainer writes (`{"weights": {...},
/// "gen": 41, ...}`) or a bare `{name: value}` map, matching Python's
/// `d.get("weights", d)`. Missing keys keep their default, [`RETIRED_KEYS`]
/// are dropped, [`LEGACY_PHASE_EARLY_FOLD`] names are losslessly folded into
/// their T1-A/C/D collapse target (see that constant's own doc comment),
/// unknown keys are an error, and [`dominance_repair`] is applied on the way
/// out -- see this module's top doc comment for why the repair belongs here
/// rather than only in the trainer.
pub fn parse_weights(text: &str) -> Result<Weights, String> {
    let doc = crate::fixtures::parse_json(text).map_err(|e| format!("{e:?}"))?;
    let map = match doc.get("weights") {
        Some(w) => w,
        None => &doc,
    };
    let fields = match map {
        crate::fixtures::Json::Obj(fields) => fields,
        crate::fixtures::Json::Null | crate::fixtures::Json::Bool(_) | crate::fixtures::Json::Num(_) | crate::fixtures::Json::Str(_) | crate::fixtures::Json::Arr(_) => return Err("champion JSON is not an object".to_string()),
    };

    // Legacy `_early` values, captured by position in
    // `LEGACY_PHASE_EARLY_FOLD` rather than resolved to a live `WeightKey`
    // (there isn't one any more) -- folded into their target once the main
    // loop below has finished, so the fold is independent of where in the
    // file each name happens to sit. `None` (not `0.0`) is the "absent"
    // state -- see the fold loop below for why that distinction is load-
    // bearing: a file that never had `workers_early` at all (every file
    // saved AFTER this collapse landed) must apply NO fold, not a
    // zero-valued one, or a genuinely new-format file would get `old_base`
    // spuriously added into its already-correct `_late` ("end") value on
    // every single load.
    let mut legacy_early = LegacyPhaseEarly::new();
    let mut w = Weights::defaults();
    for (name, value) in fields {
        if RETIRED_KEYS.contains(&name.as_str()) {
            continue;
        }
        let v = value
            .as_f64()
            .ok_or_else(|| format!("weight {name:?} is not a number"))?;
        if !v.is_finite() {
            return Err(format!("weight {name:?} is not finite"));
        }
        if legacy_early.capture(name.as_str(), v) {
            continue;
        }
        let key = WeightKey::by_name(name)
            .ok_or_else(|| format!("unknown weight {name:?}"))?;
        w.set(key, v);
    }

    // T1-A/C/D collapse (PHASECUT.txt): `new_start = old_base + old_early`,
    // `new_end = old_base + old_late`, applied ONLY when the source JSON
    // actually carried a legacy `_early` field (i.e. only for a file
    // written before this collapse landed) -- a file with no such field is
    // already in the new `{start, end}` shape and must round-trip
    // unchanged. `old_base` is captured into a local BEFORE either `target`
    // (the base key, becoming `start`) or `target.late()` (becoming `end`)
    // is overwritten, so this reproduces the OLD formula's value at every
    // lateness (`A(0) = old_base + old_early`, `A(1) = old_base +
    // old_late`) regardless of what order the three legacy names appeared
    // in the source JSON.
    legacy_early.apply(&mut w);

    Ok(dominance_repair(&w).0)
}

/// The three legacy `_early` field names T1-A/C/D's collapse retired as
/// live [`WeightKey`] variants (PHASECUT.txt, 2026-08-13), paired with the
/// key their value folds into at [`parse_weights`] time. Unlike
/// [`RETIRED_KEYS`] (whose values are thrown away outright, because nothing
/// downstream still means the same thing), these three carry real
/// information that must not be lost -- every value ever climbed into
/// `workers_early`/`tech_levels_early`/`hand_value_early` is exactly
/// preserved by the fold `parse_weights` applies (see that function's own
/// comment for the arithmetic and PHASECUT.txt for the proof that it
/// reproduces bit-for-bit identical PLAY on every champion file on disk).
///
/// A file that omits one of these three names entirely (rather than
/// carrying it at `0.0`) is treated as contributing nothing to the fold --
/// deliberately NOT re-derived from the retired key's own former default,
/// since a file written by code that no longer HAS that variant (i.e. every
/// file saved after this collapse lands) is indistinguishable from one that
/// simply never specified it, and `save_weights` has always written every
/// live key, so a real champion/gauntlet file missing one of these three
/// outright is not a shape any file on disk actually has.
const LEGACY_PHASE_EARLY_FOLD: &[(&str, WeightKey)] = &[
    ("workers_early", WeightKey::Workers),
    ("tech_levels_early", WeightKey::TechLevels),
    ("hand_value_early", WeightKey::HandValue),
];

/// The legacy `_early` capture-and-fold, as a value both weight readers
/// share rather than each open-coding.
///
/// There are two readers on purpose ([`parse_weights`] here and
/// `human_policy::parse_weights_text`), and when the T1-A/C/D collapse
/// landed only this one was taught the fold -- so every frozen human /
/// anchor file on disk became unloadable by the other, which is precisely
/// the failure its own comment says must never happen ("a file written
/// before a key was retired must stay loadable by BOTH"). Duplicating six
/// lines is what allowed the two to drift; the fold lives here once so the
/// next retirement cannot silently break one reader and not the other.
pub(crate) struct LegacyPhaseEarly([Option<f64>; LEGACY_PHASE_EARLY_FOLD.len()]);

/// The legacy `_early` field names, for the cross-reader agreement test in
/// `human_policy` -- enumerated from the fold table itself so a name added
/// there is covered without anyone remembering to list it twice.
#[cfg(test)]
pub(crate) fn legacy_phase_early_names() -> &'static [(&'static str, WeightKey)] {
    LEGACY_PHASE_EARLY_FOLD
}

impl LegacyPhaseEarly {
    pub(crate) fn new() -> Self {
        Self([None; LEGACY_PHASE_EARLY_FOLD.len()])
    }

    /// Record `name`'s value if it is a legacy `_early` field, reporting
    /// whether it was one. A caller that gets `true` must NOT go on to
    /// resolve the name against [`WeightKey::by_name`] -- there is no live
    /// variant left to resolve it to, which is the whole reason a plain
    /// `by_name` lookup rejects these files.
    pub(crate) fn capture(&mut self, name: &str, v: f64) -> bool {
        match LEGACY_PHASE_EARLY_FOLD.iter().position(|&(n, _)| n == name) {
            Some(i) => {
                self.0[i] = Some(v);
                true
            }
            None => false,
        }
    }

    /// `new_start = old_base + old_early`, `new_end = old_base + old_late`,
    /// applied only for names the source file actually carried. `None` (not
    /// `0.0`) is the absent state: see [`LEGACY_PHASE_EARLY_FOLD`] for why
    /// a new-format file must apply NO fold rather than a zero-valued one.
    /// `old_base` is read before either target is written, so the result is
    /// independent of the order the names appeared in the JSON.
    pub(crate) fn apply(&self, w: &mut Weights) {
        for (i, &(_, target)) in LEGACY_PHASE_EARLY_FOLD.iter().enumerate() {
            if let Some(early_v) = self.0[i] {
                let old_base = w.get(target);
                w.set(target, old_base + early_v);
                w.set(target.late(), old_base + w.get(target.late()));
            }
        }
    }
}

/// Read a champion vector from disk. See [`parse_weights`].
pub fn load_weights(path: &std::path::Path) -> Result<Weights, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    parse_weights(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Render a champion vector as the JSON text [`parse_weights`] reads back.
///
/// `extra` is the trainer's bookkeeping (`gen`, `players`, `sigma`,
/// `since_accept`) written alongside the vector, exactly as Python's
/// `save_weights(**extra)` does. It is `(name, f64)` rather than a general
/// JSON value because every field any caller has ever written is a number,
/// and a general value would need a serialiser for shapes nothing emits.
///
/// Keys are sorted and the indent is one space, matching the files already
/// in `experiments/`, so a champion rewritten by the native trainer produces
/// a readable diff against the one the Python league wrote rather than
/// reformatting the whole file.
pub fn weights_json(w: &Weights, extra: &[(&str, f64)]) -> String {
    let mut names: Vec<&'static str> = WeightKey::ALL.iter().map(|k| k.name()).collect();
    names.sort_unstable();

    let mut top: Vec<(String, String)> = extra
        .iter()
        .map(|(k, v)| ((*k).to_string(), fmt_num(*v)))
        .collect();
    let mut body = String::from("{\n");
    for (i, name) in names.iter().enumerate() {
        let key = WeightKey::by_name(name).expect("name came from WeightKey::ALL");
        let sep = if i + 1 == names.len() { "\n" } else { ",\n" };
        body.push_str(&format!("  \"{name}\": {}{sep}", fmt_num(w.get(key))));
    }
    body.push_str(" }");
    top.push(("weights".to_string(), body));
    top.sort_by(|a, b| a.0.cmp(&b.0));

    let mut out = String::from("{\n");
    for (i, (k, v)) in top.iter().enumerate() {
        let sep = if i + 1 == top.len() { "\n" } else { ",\n" };
        out.push_str(&format!(" \"{k}\": {v}{sep}"));
    }
    out.push('}');
    out
}

/// Write a champion vector where [`load_weights`] will read it back.
///
/// Writes to a sibling `.tmp` and renames, as Python does: the league reads
/// the champion file while the trainer is writing the next one, and a
/// half-written vector that still parses is the failure this avoids.
pub fn save_weights(
    path: &std::path::Path,
    w: &Weights,
    extra: &[(&str, f64)],
) -> Result<(), String> {
    for (name, v) in extra {
        if !v.is_finite() {
            return Err(format!("{name:?} is not finite"));
        }
    }
    for &k in WeightKey::ALL {
        if !w.get(k).is_finite() {
            return Err(format!("weight {:?} is not finite", k.name()));
        }
    }
    let tmp = path.with_extension("tmp");
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
    }
    std::fs::write(&tmp, weights_json(w, extra)).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

/// `f64` as JSON. Rust's `Display` already emits the shortest text that
/// round-trips, but writes an integral value as `1` rather than `1.0`; both
/// are valid JSON numbers and both read back as the same `f64`, so this
/// exists only to keep the non-finite case unreachable at the one place
/// that formats a weight.
fn fmt_num(v: f64) -> String {
    debug_assert!(v.is_finite(), "callers reject non-finite before formatting");
    format!("{v}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::CardId;
    use crate::game as G;

    // ------------------------------------------------------------ evaluate

    /// The all-defaults evaluation of a fresh deal is finite and, since
    /// `culture`'s weight is 1.0 and every player starts at 0 culture plus a
    /// negative-leaning economy vector, not wildly out of range -- a coarse
    /// smoke test that the whole assembly (features + phase blend + every
    /// identity-aware term gated at its default weight) runs without
    /// panicking on a real state.
    #[test]
    fn evaluate_on_a_fresh_deal_is_finite() {
        for n in [2u8, 3, 4] {
            let state = G::new_game(n, 100);
            let w = Weights::default();
            for idx in 0..n {
                let v = evaluate(&state, idx, &w, None, None);
                assert!(v.is_finite(), "{n}p idx {idx}: {v}");
            }
        }
    }

    // ------------------------------------------------- earlymil StrengthRel

    /// STRUCTURAL FIX regression (earlymil, 2026-08-13): at a genuinely
    /// early, low-lateness state, `StrengthRel`'s effective coefficient must
    /// be close to `strength_rel_early` ALONE, not `strength_rel +
    /// strength_rel_early` (the pre-fix formula) -- the whole point of the
    /// fix is that the always-on base no longer applies in full at the
    /// opening. Isolates `StrengthRel`'s own contribution as a pure
    /// before/after `evaluate` delta between two otherwise-identical states
    /// (seat 0 with 2 Warriors workers staffed vs. none), the same
    /// technique `bin/dumpweights.rs` used to confirm the mechanism in a
    /// real position (see EARLYMIL.txt step 1) -- every OTHER coordinate
    /// `Weights::default()` prices is identical between the two states and
    /// cancels out of the delta, so this isolates exactly the `Strength`/
    /// `StrengthRel` family, which the test zeroes out except for the three
    /// keys under test.
    #[test]
    fn evaluate_prices_strength_rel_near_its_early_only_coefficient_at_low_lateness_not_the_old_always_on_base() {
        let base = G::new_game(2, 60);
        let mut strong = base.clone();
        strong.players[0].techs.get_mut(CardId::by_name("Warriors").unwrap()).unwrap().workers = 2;

        // Every OTHER coordinate zeroed, not merely left at
        // `Weights::default()` -- staffing 2 Warriors workers moves more
        // than `StrengthRel` (worker-count/military-action-gap features
        // move too), so a delta taken against the full default vector picks
        // up noise from every one of those. Zeroing everything except the
        // three keys under test is what actually isolates `StrengthRel`.
        let mut w = Weights::default();
        for &k in WeightKey::ALL {
            w.set(k, 0.0);
        }
        w.set(WeightKey::StrengthRel, 17.0);
        w.set(WeightKey::StrengthRel.early(), -1.0);
        w.set(WeightKey::StrengthRel.late(), 9.0);

        let late = horizon::lateness(&base);
        assert!(late < 0.15, "fixture assumption: a fresh deal must be genuinely early, got {late}");
        let early = 1.0 - late;

        // The raw feature delta (own-strength-minus-rival's-strength gained
        // by staffing 2 Warriors workers), read directly rather than
        // assumed, so this test does not silently drift if `rel`'s exact
        // magnitude ever changes for an unrelated reason.
        let ctx_base = rivals::rival_context(&base, 0, None, None);
        let v_base = features::features(&base, 0, Some(&ctx_base), Some(&w), false).get(WeightKey::StrengthRel);
        let ctx_strong = rivals::rival_context(&strong, 0, None, None);
        let v_strong = features::features(&strong, 0, Some(&ctx_strong), Some(&w), false).get(WeightKey::StrengthRel);
        let v_delta = v_strong - v_base;
        assert!(v_delta > 0.0, "fixture assumption: staffing 2 Warriors workers must raise relative strength, got delta {v_delta}");

        let delta = evaluate(&strong, 0, &w, None, None) - evaluate(&base, 0, &w, None, None);

        // New formula: eff(L) = (1-L)*early + L*(base+late).
        let new_expected = (-early + late * (17.0 + 9.0)) * v_delta;
        assert!((delta - new_expected).abs() < 1e-9, "delta={delta} new_expected={new_expected}");

        // The OLD (pre-fix) formula would have been base + (1-L)*early +
        // L*late -- decisively bigger at this low lateness, since the base
        // (17.0) used to apply in full regardless of phase. Pinning this
        // negative comparison is what actually proves the fix moved
        // behaviour, not just that some formula matches itself.
        let old_formula_would_have_given = (17.0 + -early + late * 9.0) * v_delta;
        assert!(
            delta < old_formula_would_have_given - 1.0,
            "the new formula must price this gain decisively below the old always-on-base formula at low lateness: delta={delta} old={old_formula_would_have_given}"
        );
    }

    /// At `lateness == 1.0` the new formula is BYTE-IDENTICAL to the old one
    /// (`(1-L)*early + L*(base+late)` collapses to `base+late` exactly when
    /// `L == 1`) -- late-game strength pricing, and every champion snapshot
    /// on disk trained assuming it, is untouched by this fix. Constructed by
    /// calling `evaluate` directly with a hand-built `Features` at the
    /// literal `L == 1` endpoint (via a fixed `late` passed through
    /// `horizon::lateness`'s own clamp is state-derived and can't be forced
    /// to exactly 1.0 from a real deal) is not needed here: this test
    /// instead pins the ENDPOINT ALGEBRAICALLY, matching the doc comment in
    /// `evaluate` itself, rather than hunting for a real state that happens
    /// to clamp there.
    #[test]
    fn the_new_strength_rel_formula_collapses_to_the_old_one_exactly_at_full_lateness() {
        let base_w = 17.0_f64;
        let early_w = -1.0_f64;
        let late_w = 9.0_f64;
        let late = 1.0_f64;
        let early = 1.0 - late;
        let v = 3.0_f64;
        let new_formula = (early * early_w + late * (base_w + late_w)) * v;
        let old_formula = (base_w + early * early_w + late * late_w) * v;
        assert!((new_formula - old_formula).abs() < 1e-12, "new={new_formula} old={old_formula}");
    }

    /// REVISION (STRGATE.txt, superseding the earlymil `state.round <= 3`
    /// literal): the `StrengthRel` fix is gated to
    /// [`horizon::combat_unreachable`] -- a computed rulebook fact (no
    /// player can hold an aggression/war card) rather than a fitted round
    /// number. Once ANY player could plausibly hold a combat-capable card,
    /// `evaluate` must price a relative-strength gain EXACTLY as it did
    /// before this fix existed -- this is what makes the fix a narrow,
    /// targeted correction of the measured opening-only defect rather than
    /// a blanket re-weighting of military value for the whole game (the
    /// earlier, ungated version of this fix DID re-weight the whole game
    /// and measured decisively worse, 27.7% at n=3000 -- see `evaluate`'s
    /// own doc comment). Same before/after isolation technique as the
    /// low-lateness test above, with the gate pushed open by hand -- NOT by
    /// bumping `state.round` (that no longer controls this at all), but by
    /// giving a rival a nonzero military hand COUNT
    /// (`PlayerState::hidden_military`, a count-only field -- see
    /// `combat_unreachable`'s own doc comment on why a count, not a real
    /// card, is enough and is the only thing legal to use here).
    #[test]
    fn evaluate_prices_strength_rel_with_the_old_always_on_base_once_combat_is_reachable() {
        let mut base = G::new_game(2, 61);
        assert!(horizon::combat_unreachable(&base), "fixture assumption: a fresh deal has no military cards in any hand yet");
        // Past `horizon::EARLIEST_COMBAT_ROUND` -- that floor holds
        // regardless of hand contents (see its own dedicated test), so a
        // hand-based fixture must clear it first to isolate the hand-size
        // half of the predicate, same as `horizon.rs`'s own tests do.
        base.round = horizon::EARLIEST_COMBAT_ROUND;
        base.players[1].hidden_military = 1;
        assert!(!horizon::combat_unreachable(&base), "fixture setup: a rival with 1 military card in hand, past the round floor, must open the gate");
        let mut strong = base.clone();
        strong.players[0].techs.get_mut(CardId::by_name("Warriors").unwrap()).unwrap().workers = 2;

        let mut w = Weights::default();
        for &k in WeightKey::ALL {
            w.set(k, 0.0);
        }
        w.set(WeightKey::StrengthRel, 17.0);
        w.set(WeightKey::StrengthRel.early(), -1.0);
        w.set(WeightKey::StrengthRel.late(), 9.0);

        let late = horizon::lateness(&base);
        let early = 1.0 - late;

        let ctx_base = rivals::rival_context(&base, 0, None, None);
        let v_base = features::features(&base, 0, Some(&ctx_base), Some(&w), false).get(WeightKey::StrengthRel);
        let ctx_strong = rivals::rival_context(&strong, 0, None, None);
        let v_strong = features::features(&strong, 0, Some(&ctx_strong), Some(&w), false).get(WeightKey::StrengthRel);
        let v_delta = v_strong - v_base;
        assert!(v_delta > 0.0, "fixture assumption: staffing 2 Warriors workers must raise relative strength, got delta {v_delta}");

        let delta = evaluate(&strong, 0, &w, None, None) - evaluate(&base, 0, &w, None, None);
        let old_formula_expected = (17.0 + -early + late * 9.0) * v_delta;
        assert!((delta - old_formula_expected).abs() < 1e-9, "delta={delta} old_formula_expected={old_formula_expected}");
    }

    /// `f: Some(...)` is used exactly as given, not silently re-derived from
    /// `state`. Built by handing `evaluate` an `f` read off a DIFFERENT
    /// (mutated) state than the `state` argument itself: `culture`'s weight
    /// is 1.0 by default, so a `f` carrying 5 more culture must raise the
    /// total by ~5, even though the `state` argument the identity-aware
    /// terms (`hand_potential` and friends) read directly is unchanged.
    #[test]
    fn evaluate_with_an_explicit_f_is_used_verbatim_not_recomputed() {
        let state = G::new_game(2, 101);
        let w = Weights::default();
        let mut bumped_state = state.clone();
        bumped_state.players[0].culture += 5;
        let f_bumped = features::features(&bumped_state, 0, None, Some(&w), true);

        let with_f_bumped = evaluate(&state, 0, &w, None, Some(&f_bumped));
        let with_state_only = evaluate(&state, 0, &w, None, None);
        assert!(
            with_f_bumped > with_state_only + 4.0,
            "f={with_f_bumped} state_only={with_state_only}: an explicit f must be read as given"
        );
    }

    /// Every default-weighted, identity-aware term is gated at exactly 0.0:
    /// a champion vector that never turns on `hand_potential`/
    /// `wonder_potential`/`hand_mil_potential`/`tactic_gain`/`tactic_short`/
    /// `rival_hand_potential`/`row_urgency`/`row_bargain_forgone`/
    /// `row_last_copy`/`my_event_threat` gets a score that does not change
    /// when those functions' inputs change -- pinned by comparing two states
    /// that differ ONLY in a rival's civil hand (which only
    /// `rival_hand_potential` reads) under `DEFAULT_WEIGHTS`.
    #[test]
    fn default_weights_price_nothing_off_a_rivals_hand() {
        let mut state = G::new_game(2, 102);
        let before = evaluate(&state, 0, &Weights::default(), None, None);
        state.players[1].hand_civil.push(crate::cards::CardId::by_name("Irrigation").unwrap());
        let after = evaluate(&state, 0, &Weights::default(), None, None);
        assert_eq!(before, after, "rival_hand_potential defaults to 0.0 and must price nothing");
    }

    /// `rival_hand_potential` DOES move the score once its weight is turned
    /// on -- the positive control for the test above, so a bug that made
    /// BOTH sides silently price nothing could not hide behind it.
    #[test]
    fn rival_hand_potential_moves_the_score_once_priced() {
        let without_state = G::new_game(2, 103);
        let mut with_state = without_state.clone();
        with_state.players[1].hand_civil.push(crate::cards::CardId::by_name("Irrigation").unwrap());
        let mut w = Weights::default();
        w.set(WeightKey::RivalHandPotential, 1.0);
        let with_hand = evaluate(&with_state, 0, &w, None, None);
        let without_hand = evaluate(&without_state, 0, &w, None, None);
        assert!(with_hand > without_hand, "a priced rival hand must raise the rival's threat term");
    }

    // -------------------------------------------------------- WeightedBot

    /// A single legal move short-circuits without touching `evaluate` or the
    /// real state -- mirrors `quiescent.rs`'s identical single-move test.
    #[test]
    fn choose_with_a_single_move_returns_it_directly() {
        let state = G::new_game(2, 1);
        let moves = crate::legal::legal_moves(&state);
        let one = [moves.as_slice()[0]];
        let bot = WeightedBot::default();
        assert_eq!(bot.choose(&state, &one), one[0]);
    }

    /// `choose` never mutates the real state -- every candidate is scored on
    /// a clone.
    #[test]
    fn choose_never_mutates_the_real_state() {
        let state = G::new_game(2, 2);
        let before = state.clone();
        let moves = crate::legal::legal_moves(&state);
        let bot = WeightedBot::default();
        let _ = bot.choose(&state, moves.as_slice());
        assert_eq!(state.card_row, before.card_row);
        assert_eq!(state.turn, before.turn);
    }

    /// `choose` always returns one of the offered moves.
    #[test]
    fn choose_always_returns_an_offered_move() {
        for n in [2u8, 3, 4] {
            let state = G::new_game(n, 3);
            let moves = crate::legal::legal_moves(&state);
            let bot = WeightedBot::default();
            let mv = bot.choose(&state, moves.as_slice());
            assert!(moves.as_slice().contains(&mv), "{n}p: {mv:?} was not offered");
        }
    }

    /// `allow_resign = false` (the default) drops `Move::Resign` from the
    /// candidate set whenever a non-resign move is legal too -- built
    /// directly (round 1 never offers `Resign`), matching the shape of
    /// Python's own regression rather than hunting for a state where it is
    /// naturally legal.
    #[test]
    fn resign_is_filtered_out_when_a_live_alternative_exists() {
        let state = G::new_game(2, 4);
        let bot = WeightedBot::default();
        let picked = bot.choose(&state, &[Move::Resign, Move::EndTurn]);
        assert_eq!(picked, Move::EndTurn, "resign must never be chosen over a live alternative");
    }

    /// `allow_resign = true` restores `Resign` as an eligible candidate --
    /// forced by handing `choose` a set where `Resign` scores strictly
    /// highest under a hand-built weight vector that only rewards it via
    /// `end_turn_bias`-style domination is awkward to construct, so this
    /// instead pins the STRUCTURAL half: with the flag on, a lone `Resign`
    /// candidate is not filtered away to an empty set (which would panic on
    /// `moves[0]` of nothing).
    #[test]
    fn allow_resign_keeps_a_lone_resign_candidate_eligible() {
        let state = G::new_game(2, 5);
        let bot = WeightedBot { allow_resign: true, ..WeightedBot::default() };
        let picked = bot.choose(&state, &[Move::Resign]);
        assert_eq!(picked, Move::Resign);
    }

    /// A 1-ply `WeightedBot` trial for `Move::PrepareEvent` must not read the
    /// TRUE top of `state.current_events` -- `apply.rs::h_prepare_event`
    /// reveal-and-resolves it unconditionally, mid-`apply`, and nobody
    /// peeked it (`peeked_event` is `CardId::NONE` here). Built from two
    /// states identical in every field except which literal card sits on
    /// top of an otherwise-identical six-card pile: "Crusades" (`Strongest
    /// Player` +4 culture -- player 0 is forced strongest via
    /// `strength_extra`) versus "Pestilence" (`AllPlayers` -1 population,
    /// rank-independent). Confirmed empirically for this exact state/weight
    /// pair (not asserted here, to keep the test itself black-box): reading
    /// the literal top directly swings `evaluate` from clearly above
    /// `PolPass`'s score (Crusades) to clearly below it (Pestilence) -- a
    /// real decision flip a leaking bot could not help but make.
    ///
    /// Post-fix, [`WeightedBot::choose`] re-shuffles the pile once before
    /// either candidate is scored (see `choose`'s own doc comment), and a
    /// Fisher-Yates shuffle's sequence of index swaps depends only on the
    /// rng stream and the slice's LENGTH, never its contents -- both states
    /// hand it a length-6 slice with the SAME seed/turn/idx-derived stream
    /// (`plan::plan_rng`), so the swapped-to-the-top card is the identical
    /// FILLER card in both, and every candidate's score becomes byte-for-
    /// byte equal between the two states. `choose` must therefore return
    /// the SAME move for both, regardless of which of the two swings a
    /// leaking bot would have felt.
    #[test]
    fn a_weighted_bots_choice_does_not_depend_on_which_card_sits_atop_an_unpeeked_events_pile() {
        use crate::state::{CardList, Phase};

        let hand_card = CardId::by_name("Development of Politics").unwrap();
        let strong_top = CardId::by_name("Crusades").unwrap();
        let weak_top = CardId::by_name("Pestilence").unwrap();
        let fillers = ["Raiders", "Reign of Terror", "Border Conflict", "Uncertain Borders", "Rebellion"]
            .map(|n| CardId::by_name(n).unwrap());

        let build = |top: CardId| {
            let mut state = G::new_game(2, 1);
            state.phase = Phase::Politics;
            state.current = 0;
            // Forces player 0 strongest, so "Crusades"' `StrongestPlayer`
            // block targets them (a clean plus) rather than the rival.
            state.players[0].strength_extra = 10;
            state.players[0].hand_military.push(hand_card);
            state.current_events = CardList::new();
            for &f in &fillers {
                state.current_events.push(f);
            }
            state.current_events.push(top);
            state
        };
        let state_strong = build(strong_top);
        let state_weak = build(weak_top);
        let moves = [Move::PrepareEvent { card: hand_card }, Move::PolPass];
        let bot = WeightedBot::default();

        assert_eq!(
            bot.choose(&state_strong, &moves),
            bot.choose(&state_weak, &moves),
            "the two states differ ONLY in which card an unpeeked events pile has on top -- a bot that has \
             not legitimately peeked it must not let that difference change its move"
        );
    }

    /// `rank_moves`'s own top entry must always agree with `choose` on the
    /// SAME `state`/`moves` -- the move-agreement analysis this exists for
    /// (`bin/agreement.rs`) reports "did the bot's #1 choice match the
    /// human's" straight off `rank_moves`' first entry, so a silent
    /// divergence here would be a silent divergence in every reported
    /// agreement rate.
    #[test]
    fn rank_moves_top_entry_always_agrees_with_choose() {
        for n in [2u8, 3, 4] {
            let state = G::new_game(n, 6);
            let moves = crate::legal::legal_moves(&state);
            let bot = WeightedBot::default();
            let chosen = bot.choose(&state, moves.as_slice());
            let ranked = bot.rank_moves(&state, moves.as_slice());
            assert_eq!(ranked[0].0, chosen, "{n}p: rank_moves()[0] disagreed with choose()");
        }
    }

    /// `rank_moves` returns exactly one entry per (post-`filter_resign`)
    /// candidate, in strictly non-increasing score order.
    #[test]
    fn rank_moves_is_sorted_descending_and_covers_every_candidate() {
        let state = G::new_game(2, 7);
        let moves = crate::legal::legal_moves(&state);
        let bot = WeightedBot::default();
        let ranked = bot.rank_moves(&state, moves.as_slice());

        assert_eq!(ranked.len(), moves.len(), "every legal move must appear exactly once");
        for w in ranked.windows(2) {
            assert!(w[0].1 >= w[1].1, "not sorted descending: {:?} then {:?}", w[0], w[1]);
        }
        for &(mv, _) in &ranked {
            assert!(moves.as_slice().contains(&mv), "{mv:?} ranked but never offered");
        }
    }

    /// A single-candidate call still returns a real score, not the empty
    /// `Vec` reserved for a genuinely empty candidate list -- `rank_moves`
    /// deliberately skips `choose`'s `len() == 1` short-circuit (see its own
    /// doc comment) so a caller comparing scores across decision points
    /// never has to special-case "the human had no real choice."
    #[test]
    fn rank_moves_with_a_single_candidate_still_scores_it() {
        let state = G::new_game(2, 8);
        let bot = WeightedBot::default();
        let ranked = bot.rank_moves(&state, &[Move::EndTurn]);
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].0, Move::EndTurn);
        assert!(ranked[0].1.is_finite());
    }

    /// An empty candidate list ranks to an empty `Vec` -- no `moves[0]`
    /// panic, unlike `choose`'s own contract for this input.
    #[test]
    fn rank_moves_with_no_candidates_returns_an_empty_vec() {
        let state = G::new_game(2, 9);
        let bot = WeightedBot::default();
        assert!(bot.rank_moves(&state, &[]).is_empty());
    }

    // ------------------------------------------------- agreement-fit features

    /// `candidate_features` dotted (via `dot`) against the SAME weight
    /// vector it was frozen at must reproduce `rank_moves`' own scores
    /// exactly -- the equivalence this whole approximation rests on (see
    /// this module's own doc comment on `linear_features`).
    #[test]
    fn linear_features_dotted_with_a_weight_vector_reproduces_evaluate_exactly() {
        for n in [2u8, 3, 4] {
            let state = G::new_game(n, 11);
            let moves = crate::legal::legal_moves(&state);
            let w = Weights::default();
            let bot = WeightedBot::new(w);
            let ranked = bot.rank_moves(&state, moves.as_slice());
            let feats = candidate_features(&state, moves.as_slice(), false, &w);
            assert_eq!(ranked.len(), feats.len(), "{n}p: candidate set must match rank_moves' own");
            for &(mv, score) in &ranked {
                let (_, f) = feats.iter().find(|&&(m, _)| m == mv).unwrap_or_else(|| panic!("{mv:?} missing"));
                let linear = dot(&w, f);
                assert!(
                    (linear - score).abs() < 1e-6,
                    "{n}p {mv:?}: linear={linear} evaluate={score}"
                );
            }
        }
    }

    /// Every identity-aware gate defaults to 0.0, so the invariant above is
    /// satisfied vacuously for any coordinate `linear_features` forgets to
    /// write: nothing multiplies the missing entry. Turn one on and the
    /// omission becomes an arithmetic disagreement.
    ///
    /// `wonder_promise` is the one that was missing. It is the ONLY term
    /// nonzero on the move that TAKES a wonder, so every offline fitting
    /// tool -- all of which read `linear_features`, not `evaluate` -- saw an
    /// exactly-zero coordinate and could not learn a wonder preference at
    /// the only moment one is expressible. Measured on the 2p `Take` below
    /// with the coordinate unwritten: `linear_features` reported 0.0 where
    /// `cards::wonder_promise` returns 3.06, while `wonder_potential` was
    /// 0.0 on BOTH sides (its documented early return at `paid_fraction ==
    /// 0.0`) -- so on a take there is no second term to absorb the loss.
    #[test]
    fn linear_features_reproduces_evaluate_with_the_wonder_gates_switched_on() {
        for n in [2u8, 3, 4] {
            let mut state = G::new_game(n, 11);
            // A fresh board has nobody building anything, and
            // `cards::wonder_promise` returns 0.0 before a single stage is
            // owed -- put a real wonder in progress or the test passes
            // whether or not the coordinate is ever written.
            state.players[0].wonder = crate::cards::CardId::by_name("Pyramids").unwrap();
            assert_ne!(
                cards::wonder_promise(&state, 0, &Weights::default()),
                0.0,
                "{n}p: fixture must actually price a promise"
            );
            let moves = crate::legal::legal_moves(&state);
            let mut w = Weights::default();
            // Both halves of the wonder pricing, so a swap between the two
            // sibling coordinates cannot pass either. Ordered the way
            // `DOMINATES` requires (potential >= promise) so the fixture is a
            // vector `dominance_repair` would leave alone.
            w.set(WeightKey::WonderPotential, 2.5);
            w.set(WeightKey::WonderPromise, 1.5);
            let bot = WeightedBot::new(w);
            let ranked = bot.rank_moves(&state, moves.as_slice());
            let feats = candidate_features(&state, moves.as_slice(), false, &w);
            for &(mv, score) in &ranked {
                let (_, f) =
                    feats.iter().find(|&&(m, _)| m == mv).unwrap_or_else(|| panic!("{mv:?} missing"));
                let linear = dot(&w, f);
                assert!(
                    (linear - score).abs() < 1e-6,
                    "{n}p {mv:?}: linear={linear} evaluate={score}"
                );
            }
        }
    }

    /// The invariant above must still hold once the two NEW leaf-eval
    /// coordinates (`leader_replacement`, `wonder_pool_rival_claimed`) are
    /// actually non-zero, not just on a fresh board where both start at
    /// their default zero -- an off-by-one in either derivation (e.g.
    /// `linear_features`'s generic `WeightKey::ALL` loop reading the wrong
    /// index, or `features()` writing the wrong value) would only show up
    /// once the coordinate moves. Fixtures: an empty leader slot, an
    /// original (never-swapped) leader, a replaced leader, and a board
    /// where a rival has completed one `age_civil` wonder alongside the
    /// evaluated player.
    #[test]
    fn linear_features_still_reproduces_evaluate_once_the_two_new_coordinates_are_nonzero() {
        fn check(state: &crate::state::GameState, label: &str) {
            let moves = crate::legal::legal_moves(state);
            let w = Weights::default();
            let bot = WeightedBot::new(w);
            let ranked = bot.rank_moves(state, moves.as_slice());
            let feats = candidate_features(state, moves.as_slice(), false, &w);
            assert_eq!(ranked.len(), feats.len(), "{label}: candidate set must match rank_moves' own");
            for &(mv, score) in &ranked {
                let (_, f) =
                    feats.iter().find(|&&(m, _)| m == mv).unwrap_or_else(|| panic!("{label} {mv:?} missing"));
                let linear = dot(&w, f);
                assert!((linear - score).abs() < 1e-6, "{label} {mv:?}: linear={linear} evaluate={score}");
            }
        }

        let moses = crate::cards::CardId::by_name("Moses").expect("a base-game leader");
        for n in [2u8, 3, 4] {
            // Empty leader slot: `LeaderReplacement` must read 0.0 (proven
            // separately in features.rs's own test), but the DOT invariant
            // needs checking too, not just the raw feature value.
            let mut empty = G::new_game(n, 81);
            empty.players[0].leader = crate::cards::CardId::NONE;
            empty.players[0].taken_leader_ages = 0;
            check(&empty, &format!("{n}p empty leader"));

            // An original, never-swapped leader.
            let mut original = G::new_game(n, 82);
            original.players[0].leader = moses;
            original.players[0].taken_leader_ages = 1 << (crate::cards::Age::A as u8);
            check(&original, &format!("{n}p original leader"));

            // A replaced leader: popcount(taken_leader_ages) >= 2.
            let mut replaced = G::new_game(n, 83);
            replaced.players[0].leader = moses;
            replaced.players[0].taken_leader_ages =
                (1 << (crate::cards::Age::A as u8)) | (1 << (crate::cards::Age::I as u8));
            check(&replaced, &format!("{n}p replaced leader"));
        }

        // Wonder pool: a rival (idx 1) completes an `age_civil` wonder
        // alongside the evaluated player's own (idx 0) -- needs a real
        // rival, so 2p/3p only.
        let pyramids = crate::cards::CardId::by_name("Pyramids").expect("an Age A wonder");
        let hanging_gardens = crate::cards::CardId::by_name("Hanging Gardens").expect("an Age A wonder");
        for n in [2u8, 3] {
            let mut state = G::new_game(n, 84);
            state.age_civil = crate::cards::Age::A;
            state.players[0].completed_wonders.push(pyramids);
            state.players[1].completed_wonders.push(hanging_gardens);
            check(&state, &format!("{n}p wonder pool"));
        }
    }

    // ---------------------------------------------------------- dominance

    /// The bug this gate was added for, reproduced with the champion's own
    /// numbers rather than a token `+1.0`: the live 2p vector priced
    /// `wonder_remaining` at +11.51 (authored default -0.3) and
    /// `wonder_overrun` at +1.11, and a 200-game census of it completed ZERO
    /// wonders across 400 player-games. Every wonder-debt coordinate FALLS
    /// when a stage is paid, so pricing them positive makes `WonderStep` a
    /// scoring loss and the bot takes wonders it will never build.
    #[test]
    fn the_2p_champions_positive_wonder_debt_weights_are_repaired_away() {
        let mut w = Weights::default();
        w.set(WeightKey::WonderRemaining, 11.510_363_264_976_004);
        w.set(WeightKey::WonderOverrun, 1.111_842_383_161_555_8);

        let (out, viol) = dominance_repair(&w);

        assert_eq!(out.get(WeightKey::WonderRemaining), 0.0);
        assert_eq!(out.get(WeightKey::WonderOverrun), 0.0);
        assert_eq!(viol.len(), 2, "both violations reported, got {viol:?}");
        assert!(
            viol.iter().all(|v| v.rule.contains("unpaid wonder debt")),
            "the log has to say WHY, got {viol:?}"
        );
    }

    /// A key only belongs in the wonder-debt bucket of [`WeightKey::
    /// sign_intent`] if its author already treated it as a cost or left it
    /// unmeasured. A positive default would mean the crate itself disagrees
    /// with its own classification, which is a contradiction to resolve at
    /// the source, not to repair away every load -- the general version of
    /// this check now lives in `weights.rs`'s own `every_sign_intent_
    /// classification_agrees_with_its_own_authored_default`; this pins the
    /// specific five wonder-debt keys by name so a reader of THIS test file
    /// still sees the claim spelled out for the bug it was written for.
    #[test]
    fn no_gated_wonder_debt_weight_is_authored_as_an_upside() {
        for k in [
            WeightKey::WonderRemaining,
            WeightKey::WonderStagesLeft,
            WeightKey::WonderTurnsToFinish,
            WeightKey::WonderOverrun,
            WeightKey::WonderAgeOverrun,
        ] {
            assert!(
                k.default_weight() <= 0.0,
                "{} defaults to {}, contradicting its own gate",
                k.name(),
                k.default_weight()
            );
        }
    }

    /// The bug [`WONDER_VALUE_GATES`] closes, reproduced with the live 2p
    /// champion's own number rather than a token `-1.0`: that arm priced
    /// `wonder_potential` at **-0.7206**. `cards::wonder_potential` is
    /// benefit-shaped by construction (costs excluded, discount factor in
    /// `[0, 1]` and rising), so a negative weight scores "this specific
    /// in-progress wonder would be worth MORE once finished" as WORSE --
    /// the identical inversion [`WONDER_DEBT_GATES`] exists to prevent one
    /// level up, now landed in the function that gate's own doc comment
    /// names as the value correlation's rightful home.
    #[test]
    fn the_2p_champions_negative_wonder_potential_weight_is_repaired_away() {
        let mut w = Weights::default();
        w.set(WeightKey::WonderPotential, -0.7206);

        let (out, viol) = dominance_repair(&w);

        assert_eq!(out.get(WeightKey::WonderPotential), 0.0);
        assert_eq!(viol.len(), 1, "exactly one violation, got {viol:?}");
        assert!(
            viol[0].rule.contains("completion value"),
            "the log has to say WHY (the wonder's completion value), got {viol:?}"
        );
    }

    /// The bug [`STOCK_NONNEG_GATES`] closes, reproduced with the live 2p
    /// champion's own numbers: that arm priced `civil_actions` at **-0.520**,
    /// `civil_action_surplus` at **-1.324**, and `wonders` at **-1.20**. None
    /// of the three were in any gate table before this fix, so a vector
    /// pricing unspent civil actions and completed wonders as PENALTIES was
    /// fully legal and made the champion prefer to overpay for cards --
    /// exactly the confound [`LOSS_GATES`]'s own doc comment warns about, one
    /// gate list over.
    #[test]
    fn the_2p_champions_negative_civil_action_and_wonder_stock_weights_are_repaired_away() {
        let mut w = Weights::default();
        w.set(WeightKey::CivilActions, -0.520);
        w.set(WeightKey::CivilActionSurplus, -1.324);
        w.set(WeightKey::Wonders, -1.20);

        let (out, viol) = dominance_repair(&w);

        assert_eq!(out.get(WeightKey::CivilActions), 0.0);
        assert_eq!(out.get(WeightKey::CivilActionSurplus), 0.0);
        assert_eq!(out.get(WeightKey::Wonders), 0.0);
        assert_eq!(viol.len(), 3, "exactly three violations, got {viol:?}");
        for v in &viol {
            assert!(
                v.rule.contains("never subtract"),
                "the log has to say WHY (a stock the rules never subtract for), got {v:?}"
            );
        }
    }

    /// A key only belongs in [`non_negative_gates`] if its author already
    /// treated it as an upside or left it unmeasured -- the mirror of
    /// [`no_gated_wonder_debt_weight_is_authored_as_an_upside`] in the
    /// opposite direction, and the general form of the same claim
    /// `weights.rs`'s `every_sign_intent_classification_agrees_with_its_
    /// own_authored_default` also checks. A negative default would mean the
    /// crate itself contradicts its own gate, which is a bug to fix at the
    /// source, not to repair away on every load. Driven through
    /// [`non_negative_gates`] itself (not a hand-copied list of the keys it
    /// currently returns) so a future reclassification is covered here with
    /// no edit needed.
    #[test]
    fn no_gated_non_negative_weight_is_authored_as_a_downside() {
        for (k, _) in non_negative_gates() {
            assert!(
                k.default_weight() >= 0.0,
                "{} defaults to {}, contradicting its own gate",
                k.name(),
                k.default_weight()
            );
        }
    }

    /// NEGATIVE CONTROL: the guard must not rewrite a vector that is already
    /// legal.
    #[test]
    fn a_legal_vector_is_returned_untouched() {
        let w = Weights::default();
        let (out, viol) = dominance_repair(&w);
        assert_eq!(viol, vec![]);
        assert_eq!(out, w);
    }

    /// Repairing an already-repaired vector changes nothing the second time
    /// -- idempotence.
    #[test]
    fn repairing_twice_changes_nothing_the_second_time() {
        let mut bad = Weights::default();
        bad.set(WeightKey::BlueFree, 9.0);
        bad.set(WeightKey::WonderStagesPerAction, -2.0);
        let (once, v1) = dominance_repair(&bad);
        let (twice, v2) = dominance_repair(&once);
        assert!(!v1.is_empty());
        assert_eq!(v2, vec![]);
        assert_eq!(once, twice);
    }

    /// A climb that prices a wonder's PROMISE above its PAYOFF has, by
    /// construction, made paying a stage a net loss: the two coordinates
    /// split one wonder's value by how much of it is paid for, so buying a
    /// stage moves value from the first term into the second. Repaired by
    /// raising `wonder_potential` to meet it -- the same direction as the
    /// resource pair below, and for the same reason (never discard what the
    /// league measured).
    #[test]
    fn the_climb_may_not_price_a_wonders_promise_above_the_payoff_it_decays_into() {
        let mut w = Weights::default();
        w.set(WeightKey::WonderPotential, 0.2);
        w.set(WeightKey::WonderPromise, 3.0);
        let (out, viol) = dominance_repair(&w);
        assert_eq!(out.get(WeightKey::WonderPotential), 3.0);
        assert_eq!(out.get(WeightKey::WonderPromise), 3.0);
        assert!(
            viol.iter().any(|v| v.weight == WeightKey::WonderPotential),
            "the repair must be reported, got {viol:?}"
        );
        // ... and a vector that already respects the ordering is untouched.
        let mut legal = Weights::default();
        legal.set(WeightKey::WonderPotential, 3.0);
        legal.set(WeightKey::WonderPromise, 0.2);
        assert_eq!(dominance_repair(&legal), (legal, vec![]));
    }

    /// `hand_perishable` counts how much of the hand the next age boundary is
    /// about to throw away, so a positive price on it scores "more of my hand
    /// is about to evaporate" as an improvement. Repaired DOWN to zero, the
    /// same direction and for the same reason as every other non-positive
    /// gate. Driven through [`non_positive_gates`] itself so `HandPerishable`
    /// is checked as a member of it, not as a special case.
    #[test]
    fn a_hand_about_to_expire_may_not_be_priced_as_an_upside() {
        assert!(
            non_positive_gates().any(|(k, _)| k == WeightKey::HandPerishable),
            "HandPerishable must be classified NonPositive by WeightKey::sign_intent, which is \
             what arms both dominance_repair and bin/climb.rs's under-mutation guard"
        );
        let mut w = Weights::default();
        w.set(WeightKey::HandPerishable, 1.5);
        let (out, viol) = dominance_repair(&w);
        assert_eq!(out.get(WeightKey::HandPerishable), 0.0);
        assert!(viol.iter().any(|v| v.weight == WeightKey::HandPerishable), "{viol:?}");
        // A negative price -- "expiry costs me something" -- is the league's
        // to find and must survive untouched.
        let mut priced = Weights::default();
        priced.set(WeightKey::HandPerishable, -1.5);
        assert_eq!(dominance_repair(&priced).0.get(WeightKey::HandPerishable), -1.5);
    }

    /// `wonder_age_overrun` is an unpaid wonder debt like the four
    /// [`WONDER_DEBT_GATES`] it joined, so it inherits the same gate: a
    /// positive price makes paying a stage a loss, which is the exact bug
    /// that produced a 2p champion completing 0 wonders in 400 player-games.
    #[test]
    fn a_wonder_past_its_age_deadline_may_not_be_priced_as_an_upside() {
        let mut w = Weights::default();
        w.set(WeightKey::WonderAgeOverrun, 4.0);
        let (out, viol) = dominance_repair(&w);
        assert_eq!(out.get(WeightKey::WonderAgeOverrun), 0.0);
        assert!(viol.iter().any(|v| v.weight == WeightKey::WonderAgeOverrun), "{viol:?}");
    }

    /// `resource_stock < blue_free` is repaired by RAISING the dominant side
    /// -- the climbed side (`blue_free`) is what was measured and must not
    /// be thrown away.
    #[test]
    fn the_resource_pair_is_repaired_by_raising_the_dominant_side() {
        let mut w = Weights::default();
        w.set(WeightKey::ResourceStock, 0.0);
        w.set(WeightKey::BlueFree, 0.4220);
        let (out, _) = dominance_repair(&w);
        assert_eq!(out.get(WeightKey::ResourceStock), 0.4220);
        assert_eq!(out.get(WeightKey::BlueFree), 0.4220);
    }

    /// LEADERSIGN: `card_board_leader` deeply negative while `card_board_
    /// credit` sits at its legal default of `0.0` is exactly the live 2p
    /// champion's shape (`card_board_credit = 0.0`, `card_board_leader =
    /// -15.003`) -- `BENEFIT_GATES` gates `CardBoardCredit` alone, which
    /// does nothing here because `CardBoardCredit` itself is already legal;
    /// the bug lives entirely in the per-type offset `BENEFIT_GATES` never
    /// looks at. The repair must raise `CardBoardLeader` to `-card_board_
    /// credit` (here, `0.0`) so the EFFECTIVE multiplier `card_potential`
    /// uses lands at exactly the boundary, and it must log a `Violation`
    /// naming `CardBoardLeader`, not `CardBoardCredit` (the innocent term).
    #[test]
    fn a_deeply_negative_per_type_board_credit_is_repaired_even_though_the_base_is_legal() {
        let mut w = Weights::default();
        w.set(WeightKey::CardBoardCredit, 0.0);
        w.set(WeightKey::CardBoardLeader, -15.003_238_920_505_405);
        let (out, viol) = dominance_repair(&w);
        assert_eq!(out.get(WeightKey::CardBoardCredit), 0.0, "the base was already legal, must not move");
        assert_eq!(out.get(WeightKey::CardBoardLeader), 0.0, "raised to -base, i.e. 0.0 here");
        assert!(
            viol.iter().any(|v| v.weight == WeightKey::CardBoardLeader),
            "expected a CardBoardLeader violation, got {viol:?}"
        );
    }

    /// LEADERSIGN, the behavioural half: with the illegal vector above run
    /// straight through `card_potential` (unrepaired), a leader with a
    /// genuinely helpful board swap must price NEGATIVELY -- the inversion
    /// this whole gate exists to close. Once `dominance_repair` is applied,
    /// the same leader on the same board must never price negative again.
    ///
    /// It lands at exactly `0.0`, not some other positive number, and that
    /// is not a weaker guarantee slipped in here -- it is what THIS repair
    /// rule always does when it fires, by the same construction as every
    /// other rule in `dominance_repair`: raising the per-type key to
    /// `-base` makes `base + key` land on exactly the boundary (`0.0`),
    /// same as `BENEFIT_GATES` pinning a negative grant to exactly `0.0`
    /// rather than to some positive replacement, or `DOMINATES` "clamping
    /// to the boundary" per this file's own top doc comment. A `credit_
    /// board` of exactly `0.0` then routes `card_potential` to its static
    /// per-card table (`card_potential_core`'s own comment: every board-
    /// aware branch is gated behind `credit_board != 0.0`), which for a
    /// pure swap-type card like a leader carries no printed static price of
    /// its own -- so `0.0` ("unpriced") is the correct, honest landing
    /// spot, not `0.0` because the fix under-corrected.
    #[test]
    fn the_repaired_vector_no_longer_inverts_a_helpful_leaders_price() {
        use crate::bots::board_yields::{self, Baseline};

        let state = G::new_game(2, 42);
        let baseline = Baseline::at(&state, 0);
        let id = CardId::by_name("Hammurabi").expect("Hammurabi is a real leader");
        let swap = board_yields::board_yields(id, &baseline).expect("a leader is always a swap type");
        let raw_diff = cards::sum_board_triples(&swap, &Weights::default());
        assert!(raw_diff > 0.0, "test needs a genuinely helpful swap to mean anything, got {raw_diff}");

        // The live 2p champion's own shape: `card_board_credit` legal at its
        // `0.0` default, `card_board_leader` deeply negative.
        let mut illegal = Weights::default();
        illegal.set(WeightKey::CardBoardCredit, 0.0);
        illegal.set(WeightKey::CardBoardLeader, -15.003_238_920_505_405);
        let mut scratch = Vec::new();
        let inverted = cards::card_potential(id, &illegal, Some(&baseline), None, &mut scratch);
        assert!(inverted < 0.0, "unrepaired: a helpful leader must price negative to prove the inversion, got {inverted}");

        let (repaired, viol) = dominance_repair(&illegal);
        assert!(viol.iter().any(|v| v.weight == WeightKey::CardBoardLeader), "{viol:?}");
        scratch.clear();
        let fixed = cards::card_potential(id, &repaired, Some(&baseline), None, &mut scratch);
        assert!(fixed >= 0.0, "repaired: a helpful leader must never price negative, got {fixed}");
        assert_eq!(fixed, 0.0, "credit_board lands on exactly the boundary, so this must be the static-table price, which is 0.0 for a pure swap-type card");
    }

    /// NEGATIVE CONTROL: a phase-family coordinate that is NOT gated
    /// `NonNegative`/`NonPositive` (`Workers`'s late-extreme,
    /// `StrengthRel`'s `_early` partner -- only `HandValue`'s own pair
    /// moved to a plain per-key `NonNegative` gate, T1-D, PHASECUT.txt
    /// 2026-08-13) is entitled to a negative value; the guard must not
    /// touch it.
    #[test]
    fn a_phase_multiplier_may_still_go_negative() {
        let mut w = Weights::default();
        w.set(WeightKey::StrengthRelEarly, -9.0);
        w.set(WeightKey::WorkersLate, -9.0);
        let (out, viol) = dominance_repair(&w);
        assert_eq!(out.get(WeightKey::StrengthRelEarly), -9.0);
        assert_eq!(out.get(WeightKey::WorkersLate), -9.0);
        assert_eq!(viol, vec![]);
    }

    /// PHASECUT.txt (T1-D, 2026-08-13): after the collapse, `HandValue`
    /// (the L=0/"start" coefficient) and `HandValueLate` (the L=1/"end"
    /// coefficient) are each gated `NonNegative` INDIVIDUALLY -- the
    /// composite `NET_NONNEG_PHASE` mechanism this replaced (which used to
    /// raise a violating phase partner to exactly `-base`, SIGNAUDIT
    /// instance 3) is gone for `HandValue`; a plain `NonNegative` gate
    /// raises a violating coordinate straight to `0.0`, the same as every
    /// other such gate (`BuildDiscount` etc). Driven from the same
    /// real-world shape the old composite-mechanism test used (`hand_value`
    /// legal, its late-phase partner deeply negative, e.g.
    /// `champ_backup/rust_champion_2p.json`), to show the new mechanism
    /// repairs the identical corpus shape.
    #[test]
    fn a_deeply_negative_hand_value_late_is_repaired_to_zero_even_though_start_is_legal() {
        let mut w = Weights::default();
        w.set(WeightKey::HandValue, 0.2);
        w.set(WeightKey::HandValueLate, -27.683_553_904_987_917);
        let (out, viol) = dominance_repair(&w);
        assert_eq!(out.get(WeightKey::HandValue), 0.2, "the start value was already legal, must not move");
        assert_eq!(out.get(WeightKey::HandValueLate), 0.0, "a NonNegative gate repairs straight to 0.0, not to -start");
        assert!(
            viol.iter().any(|v| v.weight == WeightKey::HandValueLate),
            "expected a HandValueLate violation, got {viol:?}"
        );
    }

    /// The behavioural guarantee the T1-D collapse exists for: `start*(1-L)
    /// + end*L` is a CONVEX combination of `start` and `end`, so once
    /// `dominance_repair` has made both individually `>= 0`, the net
    /// coefficient `evaluate` actually applies to `hand_value` is `>= 0` at
    /// EVERY lateness in `[0,1]`, not merely at the two endpoints a
    /// composite `base + phase >= 0` check would have looked at -- checked
    /// directly at several `L` values here rather than taken on the
    /// convexity argument alone. Same "sits on unused civil actions" bug
    /// shape SIGNAUDIT.txt originally found (a card-filled hand pricing
    /// worse than an empty one in the opening) this now closes for every
    /// lateness, not just `L == 0`.
    #[test]
    fn the_repaired_vector_never_prices_hand_value_negative_at_any_lateness() {
        let illegal = {
            let mut w = Weights::default();
            w.set(WeightKey::HandValue, 0.2);
            w.set(WeightKey::HandValueLate, -27.683_553_904_987_917);
            w
        };
        let (repaired, viol) = dominance_repair(&illegal);
        assert!(viol.iter().any(|v| v.weight == WeightKey::HandValueLate), "{viol:?}");
        let start = repaired.get(WeightKey::HandValue);
        let end = repaired.get(WeightKey::HandValueLate);
        assert!(start >= 0.0 && end >= 0.0, "start={start} end={end}");
        for &late in &[0.0, 0.25, 0.5, 0.75, 1.0] {
            let early = 1.0 - late;
            let net = start * early + end * late;
            assert!(net >= 0.0, "lateness={late}: net={net}");
        }
    }

    /// `BENEFIT_GATES` pins a negative printed-benefit weight at exactly
    /// `0.0`, and reports both violations when two gates are negative at
    /// once.
    #[test]
    fn the_guard_pins_a_negative_grant_at_zero() {
        let mut w = Weights::default();
        w.set(WeightKey::WonderStagesPerAction, -0.5);
        w.set(WeightKey::UnitStrengthCredit, -2.0);
        let (out, viol) = dominance_repair(&w);
        assert_eq!(out.get(WeightKey::WonderStagesPerAction), 0.0);
        assert_eq!(out.get(WeightKey::UnitStrengthCredit), 0.0);
        let weights: std::collections::HashSet<WeightKey> = viol.iter().map(|v| v.weight).collect();
        assert_eq!(
            weights,
            [WeightKey::WonderStagesPerAction, WeightKey::UnitStrengthCredit].into_iter().collect()
        );
    }

    /// NEGATIVE CONTROL: the guard pins a SIGN, not a value -- a positive
    /// grant is left alone at whatever the league priced it.
    #[test]
    fn a_positive_grant_is_left_alone() {
        let mut w = Weights::default();
        w.set(WeightKey::WonderStagesPerAction, 4.25);
        let (out, viol) = dominance_repair(&w);
        assert_eq!(out.get(WeightKey::WonderStagesPerAction), 4.25);
        assert!(viol.iter().all(|v| v.weight != WeightKey::WonderStagesPerAction));
    }

    /// The specific "penalty the rules impose" keys `LOSS_GATES` used to name
    /// as a hand-typed list -- now `WeightKey::sign_intent`'s own
    /// classification (`weights.rs`), pinned here by name so the four tests
    /// below still document the concrete bug they were written for rather
    /// than a generic "whatever `non_positive_gates` happens to return".
    const LOSS_GATES_FOR_TEST: &[WeightKey] =
        &[WeightKey::Discontent, WeightKey::Uprising, WeightKey::StrengthDeficit];

    /// The real drift this gate was written for: every one of these was
    /// authored negative and the league still pushed it positive in at least
    /// one live arm, scoring a rulebook penalty as an upside.
    #[test]
    fn a_penalty_priced_as_a_benefit_is_pinned_back_to_zero() {
        let mut w = Weights::default();
        for &k in LOSS_GATES_FOR_TEST {
            w.set(k, 3.5);
        }
        let (out, viol) = dominance_repair(&w);
        for &k in LOSS_GATES_FOR_TEST {
            assert_eq!(out.get(k), 0.0, "{} must be repaired down to 0.0", k.name());
            assert!(viol.iter().any(|v| v.weight == k), "{} must report a violation", k.name());
        }
    }

    /// NEGATIVE CONTROL: the gate pins a SIGN, not a magnitude -- a penalty
    /// the league priced as costly is left exactly where it put it.
    #[test]
    fn a_penalty_priced_as_a_cost_is_left_alone() {
        let mut w = Weights::default();
        for &k in LOSS_GATES_FOR_TEST {
            w.set(k, -7.25);
        }
        let (out, viol) = dominance_repair(&w);
        for &k in LOSS_GATES_FOR_TEST {
            assert_eq!(out.get(k), -7.25, "{} must keep its negative price", k.name());
            assert!(viol.iter().all(|v| v.weight != k), "{} must not report a violation", k.name());
        }
    }

    /// Every gated key must be a coordinate that is only ever set to a
    /// NON-NEGATIVE magnitude, or "weight <= 0" would not mean "a penalty
    /// cannot help" -- the gate's whole justification is the feature's sign
    /// convention, so pin the convention here rather than trusting a comment.
    #[test]
    fn every_loss_gate_defaults_to_a_negative_price() {
        for &k in LOSS_GATES_FOR_TEST {
            assert!(
                k.default_weight() < 0.0,
                "{} is gated as a penalty but its authored default is {}, which is not a cost",
                k.name(),
                k.default_weight()
            );
        }
    }

    // ------------------------------------------------------------------ io

    /// HEADLINE TEST (PHASECUT.txt, T1-A/C/D collapse, 2026-08-13): loading
    /// a LEGACY-format champion file -- one still carrying
    /// `workers_early`/`tech_levels_early`/`hand_value_early`, retired as
    /// live `WeightKey` variants by this collapse -- must reproduce EXACTLY
    /// the same per-feature contribution the OLD 3-parameter blend
    /// `w[base] + (1-L)*w[early] + L*w[late]` gave, at every lateness `L`
    /// in `[0,1]`, not merely at the two endpoints the new `{start,end}`
    /// basis is literally built from. This is the correctness obligation
    /// this whole collapse rests on: every weight file on disk must produce
    /// bit-identical play, and this is the algebraic half of that proof
    /// (the empirical half -- arena self-play A-vs-A on the 5 real
    /// champion/gauntlet files, before and after this change -- is recorded
    /// in PHASECUT.txt, not as a `cargo test`).
    ///
    /// Uses the project's own OLD authored defaults as the legacy vector
    /// (`workers`/`workers_early`/`workers_late` = `1.4`/`0.8`/`-0.6`, and
    /// so on) so the same assertion doubles as a cross-check that the NEW
    /// authored defaults (`Weights::defaults()`'s `Workers = 2.2`,
    /// `WorkersLate = 0.8`, ...) are exactly the fold of the old ones, not
    /// independently retyped numbers that happen to be close.
    ///
    /// Reverting the fold in `parse_weights` (making `LEGACY_PHASE_EARLY_
    /// FOLD` a no-op) turns this test RED -- confirmed by hand while
    /// developing this fix (see PHASECUT.txt's RED-confirmation section)
    /// and left here as the permanent regression pin.
    #[test]
    fn parse_weights_folds_a_legacy_phase_triple_into_the_same_curve_at_every_lateness() {
        let legacy = r#"{
          "workers": 1.4, "workers_early": 0.8, "workers_late": -0.6,
          "tech_levels": 1.0, "tech_levels_early": 0.5, "tech_levels_late": -0.4,
          "hand_value": 0.25, "hand_value_early": 0.2, "hand_value_late": -0.2
        }"#;
        let w = parse_weights(legacy).expect("a legacy phase triple must still parse, not be rejected as unknown");

        for &(base_v, early_v, late_v, key) in &[
            (1.4_f64, 0.8_f64, -0.6_f64, WeightKey::Workers),
            (1.0, 0.5, -0.4, WeightKey::TechLevels),
            (0.25, 0.2, -0.2, WeightKey::HandValue),
        ] {
            let start = w.get(key);
            let end = w.get(key.late());
            for &late in &[0.0_f64, 0.25, 0.5, 0.75, 1.0] {
                let early = 1.0 - late;
                let old_formula = base_v + early * early_v + late * late_v;
                let new_formula = start * early + end * late;
                assert!(
                    (old_formula - new_formula).abs() < 1e-12,
                    "{}: at lateness {late}, old={old_formula} new={new_formula} (start={start} end={end})",
                    key.name()
                );
            }
        }
    }

    #[test]
    fn a_champion_round_trips_through_text() {
        let mut w = Weights::defaults();
        w.set(WeightKey::Culture, 1.75);
        w.set(WeightKey::Science, -0.125);
        let back = parse_weights(&weights_json(&w, &[("gen", 41.0)])).unwrap();
        for &k in WeightKey::ALL {
            assert_eq!(back.get(k), w.get(k), "{} did not round-trip", k.name());
        }
    }

    /// The wrapper the trainer writes and a bare `{name: value}` map are both
    /// champion files in the wild; Python's `d.get("weights", d)` reads
    /// either, and so must this.
    #[test]
    fn both_the_wrapped_and_the_bare_shape_load() {
        let wrapped = parse_weights(r#"{"gen": 3, "weights": {"culture": 2.5}}"#).unwrap();
        let bare = parse_weights(r#"{"culture": 2.5}"#).unwrap();
        assert_eq!(wrapped.get(WeightKey::Culture), 2.5);
        assert_eq!(bare.get(WeightKey::Culture), 2.5);
    }

    /// A key the file leaves out keeps its default rather than reading as
    /// zero -- the whole reason the loader starts from `Weights::defaults()`.
    #[test]
    fn an_absent_key_keeps_its_default() {
        let w = parse_weights(r#"{"culture": 2.5}"#).unwrap();
        assert_eq!(w.get(WeightKey::Science), WeightKey::Science.default_weight());
    }

    /// The live league champions, committed before this batch's marginal-need
    /// and redundancy keys existed, must still load without error -- and
    /// every one of those new keys must land at exactly its 0.0 default,
    /// since none of the sixteen new names appear in a file written before
    /// they existed. This is the "the climb restarts on this and the
    /// baseline must not move silently" guarantee from this batch's own
    /// brief: a champion loaded today must evaluate byte-identically to how
    /// it did before this batch landed, because every new coordinate
    /// contributes exactly `0.0 * feature == 0.0` to `evaluate`'s dot
    /// product until the league discovers otherwise.
    ///
    /// Gitignored, regenerated-only files (`docs/RUST_LEAGUE.md`) -- skipped
    /// rather than failed when a fresh checkout has not produced one yet, the
    /// same reasoning `advisor::load_bot`'s own fallback-to-defaults uses for
    /// a missing champion.
    #[test]
    fn the_live_champions_load_unchanged_with_every_new_key_at_its_zero_default() {
        let new_keys = [
            WeightKey::FoodGap,
            WeightKey::FoodSurplus,
            WeightKey::ResourceGap,
            WeightKey::ResourceSurplus,
            WeightKey::ScienceGap,
            WeightKey::ScienceSurplus,
            WeightKey::CultureGap,
            WeightKey::CultureSurplus,
            WeightKey::HappySurplus,
            WeightKey::CivilActionGap,
            WeightKey::CivilActionSurplus,
            WeightKey::MilitaryActionGap,
            WeightKey::MilitaryActionSurplus,
            WeightKey::WorkerGap,
            WeightKey::WorkerSurplus,
            WeightKey::TechRedundancyDiscount,
        ];
        // A FROZEN champion, never the live `experiments/rust_champion_*.json`:
        // those are rewritten by the running climb every time it accepts, so
        // once the climb has priced these keys the live files legitimately
        // carry non-zero values and a test reading them can only ever go red.
        // The property under test is about loading a file written BEFORE the
        // keys existed, so the fixture must be one that can never change.
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../analysis/frozen/gauntlet")
            .join("champion_3p_gen1384_140key_2026-08-06.json");
        let w = load_weights(&path).unwrap_or_else(|e| panic!("frozen champion failed to load: {e}"));
        for &k in &new_keys {
            assert_eq!(k.default_weight(), 0.0, "{} must default to 0.0", k.name());
            assert_eq!(
                w.get(k),
                k.default_weight(),
                "frozen champion: {} must come back as its 0.0 default, a file written before the key existed cannot name it",
                k.name()
            );
        }
    }

    /// The same guarantee for the four coordinates added for the wonder
    /// rank-deficiency (`wonder_promise`, `wonder_age_overrun`,
    /// `take_cost_share`, `hand_perishable`), checked against EVERY frozen
    /// gauntlet member rather than one of them -- the gauntlet is the ladder
    /// a candidate is judged against (docs/RUST_LEAGUE.md), so a member whose
    /// play changed the day a coordinate was added would silently move the
    /// ruler along with the thing being measured.
    ///
    /// The load path is what makes this hold: [`parse_weights`] starts from
    /// [`Weights::defaults`] and overwrites only the names the file actually
    /// carries, so a key that did not exist when the file was written keeps
    /// its default -- and every one of these four defaults to exactly 0.0, so
    /// its contribution to [`evaluate`]'s dot product is `0.0 * feature`.
    /// Seeding any of them nonzero would move every file below at once, which
    /// is precisely what this test refuses.
    #[test]
    fn a_champion_file_saved_before_these_keys_existed_still_loads_with_them_at_zero() {
        let new_keys = [
            WeightKey::WonderPromise,
            WeightKey::WonderAgeOverrun,
            WeightKey::TakeCostShare,
            WeightKey::HandPerishable,
        ];
        for &k in &new_keys {
            assert_eq!(
                k.default_weight(),
                0.0,
                "{} must default to 0.0: every champion file on disk predates it and would \
                 silently inherit anything else written here",
                k.name()
            );
        }

        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../analysis/frozen/gauntlet");
        let mut checked = 0usize;
        for entry in std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("reading {}: {e}", dir.display())) {
            let path = entry.expect("a readable directory entry").path();
            if path.extension().is_none_or(|e| e != "json") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
            let w = parse_weights(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            for &k in &new_keys {
                assert_eq!(
                    w.get(k),
                    0.0,
                    "{}: {} must come back at 0.0 -- a file written before the key existed cannot \
                     name it, so anything else here means the DEFAULT moved",
                    path.display(),
                    k.name()
                );
            }
            // Not vacuous: the file really is narrower than the vector it is
            // being loaded into, which is the whole situation under test.
            for &k in &new_keys {
                assert!(
                    !text.contains(k.name()),
                    "{}: names {}, so this file does NOT predate the key and cannot stand in for \
                     one that does",
                    path.display(),
                    k.name()
                );
            }
            checked += 1;
        }
        assert!(checked >= 6, "expected the frozen gauntlet's six members, found {checked}");
    }

    /// The same guarantee, for `leader_replacement` and
    /// `wonder_pool_rival_claimed` -- the two coordinates this task adds.
    /// Both default to 0.0 (`weights.rs`'s `weight_keys!` table), so by the
    /// same `parse_weights`-starts-from-`Weights::defaults` mechanism the
    /// sibling test above exercises, every champion already on disk --
    /// including everything under `analysis/frozen/`, which is FROZEN AND
    /// APPEND-ONLY -- must come back with both new keys at exactly 0.0 and
    /// must therefore evaluate every position bit-identically to how it did
    /// before this task landed.
    #[test]
    fn a_champion_file_saved_before_leader_replacement_and_wonder_pool_rival_claimed_existed_still_loads_with_them_at_zero(
    ) {
        let new_keys = [WeightKey::LeaderReplacement, WeightKey::WonderPoolRivalClaimed];
        for &k in &new_keys {
            assert_eq!(
                k.default_weight(),
                0.0,
                "{} must default to 0.0: every champion file on disk predates it and would \
                 silently inherit anything else written here",
                k.name()
            );
        }

        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../analysis/frozen/gauntlet");
        let mut checked = 0usize;
        for entry in std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("reading {}: {e}", dir.display())) {
            let path = entry.expect("a readable directory entry").path();
            if path.extension().is_none_or(|e| e != "json") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
            let w = parse_weights(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            for &k in &new_keys {
                assert_eq!(
                    w.get(k),
                    0.0,
                    "{}: {} must come back at 0.0 -- a file written before the key existed cannot \
                     name it, so anything else here means the DEFAULT moved",
                    path.display(),
                    k.name()
                );
            }
            // Not vacuous: the file really is narrower than the vector it is
            // being loaded into, which is the whole situation under test.
            for &k in &new_keys {
                assert!(
                    !text.contains(k.name()),
                    "{}: names {}, so this file does NOT predate the key and cannot stand in for \
                     one that does",
                    path.display(),
                    k.name()
                );
            }
            checked += 1;
        }
        assert!(checked >= 6, "expected the frozen gauntlet's six members, found {checked}");
    }

    /// Every champion on disk still carries the twelve retired names. They
    /// are expected, so they are dropped in silence -- not rejected.
    #[test]
    fn retired_keys_load_without_complaint() {
        let text = format!(r#"{{"{}": 9.0, "culture": 2.5}}"#, RETIRED_KEYS[0]);
        let w = parse_weights(&text).unwrap();
        assert_eq!(w.get(WeightKey::Culture), 2.5);
    }

    /// The one deliberate divergence from Python: a name nothing reads is a
    /// typo, and a typo that loads silently costs you the weight you meant
    /// to set. See this module's top doc comment.
    #[test]
    fn an_unknown_key_is_an_error_rather_than_a_silent_no_op() {
        let err = parse_weights(r#"{"cultrue": 2.5}"#).unwrap_err();
        assert!(err.contains("cultrue"), "{err}");
    }

    /// The repair is applied on load and not only in the trainer, because a
    /// champion file is read by the arena and by every tool as well.
    #[test]
    fn loading_repairs_a_rule_level_violation() {
        let (key, _) = non_negative_gates().next().expect("at least one NonNegative-classified key exists");
        let text = format!(r#"{{"{}": -3.0}}"#, key.name());
        assert_eq!(parse_weights(&text).unwrap().get(key), 0.0);
    }

    #[test]
    fn a_non_number_weight_is_an_error() {
        assert!(parse_weights(r#"{"culture": "2.5"}"#).is_err());
        assert!(parse_weights(r#"[1, 2]"#).is_err());
    }

    /// `save_weights` writes through a temporary and renames; both the
    /// rename and the round-trip through a real file are worth one test.
    #[test]
    fn save_then_load_a_real_file() {
        let dir = std::env::temp_dir().join("tta_weights_io_test");
        let path = dir.join("champion.json");
        let _ = std::fs::remove_dir_all(&dir);
        let mut w = Weights::defaults();
        w.set(WeightKey::CivilActions, 3.5);
        save_weights(&path, &w, &[("gen", 7.0), ("players", 3.0)]).unwrap();
        assert!(!path.with_extension("tmp").exists(), "temp file was left behind");
        let back = load_weights(&path).unwrap();
        assert_eq!(back.get(WeightKey::CivilActions), 3.5);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"gen\": 7"), "{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `choose`'s 1-ply loop must score every candidate at the same point in
    /// time. `EndTurn` is the only move whose apply arm reaches
    /// `economy::end_of_turn`, so applying it would score passing on a board
    /// that already held this turn's production while every rival candidate
    /// still carried the full negative `food_gap`/`resource_gap` -- a weight
    /// vector penalising those two gaps would then make passing look like it
    /// produced.
    ///
    /// The fixture is the first hit of a plain `0..5000` seed scan, not a
    /// hand-built position: `legal_moves` only ever offers a `Take` slot the
    /// player can already pay for (`legal.rs`'s `can_take_gated`), so the
    /// asserted `Take` is affordable by construction. `end_turn_bias` is
    /// pinned to 0.0 -- its -3.0 default is a separate pass-discouragement,
    /// and leaving it in would let the test pass for the wrong reason.
    #[test]
    fn weighted_bot_does_not_end_turn_with_an_affordable_take_and_a_civil_action_left_when_food_and_resource_gaps_are_heavily_penalized() {
        let state = G::new_game(2, 0);
        assert_eq!(state.decider(), 0, "fixture assumption: seat 0 decides seed 0's opening move");
        assert_eq!(state.round, 1, "fixture assumption: an early round, before any real production has happened");
        assert!(state.players[0].civil_actions > 0, "fixture assumption: a civil action must still be available to spend");

        let movelist = crate::legal::legal_moves(&state);
        let moves = movelist.as_slice();
        assert!(moves.contains(&Move::EndTurn), "fixture assumption: EndTurn must be offered so the bot has to choose against it");
        assert!(
            moves.iter().any(|m| matches!(m, Move::Take { .. })),
            "fixture assumption: a legal (therefore affordable) Take must be on offer, {moves:?}"
        );

        let mut w = Weights::default();
        w.set(WeightKey::FoodGap, -100.0);
        w.set(WeightKey::ResourceGap, -100.0);
        w.set(WeightKey::EndTurnBias, 0.0);

        let bot = WeightedBot::new(w);
        let chosen = bot.choose(&state, moves);
        assert!(
            !matches!(chosen, Move::EndTurn),
            "a heavy food/resource-gap penalty must not make passing with a civil action \
             and an affordable Take in hand look better than actually taking it, got {chosen:?}"
        );
    }
}
