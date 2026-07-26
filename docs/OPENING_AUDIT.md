# Opening audit: is the 4p "wonder first" opening real?

**Question.** `docs/HEURISTICS.md` says 2p and 3p champions open round 1 by taking
an action card while the 4p champion takes a wonder. Is that real strategy, a
reporting artefact of how we aggregate seats, or undertrained noise?

**Status: IN PROGRESS** — findings are written here as they land. Verdict at the
bottom.

Owned by this audit: `analysis/opening_by_seat.py`, this file. Everything under
`experiments/` and `engine/` was read-only for this work.

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

The 120/120 figure and the p10/p25/p75 language in HEURISTICS.md match the
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

The rules argument in HEURISTICS.md is that at 4p the row sweeps only 1 card per
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
The rules rationale printed in HEURISTICS.md — sweep speed, cost bands,
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
optimality. Whether it is actually good is a separate question (below).

---

## 5. Is the hill climb working at all?

From `experiments/baselines.jsonl` (champion vs the untrained `default` weight
vector, challenger rotated through every seat, null = 1/players):

| count | champion vs `default` | null | verdict |
|---|---|---|---|
| 2p | 0.448 ± 0.099 | 0.50 | **not better than untrained** |
| 3p | 0.604 ± 0.097 | 0.333 | clearly better |
| 4p | 0.349 ± 0.095 | 0.25 | marginally better (CI low ≈ 0.254) |

Being re-measured fresh here. Acceptance history from the generation logs:

| count | gens | accepted | last accept |
|---|---|---|---|
| 2p | 218 | 20 (9%) | gen 213 |
| 3p | 158 | 12 (8%) | gen 149 |
| 4p | 138 | 8 (6%) | gen 130 |
