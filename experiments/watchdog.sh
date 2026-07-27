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
#
# EVERY pool-affecting flag must be repeated here.  A relaunch that silently
# drops one is a failure mode this project has already hit: --candidate-bot is
# not persisted in the state dir (docs/UNATTENDED.md trap 5) and neither are
# --hall-dir or --human-bots, so an arm restarted without them keeps training
# but against a different, weaker pool -- and nothing in the logs says so
# except the [pool] line.  Check that line after any relaunch.
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

# EVERY flag that defines the run lives in ONE array, used by all three
# launches.  This is not style.  `--candidate-bot` is not persisted in the
# state dir (docs/UNATTENDED.md trap 5) and neither is `--objective`: an arm
# that the watchdog relaunches without them silently reverts to a 1-ply bot
# trained on the OLD objective, which is the worst possible failure because
# nothing crashes and the log looks normal.  This project has already hit that
# exact mode once.  One array, no per-arm copies, and the startup lines
# `[Kp] objective: ...` / `[Kp] trained architecture: ...` / `[pool] ...` in
# experiments/logs/league_Kp.log are the receipts to check after a relaunch.
#
# --objective blend    accept on (1-alpha)*own final culture + alpha*win share.
#                      The old default was culture MARGIN, which pays twice for
#                      a culture point stolen in a war and once for one
#                      produced, and selected a champion that scores 64.7
#                      against a human 159.5.  docs/LEAGUE_OBJECTIVE.md.
# --pool-weights       76% of the training signal on opponents that improve
#                      (mirror / past ladder / frozen hall) and 24% on the
#                      static hand-written BookBot family, which was 69%
#                      before.  Passed EXPLICITLY rather than relying on the
#                      module default, so the log records the pool this run
#                      actually used even if the default later moves.
#                      floor=0 drops greedy/random/default: they are saturated
#                      (docs/UNATTENDED.md trap 2) and under own-culture
#                      scoring they stop being inert and start pulling.
COMMON=(
    --weight-guard clamp
    --past-k 2
    --hall-dir experiments/hall_of_fame
    --candidate-bot quiescent:levels=1
    --human-bots all
    --objective blend
    --objective-alpha 0.15
    --pool-weights book=0.6,variant=0.6,human=0.6,mirror=1.0,past=1.2,hall=1.6,floor=0
)

launch() {   # players workers block extra...
    local K=$1 W=$2 B=$3; shift 3
    nohup experiments/run_league.sh "$K" "$REMAIN" "$W" 2 "$B" 4 1.2816 \
        "${COMMON[@]}" "$@" >/dev/null 2>&1 &
    echo "$(date '+%F %T') watchdog: relaunched ${K}p (${REMAIN}h left, workers=$W block=$B) ${COMMON[*]}" >> "$LOG"
}

pgrep -f "run_league.sh 2 " >/dev/null || launch 2 1 12 --init default
pgrep -f "run_league.sh 3 " >/dev/null || launch 3 2 12 --init default
# 4p: block 24 because its per-game spread is 2.8x the 2p spread (FOURP_GAP),
# and warm-started from the 2p champion, which measured 57.4% vs the 4p arm's
# 27.6% at 4p at matched generations.  --init is ignored once state exists, so
# this is only load-bearing on a genuinely fresh state dir -- which is why it
# was left alone by the 2026-07-27 objective change even though the vector it
# names is now known to score 64.7 own culture: the 4p state dir has held a
# champion since 07-26, so this flag has been inert for the whole run and
# changing it would have changed nothing.  See docs/LEAGUE_OBJECTIVE.md 6.
pgrep -f "run_league.sh 4 " >/dev/null || \
    launch 4 2 24 --init experiments/hall_of_fame/preinfo_2p_gen00188.json
