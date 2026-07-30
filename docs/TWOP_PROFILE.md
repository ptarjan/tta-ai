# What the 2-player champion actually does


> **SUBJECT: the gen-181 quiescent, margin-gated 2p champion.  That bot no longer
> exists** (2026-07-30).  The gate metric it exploited was changed specifically to
> kill this behaviour (`docs/LEAGUE_OBJECTIVE.md`, whose primary motivating
> evidence is this document), and the current live 2p champion declares 1.10
> wars/game against this one's 1.48-1.98 and completes 1.53 wonders against this
> one's 0.16-0.26 (`docs/SYSTEM_COVERAGE.md`).  Read the numbers here as "true of
> that vector".  What is *not* superseded: the causal method (ban a move class and
> re-measure), the finding that the win came from **suppression rather than
> scoring**, the MilitaryBot decomposition, and this document's naming of the
> zero-sum margin bug that `LEAGUE_OBJECTIVE.md` later fixed.

Date: 2026-07-27
Subject: `experiments/league_state/champion_2p.json` (gen 181), played the way
the league plays it: `QuiescentBot(weights=champion, levels=1)`.

Tools written for this (additive, nothing in `engine/` touched):
`tools/twop_profile.py` (collect), `tools/twop_report.py` and
`tools/twop_summary.py` (aggregate). Raw per-game records land in `/tmp/twop_*`
and are not committed; every table below is regenerable from the two commands
in §2.

**One-line answer: the champion is a war bot.** It builds a strength lead it
never uses on units, declares its first war around round 15, and takes the
opponent's culture off the scoreboard. Removing the war/aggression move class
costs it 59.0 ± 3.1 of its 85.5-point margin against `book`.

---

## 1. The question, and why the weight vector cannot answer it

The 2p champion beats every member of its training pool by 70-90 culture
points. The obvious move — read the strategy off the 78 trained weights — is
ruled out twice over:

1. Champion weight marginals are statistically indistinguishable from a random
   walk (KS p = 0.14-0.80) even while the champion massively outperforms its
   drift siblings. An individual trained weight carries no interpretable
   strategy.
2. `experiments/league_state/weight_credit_2p.json` reports 0 load-bearing /
   1 harmful / 17 no-measurable-effect out of 18 weights, from **1 cycle,
   n = 72, with edges of order 0.01-0.04**. That is an underpowered ablation,
   not a finding that weights do not matter. It can exclude effects much
   larger than its own resolution on the 18 weights it touched; it cannot rule
   anything *in*, and it says nothing about the other 60 weights.

So everything below is measured from played games.

## 2. Method

### Matchups and statistics

Opponents are the real 2p pool only: `book`, `book2` and the six `var:*`
variants. `default` / `greedy` / `random` are excluded on purpose — they are
saturated and cannot vary, which is the trap this repo has hit before.

Each pool matchup is **n = 300 games**, seat-balanced (each seed played once
with the champion in each seat). Null win share 0.5; null margin 0. Ablation
and control matchups are n = 200.

Every number is a **per-game mean ± the standard error of that mean**. Champion
and opponent are measured on the *same* games, so a "difference" column is a
paired difference with a paired SE. Where two *configurations* are compared
(§6, §7) the seed sets are shared and the comparison is paired game by game;
n is stated.

```
python3 tools/twop_profile.py --games 300 --workers 3 --out /tmp/twop_main
python3 tools/twop_report.py  /tmp/twop_main
python3 tools/twop_summary.py /tmp/twop_main
```

### Instrumentation, and the proof it changes nothing

Two additive layers:

* **Recorder** — `experiments/behaviour.py::Recorder`, imported unchanged,
  subclassed only to also note developed-tech names, prepared-event names and
  one derived per-turn quantity (`lead_over_rival`, §5). It asks the wrapped
  bot for a move, notes it, and hands it back unmodified.
* **Culture-source ledger** — `PlayerState.__setattr__` is swapped for a hook
  that records every write to `culture` together with the engine `file:line`
  that wrote it. It is installed **only around the real `actions.apply` call**
  in the driver loop, i.e. never while a bot is searching, so search is neither
  slowed nor observed. The hook chains to the previous `__setattr__` (under
  `TTA_JOURNAL=1`, `journal._journalling_setattr`).

Checks that the instrumentation is inert:

| check | result |
|---|---|
| driver + recorders + ledger vs stock `game.play_game`, same seeds | identical final scores, 12/12 seeds, two different opponents |
| ledger sums to the player's own final score | 300/300 games, every matchup, 0 mismatches |
| adding the event-attribution wrapper | margin on a fixed 8-seed set unchanged (55.2 before and after) |
| `python3 -m unittest discover -s tests -q` | passes (nothing in `engine/` was modified) |

## 3. The headline reproduces

| matchup | n | win share | margin | score (champ/opp) |
|---|---|---|---|---|
| vs book | 300 | 0.957 ± 0.012 | +85.5 ± 2.5 | 131 / 46 |
| vs book2 | 300 | 0.965 ± 0.010 | +83.3 ± 2.5 | 131 / 47 |
| vs var:culture | 300 | 0.952 ± 0.012 | +81.8 ± 2.5 | 131 / 50 |
| vs var:infra | 300 | 0.963 ± 0.011 | +85.3 ± 2.6 | 132 / 46 |
| vs var:military | 300 | **0.985 ± 0.007** | +72.1 ± 1.6 | **81 / 9** |
| vs var:science | 300 | 0.977 ± 0.009 | +84.7 ± 2.2 | 128 / 44 |
| vs var:tempo | 300 | 0.945 ± 0.013 | +77.8 ± 2.6 | 128 / 50 |
| vs var:wonder | 300 | 0.928 ± 0.015 | +78.4 ± 2.8 | 133 / 55 |

Consistent with the league's own full check (97.9 / 95.8 / 89.6 / 93.8 / 100 /
97.9 / 97.9 / 91.7 at n = 48). Zero engine errors in 2 400 games.

## 4. Where the points come from

The ledger attributes every culture point to the engine line that awarded it.
Against `book` (n = 300, paired):

| source | champion | book | difference |
|---|---|---|---|
| **aggression / war transfers** | **+31.6 ± 1.0** | **−30.4 ± 1.0** | **+62.0 ± 2.0** |
| events resolved in play | +23.1 ± 0.9 | +9.9 ± 0.6 | +13.1 ± 0.9 |
| preparing events (seeding) | +16.8 ± 0.3 | +25.1 ± 0.3 | −8.3 ± 0.4 |
| culture RATE (production phase) | +35.3 ± 0.7 | +27.8 ± 1.5 | +7.5 ± 1.5 |
| end-game Age III events | +20.2 ± 0.7 | +14.6 ± 0.5 | +5.6 ± 0.7 |
| one-off card / build culture | +5.7 ± 0.3 | +1.3 ± 0.2 | +4.4 ± 0.4 |
| food / war penalties | −1.6 ± 0.4 | −2.7 ± 0.5 | +1.1 ± 0.6 |
| **total** | **131.0** | **45.5** | **+85.5** |

Broken out to the raw engine sites, the conflict line is almost entirely one
thing: `war:spoils` (`events.py:590`) pays the champion **24.95 ± 0.94** per
game and costs `book` exactly the same, and `book` never wins a war back
(`war:lost_to_victor` for the champion is 0.00 ± 0.00). Aggression theft adds
another ±5.51 ± 0.33, plus 1.09 ± 0.13 for stealing rival leaders.

The conflict swing is 55-65 points against seven of the eight opponents:

| matchup | conflict swing | culture-rate swing | event swing |
|---|---|---|---|
| vs book | +62.0 ± 2.0 | +7.5 ± 1.5 | +10.4 ± 1.3 |
| vs book2 | +59.1 ± 2.0 | +9.1 ± 1.4 | +10.5 ± 1.4 |
| vs var:culture | +65.1 ± 1.8 | **+1.1 ± 1.7** | +10.1 ± 1.3 |
| vs var:infra | +59.7 ± 1.9 | +13.1 ± 1.3 | +8.4 ± 1.4 |
| vs var:science | +64.5 ± 1.8 | +10.9 ± 1.3 | +4.9 ± 1.4 |
| vs var:tempo | +54.7 ± 1.9 | +11.6 ± 1.4 | +7.2 ± 1.4 |
| vs var:wonder | +63.0 ± 2.0 | +2.9 ± 1.5 | +6.5 ± 1.3 |
| vs var:military | **+10.5 ± 0.8** | **+32.3 ± 0.9** | +24.9 ± 1.1 |

**It is not an economy.** Against `var:culture` — the bot built to maximise a
culture engine — the champion's culture *production* over the whole game is
+1.1 ± 1.7 points, i.e. indistinguishable from zero, and it still wins by 81.8.
It completes 0.16-0.26 wonders per game against 0.80-1.41 for the opponents,
and it buys essentially no culture buildings: per game it takes 0.02 theatres,
0.01 temples, 0.03 libraries and 0.00 labs, against `book`'s 2.22 / 1.03 /
1.98 / 1.13. It ends Age III on 8.4 ± 0.1 technologies against `book`'s
10.3 ± 0.1 — it is *behind* on tech.

### The causal check (this is not just accounting)

The table above says where points landed, not that removing the source would
remove the margin. `MoveClassBan` in `tools/twop_profile.py` hands the bot a
filtered legal-move list, so a move *class* is removed without touching the
engine, the weights or the search. Same seeds, paired, n = 200 game-pairs:

| champion configuration | win vs book | margin | paired change |
|---|---|---|---|
| full | 0.957 ± 0.012 | +85.5 ± 2.5 | — |
| war banned | 0.877 ± 0.023 | +46.2 ± 2.9 | **−38.8 ± 2.3** |
| war + aggression banned | 0.710 ± 0.032 | +25.9 ± 3.0 | **−59.0 ± 3.1** |

69% of the margin is the conflict move class, and the mechanism is
**suppression, not scoring**: banning the fighting barely moves the champion's
own total (131.0 → 119.8) but nearly doubles `book`'s (45.5 → 93.8). `book`'s
culture *production* is unchanged across all three conditions (27.8 / 27.3 /
29.0), so the champion is not wrecking the opponent's economy — it is taking
finished points off the scoreboard.

A peaceful champion still beats `book` (71.0 ± 3.2%), so the trained weights
are better than the book bot on their own; the fighting is what turns "better"
into "90-point blowout". Against a *copy of itself*, the disarmed champion
scores 0.398 ± 0.034 (null 0.5, margin −13.4 ± 3.0).

## 5. How it gets the strength — and the MilitaryBot question

*"If it is a war bot, why doesn't it lose to the hand-coded war bot?"*

It does not out-build MilitaryBot. It out-*tactics* everyone, and then
deterrence does the rest.

**Strength comes from tactics, not from army size.** Against `book` the two
sides put the same number of workers on units (3.09 ± 0.04 vs 3.15 ± 0.06) and
buy the same near-zero number of unit cards (0.09 vs 0.12), yet the champion
ends Age III at strength 15.0 ± 0.2 against 5.0 ± 0.2. The difference is
tactic upkeep: 7.06 ± 0.24 tactic plays/copies per game against `book`'s
0.99 ± 0.00 (`book` plays exactly one tactic per game and then stops). The
champion also runs 4.70 ± 0.03 military actions per turn against 2.67 ± 0.03.

**Against MilitaryBot the mechanism is deterrence, and it is decisive.** The
variants gate their offence on `variants/base.py::_lead_over` =
`attack_strength(me, rival) − rival.strength`; MilitaryBot needs ≥ 3-4 to
launch an aggression and ≥ 5 to declare a war. That exact quantity is recorded
per turn. n = 200 each:

| MilitaryBot playing… | aggressions | wars | events seeded | its score | turns with lead ≥ 3 |
|---|---|---|---|---|---|
| vs `book` | 2.75 ± 0.18 | 0.71 ± 0.07 | 6.11 ± 0.20 | 66.8 ± 2.7 | 42.2% |
| vs `var:culture` | 3.13 ± 0.18 | 0.80 ± 0.08 | 5.78 ± 0.18 | 67.1 ± 2.7 | 44.1% |
| vs `var:wonder` | 2.60 ± 0.18 | 0.61 ± 0.07 | 5.88 ± 0.21 | 64.6 ± 2.9 | 40.6% |
| **vs the champion** | **0.27 ± 0.06** | **0.04 ± 0.02** | **1.10 ± 0.12** | **9.2 ± 1.2** | **5.5%** |

MilitaryBot is a perfectly respectable bot against the rest of the family
(46.8%, 46.8%, 44.0% win share vs book / culture / wonder, null 50%). Against
the champion its entire offensive plan never fires: aggressions fall by a
factor of 10, wars by a factor of 17, and it scores **9.2** culture in a whole
game.

Two things are doing that, and the second is the bigger one:

1. **Deterrence.** The champion's strength lead over MilitaryBot grows
   monotonically — +1.1 at round 5, +3.4 at round 10, +5.6 at round 15, +8.0 at
   round 19 — and MilitaryBot holds the +3 it needs on only 2-8% of turns.
2. **A self-inflicted lockout.** MilitaryBot's profile sets
   `seed_events_when_weakest: False`. Being permanently the weaker player, it
   refuses to prepare events at all: 1.10 ± 0.12 per game against 5.8-6.1 in
   its normal games and 8.93 ± 0.17 for the champion. That alone is a
   15.5 ± 0.4 point swing on the seeding line, before any of the downstream
   event culture. It buys 4.13 ± 0.06 unit cards per game (the champion buys
   0.07) and converts them into 5.87 ± 0.24 defends and nothing else.

Note the shape: against MilitaryBot the champion's margin is its *smallest*
(+72.1) but its win rate is its *highest* (98.5%). War spoils are
`min(5 + advantage, loser.culture)` — there is simply nothing left to steal
from a bot on 9 points, so each war pays 4.60 ± 0.29 instead of the
12.36 ± 0.25 it collects from `book`.

## 6. When the game is decided

Score gap at the end of each champion turn, vs `book`, n = 300:

| round | 3 | 6 | 9 | 12 | 13 | 15 | 17 | 18 | 19 | 20 |
|---|---|---|---|---|---|---|---|---|---|---|
| gap | +1.4 | +3.7 | +7.0 | +9.9 | +11.5 | +21.8 | +44.0 | +56.8 | +69.2 | +78.1 |
| ± | 0.1 | 0.2 | 0.5 | 0.8 | 1.0 | 1.4 | 1.7 | 1.9 | 2.1 | 3.0 |
| age | I | I | II | II | III | III | III | III | IV | IV |

Through Ages I-II it is quietly ahead by ~10 points — that is the tempo edge
(§7), not the war. The blow-out is entirely Age III onward: the first
aggression lands at median round 13 (first one at round 9.90 ± 0.23, in 95% of
games) and the first **war** at median round 16 (round 15.29 ± 0.09, in 94% of
games), 1.98 ± 0.06 wars per game at 12.36 ± 0.25 culture each. Between round
13 and round 19 the gap goes +11.5 → +69.2. So: it builds through Ages I-II
and cashes in Age III, which is exactly the window
`engine/bots/variants/military.py` documents as the right one — *"declare wars
at the end of Age II or the beginning of Age III"* — and it is the only bot in
the pool that actually gets there.

## 7. Tempo, and the rest of the profile

Per own turn, vs `book`:

| | champion | book |
|---|---|---|
| civil actions available | 4.50 ± 0.02 | 5.00 ± 0.03 |
| civil actions left unspent | **0.83 ± 0.02** | **2.17 ± 0.03** |
| share of turns with any CA left | **25.0 ± 0.7%** | **64.5 ± 0.6%** |
| military actions available | 4.70 ± 0.03 | 2.67 ± 0.03 |
| military actions left unspent | 2.98 ± 0.04 | 2.14 ± 0.03 |

It has *fewer* civil actions than `book` and spends far more of them. The same
holds against every variant (0.83-0.98 unspent vs 2.08-2.68). It also takes
more cards despite having fewer actions (24.19 ± 0.19 vs 20.42 ± 0.13),
financed by yellow action cards: 10.25 ± 0.14 action cards taken per game
against `book`'s 2.15 ± 0.07, and 4.48 ± 0.10 `play_action` moves against 1.28.

What it develops is military and enabling tech, not culture tech. Share of
games in which it develops (champion | book): Strategy 0.87 | 0.00,
Warfare 0.80 | 0.06, Navigation 0.79 | 0.00, Cartography 0.68 | 0.14,
Justice System 0.55 | 0.00, Satellites 0.41 | 0.00, Military Theory 0.35 |
0.00. The other direction: Opera 0.00 | 0.56, Organized Religion 0.01 | 0.54,
Journalism 0.00 | 0.44, Alchemy 0.00 | 0.40, Drama 0.01 | 0.36. It takes
6.31 ± 0.08 special-tech cards per game against 2.04 ± 0.05.

It also harvests the opponent's own event deck: against `book`, culture from
events *the rival seeded* is 13.04 ± 0.66 for the champion and 1.45 ± 0.35 for
`book`. Many events reward the strongest player or rank on strength, and the
champion is the strongest player on 87% / 92% / 97% of turns in Ages I / II /
III. `book` seeds 12.36 events per game and the champion collects on them.

## 8. The strategy is a property of the search, not of the weights alone

Same weight vector, four configurations, paired on identical seeds, vs `book`,
n = 300:

| | 1-ply `WeightedBot` | quiescent (levels=1) |
|---|---|---|
| **DEFAULT_WEIGHTS** | −64.1 ± 3.9 (16.8%) | −62.3 ± 3.8 (16.0%) |
| **trained champion** | +65.2 ± 2.5 (90.5%) | **+85.5 ± 2.5 (95.7%)** |

Paired effects:

* training, at 1 ply: **+129.3 ± 4.3**
* training, under quiescence: **+147.8 ± 4.2**
* quiescence, on trained weights: **+20.3 ± 3.0**
* quiescence, on default weights: **+1.8 ± 3.2** (indistinguishable from 0)

Most of the champion's strength is in the weights. But the *style* is not:
**the same trained weights at 1 ply never fight at all** — 0.00 ± 0.00 wars and
0.01 ± 0.01 aggressions per game — and win their +65 a completely different
way, by farming the event deck: 13.71 ± 0.11 events prepared per game (vs 8.80
under quiescence), 28.4 seeding culture + 49.1 in-play event culture + 25.8
end-game Age III event culture = 103 of its 139 points.

That is exactly the mechanism `docs/DEEPER_SEARCH.md` predicts. An aggression's
payoff sits in `state.pending` and a war's resolves on the declarer's next
turn, so at 1 ply the evaluator sees the cost and none of the loot and the move
class is strictly dominated. Quiescence (plus `WAR_LOOKAHEAD`) resolves those
before scoring, and the class becomes selectable for the first time. The
champion was trained *under* that search, so "the 2p champion is a war bot" is
a statement about the deployed configuration — which is the one the league
scores and the one shipped in `champion_2p.json`.

## 9. What this does and does not support

**Supported.** Against this pool, at 2p, the champion's advantage is (a) a
tempo/action-economy edge worth about +10 by round 12, (b) a strength lead
built from tactics rather than units, and (c) converting that lead into direct
culture theft from Age III onward, worth 59.0 ± 3.1 of an 85.5-point margin.
It is not an economic engine and it is behind on tech and wonders.

**Not supported, and worth saying plainly.** Every one of the eight pool
opponents is `BookBot` or a `BookBot` subclass — `book`, `book2`, and all six
`var:*` bots inherit from it (`VariantBot(BookBot)`). The pool is a
**monoculture**, and the champion's dominance is measured entirely inside it.
Two of the effects documented above are explicitly *threshold* effects of that
family's hand-written rules: MilitaryBot's `agg_lead` / `war_lead` cut-offs and
its `seed_events_when_weakest` switch. A policy that keeps a rival just outside
a fixed numeric trigger is exploiting a rule, not necessarily playing well, and
that part of the 98.5% should not be expected to transfer to a human or to the
official app AI.

I cannot separate the two from this data. The evidence *against* pure
rule-exploitation is that the champion also beats a non-book opponent — a
same-architecture bot on `DEFAULT_WEIGHTS` — 98.5 ± 0.9% at +92.6 ± 2.6, and
that a *disarmed* champion still beats `book` at 71.0 ± 3.2%, so there is real
play underneath the exploitation. The evidence *for* is that the single largest
single-opponent collapse (MilitaryBot to 9.2 points) is fully accounted for by
two of its own hard-coded thresholds failing to fire. Settling it needs an
opponent from outside the BookBot family that is actually strong — which this
repo does not currently have. That is the measurement I would run next.

**Also worth flagging for training.** The margin metric the league gates on
(`margin_share`, scale 120) is being fed by a term that is *zero-sum by
construction*: war and aggression transfers move points between players, so a
policy that steals 25 points earns 50 points of margin while adding nothing to
its own board. That is a real 2p advantage, but it makes the margin signal
roughly twice as steep for theft as for production, which is a gradient the
climber will happily follow.
