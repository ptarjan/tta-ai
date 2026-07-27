"""Regression tests for tools/bgo_parse.py.

Each of these is a journal template that was *wrong* on the first pass and
silently produced a plausible number, which is the failure mode this corpus is
most exposed to -- a regex that stops matching does not raise, it just shifts a
median.  The synthetic journals below are the exact shapes taken from the real
corpus (`sources/bgo/journals.tar.gz`), reduced to the smallest case.
"""
import os
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from tools import bgo_parse as B                             # noqa: E402


def rows(*texts):
    """(date, colour, age, round, text) tuples; colour/age/round are dummies
    except where a test needs them."""
    out = []
    for t in texts:
        if isinstance(t, tuple):
            age, rnd, text = t
        else:
            age, rnd, text = "I", "3", t
        out.append(("2026-01-01 00:00:00", text.split(" ")[0], age, rnd, text))
    return out


def one(rs, colour="Orange"):
    got = B.parse_game("g1", rs, {"players": 2})
    for r in got:
        if r["colour"] == colour:
            return r
    raise AssertionError("no row for %s in %r" % (colour, [g["colour"] for g in got]))


class TakeBacks(unittest.TestCase):
    def test_putback_cancels_the_take(self):
        r = one(rows(
            "Orange takes Rich Land in hand Orange uses 3 civil action",
            "Orange puts Rich Land back in the row Orange gets 3 civil action",
            "Orange takes Rich Land in hand Orange uses 1 civil action",
        ))
        # one kept take at tier 1, not two takes and not a tier-3 take
        self.assertEqual(r["takes"], 1)
        self.assertEqual(r["tier3"], 0)
        self.assertEqual(r["tier1"], 1)
        self.assertEqual(r["takebacks"], 1)


class WonderSurcharge(unittest.TestCase):
    def test_completed_wonders_are_subtracted_from_the_take_cost(self):
        # After completing a wonder the next wonder take costs +1, so
        # "uses 3 civil action" is really row tier 2.  Completion comes from
        # BGO's own "Wonder completed" marker, not from counting stages.
        r = one(rows(
            "Orange builds 2 stages of Colossus Orange spends 6 resources; ; "
            "Wonder completed",
            "Orange takes Great Wall in hand Orange uses 3 civil action",
        ))
        self.assertEqual(r["wonders_completed"], 1)
        self.assertEqual(r["tier2"], 1)
        self.assertEqual(r["tier3"], 0)
        # the raw logged cost is still reported unchanged
        self.assertEqual(r["take_ca3"], 1)

    def test_no_surcharge_before_any_wonder_completes(self):
        r = one(rows("Orange takes Great Wall in hand Orange uses 3 civil action"))
        self.assertEqual(r["tier3"], 1)


class WonderStagesNestedInAnActionCard(unittest.TestCase):
    def test_engineering_genius_free_stage_is_counted(self):
        # 2809 of the corpus's 18307 stage lines look like this.  Anchoring
        # the stage regex at the start of the line loses all of them.
        r = one(rows(
            "Orange plays Engineering Genius Orange builds 1 stage of "
            "Library of Alexandria; Orange spends 2 resources",
            "Orange plays Engineering Genius Orange builds 1 stage of "
            "First Space Flight; Orange spends 4 resources; ; "
            "Wonder completed; Orange scores 3 culture",
        ))
        self.assertEqual(r["wonder_stages"], 2)
        self.assertEqual(r["wonders_started"], 2)
        self.assertEqual(r["wonders_completed"], 1)

    def test_a_stage_with_no_spend_clause_still_parses(self):
        r = one(rows("Orange builds 2 stages of Pyramids"))
        self.assertEqual(r["wonder_stages"], 2)
        self.assertEqual(r["wonders_started"], 1)


class HammurabiMilitaryAction(unittest.TestCase):
    def test_military_action_counts_toward_the_take_cost(self):
        # Hammurabi may pay one civil action's worth with a military action;
        # BGO logs the two clauses separately and the tier is their sum.
        r = one(rows(
            "Orange takes Urban Growth in hand Orange uses 2 civil action; "
            "Orange uses 1 military action",
        ))
        self.assertEqual(r["tier3"], 1)
        self.assertEqual(r["tier2"], 0)


class Wars(unittest.TestCase):
    def test_declaration_and_resolution_are_paired(self):
        rs = rows(
            "Orange declares War over Culture on Purple The victor takes 5 "
            "culture + 1 culture for each point of strength advantage from the "
            "defeated civilization. ; Orange uses 2 military action",
            "Orange wins War over Culture Attacker's strength: 12; "
            "Defender's strength: 7",
        )
        r = one(rs)
        d = one(rs, "Purple")
        self.assertEqual(r["wars_declared"], 1)
        self.assertEqual(r["wars_declared_won"], 1)
        self.assertEqual(r["wars_declared_lost"], 0)
        self.assertEqual(d["wars_defended"], 1)
        self.assertEqual(d["wars_defended_won"], 0)
        self.assertEqual(r["war_str_att_mean"], 12)

    def test_defender_win_is_credited_to_the_defender(self):
        rs = rows(
            "Orange declares War over Territory on Purple The victor takes 1 "
            "yellow token plus 1 yellow token for every 5 points of strength "
            "advantage from the defeated civilization's yellow bank. ; "
            "Orange uses 2 military action",
            "Purple wins War over Territory Attacker's strength: 4; "
            "Defender's strength: 9",
        )
        self.assertEqual(one(rs)["wars_declared_won"], 0)
        self.assertEqual(one(rs)["wars_declared_lost"], 1)
        self.assertEqual(one(rs, "Purple")["wars_defended_won"], 1)

    def test_an_aggression_is_not_a_war(self):
        r = one(rows(
            "Orange plays Plunder against Purple Your rival loses a total of "
            "up to 6 resources or food."))
        self.assertEqual(r["aggressions"], 1)
        self.assertEqual(r["wars_declared"], 0)


class Government(unittest.TestCase):
    def test_revolution_and_discovery_both_count(self):
        r = one(rows(
            ("II", "9", "Orange discovers Monarchy Orange loses 8 science"),
            ("III", "13", "Orange revolutions Change government to Democracy; "
                          "9 science points spent; Orange loses 9 science"),
        ))
        self.assertEqual(r["gov_changes"], 2)
        self.assertEqual(r["first_gov"], "Monarchy")
        self.assertEqual(r["first_gov_round"], 9)
        self.assertEqual(r["gov_path"], "Monarchy>Democracy")

    def test_a_plain_tech_discovery_is_not_a_government(self):
        r = one(rows("Orange discovers Bronze Orange loses 2 science"))
        self.assertEqual(r["gov_changes"], 0)


class Leaders(unittest.TestCase):
    def test_election_line_with_a_death_clause(self):
        r = one(rows(
            "Orange elects Leonardo Da Vinci Hammurabi dies; "
            "Orange gets 1 civil action"))
        self.assertEqual(r["leaders_elected"], 1)

    def test_bgo_leader_spellings_resolve(self):
        for bgo, ours in (("Charles Chaplin", "Charlie Chaplin"),
                          ("Maximillien Robespierre", "Maximilien Robespierre"),
                          ("Johannes Sebastian Bach", "J. S. Bach")):
            self.assertIn(ours, B.CARDS, ours)
            self.assertEqual(B.norm(bgo), ours)
            r = one(rows("Orange elects %s" % bgo))
            self.assertEqual(r["leaders_elected"], 1, bgo)


class PerAgeTakes(unittest.TestCase):
    def test_takes_are_bucketed_by_the_games_age_not_the_cards(self):
        # "Urban Growth" exists in the A/I/II/III decks, so only the journal's
        # own age column can say when it was taken.
        r = one(rows(
            ("A", "1", "Orange takes Urban Growth in hand Orange uses 1 civil action"),
            ("III", "15", "Orange takes Urban Growth in hand Orange uses 1 civil action"),
        ))
        self.assertEqual(r["take_ageA"], 1)
        self.assertEqual(r["take_ageIII"], 1)
        self.assertEqual(r["take_unknown_card"], 0)


class Scores(unittest.TestCase):
    def test_final_scores_and_margin_come_off_the_end_of_game_line(self):
        rs = rows(
            "End turn Orange scores:; ; 3 culture (now 91); 2 science (now 14); "
            "1 food - consumption: 3 (now 2); 3 resources (now 3)",
            "End of game Check the journal to get the final impacts effects :; "
            "Impact of Balance; ; WINNER IS ANDREW HYER AS ORANGE (195 PTS); "
            "2nd is PLAYER as Purple (160 pts)",
        )
        r = one(rs)
        self.assertEqual(r["score"], 195)
        self.assertEqual(r["rank"], 1)
        self.assertEqual(r["margin_vs_next"], 35)
        self.assertEqual(r["won"], 1)
        self.assertEqual(r["sci_final"], 14)
        self.assertEqual(one(rs, "Purple")["score"], 160)


if __name__ == "__main__":
    unittest.main()
