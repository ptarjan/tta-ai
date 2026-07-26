#!/bin/bash
# Collect behavioural statistics for every champion, one player count at a
# time so it never uses more than one core.
#
#   experiments/run_behaviour.sh [GAMES] [OPPONENT]
#
# OPPONENT defaults to `self` (mirror self-play).  Pass `greedy` for a second
# pass that shows how the champion behaves against a *different* policy --
# "military relative to opponents" only means something when the opponents are
# not running the same weights.
set -u
cd "$(dirname "$0")/.."
G=${1:-60}
OPP=${2:-self}
LOG=experiments/logs/behaviour.log
mkdir -p experiments/logs
SUF=""
[ "$OPP" != "self" ] && SUF="_vs_${OPP}"
echo "=== behaviour sweep $(date) games=$G opponent=$OPP ===" >> "$LOG"
for K in 2 3 4; do
  python3 -m experiments.behaviour --players "$K" --games "$G" --workers 1 \
      --opponent "$OPP" \
      --out "experiments/behaviour_${K}p${SUF}.json" >> "$LOG" 2>&1 \
    || echo "!!! ${K}p failed" >> "$LOG"
done
echo "=== done $(date) ===" >> "$LOG"
