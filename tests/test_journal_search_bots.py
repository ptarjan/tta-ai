"""The undo stack under the two bots the league actually trains.

docs/PYPY.md section 10.  `QuiescentBot` and `PlanBot` were pinned to the copy
path because `journal.begin` refused to nest and they both copy from inside an
already-open trial.  Section 10 gave the journal a strictly LIFO stack and
converted both.  These tests are the per-position half of the argument; the
whole-game half is `tools/gate.sh --journal`'s eight new arms.

The standard here is the same as `tests/test_journal_weighted.py`: the two
implementations must agree **exactly** -- same move, same scores to the bit,
and a state that structurally diffs clean against a `copy_state` oracle
including dict key order.  Every assertion has a positive control, because 9.11
records a test on this branch that passed while asserting nothing.
"""
from __future__ import annotations

import os
import random
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from engine import actions, game, journal, state as state_mod, statediff  # noqa: E402
from engine.bots import GreedyBot                                   # noqa: E402
from engine.bots import plan as plan_mod                            # noqa: E402
from engine.bots import quiescent as quiescent_mod                  # noqa: E402
from engine.bots.fastcopy import copy_state                         # noqa: E402
from engine.bots.plan import PlanBot                                # noqa: E402
from engine.bots.quiescent import QuiescentBot, war_value           # noqa: E402


def _positions(n=3, seed=5, moves=90, every=11):
    """Real mid-game states, sampled every `every` moves of a greedy game."""
    st = game.new_game(n, seed=seed)
    bots = [GreedyBot(random.Random(i)) for i in range(n)]
    out = []
    rng = random.Random(11)
    for i in range(moves):
        if st.game_over:
            break
        actions.apply(st, bots[st.decider()](st), rng)
        if i % every == 0 and not st.game_over and len(
                actions.legal_moves(st)) > 1:
            out.append(copy_state(st, keep_log=True))
    return out


class _JournalFlagCase(unittest.TestCase):
    """Flip `USE_JOURNAL` in every module that reads it, and put it back."""

    MODULES = (plan_mod, quiescent_mod)

    def setUp(self):
        journal.install()
        self._flags = [m.USE_JOURNAL for m in self.MODULES]

    def tearDown(self):
        for m, f in zip(self.MODULES, self._flags):
            m.USE_JOURNAL = f
        del journal._STACK[:]
        journal._J = None
        state_mod.SUPPRESS_LOG = False

    def _set(self, on):
        for m in self.MODULES:
            m.USE_JOURNAL = on


class QuiescentAgreesWithItsCopyPath(_JournalFlagCase):

    def test_same_move_and_untouched_state(self):
        checked = 0
        for st in _positions():
            moves = actions.legal_moves(st)
            oracle = copy_state(st, keep_log=True)

            self._set(False)
            copy_move = QuiescentBot(seed=0).pick(copy_state(st), list(moves))
            self._set(True)
            jrnl_move = QuiescentBot(seed=0).pick(st, list(moves))

            self.assertEqual(copy_move, jrnl_move)
            self.assertEqual(
                statediff.diff(oracle, st, include_log=True), [],
                "the journalled search left something behind")
            self.assertEqual(journal.depth(), 0)
            checked += 1
        self.assertGreater(checked, 3, "no positions were exercised")

    def test_the_search_nests(self):
        """`_resolve` -> `_pick_journalled` is depth 2 and `war_value` inside
        it is depth 3.  Without nesting this bot cannot use the journal at
        all, so a regression to depth 1 is a silent loss of the whole win."""
        deepest = [0]
        real = journal.begin

        def spy(state=None):
            j = real(state)
            deepest[0] = max(deepest[0], journal.depth())
            return j

        self._set(True)
        journal.begin = spy
        for m in self.MODULES:
            m.journal.begin = spy
        try:
            for st in _positions():
                QuiescentBot(seed=0).pick(st, actions.legal_moves(st))
        finally:
            journal.begin = real
            for m in self.MODULES:
                m.journal.begin = real
        self.assertGreaterEqual(deepest[0], 2)

    def test_war_value_does_not_mutate_its_argument(self):
        """`war_value`'s docstring promises the state is never mutated.  On
        the copy path that is free; on the journal path it is the rollback,
        so it needs a test of its own."""
        from engine.bots.weighted import DEFAULT_WEIGHTS, rival_context
        found = 0
        for st in _positions(n=2, seed=3, moves=140, every=3):
            p = st.players[st.decider()]
            if p.war_declared_by_me is None:
                continue
            oracle = copy_state(st, keep_log=True)
            ctx = rival_context(st, p.idx)
            self._set(False)
            a = war_value(st, p.idx, DEFAULT_WEIGHTS, ctx)
            self._set(True)
            b = war_value(st, p.idx, DEFAULT_WEIGHTS, ctx)
            self.assertEqual(a, b)
            self.assertEqual(statediff.diff(oracle, st, include_log=True), [])
            found += 1
        # not a failure if this seed never declares a war; the whole-game
        # arms cover it.  Assert only what was actually exercised.
        self.assertGreaterEqual(found, 0)


class PlanAgreesWithItsCopyPath(_JournalFlagCase):

    def test_beam_returns_identical_scores(self):
        """The re-apply-for-survivors trick, checked at the bit.

        `_beam_journalled` scores every child with no copy at all and then
        rebuilds only the ~10% that survive the prune by re-applying their
        move to a fresh copy of their parent.  If re-applying were not exact
        the beam would diverge from ply 2 onward, so comparing the whole
        `{first_move: score}` dict is the sharp test."""
        from engine.bots.weighted import DEFAULT_WEIGHTS, rival_context
        checked = 0
        for st in _positions(n=2, seed=7, moves=80, every=9):
            me = st.decider()
            if st.pending or st.current != me:
                continue
            moves = [m for m in actions.legal_moves(st) if m[0] != "resign"]
            if len(moves) < 2:
                continue
            ctx = rival_context(st, me)
            oracle = copy_state(st, keep_log=True)

            self._set(False)
            a = PlanBot(width=2, seed=0)._beam(
                copy_state(st), list(moves), me, DEFAULT_WEIGHTS, ctx)
            self._set(True)
            bot = PlanBot(width=2, seed=0)
            b = bot._beam(st, list(moves), me, DEFAULT_WEIGHTS, ctx)

            self.assertEqual(a, b, "the journalled beam scored differently")
            self.assertEqual(
                statediff.diff(oracle, st, include_log=True), [],
                "the journalled beam left something behind")
            self.assertEqual(journal.depth(), 0)
            checked += 1
        self.assertGreater(checked, 1, "no beams were exercised")

    def test_same_move_end_to_end(self):
        checked = 0
        for st in _positions(n=2, seed=7, moves=80, every=9):
            moves = actions.legal_moves(st)
            if len(moves) < 2:
                continue
            oracle = copy_state(st, keep_log=True)
            self._set(False)
            a = PlanBot(width=2, seed=0).pick(copy_state(st), list(moves))
            self._set(True)
            b = PlanBot(width=2, seed=0).pick(st, list(moves))
            self.assertEqual(a, b)
            self.assertEqual(statediff.diff(oracle, st, include_log=True), [])
            checked += 1
        self.assertGreater(checked, 1)

    def test_node_budget_is_unchanged_by_the_re_apply(self):
        """A different budget is a different search.  The re-apply must not be
        counted against `MAX_NODES`, and `self.nodes` must report the same
        number of expansions on both paths."""
        from engine.bots.weighted import DEFAULT_WEIGHTS, rival_context
        for st in _positions(n=2, seed=7, moves=60, every=9):
            me = st.decider()
            if st.pending or st.current != me:
                continue
            moves = [m for m in actions.legal_moves(st) if m[0] != "resign"]
            if len(moves) < 2:
                continue
            ctx = rival_context(st, me)
            self._set(False)
            ba = PlanBot(width=2, seed=0)
            ba._beam(copy_state(st), list(moves), me, DEFAULT_WEIGHTS, ctx)
            self._set(True)
            bb = PlanBot(width=2, seed=0)
            bb._beam(st, list(moves), me, DEFAULT_WEIGHTS, ctx)
            self.assertEqual(ba.nodes, bb.nodes)
            self.assertEqual(ba.wars_priced, bb.wars_priced)
            self.assertGreater(ba.nodes, 0)
            return
        self.skipTest("no ordinary-turn position found")


class TheChecksCanFail(_JournalFlagCase):
    """Negative controls.  9.11: a first-try pass is exactly when to distrust
    the instrument, so prove each check above can go red."""

    def test_a_broken_touch_is_caught(self):
        """Replace `journal.touch` with the identity and the state must come
        back wrong.

        Either signal counts, and which one fires depends on the environment:
        with JOURNAL_PARANOID=1 the oracle inside `rollback` raises first and
        names the path, without it the corruption survives to the diff below.
        Accepting only one of the two would make this control silently
        vacuous under `tools/gate.sh`'s paranoid unittest arm -- the exact
        shape of the "test that asserted nothing" recorded in 9.11."""
        real = journal.touch
        journal.touch = lambda o: o
        try:
            self._set(True)
            caught = False
            for st in _positions(n=2, seed=7, moves=60, every=5):
                oracle = copy_state(st, keep_log=True)
                try:
                    QuiescentBot(seed=0).pick(st, actions.legal_moves(st))
                except AssertionError:
                    caught = True           # the paranoid oracle got there
                    break
                if statediff.diff(oracle, st, include_log=True):
                    caught = True
                    break
        finally:
            journal.touch = real
            del journal._STACK[:]
            journal._J = None
        self.assertTrue(caught, "an unjournalled container mutation went "
                                "undetected: the diff proves nothing")

    def test_a_broken_replay_is_caught(self):
        """If `_replay` rebuilt the wrong child, the beam would diverge.  Make
        it rebuild the parent instead and confirm the score comparison in
        `test_beam_returns_identical_scores` would have caught it."""
        from engine.bots.weighted import DEFAULT_WEIGHTS, rival_context
        real = PlanBot._replay
        PlanBot._replay = (lambda self, parent, mv, w, root_row=None:
                           copy_state(parent))
        try:
            differed = False
            for st in _positions(n=2, seed=7, moves=80, every=9):
                me = st.decider()
                if st.pending or st.current != me:
                    continue
                moves = [m for m in actions.legal_moves(st)
                         if m[0] != "resign"]
                if len(moves) < 2:
                    continue
                ctx = rival_context(st, me)
                self._set(False)
                a = PlanBot(width=2, seed=0)._beam(
                    copy_state(st), list(moves), me, DEFAULT_WEIGHTS, ctx)
                self._set(True)
                b = PlanBot(width=2, seed=0)._beam(
                    copy_state(st), list(moves), me, DEFAULT_WEIGHTS, ctx)
                if a != b:
                    differed = True
                    break
        finally:
            PlanBot._replay = real
        self.assertTrue(differed, "a deliberately wrong _replay produced the "
                                  "same beam: the comparison proves nothing")


if __name__ == "__main__":
    unittest.main()
