"""The three base-game leader abilities that live in the ACTION phase.

The other half of `tests/test_leader_politics_abilities.py`, and the same bug
class: each of these was declared in `data/cards_wonders_leaders.json`, written
off in `engine.bots.weighted.DELIBERATELY_UNPRICED` as a rule change "the rules
engine expresses", and expressed by nothing in `engine/`.  Nothing failed when
the card and the engine disagreed, so each is pinned here by the behaviour it
is supposed to produce plus the limit that makes it a cost rather than a gift:

* **Frederick Barbarossa** (I) -- "By spending 1 military action, you may
  increase population and build a military unit all at once; the population
  increase costs 1 less food and the unit costs 1 less resource."  His WHOLE
  card: before this he was a blank leader worth taking for nothing.  UNLIMITED
  per turn (docs/EXPERT_STRATEGY.md:138), limited only by military actions.
* **J. S. Bach** (II) -- "Once per turn, as a civil action, you may upgrade one
  of your urban buildings to a theater of the same or higher level, paying the
  resource cost difference as normal."  The only cross-type upgrade in the
  game: §3.5's upgrade moves a worker between cards of the SAME type.
* **James Cook** (II) -- "When colonizing, you may discard up to 2 military
  cards, gaining +1 colony bonus for each card discarded."  Force that comes
  out of the hand instead of out of the army, inside the §11.3 decision.
"""
from __future__ import annotations

import os
import random
import re
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from engine import actions, cards as C, economy, effects, game  # noqa: E402
from engine import interact  # noqa: E402
from engine.bots import GreedyBot, RandomBot, WeightedBot  # noqa: E402
from engine.bots.book import BookBot  # noqa: E402
from engine.bots.fastcopy import copy_state  # noqa: E402
from engine.bots.plan import PlanBot  # noqa: E402
from engine.bots.quiescent import QuiescentBot  # noqa: E402
from engine.state import TechCard  # noqa: E402

actions.STRICT = True

_DB = C.db()
TERRITORY = [c["name"] for c in _DB.of_type("territory") if c["age"] == "I"][0]
#: Military cards that are not bonus cards, so James Cook may burn them.
JUNK = ["Rats", "Pestilence", "Raiders"]
BONUS_I = "Military Bonus (defense 2 / colonization 1)"


def _rng(seed=0):
    return random.Random(seed)


def _turn(leader=None, players=2, seed=21, food=10, resources=10, techs=()):
    """A round-3 state sitting in P0's action phase, with money to spend."""
    st = game.new_game(players, seed=seed)
    st.round = 3
    st.phase = "actions"
    st.has_military = True
    p = st.players[0]
    p.leader = leader
    p.food = food
    p.resources = resources
    p.civil_actions = 4
    p.military_actions = 2
    p.politics_done = True
    for name in techs:
        p.techs.setdefault(name, TechCard(name))
    effects.invalidate(st, p)
    return st, p


def _apply(st, mv, seed=0):
    return actions.apply(st, mv, _rng(seed))


def _kinds(st):
    return {m[0] for m in actions.legal_moves(st)}


def _of_kind(st, kind):
    return [m for m in actions.legal_moves(st) if m[0] == kind]


# ======================================================================
# Frederick Barbarossa
# ======================================================================

class BarbarossaBuysBothHalvesWithOneMilitaryAction(unittest.TestCase):

    def test_the_move_exists_only_for_him(self):
        for leader in (None, "J. S. Bach", "Julius Caesar"):
            st, _p = _turn(leader)
            self.assertNotIn("barbarossa", _kinds(st), leader)
        st, _p = _turn("Frederick Barbarossa")
        self.assertEqual(_of_kind(st, "barbarossa"), [("barbarossa", "Warriors")])

    def test_one_move_per_unit_technology(self):
        """The unit is chosen by choosing the move, exactly as Columbus's
        territory is: a bare declaration prices at what the POPULATION is
        worth and would be tied with everything else a 1-ply search sees."""
        st, p = _turn("Frederick Barbarossa", techs=("Swordsmen", "Riflemen"))
        effects.invalidate(st, p)
        self.assertEqual(
            sorted(_of_kind(st, "barbarossa")),
            [("barbarossa", n) for n in sorted(("Warriors", "Swordsmen",
                                                "Riflemen"))])

    def test_it_costs_one_military_action_and_no_civil_action(self):
        st, p = _turn("Frederick Barbarossa")
        ca, ma = p.civil_actions, p.military_actions
        _apply(st, ("barbarossa", "Warriors"))
        self.assertEqual(p.civil_actions, ca, "the population half took a CA")
        self.assertEqual(p.military_actions, ma - 1)

    def test_both_halves_happen_and_both_are_discounted(self):
        st, p = _turn("Frederick Barbarossa")
        food, res, bank = p.food, p.resources, p.yellow_bank
        free = p.workers_free
        units = p.techs["Warriors"].workers
        pop_cost = economy.pop_cost(st, p)
        build = effects.build_cost(st, p, "Warriors")
        _apply(st, ("barbarossa", "Warriors"))
        self.assertEqual(p.food, food - (pop_cost - 1), "no food discount")
        self.assertEqual(p.resources, res - (build - 1), "no unit discount")
        self.assertEqual(p.yellow_bank, bank - 1, "no population increase")
        self.assertEqual(p.techs["Warriors"].workers, units + 1, "no unit")
        self.assertEqual(p.workers_free, free,
                         "the new worker went somewhere other than the unit")

    def test_the_discounts_are_the_numbers_printed_on_the_card(self):
        eff = _DB.get("Frederick Barbarossa")["effects"]
        self.assertEqual(actions.COMBO_FOOD_DISCOUNT, eff["comboFoodDiscount"])
        self.assertEqual(actions.COMBO_RESOURCE_DISCOUNT,
                         eff["comboResourceDiscount"])

    def test_it_is_unlimited_per_turn_not_once(self):
        """docs/EXPERT_STRATEGY.md:138 -- "each activation converts an MA into
        a CA + 1 food + 1 rock, UNLIMITED per turn"; the quoted line of play is
        using him three times in one turn."""
        st, p = _turn("Frederick Barbarossa", food=40, resources=40)
        p.military_actions = 3
        for i in range(3):
            self.assertIn(("barbarossa", "Warriors"), actions.legal_moves(st),
                          f"activation {i + 1} was not offered")
            _apply(st, ("barbarossa", "Warriors"))
        self.assertEqual(p.military_actions, 0)
        self.assertEqual(p.techs["Warriors"].workers, 4)
        self.assertNotIn("barbarossa", _kinds(st), "offered with no MA left")

    def test_a_half_he_cannot_pay_for_is_not_offered(self):
        """§3 (CoL p.5): an action cannot be performed unless ALL of its
        sub-steps can be paid.  Each half is tested at exactly one short."""
        st, p = _turn("Frederick Barbarossa")
        p.food = economy.pop_cost(st, p) - 2          # one short after the -1
        self.assertNotIn("barbarossa", _kinds(st), "unpayable food half")
        p.food = economy.pop_cost(st, p) - 1          # exactly affordable
        self.assertIn("barbarossa", _kinds(st))
        st, p = _turn("Frederick Barbarossa")
        p.resources = effects.build_cost(st, p, "Warriors") - 2
        self.assertNotIn("barbarossa", _kinds(st), "unpayable unit half")

    def test_an_empty_yellow_bank_ends_the_ability(self):
        st, p = _turn("Frederick Barbarossa")
        p.yellow_bank = 0
        effects.invalidate(st, p)
        self.assertIsNone(economy.pop_cost(st, p))
        self.assertNotIn("barbarossa", _kinds(st))

    def test_the_discount_pool_stacks_on_top_of_his(self):
        """§3.11 discounts stack cumulatively, floor 0.  Churchill's
        ring-fenced resources are the pool in play here."""
        st, p = _turn("Frederick Barbarossa", resources=10)
        p.mil_discount = 1
        res = p.resources
        build = effects.build_cost(st, p, "Warriors")
        _apply(st, ("barbarossa", "Warriors"))
        self.assertEqual(p.resources, res - max(0, build - 2))
        self.assertEqual(p.mil_discount, 0, "the pool was not spent")

    def test_his_food_discount_does_not_leak_into_the_plain_action(self):
        """`Stats.pop_food_discount` is a STANDING discount that both
        evaluators read through `economy.pop_food_cost`.  Barbarossa's is per
        ACTION, so a plain `pop` -- his or anyone's -- still costs full price
        and every evaluator still believes it does."""
        st, p = _turn("Frederick Barbarossa")
        self.assertEqual(effects.state_stats(st, p).pop_food_discount, 0)
        base = economy.pop_cost_base(p.yellow_bank)
        self.assertEqual(economy.pop_cost(st, p), base)
        food = p.food
        _apply(st, ("pop",))
        self.assertEqual(p.food, food - base, "the plain action got a discount")


# ======================================================================
# J. S. Bach
# ======================================================================

class BachUpgradesAcrossBuildingTypes(unittest.TestCase):

    def _bach(self, urban=("Religion",), theaters=("Drama",), workers=1,
              resources=10):
        st, p = _turn("J. S. Bach", resources=resources,
                      techs=tuple(urban) + tuple(theaters))
        # the starting board comes with a staffed lab and temple; clear every
        # urban worker so the moves generated are exactly the fixture's
        for n, t in p.techs.items():
            if _DB.type_of(n) in C.URBAN_TYPES:
                p.workers_free += t.workers
                t.workers = 0
        for n in urban:
            p.techs[n].workers = workers
            p.workers_free -= workers
        effects.invalidate(st, p)
        return st, p

    def test_no_generic_upgrade_ever_crosses_a_building_type(self):
        """The structural claim this ability breaks, asserted on the generic
        path so that widening it later fails HERE rather than silently."""
        st, p = self._bach()
        type_of = _DB.type_by_name
        for mv in _of_kind(st, "upgrade"):
            self.assertEqual(type_of[mv[1]], type_of[mv[2]], mv)

    def test_the_move_exists_only_for_him_and_only_with_a_theater(self):
        st, _p = self._bach()
        self.assertEqual(_of_kind(st, "bach_theater"),
                         [("bach_theater", "Religion", "Drama")])
        st, p = self._bach(theaters=())
        self.assertNotIn("bach_theater", _kinds(st), "no theater in play")
        st, p = _turn("Sid Meier", techs=("Drama",))
        p.techs["Religion"].workers = 1
        effects.invalidate(st, p)
        self.assertNotIn("bach_theater", _kinds(st), "not his ability")

    def test_the_worker_moves_and_the_difference_is_paid(self):
        st, p = self._bach()
        res, ca = p.resources, p.civil_actions
        cost = actions.upgrade_cost(st, p, "Religion", "Drama")
        _apply(st, ("bach_theater", "Religion", "Drama"))
        self.assertEqual(p.techs["Religion"].workers, 0)
        self.assertEqual(p.techs["Drama"].workers, 1)
        self.assertEqual(p.resources, res - cost)
        self.assertEqual(p.civil_actions, ca - 1, "not one civil action")
        self.assertGreater(effects.state_stats(st, p).culture, 0,
                           "a staffed theater under Bach produces nothing")

    def test_the_same_level_is_allowed(self):
        """"of the same OR HIGHER level" -- the generic upgrade path's strict
        `level_of(hi) > level_of(lo)` would drop exactly this case."""
        st, p = self._bach(urban=("Bread and Circuses",))
        self.assertEqual(_DB.level_by_name["Bread and Circuses"],
                         _DB.level_by_name["Drama"])
        self.assertIn(("bach_theater", "Bread and Circuses", "Drama"),
                      actions.legal_moves(st))

    def test_a_lower_level_theater_is_not_a_target(self):
        st, p = self._bach(urban=("Organized Religion",))
        self.assertGreater(_DB.level_by_name["Organized Religion"],
                           _DB.level_by_name["Drama"])
        self.assertNotIn("bach_theater", _kinds(st))

    def test_a_cheaper_theater_costs_zero_and_never_refunds(self):
        st, p = self._bach(urban=("Alchemy",))
        self.assertGreater(effects.build_cost(st, p, "Alchemy"),
                           effects.build_cost(st, p, "Drama"))
        res = p.resources
        _apply(st, ("bach_theater", "Alchemy", "Drama"))
        self.assertEqual(p.resources, res, "an upgrade paid a refund")

    def test_it_is_once_per_turn(self):
        st, p = self._bach(urban=("Religion", "Philosophy"), workers=1)
        self.assertEqual(len(_of_kind(st, "bach_theater")), 2)
        _apply(st, ("bach_theater", "Religion", "Drama"))
        self.assertTrue(p.bach_upgrade_used)
        self.assertNotIn("bach_theater", _kinds(st),
                         "a second upgrade in the same turn")
        self.assertGreater(p.civil_actions, 0, "not for want of an action")

    def test_the_urban_limit_is_a_real_check_on_this_one(self):
        """§7.5 caps each urban TYPE at the government's number and §3.5 only
        calls it trivially satisfied because a same-type upgrade keeps the
        count constant.  This one ADDS a theater."""
        st, p = self._bach(urban=("Religion",))
        limit = effects.state_stats(st, p).urban_limit
        p.techs["Drama"].workers = limit
        p.workers_free += limit
        effects.invalidate(st, p)
        self.assertNotIn("bach_theater", _kinds(st),
                         "a theater over the government's urban limit")
        p.techs["Drama"].workers = limit - 1
        effects.invalidate(st, p)
        self.assertIn("bach_theater", _kinds(st))

    def test_an_unstaffed_building_cannot_be_upgraded(self):
        st, p = self._bach(workers=0)
        self.assertNotIn("bach_theater", _kinds(st))

    def test_resources_he_has_not_got_are_not_a_move(self):
        st, p = self._bach(urban=("Religion",), resources=0)
        self.assertGreater(actions.upgrade_cost(st, p, "Religion", "Drama"), 0)
        self.assertNotIn("bach_theater", _kinds(st))


class BachsFlagIsPerTurn(unittest.TestCase):

    def test_the_end_of_turn_sequence_clears_it(self):
        st, p = _turn("J. S. Bach", techs=("Drama",))
        p.techs["Religion"].workers = 1
        effects.invalidate(st, p)
        _apply(st, ("bach_theater", "Religion", "Drama"))
        self.assertTrue(p.bach_upgrade_used)
        economy.end_of_turn(st, p, _rng())
        self.assertFalse(p.bach_upgrade_used,
                         "a once-per-TURN ability stayed spent")


# ======================================================================
# James Cook
# ======================================================================

class CookBurnsCardsForColonizationForce(unittest.TestCase):

    def _colonizing(self, leader="James Cook", hand=(), units=1, bid=1):
        st, p = _turn(leader)
        p.hand_military = list(hand)
        p.techs["Warriors"].workers = units
        effects.invalidate(st, p)
        interact.colonize(st, p, TERRITORY, bid, _rng())
        return st, p

    def test_both_of_his_numbers_come_from_the_card(self):
        """One of them is only HALF available as data, and that is worth
        pinning rather than papering over: the +1 is the effect's VALUE, but
        the cap of two is spelled inside the effect's KEY
        (`colonizeDiscardUpTo2MilitaryCardsForBonus`), where nothing can query
        it.  `interact.COOK_DISCARDS` is therefore a module constant, and this
        is what stops it drifting from the card that prints it."""
        eff = _DB.get("James Cook")["effects"]
        key = "colonizeDiscardUpTo2MilitaryCardsForBonus"
        self.assertIn(key, eff)
        self.assertEqual(interact.COOK_BONUS_PER_DISCARD, eff[key])
        self.assertEqual(str(interact.COOK_DISCARDS),
                         re.search(r"UpTo(\d+)", key).group(1))

    def test_the_pool_is_only_his(self):
        st, p = _turn(None)
        p.hand_military = list(JUNK)
        self.assertEqual(interact.cook_pool(p), [])
        p.leader = "James Cook"
        self.assertEqual(sorted(interact.cook_pool(p)), sorted(JUNK))

    def test_bonus_cards_are_not_offered_as_discards(self):
        """§11.3 already admits ANY NUMBER of bonus cards at their printed
        colonization value (1/2/3 >= 1), uncapped, so spending one of his two
        slots on one is weakly dominated in every position."""
        st, p = _turn("James Cook")
        p.hand_military = [BONUS_I, JUNK[0]]
        self.assertEqual(interact.cook_pool(p), [JUNK[0]])

    def test_each_discard_is_worth_one_force(self):
        st, p = _turn("James Cook")
        p.techs["Warriors"].workers = 1
        effects.invalidate(st, p)
        base = interact.force_value(st, p, ["Warriors"], [])
        self.assertEqual(interact.force_value(st, p, ["Warriors"], [], ["Rats"]),
                         base + 1)
        self.assertEqual(
            interact.force_value(st, p, ["Warriors"], [], ["Rats", "Rats"]),
            base + 2)

    def test_the_bidding_ceiling_counts_them(self):
        """§11.2 caps a bid at "the maximum colonization force the bidder can
        actually send".  Leaving his discards out of it would let him win an
        auction he then could not pay for -- or, more often, stop him bidding
        the amount his own card is for."""
        st, p = _turn("James Cook")
        p.techs["Warriors"].workers = 1
        effects.invalidate(st, p)
        alone = interact.force_value(st, p, ["Warriors"], [])
        p.hand_military = [JUNK[0]]
        self.assertEqual(interact.max_force(st, p), alone + 1)
        p.hand_military = list(JUNK)                  # three, capped at two
        self.assertEqual(interact.max_force(st, p), alone + 2)

    def test_two_cards_is_the_cap(self):
        st, p = self._colonizing(hand=JUNK, units=3, bid=5)
        for _ in range(2):
            mv = [m for m in actions.legal_moves(st) if m[0] == "send_discard"]
            self.assertTrue(mv, "a discard should still be available")
            _apply(st, mv[0])
        self.assertFalse([m for m in actions.legal_moves(st)
                          if m[0] == "send_discard"], "a third discard")
        self.assertEqual(len(p.hand_military), 1)

    def test_the_cards_reach_the_military_discard(self):
        st, p = self._colonizing(hand=[JUNK[0]], units=2, bid=3)
        _apply(st, ("send_discard", JUNK[0]))
        self.assertNotIn(JUNK[0], p.hand_military)
        self.assertIn(JUNK[0], st.discarded_military[_DB.get(JUNK[0])["age"]])

    def test_no_card_can_stand_in_for_the_mandatory_unit(self):
        """§11.3: ">= 1 unit mandatory, even if other bonuses would cover the
        bid".  With no unit committed the only moves are units."""
        st, p = _turn("James Cook")
        p.hand_military = list(JUNK)
        p.techs["Warriors"].workers = 1
        p.techs["Riflemen"] = TechCard("Riflemen")
        p.techs["Riflemen"].workers = 1
        effects.invalidate(st, p)
        interact.colonize(st, p, TERRITORY, 2, _rng())
        self.assertEqual({m[0] for m in actions.legal_moves(st)},
                         {"send_unit"})

    def test_he_colonizes_a_bid_his_army_alone_cannot_cover(self):
        """The whole point of the card: force out of the hand instead of out
        of the army."""
        st, p = _turn("James Cook")
        p.hand_military = list(JUNK[:2])
        p.techs["Warriors"].workers = 1
        effects.invalidate(st, p)
        bid = interact.max_force(st, p)
        self.assertGreater(bid, interact.force_value(st, p, ["Warriors"], []))
        interact.colonize(st, p, TERRITORY, bid, _rng())
        for _ in range(8):
            if not st.pending or st.pending[-1]["kind"] != "colonize":
                break
            _apply(st, actions.legal_moves(st)[0])
        self.assertIn(TERRITORY, p.colonies)
        self.assertEqual(p.techs["Warriors"].workers, 0, "one unit, no more")

    def test_two_copies_of_one_card_are_two_discards(self):
        """`discard_options` de-duplicates for a menu; the POOL must not."""
        st, p = _turn("James Cook")
        p.hand_military = ["Rats", "Rats"]
        p.techs["Warriors"].workers = 1
        effects.invalidate(st, p)
        self.assertEqual(interact.cook_pool(p), ["Rats", "Rats"])
        alone = interact.force_value(st, p, ["Warriors"], [])
        self.assertEqual(interact.max_force(st, p), alone + 2)

    def test_nobody_else_is_offered_a_discard(self):
        st, p = self._colonizing(leader=None, hand=JUNK, units=2, bid=2)
        while st.pending and st.pending[-1]["kind"] == "colonize":
            self.assertNotIn("send_discard",
                             {m[0] for m in actions.legal_moves(st)})
            _apply(st, actions.legal_moves(st)[0])


# ======================================================================
# the bots
# ======================================================================

class TheBotsCanActuallyUseThem(unittest.TestCase):
    """An ability no bot ever selects is only half implemented."""

    BOTS = (BookBot, GreedyBot, WeightedBot, QuiescentBot)

    def test_barbarossa_beats_the_plain_population_increase(self):
        """He needs no bot-side term: the combined move IS a population
        increase, so every evaluator that can see `pop` can see it -- and it
        arrives with a unit, a food and a civil action still in hand."""
        for cls in self.BOTS:
            st, p = _turn("Frederick Barbarossa")
            # no idle worker: BookBot's population rule refuses to grow while
            # one is standing around, and that judgement is not what is
            # being measured here
            p.workers_free = 0
            effects.invalidate(st, p)
            combo = ("barbarossa", "Warriors")
            self.assertIn(combo, actions.legal_moves(st))
            mv = cls(seed=1).choose(st, [("pop",), combo, ("end_turn",)])
            self.assertEqual(mv, combo,
                             f"{cls.__name__} paid a civil action and an extra "
                             f"food for the same worker and no unit")

    def test_bach_is_taken_when_the_theater_is_worth_culture(self):
        st, p = _turn("J. S. Bach", techs=("Drama",))
        p.techs["Religion"].workers = 1
        effects.invalidate(st, p)
        mv = ("bach_theater", "Religion", "Drama")
        self.assertIn(mv, actions.legal_moves(st))
        for cls in (GreedyBot, WeightedBot, QuiescentBot):
            got = cls(seed=1).choose(st, [("end_turn",), mv])
            self.assertEqual(got, mv,
                             f"{cls.__name__} left Bach's theater unbuilt")

    def test_a_card_is_burned_before_a_second_unit_is_sacrificed(self):
        """Cook's decision priced against the alternative it exists to
        replace: a junk card out of the hand, or a worker out of the army."""
        for cls in self.BOTS:
            st, p = _turn("James Cook")
            p.hand_military = list(JUNK[:2])
            p.techs["Warriors"].workers = 3
            effects.invalidate(st, p)
            interact.colonize(st, p, TERRITORY, 2, _rng())
            moves = actions.legal_moves(st)
            self.assertIn(("send_unit", "Warriors"), moves)
            got = cls(seed=1).choose(st, moves)
            self.assertEqual(got[0], "send_discard",
                             f"{cls.__name__} threw a worker away rather than "
                             f"a spent event ({got})")

    def test_every_bot_plays_a_turn_with_each_of_them_and_terminates(self):
        for cls in (BookBot, GreedyBot, WeightedBot, QuiescentBot, RandomBot):
            for leader in ("Frederick Barbarossa", "J. S. Bach", "James Cook"):
                st, p = _turn(leader, techs=("Drama",))
                p.techs["Religion"].workers = 1
                p.hand_military = list(JUNK)
                effects.invalidate(st, p)
                bot = cls(seed=1)
                for _ in range(40):
                    if st.phase != "actions" or st.pending:
                        if not st.pending:
                            break
                    moves = actions.legal_moves(st)
                    self.assertTrue(moves, f"{cls.__name__}/{leader}: no move")
                    mv = bot.choose(st, moves)
                    self.assertIn(mv, moves, f"{cls.__name__}/{leader}: {mv}")
                    _apply(st, mv)
                    if mv == ("end_turn",):
                        break
                else:
                    self.fail(f"{cls.__name__}/{leader} never ended its turn")

    def test_the_planner_answers_a_colonization_with_discards(self):
        st, p = _turn("James Cook")
        p.hand_military = list(JUNK)
        p.techs["Warriors"].workers = 2
        effects.invalidate(st, p)
        interact.colonize(st, p, TERRITORY, 3, _rng())
        bot = PlanBot(seed=1, width=2)
        for _ in range(10):
            if not st.pending:
                break
            moves = actions.legal_moves(st)
            mv = bot.choose(st, moves)
            self.assertIn(mv, moves, mv)
            _apply(st, mv)
        self.assertIn(TERRITORY, p.colonies)


class ItSurvivesRealGames(unittest.TestCase):

    def test_random_play_reaches_all_three_and_never_deadlocks(self):
        seen = {"barbarossa": 0, "bach_theater": 0, "send_discard": 0}
        for seed in range(8):
            st = game.new_game(3, seed=seed)
            rng = random.Random(seed)
            bot = RandomBot(seed=seed)
            for i, q in enumerate(st.players):
                q.leader = ("Frederick Barbarossa", "J. S. Bach",
                            "James Cook")[i % 3]
                q.techs.setdefault("Drama", TechCard("Drama"))
                effects.invalidate(st, q)
            for _ in range(800):
                if st.game_over:
                    break
                moves = actions.legal_moves(st)
                self.assertTrue(moves, "no legal move")
                for m in moves:
                    if m[0] in seen:
                        seen[m[0]] += 1
                actions.apply(st, bot.choose(st, moves), rng)
        for k, n in seen.items():
            self.assertGreater(n, 0, f"{k} was never reachable in a real game")

    def test_the_journal_rolls_all_three_back(self):
        from engine import journal
        journal.install()

        st, p = _turn("Frederick Barbarossa", techs=("Drama",))
        p.techs["Religion"].workers = 1
        effects.invalidate(st, p)
        cases = [("Frederick Barbarossa", ("barbarossa", "Warriors")),
                 ("J. S. Bach", ("bach_theater", "Religion", "Drama"))]
        for leader, mv in cases:
            st, p = _turn(leader, techs=("Drama",))
            p.techs["Religion"].workers = 1
            effects.invalidate(st, p)
            before = copy_state(st)
            j = journal.begin(st)
            try:
                actions.apply(st, mv, _rng())
            finally:
                journal.rollback(j)
            q = before.players[0]
            self.assertEqual(p.food, q.food, leader)
            self.assertEqual(p.resources, q.resources, leader)
            self.assertEqual(p.yellow_bank, q.yellow_bank, leader)
            self.assertEqual(p.workers_free, q.workers_free, leader)
            self.assertEqual(p.civil_actions, q.civil_actions, leader)
            self.assertEqual(p.military_actions, q.military_actions, leader)
            self.assertFalse(p.bach_upgrade_used, leader)
            self.assertEqual({n: t.workers for n, t in p.techs.items()},
                             {n: t.workers for n, t in q.techs.items()}, leader)

        # Cook's discard: the card comes back out of the discard pile
        st, p = _turn("James Cook")
        p.hand_military = list(JUNK)
        p.techs["Warriors"].workers = 2
        effects.invalidate(st, p)
        interact.colonize(st, p, TERRITORY, 3, _rng())
        before = copy_state(st)
        j = journal.begin(st)
        try:
            actions.apply(st, ("send_discard", JUNK[0]), _rng())
        finally:
            journal.rollback(j)
        self.assertEqual(sorted(p.hand_military),
                         sorted(before.players[0].hand_military))
        self.assertEqual(st.discarded_military, before.discarded_military)
        self.assertEqual(st.pending[-1]["discards"], [])


if __name__ == "__main__":
    unittest.main()
