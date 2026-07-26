#!/bin/bash
# End-to-end throughput for WeightedBot at 3p and 4p, three alternating pairs.
#
# Protocol is 9.12's, unchanged: `time.process_time` (this process's own CPU --
# wall clock is meaningless with three hill climbs on the box), nice -n 15, ONE
# worker at a time, and the arms interleaved within each round so any drift in
# machine load hits all three rather than one.  9.12's own warning applies:
# take the mean of the rounds, never a single ratio.
#
# Three arms, so the journal win and the rng win are attributed separately:
#   BASE  master 6d0247c            copy path, random.Random(0) per candidate
#   RNG   3bcae9c                   copy path, shared trial rng
#   JRNL  branch, TTA_JOURNAL=1     undo stack + shared trial rng
set -u
G=${G:-10}
W=${W:-3}
arm() {   # label  dir  "ENV"
  local label="$1" dir="$2" envstr="$3"
  ( cd "$dir" && env $envstr nice -n 15 python3 -m engine.perf_check bench \
      --kinds weighted --players 3,4 --games "$G" --warmup "$W" 2>&1 ) \
    | sed "s/^/$label /"
}
for round in 1 2 3; do
  echo "===== round $round (games=$G warmup=$W)"
  arm BASE /Users/pt/tta-ai-mbase   ""
  arm RNG  /Users/pt/tta-ai-rngonly ""
  arm JRNL /Users/pt/tta-ai-journal "TTA_JOURNAL=1"
done
echo BENCHDONE
