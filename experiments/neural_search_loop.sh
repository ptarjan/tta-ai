#!/usr/bin/env bash
# SEARCH-BACKED self-play loop for the TtA value net (2p).  Every stage is a
# Rust binary; there is no Python anywhere in this file or below it.
#
# The one change that matters, and it has not changed: the improvement
# operator is a SEARCH.  The old loop labelled its ranking pairs with the
# net's own 1-ply argmax, so the untrained warm-start already satisfied 97.6%
# of them and the loss had nothing to teach (docs/NEURAL_LOOP_NULL.md).  Here
# the label is the root choice of the whole-turn beam with this same net as
# the leaf (`rankdata --teacher nplan:...`), which is measurably stronger than
# the net's own argmax, and the gate is beam-vs-beam, i.e. the policy that
# actually ships.
#
# STAGE 0 (once): bootstrap a LEAF evaluator from the strongest bot on record,
# `plan:champion_2p` (culture 189 mirror / 213 vs book, against a human 159.5).
# The net has never been trained on a turn-boundary state, which is the only
# kind of state a beam leaf is ever asked about.
#
# ---------------------------------------------------------------------------
# WHAT THE RUST PORT CHANGED, AND WHAT IT DELIBERATELY DID NOT
#
# Stage-for-stage the pipeline is the same pipeline.  Four things are new:
#
#  1. THE CHECKPOINTS ARE NOT TORCH FILES.  `neuraltrain` writes this repo's
#     own format (rust/src/bots/neural/net.rs), which nothing can confuse with
#     a `.pt`.  So the extensions here are `.ckpt`, and a box that still has a
#     Python-era `checkpoints/best_search.pt` will NOT pick it up: stage 0
#     fires and the lineage restarts, which is correct.  There is no converter
#     and there should not be one -- a lineage whose provenance is half torch
#     and half not is exactly the thing curve.tsv's comment rows exist to stop
#     someone plotting straight through.
#
#  2. NO FAN-OUT.  Every stage is ONE process with `--threads`.  The old
#     8-worker fan-out over disjoint `--seed0` ranges existed because CPython
#     could not use the box any other way; it is what forced pool_summary.py
#     into existence, and with it the whole ci / ci_cluster / se_cluster /
#     chi2 / overdispersed vocabulary.  None of that survives, because its
#     cause does not.
#
#  3. THE INTERVAL CLUSTERS ON THE DEAL, NOT THE SHARD.  `neuraleval` reports
#     `se=` from rust/src/stats.rs, clustered on the deal -- a strictly finer
#     clustering of the same games, computed from the games themselves rather
#     than from six shard summaries.  It is published SEPARATELY from `ci=`
#     precisely so arm B below never divides a half-width by a critical value
#     it already contains (the mistake pool_summary.py spent forty lines
#     warning about).  Because it is a different estimator from the shard SE,
#     the incumbent baseline lives in a DIFFERENTLY NAMED file (see $ANCHORF):
#     an old two- or three-field shard-clustered baseline must not be read
#     into a deal-clustered floor, and a new name makes that impossible rather
#     than merely discouraged.
#
#  4. THE VALUE ROWS COME FROM THE BEAM'S OWN LEAVES.  `rankdata` collects the
#     positions the teacher's search actually priced, not pre-move mid-turn
#     states.  It reports which it used on its DONE line (`values=`), so a
#     shard's distribution is a recorded fact and not an assumption.
#
# Durability contract with the box owner (do not regress any of this):
#   * reads the repo-root PAUSE file before every worker launch and yields.
#     This loop has only ever been a READER of that flag and still is.  The
#     gaming guard that used to write it was `experiments/gpu_guard.py`, whose
#     actual job was freeing VRAM by killing torch; there is no GPU and no
#     torch in this pipeline any more, so it is gone.  The flag itself is NOT
#     gone: touch PAUSE to park training, delete it to resume.  CPU politeness
#     is what remains of the guard's job, and it is handled by the two things
#     that always did the real work -- the Scheduled Task's Priority 7
#     (below-normal, INHERITED by every child) and the --threads budget below,
#     which leaves cores for the hillclimb league.
#   * runs under the tta_neural_loop Scheduled Task (logon trigger + hourly
#     repetition + RestartOnFailure, Priority 7 = below normal), so it survives
#     reboot, crash and the SSH session going away.
set -u
trap '' HUP
# Resolve the repo from the script's own location, not from a hard-coded
# ~/tta-ai: experiments/rust_league.sh already does this, it survives a
# checkout living anywhere, and it is what makes this driver testable off the
# one box it runs on.
cd "$(dirname "$0")/.." || exit 1

BIN=rust/target/release
RANKDATA=$BIN/rankdata
NEURALTRAIN=$BIN/neuraltrain
NEURALEVAL=$BIN/neuraleval

ITERS=${1:-500}
GAMES=${2:-240}          # beam self-play games per iteration
GATE=${3:-200}           # beam-vs-beam games for the promotion gate
WIDTH=${WIDTH:-8}
NODES=${NODES:-1200}
GENW=${GENW:-6}          # generation threads (the box also runs the league arms)
GATEW=${GATEW:-6}
WINDOW=3
EPOCHS=6
LR=3e-4
LAM=1.0
VWEIGHT=${VWEIGHT:-1}
EPS=0.08
STRIDE=3
KREJ=4
LEAFPD=12
CHAMP=analysis/frozen/champion_2p.json

# Games in the ANCHOR match -- us vs plan:champion_2p, the strongest bot on
# record and the only yardstick in this loop that does not move.  Was 72, run
# on promotion iterations only; both of those were wrong.
#
#   n=72  -> every anchor score ever recorded (0.4028, 0.3472, 0.3680, 0.3958)
#           sits inside every other one's interval, so seven iterations of
#           "progress" are statistically indistinguishable from a flat line.
#           A yardstick that cannot resolve the ~5pp changes we are trying to
#           make is not measuring anything.
#   n=240 -> can.
#
# Running it EVERY iteration is the price of a yardstick that works.
REFN=${REFN:-240}

# The bot specs, written once.  Both sides of the gate go through the same
# variables, so a candidate can never accidentally be measured under a
# different search from the incumbent -- the failure `--search plan` on both
# sides of neural_eval.py used to be guarding against by hand.
SEARCH="width=$WIDTH,nodes=$NODES"
ANCHOR_SPEC="plan:$CHAMP,width=8"

mkdir -p loop2 iterdata2 checkpoints teacherdata
BEST=checkpoints/best_search.ckpt
# THE TRAINING CHAIN, which is not the same object as $BEST and must not be.
#
# $BEST is the net that PLAYS: it generates the self-play data, it is the
# opponent in gate arm A, and it only ever moves when a candidate beats it.
# That gating is right and stays.
#
# $WORK is where TRAINING accumulates.  Until 2026-08-01 there was no such
# thing -- every candidate was trained with `--init "$BEST"`, so with $BEST
# frozen since it33 the loop ran fourteen iterations that each started from
# the identical weights, trained on an overlapping 3-iteration window, and
# were thrown away.  Fourteen independent draws from one point, not a hill
# climb.  The most a candidate could ever be ahead by was ONE iteration of
# training, far below arm A's resolution, so the gate could never pass on
# merit no matter how much real signal the data contained: the stall was
# structural, and the pooled arm A win rate over the whole streak read 0.5000
# exactly as that predicts.
#
# So the chain carries forward across blocked iterations and only the PLAYING
# net waits for the gate -- the standard gated-self-play arrangement.
WORK=checkpoints/work.ckpt
LOG=loop2/master.log
CURVE=loop2/curve.tsv
# Repo-root-relative, and we have already cd'd there.
PAUSE=./PAUSE
PIDFILE=loop2/driver.pid
BEAT=loop2/driver.beat
# The incumbent's own anchor score, carried forward from the iteration that
# promoted it.  THREE fields: `win ci se`.
#
#   win  the incumbent's anchor win rate
#   ci   the 95% half-width.  Printed in the gate line; NOT used in the
#        decision.
#   se   the DEAL-CLUSTERED standard error -- the only field the floor reads.
#
# The FILENAME carries `_deal` on purpose.  The pre-port baselines in
# loop2/anchor_best.txt hold a SHARD-clustered SE, a different estimator over
# a fan-out that no longer happens, and there is no arithmetic that converts
# one into the other.  A new name means an old file is never silently read
# into the new floor; this one is simply absent on the first run and gets
# seeded by measuring the incumbent once, below.
ANCHORF=loop2/anchor_best_deal.txt
# What curve.tsv holds when a measurement did not happen.  Deliberately not a
# number: see `measure` below.
NULL='-'

for b in "$RANKDATA" "$NEURALTRAIN" "$NEURALEVAL"; do
  if [ ! -x "$b" ]; then
    echo "[$(date)] no $b -- build with: cd rust && cargo build --release" >> "$LOG"
    exit 1
  fi
done

# ---- single driver, enforced -----------------------------------------------
# A driver that outlives its task registration is invisible to
# MultipleInstancesPolicy -- which is how a neural_loop.sh bash from Jul 27 was
# still alive two days later while a second one ran.
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
# gen retries mean iteration length has no ceiling.
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
rm -f checkpoints/*.ckpt.tmp.* 2>/dev/null || true

wait_if_paused() {
  local said=0
  while [ -f "$PAUSE" ]; do
    [ "$said" = 0 ] && { echo "[$(date)] PAUSED; holding" >> "$LOG"; said=1; }
    # keep the heartbeat fresh while parked, or a long pause would look like a
    # wedged driver and get reaped by the next task trigger
    touch "$BEAT" 2>/dev/null
    sleep 30
  done
}

say() { echo "$@" | tee -a "$LOG"; }

# say() on STDERR.  `say` tees to stdout, so calling it inside a function whose
# stdout the caller captures with $(...) -- which is every measurement function
# below -- would splice log prose into the SUMMARY string the caller then
# parses with sfield.  The retry path is exactly where that happens: attempt 1
# logs three lines, attempt 2 succeeds, and the caller's "win" becomes those
# three lines plus a number.  Diagnostics go to stderr; only the measurement
# goes to stdout.  Both still reach loop2/master.log (via tee) and master.out
# (via the driver's 2>&1).
sayerr() { echo "$@" | tee -a "$LOG" >&2; }

# Never launch a worker while the box is claimed.  Priority comes from the
# Scheduled Task (<Priority>7</Priority> = below normal) and is INHERITED by
# every child process, so a game always outranks us without any per-process
# wrapper.
low() { wait_if_paused; touch "$BEAT" 2>/dev/null; "$@"; }

# Every stretch where this driver goes quiet for tens of minutes is a `wait` on
# a worker.  The relaunch guard above cannot tell a slow driver from a wedged
# one -- it only reads $BEAT -- and it reaps with kill -9, which is how a
# legitimately slow iteration gets its checkpoint write torn in half.  So beat
# while we wait: this is the single choke point for every phase.
beat_wait() {
  while [ -n "$(jobs -pr)" ]; do
    touch "$BEAT" 2>/dev/null
    sleep 15
  done
  wait
  touch "$BEAT" 2>/dev/null
}

# A long worker with heartbeat coverage: background it so beat_wait can beat,
# then wait for exactly it.  Callers keep their own redirections; they check
# for the output file or the DONE line, not the exit status.
beat_run() { low "$@" & beat_wait; }

# Replace $BEST (or any checkpoint) ATOMICALLY.  cp writes in place, so a kill
# -9 landing mid-copy leaves a truncated best_search.ckpt, and that is the one
# artifact the whole run is building.  Stage into a temp file in the SAME
# directory (same filesystem, or the rename is a copy and not atomic) and
# rename over the destination: a reader then sees either the whole old file or
# the whole new one, never a half-written one.
install_ckpt() {   # install_ckpt SRC DST
  # TWO `local` statements, deliberately.  `local` is a builtin: bash expands
  # ALL of its argument words before the builtin assigns any of them, so
  #
  #     local src=$1 dst=$2 tmp="${dst}.tmp.$$"
  #
  # expands `${dst}` while dst is still unset, and under `set -u` (top of file)
  # that is a FATAL error, not an empty string.  This function is only ever
  # called on a promotion, so the driver died at the exact moment it succeeded:
  # loop2/master.out, 2026-07-30 01:49, `line 176: dst: unbound variable`,
  # after iteration 11 had passed BOTH arms of the gate.  No checkpoint was
  # installed, no curve row was written, and the scheduled task restarted the
  # iteration from scratch -- so a promotion looked exactly like an iteration
  # that never finished.
  local src=$1 dst=$2
  local tmp="${dst}.tmp.$$" i
  cp "$src" "$tmp" || { rm -f "$tmp"; say "  WARNING: could not stage $src -> $tmp; $dst UNCHANGED"; return 1; }
  for i in 1 2 3; do
    mv -f "$tmp" "$dst" 2>/dev/null && return 0
    sleep 2
  done
  # Windows refuses to rename over a file another process still holds open.
  # Falling back to cp is exactly the old behaviour, so this is never worse
  # than before -- but it is the one non-atomic path left, so it says so.
  say "  WARNING: atomic rename over $dst failed 3x; falling back to in-place cp"
  cp "$tmp" "$dst"; rm -f "$tmp"; return 1
}

# ---------------------------------------------------------------- measurement
# Pull a field out of a SUMMARY line.  Returns empty if the field is absent OR
# if its value is `NA`, which is what every caller below tests for -- `win=NA`
# must never parse as a number.
#
# The character class is `[-0-9.]`, NOT `-\?[0-9.]`.  `\?` is a GNU extension:
# BSD sed treats it as a literal `?`, so the old pattern silently matched
# NOTHING on any non-GNU box and every field came back empty -- which this
# file's own fail-closed paths then correctly reported as "no measurement",
# making a portability bug look exactly like a run that produced no games.
# Caught by running this driver on a Mac; the desktop's GNU sed had been
# hiding it.  A bracket expression means the same thing to both seds.
sfield() { printf '%s' "$2" | sed -n "s/.*[[:space:]]$1=\\([-0-9.]*\\).*/\\1/p"; }

# A MEASUREMENT THAT PRODUCED NO GAMES IS NOT A SCORE OF ZERO.
#
# This is the data-integrity half of this file.  `pool_summary.py` used to
# print `win=0.0000 ... n=0` and exit 0 when every shard was missing, and the
# loop wrote that 0.0000 into curve.tsv as if it were an observation.  Row 4 of
# the desktop's curve says `vs_planchamp=0.0000` and what actually happened is
# that the reference run did not run -- indistinguishable, after the fact, from
# the net being beaten 0-72 by the champion.  A plausible number standing in
# for absent work is strictly worse than a gap, because a gap cannot be
# averaged into a trend.
#
# `neuraleval` now emits `win=NA` itself when it played nothing, so this
# function's job is only to notice, retry once (the usual cause is transient),
# and on a second failure return 1 having printed NOTHING.  Callers write
# $NULL to curve.tsv and refuse to let the absent number drive a decision.
measure() {   # measure A_SPEC B_SPEC NGAMES LOGFILE LABEL -> SUMMARY on stdout
  local a=$1 b=$2 n=$3 out=$4 label=$5
  local attempt sum got
  for attempt in 1 2; do
    rm -f "$out"
    low "$NEURALEVAL" --a "$a" --b "$b" --games "$n" --players 2 \
        --threads "$GATEW" --seed 0 > "$out" 2>&1 &
    beat_wait
    sum=$(grep '^SUMMARY' "$out" 2>/dev/null | tail -1)
    got=$(sfield n "$sum")
    if [ -n "$got" ] && [ "${got%%.*}" -gt 0 ] 2>/dev/null; then
      printf '%s\n' "$sum"
      return 0
    fi
    sayerr "  *** NO GAMES: $label produced n=${got:-?} on attempt $attempt/2"
    sayerr "  ***   summary line was: ${sum:-<no output>}  (see $out)"
  done
  sayerr "  *** MEASUREMENT FAILED: $label produced no games twice; recording $NULL, NOT a score."
  sayerr "  ***   A missing measurement is never written as 0.0000 (see measure() in this file)."
  return 1
}

# The promotion gate: beam vs beam, the policy that actually ships.
gate_run() {        # gate_run CAND OPP LOGFILE
  measure "nplan:$1,$SEARCH" "nplan:$2,$SEARCH" "$GATE" "$3" \
          "gate $(basename "$1") vs $(basename "$2")"
}

# The anchor match: us vs the strongest bot on record.
anchor_run() {      # anchor_run CKPT LOGFILE LABEL
  measure "nplan:$1,$SEARCH" "$ANCHOR_SPEC" "$REFN" "$2" "anchor $3 vs plan:champion"
}

# ---- ARM B FLOOR (single source of truth) -----------------------------------
# One standard error OF THE DIFFERENCE below the incumbent's anchor.
#
# The two arguments are STANDARD ERRORS, not half-widths.  Read them from
# neuraleval's `se=` field and pass them straight through.  Do NOT reconstruct
# one by dividing a half-width by anything: `ci=` is t_{k-1}*se and already
# carries the critical value, so dividing it by 1.96 leaves t_{k-1}/1.96
# behind.  `se=` is published separately so no caller ever has to divide.
anchor_floor() {    # anchor_floor INC_WIN CAND_SE INC_SE -> floor on stdout
  awk -v iw="$1" -v cs="$2" -v is="$3" \
    'BEGIN{ printf "%.4f", iw - sqrt(cs*cs + is*is) }'
}

okword() { [ "${1:-0}" = 1 ] && printf 'PASS' || printf 'BLOCK'; }

# curve.tsv schema.  The last four columns:
#   vs_planchamp  the CANDIDATE's anchor win rate this iteration.  This is the
#                 number the gate tests, so it is the one that has to be here.
#   anchor_ci     its 95% half-width, so a reader can tell 0.36 +-0.06 from
#                 0.36 +-0.11 without going back to master.log.
#   inc_anchor    the INCUMBENT's anchor at decision time.  The curve of the
#                 net that actually ships is this column, not vs_planchamp;
#                 they coincide exactly on promotion rows.
#   selfplay_ok / anchor_ok
#                 the two promotion criteria, separately, so a blocked
#                 promotion says which half blocked it.
# Any of them may be '-' ($NULL): see measure().
#
# COMMENT ROWS.  A line whose first character is '#' is an annotation, not an
# observation.  It exists because some events break the comparability of the
# rows either side of them and a reader who plots the column straight through
# gets a lie.  Two rules make a comment row safe, and both are enforced below:
#   * it does not advance the iteration counter (see start_it);
#   * the schema migration passes it through verbatim instead of padding it
#     out to 13 fields, which would turn prose into data.
CURVE_HDR=$(printf 'iter\tpromoted\twin\tci\tcand_cul\tbest_cul\tdisagree\tvs_planchamp\tts\tanchor_ci\tinc_anchor\tselfplay_ok\tanchor_ok')
CURVE_NCOL=13
RUSTMARK='# ---- Rust pipeline starts here'
if [ ! -f "$CURVE" ]; then
  printf '%s\n' "$CURVE_HDR" > "$CURVE"
elif [ "$(head -1 "$CURVE" 2>/dev/null)" != "$CURVE_HDR" ]; then
  # Migrate in place rather than starting a new file: the anchor curve has to
  # stay continuous, and a header that disagrees with its own rows is how a
  # reader ends up plotting anchor_ci as a win rate.
  say "migrating $CURVE to the ${CURVE_NCOL}-column schema (old rows padded with '$NULL')"
  awk -v hdr="$CURVE_HDR" -v n="$CURVE_NCOL" -v nul="$NULL" \
      'BEGIN{FS=OFS="\t"; print hdr} NR>1 && /^#/{print; next} NR>1{for(i=NF+1;i<=n;i++)$i=nul; print}' \
      "$CURVE" > "$CURVE.mig" && mv -f "$CURVE.mig" "$CURVE"
fi
# The comparability break this port IS.  A new engine, a new checkpoint
# format, a new lineage from stage 0, and an interval clustered on the deal
# instead of the shard: rows before this marker and rows after it are two
# different rulers, and splicing them into one series invents a trend no
# measurement supports.  Exactly the convention commit 96a5db2 forced when it
# changed how the frozen champion plays.  Written once, idempotently.
if ! grep -qF "$RUSTMARK" "$CURVE" 2>/dev/null; then
  printf '%s (%s): new lineage from stage 0, deal-clustered intervals; do not plot across this line\n' \
    "$RUSTMARK" "$(date +%F)" >> "$CURVE"
fi

# ---------------------------------------------------------------- STAGE 0
# Bootstrap the leaf evaluator from plan:champion self-play.  Skipped once
# $BEST exists, so a reboot mid-run resumes the loop instead of redoing this.
if [ ! -f "$BEST" ]; then
  say "== STAGE 0 teacher bootstrap  $(date) =="
  # COMPLETE is the only signal that the teacher set is whole.  Without this
  # sentinel a kill 30 seconds into generation would leave a fraction of the
  # data on disk and the next pass would train on it as if it were the full
  # set.  `rankdata` prints DONE only after its final flush, so DONE is the
  # thing to look for, not "some shards exist".
  tries=0
  while [ ! -f teacherdata/COMPLETE ] && [ "$tries" -lt 6 ]; do
    tries=$((tries+1))
    rm -rf teacherdata; mkdir -p teacherdata
    TG=${TEACHER_GAMES:-480}
    beat_run "$RANKDATA" --teacher "$ANCHOR_SPEC" --players 2 \
        --games "$TG" --stride "$STRIDE" --krej "$KREJ" \
        --leaf-per-decision "$LEAFPD" --epsilon 0.05 --seed0 0 \
        --threads "$GENW" --out teacherdata/tg \
        > loop2/teacher.log 2>&1
    tdone=$(grep -c '^DONE' loop2/teacher.log 2>/dev/null || echo 0)
    say "  teacher attempt $tries: $(ls teacherdata/*.rkd 2>/dev/null | wc -l) shards, $(grep -h '^DONE' loop2/teacher.log 2>/dev/null)"
    [ "$tdone" -ge 1 ] && touch teacherdata/COMPLETE
  done
  [ -f teacherdata/COMPLETE ] || { say "  STAGE 0 could not complete generation; will retry on next task start"; exit 1; }
  beat_run "$NEURALTRAIN" --data teacherdata --threads "$GENW" \
      --epochs 20 --lr 1e-3 --lam "$LAM" --vweight "$VWEIGHT" \
      --out checkpoints/boot.ckpt > loop2/train_boot.log 2>&1
  [ -f checkpoints/boot.ckpt ] || { say "  STAGE 0 training produced no checkpoint; aborting"; exit 1; }
  say "  STAGE 0 $(grep -h '^VACUITY' loop2/train_boot.log 2>/dev/null)"
  install_ckpt checkpoints/boot.ckpt "$BEST"
  say "  STAGE 0 done -> $BEST"
fi

# Seed the training chain from the playing net.  Only when it is absent: on a
# restart the chain is exactly the state worth keeping, and re-seeding it here
# would silently restore the every-iteration-resets-to-BEST behaviour that made
# the loop unable to accumulate in the first place -- invisibly, because a
# restart looks like nothing happened.
if [ ! -f "$WORK" ]; then
  install_ckpt "$BEST" "$WORK"
  say "  seeded training chain $WORK from $BEST"
fi

say "LOOP START $(date)  best=$BEST work=$WORK width=$WIDTH nodes=$NODES games=$GAMES gate=$GATE refn=$REFN"

# ------------------------------------------------------ incumbent anchor seed
# Arm B compares the candidate's score against the frozen champion with the
# INCUMBENT's score against that same champion.  The incumbent's score is
# normally inherited from the iteration that promoted it -- it was measured
# then, as that iteration's candidate, so steady state costs nothing extra.
# But on a fresh box, and on the first run after the port, there is no such
# record, and an arm with no baseline is an arm that always passes.
#
# So measure it once, here, instead of defaulting to a number.  This is the
# ONLY place the anchor gate fails open, and it says so when it does.
if [ ! -s "$ANCHORF" ]; then
  say "no incumbent anchor on record -- measuring $BEST vs plan:champion once to seed arm B"
  if AS=$(anchor_run "$BEST" loop2/anchor_seed.log "incumbent"); then
    # Only ever write three real numbers.  A blank field here would read back
    # as awk 0, which silently turns arm B's floor into "beat the incumbent
    # exactly" -- a gate that is not the gate anyone wrote, which is worse than
    # no gate because it looks like one.  `se` is the field the floor reads; a
    # seed without it is not a baseline.
    sw=$(sfield win "$AS"); sc=$(sfield ci "$AS"); ss=$(sfield se "$AS")
    if [ -n "$sw" ] && [ -n "$sc" ] && [ -n "$ss" ]; then
      printf '%s %s %s\n' "$sw" "$sc" "$ss" > "$ANCHORF"
      say "  seeded incumbent anchor: $AS"
    else
      say "  *** anchor seed returned games but no parseable win/ci/se: $AS"
      say "  *** (a deal-clustered se needs >=2 deals; a single-deal seed cannot bound itself)"
    fi
  else
    say "  *** could not seed the incumbent anchor; arm B ABSTAINS until a promotion sets one"
  fi
fi

# The next iteration number is "how many observations are on file, plus one".
# Count OBSERVATIONS, not lines: comment rows are annotations and must not
# consume an iteration number, or every marker anyone ever writes silently
# punches a hole in the sequence -- iteration 11 missing from a curve reads as
# a crashed iteration, not as a note someone left.
start_it=$(( $(awk '/^#/{next} {n++} END{print n-1}' "$CURVE" 2>/dev/null || echo 0) + 1 ))
[ "$start_it" -lt 1 ] && start_it=1
for it in $(seq "$start_it" "$ITERS"); do
  say "== ITER $it  $(date) =="
  touch "$BEAT" 2>/dev/null

  # (1) beam self-play generation with BEST.  Retried as a whole if a worker
  # was killed partway: a half-generated iteration is a silently smaller and
  # seed-biased training set, not a smaller-but-fine one.
  GENDIR=iterdata2/it${it}
  GENLOG=loop2/gen_it${it}.log
  gtries=0
  while [ "$gtries" -lt 4 ]; do
    gtries=$((gtries+1))
    rm -rf "$GENDIR"; mkdir -p "$GENDIR"
    beat_run "$RANKDATA" --teacher "nplan:$BEST,$SEARCH" --players 2 \
        --games "$GAMES" --stride "$STRIDE" --krej "$KREJ" \
        --leaf-per-decision "$LEAFPD" --epsilon "$EPS" \
        --seed0 $(( it*100000 )) --threads "$GENW" --out "$GENDIR/w" \
        > "$GENLOG" 2>&1
    grep -q '^DONE' "$GENLOG" 2>/dev/null && break
    say "  gen it$it attempt $gtries incomplete -- retrying"
  done
  # health meter: the fraction of decisions where the beam OVERRULED the net's
  # own 1-ply argmax.  This is the entire information content of the target.
  # If it decays toward 0 the loop has gone vacuous and must be stopped --
  # that is precisely the failure docs/NEURAL_LOOP_NULL.md 3.1 could not see.
  # `rankdata` prints NA, not 0, when it could not measure it; NA must not be
  # read as "vacuous", so the warning below tests a real number only.
  dis=$(grep -h '^DONE' "$GENLOG" 2>/dev/null \
        | sed -n 's/.*DISAGREE=\([0-9.]*\).*/\1/p' | tail -1)
  [ -z "$dis" ] && dis=$NULL
  say "  gen it$it  DISAGREE=$dis  shards=$(ls "$GENDIR"/*.rkd 2>/dev/null | wc -l)  $(grep -h '^DONE' "$GENLOG" 2>/dev/null)"
  if [ "$dis" != "$NULL" ] && awk -v d="$dis" 'BEGIN{exit !(d+0 < 0.02)}'; then
    say "  *** WARNING DISAGREE=$dis < 0.02: the search no longer overrules the net; the target is going vacuous (docs/NEURAL_LOOP_NULL.md 3.1) ***"
  fi

  if ! ls "$GENDIR"/*.rkd >/dev/null 2>&1; then
    say "  no gen data -> retry iter $it"; continue
  fi

  # (2) train a candidate warm-started from the CHAIN on the replay window.
  # `neuraltrain --data` takes directories, so the window is a list of them.
  data=""
  for k in $(seq 0 $((WINDOW-1))); do
    j=$((it-k))
    [ "$j" -ge 1 ] && [ -d "iterdata2/it${j}" ] && data="$data --data iterdata2/it${j}"
  done
  # a stale cand from a killed iteration would otherwise be gated as if it
  # were this iteration's candidate
  rm -f checkpoints/cand.ckpt
  # shellcheck disable=SC2086  # $data is a deliberately word-split flag list
  beat_run "$NEURALTRAIN" $data --data teacherdata --threads "$GENW" \
      --init "$WORK" --epochs "$EPOCHS" --lr "$LR" --lam "$LAM" \
      --vweight "$VWEIGHT" --out checkpoints/cand.ckpt \
      > "loop2/train_it${it}.log" 2>&1
  [ -f checkpoints/cand.ckpt ] || { say "  no cand (killed?) -> skip iter $it"; continue; }
  # The epoch-0 line is the guard against training that is secretly a no-op:
  # it scores the warm start on its own validation pairs BEFORE a single
  # gradient step.  A pair accuracy already at ~1.0 is docs/NEURAL_LOOP_NULL.md
  # 3.1 happening again, and neuraltrain says VACUOUS TARGET out loud when it
  # crosses the threshold.  Both lines are surfaced here so a reader of
  # master.log alone can see it.
  say "  train it$it  $(grep -h '^VACUITY' "loop2/train_it${it}.log" 2>/dev/null)"
  grep -h 'VACUOUS TARGET' "loop2/train_it${it}.log" 2>/dev/null | while read -r l; do
    say "  *** $l"
  done

  # (3) GATE ARM A -- self-play: beam vs beam, the policy that actually ships.
  # This is the criterion the loop has always had.  On its own it is
  # self-referential: it only ever asks "is this better than the last thing
  # this same process produced", which is satisfied by drift as readily as by
  # learning.  Seven iterations of it moved self-play culture 116 -> 143 while
  # the fixed anchor did not move at all.  Hence arm B.
  win=$NULL; ci=$NULL; ccul=$NULL; bcul=$NULL; selfplay_ok=0
  if SUM=$(gate_run checkpoints/cand.ckpt "$BEST" "loop2/gate_it${it}.log"); then
    win=$(sfield win "$SUM");   ci=$(sfield ci "$SUM")
    ccul=$(sfield a_cul "$SUM"); bcul=$(sfield b_cul "$SUM")
    if [ -n "$win" ] && [ -n "$ci" ]; then
      selfplay_ok=$(awk -v w="$win" -v c="$ci" 'BEGIN{print (w-c>0.5)?1:0}')
      say "  gate it$it  ARM A self-play : win=$win ci=$ci lo=$(awk -v w="$win" -v c="$ci" 'BEGIN{printf "%.4f", w-c}') vs 0.5000 -> $(okword "$selfplay_ok")  cul=$ccul vs $bcul"
    else
      # Games were played but the interval is unbounded (one deal cannot
      # bound itself, and neuraleval prints NA rather than inf so this cannot
      # silently compare true).  Fails closed.
      win=$NULL; ci=$NULL
      say "  gate it$it  ARM A self-play : NO INTERVAL -> $(okword 0) (fails closed)"
    fi
    say "  gate it$it  ARM A detail    : se=$(sfield se "$SUM") deals=$(sfield deals "$SUM") n=$(sfield n "$SUM")"
  else
    # No games is not a loss.  Refuse to promote on an absent measurement, and
    # write $NULL rather than the 0.0000 that used to land here.
    say "  gate it$it  ARM A self-play : NO DATA -> $(okword 0)"
  fi

  # (4) GATE ARM B -- the frozen champion.  THIS is the arm that kills the
  # treadmill, by making the one yardstick that does not move into a gate
  # instead of a spectator.
  #
  # The test is deliberately NOT "beat the champion" -- the net is behind it
  # and would never promote again, which would freeze the run rather than fix
  # it.  It is "do not get WORSE against the champion than the net you are
  # replacing": the candidate's anchor win rate must not sit more than one
  # standard error below the incumbent's.
  #
  # One standard error OF THE DIFFERENCE, sqrt(se_cand^2 + se_inc^2), not of
  # either score alone -- both sides are estimates and the comparison has to
  # carry both variances or it rejects on noise.
  #
  # WHICH standard error: the DEAL-CLUSTERED one, `se=`, straight from
  # neuraleval.  Arm A above still tests against the reported `ci` for
  # continuity, and moving it would TIGHTEN promotion; changing two thresholds
  # at once would make the discontinuity in curve.tsv uninterpretable.
  cwin=$NULL; cci=$NULL; iwin=$NULL; ici=$NULL; cse=""; floor=$NULL; anchor_ok=0
  if AS=$(anchor_run checkpoints/cand.ckpt "loop2/ref_it${it}.log" "cand it$it"); then
    cwin=$(sfield win "$AS"); cci=$(sfield ci "$AS")
    cse=$(sfield se "$AS")
    say "  REF it$it  cand vs plan:champion -> $AS"
    if [ -s "$ANCHORF" ]; then
      ise=""
      read -r iwin ici ise < "$ANCHORF"
      if [ -z "$cse" ] || [ -z "$ise" ] || [ -z "$cwin" ]; then
        # Fails CLOSED.  Either the candidate's run returned a single deal (a
        # cluster SE needs k>=2 and neuraleval prints NA, which sfield refuses
        # to parse) or the baseline file is malformed.  Neither can be
        # repaired with the numbers on hand, and the one reconstruction
        # available -- dividing a half-width by a critical value it already
        # contains -- is itself the defect.
        anchor_ok=0
        say "  gate it$it  ARM B anchor   : NO CLUSTER SE (cand='${cse:-}' inc='${ise:-}') -> $(okword 0) (fails closed)"
        say "  gate it$it    delete $ANCHORF to have the next run re-seed it"
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
    # promoting on an unevaluated gate is exactly the hole this arm closes.
    say "  gate it$it  ARM B anchor   : NO DATA -> $(okword 0) (fails closed)"
  fi

  # (5) promote iff BOTH arms pass.  Both are always logged above, so a blocked
  # promotion always names which arm blocked it.
  promote=0
  [ "$selfplay_ok" = 1 ] && [ "$anchor_ok" = 1 ] && promote=1
  if [ "$promote" = "1" ]; then
    install_ckpt checkpoints/cand.ckpt "$BEST"
    install_ckpt checkpoints/cand.ckpt "checkpoints/promoted_s_it${it}.ckpt"
    # The new incumbent's anchor is the score we just measured for it.  Written
    # only on promotion, so it always describes whatever $BEST currently is.
    # Refuse to write a baseline without the se the next floor reads -- a
    # baseline whose SE has to be guessed at is worse than none, because arm B
    # fails closed on none and says so.
    if [ -n "$cwin" ] && [ "$cwin" != "$NULL" ] && [ -n "$cci" ] && [ -n "$cse" ]; then
      printf '%s %s %s\n' "$cwin" "$cci" "$cse" > "$ANCHORF"
    else
      say "  WARNING it$it  promoted but not re-seeding $ANCHORF (win=$cwin ci=$cci se=${cse:-<none>});"
      say "  WARNING it$it    the incumbent baseline still describes the PREVIOUS net"
    fi
    install_ckpt checkpoints/cand.ckpt "$WORK"
    say "  PROMOTED it$it  win=$win ci=$ci cul=$ccul vs $bcul  anchor=$cwin"
  else
    # THE CHAIN ADVANCES ON A BLOCKED ITERATION.  That is the entire point of
    # $WORK: a blocked gate means "not yet demonstrably better than the net
    # that plays", which is not the same as "this iteration of training was
    # worthless" and must not throw it away.  Only the PLAYING net waits.
    #
    # Unless the chain has gone measurably BAD, in which case it is walking
    # away from $BEST rather than toward a promotion, and every further
    # iteration compounds it.  The reset test is the promotion test with the
    # inequality reversed -- promote when the interval clears 0.5 from above
    # (win-ci>0.5), reset when it clears 0.5 from below (win+ci<0.5) -- so both
    # directions demand the same strength of evidence and the wide middle,
    # where the data cannot tell, is where the chain is simply left to keep
    # learning.  A threshold picked by feel would have had to justify itself;
    # this one is just arm A read the other way round.
    if [ "$win" = "$NULL" ] || [ "$ci" = "$NULL" ]; then
      # Arm A produced no measurement, so neither test can be evaluated.  The
      # training itself still happened and is still built on $WORK, so carry it
      # -- but say that it was carried unverified rather than let an unmeasured
      # iteration look like a measured one.
      install_ckpt checkpoints/cand.ckpt "$WORK"
      chain="chain advanced UNVERIFIED (arm A had no data)"
    elif [ "$(awk -v w="$win" -v c="$ci" 'BEGIN{print (w+c<0.5)?1:0}')" = 1 ]; then
      install_ckpt "$BEST" "$WORK"
      chain="chain RESET to $BEST (hi=$(awk -v w="$win" -v c="$ci" 'BEGIN{printf "%.4f", w+c}') < 0.5000: significantly worse)"
    else
      install_ckpt checkpoints/cand.ckpt "$WORK"
      chain="chain advanced"
    fi
    say "  kept best   it$it  cand win=$win ci=$ci cul=$ccul vs $bcul  anchor=$cwin  (A=$(okword "$selfplay_ok") B=$(okword "$anchor_ok"))  $chain"
    # A STALLED LOOP LOOKS EXACTLY LIKE A WORKING ONE, LINE BY LINE.  Every
    # iteration prints a well-formed "kept best", the box stays busy, the
    # process list looks perfect, and nothing anywhere says the RUN has stopped
    # moving.  That is precisely how docs/NEURAL_LOOP_NULL.md happened: 74
    # iterations, zero promotions, discovered only because a human happened to
    # read back through the log.  It happened again on 2026-07-31 (it33 -> it47,
    # ~11 hours).  Twice is a failure mode, not an accident, so the loop reports
    # its own stall instead of waiting to be caught.
    #
    # The streak LENGTH is not the interesting number -- arm A's POOLED win rate
    # over it is.  A streak sitting at 0.50 is a converged net that the gate is
    # correctly refusing to churn, and the answer is to stop the loop or change
    # what it trains on.  A streak sitting near 0.57 with per-iteration CIs too
    # wide to clear 0.5000 is a REAL improvement the gate cannot resolve at this
    # game count, and the answer is the opposite: raise n.  Reporting only the
    # streak would leave those two indistinguishable, which is the same
    # never-chosen-vs-never-offered ambiguity a bare zero always has.
    #
    # `m` counts separately from `n` because an iteration whose arm A returned
    # NO DATA writes $NULL ('-') in this column: it is a real link in the
    # no-promotion streak but contributes no win rate, and summing it as zero
    # would drag the pooled number toward "converged" for a reason that has
    # nothing to do with the net.  Both counts are reported so a pooled rate
    # over 3 of 11 iterations cannot be read as one over 11.
    stall_warn_after=8
    stall=$(awk -F'\t' -v w="$stall_warn_after" '
      /^#/ {next}
      $2==1 {n=0; m=0; s=0; next}
      $2==0 {n++; if ($3+0==$3 && $3!="") {m++; s+=$3}}
      END {if (n>=w && m>0) printf "%d %d %.4f", n, m, s/m}' "$CURVE" 2>/dev/null)
    if [ -n "$stall" ]; then
      set -- $stall
      say "  STALL it$it  $1 iterations without a promotion (through it$((it-1))); arm A pooled win=$3 over $2 of them"
      say "  STALL it$it    ~0.50 => converged: the gate is right, change the training, not the threshold"
      say "  STALL it$it    >0.53 => under-powered: real gain the per-iteration CI cannot resolve, raise the game count"
    fi
  fi

  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
    "$it" "$promote" "$win" "$ci" "$ccul" "$bcul" "$dis" \
    "${cwin:-$NULL}" "$(date +%s)" "${cci:-$NULL}" "${iwin:-$NULL}" \
    "$selfplay_ok" "$anchor_ok" >> "$CURVE"

  # keep disk bounded
  old=$((it-WINDOW))
  [ "$old" -ge 1 ] && rm -rf "iterdata2/it${old}" 2>/dev/null
done
say "LOOP DONE $(date)"
