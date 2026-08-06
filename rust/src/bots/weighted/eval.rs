//! `engine/bots/weighted.py` lines 4104-end: `evaluate` (the linear
//! evaluation entry point), `WeightedBot` (the 1-ply bot built on it), and
//! the dominance guard (`dominance_repair` and its three rule tables).
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
use crate::moves::{Move, MoveList};
use crate::state::GameState;

use super::super::plan;
use super::cards;
use super::events;
use super::features::{self, Features};
use super::horizon;
use super::rivals::{self, RivalContext};
use super::row;
use super::weights::{Weights, WeightKey, PHASE_KEYS, RETIRED_KEYS};

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
    for &k in WeightKey::ALL {
        let wk = w.get(k);
        if wk == 0.0 {
            continue;
        }
        let v = f.get(k);
        let scale = if hz != 1.0 && horizon::RATE_KEYS.contains(&k) { hz } else { 1.0 };
        total += wk * v * scale;
    }

    // The phase-blended body: `w[k] + (1 - L) * w[k_early] + L * w[k_late]`,
    // for the four [`PHASE_KEYS`]. The phase pair carries the same rate
    // horizon as the base term -- see [`super::rivals::feature_marginal`],
    // which sums exactly these three for a card pricer.
    let late = horizon::lateness(state);
    let early = 1.0 - late;
    for &k in PHASE_KEYS {
        let v = f.get(k);
        if v == 0.0 {
            continue;
        }
        let scale = if hz != 1.0 && horizon::RATE_KEYS.contains(&k) { hz } else { 1.0 };
        let vv = v * scale;
        let we = w.get(k.early());
        if we != 0.0 {
            total += we * early * vv;
        }
        let wl = w.get(k.late());
        if wl != 0.0 {
            total += wl * late * vv;
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

impl Default for WeightedBot {
    fn default() -> Self {
        WeightedBot { weights: Weights::default(), allow_resign: false }
    }
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
    /// (`docs/COMBAT_AUDIT.md`).
    ///
    /// # Panics
    /// If `moves` is empty (a caller bug -- a live game's `legal_moves` never
    /// returns one, matching `BookBot::choose`'s identical contract).
    pub fn choose(&self, state: &GameState, moves: &[Move]) -> Move {
        let mut filtered = MoveList::new();
        if !self.allow_resign && moves.len() > 1 {
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
            apply::apply(&mut trial, mv);
            let mut val = evaluate(&trial, idx, w, Some(&ctx), None);
            if matches!(mv, Move::EndTurn) {
                // DO NOT "fix" this asymmetry -- scoring `end_turn` on the
                // unmoved trial, with this bias added, was measured (twice,
                // two different ways) against every alternative and is
                // strictly stronger; see `weighted.py`'s own extensive note
                // immediately above `"end_turn_bias"` in `BASE_WEIGHTS`
                // (not reproduced here) for the exact A/B numbers.
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
/// since 2026-08-04 -- both entries that used to live here (`culture`,
/// `wonder_progress`) have since had their phase pair deleted outright
/// ([`PHASE_KEYS`] no longer lists either), so there is no multiplier left to
/// drag their net weight below zero. Kept, and empty, because the next
/// phase-multiplied stock -- if anyone adds one -- needs this loop; deleting
/// it would delete the argument with it. See Python's own comment on this
/// constant for the full citation trail.
///
/// Python's own test of this (empty, currently unexercised) branch drives it
/// with `mock.patch.object(weighted, "NET_NONNEG_PHASE", ("culture",))` --
/// there is no monkeypatching a `const` in Rust, so that specific test has no
/// direct port here; the branch itself is still exactly the code the next
/// phase-multiplied stock would land on.
pub const NET_NONNEG_PHASE: &[WeightKey] = &[];

/// `(dominant, dominated)` -- `w[dominant] >= w[dominated]`, repaired by
/// raising the dominant side. A resource in stock dominates the blue token it
/// sits on: spending the resource hands the token back to the bank AND buys
/// what it paid for, so a stocked resource is worth at least a free token
/// whatever either is worth.
pub const DOMINATES: &[(WeightKey, WeightKey)] = &[(WeightKey::ResourceStock, WeightKey::BlueFree)];

/// Weights that scale a PRINTED BENEFIT on one card class and nothing else --
/// none of them may be negative. Each is the ONLY per-card channel its class
/// has, and raising it raises `card_potential` for every card in the class;
/// you are never compelled to use a grant, so under the rules a card that
/// prints one is never worse than the same card without it. The repair is to
/// `0.0`, not upward: the rules say "not a cost", they do not say what it is
/// worth -- pricing it is the league's job.
pub const BENEFIT_GATES: &[WeightKey] = &[
    WeightKey::BuildDiscount,
    WeightKey::CardBoardCredit,
    WeightKey::DefenseBonus,
    WeightKey::FreeCivilAction,
    WeightKey::HandLimit,
    WeightKey::ResourceDiscount,
    WeightKey::RestrictedResources,
    WeightKey::UnitStrengthCredit,
    WeightKey::WonderStagesPerAction,
];

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

    for &k in BENEFIT_GATES {
        let v = out.get(k);
        if v < -1e-12 {
            viol.push(Violation {
                weight: k,
                value: v,
                default: k.default_weight(),
                rule: format!("{} >= 0 (scales a printed benefit)", k.name()),
            });
            out.set(k, 0.0);
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
/// are dropped, unknown keys are an error, and [`dominance_repair`] is
/// applied on the way out -- see this module's top doc comment for why the
/// repair belongs here rather than only in the trainer.
pub fn parse_weights(text: &str) -> Result<Weights, String> {
    let doc = crate::fixtures::parse_json(text).map_err(|e| format!("{e:?}"))?;
    let map = match doc.get("weights") {
        Some(w) => w,
        None => &doc,
    };
    let fields = match map {
        crate::fixtures::Json::Obj(fields) => fields,
        _ => return Err("champion JSON is not an object".to_string()),
    };

    let mut w = Weights::defaults();
    for (name, value) in fields {
        if RETIRED_KEYS.contains(&name.as_str()) {
            continue;
        }
        let key = WeightKey::by_name(name)
            .ok_or_else(|| format!("unknown weight {name:?}"))?;
        let v = value
            .as_f64()
            .ok_or_else(|| format!("weight {name:?} is not a number"))?;
        if !v.is_finite() {
            return Err(format!("weight {name:?} is not finite"));
        }
        w.set(key, v);
    }
    Ok(dominance_repair(&w).0)
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

    // ---------------------------------------------------------- dominance

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

    /// NEGATIVE CONTROL: a phase multiplier outside `NET_NONNEG_PHASE` (all
    /// of them, currently -- it is empty) is entitled to a negative net; the
    /// guard must not touch it.
    #[test]
    fn a_phase_multiplier_may_still_go_negative() {
        let mut w = Weights::default();
        w.set(WeightKey::WorkersEarly, -9.0);
        w.set(WeightKey::TechLevelsLate, -9.0);
        let (out, viol) = dominance_repair(&w);
        assert_eq!(out.get(WeightKey::WorkersEarly), -9.0);
        assert_eq!(out.get(WeightKey::TechLevelsLate), -9.0);
        assert_eq!(viol, vec![]);
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

    // ------------------------------------------------------------------ io

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
        let key = BENEFIT_GATES[0];
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
}
