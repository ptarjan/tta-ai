"""A mutation that changes no decision must not be paid for four times.

Some mutation operators rescale a GROUP of weights.  When every weight in
that group is already 0.0 -- or the group's coordinates are never consulted on
any decision path -- the mutant is behaviourally the champion: it replays the
champion's games bit for bit and every paired diff is exactly 0.0.

The sequential evaluator's two early exits do not fire on that candidate.  It
never clears the accept bound (`lo` is 0.0, not positive) and it is never
"already losing" (`m` is 0.0, not negative), so it ran the FULL block ladder
before rejecting with edge=+0.0000.  3p gen 46 spent 11.9 hours doing that to
two candidates in a row.

The invariant here is not "stop early" for its own sake.  It is that stopping
early changes the BILL and nothing else: the verdict an inert candidate gets
must be identical to the verdict it got before, or the optimisation has
quietly become a policy change.
"""

import random
import unittest

import experiments.hillclimb_league as L
import experiments.hillclimb_pool as P

from experiments.hillclimb_league import replays_champion_exactly
from experiments.hillclimb import GROUP_KEYS, GROUP_NAMES, mutate


class TestTheInertPredicate(unittest.TestCase):

    def test_all_zero_diffs_is_an_exact_replay(self):
        per = {"book": {"diffs": [0.0] * 12}, "hall:x": {"diffs": [0.0] * 12}}
        self.assertTrue(replays_champion_exactly(per))

    def test_no_games_yet_is_not_an_exact_replay(self):
        """An unplayed candidate has nothing to be identical to."""
        per = {"book": {"diffs": []}, "hall:x": {"diffs": []}}
        self.assertFalse(replays_champion_exactly(per))

    def test_one_nonzero_diff_anywhere_disqualifies_it(self):
        """NEGATIVE CONTROL: the predicate must not fire on a near-tie.

        A candidate that draws on average is a real candidate and is owed the
        full ladder.  Only an EXACT replay may be cut short.
        """
        for bad in (1e-12, -1e-12, 0.5, -30.0):
            per = {"book": {"diffs": [0.0] * 11 + [bad]},
                   "hall:x": {"diffs": [0.0] * 12}}
            self.assertFalse(replays_champion_exactly(per), f"diff={bad}")

    def test_a_candidate_that_cancels_out_is_not_an_exact_replay(self):
        """Mean zero is not the same fact as every game zero."""
        per = {"book": {"diffs": [+5.0, -5.0] * 6}}
        self.assertFalse(replays_champion_exactly(per))


class TestTheVerdictIsUnchanged(unittest.TestCase):
    """Drive the real evaluator with an arena where the candidate and the
    champion post the SAME number in every game."""

    def score(self, cand_lead, champ_lead, max_blocks=4):
        champ = {"culture": 1.0}

        def duel(a, b, players, games, seed0=0, workers=None, **kw):
            lead = cand_lead if a is not champ else champ_lead
            return {"per_game": [0.5] * games,
                    "per_game_lead": [lead] * games,
                    "per_game_margin": [lead] * games,
                    "per_game_culture": [100.0 + lead] * games}

        entries = [P.PoolEntry("book", "book", "book", 1.0, "blend"),
                   P.PoolEntry("hall:x", {"h": 1}, "hall", 1.0, "blend")]
        real = L.arena.duel
        L.arena.duel = duel
        try:
            ref = L.RefCache(champ, 2, 1, 12, 5)
            return L.score_candidate({"culture": 2.0}, entries, ref, 1.2816,
                                     1, max_blocks, 1.0, ("book",))
        finally:
            L.arena.duel = real

    def test_an_exact_replay_stops_after_the_screening_block(self):
        m, se, lo, per, games, veto, inert = self.score(10.0, 10.0)
        self.assertTrue(inert)
        # One block of each of the two opponents, not four.
        full = self.score(10.0, 10.0, max_blocks=1)[4]
        self.assertEqual(games, full)

    def test_stopping_early_does_not_change_the_verdict(self):
        """The whole safety argument, asserted rather than argued.

        Compare the early-stopped run against `max_blocks=1`, which is the
        same first block with no opportunity to stop early.  Same edge, same
        bound, same veto -- so the candidate is rejected either way.
        """
        m, se, lo, per, games, veto, inert = self.score(10.0, 10.0)
        bm, bse, blo, bper, bgames, bveto, _ = self.score(10.0, 10.0,
                                                          max_blocks=1)
        self.assertEqual((m, se, lo, veto), (bm, bse, blo, bveto))
        self.assertEqual(m, 0.0)
        self.assertEqual(lo, 0.0)
        self.assertFalse(lo > 0.0, "an exact replay must never be accepted")
        self.assertEqual(veto, [], "an exact replay must never be vetoed")

    def test_a_real_candidate_still_gets_the_full_ladder(self):
        """NEGATIVE CONTROL at the loop level.

        A candidate that is genuinely behind by a hair must NOT be short-cut
        by the inert path -- it exits by the ordinary `m < 0` reject instead,
        and `inert` stays false.
        """
        m, se, lo, per, games, veto, inert = self.score(9.0, 10.0)
        self.assertFalse(inert)
        self.assertLess(m, 0.0)

    def test_a_winning_candidate_is_not_called_inert(self):
        m, se, lo, per, games, veto, inert = self.score(40.0, 10.0)
        self.assertFalse(inert)
        self.assertGreater(lo, 0.0)


class TestRescaleNeverPicksADeadGroup(unittest.TestCase):
    """`rescale` multiplies, so it cannot lift a coordinate off 0.0.

    Choosing an all-zero group produces a mutant identical to the champion --
    a whole candidate slot, and up to a full evaluation, spent on nothing.
    """

    def test_a_dead_group_is_never_chosen_while_a_live_one_exists(self):
        w = {k: 0.0 for g in GROUP_NAMES for k in GROUP_KEYS[g]}
        live = "happiness"
        for k in GROUP_KEYS[live]:
            w[k] = 1.0

        for seed in range(60):
            rng = random.Random(seed)
            out, moved, op = mutate(w, rng, 0.25, op="rescale")
            self.assertEqual(op, f"rescale:{live}",
                             "the only group with non-zero weights is the "
                             "only one rescale can actually move")
            self.assertNotEqual(out, w, "a rescale must produce a real mutant")

    def test_an_all_zero_vector_falls_back_to_an_operator_that_adds(self):
        """NEGATIVE CONTROL: with nothing to scale, rescale must not silently
        return the champion -- it must hand off to `scatter`, which adds."""
        w = {k: 0.0 for g in GROUP_NAMES for k in GROUP_KEYS[g]}
        for seed in range(20):
            rng = random.Random(seed)
            out, moved, op = mutate(w, rng, 0.25, op="rescale")
            self.assertEqual(op, "scatter")
            self.assertNotEqual(out, w, "the fallback must move something")


if __name__ == "__main__":
    unittest.main()
