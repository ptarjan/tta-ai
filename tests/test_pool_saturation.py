"""The pool's automatic saturation pruning and the newest-biased self-ladder.

The property these tests defend is not "the numbers are these numbers", it is
the two things the rule must never do:

  * it must never let the pool become pure self-play (docs/HAZARDS.md trap
    2/3: the repo has been burned once by a monoculture and once by
    self-imitation), so a fixed EXTERNAL opponent is in every generation's
    subset even when the champion beats every one of them 100%; and
  * it must be a no-op on a pool with no measurements, so a fresh state dir
    behaves exactly as it did before saturation existed.
"""
import os
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from experiments import hillclimb_pool as P  # noqa: E402


def pool(win_rates=None):
    entries = [P.PoolEntry("mirror", P.MIRROR, "mirror"),
               P.PoolEntry("book", "book", "book"),
               P.PoolEntry("book2", "book2", "book")]
    entries += [P.PoolEntry(f"var:{i}", f"v{i}", "variant") for i in range(4)]
    entries += [P.PoolEntry(f"past:{i}", {"a": i}, "past") for i in range(4)]
    entries += [P.PoolEntry(f"hall:{i}", {"b": i}, "hall") for i in range(2)]
    return P.Pool(entries, metric="blend", win_rates=win_rates)


class Multiplier(unittest.TestCase):
    def test_unmeasured_is_full_weight(self):
        self.assertEqual(P.saturation_multiplier(None), 1.0)

    def test_below_lo_is_full_weight_and_above_hi_is_the_floor(self):
        self.assertEqual(P.saturation_multiplier(0.50), 1.0)
        self.assertEqual(P.saturation_multiplier(P.SAT_LO), 1.0)
        self.assertEqual(P.saturation_multiplier(P.SAT_HI), P.SAT_FLOOR)
        self.assertEqual(P.saturation_multiplier(1.0), P.SAT_FLOOR)

    def test_monotone_between(self):
        prev = 1.1
        for wr in (0.70, 0.75, 0.80, 0.85, 0.90, 0.95):
            m = P.saturation_multiplier(wr)
            self.assertLess(m, prev)
            prev = m

    def test_the_floor_is_not_zero_so_a_gate_can_still_veto(self):
        """`_aggregate` skips weight<=0 rows, so a 0 floor would mute the veto."""
        self.assertGreater(P.SAT_FLOOR, 0.0)


class Reweighting(unittest.TestCase):
    def test_no_measurements_reproduces_the_even_split(self):
        even, sat = pool(), pool({})
        for a, b in zip(even.sorted_entries(), sat.sorted_entries()):
            self.assertEqual(a.label, b.label)
            self.assertAlmostEqual(a.weight, b.weight, places=12)
        # and it is literally tier total / member count
        self.assertAlmostEqual(even.by_label("book").weight,
                               P.DEFAULT_TIER_WEIGHTS["book"] / 2, places=12)

    def test_tier_totals_are_preserved(self):
        p = pool({"book": 1.0, "var:0": 0.99, "var:1": 0.5,
                  "past:0": 0.98, "hall:0": 0.55})
        got = {}
        for e in p.entries:
            got[e.tier] = got.get(e.tier, 0.0) + e.weight
        for tier, total in got.items():
            self.assertAlmostEqual(total, P.DEFAULT_TIER_WEIGHTS[tier],
                                   places=9, msg=tier)

    def test_weight_moves_from_the_saturated_to_the_informative(self):
        p = pool({"book": 1.00, "book2": 0.50})
        self.assertLess(p.by_label("book").weight,
                        p.by_label("book2").weight)
        # ...and it went to the same TIER, not out of it
        self.assertAlmostEqual(p.by_label("book").weight
                               + p.by_label("book2").weight,
                               P.DEFAULT_TIER_WEIGHTS["book"], places=9)

    def test_the_external_share_cannot_be_eroded_by_saturation(self):
        """The anchor property: beating every external opponent 100% must not
        hand the whole pool to self-play."""
        def external_share(p):
            tot = sum(e.weight for e in p.entries)
            ext = sum(e.weight for e in p.entries
                      if e.tier in ("book", "human", "variant", "quiescent"))
            return ext / tot
        base = external_share(pool())
        crushed = external_share(pool({e.label: 1.0 for e in pool().entries
                                       if e.tier in ("book", "variant")}))
        self.assertAlmostEqual(base, crushed, places=9)


class Rotation(unittest.TestCase):
    def test_an_external_opponent_is_in_every_subset_even_when_all_saturated(self):
        wr = {e.label: 1.0 for e in pool().entries if not e.is_mirror}
        p = pool(wr)
        self.assertTrue(all(e.inert for e in p.entries if not e.is_mirror))
        for gen in range(30):
            sub = p.acceptance_subset(gen, 4)
            self.assertEqual(len(sub), 4, gen)
            self.assertTrue(set(e.tier for e in sub) & set(p.gate_tiers), gen)
            self.assertTrue(set(e.tier for e in sub) & set(p.ladder_tiers), gen)

    def test_the_free_slots_prefer_live_opponents(self):
        wr = {"var:0": 1.0, "var:1": 1.0, "var:2": 1.0, "book2": 1.0,
              "past:0": 1.0, "past:1": 1.0}
        p = pool(wr)
        seen = set()
        for gen in range(40):
            seen.update(e.label for e in p.acceptance_subset(gen, 4))
        # `past:0`/`past:1` are saturated and there are live ladder members,
        # so the ladder rotation never has to fall back to them.
        self.assertNotIn("past:0", seen)
        self.assertNotIn("past:1", seen)
        self.assertIn("past:2", seen)
        self.assertIn("var:3", seen)

    def test_a_saturated_gate_still_gets_rotated_when_it_is_the_only_one(self):
        p = pool({"book": 1.0, "book2": 1.0, "var:0": 1.0, "var:1": 1.0,
                  "var:2": 1.0, "var:3": 1.0})
        gates = set()
        for gen in range(20):
            gates.update(e.label for e in p.acceptance_subset(gen, 2)
                         if e.tier in p.gate_tiers)
        self.assertTrue(gates)


class RecentSpread(unittest.TestCase):
    def items(self, n):
        return [f"gen{i:05d}" for i in range(n)]

    def test_exactly_k_and_keeps_both_ends(self):
        for n in (7, 20, 105, 400):
            for k in (2, 3, 6, 7):
                got = P._recent_spread(self.items(n), k)
                self.assertEqual(len(got), k, (n, k))
                self.assertEqual(got[0], "gen00000", (n, k))
                self.assertEqual(got[-1], f"gen{n - 1:05d}", (n, k))

    def test_short_ladders_are_returned_whole(self):
        self.assertEqual(P._recent_spread(self.items(3), 6), self.items(3))
        self.assertEqual(P._recent_spread([], 6), [])
        self.assertEqual(P._recent_spread(self.items(9), 0), [])

    def test_it_is_biased_to_the_recent_where_even_spread_is_not(self):
        n, k = 105, 6
        recent = P._recent_spread(self.items(n), k)
        even = P._spread(self.items(n), k)
        half = f"gen{n // 2:05d}"
        self.assertGreater(sum(1 for x in recent if x > half),
                           sum(1 for x in even if x > half))
        # the four newest picks are all within the last quarter of the archive
        self.assertGreaterEqual(sum(1 for x in recent
                                    if x > f"gen{int(n * 0.75):05d}"), 3)


if __name__ == "__main__":
    unittest.main()
