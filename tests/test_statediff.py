"""Tests for `engine.statediff` -- the oracle the undo stack is gated on.

This differ is the whole safety argument for docs/PYPY.md section 6 (journal /
undo stack).  If it has a blind spot, a missed mutation site slides through the
paranoid check and corrupts real games.  So it gets tested for *detection*,
not just for agreeing that identical things are identical: every mutation kind
in section 6.2's table must be caught, positively.
"""
from __future__ import annotations

import copy
import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from engine import game, statediff                              # noqa: E402
from engine.state import TechCard                               # noqa: E402
from engine.bots.fastcopy import copy_state                     # noqa: E402


def _st():
    return game.new_game(3, seed=7)


class DiffFindsNothingWhenNothingChanged(unittest.TestCase):
    def test_state_vs_deepcopy(self):
        st = _st()
        self.assertEqual(statediff.diff(st, copy.deepcopy(st)), [])

    def test_state_vs_fastcopy(self):
        st = _st()
        self.assertEqual(statediff.diff(st, copy_state(st)), [])

    def test_state_vs_itself(self):
        st = _st()
        self.assertEqual(statediff.diff(st, st), [])

    def test_log_excluded_by_default(self):
        # copy_state drops the log, so the oracle cannot be asked about it.
        st, other = _st(), _st()
        other.log.append("this is not in the other log")
        self.assertEqual(statediff.diff(st, other), [])
        self.assertTrue(statediff.diff(st, other, include_log=True))

    def test_private_excluded_by_default(self):
        st, other = _st(), _st()
        other._stats_cache = {"anything": 1}
        self.assertEqual(statediff.diff(st, other), [])
        self.assertTrue(statediff.diff(st, other, include_private=True))


class DiffCatchesEveryMutationKind(unittest.TestCase):
    """One test per row of the section 6.2 undo-record table."""

    def setUp(self):
        self.a = _st()
        self.b = copy.deepcopy(self.a)

    def bad(self):
        d = statediff.diff(self.a, self.b)
        self.assertTrue(d, "differ MISSED the mutation")
        return d

    # obj.attr = v -------------------------------------------------------
    def test_scalar_attr_on_gamestate(self):
        self.b.turn += 1
        self.assertIn("state.turn", self.bad()[0])

    def test_scalar_attr_deep(self):
        self.b.players[2].food += 1
        self.assertIn("state.players[2].food", self.bad()[0])

    def test_attr_set_to_equal_but_different_type(self):
        self.b.players[1].food = 0.0     # was int 0
        self.assertTrue(self.bad())

    def test_new_attr_added(self):
        self.b.players[0].brand_new = 1
        self.assertIn("gained", self.bad()[0])

    def test_attr_deleted(self):
        del self.b.players[0].food
        self.assertIn("lost", self.bad()[0])

    def test_none_to_value(self):
        self.b.forced_winner = 0
        self.assertTrue(self.bad())

    def test_value_to_none(self):
        self.a.forced_winner = 0
        self.assertTrue(self.bad())

    # d[k] = v -----------------------------------------------------------
    def test_dict_value_changed(self):
        k = next(iter(self.b.players[0].techs))
        self.b.players[0].techs[k].workers += 1
        self.assertIn("workers", self.bad()[0])

    def test_dict_key_added(self):
        self.b.players[0].techs["Made Up Tech"] = TechCard("Made Up Tech")
        self.assertIn("gained", self.bad()[0])

    def test_dict_key_removed(self):
        k = next(iter(self.b.players[0].techs))
        del self.b.players[0].techs[k]
        self.assertIn("lost", self.bad()[0])

    def test_dict_key_ORDER_changed(self):
        """The one `==` cannot see, and the one non-LIFO rollback produces."""
        p = self.b.players[0]
        items = list(p.techs.items())
        self.assertGreater(len(items), 1)
        p.techs = dict(reversed(items))
        self.assertEqual(p.techs, self.a.players[0].techs)   # `==` says equal!
        self.assertIn("KEY ORDER", self.bad()[0])

    def test_dict_reinsert_moves_key_to_the_end(self):
        """The concrete non-LIFO bug: delete a key, put it back last."""
        p = self.b.players[0]
        k = next(iter(p.techs))
        v = p.techs.pop(k)
        p.techs[k] = v                    # same keys, same values, wrong order
        self.assertEqual(p.techs, self.a.players[0].techs)
        self.assertIn("KEY ORDER", self.bad()[0])

    # list append / pop / insert / remove / sort / reverse ----------------
    def test_list_append(self):
        self.b.players[0].hand_civil.append("Whatever")
        self.assertTrue(self.bad())

    def test_list_pop(self):
        self.a.players[0].hand_civil.append("Whatever")
        self.b.players[0].hand_civil.append("Whatever")
        self.b.players[0].hand_civil.pop()
        self.assertTrue(self.bad())

    def test_list_insert_at_wrong_index(self):
        self.a.civil_deck.insert(0, "X")
        self.b.civil_deck.insert(1, "X")
        self.assertTrue(self.bad())

    def test_list_reversed_same_elements(self):
        self.b.civil_deck.reverse()
        self.assertTrue(self.bad())

    def test_list_slice_assign(self):
        self.b.card_row[:2] = [None, None]
        self.assertTrue(self.bad())

    def test_list_of_dicts_inner_change(self):
        self.a.pending.append({"player": 0, "kind": "x"})
        self.b.pending.append({"player": 1, "kind": "x"})
        self.assertIn("player", self.bad()[0])

    # set add / discard --------------------------------------------------
    def test_set_add(self):
        self.a.some_set = {1, 2}
        self.b.some_set = {1, 2, 3}
        self.assertIn("extra", self.bad()[0])

    def test_set_discard(self):
        self.a.some_set = {1, 2, 3}
        self.b.some_set = {1, 2}
        self.assertIn("missing", self.bad()[0])

    # containers swapped for another type --------------------------------
    def test_list_became_tuple(self):
        self.b.players[0].hand_civil = tuple(self.b.players[0].hand_civil)
        self.assertIn("type", self.bad()[0])

    # nesting ------------------------------------------------------------
    def test_deeply_nested_single_scalar(self):
        k = next(iter(self.b.players[2].techs))
        self.b.players[2].techs[k].stored += 1
        d = self.bad()
        self.assertIn("state.players[2].techs[", d[0])
        self.assertIn(".stored", d[0])

    def test_wonder_dataclass_field(self):
        from engine.state import WonderInProgress
        self.a.players[0].wonder = WonderInProgress("Pyramids", 1)
        self.b.players[0].wonder = WonderInProgress("Pyramids", 2)
        self.assertIn("steps_built", self.bad()[0])


class DiffAfterRealMoves(unittest.TestCase):
    """End-to-end: a real `apply` must be visible, and the oracle must agree
    with itself across a whole game."""

    def test_real_move_is_detected(self):
        from engine import actions
        import random
        st = _st()
        before = copy_state(st)
        mv = actions.legal_moves(st)[0]
        actions.apply(st, mv, random.Random(0))
        self.assertTrue(statediff.diff(before, st),
                        f"differ missed a real move: {mv}")

    def test_copy_state_is_a_faithful_oracle_all_game(self):
        """`copy_state` must equal the state it copied at every decision.

        If this ever fails, the paranoid journal check is comparing against a
        broken oracle and proves nothing.
        """
        from engine import actions
        import random
        from engine.bots import GreedyBot
        st = game.new_game(3, seed=3)
        bots = [GreedyBot(random.Random(i)) for i in range(3)]
        for n in range(120):
            if game.is_over(st):
                break
            legal = actions.legal_moves(st)
            if not legal:
                break
            self.assertEqual(statediff.diff(st, copy_state(st)), [],
                             f"copy_state diverged at move {n}")
            actions.apply(st, bots[st.decider() % 3].choose(st, legal),
                          random.Random(n))

    def test_assert_same_raises_with_a_path(self):
        st = _st()
        other = copy.deepcopy(st)
        other.players[1].science = 99
        with self.assertRaises(AssertionError) as cm:
            statediff.assert_same(st, other)
        self.assertIn("players[1].science", str(cm.exception))
        statediff.assert_same(st, copy.deepcopy(st))    # must not raise


if __name__ == "__main__":
    unittest.main()
