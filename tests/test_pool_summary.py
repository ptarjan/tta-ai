"""Pooling the fanned-out gate shards.

A beam-vs-beam gate is ~18 cpu-s per game, so the loop runs it as N parallel
processes over disjoint seed ranges and pools the SUMMARY lines.  The promotion
decision is `win - ci > 0.5`, so a pooling bug is a silent promotion bug: it
would either promote regressions or freeze the loop.  Torch-free, so it runs in
tools/gate.sh on the Mac.
"""
import math
import os
import re
import subprocess
import sys
import tempfile
import unittest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SCRIPT = os.path.join(ROOT, "experiments", "pool_summary.py")


def _shard(dirname, name, win, n, neural=100.0, opp=90.0, margin=10.0,
           ci=0.1, errs=0, trailing="\n"):
    p = os.path.join(dirname, name)
    with open(p, "w") as f:
        f.write("loaded x on cpu\n  20/40 win 0.5\n")
        f.write(f"SUMMARY win={win:.4f} ci={ci:.4f} neural={neural:.1f} "
                f"opp={opp:.1f} margin={margin:.1f} n={n} errs={errs}"
                + trailing)
    return p


def _pool(paths):
    out = subprocess.run([sys.executable, SCRIPT] + paths,
                         capture_output=True, text=True, cwd=ROOT)
    assert out.returncode == 0, out.stderr
    line = out.stdout.strip().splitlines()[-1]
    return dict(kv.split("=", 1) for kv in line.split()[1:])


class TestPoolSummary(unittest.TestCase):

    def test_equal_shards_average(self):
        with tempfile.TemporaryDirectory() as d:
            ps = [_shard(d, "a.log", 0.6, 50), _shard(d, "b.log", 0.4, 50)]
            r = _pool(ps)
            self.assertAlmostEqual(float(r["win"]), 0.5, places=4)
            self.assertEqual(int(r["n"]), 100)
            self.assertEqual(int(r["shards"]), 2)

    def test_weights_by_n(self):
        """A short shard must not count as much as a long one."""
        with tempfile.TemporaryDirectory() as d:
            ps = [_shard(d, "a.log", 1.0, 10), _shard(d, "b.log", 0.0, 90)]
            r = _pool(ps)
            self.assertAlmostEqual(float(r["win"]), 0.1, places=4)

    def test_ci_shrinks_with_pooled_n(self):
        """The pooled CI must reflect the POOLED n, not a shard's n -- the
        promotion rule is `win - ci > 0.5`, so a CI that stayed at the shard
        width would make promotion impossible."""
        with tempfile.TemporaryDirectory() as d:
            small = _pool([_shard(d, "a.log", 0.6, 25, ci=0.2)])
            big = _pool([_shard(d, f"b{i}.log", 0.6, 25, ci=0.2)
                         for i in range(16)])
            self.assertLess(float(big["ci"]), float(small["ci"]) / 3)
            self.assertEqual(int(big["n"]), 400)

    def test_ignores_missing_and_empty_files(self):
        with tempfile.TemporaryDirectory() as d:
            good = _shard(d, "a.log", 0.7, 40)
            empty = os.path.join(d, "empty.log")
            with open(empty, "w") as f:
                f.write("loaded x\nno summary here\n")
            r = _pool([good, empty, os.path.join(d, "nope.log")])
            self.assertAlmostEqual(float(r["win"]), 0.7, places=4)
            self.assertEqual(int(r["shards"]), 1)

    def test_no_shards_is_not_a_score_at_all(self):
        """Every worker dying (a guard kill) must not yield a parseable score.

        This test used to require `win=0.0000 ci=1.0000`, on the reasoning that
        `win - ci > 0.5` is then false and nothing gets promoted on no
        evidence.  That much was true, and it is still true -- but it was only
        half the contract, and the missing half cost a real measurement: the
        loop ALSO writes the pooled win rate into loop2/curve.tsv, and row 4 of
        the desktop's curve therefore records a reference match that never ran
        as `vs_planchamp=0.0000`, indistinguishable afterwards from the net
        being beaten 0-72 by the champion.

        Failing closed on the promotion decision is not enough if the same
        number is also a datum.  So the scores are now `NA` -- unparseable by
        the numeric pattern the loop scrapes with, hence impossible to record
        as an observation -- and the exit status is nonzero.  Both halves of
        the original intent survive: no promotion on no evidence, and now no
        data point either.
        """
        with tempfile.TemporaryDirectory() as d:
            out = subprocess.run(
                [sys.executable, SCRIPT, os.path.join(d, "nope.log")],
                capture_output=True, text=True, cwd=ROOT)
            self.assertEqual(out.returncode, 3, out.stderr)
            line = out.stdout.strip()
            # the counters are honest zeroes; callers test them for emptiness
            self.assertIn("n=0", line)
            self.assertIn("shards=0", line)
            # the scores are not numbers
            self.assertIsNone(re.search(r"\swin=(-?[0-9.]+)", line))
            self.assertIsNone(re.search(r"\sci=(-?[0-9.]+)", line))
            self.assertNotIn("win=0.0000", line)

    def test_positive_control_a_real_pool_still_scores_and_exits_zero(self):
        """The matched pair for the test above: the guard must not fire on
        evidence that does exist."""
        with tempfile.TemporaryDirectory() as d:
            out = subprocess.run(
                [sys.executable, SCRIPT, _shard(d, "a.log", 0.4, 72)],
                capture_output=True, text=True, cwd=ROOT)
            self.assertEqual(out.returncode, 0, out.stderr)
            m = re.search(r"\swin=(-?[0-9.]+)", out.stdout)
            self.assertIsNotNone(m)
            self.assertAlmostEqual(float(m.group(1)), 0.4, places=4)

    def test_last_summary_wins_within_a_shard(self):
        with tempfile.TemporaryDirectory() as d:
            p = os.path.join(d, "a.log")
            with open(p, "w") as f:
                f.write("SUMMARY win=0.1000 ci=0.1 neural=1.0 opp=1.0 "
                        "margin=0.0 n=10 errs=0\n")
                f.write("SUMMARY win=0.9000 ci=0.1 neural=1.0 opp=1.0 "
                        "margin=0.0 n=10 errs=0\n")
            self.assertAlmostEqual(float(_pool([p])["win"]), 0.9, places=4)

    def test_output_parses_the_way_the_loop_parses_it(self):
        """The loop greps win= and ci= with sed; keep the format identical to
        neural_eval.py's so one code path reads both."""
        with tempfile.TemporaryDirectory() as d:
            out = subprocess.run(
                [sys.executable, SCRIPT, _shard(d, "a.log", 0.55, 200)],
                capture_output=True, text=True, cwd=ROOT).stdout.strip()
            self.assertTrue(out.startswith("SUMMARY win="))
            for key in ("win=", "ci=", "neural=", "opp=", "margin=", "n=",
                        "errs="):
                self.assertIn(key, out)


class TestShardClusteredInterval(unittest.TestCase):
    """The 2026-07-30 fix: `ci=` cannot see the shards, `ci_cluster=` can."""

    # The neural loop's real anchor, loop2/anchor_seed_{0..5}.log.
    ANCHOR = [0.3250, 0.3000, 0.3875, 0.5625, 0.4250, 0.5875]

    def _pool_wins(self, wins, n=40):
        with tempfile.TemporaryDirectory() as d:
            ps = [_shard(d, f"s{i}.log", w, n) for i, w in enumerate(wins)]
            return _pool(ps)

    def test_anchor_overdispersion_is_detected_and_quantified(self):
        r = self._pool_wins(self.ANCHOR)
        self.assertAlmostEqual(float(r["win"]), 0.4313, places=4)
        # what the project published
        self.assertAlmostEqual(float(r["ci"]), 0.0627, places=4)
        # what it should have published
        self.assertAlmostEqual(float(r["ci_cluster"]), 0.1260, places=4)
        self.assertAlmostEqual(float(r["chi2"]), 11.76, places=2)
        self.assertEqual(int(r["df"]), 5)
        self.assertEqual(int(r["overdispersed"]), 1)
        # The whole point: nearly 2x optimistic.
        self.assertGreater(float(r["ci_cluster"]) / float(r["ci"]), 1.9)

    def test_legacy_ci_field_is_unchanged(self):
        """A live loop parses `ci=`.  Its value must not move under it."""
        r = self._pool_wins(self.ANCHOR)
        n, wm = 240, 0.43125
        # the field is printed to 4dp, which is the precision the loop parses
        self.assertAlmostEqual(
            float(r["ci"]), 1.96 * math.sqrt(wm * (1 - wm) / n), places=4)

    def test_agreeing_shards_are_not_flagged(self):
        r = self._pool_wins([0.50, 0.50, 0.50, 0.50, 0.50, 0.50])
        self.assertEqual(int(r["overdispersed"]), 0)
        # Perfect agreement => no between-shard variance to report.
        self.assertAlmostEqual(float(r["ci_cluster"]), 0.0, places=9)
        # ...while the independent-samples formula still claims +/-6.3pp.
        self.assertGreater(float(r["ci"]), 0.06)

    def test_single_shard_cannot_bound_itself(self):
        with tempfile.TemporaryDirectory() as d:
            r = _pool([_shard(d, "a.log", 0.55, 200)])
            self.assertEqual(int(r["shards"]), 1)
            self.assertEqual(float(r["ci_cluster"]), float("inf"))

    def test_no_shards_still_emits_the_new_fields_as_NA(self):
        out = subprocess.run([sys.executable, SCRIPT, "/nope/missing.log"],
                             capture_output=True, text=True, cwd=ROOT)
        self.assertEqual(out.returncode, 3)
        self.assertIn("ci_cluster=NA", out.stdout)


if __name__ == "__main__":
    unittest.main()
