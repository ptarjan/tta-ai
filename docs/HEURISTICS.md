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

**Two honest caveats you should carry through the whole document.**

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
