# Opening audit: is the 4p "wonder first" opening real?

**Question.** [`docs/HEURISTICS.md`](HEURISTICS.md) says 2p and 3p champions open round 1 by taking
an action card while the 4p champion takes a wonder. Is that real strategy, a
reporting artefact of how we aggregate seats, or undertrained noise?

---

# VERDICT: UNDERTRAINED NOISE — a single weight, flipped by accident at generation 5 of 138, and never revisited

The behaviour is **real and reproducible** — it is not a seat-mixing artefact,
and comparing seat-for-seat only makes it sharper (2p seat 0 takes a wonder first
in **0%** of 400 games, 4p seat 0 in **74%**). But it has **nothing to do with
playing four players.**

Three findings, in order of how much they should change your mind:

1. **Player count cannot affect the round-1 decision, and does not.** The Age A
   deck is identical at all counts and the row's first sweep happens *after*
   round 1, so for a given seed seat 0 faces a bit-identical 13-card row at 2p,
   3p and 4p. Untrained `default` weights open identically (64% action / 36%
   leader, seat 0) at all three counts. The sweep-speed and competition
   arguments printed in [`HEURISTICS.md`](HEURISTICS.md) are inert on round 1.
2. **Cross-play proves it follows the weights, not the table.** Played *at two
   players*, the 4p weight vector still opens wonder-first 74% of the time.
   Played *at four players*, the 2p vector never does (0%). Player count changes
   nothing; the weight vector changes everything.
3. **It is one weight, it was a hitchhiker, and it is worth nothing.**
   `wonder_remaining` was flipped from −0.3 (penalise unbuilt wonder stages) to
   +0.32 by the gen-5 mutation, which moved **19 weights at once**. Revert that
   one number in today's champion and the wonder opening vanishes entirely
   (74% → 0%). The opening rate has been frozen at 77% for all 125 generations
   since, unchanged by six further accepted mutations — stable because nothing
   has searched it, not because anything converged on it. Played head-to-head
   against itself-with-the-weight-reverted over 192 games, the wonder-first
   champion wins **0.276 ± 0.063 against a 0.25 null** — indistinguishable, even
   though that test lets it take every wonder uncontested. (One caveat pointing
   the other way, measured and reported in §4: against the *untrained* bot the
   wonder version scores 0.792 vs the reverted version's 0.641.)

**What to do with it:** do not write "at 4 players, open with a wonder" as
advice. There is no evidence for the player-count claim. The honest statement is
*"our 4p weight vector happens to like wonders, at every player count, because of
one sign flip nobody tested."*

### What this implies about every other weight we quote

This is the part with consequences beyond the opening. `wonder_remaining` is a
**trained** weight — it moved from its hand-guessed default, in an accepted
generation, and HEURISTICS' framing ("the tuning moved a price, and it moved it
in a direction that won games") would license quoting it as something the AI
taught itself. Tested directly, it is indistinguishable from null.

The reason is structural, not bad luck: mutations move **19 weights at once** and
are accepted on a single 48-game win-rate test. Acceptance says *the bundle*
beat the incumbent; it says **nothing** about any individual weight in it. With
~78 weights, 8 accepted bundles at 4p, and no per-weight ablation ever run, most
individual weight moves in our champions have never been tested at all.

So: **"the AI moved this weight, therefore it matters" is not a valid inference
anywhere in [`HEURISTICS.md`](HEURISTICS.md).** Any weight-derived claim in that document is at the
same evidential level as this one — plausible, untested, and roughly as likely to
be a hitchhiker — unless someone has ablated it the way §4 ablates
`wonder_remaining`. That single-weight revert test is cheap (one variant file,
one duel) and should be the standard before any weight is written up as advice.

**Separately — the answer to "is hill climbing working?": YES, at all three
counts, and best at 4p.** Measured fresh today against the untrained starting
point: 2p **0.682** (null 0.50), 3p **0.771** (null 0.333), 4p **0.792** (null
0.25); 4p also beats the greedy bot 0.958. Relative to its null, 4p is the
*strongest* of the three (3.2x), not the weakest. The 4p climb has had the same
wall-clock time as the others and is still accepting mutations. The lower numbers
in `experiments/baselines.jsonl` (4p 0.349) that suggest a broken climb are stale
— see §5, including which [`HEURISTICS.md`](HEURISTICS.md) claims they have already contaminated.

What *is* thin at 4p is the number of accepted steps — 8 in 138 generations — and
each one moves 19 weights on a 48-game test. The climb is working; it is the
*attribution* of what it learned that does not hold up.

---

Owned by this audit: `analysis/opening_by_seat.py`, this file. Everything under
`experiments/` and `engine/` was read-only for this work. Champion snapshots were
copied to `/tmp` first because the live hill climbs rewrite them in place;
`champion_4p.json` was gen 138 and is bit-identical at gen 139, and the 2p
champion advanced 218 → 220 during the audit without changing its opening (still
0% wonder-first).

---

## 1. How the number was actually computed

Two separate answers, and neither is the script you would expect.

### `analysis/opening_order.py` did not produce it — it cannot run

The script crashes on every game:

```
$ python3 analysis/opening_order.py --players 4 --games 4 --champion /tmp/ch4.json
game error 51000 TypeError("'NoneType' object is not callable")
... (x4)
===== 4p, 0 games =====
IndexError: list index out of range
```

Two bugs:

1. Its `Logger` wrapper exposes `.choose()` and sets `__call__ = None`, but
   `engine/game.py:play_game` calls bots as `bots[state.decider()](state)` — a
   plain callable. Every game raises `TypeError`, is swallowed by the
   `except Exception` in `run()`, and zero games are logged.
2. `card_type()` uses `getattr(c, "type", None) or getattr(c, "kind", "?")`, but
   cards in the DB are **plain dicts** (`db.get("Pyramids")` →
   `{'name': ..., 'type': 'wonder', ...}`). `getattr` on a dict never sees the
   key, so every card type it reports would be `"?"` even if the games ran — and
   its farm-vs-mine "first production build" detector (`typ in ("farm","mine")`)
   could never fire.

(That file is owned by another agent; it is only diagnosed here, not edited.)

### The real source is `experiments/behaviour.py`, and it averages all seats

The 120/120 figure and the p10/p25/p75 language in [`HEURISTICS.md`](HEURISTICS.md) match the
`milestone_distribution.take_wonder` block in `experiments/behaviour_4p.json`.
`behaviour.py` builds its task list as

```python
tasks = [(seed0 + g // players * 7919 + 17, g % players) for g in range(games)]
```

so the champion is rotated through every seat and **all of those games are pooled
into one `champion_behaviour` block**. Round 1 is the one round in the game where
seats are not symmetric — `engine/game.py:68` sets `p.civil_actions = i + 1`
(§1.9), so seat 0 gets 1 civil action and seat 3 gets 4 — and taking cards is the
only legal action in round 1 (`engine/actions.py:359`).

So the pooled "opening":

* at 2p averages a 1-CA seat with a 2-CA seat (mean 1.5 CA),
* at 4p averages 1, 2, 3 and 4 CA seats (mean 2.5 CA).

A 4p player takes on average **1.67x more round-1 cards** than a 2p player purely
from seating. Anything phrased as "the champion's opening card" is therefore
comparing different seat mixes across player counts. **That confound is real and
it is in the published number.** Whether it is big enough to *cause* the reported
difference is section 2.

One confound ruled out immediately: the Age A civil deck is **identical at all
three player counts** (same 20 cards; `db.civil_deck("A", n)` is count-invariant
in Age A), so the 4p champion is not simply seeing more wonders.

---

## 2. Re-measured by seat

`analysis/opening_by_seat.py` (new, owned here) logs **every seat of every game**
and reports round 1 per seat: cards taken, the type of the first card, and
whether a wonder was taken at all. Results below.

Mirror self-play (every seat runs the champion, exactly how the hill climb
evaluates), 400 games per count, so 400 observations per seat. `wonder1st` is the
share of games where the **first** card taken in round 1 is a wonder.

| count | seat | CA | cards taken R1 | wonder 1st | action 1st | leader 1st | any wonder in R1 |
|---|---|---|---|---|---|---|---|
| **2p** | 0 | 1 | 1.00 | **0%** | 64% | 36% | 0% |
| | 1 | 2 | 2.00 | **0%** | 62% | 38% | 9% |
| | *pooled* | – | 1.50 | *0%* | *63%* | *37%* | *4%* |
| **3p** | 0 | 1 | 1.00 | **0%** | 64% | 36% | 0% |
| | 1 | 2 | 1.00 | **0%** | 60% | 40% | 0% |
| | 2 | 3 | 1.00 | **0%** | 64% | 36% | 0% |
| | *pooled* | – | 1.00 | *0%* | *63%* | *37%* | *0%* |
| **4p** | 0 | 1 | 1.00 | **74%** | 18% | 8% | 74% |
| | 1 | 2 | 1.52 | **77%** | 16% | 7% | 77% |
| | 2 | 3 | 1.56 | **80%** | 12% | 8% | 80% |
| | 3 | 4 | 2.51 | **26%** | 40% | 34% | 26% |
| | *pooled* | – | 1.65 | *64%* | *21%* | *14%* | *64%* |

Read seat-for-seat: **2p seat 0 takes a wonder first 0% of the time, 4p seat 0
takes one 74% of the time.** The difference survives the correct comparison, so
it is *not* explained by seat mixing. Seat mixing is still a real flaw in how the
number is reported, but here it works the *other* way — pooling drags the 4p
figure **down** (64%) from the 74–80% that seats 0–2 actually show, because seat
3 finds the wonders already gone (its own mirror-image opponents took them).

Also worth noting: nobody spends all their civil actions in round 1. Seat 3 at 4p
has 4 CA and takes 2.51 cards; the 3p champion takes exactly 1.00 card at every
seat and simply throws the rest away. (That belongs to the wasted-actions audit,
not this one, but it is visible here.)

---

## 3. The 4p confounds do not explain it — the round-1 board is identical

The rules argument in [`HEURISTICS.md`](HEURISTICS.md) is that at 4p the row sweeps only 1 card per
turn (`engine/game.py:40  SWEEP = {2: 3, 3: 2, 4: 1}`) so cheap Age A wonders
survive longer, and that more rivals means more competition per card.

**Neither can act on round 1.** The first sweep happens on the start player's
*second* turn — it is the event that ends Age A (`_replenish`, §1.10). On round 1
no sweep has occurred yet, and the Age A deck is count-invariant, so for the same
seed **seat 0 faces a bit-identical 13-card row at 2p, 3p and 4p**.

The control proves it. Untrained `default` weights, same seeds, mirror play:

| count | seat 0: wonder 1st | action 1st | leader 1st |
|---|---|---|---|
| 2p | 0% | 64% | 36% |
| 3p | 0% | 64% | 36% |
| 4p | 0% | 64% | 36% |

Identical to the decimal at all three counts, exactly as it must be if the board
is the same. Player count has **no** effect on the round-1 decision of a fixed
weight vector.

### Cross-play: the opening follows the weights, not the player count

400 games each, mirror, seat 0 (the only seat with no interference from earlier
takers):

| weight vector | played at 2p | played at 3p | played at 4p |
|---|---|---|---|
| `champion_2p` | **0%** | 0% | 0% |
| `champion_3p` | 0% | **0%** | 0% |
| `champion_4p` | 74% | 77% | **74%** |
| `default` | 0% | 0% | 0% |

(share of games where seat 0's first round-1 card is a wonder)

The 4p weight vector opens wonder-first *at two players* just as strongly as it
does at four. The 2p vector never opens wonder-first *at four players*. So the
reported difference is a property of **that particular weight vector**, and
nothing about playing against three opponents caused it or could have caused it.
The rules rationale printed in [`HEURISTICS.md`](HEURISTICS.md) — sweep speed, cost bands,
competition — is post-hoc: those mechanisms are all inert on round 1.

---

## 4. Training maturity: it was decided at generation 5 and frozen

Reconstructing the 4p champion after every accepted mutation (replaying the
`moved` deltas in `experiments/generations_4p.jsonl` onto `DEFAULT_WEIGHTS`;
reconstruction matches the live `champion_4p.json` to ~1e-4), then measuring
seat-0 round 1 over 300 games each:

| 4p champion as of gen | 1 | 5 | 51 | 63 | 79 | 103 | 124 | 130 |
|---|---|---|---|---|---|---|---|---|
| wonder-first (seat 0) | **0%** | **77%** | 77% | 77% | 77% | 77% | 77% | 77% |

The opening flips at **generation 5 of 138** and then never moves again — not by
one game in 300, across 125 further generations and six more accepted mutations.

The cause is a single weight. `wonder_remaining` (default **−0.3**, i.e. unbuilt
wonder stages are a *penalty*) was flipped to **+0.319** by the gen-5 mutation,
which moved **19 weights at once** and was accepted on a 48-game win rate of
0.424 (null 0.25). `wonder_remaining` was a hitchhiker in that scatter — it was
never independently tested, and the search has never revisited it. The later
`kick` at gen 79 pushed `wonder_progress` 1.0 → 4.60 and `hand_civil`
0.3 → −0.68, but the seat-0 opening rate did not budge (77% before and after):
the decision was already saturated.

Confirmation: taking the current 4p champion and reverting **only**
`wonder_remaining` to its default −0.3 removes the behaviour completely.

| 4p seat 0 | wonder 1st | action 1st | leader 1st |
|---|---|---|---|
| `champion_4p` | 74% | 18% | 8% |
| `champion_4p` with `wonder_remaining = −0.3` | **0%** | 62% | 38% |

One sign flip on one weight, taken as a passenger in one early mutation, is the
entire "4p opens with a wonder" finding.

So the opening is **stable** — but stable because it is frozen, not because it
was converged on. Stability here is evidence of *no further search*, not of
optimality. Whether it is actually good is the next question.

### Does the wonder opening earn anything? No.

Direct A/B: `champion_4p` as challenger against a table of *itself with only
`wonder_remaining` reverted to −0.3* — identical in all 77 other weights, and the
only behavioural difference is the opening (74% wonder-first vs 0%). 192 games,
challenger rotated through every seat, null = 0.25:

| challenger | defenders | games | win rate | null | verdict |
|---|---|---|---|---|---|
| `champion_4p` (wonder-first) | `champion_4p`, weight reverted | 192 | **0.276 ± 0.063** | 0.25 | **indistinguishable** |

The confidence interval (0.213–0.339) straddles the null. And note this is the
*most favourable possible* test for the wonder strategy: the challenger is the
only wonder-lover at a table of three bots that do not want wonders, so it takes
them completely uncontested — and still gains nothing measurable.

**One caveat, against my own conclusion.** Measured indirectly against the
untrained `default` bot instead, the two are not obviously equal:

| variant | vs `default` @4p (96 games) | mean culture |
|---|---|---|
| `champion_4p` | 0.792 ± 0.082 | 262.7 |
| `champion_4p`, weight reverted | 0.641 ± 0.096 | 202.4 |

That is a 15-point gap on the same seeds and the same opponent — roughly 2.5
standard errors, so the wonder version *may* genuinely be stronger against a weak
opponent. The two tests disagree, and I am not going to pretend they don't. The
head-to-head is the more direct evidence (the two bots actually play each other,
with twice the games), so the honest summary is: **the wonder opening earns
nothing detectable where it matters most, and any advantage it has is small,
unconfirmed, and was never what the search was selecting for.** It certainly does
not support a player-count heuristic, since the same weight produces the same
opening at 2p and 3p too.

(Oddity worth a follow-up: in the mirror head-to-head both bots score ~55 mean
culture, against 200–260 when either plays the default bot. Two strong, nearly
identical bots at the same table appear to strangle each other's scoring. That is
not investigated here.)

---

## 5. Is the hill climb working at all?

**Yes — clearly, at all three counts.** Re-measured today against the current
champions (`experiments/evaluate.py`, 96 games, challenger rotated through every
seat, null = 1/players, ± is the 95% CI):

| count | champion vs `default` | null | champion vs `greedy` | mean culture (champ vs default) |
|---|---|---|---|---|
| 2p | **0.682 ± 0.093** | 0.50 | 0.917 ± 0.056 | 134.6 vs 102.1 |
| 3p | **0.771 ± 0.085** | 0.333 | 0.891 ± 0.062 | 163.1 vs 108.3 |
| 4p | **0.792 ± 0.082** | 0.25 | 0.958 ± 0.040 | 262.7 vs 130.7 |

The 4p champion wins **79%** of games against the untrained weight vector at a
table where chance is 25% — better than 3x the null, and the largest margin of
the three counts. On culture it doubles the default bot. **Hill climbing is
working, and it is working best at 4p.**

### `experiments/baselines.jsonl` is stale, and it has already contaminated [`HEURISTICS.md`](HEURISTICS.md)

The file contains a block of much lower numbers for the same match-ups:

| match-up | in `baselines.jsonl` | today | champion's mean culture then → now |
|---|---|---|---|
| 2p champ vs default | 0.448 | **0.682** | 108.5 → 134.6 |
| 3p champ vs default | 0.604 | **0.771** | 124.8 → 163.1 |
| 4p champ vs default | 0.349 | **0.792** | 139.8 → **262.7** |

[`docs/HEURISTICS.md`](HEURISTICS.md) (§"How strong is the thing giving you advice?") explains
those low numbers as seed noise:

> A separate check run earlier the same morning **with different random seeds**
> scored the same match-ups much lower (2p 44.8%, 3p 60.4%, 4p 34.9%). The honest
> summary is "clearly above its starting point at 2 and 3 players, **probably at
> 4**, and nobody should quote a precise number".

**That explanation is at best incomplete, and at 4p it is wrong.** I first wrote
that it was simply wrong at all three counts; measuring it properly forced me to
soften that at 2p. What the evidence actually supports:

**Seed noise is real, and bigger than HEURISTICS claims.** Re-running 2p champion
vs default on different `--seed` values (96 games each):

| seed | win rate | champion mean culture |
|---|---|---|
| 0 | 0.682 ± 0.093 | 134.6 |
| 9000 | **0.844 ± 0.073** | 149.1 |
| 31337 | 0.708 ± 0.091 | 136.5 |
| 777 | 0.771 ± 0.085 | 146.7 |

A 16-point swing from the seed alone (0.682 → 0.844) — larger than the "±8–10 points" HEURISTICS
estimates. So at **2p**, the 23-point gap (0.448 → 0.682) is only ~1.5x the
observed seed spread and **could** be mostly seeds. My initial claim that it was
purely a stale champion was too strong at that count.

**At 4p the seed explanation cannot carry the load:**

* The gap is **44 points** (0.349 → 0.792), nearly 3x the largest seed swing
  measured (16 points at 2p) and ~7 standard errors.
* Seed noise moves both bots together and moves culture modestly (2p: 134.6 →
  149.1, +11%, when the seed changed). At 4p the *champion's* mean culture nearly
  **doubled** — 139.8 → 262.7, +88% — while the default opponent's barely moved
  (128.9 → 130.7). Getting luckier seeds does not add 123 points of culture while
  leaving your opponent where it was; being a better bot does.
* The champions demonstrably changed between the two measurements. The 4p run
  accepted mutations at gens 103, 124 and 130, and during this audit alone the 2p
  champion advanced 218 → 220. `baselines.jsonl` rows carry no generation, so an
  older, weaker bot is certainly *part* of that block.

**Concretely, these [`HEURISTICS.md`](HEURISTICS.md) claims need correcting** (that file is owned by
another agent, so this is flagged here, not edited):

1. The hedge *"clearly above its starting point at 2 and 3 players, **probably at
   4**"* — **wrong, and it is the one that matters.** 4p is not the doubtful
   count, it is the **strongest** of the three: 0.792 against a 0.25 null (3.2x
   the null) versus 1.36x at 2p and 2.3x at 3p. The user's worry that the 4p
   climb is failing is the opposite of what the data shows.
2. Attributing the low block *entirely* to *"different random seeds"* — partly
   defensible at 2p (seeds really do move it 16 points), but it cannot explain
   4p's 44-point gap and doubled culture. Both effects are present and the
   document should say it cannot separate them, because the file it is quoting
   records neither the seed nor the champion generation.
3. The table's method — *"averaged over the last four such checks"* — averages
   measurements of **different champions taken at different times** and reports
   the spread as pure noise. Those four checks are not four samples of one
   quantity; the bot changed between them. Some of that spread is signal.

The root cause is that `baselines.jsonl` has **no timestamp and no champion
generation field**, so a reader cannot tell which bot a row describes. Until it
does, do not quote it — re-run `experiments/evaluate.py`.

Acceptance history from the generation logs (all three still accepting, none has
flatlined):

| count | gens | accepted | last accept | wall clock |
|---|---|---|---|---|
| 2p | 218 | 20 (9%) | gen 213 of 218 | 5.4 h |
| 3p | 158 | 12 (8%) | gen 149 of 158 | 5.3 h |
| 4p | 138 | 8 (6%) | gen 130 of 138 | 5.3 h |

The 4p run is *not* obviously undertrained relative to the others — it has had
the same wall-clock time and its absolute strength versus the baseline is the
best of the three. What is thin at 4p is the **number of accepted steps**: 8
accepted mutations, of which the wonder decision was #2.

---

## 6. Follow-ups this audit did not do

Ordered by how much they would change what the documents say.

1. **Re-test `wonder_remaining` deliberately.** It is the only weight known to be
   worth nothing, and it drives a heuristic currently printed as advice. A
   focused hill-climb step that mutates it alone would settle it in one
   generation. More generally, the gen-5 scatter moved 19 weights on a 48-game
   acceptance test — cheap acceptance thresholds let neutral weights ride in on
   the coat-tails of useful ones and then freeze.
2. **Give `experiments/baselines.jsonl` a timestamp and a champion generation.**
   Every row is currently unattributable, which is what let a stale number become
   a published claim. The seed should be recorded too, since seeds move the
   result by up to 16 points.
3. **Fix `analysis/opening_order.py`** (owned elsewhere): the `__call__ = None`
   bug means it has never produced a number, and its `getattr`-on-a-dict card
   typing would report `"?"` for every card even once it runs.
4. **Re-check the 4p "starts 1.96 wonders, finishes 0.79" problem.** Behaviour
   data shows the 4p champion abandons ~1.2 wonders per game; since the weight
   causing it is worth nothing, that is likely pure waste rather than a trade-off.

## How to reproduce

```bash
# snapshot first -- the live hill climbs rewrite experiments/champion_*.json
cp experiments/champion_4p.json /tmp/ch4.json
cp experiments/champion_2p.json /tmp/ch2.json

# by-seat opening (stops after round 1; add --full for whole games)
python3 analysis/opening_by_seat.py --players 4 --games 400 --champion /tmp/ch4.json

# the decisive control: the 4p vector played at 2 players
python3 analysis/opening_by_seat.py --players 2 --games 400 --champion /tmp/ch4.json

# untrained control -- identical at every player count
python3 analysis/opening_by_seat.py --players 4 --games 400 --champion default

# strength vs the untrained starting point (vary --seed; it moves the answer)
python3 -m experiments.evaluate --a /tmp/ch4.json --b default --games 96 --players 4 --json
```
