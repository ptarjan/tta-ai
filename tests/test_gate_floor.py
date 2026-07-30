"""The neural loop's arm-B promotion floor.

The floor is `incumbent - sqrt(se_cand^2 + se_inc^2)`, one standard error of
the difference.  It is three lines of awk inside a 550-line bash driver that
only runs on a Windows box, which is exactly the kind of code that gets edited
without being run.  So these tests EXECUTE it: `anchor_floor` is lifted out of
experiments/neural_search_loop.sh by name and run under bash, rather than
matched against a regex.  If someone renames or deletes the function the
extraction fails and the suite goes red.

What they are defending, from docs/CARD_BLINDNESS.md 10.6:

  * the inputs are STANDARD ERRORS (`se_cluster=`), never half-widths;
  * with the anchor's real numbers the band is 6.93pp;
  * it is NOT 4.52pp (the legacy per-game `ci/1.96` that shipped until
    2026-07-30) and NOT 9.09pp (`ci_cluster/1.96`, which double-counts the
    t5 = 2.571 already inside ci_cluster -- the wrong answer that looks like
    the right one).

Torch-free and bash-only, so it runs in tools/gate.sh on the Mac.
"""
import math
import os
import re
import subprocess
import unittest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
LOOP = os.path.join(ROOT, "experiments", "neural_search_loop.sh")

# The anchor, loop2/anchor_seed_{0..5}.log: six shards of 40, win rates
# .325 .300 .3875 .5625 .425 .5875, mean 0.4313.  chi2 = 11.76 on 5 df.
ANCHOR_SE = 0.0490          # shard-clustered standard error, per side
ANCHOR_CI_CLUSTER = 0.1260  # = t5 * se = 2.571 * 0.0490
ANCHOR_CI_LEGACY = 0.0627   # = 1.96 * sqrt(p(1-p)/240), the old per-game one
ANCHOR_WIN = 0.4313


def _extract(name):
    """Lift one shell function out of the driver, by name."""
    with open(LOOP) as f:
        src = f.read()
    # `{` may be followed by a trailing comment; the body ends at the first
    # `}` in column 0, which is the driver's convention throughout.
    m = re.search(r"^%s\(\)\s*\{[^\n]*\n(.*?)^\}\s*$" % re.escape(name),
                  src, re.M | re.S)
    if not m:
        raise AssertionError(
            "%s() not found in %s -- if it was renamed, rename it here too; "
            "if it was inlined back into the gate, these tests no longer "
            "cover the floor and that is the thing to fix" % (name, LOOP))
    return "%s() {\n%s}\n" % (name, m.group(1))


def floor(inc_win, cand_se, inc_se):
    body = _extract("anchor_floor")
    out = subprocess.run(
        ["bash", "-c", body + 'anchor_floor "$1" "$2" "$3"', "_",
         str(inc_win), str(cand_se), str(inc_se)],
        capture_output=True, text=True)
    assert out.returncode == 0, out.stderr
    return float(out.stdout.strip())


def band(inc_win, cand_se, inc_se):
    return inc_win - floor(inc_win, cand_se, inc_se)


class TestAnchorFloor(unittest.TestCase):

    def test_band_is_one_se_of_the_difference(self):
        self.assertAlmostEqual(band(0.5, 0.03, 0.04), 0.05, places=4)

    def test_band_is_symmetric_in_its_two_inputs(self):
        self.assertAlmostEqual(band(0.5, 0.02, 0.07), band(0.5, 0.07, 0.02),
                               places=6)

    def test_band_exceeds_either_side_alone(self):
        """Both scores are estimates.  A floor built from one side's variance
        rejects on noise, which is the bug the sqrt-sum is here to prevent."""
        b = band(0.5, 0.049, 0.049)
        self.assertGreater(b, 0.049)

    def test_floor_tracks_the_incumbent(self):
        self.assertAlmostEqual(floor(0.60, 0.049, 0.049)
                               - floor(0.40, 0.049, 0.049), 0.20, places=4)

    def test_zero_variance_floor_is_the_incumbent(self):
        self.assertAlmostEqual(floor(0.4313, 0.0, 0.0), 0.4313, places=4)

    # ---- the three numbers, and the two that are wrong ---------------------

    def test_corrected_band_is_693pp(self):
        """The shard-clustered SE on both sides.  THE gate."""
        self.assertAlmostEqual(band(ANCHOR_WIN, ANCHOR_SE, ANCHOR_SE),
                               0.0693, places=4)

    def test_corrected_floor_on_the_live_anchor(self):
        self.assertAlmostEqual(floor(ANCHOR_WIN, ANCHOR_SE, ANCHOR_SE),
                               0.3620, places=4)

    def test_legacy_per_game_band_was_452pp_and_is_too_tight(self):
        """What shipped until 2026-07-30: se = ci/1.96 off the per-game CI."""
        legacy_se = ANCHOR_CI_LEGACY / 1.96
        self.assertAlmostEqual(legacy_se, 0.0320, places=4)
        self.assertAlmostEqual(band(ANCHOR_WIN, legacy_se, legacy_se),
                               0.0452, places=4)
        self.assertLess(band(ANCHOR_WIN, legacy_se, legacy_se),
                        band(ANCHOR_WIN, ANCHOR_SE, ANCHOR_SE))
        # ~1.5x tighter than the data supports
        self.assertAlmostEqual(
            band(ANCHOR_WIN, ANCHOR_SE, ANCHOR_SE)
            / band(ANCHOR_WIN, legacy_se, legacy_se), 1.53, places=1)

    def test_the_trap_ci_cluster_over_196_gives_909pp(self):
        """ci_cluster already carries t5 = 2.571.  Dividing it by 1.96 leaves
        2.571/1.96 = 1.312 behind and inflates the band to 9.09pp.  This is
        the plausible-looking wrong answer; assert it so that anyone who
        arrives at 9.09 knows which mistake they made."""
        trap_se = ANCHOR_CI_CLUSTER / 1.96
        self.assertAlmostEqual(trap_se, 0.0643, places=4)
        self.assertAlmostEqual(band(ANCHOR_WIN, trap_se, trap_se),
                               0.0909, places=4)
        self.assertGreater(band(ANCHOR_WIN, trap_se, trap_se),
                           band(ANCHOR_WIN, ANCHOR_SE, ANCHOR_SE))
        self.assertAlmostEqual(trap_se / ANCHOR_SE, 2.571 / 1.96, places=2)

    def test_the_three_bands_are_ordered_and_distinct(self):
        legacy = band(ANCHOR_WIN, ANCHOR_CI_LEGACY / 1.96,
                      ANCHOR_CI_LEGACY / 1.96)
        correct = band(ANCHOR_WIN, ANCHOR_SE, ANCHOR_SE)
        trap = band(ANCHOR_WIN, ANCHOR_CI_CLUSTER / 1.96,
                    ANCHOR_CI_CLUSTER / 1.96)
        self.assertLess(legacy, correct)
        self.assertLess(correct, trap)
        for a, b in ((legacy, correct), (correct, trap)):
            self.assertGreater(b - a, 0.02)


class TestFloorIsWiredUpCorrectly(unittest.TestCase):
    """The arithmetic can be right and still be fed the wrong field."""

    def setUp(self):
        with open(LOOP) as f:
            self.src = f.read()
        # the arm-B block, from its heading to the promotion decision
        i = self.src.index("GATE ARM B")
        j = self.src.index("promote iff BOTH arms pass")
        self.armb = self.src[i:j]

    def test_arm_b_reads_se_cluster(self):
        self.assertIn('sfield se_cluster "$AS"', self.armb)

    def test_arm_b_no_longer_divides_anything_by_196(self):
        """The whole defect, in one assertion.  Any `/1.96` reappearing in the
        arm-B decision is either the legacy per-game SE or the double-counted
        t5 -- there is no third thing it could be."""
        code = [ln for ln in self.armb.splitlines()
                if ln.strip() and not ln.strip().startswith("#")]
        self.assertNotIn("1.96", "\n".join(code))

    def test_arm_b_calls_the_shared_floor_function(self):
        self.assertIn("anchor_floor ", self.armb)

    def test_anchor_baseline_file_carries_three_fields(self):
        self.assertIn('printf \'%s %s %s\\n\' "$cwin" "$cci" "$cse"', self.src)
        self.assertIn("read -r iwin ici ise", self.src)


class TestFalsePromotionRate(unittest.TestCase):
    """What the band costs, stated rather than assumed.

    Arm B is a REGRESSION VETO, not a significance test -- the net is ~14pp
    behind the champion and a 5%-level test against it would freeze the loop
    forever.  Under the null (candidate identical to incumbent) the difference
    d has SE = 6.93pp, and arm B passes whenever d >= -1 SE.
    """

    @staticmethod
    def _pass_rate(floor_band, true_se_diff):
        z = -floor_band / true_se_diff
        return 0.5 * math.erfc(z / math.sqrt(2))

    def test_new_band_passes_84pc_of_null_candidates(self):
        se_d = math.sqrt(2) * ANCHOR_SE
        self.assertAlmostEqual(self._pass_rate(se_d, se_d), 0.8413, places=3)

    def test_old_band_passed_74pc(self):
        """Measured against the TRUE SE, the old 4.52pp floor sat at -0.65 SE,
        so it was already letting three null candidates in four through.  The
        correction moves 74% -> 84%: arm B was never the arm doing the
        rejecting, which is why loosening it is safe and why arm A must not be
        loosened with it."""
        se_d = math.sqrt(2) * ANCHOR_SE
        old = math.sqrt(2) * (ANCHOR_CI_LEGACY / 1.96)
        self.assertAlmostEqual(self._pass_rate(old, se_d), 0.7428, places=3)

    def test_joint_false_promotion_stays_near_2pc(self):
        """Promotion needs BOTH arms.  Arm A is a one-sided 95% test against
        0.5, so it admits ~2.5% of null candidates; the joint rate moves from
        ~1.9% to ~2.1%.  Arm A is where the type-I control lives."""
        se_d = math.sqrt(2) * ANCHOR_SE
        old = math.sqrt(2) * (ANCHOR_CI_LEGACY / 1.96)
        arm_a = 0.025
        self.assertAlmostEqual(arm_a * self._pass_rate(old, se_d), 0.0186,
                               places=3)
        self.assertAlmostEqual(arm_a * self._pass_rate(se_d, se_d), 0.0210,
                               places=3)


if __name__ == "__main__":
    unittest.main()
