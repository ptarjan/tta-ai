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

_(table pending — runs in flight)_

---

## 5. Is the hill climb working at all? (preliminary)

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
