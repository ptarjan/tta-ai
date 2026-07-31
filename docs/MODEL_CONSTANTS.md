# The evaluator was planning against a policy that no longer exists

2026-07-30.  Base game (2015), all three player counts.  Three constants in
`engine/bots/weighted.py` were estimating quantities the game state already
knows exactly, or that the bot can watch happen.  This replaces them, measures
the replacement against ground truth, and reports the cost to the one trained
vector that had something to lose.

The owner's reading of the audit list, verbatim:

> "Those all seem weird. What is deal rate? The number of cards should be
> fixed. Knowing the phase should be easy to know based on how many turns have
> happened. Rival take rate should be seen."

All three are right, and one of them was live damage rather than a style
complaint.

---

## 0. The three, in one table

| constant | it was estimating | what the state already knew | now |
|---|---|---|---|
| `CARDS_PER_ROUND = {2: 6.29, 3: 6.73, 4: 5.71}` | cards leaving the decks per round | `n * SWEEP[n]` = **6 / 6 / 4** exactly (§2.1); the rest is takes, and takes are a difference of two public counts | `n * (SWEEP[n] + take_rate(state))`, the take half **measured in the game being played** |
| `_L_ZERO = {2: 27.1, 3: 28.7, 4: 36.1}`, `_L_ONE = 5.0` | how far through the game we are | the exact number of civil cards still to deal, and the exact size of the whole supply | `lateness = 1 - cards_unseen / supply`, exact, bounded [0, 1] by construction |
| `RIVAL_TAKE_P = 0.25` | P(a rival takes this card first) | every rival's civil actions, hand fullness, surcharges and reachable slots — all open information | `rival_take_p(cost, budget, reach, slack, share)`, one fitted `share` left, exposed as the **weight** `rival_take_share` |

`AGE_IV_ROUNDS`, `_TURNS_CAP`, `_SCORING_MARGIN_CAP` and `PACT_OFFER_CREDIT`
were deliberately **not** reworked; they are now labelled in the source with
which kind of number they are, and `tests/test_model_constants.py` makes that
labelling mandatory for every module-scope constant in `engine/`,
`engine/bots/` and `experiments/`.

---

## 1. `CARDS_PER_ROUND` — the one that was live damage

### 1.1 It was one exact number plus one guess, glued together

The deal rate decomposes with no residual:

```
cards leaving the decks per round = n * SWEEP[n]        <- RULES_SPEC 2.1, EXACT
                                  + cards players took  <- policy
```

`SWEEP = {2: 3, 3: 2, 4: 1}` and `_replenish` runs at the top of every player's
turn, so the sweep half is **6 / 6 / 4** cards a round and is not an estimate
at all.  The fitted constants were therefore claiming a *take* rate of

| | 2p | 3p | 4p |
|---|---|---|---|
| `CARDS_PER_ROUND` | 6.29 | 6.73 | 5.71 |
| exact sweep half | 6 | 6 | 4 |
| implied takes/round | **0.29** | **0.73** | **1.71** |

### 1.2 And that guess had gone stale by 6.5x

Measured now, `tools/deal_rate.py`, 16 self-play games at 2p under
`DEFAULT_WEIGHTS`:

```
takes/round = 1.88          against the 0.29 the constant assumes
```

The constant's own comment predicted exactly this failure mode — *"It is
calibrated on WeightedBot self-play; a much more card-hungry policy would drain
the row faster and this would then run long"* — and the card-hungry policy had
already arrived.  `d8a2172` (`unit_tech_credit`) and `8b972ef`
(`tech_board_credit`) exist to make the bot take more cards; they succeeded;
nothing re-derived the rate afterwards.

### 1.3 The replacement: measure it, do not assume it

The take count is not hidden.  It is a difference of two exact public
quantities.  `weighted.take_rate`:

```
consumed = supply(n) - cards_unseen(state)          # both exact card data
takes    = consumed - deck_A(n) - replenishes * SWEEP[n]
```

with the four non-take terms all rule-fixed: the 13 cards dealt at setup and
the Age A deck's leftovers (§1.10/§2.1 — `_replenish` empties `civil_deck` in
Age A) together account for the whole Age A deck, and `SWEEP[n]` per replenish
accounts for the sweeps.  `replenishes = state.turn - state.num_players`,
because `start_turn` replenishes from round 2 on and `state.turn` is a
player-turn counter.

Every input is public information: deck sizes are card data, `civil_deck`'s
**length** is a count and not an order (the same line the neural encoder draws),
and the turn counter is the turn counter.

The one fitted number left is `_TAKE_PRIOR = {2: 0.30, 3: 0.35, 4: 0.40}`
(takes per replenish), which covers Age A and the first round or two before
there is any history.  It is shrunk away with weight `_TAKE_PRIOR_W = 4.0`
pseudo-replenishes, so it is gone by mid Age I — which is why the Age A column
below is the only one where the new estimator is bad, and Age A is one round
long and decides nothing with a rate horizon.

### 1.4 Error against ground truth, under two card appetites

`tools/deal_rate.py`.  Ground truth is `final_round_end - this_round + 1` from
the finished game (§12.3).  Errors are in **rounds**, over pre-Age-IV decisions
only — from Age IV on both estimators return the same exact answer.

> A measurement bug found and fixed while doing this, recorded because it
> reversed a sign: `game._advance_turn` increments the round *first* and only
> then notices it has passed `final_round_end`, so a finished game sits one
> round past its own end.  Reading ground truth off `st.round` overstates it by
> exactly 1 and flatters **every** estimator by exactly 1.  The first run of
> this table said the new estimator was 0.45 rounds pessimistic; it is 0.39
> rounds optimistic.  The comparison between the two estimators was unaffected
> (a constant offset cancels), but the absolute numbers were wrong and are
> corrected here.

**2p, 16 games per arm:**

| policy | takes/round | estimator | bias | sd | MAE | bias by age (A / I / II / III) |
|---|---|---|---|---|---|---|
| `shy` | 0.00 | fitted | −0.24 | 0.38 | 0.37 | +0.10 / −0.59 / −0.24 / +0.08 |
| `shy` | 0.00 | **measured** | **−0.10** | 0.46 | 0.37 | −0.94 / −0.48 / +0.09 / +0.21 |
| `default` | 1.88 | fitted | +1.80 | 1.07 | 1.80 | +4.16 / +2.53 / +1.68 / +0.85 |
| `default` | 1.88 | **measured** | **+0.39** | 1.03 | **0.80** | +3.12 / +0.10 / +0.20 / +0.40 |
| `hungry` | 1.88 | fitted | +1.99 | 1.20 | 1.99 | +4.66 / +2.95 / +1.85 / +0.82 |
| `hungry` | 1.88 | **measured** | **+0.40** | 0.99 | **0.67** | +3.62 / +0.27 / +0.10 / +0.26 |

**3p, 10 games, `shy`:** fitted bias −1.04, MAE 1.11; measured bias −0.22,
MAE **0.54**.

Read the table two ways.

**Accuracy at the policy that actually plays.**  On `DEFAULT_WEIGHTS` at 2p the
fitted constant is **1.80 rounds long on average** — it thinks the game has
nearly two more rounds in it than it does, everywhere, and it is worst early
(+2.53 rounds through the whole of Age I) where a wonder-overrun decision
actually gets made.  The measured estimator is +0.39, MAE 0.80 against 1.80,
and its Age I/II/III bias is +0.10 / +0.20 / +0.40.

**Robustness to card appetite, which is the whole point.**  Across the two
appetites that genuinely differ (0.00 and 1.88 takes/round):

```
fitted    bias swings  -0.24  ->  +1.99      a 2.23-round swing
measured  bias swings  -0.10  ->  +0.40      a 0.49-round swing
```

The fitted constant's error is *a function of the policy at the table*.  The
measured one's is not, because it reads the policy at the table.

**Honest caveat on `hungry`.**  The `hungry` lever
(`take_cost_paid: 4.0, hand_potential: 0.5`) did **not** raise the take rate
above `default` — both land on 1.88.  The current defaults are already close
to a hand-limit ceiling, so the real contrast in this table is `shy` (0.00)
against `default`/`hungry` (1.88), not a three-point sweep.  That is still a
0-to-1.88 range in the quantity under test, and it is the range the fitted
constant was calibrated 6.5x below.

---

## 2. `lateness()` — a fitted line for a quantity with no uncertainty in it

### 2.1 What it was

```python
_L_ZERO = {2: 27.1, 3: 28.7, 4: 36.1}   # rounds left at which L = 0
_L_ONE = 5.0                            # rounds left at which L = 1
lv = (_L_ZERO[n] - rounds_left(state, n)) / (_L_ZERO[n] - _L_ONE)
```

Three fitted constants, a per-player-count table, and a dependence on the
**estimated** `rounds_left` — to express "how far through the game are we".
The source comment is explicit that these were not fitted to the game but to
the *previous gauge*: they are "the least-squares best linear-in-`rounds_left`
approximation of the OLD age-bucket L", and "the gauge is therefore free, and
it is spent on not breaking the three already-trained champions".

**That justification expired.**  Two of the three champions it was protecting
were reset to generation 0 on `DEFAULT_WEIGHTS` when `8b972ef` deployed.  Only
the 2p arm still carries a vector trained under the old gauge.

### 2.2 What it is now

```python
lateness(state) = 1 - cards_unseen(state) / supply(n)
```

The fraction of the game's civil card supply already dealt.  Both endpoints are
rule-derived rather than chosen:

* **0** is the deal, when nothing has left the decks;
* **1** is the moment the Age III deck runs out — which is not a milestone
  anybody picked, it is *the thing that ends the game* (§12.2/§12.3: Age IV
  begins, `_set_last_round` fixes `final_round_end`, play stops).

Age IV therefore sits at exactly 1.0 by construction, because `civil_deck` is
empty and `_tail` past Age III is zero.  No fit, no player-count table, and —
unlike the line it replaces — **no dependence on the estimated deal rate at
all**.  `lateness` became exact while `rounds_left` stayed an estimate, which
is the right split: the gauge never needed a rate.

Supply totals, straight from the card data: 152 / 170 / 179 cards at 2p / 3p /
4p (Age A 20 everywhere; I/II/III 44 / 50 / 53 each).

### 2.3 The clamp is kept, and it is still load-bearing

The new gauge is bounded to [0, 1] *by construction* — `cards_unseen` is a sum
of deck lengths and cannot exceed the supply — and the clamp is kept anyway.
`docs/CULTURE_GAP.md` §8d measured what an unclamped `L` costs: `1 - L` goes
negative, which **flips the sign of every `_early` term**, and the 4p champion
fell to 19.9% against a 25% null and the 3p champion to 13.6% against 33.3%.

`tests/test_model_constants.py::LatenessIsBounded` asserts the bound on 450
adversarial states a real game cannot produce — a 10,000-card civil deck, a
negative turn counter, Age IV with a full deck — for the new gauge and for the
legacy one.

### 2.4 What actually changes, stated precisely

The phase blend is `w[k] + (1-L)*w[k_early] + L*w[k_late]`, so an **affine**
change of `L` is pure gauge *for the weight class* — it is absorbed by
rescaling the phase pair — though **not** for an already-trained vector, which
is fixed.  So:

* the two gauges' **slopes in rounds** are close: the old line moves 1/22.1 =
  0.045 per round at 2p, the new one moves ~6.3/152 = 0.041;
* what genuinely differs is **where the gauge saturates**.  The old one hits
  1.0 about five rounds from the end and stays there; the new one reaches 1.0
  only when the supply does.  At the measured per-age mean decks the new gauge
  reads 0.28 / 0.57 / 0.86 at 2p where the old age bucket read 0.33 / 0.67 /
  1.00.

That saturation point is the entire non-affine content of the change, and
§5 measures what it costs.

---

## 3. `RIVAL_TAKE_P` — a flat 0.25 over four things the rules make public

### 3.1 The old argument, and what was right about it

The comment defended the flat constant against the alternative on offer: the
seven per-slot survival rates in `docs/INFORMATION_AUDIT.md` §2.1, which that
audit flags itself as directional (n~210 per slot, generated by row-blind
opponents).  *"Baking seven numbers fitted on blind play into the evaluator
would be fitting the bug."*

That argument is correct and it survives.  It is an argument against **one
particular fitted table**, not an argument for a constant.

### 3.2 What the constant threw away, all of it public

`rival_context` already computes, per rival and exactly:

| input | where it comes from | public? |
|---|---|---|
| civil actions on their next turn | `effects.compute(state, q).civil_actions` | yes — open board |
| civil hand full | `q.hand_size("civil")` vs the limit | yes — §2.6 open civil cards convention |
| what this card costs *them* | wonder surcharge, Hammurabi's discount, Michelangelo's waiver | yes — open board |
| which other row slots they can reach | `actions._can_take_gated` over the row | yes — the row is public |

`docs/INFORMATION_AUDIT.md` §3 is explicit that a rival's civil hand
**contents** are public (RULES_SPEC:71) while their military hand and their
intentions are not.  Everything in the table is inside that line.

### 3.3 The model

```python
rival_take_p(cost, budget, reach, slack, share):
    takes = min(slack, share * budget / cost)     # cards they can buy at this price
    return min(1.0, takes / reach)                # spread over what competes for them
```

A budget split: of `budget` civil actions, a `share` goes to the row, buying
`share*budget/cost` cards at this slot's price to them, spread over the `reach`
slots competing for that money, capped by the hand.  It is monotone the way the
board says — more of their actions or fewer competing cards raises it, a dearer
slot or a fuller hand lowers it — and it is **exactly 0** when the legality gate
says they cannot take the card, which is a fact rather than a probability.

### 3.4 What stays a prior, and why it is a weight now

`share` — the fraction of a rival's civil actions that goes on the row rather
than on building, upgrading or a wonder.  That is policy and intention, which
the rules genuinely hide, so it stays a prior.  It is exposed as the **weight**
`rival_take_share` (default 0.5, in `DEFAULT_WEIGHTS`) rather than as a bare
constant, so `hillclimb.mutate` can fit it and `guard_weights` keeps it
non-negative.  It is in `summarize.GROUPS["row"]` so a group rescale moves it
with `row_bargain_forgone`, which is the only term it multiplies through.

Default 0.5 reproduces the old flat 0.25 on the canonical opening position
(4 civil actions, a 3 CA slot, one reachable card gives 0.667; on a typical
mid-game row with ~8 reachable slots and a 2 CA slot it gives 0.125) — the old
constant sat in the middle of the band the model now spreads out.

### 3.5 This change moves no fingerprint, by construction

`row_bargain_forgone` defaults to **0.0**, and `evaluate` skips `row_pressure`
entirely when both row weights are zero.  The fingerprint plays
`DEFAULT_WEIGHTS`, so no arm can see this change at all — which is what makes
the six moved digests in §6 attributable to the horizon alone.

---

## 4. The constants that were deliberately left alone

Each is now labelled in the source with *which kind of number it is*, which is
the thing the audit could not tell at a glance:

| constant | category | why not reworked |
|---|---|---|
| `AGE_IV_ROUNDS = 2.0` | rule-derived | §12.3: Age IV is this round or the next |
| `_TURNS_CAP = 20.0` | numerical guard | `wonder_turns_to_finish` is a ratio; caps an infinity. Nothing inside the cap is shaped by it |
| `_SCORING_MARGIN_CAP = 60.0` | numerical guard | the fifteen Age III scoring formulas are unbounded above; one outlier must not dominate a linear evaluator |
| `PACT_OFFER_CREDIT = 0.5` | **fitted prior** | genuinely fitted, and **not** converted to a weight: it is spent inside `_pending_terms`, called from `features()`, which has no weight vector. Plumbing one there means a second hot-path signature or a module global — which is what it already is. Labelled and filed in `docs/OPEN_ITEMS.md` instead of half-plumbed |

---

## 5. Strength: deliberately not measured, and why

**No paired A/B was run for this change, on the owner's instruction, and that
is a decision about what evidence is worth buying rather than an omission.**

The project policy is "modelling things correctly is the right thing to do and
should be committed even if it doesn't strengthen the bot", sharpened by the
owner to *"proper always"*.  Under that policy an A/B decides nothing here: the
exact gauge and the measured deal rate land whether the number comes back
positive, null or negative.  Several hundred games to produce a figure that
changes no decision is CPU spent on decoration.  The league logs are the
measurement, and they are free.

What replaces it is **§1.4, which is not an A/B**: a direct accuracy check of
the estimator against ground truth replayed out of finished games, including
the robustness-to-card-appetite property that is the entire reason for the
change.  That measures whether the model is *right*, which is the claim being
made, rather than whether it *wins*, which is the claim that is not.

The A/B harness exists and is committed anyway — `tools/horizon_model_ab.py`,
and the `horizon_legacy` weight hatch that makes it possible — because the
hatch is what lets the two horizons be seated at the same table if anybody ever
wants that, and because the same switch is what makes the per-cause digest
attribution in §6 a one-cause claim.

**The trained 2p champion.**  The earlier plan was to measure the cost to
`experiments/champion_2p.json` (gen 209 at the time of writing), the one vector
still carrying weights fitted under the old gauge.  That run was dropped too:
all three league arms are being relaunched from a clean state for an unrelated
objective change, so the vector is being discarded regardless and the number
would have decided nothing.  Stated plainly so nobody later reads its absence
as a suppressed result: **the impact on that champion is unmeasured.**  What is
known without playing a game is that the change is not gauge-free for a fixed
vector — see §2.4 — so it certainly moves that vector's effective weights.

---

## 6. Fingerprints

Six of the eight arms moved; NARROW and WIDE held.  Those two are GreedyBot,
which calls neither `rounds_left` nor `lateness`, so an arm of them moving
would have meant the change had leaked out of the evaluator.

| arm | old | new |
|---|---|---|
| NARROW | ca255af3 | ca255af3 *(unchanged — GreedyBot)* |
| WIDE | f223cea1 | f223cea1 *(unchanged — GreedyBot)* |
| WNARROW | 16dc9a1a | **6d888d7c** |
| WWIDE | a1b74078 | **c52302c2** |
| QNARROW | 2f59c5c0 | **bbbb203a** |
| QWIDE | 23b8d66e | **3df0155f** |
| PNARROW | 15bd49fc | **1b883d6f** |
| PWIDE | c8fe5d3a | **3922ebc4** |

**Clean-base control first, and it passed.**  A full gate on the parent
`8b972ef` in `/tmp/constbase` reproduced all eight committed constants exactly
(GATE PASS, 1070 tests), so the base was known-good before anything here was
measured against it.

**Two-sided.**  Derived from scratch in `/tmp/constfix` and independently in
`/tmp/constgateB` — a second fresh clone of `8b972ef` with the same file set
copied onto it — and the two agreed **byte-for-byte on all eight arms**,
including the two that did not move.

**Attributed by environment, not by patching a third clone.**
`engine/bots/weighted.py` reads three A/B hatches from the environment at
import, each restoring exactly one retired constant.  That is a stronger
control than a patched clone, because the tree being hashed is byte-identical
to the tree being shipped.

| tree / hatches set | NARROW | WNARROW | QNARROW | PNARROW |
|---|---|---|---|---|
| parent `8b972ef` | ca255af3 | 16dc9a1a | 2f59c5c0 | 15bd49fc |
| all three hatches ON | ca255af3 | **16dc9a1a** | **2f59c5c0** | **15bd49fc** |
| `TTA_LEGACY_ROW_TAKE` only | — | 6d888d7c | bbbb203a | 1b883d6f |
| new rate + legacy gauge | ca255af3 | *7ed600d1* | *487b2aa5* | *c1d0caea* |
| new gauge + legacy rate | ca255af3 | **6d888d7c** | **bbbb203a** | **1b883d6f** |
| shipped (both new) | ca255af3 | 6d888d7c | bbbb203a | 1b883d6f |

### 6.1 The gauge is the whole move, and the deal rate is inert *here*

This is the part to read carefully, because the obvious reading is wrong.

**All three hatches on reproduces the parent's eight digests.**  Everything in
this commit that is not one of the three constants — the renames, the comments,
`rival_take_share` in `DEFAULT_WEIGHTS`, the `w` threaded through `features()`
— is provably behaviour-free.

**The row-take hatch alone reproduces the *shipped* digests**, so replacing
`RIVAL_TAKE_P` moves nothing.  Predicted from `row_bargain_forgone` defaulting
to 0.0, and now measured rather than assumed.

**New gauge + legacy deal rate already lands on the shipped digests**, byte for
byte, on all three arms.  So `lateness` accounts for 100% of the six moves and
the deal rate accounts for none of them.

**But the deal rate is not doing nothing.**  With the legacy gauge on it moves
WNARROW to a *third* value, 7ed600d1, which is neither the parent's nor the
shipped one — the plumbing is live.  It is inert in the shipped combination for
a structural reason, and the reason was checked in the source rather than
inferred from the hash:

* the new `lateness` is `1 - cards_unseen/supply` and **does not call
  `rounds_left` at all** — making the gauge exact also cut the horizon
  estimate out of its largest consumer;
* the only remaining consumer of `rounds_left` inside `evaluate` is the
  `wonder_overrun` feature, and **`"wonder_overrun": 0.0`** in
  `DEFAULT_WEIGHTS`;
* the fingerprint plays `DEFAULT_WEIGHTS`.

So under these weights `rounds_left` has no path to the evaluation, and an arm
moving for the deal rate would have meant one of those three statements was
false.  A trained vector with a non-zero `wonder_overrun` **would** see it, and
so does `engine/bots/neural_encode.py`, which feeds `rounds_left` to the value
net directly.

Stated as a limitation rather than a result: **these 135 games cannot measure
the deal-rate fix.**  §1.4's estimator-error table is the evidence for that
half, and it is the right instrument for it — accuracy against ground truth,
not a hash.

Nothing here was re-derived to make the gate pass.  The gate FAILED on this
tree by design, in both clones, and these are the computed values.

Test count 1070 → 1087: +10 from `tests/test_model_constants.py`, +4 net in
`tests/test_horizon.py` (five new, and `test_calibration_against_the_old_
schedule` removed — it asserted the new gauge stayed within 0.10 of the *old
age bucket*, which is exactly the champion-compatibility constraint the honest
gauge gives up), +3 in `tests/test_row_features.py`.

**Negative control.**  The new and changed tests were run against the parent
tree: **12 of them fail there** (5 failures + 7 errors).  The five that pass on
the parent are the ones that legitimately should — two validate the allow-list
itself and do not read the tree, and three assert bounds the old code also
satisfied (the old `lateness` was clamped, and the old `rounds_left` could not
return less than 2.0).

---

## 7. The standing test

`tests/test_model_constants.py` is the permanent half of this lane.  It walks
`engine/`, `engine/bots/` and `experiments/` with `ast`, finds every
module-scope assignment of a numeric literal (or a container of them), and
fails if the name is not in an allow-list carrying one of six categories:

```
rule-derived      it is in the rulebook.  Cite the section.
numerical guard   it stops a divide-by-zero or an outlier.  Nothing inside
                  the guard is shaped by its value.
measured          it came out of an instrument.  The note MUST name the doc
                  or tool that holds the measurement (asserted).
fitted prior      somebody chose it.  It is a guess with a reason.
training policy   it defines what the hill climb maximises or how it runs.
enum-or-sentinel  an index, a tag, a cache bound, a counter, an RNG seed.
```

86 constants classified.  The list also fails when an entry goes **stale** (a
constant renamed or deleted), so it cannot rot quietly.

`engine/bots/board_yields.py:FREE_POP_UTIL` is the model the other entries are
held to: the comment carries the measurement, the sample size (410
player-turns), and an independent bracket that agrees.

---

## 8. The same eye on the rest of the tree

Swept at the owner's request; most of it is clean.  Five things were not:

**8.1 One name, two different knobs.**  `engine/bots/neural_net.py`
`MARGIN_SCALE = 100.0` and `experiments/hillclimb_pool.py`
`MARGIN_SCALE = 120.0`.  **The divergence is deliberate and they are not the
same job**, which is worse than a disagreement — it is a name collision.  The
pool's is the *tanh squash width* of the league objective, a modelling choice
about how much a decisive game is worth; changing it changes what the climb
maximises.  The net's is a plain *linear normaliser* on a regression target —
divide to train, multiply to read back — so it cancels exactly and any value of
the right order does the same job.  Renamed the net's to **`MARGIN_NORM`**
(4 call sites).  The serialised checkpoint key stays spelled `margin_scale`,
because it is data in files that already exist.

**8.2 Two tier tables, 5x apart.**  `DEFAULT_TIER_WEIGHTS` (book 0.6, variant
0.6) vs `LEGACY_TIER_WEIGHTS` (book 3.0, variant 2.5).  **Not a bug and not
dead code.**  `DEFAULT` is live — verified against the running arms' actual
command lines, which pass it explicitly as
`--pool-weights book=0.6,variant=0.6,human=0.6,mirror=1.0,past=1.2,hall=1.6,floor=0`
— and the 5x gap is the documented 2026-07-27 rebalance that moved the gradient
off a BookBot monoculture.  `LEGACY` is a **reproduction fixture**, read by
`legacy_weight_string()` and by `tools/objective_ab.py`'s `legacy` arm.  Kept,
with a comment saying which is which so the next reader does not delete it.

**8.3 `engine/bots/__init__.py:WEIGHTS`.**  GreedyBot's table, disagreeing with
`BASE_WEIGHTS` (culture_rate 6.0 vs 5.0).  Values **unchanged**; the comment
now says it is **frozen on purpose**.  GreedyBot is the fingerprint control —
NARROW and WIDE are GreedyBot and nothing else, and their job is to hold still
while evaluator changes move the other six.  A control whose weights drift with
the thing it controls for is not a control.  The old comment pointed at
`experiments/harness.py`, which has not been the trainer for a long time, and
read as a forgotten default; the obvious next move on reading it was to sync
the two, which would have moved NARROW and WIDE.

**8.4 `engine/interact.py:WAR_TECH_SCIENCE = 0`** is an **option index**, not
an amount of science, and the name read like a rules value of zero.  Renamed
`WAR_TECH_SCIENCE_IDX` (2 call sites) with the first comment line saying so.

**8.5 `engine/bots/book.py:V2_TUNABLES`.**  The `tunables=` parameter really is
plumbed (`BookBotV2`, `variants/base.py`, `human/base.py` all merge an override
dict), but **no caller in the repo ever supplies one**, so every value is a
frozen literal in practice.  Commented to say so and classified as a fitted
prior rather than a live knob.  That is the intended state — BookBot is a
deliberately hand-written external yardstick and hard-coded human opinion is the
point of it.

**8.6 `CULTURE_CENTRE = 100.0`, checked against the corpus.  Reported only;
nothing changed.**  Re-parsed the 1,011 BGO journals (`tools/bgo_parse.py`,
2,517 scored seats, 0 failures) and took the mean final culture:

| | n seats | mean | median | sd | p10 | p90 |
|---|---|---|---|---|---|---|
| 2p | 1377 | **159.5** | 156 | 58.5 | 91 | 231 |
| 3p | 398 | **176.3** | 180 | 55.9 | 107 | 247 |
| 4p | 742 | **194.6** | 182 | 72.0 | 113 | 298 |
| all | 2517 | 172.5 | 166 | — | — | — |

Two things follow, neither of them a bug.

*The number the comment cites is right.*  `hillclimb_pool.py` reasons from "we
are at 65, humans are at 160" and picks 100 to sit between them; the 2p corpus
mean is 159.5 to the decimal, so the comment is quoting this corpus correctly.
`CULTURE_CENTRE` is a **centring** choice that keeps the marginal value of a
culture point flat across the operating band — it is not an estimate of the
mean, and the corpus does not contradict it.

*But the band it reasons over is a 2p band, and 4p runs off the end of it.*
The comment checks flatness over 40–200.  The 4p human mean is 194.6 and the 4p
p90 is **298**.  With `CULTURE_SCALE = 120`, `d(own_share)/dc` is 0.00329 at
c=159.5, 0.00237 at c=194.6 and **0.00057 at c=298** — a culture point at the
4p human 90th percentile is worth **5.8x less** than one at the 2p human mean.
Since 4p is the arm furthest behind humans, that is the arm whose objective
flattens soonest.  Flagged for the owner; **changing it mid-run would
invalidate the trained vector, so nothing here touches it.**

---

## 9. Open

* `PACT_OFFER_CREDIT` is a fitted prior that cannot become a weight without
  threading one into `features()`.  In `docs/OPEN_ITEMS.md`.
* `_TAKE_PRIOR` is still fitted.  It is measurable per player count from
  `tools/deal_rate.py` and it only bites in Age A; the 3p and 4p entries are
  less well measured than the 2p one.
* The `hungry` policy in `tools/deal_rate.py` does not actually take more cards
  than the defaults do.  A genuinely hungrier lever would strengthen §1.4.
* `rival_take_share` ships at its prior and has never been climbed.  It cannot
  move anything until `row_bargain_forgone` is non-zero, which is only true on
  the three archived champions.
