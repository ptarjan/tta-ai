#!/bin/bash
# WeightedBot determinism matrix. Sequential (one worker) -- the box already
# carries three hill climbs.
set -u
run() {  # label  dir  "ENV"  [args...]
  local label="$1" dir="$2" envstr="$3"; shift 3
  local t0=$SECONDS
  local got
  got=$(cd "$dir" && env $envstr nice -n 15 python3 -m engine.perf_check hash "$@" 2>&1 | awk '/^FINGERPRINT/{print $2}')
  printf '%-46s %s   (%ds)\n' "$label" "${got:-FAILED}" "$((SECONDS-t0))"
}
run "master 6d0247c  weighted narrow(33)"  /Users/pt/tta-ai-mbase   "" --weighted
run "branch           weighted narrow(33)" /Users/pt/tta-ai-journal "" --weighted
run "branch  JOURNAL  weighted narrow(33)" /Users/pt/tta-ai-journal "TTA_JOURNAL=1" --weighted
run "branch  J+PARA   weighted narrow(33)" /Users/pt/tta-ai-journal "TTA_JOURNAL=1 JOURNAL_PARANOID=1" --weighted
run "master 6d0247c  weighted wide(102)"   /Users/pt/tta-ai-mbase   "" --weighted --wide
run "branch           weighted wide(102)"  /Users/pt/tta-ai-journal "" --weighted --wide
run "branch  JOURNAL  weighted wide(102)"  /Users/pt/tta-ai-journal "TTA_JOURNAL=1" --weighted --wide
run "branch  J+PARA   weighted wide(102)"  /Users/pt/tta-ai-journal "TTA_JOURNAL=1 JOURNAL_PARANOID=1" --weighted --wide
echo DONE
