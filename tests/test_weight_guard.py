"""`hillclimb_league.guard_weights` must be two-sided.

Recurrence test for docs/CULTURE_GAP.md section 2c: the 4p champion reached
`rival_culture` = +5.611 against a default of -0.35 inside one accepted `kick`
mutation and nothing logged it, because the guard only ever checked weights
whose default was positive.  Every test in `test_two_sided` fails against the
one-sided guard.
"""
import os
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine.bots.weighted import DEFAULT_WEIGHTS, PHASE_KEYS  # noqa: E402
from experiments import hillclimb_league as HL  # noqa: E402

guard_weights = HL.guard_weights
NONNEG = HL.NONNEG
# Resolved late and defensively ON PURPOSE: against the pre-fix one-sided guard
# these tests must fail on their assertions, showing which weight is
# unprotected, rather than dying at import with an ImportError.
NONPOS = getattr(HL, "NONPOS", frozenset())

#: the 15 value terms the two-sided guard newly protects, hand-listed so the
#: test is not a restatement of the implementation's own comprehension
EXPECTED_NONPOS = frozenset({
    "auction_bid", "consumption", "corruption_loss", "discontent",
    "end_turn_bias", "pop_cost", "rival_culture", "rival_culture_rate",
    "rival_mean_culture", "rival_science_rate", "rival_strength",
    "strength_deficit", "uprising", "wonder_remaining", "yellow_bank",
})


def _viol_names(viol):
    return {v["weight"] for v in viol}


class GuardWeights(unittest.TestCase):

    def test_positive_default_still_clamped(self):
        """The original one-sided behaviour is unchanged (`science` = -6.089)."""
        w = dict(DEFAULT_WEIGHTS, science=-6.089)
        out, viol = guard_weights(w)
        self.assertIn("science", _viol_names(viol))
        self.assertEqual(out["science"], 0.0)

    def test_two_sided_rival_culture(self):
        """The exact vector that got past the old guard."""
        w = dict(DEFAULT_WEIGHTS, rival_culture=5.611)
        out, viol = guard_weights(w)
        self.assertIn("rival_culture", _viol_names(viol))
        self.assertEqual(out["rival_culture"], 0.0)

    def test_two_sided_every_negative_value_term(self):
        """No sign-locked value term may cross zero, one at a time."""
        self.assertEqual(set(NONPOS), set(EXPECTED_NONPOS))
        for k in sorted(EXPECTED_NONPOS):
            with self.subTest(weight=k):
                out, viol = guard_weights(dict(DEFAULT_WEIGHTS, **{k: 1.25}))
                self.assertIn(k, _viol_names(viol))
                self.assertEqual(out[k], 0.0)

    def test_flag_mode_does_not_rewrite(self):
        w = dict(DEFAULT_WEIGHTS, rival_culture=5.611)
        out, viol = guard_weights(w, mode="flag")
        self.assertIn("rival_culture", _viol_names(viol))
        self.assertEqual(out["rival_culture"], 5.611)

    def test_end_turn_bias_is_protected_not_fixed(self):
        """Driving `end_turn_bias` positive is the "pass MORE" regression."""
        out, _ = guard_weights(dict(DEFAULT_WEIGHTS, end_turn_bias=4.0))
        self.assertEqual(out["end_turn_bias"], 0.0)
        # ... and it must NOT be dragged towards zero from its own side.
        out, viol = guard_weights(dict(DEFAULT_WEIGHTS))
        self.assertEqual(out["end_turn_bias"], DEFAULT_WEIGHTS["end_turn_bias"])
        self.assertEqual(viol, [])

    def test_phase_multipliers_are_exempt_from_the_new_direction(self):
        """A phase multiplier's sign is not gauge-invariant -- see the comment
        on `NONPOS`.  `w[k] + (1-L)*e + L*l` is unchanged by adding c to both
        phase weights and subtracting c from the base, so a flipped phase
        multiplier says nothing about whether the policy inverted."""
        phase = [k + s for k in PHASE_KEYS for s in ("_early", "_late")]
        for k in phase:
            with self.subTest(weight=k):
                self.assertNotIn(k, NONPOS)
        # negative-default phase weights may go positive without a violation
        out, viol = guard_weights(dict(DEFAULT_WEIGHTS, culture_rate_late=0.9,
                                       science_rate_late=1.4))
        self.assertEqual(viol, [])
        self.assertEqual(out["culture_rate_late"], 0.9)

    def test_phase_exemption_is_symmetric(self):
        """docs/CULTURE_GAP.md 15a/19c fix #1.

        The ten POSITIVE-default phase multipliers used to stay one-sided
        clamped, which made "exactly 0.000" a spurious attractor: 15.7% per
        multiplier under pure drift with the guard on, 0.0% with it off,
        against 20.0% observed in the live champions.  The gauge argument that
        exempts the negative-default half applies verbatim to this half, so the
        exemption must be symmetric or the clamp keeps manufacturing zeros.
        """
        phase = [k + s for k in PHASE_KEYS for s in ("_early", "_late")]
        pos_phase = [k for k in phase if DEFAULT_WEIGHTS.get(k, 0.0) > 0]
        # The anti-vacuity clause.  It used to read `>= 10`, back when there
        # were ten positive-default multipliers; six pairs were retired on
        # 2026-08-04 (see the PHASE_KEYS note in weighted.py) and there are now
        # four.  A hard count is the wrong shape for this -- it asserts a
        # HISTORY, and it goes red for the one change that is not a regression.
        # What the test needs is that it is exercising every multiplier that
        # exists, which is an invariant and survives the tuple shrinking again.
        self.assertEqual(
            set(pos_phase),
            {k for k, v in DEFAULT_WEIGHTS.items()
             if v > 0 and k.rsplit("_", 1)[0] in PHASE_KEYS
             and k.endswith(("_early", "_late"))},
            "the positive-default phase multipliers and PHASE_KEYS disagree")
        self.assertTrue(pos_phase, "no positive-default phase multiplier left "
                                   "-- this test is now vacuous, delete it")
        for k in pos_phase:
            with self.subTest(weight=k):
                self.assertNotIn(k, NONNEG)
                # a positive-default phase multiplier may go negative and must
                # NOT be rewritten to 0.0 -- that rewrite is the attractor
                out, viol = guard_weights(dict(DEFAULT_WEIGHTS, **{k: -0.75}))
                self.assertEqual(viol, [])
                self.assertEqual(out[k], -0.75)

    def test_value_terms_are_still_clamped_both_ways(self):
        """The exemption must not leak into the value terms it protects."""
        for k, v in (("science", -6.089), ("rival_culture", 5.611),
                     ("end_turn_bias", 4.0), ("culture", -1.0)):
            with self.subTest(weight=k):
                out, viol = guard_weights(dict(DEFAULT_WEIGHTS, **{k: v}))
                self.assertIn(k, _viol_names(viol))
                self.assertEqual(out[k], 0.0)

    def test_the_two_sets_partition_the_vector(self):
        """Nothing is in both sets, and only phase multipliers are unguarded."""
        self.assertEqual(NONNEG & NONPOS, frozenset())
        unguarded = set(DEFAULT_WEIGHTS) - NONNEG - NONPOS
        phase = {k + s for k in PHASE_KEYS for s in ("_early", "_late")}
        self.assertEqual(unguarded, {k for k in phase
                                     if DEFAULT_WEIGHTS.get(k, 0.0) != 0.0}
                         | {k for k, v in DEFAULT_WEIGHTS.items() if v == 0.0})

    def test_clean_default_vector_is_untouched(self):
        out, viol = guard_weights(dict(DEFAULT_WEIGHTS))
        self.assertEqual(viol, [])
        self.assertEqual(out, dict(DEFAULT_WEIGHTS))


if __name__ == "__main__":
    unittest.main()
