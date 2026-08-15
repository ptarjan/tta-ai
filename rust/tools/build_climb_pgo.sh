#!/bin/bash
# Build the league's `climb` binary with profile-guided optimisation.
#
# Measured 2026-08-10 on the Mac mini, quiet box, identical 2400-game 2p
# arena, three runs each (best of three):
#   baseline release              43.4s
#   -C target-cpu=native          43.7s   <- no effect, this code does not vectorise
#   PGO (this script)             38.4s   <- ~11% faster
# PGO is the only compiler-side lever that moved: the engine is branchy
# game-tree code, so the win comes from block layout and inlining decisions,
# not from wider arithmetic.
#
#   ./tools/build_climb_pgo.sh            build with the checked-in profile
#   ./tools/build_climb_pgo.sh --regen    re-collect the profile first
#
# Re-collect after a change that moves the hot paths around. A stale profile
# is not *wrong* -- rustc just optimises for the old shape -- so this is a
# performance chore, never a correctness one.
#
# BUT: measured 2026-08-10, re-collecting is not automatically an improvement,
# and `--regen` is not a thing to run "to be safe". After the `effects::compute`
# baseline hoist moved the hottest function's shape, same 2400-game arena:
#   hoisted code + profile from the PRE-hoist code:   40.6s
#   hoisted code + profile re-collected on it:        44.3s   <- WORSE, and the
#       three runs clustered 44.3/45.4/46.0, so it is not noise.
# The checked-in profile is therefore the PRE-hoist one, deliberately. Treat a
# regen as an experiment to be measured against the profile it replaces, not as
# maintenance -- keep the old .profdata until the new one has beaten it.
set -eu
export PATH="$HOME/.cargo/bin:$PATH"
cd "$(dirname "$0")/.."

PROFDATA="$PWD/pgo/climb.profdata"
PROFDATA_BIN=$(ls ~/.rustup/toolchains/*/lib/rustlib/x86_64-apple-darwin/bin/llvm-profdata | head -1)

if [ "${1:-}" = "--regen" ]; then
    RAW=/private/tmp/pgo-data-regen
    rm -rf "$RAW"
    echo "collecting profile (instrumented build is several times slower)..."
    RUSTFLAGS="-Cprofile-generate=$RAW" cargo build --release --bin arena
    # Both 2p and 4p so the profile is not overfitted to one seat count; the
    # sample only needs to see each hot path, not settle a strength question.
    ./target/release/arena --players 2 --seed 77 --games 600 --threads 6 > /dev/null
    ./target/release/arena --players 4 --seed 41 --games 300 --threads 6 > /dev/null
    "$PROFDATA_BIN" merge -o "$PROFDATA" "$RAW"
    echo "wrote $PROFDATA"
fi

[ -f "$PROFDATA" ] || { echo "no profile at $PROFDATA -- run with --regen" >&2; exit 1; }
RUSTFLAGS="-Cprofile-use=$PROFDATA" cargo build --release --bin climb
echo "built target/release/climb with PGO"
echo "the league picks it up when an arm next re-executes (--hours 6), or"
echo "restart now:  touch experiments/logs/stop_rust_league_{2,3,4}p &&"
echo "              bash experiments/rust_league.sh   # enforces the sentinel"
