# Aggressions, wars, and the 4-player colony auction

> **SUPERSEDED 2026-07-30 on its headline rates; kept for Part A and for the
> mechanism.**  Every rate in this document ("aggressions 0.00 at 2p/3p, 0.11 at
> 4p; wars 0.00 everywhere") is a **1-ply `WeightedBot` measurement**, and
> `docs/AGGRESSION_RATE.md` shows that is an artefact of the evaluation horizon,
> not a fact about the game.  Under the search the league actually trains
> (`plan:width=2`) the real rates are **0.303 / 0.870 / 3.997 aggressions per
> game** and **~1.05 / 2.23 / 7.50 wars per game** at 2p/3p/4p.  Read rates from
> `docs/AGGRESSION_RATE.md` and current behaviour from
> `docs/SYSTEM_COVERAGE.md`.
>
> What survives and is not restated anywhere else: **Part A's refutation** — the
> "4p colony auctions never start because events are not seeded" hypothesis is
> wrong; Age A seeding is correct (2p:4 / 3p:5 / 4p:6) and territories are
> correctly seeded by `prepare_event`.  The real cause was that the 2p and 4p
> champions owned essentially zero military units in play (2p 0.00/player, 4p
> 0.07/player, against 3p's 2.00), and colonization requires sacrificing at least
> one unit (RULES_SPEC §11.3), so those seats were excluded from every auction at
> the door.  That is a training/weights problem, not an engine bug, and it is
> self-reinforcing under a 1-ply search.  The mechanism diagnosis in Part B
> (payoff lands in another player's decision, therefore invisible at 1 ply) is
> also correct and is the seed of `docs/AGGRESSION_RATE.md`'s analysis.
>
> The dangling "see the next section for the implementation and the A/B result"
> at the end of this file was never written here; it landed in
> `docs/PLAN_WAR_LOOKAHEAD.md` and `docs/AGGRESSION_RATE.md`.

Date: 2026-07-26
Owner: aggression-fix agent (`engine/bots/`, colony/military paths of
`engine/actions.py`, this file)

Follow-up to `docs/PACTS_DIAGNOSIS.md`.
Two open items from the post-fix measurement:

* **A.** colony bids at 4p went 0.02 -> 0.01 (essentially never), with the
  hypothesis "4p auctions never start because events aren't seeded";
* **B.** aggressions read 0.00 at 2p/3p and 0.11 at 4p, wars 0.00
  everywhere, i.e. the war/aggression layer never fired.

Both were reproduced before any code was touched. **A's hypothesis is
refuted** — the events *are* seeded and the auctions *do* start. B is
confirmed, with a precise mechanism and a measured size.

---

## A. 4-player colony auctions: hypothesis REFUTED

### What was measured

8 mirror 4p self-play games with `experiments/champion_4p.json`
(seeds 900000-900007), instrumenting `events.reveal_current_event` and
`interact.start_auction`; same for 3p as a control.

| quantity (per game) | 3p | 4p |
|---|---|---|
| Age A current events seeded at setup | 5 | **6** |
| ...of which are territories | 0 | 0 (correct, §1.6) |
| `prepare_event` reveals | 75.5 | **121.6** |
| territories revealed | 7.88 | **2.38** |
| **auctions started** | 7.88 | **2.38** |
| auctions with **0** eligible bidders | 0.38 | **1.75** |
| auctions with 1 eligible bidder | 0.38 | 0.62 |
| auctions with 2+ eligible bidders | 7.12 | **0.00** |
| colonies gained (all seats) | 1.88 | 0.12 |

So at 4p the auction fires 19 times in 8 games and **14 of those 19 die at
`interact.start_auction` with "nobody can colonize"** — before any bot ever
gets a decision. The remaining 5 have exactly one eligible bidder.

### The real cause: the 2p and 4p champions own no military units

`start_auction` (`engine/interact.py:508-519`) builds its bidder list from
`max_force(state, p) > 0`, and `max_force` returns 0 when `unit_pool(p)` is
empty. That is correct rules: §11.3 requires sacrificing **at least one
military unit**, "even if other bonuses would cover the bid". A player with
no units in play genuinely cannot colonize.

Measured mean military units in play per player (sampled every 200 moves,
4 games per count, champion mirror):

| | 2p | 3p | 4p |
|---|---|---|---|
| units in play / player | **0.00** | 2.00 | **0.07** |
| players with zero units | **100%** | 25% | **92.5%** |
| max colonization force | 0.00 | 4.00 | 0.28 |
| strength | 1.50 | 4.25 | 1.77 |
| `unit_workers` weight | 0.051 | 0.063 | 0.132 |
| `colonies` weight | 3.311 | 1.443 | **-0.962** |

The 2p and 4p champions simply never put a worker on a military unit, so
they are excluded from every auction at the door. The 3p champion does,
and its auctions work (7.9 started, 1.9 colonies per game).

### Verdict

**Not an engine bug, and not the seeding.** `state.current_events =
age_a[:num_players + 2]` is exactly §1.6 (2p:4, 3p:5, 4p:6) and the Age A
military deck is all events by design, so no territory can *or should* be
in the setup deck at any player count; territories arrive later via
`prepare_event` -> `future_events` -> recycle, and they demonstrably do.
The engine paths are right; the 4p champion's army is the blocker.

This is self-reinforcing and cannot be fixed by pricing a deferred payoff:
a 1-ply search evaluating "put a worker on Warriors" cannot see that the
unit is what buys a colony ten turns later, and the 4p `colonies` weight
(-0.962) is pure hill-climb drift on a feature that fired 0.02 times per
game. The fix is a **weights/training** one — reset `colonies` (already
done in `15b9764`) and re-run the climb now that `auction_committed` makes
bids visible — not a code one. Recorded here; the climb restart is the
main agent's call.

---

## B. Aggressions and wars: confirmed, and it is the 1-ply horizon

### The moves are legal constantly and are essentially never taken

6 mirror games per player count, champion mirror, every politics decision
instrumented:

| per game | 2p | 3p | 4p |
|---|---|---|---|
| politics decisions | 42.0 | 54.8 | 85.3 |
| holding an aggression card | 25.0 | 23.3 | 34.5 |
| **`aggression` in `legal_moves`** | **12.3** | **11.2** | **21.3** |
| **`war` in `legal_moves`** | **13.7** | **6.7** | **15.2** |
| aggressions chosen (all seats) | 0.17 | 0.00 | 1.17 |
| wars chosen (all seats) | **0.00** | **0.00** | **0.00** |

(The behaviour harness's 0.00/0.11 numbers count one *champion seat*, not
the table — `experiments/behaviour.py`'s Watcher wraps a single bot — so
the 2p/3p zeros are the same near-zero rate as the baseline seen through a
smaller window, **not a regression**. Wars are a true structural zero.)

### Why: the payoff lands in the defender's decision

`_h_aggression` (`engine/actions.py:972`) -> `events.start_aggression`
spends the military action, discards the card, and hands a `defense`
decision to the target (`engine/interact.py:601`). The trial state the
attacker evaluates therefore shows **only the cost**. Direct probe, best
attack vs the chosen move in the same position:

```
2p  best ('prepare_event','Rebellion')  69.936 | best_attack ('aggression','Plunder (I)',0)  68.274 | pol_pass 68.504
4p  best ('pol_pass',)                  64.253 | best_attack ('aggression','Enslave',0)      62.578 | pol_pass 64.253
3p  best ('prepare_event','Impact of Industry') 134.220 | best_attack ('war','War over Culture',1) 125.399 | pol_pass 126.417
```

In every one of 357 sampled positions across the three player counts the
best attack scored **below `pol_pass`**, usually by 0.2-5 points — the
constant hand/military-action penalty, exactly as predicted for pacts.

Wars are worse still: `_h_war` (`actions.py:1036`) only writes
`war_declared_by_me` / `wars_declared_on_me`, and the spoils are taken a
whole turn later in `events.resolve_war` (`events.py:557`, called from
`game.start_turn`). No feature in `weighted.features()` read either field,
so declaring war was *literally* a pure cost with no representable
benefit — hence exactly 0.00 wars, forever, at every player count.

### Fix (same shape as the pact/colony fix, `166867d`)

See the next section for the implementation and the A/B result.
