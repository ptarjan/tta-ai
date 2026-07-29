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
#     accept and never rewritten, so it cannot tear-read a live arm's state;
#   * `proxy_check.py` holds a lock, so two arms never measure at once.
#
# Most invocations do nothing: an arm is only measured every N accepted
# champions (or after --max-hours, so a slow arm still produces a series).
#
# It keeps running for 12h past the arms' own deadline so the FINAL champion
# of each arm gets a reading -- the last accept is the one you most want
# validated, and it usually lands near the end.
set -u
cd "$(dirname "$0")/.."
DEADLINE_FILE=experiments/logs/watchdog_deadline
LOG=experiments/logs/proxy_watch.log
[ -f "$DEADLINE_FILE" ] || exit 0
DEADLINE=$(cat "$DEADLINE_FILE")
NOW=$(date +%s)
if [ "$NOW" -ge $(( DEADLINE + 43200 )) ]; then exit 0; fi

for K in 2 3 4; do
    nice -n 19 python3 -m experiments.proxy_check --players "$K" >> "$LOG" 2>&1
done
