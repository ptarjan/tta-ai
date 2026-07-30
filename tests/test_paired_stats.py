"""Guards on the paired-design estimators in experiments/paired_stats.py.

The load-bearing test here is `test_deterministic_mirror_control_has_zero_ci`.
The arena deals every duel as seat-swapped pairs, so a control arm playing
itself wins exactly one game of every pair and the true variance of the win
rate is *exactly zero*.  The independent-samples formula reports +/-3.5pp on
that data.  Any future edit that puts `sqrt(p(1-p)/n_games)` (or
`sqrt(var/n_games)`) back on a paired design will fail that test, loudly, with
a number instead of an opinion.
"""
from __future__ import annotations

import json
import math
import os
import random
import unittest

from experiments import paired_stats as PS

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CTRL = os.path.join(ROOT, "exp_quiesce", "ab.jsonl")


def mirror(k):
    """k deals, each split 1-1 by the seat swap.  True variance zero."""
    out = []
    for i in range(k):
        out.extend([1.0, 0.0] if i % 2 else [0.0, 1.0])
    return out


class TestUnitOfAnalysis(unittest.TestCase):

    def test_deal_means_groups_consecutive_seats(self):
        # arena task order: g -> (seed0 + g//P, g%P)
        pg = [1.0, 0.0, 0.0, 0.0, 1.0, 1.0]
        self.assertEqual(PS.deal_means(pg, 2), [0.5, 0.0, 1.0])
        self.assertEqual(PS.deal_means([1.0, 0.0, 0.5] * 2, 3), [0.5, 0.5])

    def test_incomplete_deal_is_dropped_whole(self):
        # Half a mirrored pair is precisely the seat-biased observation the
        # pairing exists to cancel, so it must not be half-counted.
        pg = [1.0, 0.0, 1.0, None, 0.0, 1.0]
        self.assertEqual(PS.deal_means(pg, 2), [0.5, 0.5])

    def test_ragged_tail_ignored(self):
        self.assertEqual(PS.deal_means([1.0, 0.0, 1.0], 2), [0.5])


class TestTheDefect(unittest.TestCase):
    """These are the regression guards.  Do not weaken them."""

    def test_deterministic_mirror_control_has_zero_ci(self):
        pg = mirror(400)                      # n=800 games, 400 deals
        est = PS.paired(pg, 2)
        self.assertAlmostEqual(est.mean, 0.5, places=12)
        # The truth: every deal contributes exactly 0.5, so there is no
        # variance to report.
        self.assertLess(est.half, 1e-12,
                        "paired estimator must report zero width on a "
                        "deterministic mirror control; a non-zero width here "
                        "means the independent-samples formula is back")
        # ...and the naive formula is wrong by 3.5 percentage points.
        self.assertGreater(est.naive_half, 0.03)
        self.assertAlmostEqual(est.rho, -1.0, places=9)

    def test_real_committed_control_arm(self):
        """Same thing, on data actually on disk rather than constructed."""
        if not os.path.exists(CTRL):
            self.skipTest("exp_quiesce/ab.jsonl not present")
        row = None
        with open(CTRL) as f:
            for line in f:
                d = json.loads(line)
                if d.get("label") == "ctrl_2p":
                    row = d
                    break
        if row is None:
            self.skipTest("no ctrl_2p row")
        est = PS.paired(row["per_game"], row["players"])
        self.assertLess(est.half, 1e-12)
        # The number this project published for that arm:
        self.assertAlmostEqual(row["ci"], 0.0345831, places=5)
        self.assertGreater(est.naive_half, 0.03)

    def test_negative_rho_tightens_and_positive_rho_widens(self):
        """The correction is NOT a blanket sqrt(2).  Its sign depends on rho."""
        rng = random.Random(7)
        k = 4000

        # rho = +1: the deal picks the winner, seat is irrelevant.
        pos = []
        for _ in range(k):
            w = 1.0 if rng.random() < 0.5 else 0.0
            pos.extend([w, w])
        e_pos = PS.paired(pos, 2)
        self.assertGreater(e_pos.rho, 0.9)
        # sqrt(2) wider, the worst case the naive formula can be optimistic by
        self.assertAlmostEqual(e_pos.half / e_pos.naive_half, math.sqrt(2),
                               delta=0.02)

        # rho = -1: the seat picks the winner, deal is irrelevant.
        e_neg = PS.paired(mirror(k), 2)
        self.assertLess(e_neg.half, e_neg.naive_half)

    def test_deff_matches_one_plus_rho(self):
        rng = random.Random(11)
        pg = []
        for _ in range(6000):
            base = rng.random() < 0.55
            a = base if rng.random() < 0.8 else not base
            b = base if rng.random() < 0.8 else not base
            pg.extend([float(a), float(b)])
        est = PS.paired(pg, 2)
        self.assertAlmostEqual(est.deff, 1 + est.rho, delta=0.02)


class TestSmallClusterCounts(unittest.TestCase):

    def test_t_correction_is_applied(self):
        est = PS.cluster_ci([0.325, 0.300, 0.3875, 0.5625, 0.425, 0.5875])
        self.assertEqual(est.n_clusters, 6)
        self.assertAlmostEqual(est.crit, 2.571, places=3)
        # z=1.96 would understate this interval by 31%.
        self.assertGreater(est.half, 1.96 * est.se)

    def test_single_cluster_cannot_bound_itself(self):
        est = PS.cluster_ci([0.5])
        self.assertEqual(est.half, float("inf"))

    def test_t_crit_converges_to_z(self):
        # true t_{1000,.975} = 1.96234; the expansion must hit it, and must
        # stay ABOVE z for every finite df.
        self.assertAlmostEqual(PS.t_crit(1000), 1.96234, delta=5e-4)
        self.assertAlmostEqual(PS.t_crit(100), 1.98397, delta=5e-4)
        self.assertGreater(PS.t_crit(31), PS.Z95)
        self.assertGreater(PS.t_crit(10 ** 6), PS.Z95)

    def test_t_crit_is_continuous_across_the_table_edge(self):
        # df=30 comes from the table, df=31 from the expansion; a step here
        # would mean one of the two is wrong.
        self.assertAlmostEqual(PS.t_crit(30), 2.042, places=3)
        self.assertLess(PS.t_crit(31), PS.t_crit(30))
        self.assertAlmostEqual(PS.t_crit(31), 2.0395, delta=2e-3)


class TestPooling(unittest.TestCase):

    def test_homogeneous_blocks_do_not_escalate(self):
        rng = random.Random(3)
        blocks = []
        for _ in range(8):
            pg = []
            for _ in range(200):
                a = 1.0 if rng.random() < 0.6 else 0.0
                b = 1.0 if rng.random() < 0.6 else 0.0
                pg.extend([a, b])
            blocks.append(pg)
        est = PS.pooled(blocks, 2)
        self.assertFalse(est.escalated)
        self.assertEqual(est.unit, "deal")

    def test_overdispersed_blocks_escalate_to_block_clustering(self):
        rng = random.Random(5)
        blocks = []
        # Each block has its own true rate: exactly the failure the anchor hit.
        for p in (0.35, 0.62, 0.40, 0.70, 0.38, 0.66):
            pg = []
            for _ in range(200):
                a = 1.0 if rng.random() < p else 0.0
                b = 1.0 if rng.random() < p else 0.0
                pg.extend([a, b])
            blocks.append(pg)
        est = PS.pooled(blocks, 2)
        self.assertTrue(est.escalated)
        self.assertEqual(est.unit, "block")
        self.assertEqual(est.het_df, 5)
        self.assertGreater(est.half, est.naive_half)

    def test_anchor_six_shards_reproduce_the_reported_defect(self):
        """The neural loop's anchor, from loop2/anchor_seed_{0..5}.log."""
        shards = [0.3250, 0.3000, 0.3875, 0.5625, 0.4250, 0.5875]
        n_per = 40
        m = sum(shards) / len(shards)
        self.assertAlmostEqual(m, 0.43125, places=5)
        # what pool_summary published
        naive = PS.Z95 * math.sqrt(m * (1 - m) / (n_per * len(shards)))
        self.assertAlmostEqual(naive, 0.0627, places=4)
        # between-shard over-dispersion
        chi2 = sum((s - m) ** 2 for s in shards) / (m * (1 - m) / n_per)
        self.assertAlmostEqual(chi2, 11.76, places=1)
        self.assertGreater(chi2, PS._chi2_crit(5))
        est = PS.cluster_ci(shards, unit="block", n_games=240)
        self.assertAlmostEqual(est.half, 0.126, places=3)
        # The published interval was optimistic by very nearly 2x.
        self.assertGreater(est.half / naive, 1.9)


class TestBootstrapAgrees(unittest.TestCase):

    def test_block_bootstrap_matches_closed_form(self):
        rng = random.Random(17)
        pg = []
        for _ in range(1500):
            a = 1.0 if rng.random() < 0.58 else 0.0
            b = 1.0 if rng.random() < 0.42 else 0.0
            pg.extend([a, b])
        est = PS.paired(pg, 2)
        lo, hi = PS.block_bootstrap(pg, 2, reps=4000, seed=1)
        self.assertAlmostEqual((hi - lo) / 2, est.half, delta=0.004)


class TestEstimateArithmetic(unittest.TestCase):

    def test_z_and_p(self):
        est = PS.cluster_ci([0.6] * 5 + [0.5] * 5)
        self.assertGreater(est.z_against(0.5), 0)
        self.assertLess(est.p_against(0.5), 0.05)

    def test_fmt_names_its_unit(self):
        est = PS.paired(mirror(50), 2)
        self.assertIn("deal-clustered", est.fmt())
        self.assertIn("K=50", est.fmt())


if __name__ == "__main__":
    unittest.main()
