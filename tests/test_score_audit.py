"""End-of-game scoring, one section per card type (docs/SCORE_AUDIT.md).

The question this file answers is narrow: **when the game ends, does each
card contribute exactly the culture, science, strength, happiness, food,
resources and civil/military actions the printed 2015 base-game rules say it
should?**

Two rules for everything below, both learned the hard way (see
`docs/SCORE_VALIDATION.md` 3.3 and the government pricing bug of
2026-07-29):

1. **Every expected number is derived from the printed card, by hand, in the
   docstring** -- never copied out of the engine.  A test that pins today's
   answer is worse than no test.
2. **A value that lives in a field no reader touches is the bug class**, not
   an accident.  `HardcodedConstantsMatchTheData` and `EveryFieldHasAReader`
   at the bottom exist to make that shape loud instead of silent.

The eight bugs the audit found are FIXED, and the tests that found them now
assert the rules answer directly.  They ran as `@unittest.expectedFailure`
for exactly one commit, which is how each one was shown to fail for the right
reason before the fix rather than for an accident of the position.
"""
from __future__ import annotations

import json
import os
import re
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from engine import cards as C, economy, effects, events, game   # noqa: E402
from engine.state import TechCard                               # noqa: E402

_DB = C.db()
_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def position(techs=(), *, leader=None, wonders=(), flipped=(), free=1,
             government="Despotism", bank=18, colonies=(), tactic=None,
             players=2):
    """A game whose player 0 has exactly this tableau, and nothing else."""
    st = game.new_game(players, seed=1)
    # every seat starts empty, so an opponent's starting tableau can never
    # leak into a rating comparison (it did: it cost every war test 1 point
    # of strength advantage)
    for q in st.players:
        q.techs = {}
        q.leader = None
        q.tactic = None
        q.completed_wonders = []
        q.flipped_wonders = []
        q.colonies = []
    p = st.players[0]
    p.techs = {n: TechCard(name=n, workers=w) for n, w in dict(techs).items()}
    p.government = government
    p.leader = leader
    p.tactic = tactic
    p.completed_wonders = list(wonders)
    p.flipped_wonders = list(flipped)
    p.colonies = list(colonies)
    p.workers_free = free
    p.yellow_bank = bank
    effects.invalidate(st)
    for q in st.players:
        q.civil_actions = effects.state_stats(st, q).civil_actions
        q.military_actions = effects.state_stats(st, q).military_actions
    return st, p


def stats(st, p):
    return effects.state_stats(st, p)


def impact(st, p, name, order=None):
    """The culture the named Age III event awards this player."""
    block = (_DB.get(name).get("effects") or {}).get("allPlayers") or {}
    return events.scoring_culture(st, p, block, order or st.players)


# ===================================================================== farm
#   Agriculture A / Irrigation I / Selective Breeding II / Mechanized
#   Agriculture III produce 1 / 2 / 3 / 5 food PER WORKER.

class Farm(unittest.TestCase):
    def test_printed_food_per_worker(self):
        for name, per in (("Agriculture", 1), ("Irrigation", 2),
                          ("Selective Breeding", 3),
                          ("Mechanized Agriculture", 5)):
            st, p = position({name: 2})
            self.assertEqual(stats(st, p).food, 2 * per, name)

    def test_food_is_the_sum_over_cards(self):
        # 2 workers on Irrigation (2 each) + 1 on Agriculture (1) = 5
        st, p = position({"Irrigation": 2, "Agriculture": 1})
        self.assertEqual(stats(st, p).food, 5)

    def test_a_farm_with_no_worker_produces_nothing(self):
        st, p = position({"Mechanized Agriculture": 0})
        self.assertEqual(stats(st, p).food, 0)


# ===================================================================== mine
#   Bronze A / Iron I / Coal II / Oil III produce 1 / 2 / 3 / 5 resources.

class Mine(unittest.TestCase):
    def test_printed_resources_per_worker(self):
        for name, per in (("Bronze", 1), ("Iron", 2), ("Coal", 3),
                          ("Oil", 5)):
            st, p = position({name: 2})
            self.assertEqual(stats(st, p).resources, 2 * per, name)

    def test_impact_of_industry_is_mine_production(self):
        """"culture equal to the resources produced by their mines"."""
        st, p = position({"Coal": 2, "Bronze": 1})       # 3+3+1 = 7
        self.assertEqual(impact(st, p, "Impact of Industry"), 7)


# ====================================================================== lab
#   Philosophy A / Alchemy I / Scientific Method II / Computers III
#   produce 1 / 2 / 3 / 5 science.

class Lab(unittest.TestCase):
    def test_printed_science_per_worker(self):
        for name, per in (("Philosophy", 1), ("Alchemy", 2),
                          ("Scientific Method", 3), ("Computers", 5)):
            st, p = position({name: 2})
            self.assertEqual(stats(st, p).science, 2 * per, name)


# ================================================================== library
#   Printing Press I 1/1, Journalism II 2/2, Multimedia III 3/3
#   (science / culture).

class Library(unittest.TestCase):
    def test_printed_science_and_culture(self):
        for name, per in (("Printing Press", 1), ("Journalism", 2),
                          ("Multimedia", 3)):
            st, p = position({name: 2})
            self.assertEqual(stats(st, p).science, 2 * per, name)
            self.assertEqual(stats(st, p).culture, 2 * per, name)


# =================================================================== temple
#   Religion A 1 culture 1 happy, Theology I 1/2, Organized Religion II 1/3.

class Temple(unittest.TestCase):
    def test_printed_culture_and_happy(self):
        for name, happy in (("Religion", 1), ("Theology", 2),
                            ("Organized Religion", 3)):
            st, p = position({name: 2})
            self.assertEqual(stats(st, p).culture, 2, name)
            self.assertEqual(stats(st, p).happy, 2 * happy, name)


# ================================================================== theater
#   Drama I 2 culture 1 happy, Opera II 3/1, Movies III 4/1.

class Theater(unittest.TestCase):
    def test_printed_culture_and_happy(self):
        for name, cult in (("Drama", 2), ("Opera", 3), ("Movies", 4)):
            st, p = position({name: 2})
            self.assertEqual(stats(st, p).culture, 2 * cult, name)
            self.assertEqual(stats(st, p).happy, 2, name)


# ==================================================================== arena
#   Bread and Circuses I 2 happy 1 strength, Team Sports II 3/2,
#   Professional Sports III 4/3.  Arenas are the only urban building that
#   makes strength, which is why `Impact of Competition` names them next to
#   military units and why the Internet has to count them.

class Arena(unittest.TestCase):
    def test_printed_happy_and_strength(self):
        for name, happy, st_ in (("Bread and Circuses", 2, 1),
                                 ("Team Sports", 3, 2),
                                 ("Professional Sports", 4, 3)):
            s_, p = position({name: 2})
            self.assertEqual(stats(s_, p).happy, min(8, 2 * happy), name)
            self.assertEqual(stats(s_, p).strength, 2 * st_, name)


# ============================================= infantry / cavalry / artillery
#   Strength is printed per unit and every worker is one unit.

class Units(unittest.TestCase):
    STRENGTH = {"Warriors": 1, "Swordsmen": 2, "Riflemen": 3,
                "Modern Infantry": 5, "Knights": 2, "Cavalrymen": 3,
                "Tanks": 5, "Cannon": 3, "Rockets": 5, "Air Forces": 5}

    def test_printed_strength_per_worker(self):
        for name, per in self.STRENGTH.items():
            st, p = position({name: 2})
            self.assertEqual(stats(st, p).strength, 2 * per, name)

    def test_a_unit_card_with_no_worker_is_not_an_army(self):
        st, p = position({"Modern Infantry": 0})
        self.assertEqual(stats(st, p).strength, 0)


# ====================================================================== air
#   "No tactic requires air forces; an air force unit can join an army to
#   double the army's tactics bonus (each air force unit may be assigned to
#   at most one army)."

class AirForce(unittest.TestCase):
    def test_air_units_still_carry_their_own_printed_strength(self):
        st, p = position({"Air Forces": 1})
        self.assertEqual(stats(st, p).strength, 5)

    def test_one_air_force_doubles_one_armys_bonus(self):
        """Mechanized Army (cav + art + art) = 10.  One army, one air unit:
        10 + 10 = 20 on top of the units' own printed strength."""
        st, p = position({"Cavalrymen": 1, "Cannon": 2},
                         tactic="Mechanized Army")
        self.assertEqual(effects.army_strength(st, p), 10)
        st, p = position({"Cavalrymen": 1, "Cannon": 2, "Air Forces": 1},
                         tactic="Mechanized Army")
        self.assertEqual(effects.army_strength(st, p), 20)

    def test_air_cannot_double_an_army_that_does_not_exist(self):
        st, p = position({"Air Forces": 2}, tactic="Mechanized Army")
        self.assertEqual(effects.army_strength(st, p), 0)
    def test_a_second_air_force_doubles_the_OUTDATED_armys_smaller_bonus(self):
        """FIXED (audit 3.4).  Mechanized Army is Age III, so a unit of Age I
        is outdated: 1 fresh army (10) + 1 outdated army (5) = 15.  Two air
        units double one army each -- the fresh one (+10) and the outdated
        one (+5) -- so 30.  The engine prices BOTH doublings at the fresh
        army's 10 and returns 35.
        """
        st, p = position({"Cavalrymen": 1,      # Age II: fresh
                          "Knights": 1,         # Age I: outdated for a III tactic
                          "Cannon": 4,          # Age II: fresh
                          "Air Forces": 2},
                         tactic="Mechanized Army")
        self.assertEqual(effects.army_strength(st, p), 30)


# =============================================================== government
#   The eight governments, and the four numbers each of them carries.
#   civilActions / militaryActions / urbanBuildingLimit are TOP-LEVEL fields
#   (not `effects`), and `techCost` is null because a government is priced by
#   `revolutionCost` / `peacefulCost`.  Every one of those five fields was
#   read by nobody at some point on 2026-07-29; this class is the guard.

GOVERNMENTS = {
    # name: (civil, military, urban limit, revolution, peaceful)
    "Despotism":              (4, 2, 2, None, None),
    "Monarchy":               (5, 3, 3, 2, 8),
    "Theocracy":              (4, 3, 3, 1, 6),
    "Constitutional Monarchy": (6, 4, 3, 6, 12),
    "Republic":               (7, 2, 3, 3, 13),
    "Communism":              (7, 5, 4, 5, 19),
    "Fundamentalism":         (6, 5, 4, 7, 18),
    "Democracy":              (7, 3, 4, 9, 17),
}


class Government(unittest.TestCase):
    def test_action_totals_and_urban_limit(self):
        for name, (ca, ma, lim, _, _) in GOVERNMENTS.items():
            st, p = position(government=name)
            s = stats(st, p)
            self.assertEqual((s.civil_actions, s.military_actions,
                              s.urban_limit), (ca, ma, lim), name)

    def test_printed_production(self):
        """Only four governments produce anything at all."""
        cases = {
            "Theocracy": dict(culture=1, happy=1, strength=1),
            "Communism": dict(happy=-1),
            "Fundamentalism": dict(strength=5, science=-2),
            "Democracy": dict(culture=3),
        }
        for name, want in cases.items():
            st, p = position({"Journalism": 2}, government=name)
            s = stats(st, p)
            # Journalism x2 = 4 science, 4 culture, so nothing is clamped
            self.assertEqual(s.culture, 4 + want.get("culture", 0), name)
            self.assertEqual(s.science, 4 + want.get("science", 0), name)
            self.assertEqual(s.strength, want.get("strength", 0), name)
            self.assertEqual(s.happy, max(0, want.get("happy", 0)), name)

    def test_a_peaceful_change_costs_peacefulCost_science(self):
        for name, (_, _, _, _, peaceful) in GOVERNMENTS.items():
            if peaceful is None:
                continue
            st, p = position()
            self.assertEqual(effects.tech_cost(st, p, name), peaceful, name)

    def test_revolutionCost_is_what_a_revolution_charges(self):
        for name, (_, _, _, revolution, _) in GOVERNMENTS.items():
            if revolution is None:
                continue
            self.assertEqual(_DB.get(name)["revolutionCost"], revolution, name)
            st, p = position()
            p.science = 40
            p.hand_civil = [name]
            from engine import actions
            actions._h_revolution(st, p, ("revolution", name), None)
            self.assertEqual(p.science, 40 - revolution, name)
            self.assertEqual(p.government, name)

    def test_the_urban_limit_is_per_building_kind(self):
        """RB: the limit is on buildings of the SAME kind, so Despotism's 2
        allows 2 labs AND 2 temples, but not a third lab."""
        from engine import actions
        st, p = position({"Philosophy": 2, "Religion": 2})
        self.assertEqual(actions.urban_count(p, "lab"), 2)
        self.assertEqual(actions.urban_count(p, "temple"), 2)
        self.assertEqual(stats(st, p).urban_limit, 2)

    def test_science_cannot_be_driven_negative_by_fundamentalism(self):
        """Limits on Ratings: no rating is ever below zero."""
        st, p = position(government="Fundamentalism")
        self.assertEqual(stats(st, p).science, 0)

    def test_impact_of_government_scores_the_action_TOTALS(self):
        """"2 culture for each civil action and 1 for each military action."
        Democracy 7/3, +1 CA from the Pyramids, +1/+1 from the Kremlin
        => 9 civil, 4 military => 2*9 + 4 = 22."""
        st, p = position(government="Democracy",
                         wonders=["Pyramids", "Kremlin"])
        s = stats(st, p)
        self.assertEqual((s.civil_actions, s.military_actions), (9, 4))
        self.assertEqual(impact(st, p, "Impact of Government"), 22)

    def test_impact_of_progress_counts_the_governments_level(self):
        """"2 culture for each level of their special technologies and
        government."  Age A = level 0, I = 1, II = 2, III = 3."""
        st, p = position(government="Democracy")          # III = 3
        self.assertEqual(impact(st, p, "Impact of Progress"), 6)
        st, p = position(government="Despotism")          # A = 0
        self.assertEqual(impact(st, p, "Impact of Progress"), 0)


# ============================================================= special-tech
#   Twelve cards, four icons, at most one per icon in play at a time.

class SpecialTech(unittest.TestCase):
    def test_civil_and_military_action_grants(self):
        for name, ca, ma in (("Code of Laws", 1, 0), ("Justice System", 1, 0),
                             ("Civil Service", 2, 0), ("Warfare", 0, 1),
                             ("Strategy", 0, 2), ("Military Theory", 0, 3)):
            st, p = position({name: 0})
            s = stats(st, p)
            self.assertEqual(s.civil_actions, 4 + ca, name)
            self.assertEqual(s.military_actions, 2 + ma, name)

    def test_strength_and_colonization_grants(self):
        for name, stg, col in (("Warfare", 1, 0), ("Strategy", 3, 0),
                               ("Military Theory", 5, 0),
                               ("Cartography", 1, 2), ("Navigation", 2, 3),
                               ("Satellites", 3, 4)):
            st, p = position({name: 0})
            s = stats(st, p)
            self.assertEqual(s.strength, stg, name)
            self.assertEqual(s.colonize, col, name)

    def test_construction_techs_discount_urban_builds(self):
        """Engineering: Age I -1, Age II -2, Age III -3 resources."""
        st, p = position({"Engineering": 0})
        for card, printed, disc in (("Drama", 4, 1), ("Opera", 8, 2),
                                    ("Movies", 11, 3)):
            self.assertEqual(effects.build_cost(st, p, card), printed - disc,
                             card)

    def test_masonry_leaves_age_A_alone(self):
        """"Urban buildings of Age I+ cost 1 less resource (Age A unchanged)."
        Religion is Age A and stays at 3."""
        st, p = position({"Masonry": 0})
        self.assertEqual(effects.build_cost(st, p, "Religion"), 3)

    def test_wonder_stages_per_action(self):
        for name, n in (("Masonry", 2), ("Architecture", 3),
                        ("Engineering", 4)):
            st, p = position({name: 0})
            self.assertEqual(stats(st, p).wonder_stages, n, name)

    def test_at_most_one_card_per_icon(self):
        """7.6: developing a higher card of the same icon replaces it, so a
        player can never hold two -- which is why `Impact of Variety` may
        count special techs by name."""
        from engine import actions
        st, p = position()
        p.science = 99
        p.civil_actions = 9
        for name in ("Code of Laws", "Justice System", "Civil Service"):
            p.hand_civil = [name]
            actions._h_develop(st, p, ("develop", name), None)
        held = [n for n in p.techs if _DB.type_of(n) == "special-tech"]
        self.assertEqual(held, ["Civil Service"])
        self.assertEqual(stats(st, p).civil_actions, 6)      # 4 + 2

    def test_impact_of_progress_sums_special_tech_levels(self):
        """Engineering + Civil Service + Military Theory + Satellites, all
        Age III (level 3), under Democracy (3) = 15 levels => 30 culture."""
        st, p = position({"Engineering": 0, "Civil Service": 0,
                          "Military Theory": 0, "Satellites": 0},
                         government="Democracy")
        self.assertEqual(impact(st, p, "Impact of Progress"), 30)


# =================================================================== wonder
#   docs/SCORE_VALIDATION.md 6.1 verified all 16 wonders' 53 stage costs
#   against 18,307 human stage lines.  Re-checked here that the data the
#   engine charges from is still that data, plus every wonder's benefit.

class Wonder(unittest.TestCase):
    def test_flat_benefits(self):
        cases = {
            "Pyramids": dict(civil_actions=5),
            "Hanging Gardens": dict(culture=1, happy=2),
            "Colossus": dict(strength=2, colonize=1),
            "Library of Alexandria": dict(culture=1, science=1,
                                          civil_hand_limit=1,
                                          military_hand_limit=1),
            "Universitas Carolina": dict(culture=1, science=2),
            "Taj Mahal": dict(culture=3),
            "Eiffel Tower": dict(culture=4, happy=1),
            "Kremlin": dict(culture=2, civil_actions=5, military_actions=3),
        }
        for name, want in cases.items():
            st, p = position(wonders=[name])
            s = stats(st, p)
            for attr, val in want.items():
                self.assertEqual(getattr(s, attr), val, f"{name}.{attr}")

    def test_the_kremlin_costs_a_happy_face(self):
        """+2 culture, +1 CA, +1 MA, -1 happy face."""
        st, p = position({"Theology": 1})            # 2 happy
        self.assertEqual(stats(st, p).happy, 2)
        st, p = position({"Theology": 1}, wonders=["Kremlin"])
        self.assertEqual(stats(st, p).happy, 1)

    def test_the_great_wall_arms_infantry_and_artillery_only(self):
        """"each infantry and artillery unit gains +1 strength" -- cavalry
        does not.  2 Riflemen (3 each) + 1 Knight (2) = 8 printed, +2 for the
        two infantry, +1 culture +1 happy."""
        st, p = position({"Riflemen": 2, "Knights": 1}, wonders=["Great Wall"])
        self.assertEqual(stats(st, p).strength, 8 + 2)

    def test_the_railroad_doubles_ONE_worker_on_the_best_mine(self):
        """FAQ v1.5 p.9.  2 workers on Coal (3 each) = 6, +3 for one of them."""
        st, p = position({"Coal": 2}, wonders=["Transcontinental Railroad"])
        self.assertEqual(stats(st, p).resources, 9)
        self.assertEqual(impact(st, p, "Impact of Industry"), 9)

    def test_st_peters_adds_one_happy_face_per_happy_SOURCE(self):
        """+2 culture, +1 happy, and one extra happy per card providing happy.
        Two workers on Theology (2 happy each = 4) are two buildings, so:
        4 + 1 (the wonder) + 2 (one per temple) + 1 (the wonder is itself a
        happy source) = 8."""
        st, p = position({"Theology": 2}, wonders=["St. Peter's Basilica"])
        self.assertEqual(stats(st, p).happy, 8)
        self.assertEqual(stats(st, p).culture, 2 + 2)   # 2 temples + wonder

    def test_impact_of_wonders_pays_by_age(self):
        """5 for Age A, 4 for I, 3 for II, 2 for III."""
        st, p = position(wonders=["Pyramids", "Great Wall", "Kremlin",
                                  "Hollywood"])
        self.assertEqual(impact(st, p, "Impact of Wonders"), 5 + 4 + 3 + 2)

    def test_a_ruined_wonder_still_scores_impact_of_wonders(self):
        """Code of Laws p.12: a flipped wonder still counts as a completed
        wonder of its age for all purposes."""
        st, p = position(wonders=["Pyramids"], flipped=["Pyramids"])
        self.assertEqual(impact(st, p, "Impact of Wonders"), 5)

    def test_a_ruined_wonder_loses_its_effects_and_produces_2_culture(self):
        st, p = position(wonders=["Pyramids"], flipped=["Pyramids"])
        s = stats(st, p)
        self.assertEqual(s.civil_actions, 4)     # the +1 CA is gone
        self.assertEqual(s.culture, 2)           # ruins produce 2

    # --- the four Age III one-time bombs

    def one_time(self, st, p, wonder):
        return effects._one_time_culture(st, p, wonder)

    def test_first_space_flight_sums_every_technology_level(self):
        """"culture equal to the sum of levels of all technologies you have
        developed."  Computers III (3) + Journalism II (2) + Religion A (0)
        + Military Theory III (3) + Democracy III (3) = 11."""
        st, p = position({"Computers": 1, "Journalism": 1, "Religion": 1,
                          "Military Theory": 0}, government="Democracy")
        self.assertEqual(
            effects.on_wonder_complete(st, p, "First Space Flight"), 11)

    def test_fast_food_chains_pays_2_for_production_and_1_for_the_rest(self):
        """2 per worker on farms and mines, 1 per worker on urban/military."""
        st, p = position({"Irrigation": 2, "Coal": 1,        # 3 production
                          "Journalism": 2, "Riflemen": 1})   # 3 urban/military
        self.assertEqual(self.one_time(st, p, "Fast Food Chains"), 3 * 2 + 3)

    def test_hollywood_doubles_theater_and_library_CULTURE(self):
        """Movies 4 culture + Multimedia 3 culture = 7, doubled = 14.  The
        library's 3 SCIENCE is not culture and does not count."""
        st, p = position({"Movies": 1, "Multimedia": 1})
        self.assertEqual(self.one_time(st, p, "Hollywood"), 14)

    def test_hollywood_uses_effective_output_not_printed(self):
        """docs/SCORE_VALIDATION.md 3.3: Chaplin doubles the best theater, so
        Movies produces 8, not 4 => 2 * (8 + 3) = 22."""
        st, p = position({"Movies": 1, "Multimedia": 1}, leader="Charlie Chaplin")
        self.assertEqual(self.one_time(st, p, "Hollywood"), 22)

    def test_internet_counts_culture_science_and_strength_of_urban(self):
        """Multimedia 3+3, Professional Sports 4 happy + 3 strength, Computers
        5 science.  Happy is not one of the three: 3+3+3+5 = 14."""
        st, p = position({"Multimedia": 1, "Professional Sports": 1,
                          "Computers": 1})
        self.assertEqual(self.one_time(st, p, "Internet"), 14)

    def test_internet_matches_the_FAQ_sid_meier_example(self):
        """FAQ v1.5: two Age III Computers under Sid Meier produce a combined
        8 science and 6 culture, so the Internet scores 14."""
        st, p = position({"Computers": 2}, leader="Sid Meier")
        s = stats(st, p)
        self.assertEqual((s.science, s.culture), (8, 6))
        self.assertEqual(self.one_time(st, p, "Internet"), 14)

    def test_internet_ignores_a_farm_and_a_mine(self):
        st, p = position({"Multimedia": 1, "Oil": 2, "Mechanized Agriculture": 2})
        self.assertEqual(self.one_time(st, p, "Internet"), 6)


# =================================================================== leader
#   All 24.  Only the ones that move a rating or a score are asserted
#   numerically here; the rest are named in docs/SCORE_AUDIT.md 3.

class Leader(unittest.TestCase):
    def test_flat_rating_leaders(self):
        cases = {
            "Julius Caesar": dict(strength=1, military_actions=3),
            "Homer": dict(happy=1),
            "Joan of Arc": dict(culture=1, military_actions=3),
            "Napoleon Bonaparte": dict(military_actions=4),
            "Maximilien Robespierre": dict(military_actions=3),
            "William Shakespeare": dict(happy=1),
            "Charlie Chaplin": dict(happy=2),
            "Mahatma Gandhi": dict(culture=2),
        }
        for name, want in cases.items():
            st, p = position(leader=name)
            s = stats(st, p)
            for attr, val in want.items():
                self.assertEqual(getattr(s, attr), val, f"{name}.{attr}")

    def test_alexander_arms_every_unit(self):
        st, p = position({"Riflemen": 2, "Knights": 1},
                         leader="Alexander the Great")
        self.assertEqual(stats(st, p).strength, 3 + 3 + 2 + 3)

    def test_napoleon_pays_per_unit_TYPE_not_per_unit(self):
        """"+2 strength for each type of military unit you have."  Three
        Riflemen are one type."""
        st, p = position({"Riflemen": 3}, leader="Napoleon Bonaparte")
        self.assertEqual(stats(st, p).strength, 9 + 2)
        st, p = position({"Riflemen": 1, "Knights": 1, "Cannon": 1},
                         leader="Napoleon Bonaparte")
        self.assertEqual(stats(st, p).strength, 3 + 2 + 3 + 3 * 2)

    def test_joan_of_arc_arms_temple_and_government_happy(self):
        """"+1 strength for each happy face provided by your temples and your
        government."  Theocracy gives 1 happy, two Theology temples give 4."""
        st, p = position({"Theology": 2}, leader="Joan of Arc",
                         government="Theocracy")
        # Theocracy strength 1, Joan +1 per happy face: 4 temple + 1 government
        self.assertEqual(stats(st, p).strength, 1 + 5)

    def test_leonardo_newton_einstein_use_the_BEST_lab_or_library_level(self):
        for name in ("Leonardo da Vinci", "Isaac Newton", "Albert Einstein"):
            st, p = position({"Computers": 1, "Philosophy": 1}, leader=name)
            self.assertEqual(stats(st, p).science, 5 + 1 + 3, name)

    def test_an_UNSTAFFED_lab_produces_nothing_for_newton(self):
        """FIXED (audit 3.9).  "Your best lab or library PRODUCES extra
        science."  A building is a worker standing on a technology card; a
        card with no worker on it is a technology, not a lab, and produces
        nothing for a leader to add to.

        Decided three ways, all agreeing:
          * the engine's own reading of the same phrase everywhere else --
            Chaplin's best theater and the Railroad's best mine both pass
            `require_workers=True`, the latter on FAQ v1.5 p.9's "one worker
            on the best mine technology card THAT HAS WORKERS";
          * every other per-building leader (Sid Meier, Bill Gates, Bach,
            Shakespeare) multiplies by `t.workers`;
          * BGO itself: on 150 human games this reading agrees with BGO's own
            printed per-turn science on 7303/7600 rows against 7275/7600 for
            the other one (docs/SCORE_AUDIT.md 3.9).
        """
        for name in ("Leonardo da Vinci", "Isaac Newton", "Albert Einstein"):
            st, p = position({"Computers": 0, "Philosophy": 1}, leader=name)
            # only Philosophy is staffed, and Philosophy is Age A = level 0
            self.assertEqual(stats(st, p).science, 1, name)
            st, p = position({"Computers": 1}, leader=name)
            self.assertEqual(stats(st, p).science, 5 + 3, name)

    def test_an_age_A_lab_has_level_zero(self):
        """Age A cards carry no level number: Philosophy is level 0, so
        Newton adds nothing at all."""
        st, p = position({"Philosophy": 1}, leader="Isaac Newton")
        self.assertEqual(stats(st, p).science, 1)

    def test_sid_meier_converts_lab_science_into_culture(self):
        st, p = position({"Computers": 2}, leader="Sid Meier")
        s = stats(st, p)
        self.assertEqual((s.science, s.culture), (8, 6))

    def test_chaplin_doubles_ONE_theater_not_the_card(self):
        """docs/SCORE_BUGFIX.md: two workers on Movies produce 8; Chaplin
        doubles the best THEATER (one building), so 8 + 4 = 12."""
        st, p = position({"Movies": 2}, leader="Charlie Chaplin")
        self.assertEqual(stats(st, p).culture, 12)

    def test_shakespeare_pays_per_library_theater_PAIR(self):
        """2 culture per pair.  3 theaters and 1 library is one pair."""
        st, p = position({"Drama": 3, "Printing Press": 1},
                         leader="William Shakespeare")
        # 3*2 theater culture + 1 library culture + 2 for one pair
        self.assertEqual(stats(st, p).culture, 6 + 1 + 2)

    def test_bach_pays_per_theater_WORKER(self):
        st, p = position({"Drama": 2}, leader="J. S. Bach")
        self.assertEqual(stats(st, p).culture, 4 + 2)

    def test_james_cook_pays_2_for_the_first_colony_and_1_after(self):
        st, p = position(leader="James Cook", colonies=["Historic Territory (I)"])
        self.assertEqual(stats(st, p).culture, 2)
        st, p = position(leader="James Cook",
                         colonies=["Historic Territory (I)", "Vast Territory (I)"])
        self.assertEqual(stats(st, p).culture, 2 + 1)

    def test_michelangelo_pays_per_happy_face_from_temples_theaters_wonders(self):
        """Organized Religion 3 happy + Drama 1 happy + Hanging Gardens 2
        happy = 6 culture, on top of the cards' own culture."""
        st, p = position({"Organized Religion": 1, "Drama": 1},
                         leader="Michelangelo", wonders=["Hanging Gardens"])
        self.assertEqual(stats(st, p).culture, (1 + 2 + 1) + 6)
    def test_michelangelo_does_not_pay_for_a_RUINED_wonders_happy_faces(self):
        """FIXED (audit 3.3).  A wonder flipped by Ravages of Time provides no
        happy faces at all -- `compute` skips its effects and pays 2 culture
        of ruins instead.  Michelangelo should therefore see only the temple's
        1 happy face: 1 (temple culture) + 2 (ruins) + 1 (Michelangelo) = 4.
        The engine still counts the ruin's 2 happy faces and returns 6.
        """
        st, p = position({"Religion": 1}, leader="Michelangelo",
                         wonders=["Hanging Gardens"],
                         flipped=["Hanging Gardens"])
        self.assertEqual(stats(st, p).culture, 4)
    def test_st_peters_does_not_count_a_RUINED_wonder_as_a_happy_source(self):
        """FIXED (audit 3.3), the same inconsistency on the other card.  A
        ruined Hanging Gardens provides no happy faces, so it is not a happy
        source: the answer is the same 4 as without it."""
        # St. Peter's alone over one Religion temple: 1 (temple) + 1 (wonder)
        # + 2 (one extra per happy source: the temple and the wonder) = 4
        plain_st, plain_p = position({"Religion": 1},
                                     wonders=["St. Peter's Basilica"])
        self.assertEqual(stats(plain_st, plain_p).happy, 4)
        st, p = position({"Religion": 1},
                         wonders=["St. Peter's Basilica", "Hanging Gardens"],
                         flipped=["Hanging Gardens"])
        self.assertEqual(stats(st, p).happy, 4)
    def test_st_peters_counts_a_COLONY_as_a_happy_source(self):
        """FIXED (audit 3.7).  "every building/CARD providing happy faces
        provides one additional happy face" -- and `_happy_source_count`
        already reads that as "card", not "building", because it counts the
        government card and the leader card, neither of which is a building.
        It does not walk `p.colonies`, so a Historic Territory's happy face
        is a happy face that provides no extra one.

        1 (temple) + 1 (colony) + 1 (the wonder) + 3 extras = 6.
        """
        st, p = position({"Religion": 1}, wonders=["St. Peter's Basilica"],
                         colonies=["Historic Territory (I)"])
        self.assertEqual(stats(st, p).happy, 6)

    def test_st_peters_does_count_the_government_and_leader_cards(self):
        """The reading above is the engine's own: Theocracy's happy face and
        Homer's happy face each earn an extra one.
        1 (Theocracy) + 1 (Homer) + 1 (wonder) + 3 extras = 6."""
        st, p = position(government="Theocracy", leader="Homer",
                         wonders=["St. Peter's Basilica"])
        self.assertEqual(stats(st, p).happy, 6)

    def test_genghis_khan_pays_3_culture_for_a_top_two_strength(self):
        st, p = position({"Riflemen": 1}, leader="Genghis Khan")
        before = p.culture
        economy._end_of_turn_leader_bonus(st, p)
        self.assertEqual(p.culture, before + 3)

    def test_genghis_khan_pays_nothing_from_third_place(self):
        st, p = position({"Warriors": 1}, leader="Genghis Khan", players=3)
        for q in st.players[1:]:
            q.techs = {"Modern Infantry": TechCard(name="Modern Infantry",
                                                   workers=2)}
        effects.invalidate(st)
        before = p.culture
        economy._end_of_turn_leader_bonus(st, p)
        self.assertEqual(p.culture, before)

    def test_bill_gates_labs_make_resources_equal_to_their_level(self):
        st, p = position({"Computers": 2, "Alchemy": 1}, leader="Bill Gates")
        self.assertEqual(stats(st, p).resources, 3 + 3 + 1)

    def test_bill_gates_scores_that_production_again_at_game_end(self):
        """"When Bill Gates is removed from the game or the game ends, gain
        culture equal to that extra resource production."."""
        st, p = position({"Computers": 2, "Alchemy": 1}, leader="Bill Gates")
        self.assertEqual(effects.end_of_game_bonus(st, p), 7)

    def test_nobody_else_has_an_end_of_game_bonus(self):
        for name in ("Sid Meier", "Charlie Chaplin", None):
            st, p = position({"Computers": 2}, leader=name)
            self.assertEqual(effects.end_of_game_bonus(st, p), 0, str(name))
    def test_bill_gates_also_pays_when_he_LEAVES_play(self):
        """FIXED (audit 3.2).  "...removed from the game OR the game ends".
        Replacing Bill Gates with another leader must pay the culture; the
        engine pays only at game end, so the whole bonus is lost.
        """
        from engine import actions
        st, p = position({"Computers": 2}, leader="Bill Gates")
        p.hand_civil = ["Charlie Chaplin"]
        before = p.culture
        actions._h_play_leader(st, p, ("play_leader", "Charlie Chaplin"), None)
        self.assertEqual(p.culture, before + 6)

    def test_churchills_culture_option_is_3(self):
        from engine import actions
        st, p = position(leader="Winston Churchill")
        before = p.culture
        actions._h_churchill(st, p, ("churchill", "culture"), None)
        self.assertEqual(p.culture, before + 3)

    def test_robespierre_pays_3_culture_for_a_revolution(self):
        from engine import actions
        st, p = position(leader="Maximilien Robespierre")
        p.science = 40
        p.hand_civil = ["Monarchy"]
        before = p.culture
        actions._h_revolution(st, p, ("revolution", "Monarchy"), None)
        self.assertEqual(p.culture, before + 3)

    def test_aristotle_pays_1_science_per_technology_taken(self):
        st, p = position(leader="Aristotle")
        before = p.science
        effects.on_take_card(st, p, "Computers")
        self.assertEqual(p.science, before + 1)
        effects.on_take_card(st, p, "Hollywood")     # a wonder is not a tech
        self.assertEqual(p.science, before + 1)

    def test_einstein_pays_3_culture_per_technology_played(self):
        st, p = position(leader="Albert Einstein")
        before = p.culture
        effects.on_develop(st, p, "Computers")
        self.assertEqual(p.culture, before + 3)

    def test_gandhi_cannot_attack_and_costs_double_to_attack(self):
        st, p = position(leader="Mahatma Gandhi")
        self.assertTrue(stats(st, p).no_aggression)

    def test_homer_under_a_wonder_adds_a_happy_face(self):
        st, p = position(wonders=["Pyramids"])
        p.homer_wonder = "Pyramids"
        effects.invalidate(st, p)
        self.assertEqual(stats(st, p).happy, 1)


# ================================================================ territory
#   Twelve colony cards: an immediate bonus, then a permanent one.

class Territory(unittest.TestCase):
    def test_permanent_rating_symbols_reach_the_stats(self):
        st, p = position(colonies=["Historic Territory (I)"])   # +1 happy
        self.assertEqual(stats(st, p).happy, 1)
        st, p = position(colonies=["Strategic Territory (I)"])  # +2 strength
        self.assertEqual(stats(st, p).strength, 2)

    def test_token_grants_are_applied_once_not_as_a_rating(self):
        """+3 yellow / -1 blue on Vast Territory are one-time grants, not
        per-turn production."""
        from engine import interact
        st, p = position()
        blue, bank = p.blue_total, p.yellow_bank
        interact.gain_colony(st, p, "Vast Territory (I)")
        self.assertEqual(p.yellow_bank, bank + 3)
        self.assertEqual(p.blue_total, blue - 1)

    def test_the_immediate_bonus_is_paid_on_colonization(self):
        from engine import interact
        st, p = position()
        before = p.culture
        interact.gain_colony(st, p, "Historic Territory (I)")   # gain 6 culture
        self.assertEqual(p.culture, before + 6)

    def test_impact_of_colonies_pays_3_each(self):
        st, p = position(colonies=["Historic Territory (I)", "Vast Territory (I)"])
        self.assertEqual(impact(st, p, "Impact of Colonies"), 6)

    def test_losing_a_colony_takes_the_permanent_bonus_back(self):
        from engine import interact
        st, p = position()
        interact.gain_colony(st, p, "Strategic Territory (I)")
        self.assertEqual(stats(st, p).strength, 2)          # Age I copy: +2
        interact.lose_colony(st, p, "Strategic Territory (I)")
        self.assertEqual(stats(st, p).strength, 0)


# =============================================================== aggression
#   Eleven cards.  Only the culture-moving ones score.

class Aggression(unittest.TestCase):
    def test_armed_intervention_moves_up_to_7_culture(self):
        st, p = position()
        q = st.players[1]
        p.culture, q.culture = 10, 30
        ctx = {"attacker": 0, "player": 1, "card": "Aggression: Armed Intervention",
               "atk": 10, "dfn": 0}
        events.finish_aggression(st, ctx, None)
        self.assertEqual((p.culture, q.culture), (17, 23))

    def test_it_cannot_take_more_culture_than_the_victim_has(self):
        st, p = position()
        q = st.players[1]
        p.culture, q.culture = 0, 3
        ctx = {"attacker": 0, "player": 1, "card": "Aggression: Armed Intervention",
               "atk": 10, "dfn": 0}
        events.finish_aggression(st, ctx, None)
        self.assertEqual((p.culture, q.culture), (3, 0))

    def test_a_failed_aggression_moves_nothing(self):
        st, p = position()
        q = st.players[1]
        p.culture, q.culture = 0, 30
        ctx = {"attacker": 0, "player": 1, "card": "Aggression: Armed Intervention",
               "atk": 5, "dfn": 5}
        self.assertFalse(events.finish_aggression(st, ctx, None))
        self.assertEqual((p.culture, q.culture), (0, 30))

    def test_spy_moves_science(self):
        st, p = position()
        q = st.players[1]
        p.science, q.science = 0, 12
        ctx = {"attacker": 0, "player": 1, "card": "Aggression: Spy",
               "atk": 10, "dfn": 0}
        events.finish_aggression(st, ctx, None)
        self.assertEqual((p.science, q.science), (5, 7))

    def test_infiltrate_pays_3_culture_per_LEVEL_of_the_removed_card(self):
        """The multiplier is read from the card, not hardcoded."""
        eff = _DB.get("Aggression: Infiltrate (II)")["effects"] \
            if "Aggression: Infiltrate (II)" in _DB.by_name \
            else _DB.get("Aggression: Infiltrate")["effects"]
        self.assertEqual(eff["gainCulturePerLevelOfRemovedCard"], 3)


# ===================================================================== pact
#   Ten cards.  Every one of them is a per-turn rating change on one or both
#   parties, so they all land in `compute` and all of them score.

class Pact(unittest.TestCase):
    def sign(self, st, name, a=1, b=0):
        st.players[0].pacts = [{"name": name, "owner": 0, "partner": 1,
                                "a": a, "b": b}]
        effects.invalidate(st)

    def test_peace_treaty_pays_both_parties_1_culture(self):
        st, p = position()
        self.sign(st, "Peace Treaty")
        self.assertEqual(stats(st, p).culture, 1)
        self.assertEqual(stats(st, st.players[1]).culture, 1)

    def test_loss_of_sovereignty_is_plus_2_and_minus_2(self):
        st, p = position({"Drama": 2})                # 4 culture of its own
        self.sign(st, "Loss of Sovereignty", a=0, b=1)
        self.assertEqual(stats(st, p).culture, 4 + 2)
        st, p = position({"Drama": 2})
        self.sign(st, "Loss of Sovereignty", a=1, b=0)
        self.assertEqual(stats(st, p).culture, 4 - 2)

    def test_a_pact_cannot_drive_culture_below_zero(self):
        st, p = position()
        self.sign(st, "Loss of Sovereignty", a=1, b=0)
        self.assertEqual(stats(st, p).culture, 0)

    def test_international_tourism_pays_per_wonder_of_the_OTHER_party(self):
        st, p = position()
        # three wonders that produce no culture of their own, so the only
        # culture in sight is the pact's
        st.players[1].completed_wonders = ["Pyramids", "Colossus",
                                           "Ocean Liners"]
        self.sign(st, "International Tourism")
        self.assertEqual(stats(st, p).culture, 3)
        self.assertEqual(stats(st, st.players[1]).culture, 0)

    def test_military_alliance_arms_both_parties(self):
        st, p = position()
        self.sign(st, "Military Alliance")
        self.assertEqual(stats(st, p).strength, 3)
        self.assertEqual(stats(st, st.players[1]).strength, 3)

    def test_open_borders_gives_both_a_military_action(self):
        st, p = position()
        self.sign(st, "Open Borders Agreement")
        self.assertEqual(stats(st, p).military_actions, 3)

    def test_promise_of_military_protection(self):
        """A gains +1 culture; B gains +4 strength and loses 1 culture."""
        st, p = position({"Drama": 1})               # 2 culture
        self.sign(st, "Promise of Military Protection", a=0, b=1)
        self.assertEqual(stats(st, p).culture, 3)
        st, p = position({"Drama": 1})
        self.sign(st, "Promise of Military Protection", a=1, b=0)
        self.assertEqual(stats(st, p).culture, 1)
        self.assertEqual(stats(st, p).strength, 4)

    def test_acceptance_of_supremacy_moves_a_resource_of_production(self):
        st, p = position({"Coal": 1})                # 3 resources
        self.sign(st, "Acceptance of Supremacy", a=0, b=1)
        self.assertEqual(stats(st, p).resources, 4)
        st, p = position({"Coal": 1})
        self.sign(st, "Acceptance of Supremacy", a=1, b=0)
        self.assertEqual(stats(st, p).resources, 2)

    def test_scientific_cooperation_discounts_every_technology(self):
        st, p = position()
        self.sign(st, "Scientific Cooperation")
        self.assertEqual(stats(st, p).tech_discount, 2)
        self.assertEqual(effects.tech_cost(st, p, "Computers"), 8 - 2)


# ====================================================================== war
#   Three cards; each moves a resource from loser to victor by the strength
#   advantage.  The constants live in the card data AND in `resolve_war`.

class War(unittest.TestCase):
    def fight(self, name, atk_strength, dfn_strength):
        st, p = position({"Modern Infantry": 0})
        a, d = st.players[0], st.players[1]
        a.strength_extra = atk_strength
        d.strength_extra = dfn_strength
        effects.invalidate(st)
        a.war_declared_by_me = (name, 0, 1)
        d.wars_declared_on_me = [(name, 0, 1)]
        events.resolve_war(st, a, None)
        return st, a, d

    def test_a_war_cannot_take_culture_the_loser_does_not_have(self):
        st, a, d = self.fight("War over Culture", 12, 4)
        self.assertEqual((a.culture, d.culture), (0, 0))

    def test_war_over_culture_moves_real_culture(self):
        st, p = position()
        a, d = st.players[0], st.players[1]
        d.culture = 50
        a.strength_extra, d.strength_extra = 12, 4
        effects.invalidate(st)
        a.war_declared_by_me = ("War over Culture", 0, 1)
        events.resolve_war(st, a, None)
        self.assertEqual((a.culture, d.culture), (13, 37))

    def test_war_over_technology_takes_science_equal_to_the_advantage(self):
        st, p = position()
        a, d = st.players[0], st.players[1]
        d.science = 20
        a.strength_extra, d.strength_extra = 9, 3
        effects.invalidate(st)
        a.war_declared_by_me = ("War over Technology", 0, 1)
        events.resolve_war(st, a, None)
        self.assertEqual((a.science, d.science), (6, 14))

    def test_war_over_territory_takes_1_token_per_5_points_of_advantage(self):
        st, p = position()
        a, d = st.players[0], st.players[1]
        a.strength_extra, d.strength_extra = 11, 0
        effects.invalidate(st)
        bank = d.yellow_bank
        a.war_declared_by_me = ("War over Territory", 0, 1)
        events.resolve_war(st, a, None)
        # advantage 11 => 1 + 11//5 = 3 tokens
        self.assertEqual(d.yellow_bank, bank - 3)

    def test_a_drawn_war_moves_nothing(self):
        st, p = position()
        a, d = st.players[0], st.players[1]
        d.culture = 50
        a.strength_extra = d.strength_extra = 7
        effects.invalidate(st)
        a.war_declared_by_me = ("War over Culture", 0, 1)
        events.resolve_war(st, a, None)
        self.assertEqual((a.culture, d.culture), (0, 50))

    def test_the_DEFENDER_can_win_a_war_and_take_the_spoils(self):
        st, p = position()
        a, d = st.players[0], st.players[1]
        a.culture = 40
        a.strength_extra, d.strength_extra = 2, 10
        effects.invalidate(st)
        a.war_declared_by_me = ("War over Culture", 0, 1)
        events.resolve_war(st, a, None)
        self.assertEqual((a.culture, d.culture), (40 - 13, 13))


# =================================================================== tactic
#   Fifteen cards.  An army is one copy of the composition; a tactic pays its
#   bonus per army, and the smaller `obsoleteStrength` for an army containing
#   a unit more than one age older than the tactic.

class Tactic(unittest.TestCase):
    def test_one_army_pays_the_printed_bonus(self):
        st, p = position({"Swordsmen": 2}, tactic="Fighting Band")
        self.assertEqual(effects.army_strength(st, p), 1)

    def test_two_copies_of_the_composition_are_two_armies(self):
        st, p = position({"Swordsmen": 4}, tactic="Fighting Band")
        self.assertEqual(effects.army_strength(st, p), 2)

    def test_an_incomplete_composition_is_no_army(self):
        st, p = position({"Swordsmen": 1}, tactic="Fighting Band")
        self.assertEqual(effects.army_strength(st, p), 0)

    def test_a_mixed_composition_needs_every_type(self):
        st, p = position({"Swordsmen": 3}, tactic="Medieval Army")
        self.assertEqual(effects.army_strength(st, p), 0)
        st, p = position({"Swordsmen": 1, "Knights": 1},
                         tactic="Medieval Army")
        self.assertEqual(effects.army_strength(st, p), 2)

    def test_an_outdated_army_pays_the_smaller_bonus(self):
        """Napoleonic Army is Age II (7 / 4 obsolete).  A unit of Age A is
        more than one age older, so the army is outdated."""
        st, p = position({"Riflemen": 1, "Cavalrymen": 1, "Cannon": 1},
                         tactic="Napoleonic Army")
        self.assertEqual(effects.army_strength(st, p), 7)
        st, p = position({"Warriors": 1, "Cavalrymen": 1, "Cannon": 1},
                         tactic="Napoleonic Army")
        self.assertEqual(effects.army_strength(st, p), 4)

    def test_an_age_I_tactic_has_no_obsolete_value(self):
        st, p = position({"Warriors": 2}, tactic="Fighting Band")
        self.assertEqual(effects.army_strength(st, p), 1)

    def test_genghis_khan_lets_infantry_fill_cavalry_slots(self):
        st, p = position({"Knights": 3}, tactic="Heavy Cavalry")
        self.assertEqual(effects.army_strength(st, p), 4)
        st, p = position({"Swordsmen": 3}, tactic="Heavy Cavalry")
        self.assertEqual(effects.army_strength(st, p), 0)
        st, p = position({"Swordsmen": 3}, tactic="Heavy Cavalry",
                         leader="Genghis Khan")
        self.assertEqual(effects.army_strength(st, p), 4)

    def test_the_tactic_bonus_is_part_of_the_strength_rating(self):
        st, p = position({"Swordsmen": 2}, tactic="Fighting Band")
        self.assertEqual(stats(st, p).strength, 2 + 2 + 1)

    def test_no_tactic_means_no_armies(self):
        st, p = position({"Swordsmen": 4})
        self.assertEqual(stats(st, p).strength, 8)


# ==================================================================== bonus
#   Three "Military Bonus" cards.  They are played FROM HAND during a defense
#   or a colonization, so they must contribute nothing while they sit there.

class Bonus(unittest.TestCase):
    def test_a_bonus_card_in_hand_changes_no_rating(self):
        st, p = position({"Swordsmen": 2})
        base = stats(st, p).__dict__.copy()
        p.hand_military = ["Military Bonus (defense 6 / colonization 3)"]
        effects.invalidate(st, p)
        self.assertEqual(stats(st, p).__dict__, base)

    def test_the_three_printed_values(self):
        for name, dfn, col in (("Military Bonus (defense 2 / colonization 1)", 2, 1),
                               ("Military Bonus (defense 4 / colonization 2)", 4, 2),
                               ("Military Bonus (defense 6 / colonization 3)", 6, 3)):
            eff = _DB.get(name)["effects"]
            self.assertEqual((eff["defenseBonus"], eff["colonizationBonus"]),
                             (dfn, col), name)


# =================================================================== action
#   33 yellow cards.  Three of them move culture or science directly.

class ActionCard(unittest.TestCase):
    def test_cultural_heritage_pays_its_printed_culture_and_science(self):
        from engine import actions
        for name, cult, sci in ((("Cultural Heritage (A)"), 4, 1),):
            st, p = position()
            before = (p.culture, p.science)
            actions.apply_card_gains(st, p, _DB.get(name)["effects"])
            self.assertEqual((p.culture, p.science),
                             (before[0] + cult, before[1] + sci))

    def test_revolutionary_idea_pays_science(self):
        from engine import actions
        st, p = position()
        before = p.science
        actions.apply_card_gains(st, p, {"gainScience": 6})
        self.assertEqual(p.science, before + 6)

    def test_endowment_for_the_arts_pays_per_richer_civilization(self):
        """2p: 6 culture for each civilization with more culture than you."""
        from engine import actions
        st, p = position()
        p.culture, st.players[1].culture = 10, 40
        p.hand_civil = ["Endowment for the Arts"]
        p.civil_actions = 4
        actions._h_play_action(st, p, ("play_action",
                                       "Endowment for the Arts"), None)
        self.assertEqual(p.culture, 16)

    def test_endowment_pays_nothing_when_you_lead(self):
        from engine import actions
        st, p = position()
        p.culture, st.players[1].culture = 40, 10
        p.hand_civil = ["Endowment for the Arts"]
        p.civil_actions = 4
        actions._h_play_action(st, p, ("play_action",
                                       "Endowment for the Arts"), None)
        self.assertEqual(p.culture, 40)


# ==================================================================== event
#   55 cards.  15 of them are the Age III scoring events -- the whole
#   end-of-game payout -- and the rest are checked for the culture they move.

class ScoringEvents(unittest.TestCase):
    def test_impact_of_agriculture_bonus_needs_production_over_consumption(self):
        """"culture equal to the food produced by their farms.  If production
        exceeds consumption, they gain 4 more culture."  A bank of 18 eats 0,
        so 2 food beats it: 2 + 4 = 6."""
        st, p = position({"Agriculture": 2}, bank=18)
        self.assertEqual(economy.consumption(p.yellow_bank), 0)
        self.assertEqual(impact(st, p, "Impact of Agriculture"), 6)

    def test_impact_of_agriculture_with_no_farms_at_all(self):
        st, p = position(bank=1)          # consumption 4, production 0
        self.assertEqual(impact(st, p, "Impact of Agriculture"), 0)
    def test_impact_of_agriculture_scores_FARMS_not_the_food_rating(self):
        """FIXED (audit 3.1).  It is `Impact of Industry` (SCORE_VALIDATION
        3.1) again on the other card.  The card scores "the food produced by
        their farms"; the engine scores `s.food`, the whole rating, which also
        carries a pact's food symbol.  Two Agriculture workers = 2 farm food,
        +4 for beating consumption = 6, whatever the pact adds.
        """
        st, p = position({"Agriculture": 2}, bank=18)
        p.pacts = [{"name": "International Trade Agreement",
                    "owner": 0, "partner": 1, "a": 1, "b": 0}]
        effects.invalidate(st)
        self.assertEqual(stats(st, p).food, 3)          # 2 farm + 1 pact
        self.assertEqual(impact(st, p, "Impact of Agriculture"), 6)

    def test_impact_of_competition(self):
        """1 culture per LEVEL of military units and arenas.  Modern Infantry
        III (3) x2 + Knights I (1) + Professional Sports III (3) = 10."""
        st, p = position({"Modern Infantry": 2, "Knights": 1,
                          "Professional Sports": 1})
        self.assertEqual(impact(st, p, "Impact of Competition"), 10)

    def test_impact_of_architecture(self):
        """1 culture per level of urban buildings.  Multimedia III x2 (6) +
        Drama I (1) + Professional Sports III (3) = 10.  Farms and mines are
        not urban buildings."""
        st, p = position({"Multimedia": 2, "Drama": 1,
                          "Professional Sports": 1, "Oil": 2})
        self.assertEqual(impact(st, p, "Impact of Architecture"), 10)

    def test_impact_of_technology(self):
        """4 culture per Age III technology, government included."""
        st, p = position({"Computers": 1, "Oil": 1, "Military Theory": 0,
                          "Drama": 1}, government="Democracy")
        self.assertEqual(impact(st, p, "Impact of Technology"), 4 * 4)

    def test_impact_of_population(self):
        """2 culture per content worker beyond the first ten.  8 on cards +
        4 unused = 12 workers, no discontent => 2 * 2 = 4."""
        st, p = position({"Oil": 4, "Computers": 4}, free=4, bank=18)
        self.assertEqual(economy.discontent(st, p), 0)
        self.assertEqual(impact(st, p, "Impact of Population"), 4)

    def test_impact_of_population_never_goes_negative(self):
        st, p = position({"Oil": 1}, free=0, bank=18)
        self.assertEqual(impact(st, p, "Impact of Population"), 0)

    def test_impact_of_happiness(self):
        """2 culture per happy face (max 16), -2 per discontent worker.
        Two Organized Religion workers = 6 happy => 12."""
        st, p = position({"Organized Religion": 2}, bank=18)
        self.assertEqual(stats(st, p).happy, 6)
        self.assertEqual(impact(st, p, "Impact of Happiness"), 12)

    def test_impact_of_happiness_is_capped_at_16(self):
        st, p = position({"Organized Religion": 3}, bank=18)   # 9 happy
        self.assertEqual(impact(st, p, "Impact of Happiness"), 16)

    def test_impact_of_happiness_charges_for_discontent(self):
        """Bank 1 needs 7 happy faces; one Religion worker gives 1, so 6
        workers are discontent: 2*1 - 2*6 = -10."""
        st, p = position({"Religion": 1}, bank=1)
        self.assertEqual(economy.discontent(st, p), 6)
        self.assertEqual(impact(st, p, "Impact of Happiness"), 2 - 12)

    def test_impact_of_balance_scores_the_lowest_of_four_ratings(self):
        """2 culture times the lowest of food, resources, science, culture."""
        st, p = position({"Irrigation": 2,        # 4 food
                          "Coal": 1,              # 3 resources
                          "Computers": 1,         # 5 science
                          "Drama": 1})            # 2 culture, 1 happy
        s = stats(st, p)
        self.assertEqual((s.food, s.resources, s.science, s.culture),
                         (4, 3, 5, 2))
        self.assertEqual(impact(st, p, "Impact of Balance"), 4)

    def test_impact_of_balance_ignores_strength_and_happiness(self):
        st, p = position({"Irrigation": 1, "Bronze": 1, "Philosophy": 1,
                          "Religion": 1})
        # food 2, resources 1, science 1, culture 1 -> lowest 1 -> 2 culture
        self.assertEqual(impact(st, p, "Impact of Balance"), 2)

    def test_impact_of_variety(self):
        """2 culture per different type of military unit, urban building and
        special technology.  infantry + cavalry (2) + temple + lab (2) +
        3 special techs = 7 kinds = 14."""
        st, p = position({"Warriors": 1, "Knights": 1, "Religion": 1,
                          "Philosophy": 1, "Code of Laws": 0, "Warfare": 0,
                          "Masonry": 0})
        self.assertEqual(impact(st, p, "Impact of Variety"), 14)

    def test_impact_of_variety_needs_a_WORKER_on_the_building(self):
        """A technology card with no worker on it is a technology, not a
        building: there is no such building to have a type."""
        st, p = position({"Religion": 1, "Drama": 0})
        self.assertEqual(impact(st, p, "Impact of Variety"), 2)

    def test_the_two_ranking_events_pay_by_rank(self):
        """10 / 0 at 2 players, for strength and for science production."""
        st = game.new_game(2, seed=3)
        a, b = st.players
        a.techs = {"Modern Infantry": TechCard(name="Modern Infantry", workers=2)}
        b.techs = {"Warriors": TechCard(name="Warriors", workers=1)}
        effects.invalidate(st)
        events.resolve_event(st, "Impact of Strength", None, 0)
        self.assertEqual((a.culture, b.culture), (10, 0))

    def test_a_ranking_tie_is_broken_by_turn_order(self):
        st = game.new_game(2, seed=3)
        a, b = st.players
        for q in (a, b):
            q.techs = {"Warriors": TechCard(name="Warriors", workers=1)}
        effects.invalidate(st)
        events.resolve_event(st, "Impact of Strength", None, 0)
        self.assertEqual((a.culture, b.culture), (10, 0))

    def test_every_age_III_event_is_a_scoring_event_and_vice_versa(self):
        """`evaluate_final_events` selects the end-of-game payout by AGE, not
        by the `scoringEvent` flag, so the two must agree exactly or the
        remaining events pay the wrong thing at game end."""
        for c in _DB.cards:
            if c["type"] != "event":
                continue
            self.assertEqual(c["age"] == "III", bool(c.get("scoringEvent")),
                             c["name"])
            if c["age"] == "III":
                self.assertIn("allPlayers", c["effects"], c["name"])

    def test_unrevealed_age_III_events_pay_out_at_game_end(self):
        st, p = position({"Coal": 2})
        st.current_events = ["Impact of Industry"]
        st.future_events = ["Impact of Wonders"]
        p.completed_wonders = ["Pyramids"]
        effects.invalidate(st)
        before = [q.culture for q in st.players]
        events.evaluate_final_events(st)
        self.assertEqual(p.culture - before[0], 6 + 5)

    def test_a_non_age_III_event_left_in_the_deck_pays_nothing(self):
        st, p = position({"Coal": 2})
        st.current_events = ["Cultural Influence"]
        before = p.culture
        events.evaluate_final_events(st)
        self.assertEqual(p.culture, before)


class NonScoringEvents(unittest.TestCase):
    """The Age A-II events that still move culture, science or a rating."""

    def test_cultural_influence_pays_the_culture_rating(self):
        st, p = position({"Drama": 2})           # 4 culture
        before = p.culture
        events.resolve_event(st, "Cultural Influence", None, 0)
        self.assertEqual(p.culture - before, 4)

    def test_popularization_of_science_pays_the_SCIENCE_rating_as_culture(self):
        st, p = position({"Computers": 1})       # 5 science
        before = p.culture
        events.resolve_event(st, "Popularization of Science", None, 0)
        self.assertEqual(p.culture - before, 5)

    def test_scientific_breakthrough_pays_the_science_rating_as_science(self):
        st, p = position({"Computers": 1})
        before = p.science
        events.resolve_event(st, "Scientific Breakthrough", None, 0)
        self.assertEqual(p.science - before, 5)

    def test_civil_unrest_charges_4_culture_per_discontent_worker(self):
        st, p = position({"Religion": 1}, bank=1)     # 6 discontent
        p.culture = 100
        st.players[1].techs = {"Organized Religion":
                               TechCard(name="Organized Religion", workers=3)}
        st.players[1].yellow_bank = 18
        st.players[1].culture = 100
        effects.invalidate(st)
        events.resolve_event(st, "Civil Unrest", None, 0)
        self.assertEqual(p.culture, 100 - 24)
        self.assertEqual(st.players[1].culture, 100)

    def test_national_pride_pays_the_culture_leader_5(self):
        st, p = position()
        p.culture, st.players[1].culture = 50, 10
        events.resolve_event(st, "National Pride", None, 0)
        self.assertEqual((p.culture, st.players[1].culture), (55, 10))

    def test_crusades_pays_the_strongest_and_charges_the_weakest(self):
        st, p = position({"Modern Infantry": 2})
        q = st.players[1]
        p.culture = q.culture = 20
        effects.invalidate(st)
        events.resolve_event(st, "Crusades", None, 0)
        self.assertEqual((p.culture, q.culture), (24, 16))

    def test_culture_never_goes_below_zero_from_an_event(self):
        st, p = position({"Modern Infantry": 2})
        q = st.players[1]
        p.culture, q.culture = 20, 1
        effects.invalidate(st)
        events.resolve_event(st, "Crusades", None, 0)
        self.assertEqual(q.culture, 0)

    def test_politics_of_strength_pays_culture_in_the_last_round(self):
        """"In the last round they gain / lose culture instead": +5 / -3."""
        st, p = position({"Modern Infantry": 2})
        q = st.players[1]
        p.culture = q.culture = 20
        st.last_round = True
        effects.invalidate(st)
        events.resolve_event(st, "Politics of Strength", None, 0)
        self.assertEqual((p.culture, q.culture), (25, 17))

    def test_a_mid_game_ranking_reveal_breaks_ties_toward_the_CURRENT_player(self):
        """RULES_SPEC 5.3 [CoL p.7]: "ties broken in favor of the current
        player, then proximity in clockwise order after the current player".
        Player 1 reveals the event, so player 1 wins a tied strength race."""
        st = game.new_game(3, seed=3)
        for q in st.players:
            q.techs = {"Warriors": TechCard(name="Warriors", workers=1)}
            q.culture = 0
        effects.invalidate(st)
        events.resolve_event(st, "Impact of Strength", None, 1)
        self.assertEqual([q.culture for q in st.players], [0, 14, 7])

    def test_the_END_OF_GAME_ranking_breaks_ties_toward_the_START_player(self):
        """RULES_SPEC 12.5.2: "ranked ones use the standard tie-breaker AS IF
        it were the starting player's turn"."""
        st = game.new_game(3, seed=3)
        for q in st.players:
            q.techs = {"Warriors": TechCard(name="Warriors", workers=1)}
            q.culture = 0
        st.start_player = 2
        st.current = 0
        st.current_events = ["Impact of Strength"]
        effects.invalidate(st)
        events.evaluate_final_events(st)
        self.assertEqual([q.culture for q in st.players], [7, 0, 14])

    def test_ravages_of_time_ruins_produce_the_value_printed_on_the_card(self):
        """The engine hardcodes +2; the card says `ruinsCultureProduction: 2`.
        If the data ever changes, this test is the one that notices."""
        eff = _DB.get("Ravages of Time")["effects"]["allPlayers"]
        st, p = position(wonders=["Pyramids"], flipped=["Pyramids"])
        self.assertEqual(stats(st, p).culture, eff["ruinsCultureProduction"])

    def test_good_harvest_ignores_corruption_and_consumption(self):
        """"Players produce food, ignoring corruption and consumption.""" ""
        st, p = position({"Irrigation": 2}, bank=1)   # 4 food, consumption 4
        p.food = 0
        events.resolve_event(st, "Good Harvest", None, 0)
        self.assertEqual(p.food, 4)

    def test_economic_progress_applies_corruption_then_consumption(self):
        """Order printed on the card: corruption, food, consumption,
        resources.  Blue available drives corruption; bank 18 eats 0 food."""
        st, p = position({"Irrigation": 2, "Coal": 1}, bank=18)
        p.food = p.resources = 0
        events.resolve_event(st, "Economic Progress", None, 0)
        self.assertEqual(economy.consumption(p.yellow_bank), 0)
        self.assertEqual(p.food, 4)

    def test_prosperity_gives_food_up_to_the_printed_maximum(self):
        st, p = position({"Organized Religion": 2}, bank=18)   # 6 happy
        p.food = 0
        events.resolve_event(st, "Prosperity", None, 0)
        self.assertEqual(p.food, 6)


class ForecastVersusPayout(unittest.TestCase):
    """The one place `final_event_culture` and `evaluate_final_events` differ.

    `tests/test_event_scoring.py` pins that they agree, but its game-driven
    comparison SKIPS every row where the payout's per-award zero clamp could
    have fired (`if b + f >= 0`), so the divergence itself was never asserted
    on a position that produces it.  A skipped case is not a checked case.

    The divergence is deliberate and documented in `final_event_awards`: the
    payout clamps a player's running culture at zero after EACH award, the
    forecast sums the raw awards.  This constructs the position and states
    the size of the gap, so that closing it (or widening it) is a decision
    somebody makes on purpose.
    """

    def near_zero_with_a_negative_board(self):
        """1 culture, 7 discontent workers, `Impact of Happiness` pending:
        2 x 0 happy faces - 2 x 7 discontent = -14 owed against 1 banked."""
        st, p = position({"Religion": 0}, bank=1)
        for q in st.players:
            q.culture = 1
            q.yellow_bank = 1
        st.current_events = ["Impact of Happiness"]
        st.future_events = []
        effects.invalidate(st)
        return st, p

    def test_the_forecast_is_the_raw_sum(self):
        st, p = self.near_zero_with_a_negative_board()
        self.assertEqual(economy.discontent(st, p), 7)
        self.assertEqual(events.final_event_culture(st)[0], -14)

    def test_the_payout_clamps_at_zero_and_the_two_therefore_differ(self):
        st, p = self.near_zero_with_a_negative_board()
        forecast = events.final_event_culture(st)[0]
        events.evaluate_final_events(st)
        self.assertEqual(p.culture, 0)          # clamped, not -13
        self.assertEqual(forecast, -14)
        # the gap the bot's forecast carries on this board
        self.assertEqual(forecast - (0 - 1), -13)

    def test_they_agree_whenever_no_clamp_fires(self):
        st, p = position({"Coal": 2})
        for q in st.players:
            q.culture = 100
        st.current_events = ["Impact of Industry"]
        st.future_events = []
        effects.invalidate(st)
        forecast = events.final_event_culture(st)[0]
        before = p.culture
        events.evaluate_final_events(st)
        self.assertEqual(forecast, p.culture - before)


class PluralTargets(unittest.TestCase):
    """RULES_SPEC 5.3 [CoL p.7]: `"All civilizations" with most/least: ALL
    tied civs affected, no tie-break.`

    Two event cards are worded that way -- Immigration ("the players with the
    most happy faces") and Civil Unrest ("the players with the most discontent
    workers") -- against the six that name a single "strongest/weakest player"
    and are correctly tie-broken by turn order.
    """

    def three_tied_on_happiness(self):
        st = game.new_game(3, seed=5)
        for q in st.players:
            q.techs = {"Theology": TechCard(name="Theology", workers=1)}
            q.yellow_bank = 18
            q.food = 20
            q.workers_free = 0
        effects.invalidate(st)
        return st

    def test_a_single_target_event_IS_tie_broken(self):
        """Contrast: "The player with the most culture gains 5 culture" names
        one player, so exactly one gets it."""
        st = game.new_game(3, seed=5)
        for q in st.players:
            q.culture = 10
        events.resolve_event(st, "National Pride", None, 0)
        self.assertEqual([q.culture for q in st.players], [15, 10, 10])
    def test_immigration_grows_EVERY_player_tied_on_happy_faces(self):
        """FIXED (audit 3.5).  All three are tied on 2 happy faces, so all three
        increase population; the engine picks one by turn order."""
        st = self.three_tied_on_happiness()
        self.assertEqual({effects.state_stats(st, q).happy for q in st.players},
                         {2})
        events.resolve_event(st, "Immigration", None, 0)
        self.assertEqual([q.workers_free for q in st.players], [1, 1, 1])
    def test_civil_unrest_taxes_EVERY_player_tied_on_discontent(self):
        """FIXED (audit 3.5), the same wording on the other card."""
        st = game.new_game(3, seed=5)
        for q in st.players:
            q.techs = {}
            q.yellow_bank = 1          # 7 happy faces required, 0 provided
            q.culture = 100
        effects.invalidate(st)
        blue = [q.blue_total for q in st.players]
        events.resolve_event(st, "Civil Unrest", None, 0)
        self.assertEqual([q.blue_total for q in st.players],
                         [b - 1 for b in blue])


# ============================================================== composition
#   Where single-card tests pass and the game still scores wrong.

class Composition(unittest.TestCase):
    def test_chaplin_and_shakespeare_and_hollywood_together(self):
        """Movies (4 culture) + Printing Press (1 culture, 1 science).
        Chaplin doubles the best theater (+4); Shakespeare would pay 2 per
        pair but only one leader is in play at a time -- so with Chaplin the
        theaters and libraries produce 4 + 4 + 1 = 9 and Hollywood pays 18."""
        st, p = position({"Movies": 1, "Printing Press": 1},
                         leader="Charlie Chaplin")
        self.assertEqual(effects.building_output(
            st.players[0], frozenset({"theater", "library"}), ("culture",)), 9)
        self.assertEqual(effects._one_time_culture(st, p, "Hollywood"), 18)

    def test_sid_meier_cannot_drive_the_science_rating_negative(self):
        """-1 science per lab, with only Age A labs (1 science each): the
        rating floors at 0, it does not go to -1."""
        st, p = position({"Philosophy": 2}, leader="Sid Meier")
        self.assertEqual(stats(st, p).science, 0)

    def test_a_negative_rating_is_zero_not_negative(self):
        st, p = position(government="Fundamentalism")
        s = stats(st, p)
        for attr in ("science", "culture", "food", "resources", "strength",
                     "happy"):
            self.assertGreaterEqual(getattr(s, attr), 0, attr)

    def test_happiness_is_clamped_into_0_8(self):
        """The engine clamps happy into [0, 8].  8 is exactly the most the
        rules ever ask for (`happy_required` maxes at 8) and exactly the most
        `Impact of Happiness` can pay for (16 culture / 2), so the clamp is
        invisible to scoring -- but it IS an engine choice, not a printed
        rule, and it is asserted here so a change to it is deliberate."""
        st, p = position({"Professional Sports": 3})     # 12 happy faces
        self.assertEqual(stats(st, p).happy, 8)
        self.assertEqual(economy.happy_required(0), 8)
        self.assertEqual(
            _DB.get("Impact of Happiness")["effects"]["allPlayers"]
            ["maxCultureFromHappyFaces"], 16)

    def test_government_and_wonder_and_special_tech_actions_add_up(self):
        """Democracy 7 CA, Pyramids +1, Kremlin +1, Code of Laws +1,
        Civil Service +2 -- but only one law card can be in play, so 7+1+1+2
        = 11 civil actions."""
        st, p = position({"Civil Service": 0}, government="Democracy",
                         wonders=["Pyramids", "Kremlin"])
        self.assertEqual(stats(st, p).civil_actions, 11)
        self.assertEqual(impact(st, p, "Impact of Government"),
                         2 * 11 + 1 * 4)

    def test_a_ruined_wonder_pays_no_action_and_no_hand_limit(self):
        st, p = position(wonders=["Pyramids", "Library of Alexandria"],
                         flipped=["Pyramids"])
        s = stats(st, p)
        self.assertEqual(s.civil_actions, 4)
        self.assertEqual(s.civil_hand_limit, 1)
        self.assertEqual(s.culture, 2 + 1)        # ruins 2 + Alexandria 1

    def test_the_whole_end_of_game_payout_composes(self):
        """One position, three sources at once: an unrevealed Age III event,
        an Age III event already in the future deck, and Bill Gates."""
        st, p = position({"Computers": 2, "Coal": 1}, leader="Bill Gates")
        p.culture = 100
        st.current_events = ["Impact of Industry"]      # mines only: 3
        st.future_events = ["Impact of Progress"]       # Despotism: 0
        effects.invalidate(st)
        game._finish_game(st)
        # 100 + 3 (Industry: the Bill Gates labs are NOT mines) + 0 + 6 (Gates)
        self.assertEqual(st.final_scores[0], 109)


# ================================================= the data-shape guard rails

class HardcodedConstantsMatchTheData(unittest.TestCase):
    """54 of the 200 effect keys in the card data are implemented by NAME
    DISPATCH, not by reading the key (`engine/effects.py` says so at the top).
    That is a deliberate design, and it is also exactly the shape of the two
    bugs this project has already shipped: the value lives in a field no
    reader touches, so the data and the code can drift apart in silence.

    Every constant the engine hardcodes for one of those cards is checked
    against the card here.  If someone corrects the data, this fails.
    """

    def test_tactic_bonuses_agree_with_the_strength_fields(self):
        """`_army_value` reads the top-level `strength` / `obsoleteStrength`;
        the `effects` block spells the same two numbers again."""
        for c in _DB.of_type("tactic"):
            eff = c["effects"]
            self.assertEqual(eff["tacticBonus"], c["strength"], c["name"])
            if "tacticBonusObsolete" in eff:
                self.assertEqual(eff["tacticBonusObsolete"],
                                 c["obsoleteStrength"], c["name"])

    def test_war_spoils_constants(self):
        """`resolve_war` hardcodes 1 token + 1 per 5 advantage, and 5 + the
        advantage in culture."""
        terr = _DB.get("War over Territory")["effects"]["victorTakesYellowTokens"]
        self.assertEqual((terr["base"], terr["perStrengthAdvantage"]), (1, 5))
        cult = _DB.get("War over Culture")["effects"]["victorTakesCulture"]
        self.assertEqual((cult["base"], cult["plus"]), (5, "strengthAdvantage"))
        self.assertEqual(_DB.get("War over Technology")["effects"]
                         ["victorTakesScienceUpTo"], "strengthAdvantage")

    def test_leader_constants_the_engine_spells_out_in_python(self):
        want = {
            "Genghis Khan": ("cultureIfTopTwoStrength", 3),
            "Maximilien Robespierre": ("cultureOnRevolution", 3),
            "Albert Einstein": ("cultureOnTechDevelop", 3),
            "Aristotle": ("scienceOnTechCardTake", 1),
            "Leonardo da Vinci": ("resourceOnTechDevelop", 1),
            "Homer": ("resourceOnMilitaryUnitBuildOrUpgrade", 1),
            "Isaac Newton": ("civilActionBackOnTechDevelop", 1),
            "J. S. Bach": ("theaterTechScienceDiscount", 2),
            "William Shakespeare": ("theaterResourceDiscountIfLibrary", 1),
            "Hammurabi": ("leaderTakeCivilActionDiscount", 1),
            "Alexander the Great": ("removeAsPoliticalActionForYellowToken", 1),
        }
        for name, (key, val) in want.items():
            self.assertEqual(_DB.get(name)["effects"][key], val, name)

    def test_churchills_three_numbers_are_all_3(self):
        eff = _DB.get("Winston Churchill")["effects"]["perTurnChoice"]
        self.assertEqual(eff["cultureOption"], 3)
        self.assertEqual(eff["militaryOption"]["scienceForMilitaryTechs"], 3)
        self.assertEqual(eff["militaryOption"]["resourcesForMilitaryUnits"], 3)

    def test_impact_of_balance_names_the_four_ratings_the_engine_uses(self):
        """`scoring_culture` takes min(food, resources, science, culture);
        the card's own `statistics` list says which four those are."""
        eff = _DB.get("Impact of Balance")["effects"]["allPlayers"]
        self.assertEqual(eff["statistics"],
                         ["foodProduction", "resourceProduction",
                          "scienceProduction", "cultureProduction"])
        self.assertEqual(sorted(eff["ignore"]), ["consumption", "corruption"])


class UnstaffedBuildingsProduceNothing(unittest.TestCase):
    """The generalisation of audit 3.9, across the whole modifier family.

    `sciencePerBestLabOrLibraryLevel` and `bestTheaterDoubleCulture` are the
    same shape -- "your best X produces ..." -- and they disagreed about
    whether an unstaffed X counts.  Two readers of one rule that disagree is
    the bug class this whole file is about, so rather than fixing the one
    case, this pins the rule for EVERY key in `effects._BUILDING_OUTPUT`:
    a technology card with no worker on it is not a building and contributes
    nothing.

    A new modifier key added without a `t.workers` guard fails here.
    """

    #: modifier key -> (leader or wonder carrying it, cards it reads)
    CASES = {
        "bestTheaterDoubleCulture": ("Charlie Chaplin", {"Movies": 0}),
        "culturePerTheater": ("J. S. Bach", {"Movies": 0}),
        "culturePerLabEqualToLevel": ("Sid Meier", {"Computers": 0}),
        "sciencePerLab": ("Sid Meier", {"Computers": 0}),
        "resourcesPerLabEqualToLevel": ("Bill Gates", {"Computers": 0}),
        "sciencePerBestLabOrLibraryLevel": ("Isaac Newton", {"Computers": 0}),
        "culturePerLibraryTheaterPair": ("William Shakespeare",
                                         {"Movies": 0, "Multimedia": 0}),
    }

    def test_every_case_is_a_real_modifier_key(self):
        for key, (holder, _) in self.CASES.items():
            self.assertIn(key, effects._BUILDING_OUTPUT, key)
            self.assertIn(key, _DB.get(holder)["effects"], holder)

    def test_the_table_covers_every_building_output_key(self):
        """A new key must be added here, or this fails."""
        covered = set(self.CASES) | {"doubleBestMine"}   # wonder, below
        self.assertEqual(sorted(set(effects._BUILDING_OUTPUT) - covered), [])

    def test_an_unstaffed_building_earns_no_modifier(self):
        for key, (holder, techs) in self.CASES.items():
            bare_st, bare_p = position(techs, leader=None)
            st, p = position(techs, leader=holder)
            bare = stats(bare_st, bare_p)
            got = stats(st, p)
            for attr in ("culture", "science", "resources"):
                # the leader's own printed flat production is allowed to
                # differ; the MODIFIER is not, and none of these five leaders
                # prints flat culture/science/resources
                self.assertEqual(getattr(got, attr), getattr(bare, attr),
                                 f"{key} via {holder}: {attr}")

    def test_the_railroad_does_not_double_an_unstaffed_mine(self):
        st, p = position({"Oil": 0, "Bronze": 1},
                         wonders=["Transcontinental Railroad"])
        # only Bronze is staffed, so the best STAFFED mine is Bronze (1)
        self.assertEqual(stats(st, p).resources, 1 + 1)

    def test_a_staffed_building_does_earn_it(self):
        """The negative control: the guard above must not be vacuous."""
        st, p = position({"Computers": 1}, leader="Isaac Newton")
        self.assertEqual(stats(st, p).science, 5 + 3)


class EveryFieldHasAReader(unittest.TestCase):
    """The other half of the same guard: no NEW unread key may appear.

    A key that no reader touches is either name-dispatched (and then it is in
    the list below, with the test that covers it) or it is a card the engine
    silently does not implement.  Adding a key without adding a reader is the
    bug that shipped twice; this test makes it fail instead.
    """

    #: key -> why nothing reads it.  Keep this list SHORT and justified.
    NAME_DISPATCHED = {
        # leader abilities, dispatched on `p.leader ==` (effects.py header)
        "civilActionBackOnTechDevelop", "civilActionUpgradeUrbanBuildingToTheater",
        "colonizeDiscardUpTo2MilitaryCardsForBonus", "comboFoodDiscount",
        "comboResourceDiscount", "cultureIfTopTwoStrength",
        "cultureOnRevolution", "cultureOnTechDevelop", "cultureOption",
        "infantryCountsAsCavalryForTactics", "leaderTakeCivilActionDiscount",
        "libraryDiscountsIfTheater", "militaryActionAsCivilPerTurn",
        "militaryActionCombinedPopIncreaseAndUnitBuild", "militaryOption",
        "onReplacePutUnderCompletedWonderHappy", "oncePerGameTwoPoliticalActions",
        "opponentsPayDoubleMilitaryActionsToAttackYou", "peekTopEventCardInPolitics",
        "perTurnChoice", "removeAsPoliticalActionForYellowToken",
        "removeAsPoliticalActionFreeColonize", "resourceOnMilitaryUnitBuildOrUpgrade",
        "resourceOnTechDevelop", "revolutionUsesMilitaryActionsInstead",
        "scienceForMilitaryTechs", "scienceOnTechCardTake",
        "theaterResourceDiscountIfLibrary", "theaterScienceDiscountIfLibrary",
        "theaterTechScienceDiscount", "wonderTakeNoExtraCivilActions",
        # constants hardcoded in the engine, pinned by
        # HardcodedConstantsMatchTheData above
        "base", "plus", "perStrengthAdvantage", "victorTakesCulture",
        "victorTakesScienceUpTo", "victorTakesYellowTokens",
        "ruinsCultureProduction", "tacticBonus", "tacticBonusObsolete",
        "doublesTacticBonusOfOneArmy",
        # prose / documentation of behaviour implemented structurally
        "chosenBy", "duration", "ignore", "statistics", "note", "order",
        "ignoreConsumption", "ignoreCorruption", "requiresAvailableWorker",
        "colonyImmediateBonusApplies", "colonyPermanentBonusTransfers",
        # KNOWN UNIMPLEMENTED (docs/SCORE_AUDIT.md 3.7): the victor of a War
        # over Technology may take blue technologies instead of science.
        "orTakesSpecialTechnologiesOfSameTotalScienceCost",
        # KNOWN UNIMPLEMENTED (docs/SCORE_AUDIT.md 3.2): Bill Gates pays his
        # culture at game end but not when he is removed from play.
        "cultureOnLeaveEqualToLabResourceProduction",
    }

    def all_effect_keys(self):
        keys = {}

        def walk(o, card):
            if isinstance(o, dict):
                for k, v in o.items():
                    keys.setdefault(k, card["name"])
                    walk(v, card)
            elif isinstance(o, list):
                for v in o:
                    walk(v, card)

        for c in _DB.cards:
            walk(c.get("effects") or {}, c)
            walk(c.get("immediateEffects") or {}, c)
            walk(c.get("permanentEffects") or {}, c)
        return keys

    def engine_source(self):
        out = []
        d = os.path.join(_ROOT, "engine")
        for fn in sorted(os.listdir(d)):
            if fn.endswith(".py"):
                with open(os.path.join(d, fn)) as fh:
                    out.append(fh.read())
        return "\n".join(out)

    def test_no_new_unread_effect_key(self):
        blob = self.engine_source()
        unread = {k: c for k, c in self.all_effect_keys().items()
                  if not re.search(r'["\']' + re.escape(k) + r'["\']', blob)}
        new = {k: c for k, c in unread.items() if k not in self.NAME_DISPATCHED}
        self.assertEqual(new, {},
                         "effect keys nothing in engine/ reads -- add a reader "
                         "or justify them in NAME_DISPATCHED")

    def test_the_exemption_list_does_not_rot(self):
        """Every name in NAME_DISPATCHED must still be a key in the data."""
        keys = set(self.all_effect_keys())
        self.assertEqual(sorted(self.NAME_DISPATCHED - keys), [])

    def test_every_government_field_is_read(self):
        """The 2026-07-29 bug: `civilActions` / `militaryActions` /
        `urbanBuildingLimit` / `revolutionCost` / `peacefulCost` all live at
        the TOP LEVEL of a government card, and `techCost` is null.  Each one
        must change something observable."""
        base, _ = position(government="Despotism")
        for name in GOVERNMENTS:
            card = _DB.get(name)
            self.assertIsNone(card["techCost"], name)
            st, p = position(government=name)
            s = stats(st, p)
            self.assertEqual(s.civil_actions, card["civilActions"], name)
            self.assertEqual(s.military_actions, card["militaryActions"], name)
            self.assertEqual(s.urban_limit, card["urbanBuildingLimit"], name)
            if card["peacefulCost"] is not None:
                self.assertEqual(effects.tech_cost(st, p, name),
                                 card["peacefulCost"], name)

    def test_urbanLimitCategory_is_the_card_type(self):
        """Nothing reads `urbanLimitCategory` because the limit is applied per
        card TYPE, and the two agree on every urban card.  If a card ever
        disagrees, the field starts mattering."""
        for c in _DB.cards:
            if "urbanLimitCategory" in c:
                self.assertEqual(c["urbanLimitCategory"], c["type"], c["name"])


if __name__ == "__main__":
    unittest.main()
