# Deeper search: resolving the pending stack before evaluating

Status: **COMPLETE.** Design, cost, strength and behaviour are all measured.
Recommendation in section 6. Written incrementally so a restart loses nothing.

One correction to section 1 up front, because the rest of the document rests
on it: the claim "an action card is strictly dominated" is **too strong**.
Only **18 of the 33 action cards** carry a `freeCivilAction`, i.e. order an
action and enqueue a decision; the other 15 resolve entirely inside `apply`
and were never invisible to a 1-ply search. The measured behaviour counts in
section 5 split the two. Everything else in section 1 held up under direct
measurement — see section 4.0.

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

```
nice -n 15 python3 tools/quiesce_bench.py --players N --games 8 \
    --weights experiments/champion_Np.json
```

| players | 1-ply s/game | quiescent s/game | **cost ratio** | candidates needing quiescence | extra `apply` per candidate | truncated |
|---|---|---|---|---|---|---|
| 2p | 0.468 | 0.544 | **1.16x** | 2.67% | 0.035 | 0.0% |
| 3p | 0.827 | 1.066 | **1.29x** | 3.77% | 0.069 | 1.9% |
| 4p | 1.924 | 2.265 | **1.18x** | 4.05% | 0.047 | 0.0% |

This is the result that decides the whole question, and it is much better than
the brief assumed. Deeper search normally costs a branching-factor multiple per
move. Quiescence does not, because **it only runs on the 3-4% of candidate
moves that actually leave a decision hanging**. Build, take, pop, upgrade and
end_turn — the overwhelming majority — resolve inside `apply` and cost exactly
what they cost today. The average candidate pays for 0.03-0.07 extra `apply`
calls, so the whole feature is a ~20% tax rather than a 5-30x one.

Truncation (a budget running out before the position went quiet) is 0% at 2p
and 4p and 1.9% at 3p, so the fallback path is essentially never taken and the
`MAX_DEPTH`/`MAX_NODES` budgets are not binding at their current values.

### Cost of `LEVELS = 2`

Nested resolution costs ~5% on top of `LEVELS = 1` (2.97 vs 2.82 cpu-s on a
sampled 4p game), for the same reason: the extra level also only fires on
pending-creating candidates, of which there are few. It is nonetheless not
worth taking — see section 4.5, where it measured *weaker*.

### 3.1 Correction: the table above is optimistic in two ways

Re-measured after the strength A/B, and the headline "~20% tax" does not
survive. `time.process_time`, n=24 games, same tool:

| | 1-ply s/game | quiescent s/game | ratio | quiesce rate |
|---|---|---|---|---|
| 2p, champion weights, `TTA_JOURNAL=0` | 0.416 | 0.520 | **1.25x** | 1.83% |
| 2p, champion weights, `TTA_JOURNAL=1` | 0.337 | 0.556 | **1.65x** | 1.83% |
| 4p, default weights, `TTA_JOURNAL=0` | 1.157 | 1.916 | **1.66x** | 9.76% |
| 4p, default weights, `TTA_JOURNAL=1` | 0.761 | 2.016 | **2.65x** | 9.76% |

Two separate effects, and both push the same way.

1. **The journal only speeds up the bot that can use it.**
   `experiments/run_league.sh` now exports `TTA_JOURNAL=1`, which buys
   `WeightedBot` 1.2–1.5x here. `journal.install()` is lazy and
   `QuiescentBot` never calls `journal.begin` — it holds several live trial
   states at once and must stay on `copy_state` (docs/PYPY.md 9.15) — so it
   gets none of it. **In the trainer's actual configuration the ratio is 1.65x
   at 2p and 2.65x at 4p, not 1.2x.** This is the honest number for any
   budgeting decision.
2. **The cost depends on the weight vector, not just the table size.** The
   original 4p row used the then-champion and saw 4.05% of candidates leave
   something pending; the default vector sees 9.76%. A bot that attacks more
   pays more, so a *trained* quiescent champion would pay more than either
   figure, not less.

The qualitative conclusion of section 3 survives — this is still a small
constant factor rather than the branching-factor multiple a real ply costs,
and truncation is 0.0% everywhere so the budgets are still not binding. But
"a ~20% tax" was wrong; call it **1.6x–2.7x under the trainer's own flags**.

## 4. Strength A/B

### 4.0 First: the defect itself, measured rather than argued

Section 1 is an *argument* that these moves can never rank first. Before
measuring a win rate it is worth measuring the argument.
`tools/dominance_probe.py` walks a self-play game and, at every decision where
one of the watched move kinds is legal, scores that candidate two ways — the
way `WeightedBot` scores it, and the way `QuiescentBot` scores it — and asks
whether it would have beaten the best of the other candidates. 2p, 4 games,
`champion_2p` weights:

| move kind | legal at N decisions | leaves a decision pending | ranked **first** at 1 ply | ranked **first** after quiescence |
|---|---|---|---|---|
| `aggression` | 72 | 40% | **0** | **23** |
| `play_action` | 487 | 23% | 25 | 82 |
| `war` | 40 | **0%** | 0 | 0 |

That is the mechanism, in the bot's own numbers. An aggression is never the
best-scoring move under the champion's own weight vector, and after the
defender's pending decision is resolved it is the best-scoring move a third
of the time. Note also the two rows that qualify section 1: `play_action` is
only 23% deferred (the 18-of-33 correction above), and `war` leaves **nothing**
pending — 0.0% — which is the design note "war is a separate problem" measured
rather than asserted.

### 4.1 Method

`experiments/arena.duel` puts one challenger at a table of defenders and
rotates it through every seat, so a *seed group* is one game seed played P
times, once per seat. The challenger is `QuiescentBot`, the defenders are
`WeightedBot`, **and both carry the identical weight vector** — this is a
search-only A/B with nothing else varying.

The control arm is the 1-ply bot challenging a table of itself. It is not
really a measurement: with deterministic identical bots the game does not
depend on which seat is *labelled* the challenger, so in every seed group the
challenger takes exactly 1/P of the win and exactly 0.0 of the culture margin.
That was checked rather than assumed — all 400 2p groups and all 267 3p groups
came out at exactly 1/P and exactly 0.0, to the last bit — and the 4p control
was then skipped as a provable identity, and its budget spent on the treatment
arm instead.

So the pairing is **at the seed-group level**: the reported interval is the
interval of the group means, which removes seat-assignment variance entirely.
n is reported both ways.

Weights: 2p uses the live league champion (`league_state/champion_2p.json`,
gen 337, read-only copy). **3p and 4p use the DEFAULT weight vector**, because
those arms restarted clean and their champions are 4 and 8 generations old —
5 and 21 of 82 weights differ from default. That is stated because it matters:
the 3p/4p A/B is quiescence against an *untrained* baseline, and the untrained
baseline is bad at things quiescence does not fix (section 5).

All runs `TTA_JOURNAL=1`, `nice -n 15`, at most 2 worker processes, on a box
also running three live trainers.

### 4.2 Results

Δ is against the control, paired by seed group; 95% CIs throughout.

| players | variant | n games | seed groups | win rate | null | **Δ win (paired)** | **Δ culture margin** | errors |
|---|---|---|---|---|---|---|---|---|
| 2 | LEVELS=1 | 800 | 400 | 55.8% ± 3.0% | 50.0% | **+5.8% ± 3.0%** | **+9.96 ± 3.31** | 0 |
| 2 | LEVELS=1, no war lookahead | 800 | 400 | 53.6% ± 2.9% | 50.0% | **+3.6% ± 2.9%** | +5.54 ± 3.08 | 0 |
| 2 | LEVELS=2 | 800 | 400 | 53.7% ± 2.9% | 50.0% | **+3.7% ± 2.9%** | +7.08 ± 3.14 | 0 |
| 3 | LEVELS=1 | 801 | 267 | 42.8% ± 3.0% | 33.3% | **+9.5% ± 3.0%** | **+10.09 ± 2.12** | 0 |
| 3 | LEVELS=1, no war lookahead | 801 | 267 | 39.3% ± 3.0% | 33.3% | **+6.0% ± 3.0%** | +5.48 ± 2.03 | 0 |
| 4 | LEVELS=1 | 800 | 200 | 41.7% ± 3.0% | 25.0% | **+16.7% ± 3.0%** | **+20.08 ± 2.86** | 0 |

**QuiescentBot is stronger, at every table size, well outside the interval,
and the effect grows with the number of players** — +5.8pp at 2p, +9.5pp at 3p,
+16.7pp at 4p, i.e. 1.12x, 1.28x and **1.67x par**. That ordering is what the
mechanism predicts: more players means more rivals with pending decisions to
resolve, more aggression targets, and (at 3p/4p) the pact and colony layers
existing at all. Zero engine errors in the 4,802 games of this table.

The 4p number carries the caveat from 4.1 doubled: it is quiescence against
the *default* vector, and section 5.1 shows that vector wastes three civil
actions a turn. Read +16.7pp as "quiescence is worth a lot at a full table",
not as a prediction for a trained 4p champion.

### 4.3 The two things it bundles, separated

`WAR_LOOKAHEAD` is not quiescence — a war declaration pushes nothing onto the
stack, so quiescence provably cannot see one, and the lookahead is a separate
mechanism that calls `events.resolve_war` on a scratch copy. Turning it off
and re-running the same seeds splits the 2p result:

| component | Δ win at 2p | Δ win at 3p |
|---|---|---|
| quiescence proper | **+3.6% ± 2.9%** | **+6.0% ± 3.0%** |
| war lookahead, on top | **+2.2% ± 1.8%** | **+3.5% ± 2.4%** |
| both | +5.8% ± 3.0% | +9.5% ± 3.0% |

(the "on top" row is paired directly against the LEVELS=1 arm on the same seed
groups, which is why its interval is tighter than the difference of the other
two.) Roughly a 60/40 split at both table sizes, and section 5 shows the two
are behaviourally disjoint: the lookahead is responsible for **all** of the
wars and **none** of the aggressions.

### 4.4 Cross-check against opponents that are not 1-ply searchers

A win over a mirror of yourself is the weakest kind of evidence, and the
biggest reservation about this result (6.2, point 2) is that `_pick` models a
rival's pending decision with a 1-ply pick — which is *exactly right* when the
rival is a `WeightedBot`, and might be where the whole gain comes from. The
test is to put the same two bots against opponents whose policy the model gets
completely wrong: `BookBot v2` and `CultureBot` are rule lists with no
evaluator and no search at all.

2p, 400 games each, same seeds, paired by seed group:

| challenger | vs `CultureBot` | vs `BookBot v2` |
|---|---|---|
| 1-ply champion | 51.6% ± 5.1% | 72.1% ± 4.8% |
| quiescent champion | 57.9% ± 4.8% | 78.0% ± 4.2% |
| **paired Δ** | **+6.2% ± 5.1%** | **+5.9% ± 4.5%** |
| paired Δ culture | +10.20 ± 5.75 | +14.84 ± 6.82 |

**The gain replicates, at the same size, against opponents the search models
incorrectly** (+6.2 and +5.9 against +5.8 in the mirror). The `CultureBot`
interval only just clears zero and should not be quoted alone; the two
together, plus the mirror, are the claim. This is the single most reassuring
number in the document, because it says the +5.8% is not an artefact of the
opponent model happening to be exact.

### 4.5 LEVELS=2 is not worth it

Section 3 measured LEVELS=2 at only ~5% more CPU and this document assumed it
would be at least as strong. It is not. Paired on the same 400 seed groups:

    2p   LEVELS=2 − LEVELS=1:   win −2.1% ± 1.9%   culture −2.89 ± 1.83

**LEVELS=2 is weaker than LEVELS=1**, by a small but interval-clearing margin.
The explanation is straightforward once stated, and it is a general fact about
opponent modelling rather than a bug: at LEVELS=1 the rival's pending decision
is answered with a plain 1-ply pick, which *is exactly what the rival will
really do*, because the rival is a `WeightedBot`. At LEVELS=2 the rival's
decision is itself resolved to quiescence, i.e. the search models the opponent
as **stronger than he actually is**, and plans against a player who is not at
the table. A more accurate opponent model would only help if the opponents were
themselves quiescent. Use LEVELS=1.

## 5. Behaviour counts — the mechanism check

`tools/behaviour_counts.py`, extended here to count `play_action` (missing
before), to split action cards into the 18 that order a free action and the 15
that do not, and to record the `docs/WASTED_ACTIONS.md` civil-action numbers.
**Mirror tables**: every seat runs the same bot, 120 games per cell, so these
are "what this search does when everyone uses it", not "what it does against a
field it out-searches".

Same weights as the A/B (2p: league champion; 3p/4p: default).

| per game | 2p 1-ply | 2p quiescent | 3p 1-ply | 3p quiescent | 4p 1-ply | 4p quiescent |
|---|---|---|---|---|---|---|
| **wars declared** | **0.000** | **1.433** | **0.000** | **2.625** | **0.000** | **7.550** |
| **aggressions** | 0.175 | **1.883** | 0.017 | **0.842** | 0.042 | **3.258** |
| pact offers | 0.000 † | 0.000 † | 6.375 | 2.892 | 16.742 | 4.825 |
| colony bids | 0.167 | 0.125 | 3.733 | 2.567 | 6.033 | 4.025 |
| action cards, *ordered* (deferred) | 1.725 | **2.842** | 0.658 | **1.592** | 1.650 | **3.267** |
| action cards, immediate | 5.342 | 5.817 | 2.917 | 3.183 | 4.667 | 4.608 |
| cards taken | 43.03 | 44.05 | 31.03 | 34.23 | 49.61 | 52.16 |
| colonies held at end | 0.142 | 0.125 | 1.708 | 1.967 | 2.725 | 3.017 |
| pacts live at end | 0.000 † | 0.000 † | 1.217 | 1.108 | 1.767 | 1.875 |
| turns per game | 38.02 | 37.87 | 69.17 | 68.15 | 122.57 | 119.73 |

† Pacts are removed from the deck in a 2-player game (`actions.py:258`,
`data/cards_military_actions.json`). The 2p zero is correct and is not
evidence of anything.

The `war=0` ablation arm, same cells at 2p, is what makes the attribution
below possible:

| 2p per game | 1-ply | quiescent | quiescent, **no war lookahead** |
|---|---|---|---|
| wars | 0.000 | 1.433 | **0.000** |
| aggressions | 0.175 | 1.883 | **2.050** |
| ordered action cards | 1.725 | 2.842 | 2.742 |

**How much of this is noise.** 120 games x P seats, so a rate of r per game
rests on ~120r events and has a Poisson standard error of about
sqrt(120r)/120. The war and aggression rows are 10-25 sigma and are not in
question. The pact-offer and colony-bid falls are 8-20 sigma. The
`colonies held at end` rise (1.708 -> 1.967 at 3p, 2.725 -> 3.017 at 4p) is
only about 1.5-2 sigma and is **suggestive, not established** -- it is quoted
as the direction that makes the bid story coherent, not as a result on its
own. The civil-action rows in 5.1 rest on thousands of turns each and their
differences are ~4 sigma but small in absolute terms.

**The counts move, and they move where the theory said they would.**

* **Wars: exactly 0.00 → 1.43 / 2.63 / 7.55 per game.** This is entirely the
  war lookahead: the `war=0` ablation at 2p has quiescence fully on and still
  declares **0.000** wars per game. Quiescence cannot reach a war, exactly as
  designed, and the lookahead is what unlocks it.
* **Aggressions: 0.02–0.18 → 0.84–3.26 per game**, a 10x to 78x move. This is
  entirely quiescence: the `war=0` ablation at 2p declares **2.05**
  aggressions per game, i.e. slightly *more* than with the lookahead on,
  because it is not spending its military cards on wars instead.
* **Deferred action cards roughly double** (1.725 → 2.842 at 2p, 0.658 →
  1.592 at 3p, 1.650 → 3.267 at 4p) while the *immediate* ones barely move
  (5.34 → 5.82, 2.92 → 3.18, 4.67 → 4.61). That contrast is the sharpest
  single confirmation in the document: the split was drawn on a rules
  property (does this card carry `freeCivilAction`) with no reference to the
  bot, and the bot's behaviour changed on exactly the deferred side.

**And two counts move the other way, which is the more interesting result.**
Pact offers *fall* (6.4 → 2.9 at 3p, 16.7 → 4.8 at 4p) and colony bids *fall*
(3.7 → 2.6, 6.0 → 4.0) — while pacts live at the end are flat and colonies
actually held **rise** (1.71 → 1.97, 2.73 → 3.02). The 1-ply bot is not
failing to offer pacts and bid: `weighted.deferred_credit` (commit 166867d)
already fixed that with a hand-priced constant, and `docs/PACTS_DIAGNOSIS.md`
recorded the fix working. What a flat hand-priced credit cannot do is tell a
*good* offer from a bad one, so it produces a lot of them. Quiescence plays the
auction out and the partner's accept/refuse out, so it makes **fewer and better
attempts**: fewer bids, more colonies. This is the expected signature of
replacing a priced constant with a playout, and it is the strongest available
evidence that the hand-priced patches can now be retired rather than merely
bypassed.

### 5.1 What quiescence does NOT fix: wasted civil actions

Asked directly, because the live 2p champion has driven `civil_actions`,
`ca_left` and `uprising` to exactly 0.0 against the weight guard's clamp, and
carries `end_turn_bias = −14.44` against a default of −3.0 — i.e. the trainer
has spent a lot of its search budget building a bigger and bigger correction
for `end_turn`'s production flattery (`engine/bots/weighted.py`, the DO-NOT-FIX
block; `docs/WASTED_ACTIONS.md` §6). If quiescence removed that asymmetry the
correction should become unnecessary.

| | 2p 1-ply | 2p quiescent | 3p 1-ply | 3p quiescent | 4p 1-ply | 4p quiescent |
|---|---|---|---|---|---|---|
| turns ending with CA unspent | 11.0% | 8.5% | 70.5% | 68.7% | 75.4% | 73.8% |
| CA wasted per turn | 0.391 | 0.305 | 2.811 | 2.732 | 3.100 | 3.024 |
| civil actions spent per turn | 2.828 | 2.864 | 1.039 | 1.119 | 0.939 | 1.006 |

**It does not fix it.** The waste falls by about a fifth at 2p and by ~2% at
3p/4p, which is consistent with the extra ordered action cards handing back
free actions rather than with the asymmetry going away. And it should not fix
it, for a reason worth writing down because it generalises:

> `end_turn`'s payoff is not deferred to a *pending decision*. It lands inside
> `apply`, in the production phase, at the moment the move is made. Quiescence
> drains `state.pending`; it has no opinion about a move that collects its
> reward early.

These are two halves of one architectural defect, in opposite directions.
1-ply evaluation compares states at **inconsistent points in the decision
timeline**: `offer_pact` / `aggression` / `bid` / an ordered action card show
all cost and no gain, so they are never chosen; `end_turn` shows a whole extra
production phase of gain that its alternatives do not get, so it is chosen too
readily. Quiescence is a fix for the first half only.

The `end_turn_bias` hack must therefore stay, and the fact that the 2p champion
has pushed it to −14.44 remains what it was: a trained weight doing the job of
a missing search property. Note also that removing it has already been measured
twice and made the bot *much* weaker (38.4% / 29.8% / 11.0% against a 50% null,
`docs/WASTED_ACTIONS.md` §6), so the correction is load-bearing and is also
acting as a move-quality filter. Nothing here contradicts that; quiescence
simply operates on a different axis.

The 3p/4p rows carry their own warning, and it is the reason the 3p/4p A/B
should be read as a lower bound on the baseline's quality rather than a strong
claim about quiescence: the **default** weight vector wastes 2.8–3.1 civil
actions per turn and ends 70–75% of its turns with actions unspent, against
0.39 and 11% for the trained 2p champion. Most of what 337 generations of hill
climbing bought is the discipline to stop passing.

## 6. GO / NO-GO

There are two separable decisions and they get different answers.

### 6.1 Add QuiescentBot to the training POOL — **GO**, unreserved

`experiments/hillclimb_pool.py:483` already builds it (`--with-quiescent`,
default weights), and none of the three live arms passes the flag. It is one
word on the command line, it costs ~1/N of one pool slot, it changes nothing
about what the champion *is*, and the behaviour counts say it is the only
opponent in the pool that will ever declare a war or press an aggression. The
current champions have been trained in a game with no military layer; that is
the cheapest available fix. Do this first, and do it at the next natural
supervisor restart rather than by killing a running arm.

### 6.2 Make QuiescentBot the trainer's CHALLENGER — **conditional GO**, not yet

The evidence for: stronger at 2p (+5.8%), 3p (+9.5%) and 4p (+16.7%) on
n = 800/801/800 with zero engine errors; the mechanism is confirmed
independently of the win rate (section 5); and — the load-bearing one — the
gain **replicates at the same size against two rule-list opponents the search
models incorrectly** (+6.2% and +5.9%, section 4.4), so it is not an artefact
of the mirror.

The reservations, in order of seriousness:

1. **Cost, restated honestly.** Section 3.1: under the trainer's own
   `TTA_JOURNAL=1` the ratio is **1.65x at 2p and 2.65x at 4p**, not the 1.2x
   the original section 3 advertised, because the journal accelerates only
   `WeightedBot`. A 2.65x slower 4p arm is a real budget decision, not a
   rounding error, and the arm that would pay it most is the one furthest from
   convergence.
2. **The opponent model is wrong in a league and the size of that is
   unmeasured.** `_pick` answers a rival's pending decision with a 1-ply pick
   at the *challenger's own* weights. Against a `WeightedBot` field that is
   exactly right; against the league's pool of rule bots and past champions it
   is not. Section 4.5 shows this matters in principle — making the model
   *stronger* (LEVELS=2) made the bot *weaker* — and section 4.4 shows it does
   not destroy the gain in practice. What is still unmeasured is
   quiescent-vs-quiescent, which a win rate cannot measure at all (it is a
   mirror); it needs a full league arm scored on the external roster.
3. **It reads hidden information.** Resolving a defender's `defense` decision
   requires the defender's real `hand_military`, which is hidden in the real
   game. `WeightedBot` already leaks (applying `end_turn` reveals the true next
   cards off the deck), but quiescence leaks more and leaks it deliberately.
   For a symmetric self-play trainer this is defensible; for any claim about
   play against a human it is not.
4. **It invalidates the current champions.** The trainer would be climbing a
   different objective. `deferred_credit`'s `PACT_OFFER_CREDIT`,
   `auction_committed` and `auction_bid` become dead code whenever quiescence
   completes (measured truncation 0.0%/1.9%/0.0%), so those three weights stop
   being selected on at all and their current values become noise.

**Recommendation: pool now (6.1), challenger later**, gated on one experiment
that has not been run: a league arm whose challenger *and* whose mirror
opponent are both quiescent, measured against the existing 1-ply league on the
external roster of `docs/BOT_ROSTER.md` rather than against itself. Until that
exists, switching the challenger would change what three live arms train
against on the strength of a number measured in a setting the change destroys.

### 6.3 Bugs found in QuiescentBot

**None.** It had no tests at all before this branch; `tests/test_quiescent.py`
adds six, and all six passed first time:

* the search never mutates the live state (`statediff` clean on 20 sampled
  mid-game positions),
* `LEVELS=0` with the war lookahead off returns **exactly** `WeightedBot`'s
  move, position for position — which also pins that the two bots' separate
  trial-rng pools stay in step,
* quiescence actually fires and reaches quiet over a real 3p game, with
  truncation well under budget,
* `MAX_NODES=0` degrades to a legal 1-ply move rather than raising,
* every level returns a legal move,
* `_war_value` equals evaluating the state `events.resolve_war` itself
  produces, and does not touch the state it was asked about.

Two *documentation* defects were found and corrected: the action-card claim in
section 1 (18 of 33, not all), and section 3's implicit assumption that
LEVELS=2 was a free upgrade (it is a small regression — section 4.5), plus
section 3.1, where the cost ratio itself turned out to be understated.

### 6.4 A note on `TTA_JOURNAL=1` and the cost numbers

`journal.install()` is lazy, and `QuiescentBot` never calls `journal.begin` —
it holds several live trial states at once and must stay on the `copy_state`
path (docs/PYPY.md 9.15). So with `TTA_JOURNAL=1`, which
`experiments/run_league.sh` now exports, **`WeightedBot` gets its 1.44x and
`QuiescentBot` gets nothing**. Any cost ratio measured with the journal on is
therefore biased against quiescence by roughly that factor; section 3's
~1.2x figures are the journal-off comparison and are the right ones to quote
for "how much extra work is this". The strength numbers are unaffected — the
journal changes only how a trial move is undone, and
`tests/test_journal_weighted.py` pins that the two paths return the same move.

## 7. Honest accounting: what "1 ply" means, and what this is not

Asked directly, and answered without flattery, because the question ("are
these modelled correctly? is this acting like Stockfish?") deserves a straight
technical answer.

### 7.1 What our architecture actually is

* **Search depth: one *decision*.** Not one turn — one decision. A TTA turn is
  a politics phase plus up to four to six civil actions plus military actions,
  so a turn is roughly six to ten decisions. `WeightedBot` does not see the end
  of **its own turn**, let alone an opponent's reply. Branching factor is
  around 30, and the whole search is `argmax` over those 30, at ~7,000
  evaluations per player-game.
* **Evaluation: a hand-written linear function.** 60 features, 82 weights (the
  base set plus ten early/late phase pairs, plus one deliberately non-linear
  `hand_potential` term), fitted by (1+λ) hill climbing against a pool.
* **No tree machinery, because there is no tree.** No alpha-beta, no
  transposition table, no move ordering, no iterative deepening, no null-move,
  no LMR. None of it applies to a one-level `argmax`.
* **Quiescence, added here, is not depth.** It is a *correctness* fix: resolve
  the pending-decision stack before scoring, which fires on 3–4% of candidates
  and costs ~1.2x. It buys reachability for a class of moves, not lookahead.

### 7.2 Why the Stockfish comparison misleads in both directions

Stockfish searches on the order of 30–50 plies with alpha-beta plus a large
pile of pruning heuristics and an NNUE evaluator, at tens of millions of nodes
per second. We search one ply with a linear evaluator. That gap is obvious and
is the less interesting half of the answer.

The more important half is that **Stockfish's techniques do not port to this
game at all**, so "add alpha-beta and search deeper" is not a plan:

* **Alpha-beta requires two players and zero sum.** TTA is 3–4 players and
  positive sum. The minimax value is not even defined; the n-player analogue is
  max^n (Luckhardt & Irani), which admits only shallow pruning — a constant
  factor, not alpha-beta's square-root-of-the-tree. The usual dodge is the
  "paranoid" reduction (treat all opponents as one coalition against you),
  which is a *particularly* bad model of TTA, where the thing you most want to
  predict is which opponent attacks which other opponent.
* **The game is stochastic.** Civil deck, military deck, event deck. Chance
  nodes require expectimax; the best known pruning there (\*-minimax, Star1 /
  Star2) is dramatically weaker than alpha-beta.
* **The game is imperfect information.** Military hands are hidden and deck
  order is unknown. The game tree is not the right object — information sets
  are. The correct families are determinization / PIMC, ISMCTS, or
  CFR-descended methods. (Our engine hands every bot the entire `GameState`,
  including rivals' hands and the deck order, so all our bots are already
  cheating; quiescence cheats more, because resolving a defender's `defense`
  decision reads the defender's real hand. See 6.2 point 3.)
* **The payoff horizon is enormous.** A wonder or a government change pays off
  ten to twenty turns later. Searching two or three plies deeper reaches
  nothing a good evaluation function does not already see. **This game rewards
  evaluation quality far more than it rewards depth** — it is an economic
  engine-builder, not a tactical game.

Quiescence is itself evidence for that reading: it won by fixing what the
evaluation was *blind* to, at a 1.2x cost, not by looking further ahead.

### 7.3 How strong are these bots, absolutely

Unknown on any human scale, and weaker than the repo's older documents claim
in one direction and stronger in another. Both corrections matter, so both are
given.

**The old external verdict is stale.** `docs/STRENGTH_CHECK.md` reports
BookBot — a ~200-line hand-written priority list with no search, no evaluator
and no learned weights — beating the trained champion 62.9% at 2p, and
`docs/BOT_ROSTER.md` places the champion fifth of twelve. Both were measured
against the **frozen gen-222 snapshot**, before the league pool existed. The
*current* 2p champion (gen 337) does not lose to those bots. Measured here,
400 games each, 2p:

| | vs `BookBot v2` | vs `CultureBot` |
|---|---|---|
| champion, 1-ply | **72.1% ± 4.8%** | **51.6% ± 5.1%** |
| champion, quiescent | **78.0% ± 4.2%** | **57.9% ± 4.8%** |

So the league training in `docs/LEAGUE_TRAINING.md` did what it was for: the
2p champion went from losing to a rule list to beating it comfortably.

**But this is no longer an external yardstick, and it should not be read as
one.** `experiments/hillclimb_pool.build_pool` puts `book`, `book2` and every
variant *including* `culture` **into the training pool**. The champion was
trained against these exact opponents. What the table above measures is that
the training worked on its own distribution — a real and useful result, and
emphatically not evidence about play against anything the pool does not
contain. `docs/STRENGTH_CHECK.md`'s original point stands undamaged: we have
never measured this bot against anything outside its own training loop, and we
have **no human benchmark at all**.

What can be said honestly, then:

* The 2p champion is at least as good as a competent rule list built from
  published human strategy writing. That is a real bar and it has been cleared.
* Only just, against the best of them: 51.6% ± 5.1% against `CultureBot` is a
  dead heat, and `CultureBot` is a priority list a person can hold in their
  head.
* The 3p and 4p arms restarted clean and are running at essentially default
  weights — 5 and 21 of 82 weights moved. At those table sizes the bot ends
  **three turns in four with civil actions unspent** (section 5.1). Those are
  not strong players by any standard.
* Until this branch, no champion at any table size had ever declared a war
  (exactly 0.00 per game) or pressed an aggression (0.02–0.18 per game). A bot
  that never uses a whole third of the rulebook is not playing the game well,
  whatever its self-play win rate says.
* The evaluation still cannot tell a good card from a bad one
  (`docs/WASTED_ACTIONS.md` §6). That is a large, known, unfixed hole.

Quiescence's +5.8% / +9.5% / +16.7% is a real improvement, it replicates
against opponents of a completely different design, and it opens a layer of the
game that was previously unreachable. It is a step *within* that band, not a
step out of it.

### 7.4 The realistic next architectural step

Ranked by expected value per unit of effort, most valuable first. Deeper
classical search is deliberately last.

1. **Fix the evaluation, not the search.** `docs/WASTED_ACTIONS.md` §6–7 has
   already localised the biggest single defect: `features()` reduces the entire
   civil hand to a count and a sum of age levels, so the evaluation **cannot
   tell a good card from a bad one**, and therefore cannot price `take` at all.
   `hand_potential` is a first pass at this and measured 69.6% ± 4.5% at 2p.
   The standing argument that this is where the strength is: `CultureBot` has
   **no search whatsoever** and holds the current 2p champion to a dead heat
   (7.3). A rule list matching a 1-ply searcher means the search is buying
   almost nothing that the rules do not already encode — so buy better rules,
   or a better evaluation, before buying more search. Cost: days of feature
   work plus a re-climb. Highest value/cost ratio available today, by a
   distance.
2. **A learned value function.** Replace the 82-weight linear form with a small
   network trained to regress final culture (or win probability) from self-play
   — the AlphaZero-family answer, and the only path to genuinely strong play.
   The blocker is not the model, it is throughput: a game costs 0.5–2 cpu-s in
   CPython today, so 10^5–10^6 self-play games is one to three CPU-months.
   Realistically gated on `docs/PYPY.md` (the journal/undo work, ~1.8x, and
   PyPy) or on an engine rewrite. Weeks, not days.
3. **ISMCTS or determinized MCTS.** The textbook answer for a stochastic,
   imperfect-information, multiplayer game, and it dissolves the max^n problem
   because MCTS handles n players natively. Two of the three ingredients
   already exist — BookBot is a ready-made rollout policy, and a `determinize`
   helper is being built on another branch. The third does not: at ~1000
   playouts per decision and ~200 decisions per game, one MCTS game costs on
   the order of 10^5 game-equivalents. Flatly unaffordable in CPython. It
   becomes affordable only with (2)'s value function truncating the rollouts,
   plus (2)'s speed work — so it is downstream of both, not an alternative.
4. **Deeper classical search.** Last, and the case against it is the case made
   throughout this document: a branching factor of ~30, chance nodes, hidden
   information, no usable pruning at 3–4 players, and a payoff horizon twenty
   plies out. Two-ply expectimax would cost ~30x for a lookahead that reaches
   the middle of the opponent's turn. Section 2 rejected it before any of this
   was measured and the measurements support that call.

The cheapest concrete wins right now, in order: turn on `--with-quiescent` in
the three live arms (6.1); retire `deferred_credit`'s hand-priced constants
once the challenger moves to quiescence (6.2); then (1).
