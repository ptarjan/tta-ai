"""Pooling the fanned-out gate shards.

A beam-vs-beam gate is ~18 cpu-s per game, so the loop runs it as N parallel
processes over disjoint seed ranges and pools the SUMMARY lines.  The promotion
decision is `win - ci > 0.5`, so a pooling bug is a silent promotion bug: it
would either promote regressions or freeze the loop.  Torch-free, so it runs in
tools/gate.sh on the Mac.
"""
import os
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

    def test_no_shards_is_a_safe_non_promotion(self):
        """Every worker dying (a guard kill) must read as ci=1.0 so that
        `win - ci > 0.5` is false and nothing gets promoted on no evidence."""
        with tempfile.TemporaryDirectory() as d:
            r = _pool([os.path.join(d, "nope.log")])
            self.assertEqual(int(r["n"]), 0)
            self.assertGreaterEqual(float(r["ci"]), 1.0)
            self.assertLess(float(r["win"]) - float(r["ci"]), 0.5)

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


if __name__ == "__main__":
    unittest.main()
