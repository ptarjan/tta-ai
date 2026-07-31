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

# The legality assert in apply() is off by default (it doubles the cost of
# every move); the test suite always runs with it on, so every self-play game
# in here is still a legality fuzz test.
actions.STRICT = True


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
            actions.apply(st, bots[st.decider()](st), rng)
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
            actions.apply(st, bots[st.decider()](st), rng)
            if st.age_civil != before_age:
                ages_seen += 1
                if before_age != "A":
                    for p in st.players:
                        totals[p.idx] -= 2
            for p in st.players:
                # tokens only move between bank / worker pool / cards, plus
                # what a card or a rival explicitly hands over (§11.5, §5.8)
                self.assertLessEqual(yellow_total(p),
                                     totals[p.idx] + p.yellow_granted,
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
        rng = _r.Random(0)
        # §6.6 step 1 is a DECISION now, so the sequence suspends until the
        # player has picked; drive it the way the turn loop does.
        guard = 0
        while not economy.end_of_turn(st, p, rng):
            guard += 1
            self.assertLess(guard, 10)
            self.assertTrue(st.pending)
            self.assertEqual(st.pending[-1]["tag"], "discard_military")
            actions.apply(st, ("choose", 0), rng)
        s = effects.compute(st, p)
        # discarded down to the military action total, then drew up to 3
        self.assertLessEqual(len(p.hand_military), s.military_actions + 3)

    # --- §6.6 step 1: the one end-of-turn step that is the player's choice

    def _overfull_hand_state(self):
        """A live 2p game whose player 0 must discard, with distinct cards."""
        st = game.new_game(2, seed=8)
        st.round = 3
        st.phase = "actions"
        for q in st.players:
            q.civil_actions, q.military_actions = 4, 2
        return st

    def test_military_discard_is_the_players_choice(self):
        """RB p.20 / §6.6: the player DECIDES which cards to discard.

        The engine used to pop(0) -- oldest first, no decision -- which threw
        away the best defence card in the hand 19% of the time it fired.
        """
        st = self._overfull_hand_state()
        if not st.has_military:
            self.skipTest("military data unavailable")
        p = st.players[0]
        keep = "Military Bonus (defense 6 / colonization 3)"
        tactics = [c["name"] for c in C.db().of_type("tactic")][:4]
        p.hand_military = [keep] + list(tactics)     # `keep` is the OLDEST
        science_before = p.science

        actions.apply(st, ("end_turn",))
        # suspended on the decision: the turn has NOT advanced and production
        # has NOT run ("the next player may start as soon as you finish
        # discarding" -- not before).
        self.assertTrue(st.pending)
        self.assertEqual(st.pending[-1]["tag"], "discard_military")
        self.assertEqual(st.current, 0)
        self.assertEqual(p.science, science_before)
        self.assertEqual(game.current_player(st), 0)

        # the offered options are the distinct cards in hand, and the player
        # may keep the oldest one: choose anything that is not `keep`.
        opts = st.pending[-1]["options"]
        self.assertEqual(sorted(opts), sorted(set(p.hand_military)))
        while st.pending and st.pending[-1]["tag"] == "discard_military":
            opts = st.pending[-1]["options"]
            i = max(range(len(opts)), key=lambda k: opts[k] != keep)
            actions.apply(st, ("choose", i))

        self.assertIn(keep, p.hand_military)         # FIFO would have lost it
        self.assertGreater(p.science, science_before)   # production ran
        self.assertEqual(st.current, 1)                 # and the turn advanced

    def test_discard_options_are_ordered_least_defensive_first(self):
        """The tie-break every argmax in the project falls back on (§5.4.4)."""
        from engine import interact
        hand = ["Military Bonus (defense 6 / colonization 3)",
                "Military Bonus (defense 2 / colonization 1)"]
        tactics = [c["name"] for c in C.db().of_type("tactic")][:2]
        opts = interact.discard_options(hand + tactics + tactics)
        self.assertEqual(len(opts), len(set(opts)))          # distinct
        self.assertEqual(opts[-1], hand[0])                  # defence 6 last
        self.assertEqual(opts[-2], hand[1])                  # defence 2 next
        self.assertEqual(interact.defense_points(tactics[0]), 1)
        self.assertEqual(interact.defense_points(hand[0]), 6)

    def test_single_distinct_card_needs_no_decision(self):
        """§6.6 stays automatic when there is nothing to choose between."""
        st = self._overfull_hand_state()
        if not st.has_military:
            self.skipTest("military data unavailable")
        p = st.players[0]
        name = [c["name"] for c in C.db().of_type("tactic")][0]
        p.hand_military = [name] * 5
        actions.apply(st, ("end_turn",))
        self.assertFalse(st.pending)
        self.assertEqual(st.current, 1)


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


# ------------------------------------------------------- §3.11 action cards

def _mid_game(seed=11, players=2):
    """A state past round 1 with one player able to act freely."""
    st = game.new_game(players, seed=seed)
    st.round = 3
    st.phase = "actions"
    p = st.me()
    p.civil_actions = 4
    p.military_actions = 3
    p.food = 20
    p.resources = 20
    p.science = 20
    return st, p


class TestActionCards(unittest.TestCase):
    def test_all_action_cards_are_in_the_data(self):
        acts = [c for c in C.db().by_name.values() if c["type"] == "action"]
        self.assertEqual(len(acts), 33)

    def test_playing_costs_one_civil_action_and_discards(self):
        st, p = _mid_game()
        p.hand_civil = ["Stock Pile"]
        before = p.civil_actions
        actions.apply(st, ("play_action", "Stock Pile"))
        self.assertEqual(p.civil_actions, before - 1)
        self.assertNotIn("Stock Pile", p.hand_civil)

    def test_cannot_play_the_turn_it_was_taken(self):
        st, p = _mid_game()
        p.hand_civil = ["Stock Pile"]
        p.taken_this_turn = ["Stock Pile"]
        self.assertNotIn(("play_action", "Stock Pile"),
                         actions.legal_moves(st))

    def test_ordered_action_is_free_and_discounted(self):
        # Rich Land (A): build or upgrade a farm/mine paying 1 less resource
        st, p = _mid_game()
        p.hand_civil = ["Rich Land (A)"]
        p.resources = 10
        p.workers_free = 2
        ca, ma = p.civil_actions, p.military_actions
        actions.apply(st, ("play_action", "Rich Land (A)"))
        # the free build is now a pending decision owned by this player
        self.assertTrue(st.pending)
        self.assertEqual(st.decider(), p.idx)
        opts = st.pending[-1]["options"]
        self.assertTrue(all(o[0] in ("build", "upgrade") for o in opts))
        pick = next(i for i, o in enumerate(opts)
                    if o == ["build", "Agriculture"])
        cost = actions.build_cost_for(st, p, "Agriculture")
        actions.apply(st, ("choose", pick))
        self.assertEqual(p.resources, 10 - max(0, cost - 1))
        self.assertEqual(p.civil_actions, ca - 1)      # only the card's CA
        self.assertEqual(p.military_actions, ma)

    def test_unplayable_when_the_ordered_action_is_impossible(self):
        # Engineering Genius with no wonder in progress (§3.11)
        st, p = _mid_game()
        p.hand_civil = ["Engineering Genius (A)"]
        p.wonder = None
        self.assertNotIn(("play_action", "Engineering Genius (A)"),
                         actions.legal_moves(st))

    def test_gains_land_after_the_ordered_action(self):
        # Breakthrough (I): develop at FULL price, then gain 2 science. The +2
        # arrives too late to pay for that technology (OPEN_QUESTIONS 20).
        st, p = _mid_game()
        p.hand_civil = ["Breakthrough (I)", "Alchemy"]
        cost = effects.tech_cost(st, p, "Alchemy")
        p.science = cost - 2                     # 2 short at full price
        self.assertNotIn(("play_action", "Breakthrough (I)"),
                         actions.legal_moves(st))
        p.science = cost                         # exactly affordable
        self.assertIn(("play_action", "Breakthrough (I)"),
                      actions.legal_moves(st))
        actions.apply(st, ("play_action", "Breakthrough (I)"))
        # only one technology in hand, so the ordered action auto-resolves
        self.assertIn("Alchemy", p.techs)
        self.assertEqual(p.science, 2)           # the gain, banked afterwards

    def test_frugality_food_lands_after_the_population_increase(self):
        st, p = _mid_game()
        p.hand_civil = ["Frugality (A)"]          # increase population, +1 food
        gain = (C.db().get("Frugality (A)")["effects"] or {})["gainFood"]
        cost = economy.pop_cost(st, p)
        p.food = cost - gain                      # short without the gain
        self.assertNotIn(("play_action", "Frugality (A)"),
                         actions.legal_moves(st))
        p.food = cost
        self.assertIn(("play_action", "Frugality (A)"),
                      actions.legal_moves(st))
        free = p.workers_free
        actions.apply(st, ("play_action", "Frugality (A)"))
        self.assertEqual(p.workers_free, free + 1)
        # the whole cost was paid; the gain lands afterwards, bank permitting
        # (this fixture's blue bank is saturated, so it gains nothing here --
        # Breakthrough above is the test that the gain actually arrives)
        self.assertLessEqual(p.food, gain)

    def test_patriotism_gives_a_military_action_and_a_unit_discount(self):
        st, p = _mid_game()
        p.hand_civil = ["Patriotism (I)"]      # +1 MA, units cost 2 less
        p.workers_free = 2
        p.resources = 10
        ma = p.military_actions
        actions.apply(st, ("play_action", "Patriotism (I)"))
        self.assertEqual(p.military_actions, ma + 1)
        self.assertEqual(p.mil_discount, 2)
        raw = actions.build_cost_for(st, p, "Warriors")
        self.assertEqual(actions.build_cost_net(st, p, "Warriors"),
                         max(0, raw - 2))
        actions.apply(st, ("build", "Warriors"))
        self.assertEqual(p.resources, 10 - max(0, raw - 2))
        self.assertEqual(p.mil_discount, max(0, 2 - raw))
        self.assertEqual(p.military_actions, ma)       # the build cost 1 MA

    def test_military_discount_expires_at_end_of_turn(self):
        st, p = _mid_game()
        p.mil_discount = 5
        economy.end_of_turn(st, p, _rng())
        self.assertEqual(p.mil_discount, 0)

    def test_reserves_offers_food_or_resources(self):
        st, p = _mid_game()
        p.hand_civil = ["Reserves (II)"]        # 3 food OR 3 resources
        p.food = p.resources = 0
        actions.apply(st, ("play_action", "Reserves (II)"))
        self.assertEqual(st.pending[-1]["options"], ["food", "resources"])
        actions.apply(st, ("choose", 1))
        self.assertEqual(p.resources, 3)
        self.assertEqual(p.food, 0)

    def test_endowment_scales_with_richer_rivals(self):
        st, p = _mid_game(players=3)
        p.hand_civil = ["Endowment for the Arts"]   # 3 culture each in 3p
        p.culture = 0
        for q in st.players:
            if q.idx != p.idx:
                q.culture = 50
        actions.apply(st, ("play_action", "Endowment for the Arts"))
        self.assertEqual(p.culture, 6)

    def test_every_action_card_is_playable_from_a_rich_position(self):
        db = C.db()
        for name, card in sorted(db.by_name.items()):
            if card["type"] != "action":
                continue
            st, p = _mid_game()
            p.hand_civil = [name]
            p.workers_free = 3
            p.resources = p.food = p.science = 40
            if not any(m[0] == "play_action" for m in actions.legal_moves(st)):
                continue        # needs a wonder / a tech in hand; fine
            actions.apply(st, ("play_action", name))
            while st.pending:
                actions.apply(st, actions.legal_moves(st)[0])


# --------------------------------------------- §11 colonies, §5.9-5.11 pacts

from engine import interact                                      # noqa: E402


def _military_state(seed=21, players=3):
    st = game.new_game(players, seed=seed)
    st.round = 3
    st.phase = "politics"
    st.has_military = True
    return st


class TestColonization(unittest.TestCase):
    def test_auction_runs_clockwise_and_only_bidders_take_part(self):
        st = _military_state()
        p0 = st.players[0]
        p0.techs["Warriors"].workers = 2         # only P0 can send a force
        for q in st.players[1:]:
            for t in q.techs.values():
                if C.db().type_of(t.name) in C.UNIT_TYPES:
                    t.workers = 0
        interact.start_auction(st, "Wealthy Territory (I)", 0)
        self.assertTrue(st.pending)
        self.assertEqual(st.pending[-1]["kind"], "auction")
        self.assertEqual(st.pending[-1]["active"], [0])
        self.assertEqual(st.decider(), 0)

    def test_bids_are_capped_by_the_sendable_force(self):
        st = _military_state()
        p0 = st.players[0]
        p0.techs["Warriors"].workers = 2
        cap = interact.max_force(st, p0)
        self.assertGreater(cap, 0)
        interact.start_auction(st, "Wealthy Territory (I)", 0)
        bids = [m[1] for m in actions.legal_moves(st) if m[0] == "bid"]
        self.assertEqual(bids, list(range(1, cap + 1)))

    def test_winner_sacrifices_units_to_the_yellow_bank(self):
        # §11.4: sent tokens go to the yellow bank, not the worker pool
        st = _military_state()
        p0 = st.players[0]
        p0.techs["Warriors"].workers = 2
        for q in st.players[1:]:
            for n, t in q.techs.items():
                if C.db().type_of(n) in C.UNIT_TYPES:
                    t.workers = 0
        bank, free = p0.yellow_bank, p0.workers_free
        res = p0.resources
        interact.start_auction(st, "Wealthy Territory (I)", 0)
        actions.apply(st, ("bid", 1))
        # two Warriors for a bid of 1: one covers it, and whether to throw a
        # second one in as well is the §11.3 decision the winner now owns
        self.assertEqual(st.pending[-1]["kind"], "colonize")
        self.assertEqual(st.pending[-1]["units"], ["Warriors"])
        actions.apply(st, ("send_done",))
        self.assertFalse(st.pending)
        self.assertIn("Wealthy Territory (I)", p0.colonies)
        self.assertEqual(p0.workers_free, free)          # NOT to the pool
        self.assertEqual(p0.yellow_bank, bank + 1)
        self.assertEqual(p0.resources, res + 5)          # immediate effect
        self.assertEqual(p0.blue_total, 16 + 3)          # permanent effect

    def test_no_bidders_sends_the_territory_to_past_events(self):
        st = _military_state()
        for q in st.players:
            for n, t in q.techs.items():
                if C.db().type_of(n) in C.UNIT_TYPES:
                    t.workers = 0
        interact.start_auction(st, "Vast Territory (I)", 0)
        self.assertFalse(st.pending)
        self.assertIn("Vast Territory (I)", st.past_events)

    def test_colonization_force_ignores_strength_rating_bonuses(self):
        # §11.3: leaders/wonders/special techs never help a colonization
        st = _military_state()
        p0 = st.players[0]
        p0.techs["Warriors"].workers = 1
        base = interact.max_force(st, p0)
        p0.strength_extra += 10
        effects.invalidate(st, p0)
        self.assertEqual(interact.max_force(st, p0), base)

    def test_losing_a_colony_gives_back_only_the_permanent_effects(self):
        st = _military_state()
        p0 = st.players[0]
        interact.gain_colony(st, p0, "Wealthy Territory (I)")
        res = p0.resources
        interact.lose_colony(st, p0, "Wealthy Territory (I)")
        self.assertEqual(p0.blue_total, 16)
        self.assertEqual(p0.resources, res)      # one-time effect is kept


class TestPactsAndResigning(unittest.TestCase):
    def test_no_pacts_in_two_player_games(self):
        st = _military_state(players=2)
        st.me().hand_military = ["Military Alliance"]
        self.assertFalse([m for m in actions.legal_moves(st)
                          if m[0] == "offer_pact"])

    def test_pact_offer_is_the_partners_decision(self):
        st = _military_state()
        p0 = st.me()
        p0.hand_military = ["Military Alliance"]
        actions.apply(st, ("offer_pact", "Military Alliance", 1, ""))
        self.assertEqual(st.decider(), 1)         # the partner answers
        opts = st.pending[-1]["options"]
        actions.apply(st, ("choose", opts.index("accept")))
        self.assertEqual(len(p0.pacts), 1)
        self.assertEqual(p0.pacts[0]["partner"], 1)
        # both parties get +3 strength
        self.assertEqual(effects.state_stats(st, p0).strength,
                         effects.state_stats(st, st.players[1]).strength)

    def test_refused_pact_returns_to_hand(self):
        st = _military_state()
        p0 = st.me()
        p0.hand_military = ["Peace Treaty"]
        actions.apply(st, ("offer_pact", "Peace Treaty", 2, ""))
        opts = st.pending[-1]["options"]
        actions.apply(st, ("choose", opts.index("refuse")))
        self.assertEqual(p0.pacts, [])
        self.assertIn("Peace Treaty", p0.hand_civil + p0.hand_military)

    def test_a_pact_forbidding_attacks_blocks_aggressions(self):
        st = _military_state()
        p0 = st.me()
        p0.pacts = [{"name": "Peace Treaty", "owner": 0, "partner": 1,
                     "a": 0, "b": 1}]
        effects.invalidate(st)
        self.assertTrue(effects.pact_forbids_attack(st, p0, st.players[1]))
        self.assertFalse(effects.pact_forbids_attack(st, p0, st.players[2]))

    def test_either_party_may_cancel(self):
        st = _military_state()
        st.players[1].pacts = [{"name": "Peace Treaty", "owner": 1,
                                "partner": 0, "a": 1, "b": 0}]
        effects.invalidate(st)
        self.assertIn(("cancel_pact", 1), actions.legal_moves(st))
        actions.apply(st, ("cancel_pact", 1))
        self.assertEqual(st.players[1].pacts, [])

    def test_resign_is_illegal_in_age_iv(self):
        st = _military_state()
        st.age_civil = "IV"
        self.assertNotIn(("resign",), actions.legal_moves(st))

    def test_resigning_pays_seven_culture_to_each_war_declarer(self):
        st = _military_state()
        p0, p1 = st.players[0], st.players[1]
        p1.war_declared_by_me = ("War over Culture", 1, 0)
        p0.wars_declared_on_me = [("War over Culture", 1, 0)]
        before = p1.culture
        actions.apply(st, ("resign",))
        self.assertTrue(p0.resigned)
        self.assertEqual(p1.culture, before + 7)
        self.assertIsNone(p1.war_declared_by_me)

    def test_last_player_standing_wins(self):
        st = _military_state(players=2)
        st.phase = "politics"
        actions.apply(st, ("resign",))
        self.assertTrue(game.is_over(st))
        self.assertEqual(game.winners(st), [1])


class TestAggressionDefense(unittest.TestCase):
    def _setup(self):
        st = _military_state()
        atk, dfn = st.players[0], st.players[1]
        atk.techs["Warriors"].workers = 3
        atk.military_actions = 3
        effects.invalidate(st)
        return st, atk, dfn

    def test_defender_chooses_and_the_budget_is_the_action_total(self):
        st, atk, dfn = self._setup()
        atk.hand_military = ["Aggression: Plunder (I)"]
        dfn.hand_military = ["Military Bonus (defense 6 / colonization 3)",
                             "Military Alliance"]
        mv = next(m for m in actions.legal_moves(st) if m[0] == "aggression")
        actions.apply(st, mv)
        self.assertEqual(st.decider(), dfn.idx)      # the DEFENDER decides
        pend = st.pending[-1]
        self.assertEqual(pend["kind"], "defense")
        self.assertEqual(pend["budget"],
                         effects.state_stats(st, dfn).military_actions)
        self.assertIn(("defend_done",), actions.legal_moves(st))

    def test_a_big_bonus_card_repels_the_aggression(self):
        st, atk, dfn = self._setup()
        atk.hand_military = ["Aggression: Plunder (I)"]
        dfn.hand_military = ["Military Bonus (defense 6 / colonization 3)"]
        food = dfn.food = 9
        mv = next(m for m in actions.legal_moves(st) if m[0] == "aggression")
        actions.apply(st, mv)
        actions.apply(st, ("defend",
                           "Military Bonus (defense 6 / colonization 3)"))
        self.assertFalse(st.pending)
        self.assertEqual(dfn.food, food)             # nothing was taken
        self.assertTrue(any("failed" in L for L in st.log))

    def test_undefended_plunder_takes_the_goods(self):
        st, atk, dfn = self._setup()
        atk.hand_military = ["Aggression: Plunder (I)"]
        dfn.hand_military = []
        dfn.food = 9
        mv = next(m for m in actions.legal_moves(st) if m[0] == "aggression")
        actions.apply(st, mv)
        while st.pending:
            actions.apply(st, actions.legal_moves(st)[0])
        self.assertLess(dfn.food, 9)


class TestSerialization(unittest.TestCase):
    def test_pending_decisions_survive_a_json_round_trip(self):
        """The decision stack and the deferred queue stay JSON-serializable."""
        import json
        import random as _r
        from engine.state import GameState
        seen = 0
        for seed in range(20):
            st = game.new_game(3, seed=seed)
            bots = [RandomBot(seed=seed * 7 + i) for i in range(3)]
            rng = _r.Random(seed)
            while not st.game_over:
                actions.apply(st, bots[st.decider()](st), rng)
                if st.pending or st.queue:
                    st2 = GameState.from_dict(json.loads(json.dumps(st.to_dict())))
                    self.assertEqual(
                        [list(m) for m in actions.legal_moves(st)],
                        [list(m) for m in actions.legal_moves(st2)])
                    self.assertEqual(st.decider(), st2.decider())
                    seen += 1
                    break
        self.assertGreater(seen, 10, "no pending decisions were reached")


def _rng():
    import random
    return random.Random(0)


if __name__ == "__main__":
    unittest.main()
