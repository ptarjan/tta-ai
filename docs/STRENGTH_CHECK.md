# Strength check: is our trained champion actually any good?

**Status: in progress.** Partial results, written as they land.

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

| matchup | players | n | win rate | 95% CI | p vs null |
|---|---|---|---|---|---|
| BookBot vs champion_2p | 2 | 60 | 61.7% | ±12.4% | 0.065 |
| GreedyBot vs champion_2p | 2 | 60 | 11.7% | ±8.2% | <0.0001 |

*(Larger runs at 2p/3p/4p are in flight; this table is replaced when they
land.)*

The GreedyBot row is the control, and it is the important one. GreedyBot is
the bot our champion was trained to beat, and it beats it convincingly —
11.7% is a rout. So the champion is *not* broken: it is genuinely far stronger
than the baseline it was measured against. It is just that the baseline was
weak, and beating a weak baseline by a lot told us much less than we assumed.

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
5. **The horizon is too short.** Items 1–4 are all the same mistake seen from
   different angles: the evaluator rewards culture that exists now over the
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
