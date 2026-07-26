# Heuristics for human players

**Through the Ages: A New Story of Civilization — base game, 2015 edition, no expansion.**

Written for someone sitting at a table with the physical game. Everything here
comes from a rules-complete engine plus a self-play AI that is still training.

**If you are about to play and have five minutes**, read *If you remember
nothing else* (eight rules), then trap #2 (starvation — the biggest single
culture leak we measured), then the opening cheat sheet for your player count.

1. [How to read this document](#how-to-read-this-document) — where the numbers
   come from, how strong the AI is, what the confidence tags mean
2. [If you remember nothing else](#if-you-remember-nothing-else) — eight rules
3. [Opening: Age A and the first four rounds](#opening-age-a-and-the-first-four-rounds)
   — including [**the build order, turn by turn**](#the-build-order-turn-by-turn)
   and [mine or farm?](#mine-or-farm)
4. [Midgame: late Age I through Age II](#midgame-late-age-i-through-age-ii-roughly-rounds-614)
5. [Endgame: Age III and Age IV](#endgame-age-iii-and-age-iv-roughly-rounds-1523)
6. [Four questions a reader asked](#four-questions-a-reader-asked) — wasted
   actions, round numbering, mine-or-farm, and when the first temple goes up
7. [Priority lists: which card do I take?](#priority-lists-which-card-do-i-take)
   — leaders, wonders, civil buildings and technologies, ranked per age
8. [What changes with the player count](#what-changes-with-the-player-count)
9. [Common traps](#common-traps) — six ways the game quietly takes points off you
10. [Quick reference](#quick-reference) — rulebook tables only, nothing learned
11. [What this document does not know](#what-this-document-does-not-know)

---

## How to read this document

**How rounds are numbered here.** A round is one full circuit of the table:
every player takes one turn. **The first round of the game is round 1.** There
is no round 0, and the Age A turn — where you can only take cards — *is* round
1. So "play a leader on round 3" means the third time you sit down, i.e. your
second Age I turn. (For the pedantic: the engine's counter starts at 1 and is
set to 1 at setup — `engine/state.py:110`, `engine/game.py:75`.) Whenever this
document says "median round 5", it means half the games did it on or before the
fifth time that player took a turn.

**Where the numbers come from.** Two different kinds of evidence, and it is
worth knowing which is which:

1. **What the AI actually did.** We ran the AI against copies of itself and
   watched: **120 games at each of 2, 3 and 4 players**, ~2,600 AI turns each,
   no engine errors. Every "median round 5", "3.65 temples per game", "88% of
   its cards from the cheap end" number below is counted off those games.
2. **What the AI taught itself to want.** The AI decides its move by putting a
   price on about 78 things it can see on the board — a point of science, a
   happy face, a worker, being behind on strength — and picking the move that
   leaves it holding the most value. Those prices started as our hand guesses
   and were then tuned by playing games and keeping the changes that won more.
   When this document says *"the AI taught itself to fear starvation twice as
   much as we told it to"*, that is what it means: the tuning moved a price,
   and it moved it in a direction that won games.

When both kinds of evidence point the same way, the advice is solid. When they
disagree, this document says so instead of picking a winner.

**How strong is the thing giving you advice?** Periodically the AI is re-played
against three fixed opponents, 96 games each: the hand-guessed prices it started
from, a bot that just grabs the best-looking card, and a bot that moves at
random. Win rate against its own starting point, averaged over the last four
such checks:

| | vs. its own starting point | a coin flip would be | vs. the grab-the-best-card bot |
|---|---|---|---|
| 2 players | **78%** — individual checks 71.9 / 74.5 / 82.3 / 82.3 | 50% | 89.6–95.8% |
| 3 players | **65%** — 59.9 / 60.4 / 68.2 / 70.3 | 33.3% | 74.0–80.2% |
| 4 players | **72%** — 66.1 / 71.9 / 72.9 / 76.0 | 25% | 90.6–99.0% |

All three are clearly better than where they began — but note the spread. Each
96-game check is worth ±8–10 points either way, so the bouncing above is noise,
not the AI getting better and worse from week to week. **Do not read a 5-point
difference between two of these rows as meaning anything.** A separate check run
earlier the same morning with different random seeds scored the same match-ups
much lower (2p 44.8%, 3p 60.4%, 4p 34.9%). The honest summary is "clearly above
its starting point at 2 and 3 players, probably at 4, and nobody should quote a
precise number".

Source files, if you want to check the work: `experiments/behaviour_{2,3,4}p.json`
(the 120-game observations), `experiments/logs/leak_check.log` (60 instrumented
games per count measuring culture lost to starvation and uprisings — the source
for trap #2), `experiments/analyze_weights.py` (which prices the tuning moved
and how far), `docs/RULES_SPEC.md` (the rules; every table in *Quick reference*
is straight from the rulebook, nothing learned) and `docs/PACTS_DIAGNOSIS.md`
(why the AI never offers a pact, declares war or colonises — see caveat 3).

**Confidence tags.** Each claim is tagged:

- **[rules]** — a fact from the rulebook. Not an opinion.
- **[strong]** — what the AI did and what it taught itself to want agree, and it
  holds at more than one player count.
- **[mixed]** — the player counts disagree, or two sources point different ways.
  Read the caveat before acting on it.
- **[provisional]** — one player count, small sample, or something that looks
  like a quirk of how the AI thinks. Interesting, not proven.
- **[thin]** — a median or a rate computed over a handful of games. Directional
  at best.
- **[not evidence]** — the AI's behaviour here is *forced* by a limitation of the
  AI, not learned from playing. It is a fact about the software, not about the
  game, and must not be read as advice in either direction. See caveat 3 below
  and `docs/PACTS_DIAGNOSIS.md`.

**Three honest caveats you should carry through the whole document.**

1. *The three AIs have had very different amounts of practice.* Tuning only
   accepts a change if it wins more games, and the counts have banked very
   different numbers of accepted changes: **15 at 2 players, 10 at 3, only 6 at
   4**. So when the counts disagree, the 4-player number is the one most likely
   to be undercooked rather than right — and its price list has some wild
   entries (it prices a banked science point at **−6.09** where we had guessed
   +0.5) that look like one lucky change nobody has trimmed back. Treat extreme
   4-player figures as **[provisional]** unless the 120 games back them up.
2. *Every game here is the AI against copies of itself.* So any "compared to my
   opponents" figure is close to 1.0 by definition and tells you nothing.
   Absolute figures (my strength, my science rate, my worker split) are the
   useful ones.
3. *The AI only looks one move ahead, and that puts a hole in the middle of the
   game.* It never plans a two-turn combo, so anything in this document about
   *sequencing* is read off when things happened, not off the AI reasoning about
   them. Worse: it judges a move by making it and then looking at **its own
   board, immediately, before anyone else responds**. So any move whose payoff
   arrives inside *somebody else's* decision is invisible to it. That is exactly
   the shape of **offering a pact, declaring a war, playing an aggression, and
   bidding on a colony while rivals are still bidding**: you spend the card or
   the worker now, and the result only exists after another player answers. The
   AI sees the cost and none of the gain, so all of these score **worse than
   simply passing, by a fixed amount, in every position it will ever face**. It
   cannot pick them, at any price. Measured: it was legal to offer a pact in
   **16% of political decisions across 240 games, and it was chosen zero times**
   (`docs/PACTS_DIAGNOSIS.md`). Knock-on effect: because no game outcome ever
   depended on the prices for pacts, colonies, aggressions and war, those prices
   were never tuned at all — the 3-player colony price is still, bit for bit, our
   original hand guess, and the 4-player one has wandered to −0.96 at random.
   Everywhere below where this document says the AI never does these things,
   that is a statement about **the software**, not about *Through the Ages*.
   **[not evidence]** — and see rule 8, which is entirely about this.

---

## If you remember nothing else

Eight rules. In rough order of how much they are worth.

1. **Spend all your civil actions in Ages A–II.** Actions do not carry over
   [rules] — an unspent action is simply destroyed at end of turn. Share of
   *available* civil actions the AIs threw away, by age:

   | actions wasted | Age I | Age II | Age III | Age IV |
   |---|---|---|---|---|
   | 2p | 1% | 41% | 58% | 64% |
   | 3p | 2% | 48% | 70% | 60% |
   | 4p | 0.5% | 7% | 13% | 16% |

   In Age I **nobody wastes anything** — if you are leaving actions on the table
   in Age I you are already badly behind. Waste is an endgame phenomenon: by Age
   III there is often nothing left worth buying. The 4-player AI is the outlier
   that keeps spending all game (0.38 wasted per turn against 1.74 at 2p and
   1.93 at 3p) and it finishes with by far the most technologies, **16.4 against
   12.9 and 9.8**. Be careful, though: this is the one headline rule the AI
   itself only half believes. Only the 3-player AI ever learned to dislike
   leftover actions; the other two are still mildly happy to bank them. **A
   reader asked whether the AI is simply wrong to waste actions — it is a fair
   question and it has its own section: [Is wasting a civil action ever
   right?](#is-wasting-a-civil-action-ever-right)**
   **[rules] for the carry-over fact; [mixed] for how much the waste
   costs — the 2-player AI wastes 58% of its Age III actions and still scores
   the most culture of any count.**

2. **Take a leader early and put it in play by round 3–4.** Half of all games
   *take* a leader by round 2 (2p and 3p) or round 3 (4p), and *play* one by
   round 3 (2p), 5 (3p) or 4 (4p) — see the [round numbering
   note](#how-to-read-this-document): round 1 is the Age A turn, there is no
   round 0. Across 120 games per count the AI plays a leader at all in 96.7% /
   82.5% / 98.3% of games, and has one on the table for 70% / 42% / 54% of its
   Age I turns. Both of the AIs with the most practice roughly doubled the value
   they put on having a leader out; the least-practised one (4 players) nudged it
   down, which is more likely noise than a finding. Practical version: **take the
   leader in round 2–3, before the good ones are gone.** **[strong]**

3. **Upgrade your production on round 2.** At 2p the AI's first farm/mine
   upgrade lands on round 2 in **100% of games** (median and both quartiles are
   round 2). At 4p the median is also round 2 (99.2% of games do it eventually,
   mean round 3.5, upper quartile round 5). The first *urban* building upgrade
   follows on round 3 at 2p and 4p (both quartiles round 3), round 5 at 3p.
   3p is the exception on production and delays it badly — median round 8, and
   in 39% of games it never upgrades production at all. See the per-count
   section for why that is probably a flaw, not a plan. **[strong at 2p/4p]**

4. **Build about three temples, and never let an uprising happen.** "Temples" is
   really three separate milestones and this document used to blur them; they get
   pulled apart properly in [When exactly do you build the first
   temple?](#when-exactly-do-you-build-the-first-temple). The short version:
   **you already have the technology** — Religion is one of the five cards
   printed on your player board [rules] — so there is nothing to research, and
   the first temple is a *build*, not a research. Across 120 games per count,
   temples soak up **3.65 / 2.84 / 3.71** civil actions per game once you add up
   building and upgrading. They are the most-worked urban building at 2 and 3
   players; at 4 players labs just edge them out (4.71). An uprising cancels your entire
   production phase [rules], and it is the single most feared thing on the AI's
   whole list of 78 board features — we hand-guessed it at −12 and all three AIs
   independently made it *worse*, ending at −14, −15 and −21. The reason you
   never see the AI suffer one is that it buys the happiness in advance: across
   60 instrumented games per count, uprisings cost it only **0.27 / 0.03 / 0.64
   culture per game**. That is the number you get *after* buying the temples,
   not instead of them. **[strong]**

5. **Science first, culture later — and the switch happens once, at the Age I /
   Age II boundary.** Science rate divided by culture rate, by age:

   | science ÷ culture | Age I | Age II | Age III | Age IV |
   |---|---|---|---|---|
   | 2p | 0.79 | 0.78 | 0.92 | 0.87 |
   | 3p | **1.67** | 0.63 | 0.60 | 0.58 |
   | 4p | **1.53** | 0.90 | 0.94 | 0.86 |

   It is *not* a smooth decline — it is one step down at the I → II boundary and
   then flat (2p and 4p even drift back up in Age III). So the practical version
   is: **out-science the table in Age I, then stop shifting and let the culture
   engine you built run.** All three AIs independently taught themselves to care
   *less* about science rate late in the game, and the two that moved on the
   question taught themselves to care considerably more about culture rate early.
   Note 2p's Age I ratio is already below 1 —
   at two players the AI is on culture from the start. **[strong for the
   direction; [mixed] on the exact crossover round — 3p and 4p cross inside Age
   II, 2p never has a science-heavy phase at all.]**

6. **Do not hoard. Not science points, not cards.** This is one of only four
   things all three AIs independently agree on. We told them a banked science
   point was mildly good; **two of the three now treat a science pile as actively
   bad**, and the third barely values it. Same with cards: all three decided that
   a card sitting in your hand late is worse than we thought, while a card in
   hand *early* went up in value. So: hold cards in Ages A–I, cash them out from
   Age II on. What they actually did agrees — science left unspent at the end of
   the game is 25.7 / 12.9 / 6.2, and the one that hoards least finishes with the
   most technologies (16.4 at 4 players against 12.9 at 2). **[strong]**

7. **Stop buying *science* rate in Age III. Be more careful about food.** All
   three AIs taught themselves to want late science rate less: a lab bought in
   Age III does not pay for itself before the game ends, so buy culture instead.
   Late *resource* rate is the same story at 2 and 3 players — but the 4-player
   AI went the other way and decided late resource rate was *good*, and the
   4-player AI is precisely the one starving to death (trap #2). A farm bought in
   Age III that closes a food gap is not "rate", it is a penalty you stop paying,
   and it is worth roughly 24 culture over the rest of the game. Buy that farm.
   **[mixed — the science half is agreed by all three counts; the resource half
   depends on whether you are short of food.]** Note also that none of the AIs
   actually plays this way: they keep buying rate in Age III at every count. This
   rule is what they *learned to value*, not what they *did*.

8. **Military: the AI never fights — and that is a limitation of the AI, not a
   fact about the game.** **Zero wars in 360 games** at all three counts, and
   aggressions are near-zero (0.01 / 0.03 / 0.11 per game). Do **not** read that
   as "fighting is weak". An aggression or a war pushes a defence choice onto
   your victim and only pays off once they answer it — and the AI only ever looks
   at its own board *before* anyone answers, so it sees the cost and none of the
   gain. Attacking therefore scores below simply passing in every position it
   will ever face: these AIs could not attack even if attacking were the
   strongest move on the board (caveat 3 above; `docs/PACTS_DIAGNOSIS.md`).
   Because no game ever hinged on it, the AI never learned anything about
   fighting either. **[not evidence]** for "nobody fights". What the numbers
   below *do* describe is a table of pure builders with the threat side of the
   game switched off. The AI's strength relative to the *strongest* rival, by
   age:

   | ratio to strongest rival | Age I | Age II | Age III | Age IV |
   |---|---|---|---|---|
   | 2p | 1.04 | 1.05 | 1.02 | 1.07 |
   | 3p | 0.82 | 0.84 | 0.78 | 0.75 |
   | 4p | **0.46** | **0.52** | **0.59** | **0.60** |

   Parity holds **at 2 players only** — where there is exactly one rival, so
   "the strongest rival" and "the average rival" are the same thing. At 3p the
   AI runs about 20% behind the table leader, and at 4p it runs at *half*
   the leader's strength and spends **48–52% of its turns below half the
   strongest rival's strength** [`military_by_age`, 120 games each].

   In absolute terms, so you know what these ratios are ratios *of*: AI
   strength averaged over **every Age III turn** is **3.1 (2p) / 6.8 (3p) / 2.3
   (4p)**, against a strongest rival of 3.0 / 8.8 / 3.8. (The snapshot taken on
   the single last turn *of* Age III is a little higher — 3.8 / 7.3 / 3.0 — which
   is the number quoted in the opening and per-count sections. Same data, one is
   an average over the age and one is its final turn.) A 3p table is running
   roughly twice the army of a 2p table at the same point in the game.

   **Read this as a known weakness in the AI, not as advice.** Nothing at that
   table can attack, so being weak is never punished, so nothing ever told the AI
   to build an army. A human table will punish it. What the data honestly
   supports is only the narrow claim: *at 2 players, matching your single
   opponent is enough and more is waste.* At 3 and 4 players we do not know what
   the right army size is — only that the AI is below it and could never have
   been made to pay for that. **[mixed — 2 players only; the 3p/4p figures are a
   side-effect of an AI that cannot attack]**

---

## Opening: Age A and the first four rounds

Age A is one round long. It ends the moment the card row is first replenished —
on the starting player's *second* turn — so you get exactly one turn in it, with
**1 / 2 / 3 / 4 civil actions by seat order, zero military actions, and taking
cards from the row as your only legal action**. [rules, §1.9]

Everything else in this section is Age I, rounds 2 through about 5.

### The build order, turn by turn

This is the concrete opening: what the AI takes and what it plays, in order,
round by round. It comes from a fresh run that logs **every move the AI makes in
rounds 1–6**, 60 self-play games each at 2 and 3 players
(`analysis/opening_order.py`; raw output in `analysis/out_opening_2p.txt` and
`out_opening_3p.txt`). Numbers in brackets are **how many times per game** the AI
did that thing on that round, so "1.00" means every single game and "0.50" means
half of them.

Two health warnings before you copy it. First, **the two player counts genuinely
open differently** — this is the largest split in the whole document, and it is
not a rounding difference. Second, the AI is blind to the political and military
half of the game (caveat 3), so nothing below tells you when to attack, and the
"disband your Warriors" line in particular is a move you should think twice about
at a human table.

#### 2 players

> **R1** action card → **R2** second mine + leader + disband Warriors →
> **R3** first temple → **R4** second temple + population →
> **R5** first lab + population → **R6** population + wonder stage

| Round | What it does | How often |
|---|---|---|
| **1** | Take an **action card** — most often `Urban Growth (A)` (27% of games), `Frugality (A)` (20%) or `Rich Land (A)` (17%) | 0.92 |
| | Take a **leader** if you have a second action | 0.52 |
| **2** | **Put a worker on Bronze — a second mine** | **1.00** |
| | **Disband the starting Warriors** (1 military action, worker goes back to your pool) | **1.00** |
| | **Play your leader** | 0.78 |
| | Take another action card | 0.83 |
| **3** | **Put a worker on Religion — your first temple** | **1.00** |
| | Increase population | 0.57 |
| | Prepare an event (political action, costs no civil action) | 0.50 |
| | Take a card | 0.48 |
| **4** | Population | 0.63 |
| | Prepare an event | 0.58 |
| | **Second temple** | 0.55 |
| | Revolution — usually to Theocracy | 0.17 |
| **5** | Population | 0.62 |
| | **Put a worker on Philosophy — your first lab** | 0.28 |
| | Third temple | 0.25 |
| | Prepare an event / play or copy a tactic | 0.55 / 0.75 |
| **6** | Population | 0.60 |
| | Copy a tactic | 0.52 |
| | **First wonder stage** | 0.28 |

The single most common complete round-2 turn, played move for move in 23% of
games, is: **build the mine → play the leader → take a card → disband the
Warriors.**

#### 3 players

> **R1** one card only → **R2** population + second **farm** + disband Warriors →
> **R3** another farm + rebuild Warriors → **R4** farm / first temple / infantry →
> **R5–6** population + tactics + more infantry

| Round | What it does | How often |
|---|---|---|
| **1** | Take **exactly one card** — an action card (0.60) or a leader (0.40). 100% of games take one card and stop | 1.00 |
| **2** | **Increase population** | **1.00** |
| | **Put a worker on Agriculture — a second farm** | **0.97** |
| | **Disband the starting Warriors** | 0.83 |
| | Take an action card | 0.48 |
| **3** | **Another farm worker** | 0.47 |
| | **Build Warriors again** (yes, really — see below) | 0.37 |
| | Take an action card | 0.52 |
| | First temple | 0.17 |
| **4** | Farm | 0.42 |
| | **First temple** | 0.28 |
| | Infantry | 0.27 |
| | Take a government card, usually Monarchy | 0.15 |
| **5** | Population | 0.58 |
| | Copy a tactic | 0.50 |
| | Infantry | 0.25 |
| | Revolution to Monarchy | 0.13 |
| **6** | Population | 0.80 |
| | Copy a tactic | 0.62 |
| | Infantry | 0.28 |

The most common complete round-2 turn at 3 players — **63% of games, move for
move** — is: **build the farm → increase population → take a card → disband the
Warriors.** That is a much more uniform opening than 2 players manages.

The round-3 "disband the Warriors then build Warriors again" looks mad and
partly is: it is the AI reclaiming a worker on round 2 to get its economy going
and then paying to put a unit back once it has the food. Do not copy the churn;
copy the priority, which is **economy first, army from round 3**.

#### 4 players

Fewer games behind this one — **20**, not 60, because the machine was busy
training — so treat the small numbers as directional.

> **R1** wonder + action card + leader (you have up to 4 actions) →
> **R2** disband Warriors + population + second mine →
> **R3** first lab → **R4** first temple + wonder stage

| Round | What it does | How often |
|---|---|---|
| **1** | **Take a wonder** — most often `Pyramids` (30% of games) or `Colossus` (15%) | 0.60 |
| | Take an action card | 0.60 |
| | Take a leader | 0.55 |
| **2** | **Disband the starting Warriors** | **1.00** |
| | **Increase population** | **0.95** |
| | **Put a worker on Bronze — a second mine** | 0.70 |
| | Play your leader | 0.35 |
| **3** | **Put a worker on Philosophy — your first lab** | 0.75 |
| | Take an action card | 0.80 |
| **4** | **Put a worker on Religion — your first temple** | 0.50 |
| | **Raze your own farm** (`Agriculture`) to reclaim the worker | 0.45 |
| | Pay a wonder stage | 0.30 |

The most common complete round-2 turn, in 35% of games: **increase population →
build the mine → take a card → disband the Warriors.**

**Do not copy round 4.** Razing `Agriculture` in nearly half of games, on round
4, is the first visible move in the chain that ends with the 4-player AI
producing about one food a turn against a bill of two or three, and burning
**56 culture a game to starvation** — more than it finishes with. See trap #2.

#### Mine or farm?

The reader asked this directly, and the answer is clean, different at each count,
and one of the most reliable numbers in this document:

| | round-2 production build | how often |
|---|---|---|
| **2 players** | **a mine** — a second worker on Bronze | **100% of 60 games** |
| **3 players** | **a farm** — a second worker on Agriculture | **97% of 60 games** |
| **4 players** | **a mine** — a second worker on Bronze | **100% of 20 games** |

So: **mine at 2 and 4 players, farm at 3.** The reasoning that fits the rest of
the data (this is our reading, not a measurement): the 2- and 4-player AIs disband
their Warriors and go straight for resources, because resources are what buy
buildings. The 3-player AI increases population on round 2 in *every* game, and
population eats food, so it has to farm first.

**But note which of those three is starving.** The 4-player AI takes the mine,
then razes its own farm on round 4 in nearly half of games, then spends the rest
of the game producing about half the food it eats and burning 56 culture a game
to the starvation penalty — more than it finishes with (trap #2). The 3-player
AI, the one that farms, has the lowest starvation loss of the three. If you are
unsure, **take the mine but do not skip the farm** — and do the food subtraction
in trap #2 before round 8, not after.

Over the whole game the emphasis flips back: the 2-player AI ends up doing the
most work on **mines** (4.28 build-or-upgrade actions per game) and the 4-player
AI likewise (5.38), while the 3-player AI does almost none (0.41) because it
spends everything on infantry.

### Round 1: take a card. That is the whole turn.

You cannot build, upgrade, play a leader or increase population on round 1 — the
rules do not allow it. [rules, §1.9] So the only question is *which* card.

The AIs split into two answers, and both are defensible.

**2p and 3p take an action card.** Median round of the first action-card take is
1 at both counts, in 100% of games. The two Age A action cards are the most-taken
round-1 cards: at 2p `Frugality (A)` 0.37 per game and `Urban Growth (A)` 0.33
per game, both with a median round of 1. **[strong at 2p/3p]**

That is arithmetic rather than insight: on round 1 you have nothing to spend
resources on, and an Age A action card is a resource or food rebate you can cash
on round 2 when you suddenly have four actions and nothing banked. The seat-1
player, with a single civil action, gets exactly one shot at it.

**4p takes a wonder.** In **120 games out of 120**, the 4-player AI takes a
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

The problem: across a game the 4-player AI **starts 1.96 wonders and finishes
0.79**. Wonders it never finishes — Transcontinental Railroad, Ocean Liners,
Kremlin, Pyramids, Colossus are each started in 9–18% of games and completed in
**0%** — get removed from play at the next age change, taking the actions and
resources with them [rules, §12.2]. Take the round-1 wonder idea; do not take the
"start a second one you cannot pay for" idea.

### Round 2 is the highest-leverage turn in the game

You go from 1–4 civil actions and no military actions to a full **4 CA + 2 MA**,
and the board is still symmetric. Three things the 2-player AI does on round 2,
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
   and your 2 military actions are otherwise dead in Age I. The AI converts
   its warrior into a farm worker on turn 2 and stays at essentially zero strength
   for all of Age I (mean military workers in Age I: **0.16 at 2p, 0.03 at 4p**).
   **[provisional — and see the warning below]**

**Warning on #3.** This is mirror self-play with **all pacts removed at 2p**
[rules, §13] against opponents that have never once attacked in 240 games at
those two counts — and, more to the point, *cannot*: an aggression is
a move the AI cannot see the point of (caveat 3;
`docs/PACTS_DIAGNOSIS.md`). Sitting at 0.06 strength through Age I is defensible
only because nobody in its world was able to punish it. Against a human who will
Plunder you for 1 military action, disbanding your only unit is throwing three
food and three resources at them. Read #3 as *"the starting warrior is worth less
than you think and your early military actions are worth more"*, not as an
instruction.

The 3-player AI does the exact opposite — see below.

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

The 3-player AI is a **military opening**, and it is the single largest
disagreement in this document:

| Round 2 | 2-player AI | 3-player AI | 4-player AI |
|---|---|---|---|
| Military workers | 0.00 | **1.68** | 0.05 |
| Strength | 0.06 | **1.82** | 0.12 |
| Production workers | 4.98 | 4.00 | 4.51 |
| Urban workers | 1.00 | 1.00 | 1.00 |
| Unused workers | 1.04 | 1.00 | 2.38 |

The 3-player AI **never upgrades production in 39% of its games**, and when it
does the median round is 8. It puts its round-2 actions into a second infantry
unit instead. Across the whole game it builds **7.14 infantry** (median round 6)
against 2.41 at both 2p and 4p, and it ends Age III at **strength 7.28** against
3.79 (2p) and 2.99 (4p). What it learned to want matches what it does: being
*ahead* on strength is the single value it moved furthest from our starting
guess — it now rates it more than five times as highly as we did — and it cut the
value of early workers by three quarters.

Is that right, or has it just got stuck in a rut it cannot climb out of?
Honestly: **unclear**, and the 4-player data argues against it. The 3-player AI scores less culture (113.2 mean vs
2p's 123.7) and finishes with fewer techs (9.81 vs 12.88 and 16.35), and it still
never actually attacks (4 aggressions in 120 games — but no AI *can*
attack, so that figure is not an argument against the army; caveat 3). Note what
the 3p army can and cannot be paying for: in this world strength earns through
military events and colonisation requirements only, never through defence or
threat. The 4-player AI, which faces
*three* opponents rather than two, opens as economically as 2p does and ends with
the most technologies of any of them. So the 3-player style looks like a rut
that particular AI fell into rather than something the 3-player rules demand. **[mixed, leaning against]**

What survives is this: **fear of being the weakest player at the table** is one
of only four things all three AIs independently agree on, and all three roughly
doubled the penalty we had guessed for it. Being *behind* on strength is punished
everywhere; being *ahead* only pays at 3 players. Read that as "do not be the
weakest player", not "build seven infantry".

### How deep into the row to reach, early

The row sweeps **3 cards per turn at 2p, 2 at 3p, 1 at 4p** — six a round at both
2p and 3p, four at 4p. [rules, §1.5] A card in space 7 at 2p has about one round
to live.

The AIs handle this very differently:

| | cards taken per game | CA spent taking | share from spaces 1–5 | share from 10–13 |
|---|---|---|---|---|
| 2p | 22.0 | 25.2 | **88.4%** | 3.0% |
| 3p | 12.8 | 29.8 | 23.5% | **56.9%** |
| 4p | **31.9** | 39.1 | 82.7% | 5.0% |

2p and 4p are **volume buyers** — they take almost everything from the cheap end
of the row (22 cards for 25 actions, 32 cards for 39 actions) and barely ever pay
3 CA. Only the 3-player AI pays up, taking **half as many cards for more
actions**, mostly from the expensive end. **[mixed]**

Since 2p and 4p — the two counts with the most and least sweeping — agree with
each other and 3p is the outlier, we read the 3p behaviour as a quirk of that
AI rather than a 3-player effect. The default advice is the 2p/4p one:
**be patient, let cards slide left, and buy from spaces 1–5.** Paying 3 civil
actions for a card is something you should have to justify, not a habit.

The count that most rewards patience is **4p**, where only 1 card is swept per
turn (4 per round against 6 at both 2p and 3p) [rules, §1.5] — cards live half
again as long there, which is exactly where the AI takes the most of them.

### Government: later than you think

No AI rushes a government.

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

The one row to look at twice is **food rate**. All three AIs have 11 or so
workers eating 2 food a turn by this point, and the 4-player AI is already
producing only 1.2. That gap is what eventually eats its entire score — see
trap #2.

The number to steal from that table is **yellow bank ~14–15**: all three
AIs have taken three or four population by the end of Age I, which keeps
them in the "cost 3, consume 1, 1 happy face required" band and two steps clear
of the nasty jump at 10 tokens. [rules, §6.1] The agreement across counts here is
as tight as anything in this document.

And note the last row. **No AI completes a wonder in Age I at any count**,
including the 4p one that takes a wonder on round 1. Wonders are covered in the
midgame and per-count sections; the opening verdict is that taking one is cheap
and finishing one is not.

---

## Midgame: late Age I through Age II (roughly rounds 6–14)

Everything below is 120 self-play games at each of 2, 3 and 4 players.

### Stop growing around round 9. All three AIs do.

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
uprisings. [rules, §6.1] The AIs buy the 12–11 band and sit in it.

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

The 4-player AI takes this furthest: by round 20 it is down to **2.39 production
workers and 5.41 urban**, with 3.96 workers sitting unused. Every count moves the
same direction. **[strong]**

The mechanism is the `destroy` action: **destroying a farm, mine or urban
building costs 1 civil action, returns the worker to your pool, and refunds
nothing** [rules, §3.6]. The AIs use it constantly — **5.9 (2p), 5.5 (3p),
10.9 (4p) destroys per game**. A level-1 farm you built in Age I is not a
building you keep, it is a worker you parked there.

If you take one thing from this section: in the midgame, stop asking "can I
afford another worker" and start asking "which of my existing workers is in the
wrong place".

### Build order inside the urban buildings

The **middle** round of all the building work you do on each type — half your
temple work happens before this round, half after — with the number of civil
actions each type absorbs over a whole game in brackets. (This is *not* the round
you build your first one; that is much earlier and is covered in the [build
order](#the-build-order-turn-by-turn).)

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
AI is a technology engine. Theaters and arenas are
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

At 2p the AI's culture rate is above its science rate from **round 3
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

The 4-player AI changes government **1.12 times per game** and has Republic
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

The 4-player AI spends roughly 28 military actions a game copying tactics
[rules: copying costs **2 MA**, §4.4-4.5, one play-or-copy per Action Phase], has
the fewest unused MAs, therefore draws the fewest military cards, therefore has
almost nothing to prepare — and passes in the Politics Phase on **87% of its
turns**. It also scores less than half the culture of the 2-player AI. Over 11.3
preparations of mixed ages, the 2-player AI is collecting on the order of 20
culture from the Politics Phase alone — a sixth of its final score, for zero
civil actions. **[mixed, and partly inference]**

**Read the "pass" row as a symptom, not a decision.** The politics phase offers
five things — prepare an event, offer a pact, play an aggression, declare a war,
or pass — and the AIs have only ever done two of them. Preparing an event
is the *only* political move that pays you immediately, on your own board, with
nobody else's answer required; pacts, aggressions and wars all pay off through
another player's response, and the AI cannot see that far (caveat 3;
`docs/PACTS_DIAGNOSIS.md`). So "passes on 87% of its turns" means *"had no event
worth preparing"*, not *"looked at the political options and declined them"* —
the political options were never really on the table. A human sitting in that
seat has three more buyers for a military card than this AI does.
**[not evidence]** for the pass rate as a strategic choice; the
prepare-an-event arithmetic above is [rules] and stands on its own.

Two further honest caveats. First, final culture is not comparable across player
counts in a mirror — a 4p game divides the same card row four ways. Second, the
4-player AI is the least practised and has the strangest set of values, so how
little it bothers to prepare events may be a hole in its judgement rather than a
strategy.

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
4-player AI loses more than one wonder per game to age-end removal. Do not copy
that part. (Health warning on that table: "started" counts a wonder the first
turn a stage is paid for, so a wonder taken, started and finished inside one
turn can register as a completion with no start — which is why 2p shows 0.18
completed against 0.17 started, and why 4p's St. Peter's shows 13 completions
from 8 starts. The gap at 4p, 1.96 vs 0.79, is far too large to be that
artefact.)

**The one wonder number worth memorising.** The 4-player AI started wonders 235
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
wonders, so this is one AI's data.]**

### Where your actions start going to waste

Share of civil actions left unspent at end of turn, by age:

| | Age I | Age II | Age III | Age IV |
|---|---|---|---|---|
| 2p | 1% | **41%** | 58% | 64% |
| 3p | 2% | **48%** | 70% | 60% |
| 4p | 0.5% | **6.5%** | 13% | 16% |

Age I is fully spent at every count. Age II is where 2p and 3p fall off a cliff
and 4p does not. The 4-player AI — the one that keeps spending — is also the one
that ends with **16.35 technologies against 12.88 and 9.81**, and the only one
that finishes wonders. **[mixed]**

We are not claiming the 4-player AI is the strongest of the three; the strength
table at the top says it is the least-improved. But when the counts disagree
about *whether it is fine to waste half your actions in Age II*, the count that
says "no" is the one with three more technologies, and that is the direction we
would bet on. If you are ending Age II turns with 2 civil actions spare, you have
run out of *plan*, not out of *game*.

---

## Endgame: Age III and Age IV (roughly rounds 15–23)

### Age IV is one turn. Plan for that, not for an "Age IV".

Across 360 games the AIs took **143 / 155 / 163 Age IV turns in 120 games
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
| 2p | **25.7** | 12.88 |
| 3p | 12.9 | 9.81 |
| 4p | **6.2** | **16.35** |

The count that ends with the *least* banked science ends with the *most*
technologies, by three and a half techs. That is not a coincidence — banked
science is a technology you did not develop. And "a banked science point is
worth something" is one of only four judgements all three AIs revised in the same
direction — all three revised it **down**, and two of them all the way to
*negative*. **[strong]**

The 2-player AI banking 25.7 science at the end is a genuine flaw in that
AI, not a strategy. Do not copy it.

The same applies to your hand. Age IV hand size: **2.50 (2p), 1.57 (3p), 4.77
(4p)**. `hand_value_late` is negative at all three counts (−0.35 / −0.40 / −0.33
against a −0.2 default) — another full-consensus lever. The 4-player AI ending
with nearly five dead cards is the same mistake in a different currency.
**[strong on the principle, and the 4-player AI violates it]**

### Workers stop being placed, and that is partly on purpose

Unused workers, from the start of Age III to the last full round:

| | round 15 | round 21 |
|---|---|---|
| 2p | 0.81 | 1.53 |
| 3p | 1.11 | 1.38 |
| 4p | 2.81 | **4.38** |

Meanwhile production workers **fall**: 2p 4.82 → 4.30, 4p **3.16 → 2.09**. The 4p
AI finishes with more than a third of its workers idle.

Two things are going on and only one of them is good:

- **Good:** unused workers absorb discontent. An uprising happens when discontent
  workers **exceed your unused workers**, and unused workers do not reduce
  discontent, they only soak it. [rules, §6.3] You lose 2 yellow tokens at the
  end of Age III [rules, §12.2], which pushes your happiness requirement up right
  when you can least afford an uprising. Carrying spare workers into the last
  rounds is cheap insurance.
- **Probably bad:** at 4p the happiness margin in Age IV is already **+4.34**, so
  those four idle workers are not paying for insurance — they look like
  population the AI bought and then could not afford to place. **[mixed]**

### Military in the endgame

| Age IV | my strength | vs. the *average* rival | vs. the *strongest* rival | aggressions/game | wars/game |
|---|---|---|---|---|---|
| 2p | 4.27 | 1.07 | **1.07** | 0.008 | **0** |
| 3p | 7.39 | 1.03 | **0.75** | 0.033 | **0** |
| 4p | 3.48 | 1.06 | **0.60** | 0.108 | **0** |

Those two ratio columns are the whole story, so read them side by side. Against
the *average* rival every AI looks like it is at parity — but that column
is meaningless in mirror self-play, where you are the average rival by
construction (caveat 2 at the top). Against the *strongest* rival, only 2p is at
parity: at 3p the AI is 25% short of the table leader in Age IV and at 4p
it is at 60% of it, having spent about half of every age below *half* the
leader's strength [`military_by_age`, 120 games each].

**Zero wars in 360 games at every player count**, and the two rightmost columns
above are there for completeness, not as findings: declaring a war and playing an
aggression are both invisible to an AI that only looks at its own board before
anyone answers, so those cells were guaranteed to be ~0 before a single game was
played (caveat 3;
`docs/PACTS_DIAGNOSIS.md`). The handful of aggressions that do occur happen
*late* — at 4p the median first aggression is **round 18.5** (p25 17, p75 20),
i.e. in Age III — but that is a median over ten games of a move the AI never
deliberately selects. **[not evidence]** on the fighting columns; the strength
columns themselves are real measurements.

The caveat matters. These are mirror self-play games between civilizations that
*cannot* attack — they did not learn that nobody attacks, it was never an option
they could take. A table of humans is not remotely that. What survives is one
judgement and one target. The judgement: the penalty for being *behind* on
strength is one of the four things all three AIs agree on, and all three made it
about twice as harsh as we had guessed — being weakest is punished everywhere,
while being ahead only pays at 3 players.
The target: **match the strongest player at the table, and do not pay for more
than that.** The AIs only actually manage this at 2p; at 3p and 4p they
fall short and could never have been punished for it, so take the target from the
what they value, not from what they did. See headline rule 8.

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

The 4-player AI keeps a leader out through the whole endgame; 2p and 3p let
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
tension is real: all three AIs *learned* to want less late science rate (and two
of the three less late resource rate), but not one of them actually stops buying.
**[mixed — what they value says stop, what they do says keep going]**

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

## Four questions a reader asked

### Is wasting a civil action ever right?

> *"I'm really surprised they ever waste an action. Isn't taking or playing a
> yellow card almost always worth it?"*

**You are right and the AI is wrong.** This was measured properly after the
question was asked, over 200 self-play games per player count and 8,531 turns
that ended with a civil action unspent (`docs/WASTED_ACTIONS.md`). The results
are not kind to the AI:

| At 2 players, of all turns ending with a civil action thrown away | |
|---|---|
| had **no** legal, affordable thing to spend it on — genuinely stuck | **1.6%** |
| had something legal and affordable, and declined it | **98.4%** |
| declined a move **its own scoring rated as an improvement** | **60.1%** |
| would have made a real move if one specific bug were fixed | **98.0%** |

At 3 players it is nearly as bad (97.1% avoidable, 44.9% declining a move it
scored as positive). Only Age I is genuinely blameless — there the AI's hand is
full 96% of the time, which is a real constraint, and that is exactly the age
where it wastes almost nothing.

**The bug.** When the AI considers the "end my turn" move it applies it and looks
at the resulting board — but ending your turn *runs your whole production phase*,
so the board it is admiring already has a turn's income on it. Every other move
is judged mid-turn, before income. So the AI is comparing "my board after taking a
card" against "my board after collecting everything", and income wins every time.
Measured, that phantom is worth **+12.6 points** on an average wasted-action turn
at 2 players, rising to **+26.3** in Age IV, while the moves it is turning down
are worth a fraction of a point. There is a hand-set correction for this in the
code and it is a fixed number, which cannot possibly cancel a distortion that
grows with your economy.

**Specifically on yellow cards.** Your instinct is right twice over. Not only is
taking a card nearly always worth it, the AI cannot even *see* which card it is:
when judging a take, its scoring reduces your whole hand to two numbers — how
many cards you hold and roughly what age they are. Taking `Ocean Liners` and
taking `Revolutionary Idea` produce literally identical inputs. So it scores
every take at approximately zero and then loses the comparison to that
twelve-point phantom. At 2 players, **31.9% of wasted-action turns had a yellow
card sitting in hand that was legal to play right then**, and it was declined.

**So: ignore rule 1's waste table as a model and follow rule 1's advice.** Spend
your actions. The one honest caveat is that at 4 players the story is different
and worse — there the AI refuses to *play* cards, so its hand fills up, so taking
becomes illegal, and 14.3% of its wasted actions genuinely have nowhere to go.
That is still a defect, just a different one.

### Is the draft round counted as round 0?

> *"Round 3 leader seems later. Is the draft round counted as round 0?"*

**No.** There is no round 0 anywhere in this study. The first time you sit down
is **round 1**, and that is the Age A turn where taking cards is your only legal
action. The engine's counter starts at 1 and is set to 1 at setup
(`engine/state.py:110`, `engine/game.py:75`).

So "median round 3 to play a leader" means: Age A turn, then your first Age I
turn, then on your *second* Age I turn the leader is on the table. It is the
third time you have taken a turn, not the fourth. The convention is now stated at
the top of the document under [How to read this
document](#how-to-read-this-document).

If it still feels late, note two things. First, the leader is *taken* earlier
than it is played — median round 2 at 2 and 3 players — and a quarter of games
take one on round 1, spending the Age A turn on a leader instead of an action
card. Second, at 2 players the leader is out by the end of round 3 in 61.7% of
games and by the end of round 4 in 74.2%, so the median is not hiding a long
tail.

### Do you do mine or farm?

Answered in full in [Mine or farm?](#mine-or-farm) above, with the build order.
The short version: **a mine (a second worker on Bronze) at 2 and 4 players, on
round 2, in 100% of games. A farm (a second worker on Agriculture) at 3 players,
on round 2, in 97%.** The 3-player AI farms because it increases population on
round 2 in every single game and has to feed it.

### When exactly do you build the first temple?

> *"when do you build the first one?"*

The old draft blurred three completely different milestones together. They are
different rounds and they cost different things:

| | **Research** a temple technology | **Build** your first temple | Need a 2nd / 3rd happy face |
|---|---|---|---|
| 2 players | round 9 — and only 50% of games ever bother | **round 3** (100% of games) | ~round 9 / before round 12 |
| 3 players | round 9 (38% of games) | **round 7** (82% of games) | ~round 9 / before round 12 |
| 4 players | round 11 (45% of games) | **round 4** (95% of games) | ~round 9 / before round 12 |

Three things to take from that table.

1. **There is nothing to research.** `Religion` is one of the five technologies
   printed on your player board at setup [rules], so you can build a temple on
   your very first full turn without spending a single science point. The
   "research" column above is about upgrading to a *better* temple — `Theology`
   and later `Organized Religion` — and half the games never do it at all.
2. **The first temple is early: round 3 at 2 players, in every single game.**
   Not round 5, which is what an earlier draft of this document said; that number
   was the *middle* of all the temple work across a whole game, not the first
   one.
3. **The happy-face deadline is set by the population track, not by the temple.**
   You need 1 happy face from 16 yellow tokens left, **2 from 12**, and **3 from
   10** [rules, §6.1]. Every AI sprints from 18 tokens down to about 12 by round
   9 and then stops dead — which means the second happy face is due around round
   9, and the third is due whenever you next cross into the 10-token band. That
   is the moment the game quietly starts trying to cause an uprising, and it is
   why the practical rule is *get to 12 tokens fast, then stop until you have
   bought the third happy face.*

Put together: **build a temple on round 3–4, a second one on round 4–5, and have
your third happy face before you spend the population that takes you past 10
yellow tokens.** The AIs spend 2.8–3.7 civil actions per game on temples in
total, and it buys them near-total immunity from uprisings — 0.03 to 0.64 culture
lost per game, which is nothing.

---

## Priority lists: which card do I take?

### These are per-age lists, not one global list — here is why

The card row only ever contains cards from the current age and the one after it,
and each age's deck is exhausted before the next begins [rules, §12.1]. **You will
never once be choosing between an Age A temple and an Age III temple.** A single
global ranking of individual cards would therefore be a list you could never
actually use. So every list below ranks cards *within* an age.

There is exactly one ranking worth stating globally, and it is between **kinds**
of card rather than between cards. Averaged over 120 games at each player count,
here is where the civil actions actually go — this is the real priority order:

| Rank | Card type | Actions spent per game (2p / 3p / 4p) |
|---|---|---|
| 1 | **Mines** | 4.28 / 0.41 / 5.38 |
| 2 | **Temples** | 3.65 / 2.84 / 3.71 |
| 3 | **Labs** | 3.08 / 1.07 / 4.71 |
| 4 | Infantry | 2.41 / **7.14** / 2.41 |
| 5 | Libraries | 1.93 / 0.86 / 2.45 |
| 6 | Special technologies | 1.93 / 1.24 / 2.36 |
| 7 | Farms | 1.50 / 0.92 / 1.69 |
| 8 | Cavalry | 1.23 / 0.56 / 1.32 |
| 9 | Theaters | 1.14 / 0.68 / 1.98 |
| 10 | Arenas | 0.78 / 0.75 / 0.94 |
| 11 | Governments | 0.12 / 0.07 / 0.56 |

(Infantry at 3 players is the outlier discussed at length in the per-count
section; treat it as that one AI's habit, not as a 3-player rule.)

### How much to trust these lists

Two very different grades of evidence are mixed together here, and you should
know which you are reading.

- **Leaders and wonders: reasonably solid.** Putting a leader in play or paying a
  wonder stage changes your board immediately, so the AI's judgement is actually
  engaged when it chooses, and the counts below are what it chose across 120
  games per player count.
- **Buildings and technologies: weak, and you should mostly ignore the per-card
  order.** When the AI *takes* a card off the row it genuinely cannot tell one
  card from another: its scoring compresses your entire hand down to two numbers,
  how many cards you hold and roughly what age they are. Taking `Ocean Liners`
  and taking `Revolutionary Idea` look *identical* to it
  (`docs/WASTED_ACTIONS.md` §4). So the per-card take counts are close to a
  measure of what happened to be available and cheap. Trust the **type** order in
  the table above; treat the card names below as a weak hint.
- **Anything military or political is systematically underrated below.** The AI
  never attacks, never defends, never signs a pact and never colonises (caveat
  3). Every leader and wonder whose value is deterrence, aggression or politics —
  Alexander, Caesar, Genghis Khan, Napoleon, Robespierre, Colossus, the Great
  Wall, Transcontinental Railroad — is being judged by a player who cannot use
  half the card. **A low number below is not evidence against those cards.**

We do not have a sourced community ranking checked into this repo, so where this
document says "human tables generally rate X higher", that is the author's
recollection of common opinion and is **not** evidence. It is flagged so you can
weigh it yourself.

### Leaders, by age

There are six leaders per age and you can only ever have one of that age in play
[rules]. The percentage is the share of the 120 games in which the AI got that
leader into play, averaged across the three player counts.

**Read the spread, not the order.** Within every age the best and worst leader
here are only about 1.5× apart, and much of that gap is availability rather than
quality. The AI does not have strong leader opinions; take the ordering as a
tiebreak, not as gospel.

**Age A** — all six are within noise of each other (5.5%–8.3%). Effectively:
*take whichever Age A leader you see on round 1.*

| | Leader | Played | What it does for you |
|---|---|---|---|
| 1 | Julius Caesar | 8.3% | +1 strength, +1 military action, and one double political action — mostly military, so the AI is the wrong judge of it |
| 2 | Alexander the Great | 7.8% | +1 strength per unit; cash him in later for a yellow token |
| 3 | Hammurabi | 7.5% | Use a military action as a civil action, and cheaper leaders afterwards — the best *economic* Age A leader on the card text |
| 4 | Moses | 6.1% | Population costs 1 food less; strong if you intend to grow hard, which at 3 players you do |
| 5 | Homer | 5.6% | +1 happy face, plus a resource whenever you build a unit |
| 6 | Aristotle | 5.5% | 1 science every time you take a technology card — rewards a high card-taking tempo |

**Age I** — the one age where the AI has a mild opinion.

| | Leader | Played | What it does for you |
|---|---|---|---|
| 1 | Christopher Columbus | 14.7% | Free colonisation. **Distrust this ranking entirely** — the AI never colonises, so it cannot possibly be valuing the actual ability |
| 2 | Joan of Arc | 13.9% | +1 culture, +1 military action, and strength from your temple/government happy faces — pairs with the temple-first build order |
| 3 | Michelangelo | 12.5% | 1 culture per happy face from temples, theaters and wonders. Human tables generally rate this the best Age I leader; the engine has it third |
| 4 | Leonardo da Vinci | 11.4% | Extra science from your best lab or library, plus a resource per technology played |
| 5 | Frederick Barbarossa | 10.6% | Population and a unit in one military action, cheaper |
| 6 | Genghis Khan | 9.4% | Pure military — underrated here by definition |

**Age II**

| | Leader | Played | What it does for you |
|---|---|---|---|
| 1 | Isaac Newton | 10.8% | Big science from your best lab/library, and a civil action back every time you play a technology — the action refund is the real prize |
| 2 | Napoleon Bonaparte | 10.0% | +2 military actions and large strength. Underrated here by definition |
| 3 | William Shakespeare | 10.0% | +1 happy face and cheap theaters — the happy face is what matters |
| 4 | James Cook | 8.6% | Colonies. Same warning as Columbus: untested, not weak |
| 5 | Maximilien Robespierre | 8.3% | Makes revolutions cost military actions instead of civil ones — worth much more to a human who actually changes government |
| 6 | J. S. Bach | 8.0% | Culture per theater and cheap theater technologies — but theaters are the *last* thing the AI builds, so this is circular |

**Age III** — the AI plays these on round 18–19, i.e. for the last three or four
production phases only. Judge them on how much they score in four turns.

| | Leader | Played | What it does for you |
|---|---|---|---|
| 1 | Albert Einstein | 9.1% | Science from your best lab, and 3 culture per technology played |
| 2 | Mahatma Gandhi | 8.9% | +2 culture and near-immunity to aggression. **The AI cannot value the immunity at all** — at a human table this is the point of the card |
| 3 | Charlie Chaplin | 8.6% | +2 happy faces and double culture from your best theater |
| 4 | Bill Gates | 7.2% | Your labs also produce resources |
| 5 | Winston Churchill | 7.2% | 3 culture, or science-and-resources for military, every turn |
| 6 | Sid Meier | 6.7% | Labs make culture instead of some science — a pure endgame conversion |

### Wonders, by age

Almost all of this comes from the 4-player AI, which is the only one that builds
wonders in volume (235 builds across 120 games). "Started/completed" is out of
120 games at 4 players. **The rule that matters is finishing, not starting.**

| Age | Wonder | Started / completed (4p) | Verdict |
|---|---|---|---|
| **A** | **Hanging Gardens** | 14 / **12** | +1 culture and **+2 happy faces** for 6 resources. The best wonder in the game by this data — 86% completion, done by round 4 |
| A | Library of Alexandria | 22 / 7 | +1 culture, +1 science, +1 to both hand limits. Started most often, finished a third of the time |
| A | Pyramids | 13 / **0** | +1 civil action — excellent on paper, **never once finished in 120 games** |
| A | Colossus | 11 / **0** | Strength and colony bids; never finished, and the AI could not use it anyway |
| **I** | **Universitas Carolina** | 14 / **14** | +1 culture, +2 science. **100% completion.** Start it around round 7 |
| **I** | **Taj Mahal** | 15 / **14** | +3 culture, +1 blue token. 93% completion |
| I | St. Peter's Basilica | 8 / 13 | +2 culture, +1 happy face, and *doubles every other happy face source you own* — the strongest text on this list |
| I | Great Wall | 21 / 5 | Culture, a happy face, and strength on infantry/artillery. 24% completion; the strength half is invisible to the AI |
| **II** | **Eiffel Tower** | 22 / **17** | +4 culture, +1 happy face. **The only Age II wonder that ever finishes** — 77%, started round 12 |
| II | Transcontinental Railroad | 21 / **0** | 0 for 21 |
| II | Ocean Liners | 19 / **0** | 0 for 19 |
| II | Kremlin | 18 / **0** | 0 for 18 |
| III | First Space Flight | 12 / 4 | Culture for your whole technology pile — scales with a tech-heavy game |
| III | Hollywood | 9 / 3 | Twice the culture output of your theaters and libraries |
| III | Fast Food Chains | 8 / 3 | Culture per worker |
| III | Internet | 8 / 3 | Culture for everything your urban buildings produce |

**The single most useful line in this section:** the three 12-resource Age II
wonders — Transcontinental Railroad, Ocean Liners and Kremlin — went **0 for 58**
across 120 games. Not one was ever finished. Meanwhile the cheap Age A and Age I
wonders started early finish 86–100% of the time. **Start a wonder by round 12 or
do not start it**, and prefer the cheap early ones.

### Civil (blue) buildings, by age

The **type** order is the strong finding and it is the same at every player
count: **temples first, then labs and libraries, and theaters and arenas last.**
At 2 players the first temple goes up on **round 3 in 100% of games**; the first
lab follows around round 5–6; theaters and arenas do not appear until rounds
11–15.

The reason is mechanical: a temple is a happy face *and* a culture point, so it
pays the happiness bill that the population track keeps sending you while also
scoring. A theater is culture with no happy face, so it can wait until you are
safe. An arena gives happiness but no culture, and the AI reaches for it least of
all.

Per-card, within each age (remember: weak evidence, the AI is nearly card-blind
when taking):

| Age | Temple | Lab | Library | Theater | Arena |
|---|---|---|---|---|---|
| I | Theology | **Alchemy** | **Printing Press** | Drama | Bread and Circuses |
| II | Organized Religion | Scientific Method | Journalism | Opera | — |
| III | — | Computers | Multimedia | Movies | Professional Sports |

Alchemy and Printing Press are the two most-taken Age I urban cards at every
count that reaches for them, both around round 4–5. A dash means the card never
appeared in any player count's top-30 list — that is an absence of data, not a
verdict on the card.

### Technologies, by age

"Technology" here means everything else on a blue card: farms, mines, military
units and the special technologies. Ranked by how often the AI took them, with
the same card-blindness warning.

| Age | Most-taken, in order |
|---|---|
| **I** | **Iron** (mine, round 3–5) → **Knights** (cavalry) → **Irrigation** (farm) → Swordsmen (infantry) → Warfare → Code of Laws → Cartography → Masonry |
| **II** | **Cavalrymen** → **Cannon** → **Coal** (mine) → **Selective Breeding** (farm) → Riflemen → Architecture |
| **III** | **Air Forces** → **Tanks** → **Mechanized Agriculture** (farm) → Rockets → Modern Infantry → Military Theory → Oil |

Two things are worth pulling out of that:

1. **Iron is the first technology you want.** It is the most-taken card in the
   game at 2 and 4 players, at round 3–5, which lines up exactly with the
   mine-on-round-2 build order.
2. **The special technologies** (Warfare, Code of Laws, Cartography, Masonry,
   Philosophy-line cards) are taken *early* — median round 3–5, in 89% / 78% /
   99% of games. They are the least glamorous cards on this list and the AI
   reaches for them more reliably than for anything except action cards.

---

## What changes with the player count

This is the section to read if you learned the game at one count and are now
sitting down at another. The three AIs are not three strengths of the same
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

The AIs' final numbers diverge enormously:

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
on which count is *right*, because the three AIs have had very different amounts
of practice (15 / 10 / 6 accepted improvements).

### 2 players: the row is a conveyor belt, so cheap cards are everywhere

**Three** cards are discarded off the left of the row at the start of every turn
and the row is refilled to 13 immediately [rules, §2.1] — so six cards a round
churn through, and there is only one other player bidding on them. Cards die
fast, but they arrive just as fast. The 2-player AI takes **88.4% of its cards from spaces 1–5** (1 CA
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

So the 2p lesson is not "be efficient" — this AI is not efficient. It is
that at 2p the row keeps handing you cheap, good cards, and the binding
constraint is *what you can build and feed*, not what you can reach.
**[strong]** on the card economics (rules + behaviour agree); the waste is a
flaw, not advice — see "Where your actions start going to waste".

### 3 players: expensive cards, a big army, and a rut

The 3-player AI is the odd one out at almost every measurement, and you should
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
game — but be careful what you conclude from that. *No* AI at *any* count
can choose an aggression or a war (caveat 3; `docs/PACTS_DIAGNOSIS.md`), so those
zeroes were fixed before the games were played and are **[not evidence]** that
the army was wasted. Nor could it ever pay off defensively in a world where
nobody attacks. What *is* measurable is the price: roughly two-thirds of the
economy the other counts run, bought with strength that can only cash out through
military events.

Our reading: this is **a rut that AI fell into**, not something the 3-player rules
demand. The decisive evidence is 4p — it faces *three* opponents rather than two (though in this AI's
world none of them can attack it) and instead opens economically, keeps almost no army,
and ends with the most technologies in the study. **[mixed, leaning against the
3p style]** — the 3-player AI does beat its own start point (70.3% ± 9.1), so
the style works; there is no evidence it is the best available style.

### 4 players: a wonder on round one, and a starving engine

The 4-player AI is the most *interesting* and the most *broken*.

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
game against 11.3 at 2p, so the military-card economy is dead too. (The pass rate
itself is not a choice — see "Read the 'pass' row as a symptom" in the midgame
section — but the ~10 missing preparations are real culture left on the table.
They also have a knock-on: territories only reach the board by being seeded with
`prepare_event`, so a 4-player AI that never prepares never even sees a colony
auction, which is why 4p colony bids are rarer still
[`docs/PACTS_DIAGNOSIS.md`].)

What to take from 4p and what to leave: **take** the action discipline, the
urban-heavy worker split (65% urban by Age III), and the round-1 wonder
consideration. **Leave** the food curve: hold production at **consumption + 1**
— that is 3/turn while the yellow bank is at 12–9 and 4/turn once it drops to
8–5 — which is two to three farm-levels more than this AI ever builds.
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
**[thin]**. The aggression row is worse than thin: it is three or four games'
worth of a move the AI cannot deliberately select at all (caveat 3;
`docs/PACTS_DIAGNOSIS.md`). Ignore it — it says nothing about when *you* should
attack. **[not evidence]**

### Where the counts actually agree

Three things hold at 2p, 3p and 4p, and those are the ones to trust. A fourth is
listed here only because it is the thing readers most often mistake for a
finding, and it is not one:

1. **Take a leader early and play it.** 97% / 83% / 98% of games take one; median
   play round 3 / 5 / 4. **[strong]**
2. **Temples are the first urban building at every count** (median round 5 / 8 /
   8) and the most-built *urban* one at 2p and 3p — 3.65 / 2.84 / 3.71 builds
   per game — with theaters and arenas last everywhere. (Only at 4p does another
   urban building beat them: labs, 4.71. And across *all* card types the biggest
   spender is mines at 2p and 4p, 4.28 and 5.38, and infantry at 3p, 7.14
   [`builds_by_type`].) **[strong]**
3. **Stop growing around round 9** and park the yellow bank just above 11 tokens,
   avoiding the 10-token happiness step. **[strong]**
4. **Nobody fights — because nobody *can*.** Zero wars in 360 games, aggressions
   0.01 / 0.03 / 0.11 per game. This is the one item on this list that is not a
   discovery. The AI simply cannot see the point of attacking: the payoff sits
   inside the victim's defence choice, which happens after the AI has already
   finished judging the move, so attacking always scores worse than passing. No
   version of it ever tried, none ever could, and nothing ever taught it whether
   an army is worth having (caveat 3; `docs/PACTS_DIAGNOSIS.md`). A true
   description of these games and **worthless as advice** — it is not evidence that an army is a
   wasted investment at a human table. **[not evidence]**

---

## Common traps

Six ways this game quietly takes points off you. All six are things the AI, left
to tune itself, decided were *worse* than we had guessed — which is its way of
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

All three tuning runs made the uprising penalty worse than the hand-set −12:
**−14.0 (2p), −15.5 (3p), −21.2 (4p)**. It is the largest single term in the
thing on its list at every player count. **[strong]**

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

How much it actually costs. `analysis/leak_check.py` replays AI mirror
games with the end-of-turn economy wrapped, comparing the culture your rating
says you should score against what you actually banked. The gap is starvation.
Over **60 games per player count** (`experiments/logs/leak_check.log`):

| | culture burned to starvation, per player-game | share of turns short | final culture |
|---|---|---|---|
| 2p | **21.4** | 16.5% | 129.9 |
| 3p | 6.0 | 6.3% | 107.5 |
| 4p | **56.1** | **46.1%** | 60.1 |

(Those final-culture figures are from the leak_check run's own 60 games, which
is why they differ by a few points from the 123.7 / 113.2 / 56.4 quoted
elsewhere from the 120-game set. Different games, same AIs, same story.)

At 4 players the AI burns **roughly as much culture to starvation as it
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
  [rules, §6.1] — and every AI is at 9.2–9.8 tokens at the end of Age III
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

What the AIs learned, for what it is worth: all three raised the value they put
on food production, and the 4-player one — the one that starves worst — is the
only one that flipped *late* food from bad to good, i.e. it has half-noticed that
a farm in Age III is worth buying. That is a real signal pointing the right way,
but it is far weaker than the behaviour warrants — the AIs have not
learned this lesson yet, which is exactly why they are all still bleeding.
**[strong on the leak, thin on the fix]** — we can measure the cost precisely;
we are inferring the remedy from the rules, not from a AI that solved it.

### 3. Corruption from a half-built wonder

Corruption is charged **before** your mines produce, and a shortfall is taken
out of your food. [rules, §6.6] Blue tokens sitting on the stages of an
unfinished wonder are *out of your blue bank*, so a wonder you started and did
not finish is charging you 2 or 4 resources a turn for the privilege.
[rules, §6.2, §9.2]

The 3-player AI tripled the penalty it puts on corruption, and both the 3- and
4-player AIs roughly doubled the value of having spare blue tokens — buy the
corruption headroom **before** you need it, not when the bill arrives.
**[mixed]** — the 2-player AI never moved on either, so this is a 3p/4p finding.

And if the age turns while your wonder is unfinished and now antiquated, the
wonder is removed from play entirely. You get the blue tokens back; you do not
get the actions or the resources back. [rules, §12.2]

### 4. Buying rate in Age III

A lab bought on round 19 of a 23-round game scores four times. A farm bought
then scores nothing at all, because food is not victory points. All three AIs
worked this out for themselves: every one of them lowered the value of late
science rate, two of the three lowered late resource rate, and **all three**
lowered the value of a card still in hand late — one of only four judgements they
all agree on. **[strong]**

The exception is 4 players, where late resource rate went the *other* way. Given
that the 4-player AI has banked only six accepted improvements, treat that as
**[provisional]** and follow the 2p/3p reading — except for food, where trap #2
overrides this whole trap.

### 5. Hoarding science points

Unspent science points score nothing. Ever. "A banked science point is worth
something" is one of the four judgements all three AIs revised in the same
direction: all three cut it, and two cut it past zero into *negative*. Banked
science is not a war chest, it is a civil action you failed to take. **[strong]**

The same goes for cards: all three AIs price a card still in hand late as a
liability.
Hold cards in Ages A and I when you cannot yet afford them; from Age II
onwards, a card in hand on the last turn is worth exactly zero.

Note the contrast with **resources**, which the 3-player AI tripled in value.
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
between the three AIs.

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

---

## What this document does not know

Read this before you treat anything above as complete. These are not hedges;
they are parts of the game the study has **no data on at all**, because the
AIs never went there.

### The one misreading this document must not cause

**The AI never signs a pact, never declares war, never plays an aggression and
almost never colonises. This is not because those things are weak. It is because
the AI is incapable of choosing them.** Full working in
`docs/PACTS_DIAGNOSIS.md`; the short version:

The AI picks its move by trying each one and looking at **its own board
immediately afterwards, before anybody else responds**. But offering a pact,
declaring a war, playing an aggression and opening a colony auction all work the
same way in the rules: you spend the card or the worker now, and the result is a
**decision that lands on somebody else** — the partner accepts or refuses, the
defender chooses what to lose, the rival bidders answer. None of that has
happened yet when the AI looks. So all it sees is the cost.

Here is one such comparison, taken straight from the instrumented run. The AI is
deciding between passing and offering the pact *International Tourism*. Only two
things on its board change, and both change for the worse — it has one fewer
military card in hand, worth 4 points less. Everything else on its board is
identical. Score for offering the pact versus passing: **−1.10445**. Not "worse
in this position": worse by that same fixed amount in *every* position, because
nothing else ever moves.

That is why no amount of practice could ever make it offer a pact, and why ties
in a colony auction always break towards passing. Measured directly: **it was
legal to offer a pact in 16% of political decisions across 240 games, and it was
chosen zero times.**

Two consequences you must carry:

1. **The zeroes above are not results.** "Zero wars in 360 games" has the same
   evidential value as "zero wars in a game where the war cards were left in the
   box". It is not a discovery that war is bad; the experiment could not have
   come out any other way. Same for pacts, aggressions and colony bids.
2. **Whatever the AI "thinks" pacts and colonies are worth is noise, not
   advice.** Those values were never tuned, because no game it ever played
   depended on them. The tell is stark: after thousands of rounds of tuning, the
   3-player AI's price for a colony is still **2.000 — bit for bit the number we
   typed in by hand on day one**, while the 4-player one has wandered to
   **−0.962** at random. Do not quote either, in either direction.
   **[not evidence]**

The honest position on the whole political half of *Through the Ages* is
therefore **"untested"**, not "unimportant". A human table plays a different game
from the one these AIs played.

- **Pacts.** Zero pacts were played in 240 games at 3p and 4p — the move type
  never appears in the log (`moves_per_game`, which lists every move the
  AIs made). Pacts are legal at 3p and 4p [rules, §13]. We can tell you
  nothing about them, and the zero tells you nothing either: it is the structural
  blind spot above, not a verdict. **[not evidence]**
- **Colonies.** The AIs bid on a colony **0.18 (2p) / 0.08 (3p) / 0.02
  (4p) times per game** and pass instead 0.79 / 3.81 / 0.08 times
  (`moves_per_game`: `bid`, `bid_pass`). Effectively they never colonise — and
  again, they *cannot*: while any rival is still bidding, a bid changes nothing on
  your own board at all, so a bid and a pass score exactly the same and the tie
  breaks towards passing. On the rare occasions a bid *does* show up on its board
  (it is the last bidder standing, so the colony resolves at once) it is judged
  using that never-tuned colony price. This document has no
  colony advice and you should not read the silence as "colonies are bad" — read
  it as "untested". **[not evidence]**
- **Fighting.** Zero wars in 360 games, aggressions 0.01 / 0.03 / 0.11 per game —
  guaranteed in advance by the blind spot, not learned. Everything above about
  military is about *deterrence levels*, inferred from a world where nobody
  attacks and nobody could. **We do not know what the correct army size is at 3p
  or 4p against opponents who attack**, and we have no evidence at all on when to
  attack, whom to attack, or what a threat is worth. See headline rule 8.
  **[not evidence]** for the fighting rates.
- **Wonders at 2p and 3p.** Only 25% / 19% of games touch a wonder at all, so
  the round-12 rule above rests on the 4-player AI's 235 builds. At 2p and 3p we
  have too few builds to say whether wonders are good.
- **Which culture buildings are best.** We report *when* things get built and
  *how many actions* they absorb, not what any individual card is worth. There
  is no card-strength ranking in this document.
- **Anything about a human opponent.** Every number here comes from mirror
  self-play against copies of the same AI. They have
  never seen a bluff, a rush, a targeted aggression or a player who is behind
  and playing for variance.
