#!/bin/bash
# Keep the three league arms alive unattended.
#
# Runs from cron every 10 minutes.  For each player count it checks whether a
# run_league.sh supervisor is alive and relaunches it if not, until DEADLINE
# passes -- then it stops relaunching and removes its own cron entry's reason
# to exist (the entry is harmless after that; it just no-ops).
#
# Why this exists: the arms are meant to run 48h unattended.  A supervisor can
# die for reasons run_league.sh cannot catch itself -- an OOM kill, a reboot,
# a terminal closing on a stray foreground launch.  Without this, an arm that
# dies at hour 3 is simply gone for the remaining 45.
#
# DEADLINE is an absolute epoch second, written at setup time.  Cron gives no
# clean way to say "stop after N hours", so the deadline lives in a file.
set -u
cd "$(dirname "$0")/.."
DEADLINE_FILE=experiments/logs/watchdog_deadline
LOG=experiments/logs/watchdog.log
[ -f "$DEADLINE_FILE" ] || exit 0
DEADLINE=$(cat "$DEADLINE_FILE")
NOW=$(date +%s)
if [ "$NOW" -ge "$DEADLINE" ]; then exit 0; fi

# Hours remaining, rounded up -- a relaunched supervisor gets only the time
# left on the original budget, not a fresh 48h.
REMAIN=$(( (DEADLINE - NOW + 3599) / 3600 ))
[ "$REMAIN" -lt 1 ] && REMAIN=1

launch() {   # players workers block extra...
    local K=$1 W=$2 B=$3; shift 3
    nohup experiments/run_league.sh "$K" "$REMAIN" "$W" 2 "$B" 4 1.2816 \
        --weight-guard clamp --past-k 2 --hall-dir experiments/hall_of_fame \
        --candidate-bot quiescent:levels=1 "$@" >/dev/null 2>&1 &
    echo "$(date '+%F %T') watchdog: relaunched ${K}p (${REMAIN}h left, workers=$W block=$B)" >> "$LOG"
}

pgrep -f "run_league.sh 2 " >/dev/null || launch 2 1 12 --init default
pgrep -f "run_league.sh 3 " >/dev/null || launch 3 2 12 --init default
# 4p: block 24 because its per-game spread is 2.8x the 2p spread (FOURP_GAP),
# and warm-started from the 2p champion, which measured 57.4% vs the 4p arm's
# 27.6% at 4p at matched generations.  --init is ignored once state exists, so
# this is only load-bearing on a genuinely fresh state dir.
pgrep -f "run_league.sh 4 " >/dev/null || \
    launch 4 2 24 --init experiments/hall_of_fame/preinfo_2p_gen00188.json
