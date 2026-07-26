#!/bin/bash
# Snapshot every champion's absolute strength against the fixed reference bots.
#
#   experiments/measure_champions.sh [GAMES] [WORKERS]
#
# Appends one result row per matchup to experiments/baselines.jsonl and logs to
# experiments/logs/measure.log.  Safe to run while the climbs are running --
# keep WORKERS at 1 so it does not steal their cores.
#
# The engine is edited by another agent while this runs, so every matchup waits
# for the engine to import and is retried up to 3 times; a matchup that still
# fails is logged and skipped rather than aborting the sweep.
set -u
cd "$(dirname "$0")/.."
G=${1:-96}; W=${2:-1}
LOG=experiments/logs/measure.log
mkdir -p experiments/logs
echo "=== champion measurement $(date) games=$G workers=$W ===" >> "$LOG"

wait_for_engine() {
    for _ in $(seq 1 30); do
        python3 -c "import engine.game, engine.effects" >/dev/null 2>&1 && return 0
        echo "--- engine does not import, waiting 30s $(date)" >> "$LOG"
        sleep 30
    done
    return 1
}

for K in 2 3 4; do
  for B in random greedy default; do
    for TRY in 1 2 3; do
      wait_for_engine || { echo "!!! engine never imported, giving up" >> "$LOG"; exit 1; }
      if python3 -m experiments.evaluate --a experiments/champion_${K}p.json \
            --b "$B" --games "$G" --players "$K" --workers "$W" \
            --out experiments/baselines.jsonl >> "$LOG" 2>&1; then
        break
      fi
      echo "--- ${K}p vs $B failed (try $TRY), retrying in 30s" >> "$LOG"
      sleep 30
    done
  done
done
echo "=== done $(date) ===" >> "$LOG"
