# War-rate decision census: (A) war overpriced, or (B) everything else underpriced? (2026-07-31)

Diagnosis only, per the brief — **no pricing weight is touched in this
document or its instrument.**

## 0. The question

Against the 1,011-journal human corpus (`docs/HUMAN_BASELINE.md`), the bot
declares war at 2.9x the human rate and it is worse at 3p/4p than 2p
(`docs/SYSTEM_COVERAGE.md` §3: 2.2x at 2p, 6.6x at 3p, 7.9x at 4p). A related
pathology: `copy_tactic` is played at a 27.3:1 ratio versus `play_tactic`
(`docs/CARD_BLINDNESS_MILITARY.md` §5.4). Nobody had established which of two
causes is responsible, and they have opposite fixes:

* **(A)** the bot over-values war — the war-side features price too high; or
* **(B)** everything else prices too low — war wins by default because the
  alternatives are underpriced or invisible.

`row_pressure` (`engine/bots/weighted.py`) contains `if val <= 0.0: continue`
— any row-card option whose `card_potential` comes out <= 0 contributes
**nothing** to the "value left behind" terms (`row_urgency`,
`row_bargain_forgone`) that would otherwise count against declining to take
it. That is a concrete, named mechanism for (B): it has silently hidden
units, yellow/technology cards, governments and Knights at various times
(`docs/OPEN_ITEMS.md` items 1, 3, 19; `docs/SYSTEM_COVERAGE.md` §5).

## 1. Instrument

`tools/war_census.py` — see its module docstring for the full fidelity
argument; summary here.

* Instruments the two searches the league actually trains: `PlanBot`
  (2p, `plan:width=2`) and `QuiescentBot` (3p, `quiesce:levels=1`).
* Each bot's `pick()` is replaced with a byte-for-byte copy of the original
  plus one additive recording call after the real decision is made, so
  control flow, RNG draw order and the returned move are unchanged.
  `tools/gate.sh`'s eight fingerprints (below) are the proof this holds.
* Records fire at **every real decision** (never inside the bot's own
  search — the census patches the outer `pick()`, and `_beam` /
  `_resolve` remain the untouched originals) where a `war`/`aggression`
  move is legal, and separately wherever `copy_tactic`/`play_tactic` is
  legal.
* Recorded per decision: every candidate's (kind, identity, score); the
  chosen move; the runner-up and the margin between them; round and age
  (for the horizon-blindness check below); and, for `("take", idx)`
  candidates specifically, the row card's `card_potential`, whether it is
  <= 0 (**suppressed** — this is `row_pressure`'s own skip condition, not
  a re-derivation of it) and whether its type is a per-turn-rate production
  building (farm/mine/lab/temple/library/arena/theater) vs everything else.
* **Bounds, stated up front.** Only the non-journalled search path is
  instrumented (`TTA_JOURNAL` unset, the league's own default). "Suppressed"
  is computed only for row-card `take` candidates — that is exactly and only
  what `row_pressure` gates; a negative-priced HAND card (`develop`/`build`/
  `play_action`) is merely discouraged (`hand_potential` sums it with no
  skip), not invisible, and this document makes no claim about that path.
  PlanBot's beam is multi-ply/multi-sample, so no feature-level attribution
  is attempted for its decisions; the `copy_tactic` feature-diff table is
  QuiescentBot (3p) only, where the loop is a plain one-ply `evaluate` per
  candidate. Sample sizes are stated at the top of §2 and are what they are
  — not a claim of full coverage.
* Champion weights: snapshotted 2026-07-31 from the LIVE, currently-training
  `experiments/league_state/champion_{2,3}p.json` (2p gen 84, 3p gen 14) into
  this lane's own clone before the run, so the actively-retraining league
  arms could not move the target mid-run.

## 2. Run — PARTIAL, one arm only, and it is important to say why

**This run was cut short mid-task and the 3p arm was never started.** A
process-change message arrived while the 2p batch was in flight, telling this
lane to stop running its own game batches and land the instrumentation
instead. That instruction is followed for the batches (stopped, no 3p run).
It is **not** followed for its other half — landing the logging inside
`engine/bots/plan.py`/`quiescent.py` so the live league arms emit it, with
`tools/gate.sh` skipped — because that modifies the hot path of the ten
running league arms with the one verification step that would catch a
regression there turned off, which is the exact risk this lane's own brief
was most emphatic about never taking. See `docs/OPEN_ITEMS.md`'s new entry
for the full reasoning and what a safe version of that step would need.

So: **one arm (2p, `plan:width=2`), ~12-13 games (the run was time-boxed and
stopped mid-batch — game count is read off round-number resets in the
decision stream, not a clean counter, so treat it as approximate), 1,308
decision records, 129 of them war/aggression-eligible.** This is roughly 20x
smaller than the "a few hundred games at 2p and 3p" originally scoped, no 3p
data exists at all, and every number below is a small-n, single-arm point
estimate. It is reported because a labeled partial number is worth more than
none — not as a settled answer.

Champion: `experiments/league_state/champion_2p.json` snapshotted 2026-07-31,
gen 84, from the live (still-training) arm, `plan:width=2,det=1` mirror,
self-play. Command:

    python3.13 -m tools.war_census \
        --spec 'plan:experiments/league_state/champion_2p_snapshot.json,width=2' \
        --players 2 --games 200 --seed 20000 --out war_2p.jsonl   # stopped early
    python3.13 -m tools.war_report war_2p.jsonl

## 3. (A) vs (B) — a real signal, textured rather than binary, n this small

**When war/aggression is offered, the bot mostly does NOT take it.** Of 129
politics decisions where `war`/`aggression` was a legal move, `pol_pass`
(39.5%) and `prepare_event` (33.3%) together outdrew `aggression` (18.6%) and
`war` (8.5%) — combined 27.1% chosen when offered, declined 72.9% of the
time. That alone is evidence against a pure, unconditional "war always wins"
story.

**But when it wins, it wins by a wide, and mostly non-suppressed, margin.**
35 decisions where war/aggression WAS chosen: mean margin over the runner-up
24.41 evaluation points (scores at this stage of the game run ~50-250, so
this is not a photo finish). At those SAME 129 decisions, the row itself
(everything `row_pressure` would price, gated the same way it gates it,
n=1,081 row-card instances) is suppressed (`card_potential <= 0`) **40.8% of
the time** — a large share of the row genuinely contributes nothing to the
opportunity-cost the bot weighs against fighting. But **only 8 of 129
decisions (6.2%) had EVERY gated row card suppressed** — most of the time
(93.8%) there was at least one visibly-priced alternative sitting in the row
when war was declared or available. So this is not "war wins by walkover
against an invisible field" in the strong, all-or-nothing sense; it is "war
wins by a wide margin against a field roughly 40% of which the bot cannot see
at all, and the rest of which it can." Read as **both (A) and (B) contribute,
neither cleanly dominant at this sample size**: the margin (24 points) is
large enough that removing 40% suppression might not flip most decisions
(leaning A — war really is priced as very strong), but a 40.8% suppression
rate is far too large to wave off as noise (leaning B — a meaningful chunk of
the field genuinely never entered scoring). **This is the single most
important thing a larger run needs to settle**: does closing the suppression
gap change the CHOSEN move (the number that matters), or only shrink a
margin that was already decisive? This run cannot answer that; it can only
show the suppression rate is real and large.

**The suppressed cards skew AWAY from rate-building, not toward it** — the
opposite of the a-priori guess. Rate-building production cards
(farm/mine/lab/temple/library/arena/theater) in the row were suppressed
27.4% of the time (69/252); everything else (action cards, special techs,
wonders, urban buildings not in the rate list, territories) was suppressed
44.9% of the time (372/829). So at this sample size, **`row_pressure`'s
blindness is not concentrated on the production techs** the yellow-tech
pricing fix already addressed (`docs/YELLOW_TECH_PRICING.md`) — it is
broader and, if anything, worse on the *other* card types. That is worth
someone's attention independent of this document's main question.

## 4. Horizon-blindness check (coordinator's addition) — null at this n

Hypothesis: a per-turn rate is priced at face value without a
turns-remaining multiplier, so war's margin over its runner-up should GROW
with age/round if the alternatives are horizon-blind, flat if not.

| | age I (n=7) | age II (n=13) | age III (n=15) |
|---|---|---|---|
| mean margin | 23.77 | 28.33 | 21.32 |

| | early 1-6 (n=6) | mid 7-13 (n=15) | late 14+ (n=14) |
|---|---|---|---|
| mean margin | 20.24 | 28.16 | 22.18 |

**No monotonic trend either way** — mid-game is highest, not late-game, and
age III is the *lowest* of the three ages, the opposite of what a growing
horizon-blindness would predict. At n=35 total (7-15 per bucket) this is not
strong evidence of anything; it reads as a genuine null at this sample size,
not as a refutation. Needs the full run to say more.

## 5. copy_tactic: what does the bot think it is buying?

**Two different "27.3:1"-shaped claims exist and this run only speaks to
one of them.** Reading `docs/CARD_BLINDNESS_MILITARY.md` §5.4 directly: the
27.3:1 (60.4:1 including `ma_left`) figure there is a ratio of **feature
weighted-influence** — bookkeeping terms (`hand_military`, `hand_mil_value`,
`tactic_level`) versus `strength`, measured by `tools/tactic_plumbing.py`'s
methodology — not a literal copy-count vs play-count ratio. The separate
"~10.5 copies/game" behavioural number in `docs/OPEN_ITEMS.md` item 9 is the
play-frequency claim, and its search/bot is not stated in either document.

**This run's own play-frequency number, under `plan:width=2` (what 2p
actually trains), is the opposite direction**: of 1,179 decisions where
`copy_tactic`/`play_tactic` was legal, `copy_tactic` was chosen 8 times and
`play_tactic` 23 times — **0.3 : 1**, i.e. `play_tactic` chosen ~3x more
often than `copy_tactic`, at n=31 total choices. Feature-diff attribution
(only implemented for the QuiescentBot one-ply path, per the tool's scope
note) collected **zero** usable rows because zero `copy_tactic` decisions
came from the QuiescentBot path in this 2p-only, PlanBot-only partial run.

**What this does and does not show.** It does not contradict the documented
pathology — different metric, and very possibly a different search (the
27.3:1 weight-ratio and the ~10.5/game figure may both come from a one-ply
style evaluation, where PlanBot's multi-ply beam sees several turns further
and its `_quiesce`/`_score` may correct the bookkeeping-avoidance effect that
dominates a single `evaluate()` call). It DOES suggest, at n=31, that the
copy_tactic play-frequency pathology may be **search-depth-dependent**
rather than a pure pricing defect independent of search — which is directly
testable (run this same instrument's QuiescentBot path, i.e. the 3p arm, or
a 2p `quiesce:` spec) and was the very next step this lane was stopped
before taking. **Flagged as the sharpest concrete follow-up in this
document**, not answered by it.

## 6. What this does not measure

* **No 3p data at all.** The pathology is documented as worse at 3p/4p; this
  run cannot speak to player-count scaling in any way.
* **n is small everywhere** — 129 war-eligible decisions, 35 chosen-war
  decisions, 31 tactic choices, all from ~12-13 games of one arm. Every
  percentage above should be read as "what this small sample showed", not as
  a settled rate. Compare to this project's own precedent: `SYSTEM_COVERAGE.md`
  used n=24-40 games/cell, `AGGRESSION_RATE.md` used n=300 — this run is
  below even the lower end of that range.
* **Only the non-journalled search path** (`TTA_JOURNAL` unset, the league's
  own default) is instrumented; the journalled twin is not.
* **Suppression is measured over the ROW, not over a `take` candidate**,
  because a war/aggression decision and a `take` decision are never the same
  node (§1 of `tools/war_census.py`'s docstring) — this is the right
  operationalization of what `row_pressure` gates, but it means "suppressed"
  here is never literally "the move that would have beaten war was invisible"
  — it is "a fraction of the row's opportunity-cost signal was zero at the
  moment war was being weighed."
* **No attempt to attribute PlanBot's multi-ply margin to a feature.** The
  beam searches whole-turn sequences several plies deep; a terminal score
  cannot be cleanly split by feature the way a one-ply `evaluate()` can. The
  `copy_tactic` feature-diff table (§5) is QuiescentBot-only for this reason,
  and this run collected no QuiescentBot data.
* **Nothing here is a strength (win-rate) measurement.** Every number is
  behavioural/positional, same convention as `docs/SYSTEM_COVERAGE.md`.
