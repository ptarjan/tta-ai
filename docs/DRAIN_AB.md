# The drain A/B: why `QUIET_PENDING` was flipped to `True`

`engine/bots/pending.py` holds one judgement call, `QUIET_PENDING`: when a
pending decision is **mine**, should the bot drain the pending stack before
pricing its candidates?  Its own `_beam` already does (`apply -> _quiesce ->
score`) at every node it searches.  Until 2026-07-30 the real decision did
not.  This file records the measurement that accompanied the flip.

**The flip does not rest on this measurement.**  It is a consistency fix: the
bot was pricing its own live position by a different rule than it prices the
identical position inside its own search, and `weighted.features` reads
nothing from `pend["atk"]`/`pend["dfn"]`, so an undrained position cannot
express whether a defence succeeds at all.  That is wrong independently of
what it scores.  The numbers below are corroboration, and the 4p numbers are
notably weaker than the 3p ones -- recorded here honestly rather than pooled
into a single flattering figure.

## 1. Design

Run on the desktop compute node (`micro@100.68.145.15`, working dir
`/c/Users/micro/tta-defence`), against frozen league references so the
opponent cannot drift under the comparison:

    python -m experiments.evaluate \
      --a plan:$W,width=2,qp=1 --b plan:$W,width=2,qp=0 \
      --players $P --games 200 --seed $s --workers 5 --out $OUT

with `W = analysis/frozen/champion_3p_gen1255_99key.json` at 3p and
`analysis/frozen/champion_4p_gen350_99key.json` at 4p.  200 games per block,
disjoint seeds, no block discarded.  The reported statistic is A's own-win
share (ties split), against a null of 1/3 at 3p and 1/4 at 4p.

Two arms were run:

* **pure `qp`** -- `qp=1` vs `qp=0`, driver `ab_qp.sh`, output `ab_qp_{3,4}p.jsonl`.
* **`qp`+`qd`** -- `qp=1,qd=1` vs plain default, drivers `ab_clean.sh` /
  `ab_rest.sh`, output `abc_qp_{3,4}p.jsonl`.  The second flag determinizes
  the pending root; see section 3.

FILE HAZARD, recorded because it will otherwise mislead: `ab_more.sh`
(launched 12:52 on 2026-07-30) appends **pure-qp** blocks to the `abc_*`
files, which until then held only the `qp`+`qd` arm.  The boundary is by line
number: `abc_qp_3p.jsonl` lines 1-2 and `abc_qp_4p.jsonl` line 1 are the
`qp`+`qd` arm; everything after is pure `qp`.

## 2. Result, block by block

Every block favours the drain.  Nothing here is a selected subset.

| arm | players | seed-block | own-win share | null | culture margin |
|---|---|---|---|---|---|
| qp     | 3 | 1 | 0.5325 | 0.3333 | +26.01 |
| qp     | 3 | 2 | 0.5250 | 0.3333 | +27.96 |
| qp     | 3 | 3 | 0.5075 | 0.3333 | +25.68 |
| qp+qd  | 3 | 1 | 0.5325 | 0.3333 | +26.01 |
| qp+qd  | 3 | 2 | 0.5975 | 0.3333 | +40.20 |
| qp     | 4 | 1 | 0.3000 | 0.2500 | +15.23 |
| qp+qd  | 4 | 1 | 0.3950 | 0.2500 | +20.33 |

The `qp+qd` 3p block 1 is byte-identical to the pure-`qp` 3p block 1 -- same
share, same margin to every printed digit.  That is not a transcription
error; it is the point of section 3.

Pooled over per-game shares, distinct blocks only (the duplicate counted
once), z against the null using the observed per-game variance:

| pool | blocks | games | mean share | z (observed var) | z (null var) |
|---|---|---|---|---|---|
| 3p, pure qp only  | 3 | 600 | 0.5217 | 9.26 | 9.79 |
| 3p, all arms      | 4 | 800 | 0.5406 | 11.81 | 12.44 |
| 4p, pure qp only  | 1 | 200 | 0.3000 | **1.54** | 1.63 |
| 4p, all arms      | 2 | 400 | 0.3475 | 4.10 | 4.50 |

**The 4p case is not independently established.**  On the pure-`qp` arm alone
4p is a single block at z = 1.54, which is nothing (p ~ 0.12).  The 4p pooled
figure of 4.10 draws half its weight from the `qp`+`qd` arm.  3p is decisive
on either accounting; 4p is suggestive and no more.  Five further 4p blocks
(seeds 3000/3200/3400/3600/3800) and three further 3p blocks were queued on
2026-07-30 to settle it.

Do not quote a single pooled z for "the drain A/B" -- there are two pools and
they say different things.  An earlier draft of the comment in
`engine/bots/pending.py` paired the 800-game mean (0.5406) with the 600-game
z (9.35, computed on the pure arm only); that pairing was wrong and is
corrected above.

## 3. The leak confound, answered rather than argued away

The obvious objection: `qp=1` adds `apply` calls, `fastcopy.copy_state`
copies the draw decks verbatim, so a trial `apply` that draws draws the REAL
next card.  `tools/pending_leak.py` measures the drain consuming real deck
cards in 34.7% of candidate evaluations at 3p (master's own apply: 24.0%).
So a win measured this way might be the bot peeking, not the bot playing.

`qd=1` determinizes the pending root, which removes the peek.  If the win
were the peek, turning `qd` on would shrink it.

Run at the same seed, `qp=1` vs `qp=0` and `qp=1,qd=1` vs plain return the
same numbers to every printed digit: 0.5325 own-win share, +26.01 culture
margin.  Removing the peek changes nothing, so the win is not the peek.

This is a second, independent instrument agreeing with the first: the
1,346-pick census in `tools/pending_leak.py` had already found `qd` inert.
Two different measurements, same verdict.

## 4. What was deliberately NOT changed

`DETERMINIZE` stays `False`.  It is the other half of the same
inconsistency -- `NeuralPlanBot` determinizes the pending root and `PlanBot`
does not -- and it is a real correctness item.  It is held back so that the
digest movement in `tools/gate.sh` is attributable to exactly one constant.
Two constants moving at once would make it impossible to say which moved
which arm.  It gets its own commit.
