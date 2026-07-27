#!/bin/bash
# External yardstick.  docs/STRENGTH_CHECK.md and docs/BOT_ROSTER.md both put
# the trained champion BEHIND several hand-written rule-list bots.  A win over
# a 1-ply mirror is a self-play number; the only question that says anything
# about absolute strength is whether quiescence moves the champion up THAT
# ladder.  Paired: the same seeds for the 1-ply and the quiescent challenger.
set -u
cd "$(dirname "$0")/.."
export TTA_JOURNAL=1
OUT=exp_quiesce/yardstick.jsonl
W2=exp_quiesce/champ_2p.json

run() {  # players label games spec_a spec_b
    p=$1; lbl=$2; g=$3; a=$4; b=$5
    if grep -q "\"label\": \"$lbl\"" "$OUT" 2>/dev/null; then
        echo "skip $lbl"; return
    fi
    echo "=== $lbl ($g games, ${p}p) $(date +%T)"
    nice -n 15 python3 - "$p" "$g" "$a" "$b" "$lbl" <<'PY'
import json, sys, time
sys.path.insert(0, ".")
from experiments import roster_match as R      # patches arena.make_bot
from experiments import arena
p, g, a, b, lbl = int(sys.argv[1]), int(sys.argv[2]), sys.argv[3], sys.argv[4], sys.argv[5]
t0 = time.time()
res = arena.duel(R.load_spec(a, p), R.load_spec(b, p), p, g, seed0=3100,
                 workers=1)
res.update(label=lbl, a=a, b=b, secs=round(time.time() - t0, 1))
res.pop("shares", None)
with open("exp_quiesce/yardstick.jsonl", "a") as fh:
    fh.write(json.dumps(res) + "\n")
print(arena.fmt(res, lbl, b), f"[{res['secs']}s]")
PY
}

run 2 y_1ply_culture 400 "$W2"                  culture
run 2 y_qui_culture  400 "quiesce:$W2,levels=1" culture
run 2 y_1ply_book    400 "$W2"                  book2
run 2 y_qui_book     400 "quiesce:$W2,levels=1" book2
echo "DONE $(date +%T)"
