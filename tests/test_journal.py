"""Tests for `engine.journal` -- the undo stack primitives.

NOTE on scope: at this commit **no engine call site is converted yet**, so a
journalled `actions.apply` journals its attribute writes but not its container
mutations.  These tests therefore exercise the primitives directly, plus the
one end-to-end property that matters right now: that paranoid mode *detects*
an unjournalled container mutation.  If it did not, the whole conversion plan
would be unguarded.
"""
from __future__ import annotations

import copy
import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from engine import game, journal, state as state_mod, statediff  # noqa: E402
from engine.state import TechCard, PlayerState                 # noqa: E402
from engine.bots.fastcopy import copy_state                    # noqa: E402


def _st():
    return game.new_game(3, seed=5)


class JournalTestCase(unittest.TestCase):
    def setUp(self):
        journal.install()

    def tearDown(self):
        # never leave a journal open -- or the log suppressed -- for the
        # next test
        if journal.active():
            journal._J = None
        state_mod.SUPPRESS_LOG = False


class AttributeUndo(JournalTestCase):
    def test_scalar_restored(self):
        st = _st()
        before = copy_state(st)
        j = journal.begin(st)
        st.turn = 999
        st.players[0].food = 77
        st.players[1].techs["Bronze"] = TechCard("Bronze", 4, 5)
        journal.rollback(j)
        self.assertEqual(st.turn, before.turn)
        self.assertEqual(st.players[0].food, before.players[0].food)

    def test_repeated_writes_to_one_attr_restore_the_oldest(self):
        st = _st()
        orig = st.turn
        j = journal.begin(st)
        st.turn = 1
        st.turn = 2
        st.turn = 3
        journal.rollback(j)
        self.assertEqual(st.turn, orig)

    def test_new_attribute_is_deleted_again(self):
        st = _st()
        j = journal.begin(st)
        st.players[0].invented = 1
        journal.rollback(j)
        self.assertFalse(hasattr(st.players[0], "invented"))

    def test_nested_dataclass_attr(self):
        st = _st()
        p = st.players[0]
        k = next(iter(p.techs))
        before = p.techs[k].workers
        j = journal.begin(st)
        p.techs[k].workers += 5
        journal.rollback(j)
        self.assertEqual(p.techs[k].workers, before)

    def test_objects_created_during_the_trial_are_harmless(self):
        st = _st()
        j = journal.begin(st)
        t = TechCard("Nothing", 1, 2)      # __init__ writes get journalled
        journal.rollback(j)
        self.assertIsNotNone(t)            # rollback must not explode

    def test_off_by_default(self):
        st = _st()
        self.assertFalse(journal.active())
        st.turn = 4321
        self.assertEqual(st.turn, 4321)    # no journal, no undo


class ContainerUndo(JournalTestCase):
    def test_list_append_and_pop(self):
        st = _st()
        before = list(st.players[0].hand_civil)
        j = journal.begin(st)
        journal.touch(st.players[0].hand_civil).append("Bogus")
        journal.touch(st.players[0].hand_civil).pop(0)
        journal.rollback(j)
        self.assertEqual(st.players[0].hand_civil, before)

    def test_list_order_restored_after_reverse(self):
        st = _st()
        before = list(st.civil_deck)
        j = journal.begin(st)
        journal.touch(st.civil_deck).reverse()
        journal.rollback(j)
        self.assertEqual(st.civil_deck, before)

    def test_list_slice_assignment(self):
        st = _st()
        before = list(st.card_row)
        j = journal.begin(st)
        journal.touch(st.card_row)[:] = [None] * len(st.card_row)
        journal.rollback(j)
        self.assertEqual(st.card_row, before)

    def test_dict_KEY_ORDER_restored(self):
        """The hazard a per-op LIFO journal gets wrong and snapshots cannot."""
        st = _st()
        p = st.players[0]
        before = list(p.techs)
        self.assertGreater(len(before), 1)
        j = journal.begin(st)
        k = before[0]
        v = journal.touch(p.techs).pop(k)
        p.techs[k] = v                     # would land LAST without a snapshot
        p.techs["Extra"] = TechCard("Extra")
        journal.rollback(j)
        self.assertEqual(list(p.techs), before)
        self.assertEqual(statediff.diff(st, st), [])

    def test_dict_del_and_add(self):
        st = _st()
        before = dict(st.seeded_by)
        j = journal.begin(st)
        journal.touch(st.seeded_by)["Made Up Event"] = 2
        journal.rollback(j)
        self.assertEqual(st.seeded_by, before)

    def test_set_add_and_discard(self):
        st = _st()
        st.a_set = {1, 2, 3}
        j = journal.begin(st)
        journal.touch(st.a_set).add(4)
        journal.touch(st.a_set).discard(1)
        journal.rollback(j)
        self.assertEqual(st.a_set, {1, 2, 3})

    def test_touch_is_idempotent_and_keeps_the_OLDEST_snapshot(self):
        st = _st()
        lst = st.players[0].hand_civil
        before = list(lst)
        j = journal.begin(st)
        journal.touch(lst).append("a")
        journal.touch(lst).append("b")     # second touch must be a no-op
        journal.touch(lst).append("c")
        self.assertEqual(len([r for r in j if r[0] == journal._LIST]), 1)
        journal.rollback(j)
        self.assertEqual(lst, before)

    def test_touch_is_a_noop_with_no_journal(self):
        lst = [1, 2]
        self.assertIs(journal.touch(lst), lst)

    def test_touch_rejects_unknown_types(self):
        st = _st()
        j = journal.begin(st)
        try:
            with self.assertRaises(journal.JournalError):
                journal.touch(("a", "tuple"))
        finally:
            journal.rollback(j)


class Lifecycle(JournalTestCase):
    def test_partial_journal_after_an_exception(self):
        """Hazard 5: `apply` raising mid-mutation must still roll back."""
        st = _st()
        before = copy_state(st)
        j = journal.begin(st)
        try:
            st.turn = 42
            journal.touch(st.players[0].hand_civil).append("x")
            st.players[2].culture = 1234
            raise ValueError("STRICT legality assert, say")
        except ValueError:
            pass
        finally:
            journal.rollback(j)
        self.assertEqual(statediff.diff(before, st), [])

    def test_scope_rolls_back_on_exception(self):
        st = _st()
        before = copy_state(st)
        with self.assertRaises(ValueError):
            with journal.scope(st):
                st.turn = 7
                raise ValueError
        self.assertEqual(statediff.diff(before, st), [])
        self.assertFalse(journal.active())

    def test_nesting_is_refused(self):
        st = _st()
        j = journal.begin(st)
        try:
            with self.assertRaises(journal.JournalError):
                journal.begin(st)
        finally:
            journal.rollback(j)

    def test_rollback_of_the_wrong_journal_is_refused(self):
        st = _st()
        j = journal.begin(st)
        try:
            with self.assertRaises(journal.JournalError):
                journal.rollback(journal._Journal())
        finally:
            journal.rollback(j)

    def test_stats_cache_is_dropped_on_rollback(self):
        st = _st()
        j = journal.begin(st)
        st._stats_cache = {"polluted": True}
        journal.rollback(j)
        self.assertFalse(hasattr(st, "_stats_cache"))

    def test_copy_state_inside_a_journal_is_refused(self):
        """Aliasing a half-mutated trial into a copy would be silent
        corruption; it must be loud instead."""
        st = _st()
        j = journal.begin(st)
        try:
            with self.assertRaises(journal.JournalError):
                copy_state(st)
        finally:
            journal.rollback(j)


class LogSuppression(JournalTestCase):
    """Hazard 2 of section 6.5.  `copy_state` drops the log, so today a trial
    move physically cannot reach the real one.  The undo stack has no copy to
    absorb the writes, and `emit` is *destructive* past 400 entries
    (`del self.log[:100]`) -- and the log is inside the fingerprint digest."""

    def test_emit_is_suppressed_inside_a_journal(self):
        st = _st()
        before = list(st.log)
        j = journal.begin(st)
        st.emit("a trial move must not say anything")
        journal.rollback(j)
        self.assertEqual(st.log, before)

    def test_emit_works_again_after_rollback(self):
        st = _st()
        journal.rollback(journal.begin(st))
        n = len(st.log)
        st.emit("real play still logs")
        self.assertEqual(len(st.log), n + 1)

    def test_suppression_is_lifted_even_when_apply_raises(self):
        st = _st()
        with self.assertRaises(ValueError):
            with journal.scope(st):
                st.emit("swallowed")
                raise ValueError("boom")
        self.assertFalse(state_mod.SUPPRESS_LOG)
        n = len(st.log)
        st.emit("back to normal")
        self.assertEqual(len(st.log), n + 1)

    def test_the_destructive_truncation_cannot_fire_during_a_trial(self):
        """The specific reason suppression is not merely an optimisation: at
        >400 entries `emit` deletes the oldest 100, which no undo record
        would have captured."""
        st = _st()
        st.log = [f"line {i}" for i in range(400)]
        before = list(st.log)
        j = journal.begin(st)
        for i in range(50):
            st.emit(f"trial {i}")
        journal.rollback(j)
        self.assertEqual(st.log, before)

    def test_paranoid_mode_would_catch_a_log_leak(self):
        """If suppression regresses, the oracle must say so -- that is why the
        paranoid oracle keeps the log and the diff includes it."""
        st = _st()
        old = journal.PARANOID
        journal.PARANOID = True
        try:
            j = journal.begin(st)
            state_mod.SUPPRESS_LOG = False          # simulate the regression
            st.emit("leaked into the real log")
            with self.assertRaises(AssertionError) as cm:
                journal.rollback(j)
            self.assertIn("log", str(cm.exception))
        finally:
            journal.PARANOID = old


class ParanoidModeCatchesMisses(JournalTestCase):
    """The safety net itself.  The entire conversion plan rests on paranoid
    mode detecting a mutation site that was NOT journalled."""

    def setUp(self):
        super().setUp()
        self._old = journal.PARANOID
        journal.PARANOID = True

    def tearDown(self):
        journal.PARANOID = self._old
        super().tearDown()

    def test_clean_rollback_passes(self):
        st = _st()
        j = journal.begin(st)
        st.turn += 1
        journal.touch(st.players[0].hand_civil).append("x")
        journal.rollback(j)               # must not raise

    def test_unjournalled_list_append_is_CAUGHT(self):
        st = _st()
        j = journal.begin(st)
        st.players[0].hand_civil.append("forgot to touch()")
        with self.assertRaises(AssertionError) as cm:
            journal.rollback(j)
        self.assertIn("hand_civil", str(cm.exception))

    def test_unjournalled_dict_write_is_CAUGHT(self):
        st = _st()
        j = journal.begin(st)
        st.seeded_by["forgot"] = 1
        with self.assertRaises(AssertionError) as cm:
            journal.rollback(j)
        self.assertIn("seeded_by", str(cm.exception))

    def test_unjournalled_nested_dict_write_is_CAUGHT(self):
        st = _st()
        j = journal.begin(st)
        st.players[1].techs["Sneaky"] = TechCard("Sneaky")
        with self.assertRaises(AssertionError) as cm:
            journal.rollback(j)
        self.assertIn("techs", str(cm.exception))

    def test_unjournalled_del_is_CAUGHT(self):
        st = _st()
        j = journal.begin(st)
        del st.players[0].techs[next(iter(st.players[0].techs))]
        with self.assertRaises(AssertionError):
            journal.rollback(j)


class SetattrCoverageIsComplete(unittest.TestCase):
    """The `__setattr__` hook covers 300 of the 470 sites -- IF the class list
    is complete.

    `journal.JOURNALLED_CLASSES` names four dataclasses.  Every attribute write
    to an object of some *other* class that is nonetheless reachable from the
    GameState would be silently unjournalled, and unlike a container site it
    would not be found by `tools/find_mutations.py` (which only looks at
    subscripts and mutator calls).  That is the one hole the conversion plan
    cannot grep its way out of, so it is closed here by walking a real
    mid-game state and asserting the reachable type set is exactly what the
    journal assumes: the four dataclasses, plain containers, and scalars.

    If someone later adds a fifth state dataclass, this fails immediately and
    names it -- rather than the journal quietly failing to restore it.
    """

    def _walk(self, root):
        seen, todo, classes, containers = set(), [root], set(), set()
        while todo:
            o = todo.pop()
            if id(o) in seen:
                continue
            seen.add(id(o))
            t = type(o)
            if t in (str, int, float, bool, bytes, type(None)):
                continue
            if hasattr(o, "__dataclass_fields__"):
                classes.add(t)
                todo.extend(v for k, v in o.__dict__.items() if k[0] != "_")
            elif t in (list, tuple, set, frozenset):
                containers.add(t)
                todo.extend(o)
            elif t is dict:
                containers.add(t)
                todo.extend(o.keys())
                todo.extend(o.values())
            else:
                containers.add(t)
        return classes, containers

    def _midgame(self):
        from engine.bots import make_bots
        st = game.new_game(4, seed=3)
        bots = make_bots("greedy", 4, seed=1)
        for _ in range(120):
            if st.game_over:
                break
            mv = bots[st.decider()](st)
            from engine import actions
            actions.apply(st, mv, __import__("random").Random(7))
        return st

    def test_only_journalled_dataclasses_are_reachable(self):
        st = self._midgame()
        classes, containers = self._walk(st)
        self.assertTrue(classes, "walk found no dataclasses -- walk is broken")
        extra = classes - set(journal.JOURNALLED_CLASSES)
        self.assertFalse(
            extra,
            "state-reachable dataclass(es) whose attribute writes are NOT "
            f"journalled: {sorted(c.__name__ for c in extra)}.  Add them to "
            "journal.JOURNALLED_CLASSES.")

    def test_only_container_types_the_journal_can_undo_are_reachable(self):
        st = self._midgame()
        classes, containers = self._walk(st)
        # `touch()` raises JournalError on anything that is not one of these
        # three; tuple/frozenset are immutable so they need no undo record.
        allowed = {list, dict, set, tuple, frozenset}
        extra = containers - allowed
        self.assertFalse(
            extra,
            "state-reachable mutable container type(s) `journal.touch` cannot "
            f"undo: {sorted(c.__name__ for c in extra)}")


if __name__ == "__main__":
    unittest.main()
