#!/bin/bash
# Supervisor for a long hill-climbing run.
#
#   experiments/run_hillclimb.sh PLAYERS HOURS WORKERS LAMBDA SCREEN MAXGAMES
#
# Restarts the climber every hour until the budget is spent.  Restarting is
# free (the champion and the generation log are on disk) and has two upsides:
# a crash -- e.g. an engine bug hit by an exotic line of play -- costs at most
# one generation, and every restart picks up the latest engine code.
set -u
cd "$(dirname "$0")/.."
K=${1:-4}; H=${2:-8}; W=${3:-2}; L=${4:-1}; S=${5:-48}; M=${6:-288}
mkdir -p experiments/logs
LOG=experiments/logs/hc_${K}p.log
END=$(python3 -c "import time,sys; print(time.time()+float(sys.argv[1])*3600)" "$H")
echo "=== hillclimb ${K}p started $(date) budget ${H}h workers=$W lambda=$L ===" >> "$LOG"
while python3 -c "import time,sys; sys.exit(0 if time.time() < float(sys.argv[1]) else 1)" "$END"; do
    python3 -m experiments.hillclimb --players "$K" --hours 1 --workers "$W" \
        --lambda "$L" --screen "$S" --max-games "$M" >> "$LOG" 2>&1
    echo "--- climber exited ($?), restarting $(date) ---" >> "$LOG"
    sleep 3
done
echo "=== hillclimb ${K}p finished $(date) ===" >> "$LOG"
