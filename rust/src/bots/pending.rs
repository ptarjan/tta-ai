//! The ONE implementation of the "not my ordinary turn" short-circuit.
//!
//! Ports `engine/bots/pending.py` (197 lines) -- read that file's own module
//! doc comment first; it is the design rationale for everything below and is
//! restated here only where the Rust shape earns its own note.
//!
//! Both Python search bots -- `PlanBot` and `NeuralPlanBot` -- have the same
//! three-way decision at the top of `pick`:
//!
//! 1. an ordinary turn of mine -> plan the whole turn with the beam;
//! 2. somebody else's pending decision, or mine nested inside another
//!    player's turn -> there is no turn to plan, so price the candidates one
//!    ply deep at a common horizon of "now";
//! 3. a pending decision that is **mine** -> price them one ply deep *after
//!    draining the stack*, because that is how each bot's own beam already
//!    prices every node it searches (`apply -> quiesce -> score`).
//!
//! Case 3 is the bug this module exists to make unrepeatable. Skipping the
//! drain at a real decision while performing it at every searched node means
//! the bot prices its own position differently from the way it prices the
//! identical position inside its own search. Measured consequence,
//! `docs/AGGRESSION_RATE.md`: 1,549 defences faced, 1,104 winnable by
//! arithmetic, **0 ever held off**, with cards burned in 335 arithmetically
//! hopeless defences. An undrained position cannot express whether a defence
//! succeeds, and 588 of 589 winnable defences need 2+ cards, so the first
//! `defend` always leaves the outcome invisible. The drain is a rule-fact --
//! rule 5.4 step 5 is a *threshold*, so a card spent below the attacker's
//! strength buys nothing.
//!
//! WHY IT LIVES HERE RATHER THAN IN EACH BOT. In Python it was written
//! twice -- `plan.py` had it and `neural_plan.py` had it copied out, so the
//! fix for one was not the fix for the other. That is the shape that has
//! cost this repo repeatedly (the build discount, the hand double-count, the
//! population cost, the `rankingCulture` block). The evaluator-specific part
//! (a linear dot product scored serially versus a net scored in one batch
//! per ply) belongs in each bot; the *policy* belongs here, once, so two
//! bots cannot resolve it differently. Python enforces the "once" with a
//! test that fails if either bot re-inlines the branch or grows a
//! class-level default of its own (`tests/test_pending_fallback_is_shared.py`);
//! this port keeps the same shape so a future Rust search bot inherits the
//! guarantee for free rather than having to remember it.
//!
//! ## No caller in this port yet
//!
//! Exactly like `counting.rs`'s "CALLERS MUST ROOT-CACHE" note: `bots/`
//! currently has no search bot (`weighted.rs`/`plan.rs` are not ported), so
//! nothing calls [`not_my_turn`], [`wants_quiet`], [`wants_determinize`],
//! [`prepare_root`] or [`fallback_pick`] yet. This module is landed now,
//! ahead of its caller, for the same reason `counting.rs` was: so the FIRST
//! Rust search bot is built on the shared policy from day one, rather than
//! inventing its own short-circuit that a second bot then has to be kept in
//! sync with by hand -- the exact defect this module exists to prevent.
//!
//! ## Shape changes from the Python
//!
//! * **`bot` is a plain [`BotConfig`], not `getattr` on an arbitrary
//!   object.** Python resolves `QUIET_PENDING`/`PENDING_DETERMINIZE`/
//!   `determinize` via `getattr(bot, name, default)`, because a bot is a
//!   duck-typed object that may or may not carry an override. Rust has no
//!   `getattr`, so the same "class default, instance override" shape is
//!   spelled as a struct: `quiet_pending: Option<bool>` and
//!   `pending_determinize: Option<bool>` are `None` for "ask this module's
//!   default" (Python's `getattr(..., None)` returning `None`) and
//!   `Some(bool)` for "this bot overrides it" (Python's per-instance
//!   `QUIET_PENDING=True/False`). `determinize: bool` has no `Option` because
//!   Python's own default for it is not `None`, it is `True`
//!   (`getattr(bot, "determinize", True)`) -- `BotConfig::default()` matches
//!   that literally rather than adding a third state nothing in Python ever
//!   produces.
//! * **`prepare_root` returns `Cow<GameState>`, not `state` itself sometimes
//!   and a copy other times as the SAME return type.** Python can do that
//!   because both branches return a `Db`/`GameState`-shaped duck-typed
//!   object either way; `test_the_bot_wide_switch_turns_this_path_off_too`
//!   pins the off branch as `assertIs(..., st)` (object identity, not
//!   equality) specifically so the `qd=0`/`det=0` A/B arms are
//!   byte-for-byte the pre-drain behaviour. `Cow::Borrowed(state)` is that
//!   same identity guarantee, checkable in Rust with `matches!(root,
//!   Cow::Borrowed(_))`; `Cow::Owned(root)` is the determinized copy.
//! * **No `copy_fn` injection.** Python injects `fastcopy.copy_state`
//!   (performance hack around `copy.deepcopy`) at the call site because
//!   Python has no cheap structural copy of its own to fall back on.
//!   [`crate::state::GameState`] already derives `Clone`, and `mod.rs`'s own
//!   doc comment establishes that a derived `Clone` over this struct's
//!   fixed-size arrays *is* the cheap structural copy `fastcopy.py` exists
//!   to hand-roll in CPython -- there is no second implementation of "copy a
//!   `GameState`" for a caller to inject, so [`prepare_root`] below just
//!   calls `state.clone()`. Threading a closure through for an operation
//!   that only ever has one implementation would be indirection with
//!   nothing behind it.
//! * **`determinize_fn` IS still injected, and that one earns its keep.**
//!   Unlike the copy, the reshuffle has no Rust implementation to call
//!   directly yet: it is Python's `plan.determinize` (`engine/bots/plan.py`),
//!   and `plan.rs` is not ported (see "No caller in this port yet" above).
//!   This module holds the POLICY of when to reshuffle; the MECHANISM
//!   belongs to whichever bot module eventually reshuffles a root, exactly
//!   as this module's top doc comment already draws that line for the
//!   evaluator-specific scoring. A generic `impl FnOnce(&mut GameState, &mut
//!   PyRandom)` parameter is the ordinary Rust spelling of "the caller
//!   supplies the one mechanism this module intentionally does not own" --
//!   kept as a single generic parameter, not two, now that the copy no
//!   longer needs one.
//! * **Counters are owned by the caller, not process-global statics.**
//!   Python's `_CALLS`/`_QUIET_CALLS`/`_DET_CALLS` are module globals
//!   because CPython gives every caller the same interpreter-wide module
//!   object; a differential test reads them to prove the shared path was
//!   actually taken rather than re-inlined. Rust has no equivalent
//!   "the module IS the instance" -- a process-global `static` here would
//!   make every caller in the process (and every test in the same binary)
//!   share one counter, which is exactly why the earlier version of this
//!   file needed a `Mutex` in its own test module purely to stop unrelated
//!   tests from interleaving on shared global state. [`Counters`] is instead
//!   an ordinary value the caller owns -- one per bot instance, passed
//!   `&mut` into [`fallback_pick`]/[`prepare_root`] -- so two callers (or two
//!   tests) never share storage and there is nothing left to lock. Resetting
//!   is just constructing a fresh `Counters::default()`; there is no
//!   `reset_counters` function because zeroing caller-owned state needs no
//!   API of its own.

use crate::rng::PyRandom;
use crate::state::GameState;
use std::borrow::Cow;

/// Drain a pending decision of MINE before pricing the candidates.
///
/// Measured at 3p and 4p against the live league references before being
/// flipped -- see `docs/AGGRESSION_RATE.md` 7 for the A/B and the
/// behavioural counters. It lands as a CONSISTENCY FIX, not on the strength
/// measurement: the beam already drains before scoring and the live
/// decision did not. Six distinct blocks of 200 games against the frozen
/// league references all favoured the drain; 3p is decisive (pure-qp pool
/// 0.5217 own-win share against a 0.3333 null over 600 games, z = 9.26) and
/// 4p is NOT independently established (one pure-qp block, 0.3000 against a
/// 0.2500 null, z = 1.54, p ~ 0.12).
pub const QUIET_PENDING: bool = true;

/// THE SECOND HALF OF THE SAME INCONSISTENCY. `pick`'s beam path prices
/// candidates on a determinized root; on the pending path one Python bot
/// already determinized and the other did not -- the copy had drifted from
/// the original before anyone noticed it was a copy.
///
/// Lands as a CORRECTNESS FIX with a measured-zero behavioural effect:
/// `tools/pending_divergence.py --lever det` asked whether the pick itself
/// ever changed, at 1,346 of the bot's own pending decisions at 3p, and it
/// changed **0 times** -- the drain resolves the SAME pending stack for
/// every candidate, so the peeked cards enter every candidate's score as a
/// common additive term and an argmax is invariant to a common offset.
pub const DETERMINIZE: bool = true;

/// Shared-path call counts, owned by the caller (one per bot instance). A
/// differential test reads a caller's own `Counters` to prove a bot actually
/// routed through [`fallback_pick`]/[`prepare_root`] rather than re-inlining
/// the branch -- see this module's top doc comment.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counters {
    pub calls: u64,
    pub quiet: u64,
    pub roots: u64,
}

/// Per-bot override of this module's two module-level defaults. `None`
/// means "ask this module" (Python's `getattr(bot, NAME, None)` returning
/// `None`); `Some(v)` is a per-instance override (Python's
/// `PlanBot(quiet_pending=True)` / `plan:FILE,qp=1`).
///
/// See this module's top doc comment for why `determinize` is a plain
/// `bool` rather than `Option<bool>`: Python's own default for it is `True`,
/// not "ask the module", so [`BotConfig::default`] spells that directly.
#[derive(Clone, Copy, Debug)]
pub struct BotConfig {
    /// The bot-wide A/B switch (`plan:FILE,det=0`) that also gates the
    /// beam's own determinization. A run built to measure the leak must
    /// leak EVERYWHERE -- beam and pending alike -- so this must be checked
    /// before [`BotConfig::pending_determinize`], never instead of it.
    pub determinize: bool,
    pub quiet_pending: Option<bool>,
    pub pending_determinize: Option<bool>,
}

impl Default for BotConfig {
    fn default() -> Self {
        BotConfig { determinize: true, quiet_pending: None, pending_determinize: None }
    }
}

/// True when there is no whole turn of `me`'s to plan: either somebody has a
/// decision pending (which may be mine, nested inside another player's
/// turn) or it is simply not my turn.
pub fn not_my_turn(state: &GameState, me: u8) -> bool {
    !state.pending.is_empty() || state.current != me
}

/// Should `bot` drain the pending stack before pricing candidates? Only
/// ever true when there is something TO drain. Resolution order: the
/// instance override when it is `Some`, otherwise this module's
/// [`QUIET_PENDING`].
pub fn wants_quiet(bot: &BotConfig, state: &GameState) -> bool {
    if state.pending.is_empty() {
        return false;
    }
    bot.quiet_pending.unwrap_or(QUIET_PENDING)
}

/// Should `bot` re-shuffle the unseen piles before pricing candidates? Two
/// gates, in this order:
///
/// 1. [`BotConfig::determinize`] -- the bot-wide switch. `false` turns this
///    path off regardless of [`BotConfig::pending_determinize`].
/// 2. the instance override when it is `Some`, otherwise this module's
///    [`DETERMINIZE`].
///
/// `state` is accepted (and unused) to match Python's `wants_determinize(bot,
/// state)` signature and this module's other two predicates -- Python's own
/// body never reads it either, and keeping the parameter here is what makes
/// [`fallback_pick`] and [`prepare_root`] able to call every predicate the
/// same way without a special case.
pub fn wants_determinize(bot: &BotConfig, _state: &GameState) -> bool {
    if !bot.determinize {
        return false;
    }
    bot.pending_determinize.unwrap_or(DETERMINIZE)
}

/// The state the fallback should price candidates from.
///
/// Returns `Cow::Borrowed(state)` when determinization is off, so the
/// caller's behaviour is byte-for-byte unchanged -- the SAME state, not an
/// equal copy of it, exactly as Python's `test_the_bot_wide_switch_turns_
/// this_path_off_too` pins with `assertIs`. Otherwise a determinized copy:
/// one copy per decision, not per candidate -- the candidate loop copies
/// again on top of this.
///
/// `counters.roots` is incremented on every call, drained or not, exactly
/// like Python's `_ROOT_CALLS`. `determinize_fn` is injected exactly as
/// Python's `plan.determinize` is; see this module's top doc comment for why
/// that one (and not the copy) still earns a caller-supplied hook.
pub fn prepare_root<'a>(
    bot: &BotConfig,
    state: &'a GameState,
    counters: &mut Counters,
    determinize_fn: impl FnOnce(&mut GameState, &mut PyRandom),
    rng: &mut PyRandom,
) -> Cow<'a, GameState> {
    counters.roots += 1;
    if !wants_determinize(bot, state) {
        return Cow::Borrowed(state);
    }
    let mut root = state.clone();
    determinize_fn(&mut root, rng);
    Cow::Owned(root)
}

/// Price a non-ordinary-turn decision, draining first iff configured.
///
/// `plain` and `quiet` are the caller's own one-ply scorer, already bound to
/// its candidates, so this function holds the policy and none of the
/// scoring. Call it only when [`not_my_turn`].
pub fn fallback_pick<T>(
    bot: &BotConfig,
    state: &GameState,
    counters: &mut Counters,
    plain: impl FnOnce() -> T,
    quiet: impl FnOnce() -> T,
) -> T {
    counters.calls += 1;
    if wants_quiet(bot, state) {
        counters.quiet += 1;
        quiet()
    } else {
        plain()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::CardId;
    use crate::game;
    use crate::state::{Defense, Pending};

    /// A state with a real pending Defense, decider = player 1 (the
    /// attacker's target). Mirrors `tests/test_plan_defends_when_it_can_win
    /// .py::defence`'s shape closely enough for this module's own tests:
    /// this module never reads the Defense's fields, only whether the stack
    /// is empty and whose turn `current` is.
    fn defending_state() -> GameState {
        let mut state = game::new_game(3, 1);
        state.current = 0;
        state.pending.push(Pending::Defense(Defense {
            player: 1,
            attacker: 0,
            card: CardId::NONE,
            atk: 6,
            dfn: 0,
            spent: 0,
            budget: 3,
        }));
        state
    }

    #[test]
    fn not_my_turn_is_the_only_predicate() {
        let mut state = defending_state();
        assert!(not_my_turn(&state, state.decider()));
        assert!(not_my_turn(&state, 0));
        state.pending = crate::state::PendingStack::new();
        state.current = 0;
        assert!(!not_my_turn(&state, 0));
        assert!(not_my_turn(&state, 1));
    }

    #[test]
    fn wants_quiet_is_false_with_nothing_to_drain() {
        let mut state = defending_state();
        state.pending = crate::state::PendingStack::new();
        let bot = BotConfig { quiet_pending: Some(true), ..BotConfig::default() };
        assert!(!wants_quiet(&bot, &state));
    }

    #[test]
    fn wants_quiet_resolves_to_the_shared_default_with_no_override() {
        let state = defending_state();
        let bot = BotConfig::default();
        assert_eq!(wants_quiet(&bot, &state), QUIET_PENDING);
    }

    #[test]
    fn wants_quiet_honours_a_per_instance_override_either_way() {
        let state = defending_state();
        let on = BotConfig { quiet_pending: Some(true), ..BotConfig::default() };
        let off = BotConfig { quiet_pending: Some(false), ..BotConfig::default() };
        assert!(wants_quiet(&on, &state));
        assert!(!wants_quiet(&off, &state));
    }

    #[test]
    fn wants_determinize_resolves_to_the_shared_default_with_no_override() {
        let state = defending_state();
        let bot = BotConfig::default();
        assert_eq!(wants_determinize(&bot, &state), DETERMINIZE);
    }

    #[test]
    fn the_bot_wide_switch_turns_determinize_off_regardless_of_the_override() {
        // `det=0`: a bot built to measure the leak must leak nowhere, not
        // just skip its own PENDING_DETERMINIZE override.
        let state = defending_state();
        let bot = BotConfig {
            determinize: false,
            pending_determinize: Some(true),
            ..BotConfig::default()
        };
        assert!(!wants_determinize(&bot, &state));
    }

    #[test]
    fn determinize_off_prices_from_the_state_itself() {
        let state = defending_state();
        let bot = BotConfig { pending_determinize: Some(false), ..BotConfig::default() };
        assert!(!wants_determinize(&bot, &state));
        let mut rng = PyRandom::new(1);
        let mut counters = Counters::default();
        let root = prepare_root(
            &bot,
            &state,
            &mut counters,
            |_s, _r| panic!("determinize_fn must not run when determinization is off"),
            &mut rng,
        );
        assert!(
            matches!(root, Cow::Borrowed(_)),
            "with determinization off the fallback must price the state itself, byte-for-byte"
        );
        assert_eq!(&*root as *const GameState, &state as *const GameState);
        assert_eq!(counters.roots, 1, "prepare_root must count the call whether it drains or not");
    }

    #[test]
    fn determinize_on_calls_determinize_and_returns_a_copy() {
        let state = defending_state();
        let bot = BotConfig { pending_determinize: Some(true), ..BotConfig::default() };
        assert!(wants_determinize(&bot, &state));
        let mut rng = PyRandom::new(1);
        let mut counters = Counters::default();
        let mut det_called = false;
        let root = prepare_root(
            &bot,
            &state,
            &mut counters,
            |_s, _r| {
                det_called = true;
            },
            &mut rng,
        );
        assert!(det_called, "prepare_root must determinize the copy");
        assert!(matches!(root, Cow::Owned(_)), "must not return the real state's identity");
        assert_eq!(counters.roots, 1);
    }

    #[test]
    fn fallback_pick_takes_the_quiet_path_iff_configured() {
        let state = defending_state();
        let bot = BotConfig { quiet_pending: Some(true), ..BotConfig::default() };
        let mut counters = Counters::default();
        let picked = fallback_pick(&bot, &state, &mut counters, || "plain", || "quiet");
        assert_eq!(picked, "quiet");
        assert_eq!(counters.calls, 1);
        assert_eq!(counters.quiet, 1);
    }

    #[test]
    fn fallback_pick_takes_the_plain_path_when_quiet_is_off() {
        let state = defending_state();
        let bot = BotConfig { quiet_pending: Some(false), ..BotConfig::default() };
        let mut counters = Counters::default();
        let picked = fallback_pick(&bot, &state, &mut counters, || "plain", || "quiet");
        assert_eq!(picked, "plain");
        assert_eq!(counters.calls, 1);
        assert_eq!(counters.quiet, 0);
    }

    /// Two callers -- here, two independent `Counters` in the same test --
    /// never share storage. This is the property a process-global `static`
    /// could not offer without a `Mutex` (see this module's top doc
    /// comment): owning the counter is what makes it safe to call
    /// [`fallback_pick`] from anywhere, including concurrently, with no lock.
    #[test]
    fn counters_accumulate_per_owner_not_globally() {
        let state = defending_state();
        let bot = BotConfig { quiet_pending: Some(true), ..BotConfig::default() };
        let mut a = Counters::default();
        let mut b = Counters::default();
        fallback_pick(&bot, &state, &mut a, || (), || ());
        fallback_pick(&bot, &state, &mut a, || (), || ());
        fallback_pick(&bot, &state, &mut b, || (), || ());
        assert_eq!(a, Counters { calls: 2, quiet: 2, roots: 0 });
        assert_eq!(b, Counters { calls: 1, quiet: 1, roots: 0 });
    }
}
