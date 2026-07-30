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


class NearIdenticalDescendantsAreRefused(unittest.TestCase):
    """The exact-content test this replaced had a hole big enough to drive
    every 4p measurement through.

    `analysis/frozen/champion_4p.json` is the degenerate vector six
    generations later.  It reproduces all 62 of its informative weights
    bit-for-bit -- including `science=-6.08883` -- and differs on exactly two
    keys (`colonies`, `pacts`), which is enough to defeat
    ``all(mine.get(k) == v ...)``.  It was the frozen 4p reference every A/B
    harness loaded.
    """

    ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")

    def test_the_frozen_4p_reference_is_refused(self):
        """Under EITHER name.  The file was renamed to
        `champion_4p.DEGENERATE.json` when it was quarantined; the guard must
        not depend on that, which is the whole point of a content test."""
        found = False
        for base in ("champion_4p.json", "champion_4p.DEGENERATE.json"):
            p = os.path.join(self.ROOT, "analysis", "frozen", base)
            if not os.path.exists(p):
                continue
            found = True
            with self.subTest(name=base), self.assertRaises(SystemExit):
                refuse_if_degenerate_champion(p, "test")
        self.assertTrue(found, "the quarantined 4p vector should still be on "
                                "disk under one of its two names -- it is kept "
                                "so published numbers stay auditable")

    def test_the_frozen_4p_reference_is_not_loadable_under_its_old_name(self):
        """The rename must be a real quarantine, not a second copy."""
        self.assertFalse(
            os.path.exists(os.path.join(self.ROOT, "analysis", "frozen",
                                        "champion_4p.json")),
            "analysis/frozen/champion_4p.json is back -- it is the degenerate "
            "vector and must stay renamed to champion_4p.DEGENERATE.json")

    def test_a_vector_differing_on_two_keys_is_refused(self):
        """The exact shape of the hole, built from scratch so this test does
        not depend on the frozen file still being on disk."""
        w = dict(arena._weights_of(DEGENERATE))
        w["colonies"] = -0.96161
        w["pacts"] = 0.46889
        with tempfile.NamedTemporaryFile(
                "w", suffix=".json", delete=False) as tmp:
            json.dump({"gen": 139, "players": 4, "weights": w}, tmp)
            near = tmp.name
        try:
            with self.assertRaises(SystemExit):
                refuse_if_degenerate_champion(near, "test")
        finally:
            os.unlink(near)

    def test_default_weights_alone_is_not_a_match(self):
        """`DEFAULT_WEIGHTS` agrees with the degenerate vector on 20% of all
        keys purely through untouched entries.  Scoring provenance on the
        MOVED keys only is what keeps that from being a false positive."""
        from engine.bots.weighted import DEFAULT_WEIGHTS
        self.assertEqual(
            arena._degenerate_match(DEFAULT_WEIGHTS,
                                    arena._weights_of(DEGENERATE)), 0.0)

    def test_live_league_champions_are_not_refused(self):
        """The separation has to be total in BOTH directions, or the guard
        starts refusing the bot we actually train."""
        for n in ("2p", "3p", "4p"):
            p = os.path.join(self.ROOT, "experiments", "league_state",
                             f"champion_{n}.json")
            if not os.path.exists(p):
                continue
            with self.subTest(players=n):
                self.assertEqual(
                    arena._degenerate_match(arena._weights_of(p),
                                            arena._weights_of(DEGENERATE)),
                    0.0)
                refuse_if_degenerate_champion(p, "test")  # must not raise

    def test_unrelated_champions_score_zero_not_merely_below_threshold(self):
        """Every other champion vector in the repo must score a flat 0.0 on
        the informative keys.  If any of them crept up toward the threshold
        the fraction would be a similarity score, not a fingerprint."""
        known = arena._weights_of(DEGENERATE)
        for rel in ("analysis/frozen/champion_2p.json",
                    "analysis/frozen/champion_3p.json",
                    "experiments/champion_2p.json",
                    "experiments/champion_3p.json"):
            p = os.path.join(self.ROOT, rel)
            if not os.path.exists(p):
                continue
            with self.subTest(vector=rel):
                self.assertEqual(
                    arena._degenerate_match(arena._weights_of(p), known), 0.0)


class LeverMustBePluggedIn(unittest.TestCase):
    """The other half of the same failure: a vector that cannot express the
    thing being measured returns a clean null, not an error.

    `docs/CARD_BLINDNESS.md` Sec 5.3 spent 12,800 games measuring
    `card_rate_credit` against a vector whose `row_urgency` is 0.0. For a
    WONDER that is the only channel, so the answer was zero before the first
    game was dealt.
    """

    ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")

    def _w(self, rel):
        return arena._weights_of(os.path.join(self.ROOT, rel))

    def test_frozen_2p_cannot_express_a_wonder_reprice(self):
        from engine.bots.weighted import load_weights
        w = load_weights(os.path.join(self.ROOT,
                                      "analysis/frozen/champion_2p.json"))
        open_, closed = arena.lever_conduction(
            w, arena.WONDER_CARD_POTENTIAL_CONSUMERS)
        self.assertEqual(open_, (), "frozen 2p should have NO open wonder path")
        self.assertEqual(set(closed), {"wonder_potential", "row_pressure"})
        with self.assertRaises(SystemExit):
            arena.assert_lever_conducts(
                w, "card_rate_credit", "test",
                arena.WONDER_CARD_POTENTIAL_CONSUMERS)

    def test_frozen_2p_CAN_express_a_leader_reprice(self):
        """The same vector, the same lever, a different card class -- which is
        why Sec 5's +9.5pp headline is real and its wonder null is not."""
        from engine.bots.weighted import load_weights
        w = load_weights(os.path.join(self.ROOT,
                                      "analysis/frozen/champion_2p.json"))
        open_, _ = arena.lever_conduction(w)          # all consumers
        self.assertIn("hand_potential", open_)
        arena.assert_lever_conducts(w, "card_rate_credit", "test")  # no raise

    def test_a_live_league_champion_can_express_a_wonder_reprice(self):
        from engine.bots.weighted import load_weights
        p = os.path.join(self.ROOT, "experiments/league_state/champion_2p.json")
        if not os.path.exists(p):
            p = os.path.join(self.ROOT,
                             "analysis/frozen/champion_2p_gen54_99key.json")
        if not os.path.exists(p):
            self.skipTest("no 99-key champion available")
        w = load_weights(p)
        open_, _ = arena.lever_conduction(
            w, arena.WONDER_CARD_POTENTIAL_CONSUMERS)
        self.assertIn("row_pressure", open_)
        arena.assert_lever_conducts(
            w, "card_rate_credit", "test",
            arena.WONDER_CARD_POTENTIAL_CONSUMERS)                  # no raise

    def test_every_gate_names_real_weights(self):
        """A typo in EVALUATE_GATES would silently make a closed gate look
        open forever."""
        from engine.bots.weighted import DEFAULT_WEIGHTS
        for fn, gates in arena.EVALUATE_GATES.items():
            for g in gates:
                with self.subTest(fn=fn, gate=g):
                    self.assertIn(g, DEFAULT_WEIGHTS)

    def test_consumer_lists_are_covered_by_the_gate_map(self):
        for fn in (arena.CARD_POTENTIAL_CONSUMERS
                   + arena.WONDER_CARD_POTENTIAL_CONSUMERS):
            self.assertIn(fn, arena.EVALUATE_GATES)


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
