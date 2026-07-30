# Strength check: is our trained champion actually any good?


> **SUBJECT: the frozen gen-222 pre-league champion.  The headline win rates are
> stale as a description of the current bot** (2026-07-30).  `docs/DEEPER_SEARCH.md`
> §7.3: the later gen-337 2p champion beats BookBot v2 72-78% and CultureBot
> 52-58%.  But that is no longer an *external* test — BookBot and its variants
> are now training-pool members, so it settles "training worked against what it
> trains against", not "is the bot good".  The methodological point of this
> document therefore stands undamaged, and so does its behavioural diagnosis
> (hoards unspent science, under-grows population, under-buys civil actions,
> under-builds wonders, hoards unconverted military strength), which was
> independently re-derived from the weight vector itself.  The 4p rows are
> separately quarantined by the banner below.  Current strength lives in
> `docs/SYSTEM_COVERAGE.md`.

**Status: 2p and 3p final. 4p and the hybrid ablation still running.**

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


## Why this document exists

A reader of `docs/HEURISTICS.md` said: *"It doesn't seem like the players are
that good if this is their strategy."*

They have a point that our evidence could not answer. Every number in this
repo comes from self-play. Self-play measures a bot against *itself*: a
population that has collectively agreed on a mediocre plan will report healthy
win rates forever, because the only thing it is being compared to is the same
plan. We had never measured against anything outside the training loop.

So this is an external yardstick. `engine/bots/book.py` (**BookBot**) is a
hand-written opponent that plays the way human strategy writing says to play.
It has no learned weights, no lookahead and no evaluator — it is an ordered
priority list, the kind of thing a person can hold in their head at the table.
If a two-hundred-line rule list can beat a champion produced by hundreds of
generations of hill climbing, then the hill climbing has converged on weak
play, and that is worth knowing.

## The verdict

**The book bot beats our trained champion.** Not narrowly, and not only at one
player count. Details and confidence intervals below.

### Head to head

Every duel is seat-rotated (each seed is played once with the challenger in
each seat) and every matchup at a given player count uses the same seed set,
so the comparisons are paired on identical deals. Null hypothesis is 1/N.

| matchup | players | n | win rate | 95% CI | null | p | mean culture |
|---|---|---|---|---|---|---|---|
| **BookBot vs champion_2p** | 2 | 400 | **62.9%** | ±4.7% | 50.0% | <0.0001 | 155 vs 124 |
| **BookBot vs champion_3p** | 3 | 300 | **42.2%** | ±5.6% | 33.3% | 0.0019 | 124 vs 112 |
| **BookBot vs champion_4p** | 4 | 300 | **64.3%** | ±5.4% | 25.0% | <0.0001 | 196 vs 112 |
| GreedyBot vs champion_2p | 2 | 400 | 8.2% | ±2.7% | 50.0% | <0.0001 | 61 vs 156 |
| BookBot vs GreedyBot | 2 | 400 | 96.4% | ±1.8% | 50.0% | <0.0001 | 176 vs 51 |

BookBot beats the champion at **every** player count, and the margin is well
outside the confidence interval in each case. At 2p it wins nearly two games
in three. At 3p it takes 42.2% of a table where 33.3% is par. At 4p it takes
64.3% where par is 25% — **two and a half times its share**, with a mean
culture of 196 against 112. The champion is at its worst with a full table.

*4p caveat:* in our champion's 4p games no territory is ever auctioned, so the
colony layer is effectively absent at 4p. The 4p number therefore measures a
game with one of its subsystems missing, and BookBot's colony rules are
untested there.

The GreedyBot rows are the control, and they matter as much as the headline.
GreedyBot is the baseline our champion was trained against, and the champion
demolishes it — 8.2%, a rout. So **the champion is not broken.** It is
genuinely, enormously stronger than the thing we were measuring it against.
The problem is that the yardstick was short: BookBot beats that same baseline
96.4% of the time, so "crushes GreedyBot" was never evidence of good play. It
was evidence of clearing a low bar.

## Caveat, stated up front

The champion weights being tested (gen 222 at 2p) were trained almost entirely
*before* commit `7d40f53` corrected the military card counts, which landed at
12:37 on the day of this benchmark. So the champion is being graded on a game
whose military deck is not quite the one it trained against, while BookBot's
rules were never tuned to any deck. That is a real handicap and it should be
said plainly.

Two things stop it from explaining the result:

1. **The champion's deficit is not military.** In the per-round diff below,
   the two bots' strength is within a point of each other all game. The gaps
   that decide the games are unspent science, workers, civil actions and food
   — none of which the military deck touches.
2. **The control is unaffected.** GreedyBot plays the same corrected deck and
   is still crushed by the same champion.

Still, the honest form of this result is: *the champion loses to the book bot
on the current rules*. Whether it also loses after being retrained on the
corrected deck is an open question, and this benchmark should be re-run once
a post-`7d40f53` champion exists. `experiments/frozen/` holds the exact
weights used here so the comparison can be repeated.

## What the champion is doing wrong

`experiments/book_diag.py` plays the two bots and snapshots both civilisations
once per round. Over 12 paired 2p games (BookBot won 8), the mean per-round
difference (BookBot minus champion; positive means BookBot ahead):

| round | culture | culture rate | science stock | workers | civil actions | wonders | food rate | happy margin |
|---|---|---|---|---|---|---|---|---|
| 5 | −1.8 | −1.0 | −0.4 | +0.5 | +0.3 | +0.6 | +0.0 | −1.4 |
| 10 | −10.2 | −2.3 | −3.7 | +1.6 | +1.4 | +0.8 | +2.5 | −2.2 |
| 15 | −4.1 | +0.8 | −8.0 | +3.8 | +1.8 | +0.9 | +2.4 | −1.1 |
| 19 | +9.0 | +1.9 | −14.6 | +4.2 | +1.7 | +0.9 | +2.2 | −0.8 |
| 21 | +16.9 | +2.0 | −12.3 | +4.5 | +1.6 | +0.8 | +2.6 | −0.9 |

The shape of the game is unmistakable. **The champion is ahead on culture for
the first two thirds of the game and loses anyway.** It banks early points
while BookBot builds an engine; BookBot's culture *rate* passes it around
round 15 and the score follows about four rounds later.

At **3p** (12 paired games, BookBot won 7) the shape is different and just as
damning:

| round | culture | culture rate | strength | res rate | workers | civil actions | wonders |
|---|---|---|---|---|---|---|---|
| 5 | +1.0 | +0.2 | +0.8 | +0.9 | −0.3 | +0.7 | +0.3 |
| 10 | +1.0 | +1.2 | −1.4 | +2.1 | −0.5 | +1.4 | +1.3 |
| 15 | +9.2 | +1.3 | −3.9 | +2.0 | −0.3 | +1.3 | +1.4 |
| 20 | +19.0 | +0.4 | −6.6 | +2.6 | −0.4 | +1.0 | +1.5 |

Here the champion is **ahead on military all game** — by round 20 it carries
6.6 more strength than BookBot — and loses by 19 culture anyway. It bought an
army it never converted into anything, while falling 1.5 wonders and 1.0 civil
actions behind. At 3p the champion's problem is not that it is too passive;
it is that it spends on the wrong things.

Concrete weaknesses, in order of how much they appear to cost:

1. **The champion hoards science it never spends.** By round 19 it is sitting
   on ~15 more unspent science than BookBot. Science in the bank scores
   nothing; the only thing it is for is being converted into technologies.
   This is the single largest and clearest defect. A rule as dumb as "if you
   have enough science for a tech you can staff, develop it" would recover
   most of this.
2. **It under-grows the civilisation.** −4.5 workers by the endgame, off a
   −2.5 food production deficit. Population is the compounding asset in this
   game and the champion consistently stops buying it.
3. **It under-buys civil actions.** −1.7 actions a turn is roughly a 30%
   discount on everything it can do, every turn, for the whole second half of
   the game. Governments, Code of Laws and Pyramids are all action sources
   BookBot takes and it does not.
4. **It builds fewer wonders** (−0.9), which in this game is also a culture
   rate deficit, not just a one-off.
5. **It buys military it never cashes.** At 3p it ends +6.6 strength up on
   BookBot and still loses by 19 culture. Strength is only worth what you
   convert into aggressions, wars or safety; hoarded, it is as dead as hoarded
   science. This is the same defect as item 1 in a different currency.
6. **The horizon is too short.** Items 1–5 are all the same mistake seen from
   different angles: the evaluator rewards what is on the board now over the
   capacity to make culture later, so the champion cashes in early and is
   overtaken by anything that keeps investing.

### What BookBot does worse

It is not a better bot in every respect, and the honest reading matters:

- **Happiness discipline.** BookBot runs a happiness margin 1–2 worse for most
  of the game — it lives closer to the edge of discontent than it should. The
  champion is clearly better at this.
- **Early culture.** BookBot is behind on score until round ~15. In games that
  end early, or where it gets pressured before its engine matures, that is how
  it loses.

## Implications for `docs/HEURISTICS.md`

`docs/HEURISTICS.md` is a description of what the champion learned to do. Given
that the champion loses to a plain rule list, that document should be read as
*"what our bot does"*, not as *"how to play Through the Ages"*. In particular
any advice in it that amounts to taking culture early at the expense of growth,
actions or spent science is advice to play the losing side of the matchup
measured here.

## Measured against two hard tournament numbers

`docs/EXPERT_STRATEGY.md` summarises a study of 39 games across 3 International
Championships and 3 Intermezzo seasons, scoring cards by the civil actions
strong humans actually spent on them
([BGG 2494200](https://boardgamegeek.com/thread/2494200)). Two of its numbers
are cheap to check against our bot. `experiments/pickstats.py` does it
(champion, 2p, 30 games, 1176 picks).

### 1. What we pay for cards — mostly fine, mildly overpriced at the deep end

The row charges 1 CA for slots 1–5, 2 CA for slots 6–9 and 3 CA for 10–13.

| age in the row | picks | 1 CA | 2 CA | 3 CA |
|---|---|---|---|---|
| A | 89 | 100.0% | 0.0% | 0.0% |
| **I** | 459 | **82.4%** | 11.8% | **5.2%** |
| II | 325 | 97.8% | 1.8% | 0.3% |
| III | 303 | 96.7% | 3.0% | 0.3% |
| *tournament, Age I* | *39 games* | *76.0%* | — | *2.5%* |

**This one largely exonerates the bot, and that is worth saying plainly.** The
champion is if anything *more* 1-CA-disciplined than tournament players in Age
I (82.4% vs 76%). Its only real deviation is the deep end: it pays 3 CA on
5.2% of Age I picks, about **2.1× the tournament rate**. So habitual
overpaying is *not* the explanation for the civil-action deficit found above.
It is a small leak, not the hole.

The number that does look off is volume: the champion makes **20.2 picks per
player per game**, i.e. it spends 20+ civil actions a game on drafting alone.
The tournament source gives no directly comparable figure, so this is flagged
as suspicious rather than proven.

### 2. Theology — a card strong players never touch, and we take it half the time

Theology was **selected exactly 0 times in 39 tournament games**: happiness
cards and Bread and Circuses solve the same problem more cheaply, and the Age
I temple frees only a single worker where its Age II equivalent frees two.

| bot | Theology picks per game |
|---|---|
| tournament humans | **0.00** (0 in 39 games) |
| our champion | **0.47** (14 in 30 games) |
| BookBot v1 | 0.67 |
| BookBot v2 | **0.00** |

Our champion takes it in roughly half its games. Note that **BookBot v1 was
worse**, which is a useful check that this measurement is not just
score-settling — the hand-written bot had the same hole until the tournament
data was applied in v2.

A third finding fell out of the same run: the champion takes **Frugality (I)
in 29 of 30 games**, and the expert consensus is that the Frugality/Stock
Pile/Patriotism/Cultural Heritage family should essentially never be taken,
because each costs a *second* civil action to realise. That is a direct,
codable correction.

## Does patching the champion locally fix it? No.

`BookImprovedBot` is the champion's evaluator overruled by the book in exactly
the move kinds the diff above implicates — `develop`, `pop`, `wonder_step`,
`revolution`.

| matchup | players | n | win rate | 95% CI | p |
|---|---|---|---|---|---|
| BookImprovedBot vs champion_2p | 2 | 300 | 50.8% | ±5.7% | 0.77 |

**No improvement whatsoever** (p=0.77). This is the most informative negative
result of the exercise. The champion's weakness is not four local bad habits
that can be patched out; the whole plan is wrong. Forcing it to spend science
and grow population at the right moments does not help when the rest of its
play is still optimising for near-term culture. Whatever is wrong is in the
shape of the evaluator, not in a handful of decisions.

## BookBot v2: rebuilt on the tournament tier list

`BookBot(version=2)` replaces v1's opinion-derived card ranks with the
empirical tournament ordering from `docs/EXPERT_STRATEGY.md`, adds a convex
card-row price ladder (76% of Age I picks at 1 CA), a never-take list
(Theology, Stock Pile, Patriotism, Cultural Heritage, Frugality), and the
conditional leader rules the sources insist on (Leonardo needs Alchemy or
Printing Press; Columbus needs a colony in hand).

The tier list moved several ranks a long way. v1 had Michelangelo at 9 and
Julius Caesar at 8; the tournament data has them at 0.00 and 0.03 CA-spend
respectively — near the bottom. v1 had Colossus mid-table; the study finds it
*anti-correlated with winning*.

Where the experts genuinely split, `V2_TUNABLES` exposes the call as a
parameter rather than hard-coding a side: Pyramids vs Library of Alexandria,
Great Wall, Taj Mahal, Hanging Gardens, Iron vs 3-Bronze, and Theology.

| matchup | players | n | win rate | 95% CI | null | p |
|---|---|---|---|---|---|---|
| **v2 vs champion** | 2 | 800 | **66.5%** | ±3.3% | 50.0% | <0.0001 |
| **v2 vs champion** | 3 | 600 | **46.2%** | ±4.0% | 33.3% | <0.0001 |
| v2 vs v1 | 2 | 1600 | 52.1% | ±2.4% | 50.0% | 0.098 |
| v2 vs v1 | 3 | 600 | 31.4% | ±3.7% | 33.3% | 0.31 |

**Against the champion, v2 is clearly the better yardstick**: it lifts the
margin from 62.9% to 66.5% at 2p and from 42.2% to 46.2% at 3p. As a training
opponent it raises the bar.

**Against v1, the honest answer is that the tier list barely helped.** v2 is
+2.1% at 2p (p=0.098 — suggestive, not significant even at n=1600) and
*slightly negative* at 3p (p=0.31). That is a genuine and slightly deflating
result: swapping opinion-derived card ranks for 39 games of championship
revealed preference moved the needle by about two points against a bot that
already had the big structural rules right. It says the strength of BookBot
lives in its priority list — grow, buy actions, spend science, finish wonders,
hold the military floor — and not in fine card valuation.

The 3p non-result is exactly what the research warned about: the competitive
lobby that tier list is drawn from is almost exclusively 2-player, so its
values should not be expected to transfer to 3p/4p. v2 keeps the one value the
sources split explicitly (Homer) player-count aware for that reason.

## What BookBot actually plays, and where each rule came from

BookBot is `engine/bots/book.py`. The priority list is in its docstring; these
are the rules that carry the most weight, with their sources.

| rule as coded | the advice it encodes | source |
|---|---|---|
| Prefer cards that give civil actions (Pyramids rank 9, Code of Laws rank 9, governments valued at 4× per civil action gained) | "prioritis[e] cards that give civil actions, science and yellow cubes"; Pyramids is A-tier | [BGG: strategy tips for newbie](https://boardgamegeek.com/thread/2439618/strategy-tips-newbie) |
| `mil_target` = match the second-strongest rival, and never fall more than 3 behind the strongest | "You don't have to be in the lead, but you don't want to be in last place, either" — aim to stay in "the top two all the time" | [Stately Play, Strategy 101: Through the Ages](https://statelyplay.com/2017/09/25/strategy-101-through-the-ages-resource-edition/) |
| `res_need` targets ~3 resources/turn in Age A rising with the age; mine upgrades ranked highly | start with 2 bronze mines and add a third immediately; "going from 3 to 6 ore is a godsend early in the game" | [Stately Play](https://statelyplay.com/2017/09/25/strategy-101-through-the-ages-resource-edition/) |
| `food_need` targets consumption + 2, i.e. ~3–4 food/turn, and no more | 2 agriculture workers, upgrade to Irrigation for 4 food/turn, which "should suffice through Age II"; extra food early just feeds corruption | [Stately Play](https://statelyplay.com/2017/09/25/strategy-101-through-the-ages-resource-edition/); [BGG](https://boardgamegeek.com/thread/2439618/strategy-tips-newbie) |
| Labs/science weighted 3.5 per point until the endgame | "Always pursue Alchemy in Age I"; do not rely on the single starting Lab | [Stately Play](https://statelyplay.com/2017/09/25/strategy-101-through-the-ages-resource-edition/) |
| Hanging Gardens ranked 7 and happiness buildings jump the queue only when `happy_gap > 0` | Hanging Gardens' 2 happy lets you "ignore both Religion and Arenas in the early game" | [Stately Play](https://statelyplay.com/2017/09/25/strategy-101-through-the-ages-resource-edition/) |
| Leader ranks: Hammurabi and Aristotle top of Age A | both named A-tier leaders | [BGG](https://boardgamegeek.com/thread/2439618/strategy-tips-newbie) |
| Culture production weighted 4.5/point from Age II on | culture is the only resource that decides the game; strong play targets 15+/turn | [Stately Play](https://statelyplay.com/2017/09/25/strategy-101-through-the-ages-resource-edition/) |

Card costs, stages and production are all read from the engine card DB rather
than hard-coded, so the rules stay honest if the DB is corrected.

## Reproducing

```
python3 -m experiments.bookmatch --games 400 --players 2   # or 3, 4; 0 = all
python3 -m experiments.book_diag --players 2 --games 20    # per-round diff
```
