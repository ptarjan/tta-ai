#!/bin/bash
# Lane C A/B: board-aware card pricing on vs off, paired on the deal.
#
# Both arms are the SAME weight vector (analysis/frozen/champion_2p.json)
# differing only in the board-pricing credits, so the duel isolates the
# pricing and nothing else.  `experiments.arena.duel` at 2 players plays each
# deal twice with the seats swapped, so the comparison is paired on the deal.
#
# MDE: n = 3200 games gives SE ~= 0.88pp on the win rate, so the minimum
# detectable effect at 80% power and alpha = 0.05 two-sided is ~= 2.5pp.
# Pairing makes the true SE smaller than that, so 2.5pp is conservative.
# NOTE the caveat in docs/CARD_PRICING_LEADERS.md 5.2: `agg.py`'s interval is
# the independent-samples one on a paired design, and is optimistic.
#
# The arm is a WEIGHT configuration now, not `TTA_BOARD_TYPES` -- the type
# knob became `card_board_leader` / `card_board_government` /
# `card_board_action`, offsets on the shared `card_board_credit`, so the
# league can fit what only this script could set before.  A -1.0 offset
# cancels the shared credit for that type exactly, which is what the
# environment variable used to do.
#
#   bash analysis/laneC/run_ab.sh main            # everything on
#   bash analysis/laneC/run_ab.sh government      # governments alone
#   bash analysis/laneC/run_ab.sh leader          # leaders alone
#   bash analysis/laneC/run_ab.sh leader 1.0      # ...with the hand-term
#                                                 #    double-count restored
#
# The second argument is `hand_swap_extra`: 0.0 (the default, and what ships)
# prices a hand of leaders as its best single replacement; 1.0 is exactly the
# summing that preceded that fix, so the defect is reproducible as a control
# arm in the same binary instead of only by checking out an old commit.
#
# 8 blocks of 400 on EVERY arm, on the same eight seeds, which is what makes
# the arms comparable to each other and to analysis/laneC/results.txt block by
# block.  (An earlier version of this script ran 4 blocks for the
# decomposition arms; the numbers in the doc are 8, and 8 is what was run.)
set -u
cd "$(dirname "$0")/../.."
arm="${1:-main}"
extra="${2:-0.0}"
tag="${arm}_x${extra}"
out="/tmp/laneC_ab_${tag}.jsonl"
: > "$out"
seeds="0 200 400 600 800 1000 1200 1600"

# The two vectors for this configuration, written to disk so a finished run
# can always be re-read for what it actually ran.
on="/tmp/laneC_on_${tag}.json"
off="/tmp/laneC_off_${tag}.json"
nice -n 19 python3 analysis/laneC/make_arm.py "$arm" "$extra" "$on" "$off" \
  || exit 1
for s in $seeds; do
  nice -n 19 python3 -m experiments.evaluate \
    --a "$on" --b "$off" \
    --players 2 --games 400 --seed "$s" --workers "${WORKERS:-2}" \
    --out "$out" || exit 1
done
# Both summaries, deliberately.  `agg.py` is the estimator the numbers in
# results.txt were computed with (independent-samples on a paired design), and
# `tools/ab_summary.py` is the corrected one that landed on 2026-07-30 and
# clusters on the deal.  Printing both is what makes a new arm comparable to
# the old table AND honestly stated.
nice -n 19 python3 analysis/laneC/agg.py "$out"
nice -n 19 python3 tools/ab_summary.py "$out"
