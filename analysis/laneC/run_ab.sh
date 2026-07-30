#!/bin/bash
# Lane C A/B: board-aware card pricing on vs off, paired on the deal.
#
# Both arms are the SAME weight vector (analysis/frozen/champion_2p.json)
# differing only in `card_board_credit` (1.0 vs 0.0), so the duel isolates the
# pricing and nothing else.  `experiments.arena.duel` at 2 players plays each
# deal twice with the seats swapped, so the comparison is paired on the deal.
#
# MDE: n = 3200 games gives SE ~= 0.88pp on the win rate, so the minimum
# detectable effect at 80% power and alpha = 0.05 two-sided is ~= 2.5pp.
# Pairing makes the true SE smaller than that, so 2.5pp is conservative.  The
# decomposition arms are n = 1600 (SE ~= 1.25pp, MDE ~= 3.5pp) and can only
# speak to larger effects -- stated up front so a null there is read as
# "smaller than 3.5pp", not as "zero".
#
#   bash analysis/laneC/run_ab.sh main        # 8 x 400, everything on
#   bash analysis/laneC/run_ab.sh government  # 4 x 400, governments alone
#   bash analysis/laneC/run_ab.sh leader      # 4 x 400, leaders alone
set -u
cd "$(dirname "$0")/../.."
arm="${1:-main}"
out="/tmp/laneC_ab_${arm}.jsonl"
: > "$out"
case "$arm" in
  main)   seeds="0 200 400 600 800 1000 1200 1600"; unset TTA_BOARD_TYPES ;;
  *)      seeds="0 200 400 600"; export TTA_BOARD_TYPES="$arm" ;;
esac
for s in $seeds; do
  nice -n 19 python3 -m experiments.evaluate \
    --a analysis/laneC/on.json --b analysis/laneC/off.json \
    --players 2 --games 400 --seed "$s" --workers 2 --out "$out" || exit 1
done
nice -n 19 python3 analysis/laneC/agg.py "$out"
