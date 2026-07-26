# Bot roster: who is actually strong, and who is a sparring partner

`docs/STRENGTH_CHECK.md` showed the problem: self-play had converged, and the
trained champion only ever had to beat a mirror of itself. `engine/bots/variants/`
was built to fix that — six rule-based opponents derived from the points where
the experts in `docs/EXPERT_STRATEGY.md` genuinely disagree, so that training
has something structurally different to beat.

A pool is only useful if you know what is in it. This document measures every
entrant against every other entrant, at 2p, 3p and 4p, and labels each one
honestly on the axis that matters for training: **is this bot strong, or is it
only useful as pool diversity?** Both are legitimate roles. A bot that loses to
everything is still worth keeping if it loses in an unusual way — but it must
not be presented as an equal.

## Method

- **Runner:** `python3 -m experiments.roster_match --games 240` →
  `experiments/roster_match.jsonl`.
- **Tables:** `python3 -m experiments.roster_report` regenerates everything
  below from that JSONL without replaying a game.
- **Seed-paired.** Every matchup at a given player count uses the same seed set
  (`--seed 2000`), so all cells of a table are paired game-for-game on identical
  deals. Differences between cells are differences between bots, not between
  decks.
- **Seat-rotated.** `arena.duel` plays each seed once with the challenger in
  each seat, so no result is a seating artefact.
- **n = 240 games per ordered pairing**, 66 pairings per player count, 198
  pairings total — 47,520 games.
- **Null is 1/N** (50% / 33.3% / 25%). At 3p and 4p a "duel" is one challenger
  against a table of identical defenders, which is the only sense in which a
  1-vs-1 comparison is defined at those counts. A cell therefore reads *"A alone
  against a table of Bs"*.
- **Zero engine errors** across all 47,520 games.

### Two things this table does not do

**It does not fill in the reciprocal cell at 3p/4p.** At 2p, "A beats B 79%"
makes "B beats A 21%" true by definition and the matrix is filled in. At 3p and
4p it does not: *one CultureBot against three InfraBots* and *one InfraBot
against three CultureBots* are different games with different dynamics. Each
ordered pair is played on its own, and the lower triangle is left blank rather
than guessed. The **all games** column is therefore *not* the mean of the
printed row — it is every game the bot played, counting it as the lone
challenger (share = win rate) and as one of the `n-1` identical defenders
(share = `(1 - win rate) / (n - 1)`).

**It does not lead with Elo.** A single rating assumes transitivity, and a
diverse pool is supposed to violate it — see the CultureBot/MilitaryBot cycle
below, which any single number would erase. Elo is reported as a summary; the
per-pairing matrix is the evidence.

### Statistical note: shutout cells

`arena.mean_ci` is a normal approximation over per-game win shares. When a bot
wins *every* game the sample variance is exactly zero, so the harness prints
`100.0% ± 0.0%, p=1.0000`. That p-value is an artefact of dividing by a zero
standard error — a 240–0 sweep is the *most* significant cell in the table, not
the least. `roster_report.py` substitutes a Wilson score interval for those
cells, which is well behaved at the boundary. The raw JSONL keeps the harness's
own numbers; the correction lives in the report layer so shared `arena.py` is
untouched.

### Reproducibility

Two runner processes accidentally overlapped during a restart and re-measured
12 pairings. All 12 returned **bit-identical** win rates. The harness is
deterministic given a seed, so every number here is reproducible exactly.

## Headline table

Share is the fraction of the win this bot took, averaged over every game it
played in either role. Shares are not comparable across player counts (par is
50% / 33.3% / 25%), so **× par** divides by the null: 1.00 is an average bot at
any table size, and that column can be read across.

<!-- BEGIN summary -->
| bot | 2p share | x par | 3p share | x par | 4p share | x par | Elo 2p | Elo 3p | Elo 4p |
|---|---|---|---|---|---|---|---|---|---|
| **CultureBot** | 79.0% | 1.58 | 54.2% | 1.62 | 41.8% | 1.67 | 1794 | 1745 | 1757 |
| **BookBot v1** | 58.6% | 1.17 | 39.9% | 1.20 | 31.6% | 1.26 | 1623 | 1608 | 1655 |
| **BookBot v2** | 59.5% | 1.19 | 40.9% | 1.23 | 29.8% | 1.19 | 1630 | 1618 | 1606 |
| **BookImprovedBot** | 54.1% | 1.08 | 43.8% | 1.32 | 26.0% | 1.04 | 1589 | 1680 | 1659 |
| **champion** | 50.9% | 1.02 | 39.9% | 1.20 | 25.3% | 1.01 | 1563 | 1623 | 1635 |
| **InfraBot** | 53.5% | 1.07 | 32.3% | 0.97 | 26.8% | 1.07 | 1584 | 1488 | 1485 |
| **WonderBot** | 62.6% | 1.25 | 38.7% | 1.16 | 29.9% | 1.20 | 1654 | 1584 | 1600 |
| **ScienceBot** | 58.6% | 1.17 | 39.4% | 1.18 | 30.4% | 1.21 | 1623 | 1586 | 1593 |
| **TempoBot** | 52.8% | 1.06 | 34.2% | 1.03 | 28.0% | 1.12 | 1578 | 1507 | 1498 |
| **MilitaryBot** | 57.0% | 1.14 | 45.7% | 1.37 | 42.7% | 1.71 | 1610 | 1630 | 1722 |
| **GreedyBot** | 12.2% | 0.24 | 8.2% | 0.25 | 1.9% | 0.08 | 1079 | 1102 | 916 |
| **RandomBot** | 1.2% | 0.02 | 2.6% | 0.08 | 1.5% | 0.06 | 672 | 830 | 876 |
<!-- END summary -->

## The tier list

<!-- BEGIN tiers -->
<!-- END tiers -->

## Per-pairing results

Row A, column B = A's share of the win when A sits alone at a table of Bs.
Blank = that ordered pair has not been played (3p/4p only; see Method).

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
| **BookImprovedBot** |  |  |  | – |  |  |  |  |  |  |  |  | **26.0%** |
| **champion** |  |  |  |  | – |  |  |  |  |  |  |  | **25.3%** |
| **InfraBot** | 4% | 10% | 12% | 10% | 12% | – | 18% | 11% |  | 16% | 88% | 92% | **26.8%** |
| **WonderBot** |  | 22% | 25% | 19% | 21% |  | – |  |  |  | 92% | 92% | **29.9%** |
| **ScienceBot** |  | 24% | 24% | 15% | 19% |  | 21% | – |  |  | 96% | 97% | **30.4%** |
| **TempoBot** | 5% | 11% | 17% | 11% | 12% | 24% | 14% | 16% | – | 16% | 92% | 92% | **28.0%** |
| **MilitaryBot** | 35% | 40% | 50% | 28% | 28% |  | 48% | 46% |  | – | 100% | 98% | **42.7%** |
| **GreedyBot** |  |  |  |  |  |  |  |  |  |  | – |  | **1.9%** |
| **RandomBot** |  |  |  |  |  |  |  |  |  |  |  | – | **1.5%** |
<!-- END matrix -->

## Behaviour: what each bot actually does

Win rate alone does not tell you what a bot is worth as a training partner. Two
bots with the same win rate are different opponents if one declares wars and the
other never fights — a pool that never attacks cannot teach a learner to defend.

Counts are per game on **mirror tables** (every seat runs the same bot), summed
over all seats, so they read as "what this bot does when everyone plays it"
rather than "what it does against some particular field". Measured by
`python3 -m experiments.roster_behaviour`.

<!-- BEGIN behaviour -->
_(not measured)_
<!-- END behaviour -->

## Cost

<!-- BEGIN cost -->
_(not measured)_
<!-- END cost -->
