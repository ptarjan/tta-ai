"""WeightedBot on the undo stack, and the QuiescentBot nesting hazard.

`WeightedBot` is the bot the hill-climb league actually trains and runs
(docs/PYPY.md 9.14 measured the seat census), so it -- not `GreedyBot` -- is
where the journal has to be correct.  Two properties are pinned here:

1. **Agreement.**  `_pick_journalled` returns the same move as the copy path,
   and leaves the state structurally identical, on real mid-game positions.
2. **Isolation.**  `QuiescentBot` must stay on the copy path forever.  It holds
   several live trial states at once (`_war_value` copies a state that is
   itself already a trial) and `journal.begin` refuses to nest.  The tests
   below assert it opens no journal even with `TTA_JOURNAL=1`, that it does not
   route through `WeightedBot.pick`, and -- the important one -- that if
   somebody ever DOES make it nest, the failure is a loud `JournalError` and
   never a silently corrupted state.
"""
from __future__ import annotations

import os
import random
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from engine import actions, game, journal, state as state_mod, statediff  # noqa: E402
from engine.bots import GreedyBot                                   # noqa: E402
from engine.bots.fastcopy import copy_state                         # noqa: E402
from engine.bots.quiescent import QuiescentBot                      # noqa: E402
from engine.bots.weighted import WeightedBot                        # noqa: E402
from engine.bots import weighted as weighted_mod                    # noqa: E402


def _positions(n=3, seed=5, moves=90, every=7):
    """Real mid-game states, sampled every `every` moves of a greedy game."""
    st = game.new_game(n, seed=seed)
    bots = [GreedyBot(random.Random(i)) for i in range(n)]
    out = []
    rng = random.Random(11)
    for i in range(moves):
        if st.game_over:
            break
        mv = bots[st.decider()](st)
        actions.apply(st, mv, rng)
        if i % every == 0 and not st.game_over and len(
                actions.legal_moves(st)) > 1:
            out.append(copy_state(st, keep_log=True))
    return out


class JournalledPickAgreesWithCopyPick(unittest.TestCase):
    """The whole claim, at the level of a single decision."""

    def setUp(self):
        journal.install()
        self.addCleanup(setattr, state_mod, "SUPPRESS_LOG", False)

    def tearDown(self):
        if journal.active():
            journal._J = None
        state_mod.SUPPRESS_LOG = False

    def test_same_move_and_untouched_state(self):
        positions = _positions()
        self.assertGreater(len(positions), 5, "no positions sampled")
        checked = 0
        for st in positions:
            moves = actions.legal_moves(st)
            bot = WeightedBot(seed=0)
            idx = st.decider()
            ctx = weighted_mod.rival_context(st, idx)
            w = bot.weights
            end_bias = w.get("end_turn_bias", 0.0)

            want = bot.pick(copy_state(st, keep_log=True), list(moves))
            oracle = copy_state(st, keep_log=True)
            got = bot._pick_journalled(st, list(moves), idx, ctx, w, end_bias)

            self.assertEqual(want, got)
            diffs = statediff.diff(oracle, st, include_log=True)
            self.assertEqual(diffs, [], f"state not restored: {diffs[:3]}")
            checked += 1
        self.assertGreater(checked, 5)

    def test_an_unscorable_candidate_is_skipped_not_fatal(self):
        """WeightedBot's own semantics: `evaluate` raising must not kill the
        game.  GreedyBot's journalled loop evaluates OUTSIDE its `try`; copying
        that shape into WeightedBot would turn a skip into a crash."""
        st = _positions()[0]
        moves = actions.legal_moves(st)
        bot = WeightedBot(seed=0)
        idx = st.decider()
        real = weighted_mod.evaluate
        calls = [0]

        def flaky(*a, **k):
            calls[0] += 1
            if calls[0] % 2:
                raise ValueError("unscorable")
            return real(*a, **k)

        weighted_mod.evaluate = flaky
        try:
            oracle = copy_state(st, keep_log=True)
            mv = bot._pick_journalled(st, list(moves), idx, {}, bot.weights, 0.0)
        finally:
            weighted_mod.evaluate = real
        self.assertIn(mv, moves)
        self.assertGreater(calls[0], 1, "evaluate was never exercised")
        self.assertEqual(statediff.diff(oracle, st, include_log=True), [])

    def test_every_candidate_unscorable_still_returns_a_legal_move(self):
        st = _positions()[0]
        moves = actions.legal_moves(st)
        bot = WeightedBot(seed=0)
        real = weighted_mod.evaluate
        weighted_mod.evaluate = lambda *a, **k: (_ for _ in ()).throw(
            ValueError("nope"))
        try:
            oracle = copy_state(st, keep_log=True)
            mv = bot._pick_journalled(st, list(moves), st.decider(), {},
                                      bot.weights, 0.0)
        finally:
            weighted_mod.evaluate = real
        self.assertIn(mv, moves)
        self.assertEqual(statediff.diff(oracle, st, include_log=True), [])


class QuiescentBotStaysOnTheCopyPath(unittest.TestCase):
    """docs/PYPY.md 9.13/9.15 -- the nesting hazard, pinned.

    If somebody later gives QuiescentBot a journal, or routes it through
    `WeightedBot.pick`, one of these fails.
    """

    def setUp(self):
        journal.install()

    def tearDown(self):
        if journal.active():
            journal._J = None
        state_mod.SUPPRESS_LOG = False

    @staticmethod
    def _play(make_bot, moves=60, seed=4):
        st = game.new_game(3, seed=seed)
        bots = [make_bot(i) for i in range(3)]
        rng = random.Random(seed)
        for _ in range(moves):
            if st.game_over:
                break
            actions.apply(st, bots[st.decider()](st), rng)

    def _count_begins(self, make_bot):
        """Journals opened during a short game, with TTA_JOURNAL=1 simulated."""
        opened = [0]
        real_begin = journal.begin

        def counting(state=None):
            opened[0] += 1
            return real_begin(state)

        journal.begin = counting
        weighted_mod.journal.begin = counting
        real_flag = weighted_mod.USE_JOURNAL
        weighted_mod.USE_JOURNAL = True          # as if TTA_JOURNAL=1
        try:
            self._play(make_bot)
        finally:
            journal.begin = real_begin
            weighted_mod.journal.begin = real_begin
            weighted_mod.USE_JOURNAL = real_flag
        return opened[0]

    def test_quiescent_opens_no_journal_even_with_the_flag_on(self):
        # POSITIVE CONTROL FIRST.  9.11 records a test on this branch that
        # asserted nothing at all for a while; an isolation test that cannot
        # fail is worth less than no test.  Prove the counter is wired by
        # showing WeightedBot *does* trip it under the same harness.
        weighted = self._count_begins(lambda i: WeightedBot(seed=i))
        self.assertGreater(weighted, 0,
                           "positive control failed: the begin() counter is "
                           "not wired, so the assertion below proves nothing")
        quiescent = self._count_begins(lambda i: QuiescentBot(seed=i))
        self.assertEqual(
            quiescent, 0,
            "QuiescentBot opened a journal.  It holds several live trial "
            "states at once (_war_value copies a state that is itself a "
            "trial) and journal.begin cannot nest -- docs/PYPY.md 9.15.")

    def test_quiescent_does_not_route_through_weightedbot_pick(self):
        """The structural half: even if QuiescentBot never begins a journal
        itself, calling into `WeightedBot.pick` would begin one for it."""
        seen = [0]
        real_pick = WeightedBot.pick

        def spy(self, state, moves):
            seen[0] += 1
            return real_pick(self, state, moves)

        WeightedBot.pick = spy
        try:
            self._play(lambda i: WeightedBot(seed=i))     # positive control
            control = seen[0]
            seen[0] = 0
            self._play(lambda i: QuiescentBot(seed=i))
        finally:
            WeightedBot.pick = real_pick
        self.assertGreater(control, 0, "positive control failed: the spy on "
                                       "WeightedBot.pick is not wired")
        self.assertEqual(seen[0], 0,
                         "QuiescentBot entered WeightedBot.pick; with the "
                         "journal on that is a nested begin()")

    def test_nesting_fails_loudly_rather_than_silently(self):
        """The safety net.  Whatever anyone does later, a nested journalled
        search must raise, not corrupt.  `copy_state` inside an open journal
        raises too, so there is no quiet fallback to hide behind -- which is
        the point."""
        st = _positions()[0]
        moves = actions.legal_moves(st)
        bot = WeightedBot(seed=0)
        j = journal.begin(st)
        try:
            with self.assertRaises(journal.JournalError):
                bot._pick_journalled(st, list(moves), st.decider(), {},
                                     bot.weights, 0.0)
        finally:
            journal.rollback(j)

    def test_quiescent_search_is_unaffected_by_an_installed_hook(self):
        """QuiescentBot's copy-inside-a-trial pattern still works once the
        journalling `__setattr__` is installed process-wide (which it is, as
        soon as any WeightedBot search runs)."""
        journal.install()
        self.assertTrue(journal._installed)
        st = game.new_game(3, seed=9)
        bots = [QuiescentBot(seed=i) for i in range(3)]
        rng = random.Random(9)
        for _ in range(40):
            if st.game_over:
                break
            actions.apply(st, bots[st.decider()](st), rng)
        self.assertGreater(st.round, 0)


if __name__ == "__main__":
    unittest.main()
