"""Regression tests for tools/bgo_rescore.py.

Every case below is a replay bug that first presented as an *engine* bug in
`docs/SCORE_VALIDATION.md`: the reconstructed position was wrong, our scorer
was asked about it, and the disagreement looked like a scoring error.  A
replayer that silently loses a worker does not raise, it moves an Impact-of
residual, which is exactly the failure mode `docs/HUMAN_BASELINE.md` warns
about for this corpus.
"""
import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from tools import bgo_rescore as R                            # noqa: E402


def journal(*rows):
    """Write a temp journal; rows are `text` or `(age, round, text)`."""
    fh = tempfile.NamedTemporaryFile("w", suffix=".tsv", delete=False)
    fh.write("date\tplayer_colour\tage\tround\ttext\n")
    for r in rows:
        age, rnd, text = r if isinstance(r, tuple) else ("I", "3", r)
        fh.write("2026-01-01 00:00:00\t%s\t%s\t%s\t%s\n"
                 % (text.split(" ")[0], age, rnd, text))
    fh.close()
    return fh.name


def replay(*rows, **kw):
    path = journal(*rows)
    try:
        return R.replay(path, **kw)[0]
    finally:
        os.unlink(path)


def tokens_ok(seat):
    return seat.bank + seat.free + sum(seat.techs.values()) == seat.tokens


class WorkerBookkeeping(unittest.TestCase):
    def test_upgrade_moves_the_worker_and_mints_nothing(self):
        """The worker must not pass through the unused pool.

        It did, which minted one yellow token per upgrade -- ~10 a game, enough
        to put every late-game yellow-bank band one step wrong.
        """
        s = replay("Orange increases population Orange spends 2 food",
                   "Orange builds Bronze Orange spends 2 resources",
                   "Orange upgrades Bronze to Iron Orange spends 3 resources"
                   )["Orange"]
        self.assertEqual(s.techs["Bronze"], 2)     # 2 at setup, +1, -1
        self.assertEqual(s.techs["Iron"], 1)
        self.assertEqual(s.free, 1)                # started 1, +1 pop, -1 build
        self.assertTrue(tokens_ok(s))

    def test_upgrade_using_an_action_card_still_parses(self):
        s = replay("Orange upgrades Bronze to Iron using Rich Land "
                   "Orange spends 1 resource")["Orange"]
        self.assertEqual(s.techs["Iron"], 1)
        self.assertTrue(tokens_ok(s))

    def test_bgo_spells_the_age_A_infantry_in_the_singular(self):
        """`builds Warrior`, not `builds Warriors`.

        923 lines in a 150-game sample.  Unresolved, they are silently dropped
        builds, which understates military workers -- invisible to a
        production-rate check because units produce strength, and strength is
        never printed outside a war resolution.
        """
        s = replay("Orange increases population Orange spends 2 food",
                   "Orange builds Warrior Orange spends 2 resources")["Orange"]
        self.assertEqual(s.techs["Warriors"], 2)
        self.assertEqual(s.bad, 0)
        self.assertTrue(tokens_ok(s))


class Leaders(unittest.TestCase):
    def test_election_name_is_not_truncated_by_the_death_clause(self):
        """`elects William Shakespeare Leonardo Da Vinci dies` -> `William`.

        A generic pattern stops at the first word that can be followed by
        `<Name> dies`.  The wrong leader is then either unknown (no effects at
        all) or a different card.
        """
        s = replay("Orange elects Leonardo Da Vinci",
                   "Orange elects William Shakespeare Leonardo Da Vinci dies; "
                   "Orange gets 1 civil action")["Orange"]
        self.assertEqual(s.leader, "William Shakespeare")

    def test_leader_dies_of_antiquation_with_no_journal_line(self):
        """BGO prints nothing when a leader antiquates (only replacements and
        Iconoclasm are logged), so the replayer must apply 9.1 itself."""
        s = replay(("I", "3", "Orange elects Hammurabi"),
                   ("II", "8", "Orange passes Political Phase"))["Orange"]
        self.assertIsNone(s.leader)                # Age I leader, Age II began

    def test_a_leader_survives_the_age_after_its_own(self):
        s = replay(("II", "8", "Orange elects Michelangelo"),
                   ("II", "9", "Orange passes Political Phase"))["Orange"]
        self.assertEqual(s.leader, "Michelangelo")


class YellowBank(unittest.TestCase):
    def test_two_yellow_tokens_are_lost_at_the_end_of_each_age(self):
        """12.2.4.  Measured against the corpus, not assumed: `age_loss=2`
        predicts BGO's printed consumption on 91.6% of 43,847 end-turn lines,
        against 68.7% at 1 and 52.2% at 0."""
        s = replay(("I", "2", "Orange passes Political Phase"),
                   ("II", "8", "Orange passes Political Phase"),
                   ("III", "13", "Orange passes Political Phase"),
                   ("IV", "18", "Orange passes Political Phase"))["Orange"]
        self.assertEqual(s.bank, 18 - 6)
        self.assertEqual(s.tokens, 25 - 6)

    def test_no_yellow_is_lost_when_age_A_ends(self):
        s = replay(("A", "1", "Orange passes Political Phase"),
                   ("I", "2", "Orange passes Political Phase"))["Orange"]
        self.assertEqual(s.bank, 18)

    def test_colonising_returns_units_to_the_bank_and_pays_the_permanent(self):
        """Sacrificed units go to the YELLOW BANK, not the worker pool (11.4),
        and a territory's `+N yellow tokens` is a grant from outside."""
        s = replay("Orange plays event Orange scores 1 culture; Current event:;"
                   " I / Inhabited Territory; Increase population by 1",
                   "Orange colonizes a Inhabited Territory Sacrificed Units:; "
                   "1 Warrior; 1 Colonization card +1; Total force: 3; "
                   "Orange gets 1 population")["Orange"]
        self.assertEqual(s.colonies, [("Inhabited Territory", "I")])
        self.assertEqual(s.techs["Warriors"], 0)
        # 18 - 1 (the colony's population) + 1 (sacrificed unit) + 2 (permanent)
        self.assertEqual(s.bank, 20)
        self.assertEqual(s.tokens, 27)
        self.assertTrue(tokens_ok(s))

    def test_an_event_that_names_nobody_still_moves_every_bank(self):
        """`Each civilization gains 1 population.` has no per-player line in
        the journal; missing it leaves every bank one token high for the rest
        of the game."""
        s = replay("Orange plays event Orange scores 1 culture; Current event:;"
                   " A / Development of Settlement; "
                   "Each civilization gains 1 population.")["Orange"]
        self.assertEqual(s.bank, 17)
        self.assertEqual(s.free, 2)


class Wonders(unittest.TestCase):
    def test_a_nested_engineering_genius_stage_still_counts(self):
        s = replay("Orange builds 1 stage of Colossus Orange spends 3 resources",
                   "Orange plays Engineering Genius Orange builds 1 stage of "
                   "Colossus; Orange spends 1 resource; Wonder completed"
                   )["Orange"]
        self.assertEqual(s.completed, ["Colossus"])

    def test_ravages_of_time_flips_the_wonder_it_names(self):
        s = replay("Orange builds 2 stages of Colossus Orange spends 6 resources"
                   "; ; Wonder completed",
                   "Purple plays event Purple scores 3 culture; Current event:; "
                   "II / Ravages of Time; ...; The Colossus crumbles")["Orange"]
        self.assertEqual(s.flipped, ["Colossus"])


if __name__ == "__main__":
    unittest.main()
