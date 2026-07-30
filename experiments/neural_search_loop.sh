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

# Games in the ANCHOR match -- us vs plan:champion_2p, the strongest bot on
# record and the only yardstick in this loop that does not move.  Was 72, run
# on promotion iterations only; both of those were wrong.
#
#   n=72  -> 95% CI is about +-0.11.  Every anchor score ever recorded (0.4028,
#           0.3472, 0.3680, 0.3958) sits inside every other one's interval, so
#           seven iterations of "progress" are statistically indistinguishable
#           from a flat line.  A yardstick that cannot resolve the ~5pp changes
#           we are trying to make is not measuring anything.
#   n=240 -> about +-0.063 (40 games x 6 shards), which can.
#
# Cost, measured off loop2/master.log timestamps on the desktop (six clean
# iterations, 2p, 6 workers): an iteration with no anchor run is ~22m40s and
# one with the old n=72 anchor is ~26m, so the anchor is ~3min at n=72 and
# ~10min at n=240.  Running it EVERY iteration therefore puts an iteration at
# ~33min, up from ~23-26min.  That is the price of a yardstick that works, and
# it stays under the 45min STALE_BEAT reap threshold below with room to spare.
REFN=${REFN:-240}

mkdir -p loop2 iterdata2 checkpoints teacherdata
BEST=checkpoints/best_search.pt
LOG=loop2/master.log
CURVE=loop2/curve.tsv
PAUSE=~/tta-ai/PAUSE
PIDFILE=loop2/driver.pid
BEAT=loop2/driver.beat
# The incumbent's own anchor score, carried forward from the iteration that
# promoted it.  THREE fields since the gate-floor correction: `win ci se`.
#
#   win  the incumbent's anchor win rate
#   ci   pool_summary's legacy per-game 95% half-width.  Carried for continuity
#        with older logs and printed in the gate line; NOT used in the decision.
#   se   the SHARD-CLUSTERED standard error -- the only field the floor reads.
#
# A two-field (pre-correction) file is not silently upgraded: there is no way
# to recover a cluster SE from a per-game CI, and the legacy `ci/1.96` is
# exactly the too-tight number this change removes.  Arm B fails CLOSED and
# says so.  See the anchor gate in step (4).
ANCHORF=loop2/anchor_best.txt
# What curve.tsv holds when a measurement did not happen.  Deliberately not a
# number: see `fanout` below.
NULL='-'

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

# ---------------------------------------------------------------- measurement
# Pull a field out of a pooled SUMMARY line.  Returns empty if absent, which is
# what every caller below tests for -- `win=NA` must never parse as a number.
sfield() { printf '%s' "$2" | sed -n "s/.*[[:space:]]$1=\\(-\\?[0-9.]*\\).*/\\1/p"; }

# say() on STDERR.  `say` tees to stdout, so calling it inside a function whose
# stdout the caller captures with $(...) -- which is every measurement function
# below -- would splice log prose into the SUMMARY string the caller then
# parses with sfield.  The retry path is exactly where that happens: attempt 1
# logs three lines, attempt 2 succeeds, and the caller's "win" becomes those
# three lines plus a number.  Diagnostics go to stderr; only the measurement
# goes to stdout.  Both still reach loop2/master.log (via tee) and master.out
# (via the driver's 2>&1).
sayerr() { echo "$@" | tee -a "$LOG" >&2; }

# A MEASUREMENT THAT PRODUCED NO GAMES IS NOT A SCORE OF ZERO.
#
# This is the data-integrity half of this file.  `pool_summary.py` used to
# print `win=0.0000 ci=1.0000 ... n=0 shards=0` and exit 0 when every shard was
# missing or unparseable, and the loop wrote that 0.0000 into curve.tsv as if
# it were an observation.  Row 4 of the desktop's curve says
# `vs_planchamp=0.0000` and what actually happened is that the reference run
# did not run -- indistinguishable, after the fact, from the net being beaten
# 0-72 by the champion.  A plausible number standing in for absent work is
# strictly worse than a gap, because a gap cannot be averaged into a trend.
#
# Same class of failure as the zero-game alarm in the hillclimb league (commit
# 9018624): work that completes zero games must say so loudly instead of
# quietly reporting on an empty sample.  Same remedy, scaled to this loop's
# risk -- the league arm halts behind a sentinel because it would otherwise
# crash-loop for hours; here one bad fan-out is recoverable, so:
#
#   1. log it loudly, with the summary line that came back, so it is
#      diagnosable and not just a count;
#   2. retry the whole fan-out ONCE (the usual cause is the gaming guard
#      killing every worker, which is transient by definition);
#   3. on a second failure return 1 and print NOTHING.  Callers write $NULL to
#      curve.tsv and refuse to let the absent number drive a decision.
#
# Never widen this to "some shards came back".  A partial fan-out is a smaller
# but honest sample and pool_summary weights it correctly; zero shards is the
# only case that is a lie.
fanout() {   # fanout CKPT OPPSPEC NGAMES PREFIX LABEL  -> SUMMARY on stdout
  local ckpt=$1 opp=$2 total=$3 prefix=$4 label=$5
  local per attempt w out n shards
  per=$(( (total + GATEW - 1) / GATEW ))
  per=$(( (per + 1) / 2 * 2 ))          # even, so seats stay balanced
  for attempt in 1 2; do
    rm -f "${prefix}"_*.log
    for w in $(seq 0 $((GATEW-1))); do
      low experiments/neural_eval.py --ckpt "$ckpt" --search plan \
          --width "$WIDTH" --nodes "$NODES" --opponent "$opp" \
          --games "$per" --players 2 --device cpu --threads 1 --report 1000 \
          --seed0 $((w*1000)) > "${prefix}_${w}.log" 2>&1 &
    done
    beat_wait
    out=$("$PY" experiments/pool_summary.py "${prefix}"_*.log 2>/dev/null)
    n=$(sfield n "$out"); shards=$(sfield shards "$out")
    if [ -n "$n" ] && [ "${n%%.*}" -gt 0 ] 2>/dev/null \
       && [ -n "$shards" ] && [ "${shards%%.*}" -gt 0 ] 2>/dev/null; then
      printf '%s\n' "$out"
      return 0
    fi
    sayerr "  *** NO GAMES: $label produced n=${n:-?} shards=${shards:-?} on attempt $attempt/2"
    sayerr "  ***   pooled line was: ${out:-<no output>}"
    sayerr "  ***   (guard kill, or every worker died -- see ${prefix}_*.log)"
  done
  sayerr "  *** MEASUREMENT FAILED: $label produced no games twice; recording $NULL, NOT a score."
  sayerr "  ***   A missing measurement is never written as 0.0000 (see fanout in this file)."
  return 1
}

# The promotion gate: beam vs beam, the policy that actually ships.  A
# beam-vs-beam game is ~18 cpu-s, so a serial n=200 gate is an hour; the
# fan-out over disjoint seed ranges is ~8 minutes.
gate_parallel() {   # gate_parallel CAND OPP PREFIX
  fanout "$1" "nplan:$2" "$GATE" "$3" "gate $(basename "$1") vs $(basename "$2")"
}

# The anchor match: us vs the strongest bot on record, same search both sides.
anchor_run() {      # anchor_run CKPT PREFIX LABEL
  fanout "$1" "plan:$CHAMP,width=8" "$REFN" "$2" "anchor $3 vs plan:champion"
}

# ---- ARM B FLOOR (single source of truth; tests/test_gate_floor.py runs it) --
# One standard error OF THE DIFFERENCE below the incumbent's anchor.
#
# The two arguments are STANDARD ERRORS, not half-widths.  Read them from
# pool_summary's `se_cluster=` field and pass them straight through.  Do not
# reconstruct them by dividing a CI by anything:
#
#   se_cluster            0.0490  ->  band 6.93pp   CORRECT
#   ci (legacy, /1.96)    0.0320  ->  band 4.52pp   the old, too-tight gate
#   ci_cluster / 1.96     0.0643  ->  band 9.09pp   double-counts t_{k-1}
#
# The middle line is what shipped until 2026-07-30 and the bottom line is the
# trap that looks like a fix.  docs/CARD_BLINDNESS.md 10.6 has the derivation.
anchor_floor() {    # anchor_floor INC_WIN CAND_SE INC_SE -> floor on stdout
  awk -v iw="$1" -v cs="$2" -v is="$3" \
    'BEGIN{ printf "%.4f", iw - sqrt(cs*cs + is*is) }'
}

# curve.tsv schema.  The last four columns are new with the anchor gate:
#   vs_planchamp  the CANDIDATE's anchor win rate this iteration (was: the
#                 incumbent's, and only on promotion iterations).  This is the
#                 number the gate tests, so it is the one that has to be here.
#   anchor_ci     its 95% half-width, so a reader can tell 0.36 +-0.06 from
#                 0.36 +-0.11 without going back to master.log.
#   inc_anchor    the INCUMBENT's anchor at decision time.  The curve of the
#                 net that actually ships is this column, not vs_planchamp;
#                 they coincide exactly on promotion rows.
#   selfplay_ok / anchor_ok
#                 the two promotion criteria, separately, so a blocked
#                 promotion says which half blocked it.
# Any of them may be '-' ($NULL): see fanout().
#
# COMMENT ROWS.  A line whose first character is '#' is an annotation, not an
# observation.  It exists because some events break the comparability of the
# rows either side of them and a reader who plots the column straight through
# gets a lie: the one that forced this convention was commit 96a5db2, which
# taught weighted.py to price effects.culture/effects.science.  That changed
# how the FROZEN champion plays, so anchor scores measured before it and after
# it are two different rulers, and splicing them into one series invents a
# trend that no measurement supports.  Write the marker, re-seed, carry on.
#
# Two rules make a comment row safe, and both are enforced below:
#   * it does not advance the iteration counter (see start_it);
#   * the schema migration passes it through verbatim instead of padding it
#     out to 13 fields, which would turn prose into data.
CURVE_HDR=$(printf 'iter\tpromoted\twin\tci\tcand_cul\tbest_cul\tdisagree\tvs_planchamp\tts\tanchor_ci\tinc_anchor\tselfplay_ok\tanchor_ok')
CURVE_NCOL=13
if [ ! -f "$CURVE" ]; then
  printf '%s\n' "$CURVE_HDR" > "$CURVE"
elif [ "$(head -1 "$CURVE" 2>/dev/null)" != "$CURVE_HDR" ]; then
  # Migrate in place rather than starting a new file.  The anchor curve is the
  # entire point of this change and it has to stay continuous across the
  # upgrade; a header that disagrees with its own rows is how a reader ends up
  # plotting anchor_ci as a win rate.
  say "migrating $CURVE to the ${CURVE_NCOL}-column schema (old rows padded with '$NULL')"
  awk -v hdr="$CURVE_HDR" -v n="$CURVE_NCOL" -v nul="$NULL" \
      'BEGIN{FS=OFS="\t"; print hdr} NR>1 && /^#/{print; next} NR>1{for(i=NF+1;i<=n;i++)$i=nul; print}' \
      "$CURVE" > "$CURVE.mig" && mv -f "$CURVE.mig" "$CURVE"
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
    say "  STAGE 0 boot vs old best (both under the beam): $(gate_parallel checkpoints/boot.pt checkpoints/best.pt loop2/gate_boot || echo "$NULL no data")"
  fi
  install_ckpt checkpoints/boot.pt "$BEST"
  say "  STAGE 0 done -> $BEST"
fi

say "LOOP START $(date)  best=$BEST width=$WIDTH nodes=$NODES games=$GAMES gate=$GATE refn=$REFN"

okword() { [ "${1:-0}" = 1 ] && printf 'PASS' || printf 'BLOCK'; }

# ------------------------------------------------------ incumbent anchor seed
# Arm B of the gate compares the candidate's score against the frozen champion
# with the INCUMBENT's score against that same champion.  The incumbent's score
# is normally inherited from the iteration that promoted it -- it was measured
# then, as that iteration's candidate, so steady state costs nothing extra.
# But on the first run after this gate was added, and on a fresh box, there is
# no such record, and an arm with no baseline is an arm that always passes.
#
# So measure it once, here, instead of defaulting to a number.  This is the
# ONLY place the anchor gate fails open, and it says so when it does.
if [ ! -s "$ANCHORF" ]; then
  say "no incumbent anchor on record -- measuring $BEST vs plan:champion once to seed arm B"
  if AS=$(anchor_run "$BEST" loop2/anchor_seed "incumbent"); then
    # Only ever write three real numbers.  A blank field here would read back
    # as awk 0, which silently turns arm B's floor into "beat the incumbent
    # exactly" (or, before the sqrt was split out, "beat -0.045") -- a gate
    # that is not the gate anyone wrote, which is worse than no gate because it
    # looks like one.  `se` is the shard-clustered standard error and is the
    # field the floor reads; a seed without it is not a baseline.
    sw=$(sfield win "$AS"); sc=$(sfield ci "$AS"); ss=$(sfield se_cluster "$AS")
    if [ -n "$sw" ] && [ -n "$sc" ] && [ -n "$ss" ]; then
      printf '%s %s %s\n' "$sw" "$sc" "$ss" > "$ANCHORF"
      say "  seeded incumbent anchor: $AS"
    else
      say "  *** anchor seed returned games but no parseable win/ci/se_cluster: $AS"
      say "  *** (se_cluster needs >=2 shards; a single-shard seed cannot bound itself)"
    fi
  else
    say "  *** could not seed the incumbent anchor; arm B ABSTAINS until a promotion sets one"
  fi
fi

# The next iteration number is "how many observations are on file, plus one".
# Count OBSERVATIONS, not lines: comment rows (see the schema block above) are
# annotations and must not consume an iteration number, or every marker anyone
# ever writes silently punches a hole in the sequence -- iteration 11 missing
# from a curve reads as a crashed iteration, not as a note someone left.
start_it=$(( $(awk '/^#/{next} {n++} END{print n-1}' "$CURVE" 2>/dev/null || echo 0) + 1 ))
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

  # (3) GATE ARM A -- self-play: beam vs beam, the policy that actually ships.
  # This is the criterion the loop has always had.  On its own it is
  # self-referential: it only ever asks "is this better than the last thing
  # this same process produced", which is satisfied by drift as readily as by
  # learning.  Seven iterations of it moved self-play culture 116 -> 143 while
  # the fixed anchor did not move at all.  Hence arm B.
  win=$NULL; ci=$NULL; ccul=$NULL; bcul=$NULL; selfplay_ok=0
  if SUM=$(gate_parallel checkpoints/cand.pt "$BEST" "loop2/gate_it${it}"); then
    win=$(sfield win "$SUM");   ci=$(sfield ci "$SUM")
    ccul=$(sfield neural "$SUM"); bcul=$(sfield opp "$SUM")
    selfplay_ok=$(awk -v w="$win" -v c="$ci" 'BEGIN{print (w-c>0.5)?1:0}')
    say "  gate it$it  ARM A self-play : win=$win ci=$ci lo=$(awk -v w="$win" -v c="$ci" 'BEGIN{printf "%.4f", w-c}') vs 0.5000 -> $(okword "$selfplay_ok")  cul=$ccul vs $bcul"
    # Logged, NOT acted on.  Arm B's floor moved on 2026-07-30 because the
    # anchor's shards were measured and found over-dispersed; nobody has
    # measured whether the GATE's shards are.  These three fields are that
    # measurement, accumulating one row per iteration, so the same decision
    # about arm A can be made on evidence rather than by symmetry.  Do not
    # wire them into `selfplay_ok` without a written-up chi2 the way 10.6
    # wrote up the anchor's.
    say "  gate it$it  ARM A cluster   : ci_cluster=$(sfield ci_cluster "$SUM") se_cluster=$(sfield se_cluster "$SUM") chi2=$(sfield chi2 "$SUM") on $(sfield df "$SUM") df overdispersed=$(sfield overdispersed "$SUM")  [logged, not gated]"
  else
    # No games is not a loss.  Refuse to promote on an absent measurement, and
    # write $NULL rather than the 0.0000 that used to land here.
    say "  gate it$it  ARM A self-play : NO DATA -> $(okword 0)"
  fi

  # (4) GATE ARM B -- the frozen champion.  THIS is the arm that kills the
  # treadmill, by making the one yardstick that does not move into a gate
  # instead of a spectator.
  #
  # The test is deliberately NOT "beat the champion" -- the net is currently
  # ~14pp behind it and would never promote again, which would freeze the run
  # rather than fix it.  It is "do not get WORSE against the champion than the
  # net you are replacing": the candidate's anchor win rate must not sit more
  # than one standard error below the incumbent's.
  #
  # One standard error OF THE DIFFERENCE, sqrt(se_cand^2 + se_inc^2), not of
  # either score alone -- both sides are estimates and the comparison has to
  # carry both variances or it rejects on noise.
  #
  # WHICH standard error.  Until 2026-07-30 this read `ci/1.96`, where `ci` was
  # pool_summary's independent-samples binomial half-width over pooled GAMES.
  # That formula cannot see the six shards at all: shards whose spread is twice
  # what binomial noise allows pool to the same number as shards that agree
  # perfectly.  The anchor's own shards do not agree -- chi2 = 11.76 on 5 df
  # against a critical 11.07 -- so the honest interval is the shard-clustered
  # one, which assumes only that disjoint --seed0 ranges are independent (true
  # by construction) and nothing whatever about what happens inside a shard.
  #
  #   old:  se = ci/1.96        = 0.0320/side  -> band 4.52pp
  #   new:  se = se_cluster     = 0.0490/side  -> band 6.93pp
  #
  # The old gate was ~1.5x tighter than the data supports: it demanded more of
  # a candidate than the evidence justified and blocked real improvements, and
  # 74 iterations with zero promotions (docs/NEURAL_LOOP_NULL.md) is the shape
  # that failure makes.  So a candidate now has to give up about 6.9pp on the
  # anchor before arm B blocks it, not 4.5pp.
  #
  # NOT `ci_cluster/1.96`.  ci_cluster is t_{k-1}*se and already carries the
  # t5 = 2.571 factor; dividing it by 1.96 leaves 2.571/1.96 behind and gives
  # 9.09pp, which is too wide by exactly that ratio.  Read `se_cluster`.
  #
  # This is the ONLY arm that moved.  Arm A above still uses the legacy `ci`
  # deliberately: it is the arm that carries the type-I control, moving it
  # would TIGHTEN promotion, and changing two thresholds in one commit would
  # make the discontinuity recorded in curve.tsv uninterpretable.  Arm A's
  # cluster numbers are logged from here on so the evidence to decide that
  # separately accumulates.
  cwin=$NULL; cci=$NULL; iwin=$NULL; ici=$NULL; cse=""; floor=$NULL; anchor_ok=0
  if AS=$(anchor_run checkpoints/cand.pt "loop2/ref_it${it}" "cand it$it"); then
    cwin=$(sfield win "$AS"); cci=$(sfield ci "$AS")
    cse=$(sfield se_cluster "$AS")
    say "  REF it$it  cand vs plan:champion -> $AS"
    if [ -s "$ANCHORF" ]; then
      ise=""
      read -r iwin ici ise < "$ANCHORF"
      if [ -z "$cse" ] || [ -z "$ise" ]; then
        # Fails CLOSED.  Either the candidate's fan-out returned a single
        # shard (a cluster SE needs k>=2 and pool_summary emits `inf`, which
        # sfield refuses to parse), or $ANCHORF is a pre-correction two-field
        # file.  Neither can be repaired with the numbers on hand, and the one
        # reconstruction available -- the legacy ci/1.96 -- is the defect.
        anchor_ok=0
        say "  gate it$it  ARM B anchor   : NO CLUSTER SE (cand='${cse:-}' inc='${ise:-}') -> $(okword 0) (fails closed)"
        say "  gate it$it    re-seed $ANCHORF as 'win ci se' with the incumbent's se_cluster to clear this"
      else
        floor=$(anchor_floor "$iwin" "$cse" "$ise")
        anchor_ok=$(awk -v cw="$cwin" -v f="$floor" 'BEGIN{print (cw >= f) ? 1 : 0}')
        say "  gate it$it  ARM B anchor   : cand=$cwin+-$cci (se=$cse) vs incumbent=$iwin+-$ici (se=$ise) floor=$floor band=$(awk -v cs="$cse" -v is="$ise" 'BEGIN{printf "%.4f", sqrt(cs*cs+is*is)}') -> $(okword "$anchor_ok")"
      fi
    else
      # Seeding failed earlier; abstain rather than block forever.
      anchor_ok=1
      say "  gate it$it  ARM B anchor   : no incumbent baseline -> ABSTAIN (treated as $(okword 1))"
    fi
  else
    # Fails CLOSED, unlike the seed above: once a baseline exists, an anchor
    # run that produced nothing means arm B could not be evaluated, and
    # promoting on an unevaluated gate is exactly the hole this change closes.
    say "  gate it$it  ARM B anchor   : NO DATA -> $(okword 0) (fails closed)"
  fi

  # (5) promote iff BOTH arms pass.  Both are always logged above, so a blocked
  # promotion always names which arm blocked it.
  promote=0
  [ "$selfplay_ok" = 1 ] && [ "$anchor_ok" = 1 ] && promote=1
  if [ "$promote" = "1" ]; then
    install_ckpt checkpoints/cand.pt "$BEST"
    install_ckpt checkpoints/cand.pt "checkpoints/promoted_s_it${it}.pt"
    # The new incumbent's anchor is the score we just measured for it.  Written
    # only on promotion, so it always describes whatever $BEST currently is.
    # Three fields: `win ci se`.  The se is the one the next iteration's floor
    # reads, so refuse to write a baseline without it -- a two-field file makes
    # arm B fail closed and that is better than a baseline whose SE has to be
    # guessed at.
    if [ -n "$cwin" ] && [ "$cwin" != "$NULL" ] && [ -n "$cci" ] && [ -n "$cse" ]; then
      printf '%s %s %s\n' "$cwin" "$cci" "$cse" > "$ANCHORF"
    else
      say "  WARNING it$it  promoted but not re-seeding $ANCHORF (win=$cwin ci=$cci se=${cse:-<none>});"
      say "  WARNING it$it    the incumbent baseline still describes the PREVIOUS net"
    fi
    say "  PROMOTED it$it  win=$win ci=$ci cul=$ccul vs $bcul  anchor=$cwin"
  else
    say "  kept best   it$it  cand win=$win ci=$ci cul=$ccul vs $bcul  anchor=$cwin  (A=$(okword "$selfplay_ok") B=$(okword "$anchor_ok"))"
  fi

  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
    "$it" "$promote" "$win" "$ci" "$ccul" "$bcul" "$dis" \
    "${cwin:-$NULL}" "$(date +%s)" "${cci:-$NULL}" "${iwin:-$NULL}" \
    "$selfplay_ok" "$anchor_ok" >> "$CURVE"

  # keep disk bounded
  old=$((it-WINDOW))
  [ "$old" -ge 1 ] && rm -f iterdata2/it${old}_w*.npz 2>/dev/null
done
say "LOOP DONE $(date)"
