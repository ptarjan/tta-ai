# BOT ROSTER — WORKING NOTES

> **This document predates PlanBot and QuiescentBot entirely (flagged
> 2026-07-30, not re-run).** Its `champion` entrant is the 1-ply `WeightedBot`
> evaluator only (confirmed below, "`champion` runs the 1-ply evaluator") —
> there is no `plan:width=2` or `quiescent:levels=1` row anywhere in this
> roster. Do not read this as a current ranking of the bots the league
> actually trains and ships today; it is a snapshot of the 1-ply generation.
> A re-run against the current search-based bots has not been done.

**Not a deliverable.** Raw measurements + honest read, parked here so nothing is
lost. The polished tier-list write-up is deliberately deferred until a bot has
actually finished hill climbing — a ranking of bots is only worth reading once
the bots are good.

> **⚠ EVERY 4p NUMBER IN THIS DOCUMENT IS QUARANTINED (2026-07-30).** The 4p
> vector it was measured against — `analysis/frozen/champion_4p.json`, now
> renamed `analysis/frozen/champion_4p.DEGENERATE.json`, and its twin
> `experiments/frozen/champion_4p_strengthcheck.json` — reproduces **all 62
> informative weights** of `experiments/champion_4p.json` bit-for-bit,
> including `science = −6.08883`. That is the vector `docs/TRAINING_RUN.md`
> says never to warm-start from and that `docs/CULTURE_GAP.md` §8f measured at
> **20.1% against a 25% null** — a bot that loses to random seating.
> `refuse_if_degenerate_champion` was supposed to catch it and did not: it
> tested exact content, and the frozen copy is six generations later and
> differs on two keys (`colonies`, `pacts`). The guard now tests provenance
> over the informative keys and refuses it under any name.
>
> **The 4p rows below are not retracted — they are unreliable and left in
> place so they stay auditable.** They describe a known-degenerate bot. Do not
> quote them as facts about 4p play, and do not quote them as facts about the
> engine. The 2p and 3p numbers in this document are unaffected by *this*
> issue. See `analysis/frozen/README.md`.


Data: `experiments/roster_match.jsonl`. Regenerate tables with
`python3 -m experiments.roster_report --inject docs/BOT_ROSTER.md`.

## What was run

Full round-robin, 12 entrants, at 2p / 3p / 4p. 66 ordered pairings per player
count, 198 total, **n=240 games each — 47,520 games, zero engine errors**.

- Seed-paired: every matchup at a player count uses the same seed set
  (`--seed 2000`), so cells are paired game-for-game on identical deals.
- Seat-rotated: `arena.duel` plays each seed once with the challenger in each
  seat.
- Null = 1/N. At 3p/4p a cell is "A alone against a table of Bs".
- Determinism check: two runner processes accidentally overlapped and
  re-measured 12 pairings. All 12 returned bit-identical win rates.

## Caveats that matter

- **Shutout cells.** `arena.mean_ci` is a normal approximation over per-game
  shares; when a bot wins every game the variance is 0 and the harness prints
  `100.0% ± 0.0%, p=1.0000`. That p is an artefact of a zero standard error, not
  a null result. `roster_report.py` substitutes a Wilson interval in the report
  layer (shared `arena.py` untouched).
- **3p/4p lower triangle is blank on purpose.** "One Culture vs three Infra" and
  "one Infra vs three Culture" are different games, so the reciprocal is not
  `1-x` and is not guessed. The **all games** column is therefore NOT the row
  mean — it is every game the bot played, as lone challenger (share = win rate)
  and as one of `n-1` defenders (share = `(1-w)/(n-1)`).
- **Not run:** the reverse direction at 3p/4p (would fill the lower triangle,
  ~2h), and `experiments/roster_behaviour.py` (wars / aggressions / pact offers
  / colony bids per game on mirror tables — script is written and committed but
  never executed). Both are cheap to add later.

## Headline

`x par` = share ÷ null, so 1.00 is an average bot at any table size and the
column reads across player counts.

<!-- BEGIN summary -->
| bot | 2p share | x par | 3p share | x par | 4p share | x par | Elo 2p | Elo 3p | Elo 4p |
|---|---|---|---|---|---|---|---|---|---|
| **CultureBot** | 79.0% | 1.58 | 54.2% | 1.62 | 41.8% | 1.67 | 1794 | 1745 | 1767 |
| **BookBot v1** | 58.6% | 1.17 | 39.9% | 1.20 | 31.6% | 1.26 | 1623 | 1608 | 1664 |
| **BookBot v2** | 59.5% | 1.19 | 40.9% | 1.23 | 29.8% | 1.19 | 1630 | 1618 | 1616 |
| **BookImprovedBot** | 54.1% | 1.08 | 43.8% | 1.32 | 31.4% | 1.26 | 1589 | 1680 | 1669 |
| **champion** | 50.9% | 1.02 | 39.9% | 1.20 | 30.2% | 1.21 | 1563 | 1623 | 1645 |
| **InfraBot** | 53.5% | 1.07 | 32.3% | 0.97 | 26.8% | 1.07 | 1584 | 1488 | 1494 |
| **WonderBot** | 62.6% | 1.25 | 38.7% | 1.16 | 29.9% | 1.20 | 1654 | 1584 | 1609 |
| **ScienceBot** | 58.6% | 1.17 | 39.4% | 1.18 | 30.4% | 1.21 | 1623 | 1586 | 1602 |
| **TempoBot** | 52.8% | 1.06 | 34.2% | 1.03 | 28.0% | 1.12 | 1578 | 1507 | 1507 |
| **MilitaryBot** | 57.0% | 1.14 | 45.7% | 1.37 | 42.7% | 1.71 | 1610 | 1630 | 1731 |
| **GreedyBot** | 12.2% | 0.24 | 8.2% | 0.25 | 3.9% | 0.16 | 1079 | 1102 | 975 |
| **RandomBot** | 1.2% | 0.02 | 2.6% | 0.08 | 2.0% | 0.08 | 672 | 830 | 722 |
<!-- END summary -->

## Honest read

**Useful as a training gate** (clearly above par, beating them means something):

- **CultureBot** — the only bot that is strong at every table size (1.58 /
  1.62 / 1.67 × par). Top Elo at all three counts. This is the real gate.
- **MilitaryBot** — scales hard with table size: 1.14 × par at 2p but 1.71 at
  4p, where it essentially ties CultureBot (42.7% vs 41.8% share). At 4p it is
  a genuine gate; at 2p it is mid-pack.
- **BookBot v2 / WonderBot / ScienceBot / BookBot v1** — a tight 1.17–1.25 ×
  par band at 2p, ~1.2 × par at 3p/4p. Respectable, but they are all beaten
  soundly by CultureBot. Useful as intermediate checkpoints, not as the bar.

**Sparring partners only** (at or below par — keep for pool diversity, do not
present as a bar to clear):

- **InfraBot** (0.97–1.07 × par) and **TempoBot** (1.03–1.12 × par) — below or
  barely at par at 3p/4p. InfraBot is the clearest case: at both 3p and 4p it
  beats *only* GreedyBot and RandomBot and loses significantly to all 8 other
  entrants. Still worth keeping: they lose in structurally different ways
  (Infra over-builds economy, Tempo over-spends actions early), which is exactly
  the pool diversity that self-play lacked.
- **GreedyBot** (0.16–0.25 × par) and **RandomBot** (0.02–0.08 × par) — the
  floor. Sanity check only. Note GreedyBot is *the baseline the champion was
  originally trained against*, which is the whole reason self-play converged to
  something weak.

**The trained champion is not a gate.** 1.02 × par at 2p (10th of 12), 1.20 at
3p, 1.21 at 4p. It loses to CultureBot 15%/85% at 2p. Consistent with
`docs/STRENGTH_CHECK.md`'s conclusion. **BookImprovedBot** (champion overruled
by the book) is better than the champion at 3p/4p but still below CultureBot.

**Non-transitivity is real** — MilitaryBot holds CultureBot to 52/48 at 2p while
losing to WonderBot (41%), which CultureBot beats 71%. Do not collapse the pool
to a single Elo.

## Cost

Per-bot seconds fitted from the pairing wall clocks by least squares on
`secs(a,b) ≈ c[a] + c[b]·(n-1)` — a fit over existing timings, not a direct
benchmark, so treat as relative. Units: seconds per seat per 240 games.

| bot | 2p | 3p | 4p |
|---|---|---|---|
| champion | 27.7 | 25.3 | 33.2 |
| BookImprovedBot | 11.6 | 19.4 | 49.5 |
| GreedyBot | 5.3 | 9.0 | 17.8 |
| TempoBot | 8.0 | 4.0 | 5.7 |
| InfraBot | 6.3 | 2.9 | 3.5 |
| CultureBot | 5.8 | 2.3 | 1.5 |
| ScienceBot | 5.7 | 2.8 | 1.3 |
| MilitaryBot | 5.5 | 2.1 | ~0 |
| BookBot v1 / v2 | 5.3 | 1.2–1.6 | 1.2 |
| WonderBot | 5.3 | 2.5 | 1.4 |
| RandomBot | 1.5 | 0.6 | 1.4 |

**The one cost fact that matters for the training run:** the two WeightedBot-based
entrants are 10–40× the rule-based ones. `champion` runs the 1-ply evaluator;
`BookImprovedBot` runs *both* the champion evaluator and the book, and at 4p it
is the single most expensive bot in the pool (~50s vs ~1.2s for BookBot). The
six variants and both BookBots are all ~1–6s and effectively free. A pool of
CultureBot + MilitaryBot + the book bots costs almost nothing; adding
BookImprovedBot to a 4p pool roughly dominates the generation time.

## Per-pairing matrices

Row A, column B = A's share when A sits alone at a table of Bs.

<!-- BEGIN matrix -->
### 2 players (null = 50.0%)

| A \ table of B | Culture | Book v1 | Book v2 | BookImproved | champion | Infra | Wonder | Science | Tempo | Military | Greedy | Random | all games |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| **CultureBot** | – | 79% | 73% | 86% | 85% | 80% | 71% | 67% | 78% | 52% | 99% | 100% | **79.0%** |
| **BookBot v1** | 21% | – | 44% | 57% | 64% | 56% | 50% | 49% | 49% | 57% | 98% | 100% | **58.6%** |
| **BookBot v2** | 27% | 56% | – | 59% | 58% | 57% | 50% | 52% | 56% | 48% | 93% | 100% | **59.5%** |
| **BookImprovedBot** | 14% | 43% | 41% | – | 60% | 49% | 35% | 44% | 60% | 54% | 97% | 99% | **54.1%** |
| **champion** | 15% | 36% | 42% | 40% | – | 46% | 36% | 35% | 56% | 54% | 99% | 100% | **50.9%** |
| **InfraBot** | 20% | 44% | 43% | 51% | 54% | – | 39% | 44% | 50% | 52% | 93% | 99% | **53.5%** |
| **WonderBot** | 29% | 50% | 50% | 65% | 64% | 61% | – | 59% | 57% | 59% | 95% | 99% | **62.6%** |
| **ScienceBot** | 33% | 51% | 48% | 56% | 65% | 56% | 41% | – | 55% | 45% | 95% | 100% | **58.6%** |
| **TempoBot** | 22% | 51% | 44% | 40% | 44% | 50% | 43% | 45% | – | 50% | 92% | 99% | **52.8%** |
| **MilitaryBot** | 48% | 43% | 52% | 46% | 46% | 48% | 41% | 55% | 50% | – | 97% | 100% | **57.0%** |
| **GreedyBot** | 1% | 3% | 7% | 3% | 1% | 7% | 5% | 5% | 8% | 3% | – | 92% | **12.2%** |
| **RandomBot** | 0% | 0% | 0% | 1% | 0% | 1% | 1% | 0% | 1% | 0% | 8% | – | **1.2%** |

### 3 players (null = 33.3%)

| A \ table of B | Culture | Book v1 | Book v2 | BookImproved | champion | Infra | Wonder | Science | Tempo | Military | Greedy | Random | all games |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| **CultureBot** | – | 58% | 51% | 41% | 50% |  | 55% | 47% |  |  | 99% | 100% | **54.2%** |
| **BookBot v1** |  | – |  | 28% | 39% |  |  |  |  |  | 96% | 98% | **39.9%** |
| **BookBot v2** |  | 41% | – | 25% | 30% |  |  |  |  |  | 92% | 96% | **40.9%** |
| **BookImprovedBot** |  |  |  | – | 40% |  |  |  |  |  | 89% | 94% | **43.8%** |
| **champion** |  |  |  |  | – |  |  |  |  |  | 89% | 94% | **39.9%** |
| **InfraBot** | 7% | 20% | 17% | 16% | 19% | – | 24% | 19% |  | 25% | 83% | 94% | **32.3%** |
| **WonderBot** |  | 30% | 30% | 23% | 30% |  | – |  |  |  | 89% | 96% | **38.7%** |
| **ScienceBot** |  | 29% | 29% | 22% | 31% |  | 31% | – |  |  | 92% | 96% | **39.4%** |
| **TempoBot** | 9% | 23% | 18% | 14% | 18% | 36% | 20% | 23% | – | 35% | 84% | 96% | **34.2%** |
| **MilitaryBot** | 26% | 41% | 44% | 21% | 28% |  | 48% | 51% |  | – | 99% | 97% | **45.7%** |
| **GreedyBot** |  |  |  |  |  |  |  |  |  |  | – | 83% | **8.2%** |
| **RandomBot** |  |  |  |  |  |  |  |  |  |  |  | – | **2.6%** |

### 4 players (null = 25.0%)

| A \ table of B | Culture | Book v1 | Book v2 | BookImproved | champion | Infra | Wonder | Science | Tempo | Military | Greedy | Random | all games |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| **CultureBot** | – | 44% | 48% | 37% | 39% |  | 42% | 46% |  |  | 98% | 100% | **41.8%** |
| **BookBot v1** |  | – |  | 35% | 36% |  |  |  |  |  | 95% | 97% | **31.6%** |
| **BookBot v2** |  | 22% | – | 22% | 27% |  |  |  |  |  | 94% | 96% | **29.8%** |
| **BookImprovedBot** |  |  |  | – | 28% |  |  |  |  |  | 99% | 97% | **31.4%** |
| **champion** |  |  |  |  | – |  |  |  |  |  | 100% | 98% | **30.2%** |
| **InfraBot** | 4% | 10% | 12% | 10% | 12% | – | 18% | 11% |  | 16% | 88% | 92% | **26.8%** |
| **WonderBot** |  | 22% | 25% | 19% | 21% |  | – |  |  |  | 92% | 92% | **29.9%** |
| **ScienceBot** |  | 24% | 24% | 15% | 19% |  | 21% | – |  |  | 96% | 97% | **30.4%** |
| **TempoBot** | 5% | 11% | 17% | 11% | 12% | 24% | 14% | 16% | – | 16% | 92% | 92% | **28.0%** |
| **MilitaryBot** | 35% | 40% | 50% | 28% | 28% |  | 48% | 46% |  | – | 100% | 98% | **42.7%** |
| **GreedyBot** |  |  |  |  |  |  |  |  |  |  | – | 74% | **3.9%** |
| **RandomBot** |  |  |  |  |  |  |  |  |  |  |  | – | **2.0%** |
<!-- END matrix -->
