#!/usr/bin/env bash
# SEARCH-BACKED self-play loop for the TtA value net (2p).  Replaces
# experiments/neural_loop.sh, whose 41-hour null is written up in
# docs/NEURAL_LOOP_NULL.md.
#
# The one change that matters: the improvement operator is a SEARCH.  The old
# loop labelled its ranking pairs with the net's own 1-ply argmax, so the
# untrained warm-start already satisfied 97.6% of them and the loss had nothing
# to teach.  Here the label is the root choice of `NeuralPlanBot` -- PlanBot's
# whole-turn beam with this same net as the leaf -- which is measurably stronger
# than the net's own argmax:
#
#     nplan(width=8) vs neural(1-ply), identical checkpoint, seat-balanced
#     ... measured in probe/ before this loop was written; the beam wins.
#
# and the gate is beam-vs-beam, i.e. the policy that actually ships.
#
# STAGE 0 (once): bootstrap a LEAF evaluator from the strongest bot on record,
# `plan:champion_2p` (culture 189 mirror / 213 vs book, against a human 159.5).
# The net has never been trained on a turn-boundary state, which is the only
# kind of state a beam leaf is ever asked about.
#
# Durability contract with the box owner (do not regress any of this):
#   * reads C:\Users\micro\tta-ai\PAUSE before every python launch and yields;
#     the gaming guard (experiments/gpu_guard.py) is the SINGLE WRITER of that
#     flag and also hard-kills our python to free VRAM.  We only ever read it.
#   * runs under the tta_neural_loop Scheduled Task (logon trigger + hourly
#     repetition + RestartOnFailure, Priority 7 = below normal), so it survives
#     reboot, crash, guard-kill and the SSH session going away.
#   * every worker is launched below-normal and torch is pinned to ONE thread
#     per process, so a game gets the box back immediately and the league arms
#     are not starved.
set -u
trap '' HUP
cd ~/tta-ai || exit 1
PY=/c/Users/micro/AppData/Local/Programs/Python/Python312/python.exe
export PYTHONPATH=.

ITERS=${1:-500}
GAMES=${2:-240}          # beam self-play games per iteration
GATE=${3:-200}           # beam-vs-beam games for the promotion gate
WIDTH=${WIDTH:-8}
NODES=${NODES:-1200}
GENW=${GENW:-8}          # gen workers (the box also runs the league arms)
GATEW=${GATEW:-8}
WINDOW=3
EPOCHS=6
LR=3e-4
LAM=1.0
VWEIGHT=${VWEIGHT:-1}
EPS=0.08
CHAMP=analysis/frozen/champion_2p.json
REFEVERY=5

mkdir -p loop2 iterdata2 checkpoints teacherdata
BEST=checkpoints/best_search.pt
LOG=loop2/master.log
CURVE=loop2/curve.tsv
PAUSE=~/tta-ai/PAUSE
PIDFILE=loop2/driver.pid
BEAT=loop2/driver.beat

# ---- single driver, enforced -----------------------------------------------
# The guard kills python.exe only, so THIS bash survives a game (deliberately:
# it should resume without waiting for a task trigger).  But the task also has
# an hourly repetition, and a driver that outlives its task registration is
# invisible to MultipleInstancesPolicy -- which is how a neural_loop.sh bash
# from Jul 27 was still alive two days later while a second one ran.  Same
# class of leak as the arm watchdog's.
#
# So: a live driver with a fresh heartbeat wins and the newcomer exits; a
# driver whose heartbeat has gone stale is presumed wedged and gets its tree
# killed.  Both halves are needed -- the first stops duplicates, the second
# stops a wedged driver from blocking recovery forever.
# $BEAT is beaten CONTINUOUSLY -- every 15s while any worker runs (beat_wait)
# and every 30s while parked (wait_if_paused) -- so a fresh heartbeat means
# "the driver is executing", not "an iteration boundary just went by".  That is
# what makes this threshold safe: it no longer has to exceed the longest
# possible iteration, only the longest gap between two beats.  Do NOT "fix" a
# false reap by raising this number; that only swaps one guess for another, and
# gen retries (4x here, 6x in stage 0) mean iteration length has no ceiling.
STALE_BEAT=2700          # 45 min; beats are 15s apart, so this is ~180x slack
if [ -f "$PIDFILE" ]; then
  oldpid=$(cat "$PIDFILE" 2>/dev/null | tr -d '[:space:]')
  if [ -n "$oldpid" ] && kill -0 "$oldpid" 2>/dev/null; then
    age=999999
    [ -f "$BEAT" ] && age=$(( $(date +%s) - $(date -r "$BEAT" +%s 2>/dev/null || echo 0) ))
    if [ "$age" -lt "$STALE_BEAT" ]; then
      echo "[$(date)] driver $oldpid alive (heartbeat ${age}s) -- exiting, not starting a second" >> "$LOG"
      exit 0
    fi
    echo "[$(date)] driver $oldpid heartbeat stale (${age}s) -- reaping and taking over" >> "$LOG"
    kill -9 "$oldpid" 2>/dev/null || true
  fi
fi
echo $$ > "$PIDFILE"
touch "$BEAT"
trap 'rm -f "$PIDFILE"' EXIT
# a driver killed mid-promotion leaves a staging file behind (see install_ckpt);
# only one driver exists at this point, so anything left is ours and is dead
rm -f checkpoints/*.pt.tmp.* 2>/dev/null || true

wait_if_paused() {
  local said=0
  while [ -f "$PAUSE" ]; do
    [ "$said" = 0 ] && { echo "[$(date)] PAUSED for gaming; holding" >> "$LOG"; said=1; }
    # keep the heartbeat fresh while parked, or a long gaming session would
    # look like a wedged driver and get reaped by the next task trigger
    touch "$BEAT" 2>/dev/null
    sleep 30
  done
}

say() { echo "$@" | tee -a "$LOG"; }

# Never launch python while a game is up.  Priority comes from the Scheduled
# Task (<Priority>7</Priority> = below normal) and is INHERITED by every child
# process, so a game always outranks us without any per-process wrapper.
low() { wait_if_paused; touch "$BEAT" 2>/dev/null; "$PY" "$@"; }

# Every stretch where this driver goes quiet for tens of minutes is a `wait` on
# python workers.  The relaunch guard above cannot tell a slow driver from a
# wedged one -- it only reads $BEAT -- and it reaps with kill -9, which is how a
# legitimately slow iteration gets its checkpoint write torn in half.  So beat
# while we wait: this is the single choke point for every background phase (gen,
# stage-0 teacher gen, gate, ref), and with beat_run it covers the foreground
# training runs too.  A 1441s quiet stall was already observed in stage 0 --
# more than half the reap threshold, from one phase.
beat_wait() {
  while [ -n "$(jobs -pr)" ]; do
    touch "$BEAT" 2>/dev/null
    sleep 15
  done
  wait
  touch "$BEAT" 2>/dev/null
}

# A long FOREGROUND python (the trainers) with the same heartbeat coverage:
# background it so beat_wait can beat, then wait for exactly it.  Callers keep
# their own redirections; they check for the output file, not the exit status,
# exactly as they did when this was a bare "$PY".
beat_run() { low "$@" & beat_wait; }

# Replace $BEST (or any checkpoint) ATOMICALLY.  cp writes in place, so a kill
# -9 landing mid-copy -- from the reaper above, or from the gaming guard --
# leaves a truncated best_search.pt, and best_search.pt is the one artifact the
# whole run is building.  Stage into a temp file in the SAME directory (same
# filesystem, or the rename is a copy and not atomic) and rename over the
# destination: a reader then sees either the whole old file or the whole new
# one, never a half-written one.
install_ckpt() {   # install_ckpt SRC DST
  local src=$1 dst=$2 tmp="${dst}.tmp.$$" i
  cp "$src" "$tmp" || { rm -f "$tmp"; say "  WARNING: could not stage $src -> $tmp; $dst UNCHANGED"; return 1; }
  for i in 1 2 3; do
    mv -f "$tmp" "$dst" 2>/dev/null && return 0
    sleep 2
  done
  # Windows refuses to rename over a file another process still holds open.
  # Falling back to cp is exactly the old behaviour, so this is never worse than
  # before -- but it is the one non-atomic path left, so it says so out loud.
  say "  WARNING: atomic rename over $dst failed 3x; falling back to in-place cp"
  cp "$tmp" "$dst"; rm -f "$tmp"; return 1
}

# ---------------------------------------------------------------- gate
# Fan the head-to-head out over disjoint seed ranges and pool.  A beam-vs-beam
# game is ~18 cpu-s, so a serial n=200 gate is an hour; this is ~8 minutes.
gate_parallel() {   # gate_parallel CAND OPP PREFIX
  local cand=$1 opp=$2 prefix=$3
  local per=$(( (GATE + GATEW - 1) / GATEW ))
  per=$(( (per + 1) / 2 * 2 ))          # even, so seats stay balanced
  rm -f "${prefix}"_*.log
  for w in $(seq 0 $((GATEW-1))); do
    low experiments/neural_eval.py --ckpt "$cand" --search plan \
        --width "$WIDTH" --nodes "$NODES" --opponent "nplan:$opp" \
        --games "$per" --players 2 --device cpu --threads 1 --report 1000 \
        --seed0 $((w*1000)) > "${prefix}_${w}.log" 2>&1 &
  done
  beat_wait
  "$PY" experiments/pool_summary.py "${prefix}"_*.log
}

if [ ! -f "$CURVE" ]; then
  printf 'iter\tpromoted\twin\tci\tcand_cul\tbest_cul\tdisagree\tvs_planchamp\tts\n' > "$CURVE"
fi

# ---------------------------------------------------------------- STAGE 0
# Bootstrap the leaf evaluator from plan:champion self-play.  Skipped once
# $BEST exists, so a reboot mid-run resumes the loop instead of redoing this.
if [ ! -f "$BEST" ]; then
  say "== STAGE 0 teacher bootstrap  $(date) =="
  # COMPLETE is the only signal that the teacher set is whole.  The guard kills
  # our python the instant a game starts, so "some shards exist" means nothing;
  # without this sentinel a guard-kill 30 seconds into generation would leave a
  # fraction of the data on disk and the next pass would train on it as if it
  # were the full set.  Every worker must report DONE.
  tries=0
  while [ ! -f teacherdata/COMPLETE ] && [ "$tries" -lt 6 ]; do
    tries=$((tries+1))
    rm -f teacherdata/tg_*.npz loop2/teacher_w*.log
    TG=${TEACHER_GAMES:-480}
    per=$(( (TG + GENW - 1) / GENW ))
    for w in $(seq 0 $((GENW-1))); do
      low experiments/plan_teacher_gen.py --weights "$CHAMP" --players 2 \
          --games "$per" --width 8 --seed0 $((w*10000)) \
          --out "teacherdata/tg_w${w}" > "loop2/teacher_w${w}.log" 2>&1 &
    done
    beat_wait
    done_n=$(grep -l "^DONE" loop2/teacher_w*.log 2>/dev/null | wc -l)
    say "  teacher attempt $tries: $done_n/$GENW workers finished, "\
"$(ls teacherdata/tg_*.npz 2>/dev/null | wc -l) shards"
    [ "$done_n" -ge "$GENW" ] && touch teacherdata/COMPLETE
  done
  [ -f teacherdata/COMPLETE ] || { say "  STAGE 0 could not complete generation; will retry on next task start"; exit 1; }
  beat_run experiments/neural_train_rank.py --data 'teacherdata/tg_*.npz' \
      --epochs 20 --lr 1e-3 --lam "$LAM" --vweight "$VWEIGHT" \
      --select last --val-split rows --out checkpoints/boot.pt --device cuda \
      > loop2/train_boot.log 2>&1
  [ -f checkpoints/boot.pt ] || { say "  STAGE 0 training produced no checkpoint; aborting"; exit 1; }
  # record what the bootstrap bought against the old 1-ply lineage net, both
  # sides under the beam.  Informational: the bootstrap starts a new lineage
  # either way, because the old net was never trained on a turn-boundary state.
  if [ -f checkpoints/best.pt ]; then
    say "  STAGE 0 boot vs old best (both under the beam): $(gate_parallel checkpoints/boot.pt checkpoints/best.pt loop2/gate_boot)"
  fi
  install_ckpt checkpoints/boot.pt "$BEST"
  say "  STAGE 0 done -> $BEST"
fi

say "LOOP START $(date)  best=$BEST width=$WIDTH nodes=$NODES games=$GAMES gate=$GATE"

start_it=$(( $(awk 'END{print NR-1}' "$CURVE" 2>/dev/null || echo 0) + 1 ))
[ "$start_it" -lt 1 ] && start_it=1
for it in $(seq "$start_it" "$ITERS"); do
  say "== ITER $it  $(date) =="
  touch "$BEAT" 2>/dev/null

  # (1) beam self-play generation with BEST.  Retried as a whole if the guard
  # killed workers partway: a half-generated iteration is a silently smaller
  # and seed-biased training set, not a smaller-but-fine one.
  per=$(( (GAMES + GENW - 1) / GENW ))
  gtries=0
  while [ "$gtries" -lt 4 ]; do
    gtries=$((gtries+1))
    rm -f iterdata2/it${it}_w*.npz loop2/gen_it${it}_w*.log
    for w in $(seq 0 $((GENW-1))); do
      low experiments/neural_gen_plan.py --ckpt "$BEST" --games "$per" \
          --players 2 --width "$WIDTH" --nodes "$NODES" --epsilon "$EPS" \
          --stride 3 --krej 4 --seed0 $(( it*100000 + w*5000 )) \
          --out "iterdata2/it${it}_w${w}" --device cpu --threads 1 \
          > "loop2/gen_it${it}_w${w}.log" 2>&1 &
    done
    beat_wait
    gdone=$(grep -l "^DONE" loop2/gen_it${it}_w*.log 2>/dev/null | wc -l)
    [ "$gdone" -ge "$GENW" ] && break
    say "  gen it$it attempt $gtries incomplete ($gdone/$GENW workers) -- retrying"
  done
  # health meter: the fraction of decisions where the beam OVERRULED the net's
  # own 1-ply argmax.  This is the entire information content of the target.
  # If it decays toward 0 the loop has gone vacuous and must be stopped --
  # that is precisely the failure docs/NEURAL_LOOP_NULL.md 3.1 could not see.
  dis=$(grep -h "^DONE" loop2/gen_it${it}_w*.log 2>/dev/null \
        | sed -n 's/.*DISAGREE=\([0-9.]*\).*/\1/p' \
        | awk '{s+=$1;n++} END{if(n)printf "%.4f", s/n; else print "0"}')
  say "  gen it$it  DISAGREE=$dis  shards=$(ls iterdata2/it${it}_w*.npz 2>/dev/null | wc -l)"
  awk -v d="$dis" 'BEGIN{exit !(d+0 < 0.02)}' && \
    say "  *** WARNING DISAGREE=$dis < 0.02: the search no longer overrules the net; the target is going vacuous (docs/NEURAL_LOOP_NULL.md 3.1) ***"

  if ! ls iterdata2/it${it}_w*.npz >/dev/null 2>&1; then
    say "  no gen data (guard kill?) -> retry iter $it"; continue
  fi

  # (2) train a candidate warm-started from BEST on the replay window
  globs=""
  for k in $(seq 0 $((WINDOW-1))); do
    j=$((it-k)); [ "$j" -ge 1 ] && globs="$globs iterdata2/it${j}_w*.npz"
  done
  # a stale cand.pt from a guard-killed iteration would otherwise be gated as
  # if it were this iteration's candidate
  rm -f checkpoints/cand.pt
  beat_run experiments/neural_train_rank.py --data $globs 'teacherdata/tg_*.npz' \
      --init "$BEST" --epochs "$EPOCHS" --lr "$LR" --lam "$LAM" \
      --vweight "$VWEIGHT" --select last --val-split rows \
      --out checkpoints/cand.pt --device cuda \
      > "loop2/train_it${it}.log" 2>&1
  [ -f checkpoints/cand.pt ] || { say "  no cand (killed?) -> skip iter $it"; continue; }

  # (3) gate: beam vs beam, the policy that actually ships
  SUM=$(gate_parallel checkpoints/cand.pt "$BEST" "loop2/gate_it${it}")
  win=$(echo "$SUM" | sed -n 's/.*win=\([0-9.]*\).*/\1/p')
  ci=$(echo "$SUM" | sed -n 's/.*ci=\([0-9.]*\).*/\1/p')
  ccul=$(echo "$SUM" | sed -n 's/.*neural=\([0-9.-]*\).*/\1/p')
  bcul=$(echo "$SUM" | sed -n 's/.*opp=\([0-9.-]*\).*/\1/p')
  win=${win:-0}; ci=${ci:-1}; ccul=${ccul:-0}; bcul=${bcul:-0}

  # (4) promote iff the 95% CI lower bound clears 0.5
  promote=$(awk -v w="$win" -v c="$ci" 'BEGIN{print (w-c>0.5)?1:0}')
  if [ "$promote" = "1" ]; then
    install_ckpt checkpoints/cand.pt "$BEST"
    install_ckpt checkpoints/cand.pt "checkpoints/promoted_s_it${it}.pt"
    say "  PROMOTED it$it  win=$win ci=$ci cul=$ccul vs $bcul"
  else
    say "  kept best   it$it  cand win=$win ci=$ci cul=$ccul vs $bcul"
  fi

  # (5) the honest yardstick: us vs the strongest bot on record, same search
  vp="-"
  if [ $(( it % REFEVERY )) -eq 0 ] || [ "$promote" = "1" ]; then
    rm -f loop2/ref_it${it}_*.log
    perr=$(( 40 / GATEW * 2 )); [ "$perr" -lt 2 ] && perr=2
    for w in $(seq 0 $((GATEW-1))); do
      low experiments/neural_eval.py --ckpt "$BEST" --search plan \
          --width "$WIDTH" --nodes "$NODES" --opponent "plan:$CHAMP,width=8" \
          --games "$perr" --players 2 --device cpu --threads 1 --report 1000 \
          --seed0 $((w*1000)) > "loop2/ref_it${it}_${w}.log" 2>&1 &
    done
    beat_wait
    RS=$("$PY" experiments/pool_summary.py loop2/ref_it${it}_*.log)
    vp=$(echo "$RS" | sed -n 's/.*win=\([0-9.]*\).*/\1/p')
    say "  REF it$it  vs plan:champion -> $RS"
  fi

  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
    "$it" "$promote" "$win" "$ci" "$ccul" "$bcul" "$dis" "${vp:--}" "$(date +%s)" >> "$CURVE"

  # keep disk bounded
  old=$((it-WINDOW))
  [ "$old" -ge 1 ] && rm -f iterdata2/it${old}_w*.npz 2>/dev/null
done
say "LOOP DONE $(date)"
