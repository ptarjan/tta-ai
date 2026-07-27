"""PlanBot's war lookahead (engine/bots/plan.py, defect 4).

The point of these tests is narrow: PlanBot and QuiescentBot must price an
unresolved war *the same way*, because docs/TRANSFER_TEST.md measured that a
weight vector trained under one search does not transfer to the other when
they disagree about a move class.  So the assertions are equalities against
`quiescent.war_value` rather than "the number went up".

Positions are built directly instead of searched for: war declarations are
rare enough in sampled play that waiting for one makes a flaky test (the same
reasoning as tests/test_quiescent.py's war test).
"""
import unittest

from engine import actions, events, game
from engine.bots.fastcopy import copy_state
from engine.bots.plan import PlanBot
from engine.bots import quiescent as Q
from engine.bots.weighted import DEFAULT_WEIGHTS, evaluate, rival_context


def _war_card():
    return next((c["name"] for c in actions._DB.cards
                 if c.get("type") == "war"), None)


class PlanWarLookahead(unittest.TestCase):

    def setUp(self):
        self.st = game.new_game(2, seed=77)
        self.war = _war_card()
        self.assertIsNotNone(self.war, "no war card in the DB")
        self.ctx = rival_context(self.st, 0)

    def test_matches_the_quiescent_helper_exactly(self):
        st = self.st
        st.players[0].war_declared_by_me = (self.war, 0, 1)
        bot = PlanBot(seed=1)
        got = bot._score(st, 0, DEFAULT_WEIGHTS, self.ctx)
        want = Q.war_value(st, 0, DEFAULT_WEIGHTS, self.ctx)
        self.assertIsNotNone(want)
        self.assertAlmostEqual(got, want, places=9)
        # and it is the engine's own resolution, not a priced guess
        scratch = copy_state(st)
        events.resolve_war(scratch, scratch.players[0], None)
        self.assertAlmostEqual(got, evaluate(scratch, 0, DEFAULT_WEIGHTS,
                                             self.ctx), places=9)
        self.assertEqual(bot.wars_priced, 1)

    def test_scoring_does_not_mutate_the_position(self):
        st = self.st
        st.players[0].war_declared_by_me = (self.war, 0, 1)
        PlanBot(seed=1)._score(st, 0, DEFAULT_WEIGHTS, self.ctx)
        self.assertEqual(st.players[0].war_declared_by_me, (self.war, 0, 1))
        # scoring twice must give the same answer -- i.e. the spoils are not
        # accumulating into the state the next ply would expand
        b = PlanBot(seed=1)
        a1 = b._score(st, 0, DEFAULT_WEIGHTS, self.ctx)
        a2 = b._score(st, 0, DEFAULT_WEIGHTS, self.ctx)
        self.assertAlmostEqual(a1, a2, places=9)

    def test_no_war_is_plain_evaluate(self):
        st = self.st
        self.assertIsNone(st.players[0].war_declared_by_me)
        bot = PlanBot(seed=1)
        self.assertAlmostEqual(bot._score(st, 0, DEFAULT_WEIGHTS, self.ctx),
                               evaluate(st, 0, DEFAULT_WEIGHTS, self.ctx),
                               places=9)
        self.assertEqual(bot.wars_priced, 0)

    def test_flag_off_is_the_old_behaviour(self):
        st = self.st
        st.players[0].war_declared_by_me = (self.war, 0, 1)
        bot = PlanBot(seed=1, war_lookahead=False)
        self.assertAlmostEqual(bot._score(st, 0, DEFAULT_WEIGHTS, self.ctx),
                               evaluate(st, 0, DEFAULT_WEIGHTS, self.ctx),
                               places=9)
        self.assertEqual(bot.wars_priced, 0)

    def test_game_over_is_not_priced(self):
        """A war declared into a finished game never resolves."""
        st = self.st
        st.players[0].war_declared_by_me = (self.war, 0, 1)
        st.game_over = True
        bot = PlanBot(seed=1)
        self.assertAlmostEqual(bot._score(st, 0, DEFAULT_WEIGHTS, self.ctx),
                               evaluate(st, 0, DEFAULT_WEIGHTS, self.ctx),
                               places=9)
        self.assertEqual(bot.wars_priced, 0)

    def test_only_my_own_war_counts(self):
        """A war declared ON me resolves on the declarer's turn, not mine."""
        st = self.st
        st.players[0].wars_declared_on_me = [(self.war, 1, 0)]
        st.players[1].war_declared_by_me = (self.war, 1, 0)
        bot = PlanBot(seed=1)
        self.assertAlmostEqual(bot._score(st, 0, DEFAULT_WEIGHTS, self.ctx),
                               evaluate(st, 0, DEFAULT_WEIGHTS, self.ctx),
                               places=9)
        self.assertEqual(bot.wars_priced, 0)

    def test_spec_parses_war_flag(self):
        from experiments import arena
        spec = arena.load_spec("plan:default,width=2,war=0")
        self.assertEqual(spec[0], "plan")
        self.assertEqual(spec[2].get("war"), 0)
        self.assertFalse(arena.make_bot(spec, 3).WAR_LOOKAHEAD)
        self.assertTrue(arena.make_bot(
            arena.load_spec("plan:default,width=2"), 3).WAR_LOOKAHEAD)

    def test_still_returns_legal_moves(self):
        st = game.new_game(2, seed=5)
        for _ in range(40):
            if st.game_over:
                break
            moves = actions.legal_moves(st)
            if not moves:
                break
            mv = PlanBot(seed=7, width=2).pick(st, moves)
            self.assertIn(mv, moves)
            actions.apply(st, mv, None)


if __name__ == "__main__":
    unittest.main()
