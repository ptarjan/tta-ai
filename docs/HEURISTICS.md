# Heuristics for human players

**Through the Ages: A New Story of Civilization — base game, 2015 edition, no expansion.**

Written for someone sitting at a table with the physical game. Everything here
comes from a rules-complete engine plus a self-play AI that is still training.

---

## How to read this document

**Snapshot warning.** The AI is mid-training. Every number below is a snapshot
taken on **2026-07-26**, with the champions at generation **149 (2 players),
116 (3 players) and 101 (4 players)**. Behaviour was harvested from those exact
champions at **120 games per player count**, mirror self-play. Numbers will
move. Structural advice ("spend your actions", "science early, culture late")
is much more stable than any single figure.

**How strong is the thing giving you advice?** Measured against the hand-set
weights it started from, 96 games each (`experiments/generations_*.jsonl`
anchor series):

| | champion vs. its own start point | null | vs. a greedy bot |
|---|---|---|---|
| 2p | **82.3% ± 7.7** (gen 140) | 50% | 90.6% |
| 3p | **70.3% ± 9.1** (gen 110) | 33.3% | 74.0% |
| 4p | **66.2% ± 9.5** (gen 100) | 25% | 90.6% |

All three are now decisively better than where they began. That was *not* true
of the earlier draft of this document.

**Where the numbers come from.**

| Source | What it is |
|---|---|
| `experiments/behaviour_{2,3,4}p.json` | 60 self-play games per player count. What the champion actually *did*: milestone rounds, worker splits, rates by age, cards bought. Board snapshots taken at the end of each of its turns. |
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
   mutants in 151 generations, 3p 9 in 119, 4p only **5 in 103**. So when the
   counts disagree, the 4p number is the one most likely to be young rather
   than right — and the 4p weight vector contains some wild values (a
   `science` stock weight of −6.1, a `science_rate` of +22.5) that look like a
   single accepted mutant that has not yet been trimmed back. Treat extreme 4p
   figures as **[provisional]** unless the behaviour data backs them.
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

1. **Spend all your civil actions, every single turn.** Actions do not carry
   over [rules]. The strongest-improving climb (3p) is also the one that wastes
   the fewest — it ends its turns with 1.77 unused civil actions against 2.75
   and 2.98 for the other two, and it leaves *something* unspent on 46% of turns
   against 68% and 71%. The search independently drove the "leftover actions are
   nice" weights to zero or below at every count that moved. **[strong]**

2. **Take a leader on round 1 or 2 and put it in play by round 3.** Median round
   to take one: 2 at all three player counts. Median round to play one: 3 (2p),
   4 (3p), 3 (4p). The champion has a leader out in 75% of Age I turns at 2p and
   70% at 4p. The "having any leader in play" weight more than doubled at 3p
   (+1.5 → +3.4). **[strong]**

3. **Upgrade your production on round 2.** At 2p and 4p the champion's first
   farm/mine upgrade lands on round 2 in **100% of games** (median and both
   quartiles are round 2). First urban building upgrade follows on round 3-4.
   3p is the exception and delays it — see the per-count section. **[strong]**

4. **Build about three temples, and never let an uprising happen.** Temples are
   the single most-built card type at every player count: 2.83 / 2.95 / 2.67
   builds per game, first one around round 5-8. An uprising cancels your entire
   production phase [rules] and carries the largest penalty in the whole
   78-weight evaluation (−12 by hand; the search pushed it to −13.2 / −17.6 /
   −13.1). **[strong]**

5. **Science first, culture later — the crossover is in Age II.** Champion
   science-to-culture ratio by age (2p / 3p / 4p): Age I **0.96 / 1.94 / 0.85**,
   Age II 0.79 / 0.63 / 0.46, Age III 0.82 / 0.56 / 0.32, Age IV 0.88 / 0.49 /
   0.28. It falls monotonically at all three counts. Both learning climbs raised
   "early culture rate" (+54% / +42%) and cut "late science rate" (−40% / −30%).
   **[strong]**

6. **Do not hoard. Not science points, not cards.** The 3p search flipped the
   sign on banked science points (+0.5 → −0.19): unspent science is dead weight.
   Both learning climbs raised the value of cards in hand *early* and cut it
   *late* (−78% / −50%). Hold cards in Ages A-I; cash them out from Age II on.
   **[strong]**

7. **Stop buying rate in Age III.** Both climbs that moved cut the value of
   late-game resource rate (−54% / −31%) and late-game science rate (−40% /
   −30%). A farm or lab bought in Age III does not pay for itself before
   scoring. Buy culture instead. **[strong]**

8. **Keep military at parity with the strongest player — that is all you need,
   until 3 players.** The champions never attack: **zero wars** in 180 games,
   and 4 aggressions total (all at 3p). They keep strength at 1.02-1.06× the
   table in every age. But the size of army needed to hold that parity is wildly
   different by count — see the per-count section. **[mixed]**

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

Both champions take an **action card** on round 1 in essentially every game
(median round 1 at both counts; 100% of games take one eventually). The two Age A
action cards are the most-taken round-1 cards at both counts: at 2p
`Frugality (A)` 0.37 per game and `Urban Growth (A)` 0.33 per game, both with a
median round of 1. **[strong]**

That is not a deep insight so much as an arithmetic one: on round 1 you have
nothing to spend resources on, and an Age A action card is a resource or food
rebate you can cash on round 2 when you suddenly have four actions and nothing
banked. The seat-1 player, with a single civil action, gets exactly one shot at
this.

### Round 2 is the highest-leverage turn in the game

You go from 1–4 civil actions and no military actions to a full **4 CA + 2 MA**,
and the board is still symmetric. Three things the 2p champion does on round 2,
in **100% of 120 games** — not a median, the whole distribution sits on round 2:

1. **Add production.** First farm-or-mine build/upgrade lands on round 2 in every
   single game (p10 = p25 = p75 = p90 = 2). Production workers go 4.00 → 4.98
   between rounds 1 and 2. **[strong at 2p]**
2. **Take a leader, or be about to.** Median round to *take* a leader is 2 at both
   counts, and the 25th percentile is round 1 — a quarter of games spend the Age A
   turn on a leader instead of an action card. **[strong]**
3. **Disband the starting Warriors.** This one is real and it is startling: at 2p
   military workers go **1.00 → 0.00** on round 2 and strength goes **1.00 →
   0.06**. Disbanding a unit costs 1 military action and returns the worker to
   your pool [rules, §4.3] — and your 2 military actions are otherwise dead in
   Age I. The champion converts its warrior into a farm worker on turn 2 and
   stays at essentially zero strength for all of Age I (mean military workers in
   Age I: **0.16**). **[provisional — and see the warning below]**

**Warning on #3.** This is mirror self-play with **all pacts removed at 2p**
[rules, §13] against an opponent that has never once attacked in 120 games. A
champion at 0.06 strength across Age I is defensible only because nobody in its
world has ever punished it. Against a human who will Plunder you for 1 military
action, disbanding your only unit is throwing three food and three resources at
them. Read #3 as *"the starting warrior is worth less than you think and your
early military actions are worth more"*, not as an instruction.

The 3p champion does the exact opposite — see below.

### Round 3: the first urban building

At 2p, the first lab/temple/library/theater/arena build lands on **round 3 in
100% of games** (again the entire distribution, p10 through p90, sits on 3), and
the leader is in play by round 3 (median 3, 61.7% of games have one out by the
end of round 3, 74.2% by round 4). Urban workers go 1.0 → 1.93 → 2.59 across
rounds 2–4. **[strong at 2p]**

So the 2p opening skeleton is: **R1 action card → R2 production + leader taken →
R3 urban building + leader played → R4-5 second urban building.** Techs go 5.0
(the board) → 5.21 → 5.75 → 6.18 over rounds 3–5; science rate does not leave 1
until round 5 (1.58) and culture rate reaches 2.37 by round 5.

### 3p opens completely differently, and you should know why

The 3p champion is a **military opening**, and it is the single largest
disagreement in this document:

| Round 2 | 2p champion | 3p champion |
|---|---|---|
| Military workers | 0.00 | 1.68 |
| Strength | 0.06 | 1.82 |
| Production workers | 4.98 | 4.00 |
| Urban workers | 1.00 | 1.00 |

The 3p champion **never upgrades production in 39% of its games**, and when it
does the median round is 8. It puts its round-2 actions into a second infantry
unit instead. Across the whole game it builds **7.14 infantry** (median round 6)
against the 2p champion's 2.41, and it ends Age III at **strength 7.28** against
2p's 3.79. Its weight vector agrees: `strength_rel` is its single most-moved
weight (+0.35 → **+1.88**, +436%) and `workers_early` was cut 74%.

Is that right, or is it a local optimum? Honestly: **unclear**. The 3p champion
scores less culture (113.2 mean vs 2p's 123.7) and finishes with fewer techs
(9.81 vs 12.88), and it still never actually attacks (4 aggressions in 120
games). It may have learned "be scary" rather than "be strong". But two things
make it worth taking seriously: 3p and 4p are the counts where **pacts exist**
and where **two opponents can both come at you**, and the strength-deficit
penalty is one of only four levers all three player counts agree on
(−0.6 → −1.02 / −0.95 / −1.30). **[mixed]**

Practical read: **at 2p your opening is economic; at 3p and 4p you cannot open
economic without a plan for how you survive Age I at strength 1.**

### How deep into the row to reach, early

The row sweeps **3 cards per turn at 2p, 2 at 3p, 1 at 4p** — six a round at both
2p and 3p, four at 4p. [rules, §1.5] A card in space 7 at 2p has about one round
to live.

The champions handle this very differently:

| | cards taken per game | CA spent taking | share from spaces 1–5 | share from 10–13 |
|---|---|---|---|---|
| 2p | 22.0 | 25.2 | **88.4%** | 3.0% |
| 3p | 12.8 | 29.8 | 23.5% | **56.9%** |

The 2p champion is a **volume buyer** — it takes almost everything from the cheap
end of the row, 22 cards for 25 actions, barely ever paying 2 or 3 CA. The 3p
champion takes **half as many cards for more actions**, mostly from the expensive
end. With three players competing for one row, the card you want rarely survives
to drift left. **[mixed]** — this is a genuine structural difference between the
counts, but the 3p figure is also consistent with a champion that simply does not
need many cards because it spends its actions building infantry off techs it
already has.

If you are at 2p: be patient, let cards slide, take the cheap end. If you are at
3p or 4p: budget for the fact that you will sometimes have to pay 3 actions for
the card you actually need.

### Government: later than you think

Neither champion rushes a government.

| | ever take a govt card | median round taken | ever change govt | median round changed |
|---|---|---|---|---|
| 2p | 72.5% | 7 | 70.0% | 8.5 |
| 3p | 55.8% | 5 | 50.8% | 7 |

Most-taken first governments: 2p **Theocracy** (25.8% of games, median round 5)
then Republic (16.7%, round 12) and Monarchy (15.0%, round 6); 3p **Monarchy**
(23.3%, round 5.5) then Theocracy (16.7%, round 6). Nearly a third of 2p games
and half of 3p games **never leave Despotism at all**. **[strong]**

Despotism's 4 CA / 2 CA-worth-of-limits is not so bad that you should burn a
whole turn's civil actions on a revolution in Age I. Note the rules asymmetry: a
**revolution costs all your civil actions** and burns any actions the new
government grants that turn, while a **peaceful change costs 1 CA plus a higher
science price** and lets you keep playing. [rules, §8] If you are changing
government early, you almost certainly want the peaceful version.

### What "on pace" looks like at the end of Age I

Age I ends around round 6–7. Champion state at that moment:

| At end of Age I | 2p | 3p |
|---|---|---|
| Round | 7 | 6 |
| Workers | 11.0 | 10.1 |
| Techs (incl. the 5 starting cards) | 7.3 | 6.4 |
| Science rate | 2.5 | 1.3 |
| Culture rate | 3.4 | 1.5 |
| Resource rate | 3.8 | 2.0 |
| Strength | 1.5 | 2.6 |
| Yellow bank left | 14.0 | 14.9 |
| Wonders completed | 0.06 | 0.00 |

The number to steal from that table is **yellow bank ~14**: both champions have
taken about four population by the end of Age I, which keeps them in the
"cost 3, consume 1, 1 happy face required" band and one step clear of the nasty
jump at 10 tokens. [rules, §6.1]

And note the last row. **Neither champion completes a wonder in Age I**, at
either count. Wonders are covered in the midgame and per-count sections; the
opening verdict is that they are not an opening.

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

**Temples are first and most-built at every player count** — 3.65 / 2.84 / 3.71
per game, ahead of every other urban type at 2p and 4p. Theaters and arenas are
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
that part.

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

| | strength (Age IV) | ratio to strongest rival | aggressions/game | wars/game |
|---|---|---|---|---|
| 2p | 4.27 | 1.07 | 0.008 | **0** |
| 3p | 7.39 | 1.03 | 0.033 | **0** |
| 4p | 3.48 | 1.13 | 0.108 | **0** |

**Zero wars in 360 games at every player count.** Aggressions are rare everywhere,
and where they happen at all they happen *late*: at 4p the median first
aggression is **round 18.5** (p25 17, p75 20), i.e. in Age III. **[strong on the
behaviour; see the caveat]**

The caveat matters. These are mirror self-play games between civilizations that
have all learned nobody attacks. A table of humans is not that. What survives the
caveat is the *shape*: all three champions hold strength at **1.03–1.13× the
strongest rival** in Age IV, and the `strength_deficit` penalty is one of the four
full-consensus levers (−0.6 default → −1.02 / −0.95 / −1.30). **Parity is the
target; being the biggest army is not.**

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

### 2. Starving for one food

Missing food at consumption costs **4 culture per missing food**, every turn it
happens. [rules, §6.6] Four culture is roughly a full turn of Age I culture
production — you can bleed a whole age's worth of points through a one-food
gap and barely notice.

Consumption steps at 16, 12, 8 and 4 tokens left in the yellow bank; the pop
*cost* steps at different squares (16, 12, 8, 4 for cost 3/4/5/7), which is why
the two feel out of sync. [rules, §6.1] The search raised `food_rate` at all
three counts (+5% / +48% / +12%) — 3p most of all. **[mixed]** — 3p moved it
hard, the other two barely.

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
sign flip). Given that the 4p climb has accepted only 5 mutants, treat that as
**[provisional]** and follow the 2p/3p reading.

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
