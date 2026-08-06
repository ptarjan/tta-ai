//! Quiescence search: resolve `state.pending` before scoring a candidate move.
//!
//! Ports `engine/bots/quiescent.py` (505 lines) -- read that file's own module
//! doc comment first; it is the design rationale for everything below and is
//! restated here only where the Rust shape earns its own note.
//!
//! Why this exists (short version, full version in the Python doc comment):
//! a search that scores a candidate by applying it to a trial and evaluating
//! immediately is correct only when the WHOLE effect of the move lands in the
//! trial state. A `pact`, `aggression`, `bid` or action-card move instead
//! leaves the trial showing the entire cost (a card gone, actions spent) and
//! none of the gain (a pending decision for the mover or a rival), so a
//! 1-ply evaluator ranks it as strictly dominated by passing under any
//! weight vector. Quiescence is the fix: after applying a candidate, keep
//! resolving `state.pending` -- whoever the decider is -- until the stack is
//! empty, and only then score. `docs/COMBAT_AUDIT.md` proves this for pacts
//! and colony bids; the same argument covers aggressions and action cards.
//!
//! ## This port is generic over the evaluator, deliberately
//!
//! `bots/` has no search bot yet (`weighted.rs` -- `evaluate`/`features`/
//! `rival_context`, ~4,100 lines of Python -- is not ported), so unlike
//! Python's `QuiescentBot`, which calls `weighted.evaluate` by name, every
//! function here takes the scorer as `eval: &impl Fn(&GameState, u8) -> f64`
//! and never constructs one itself. This is the same shape `pending.rs`
//! chose for its `plain`/`quiet` closures: the POLICY (when to resolve, how
//! deep, under what budget, how a war-in-progress prices) lives here, once,
//! so the first Rust search bot inherits it correctly rather than reinventing
//! it; the SCORING stays wherever `weighted.rs` eventually lands. Exactly
//! like `counting.rs`'s "CALLERS MUST ROOT-CACHE" contract, a future caller's
//! `eval` closure is responsible for reading fresh information off the
//! `&GameState` it is handed each call -- there is no separate "recompute
//! stale context after resolving" step here because the closure always sees
//! the CURRENT trial, not a snapshot taken before resolution ran.
//!
//! ## `engine/bots/trial.py` (78 lines): NOT ported
//!
//! See `bots/mod.rs`'s module doc comment for the full verdict and evidence.
//! Short version: `trial.py`'s whole job is handing `actions.apply(state, mv,
//! rng)` a freshly seeded `Random(0)` per candidate, cheaply, by reusing a
//! pooled object that is reset only when actually drawn from. This port's
//! [`crate::apply::apply`] takes no rng parameter at all -- `game.rs`
//! derives one deterministically from `state.seed`/`state.turn`/
//! `state.round` instead -- so there is no call site anywhere in this file
//! that could receive an injected stream, and nothing here threads one.
//!
//! ## Levels
//!
//! [`QuiescenceConfig::levels`] says how many nested quiescence resolutions
//! the search performs.
//!
//! `levels = 1` (the default, matching Python's `LEVELS = 1`): the root's
//! candidates are resolved to quiet; the rivals' decisions inside that
//! resolution are answered with a plain 1-ply pick, i.e. the current
//! champion. In self-play that is not an approximation of the opponent, it
//! *is* the opponent.
//!
//! `levels = 2`: the rivals' decisions inside the resolution are themselves
//! resolved to quiet. This matters specifically for a rival deciding whether
//! to raise a colony bid, which is blind in exactly the way described above
//! at 1 ply.
//!
//! `levels = 0` is a plain 1-ply evaluator and exists for A/B control.
//!
//! ## Cost
//!
//! [`resolve`] runs only when a candidate move leaves the stack non-empty;
//! the overwhelming majority of moves (build, take, pop, upgrade, end_turn)
//! resolve inside `apply` and cost exactly what they cost without this
//! module. The expensive shapes are auctions, whose resolution is itself a
//! chain of rival bid decisions. Two budgets keep that bounded:
//!
//! [`QuiescenceConfig::max_depth`]  pending decisions resolved per candidate move
//! [`QuiescenceConfig::max_nodes`]  total `apply` calls spent on quiescence per root decision, SHARED across every candidate in one [`pick`] call
//!
//! When a budget is exhausted the stack is left as it is and the position is
//! scored as it stands -- a budget miss degrades to a 1-ply score of an
//! unresolved position, not to a crash ([`resolve`] returns `false` rather
//! than panicking or looping forever).
//!
//! ## War
//!
//! A war declaration is NOT fixed by quiescence: `Move::War` pushes nothing
//! onto `state.pending` (it just records a declaration) and resolves at the
//! start of the declarer's *next* turn ([`crate::game::start_turn`] ->
//! [`crate::combat::resolve_war_outcome`]), a full round away. [`war_value`]
//! handles it separately by resolving the war on a scratch copy through the
//! engine's own combat resolution, which is a pure deterministic function of
//! the two players' current strengths -- a real (if optimistic -- the
//! defender gets a turn in between) resolution, not a hand-priced guess.

use crate::apply;
use crate::combat;
use crate::interact;
use crate::moves::Move;
use crate::state::GameState;

// ------------------------------------------------------------ configuration

/// Search-shape knobs, mirroring `QuiescentBot`'s class attributes.
///
/// `end_bias` is the one field with no Python analogue at this layer: Python
/// reads `weights.get("end_turn_bias", 0.0)` out of the evaluator's own
/// weight vector at call time rather than carrying it as a bot constant.
/// Since this port has no weight vector to read it from yet, it is threaded
/// in here explicitly; a future `weighted.rs`-backed caller should set it
/// per call from its own weights rather than treating `0.0` as permanent.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QuiescenceConfig {
    /// Nested quiescence resolutions. See this module's top doc comment.
    pub levels: u32,
    /// Pending decisions resolved per candidate move.
    pub max_depth: u32,
    /// Total quiescence `apply` calls per root decision (shared across every
    /// candidate [`pick`] considers).
    pub max_nodes: i64,
    /// Resolve a declared-but-unfought war through the engine before scoring.
    pub war_lookahead: bool,
    /// Added to a candidate's score when the candidate is `Move::EndTurn`.
    pub end_bias: f64,
}

impl Default for QuiescenceConfig {
    fn default() -> Self {
        QuiescenceConfig { levels: 1, max_depth: 12, max_nodes: 600, war_lookahead: true, end_bias: 0.0 }
    }
}

/// Instrumentation, mirroring `QuiescentBot.stats`. A future caller
/// (`census.rs`, not ported -- `engine/bots/census.py` has no Rust
/// counterpart yet either) reads this the way `docs/DEEPER_SEARCH.md`'s
/// Python measurements read `bot.stats`: to prove quiescence actually fired
/// rather than trusting that it did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub decisions: u64,
    pub candidates: u64,
    pub quiesced: u64,
    pub qnodes: u64,
    pub truncated: u64,
}

// ------------------------------------------------------------- the search

/// Play out `state.pending` until the position is quiet.
///
/// Returns `true` if it reached a quiet position, `false` if a budget ran
/// out. Mirrors `_resolve`: budget checked BEFORE each step (so a budget
/// that is already exhausted resolves zero further decisions rather than
/// one-too-many), `nodes_left` decremented once per decision resolved and
/// shared across the whole call tree via `&mut`, exactly as Python's `box`
/// (a one-element list every level decrements) is shared so "a single
/// pathological auction cannot make one root decision cost unboundedly more
/// than another."
pub fn resolve<E>(state: &mut GameState, level: u32, nodes_left: &mut i64, max_depth: u32, eval: &E) -> bool
where
    E: Fn(&GameState, u8) -> f64,
{
    let mut n = 0u32;
    while !state.pending.is_empty() {
        if n >= max_depth || *nodes_left <= 0 || state.game_over {
            return false;
        }
        let idx = state.decider();
        let moves = crate::legal::legal_moves(state);
        if moves.is_empty() {
            return false;
        }
        let mv = pick_one(state, moves.as_slice(), idx, level, nodes_left, max_depth, 0.0, eval);
        apply::apply(state, mv);
        n += 1;
        *nodes_left -= 1;
    }
    true
}

/// Best move for `idx` at `level`: a plain 1-ply pick when `level == 0`,
/// otherwise each candidate is itself resolved to quiet before scoring.
///
/// Mirrors `_pick` -- the helper `_resolve` calls to answer WHOEVER the
/// decider is, rival or self, inside a resolution. Deliberately does not
/// apply the war lookahead: `war_value` is [`pick`]'s (the root's) call to
/// make, exactly as only `QuiescentBot.pick`/`_pick_journalled` call
/// `_war_value` in Python and the module-level `_pick` helper never does.
///
/// No `try`/`except`-shaped fallback for a candidate whose `apply` fails:
/// Python defensively `continue`s past an exception per candidate and falls
/// back to a random legal move if every one of them raised. This port lets
/// [`crate::apply::apply`] panic on an invariant violation instead, matching
/// this codebase's fail-loud convention elsewhere (e.g. `counting.rs::
/// homer()`) rather than silently masking a legality bug in the caller.
fn pick_one<E>(
    state: &GameState,
    moves: &[Move],
    idx: u8,
    level: u32,
    nodes_left: &mut i64,
    max_depth: u32,
    end_bias: f64,
    eval: &E,
) -> Move
where
    E: Fn(&GameState, u8) -> f64,
{
    if moves.len() == 1 {
        return moves[0];
    }
    let mut best: Option<(Move, f64)> = None;
    for &mv in moves {
        let mut trial = state.clone();
        apply::apply(&mut trial, mv);
        if level > 0 && !trial.pending.is_empty() {
            resolve(&mut trial, level - 1, nodes_left, max_depth, eval);
        }
        let mut val = eval(&trial, idx);
        if matches!(mv, Move::EndTurn) {
            val += end_bias;
        }
        if best.is_none_or(|(_, bv)| val > bv) {
            best = Some((mv, val));
        }
    }
    best.map(|(m, _)| m).unwrap_or(moves[0])
}

/// Value of the position with `idx`'s pending war already fought.
///
/// [`crate::combat::resolve_war_outcome`] is deterministic and consumes no
/// rng, so this is a real (if optimistic -- the defender gets a turn in
/// between) resolution rather than a priced guess. `state` is never
/// mutated; the resolution happens on a scratch clone, so a caller may score
/// the same position with and without it.
///
/// Python wraps this whole function in `try/except Exception: return None`
/// and its callers treat `None` as "keep the un-looked-ahead score." Worked
/// through: neither branch this function actually takes -- no war declared
/// (`events.resolve_war`'s `if not war: return`) nor a dead tie
/// (`resolve_war_outcome`'s own `a == d` early return, AFTER the card is
/// discarded and the tracking fields are cleared) -- raises in Python
/// either; both fall through to `evaluate` and return a real float. The
/// `except` guards only a genuine engine bug, which this port lets panic
/// instead of swallowing (see [`pick_one`]'s doc comment), so this returns
/// `f64` directly rather than `Option<f64>`: there is no longer a benign
/// case that produces `None`, and returning one would have made a real tie
/// -- fields cleared, card discarded, no spoils -- silently fall back to
/// the un-fought score instead of being priced as the (spoils-free) fought
/// position Python actually prices it as.
///
/// `resolve_war_outcome` is no longer total: "War over Technology" leaves
/// the victor a decision (steal blue technologies instead of science, Code
/// of Laws p.3), so [`crate::interact::settle_war_spoils`] settles the
/// spoils -- preferring the priciest affordable steal over science -- before
/// scoring. That is still an approximation rather than the real chooser's
/// preference, but it is the ceiling `war_tech_options` can offer for the
/// budget rather than the floor -- see that function's own doc comment.
pub fn war_value<E>(state: &GameState, idx: u8, eval: &E) -> f64
where
    E: Fn(&GameState, u8) -> f64,
{
    let mut scratch = state.clone();
    if let Some(outcome) = combat::resolve_war_outcome(&mut scratch, idx) {
        combat::apply_war_spoils(&mut scratch, &outcome);
    }
    interact::settle_war_spoils(&mut scratch);
    eval(&scratch, idx)
}

/// Best move for `idx` at the root: resolve each candidate to quiescence
/// (per [`QuiescenceConfig::levels`]), price a declared war through
/// [`war_value`] when configured to, and apply [`QuiescenceConfig::end_bias`]
/// to `Move::EndTurn`.
///
/// Mirrors `QuiescentBot.pick`'s copy-path candidate loop (Python's
/// journalled twin, `_pick_journalled`, has no Rust counterpart -- see
/// `bots/mod.rs`'s `trial.py` verdict: there is no undo-stack search here to
/// journal against). `stats` accumulates exactly what `QuiescentBot.stats`
/// does; `census.record` (Python's per-decision instrumentation hook) is not
/// called because `census.py` has no Rust port yet either.
pub fn pick<E>(cfg: &QuiescenceConfig, stats: &mut Stats, state: &GameState, moves: &[Move], eval: &E) -> Move
where
    E: Fn(&GameState, u8) -> f64,
{
    if moves.len() == 1 {
        return moves[0];
    }
    let idx = state.decider();
    stats.decisions += 1;
    let mut nodes_left = cfg.max_nodes;
    let mut best: Option<(Move, f64)> = None;
    for &mv in moves {
        stats.candidates += 1;
        let mut trial = state.clone();
        apply::apply(&mut trial, mv);
        if cfg.levels > 0 && !trial.pending.is_empty() {
            stats.quiesced += 1;
            let before = nodes_left;
            let quiet = resolve(&mut trial, cfg.levels - 1, &mut nodes_left, cfg.max_depth, eval);
            stats.qnodes += (before - nodes_left) as u64;
            if !quiet {
                stats.truncated += 1;
            }
        }
        let mut val = eval(&trial, idx);
        if cfg.war_lookahead && matches!(mv, Move::War { .. }) {
            val = war_value(&trial, idx, eval);
        }
        if matches!(mv, Move::EndTurn) {
            val += cfg.end_bias;
        }
        if best.is_none_or(|(_, bv)| val > bv) {
            best = Some((mv, val));
        }
    }
    best.map(|(m, _)| m).unwrap_or(moves[0])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::CardId;
    use crate::game;
    use crate::state::{Pending, TechSlot};

    /// A scorer with no game knowledge at all: every position is worth the
    /// same. Used to pin the SEARCH MECHANICS (does it drain the stack, does
    /// it respect the budgets, does it leave the real state untouched)
    /// independently of any evaluator, since `weighted.rs` does not exist
    /// yet for a real one to come from.
    fn flat(_state: &GameState, _idx: u8) -> f64 {
        0.0
    }

    /// A scorer that prefers whichever candidate leaves `idx` with more
    /// culture, breaking every other tie -- just enough signal to check that
    /// [`pick`]/[`pick_one`] actually select an argmax rather than always
    /// returning the first or last candidate.
    fn culture(state: &GameState, idx: u8) -> f64 {
        state.players[idx as usize].culture as f64
    }

    fn war_card(name: &str) -> CardId {
        CardId::by_name(name).unwrap_or_else(|| panic!("no card named {name:?}"))
    }

    // --------------------------------------------------------------- pick

    #[test]
    fn pick_with_a_single_move_never_touches_the_evaluator_or_state() {
        let state = game::new_game(2, 1);
        let moves = crate::legal::legal_moves(&state);
        // Round 1 always has more than one legal move (take vs end_turn), so
        // force the single-move short circuit directly rather than hunting
        // for a position that happens to have exactly one.
        let one = [moves.as_slice()[0]];
        let mut stats = Stats::default();
        let picked = pick(&QuiescenceConfig::default(), &mut stats, &state, &one, &flat);
        assert_eq!(picked, one[0]);
        assert_eq!(stats, Stats::default(), "the single-move short circuit must not touch stats");
    }

    #[test]
    fn pick_never_mutates_the_real_state() {
        let state = game::new_game(3, 5);
        let before = state.clone();
        let moves = crate::legal::legal_moves(&state);
        let mut stats = Stats::default();
        let _ = pick(&QuiescenceConfig::default(), &mut stats, &state, moves.as_slice(), &culture);
        assert_eq!(
            state.card_row, before.card_row,
            "search must run on clones only -- the real state's row moved"
        );
        assert_eq!(state.players[0].hand_civil, before.players[0].hand_civil);
        assert_eq!(state.turn, before.turn);
    }

    #[test]
    fn pick_always_returns_one_of_the_offered_moves() {
        for levels in [0u32, 1, 2] {
            let state = game::new_game(2, 3);
            let moves = crate::legal::legal_moves(&state);
            let cfg = QuiescenceConfig { levels, ..QuiescenceConfig::default() };
            let mut stats = Stats::default();
            let mv = pick(&cfg, &mut stats, &state, moves.as_slice(), &culture);
            assert!(moves.as_slice().contains(&mv), "levels={levels}: {mv:?} was not offered");
        }
    }

    #[test]
    fn a_zero_node_budget_degrades_to_an_unresolved_one_ply_score_not_a_crash() {
        // §1.9: round 1 offers `Take`/`EndTurn` only, neither of which opens
        // a pending decision, so a starved budget cannot be exercised here --
        // this test only pins that MAX_NODES=0 does not panic and still
        // returns a legal move, matching Python's
        // `test_zero_budget_degrades_to_one_ply`.
        let state = game::new_game(2, 9);
        let moves = crate::legal::legal_moves(&state);
        let cfg = QuiescenceConfig { max_nodes: 0, ..QuiescenceConfig::default() };
        let mut stats = Stats::default();
        let mv = pick(&cfg, &mut stats, &state, moves.as_slice(), &culture);
        assert!(moves.as_slice().contains(&mv));
    }

    #[test]
    fn pick_picks_the_argmax_not_the_first_or_last_candidate() {
        // A hand-built two-candidate choice: one move that gains culture
        // directly, one that does not, scored with `culture`. The argmax
        // must be the culture-gaining move regardless of which slot in
        // `moves` it occupies.
        let mut state = game::new_game(2, 1);
        state.players[0].culture = 5;
        let gain = Move::EndTurn; // any move `culture` scores identically for both orderings
        let mut stats = Stats::default();
        let cfg = QuiescenceConfig::default();
        let a = pick(&cfg, &mut stats, &state, &[gain, Move::Take { slot: 0 }], &culture);
        let b = pick(&cfg, &mut stats, &state, &[Move::Take { slot: 0 }, gain], &culture);
        // `culture` cannot distinguish `end_turn` from `take` here (neither
        // changes `players[0].culture`), so the real assertion is symmetry:
        // candidate order must not change which move an equal-valued search
        // returns to a DIFFERENT winner depending on position -- the search
        // must be a stable argmax, not "first larger-or-equal wins" flipped
        // by ordering. `end_bias` breaks the tie deterministically.
        let cfg_biased = QuiescenceConfig { end_bias: 1000.0, ..QuiescenceConfig::default() };
        let a2 = pick(&cfg_biased, &mut stats, &state, &[gain, Move::Take { slot: 0 }], &culture);
        let b2 = pick(&cfg_biased, &mut stats, &state, &[Move::Take { slot: 0 }, gain], &culture);
        assert_eq!(a2, Move::EndTurn, "a large positive end_bias must win regardless of position");
        assert_eq!(b2, Move::EndTurn, "a large positive end_bias must win regardless of position");
        let _ = (a, b);
    }

    // ------------------------------------------------------------ resolve

    /// A hand-built pending stack, resolved with `flat`: the search has no
    /// reason to prefer any answer, so this only pins that `resolve` drains
    /// it to empty and reports quiet.
    #[test]
    fn resolve_drains_a_real_pending_choice_to_quiet() {
        let mut state = game::new_game(2, 6);
        // §6.6 step 1's genuine multi-card discard is the one pending
        // `Choice` reachable with no further module dependencies -- build it
        // directly the way `interact::discard_excess_military` would, by
        // over-filling player 0's military hand past the limit and pushing
        // the choice the same way that function does.
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
        let forced = crate::interact::discard_excess_military(&mut state, 0);
        assert!(forced, "the hand was built oversized on purpose");
        assert!(!state.pending.is_empty(), "an oversized hand must open a discard decision");
        let mut nodes_left = 600i64;
        let quiet = resolve(&mut state, 0, &mut nodes_left, 12, &flat);
        assert!(quiet, "a real discard decision must resolve within the default budget");
        assert!(state.pending.is_empty());
        assert!(nodes_left < 600, "resolving a real decision must spend at least one node");
    }

    #[test]
    fn resolve_with_zero_depth_reports_truncated_when_something_is_pending() {
        let mut state = game::new_game(2, 6);
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
        crate::interact::discard_excess_military(&mut state, 0);
        assert!(!state.pending.is_empty());
        let mut nodes_left = 600i64;
        let quiet = resolve(&mut state, 0, &mut nodes_left, 0, &flat);
        assert!(!quiet, "max_depth=0 must refuse to resolve anything pending, not silently skip it");
        assert!(!state.pending.is_empty(), "a truncated resolve must leave the stack as it found it");
        assert_eq!(nodes_left, 600, "a resolve that never applies a move must not spend a node");
    }

    #[test]
    fn resolve_with_nothing_pending_is_a_no_op() {
        let mut state = game::new_game(2, 1);
        assert!(state.pending.is_empty());
        let mut nodes_left = 5i64;
        assert!(resolve(&mut state, 1, &mut nodes_left, 12, &flat));
        assert_eq!(nodes_left, 5);
    }

    // ---------------------------------------------------------- war_value

    /// Mirrors `test_war_lookahead_prices_the_spoils`: a war the engine can
    /// resolve outright (no spoils choice pending), scored through
    /// `war_value`, must equal evaluating the SAME state
    /// `resolve_war_outcome`/`apply_war_spoils` themselves produce -- i.e.
    /// `war_value` really does delegate to the engine's own combat
    /// resolution and not to a hand-priced guess.
    #[test]
    fn war_value_prices_a_resolvable_war_through_the_engine_itself() {
        let mut state = game::new_game(2, 77);
        // "War over Territory" pays out from the loser's yellow bank
        // (`combat.rs::apply_war_spoils`'s own tested formula), which every
        // fresh 2p game already has stocked -- no extra setup needed to get
        // real, nonzero spoils.
        let war = war_card("War over Territory");
        state.players[0].war_declared_by_me = war;
        state.players[0].war_target = 1;
        state.players[1].wars_declared_on_me[0] = war;
        // Give player 0 a decisive strength edge so the outcome is
        // deterministic rather than a coin flip on the starting position.
        let warriors = war_card("Warriors");
        state.players[0].techs.get_mut(warriors).unwrap().workers = 12;

        let looked = war_value(&state, 0, &culture);

        let mut scratch = state.clone();
        let outcome =
            combat::resolve_war_outcome(&mut scratch, 0).expect("a 12-worker edge must not be a tie");
        assert_eq!(outcome.victor, 0, "the boosted player must win this war");
        combat::apply_war_spoils(&mut scratch, &outcome);
        interact::settle_war_spoils(&mut scratch);
        assert_eq!(looked, culture(&scratch, 0));

        // and the position asked about must be untouched.
        assert_eq!(state.players[0].war_declared_by_me, war);
        assert_eq!(state.players[0].culture, 0);
    }

    /// "War over Technology" leaves the victor a live choice
    /// (`ChoiceKind::WarTech`) that `war_value` must settle before scoring,
    /// rather than pricing the position with the choice still open (which
    /// would price it as if the war meant nothing). With a stealable
    /// technology on the table, `settle_war_spoils` takes the steal (the
    /// ceiling `war_tech_options` can offer), and only whatever advantage is
    /// left over lands as science -- it is no longer priced as pure science
    /// the way `test_war_over_technology_is_priced_at_its_science_value`
    /// (the Python test this once mirrored bit-for-bit) assumed.
    #[test]
    fn war_value_settles_a_war_over_technology_choice_by_stealing_then_taking_leftover_science() {
        let mut state = game::new_game(2, 77);
        let war = war_card("War over Technology");
        state.players[0].war_declared_by_me = war;
        state.players[0].war_target = 1;
        state.players[1].wars_declared_on_me[0] = war;
        // Give the loser a stealable special tech and enough science that
        // the science branch is unambiguous, and give the attacker a
        // decisive edge exactly as the other war_value test does.
        let code_of_laws = war_card("Code of Laws");
        state.players[1].techs.insert(code_of_laws, TechSlot { workers: 0, stored: 0 });
        state.players[1].science = 30;
        let warriors = war_card("Warriors");
        state.players[0].techs.get_mut(warriors).unwrap().workers = 12;

        // The choice really is live in this position: resolving the war
        // on a scratch copy without settling it must leave a WarTech choice
        // pending, with no science moved yet.
        let mut probe = state.clone();
        let outcome = combat::resolve_war_outcome(&mut probe, 0).unwrap();
        combat::apply_war_spoils(&mut probe, &outcome);
        assert!(
            matches!(
                probe.pending.top(),
                Some(Pending::Choice(c)) if matches!(c.kind, crate::state::ChoiceKind::WarTech { .. })
            ),
            "this position must leave a live WarTech choice before settling"
        );
        assert_eq!(probe.players[0].science, 0, "nothing landed yet -- the choice is still open");

        let looked = war_value(&state, 0, &culture);

        interact::settle_war_spoils(&mut probe);
        assert!(probe.pending.is_empty());
        assert!(probe.players[0].techs.has(code_of_laws), "the affordable steal must be taken");
        assert!(probe.players[0].science > 0, "leftover advantage must still land as science");
        assert_eq!(looked, culture(&probe, 0));

        // and the position asked about is untouched.
        assert_eq!(state.players[0].science, 0);
        assert!(state.players[1].techs.has(code_of_laws));
    }

    #[test]
    fn war_value_with_no_war_declared_still_scores_the_unchanged_position() {
        // `resolve_war_outcome` returns `None` when nothing is declared
        // (Python's `events.resolve_war`'s `if not war: return`, which does
        // not raise either) -- `war_value` must fall through to scoring the
        // untouched position, not treat the `None` as an error.
        let state = game::new_game(2, 1);
        assert!(state.players[0].war_declared_by_me.is_none());
        assert_eq!(war_value(&state, 0, &culture), culture(&state, 0));
    }
}
