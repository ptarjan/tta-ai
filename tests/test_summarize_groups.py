"""experiments/summarize.py:GROUPS enumerates feature names by hand, and
`group_of()` used to fall through to `"?"` for anything missing. Four
BASE_WEIGHTS keys were missing -- pact_blocks_attack, auction_committed,
auction_bid, hand_potential -- so every published weight table
(docs/HEURISTICS.md, docs/HEURISTICS.md, experiments/PROGRESS.md,
all generated via experiments/analyze_weights.py's use of group_of) silently
binned those four into "?". hand_potential is the ablation ledger's single
most load-bearing 2p weight (mean_edge -0.194), so this was not a cosmetic
gap.

This is a recurrence test, not just a fix check: it also asserts that an
actually-unknown feature name now raises instead of returning "?", so the
next new feature added to BASE_WEIGHTS without a GROUPS entry fails loudly
at report time instead of vanishing the same way.
"""
import ast
import inspect
import os
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine.bots.weighted import BASE_WEIGHTS, DEFAULT_WEIGHTS  # noqa: E402
from experiments import summarize                                # noqa: E402
from experiments.summarize import group_of                       # noqa: E402


class GroupsLiteralIsWellFormed(unittest.TestCase):
    """`GROUPS` is a dict literal, so a REPEATED bucket name is legal Python
    that silently throws the earlier bucket away.

    Not hypothetical: adding an `"events"` bucket for the own-seed terms while
    an `"events"` bucket already existed dropped `event_scoring_margin` out of
    every group, and what surfaced was `group_of` raising on a key that had
    been correctly bucketed for weeks -- an error naming the innocent party.
    docs/COORDINATE_REGISTRY.md section 10 lists GROUPS as a registry that can
    rot with nothing failing; this closes one way of rotting it.
    """

    def _literal(self):
        tree = ast.parse(inspect.getsource(summarize))
        for node in ast.walk(tree):
            if isinstance(node, ast.Assign) and any(
                    isinstance(t, ast.Name) and t.id == "GROUPS"
                    for t in node.targets):
                return node.value
        self.fail("GROUPS assignment not found in experiments/summarize.py")

    def test_no_duplicate_bucket_names(self):
        lit = self._literal()
        self.assertIsInstance(lit, ast.Dict, "GROUPS is no longer a literal")
        names = [k.value for k in lit.keys
                 if isinstance(k, ast.Constant) and isinstance(k.value, str)]
        self.assertEqual(len(names), len(lit.keys),
                         "a GROUPS bucket key is not a string literal")
        dupes = sorted({n for n in names if names.count(n) > 1})
        self.assertFalse(dupes, (
            f"GROUPS declares {dupes} more than once.  The later literal "
            "silently replaces the earlier one and every key in the first "
            "bucket falls out of the summary.  Merge them into one bucket."))

    def test_no_feature_is_in_two_buckets(self):
        seen = {}
        for group, keys in summarize.GROUPS.items():
            for k in keys:
                self.assertNotIn(k, seen, (
                    f"{k!r} is in both {seen.get(k)!r} and {group!r}; "
                    "group_of reports whichever iteration reaches first."))
                seen[k] = group

PREVIOUSLY_MISSING = {
    "pact_blocks_attack": "military",
    "auction_committed": "military",
    "auction_bid": "military",
    "hand_potential": "cards",
}


class GroupOfCoversEveryFeature(unittest.TestCase):

    def test_the_four_previously_unbucketed_features_get_a_real_group(self):
        for key, expected in PREVIOUSLY_MISSING.items():
            with self.subTest(key=key):
                self.assertEqual(group_of(key), expected)

    def test_no_base_weight_falls_through_to_unknown(self):
        for key in BASE_WEIGHTS:
            with self.subTest(key=key):
                self.assertNotEqual(group_of(key), "?")

    def test_no_default_weight_falls_through_to_unknown(self):
        """Includes every _early/_late phase-multiplier key."""
        for key in DEFAULT_WEIGHTS:
            with self.subTest(key=key):
                self.assertNotEqual(group_of(key), "?")

    def test_hand_potential_is_specifically_reachable(self):
        """The archaeology finding singled this one out: it is the
        single most load-bearing 2p weight in the ablation ledger, and it
        was one of the four silently binned as '?'."""
        self.assertIn("hand_potential", BASE_WEIGHTS)
        self.assertEqual(group_of("hand_potential"), "cards")

    def test_an_actually_unknown_feature_fails_loudly(self):
        """The whole point of the fix: no feature can vanish as '?' again,
        because an unrecognised key now raises instead of being bucketed."""
        with self.assertRaises(KeyError):
            group_of("this_feature_does_not_exist")

    def test_phase_suffixed_unknown_feature_also_fails_loudly(self):
        with self.assertRaises(KeyError):
            group_of("this_feature_does_not_exist_late")


if __name__ == "__main__":
    unittest.main()
