# A per-turn rate is worth `rate x turns remaining`, and nothing multiplied by the turns

Date: 2026-07-31.  Lane: the "missing endgame culture channel".

**The lane was briefed as an endgame problem and that framing is wrong, which
is the first result.**  The trade-off between a per-turn RATE and the TURNS
REMAINING to collect it is not a late-game special case: it is true on turn one
and on every turn after, it applies to culture, science, food and resources
alike, and it should be one continuous expression rather than a bonus that
switches on near the end.  Nothing below has a late-game branch in it.  The
endgame behaviour the brief wanted — one-shot payoffs and completions
dominating rates as the game closes — falls out of the same expression, because
`culture` is FROZEN at 1.0 and does not move with the horizon while a rate
does.

---

## 1. The finding: the evaluator is horizon-blind on every rate channel

`engine/bots/weighted.py` has a good, state-derived estimate of how much game is
left.  `rounds_left(state)` is exact once Age IV begins (`final_round_end` is
pinned) and is otherwise the **exact** count of undealt civil cards divided by a
deal rate **measured in the game being played** (`take_rate`, shrunk off a
labelled prior within a couple of rounds).  That machinery was built by the
horizon lane in [`docs/CULTURE_GAP.md`](CULTURE_GAP.md) part 2 and it is not the
problem.

The problem is what reads it.  Before this change:

```
$ grep -rn "rounds_left" engine/
engine/bots/weighted.py:734:def rounds_left(state, n=None, w=None):
engine/bots/weighted.py:801:    lv = (z - rounds_left(state, n, w)) / (z - _L_ONE)   # LEGACY hatch only
engine/bots/weighted.py:925:                              - rounds_left(state, None, w))
engine/bots/neural_encode.py:277:        rl = rounds_left(state)
```

Line 925 is `wonder_overrun`.  **`rounds_left` was consumed by exactly one
feature in the entire 118-key vector**, and by nothing that prices a rate.
(`neural_encode` feeds it to the value net, which is a different evaluator.)

Every rate instead went through the phase blend

    contribution = ( w[k] + (1-L)*w[k_early] + L*w[k_late] ) * rate

where `L = lateness(state)` is a **[0, 1] shape** — the fraction of the civil
supply already dealt — not a count of turns.  So the exchange rate between "one
culture per turn" and "one culture" was a constant the hill climb had to
discover, in units it was never told.

## 2. The arithmetic, and it is not close

`culture` is in `hillclimb.FROZEN` at 1.0, so every weight is denominated in
culture points and **the ceiling on what `+1 culture/turn` can be worth is
exactly `rounds_left`** — you cannot collect a rate more times than there are
turns.  Six 2p self-play games, every seat-0 decision, bucketed by age
(reproduce with the snippet in §6):

| age | decisions | rounds_left = **ceiling** | `DEFAULT_WEIGHTS` pays | live champion_2p (gen 84) pays | `DEFAULT` + `rate_horizon=1` |
|---|---|---|---|---|---|
| A   |  30 | 23.06 |  6.66 | **31.45** | 12.76 |
| I   | 405 | 16.47 |  5.82 | **31.85** |  9.24 |
| II  | 488 | 10.97 |  4.72 | **32.36** |  4.94 |
| III | 467 |  5.05 |  3.59 | **32.89** |  1.77 |
| IV  |  99 |  1.32 |  3.00 | **33.17** |  0.38 |

Read the third and fourth columns against the second.

* **`DEFAULT_WEIGHTS` has a dynamic range of 2.2x against a true 17x.**  It
  under-prices a rate 3.5x in Age A and over-prices it 2.3x in Age IV.  The
  phase pair (`culture_rate_early` +2.0, `_late` −2.0) is pulling in the right
  direction and is far too small.
* **The live 2p champion is above the ceiling everywhere, and it goes the wrong
  way.**  31.45 → 33.17: it pays *more* for a rate as the horizon collapses,
  because it has climbed to `culture_rate` 31.678 with a phase pair of
  (−0.386, **+1.492**) — the opposite sign to the default's shaping.  In Age IV
  it pays **33.2 culture points for something worth at most 1.32**, a 25x
  overpay, on the last turns of the game where the decision is between a rate
  and a one-shot.
* That is the same mispricing [`docs/CULTURE_GAP.md`](CULTURE_GAP.md) §23b
  explicitly declined to retract ("pricing +1 culture/turn at 35.6 flat is above
  the theoretical ceiling everywhere in the game"), measured here on the
  currently-live vector and per age.

**And it is not specific to culture.**  `science_rate`, `food_rate` and
`resource_rate` have exactly the same shape and the same flat treatment; the
table is culture's because culture is the score and therefore the one channel
where the ceiling is rule-derived rather than itself a learned conversion.

## 3. Why "the phase weights can already express this" is true and does not help

The class of vectors *can* represent a horizon: `rounds_left` is roughly affine
in `1 - L`, so some (base, early, late) triple approximates `c * rounds_left`.
The search does not find it, and that is measured rather than asserted:

* [`docs/CULTURE_GAP.md`](CULTURE_GAP.md) §11, the trainer's own `--ablate`
  machinery, n=72, paired: at 2p, deleting `culture_rate_early` and
  `culture_rate_late` is worth **+0.0000 ± 0.0000** win share each — not a
  statistical null but the discrete statement that **it changed not one game** —
  while `culture_rate` next to them is worth 0.110 ± 0.029.
* §12c of the same document measured why: the level term moves ~61x faster than
  the shape term under the climber's step, so the shape axis is explored at a
  rate the gate cannot resolve.
* The consequence is visible on disk.  Against a default phase pair of
  (+2.0, −2.0), the live 2p champion carries (−0.386, +1.492) and the archived
  4p one carried (0.000, −0.316): **the arms that lose the culture-rate race
  price a rate as a constant.**

Making the horizon a **structural** multiplier rather than something the climber
must rediscover in every arm is the narrowest change that tests the hypothesis.

## 4. What was built

Three additions to `engine/bots/weighted.py`, plus one weight.

```python
def horizon_scale(state, n=None, w=None):
    rl  = rounds_left(state, n, w)
    ref = 0.5 * (rl + max(0.0, state.round - 1.0) + 1.0)
    return rl / ref
```

**There is no fitted constant in it, and both halves come off the state.**
`rounds_left` is exact-or-measured as described in §1.  `ref` is the *mean* of
rounds-left over one game: `rounds_left + (round - 1)` is a live estimate of the
game's total length, rounds-left decrements by one per round from that total to
1, so its mean is `(total + 1) / 2` — arithmetic, not a fit.  Because `total` is
re-estimated at every decision from the same live quantities, a game that runs
long or short renormalises itself and there is no per-player-count table.

`rate_multiplier(state, w)` blends it against 1.0 by a new tunable weight,
`rate_horizon`:

    mult = max(0.0, 1 + rate_horizon * (horizon_scale - 1))

and `RATE_KEYS` — `culture_rate`, `science_rate`, `food_rate`, `resource_rate` —
are priced through it.

### 4a. It lives on the PRICE, not on the board, and that took two goes

The first cut multiplied the rate *features* in `features()`.  That is wrong,
and `tests/test_build_fresh.py` said so in one line: *"Irrigation: priced
food_rate at 2.0, features() moved 3.83"*.  `board_yields` emits a card's
**printed** yield and the suite asserts it against the real `features()` delta;
scaling the feature silently put those two in different units.

A civilisation producing 5 culture a turn produces 5 culture a turn however much
game is left.  The horizon is a property of **what that is worth**.  So:

* `features()` reports the board, unscaled — byte-identical to before;
* `evaluate()` multiplies the `RATE_KEYS` contributions (base weight *and* the
  phase pair, which are part of the same price) by the multiplier;
* `feature_marginal()` — the single definition of "what one unit of this feature
  is worth" — multiplies by it too, so **every card-pricing site picks the
  horizon up for free and cannot disagree with `evaluate` about it.**

That last point is the reason the placement matters rather than being a matter of
taste: with the horizon on the price there is exactly one place to put it, and
`tests/test_yellow_pricing.py`'s numerical-derivative check (`feature_marginal`
== d`evaluate`/d`feature`, to 1e-9) holds automatically.  With it on the board
there were two places, they disagreed, and the derivative check failed.

The one remaining second implementation of `evaluate` — `tools/feature_variance.
py:score_from`, which scores from a cached feature vector — now takes the
multiplier as an argument, because a cached feature vector deliberately does not
carry it.  `tests/test_coverage_tools.py` pins the two together at 1e-9.

### 4b. Three deliberate choices

1. **Mean-normalised, not raw `rounds_left`.**  The divisor is pure gauge for
   the weight *class* — any constant factor is absorbed by rescaling `w[k]` — so
   it is spent, exactly as [`docs/CULTURE_GAP.md`](CULTURE_GAP.md) §8b spent it,
   on disturbing an already-trained vector as little as possible.  Mean-
   normalising leaves `w["culture_rate"]` meaning what it means today ("a rate
   point at an average moment"), so what changes is the SHAPE and not the level.
2. **Floored at zero.**  A credit the league climbs outside [0, 1] may flatten a
   rate; it may never turn a positive rate into a liability.  That is the failure
   mode [`docs/CULTURE_GAP.md`](CULTURE_GAP.md) §8b(i) measured when an unclamped
   horizon drove `1 - L` negative and flipped every `_early` term.
3. **No endgame branch, and no term for the one-shot side.**  A one-shot culture
   payoff — a completed Age III wonder's `onBuildCulture`, an Age III scoring
   event — already lands on `culture`, whose weight is frozen at 1.0 and does not
   move with the horizon.  Rates decaying toward zero *is* the endgame preference
   for one-shots.  If a future change needs a late-game branch here, that is a
   signal the model is wrong, not that the game has a special case.

### 4c. What is NOT scaled, and why

`rival_culture_rate` and `rival_science_rate` are per-turn rates too, and the
argument that they only matter for the turns remaining is sound.  They are
deliberately left out.  Including them made two coordinates that
`tests/test_coverage_tools.py:TestInertFeatures` declares **inert across a
candidate set** start varying between candidates — a behavioural change nobody
asked for, smuggled in under a different change.  It is `docs/OPEN_ITEMS.md`
item 2.31, not this one.

## 5. How this was validated, and it is not an A/B

**Mid-lane the project's validation method changed, and this document is the
first one written under it.**  The rule now lives in
[`docs/SYSTEM_COVERAGE.md`](SYSTEM_COVERAGE.md) ("How changes are validated on
this project"): changes are landed on master and judged by the real league runs
plus logging, **not** by offline paired A/B batches and **not** by replaying
`tools/gate.sh`'s fingerprint arms, because both compete with training for the
same six cores.  So:

* **`rate_horizon` ships at 1.0, enabled**, not pinned at 0.0 pending a duel.
  1.0 is the only defensible value: it is where the multiplier IS the derived
  horizon, and every other value has a fitted number in it.
* **It is live on all three league arms the moment this lands.**  The key is
  absent from every champion file on disk, so `load_weights` fills it from
  `DEFAULT_WEIGHTS` — the `tech_board_credit` / `gov_board_credit` /
  `action_board_credit` precedent.  A default of 0.0 would have left the fix off
  in exactly the arms that need it, which is the mistake `card_board_credit` is
  still making for wonders (`docs/OPEN_ITEMS.md` item 2.29).
* **It is a weight**, so the league may climb it away if it is wrong, and
  `hillclimb.mutate` can reach it from any value (its step is
  `gauss(0, s) * (abs(w) + 0.15)`; only the multiplicative `rescale` operator
  has a fixed point at 0).

### 5a. The logging that replaces the gate

The digest gate's *job* — catching a change that altered behaviour nobody
intended to alter — is real, so it is replaced rather than dropped.
`experiments/behaviour.py`, the league's existing behavioural channel, now emits
per age:

| field | what it answers |
|---|---|
| `rounds_left` | the CEILING on what a +1 rate can be worth |
| `rate_mult` | what the horizon term is actually doing, and whether it is on |
| `culture_rate_priced` | what this vector pays for one point of culture production |
| `culture_rate_priced_over_ceiling` | **>1.0 means the vector is paying above its own ceiling** |
| `takes_made` / `takes_per_game` | what it took in that phase age |
| `decisions`, `decisions_with_a_legal_take`, `take_rate_when_offered` | the DENOMINATOR, always, beside the rate |

Measured on `DEFAULT_WEIGHTS`, 2p, 3 games, before and after — this is the
observable, not a strength claim:

| age | ceiling | priced OFF | over-ceiling OFF | priced ON | over-ceiling ON |
|---|---|---|---|---|---|
| A | 23.06 | 6.66 | 0.29 | 12.76 | 0.55 |
| I | ~16.3 | 5.88 | 0.36 | 9.48 | 0.58 |
| II | ~11.0 | 4.79 | 0.43 | 5.10 | 0.46 |
| III | ~5.1 | 3.58 | 0.71 | 1.83 | 0.35 |
| **IV** | ~1.3 | **3.00** | **3.00** | **0.36** | **0.29** |

The Age IV row is the whole change: a vector paying 3x its own ceiling now pays
0.29x it.  And the behaviour moved with it — Age IV `take_rate_when_offered`
went **0.182 → 0.533** on the same three games (takes 2 → 8), against a human
Age IV take rate of 1.59 per seat-game.  n=3 games; this is a mechanism check,
not a measurement.

### 5b. The partial A/B that was run before the method changed, reported in full

Two ladder rungs completed at 2p before the batches were stopped.
`tools/rate_horizon_ab.py`, `DEFAULT_WEIGHTS + rate_horizon = c` challenging a
table of `DEFAULT_WEIGHTS`, seat-rotated, n=300, shared seeds, null exactly
0.500:

| `rate_horizon` | win | ±95% | p | culture A vs B |
|---|---|---|---|---|
| 0.00 (control) | **0.500** | 0.056 | 0.69 | 113.7 vs 113.7 |
| 0.25 | 0.510 | 0.056 | 0.53 | 114.9 vs 116.2 |
| 0.50 | 0.483 | 0.056 | 0.69 | 114.5 vs 116.5 |

0.75 and 1.00 were never run, and neither was 3p.  **Read this as two nulls and
a clean machinery check, and not as evidence for or against 1.0.**  Three
caveats, all of which cut against leaning on it:

* the control rung landing on exactly 0.500 is the byte-identity check passing,
  which is the one thing this table establishes firmly;
* both live rungs are well inside their error bars, and 2p is the arm where the
  previous horizon change was also a null ([`docs/CULTURE_GAP.md`](CULTURE_GAP.md)
  §8d: 2p `DEFAULT_WEIGHTS` 47.2%, p=0.27, while 3p and 4p moved +5.5 and +7.5).
  2p games are the shortest, so the horizon has the least to say about them;
* **these rungs were run against the FIRST implementation** — the one that scaled
  the feature (§4a) and included the two rival rates (§4c).  `evaluate` and card
  pricing are arithmetically identical between the two builds, so the numbers
  transfer for the four own-rate channels, but they describe a slightly wider
  change than the one that ships.

## 6. Reproducing §2

```python
import random, collections
from engine import game as G
from engine.bots import weighted as W
from engine.bots import WeightedBot
d = dict(W.DEFAULT_WEIGHTS); don = dict(d); don['rate_horizon'] = 1.0
champ = W.load_weights('experiments/league_state/champion_2p.json')
b = collections.defaultdict(lambda: [0, 0.0, 0.0, 0.0, 0.0])
for seed in (1, 2, 3, 4, 5, 6):
    st = G.new_game(2, seed=seed); rng = random.Random(seed ^ 0x5EED)
    bots = [WeightedBot(seed=seed * 7 + i) for i in range(2)]
    while not st.game_over:
        r = b[st.age_civil]; r[0] += 1
        r[1] += W.rounds_left(st, 2)
        for j, v in enumerate((d, champ, don), start=2):
            r[j] += W.feature_marginal('culture_rate', st, 0, v)
        G.apply(st, bots[st.decider()](st), rng)
```

## 7. What is bounded, and what I looked for and did not find

Stated explicitly because a bound that is not written down reads as full
coverage.

* **No offline A/B and no digest replay, by the standing rule** (§5,
  [`docs/SYSTEM_COVERAGE.md`](SYSTEM_COVERAGE.md)).  What backs the change is
  the arithmetic in §2, the mechanism check in §5a, the two 2p nulls in §5b, and
  1,297 unit tests — not a win rate.  **The strength of this change is
  unmeasured and is expected to be read off the league arms.**
* **Seven tests failed when the credit was turned on, and every one of them was
  mine.**  Two were real design errors and are fixed in the design (§4a, §4c);
  the other five were fixtures that had silently stopped exercising their own
  precondition once the policy moved — a state where a take is legal, a leader
  whose effects reach `Stats`, a hidden pile with two distinct entries, a hand
  holding a leader worse than the incumbent, a corpus that reaches discontent.
  All five now **seek or construct** their precondition and fail loudly if they
  cannot, rather than hardcoding a ply count.  That is the gate's job being done
  by tests, and it is the argument that it can be.
* **The static card table still prices a rate through the bare `w[k]`.**
  `_sum_yields` — the `card_potential` path taken when a card has no board-aware
  handler and no board credit — multiplies a yield by `w.get(k)` and therefore
  carries neither the phase blend nor the horizon.  This is a **pre-existing**
  divergence (the exact defect [`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md)
  found for the phase blend and closed for technologies, governments and action
  cards by routing them through `feature_marginal`), and this change neither
  widens nor narrows it.  `docs/OPEN_ITEMS.md` item 2.30.
* **`rate_horizon` is in neither `NONNEG` nor `NONPOS`.**  `guard_weights` keys
  those sets off the sign of the *default*, and 1.0 is positive, so it is in
  `NONNEG` and cannot be driven negative.  Noted because the earlier draft
  shipped it at 0.0, where it would have been in neither.
* **Not measured at 4p or 3p at all.**  4p is where the wonder and culture gaps
  are largest and is the arm most likely to move.
* **What I looked for and did not find: a missing one-shot channel.**  The lane
  was briefed on the hypothesis that endgame culture is unpriced.  **It is not.**
  `effects.wonder_completion_culture` is the single implementation of the four
  Age III wonders' `onBuildCulture` bomb, `board_yields._on_build_culture`
  prices it by calling *that same function*, and
  `tests/test_card_pricing.py:TestOneImplementation` fails if they diverge.
  What is true is narrower: that handler is reachable only through
  `card_board_credit` (0.0 by default, 0.361 on the live 2p champion, **0.0 on
  3p and 4p**) and `wonder_potential` (0.0 by default, 0.115 at 2p, 0.0 at
  3p/4p), so the wonder identity channel is switched off on two of the three
  arms — which tracks the completion rates exactly (1.53 / 0.24 / 0.16 per
  seat-game).  Wonders are the one civil type that never got the
  `*_board_credit`-defaults-to-1.0 treatment.  `docs/OPEN_ITEMS.md` item 2.29;
  deliberately not this lane's change.
* **What I looked for and did not find, second: an Age IV coverage hole.**
  `docs/OPEN_ITEMS.md` item 2.17's exact zero is not the engine and not the
  evaluator.  `tools/age_iv_row.py`, 2p, 20 games: the row is non-empty on
  **288 of 288** Age IV decisions, a take is legal on 65% of them, and
  `DEFAULT_WEIGHTS` takes **2.00 per seat-game** against a human 1.59 — with the
  live 2p champion vector, **2.33**.  Both are *above* the human rate.  Whatever
  produces the census's 0.00 is neither of those two, and the remaining
  candidate is the search the census ran (`plan:width=2,det=1`).  Bounded: one
  player count, `WeightedBot`, n=20 games.
