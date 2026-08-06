#!/bin/bash
# Keep the three Rust league arms alive unattended.  Runs from cron every 10
# minutes; each pass relaunches any arm that is not running.
#
# This replaces watchdog.sh + run_league.sh, and is a tenth of their size for
# one structural reason: the Rust climber persists everything it needs to
# resume -- champion, generation, sigma, since_accept, its standing against the
# anchor -- INSIDE its own checkpoint.  The Python pair had to re-supply a list
# of flags on every relaunch that were deliberately not persisted, and an arm
# relaunched without one of them kept training against a different, weaker
# configuration with nothing crashing and nothing in the logs saying so.  That
# whole hazard class is gone: there is no flag here whose absence changes what
# is being trained, only how long and how fast.
#
# The one thing that is NOT in the checkpoint is the anchor, and that is on
# purpose -- see rust/src/bin/climb.rs.  The anchor is the built-in default
# vector, a compile-time constant, so it cannot drift between relaunches and
# there is nothing to re-supply.
#
# Stop an arm by touching experiments/logs/stop_rust_league_Kp.  The sentinel
# stops a RUNNING arm as well as a relaunch: a stop that only took effect at
# the next natural exit would be a stop you cannot rely on.
set -u
cd "$(dirname "$0")/.."

CLIMB=rust/target/release/climb
LOGDIR=experiments/logs
LOG=$LOGDIR/rust_league.log

mkdir -p "$LOGDIR"

log() { printf '%s %s\n' "$(date '+%F %T')" "$*" >>"$LOG"; }

if [ ! -x "$CLIMB" ]; then
    log "no $CLIMB -- build it with: cd rust && cargo build --release"
    exit 1
fi

# Six physical cores, three arms.  Two threads each leaves the box responsive
# and keeps all three arms progressing at the same rate, which matters because
# a champion is only comparable against its own player count.
THREADS=2

# The frozen gauntlet (docs/RUST_LEAGUE.md "The anchor number is not a
# strength ladder", analysis/frozen/gauntlet/README.md): dated past champions
# that never move, reported alongside the anchor because the anchor itself
# saturated long ago.  Every arm plays all three, 2p included -- the finding
# that motivated this was a *cross*-player-count matchup (the 3p vector,
# seated at a 2p table).  Purely observational: it cannot veto or accept a
# generation, only get logged.  At the cadence and game count `climb`
# defaults to (--gauntlet-every 50, --gauntlet-games 60), this costs about
# 3.6 games/generation amortized -- see that doc section for the arithmetic.
GAUNTLET_2P=analysis/frozen/gauntlet/champion_2p_gen1454_140key_2026-08-06.json
GAUNTLET_3P=analysis/frozen/gauntlet/champion_3p_gen1384_140key_2026-08-06.json
GAUNTLET_4P=analysis/frozen/gauntlet/champion_4p_gen448_140key_2026-08-06.json

for PLAYERS in 2 3 4; do
    SENTINEL=$LOGDIR/stop_rust_league_${PLAYERS}p
    # `-f` matches the whole command line, so `--players N` is what
    # distinguishes the three arms from each other.
    PIDS=$(pgrep -f "climb --players $PLAYERS" || true)

    if [ -e "$SENTINEL" ]; then
        if [ -n "$PIDS" ]; then
            log "[${PLAYERS}p] sentinel present -- stopping $PIDS"
            kill $PIDS
        fi
        continue
    fi

    if [ -n "$PIDS" ]; then
        continue
    fi

    OUT=experiments/rust_champion_${PLAYERS}p.json
    log "[${PLAYERS}p] launching (champion $OUT)"
    # --hours 6 rather than forever: a process that re-executes periodically
    # picks up a rebuilt binary without anyone having to remember to restart
    # it, and re-reads its own checkpoint on the way in, which is the path
    # worth exercising regularly rather than only after a crash.
    nohup "$CLIMB" \
        --players "$PLAYERS" \
        --out "$OUT" \
        --log "$LOGDIR/rust_climb_${PLAYERS}p.jsonl" \
        --threads "$THREADS" \
        --hours 6 \
        --seed "$PLAYERS" \
        --gauntlet "$GAUNTLET_2P" \
        --gauntlet "$GAUNTLET_3P" \
        --gauntlet "$GAUNTLET_4P" \
        >>"$LOGDIR/rust_league_${PLAYERS}p.log" 2>&1 &
done
