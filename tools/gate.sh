#!/bin/bash
# The verification gate for the journal/undo work (docs/PYPY.md section 6).
#
#   58 unit tests green
#   narrow fingerprint == c2befef1...   (33 games)
#   wide   fingerprint == 47e06a41...   (102 games)
#   all of the above unchanged under FASTCOPY_PARANOID=1
#
# NOTE: do NOT use `python3 -m engine.perf_check check tools/fingerprint.json`.
# Those JSON files are STALE (last saved at commit 7c2eef1, digests 3229c4a0 /
# c7e73ede) and legitimate behaviour changes landed after them without a
# re-save.  The digests this project actually gates on are the ones written
# down in docs/PYPY.md, which is what this script compares against.
#
#   bash tools/gate.sh            # tests + both fingerprints, plain and paranoid
#   bash tools/gate.sh --fast     # tests + narrow only (quick inner loop)
set -u
cd "$(dirname "$0")/.."

# Re-derived on master 6d0247c (docs/PYPY.md 9.6).  The previous pair
# (c2befef1 / 47e06a41) was correct up to master 15b9764; 6d0247c's effects.py
# clamps every rating at 0 rather than only happiness, which GreedyBot's
# evaluation goes through, so the digests legitimately moved.  Both numbers
# below were computed from scratch on a clean master worktree AND on this
# branch, and agree -- which is the only thing that makes them trustworthy.
NARROW=6f5c72ef
WIDE=7814c5c9

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
if [ "${1:-}" != "--fast" ]; then
  check_fp "wide fingerprint"        "$WIDE" "" --wide
  check_fp "wide FASTCOPY_PARANOID"  "$WIDE" "FASTCOPY_PARANOID=1" --wide
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
fi

if [ "$fail" = 0 ]; then echo "GATE PASS"; else echo "GATE FAIL"; fi
exit $fail
