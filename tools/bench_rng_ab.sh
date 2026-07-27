#!/bin/bash
# BASE vs RNG only, longer.  9.14's first pass put the rng fix at +6% (3p) and
# -1% (4p) -- i.e. within noise at 4p, which is section 8.1 repeating itself:
# the profiler put this fix at 10.8% and was wrong about it once already.  So
# it gets its own measurement rather than a footnote.
set -u
G=${G:-16}; W=${W:-4}
arm() { local l="$1" d="$2"; ( cd "$d" && nice -n 15 python3 -m engine.perf_check bench \
    --kinds weighted --players 3,4 --games "$G" --warmup "$W" 2>&1 ) | sed "s/^/$l /"; }
for r in 1 2 3 4 5; do
  echo "===== round $r (games=$G)"
  arm BASE /Users/pt/tta-ai-mbase
  arm RNG  /Users/pt/tta-ai-rngonly
done
echo BENCHDONE
