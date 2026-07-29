"""NeuralPlanBot invariants, exercised with a FAKE evaluator (no torch).

`engine/bots/neural_plan.py` never imports torch -- the evaluator is injected
as anything with `.value(list_of_encodings) -> list[float]` -- so the beam
itself is testable on a machine without torch, which is where `tools/gate.sh`
runs.  A fixed random projection of the encoding stands in for the net: it is
deterministic, it is not constant (so the beam actually has to order things),
and it is not the linear evaluator (so nothing here can pass by accident
because PlanBot would have passed).
"""
import random
import unittest

from engine import actions, game
from engine.bots.neural_encode import ENCODING_DIM
from engine.bots.neural_plan import NeuralPlanBot


class FakeValue:
    """Deterministic pseudo-net: a fixed random projection of the encoding."""

    def __init__(self, seed=7):
        r = random.Random(seed)
        self.w = [r.uniform(-1.0, 1.0) for _ in range(ENCODING_DIM)]
        self.calls = 0
        self.rows = 0

    def value(self, encodings):
        self.calls += 1
        self.rows += len(encodings)
        w = self.w
        return [sum(a * b for a, b in zip(e, w)) for e in encodings]


class ConstValue:
    """Every position is worth the same -- the beam must still return a legal
    move rather than crashing on ties or falling through to comparing move
    tuples against each other."""

    def value(self, encodings):
        return [0.0] * len(encodings)


def _advance(n=2, seed=3, plies=25):
    """A mid-game state, driven by random legal moves."""
    rng = random.Random(seed)
    st = game.new_game(n, seed=seed)
    for _ in range(plies):
        mv = [m for m in actions.legal_moves(st) if m[0] != "resign"]
        if not mv or st.game_over:
            break
        actions.apply(st, rng.choice(mv), rng)
    return st


def _planning_state(n=2, plies=25):
    """A state where the bot actually gets to plan: my ordinary turn, nothing
    pending, more than one candidate.  Otherwise `pick` short-circuits and the
    beam is never entered."""
    for seed in range(1, 60):
        st = _advance(n, seed, plies)
        if st.game_over or st.pending:
            continue
        moves = [m for m in actions.legal_moves(st) if m[0] != "resign"]
        if len(moves) > 1 and st.current == st.decider():
            return st, moves
    raise unittest.SkipTest("no planning state found")


class TestNeuralPlanBot(unittest.TestCase):

    def test_picks_a_legal_move(self):
        v = FakeValue()
        bot = NeuralPlanBot(v, seed=1, width=4, max_nodes=300)
        for seed in (1, 2, 3, 5, 8):
            st = _advance(2, seed)
            if st.game_over:
                continue
            moves = actions.legal_moves(st)
            self.assertIn(bot.pick(st, moves), moves)
        self.assertGreater(v.rows, 0, "the beam never called the evaluator")

    def test_batches_the_evaluator(self):
        """Every ply is scored in ONE call, not one call per node -- that is
        the whole reason this class exists rather than reusing PlanBot."""
        v = FakeValue()
        bot = NeuralPlanBot(v, seed=1, width=8, max_nodes=400)
        st, moves = _planning_state()
        bot.pick(st, moves)
        self.assertGreater(v.rows, 0)
        # one batched call per PLY, not one per node
        self.assertLessEqual(v.calls, bot.MAX_PLIES + 1)
        self.assertLess(v.calls, bot.nodes)

    def test_deterministic_for_a_fixed_seed(self):
        st = _advance(2, 4)
        if st.game_over:
            self.skipTest("random walk ended the game")
        moves = actions.legal_moves(st)
        a = NeuralPlanBot(FakeValue(), seed=42, width=4, max_nodes=300)
        b = NeuralPlanBot(FakeValue(), seed=42, width=4, max_nodes=300)
        self.assertEqual(a.pick(st, moves), b.pick(st, moves))

    def test_constant_evaluator_still_returns_a_legal_move(self):
        st = _advance(2, 6)
        if st.game_over:
            self.skipTest("random walk ended the game")
        moves = actions.legal_moves(st)
        bot = NeuralPlanBot(ConstValue(), seed=1, width=4, max_nodes=200)
        self.assertIn(bot.pick(st, moves), moves)

    def test_never_returns_resign_when_anything_else_is_legal(self):
        st = _advance(2, 9)
        if st.game_over:
            self.skipTest("random walk ended the game")
        moves = actions.legal_moves(st)
        if not any(m[0] == "resign" for m in moves) or len(moves) < 2:
            self.skipTest("no resign candidate at this state")
        bot = NeuralPlanBot(FakeValue(), seed=1, width=4, max_nodes=200)
        self.assertNotEqual(bot.pick(st, moves)[0], "resign")

    def test_plays_a_whole_game(self):
        bots = [NeuralPlanBot(FakeValue(seed=i + 1), seed=i, width=3,
                              max_nodes=150) for i in range(2)]
        st = game.play_game(bots, 2, seed=17, move_cap=4000)
        self.assertTrue(st.game_over or st.moves_played >= 1)
        self.assertEqual(len(game.scores(st)), 2)

    def test_width_one_is_still_a_search(self):
        """width=1 keeps the horizon fix and drops the lookahead; it must still
        expand past ply 1 (docs/BOT_ARCHITECTURE.md measures this rung at
        62.3% against a 1-ply bot on identical weights)."""
        v = FakeValue()
        bot = NeuralPlanBot(v, seed=1, width=1, max_nodes=300)
        st = _advance(2, 13)
        if st.game_over:
            self.skipTest("random walk ended the game")
        bot.pick(st, actions.legal_moves(st))
        self.assertGreaterEqual(bot.searches, 1)

    def test_war_lookahead_toggle_runs(self):
        for war in (True, False):
            bot = NeuralPlanBot(FakeValue(), seed=1, width=3, max_nodes=200,
                                war_lookahead=war)
            st = _advance(2, 21)
            if st.game_over:
                continue
            moves = actions.legal_moves(st)
            self.assertIn(bot.pick(st, moves), moves)


class TestNplanSpec(unittest.TestCase):
    """`nplan:` must PARSE without torch; only make_bot needs it."""

    def test_load_spec_is_torch_free(self):
        from experiments import arena
        spec = arena.load_spec("nplan:checkpoints/best.pt,width=8,det=1,war=1")
        self.assertEqual(spec[0], "nplan")
        self.assertEqual(spec[1], "checkpoints/best.pt")
        self.assertEqual(spec[2]["width"], "8")


if __name__ == "__main__":
    unittest.main()
