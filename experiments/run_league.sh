#!/bin/bash
# Supervisor for a long LEAGUE hill-climbing run (pool-scored training).
#
#   experiments/run_league.sh PLAYERS HOURS WORKERS LAMBDA BLOCK SUBSET [ACCEPT_Z] [EXTRA...]
#
# Same detached pattern as run_hillclimb.sh, and for the same reasons: the
# climber is restarted every hour until the budget is spent.  Restarting is
# free (champion, run state, generation log, full-check log and ablation log
# are all on disk after every generation), and it buys two things -- a crash
# costs at most one generation, and every restart picks up the latest engine
# code AND the latest engine/bots/variants/, so new strategy variants join
# the pool without touching the running job.
#
# Launch detached so it survives the agent being killed (the Discord bridge
# restarts constantly):
#
#   nohup experiments/run_league.sh 2 48 6 2 12 4 >/dev/null 2>&1 &
#
# Watch it:  tail -f experiments/logs/league_2p.log
set -u
cd "$(dirname "$0")/.."
export TTA_JOURNAL=1        # docs/PYPY.md 9.14-9.16: 1.44x on WeightedBot
K=${1:-2}; H=${2:-8}; W=${3:-6}; L=${4:-2}; B=${5:-12}; S=${6:-4}; Z=${7:-1.2816}
shift $(( $# > 7 ? 7 : $# ))
mkdir -p experiments/logs
LOG=experiments/logs/league_${K}p.log
# The zero-game stop sentinel (docs/HAZARDS.md trap 8).  The climber writes
# it when a whole generation completed ZERO games, which means the engine is
# broken and every further hour is wasted.  Restarting is what this script does
# for a living, so the halt has to be a file this loop checks -- otherwise the
# loop, and the 10-minute cron watchdog above it, would undo the halt within
# seconds.  Written by experiments/hillclimb_league.py:stop_path().
STOP=experiments/logs/stop_league_${K}p.json
END=$(python3 -c "import time,sys; print(time.time()+float(sys.argv[1])*3600)" "$H")
echo "=== league ${K}p started $(date) budget ${H}h workers=$W lambda=$L block=$B subset=$S z=$Z $* ===" >> "$LOG"
while python3 -c "import time,sys; sys.exit(0 if time.time() < float(sys.argv[1]) else 1)" "$END"; do
    if [ -f "$STOP" ]; then
        echo "=== league ${K}p HALTED $(date): $STOP exists -- a generation" \
             "completed ZERO games.  \`cat\` it for the exception census;" \
             "fix the engine and delete it, and the cron watchdog relaunches" \
             "this arm within 10 minutes. ===" >> "$LOG"
        exit 0
    fi
    T0=$SECONDS
    python3 -m experiments.hillclimb_league --players "$K" --hours 1 \
        --workers "$W" --lambda "$L" --block "$B" --subset "$S" \
        --accept-z "$Z" "$@" >> "$LOG" 2>&1
    RC=$?
    DT=$(( SECONDS - T0 ))
    echo "--- league climber exited ($RC) after ${DT}s, restarting $(date) ---" >> "$LOG"
    # A near-instant exit means the engine does not import (another agent is
    # mid-edit).  Back off hard so we do not spin, but keep retrying: the next
    # restart picks up the repaired engine automatically.
    if [ "$DT" -lt 60 ]; then sleep 60; else sleep 3; fi
done
echo "=== league ${K}p finished $(date) ===" >> "$LOG"
