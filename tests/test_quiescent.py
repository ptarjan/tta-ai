"""QuiescentBot: the invariants its A/B result rests on.

`docs/DEEPER_SEARCH.md` measures QuiescentBot's strength, and until this file
existed the bot had no tests of its own at all -- `tests/test_journal_weighted`
only pins the negative property that it never enters the journal.  Four things
have to hold before any win rate from it means anything:

1. **It does not corrupt the real state.**  The whole search runs on copies;
   if a single `apply` escaped onto the live state the game would silently
   diverge and every measurement taken with it would be worthless.
2. **`LEVELS = 0`, war lookahead off, is exactly `WeightedBot`.**  That is the
   A/B control the module docstring claims, and it is only true if the two
   loops agree move for move -- including their trial-rng discipline, which
   they implement with two separate (and separately reseeded) `Random(0)`
   pools.
3. **Quiescence actually fires, and reaches quiet.**  A bot whose stats say
   `quiesced == 0` is 1-ply with extra steps.
4. **A budget of zero degrades to 1-ply, not to a crash.**  That is the
   documented fallback, and the whole "a miss degrades to today's behaviour"
   argument depends on it.
"""
from __future__ import annotations

import os
import random
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from engine import actions, game, statediff                     # noqa: E402
from engine.bots import GreedyBot                               # noqa: E402
from engine.bots.fastcopy import copy_state                     # noqa: E402
from engine.bots.quiescent import QuiescentBot                  # noqa: E402
from engine.bots.weighted import WeightedBot                    # noqa: E402


def _positions(n=3, seed=5, moves=120, every=6):
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


class TestQuiescent(unittest.TestCase):

    def test_search_does_not_mutate_the_real_state(self):
        """Every apply the search performs must land on a copy."""
        for st in _positions():
            before = copy_state(st, keep_log=True)
            QuiescentBot(seed=1).pick(st, actions.legal_moves(st))
            diff = statediff.diff(before, st)
            self.assertEqual(diff, [], f"search mutated the live state: {diff}")

    def test_levels_zero_is_weighted_bot(self):
        """The A/B control: LEVELS=0 + no war lookahead == 1-ply WeightedBot."""
        q = QuiescentBot(seed=1, levels=0, war_lookahead=False)
        w = WeightedBot(seed=1)
        checked = 0
        for st in _positions():
            moves = actions.legal_moves(st)
            self.assertEqual(q.pick(st, moves), w.pick(st, moves))
            checked += 1
        self.assertGreater(checked, 4)

    def test_quiescence_fires_and_reaches_quiet(self):
        """Over a real game the search resolves pending stacks, and finishes."""
        bots = [QuiescentBot(seed=100 + i) for i in range(3)]
        game.play_game(bots, num_players=3, seed=4242, move_cap=20000)
        st = {}
        for b in bots:
            for k, v in b.stats.items():
                st[k] = st.get(k, 0) + v
        self.assertGreater(st["quiesced"], 0, "quiescence never fired")
        self.assertGreater(st["qnodes"], 0)
        # the budgets are documented as non-binding at their current values
        self.assertLess(st["truncated"], 0.25 * st["quiesced"])

    def test_zero_budget_degrades_to_one_ply(self):
        """MAX_NODES=0 must fall back, not raise -- the documented fallback."""
        for st in _positions(n=2, seed=9, moves=60):
            moves = actions.legal_moves(st)
            mv = QuiescentBot(seed=3, max_nodes=0).pick(st, moves)
            self.assertIn(mv, moves)

    def test_returns_a_legal_move_at_every_level(self):
        for levels in (0, 1, 2):
            for st in _positions(n=2, seed=13, moves=60):
                moves = actions.legal_moves(st)
                mv = QuiescentBot(seed=7, levels=levels).pick(st, moves)
                self.assertIn(mv, moves)

    def test_war_lookahead_prices_the_spoils(self):
        """A declared war is scored through the engine's own resolve_war.

        Built directly rather than searched for: war declarations are rare
        enough in sampled play that waiting for one makes a flaky test.
        """
        from engine import events
        from engine.bots import quiescent as Q
        from engine.bots.weighted import DEFAULT_WEIGHTS, evaluate, rival_context

        st = game.new_game(2, seed=77)
        p = st.players[0]
        # a war the engine can resolve: attacker outguns the defender
        war = next((c["name"] for c in actions._DB.cards
                    if c.get("type") == "war"), None)
        self.assertIsNotNone(war, "no war card in the DB")
        p.war_declared_by_me = (war, 0, 1)
        ctx = rival_context(st, 0)
        plain = evaluate(st, 0, DEFAULT_WEIGHTS, ctx)
        looked = Q._war_value(st, 0, DEFAULT_WEIGHTS, ctx)
        self.assertIsNotNone(looked)
        # resolve_war is deterministic, so the lookahead must equal evaluating
        # the state the engine itself would produce
        scratch = copy_state(st)
        events.resolve_war(scratch, scratch.players[0], None)
        self.assertAlmostEqual(looked,
                               evaluate(scratch, 0, DEFAULT_WEIGHTS, ctx),
                               places=9)
        # and it must not have touched the state it was asked about
        self.assertEqual(st.players[0].war_declared_by_me, (war, 0, 1))
        self.assertIsInstance(plain, float)

    def test_war_over_technology_is_priced_at_its_science_value(self):
        """`resolve_war` is no longer total: `War over Technology` leaves the
        victor a decision (Code of Laws p.3).  A lookahead that scored the
        position with that decision outstanding would price the war at ZERO,
        so `war_value` settles it -- as science, deliberately, which is a
        lower bound and is exactly how the war was priced before the choice
        existed.
        """
        from engine import effects, events, interact
        from engine.bots import quiescent as Q
        from engine.bots.weighted import DEFAULT_WEIGHTS, evaluate, rival_context
        from engine.state import TechCard

        st = game.new_game(2, seed=77)
        p, q = st.players[0], st.players[1]
        p.war_declared_by_me = ("War over Technology", 0, 1)
        q.wars_declared_on_me = [("War over Technology", 0, 1)]
        q.techs["Code of Laws"] = TechCard("Code of Laws")   # stealable
        q.science = 30
        p.techs["Warriors"].workers = 12                     # a big advantage
        effects.invalidate(st)
        ctx = rival_context(st, 0)

        # the choice really is live in this position
        probe = copy_state(st)
        events.resolve_war(probe, probe.players[0], None)
        self.assertTrue(probe.pending)
        self.assertEqual(probe.pending[-1]["tag"], "war_tech")
        self.assertEqual(probe.players[0].science, 0)        # nothing yet

        looked = Q.war_value(st, 0, DEFAULT_WEIGHTS, ctx)
        self.assertIsNotNone(looked)
        interact.settle_war_spoils(probe, None)
        self.assertEqual(probe.pending, [])
        self.assertGreater(probe.players[0].science, 0)      # spoils landed
        self.assertAlmostEqual(looked,
                               evaluate(probe, 0, DEFAULT_WEIGHTS, ctx),
                               places=9)
        # and the position it was asked about is untouched
        self.assertEqual(st.players[0].science, 0)
        self.assertIn("Code of Laws", st.players[1].techs)


if __name__ == "__main__":
    unittest.main()
