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
import shutil
import subprocess
import sys
import unittest

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
POOL = os.path.join(ROOT, "experiments", "pool_summary.py")
LOOP = os.path.join(ROOT, "experiments", "neural_search_loop.sh")


def loop_src():
    with open(LOOP) as f:
        return f.read()


def find_awk():
    """Locate awk, including on the Windows box that actually runs the loop.

    These tests execute awk programs lifted out of the driver script, which
    means they are the only tests that can tell you the loop's rules still
    hold -- and the machine with the most reason to ask is paul-desktop, where
    the loop runs.  There, `awk` is not on cmd.exe's PATH: it ships inside
    git-for-windows, which is also what interprets the driver script, so the
    awk the loop will really use is the one under Git\\usr\\bin.  Falling back
    to it turns six silent FileNotFoundError errors into six executed tests.
    """
    found = shutil.which("awk")
    if found:
        return found
    for cand in (r"C:\Program Files\Git\usr\bin\awk.exe",
                 r"C:\Program Files (x86)\Git\usr\bin\awk.exe"):
        if os.path.exists(cand):
            return cand
    return None


AWK = find_awk()


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


@unittest.skipIf(AWK is None, "awk not available; cannot execute the extracted rules")
class AnchorGateRule(unittest.TestCase):
    """Arm 4: the actual arithmetic of arm B, extracted from the script.

    Not a reimplementation -- the awk program below is read out of
    neural_search_loop.sh, so if the rule changes these tests exercise the
    changed rule and the properties still have to hold.
    """

    def setUp(self):
        src = loop_src()
        # The floor, lifted out of anchor_floor().  Since 2026-07-30 the rule
        # is two pieces -- compute the floor, then compare against it -- and
        # both are extracted so neither can drift away from the driver.
        m = re.search(r"^anchor_floor\(\)[^\n]*\n.*?'(BEGIN\{[^']*)'",
                      src, re.S | re.M)
        self.assertIsNotNone(m, "could not find the anchor_floor awk program")
        self.floor_prog = m.group(1)
        d = re.search(r"anchor_ok=\$\(awk [^']*'(BEGIN\{[^']*)'", src, re.S)
        self.assertIsNotNone(d, "could not find the anchor-gate awk program")
        self.decide_prog = d.group(1)

    def _awk(self, prog, **vars):
        argv = [AWK]
        for k, v in vars.items():
            argv += ["-v", "%s=%s" % (k, v)]
        r = subprocess.run(argv + [prog], stdin=subprocess.DEVNULL,
                           capture_output=True, text=True)
        self.assertEqual(r.returncode, 0, r.stderr)
        return r.stdout.strip()

    def floor(self, inc_win, cand_se, inc_se):
        # `is` is a keyword, so the awk variable names go through a dict
        return self._awk(self.floor_prog,
                         **{"iw": inc_win, "cs": cand_se, "is": inc_se})

    def ok(self, cand_win, cand_se, inc_win, inc_se):
        return self._awk(self.decide_prog, cw=cand_win,
                         f=self.floor(inc_win, cand_se, inc_se))

    # The inputs are SHARD-CLUSTERED STANDARD ERRORS now, not the per-game 95%
    # half-widths this class used to pass.  At n=240 over six shards a side is
    # se ~ 0.049 (it was being read as ci/1.96 ~ 0.032), so the band is
    # ~0.069 where it used to be ~0.045.  docs/CARD_BLINDNESS.md 10.6.1.
    SE = "0.049"

    def test_negative_control_a_materially_worse_candidate_is_blocked(self):
        # 10pp below the incumbent on the fixed anchor: this is the treadmill
        # case, and it must not promote no matter what self-play says.  Still
        # blocked at the wider band -- widening the floor did not disarm it.
        self.assertEqual(self.ok("0.30", self.SE, "0.40", self.SE), "0")

    def test_positive_control_an_equal_candidate_passes(self):
        self.assertEqual(self.ok("0.40", self.SE, "0.40", self.SE), "1")

    def test_a_better_candidate_passes(self):
        self.assertEqual(self.ok("0.46", self.SE, "0.40", self.SE), "1")

    def test_noise_sized_regressions_are_tolerated(self):
        # 2pp below, well inside one se of the difference (~0.069): the gate
        # must not reject on noise or nothing will ever promote again.
        self.assertEqual(self.ok("0.38", self.SE, "0.40", self.SE), "1")

    def test_the_band_is_one_standard_error_of_the_DIFFERENCE(self):
        # Both sides carry variance.  A candidate 6pp down passes with both
        # sides measured (se_diff ~ 0.069) ...
        self.assertEqual(self.ok("0.34", self.SE, "0.40", self.SE), "1")
        # ... and the same 6pp gap is blocked once both sides are precise,
        # which is the property that makes the gate bite as n grows.
        self.assertEqual(self.ok("0.34", "0.010", "0.40", "0.010"), "0")

    def test_a_wide_incumbent_estimate_widens_the_band_not_narrows_it(self):
        # An imprecisely known incumbent must make the gate MORE permissive,
        # never less -- otherwise early, noisy baselines would freeze the run.
        tight = self.ok("0.32", self.SE, "0.40", self.SE)
        wide = self.ok("0.32", self.SE, "0.40", "0.200")
        self.assertEqual(tight, "0")
        self.assertEqual(wide, "1")

    def test_the_band_widened_from_452pp_to_693pp(self):
        """The correction itself, as a decision rather than as arithmetic.

        A candidate 5.5pp below the incumbent sits between the two floors: it
        was blocked by the old per-game band (4.52pp) and passes under the
        shard-clustered one (6.93pp).  This is exactly the class of candidate
        the audit says was being rejected on an over-confident interval.
        """
        old_se = 0.0627 / 1.96          # what the loop used to compute
        self.assertAlmostEqual(old_se, 0.032, places=3)
        self.assertEqual(self.ok("0.3763", str(old_se), "0.4313",
                                 str(old_se)), "0")
        self.assertEqual(self.ok("0.3763", self.SE, "0.4313", self.SE), "1")

    def test_the_floor_on_the_live_anchor_is_3620(self):
        self.assertEqual(self.floor("0.4313", "0.0490", "0.0490"), "0.3620")

    def test_the_floor_is_not_the_909pp_trap(self):
        """ci_cluster/1.96 double-counts t5 and would put the floor at 0.3404.
        Asserting the number we do NOT use keeps the mistake identifiable."""
        trap = str(0.1260 / 1.96)
        self.assertEqual(self.floor("0.4313", trap, trap), "0.3404")
        self.assertNotEqual(self.floor("0.4313", "0.0490", "0.0490"),
                            self.floor("0.4313", trap, trap))


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
        # Three fields since 2026-07-30: `win ci se`, where `se` is the
        # shard-clustered standard error the floor is built from.  Writing two
        # would leave the next iteration with no SE and arm B failing closed.
        self.assertIn(
            'printf \'%s %s %s\\n\' "$cwin" "$cci" "$cse" > "$ANCHORF"', block)
        self.assertNotIn('printf \'%s %s\\n\'', block)

    def test_a_baseline_without_a_cluster_se_is_not_written(self):
        """The matched negative: a promotion whose anchor run yielded no
        cluster SE must leave $ANCHORF alone and say so, rather than write a
        two-field baseline that the next iteration cannot gate against."""
        src = loop_src()
        promoted = src.index('say "  PROMOTED it$it')
        block = src[src.index("promote=0"):promoted]
        self.assertIn('[ -n "$cse" ]', block)
        self.assertIn("not re-seeding", block)

    def test_the_seed_path_also_writes_a_cluster_se(self):
        """$ANCHORF has two writers -- the promotion path above and the
        one-shot seed on a fresh box.  A seed that wrote two fields would make
        arm B fail closed forever on a new machine."""
        src = loop_src()
        seed = src[src.index("no incumbent anchor on record"):
                   src.index("The next iteration number is")]
        self.assertIn('ss=$(sfield se_cluster "$AS")', seed)
        self.assertIn('printf \'%s %s %s\\n\' "$sw" "$sc" "$ss"', seed)
        self.assertIn('[ -n "$ss" ]', seed)


@unittest.skipIf(AWK is None, "awk not available; cannot execute the extracted rules")
class CommentRowsAreAnnotationsNotObservations(unittest.TestCase):
    """Arm 5: a '#' row in curve.tsv is prose, and prose is not data.

    Some events make the rows either side of them incomparable -- commit
    96a5db2 repriced effects.culture/effects.science in weighted.py, which
    changed how the FROZEN champion plays and so put every anchor score
    measured after it on a different ruler from the ones before.  The record
    of that has to live in the curve itself, or the next reader plots one
    continuous line through a discontinuity and reads a trend that no
    measurement supports.

    A comment row only stays safe if it is inert in both places the loop
    touches the file, so both are extracted and executed here rather than
    pinned as text:

      * the iteration counter counts observations, not lines -- otherwise
        every marker punches a hole in the sequence and a missing iteration
        number reads as a crash;
      * the schema migration passes it through verbatim -- otherwise the
        first migration pads the prose out to 13 tab-separated fields.
    """

    def awk(self, prog, text, argv=()):
        r = subprocess.run([AWK] + list(argv) + [prog],
                           input=text, capture_output=True, text=True)
        self.assertEqual(r.returncode, 0, r.stderr)
        return r.stdout

    def counter(self):
        src = loop_src()
        m = re.search(r"start_it=\$\(\( \$\(awk '([^']*)'", src)
        self.assertIsNotNone(m, "could not find the start_it awk program")
        return m.group(1)

    def migration(self):
        src = loop_src()
        m = re.search(r"'(BEGIN\{FS=OFS=\"\\t\"; print hdr\}[^']*)'", src)
        self.assertIsNotNone(m, "could not find the migration awk program")
        return m.group(1)

    HDR = "iter\tpromoted\twin\n"
    ROWS = "1\t1\t0.71\n2\t0\t0.60\n3\t1\t0.55\n"
    MARK = "# engine changed here; anchor re-seeded\n"

    def next_iter(self, text):
        return int(self.awk(self.counter(), text).strip()) + 1

    def test_negative_control_the_old_line_count_miscounts_a_marked_curve(self):
        # This is the bug the fix exists to prevent, stated as the old rule:
        # NR-1 over a curve with one marker says the next iteration is 5 when
        # only four have ever run.
        old = self.awk("END{print NR-1}", self.HDR + self.ROWS + self.MARK)
        self.assertEqual(int(old.strip()) + 1, 5)

    def test_positive_control_the_shipped_counter_skips_the_marker(self):
        self.assertEqual(self.next_iter(self.HDR + self.ROWS + self.MARK), 4)

    def test_the_counter_is_unchanged_on_a_curve_with_no_markers(self):
        # The fix must not move the numbering of the curves already on disk.
        self.assertEqual(self.next_iter(self.HDR + self.ROWS), 4)

    def test_an_empty_or_absent_curve_still_starts_at_one(self):
        # awk over no input leaves n unset; the loop clamps, but the arithmetic
        # must not blow up or come back positive.
        self.assertLessEqual(self.next_iter(""), 1)

    def test_markers_anywhere_in_the_file_are_all_skipped(self):
        text = self.HDR + self.MARK + "1\t1\t0.71\n" + self.MARK + "2\t0\t0.60\n"
        self.assertEqual(self.next_iter(text), 3)

    def test_the_migration_leaves_a_marker_verbatim_instead_of_padding_it(self):
        out = self.awk(self.migration(), self.HDR + "1\t1\n" + self.MARK,
                       ("-v", "hdr=iter\tpromoted\twin", "-v", "n=3",
                        "-v", "nul=-"))
        lines = out.rstrip("\n").split("\n")
        self.assertEqual(lines[-1], self.MARK.rstrip("\n"))
        self.assertNotIn("\t-", lines[-1])
        # and the real rows are still padded to the new width
        self.assertEqual(lines[1], "1\t1\t-")


if __name__ == "__main__":
    unittest.main()
