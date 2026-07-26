#!/bin/bash
# Snapshot every champion's absolute strength against the fixed reference bots.
#   experiments/measure_champions.sh [GAMES] [WORKERS]
# Appends to experiments/baselines.jsonl and echoes one line per matchup to
# experiments/logs/measure.log.  Safe to run while the climbs are running --
# keep WORKERS small so it does not steal their cores.
set -u
cd "$(dirname "$0")/.."
G=${1:-96}; W=${2:-1}
LOG=experiments/logs/measure.log
echo "=== champion measurement $(date) games=$G workers=$W ===" >> "$LOG"
for K in 2 3 4; do
  for B in random greedy default; do
    python3 -m experiments.evaluate --a experiments/champion_${K}p.json --b "$B" \
        --games "$G" --players "$K" --workers "$W" \
        --out experiments/baselines.jsonl >> "$LOG" 2>&1
  done
done
echo "=== done $(date) ===" >> "$LOG"
