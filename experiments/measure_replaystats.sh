#!/bin/bash
# Regenerate the replaystats corpus measurement and persist it as a dated,
# committed artifact -- so nobody pays the ~full-corpus replay cost again just
# to re-derive a number a previous session already computed. Run this from
# the repo root (`bash experiments/measure_replaystats.sh`).
#
# ONE corpus replay pass (SCOREDIV_DUMP_IDS=1 is a pure additional-print gate
# on `replaystats` -- see that binary's own doc -- it changes no other
# number), split into two files by grep so callers never have to scroll a
# combined report to find the ID list:
#   analysis/replaystats_<date>.txt        -- full raw `replaystats` stdout
#   analysis/score_divergence_<date>.txt   -- one SCOREDIV_DIVERGING_ID line +
#                                              one SCOREDIV_DETAIL line (engine
#                                              vs index scores, whether culture
#                                              drifted DURING play or only at
#                                              final scoring, final event
#                                              cards) per non-matching game
#
# Both files start with the exact command line, the git commit measured, the
# date, and the corpus size, so a diff between two dated runs is
# self-describing without cross-referencing this script.
set -eu
cd "$(dirname "$0")/.."

# Rust lives in ~/.cargo/bin, which is not on the default PATH on this machine
# (nor in a cron/nohup environment). Without this the script dies at the first
# cargo invocation with "cargo: command not found".
export PATH="$HOME/.cargo/bin:$PATH"

DATE=$(date +%F)
INDEX=sources/bgo/index.tsv
JOURNALS=/private/tmp/bgo-journals/journals
OUT=analysis/replaystats_${DATE}.txt
DIVOUT=analysis/score_divergence_${DATE}.txt
CMD="cd rust && SCOREDIV_DUMP_IDS=1 cargo run --profile difftest --bin replaystats -- ../$INDEX $JOURNALS"

mkdir -p analysis

cd rust
cargo build --profile difftest --bin replaystats
cd ..

COMMIT=$(git -C rust rev-parse HEAD 2>/dev/null || echo "unknown (not a git checkout at run time)")
N_GAMES=$(($(wc -l <"$INDEX") - 1))

RAW=$(cd rust && SCOREDIV_DUMP_IDS=1 ./target/difftest/replaystats "../$INDEX" "$JOURNALS")

{
    echo "# command: $CMD"
    echo "# git commit measured: $COMMIT"
    echo "# date: $DATE"
    echo "# corpus size (index.tsv rows, minus header): $N_GAMES"
    echo
    echo "$RAW" | grep -v '^SCOREDIV_'
} >"$OUT"
echo "wrote $OUT"

{
    echo "# command: $CMD"
    echo "# git commit measured: $COMMIT"
    echo "# date: $DATE"
    echo "# one SCOREDIV_DIVERGING_ID line + one SCOREDIV_DETAIL line per game whose sorted"
    echo "# game::scores multiset did not exactly match index.tsv's own recorded scores"
    echo
    echo "$RAW" | grep '^SCOREDIV_'
} >"$DIVOUT"
echo "wrote $DIVOUT"
