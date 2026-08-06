"""Regression: Development of Civil Life's discount is one-shot, not standing.

THE BUG (fixed 2026-08-05, both engines in the same commit): `events.py:360`
writes `p.one_time_discount` when the Age A event "Development of Civil
Life" resolves. `grep -rn --include=*.py one_time_discount engine/` found
exactly one write, three reads (`effects.build_cost`, `effects.tech_cost`,
`economy.pop_food_cost`/`pop_cost`) and NO CLEAR anywhere in the engine --
only `tools/bgo_moves.py`, a tool rather than the engine, ever zeroed it
(and it did so every turn, a different, out-of-scope bug). So the discount
silently applied to EVERY build, EVERY develop and EVERY population
increase for the rest of the game, for every player alive when the event
resolved.

THE RULE, verbatim (`data/cards_military_actions.json`, "Development of
Civil Life"): *"Players may increase population, build a farm, mine or
urban building, or develop a technology, paying 1 food, 1 resource or 1
science less."* -- one discounted population increase, one discounted
build and one discounted technology, EACH consumed independently the first
time an action of that kind is taken; nothing about a second such action,
ever.
"""
from __future__ import annotations

import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from engine import actions as A, economy, game as G  # noqa: E402


def _st():
    """A fresh 2p game with one player given room to act twice over."""
    st = G.new_game(2, seed=7)
    p = st.players[0]
    p.civil_actions = 20
    p.resources = 20
    p.science = 20
    p.food = 20
    p.workers_free = 5
    return st, p


def _grant_all(p):
    """As if Development of Civil Life had just resolved for `p`."""
    p.one_time_discount = {
        "increasePopulation": {"food": 1},
        "build": {"resources": 1},
        "developTechnology": {"science": 1},
    }


class BuildDiscountIsOneTime(unittest.TestCase):
    def test_second_build_costs_full_price(self):
        st, p = _st()
        _grant_all(p)
        # Religion: starting Age A temple, buildCost 3, 0 workers on it yet
        # (START_TECHS gives it worker=0) -- adding a worker is a plain build.
        before = p.resources
        A.do_build(st, p, "Religion")
        self.assertEqual(before - p.resources, 2,
                          "first build should be discounted 3 -> 2")
        before = p.resources
        A.do_build(st, p, "Religion")
        self.assertEqual(
            before - p.resources, 3,
            "REGRESSION: the one-shot build discount was never consumed and "
            "silently applied to a second build")


class PopulationDiscountIsOneTime(unittest.TestCase):
    def test_second_increase_costs_full_price(self):
        st, p = _st()
        _grant_all(p)
        before = p.food
        self.assertTrue(economy.increase_population(st, p))
        first = before - p.food
        before = p.food
        self.assertTrue(economy.increase_population(st, p))
        second = before - p.food
        self.assertEqual(
            second, first + 1,
            "REGRESSION: the one-shot population-increase discount was "
            "never consumed and silently applied to a second increase")


class DevelopDiscountIsOneTime(unittest.TestCase):
    def test_second_technology_costs_full_price(self):
        st, p = _st()
        _grant_all(p)
        # Irrigation (techCost 3) and Iron (techCost 5): two distinct Age I
        # technologies with a real printed science cost, so a second develop
        # is not just re-developing the same card.
        p.hand_civil = ["Irrigation", "Iron"]
        before = p.science
        A._h_develop(st, p, ("develop", "Irrigation"), None)
        self.assertEqual(before - p.science, 2,
                          "first develop should be discounted 3 -> 2")
        before = p.science
        A._h_develop(st, p, ("develop", "Iron"), None)
        self.assertEqual(
            before - p.science, 5,
            "REGRESSION: the one-shot develop discount was never consumed "
            "and silently applied to a second technology")


class CategoriesAreConsumedIndependently(unittest.TestCase):
    """Using one discount must not touch the other two (§ card text: three
    separate discounted actions, not one discount usable on anything)."""

    def test_spending_the_population_discount_leaves_build_and_develop_alone(self):
        st, p = _st()
        _grant_all(p)
        self.assertTrue(economy.increase_population(st, p))
        self.assertNotIn("increasePopulation", p.one_time_discount)
        self.assertIn("build", p.one_time_discount)
        self.assertIn("developTechnology", p.one_time_discount)
        # and the still-pending build discount is for real, not just a key
        before = p.resources
        A.do_build(st, p, "Religion")
        self.assertEqual(before - p.resources, 2,
                          "spending the population discount must not have "
                          "consumed the still-pending build discount")

    def test_spending_the_build_discount_leaves_population_and_develop_alone(self):
        st, p = _st()
        _grant_all(p)
        A.do_build(st, p, "Religion")
        self.assertNotIn("build", p.one_time_discount)
        self.assertIn("increasePopulation", p.one_time_discount)
        self.assertIn("developTechnology", p.one_time_discount)


if __name__ == "__main__":
    unittest.main()
