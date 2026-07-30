"""The two search bots must short-circuit a pending decision the SAME way.

WHY THIS FILE EXISTS.  `PlanBot.pick` and `NeuralPlanBot.pick` each had their
own copy of

    if state.pending or state.current != me:
        return <my own 1-ply pick>

so when `docs/AGGRESSION_RATE.md` found that the missing drain was a
correctness defect -- the bot pricing its own defence differently from the way
its own beam prices the identical position -- fixing `plan.py` fixed nothing in
`neural_plan.py`.  That is the duplication shape this repo has paid for
repeatedly (the build discount, the hand double-count, the population cost, the
`rankingCulture` block), and the settled remedy is: ONE implementation, plus a
test that fails when the copies drift apart.

The policy now lives in `engine.bots.pending`.  These tests pin the three ways
the two paths could silently diverge again:

1. **Re-inlining.**  If either bot stops calling `pending.fallback_pick`, the
   shared call counter stops moving.  That is a structural check on tracked
   state, not a regex over source text: a bot that reimplements the branch
   *identically* still fails, because the point is that there is one
   implementation, not two that happen to agree today.
2. **A second default.**  If either class hard-codes a bool `QUIET_PENDING`,
   the two can be flipped apart.  The default must live in exactly one place.
3. **A different drain.**  With the drain on, EVERY position either bot prices
   at a real pending decision must be quiet; with it off, at least one must
   still be pending.  Asserted with the same test body against both classes,
   because the invariant is about the position priced -- which is the thing the
   defect was about -- and not about the evaluator that prices it.

`NeuralPlanBot` takes its evaluator by injection ("anything with
``.value(list) -> list[float]``"), so nothing here imports torch: a stub value
is enough, since these are assertions about WHICH positions get scored.
"""
from __future__ import annotations

import os
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)),
                                ".."))

from engine.bots import pending                              # noqa: E402
from engine.bots.neural_plan import NeuralPlanBot            # noqa: E402
from engine.bots.plan import PlanBot                         # noqa: E402
from engine.bots.weighted import load_weights                # noqa: E402
from tests.test_plan_defends_when_it_can_win import (         # noqa: E402
    BONUS2, defence)

_W = load_weights(os.path.join(os.path.dirname(os.path.abspath(__file__)),
                              "..", "analysis", "frozen", "champion_2p.json"))


class _StubValue:
    """`.value(encs) -> list[float]`, the whole contract NeuralPlanBot needs.

    Returns a constant.  Every test here asks which positions were priced, not
    which move won, so a constant is not a weakened assertion -- and it keeps
    this file torch-free.
    """

    def value(self, encs):
        return [0.0] * len(encs)


def _bots(quiet):
    """One of each search bot, configured identically, `quiet` either way."""
    return [
        ("PlanBot", PlanBot(weights=_W, seed=3, width=2,
                            quiet_pending=quiet)),
        ("NeuralPlanBot", NeuralPlanBot(_StubValue(), seed=3, width=2,
                                        quiet_pending=quiet)),
    ]


def _priced_positions(bot, state, moves):
    """Every position `bot` hands to its leaf scorer for this one decision.

    Each bot is instrumented at its OWN leaf entry point, which is the only
    per-class thing in this file: `PlanBot` scores serially through `_score`
    and through the module-level `evaluate` on its non-quiet path, while
    `NeuralPlanBot` encodes through `_leaf_enc` and scores a batch.  Both hand
    a real `GameState` to that entry point, so "was the stack drained before
    this position was priced?" is answerable for both.
    """
    seen = []

    if isinstance(bot, PlanBot):
        import engine.bots.plan as plan_mod
        real_score, real_eval = bot._score, plan_mod.evaluate

        def spy_score(t, me, w, ctx):
            seen.append(bool(t.pending))
            return real_score(t, me, w, ctx)

        def spy_eval(t, me, w, ctx=None):
            seen.append(bool(t.pending))
            return real_eval(t, me, w, ctx)

        bot._score = spy_score
        plan_mod.evaluate = spy_eval
        try:
            bot.pick(state, moves)
        finally:
            bot._score = real_score
            plan_mod.evaluate = real_eval
    else:
        real_enc = bot._leaf_enc

        def spy_enc(t, me):
            seen.append(bool(t.pending))
            return real_enc(t, me)

        bot._leaf_enc = spy_enc
        try:
            bot.pick(state, moves)
        finally:
            bot._leaf_enc = real_enc
    return seen


class SharedShortCircuit(unittest.TestCase):
    """1 and 2: there is one implementation and one default."""

    def test_both_bots_route_through_the_shared_helper(self):
        # A pending defence decision of the decider's own -- the case the
        # defect was about.
        for name, bot in _bots(quiet=True):
            st = defence(atk=6)
            moves = [("defend", BONUS2), ("defend_done",)]
            pending.reset_counters()
            bot.pick(st, moves)
            c = pending.counters()
            self.assertEqual(
                c["calls"], 1,
                f"{name} did not call pending.fallback_pick for a pending "
                f"decision: the short-circuit has been re-inlined, so a fix "
                f"to one bot is no longer a fix to the other ({c})")
            self.assertEqual(
                c["quiet"], 1,
                f"{name} called the shared helper but did not take the quiet "
                f"path with quiet_pending=True ({c})")

    def test_quiet_off_routes_through_the_helper_too(self):
        for name, bot in _bots(quiet=False):
            st = defence(atk=6)
            pending.reset_counters()
            bot.pick(st, [("defend", BONUS2), ("defend_done",)])
            c = pending.counters()
            self.assertEqual(c["calls"], 1, f"{name}: {c}")
            self.assertEqual(
                c["quiet"], 0,
                f"{name} drained despite quiet_pending=False ({c})")

    def test_neither_class_carries_its_own_default(self):
        # If a bool lands on either class, the two bots can be flipped apart
        # and `pending.QUIET_PENDING` stops being the answer.
        for cls in (PlanBot, NeuralPlanBot):
            self.assertIsNone(
                cls.QUIET_PENDING,
                f"{cls.__name__}.QUIET_PENDING must stay None ('ask "
                f"engine.bots.pending'); a bool here is a SECOND default and "
                f"the two search bots will diverge the next time it is "
                f"flipped")

    def test_both_bots_build_their_root_through_the_shared_helper(self):
        # `prepare_root` is the OTHER half of the same short-circuit: the beam
        # path determinizes and this path is where NeuralPlanBot already did
        # and PlanBot did not.  Counting it keeps that difference to one
        # documented value instead of two implementations.
        for name, bot in _bots(quiet=True):
            st = defence(atk=6)
            pending.reset_counters()
            bot.pick(st, [("defend", BONUS2), ("defend_done",)])
            self.assertEqual(
                pending.counters()["roots"], 1,
                f"{name} did not build its fallback root through "
                f"pending.prepare_root ({pending.counters()})")

    def test_neither_class_carries_its_own_determinize_default(self):
        # These two used to differ -- False on PlanBot, True on NeuralPlanBot
        # -- and the difference was a measured defect in PlanBot's direction
        # (tools/pending_leak.py: the drain consumes real deck cards in 34.7%
        # of candidate evaluations at 3p).  It is closed: both are None, so
        # `pending.DETERMINIZE` is the single answer.  A bool landing on either
        # class is a SECOND default and the two bots will diverge again the
        # next time somebody flips one.
        for cls in (PlanBot, NeuralPlanBot):
            self.assertIsNone(
                cls.PENDING_DETERMINIZE,
                f"{cls.__name__}.PENDING_DETERMINIZE must stay None ('ask "
                f"engine.bots.pending'); that drift is what this module and "
                f"docs/AGGRESSION_RATE.md 9 exist to prevent")

    def test_the_bot_wide_switch_turns_this_path_off_too(self):
        # `det=0` is the A/B control that measures the leak.  A bot built that
        # way must leak EVERYWHERE -- beam and pending alike -- or the A/B is
        # measuring half a lever.  The gate lives in `wants_determinize` so
        # neither bot can spell it at its own call site again.
        bot = PlanBot(weights=_W, seed=3, width=2, quiet_pending=True,
                      determinize=False)
        st = defence(atk=6)
        self.assertFalse(pending.wants_determinize(bot, st))
        self.assertIs(
            pending.prepare_root(bot, st, lambda s: None, lambda s, r: None,
                                 bot.rng), st,
            "with determinization off the fallback must price the state "
            "itself, byte-for-byte")

    def test_determinize_off_prices_from_the_state_itself(self):
        bot = PlanBot(weights=_W, seed=3, width=2, quiet_pending=True,
                      pending_determinize=False)
        st = defence(atk=6)
        self.assertFalse(pending.wants_determinize(bot, st))
        self.assertIs(
            pending.prepare_root(bot, st, lambda s: None, lambda s, r: None,
                                 bot.rng), st,
            "with determinization off the fallback must price the state "
            "itself, so the `qd=0` A/B arm is byte-for-byte the old behaviour")

    def test_determinize_on_reshuffles_only_the_unseen_decks(self):
        from engine.bots.fastcopy import copy_state
        from engine.bots.plan import determinize
        bot = PlanBot(weights=_W, seed=3, width=2, quiet_pending=True,
                      pending_determinize=True)
        st = defence(atk=6)
        self.assertTrue(pending.wants_determinize(bot, st))
        root = pending.prepare_root(bot, st, copy_state, determinize, bot.rng)
        self.assertIsNot(root, st, "must not determinize the real state")
        self.assertEqual(root.card_row, st.card_row, "the row is PUBLIC")
        self.assertEqual(sorted(root.civil_deck), sorted(st.civil_deck),
                         "determinization re-orders the deck, it does not "
                         "change what is in it")

    def test_the_shared_default_is_what_both_bots_resolve(self):
        class _Bare:
            pass

        st = defence(atk=6)
        self.assertEqual(pending.wants_quiet(_Bare(), st),
                         pending.QUIET_PENDING)
        for name, bot in [("PlanBot", PlanBot(weights=_W, seed=3, width=2)),
                          ("NeuralPlanBot",
                           NeuralPlanBot(_StubValue(), seed=3, width=2))]:
            self.assertEqual(
                pending.wants_quiet(bot, st), pending.QUIET_PENDING,
                f"{name} resolves the drain differently from the shared "
                f"default")

    def test_not_my_turn_is_the_only_predicate(self):
        st = defence(atk=6)                     # pending, decider is player 1
        self.assertTrue(pending.not_my_turn(st, st.decider()))
        self.assertTrue(pending.not_my_turn(st, 0))
        st.pending = []
        st.current = 0
        self.assertFalse(pending.not_my_turn(st, 0))
        self.assertTrue(pending.not_my_turn(st, 1))

    def test_wants_quiet_is_false_with_nothing_to_drain(self):
        st = defence(atk=6)
        st.pending = []
        for _name, bot in _bots(quiet=True):
            self.assertFalse(pending.wants_quiet(bot, st))


class SameDrainOnBothPaths(unittest.TestCase):
    """3: the positions priced are quiet iff the drain is on -- both bots."""

    def test_quiet_on_prices_only_drained_positions(self):
        for name, bot in _bots(quiet=True):
            st = defence(atk=6)
            still_pending = _priced_positions(
                bot, st, [("defend", BONUS2), ("defend_done",)])
            self.assertTrue(
                still_pending,
                f"{name} priced nothing -- the spy missed the leaf, so this "
                f"test would pass vacuously")
            self.assertFalse(
                any(still_pending),
                f"{name} priced {sum(still_pending)}/{len(still_pending)} "
                f"positions with the stack still pending while "
                f"quiet_pending=True: this is exactly the inconsistency "
                f"engine.bots.pending exists to remove")

    def test_quiet_off_prices_an_undrained_position(self):
        # The control: without the drain the defect is still visible, which
        # proves the assertion above is about the drain and not about the
        # fixture happening to be quiet already.
        for name, bot in _bots(quiet=False):
            st = defence(atk=6)
            still_pending = _priced_positions(
                bot, st, [("defend", BONUS2), ("defend_done",)])
            self.assertTrue(
                still_pending,
                f"{name} priced nothing -- spy missed the leaf")
            self.assertTrue(
                any(still_pending),
                f"{name} with quiet_pending=False priced no pending position, "
                f"so this fixture cannot tell the two paths apart and the "
                f"companion test is vacuous")


if __name__ == "__main__":
    unittest.main()
