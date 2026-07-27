#!/bin/bash
# The verification gate for the journal/undo work (docs/PYPY.md section 6).
#
#   248 unit tests green
#   narrow fingerprint == 2fd656b3...   (33 games)
#   wide   fingerprint == 1169007d...   (102 games)
#   all of the above unchanged under FASTCOPY_PARANOID=1
#
# tools/fingerprint.json / tools/fingerprint_wide.json are now IN SYNC with
# the values below (re-saved via `perf_check save`) -- `python3 -m
# engine.perf_check check tools/fingerprint.json` is safe to use again. The
# digests this script gates on are still the ones written down here, not
# re-read from those files, so this comment (and PYPY.md, where this table is
# mirrored) remains the thing to update whenever a digest legitimately moves.
#
#   bash tools/gate.sh            # tests + both fingerprints, plain and paranoid
#   bash tools/gate.sh --fast     # tests + narrow only (quick inner loop)
set -u
cd "$(dirname "$0")/.."

# Re-derived on master 3439b0e (2026-07-26), per docs/PYPY.md 9.0's rule:
# compute from scratch on a clean detached worktree of master AND
# independently on a second worktree, and require the two to agree --
# agreement is the proof, not either number alone. Both narrow and wide moved
# this time (previously it was WIDE only, and NARROW's 3-seed greedy set was
# assumed too small to be touched -- this round it WAS touched, so nothing
# was assumed and all four were re-derived from scratch).
#
# Cause: commit 7315494 ("Coverage audit: census + variance instruments, 3
# rulebook fixes", see docs/COVERAGE_AUDIT.md Sec 2) changed engine/actions.py
# with two real rules fixes both bots' evaluation goes through --
# (1) `_h_revolution` no longer discards the actions the new government
# grants (Sec 2.1: a revolt from Despotism to Monarchy now correctly yields
# 3 military actions, not 2 -- Revolution has a 30-65% take-rate, so this
# moves many games), and (2) the one-per-name rule is no longer applied to
# yellow action cards, which exist in 2-3 copies per deck (Sec 2.2). Verified
# as the only behaviour-affecting change in range: `git diff 4886b65..3439b0e
# --stat` touches engine/actions.py only inside engine/; everything else in
# range (experiments/arena.py's degenerate-champion guard, experiments/
# summarize.py's feature grouping, the new standalone tools/coverage_census.py
# and tools/feature_variance.py) is additive/reporting-only and not on the
# perf_check hash path.
NARROW=2fd656b3
WIDE=1169007d

# The greedy fingerprint above plays GreedyBot ONLY, which is exactly why four
# master rebases left it untouched (9.0/9.6) -- and exactly why it can never
# catch a change to WeightedBot, the bot the league actually trains (9.14).
# These two are the same 33/102 split played by WeightedBot instead.
#
# Re-derived on master 3439b0e (2026-07-26), same two-sided discipline and the
# same cause (7315494) as above -- both bots' evaluation reads
# `_h_revolution`/`_can_take_gated` in engine/actions.py.
WNARROW=a7691eaa
WWIDE=c7045ab1

fail=0
note() { printf '%-32s %s\n' "$1" "$2"; }

# NOTE: /bin/bash on macOS is 3.2.  An earlier version of these two helpers
# collected the env assignments into an array (`envs+=(...)`, `"${envs[@]}"`);
# under 3.2 that silently produced garbled output and a spurious GATE FAIL on
# a tree whose digests were provably correct when the same command was run by
# hand.  A gate that cries wolf is worse than no gate -- see 9.0 for how much
# damage a misleading gate reading does on this project -- so both helpers now
# take the environment as ONE plain string and there are no arrays anywhere.

check_fp() {   # name  want-prefix  "ENV=1 ENV2=2"  [perf_check args...]
  local name="$1" want="$2" envstr="$3"; shift 3
  local got
  got=$(env $envstr nice -n 10 python3 -m engine.perf_check hash "$@" 2>&1 \
        | awk '/^FINGERPRINT/{print $2}')
  case "$got" in
    "$want"*) note "$name" "OK   ${got:0:16}" ;;
    *)        note "$name" "FAIL ${got:0:16} != ${want}..."; fail=1 ;;
  esac
}

run_tests() {   # name  "ENV=1"
  local name="$1" envstr="$2"
  local out
  out=$(env $envstr nice -n 10 python3 -m unittest discover -s tests 2>&1 | tail -4)
  if echo "$out" | grep -q '^OK'; then
    note "$name" "OK   $(echo "$out" | grep -o 'Ran [0-9]* tests')"
  else
    note "$name" "FAIL"; echo "$out"; fail=1
  fi
}

run_tests "unittest" ""
# The suite again with the journal checking itself against a copy_state oracle
# on every rollback.  This is nearly free (the tests are seconds) and it is a
# real arm: a test that performs an unjournalled container mutation passes
# here-but-not-there, which is precisely the bug class this branch exists to
# prevent.  Keeping the suite paranoid-clean is what makes it usable as a
# check rather than merely as a test.
run_tests "unittest JOURNAL_PARANOID" "JOURNAL_PARANOID=1"

check_fp "narrow fingerprint"        "$NARROW" ""
check_fp "narrow FASTCOPY_PARANOID"  "$NARROW" "FASTCOPY_PARANOID=1"
check_fp "weighted narrow"           "$WNARROW" "" --weighted
if [ "${1:-}" != "--fast" ]; then
  check_fp "wide fingerprint"        "$WIDE" "" --wide
  check_fp "wide FASTCOPY_PARANOID"  "$WIDE" "FASTCOPY_PARANOID=1" --wide
  check_fp "weighted wide"           "$WWIDE" "" --weighted --wide
fi

# The journal arms.  These only mean anything once step 5 has converted every
# module, so they are opt-in until then -- but when they do run they are the
# strongest check in the file: TTA_JOURNAL=1 makes GreedyBot search by undo
# instead of by copy, and JOURNAL_PARANOID=1 additionally copies the state,
# rolls back, and structurally diffs the two on EVERY candidate move.  A
# missed mutation site raises there, naming the attribute path, rather than
# showing up as a digest mismatch with no clue attached.
if [ "${1:-}" = "--journal" ]; then
  check_fp "narrow JOURNAL"            "$NARROW" "TTA_JOURNAL=1"
  check_fp "narrow JOURNAL+PARANOID"   "$NARROW" "TTA_JOURNAL=1 JOURNAL_PARANOID=1"
  check_fp "wide JOURNAL"              "$WIDE" "TTA_JOURNAL=1" --wide
  check_fp "wide JOURNAL+PARANOID"     "$WIDE" "TTA_JOURNAL=1 JOURNAL_PARANOID=1" --wide
  # The same four arms with WEIGHTED searching.  9.14 showed the two bots do
  # not cover the same mutation sites -- WeightedBot reaches the refused-pact
  # site (interact.py:228) that 60 games of GreedyBot never execute -- so these
  # are not a duplicate of the arms above, they are the other half of the
  # coverage claim.
  check_fp "weighted narrow JOURNAL"          "$WNARROW" "TTA_JOURNAL=1" --weighted
  check_fp "weighted narrow JOURNAL+PARANOID" "$WNARROW" "TTA_JOURNAL=1 JOURNAL_PARANOID=1" --weighted
  check_fp "weighted wide JOURNAL"            "$WWIDE" "TTA_JOURNAL=1" --weighted --wide
  check_fp "weighted wide JOURNAL+PARANOID"   "$WWIDE" "TTA_JOURNAL=1 JOURNAL_PARANOID=1" --weighted --wide
fi

if [ "$fail" = 0 ]; then echo "GATE PASS"; else echo "GATE FAIL"; fi
exit $fail
