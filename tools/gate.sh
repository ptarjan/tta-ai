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

NARROW=c2befef1
WIDE=47e06a41

fail=0
note() { printf '%-32s %s\n' "$1" "$2"; }

check_fp() {   # name  want-prefix  paranoid(0|1)  [extra perf_check args]
  local name="$1" want="$2" par="$3"; shift 3
  local got
  if [ "$par" = 1 ]; then
    got=$(FASTCOPY_PARANOID=1 nice -n 10 python3 -m engine.perf_check hash "$@" 2>&1 \
          | awk '/^FINGERPRINT/{print $2}')
  else
    got=$(nice -n 10 python3 -m engine.perf_check hash "$@" 2>&1 \
          | awk '/^FINGERPRINT/{print $2}')
  fi
  case "$got" in
    "$want"*) note "$name" "OK   ${got:0:16}" ;;
    *)        note "$name" "FAIL ${got:0:16} != ${want}..."; fail=1 ;;
  esac
}

out=$(nice -n 10 python3 -m unittest discover -s tests 2>&1 | tail -4)
if echo "$out" | grep -q '^OK'; then
  note "unittest" "OK   $(echo "$out" | grep -o 'Ran [0-9]* tests')"
else
  note "unittest" "FAIL"; echo "$out"; fail=1
fi

check_fp "narrow fingerprint"        "$NARROW" 0
check_fp "narrow FASTCOPY_PARANOID"  "$NARROW" 1
if [ "${1:-}" != "--fast" ]; then
  check_fp "wide fingerprint"        "$WIDE" 0 --wide
  check_fp "wide FASTCOPY_PARANOID"  "$WIDE" 1 --wide
fi

if [ "$fail" = 0 ]; then echo "GATE PASS"; else echo "GATE FAIL"; fi
exit $fail
