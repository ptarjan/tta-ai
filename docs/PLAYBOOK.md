# Playbook — how to actually play Through the Ages (2015 base game)

**Read this one at the table. `HEURISTICS.md` is the long reference; it is
2443 lines and much of it describes bot weight vectors that no longer
exist.**

Everything in this file comes from **1,011 human games** on
boardgaming-online (base game only, expansion cards verified absent),
split by player count, every number carrying its sample size. Source:
`rust/src/bin/humanwinners.rs`, full tables in the run output.

Two things to hold onto before the details:

- These are **correlations between what winners did and what losers did**,
  not proofs of cause. Where a finding is plausibly a symptom of already
  being ahead rather than a lever you can pull, it says so.
- The baselines differ by player count. At 2p a random seat wins 50% of
  the time, at 3p 33.3%, at 4p 25%. Every win rate below is only
  interesting relative to its own baseline.

---

## The one thing to remember

**Track your culture position relative to the table, not your culture
rate.**

This is the cleanest result in the entire corpus, and it replicates in all
three player counts. Cumulative culture *rank* at each age boundary,
winners vs losers:

| age end | 2p (1..2) | 3p (1..3) | 4p (1..4) |
|---|---|---|---|
| I | 1.479 vs 1.521 | 1.955 vs 2.023 | 2.349 vs 2.550 |
| II | 1.443 vs 1.557 | 1.992 vs 2.004 | 2.151 vs 2.616 |
| III | 1.289 vs 1.711 | 1.602 vs 2.199 | 1.667 vs 2.778 |
| IV | **1.137 vs 1.862** | **1.248 vs 2.374** | **1.253 vs 2.916** |

By the end of the game the winner is table-first on cumulative culture in
almost every game. The *absolute* rate gap is real but far less decisive
(4p Age IV: 9.16 vs 6.72 culture/round) — the rank gap is nearly fully
sorted.

Two practical consequences:

1. **At Age I, being behind means nothing.** The rank gap at Age I end is
   inside noise in every bucket. Do not panic in the first age, and do not
   let anyone at the table tell you an early culture lead is decisive.
2. **The gap opens in Age III.** That is where winners and losers actually
   separate. Age III is the age to spend on, not to bank through.

Science shows the same *direction* but a small, flat gap at every age in
every bucket (2p Age IV: 0.888 vs 0.800/round). Science is how you get the
culture; it is not the thing being measured.

---

## Military: the rule is "don't be last", not "be first"

Military's payoff depends sharply on player count. From the
Impact-of-Strength event (a public end-game scoring line, present in 321
of 1,011 games):

| | win rate as strength LEADER | win rate as strength LAST | baseline |
|---|---|---|---|
| 2p | **68.8%** (n=215) | 31.2% | 50.0% |
| 3p | 52.2% (n=46) | **13.0%** | 33.3% |
| 4p | 33.3% (n=60) | **6.7%** | 25.0% |

Read the two columns separately, because they say different things:

- **At 2 players, leading on military is worth a lot** — 68.8% against a
  50% baseline. (Caveat: in a 2p game leader and last are the same game's
  two seats, so this is one split, not two independent samples.)
- **At 3 and 4 players, leading barely beats baseline — but being last is
  a catastrophe.** 6.7% at 4p against a 25% baseline. You do not need the
  biggest army at a big table. You need to not be the obvious victim.

So: at 2p, contest military. At 3p/4p, buy just enough military that
someone else is the softest target, and spend the rest elsewhere.

*(Engine-replay military ranks at the Age II and Age III boundaries show
the same direction but much smaller gaps — e.g. 4p Age III winners 2.455
vs losers 2.515. The end-game signal above is the stronger one, plausibly
because it is measured later and captures the scoring bonus directly
rather than as a proxy.)*

---

## Aggression: winners start fights, roughly twice as often

| | ≥1 war declared (W vs L) | ≥1 aggression played (W vs L) | aggressor win rate | target win rate | baseline |
|---|---|---|---|---|---|
| 2p | 24.9% vs 9.4% | 44.7% vs 31.4% | 59.1% (n=597) | 40.9% | 50.0% |
| 3p | 18.0% vs 7.9% | 39.8% vs 27.1% | 41.7% (n=132) | 26.4% | 33.3% |
| 4p | — | — | above base | 19.8% (n=334) | 25.0% |

Three for three: **aggressors beat their baseline, targets fall below it.**

The honest caveat is large here. Aggression is a tool a player who is
already strong can safely reach for, so some of the aggressor's edge is a
symptom of already winning. Symmetrically, stronger players choose weaker
targets, so the target's deficit partly reflects who gets picked on rather
than what being attacked does to you.

What survives the caveat is the **defensive** half, and it agrees with the
military table: being the table's designated victim is measurably
expensive at every player count, and worst at 4p.

---

## Government: a real null. Staying in Despotism is not a losing tell

This one contradicts a lot of received advice, so it is worth stating
flatly.

| | win rate, never changed government | win rate, did change | baseline |
|---|---|---|---|
| 2p | 50.1% (533 W / 531 L) | 49.7% (158 W / 160 L) | 50.0% |
| 3p | **35.0%** (103 W / 191 L) | 28.6% (30 W / 75 L) | 33.3% |

At 2p it is a dead-flat null. At 3p, never changing correlates with a
*higher* win rate than changing.

**Do not over-read this.** It is very plausibly confounded by game pace: a
player who is already snowballing may simply never need the swap, and
games that end quickly never reach the point where the revolution pays.
It is not evidence that government is a trap.

What it does support is: **don't revolt out of obligation.** And if you
are going to change, change early — among players who did change, winners
made the switch about one round sooner than losers (2p round 10.81 vs
11.65; 3p 10.87 vs 11.69).

Note also the comeback data below: players who came back from behind
changed government *less* often after Age II, not more, in every bucket.

---

## Wonders: a modest edge, and some specific traps

Mean wonders completed, winners vs losers:

| | winners | losers |
|---|---|---|
| 2p | 2.434 (n=691) | 2.333 (n=691) |
| 3p | 2.519 (n=133) | 2.083 (n=266) |
| 4p | 2.559 (n=186) | 2.158 (n=558) |

Real but modest, and partly a symptom — a strong economy finishes wonders,
so this is not purely a lever. The more useful signal is *which* wonder.

**Consistently below baseline in all three player counts — treat as
traps:**

- **Hanging Gardens** — 41.0% / 27.5% / 18.5% against 50% / 33.3% / 25%.
  Built often, correlates with losing every time. The most reliable
  negative in the corpus.
- **Transcontinental Railroad** (2p 40.7%, 3p 29.3%) and **Colossus**
  (2p 40.8%, 3p 30.8%).

**Consistently above baseline:**

- **Hollywood** (2p 70.5%, n=61; 3p 50.0%, n=46)
- **First Space Flight** (2p 59.8%, n=184)
- **St. Peter's Basilica** (2p 56.9%, n=399; 3p 47.4%)
- **Internet** (2p 55.1%; 3p 48.3%)
- **Library of Alexandria** (2p 53.7%, n=406)

**Player-count-dependent — don't collapse to one number:** Ocean Liners is
above baseline at 2p (59.4%) and well below at 4p.

---

## Leaders

**Hammurabi is the only leader above its baseline in all three player
counts**: 55.2% (n=449) / 46.1% (n=102) / 39.4% (n=137) against
50% / 33.3% / 25%. It is the single most consistent leader signal in the
corpus.

Also above baseline with usable sample sizes: Joan of Arc (2p 60.7%,
n=239), Albert Einstein (2p 56.1%), Isaac Newton (3p 44.3%, 4p 33.1%),
Leonardo da Vinci (3p 42.9%).

Below baseline: **Aristotle** — and this one reverses with player count.
Roughly neutral at 2p (51.7%), clearly bad at 3p (25.8%) and 4p (15.4%).
Also weak: Julius Caesar (2p 40.8%), Michelangelo (2p 42.0%, 3p 24.7% —
though 4p 33.3%, above base), Mahatma Gandhi (2p 39.7%, n=58).

Leader choice is a skill- and context-dependent decision, not a random
draw, so these are correlations over the pool of players who chose them —
not "always take Hammurabi".

---

## Two things people optimize that don't matter

**Build-to-take ratio.** Winners 0.511 vs losers 0.523 at 2p; 0.613 vs
0.616 at 4p. A flat null three times out of three. Do not spend thought on
"conversion efficiency". Winners take slightly *more* cards in raw volume
(2p 38.2 vs 36.4/game) — that is the only difference, and it is small.

**Take-backs.** PutBack actions per game are essentially identical between
winners and losers in every bucket.

---

## Coming back from behind

Win rate for a player not in first on cumulative culture at the end of
Age II:

| | win rate | baseline |
|---|---|---|
| 2p | 44.3% (n=691) | 50.0% |
| 3p | 32.0% | 33.3% |
| 4p | **20.1%** (n=558) | 25.0% |

At 2p and 3p being behind at Age II costs you almost nothing — consistent
with the top finding that the gap only opens in Age III. At 4p it is a
real cost: more rivals to climb past.

Among players who *were* behind, what separated the ones who came back:

| | culture-rate acceleration in Age III | wars declared in Age III |
|---|---|---|
| 2p | 9.76 vs 6.48 | 0.408 vs 0.106 (**~4x**) |
| 4p | 11.73 vs 7.01 | 0.286 vs 0.085 (**~3.4x**) |

And what did **not** separate them: wonders completed in Age III (2p 0.297
vs 0.353 — slightly *fewer*), and changing government after Age II (2p
9.2% vs 11.7%; 4p 9.8% vs 16.6% — comeback players changed *less*).

The comeback recipe in the data is: **accelerate culture in Age III and
start a fight.** Not: build a wonder, not: revolt. The same causation
caveat applies to the war number — declaring war may be something that
becomes *available* once you have partly caught up.

---

## At the table: the short version

1. Age I: don't panic about culture. The gap that decides the game opens
   in Age III.
2. Know your culture *rank*, not your culture rate. Being second at a
   4-player table in Age IV is losing.
3. At 2p, contest military. At 3p/4p, just don't be the weakest — last
   place on military wins 6.7% of 4p games.
4. Don't revolt out of obligation. If you're going to, do it early.
5. Don't build Hanging Gardens.
6. Behind at Age II? Push culture rate in Age III and pick a fight. Don't
   reach for a wonder or a revolution.
7. Stop optimizing your build-to-take ratio. It has never mattered.

---

## What this document does not cover

- **Anything the bot knows that humans don't.** This file is human-corpus
  evidence only. The bot's own play is measured separately and is not yet
  trustworthy enough to convert into advice — see the census note in
  `HEURISTICS.md`.
- **Card-by-card pricing and take priority.** `HEURISTICS.md` has priority
  lists; treat its `[rules]` and `[confirmed]` material as durable and its
  self-play-derived material as a snapshot of champions that no longer
  exist.
- **The expansion.** Everything here is the 2015 base game, "A New Story
  of Civilization". The corpus was verified free of expansion cards.
