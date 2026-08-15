#!/bin/bash
# Run the PROXY GUARDRAIL for each arm when one is due.  From cron, every
# 20 minutes.  See docs/PROXY_GUARDRAIL.md and experiments/proxy_check.py.
#
# The arms climb a number measured under their TRAINING architecture.  That
# number is not known to track strength under the policy we ship: it was
# measured actively inverted (docs/TRANSFER_TEST.md) and is now merely
# uninformative (docs/PLAN_WAR_LOOKAHEAD.md 6).  This script is what turns
# that from a one-off finding into a monitored time series.
#
# It must never cost an arm anything:
#
#   * it is a SEPARATE process on a SEPARATE cron entry -- it cannot block,
#     slow or restart an arm, and if it dies the arms do not notice;
#   * `nice -n 19`, one worker, and a bounded number of deals per reading;
#   * `proxy_check.py` reads only LADDER files, which are written once on
#     accept and never rewritten, so it cannot tear-read a live arm's state.
#
# Most invocations do nothing: an arm is only measured every N accepted
# champions (or after --max-hours, so a slow arm still produces a series).
#
# TWO LOCKS, FOR TWO DIFFERENT REASONS.
#
#   * `proxy_check.lock` (inside proxy_check.py) keeps two MEASUREMENTS from
#     running at once, so the guardrail never takes more than one core.  It is
#     WAITED on, not skipped -- see below.
#   * `proxy_watch.lock` (this file) keeps two INVOCATIONS of this script from
#     overlapping, because an invocation can now spend minutes waiting.
#     `mkdir` is the atomic primitive; macOS /bin/bash has no `flock`.
#
# WHY WAITING MATTERS.  The first version skipped immediately when the
# measurement lock was held.  A neighbouring agent's replication job held it
# every time cron looked, so proxy_watch.log filled up with "another
# measurement holds the lock, skipping" -- six times -- and the guardrail
# never ran.  A monitor that goes quiet when the box is busy is a monitor that
# goes quiet exactly when you need it.  Each arm now waits up to 5 minutes,
# and `--stale-hours` inside proxy_check.py shouts if a lock that never clears
# has left an arm unvalidated.
#
# It keeps running for 12h past the arms' own deadline so the FINAL champion
# of each arm gets a reading -- the last accept is the one you most want
# validated, and it usually lands near the end.
set -u
cd "$(dirname "$0")/.."
DEADLINE_FILE=experiments/logs/watchdog_deadline
LOG=experiments/logs/proxy_watch.log
WATCHLOCK=experiments/logs/proxy_watch.lock
[ -f "$DEADLINE_FILE" ] || exit 0
DEADLINE=$(cat "$DEADLINE_FILE")
NOW=$(date +%s)
if [ "$NOW" -ge $(( DEADLINE + 43200 )) ]; then exit 0; fi

# One watcher at a time.  A lock older than 3h is stale (the holder was
# killed) and is taken over rather than wedging the guardrail forever.
if ! mkdir "$WATCHLOCK" 2>/dev/null; then
    AGE=$(( NOW - $(stat -f %m "$WATCHLOCK" 2>/dev/null || echo "$NOW") ))
    if [ "$AGE" -lt 10800 ]; then
        exit 0
    fi
    echo "$(date '+%F %T') proxy_watch: taking over a ${AGE}s-old watcher lock" >> "$LOG"
    rmdir "$WATCHLOCK" 2>/dev/null
    mkdir "$WATCHLOCK" 2>/dev/null || exit 0
fi
trap 'rmdir "$WATCHLOCK" 2>/dev/null' EXIT

# Rotate which arm goes first, so a slow 4p reading cannot permanently starve
# the arm behind it in a fixed order.
case $(( (NOW / 1200) % 3 )) in
    0) ARMS="2 3 4" ;;
    1) ARMS="3 4 2" ;;
    *) ARMS="4 2 3" ;;
esac

for K in $ARMS; do
    nice -n 19 python3 -m experiments.proxy_check --players "$K" \
        --lock-wait 5 >> "$LOG" 2>&1
done
