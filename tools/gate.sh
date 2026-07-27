#!/bin/bash
# The verification gate for the journal/undo work (docs/PYPY.md section 6).
#
#   58 unit tests green
#   narrow fingerprint == 6f5c72ef...   (33 games)
#   wide   fingerprint == a966d158...   (102 games)
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

# Re-derived on master 4886b65 (2026-07-26), per docs/PYPY.md 9.0's rule:
# compute from scratch on a clean detached worktree of master AND
# independently on this worktree, and require the two to agree -- agreement
# is the proof, not either number alone.
#
# NARROW (6f5c72ef) is unchanged from the 6d0247c derivation (9.6): none of
# its 3 greedy seeds happen to touch a combat rules interaction. WIDE moved,
# 7814c5c9 -> a966d158: 33bd156 ("Fix three combat rules bugs found by the
# audit", between 6d0247c and 4886b65) changed engine/actions.py and
# engine/effects.py -- real war/pact/aggression rules fixes that GreedyBot's
# own evaluation goes through -- and the wider 10-seed greedy set is large
# enough to catch a game where one of the three bugs used to fire. This is a
# legitimate behaviour change, not a broken gate; verified as the cause by
# `git log 6d0247c..4886b65 -- engine/ ':!engine/bots'` naming exactly that
# commit as the only non-journal/non-perf-check engine change in range.
NARROW=6f5c72ef
WIDE=a966d158

# The greedy fingerprint above plays GreedyBot ONLY, which is exactly why four
# master rebases left it untouched (9.0/9.6) -- and exactly why it can never
# catch a change to WeightedBot, the bot the league actually trains (9.14).
# These two are the same 33/102 split played by WeightedBot instead.
#
# Re-derived on master 4886b65 (2026-07-26), same two-sided discipline as
# above. Both moved from the dff85378/477d1c1f pair derived at 6d0247c:
# e990920 replaced WeightedBot's default `lateness()` schedule (the
# turns-remaining horizon fix) well after 6d0247c, and that is exactly the
# function every WeightedBot feature reads to price a card/action against the
# turns remaining -- there was never a chance the old pair would still match.
WNARROW=b943e1a6
WWIDE=540c3f97

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
