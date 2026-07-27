#!/usr/bin/env bash
# Stage-2 AlphaZero-style self-play loop for the TtA value net (2p).
#
# Each iteration: (1) self-play GAMES with the current BEST net (parallel CPU
# workers, temperature exploration) -> value rows + ranking pairs; (2) train a
# CANDIDATE warm-started from BEST on a replay window of recent iters (GPU);
# (3) GATE candidate-vs-best head-to-head, n=GATE (GPU); (4) PROMOTE only if the
# 95% CI lower bound clears 0.5 (win-ci>0.5); (5) every REFEVERY iters, measure
# BEST vs the linear champion / book / default for the honest learning curve.
# Everything logged to loop/curve.tsv and loop/iter*.log. Checkpoints kept.
set -u
# survive an SSH-session teardown: ignore HUP so a dropped connection cannot
# kill the overnight loop (child workers are non-PTY and file-redirected, so
# they already survive; this protects the driver bash itself).
trap '' HUP
cd ~/tta-ai || exit 1
PY=/c/Users/micro/AppData/Local/Programs/Python/Python312/python.exe
export PYTHONPATH=.

GEN0=${1:-checkpoints/value2p_rank.pt}   # strongest 1-ply net (vw=1, pair 0.821)
ITERS=${2:-120}
GAMES=${3:-480}         # total self-play games per iter (split over workers)
GATE=${4:-300}          # head-to-head games for the promotion gate
WORKERS=12
WINDOW=3                # replay buffer: train on the last WINDOW iters' data
EPOCHS=8
LR=3e-4                 # gentle warm-start fine-tune (don't drift off BEST)
LAM=1.0
VWEIGHT=${5:-1}         # keep pair-acc (ranking = 1-ply strength) maximal;
                        # measured: raising it calibrates value but hurts play
SELECT=pair             # best-checkpoint by ranking accuracy (drives strength)
EPS=0.1                 # epsilon-greedy exploration (scale-independent)
# BookBot anchor: mix the strong teacher's ranking pairs into every train so a
# candidate cannot drift BELOW book-level. iteration 1 of pure self-play
# regressed (cand 54 vs best 96 culture) -- self-play data from a sub-BookBot
# policy pulls the net down toward its own weaker play. The anchor caps that
# downside; surpassing book still needs a stronger target operator (Stage 3
# beam), see docs/NEURAL_EVAL.md.
ANCHOR="rankdata/rk_*.npz"
REFEVERY=3
CHAMP=analysis/frozen/champion_2p.json

PAUSE=~/tta-ai/PAUSE
# honor the gaming guard's PAUSE flag: block before launching any python so a
# game gets the whole box. The guard KILLS our python on detection (freeing
# VRAM); this keeps us from relaunching until the guard clears PAUSE.
wait_if_paused() {
  while [ -f "$PAUSE" ]; do
    echo "[$(date)] PAUSED for gaming; holding" >> loop/master.log
    sleep 30
  done
}

mkdir -p loop iterdata checkpoints
BEST=checkpoints/best.pt
# resume-safe: only seed BEST from gen0 if we don't already have one (a reboot
# mid-run should keep the promoted champion, not reset to gen0)
[ -f "$BEST" ] || cp "$GEN0" "$BEST"
CURVE=loop/curve.tsv
if [ ! -f "$CURVE" ]; then
  echo -e "iter\tpromoted\tcand_vs_best_win\tci\tcand_cul\tbest_cul\tvs_champ\tvs_book\tvs_default\tts" > "$CURVE"
fi
echo "LOOP START $(date)  gen0=$GEN0 iters=$ITERS games=$GAMES gate=$GATE vw=$VWEIGHT" | tee -a loop/master.log

per=$(( (GAMES + WORKERS - 1) / WORKERS ))
for it in $(seq 1 "$ITERS"); do
  echo "== ITER $it  $(date) ==" | tee -a loop/master.log
  wait_if_paused
  # (1) self-play generation with BEST
  for w in $(seq 0 $((WORKERS-1))); do
    s=$(( it*100000 + w*5000 ))
    $PY experiments/neural_gen_iter.py --ckpt "$BEST" --games "$per" --players 2 \
        --epsilon "$EPS" --stride 3 --krej 6 --seed0 "$s" \
        --out "iterdata/it${it}_w${w}" --device cpu > "loop/gen_it${it}_w${w}.log" 2>&1 &
  done
  wait
  # replay window: this iter and the previous WINDOW-1 iters
  globs=""
  for k in $(seq 0 $((WINDOW-1))); do
    j=$((it-k)); [ "$j" -ge 1 ] && globs="$globs iterdata/it${j}_w*.npz"
  done
  # (2) train candidate warm-started from BEST (self-play + BookBot anchor)
  wait_if_paused
  $PY experiments/neural_train_rank.py --data $globs $ANCHOR --init "$BEST" \
      --epochs "$EPOCHS" --lr "$LR" --lam "$LAM" --vweight "$VWEIGHT" \
      --select "$SELECT" --out checkpoints/cand.pt --device cuda \
      > "loop/train_it${it}.log" 2>&1
  # (3) gate candidate vs best
  wait_if_paused
  [ -f checkpoints/cand.pt ] || { echo "  no cand (killed?) -> skip iter $it" | tee -a loop/master.log; continue; }
  $PY experiments/neural_eval.py --ckpt checkpoints/cand.pt \
      --opponent "neural:$BEST" --games "$GATE" --players 2 --device cuda \
      > "loop/gate_it${it}.log" 2>&1
  SUM=$(grep "^SUMMARY" "loop/gate_it${it}.log" | tail -1)
  win=$(echo "$SUM" | sed -n 's/.*win=\([0-9.]*\).*/\1/p')
  ci=$(echo "$SUM" | sed -n 's/.*ci=\([0-9.]*\).*/\1/p')
  ccul=$(echo "$SUM" | sed -n 's/.*neural=\([0-9.-]*\).*/\1/p')
  bcul=$(echo "$SUM" | sed -n 's/.*opp=\([0-9.-]*\).*/\1/p')
  win=${win:-0}; ci=${ci:-1}; ccul=${ccul:-0}; bcul=${bcul:-0}
  # (4) promote iff CI lower bound clears 0.5
  promote=$(awk -v w="$win" -v c="$ci" 'BEGIN{print (w-c>0.5)?1:0}')
  if [ "$promote" = "1" ]; then
    cp checkpoints/cand.pt "$BEST"
    cp checkpoints/cand.pt "checkpoints/promoted_it${it}.pt"
    echo "  PROMOTED it$it  win=$win ci=$ci" | tee -a loop/master.log
  else
    echo "  kept best   it$it  cand win=$win ci=$ci (not CI-clear)" | tee -a loop/master.log
  fi
  # (5) reference curve
  vc="-"; vb="-"; vd="-"
  if [ $(( it % REFEVERY )) -eq 0 ] || [ "$promote" = "1" ]; then
    $PY experiments/neural_eval.py --ckpt "$BEST" --opponent "$CHAMP" --games 200 --device cuda > "loop/ref_champ_it${it}.log" 2>&1
    vc=$(grep "^SUMMARY" "loop/ref_champ_it${it}.log" | sed -n 's/.*win=\([0-9.]*\).*/\1/p')
    $PY experiments/neural_eval.py --ckpt "$BEST" --opponent book --games 200 --device cuda > "loop/ref_book_it${it}.log" 2>&1
    vb=$(grep "^SUMMARY" "loop/ref_book_it${it}.log" | sed -n 's/.*win=\([0-9.]*\).*/\1/p')
    $PY experiments/neural_eval.py --ckpt "$BEST" --opponent default --games 200 --device cuda > "loop/ref_default_it${it}.log" 2>&1
    vd=$(grep "^SUMMARY" "loop/ref_default_it${it}.log" | sed -n 's/.*win=\([0-9.]*\).*/\1/p')
    echo "  REF it$it  vs_champ=$vc vs_book=$vb vs_default=$vd best_cul(vs_champ side)" | tee -a loop/master.log
  fi
  echo -e "${it}\t${promote}\t${win}\t${ci}\t${ccul}\t${bcul}\t${vc:--}\t${vb:--}\t${vd:--}\t$(date +%s)" >> "$CURVE"
  # keep disk bounded: drop iterdata older than the window
  old=$((it-WINDOW))
  [ "$old" -ge 1 ] && rm -f iterdata/it${old}_w*.npz 2>/dev/null
done
echo "LOOP DONE $(date)" | tee -a loop/master.log
