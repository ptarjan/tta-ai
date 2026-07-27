#!/bin/bash
# Attribution.  QuiescentBot bundles two independent things: quiescence proper
# (drain state.pending before scoring) and WAR_LOOKAHEAD (call the engine's own
# resolve_war on a scratch copy, because a war declaration pushes nothing onto
# the stack at all).  The behaviour counts show wars going 0.00 -> 1.43/game at
# 2p and 0.00 -> 7.55/game at 4p, which is entirely the lookahead's doing, so
# the win rate has to be split between the two or the headline is unreadable.
set -u
cd "$(dirname "$0")/.."
export TTA_JOURNAL=1
OUT=exp_quiesce/ab.jsonl
W2=exp_quiesce/champ_2p.json

duel() {  # players label games spec_a spec_b
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

bhv() {  # players label games spec
    p=$1; lbl=$2; g=$3; spec=$4
    if grep -q "\"label\": \"$lbl\"" exp_quiesce/behaviour.jsonl 2>/dev/null; then
        echo "skip $lbl"; return
    fi
    echo "=== $lbl $(date +%T)"
    nice -n 15 python3 tools/behaviour_counts.py --players "$p" --games "$g" \
        --spec "$spec" --label "$lbl" >> exp_quiesce/behaviour.jsonl
    tail -1 exp_quiesce/behaviour.jsonl
}

# same seeds as q1_2p / ctrl_2p, so all three are paired game for game
duel 2 qnw_2p 800 "quiesce:$W2,levels=1,war=0" "$W2"
bhv  2 bqnw_2p 120 "quiesce:$W2,levels=1,war=0"
duel 3 qnw_3p 801 "quiesce:default,levels=1,war=0" default
echo "DONE $(date +%T)"
