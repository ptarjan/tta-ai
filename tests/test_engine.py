"""Engine tests, keyed to docs/RULES_SPEC.md sections.

Written as plain ``unittest`` so they run either way::

    python3 -m unittest discover -s tests
    python3 -m pytest tests
"""
from __future__ import annotations

import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from engine import actions, cards as C, economy, effects, game  # noqa: E402
from engine.bots import GreedyBot, RandomBot                     # noqa: E402


# --------------------------------------------------------------- §6 tables

class TestEconomyTables(unittest.TestCase):
    def test_pop_cost_bands(self):
        # §6.1: 2 / 3 / 4 / 5 / 7 as the bank empties
        expect = {18: 2, 17: 2, 16: 3, 13: 3, 12: 4, 9: 4, 8: 5, 5: 5,
                  4: 7, 1: 7}
        for bank, cost in expect.items():
            self.assertEqual(economy.pop_cost_base(bank), cost, bank)
        self.assertIsNone(economy.pop_cost_base(0))

    def test_consumption_bands(self):
        expect = {18: 0, 17: 0, 16: 1, 13: 1, 12: 2, 9: 2, 8: 3, 5: 3,
                  4: 4, 1: 4, 0: 6}
        for bank, c in expect.items():
            self.assertEqual(economy.consumption(bank), c, bank)

    def test_happy_required_bands(self):
        expect = {18: 0, 17: 0, 16: 1, 13: 1, 12: 2, 11: 2, 10: 3, 9: 3,
                  8: 4, 7: 4, 6: 5, 5: 5, 4: 6, 3: 6, 2: 7, 1: 7, 0: 8}
        for bank, h in expect.items():
            self.assertEqual(economy.happy_required(bank), h, bank)

    def test_corruption_bands(self):
        for blue, corr in {16: 0, 11: 0, 10: 2, 6: 2, 5: 4, 1: 4, 0: 6}.items():
            self.assertEqual(economy.corruption(blue), corr, blue)

    def test_row_cost_bands(self):
        # §2.3: spaces 1-5 cost 1, 6-9 cost 2, 10-13 cost 3
        self.assertEqual([actions.row_cost(i) for i in range(13)],
                         [1] * 5 + [2] * 4 + [3] * 4)


# ------------------------------------------------------------------ setup

class TestSetup(unittest.TestCase):
    def test_starting_tableau(self):
        st = game.new_game(4, seed=1)
        p = st.players[0]
        self.assertEqual(p.government, "Despotism")
        self.assertEqual(sorted(p.techs), sorted(game.START_TECHS))
        # §1.2: 25 yellow tokens (18 bank + 6 on cards + 1 unused worker)
        self.assertEqual(p.yellow_bank + p.workers_free
                         + sum(t.workers for t in p.techs.values()), 25)
        self.assertEqual(p.blue_total, 16)
        s = effects.compute(st, p)
        self.assertEqual(s.science, 1)      # Philosophy
        self.assertEqual(s.strength, 1)     # Warriors
        self.assertEqual(s.culture, 0)
        self.assertEqual(s.civil_actions, 4)
        self.assertEqual(s.military_actions, 2)
        self.assertEqual(s.urban_limit, 2)

    def test_first_round_action_totals(self):
        # §1.9: 1/2/3/4 civil actions by seat, no military actions
        st = game.new_game(4, seed=1)
        self.assertEqual([p.civil_actions for p in st.players], [1, 2, 3, 4])
        self.assertEqual([p.military_actions for p in st.players], [0] * 4)

    def test_first_round_moves_are_takes_only(self):
        st = game.new_game(3, seed=2)
        kinds = {m[0] for m in actions.legal_moves(st)}
        self.assertLessEqual(kinds, {"take", "end_turn"})

    def test_card_row_full_and_events_seeded(self):
        for n in (2, 3, 4):
            st = game.new_game(n, seed=n)
            self.assertEqual(len(st.card_row), actions.ROW_SIZE)
            self.assertTrue(all(c is not None for c in st.card_row))
            if st.has_military:
                self.assertEqual(len(st.current_events), n + 2)

    def test_card_db_names_unique(self):
        db = C.db()
        self.assertEqual(len(db.by_name), len(db.cards))


# ------------------------------------------------------- move legality

class TestLegality(unittest.TestCase):
    def test_apply_rejects_illegal_move(self):
        st = game.new_game(2, seed=3)
        with self.assertRaises(AssertionError):
            actions.apply(st, ("pop",))

    def test_no_wonder_take_while_unfinished(self):
        st = game.new_game(4, seed=4)
        db = C.db()
        p = st.players[0]
        p.civil_actions = 10
        wonders = [i for i, n in enumerate(st.card_row)
                   if n and db.type_of(n) == "wonder"]
        if not wonders:
            self.skipTest("no wonder in the opening row")
        actions.apply(st, ("take", wonders[0]))
        self.assertIsNotNone(p.wonder)
        for i, n in enumerate(st.card_row):
            if n and db.type_of(n) == "wonder":
                self.assertFalse(actions.can_take(st, p, i))

    def test_civil_hand_limit(self):
        # §2.5: cannot take when hand size >= civil action total
        st = game.new_game(4, seed=5)
        p = st.players[0]
        p.hand_civil = [n for n in st.card_row
                        if C.db().type_of(n) not in ("wonder",)][:4]
        effects.invalidate(st, p)
        for i, n in enumerate(st.card_row):
            if n and C.db().type_of(n) != "wonder":
                self.assertFalse(actions.can_take(st, p, i))

    def test_take_costs_the_advertised_actions(self):
        st = game.new_game(4, seed=6)
        st.players[0].civil_actions = 4
        p = st.players[0]
        before = p.civil_actions
        cost = actions.take_cost(st, p, 0)
        actions.apply(st, ("take", 0))
        self.assertEqual(p.civil_actions, before - cost)
        self.assertIsNone(st.card_row[0])

    def test_random_games_only_use_generated_moves(self):
        # STRICT apply() asserts legality, so a clean game proves the
        # generator and the transition function agree.
        for seed in range(5):
            st = game.play_game([RandomBot(seed=seed + i) for i in range(3)],
                                3, seed)
            self.assertTrue(st.game_over)


# --------------------------------------------------------- full games

def invariants(test, st):
    db = C.db()
    for p in st.players:
        test.assertGreaterEqual(p.food, 0, "food went negative")
        test.assertGreaterEqual(p.resources, 0, "resources went negative")
        test.assertGreaterEqual(p.science, 0, "science went negative")
        test.assertGreaterEqual(p.culture, 0, "culture went negative")
        test.assertGreaterEqual(p.yellow_bank, 0)
        test.assertGreaterEqual(p.workers_free, 0)
        test.assertGreaterEqual(p.civil_actions, 0)
        test.assertGreaterEqual(p.military_actions, 0)
        # blue tokens: what food/resources/wonder stages occupy can never
        # exceed what the player owns (§6.4)
        test.assertLessEqual(effects.blue_used(p), p.blue_total,
                             "blue token overdraft")
        test.assertGreaterEqual(effects.blue_available(p), 0)
        for t in p.techs.values():
            test.assertGreaterEqual(t.workers, 0)
        # one urban/production/unit card per name, workers only on those
        for name, t in p.techs.items():
            if t.workers:
                test.assertIn(db.type_of(name), C.WORKER_TYPES)


def yellow_total(p):
    return p.yellow_bank + p.workers_free + sum(t.workers
                                                for t in p.techs.values())


class TestFullGames(unittest.TestCase):
    def test_random_game_completes_and_stays_legal(self):
        for n in (2, 3, 4):
            st = game.play_game([RandomBot(seed=100 + i) for i in range(n)],
                                n, seed=n)
            self.assertTrue(st.game_over, f"{n}p game did not finish")
            self.assertFalse(getattr(st, "move_cap_hit", False))
            self.assertEqual(st.age_civil, "IV")
            self.assertEqual(len(game.scores(st)), n)
            invariants(self, st)

    def test_invariants_hold_at_every_step(self):
        bots = [RandomBot(seed=7 + i) for i in range(4)]
        st = game.new_game(4, seed=11)
        import random as _r
        rng = _r.Random(11)
        steps = 0
        while not st.game_over and steps < 20000:
            actions.apply(st, bots[st.current](st), rng)
            invariants(self, st)
            steps += 1
        self.assertTrue(st.game_over)

    def test_token_conservation(self):
        """Yellow tokens are only lost at age ends (2 per player, §12.2.4);
        blue tokens change only through card effects."""
        bots = [RandomBot(seed=21 + i) for i in range(3)]
        st = game.new_game(3, seed=21)
        import random as _r
        rng = _r.Random(21)
        totals = {p.idx: yellow_total(p) for p in st.players}
        ages_seen = 0
        while not st.game_over:
            before_age = st.age_civil
            actions.apply(st, bots[st.current](st), rng)
            if st.age_civil != before_age:
                ages_seen += 1
                if before_age != "A":
                    for p in st.players:
                        totals[p.idx] -= 2
            for p in st.players:
                # tokens only move between bank / worker pool / cards
                self.assertLessEqual(yellow_total(p), totals[p.idx],
                                     "yellow tokens created")
        self.assertGreaterEqual(ages_seen, 3)

    def test_players_get_equal_turns(self):
        for n in (2, 3, 4):
            st = game.play_game([RandomBot(seed=31 + i) for i in range(n)],
                                n, seed=31 + n)
            # every player took the same number of turns (§12.3)
            self.assertEqual((st.turn - 1) % n, 0,
                             f"{n}p: {st.turn - 1} turns played")

    def test_determinism(self):
        a = game.play_game([RandomBot(seed=5 + i) for i in range(4)], 4, 42)
        b = game.play_game([RandomBot(seed=5 + i) for i in range(4)], 4, 42)
        self.assertEqual(game.scores(a), game.scores(b))

    def test_greedy_beats_random_on_average(self):
        wins = 0
        for seed in range(6):
            st = game.play_game([GreedyBot(seed=seed), RandomBot(seed=seed)],
                                2, seed)
            sc = game.scores(st)
            wins += sc[0] >= sc[1]
        self.assertGreaterEqual(wins, 4, "greedy should usually beat random")


# ------------------------------------------------------ end-of-turn detail

class TestEndOfTurn(unittest.TestCase):
    def _mid_game_state(self):
        st = game.new_game(2, seed=8)
        st.round = 3
        for p in st.players:
            p.civil_actions, p.military_actions = 4, 2
        return st

    def test_production_and_consumption(self):
        st = self._mid_game_state()
        p = st.players[0]
        p.yellow_bank = 14            # consumption 1
        import random as _r
        economy.end_of_turn(st, p, _r.Random(0))
        # 2 farmers on Agriculture = 2 food, minus 1 consumed
        self.assertEqual(p.food, 1)
        # 2 miners on Bronze = 2 resources, no corruption at 16 blue in bank
        self.assertEqual(p.resources, 2)
        self.assertEqual(p.science, 1)

    def test_uprising_skips_production(self):
        st = self._mid_game_state()
        p = st.players[0]
        p.yellow_bank = 2             # needs 7 happy faces
        p.workers_free = 0
        self.assertTrue(economy.uprising(st, p))
        import random as _r
        economy.end_of_turn(st, p, _r.Random(0))
        self.assertEqual(p.food, 0)
        self.assertEqual(p.resources, 0)
        self.assertEqual(p.science, 0)   # no science scored either
        self.assertEqual(p.civil_actions, 4)  # but actions still reset

    def test_missing_food_costs_culture(self):
        st = self._mid_game_state()
        p = st.players[0]
        p.yellow_bank = 4             # consumption 4, production 2
        p.culture = 20
        p.happy_extra = 8             # no uprising
        effects.invalidate(st, p)
        import random as _r
        economy.end_of_turn(st, p, _r.Random(0))
        self.assertEqual(p.food, 0)
        self.assertEqual(p.culture, 20 - 4 * 2)   # 2 food short

    def test_military_hand_discarded_to_action_total(self):
        st = self._mid_game_state()
        if not st.has_military:
            self.skipTest("military data unavailable")
        p = st.players[0]
        tactics = [c["name"] for c in C.db().of_type("tactic")][:6]
        p.hand_military = list(tactics)
        import random as _r
        economy.end_of_turn(st, p, _r.Random(0))
        s = effects.compute(st, p)
        # discarded down to the military action total, then drew up to 3
        self.assertLessEqual(len(p.hand_military), s.military_actions + 3)


# ------------------------------------------------------------ age rules

class TestAges(unittest.TestCase):
    def test_ages_advance_and_end_the_game(self):
        st = game.play_game([RandomBot(seed=i) for i in range(4)], 4, 77)
        self.assertEqual(st.age_civil, "IV")
        self.assertIsNotNone(st.final_round_end)
        self.assertTrue(st.game_over)

    def test_antiquation_removes_old_leaders(self):
        st = game.new_game(2, seed=9)
        p = st.players[0]
        p.leader = "Aristotle"                     # Age A leader
        p.hand_civil = ["Alchemy"]                 # Age I card, survives
        game._antiquate(st, C.level("I"))          # Age I just ended
        self.assertIsNone(p.leader)
        self.assertEqual(p.hand_civil, ["Alchemy"])

    def test_age_end_costs_two_yellow_tokens(self):
        st = game.new_game(2, seed=10)
        st.age_civil = "I"
        st.civil_deck = []
        banks = [p.yellow_bank for p in st.players]
        import random as _r
        game._advance_age(st, _r.Random(0))
        self.assertEqual(st.age_civil, "II")
        self.assertEqual([p.yellow_bank for p in st.players],
                         [b - 2 for b in banks])


if __name__ == "__main__":
    unittest.main()
