# Deeper search: resolving the pending stack before evaluating

Status: **in progress** — design and cost are measured, the A/B is running.
Written incrementally so a restart loses nothing.

## 1. The defect this is aimed at

`docs/PACTS_DIAGNOSIS.md` proves it for pacts and colony bids; the argument
generalises. `WeightedBot.pick` scores a candidate by

```python
trial = copy_state(state)
actions.apply(trial, mv, rng)
val = evaluate(trial, idx, w, ctx)
```

That is correct only if the move's whole effect has landed in `trial` by the
time `apply` returns. For a whole class of TTA moves it has not, because
`apply` stopped at a decision it is not allowed to make:

| move | what `apply` leaves in the trial state | where the payoff actually is |
|---|---|---|
| `offer_pact` | card gone from hand, `choice` pending for the partner | the pact object, created in `interact._c_pact_offer` when the partner accepts |
| aggression | military card gone, MA spent, `defense` pending for the defender | `events.finish_aggression` — loot, raid, annex, culture |
| `bid` | one integer changed in the auction dict | `interact.colonize` — the colony, and its price in sacrificed units |
| action card | card gone, `free_civil` choice pending for the mover | `actions.apply_free_action` + the card's gains |

In every row the trial state shows **the entire cost and none of the gain**.
So the move is worse than passing under *any* weight vector — it is strictly
dominated, not merely underweighted. No amount of hill climbing can select for
a move that can never rank first, which is why pacts were offered 0 times in
240 games, wars 0.00 per game, aggressions ~0, and colony bids ~0 at 4p.

The existing fix (`weighted.deferred_credit`, commit 166867d) hand-prices those
payoffs: `PACT_OFFER_CREDIT = 0.5`, an auction share of `1/(1+rivals)`, and two
dedicated weights `auction_committed` / `auction_bid`. It works, but it is a
per-move-type special case, it has to be re-derived every time the engine grows
a new deferred effect, and it is priced by hand rather than by the rules.

## 2. Design

Three options were on the table.

1. **Full 2-ply over the opponent's reply.** A TTA turn is a politics phase
   plus up to ~4-6 civil actions with a branching factor around 30, so one
   opponent reply is ~30^5 leaves. Not affordable, and it does not even
   target the defect: the payoff we are blind to is not in the opponent's
   *turn*, it is in the opponent's *pending decision*, which happens inside
   our own move.
2. **Shallow expectimax over the decision stack.** Correct in principle, but
   it needs a probability model for opponent choices that we do not have, and
   in self-play the opponent's policy is known exactly.
3. **Resolve my own pending stack to quiescence, then evaluate.** Chosen.

This is the game-tree analogue of quiescence search: never evaluate a position
while a decision is still hanging. After applying a candidate move, keep
resolving `state.pending` — whoever the decider is, rivals included — until the
stack is empty, and only then call `evaluate`. `interact.run_queue` drains
`state.queue` whenever the stack empties, so "pending is empty" is exactly the
quiet position, and the FIFO/LIFO machinery needs no special handling.

It targets the defect precisely: **resolving through pending decisions is what
matters, not raw depth.** Every row of the table above becomes visible, and it
becomes visible *by playing the rules out*, not by pricing it.

### Opponent model

A rival's pending decision is resolved with a plain 1-ply weighted pick
maximising **that rival's own** evaluation — i.e. the current champion. In
self-play that is not an approximation of the opponent, it *is* the opponent,
so the line the search reads is the line that will really be played. The inner
pick is never recursive, which bounds the cost at one extra level.

### Budgets, and what a miss degrades to

`MAX_DEPTH` (12) pending decisions per candidate, `MAX_NODES` (600) total
quiescence `apply` calls per root decision. On exhaustion the stack is left as
it stands and the position is scored as-is — which is exactly the existing
`deferred_credit` path, because `weighted.features` only applies the credit
`if state.pending`. So a budget miss degrades to today's behaviour, not to
nonsense.

The converse is the load-bearing observation for the "remove the hand-priced
patches" question: **when quiescence completes, `state.pending` is empty, so
`deferred_credit` contributes exactly zero.** At a measured 0% truncation the
quiescent bot is already running with the hand-priced credit as dead code. Any
attacking behaviour it shows is therefore emergent, not priced.

### War is a separate problem

A war declaration pushes nothing onto the stack. It sets
`p.war_declared_by_me` in the politics phase and resolves at the start of the
declarer's *next* turn, in `game.start_turn -> events.resolve_war` — a full
round away. Quiescence cannot reach it and neither could any affordable depth.

`events.resolve_war` is, however, a pure deterministic function of the two
players' current strengths and consumes no rng. So `QuiescentBot` scores a
`war` candidate by calling the engine's own `resolve_war` on a scratch copy.
That is a lookahead, not a hand-priced weight: the number it produces is the
spoils the engine itself would award. It is optimistic — the defender gets a
turn in between to build strength — and it is flagged separately
(`WAR_LOOKAHEAD`) so the A/B can attribute its effect.

### Why the undo stack (docs/PYPY.md §6) is not needed here

§6 recommends Design A, a journalling `apply` with rollback, for ~1.8x. The
`journal-undo` branch has `engine/journal.py`, `engine/statediff.py`, the
paranoid differ and 26 tests, but **no call site converted** — the remaining
work is converting ~470 mutation sites across six modules, which §6.5 itself
identifies as the whole risk.

That work is not on the critical path for this prototype, because the measured
cost of quiescence turned out to be small (section 3). §6.2 also notes an undo
stack cannot hold many trial states at once; that is irrelevant here, since
quiescence is strictly depth-first make/unmake, but so is the 1-ply search it
would also serve. The journal remains worth finishing on its own merits — it
speeds up 1-ply and quiescent search alike — but it is not a precondition.

## 3. Cost

`tools/quiesce_bench.py` plays the same seeds with both bots and the same
weights, timing with `time.process_time` (the hill climbs saturate the box, so
wall clock is meaningless).

RESULTS PENDING — see section 6.

## 4. Strength A/B

RESULTS PENDING.

## 5. Behaviour counts

RESULTS PENDING.

## 6. GO / NO-GO

RESULTS PENDING.
