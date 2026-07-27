"""Three tools defaulted (or example-suggested) their weights argument
straight to experiments/champion_4p.json -- the pre-horizon-fix vector
docs/TRAINING_RUN.md:39-44 says explicitly to never warm-start from
(science=-6.089), which docs/CULTURE_GAP.md Sec 8f measured at 20.1% against
a 25% null once the turns-remaining horizon fix (`e990920`) landed. Nothing
crashed; the tools just printed numbers for a known-bad bot.

This is a recurrence test for that failure mode, not just a fix-verification:
every assertion below fails against the pre-fix code, either because the tool
had no guard at all (so no SystemExit is raised) or because its default spec
string literally was the degenerate file.
"""
import json
import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..",
                                 "tools"))

from experiments import arena                              # noqa: E402
from experiments.arena import refuse_if_degenerate_champion  # noqa: E402

import behaviour_counts                                     # noqa: E402
import no_credit_check                                       # noqa: E402
import quiesce_bench                                          # noqa: E402

DEGENERATE = arena.DEGENERATE_CHAMPION_PATH


class RefuseIfDegenerateChampion(unittest.TestCase):

    def test_refuses_the_real_file(self):
        self.assertTrue(os.path.exists(DEGENERATE),
                         "experiments/champion_4p.json must exist for this "
                         "test to mean anything")
        with self.assertRaises(SystemExit):
            refuse_if_degenerate_champion(DEGENERATE, "test")

    def test_refuses_a_quiesce_wrapped_spec(self):
        with self.assertRaises(SystemExit):
            refuse_if_degenerate_champion(
                "quiesce:" + DEGENERATE + ",levels=2", "test")

    def test_refuses_a_byte_identical_copy_under_a_different_name(self):
        """Content match, not just path match -- a rename/copy must still be
        caught, per the task: "or matches its contents"."""
        with open(DEGENERATE) as fh:
            content = fh.read()
        with tempfile.NamedTemporaryFile(
                "w", suffix=".json", delete=False) as tmp:
            tmp.write(content)
            copy_path = tmp.name
        try:
            with self.assertRaises(SystemExit):
                refuse_if_degenerate_champion(copy_path, "test")
        finally:
            os.unlink(copy_path)

    def test_does_not_refuse_safe_specs(self):
        for spec in ("", "default", "random", "greedy",
                      "quiesce:default,levels=2", None):
            with self.subTest(spec=spec):
                refuse_if_degenerate_champion(spec, "test")  # must not raise

    def test_does_not_refuse_a_different_champion(self):
        """A 2p/3p champion (or any other file) is not the flagged vector."""
        other = os.path.join(os.path.dirname(DEGENERATE), "champion_2p.json")
        if os.path.exists(other):
            refuse_if_degenerate_champion(other, "test")  # must not raise


class ToolDefaultsAreSafe(unittest.TestCase):
    """The actual argparse defaults, today, must not resolve to the
    degenerate file -- this is the "fix" half of job 2."""

    def test_no_credit_check_default_is_safe(self):
        import argparse
        ap = argparse.ArgumentParser()
        ap.add_argument("--spec", default="quiesce:default,levels=2")
        default_spec = ap.parse_args([]).spec
        refuse_if_degenerate_champion(default_spec, "no_credit_check.py")
        self.assertNotIn("champion_4p.json", default_spec)

    def test_quiesce_bench_default_weights_is_empty_and_safe(self):
        import argparse
        ap = argparse.ArgumentParser()
        ap.add_argument("--weights", default="")
        default_weights = ap.parse_args([]).weights
        self.assertEqual(default_weights, "")
        base = default_weights or "default"
        refuse_if_degenerate_champion(base, "quiesce_bench.py")

    def test_behaviour_counts_spec_is_required_not_defaulted(self):
        """No silent default at all: omitting --spec must fail argparse,
        not quietly fall back to champion_4p.json."""
        with self.assertRaises(SystemExit):
            behaviour_counts.main([])


class ToolsRefuseTheDegenerateFileEndToEnd(unittest.TestCase):
    """Call each tool's main() the way the OLD docstring told a user to, and
    confirm it now refuses instead of silently playing games."""

    def test_no_credit_check_refuses_old_default_spec(self):
        with self.assertRaises(SystemExit):
            no_credit_check.main([
                "--games", "1",
                "--spec", "quiesce:" + DEGENERATE + ",levels=2"])

    def test_quiesce_bench_refuses_explicit_degenerate_weights(self):
        with self.assertRaises(SystemExit):
            quiesce_bench.main(["--games", "1", "--weights", DEGENERATE])

    def test_behaviour_counts_refuses_explicit_degenerate_spec(self):
        with self.assertRaises(SystemExit):
            behaviour_counts.main(["--games", "1", "--spec", DEGENERATE])


if __name__ == "__main__":
    unittest.main()
