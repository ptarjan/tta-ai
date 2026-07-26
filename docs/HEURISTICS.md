# Heuristics for human players

**Through the Ages: A New Story of Civilization — base game, 2015 edition, no expansion.**

Written for someone sitting at a table with the physical game. Everything here
comes from a rules-complete engine plus a self-play AI that is still training.

---

## How to read this document

**Snapshot warning.** The AI is mid-training, and it was still training while
this was written. Every behaviour number below was harvested on **2026-07-26**
from frozen copies of the champions at generation **149 (2 players), 116
(3 players) and 101 (4 players)** — **120 games per player count**, mirror
self-play, 0 engine errors (`experiments/behaviour_{2,3,4}p.json`). The climbs
kept running while this was written and were at **gen 176 / 132 / 113** when the
last edit was made, so the *weights* quoted here are slightly newer than the
*behaviour*. Those generations bought very little: each climb has accepted only
**15 / 10 / 6** mutants in total, and the most recent acceptance was at gen
147 / 120 / 103 — all three have been on a plateau for 30 / 12 / 10 generations.
Numbers will move, but slowly.
Structural advice ("spend your actions", "science early, culture late") is much
more stable than any single figure.

**How strong is the thing giving you advice?** The climb periodically re-plays
its champion against the hand-set weights it started from (`default`), a greedy
bot and a random bot, 96 games each. The last four such measurements
(`experiments/generations_*.jsonl`, rows containing `vs_default`):

| | vs. its own start point (last 4 anchors) | null | vs. a greedy bot |
|---|---|---|---|
| 2p (gen 130–160) | **78%** — individual runs 71.9 / 74.5 / 82.3 / 82.3 | 50% | 89.6–95.8% |
| 3p (gen 90–120) | **65%** — 59.9 / 60.4 / 68.2 / 70.3 | 33.3% | 74.0–80.2% |
| 4p (gen 80–110) | **72%** — 66.1 / 71.9 / 72.9 / 76.0 | 25% | 90.6–99.0% |

All three are clearly better than where they began — but note the spread. Each
individual 96-game anchor carries a ±8–10 point confidence interval, so the
bouncing above is mostly noise, not a champion getting better and worse from
week to week. **Do not read a 5-point difference between two of these rows as
meaning anything.** An independent measurement run earlier the same morning
(`experiments/logs/measure.log`, older champion files, different seeds) scored
the same match-ups much lower (2p 44.8%, 3p 60.4%, 4p 34.9% vs `default`); the
honest summary is "clearly above its starting point at 2p and 3p, probably at
4p, and nobody should quote a precise number".

**Where the numbers come from.**

| Source | What it is |
|---|---|
| `experiments/behaviour_{2,3,4}p.json` | **120** self-play games per player count (2,627 / 2,549 / 2,544 champion turns). What the champion actually *did*: milestone rounds, worker splits, rates by age, cards bought. Board snapshots taken at the end of each of its turns. |
| `experiments/logs/leak_check.log` | 60 games per count, instrumented: how much culture was actually destroyed by starvation and by uprisings, by age. The source for trap #2. |
| `experiments/analyze_weights.py` | Which of the 78 evaluation weights the search moved away from the hand-set defaults, and in which direction. |
| `experiments/PROGRESS.md` | Strength measurements against fixed baselines, and the search's history. |
| `docs/RULES_SPEC.md` | The rules themselves — every table in the Quick reference section is rulebook-accurate, not learned. |

**Confidence tags.** Each claim is tagged:

- **[rules]** — a fact from the rulebook. Not an opinion.
- **[strong]** — the behaviour data and the weight drift agree, and it holds at
  more than one player count.
- **[mixed]** — the player counts disagree, or two sources point different ways.
  Read the caveat before acting on it.
- **[provisional]** — one player count, one climb, small sample, or a plausible
  artefact of how the AI searches. Interesting, not proven.

**Three honest caveats you should carry through the whole document.**

1. *The three climbs have run for very different lengths.* 2p has accepted 15
   mutants in 169 generations, 3p 10 in 129, 4p only **6 in 111**. So when the
   counts disagree, the 4p number is the one most likely to be young rather
   than right — and the 4p weight vector contains some wild values (a
   `science` stock weight of **−6.09** against a hand-set +0.5, a `science_rate`
   of **+22.5** against +4.0) that look like one or two accepted mutants that
   have not been trimmed back. Treat extreme 4p figures as **[provisional]**
   unless the behaviour data backs them.
2. *All behaviour numbers come from mirror self-play.* The champion plays copies
   of itself, so any "relative to opponents" figure is close to 1.0 by
   construction and tells you very little. Absolute figures (my strength, my
   science rate, my worker split) are the useful ones.
3. *The search is 1-ply.* It never plans a combo two turns ahead. Anything in
   this document about *sequencing* is inferred from when things happened, not
   from the AI reasoning about them.

---

## If you remember nothing else

Eight rules. In rough order of how much they are worth.

1. **Spend all your civil actions in Ages A–II.** Actions do not carry over
   [rules] — an unspent action is simply destroyed at end of turn. Share of
   *available* civil actions the champions threw away, by age:

   | actions wasted | Age I | Age II | Age III | Age IV |
   |---|---|---|---|---|
   | 2p | 1% | 41% | 58% | 64% |
   | 3p | 2% | 48% | 70% | 60% |
   | 4p | 0.5% | 7% | 13% | 16% |

   In Age I **nobody wastes anything** — if you are leaving actions on the table
   in Age I you are already badly behind. Waste is an endgame phenomenon: by Age
   III there is often nothing left worth buying. The 4p champion is the outlier
   that keeps spending all game (0.38 wasted per turn against 1.74 at 2p and
   1.93 at 3p) and it finishes with by far the most technologies, **16.4 against
   12.9 and 9.8**. Weight evidence is thinner than it looks: only the 3p climb
   actually pushed the "leftover actions are nice" weights negative
   (`ca_left` +0.05 → −0.10, `ma_left` → −0.07); 2p and 4p left `ca_left`
   alone. **[rules] for the carry-over fact; [mixed] for how much the waste
   costs — the 2p champion wastes 58% of its Age III actions and still scores
   the most culture of any count.**

2. **Take a leader early and put it in play by round 3–4.** Median round to
   *take* one: 2 / 2 / 3. Median round to *play* one: 3 (2p), 5 (3p), 4 (4p).
   The champions play a leader at all in 96.7% / 82.5% / 98.3% of games, and
   have one in play on 70% / 42% / 54% of Age I turns. The "any leader in play"
   weight rose at both counts that have a large army of accepted mutants
   (+1.5 → +2.80 at 2p, +2.31 at 3p) but *fell* at 4p (+0.85), which is the
   count with only six accepted mutants — treat the 4p direction as noise.
   **[strong]**

3. **Upgrade your production on round 2.** At 2p the champion's first farm/mine
   upgrade lands on round 2 in **100% of games** (median and both quartiles are
   round 2). At 4p the median is also round 2 (99.2% of games do it eventually,
   mean round 3.5, upper quartile round 5). The first *urban* building upgrade
   follows on round 3 at 2p and 4p (both quartiles round 3), round 5 at 3p.
   3p is the exception on production and delays it badly — median round 8, and
   in 39% of games it never upgrades production at all. See the per-count
   section for why that is probably a flaw, not a plan. **[strong at 2p/4p]**

4. **Build about three temples, and never let an uprising happen.** Temple
   cards absorb **3.65 / 2.84 / 3.71** actions per game (researching, building
   and upgrading combined), first one at median round 5 / 8 / 8. Temples are the
   most-worked urban building at 2p and 3p; at 4p labs just edge them out
   (4.71). An uprising cancels your entire production phase [rules] and carries
   the largest penalty in the whole 78-weight evaluation: −12 by hand, and all
   three climbs pushed it further down, to **−14.0 / −15.5 / −21.2**.
   The reason you have probably never seen the champions suffer one is that
   they pay for happiness in advance — measured cost of uprisings is only
   **0.27 / 0.03 / 0.64 culture per player-game** (`leak_check.log`, 60 games
   each). That is the number you get *after* buying the temples, not instead of
   them. **[strong]**

5. **Science first, culture later — and the switch happens once, at the Age I /
   Age II boundary.** Champion science-rate-to-culture-rate ratio by age:

   | science ÷ culture | Age I | Age II | Age III | Age IV |
   |---|---|---|---|---|
   | 2p | 0.79 | 0.78 | 0.92 | 0.87 |
   | 3p | **1.67** | 0.63 | 0.60 | 0.58 |
   | 4p | **1.53** | 0.90 | 0.94 | 0.86 |

   It is *not* a smooth decline — it is one step down at the I → II boundary and
   then flat (2p and 4p even drift back up in Age III). So the practical version
   is: **out-science the table in Age I, then stop shifting and let the culture
   engine you built run.** All three climbs cut the "late science rate" weight
   (−3% / −40% / −66%), and the two counts that moved raised "early culture
   rate" (+54% at 3p, +178% at 4p). Note 2p's Age I ratio is already below 1 —
   at two players the champion is on culture from the start. **[strong for the
   direction; [mixed] on the exact crossover round — 3p and 4p cross inside Age
   II, 2p never has a science-heavy phase at all.]**

6. **Do not hoard. Not science points, not cards.** This is one of only four
   levers where **all three counts agree** (`analyze_weights.py` consensus
   table). Banked science: hand-set +0.5, champions **+0.19 / −0.19 / −6.09** —
   two of the three now consider a science pile actively *bad*. Cards in hand
   late: hand-set −0.2, champions **−0.35 / −0.40 / −0.33**, all further
   negative, while `hand_value_early` went *up*. Hold cards in Ages A–I; cash
   them out from Age II on. The behaviour agrees: banked science at the end of
   the game is 25.7 / 12.9 / 6.2, and the champion that banks least finishes
   with the most technologies (16.4 at 4p against 12.9 at 2p). **[strong]**

7. **Stop buying *science* rate in Age III. Be more careful about food.** All
   three climbs cut the late-game science-rate weight (−2.5 → **−2.58 / −3.49 /
   −4.14**): a lab bought in Age III does not pay for itself before scoring, so
   buy culture instead. Late *resource* rate is the same story at 2p and 3p
   (−0.4 → −0.50 / −0.62) but the 4p champion **flipped it positive (+0.49)** —
   and 4p is precisely the champion that is starving to death (trap #2). A farm
   bought in Age III that closes a food gap is not "rate", it is a penalty you
   stop paying, and it is worth roughly 24 culture over the rest of the game.
   Buy that farm. **[mixed — the science half is a 3-count consensus; the
   resource half depends on whether you are food-negative.]** Note also that no
   champion actually behaves this way: they keep buying in Age III at every
   count, so this rule rests on the weights, not on the behaviour.

8. **Military: nobody fights, but only the 2p champion is actually safe.**
   **Zero wars in 360 games** at all three counts, and aggressions are rare
   (0.01 / 0.03 / 0.11 per game). The champions' strength relative to the
   *strongest* rival, by age:

   | ratio to strongest rival | Age I | Age II | Age III | Age IV |
   |---|---|---|---|---|
   | 2p | 1.04 | 1.05 | 1.02 | 1.07 |
   | 3p | 0.82 | 0.84 | 0.78 | 0.75 |
   | 4p | **0.46** | **0.52** | **0.59** | **0.60** |

   Parity holds **at 2 players only** — where there is exactly one rival, so
   "the strongest rival" and "the average rival" are the same thing. At 3p the
   champion runs about 20% behind the table leader, and at 4p it runs at *half*
   the leader's strength and spends **48–52% of its turns below half the
   strongest rival's strength** [`military_by_age`, 120 games each].

   In absolute terms, so you know what these ratios are ratios *of*: champion
   strength averaged over **every Age III turn** is **3.1 (2p) / 6.8 (3p) / 2.3
   (4p)**, against a strongest rival of 3.0 / 8.8 / 3.8. (The snapshot taken on
   the single last turn *of* Age III is a little higher — 3.8 / 7.3 / 3.0 — which
   is the number quoted in the opening and per-count sections. Same data, one is
   an average over the age and one is its final turn.) A 3p table is running
   roughly twice the army of a 2p table at the same point in the game.

   **Read this as a possible weakness in the AI, not as advice.** In mirror
   self-play nobody attacks, so being weak is never punished and the search has
   no gradient telling it to build an army. A human table will punish it. What
   the data honestly supports is only the narrow claim: *at 2 players, matching
   your single opponent is sufficient and more is waste.* At 3p and 4p we do not
   know what the right number is — we only know the champions are below it and
   have never been made to pay. **[mixed — 2p only; 3p/4p is a likely artefact
   of mirror self-play]**

---

## Opening: Age A and the first four rounds

Age A is one round long. It ends the moment the card row is first replenished —
on the starting player's *second* turn — so you get exactly one turn in it, with
**1 / 2 / 3 / 4 civil actions by seat order, zero military actions, and taking
cards from the row as your only legal action**. [rules, §1.9]

Everything else in this section is Age I, rounds 2 through about 5.

### Round 1: take a card. That is the whole turn.

You cannot build, upgrade, play a leader or increase population on round 1 — the
rules do not allow it. [rules, §1.9] So the only question is *which* card.

The champions split into two answers, and both are defensible.

**2p and 3p take an action card.** Median round of the first action-card take is
1 at both counts, in 100% of games. The two Age A action cards are the most-taken
round-1 cards: at 2p `Frugality (A)` 0.37 per game and `Urban Growth (A)` 0.33
per game, both with a median round of 1. **[strong at 2p/3p]**

That is arithmetic rather than insight: on round 1 you have nothing to spend
resources on, and an Age A action card is a resource or food rebate you can cash
on round 2 when you suddenly have four actions and nothing banked. The seat-1
player, with a single civil action, gets exactly one shot at it.

**4p takes a wonder.** In **120 games out of 120**, the 4p champion takes a
wonder, and the median round it does so is **1** — p10 and p25 are both round 1,
p75 is round 2. It then starts building it around round 5. **[provisional — one
player count, and see the completion problem below]**

There is a clean rules argument for this. A wonder goes **directly into play
sideways and never enters your hand**, so the civil hand limit does not apply
[rules, §2.4] — and on round 1, taking cards is the *only* legal action anyway,
so a wonder costs you nothing you could otherwise have used. A wonder taken on
round 1 also costs its printed price with **zero** completed-wonder surcharge
[rules, §2.4], and at 4p the row sweeps only 1 card per turn so the cheap Age A
wonders survive longest.

The problem: across a game the 4p champion **starts 1.96 wonders and finishes
0.79**. Wonders it never finishes — Transcontinental Railroad, Ocean Liners,
Kremlin, Pyramids, Colossus are each started in 9–18% of games and completed in
**0%** — get removed from play at the next age change, taking the actions and
resources with them [rules, §12.2]. Take the round-1 wonder idea; do not take the
"start a second one you cannot pay for" idea.

### Round 2 is the highest-leverage turn in the game

You go from 1–4 civil actions and no military actions to a full **4 CA + 2 MA**,
and the board is still symmetric. Three things the 2p champion does on round 2,
in **100% of 120 games** — not a median, the whole distribution sits on round 2:

1. **Add production.** First farm-or-mine build/upgrade lands on round 2 in every
   single game (p10 = p25 = p75 = p90 = 2). Production workers go 4.00 → 4.98
   between rounds 1 and 2. 4p does the same but less rigidly: median round 2,
   99.2% of games, but p75 is round 5. 3p does not — see below. **[strong at
   2p/4p]**
2. **Take a leader, or be about to.** Median round to *take* a leader is 2 at 2p
   and 3p and 3 at 4p, and the 25th percentile is round 1 at all three — a
   quarter of games spend the Age A turn on a leader instead of an action card.
   **[strong]**
3. **Disband the starting Warriors.** This one is real and it is startling: at 2p
   military workers go **1.00 → 0.00** on round 2 and strength goes **1.00 →
   0.06**; at 4p, **1.00 → 0.05** and strength **1.00 → 0.12**. Disbanding a unit
   costs 1 military action and returns the worker to your pool [rules, §4.3] —
   and your 2 military actions are otherwise dead in Age I. The champion converts
   its warrior into a farm worker on turn 2 and stays at essentially zero strength
   for all of Age I (mean military workers in Age I: **0.16 at 2p, 0.03 at 4p**).
   **[provisional — and see the warning below]**

**Warning on #3.** This is mirror self-play with **all pacts removed at 2p**
[rules, §13] against opponents that have never once attacked in 240 games at
those two counts. A champion at 0.06 strength across Age I is defensible only
because nobody in its world has ever punished it. Against a human who will
Plunder you for 1 military action, disbanding your only unit is throwing three
food and three resources at them. Read #3 as *"the starting warrior is worth less
than you think and your early military actions are worth more"*, not as an
instruction.

The 3p champion does the exact opposite — see below.

### Round 3: the first urban building

The first lab/temple/library/theater/arena build lands on **round 3 in 100% of
games at both 2p and 4p** — at 2p the entire distribution p10 through p90 sits on
round 3, at 4p p10 through p75 do. 3p delays it to a median of round 5.
**[strong at 2p/4p]**

At 2p the leader is also in play by round 3 (median 3; 61.7% of games have one
out by the end of round 3, 74.2% by round 4). 4p is a round slower (median 4,
55.8% by end of round 4). Urban workers at 2p go 1.0 → 1.93 → 2.59 across rounds
2–4.

So the 2p opening skeleton is: **R1 action card → R2 production + leader taken →
R3 urban building + leader played → R4-5 second urban building.** Techs go 5.0
(the board) → 5.21 → 5.75 → 6.18 over rounds 3–5; science rate does not leave 1
until round 5 (1.58) and culture rate reaches 2.37 by round 5.

The 4p skeleton is the same shape with a wonder bolted on the front: **R1 wonder
→ R2 production + population + disband → R3 urban building → R4-5 leader.** Its
science rate is well ahead of 2p's early on (round 5: 2.21 vs 1.58) and its
culture rate behind (1.43 vs 2.37).

### 3p opens completely differently, and you should know why

The 3p champion is a **military opening**, and it is the single largest
disagreement in this document:

| Round 2 | 2p champion | 3p champion | 4p champion |
|---|---|---|---|
| Military workers | 0.00 | **1.68** | 0.05 |
| Strength | 0.06 | **1.82** | 0.12 |
| Production workers | 4.98 | 4.00 | 4.51 |
| Urban workers | 1.00 | 1.00 | 1.00 |
| Unused workers | 1.04 | 1.00 | 2.38 |

The 3p champion **never upgrades production in 39% of its games**, and when it
does the median round is 8. It puts its round-2 actions into a second infantry
unit instead. Across the whole game it builds **7.14 infantry** (median round 6)
against 2.41 at both 2p and 4p, and it ends Age III at **strength 7.28** against
3.79 (2p) and 2.99 (4p). Its weight vector agrees: `strength_rel` is its single
most-moved weight (+0.35 → **+1.88**, +436%) and `workers_early` was cut 74%.

Is that right, or is it a local optimum? Honestly: **unclear**, and the fresh 4p
data now argues against it. The 3p champion scores less culture (113.2 mean vs
2p's 123.7) and finishes with fewer techs (9.81 vs 12.88 and 16.35), and it still
never actually attacks (4 aggressions in 120 games). The 4p champion, which faces
*three* opponents rather than two, opens as economically as 2p does and ends with
the most technologies of any of them. So 3p looks like a local optimum rather
than a player-count effect. **[mixed, leaning against]**

What survives: the `strength_deficit` penalty is one of only four levers all
three player counts agree on (−0.6 default → −1.02 / −0.95 / −1.30). Being
*behind* on strength is punished everywhere; being *ahead* is only rewarded at
3p. Read that as "do not be the weakest player", not "build seven infantry".

### How deep into the row to reach, early

The row sweeps **3 cards per turn at 2p, 2 at 3p, 1 at 4p** — six a round at both
2p and 3p, four at 4p. [rules, §1.5] A card in space 7 at 2p has about one round
to live.

The champions handle this very differently:

| | cards taken per game | CA spent taking | share from spaces 1–5 | share from 10–13 |
|---|---|---|---|---|
| 2p | 22.0 | 25.2 | **88.4%** | 3.0% |
| 3p | 12.8 | 29.8 | 23.5% | **56.9%** |
| 4p | **31.9** | 39.1 | 82.7% | 5.0% |

2p and 4p are **volume buyers** — they take almost everything from the cheap end
of the row (22 cards for 25 actions, 32 cards for 39 actions) and barely ever pay
3 CA. Only the 3p champion pays up, taking **half as many cards for more
actions**, mostly from the expensive end. **[mixed]**

Since 2p and 4p — the two counts with the most and least sweeping — agree with
each other and 3p is the outlier, we read the 3p behaviour as a quirk of that
champion rather than a 3-player effect. The default advice is the 2p/4p one:
**be patient, let cards slide left, and buy from spaces 1–5.** Paying 3 civil
actions for a card is something you should have to justify, not a habit.

The count that most rewards patience is **4p**, where only 1 card is swept per
turn (4 per round against 6 at both 2p and 3p) [rules, §1.5] — cards live half
again as long there, which is exactly where the champion takes the most of them.

### Government: later than you think

No champion rushes a government.

| | ever take a govt card | median round taken | ever change govt | median round changed |
|---|---|---|---|---|
| 2p | 72.5% | 7 | 70.0% | 8.5 |
| 3p | 55.8% | 5 | 50.8% | 7 |
| 4p | 91.7% | 7.5 | 85.0% | 9 |

Most-taken first governments: 2p **Theocracy** (25.8% of games, median round 5)
then Republic (16.7%, round 12) and Monarchy (15.0%, round 6); 3p **Monarchy**
(23.3%, round 5.5) then Theocracy (16.7%, round 6); 4p **Monarchy** (35.8%, round
8), Republic (32.5%, round 15) and Democracy (30.8%, round 19). Nearly a third of
2p games and half of 3p games **never leave Despotism at all**; at 4p only 15% do
not. The median change is round **7–9 at every count** — nobody does it in Age I.
**[strong on the timing, mixed on the frequency]**

Despotism's 4 CA / 2 CA-worth-of-limits is not so bad that you should burn a
whole turn's civil actions on a revolution in Age I. Note the rules asymmetry: a
**revolution costs all your civil actions** and burns any actions the new
government grants that turn, while a **peaceful change costs 1 CA plus a higher
science price** and lets you keep playing. [rules, §8] If you are changing
government early, you almost certainly want the peaceful version.

### What "on pace" looks like at the end of Age I

Age I ends around round 6–8. Champion state at that moment:

| At end of Age I | 2p | 3p | 4p |
|---|---|---|---|
| Round | 7 | 6 | 8 |
| Workers | 11.0 | 10.1 | 10.3 |
| Techs (incl. the 5 starting cards) | 7.3 | 6.4 | 7.8 |
| Science rate | 2.5 | 1.3 | 3.0 |
| Culture rate | 3.4 | 1.5 | 2.9 |
| Resource rate | 3.8 | 2.0 | 3.1 |
| Food rate (gross, see trap #2) | 2.2 | 2.1 | **1.2** |
| Culture banked | 12.8 | 3.4 | 6.5 |
| Strength | 1.5 | 2.6 | 0.8 |
| Yellow bank left | 14.0 | 14.9 | 14.7 |
| Wonders completed | 0.06 | 0.00 | 0.23 |

The one row to look at twice is **food rate**. All three champions have 11 or so
workers eating 2 food a turn by this point, and the 4p champion is already
producing only 1.2. That gap is what eventually eats its entire score — see
trap #2.

The number to steal from that table is **yellow bank ~14–15**: all three
champions have taken three or four population by the end of Age I, which keeps
them in the "cost 3, consume 1, 1 happy face required" band and two steps clear
of the nasty jump at 10 tokens. [rules, §6.1] The agreement across counts here is
as tight as anything in this document.

And note the last row. **No champion completes a wonder in Age I at any count**,
including the 4p one that takes a wonder on round 1. Wonders are covered in the
midgame and per-count sections; the opening verdict is that taking one is cheap
and finishing one is not.

---

## Midgame: late Age I through Age II (roughly rounds 6–14)

The fresh 4-player harvest is in for this section, so everything below is three
counts at 120 games each.

### Stop growing around round 9. All three champions do.

This is the cleanest three-count consensus in the whole dataset. Watch the yellow
bank (population tokens left):

| Round | 2p | 3p | 4p |
|---|---|---|---|
| 7 | 14.0 | 13.5 | 14.8 |
| 9 | 12.1 | 12.1 | 12.3 |
| 11 | 12.1 | 12.0 | 12.0 |
| 13 | 11.9 | 11.4 | 11.7 |
| 14 (end of Age II) | 11.5 | — | 11.3 |

All three sprint from 18 down to about 12 in the first eight or nine rounds, and
then **stop dead and park just above 11 for the rest of Age II**. Total workers
go 11.0 → 11.2 at 2p over rounds 8–14; 10.9 → 11.3 at 4p. **[strong]**

That is not laziness, it is the population table. At **12–11 tokens left** a
worker costs 4 food, you consume 2, and you need 2 happy faces. Cross into
**10–9** and the happiness requirement jumps to **3** while consumption does not
move — the cost of that worker is hidden, and it is the step that causes
uprisings. [rules, §6.1] The champions buy the 12–11 band and sit in it.

**Practical rule: get to 12 tokens fast, then stop until you have bought the
third happy face.** Remember you also lose 2 tokens free at the end of each of
Ages I, II and III [rules, §12.2] — the game pushes you across those steps
whether you spend or not.

### The midgame is reallocation, not expansion

If total workers stop growing but your rates keep rising, the workers must be
moving. They are:

| Production → urban workers | round 8 | round 14 (2p/4p) / 13 (3p) |
|---|---|---|
| 2p production | 5.49 | 4.97 |
| 2p urban | 4.55 | 5.17 |
| 3p production | 3.98 | 3.62 |
| 3p urban | 2.98 | 3.87 |
| 4p production | 4.01 | 3.20 |
| 4p urban | 4.28 | 5.43 |

The 4p champion takes this furthest: by round 20 it is down to **2.39 production
workers and 5.41 urban**, with 3.96 workers sitting unused. Every count moves the
same direction. **[strong]**

The mechanism is the `destroy` action: **destroying a farm, mine or urban
building costs 1 civil action, returns the worker to your pool, and refunds
nothing** [rules, §3.6]. The champions use it constantly — **5.9 (2p), 5.5 (3p),
10.9 (4p) destroys per game**. A level-1 farm you built in Age I is not a
building you keep, it is a worker you parked there.

If you take one thing from this section: in the midgame, stop asking "can I
afford another worker" and start asking "which of my existing workers is in the
wrong place".

### Build order inside the urban buildings

Median round of the first build of each type, per game:

| | 2p | 3p | 4p |
|---|---|---|---|
| Temple | **round 5** (3.65/game) | round 8 (2.84) | round 8 (3.71) |
| Lab | round 10 (3.08) | round 11 (1.07) | round 10 (4.71) |
| Library | round 10 (1.93) | round 9 (0.86) | round 12 (2.45) |
| Arena | round 11 (0.78) | round 9 (0.75) | round 13 (0.94) |
| Theater | round 12 (1.14) | round 11 (0.68) | round 15 (1.98) |

**Temples are the first urban building at every player count**, and the
most-worked one at 2p and 3p — 3.65 / 2.84 / 3.71 card-actions per game. The one
exception is 4p, where labs narrowly beat them (4.71 vs 3.71) because the 4p
champion is a technology engine. Theaters and arenas are
consistently the *last* urban buildings anyone puts a worker on, at all three
counts. **[strong]**

The reason is that a temple is a happy face *and* a culture point, so it pays the
happiness bill that the population table is sending you (previous section) while
also scoring. A theater is pure culture with no happiness, which is why it can
wait until you are safe.

### Where the science/culture crossover actually is

The headline rule says "science early, culture late". The fresh data says
something more specific and slightly different. Science rate divided by culture
rate, by age:

| sci/culture | Age I | Age II | Age III | Age IV |
|---|---|---|---|---|
| 2p | 0.79 | 0.78 | 0.92 | 0.87 |
| 3p | 1.67 | 0.63 | 0.60 | 0.58 |
| 4p | 1.53 | 0.90 | 0.94 | 0.86 |

The big move happens **between Age I and Age II at all three counts** — that is
where culture overtakes science. After that the ratio is *flat*, not falling: at
2p and 4p it even ticks back up in Age III. The old claim that it falls
monotonically through the game does not survive the fresh data. **[strong on the
Age I → II crossover, retracted on the monotone claim]**

At 2p the champion's culture rate is above its science rate from **round 3
onward** (round 5: science 1.58, culture 2.37). So at 2p there is barely a
"science first" phase at all.

Practically: **the turn Age II starts is the turn you stop buying labs first.**
That is round 7–9 at all three counts.

### Change government in the midgame, not the endgame

| | ever change govt | median round | most common first govt |
|---|---|---|---|
| 2p | 70.0% | 8.5 | Theocracy (25.8%, round 5) |
| 3p | 50.8% | 7 | Monarchy (23.3%, round 5.5) |
| 4p | **85.0%** | 9 | Monarchy (35.8%, round 8) |

The 4p champion changes government **1.12 times per game** and has Republic
(32.5%), Democracy (30.8%) and Constitutional Monarchy (30.0%) in nearly a third
of games each — it is often changing twice. 2p and 3p change once or not at all.
**[mixed]** — the direction (midgame, not endgame) agrees at all three counts;
the *frequency* does not.

Remember the rules asymmetry: **revolution costs all your civil actions** and
burns whatever the new government grants that turn, while a **peaceful change
costs 1 CA plus a higher science price**. [rules, §8] A revolution on round 9 is
a whole turn of your life.

### Your military actions have three buyers, and you can only pay two

This is the most under-appreciated thing in the data. Military actions are spent
on tactics, on aggressions — and, if unspent at end of turn, on **drawing 1
military card per unused MA, up to 3** [rules, §6.6 step 4]. Those cards are what let
you **prepare an event, which scores culture equal to the card's age level
(I = 1, II = 2, III = 3) as a political action, costing no civil action at all**
[rules, §5.2].

| per game | 2p | 3p | 4p |
|---|---|---|---|
| Copy a tactic (2 MA each) | 5.07 | 2.47 | **14.03** |
| Play a tactic (1 MA) | 1.80 | 0.83 | 1.72 |
| Prepare an event | **11.30** | 9.66 | **1.40** |
| Pass in the Politics Phase | 9.16 | 9.98 | **18.38** |
| Unused MA per turn | 1.93 | 1.82 | 1.22 |
| Final culture | 123.7 | 113.2 | **56.4** |

The 4p champion spends roughly 28 military actions a game copying tactics
[rules: copying costs **2 MA**, §4.4-4.5, one play-or-copy per Action Phase], has
the fewest unused MAs, therefore draws the fewest military cards, therefore has
almost nothing to prepare — and passes in the Politics Phase on **87% of its
turns**. It also scores less than half the culture of the 2p champion. Over 11.3
preparations of mixed ages, the 2p champion is collecting on the order of 20
culture from the Politics Phase alone — a sixth of its final score, for zero
civil actions. **[mixed, and partly inference]**

Two honest caveats. First, final culture is not comparable across player counts
in a mirror — a 4p game divides the same card row four ways. Second, the 4p
champion's weight vector is the youngest and the strangest, so its politics
behaviour may simply be a hole in its evaluation rather than a strategy.

But the *rules* logic stands on its own and you should act on it: **an unused
military action at end of turn is a free card, and a green card with a harp on it
is free culture in a phase that costs you no civil actions.** If you are ending
turns with 2 military actions unspent and no plan for them, that is fine — it is
when you spend them on nothing that you lose the events.

### Wonders: the midgame is when you take one, if you take one

| | ever take a wonder | median round taken | started/game | completed/game |
|---|---|---|---|---|
| 2p | 25.0% | 6.5 | 0.17 | 0.18 |
| 3p | 19.2% | 6 | 0.06 | 0.04 |
| 4p | **100%** | **1** | **1.96** | 0.79 |

This is the largest disagreement in the document and it gets its own treatment in
the per-player-count section. The midgame point is narrow: **at every count that
touches wonders at all, the wonder is taken in Age I or early Age II, never
later.** A wonder costs its row price **+1 CA per wonder you have already
completed** [rules, §2.4], and you cannot take one while another is unfinished
[rules, §9.2] — so a late wonder is both more expensive and more likely to be
antiquated out from under you at the next age change.

Note also 4p's completion rate: **1.96 started, 0.79 completed.** Over a game the
4p champion loses more than one wonder per game to age-end removal. Do not copy
that part. (Health warning on that table: "started" counts a wonder the first
turn a stage is paid for, so a wonder taken, started and finished inside one
turn can register as a completion with no start — which is why 2p shows 0.18
completed against 0.17 started, and why 4p's St. Peter's shows 13 completions
from 8 starts. The gap at 4p, 1.96 vs 0.79, is far too large to be that
artefact.)

**The one wonder number worth memorising.** The 4p champion started wonders 235
times across 120 games, so we can ask which ones actually finish. Split by the
median round the build *starts*:

| 4p, 120 games | wonders started | completed | completion rate |
|---|---|---|---|
| Builds starting round ≤ 12 | 140 | 82 | **59%** |
| Builds starting round ≥ 13 | 95 | 13 | **14%** |

And within that late group, the three 12-resource Age II wonders — **Ocean
Liners, Kremlin and Transcontinental Railroad — went 0 for 58**. Not one was
ever finished, in 120 games, at any point. Meanwhile the cheap Age A/I ones
started early finish reliably: Taj Mahal 14/15, Universitas Carolina 14/14,
Hanging Gardens 12/14, Eiffel Tower 17/22 (started round 12). Wonder cost is
6 resources for the Age A ones, 8–9 for Age I and 12–13 for Age II
[`data/cards_wonders_leaders.json`].

**Practical rule: start a wonder by round 12 or do not start it.** After that
you are paying 12 resources across three or four civil actions for a card that
will be removed, unfinished and unrefunded, at the next age change
[rules, §12.2]. **[strong at 4p, [thin] elsewhere — 2p and 3p barely touch
wonders, so this is one champion's data.]**

### Where your actions start going to waste

Share of civil actions left unspent at end of turn, by age:

| | Age I | Age II | Age III | Age IV |
|---|---|---|---|---|
| 2p | 1% | **41%** | 58% | 64% |
| 3p | 2% | **48%** | 70% | 60% |
| 4p | 0.5% | **6.5%** | 13% | 16% |

Age I is fully spent at every count. Age II is where 2p and 3p fall off a cliff
and 4p does not. The 4p champion — the one that keeps spending — is also the one
that ends with **16.35 technologies against 12.85 and 9.98**, and the only one
that finishes wonders. **[mixed]**

We are not claiming the 4p champion is the strongest of the three; the strength
table at the top says it is the least-improved. But when the counts disagree
about *whether it is fine to waste half your actions in Age II*, the count that
says "no" is the one with three more technologies, and that is the direction we
would bet on. If you are ending Age II turns with 2 civil actions spare, you have
run out of *plan*, not out of *game*.

---

## Endgame: Age III and Age IV (roughly rounds 15–23)

### Age IV is one turn. Plan for that, not for an "Age IV".

Across 360 games the champions took **143 / 155 / 163 Age IV turns in 120 games
each** — that is **1.19 (2p), 1.29 (3p), 1.36 (4p) turns per game**. Age IV is
not a phase of the game. It is a single final turn, occasionally two. **[strong]**

That follows from the rules: when the Age III civil deck runs out, Age IV begins,
and **if that happens during the starting player's turn the current round is the
last; otherwise the next round is** [rules, §12.3]. Everyone gets the same number
of turns.

Age III, by contrast, is long. It starts around round **15 / 14 / 15** and the
game ends around round **22.9 / 22.9 / 22.2**. So:

- A card bought at the **start of Age III** produces about **7 more times**.
- A card bought at **round 20** produces **two or three** times.
- A card bought in **Age IV** produces **once**, and a food or resource card
  bought in Age IV produces nothing you can score.

That is the honest version of "stop buying rate late": the deadline is not the
age boundary, it is **roughly four turns from the end, which is round 19–20**.

### The last two rounds are worth 10–20 culture on their own

Culture at the moment Age IV begins, versus final culture:

| | culture when Age IV starts | final culture | difference |
|---|---|---|---|
| 2p | 104.1 | 123.7 | **+19.6** |
| 3p | 94.9 | 113.2 | **+18.3** |
| 4p | 45.2 | 56.4 | **+11.2** |

Part of that is one more production phase, but a large part is **final scoring:
after the last turn, every Age III event still sitting in the current *or* future
events decks is evaluated** [rules, §12.5]. Age I and Age II leftovers are simply
ignored. Two consequences you can act on:

- **Preparing an Age III event guarantees it will be evaluated**, even if the
  deck never reaches it. [rules, §12.5] If you know an Age III event favours you,
  putting it in the future deck is a guaranteed score, not a gamble.
- Ranked events ("14/7/0") are tie-broken **as if it were the starting player's
  turn** at final scoring, not your turn. [rules, §12.5] If you are relying on a
  tie in a ranked event, check the seat order first.

### Stop banking science

Unspent science points at the end of the game:

| | science banked at end | final technologies |
|---|---|---|
| 2p | **25.7** | 12.85 |
| 3p | 12.9 | 9.98 |
| 4p | **6.2** | **16.35** |

The count that ends with the *least* banked science ends with the *most*
technologies, by three and a half techs. That is not a coincidence — banked
science is a technology you did not develop. And the `science` stock weight is
one of only four levers all three climbs agree on, all downward: **+0.5 default →
+0.185 (2p) / −0.194 (3p) / −6.089 (4p)**. **[strong]**

The 2p champion banking 25.7 science at the end is a genuine flaw in that
champion, not a strategy. Do not copy it.

The same applies to your hand. Age IV hand size: **2.50 (2p), 1.57 (3p), 4.77
(4p)**. `hand_value_late` is negative at all three counts (−0.35 / −0.40 / −0.33
against a −0.2 default) — another full-consensus lever. The 4p champion ending
with nearly five dead cards is the same mistake in a different currency.
**[strong on the principle, and the 4p champion violates it]**

### Workers stop being placed, and that is partly on purpose

Unused workers, from the start of Age III to the last full round:

| | round 15 | round 21 |
|---|---|---|
| 2p | 0.81 | 1.53 |
| 3p | 1.11 | 1.38 |
| 4p | 2.81 | **4.38** |

Meanwhile production workers **fall**: 2p 4.82 → 4.30, 4p **3.16 → 2.09**. The 4p
champion finishes with more than a third of its workers idle.

Two things are going on and only one of them is good:

- **Good:** unused workers absorb discontent. An uprising happens when discontent
  workers **exceed your unused workers**, and unused workers do not reduce
  discontent, they only soak it. [rules, §6.3] You lose 2 yellow tokens at the
  end of Age III [rules, §12.2], which pushes your happiness requirement up right
  when you can least afford an uprising. Carrying spare workers into the last
  rounds is cheap insurance.
- **Probably bad:** at 4p the happiness margin in Age IV is already **+4.34**, so
  those four idle workers are not paying for insurance — they look like
  population the champion bought and then could not afford to place. **[mixed]**

### Military in the endgame

| Age IV | my strength | vs. the *average* rival | vs. the *strongest* rival | aggressions/game | wars/game |
|---|---|---|---|---|---|
| 2p | 4.27 | 1.07 | **1.07** | 0.008 | **0** |
| 3p | 7.39 | 1.03 | **0.75** | 0.033 | **0** |
| 4p | 3.48 | 1.06 | **0.60** | 0.108 | **0** |

Those two ratio columns are the whole story, so read them side by side. Against
the *average* rival every champion looks like it is at parity — but that column
is meaningless in mirror self-play, where you are the average rival by
construction (caveat 2 at the top). Against the *strongest* rival, only 2p is at
parity: at 3p the champion is 25% short of the table leader in Age IV and at 4p
it is at 60% of it, having spent about half of every age below *half* the
leader's strength [`military_by_age`, 120 games each].

**Zero wars in 360 games at every player count.** Aggressions are rare everywhere,
and where they happen at all they happen *late*: at 4p the median first
aggression is **round 18.5** (p25 17, p75 20), i.e. in Age III. **[strong on the
behaviour; see the caveat]**

The caveat matters. These are mirror self-play games between civilizations that
have all learned nobody attacks. A table of humans is not that. What survives the
caveat is one weight fact and one target. The weight fact: `strength_deficit`
(the penalty for being *behind*) is one of the four full-consensus levers, and
all three climbs pushed it further down (−0.6 default → −1.02 / −0.95 / −1.30) —
being weakest is punished everywhere, while being ahead is only rewarded at 3p.
The target: **match the strongest player at the table, and do not pay for more
than that.** The champions only actually manage this at 2p; at 3p and 4p they
fall short and have never been punished for it, so take the target from the
weights, not from the play. See headline rule 8.

Two rules to remember for the last turns:

- **No military cards are drawn in Age IV** [rules, §6.6 step 4, §12.4].
  Whatever is in your military hand at the end of Age III is all the defence you
  will ever have. Count it before you let your strength slip. In Age IV the card
  row is also **swept but never refilled** [rules, §12.4] — the row only shrinks.
- **You may not declare a war during the last round** [rules, §5.1], but you may
  play an aggression. If someone at your table is one Plunder away from the lead,
  Age III is the last moment you can build against it.

### Leaders in the endgame

Share of turns with a leader in play:

| | Age II | Age III | Age IV |
|---|---|---|---|
| 2p | 0.83 | 0.60 | 0.43 |
| 3p | 0.60 | 0.22 | 0.20 |
| 4p | 0.82 | **0.83** | **0.81** |

The 4p champion keeps a leader out through the whole endgame; 2p and 3p let
theirs lapse. [rules, §12.2] **a leader in play survives through the age after its
own** — an Age II leader dies when Age III ends — so keeping one out in Age IV
requires having taken an Age III leader.

Worth knowing: **replacing a leader costs 1 CA and gives you 1 spent civil action
back** [rules, §3.7], so a swap is effectively free in actions. If your Age II
leader is about to be antiquated anyway, there is no action cost to putting a new
one over it. **[rules]** — the behaviour is **[mixed]**, since only 4p does it.

### What Age III culture actually looks like

Culture rate by age:

| | Age II | Age III | Age IV |
|---|---|---|---|
| 2p | 4.42 | 4.82 | 5.83 |
| 3p | 2.64 | 3.27 | 3.65 |
| 4p | 4.90 | 6.63 | **8.88** |

Every count is still *increasing* its culture rate right to the end — nobody
coasts. But look at 4p: its science rate in Age IV is **7.68**, still rising, and
it has the highest culture rate too. It is buying both to the last turn. That is
in direct tension with the headline "stop buying rate in Age III" rule, and the
tension is real: the weight evidence for that rule is strong (`science_rate_late`
−3% / −40% / −66%, `resource_rate_late` −25% / −54% / +222%) but the **behaviour**
evidence is weak — no champion actually stops. **[mixed — the weights say stop,
the play says keep going]**

Our reading, and it is a reading rather than a measurement: keep buying things
that score (labs feed technologies, technologies feed culture buildings) and stop
buying things that only feed *other* purchases (farms, mines) once you are inside
four turns of the end. Food and resources are not victory points.

**One large exception, and it is the most valuable sentence in this section:**
that only applies to rate you are buying for *growth*. If you are producing less
food than you consume, you are losing **4 culture per missing food, every single
turn** [rules, §6.6], and a farm that closes a 1-food gap on round 19 pays back
about 4 × 4 = **16 culture** by the end — more than any Age III culture building
will earn you in the same four turns. Before you apply "stop buying rate", do the
subtraction in trap #2: gross food production minus 2 (or 3 if you are down to
8 or fewer yellow tokens). If that number is negative, buy the farm.

---

## What changes with the player count

This is the section to read if you learned the game at one count and are now
sitting down at another. The three champions are not three strengths of the same
player — they play **different games**, and the differences are much larger than
anything else in this document.

### The rules differences are small. The consequences are not.

The rulebook changes only a handful of things [rules, §13 — full table in the
Quick reference]. The two that dominate play are:

| | 2 players | 3 players | 4 players |
|---|---|---|---|
| Cards swept off the left of the row per turn | **3** | 2 | **1** |
| Civil decks I–III trimmed | remove 9 per deck | remove 3 per deck | none |

(Plus: no pacts at 2p, 4/5/6 Age-A events seeded, and first-round civil actions
1,2 / 1,2,3 / 1,2,3,4 by seat.) The row is always 13 spaces with the same
1/2/3-action cost bands, and costs, scoring and the yellow bank are identical.

Two consequences fall straight out of the sweep number. At **2p** a card you
leave behind is gone in a turn or two, and the row is refilled aggressively, so
cheap cards keep arriving. At **4p** cards linger for many turns, so the row
fills up with things nobody wanted and the *good* card in space 1 was taken by
one of your three opponents long before your turn came round.

The champions' final numbers diverge enormously:

| Measured over 120 mirror games each | 2p | 3p | 4p |
|---|---|---|---|
| Final culture | 123.7 | 113.2 | **56.4** |
| Final technologies | 12.88 | 9.81 | **16.35** |
| Cards taken per game | 22.0 | **12.8** | 31.9 |
| CA spent per card taken | 1.15 | **2.33** | 1.22 |
| Wonders completed per game | 0.18 | 0.04 | **0.79** |
| Military strength, end of Age III | 3.79 | **7.28** | 2.99 |
| Civil actions wasted per turn | 1.74 | 1.93 | **0.38** |

Source: `experiments/behaviour_{2,3,4}p.json`. **[strong]** on the shape of the
divergence, because 120 games is enough to make gaps this large real; **[mixed]**
on which count is *right*, because the three climbs are at very different ages
(gen 176 / 132 / 113, with 15 / 10 / 6 accepted mutants).

### 2 players: the row is a conveyor belt, so cheap cards are everywhere

**Three** cards are discarded off the left of the row at the start of every turn
and the row is refilled to 13 immediately [rules, §2.1] — so six cards a round
churn through, and there is only one other player bidding on them. Cards die
fast, but they arrive just as fast. The 2p champion takes **88.4% of its cards from spaces 1–5** (1 CA
each) and averages **1.15 civil actions per card**. It takes 22 cards a game and
pays only 25.2 actions for them. [`cost_bands`]

What that buys, in practice:

- The most balanced economy of the three: resource rate 4.85 and science rate
  4.45 in Age III, and food production of 2.3 a turn all game. Note that "2.3"
  is only comfortable while consumption is 2 — it still burns 21.4 culture a
  game to starvation, almost all of it in Age III–IV once the bank drops past 8
  tokens and the bill becomes 3 (trap #2).
- The **highest final culture (123.7)** on the second-fewest techs.
- The worst action discipline. It leaves 1.74 CA unused per turn and something
  unspent on 42.8% of turns; in Age III it wastes **57.6%** of its civil actions.
  It ends with **25.7 banked science** it never spends.

So the 2p lesson is not "be efficient" — this champion is not efficient. It is
that at 2p the row keeps handing you cheap, good cards, and the binding
constraint is *what you can build and feed*, not what you can reach.
**[strong]** on the card economics (rules + behaviour agree); the waste is a
flaw, not advice — see "Where your actions start going to waste".

### 3 players: expensive cards, a big army, and a local optimum

The 3p champion is the odd one out at almost every measurement, and you should
treat its style with suspicion rather than copying it.

- It reaches **deep** into the row: **56.9% of its cards come from spaces 10–13**
  (3 CA each), only 23.5% from the cheap band. It pays **2.33 CA per card** —
  double the other counts — and so takes only 12.8 cards a game while spending
  *more* actions (29.8) doing it. [`cost_bands`]
- It is the only military build. **7.14 infantry per game** against 2.41 at both
  other counts, 3.12 army units in play in Age III against 0.42 and 0.08, and
  military strength 7.28 at the end of Age III against 3.79 and 2.99.
- It pays for that army with its economy: resource rate **1.52** in Age III
  (2p: 4.85, 4p: 4.18) and science rate **1.96** (2p: 4.45, 4p: 6.26). It ends
  with the fewest technologies of any count, 9.81.
- It delays production upgrades: first farm/mine upgrade at median round 8, and
  in **39% of games it never upgrades production at all**.

The army does not get used — **zero wars in 120 games** and 0.03 aggressions per
game. It is pure deterrence, and it is being paid for with roughly two-thirds of
the economy the other counts run.

Our reading: this is a **local optimum**, not a player-count effect. The decisive
evidence is 4p — it faces *three* opponents rather than two, has strictly more
reason to fear aggression, and instead opens economically, keeps almost no army,
and ends with the most technologies in the study. **[mixed, leaning against the
3p style]** — the 3p champion does beat its own start point (70.3% ± 9.1), so
the style works; there is no evidence it is the best available style.

### 4 players: a wonder on round one, and a starving engine

The 4p champion is the most *interesting* and the most *broken*.

The good half. It is far and away the best at spending actions — 0.38 CA wasted
per turn against 1.74 and 1.93, and it leaves nothing unspent on **89.2%** of
turns. It takes the most cards (31.9), builds the most labs (4.71), mines (5.38)
and temples (3.71), and finishes with **16.35 technologies** and a culture *rate*
of 8.88 in Age IV, the highest of any count in any age. It takes a wonder on
**round 1 in 120/120 games** and completes 0.79 per game — four times the 2p rate
and twenty times the 3p rate. **[strong]** on the action discipline;
**[provisional]** on the round-1 wonder, because it starts 1.96 wonders and only
finishes 0.79.

The broken half. Its final culture is **56.4**, less than half of 2p's, despite
having three more technologies and a much higher culture rate. The reason is not
subtle once you measure it: **it starves.** Its food production measured at the
end of each age is 1.20 / 1.18 / 1.03 / 0.89 (Ages I–IV), against a consumption
of 2 rising to 3 — roughly *half* its bill, and *falling* — and it burns
**56.1 culture per game to the starvation penalty** against roughly 60 actually
banked, going short on food on **46.1% of all turns**
(`analysis/leak_check.py`, 60 games, 240 player-games). Details in trap #2. It
also passes in the Politics Phase on 87% of turns and prepares only 1.4 events a
game against 11.3 at 2p, so the military-card economy is dead too.

What to take from 4p and what to leave: **take** the action discipline, the
urban-heavy worker split (65% urban by Age III), and the round-1 wonder
consideration. **Leave** the food curve: hold production at **consumption + 1**
— that is 3/turn while the yellow bank is at 12–9 and 4/turn once it drops to
8–5 — which is two to three farm-levels more than this champion ever builds.
**[mixed]**

### Per-count opening cheat sheet

| | 2p | 3p | 4p |
|---|---|---|---|
| Median round: first leader played | 3 | 5 | 4 |
| Median round: first production upgrade | **2** | 8 | **2** |
| Games that ever upgrade production | 100% | 61% | 99% |
| Median round: first urban upgrade | 3 | 5 | 3 |
| Median round: government taken | 8.5 (70%) | 7 (51%) | 9 (85%) |
| Median round: first wonder taken | 6.5 (25%) | 6 (19%) | **1 (100%)** |
| Median round: first aggression | 19 (0.8%) | 4 (3.3%) | 18.5 (8.3%) |

Percentages are the share of games in which it happens at all. Where the share
is under ~25%, the median is a median over a handful of games — treat it as
**[thin]**.

### Where the counts actually agree

Four things hold at 2p, 3p and 4p, and those are the ones to trust:

1. **Take a leader early and play it.** 97% / 83% / 98% of games take one; median
   play round 3 / 5 / 4. **[strong]**
2. **Temples are the most-built card at every count** — 3.65 / 2.84 / 3.71 per
   game — and they are built before theaters and arenas everywhere. **[strong]**
3. **Stop growing around round 9** and park the yellow bank just above 11 tokens,
   avoiding the 10-token happiness step. **[strong]**
4. **Nobody fights.** Zero wars in 360 games. Aggressions per game 0.01 / 0.03 /
   0.11. In a game where every champion is a builder, the player who spends on an
   army spends on nothing. **[strong]** as a description of the champions;
   **[mixed]** as advice, because mirror self-play cannot discover that fighting
   is good if no ancestor ever tried it.

---

## Common traps

Six ways this game quietly takes points off you. All six are things the search
priced *more harshly* than the hand-set weights did — which is the AI's way of
saying "you are underestimating this".

### 1. The uprising you did not see coming

An uprising skips your **entire production phase**: no science scored, no
culture scored, no food, no resources. Only the military draw survives.
[rules, §6.6]

The trap is that you can walk into one without taking an action. Two triggers
fire on their own:

- **Increasing population** can empty a yellow-bank subsection and step the
  happy-faces requirement up by one — the nastiest is at **10 tokens left,
  where the requirement jumps from 2 to 3** while consumption does not move, so
  the cost is invisible on the food side. [rules, §6.1]
- **Every age end takes 2 yellow tokens off you automatically** (ends of Ages
  I, II and III). That is a free, unavoidable push toward the next happiness
  step, three times a game. [rules, §12.2]

All three climbs made the uprising penalty worse than the hand-set −12:
**−14.0 (2p), −15.5 (3p), −21.2 (4p)**. It is the largest single term in the
78-weight evaluation at every player count. **[strong]**

Practical drill: before you spend a civil action on population, look at where
the *next* token comes from and whether that empties a subsection. If it does,
buy the happy face first.

### 2. Starving for one food — this is the biggest leak in the game

**This is the single largest culture sink we measured, at every player count,
and it is larger than everything else combined.** If you take one thing from
this document, take this one.

The rule. In the production phase you pay food equal to your consumption; if you
are short, **you pay what you can and lose 4 culture per missing food**.
[rules, §6.6 step d, CoL p.6] There is no cap, it fires every single turn you are
short, and nothing on the board announces it.

How much it actually costs. `analysis/leak_check.py` replays champion mirror
games with the end-of-turn economy wrapped, comparing the culture your rating
says you should score against what you actually banked. The gap is starvation.
Over **60 games per player count** (`experiments/logs/leak_check.log`):

| | culture burned to starvation, per player-game | share of turns short | final culture |
|---|---|---|---|
| 2p | **21.4** | 16.5% | 129.9 |
| 3p | 6.0 | 6.3% | 107.5 |
| 4p | **56.1** | **46.1%** | 60.1 |

At 4 players the champion burns **roughly as much culture to starvation as it
finishes the game with**. Compare that to the trap everyone worries about —
uprisings cost 0.27 / 0.03 / 0.64 culture per player-game, essentially nothing.
You are guarding the wrong door. **[strong]** — three player counts, 60 games
each, one mechanism, and the effect size is not close.

It gets worse as the game goes on, because consumption steps up as the yellow
bank empties while your farms do not automatically keep pace. Culture burned per
turn, by age:

| | Age I | Age II | Age III | Age IV |
|---|---|---|---|---|
| 2p | 0.03 | 0.82 | 1.75 | 2.83 |
| 3p | 0.00 | 0.12 | 0.53 | 1.20 |
| 4p | 0.33 | 2.51 | **4.71** | **6.25** |

The 4p Age III figure is the one to stare at: **4.71 culture burned per turn
against a culture *rate* of 6.63**. It is netting about 1.9 culture a turn out of
an engine that looks, on the rate track, like it is producing 6.6. That single
mechanism explains why 4p finishes on 56 culture while 2p finishes on 124 despite
4p having three more technologies and a higher culture rate in every age.

Why it sneaks up on you. Consumption steps at 16, 12, 8 and 4 tokens left in the
yellow bank; the population *cost* steps at the same squares but to different
numbers (3 / 4 / 5 / 7), and the *happiness* requirement steps at yet another set
of squares (16, 12, 10, 8, 6, 4, 2, 0). [rules, §6.1] Three different staircases
on one strip of board. The one that costs you culture is the quietest of the
three, because unlike an uprising nothing stops — you just score less, forever.

What to do about it, concretely:

- **Compare production against consumption, not against zero.** The behaviour
  figures below are food **produced** per turn, gross. Consumption is 2 while
  you have 12–9 yellow tokens left and **3 once you are down to 8–5**
  [rules, §6.1] — and every champion is at 9.2–9.8 tokens at the end of Age III
  and 7.2–7.8 by Age IV, so **consumption steps from 2 to 3 during the last age
  at every player count.**

  | food produced per turn | Age I | Age II | Age III | Age IV |
  |---|---|---|---|---|
  | 2p | 2.13 | 2.34 | 2.33 | 2.18 |
  | 3p | 2.05 | 2.22 | 2.28 | 2.39 |
  | 4p | 1.60 | 1.12 | 1.05 | 1.04 |

  Line those up against consumption and the whole table falls out. 2p produces
  ~2.3 against a consumption of 2 — fine — and then the bank crosses 8, the bill
  becomes 3, and it starts burning 2.83 culture a turn. 4p produces **1.0
  against a consumption of 2 for the entire midgame**: about one food short every
  turn, times 4 culture, which is exactly the 2.5–4.7 per turn measured. This is
  not bad luck; it is arithmetic that was visible ten rounds earlier.

  Practical target: **produce consumption + 1**, and add a farm the moment you
  can see the bank crossing 8.
- **Before you take a population, check whether it steps consumption.** Adding a
  worker when the bank is about to cross 16, 12, 8 or 4 raises your bill
  permanently.
- **Every age end takes 2 yellow tokens off you for free** [rules, §12.2] —
  three times a game, unavoidable, and each one can step consumption. Budget food
  for it before the age turns, not after.
- **A farm bought in Age III still pays.** This is the honest exception to
  "stop buying rate in Age III" (trap #4): a farm does not score, but starving
  costs 4 culture a turn, so a farm that closes a 1-food gap on round 17 is worth
  about 24 culture by the end. Rate that *prevents a penalty* is not the same as
  rate that feeds a future purchase.

Weight evidence, for what it is worth: the search raised `food_rate` at all three
counts (+5% / +48% / +12%), and the 4p climb — the one that starves worst — is
the only one that flipped `food_rate_late` positive (−0.6 → **+0.17**), i.e. it
has half-noticed that late food is worth buying. That is a real signal and it
points the right way,
but it is far weaker than the behaviour warrants — the champions have not
learned this lesson yet, which is exactly why they are all still bleeding.
**[strong on the leak, thin on the fix]** — we can measure the cost precisely;
we are inferring the remedy from the rules, not from a champion that solved it.

### 3. Corruption from a half-built wonder

Corruption is charged **before** your mines produce, and a shortfall is taken
out of your food. [rules, §6.6] Blue tokens sitting on the stages of an
unfinished wonder are *out of your blue bank*, so a wonder you started and did
not finish is charging you 2 or 4 resources a turn for the privilege.
[rules, §6.2, §9.2]

The 3p search tripled the corruption penalty (−0.9 → **−2.55**, −183%) and both
3p and 4p raised the value of *free* blue tokens (+89% / +134%) — buy the
corruption headroom **before** you need it, not when the bill arrives.
**[mixed]** — 2p left both weights untouched, so this is a 3p/4p finding.

And if the age turns while your wonder is unfinished and now antiquated, the
wonder is removed from play entirely. You get the blue tokens back; you do not
get the actions or the resources back. [rules, §12.2]

### 4. Buying rate in Age III

A lab bought on round 19 of a 23-round game scores four times. A farm bought
then scores nothing at all, because food is not victory points. The search
found this independently: `science_rate_late` fell at all three counts
(−3% / −40% / −66%), `resource_rate_late` fell at 2p and 3p (−25% / −54%),
and `hand_value_late` fell at **all three** (−59% / −78% / −50%) — one of only
four full-consensus levers in the whole table. **[strong]**

The exception is 4p, where `resource_rate_late` went the other way (+222%,
sign flip). Given that the 4p climb has accepted only 6 mutants, treat that as
**[provisional]** and follow the 2p/3p reading — except for food, where trap #2
overrides this whole trap.

### 5. Hoarding science points

Unspent science points score nothing. Ever. The `science` *stock* weight is one
of the four levers all three counts agree on, and all three cut it:
**+0.185 (2p, −63%), −0.194 (3p, sign flip), −6.089 (4p, sign flip)**.
Banked science is not a war chest, it is a civil action you failed to take.
**[strong]**

The same goes for cards: `hand_value_late` is negative at all three counts.
Hold cards in Ages A and I when you cannot yet afford them; from Age II
onwards, a card in hand on the last turn is worth exactly zero.

Note the contrast with **resources**, which the 3p climb valued *up* (+210%).
Stockpiled resources are spendable on the last turn; stockpiled science mostly
is not, because the thing you would buy with it has to then produce.

### 6. Taking a wonder late, or taking your fourth one

A wonder costs its printed row price **+1 civil action per wonder you have
already completed**. [rules, §2.4] Your fourth wonder from space 6 costs
2 + 3 = 5 civil actions to *take*, before you have paid a single resource for a
stage. There is no way to abandon an unfinished wonder voluntarily, and you
cannot take another while one is unfinished. [rules, §9.2]

The engine's own behaviour splits sharply on wonders by player count — see the
per-player-count section. This is the single biggest strategic disagreement
between the three champions.

---

## Quick reference

Everything in this section is **[rules]** — straight from `docs/RULES_SPEC.md`,
which was built from the Code of Laws, the Handbook and FAQ v15. None of it is
learned, none of it is opinion. Base game 2015, no expansion.

### Card row: what a card costs in actions

| Space (1 = leftmost) | Civil actions |
|---|---|
| 1–5 | 1 |
| 6–9 | 2 |
| 10–13 | 3 |

A **wonder** costs the printed row cost **+1 CA per wonder you have already
completed** (destroyed wonders still count), and goes straight into play
sideways — it never enters your hand, so the hand limit does not apply. You may
not take a wonder while another is unfinished. [RULES_SPEC §2.3–2.4]

Other take limits: you may not take a card if your civil cards in hand ≥ your
civil action *total*; you may not take a technology whose name you already have
in hand or in play; you may never take a second leader of the same age, even if
the first one has left play. [§2.5]

### Sweep: how long a card survives

At the start of every turn from round 2 on, the leftmost N cards are discarded
and gone for good, then the row slides left and refills from the right.

| Players | Cards swept per turn | Cards swept per full round |
|---|---|---|
| 2 | 3 | 6 |
| 3 | 2 | 6 |
| 4 | 1 | 4 |

Six cards a round at 2p and 3p, four at 4p. A card sitting in space 7 at 2p has
about one round to live. [§1.5, §2.1]

### Population: cost, consumption, happiness

Read the row by **how many yellow tokens are still in your bank**.

| Yellow tokens left | Food to add a worker | Food consumed each turn | Happy faces required |
|---|---|---|---|
| 18–17 | 2 | 0 | 0 |
| 16–15 | 3 | 1 | 1 |
| 14–13 | 3 | 1 | 1 |
| 12–11 | 4 | 2 | 2 |
| 10–9 | 4 | 2 | 3 |
| 8–7 | 5 | 3 | 4 |
| 6–5 | 5 | 3 | 5 |
| 4–3 | 7 | 4 | 6 |
| 2–1 | 7 | 4 | 7 |
| 0 | can't | 6 | 8 |

The three numbers move on *different* squares, which is why they look
misaligned: cost is the white number under the rightmost occupied section,
consumption is the leftmost uncovered negative number, and the happiness
requirement steps only when a whole subsection empties. Note the two nasty
steps: **at 10 tokens left the happiness requirement jumps to 3** while
consumption does not move, and **at 4 tokens left the pop cost jumps 5 → 7**.
[§6.1, §6.3]

You also **lose 2 yellow tokens from the bank at the end of Age I, II and
III** — not at the end of Age A. That is 6 free-consumption-and-happiness
penalties you get whether you like it or not. [§12.2]

### Corruption

Read by **how many blue tokens are still in your blue bank** (16 total).

| Blue tokens in bank | Resources paid each turn |
|---|---|
| 16–11 | 0 |
| 10–6 | 2 |
| 5–1 | 4 |
| 0 | 6 |

Blue tokens sitting on your unfinished wonder are *out of the bank*, so a
half-built wonder actively costs you corruption. Corruption is paid **before**
production in the 2015 sequence, and a shortfall is taken out of your food.
[§6.2, §6.6]

### End-of-turn sequence (2015 order — memorise this one)

1. Discard military cards down to your military action total.
2. **Uprising check**: if discontent workers > unused workers, **skip step 3
   entirely**.
3. Production: (a) score science and culture, (b) pay corruption in resources,
   shortfall in food, (c) produce food, (d) pay consumption — **4 culture lost
   per missing food**, (e) produce resources.
4. Draw 1 military card per unused military action, max 3. None in Age IV.
5. Reset all actions.

Note what this ordering means: an uprising costs you your culture and science
score *for the turn* as well as your production, but it does **not** stop your
military draw. And corruption is charged before your mines produce, so a bad
blue bank hits you a full turn earlier than it feels like it should. [§6.6]

### Happiness and uprisings

Happiness rating = happy faces from cards and workers, minus unhappy faces,
clamped 0–8. **Discontent workers = happy faces required − your happiness
rating** (min 0). An uprising happens when discontent workers exceed your
**unused** workers. Unused workers do not reduce discontent — they only
absorb it. [§6.3]

### Actions

- Civil and military actions are spent in any order and any mix during the
  Action Phase, and **do not carry over** to the next turn.
- Your **civil hand limit is your civil action total** (checked only when
  taking a card). Your **military hand limit is your military action total**
  (checked only at end of turn).
- A **revolution** costs *all* your civil actions plus the lower science cost,
  and any civil actions the new government grants are burned immediately. A
  **peaceful change** costs 1 CA plus the higher science cost and lets you keep
  playing. [§3, §8]
- At most **one** play-or-copy tactic action per Action Phase; at most **one**
  political action per turn. [§4, §5.1]

### Ages and the end of the game

- Age A ends at the **first card-row replenish** (i.e. immediately, on the
  starting player's second turn).
- Ages I, II, III end the moment the **last card of the current civil deck is
  dealt into the row** — mid-replenish, on anybody's turn.
- When an age ends, cards **older** than the age that just ended are
  antiquated: discarded from hands, leaders removed from play, unfinished
  wonders removed, pacts removed. Technologies, completed wonders, colonies,
  exclusive tactics and declared wars all survive. Everyone loses 2 yellow
  tokens.
- **A leader in play survives through the age after its own.** An Age I leader
  dies when Age II ends.
- When the Age III civil deck runs out, Age IV begins. **If that happens during
  the starting player's turn, this round is the last; otherwise the next round
  is.** Everyone gets the same number of turns. [§12.1–12.3]

### Final scoring

After the last turn, **every Age III event still sitting in the current or
future events decks is evaluated**. Age I and Age II events left over are
simply ignored. Ranked events ("14/7/0") are tie-broken as if it were the
starting player's turn. Most culture wins; ties share the win. [§12.5]

### 2p / 3p / 4p rules differences

| Rule | 2p | 3p | 4p |
|---|---|---|---|
| Civil decks I–III trimmed | remove 9 per deck | remove 3 per deck | none |
| Military decks | **all pacts removed** | full | full |
| Sweep per turn | 3 | 2 | 1 |
| Age A current events | 4 | 5 | 6 |
| First-round civil actions | 1, 2 | 1, 2, 3 | 1, 2, 3, 4 |
| Pacts playable | **no** | yes | yes |
| "Two strongest/weakest" | read as "the stronger/weaker" | normal | normal |

Everything else is identical. [§13]
