#!/usr/bin/env bash
# Phase-A probe: is the value net worth more as the LEAF OF A BEAM than as a
# 1-ply argmax, and AT WHAT WIDTH?
#
# The 41-hour Stage-2 null (docs/NEURAL_LOOP_NULL.md) killed the "make the
# 1-ply net better by self-play against its own argmax" branch.  Before
# rebuilding the loop around a search-backed target it is worth one cheap
# battery that says whether the search is actually where the points are for
# THIS net, because docs/PLAN_WAR_LOOKAHEAD.md 5 measured the beam at +0.29 win
# on the P vector and a flat 0.000 on the Q vector: the lift is EVALUATOR-
# dependent and must be measured, not assumed.  A single-game spot check
# already suggested it is not monotone in width here (width=1 scored ~120
# culture, width=8 scored 0), which is the classic "search amplifies evaluator
# error" signature and is exactly what the width ladder below measures.
#
# Every row is one process, CPU, ONE torch thread, below-normal priority,
# honouring the guard's PAUSE flag.  Keep PARALLEL small: the box also runs
# the league arms, and 8 unthrottled torch processes measured 0.25 core each.
#
# Usage: bash experiments/nplan_probe.sh [N_GAMES] [TAG]
set -u
trap '' HUP
cd ~/tta-ai || exit 1
PY=/c/Users/micro/AppData/Local/Programs/Python/Python312/python.exe
export PYTHONPATH=.
export NEURAL_DEVICE=cpu

N=${1:-40}
TAG=${2:-a}
CK=${CK:-checkpoints/best.pt}
CHAMP=analysis/frozen/champion_2p.json
OUT=probe
mkdir -p "$OUT"
PAUSE=~/tta-ai/PAUSE

run() {  # run NAME args...
  local name=$1; shift
  while [ -f "$PAUSE" ]; do sleep 30; done
  echo "[$(date +%H:%M:%S)] start $name" >&2
  "$PY" experiments/neural_eval.py --ckpt "$CK" --players 2 --device cpu \
      --threads 1 --report 5 --games "$N" "$@" > "$OUT/${TAG}_$name.log" 2>&1
  echo "[$(date +%H:%M:%S)] done  $name: $(grep -h '^SUMMARY' "$OUT/${TAG}_$name.log")" >&2
}

# --- the search ladder on ONE evaluator: this is the whole question ---------
run w8_vs_1ply  --search plan --width 8 --nodes 1200 --opponent "neural:$CK" &
run w2_vs_1ply  --search plan --width 2 --nodes 300  --opponent "neural:$CK" &
run w1_vs_1ply  --search plan --width 1 --nodes 200  --opponent "neural:$CK" &
wait
# --- controls and yardsticks ------------------------------------------------
run mirror_w2   --search plan --width 2 --nodes 300 --opponent "nplan:$CK" &
run w2_vs_book  --search plan --width 2 --nodes 300 --opponent book &
run w2_vs_champ --search plan --width 2 --nodes 300 --opponent "$CHAMP" &
wait
echo "== PROBE DONE $(date) =="
grep -H "^SUMMARY" "$OUT/${TAG}"_*.log
