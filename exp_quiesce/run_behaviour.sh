#!/bin/bash
# Behaviour counts: MIRROR tables (every seat the same bot), so the numbers are
# "what this search does when everyone uses it", not "what it does against a
# field it out-searches".
set -u
cd "$(dirname "$0")/.."
export TTA_JOURNAL=1
OUT=exp_quiesce/behaviour.jsonl
W2=exp_quiesce/champ_2p.json

run() {  # players label games spec
    p=$1; lbl=$2; g=$3; spec=$4
    if grep -q "\"label\": \"$lbl\"" "$OUT" 2>/dev/null; then
        echo "skip $lbl"; return
    fi
    echo "=== $lbl $(date +%T)"
    nice -n 15 python3 tools/behaviour_counts.py --players "$p" --games "$g" \
        --spec "$spec" --label "$lbl" >> "$OUT"
    tail -1 "$OUT"
}

run 2 b1_2p 120 "$W2"
run 2 bq1_2p 120 "quiesce:$W2,levels=1"
run 3 b1_3p 120 default
run 3 bq1_3p 120 "quiesce:default,levels=1"
run 4 b1_4p 120 default
run 4 bq1_4p 120 "quiesce:default,levels=1"
run 2 bq2_2p 120 "quiesce:$W2,levels=2"
run 3 bq2_3p 120 "quiesce:default,levels=2"
run 4 bq2_4p 120 "quiesce:default,levels=2"
echo "DONE $(date +%T)"
