"""The neural loop's promotion gate and its refusal to invent measurements.

Two bugs are pinned here, both of which shipped and both of which are the same
shape: a number that looks like an observation but is not one.

  1. A reference match that completed ZERO games was written into
     loop2/curve.tsv as `vs_planchamp=0.0000` -- indistinguishable, afterwards,
     from the net losing 0-72 to the champion.  `pool_summary.py` printed a
     parseable `win=0.0000` for an empty pool and exited 0, and the loop
     scraped it with a numeric pattern.

  2. The promotion gate was self-referential: candidate vs current best, and
     nothing else.  Drift satisfies that as readily as learning does, and it
     did -- self-play culture climbed 116 -> 143 over seven iterations while
     the fixed anchor (plan:champion_2p) stayed flat at ~0.37.  The anchor is
     now a second arm of the gate.

Every test is a matched PAIR, negative control first, because the only
interesting property of a guard is that it fires.

`experiments/neural_search_loop.sh` is bash, and the repo has no harness for
executing a driver script.  The precedent for testing one is
`tests/test_zero_game_alarm.py::Halt`'s source-text assertions.  Arm 3 goes one
better than pinning text: it EXTRACTS the awk program that implements the
anchor rule out of the script and runs it, so these are tests of the rule the
loop actually executes rather than of a copy that can drift away from it.
"""
import os
import re
import subprocess
import sys
import unittest

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
POOL = os.path.join(ROOT, "experiments", "pool_summary.py")
LOOP = os.path.join(ROOT, "experiments", "neural_search_loop.sh")


def loop_src():
    with open(LOOP) as f:
        return f.read()


def pool(paths):
    return subprocess.run([sys.executable, POOL] + paths,
                          capture_output=True, text=True, cwd=ROOT)


# The loop scrapes numbers out of a SUMMARY line with this pattern (sfield in
# neural_search_loop.sh).  Kept here in the same form so a "win" that is not a
# number is tested the way the loop would actually read it.
def scrape(field, line):
    m = re.search(r"[\s]%s=(-?[0-9.]+)" % field, line)
    return m.group(1) if m else None


class EmptyPoolIsNotAScore(unittest.TestCase):
    """Arm 1: pool_summary.py must not emit a parseable win rate for no games."""

    def setUp(self):
        self.good = os.path.join(ROOT, "tests", "_tmp_pool_good.log")
        with open(self.good, "w") as f:
            f.write("SUMMARY win=0.4000 ci=0.1000 neural=140.0 opp=160.0 "
                    "margin=-20.0 n=72 errs=0\n")

    def tearDown(self):
        for p in (self.good,):
            if os.path.exists(p):
                os.remove(p)

    def test_negative_control_no_shards_yields_no_number_and_a_bad_status(self):
        r = pool([os.path.join(ROOT, "tests", "_tmp_does_not_exist.log")])
        self.assertEqual(r.returncode, 3, r.stdout + r.stderr)
        self.assertIn("n=0", r.stdout)
        self.assertIn("shards=0", r.stdout)
        # the whole bug: this must NOT parse as a win rate
        self.assertIsNone(scrape("win", r.stdout))
        self.assertIsNone(scrape("ci", r.stdout))
        self.assertNotIn("win=0.0000", r.stdout)

    def test_positive_control_real_shards_still_yield_a_number_and_status_zero(self):
        r = pool([self.good])
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertEqual(scrape("win", r.stdout), "0.4000")
        self.assertEqual(scrape("n", r.stdout), "72")
        self.assertEqual(scrape("shards", r.stdout), "1")

    def test_the_counters_stay_numeric_because_callers_test_them(self):
        # n and shards are genuinely 0, and the loop's emptiness check reads
        # them; only the SCORES are NA.
        r = pool([os.path.join(ROOT, "tests", "_tmp_does_not_exist.log")])
        self.assertEqual(scrape("n", r.stdout), "0")
        self.assertEqual(scrape("shards", r.stdout), "0")


class LoopRefusesToRecordAbsentWork(unittest.TestCase):
    """Arm 2: the driver treats a failed measurement as missing, not as zero."""

    def test_negative_control_the_old_zero_defaults_are_gone(self):
        src = loop_src()
        # `win=${win:-0}; ci=${ci:-1}` is exactly how an empty pool became a
        # promotion-gate input of 0.0000.
        self.assertNotIn("win=${win:-0}", src)
        self.assertNotIn("${vp:--}", src)

    def test_positive_control_a_failed_measurement_writes_the_null_token(self):
        src = loop_src()
        self.assertIn("NULL='-'", src)
        # every score written into the curve falls back to $NULL, never a digit
        self.assertIn('"${cwin:-$NULL}"', src)
        self.assertIn('"${cci:-$NULL}"', src)
        self.assertIn('"${iwin:-$NULL}"', src)

    def test_the_fanout_retries_once_then_reports_failure(self):
        src = loop_src()
        self.assertIn("for attempt in 1 2; do", src)
        self.assertIn("MEASUREMENT FAILED", src)
        self.assertIn("NO GAMES", src)
        # and it must signal failure to its caller rather than printing a score
        self.assertRegex(src, r"MEASUREMENT FAILED[\s\S]{0,400}?\n  return 1")

    def test_emptiness_is_decided_on_n_and_shards_not_on_the_win_rate(self):
        src = loop_src()
        self.assertIn('n=$(sfield n "$out"); shards=$(sfield shards "$out")', src)

    def test_fanout_keeps_its_stdout_clean_of_log_prose(self):
        """Only the measurement may go to fanout's stdout.

        `say` tees to stdout, and every caller of fanout captures its stdout
        with $(...) and parses it with sfield.  A `say` inside fanout therefore
        splices log lines into the SUMMARY string -- and the retry path is
        exactly where it bites: attempt 1 logs, attempt 2 succeeds, and the
        caller's `win` becomes three lines of prose plus a number.  Diagnostics
        use sayerr (stderr); the one bare `printf` of $out is the only thing on
        stdout.
        """
        src = loop_src()
        body = src[src.index("fanout() {"):]
        body = body[:body.index("\n}\n")]
        self.assertNotRegex(body, r"(?m)^\s*say\s",
                            "fanout must not call say(); use sayerr()")
        self.assertIn("sayerr", body)
        # and sayerr must actually redirect away from stdout
        self.assertRegex(src, r"sayerr\(\)\s*\{[^}]*>&2")


class ReferenceRunsEveryIteration(unittest.TestCase):
    """Arm 3: the anchor is measured every iteration, at a usable n."""

    def test_negative_control_the_promotion_only_condition_is_gone(self):
        src = loop_src()
        # `if [ $(( it % REFEVERY )) -eq 0 ] || [ "$promote" = "1" ]` is what
        # put '-' in the vs_planchamp column of iterations 3, 6 and 8.
        self.assertNotIn("REFEVERY", src)
        self.assertNotIn('it % REFEVERY', src)

    def test_positive_control_the_anchor_default_resolves_the_effects_we_chase(self):
        src = loop_src()
        self.assertIn("REFN=${REFN:-240}", src)

    def test_the_anchor_runs_before_the_promotion_decision_not_after(self):
        # It is an input to the gate now, so it has to be measured first.
        src = loop_src()
        anchor = src.index("ARM B anchor")
        decide = src.index("promote=0")
        self.assertLess(anchor, decide,
                        "the anchor must be measured before promotion is decided")


class AnchorGateRule(unittest.TestCase):
    """Arm 4: the actual arithmetic of arm B, extracted from the script.

    Not a reimplementation -- the awk program below is read out of
    neural_search_loop.sh, so if the rule changes these tests exercise the
    changed rule and the properties still have to hold.
    """

    def setUp(self):
        src = loop_src()
        m = re.search(r"'(BEGIN\{se=sqrt\([^']*)'", src, re.S)
        self.assertIsNotNone(m, "could not find the anchor-gate awk program")
        self.prog = m.group(1)

    def ok(self, cand_win, cand_ci, inc_win, inc_ci):
        r = subprocess.run(
            ["awk", "-v", "cw=%s" % cand_win, "-v", "cc=%s" % cand_ci,
             "-v", "iw=%s" % inc_win, "-v", "ic=%s" % inc_ci, self.prog],
            stdin=subprocess.DEVNULL, capture_output=True, text=True)
        self.assertEqual(r.returncode, 0, r.stderr)
        return r.stdout.strip()

    # n=240 gives ci ~ 0.063 a side, so se_diff ~ 0.045.
    CI = "0.063"

    def test_negative_control_a_materially_worse_candidate_is_blocked(self):
        # 10pp below the incumbent on the fixed anchor: this is the treadmill
        # case, and it must not promote no matter what self-play says.
        self.assertEqual(self.ok("0.30", self.CI, "0.40", self.CI), "0")

    def test_positive_control_an_equal_candidate_passes(self):
        self.assertEqual(self.ok("0.40", self.CI, "0.40", self.CI), "1")

    def test_a_better_candidate_passes(self):
        self.assertEqual(self.ok("0.46", self.CI, "0.40", self.CI), "1")

    def test_noise_sized_regressions_are_tolerated(self):
        # 2pp below, well inside one se of the difference (~0.045): the gate
        # must not reject on noise or nothing will ever promote again.
        self.assertEqual(self.ok("0.38", self.CI, "0.40", self.CI), "1")

    def test_the_band_is_one_standard_error_of_the_DIFFERENCE(self):
        # Both sides carry variance.  A candidate 4pp down passes with both
        # sides measured (se_diff ~ 0.045) ...
        self.assertEqual(self.ok("0.36", self.CI, "0.40", self.CI), "1")
        # ... and the same 4pp gap is blocked once both sides are precise,
        # which is the property that makes the gate bite as n grows.
        self.assertEqual(self.ok("0.36", "0.010", "0.40", "0.010"), "0")

    def test_a_wide_incumbent_estimate_widens_the_band_not_narrows_it(self):
        # An imprecisely known incumbent must make the gate MORE permissive,
        # never less -- otherwise early, noisy baselines would freeze the run.
        tight = self.ok("0.34", self.CI, "0.40", self.CI)
        wide = self.ok("0.34", self.CI, "0.40", "0.200")
        self.assertEqual(tight, "0")
        self.assertEqual(wide, "1")


class BothArmsAreRequiredAndLoggedSeparately(unittest.TestCase):
    """Arm 5: promotion needs both criteria, and the log says which blocked."""

    def test_negative_control_self_play_alone_no_longer_promotes(self):
        src = loop_src()
        # the old rule was the whole decision:
        #   promote=$(awk -v w="$win" -v c="$ci" 'BEGIN{print (w-c>0.5)?1:0}')
        self.assertNotIn('promote=$(awk -v w="$win"', src)

    def test_positive_control_promotion_requires_both_arms(self):
        src = loop_src()
        self.assertIn('[ "$selfplay_ok" = 1 ] && [ "$anchor_ok" = 1 ] && promote=1',
                      src)

    def test_each_arm_is_logged_under_its_own_name(self):
        src = loop_src()
        self.assertIn("ARM A self-play", src)
        self.assertIn("ARM B anchor", src)
        # and both land in the curve so a blocked promotion is greppable later
        self.assertIn("selfplay_ok\\tanchor_ok", src)

    def test_a_missing_anchor_measurement_fails_closed(self):
        src = loop_src()
        b = src.index("ARM B anchor   : NO DATA")
        self.assertIn("fails closed", src[b:b + 200])

    def test_the_incumbent_anchor_is_only_rewritten_on_promotion(self):
        # If it were rewritten every iteration the gate would ratchet down to
        # whatever the last candidate scored, which is the treadmill again.
        src = loop_src()
        promoted = src.index('say "  PROMOTED it$it')
        block = src[src.index("promote=0"):promoted]
        self.assertIn('printf \'%s %s\\n\' "$cwin" "$cci" > "$ANCHORF"', block)


if __name__ == "__main__":
    unittest.main()
