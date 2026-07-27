#!/bin/bash
# Paired Strength A/B: QuiescentBot vs the 1-ply WeightedBot, same weights,
# same seeds.  Sequential on purpose: 2 workers each, nice 15, on a box that
# already runs three live trainers.
set -u
cd "$(dirname "$0")/.."
export TTA_JOURNAL=1
OUT=exp_quiesce/ab.jsonl
W2=exp_quiesce/champ_2p.json

run() {   # players weights label games spec_a
    p=$1; lbl=$2; g=$3; a=$4; b=$5
    if grep -q "\"label\": \"$lbl\"" "$OUT" 2>/dev/null; then
        echo "skip $lbl"; return
    fi
    echo "=== $lbl ($g games, ${p}p) $(date +%T)"
    nice -n 15 python3 - "$p" "$g" "$a" "$b" "$lbl" <<'PY'
import json, sys, time
sys.path.insert(0, ".")
from experiments import arena
p, g, a, b, lbl = int(sys.argv[1]), int(sys.argv[2]), sys.argv[3], sys.argv[4], sys.argv[5]
t0 = time.time()
res = arena.duel(arena.load_spec(a), arena.load_spec(b), p, g, seed0=9000, workers=1)
res.update(label=lbl, a=a, b=b, secs=round(time.time() - t0, 1))
with open("exp_quiesce/ab.jsonl", "a") as fh:
    fh.write(json.dumps(res) + "\n")
print(arena.fmt(res, lbl, "1ply"), f"[{res['secs']}s]")
PY
}

# 2p uses the trained league champion; 3p/4p arms restarted clean tonight and
# have almost no training, so they use the DEFAULT vector.
# Breadth before depth: LEVELS=1 at every table size first, so a run that has
# to be cut short still answers the main question at 2p/3p/4p.
run 2 ctrl_2p   800 "$W2"                     "$W2"
run 2 q1_2p     800 "quiesce:$W2,levels=1"    "$W2"
run 3 ctrl_3p   801 default                   default
run 3 q1_3p     801 "quiesce:default,levels=1" default
# NO ctrl_4p.  The control arm is provably redundant: identical deterministic
# bots make the game independent of which seat is labelled "the challenger", so
# in every seed group the challenger takes exactly 1/P of the win and exactly
# 0.0 of the culture margin.  That was CHECKED, not assumed -- all 400 2p and
# all 267 3p control groups came out at exactly 1/P and exactly 0.0 margin, to
# the last bit.  Spending ~1800 cpu-s to re-derive an identity at 4p is waste.
run 4 q1_4p     800 "quiesce:default,levels=1" default
run 2 q2_2p     800 "quiesce:$W2,levels=2"    "$W2"
echo "DONE $(date +%T)"
