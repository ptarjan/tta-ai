#!/bin/bash
# Keep the three league arms alive unattended.
#
# Runs from cron every 10 minutes.  For each player count it checks whether a
# run_league.sh supervisor is alive and relaunches it if not, until DEADLINE
# passes -- then it stops relaunching and removes its own cron entry's reason
# to exist (the entry is harmless after that; it just no-ops).
#
# Why this exists: the arms are meant to run 48h unattended.  A supervisor can
# die for reasons run_league.sh cannot catch itself -- an OOM kill, a reboot,
# a terminal closing on a stray foreground launch.  Without this, an arm that
# dies at hour 3 is simply gone for the remaining 45.
#
# DEADLINE is an absolute epoch second, written at setup time.  Cron gives no
# clean way to say "stop after N hours", so the deadline lives in a file.
#
# ---------------------------------------------------------------------------
# HOW THE FLAGS ARE ORGANISED, AND WHY IT IS NOT NEGOTIABLE
# ---------------------------------------------------------------------------
# `--candidate-bot`, `--objective`, `--hall-dir`, `--human-bots`,
# `--pool-weights` and `--past-k` are NOT persisted in the state dir
# (docs/HAZARDS.md trap 5).  An arm the watchdog relaunches without one of
# them keeps training -- against a different, weaker configuration, with
# nothing crashing and nothing in the logs saying so except the `[pool]` line.
# This project has already hit that exact mode once.
#
# It used to be enforced by "ONE array, no per-arm copies".  That worked while
# all three arms were identical and stopped working on 2026-07-29, when the
# converged 2p arm was retargeted to train under PlanBot (docs/TRAINING_RUN.md)
# and the arms stopped being identical.  So the structure is now:
#
#   COMMON      every flag that is the same for all three arms.  Still one
#               array, still no copies.
#   arm_flags   the ONLY place an arm may differ, one `case` branch each, so
#               the difference between the arms is a three-line diff you can
#               read in one screen rather than three drifting arrays.
#   REQUIRED    the flags that are not persisted.  `launch` asserts each
#               appears EXACTLY ONCE in the assembled command line and
#               REFUSES to start the arm otherwise.
#
# The refusal is the point.  A dead arm is loud -- `pgrep -f run_league.sh`
# shows two supervisors instead of three -- while a silently mis-configured
# arm looks perfectly healthy for two days and produces a champion trained on
# something nobody chose.  Given the choice, fail loudly.
#
# `launch` refuses for a second reason too: experiments/logs/stop_league_Kp.json,
# the zero-game stop sentinel.  See the comment on it in `launch`.
#
# The receipts, after ANY relaunch, in experiments/logs/league_Kp.log:
#     [Kp] objective: ...
#     [Kp] trained architecture: ...
#     [Kp] saturation: ...
#     [pool] ...
set -u
cd "$(dirname "$0")/.."
DEADLINE_FILE=experiments/logs/watchdog_deadline
LOG=experiments/logs/watchdog.log

# ---------------------------------------------------------------------------
# REAP: the sentinel stops a RUNNING arm, not just a relaunch
# ---------------------------------------------------------------------------
# `launch` below refuses to START an arm whose sentinel exists, and that was
# taken to mean the sentinel stops the arm.  It does not.  run_league.sh tests
# the sentinel too, but bash parses its `while` loop ONCE, at supervisor start
# -- so an arm launched before that test was written executes a loop body that
# has never heard of the file, and no edit to run_league.sh can reach it.  On
# 2026-07-30 that was all three arms, and 3p spun on a 60-second
# refuse/sleep/restart cycle that would have run to the deadline looking
# perfectly healthy in `ps` (docs/CARD_BLINDNESS.md 9).
#
# The fix belongs HERE and not there, because this file is what cron
# re-executes from disk every ten minutes: fixing it fixes the arms already
# running.  So the sentinel is now sufficient on its own -- write it, wait up
# to ten minutes (or run this script by hand), and the arm is gone.
#
# The kill is per-arm and by matched command line: the supervisor first so it
# cannot restart the climber, then the climber and its workers.  TERM, not
# KILL -- a climber is between generations far more often than it is inside a
# state write, and TERM lets it unwind.  This runs BEFORE the deadline check
# on purpose: a halt must work even after the budget has expired.
reap() {
    local K sup clim
    for K in 2 3 4; do
        [ -f "experiments/logs/stop_league_${K}p.json" ] || continue
        sup=$(pgrep -f "run_league.sh $K " || true)
        clim=$(pgrep -f "hillclimb_league --players $K " || true)
        [ -n "$sup$clim" ] || continue
        [ -n "$sup" ] && kill -TERM $sup 2>/dev/null
        sleep 2
        [ -n "$clim" ] && kill -TERM $clim 2>/dev/null
        echo "$(date '+%F %T') watchdog: REAPED ${K}p on its stop sentinel -- supervisor(s) [${sup:-none}] climber(s) [${clim:-none}].  The sentinel alone does not stop a supervisor launched before run_league.sh learned to check it." >> "$LOG"
    done
}
reap

[ -f "$DEADLINE_FILE" ] || exit 0
DEADLINE=$(cat "$DEADLINE_FILE")
NOW=$(date +%s)
if [ "$NOW" -ge "$DEADLINE" ]; then exit 0; fi

# Hours remaining, rounded up -- a relaunched supervisor gets only the time
# left on the original budget, not a fresh 48h.
REMAIN=$(( (DEADLINE - NOW + 3599) / 3600 ))
[ "$REMAIN" -lt 1 ] && REMAIN=1

# --objective blend    accept on (1-alpha)*lead_share + alpha*win share, where
#                      the LEAD is own final culture minus the BEST opponent's.
#                      Zero lead is exactly the win/lose boundary, so the curve
#                      needs no fitted "typical score" centre -- the previous
#                      default scored ABSOLUTE own culture squashed around an
#                      assumed 100, which was measurably stale within a month.
#                      docs/LEAGUE_OBJECTIVE.md.
# --pool-weights       the tier totals: 32% of the training signal on fixed
#                      external opponents (book / human archetypes / strategy
#                      variants) and 68% on opponents that improve (mirror /
#                      past ladder / frozen hall).  It was 69% external before
#                      the 2026-07-27 rebalance.  Passed EXPLICITLY rather than
#                      relying on the module default, so the log records the
#                      pool this run actually used even if the default later
#                      moves.  floor=0 drops greedy/random/default: they are
#                      saturated (docs/HAZARDS.md trap 2) and under
#                      own-culture scoring they stop being inert and start
#                      pulling.
# --saturation         AUTOMATIC PRUNING, docs/LEAGUE_POOL.md.  An opponent's
#                      share of its tier is scaled by its measured win rate
#                      from the full pool check: full weight at or below 70%,
#                      down to 0.15 at 95% and above, where it also stops
#                      being drawn into the acceptance rotation.  A 98% win
#                      rate cannot go up, so those games bought no gradient.
#                      The freed weight stays INSIDE the tier, so the 32/68
#                      external/self-play split above is untouched and the
#                      pool cannot collapse into pure self-play.
# --past-k 6           the self-ladder, newest-biased (offsets 0,1,3,7,15 from
#                      the newest, plus the founder).  It was 2, which under
#                      even spreading meant exactly ONE informative self-play
#                      opponent (the newest, ~50%) and one saturated founder
#                      (~96%).  Six recent selves put several opponents in the
#                      50-70% band where a mutation can show.
COMMON=(
    --weight-guard clamp
    --past-k 6
    --hall-dir experiments/hall_of_fame
    --human-bots all
    --objective blend
    --objective-alpha 0.15
    --saturation 0.70,0.95,0.15
    --pool-weights book=0.6,variant=0.6,human=0.6,mirror=1.0,past=1.2,hall=1.6,floor=0
)

# Not persisted in the state dir; a launch missing one of these is refused.
REQUIRED="--candidate-bot --objective --hall-dir --human-bots --pool-weights --past-k --saturation"

arm_flags() {   # players -> the flags that are NOT shared, space separated
    case "$1" in
    2)
        # 2p CONVERGED under the quiescent proxy: 8 accepts in its last 100
        # generations, culture flat in a 128-149 band.  A converged arm is
        # spending compute to re-measure a number that has stopped moving, so
        # it is retargeted to train under the search we would SHIP.
        #
        # width=2, from `tools/arch_cost.py --players 2 --weights <the 2p
        # champion>` (cpu-s/game, TTA_JOURNAL=1, workers=1) -- measured AT 2p
        # ON THE CHAMPION, not extrapolated from the 4p DEFAULT_WEIGHTS table
        # in docs/TRAINING_RUN.md, which understates a trained vector badly
        # (quiescent is 0.732 here against that table's 0.272):
        #
        #     arch                book   mirror   x quiescent on the real mix
        #     quiescent:levels=1  0.732   1.498    1.0x  <- what it trained on
        #     plan:width=1        1.395   3.316    2.2x
        #     plan:width=2        2.097   6.504    4.1x  <- chosen
        #     plan:width=4        6.116   9.702    6.7x
        #     plan:width=8        9.069  15.829   10.8x  <- the ship policy
        #
        # "the real mix" is ~3/4 mirror-shaped duels (mirror + the past/hall
        # ladder) and ~1/4 book-shaped, which reproduces the arm's observed
        # 168 s/generation under quiescence.  So width=2 is ~690 s/generation
        # => ~200 generations in the 46h left, against ~1000 at quiescence.
        # width=4 gives ~145 and width=8 ~90, which is too few accepts to
        # climb with; width=1 is cheap but docs/BOT_ARCHITECTURE.md calls it
        # "everything except the multi-action search" -- no beam at all, so it
        # is not the shape we ship.  width=2 is the cheapest configuration
        # that is still a beam search.
        #
        # The residual gap between training at width=2 and shipping at
        # width=8 is exactly what experiments/proxy_check.py measures.
        #
        # --full-check-every 25 --check-games 24 --ablate-every 0: a full
        # check plays EVERY pool opponent and an ablation cycle plays four
        # more duels per weight, and under PlanBot each of those games costs
        # ~10x what it did.  At the old cadence this arm would spend more time
        # checking than training.  Ablation is off rather than merely rarer:
        # single trained weights are not interpretable anyway
        # (docs/HAZARDS.md trap 4) and this arm exists to climb.
        echo "--candidate-bot plan:width=2 --full-check-every 25 --check-games 24 --ablate-every 0"
        ;;
    3|4)
        # WAS `quiescent:levels=1`, on the grounds that it was still
        # productive (3p: 19 accepts in its last 100 generations) and it is
        # ~4x cheaper per game.  SWITCHED TO PlanBot 2026-07-30, and the
        # reason is not cost or strength -- it is that QuiescentBot is not a
        # bot we can ship.
        #
        # `docs/DEEPER_SEARCH.md` 6.2 said so when it was adopted:
        # "conditional GO, not yet", reservation 3 -- resolving a defender's
        # `defense` decision requires the defender's real `hand_military`,
        # which is hidden in the real game.  "For a symmetric self-play
        # trainer this is defensible; for any claim about play against a
        # human it is not."  The gating experiment 6.2 named (a league arm
        # whose challenger AND mirror are both quiescent, scored on opponents
        # held out of both pools) was never run, and the arms were switched
        # anyway.
        #
        # That makes every 3p/4p generation since then a climb against an
        # objective the shipped bot cannot pursue: the weights were being
        # tuned to exploit information the ship policy will not have.  The
        # project's goal is to beat humans and to tell a human what to play,
        # so a bot that must see hidden hands is not a bot -- it is an upper
        # bound.  Gen 1315 (3p) and gen 370 (4p) were discarded for this.
        #
        # PlanBot leaks too and that is being closed separately (the beam
        # draws the true next deck card; docs/AGGRESSION_RATE.md 9).  The
        # difference is that PlanBot's leak is a defect with a fix, and
        # QuiescentBot's is the mechanism it works by.
        #
        # The cost is real and is accepted knowingly: ~4.1x per game (see the
        # 2p table above), so ~690 s/generation against ~168 under
        # quiescence.  Fewer, honest generations beat many dishonest ones.
        # The check cadence is cut for the same reason it is cut at 2p --
        # under PlanBot a full check costs ~10x, and at the old cadence these
        # arms would spend more time checking than training.
        echo "--candidate-bot plan:width=2 --full-check-every 25 --check-games 24 --ablate-every 0"
        ;;
    esac
}

launch() {   # players workers block extra...
    local K=$1 W=$2 B=$3; shift 3
    # THE ZERO-GAME HALT (docs/HAZARDS.md trap 8).  hillclimb_league.py
    # writes this file when a whole generation completed zero games -- i.e. the
    # engine is broken and the arm is burning CPU for nothing.  This watchdog is
    # exactly what would undo a plain crash-halt, ten minutes at a time, so it
    # has to be the thing that respects it.  Deleting the file is the resume:
    # the next 10-minute tick relaunches the arm with nothing else to do.
    local STOP=experiments/logs/stop_league_${K}p.json
    if [ -f "$STOP" ]; then
        echo "$(date '+%F %T') watchdog: NOT launching ${K}p -- $STOP exists (a generation completed ZERO games; the file names the exception, the frame and a reproducing seed).  Fix the engine and delete the file." >> "$LOG"
        return 1
    fi
    # shellcheck disable=SC2046  -- deliberate word splitting; no flag value
    # here contains a space, and bash 3.2 (macOS) has no better option.
    local ALL=("${COMMON[@]}" $(arm_flags "$K") "$@")
    local req n
    for req in $REQUIRED; do
        n=0
        for f in "${ALL[@]}"; do [ "$f" = "$req" ] && n=$((n + 1)); done
        if [ "$n" != 1 ]; then
            echo "$(date '+%F %T') watchdog: REFUSING to launch ${K}p -- $req appears $n times in: ${ALL[*]}" >> "$LOG"
            return 1
        fi
    done
    # nice 19: training is the box's background load, not its foreground job.
    # Nothing else runs here normally, so the arms still get every idle cycle
    # -- but any benchmark, profile or A/B run at the default priority now
    # preempts them instead of timesharing with 8 workers on 6 cores.  Without
    # this, every perf measurement on this machine is taken under ~1.6x
    # oversubscription and cache contention, and the numbers cannot be
    # compared across runs (docs/PYPY.md).  Children inherit it, so this one
    # word covers the whole arm.  Reverting means dropping `nice -n 19` and
    # waiting for the next relaunch; no running process needs touching.
    nohup nice -n 19 experiments/run_league.sh "$K" "$REMAIN" "$W" 2 "$B" 4 1.2816 \
        "${ALL[@]}" >/dev/null 2>&1 &
    echo "$(date '+%F %T') watchdog: relaunched ${K}p (${REMAIN}h left, workers=$W block=$B) ${ALL[*]}" >> "$LOG"
}

# 2p: warm-started from P, the 1-PLY LINEAGE vector
# (experiments/archive_preplan/league_state_1ply_20260726/champion_2p.json,
# gen 355, sha256 55c7a3dea72e..., byte-identical to the pool's
# hall_of_fame/oneply_2p_gen00355.json), NOT from its own quiescent-trained
# champion.  Under `plan:width=8` -- the policy this arm now gates on -- P
# scores 190.6 [185.5, 196.3] own culture in a 2p mirror against the league
# champion's 61.4 [56.6, 65.9] at n=162 per vector, and 213.4 against `book`
# where the league champion's descendant measures 132.8
# (docs/PROXY_GUARDRAIL.md's first reading, and docs/PLAN_WAR_LOOKAHEAD.md 4a).
# Warm-starting a PlanBot arm from the quiescent lineage would have started it
# ~80-130 culture points behind, on a vector whose whole strategy the ship
# policy prices differently (docs/TRANSFER_TEST.md).
#
# `--init` is IGNORED once the state dir holds a champion, so this is
# load-bearing exactly once: the 2p state was moved to
# experiments/archive_2p_quiescent_20260729/ on 2026-07-29 to make it fire.
# The quiescent champion (gen 727, sha256 69e01f781a4a...) is recoverable
# there and remains the best vector under the quiescent proxy.
pgrep -f "run_league.sh 2 " >/dev/null || \
    launch 2 1 12 --init experiments/archive_preplan/league_state_1ply_20260726/champion_2p.json
pgrep -f "run_league.sh 3 " >/dev/null || launch 3 2 12 --init default
# 4p: block 24 because its per-game spread is 2.8x the 2p spread (FOURP_GAP),
# and warm-started from the 2p champion, which measured 57.4% vs the 4p arm's
# 27.6% at 4p at matched generations.  --init is ignored once state exists, so
# this is only load-bearing on a genuinely fresh state dir -- which is why it
# was left alone by the 2026-07-27 objective change even though the vector it
# names is now known to score 64.7 own culture: the 4p state dir has held a
# champion since 07-26, so this flag has been inert for the whole run and
# changing it would have changed nothing.  See docs/LEAGUE_OBJECTIVE.md 6.
pgrep -f "run_league.sh 4 " >/dev/null || \
    launch 4 2 24 --init experiments/hall_of_fame/preinfo_2p_gen00188.json
