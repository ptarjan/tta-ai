# Information audit: what the rules make knowable vs what the evaluator reads

Base game 2015 ("A New Story of Civilization"), no expansion. Every claim below
carries a `file:line`. Anything I could not verify says so.

This audit was prompted by the project owner's description of the real skill of
the game:

> "Your turn is mostly yourself but you need to keep track of all the things you
> put in the politics deck, and look at the card row to know and what your
> opponents will want so you know if you have to pay three points for it now or
> you can wait. And to know that you have to keep track of what is in your
> opponents board and hand."

**The hypothesis was right, and it is stronger than stated** *(conclusion as of
2026-07-27)*. Almost all of that information is already sitting in `GameState`
in full, and `engine/bots/weighted.py:features()` reads *none* of it. The bot is
not failing to track hidden information; it is failing to look at information
the engine has already handed it.

**Status as of 2026-07-29 (§0b).** Three of the six gaps below have shipped as
code and two of them are now *live in a trained champion*. The 60-key table in
§0a is preserved verbatim because it is still the correct description of
`DEFAULT_WEIGHTS`, of all three frozen champions, and of the current 2p league
arm. It is **no longer** a correct description of the 3p and 4p league
champions. Read §0a and §0b together or you will draw the wrong conclusion in
either direction.

---

## 0a. Headline result — the 2026-07-27 measurement, 60 keys (HISTORICAL)

*Kept as measured. `features()` returned a 60-key dict; `DEFAULT_WEIGHTS` was 78
keys. Measured invariance on a single real mid-game 2p position (turn ~60,
`/tmp/invcheck.py`, see §7.1). Every row of this table was still reproduced on
2026-07-29 under `DEFAULT_WEIGHTS` and under all three frozen champions — see
§0b.*

| perturbation of the state | feature vector | eval delta |
|---|---|---|
| reverse the entire card row | **identical** | 0.0 |
| **delete the entire card row** | **identical** | 0.0 |
| replace the rival's `hand_civil` with different cards | **identical** | 0.0 |
| empty the rival's `hand_military` | **identical** | 0.0 |
| **wipe `future_events` and `current_events`** | **identical** | 0.0 |
| replace every card in `civil_deck` with "Bronze" | **identical** | 0.0 |

Per-rival-field eval sensitivity, same position:

```
rival.culture            -441.0000     <- the only rival scalar read directly
rival.science              +0.0000
rival.food                 +0.0000
rival.resources            +0.0000
rival.civil_actions        +0.0000
rival.military_actions     +0.0000
rival.workers_free         +0.0000
rival.yellow_bank          +0.0000
rival.colonies             +0.0000
rival.completed_wonders    +0.0000
```

Only five of the 60 features were rival-derived at all — `rival_culture`,
`rival_mean_culture`, `rival_culture_rate`, `rival_science_rate`,
`rival_strength` (aggregates built in `rival_context`).

And the single most damaging consequence, measured in §7.2: pricing the **same
card** at slot 0 (1 CA) versus slot 9 (3 CA),

```
2p champion   slot 0 +1.44621   slot 5 +1.38226   slot 9 +1.31831   (spread 0.128)
3p champion   slot 0 +1.55058   slot 5 +1.64798   slot 9 +1.74538   (spread -0.195)
4p champion   slot 0 +0.06640   slot 5 +0.03276   slot 9 -0.00088   (spread 0.067)
```

**The 3p champion prefers to pay 3 civil actions.** That is not a metaphor: the
only channel by which row depth reached the evaluation was the `ca_left` feature,
whose frozen 3p champion weight is **-0.0974**
(`analysis/frozen/champion_3p.json`), so spending two extra civil actions was
scored as a *gain* of 0.195. This is the mechanism behind an already-measured
behaviour that nobody had explained: the 3p champion takes **56.9% of its cards
from cost band 3 at 2.33 CA/card** while the 2p champion takes 88.4% from band 1
at 1.15 CA/card (`docs/HEURISTICS_PROGRESS.md:119-121`), against a human
tournament baseline of **76% of Age I picks at 1 CA and 2.5% at 3 CA**
(`docs/EXPERT_STRATEGY.md:688`).

---

## 0b. Re-measurement, 2026-07-29 — 89 weight keys

`DEFAULT_WEIGHTS` is now **89 keys** and `features()` returns **64**. The
protocol is §0a's, reconstructed from this document's own §7.1 description
(the original `/tmp/invcheck.py` was throwaway and no longer exists) and
generalised in two ways that the original could not distinguish:

* **35 positions, not one** — seeds 0/1/2 x four ply depths x 2p/3p/4p,
  distributed `{2p: 5 Age I, 3 Age II, 3 Age III; 3p: 6/3/3; 4p: 6 Age I,
  6 Age II}`. A feature that does not respond at one position may respond at
  another, and several of them do.
* **seven weight vectors, not one** — `DEFAULT_WEIGHTS`, the three frozen
  champions, and the three **live league champions** snapshotted mid-run at
  gen 16 (2p) / gen 1139 (3p) / gen 308 (4p). This split turns out to be the
  entire story.

### 0b.1 The structural reason the headline both did and did not change

Four of the eleven new weight keys are **not features at all**. `hand_potential`,
`rival_hand_potential`, `row_urgency` and `row_bargain_forgone` are priced
*through* the weight vector, so they are non-linear and live in `evaluate()`
(`engine/bots/weighted.py:968-992`), not in `features()`. Two consequences,
both measured:

1. **`features()` is still bit-identical under every row, deck and event
   perturbation, at all 35 positions.** That is not a bug; it is where the code
   put them. Any tool that audits blindness by diffing feature vectors — this
   document's own §0a method included — will now under-report what the bot sees.
2. **Each term is skipped entirely when its scale is 0.0**, and 0.0 is the
   default. So the new information is *wired but dark* unless a trainer has
   fitted a non-zero scale onto it.

Who has: measured off the weight files, not asserted.

| weight | DEFAULT | frozen 2p/3p/4p | live 2p (g16) | live 3p (g1139) | live 4p (g308) |
|---|---|---|---|---|---|
| `row_urgency` | 0.0 | 0.0 (absent, filled from default) | 0.0 | **+0.1063** | +0.0024 |
| `row_bargain_forgone` | 0.0 | 0.0 | 0.0 | **+1.5164** | +0.0238 |
| `rival_hand_potential` | 0.0 | 0.0 | 0.0 | -0.0199 | **+1.3285** |
| `rival_free_ca` | 0.0 | 0.0 | 0.0 | **-0.3234** | +0.0264 |
| `rival_hand_civil` | 0.0 | 0.0 | 0.0 | **-0.3538** | -0.0149 |
| `rival_wonders` | 0.0 | 0.0 | 0.0 | **+1.6887** | +0.0678 |
| `take_cost_paid` | 0.0 | 0.0 | 0.0 | +0.0492 | -0.0260 |
| `ca_left` | 0.05 | 0.064 / **-0.0974** / 0.034 | 0.0186 | **+0.6072** | +0.1473 |

The frozen champions predate all eleven keys, so `load_weights` fills them from
`DEFAULT_WEIGHTS` at 0.0 and they evaluate exactly as they did on 2026-07-27 —
by design (`engine/bots/weighted.py:811-829`). The 2p arm restarted and is only
at gen 16, so it has not scattered onto them yet. **GAP 1's wrong sign is fixed
in the live 3p arm**: `ca_left` has climbed from -0.0974 to +0.6072, so paying
two extra civil actions is now a cost, not a gain.

### 0b.2 Structural perturbations, 35 positions

"feat" = `features()` dict differs. "eval" = number of positions where
`evaluate()` moved, out of the positions of that player count.

| perturbation | feat chg | default | frozen (all 3) | live 2p | live 3p | live 4p |
|---|---|---|---|---|---|---|
| reverse the card row | **0/35** | 0/35 | 0/35 | 0/11 | **8/12** | **10/12** |
| delete the card row | **0/35** | 0/35 | 0/35 | 0/11 | **8/12** | **10/12** |
| change which cards are in the row | **0/35** | 0/35 | 0/35 | 0/11 | **8/12** | **10/12** |
| rival `hand_civil` → different cards | **0/33** | 0/33 | 0/33 | 0/9 | **9/12** | **8/12** |
| rival `hand_civil` → empty | 33/33 | 0/33 | 0/33 | 0/9 | **12/12** | **12/12** |
| rival `hand_military` → empty | **0/35** | **0/35** | **0/35** | **0/11** | **0/12** | **0/12** |
| wipe `future_events` + `current_events` | **0/35** | **0/35** | **0/35** | **0/11** | **0/12** | **0/12** |
| `civil_deck` → every card "Bronze" | **0/35** | **0/35** | **0/35** | **0/11** | **0/12** | **0/12** |
| `civil_deck` reordered | **0/35** | **0/35** | **0/35** | **0/11** | **0/12** | **0/12** |

Magnitudes for "delete the card row", the bluntest row test, against a base
evaluation whose mean magnitude is given for scale:

```
2p  n=11   delta  +0.000 +/- 0.000   (base |eval| 114.8)   row weights are 0.0
3p  n=12   delta  -3.005 +/- 2.728   (base |eval| 264.5)
4p  n=12   delta  -0.037 +/- 0.027   (base |eval|  92.6)
```

Read those against the *inter-candidate* spreads in §0a — a whole `take` was
worth +1.45 at 2p and +0.07 at 4p. So at 3p the row is now a first-class term;
at 4p its scale is ~0.002/0.024 and it is present but nearly dark.

The zeros in the live columns are **not** blindness. Position-by-position,
every zero-delta position is one where `row_pressure` returns `(0.0, 0.0)`
structurally — no row card that is both legally takeable by me and has
`card_potential > 0` (3p: 4/12 positions, 4p: 2/12). This is exactly the
distinction the single-position 2026-07-27 run could not draw.

### 0b.3 Per-rival-field sensitivity, 35 positions

`+5` on a scalar, one extra entry on a list. Rows in **bold** are new since
2026-07-27.

| rival field | feature keys that move | feat chg | default / frozen | live 3p | live 4p |
|---|---|---|---|---|---|
| `culture` | `rival_culture`, `rival_mean_culture` | 35/35 | **yes** | yes | yes |
| **`civil_actions`** | `rival_free_ca` | 35/35 | 0/35 (weight 0.0) | **12/12** | **12/12** |
| **`completed_wonders`** | `rival_wonders` | 35/35 | 0/35 (weight 0.0) | **12/12** | **12/12** |
| **`hand_civil` (size)** | `rival_hand_civil` | 33/33 | 0/33 (weight 0.0) | **12/12** | **12/12** |
| **`hand_civil` (identity)** | *none* — eval-only | 0/33 | 0/33 | **9/12** | **8/12** |
| `techs` | `rival_science_rate` 23/24, `rival_culture_rate` 17/24, `rival_strength` / `strength_rel` 13/24, `strength_lead` 8/24, `strength_deficit` 6/24 | 24/24 | yes | yes | yes |
| `leader` / `tactic` | via the same three rates, 1-4/24 | 24/24 | yes | yes | yes |
| `science` | *none* | 0/35 | 0 | 0 | 0 |
| `food` | *none* | 0/35 | 0 | 0 | 0 |
| `resources` | *none* | 0/35 | 0 | 0 | 0 |
| `military_actions` | *none* | 0/35 | 0 | 0 | 0 |
| `workers_free` | *none* | 0/35 | 0 | 0 | 0 |
| `yellow_bank` | *none* | 0/35 | 0 | 0 | 0 |
| `colonies` | *none* | 0/35 | 0 | 0 | 0 |
| `wonder` (in progress) | *none* | 0/24 | 0 | 0 | 0 |
| `destroyed_wonders` | *none* | 0/24 | 0 | 0 | 0 |
| `hand_military` | *none* | 0/35 | **0** | **0** | **0** |

Seven rival board fields the rules make public are still completely invisible:
their **science, food and resource stocks, their military actions, their free
workers, their yellow bank (population cost), their colonies**, and whether
they have a **wonder under construction** — the last of which is the single
cheapest "is this wonder safe to let slide" signal in the game and is already
snapshotted into `_RivalView.wonder` for the legality gate but never scored.

### 0b.4 Feature census: which of the 89 keys read what

Measured by perturbing one information source at a time and recording which
keys move (n=24 positions), not by grepping.

```
DEFAULT_WEIGHTS                                                89
  features() keys                                              64
  phase keys (_early/_late on 10 features)                     20
  eval-only non-linear scales                                   4
     hand_potential, rival_hand_potential,
     row_urgency, row_bargain_forgone
  search bias (not a feature)                                   1   end_turn_bias
```

* **Rival-derived: 12 of 89** (was 5 of 60).
  Directly: `rival_culture`, `rival_mean_culture`, `rival_culture_rate`,
  `rival_science_rate`, `rival_strength`, `rival_free_ca`, `rival_hand_civil`,
  `rival_wonders` (8, all in `features()`); relative:
  `strength_rel`, `strength_deficit`, `strength_lead` (3, functions of
  `rival_strength`); eval-only: `rival_hand_potential` (1).
  `pacts` and `pact_blocks_attack` read rival state only for pacts I am a party
  to; no pact formed in the 35 sampled positions, so they are not counted.
* **Row-derived: 3 of 89.** `row_urgency` and `row_bargain_forgone` (content and
  slot cost, eval-only) and `take_cost_paid` (CA I spent reaching into the row
  this turn — verified responsive, 24/24 positions). **Zero `features()` keys
  read the row.** Was 0.
* **Deck-derived: 0 for composition, and the count is read only indirectly.**
  Replacing every card in `civil_deck` with one name moves the evaluation at
  **0/24 positions under every one of the seven weight vectors**. Truncating the
  deck by 20 cards moves it at **22/24** (mean -2.81 under live weights) — but
  that is `len(state.civil_deck)` reaching `rounds_left`
  (`engine/bots/weighted.py:316`) and thence `lateness()`, which scales the 20
  phase keys. **The evaluator has a game clock, not a card counter.** Unchanged
  since 2026-07-27.
* **Event-derived: 0 of 89.** Wiping `future_events`, `current_events`,
  `past_events`, `scoring_events` and `seeded_by` together moves **no feature
  key at any of the 24 positions and no evaluation under any of the seven weight
  vectors**. Unchanged since 2026-07-27. The owner's *first* item is still the
  one nothing touches.
* **Military discard: 0.** Emptying `discarded_military` moves nothing.

### 0b.5 The one thing that got worse

The `end_turn` information leak is **no longer inert**. See §6.1.

---

## 1. Master table

"Engine represents it" = the fact is recoverable from `GameState` today.
"A feature reads it" = it changes the output of `weighted.features()` /
`weighted.evaluate()`. **Updated 2026-07-29** — where a row changed, the old
verdict is struck through and the weight vector under which the new verdict
holds is named, because for six of these rows the answer is now
weight-vector-dependent.

| # | Information the rules make available to a player | Engine represents it? | Any feature reads it? |
|---|---|---|---|
| 1 | Which 13 cards are in the civil row | YES `engine/state.py:151` | ~~NO~~ → **`evaluate()` only, and only where a scale is fitted**: `row_urgency`/`row_bargain_forgone` (`weighted.py:718-777`). Live 3p **yes** (8/12 positions), live 4p yes but ~0.002 scale, default/frozen/live-2p **still no**. `features()` itself: **NO**, 0/35 |
| 2 | Each row card's slot, hence its CA cost | YES `engine/actions.py:36-45,79-89` | **YES** now — `row_bargain_forgone` prices the slide directly, and `take_cost_paid` (`weighted.py:500`) is a second channel beside `ca_left`. Live 3p `ca_left` is **+0.607**, so §0a's wrong sign is gone in that arm |
| 3 | Where a card will slide to next turn | Derivable (exact sweep constant) `engine/game.py:41,104-121,219-220` | **YES** — `row_pressure` computes `nxt = i - live*SWEEP[live]` exactly (`weighted.py:751,765`) |
| 4 | Whether a card will be swept before I act again | Derivable, same source | **YES** — `row_urgency` is exactly this sum |
| 5 | Whether an opponent can/wants to take a given row card | Derivable (their CA, hand limit, techs, wonder-in-progress, leader ages) | **Legality YES, desire NO** — `_RivalView` + `_can_take_gated` per rival (`weighted.py:176-233,773-775`), then a single flat `RIVAL_TAKE_P = 0.25`. No desire model |
| 6 | Opponents' civil cards in hand (PUBLIC, `docs/RULES_SPEC.md:71`) | YES `engine/state.py:60` | ~~NO~~ → **YES in `evaluate()`**: `rival_hand_potential` (`weighted.py:667-692`). Live 4p scale **+1.329**, live 3p -0.020. `features()`: **NO**, 0/33 |
| 7 | Opponents' civil hand *size* | YES same field | ~~NO~~ → **YES**, `rival_hand_civil` (`weighted.py:459`), and it uses `hand_size` so the app harness's `hidden_civil` counts too |
| 8 | Opponents' military hand *size* (public) | YES `engine/state.py:61` | **NO** — still own hand only |
| 9 | Opponents' military hand *contents* (HIDDEN by rules) | YES, truthfully — no info-set abstraction | **NO** by features or eval, re-verified 0/35 positions x 7 weight vectors; **YES** by QuiescentBot's defence resolution `docs/DEEPER_SEARCH.md:507-512` |
| 10 | Age I/II/III deck composition (fixed, public) | YES `engine/cards.py:155-175` | **NO** — still only a **count**, via `rounds_left` → `lateness` (`weighted.py:305-317`). Composition: 0/24 positions, all 7 vectors |
| 11 | Which civil cards have already been seen (row/hands/boards) | Partially — **swept row cards are destroyed with no record** `engine/game.py:117-120` | **NO** |
| 12 | Which military cards have been discarded | YES `engine/state.py:132`, `engine/economy.py:186-197` | **NO** (0/24) |
| 13 | **What I put into the politics (future events) deck** | YES, with owner attribution `engine/state.py:125,129`; written at `engine/actions.py:992` | **NO** by the linear bot (0/24). **YES** by the neural encoder, correctly masked to my own seeds (`engine/bots/neural_encode.py:182-188`) |
| 14 | The current-events deck contents/order (hidden) | YES, in the clear `engine/state.py:126` | **NO** by features; **readable by any deeper search** — `plan.determinize` still does not touch it `engine/bots/plan.py:111-114` |
| 15 | Events already resolved (`past_events`, public) | YES `engine/state.py:127` | **NO** |
| 16 | Opponent boards: techs, workers per card, government, leader, wonders, tactic, colonies, pacts, happiness, food/resources/science, CA/MA | YES `engine/state.py:43-101` | **3 derived rates + 3 raw scalars**: culture *rate*, science *rate*, strength, plus raw `culture`, `civil_actions` (`rival_free_ca`) and `len(completed_wonders)` (`rival_wonders`). Their science/food/resource stocks, MA, free workers, yellow bank, colonies and wonder-in-progress remain invisible (§0b.3) |
| 17 | Turn order / how many opponent turns before mine | YES `engine/state.py:117-119` | **NO** (only aggregate `rounds_left`) |
| 18 | Civil deck order (HIDDEN) | YES, in the clear `engine/state.py:122` | **NO** by features; read by `end_turn` trials — re-measured 2026-07-29 at **94.2%** leaky at 2p and 92.0% at 3p, and **it now changes the move** (§6.1) |

---

## 2. The card row (question 1)

### 2.1 Representation and the actual cost rule, from the code

* `card_row` is a flat `list` of **13** slots holding card *names* or `None` —
  `engine/state.py:124`, `engine/actions.py:33` (`ROW_SIZE = 13`).
* Cost by slot is a hard-coded tuple, `engine/actions.py:36`:
  `ROW_COST = (1, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3)` — **slots 0-4 cost 1 CA,
  slots 5-8 cost 2 CA, slots 9-12 cost 3 CA**, with the same thing spelled as a
  function at `engine/actions.py:39-45` and the hot path using the tuple at
  `engine/actions.py:133`. This matches `docs/HEURISTICS.md:2178-2188` and
  `docs/OPEN_QUESTIONS.md:38`, and is player-count independent. Verified from
  code, not from memory.
* Surcharges on top of the slot cost, `engine/actions.py:79-89`: a **wonder**
  costs +1 CA per completed *or destroyed* wonder you own (waived for
  Michelangelo); a **leader** costs -1 for Hammurabi; floored at 0.
* Taking a card does **not** compact the row — `take_card` just nulls the slot
  (`engine/actions.py:676`) and `_deal` refills holes from the right
  (`engine/game.py:129-140`).
* The row compacts only at **replenish**, which happens at the start of *every
  player's turn* from round 2 (`engine/game.py:219-220`): discard the leftmost
  `SWEEP[n]`, slide everything left, refill from the right
  (`engine/game.py:108-121`), with `SWEEP = {2: 3, 3: 2, 4: 1}`
  (`engine/game.py:41`).

**Therefore the slide is arithmetically predictable.** Between two of my
consecutive turns there are exactly `num_players` replenishes, so a card drops
`num_players * SWEEP[num_players]` slots from sweeping alone — **6 at 2p, 6 at
3p, 4 at 4p** — plus one more slot for every card to its left that somebody
takes.

Measured on 12 champion self-play games (§7.3), tracking every row card from one
of my turns to my next:

```
2p   slot  cost   n    survives to my next turn   its cost then
      6    2CA   213        74.6%                 1CA 93%
      7    2CA   211        87.2%                 1CA 94%
      8    2CA   213        88.7%                 1CA 95%
      9    3CA   215        91.6%                 1CA 96%
     10    3CA   218        90.4%                 1CA 95%
     11    3CA   218        95.4%                 1CA 46% / 2CA 50%
     12    3CA   219        91.3%                 2CA 79%
```

(Slots 0-5 at 2p are swept with certainty; the 3-9% "survival" there is a name
collision with a freshly dealt duplicate — `Rich Land`, `Reserves`,
`Military Bonus` etc. — and the tell is that those "survivors" got *more*
expensive, i.e. they are new copies dealt to the right.)

Read that table as: **at 2p, a card sitting at 3 CA is 91.6% likely to still be
there next turn at 1 CA.** Paying 3 CA buys you a ~8% insurance policy for
2 civil actions. That is precisely the expert claim in
`docs/EXPERT_STRATEGY.md:546` ("let cards slide deliberately") and
`docs/EXPERT_STRATEGY.md:550`, now measured against this engine. 3p is the same
shape (slot 9 survives 85.4%, lands at 1 CA 88% of the time) because
`3 * SWEEP[3] == 2 * SWEEP[2] == 6`.

### 2.2 What reads the row today

**Updated 2026-07-29.** `row_pressure` (`engine/bots/weighted.py:718-777`) now
implements GAP 2 almost exactly as proposed below, including the exact slide
arithmetic and the per-rival legality gate from GAP 3 — but it deliberately did
*not* bake in the §2.1 survival table, using one flat `RIVAL_TAKE_P = 0.25`
instead, on the stated grounds that the table was fitted on row-blind opponents
(`weighted.py:701-715`). Its two outputs are computed in `evaluate()` and are
skipped when their scale is 0.0, which is the default. So the paragraph below is
still true of `features()` and still true of every frozen champion; it is false
of the live 3p and 4p league arms. See §0b.

*Original text, 2026-07-27:*

* **`features()` reads nothing about the row.** Grep: `card_row` appears in
  `advisor/`, `tools/`, `analysis/`, `experiments/`, `tests/` and
  `engine/bots/fastcopy.py:85` (as a field name to copy) — and in exactly two
  bot decision paths, neither of them the evaluator:
  * `engine/bots/book.py:636-683` (`BookBot._best_take`) — the hand-written rule
    bot **does** read the slot cost, subtracts `cost * 3.0` (v1) or a convex
    `V2_PRICE_LADDER` (v2), refuses leaders at 3 CA before Age III, and refuses
    Taj Mahal / Great Wall above 1 CA. BookBot is the bot that *beats* the
    trained WeightedBot 62.9% ± 4.7% (`docs/BOT_ARCHITECTURE.md:195-199`).
  * `engine/bots/variants/base.py:518-527` — the rule-bot variants gate takes on
    a per-age `max_take_cost` table.
* So the two families that read row cost are the rule bots; the *learned*
  evaluator does not.
* Nothing anywhere represents "this card will get cheaper if I wait" or "an
  opponent is likely to take this". Grep for `sweep`, `slide`, `contention`
  across `engine/bots/` returns only a comment in
  `engine/bots/weighted.py:221` explaining the game-length horizon.

---

## 3. Opponent hands (question 2)

* The engine keeps two separate lists per player: `hand_civil` and
  `hand_military` (`engine/state.py:60-61`). It does **not** mark either as
  hidden. There is no information-set abstraction anywhere in the engine — this
  was already established in `docs/BOT_ARCHITECTURE.md:208-213`.
* **The rules make `hand_civil` public**: `docs/RULES_SPEC.md:71` — "2.6 Cards
  taken are public knowledge (open civil cards convention) [RB p.7]". So reading
  `state.players[j].hand_civil` is **not cheating**; it is free, legal, complete
  information that the engine already stores exactly.
* Military cards are drawn hidden (`docs/RULES_SPEC.md:196`); only the *count* is
  public, which `plan.determinize` correctly identifies
  (`engine/bots/plan.py:83-88`).
* Recoverability of the public part does not even require a log — the field is
  right there. (There *is* a `state.log` with `took {name}` lines,
  `engine/actions.py:687`, but it is truncated to 400 entries
  (`engine/state.py:201-202`), suppressed during trials
  (`engine/state.py:198-199`), and nothing reads it during play.)
* ~~**No feature reads any opponent hand — contents or size.**~~ **Superseded
  2026-07-29.** Two terms now read the rival civil hand:
  `rival_hand_civil` (size, via `hand_size` so the app harness's `hidden_civil`
  counts, `weighted.py:459`) and `rival_hand_potential` (contents, priced
  through the same weight vector, `weighted.py:667-692`). The size term is a
  `features()` key; the contents term is eval-only and dark at scale 0.0.
  Measured: emptying rival civil hands moves `features()` at 33/33 positions and
  the evaluation at 12/12 under both the live 3p and live 4p champions;
  *replacing* the contents with different cards moves `features()` at **0/33**
  and the evaluation at 9/12 (3p) and 8/12 (4p).
* **The legality of that is verified, not assumed.** `docs/RULES_SPEC.md:71`
  reads verbatim: *"2.6 Cards taken are public knowledge (open civil cards
  convention) [RB p.7]."* Reading `q.hand_civil` is therefore free public
  information, not a cheat. The docstring on `rival_hand_potential` cites this
  correctly.
* **Nothing reads a rival's military hand.** Re-verified 2026-07-29 by
  measurement, not grep: emptying every rival's `hand_military` leaves
  `features()` bit-identical at **35/35** positions and the evaluation
  bit-identical under **all seven** weight vectors (default, three frozen, three
  live). Every `hand_military` read in `weighted.py` is `p = state.players[idx]`
  — self. `engine/bots/neural_encode.py:230` is explicit about the same rule
  (*"military hand contents: mine only; rival gets a zero-vector"*), and
  `book.py:216` reads only the acting bot's own hand. Military cards are drawn
  hidden (`docs/RULES_SPEC.md:196`); only the count is public.
* The one place opponent military hands *are* read is QuiescentBot resolving a
  defender's `defense` decision — `docs/DEEPER_SEARCH.md:507-512` states this
  openly and calls it indefensible for play against a human. Still true: the
  defender's legal `defense` moves are enumerated from its real hand inside the
  search. That is a *search* cheat, not an evaluator cheat, and it is the only
  one in the repo.

---

## 4. Deck composition and card counting (question 3)

* **The engine knows the composition exactly.** `CardDB.civil_deck(age, n)` and
  `CardDB.military_deck(age, n)` expand per-player-count `count` fields into the
  full multiset (`engine/cards.py:155-175`), cached. Measured composition (2p):
  Age A civil 20 cards / 17 distinct; Ages I, II, III civil 44 cards each
  (33/36/35 distinct); military 10 / 43 / 46 / 41.
* **Can a bot compute what remains unseen?** For the *military* deck, yes and
  cleanly: `state.discarded_military` is a per-age list
  (`engine/state.py:132`, written in `engine/economy.py:186`) and it is
  reshuffled into a fresh deck when the deck empties (`engine/economy.py:191-197`
  = `docs/RULES_SPEC.md:196`). For the **civil** deck, **no**: `_replenish`
  destroys swept cards by writing `None` over the slot
  (`engine/game.py:117-118`) and there is no `civil_discard` anywhere (grep:
  zero hits in `engine/`). *The engine throws away public information a human at
  the table can see.* Closing that is a one-line change plus a state field.
* **Does anything do so?** Only in the crudest possible form: `rounds_left`
  (`engine/bots/weighted.py:264-276`) uses `len(state.civil_deck)` plus a
  precomputed tail of *future*-age deck sizes (`_tail`,
  `engine/bots/weighted.py:243-252`) to estimate how many rounds remain. That is
  a count, never a composition, and it is the only deck-derived quantity in the
  evaluation. Test F in §0 confirms it: replacing every card in `civil_deck` with
  "Bronze" leaves the evaluation bit-identical.
* **Re-verified 2026-07-29, and separated properly.** Over 24 positions and all
  seven weight vectors: replacing every civil-deck card with one name moves the
  evaluation at **0/24** positions; *truncating* the deck by 20 cards moves it at
  **22/24** (mean -2.81 under the live champions, max |Δ| 14.8). The count is
  read, the composition is not, and the count is read only as a game clock —
  `len(state.civil_deck)` → `rounds_left` → `lateness()` → the 20 phase weights.
  **There is still no card counting of any kind, at any player count, under any
  weight vector.**

### 4.1 The military/politics deck specifically — the owner's first item

This is the biggest single blind spot in the audit.

* "Prepare an event" is `engine/actions.py:986-995`: the card leaves
  `hand_military`, you score its age level in culture, it is appended to
  `state.future_events`, **and `state.seeded_by[name] = p.idx` records who put
  it there**. Rules: `docs/RULES_SPEC.md:117` — face down, so *you* know what you
  seeded and nobody else does.
* When the current-events deck empties, `future_events` is shuffled, sorted so
  earlier ages resolve first, and becomes the new `current_events`
  (`engine/events.py:149-158`); cards are revealed by popping the end
  (`engine/events.py:134`).
* `seeded_by` is keyed by card *name*, and that is safe: `CardDB` rejects
  duplicate names (`engine/cards.py:83-84`) and disambiguates same-named cards
  with an age suffix (`engine/cards.py:60-75`). Verified: no event or territory
  name appears twice in any age's military deck (the only duplicated military
  names are units, aggressions and Military Bonus).
* **Nothing in the linear bot reads any of it, and that has not changed.**
  Re-measured 2026-07-29: wiping `future_events`, `current_events`,
  `past_events`, `scoring_events` **and** `seeded_by` together moves no
  `features()` key at any of 24 positions, and moves the evaluation at 0/35
  positions under all seven weight vectors including both live champions. This
  is the owner's *first-listed* skill and it remains the single largest
  untouched blind spot; GAP 4 below is unstarted.
* The **neural** encoder does read it, and reads it legally: `seeded_n` /
  `seeded_lv` are filtered to `owner == idx` (`neural_encode.py:182-188`) and it
  explicitly does not encode other players' seeds or the current-events order
  (`neural_encode.py:29-32`). So the legal design exists; the linear evaluator
  the league trains has not adopted it.
* Consequence for the bot's play: seeding an event is scored purely by the
  immediate culture gain and the military card leaving hand. A `Good Harvest`
  you planted and a `Barbarians` you planted are the same move. The bot cannot
  prepare for its own events, cannot avoid walking into one it seeded, and
  cannot notice that an Age III `War over Culture` is waiting in the current
  deck.

---

## 5. Opponent boards (question 4)

Boards are fully public and the engine models them completely — `PlayerState`
carries `techs` (with per-card worker and blue-token counts), `government`,
`leader`, `wonder`/`completed_wonders`/`destroyed_wonders`, `tactic`, `colonies`,
`pacts`, `yellow_bank`, `workers_free`, `blue_total`, `food`, `resources`,
`science`, `culture`, `civil_actions`, `military_actions`
(`engine/state.py:43-101`).

What the feature set actually reads about a rival, exhaustively:

1. `q.culture` — raw, as `rival_culture` (max) and `rival_mean_culture` (mean),
   `engine/bots/weighted.py:404-406,476-477`.
2. `effects.compute(state, q).culture` — their culture *rate*, max over rivals.
3. `effects.compute(state, q).science` — their science *rate*, max over rivals.
4. `effects.compute(state, q).strength` — their military strength, max over
   rivals. (2, 3, 4 all from `rival_context`, `engine/bots/weighted.py:176-192`.)
5. `len(state.players[other].completed_wonders)`, but *only* inside the pact
   effect `cultureProductionPerCompletedWonderOfTheOtherParty`
   (`engine/bots/weighted.py:114-118`).
6. Pact membership, and only for pacts I am a party to
   (`engine/bots/weighted.py:368-376`).

So (as of 2026-07-27): **three derived rates plus one raw score.**

**Updated 2026-07-29.** Three more raw scalars and one non-linear term were
added, all of them public:

7. `q.civil_actions`, max over rivals, as `rival_free_ca` (`weighted.py:458`).
8. `q.hand_size("civil")`, max, as `rival_hand_civil` (`weighted.py:459`).
9. `len(q.completed_wonders)`, max, as `rival_wonders` (`weighted.py:460`).
10. `q.hand_civil` *contents*, priced through `w`, as `rival_hand_potential`
    (eval-only, `weighted.py:667-692`).

Plus, for the row legality gate only and never scored directly, `_RivalView`
snapshots `q.wonder is None`, `q.taken_leader_ages`, `q.techs` and
`q.government` (`weighted.py:190-201`).

Still invisible, confirmed field by field over 35 positions (§0b.3): their
**science stock, food stock, resource stock, military actions, free workers,
yellow bank, colonies, destroyed wonders, and whether they have a wonder under
construction**. Also still invisible: their happiness margin, and their
government and leader except insofar as those roll up into the three rates.
`q.wonder` is the sharpest omission — the engine already snapshots it for
legality, and "they cannot take a wonder while one is unfinished" is the exact
signal `docs/EXPERT_STRATEGY.md:546` says to play around.

---

## 6. The information-set question (question 5)

Honest answer: **the bot has no notion of known vs unknown anywhere in the
engine or in the evaluator.** The only place in the repo with an explicit
information-set concept is the *human advisor*, which wraps `GameState` in a
`Board` carrying `hidden: {(player, "civil"|"military"): count}` and an
`unknown: set` of explicitly-undeclared fields (`advisor/state_io.py:158-184`,
`advisor/state_io.py:265-275`). That machinery is not available to the bots.

Where determinization stands:

* `engine/bots/plan.py:82-94` re-shuffles `civil_deck` and `military_deck` and
  nothing else. Its docstring is accurate about *why* nothing else was needed:
  the evaluator reads only public rival aggregates, so nothing hidden is
  currently readable.
* Things that determinization would have to fix but does not, and which are
  therefore assumptions of perfect information waiting to become live:
  1. **Rival military hands are left at their true contents.** A correct
     determinization pools every rival's `hand_military` back into the deck and
     re-deals to the same counts. `docs/BOT_ARCHITECTURE.md:1003` already flags
     this. It matters the moment anything prices rival military cards, or the
     moment quiescence resolves a rival `defense`.
  2. **`current_events` is left in its true order** and is *not* shuffled. A
     search that runs `prepare_event` or reaches another player's politics phase
     reads the real next event. Note that `tools/infoleak.py:76-89` measures
     only civil-deck draws, military-deck draws, own-military-hand growth and
     row reveals — **it does not measure event reveals at all**, so the size of
     this one is *unmeasured*, not small.
  3. **`future_events` contents other than my own seeds are readable.** A
     correct view keeps `{n for n in future_events if seeded_by[n] == me}` and
     samples the rest from the unseen military multiset.
* The known leak, already measured and already documented, is
  `docs/BOT_ARCHITECTURE.md:208-278`: 5.46% of candidates draw from a real deck,
  71.1% of decisions have at least one such candidate, `end_turn` is 94.9%
  leaky — but re-shuffling changed the chosen move 0 times in 3,957 decisions,
  because the evaluator is too blind to read what it peeked at. **Every gap
  closed in §7 below makes that gun loaded.** Anything that teaches the bot to
  value card identity in the row or the deck must ship with determinization of
  the corresponding deck in the same change.

### 6.1 The gun is now loaded — measured 2026-07-29

The warning above was written as a prediction. It has come true, and the amount
is small but no longer zero. `row_urgency` / `row_bargain_forgone` read
`state.card_row`, which is public *at the root* — but an `end_turn` trial runs
`_replenish`, which deals the **real** next civil cards into the row, and the
row terms then price them. That is the exact mechanism §6 warned about, and
`row_pressure`'s own docstring (`weighted.py:743`) asserts the opposite
("does not load the `end_turn` information leak"). That assertion is wrong for
`end_turn` specifically, and correct for every other move.

Re-run of `tools/infoleak.py` (it still works, unmodified):

```
2p,  8 games   1046 decisions / 12421 candidates   end_turn 94.2% leaky
                                                   (was 94.9%)
               61.1% of decisions have >=1 leaky candidate
3p,  5 games   1160 decisions / 13213 candidates   end_turn 92.0% leaky
               86.2% of decisions have >=1 leaky candidate
```

Re-run of `tools/leak_impact.py`, which re-shuffles `civil_deck` and
`military_deck` K times and asks whether the *chosen move* changes:

```
weights                       decisions   move changed        end_turn cheat-minus-honest
DEFAULT (2p, 10 games, K=6)        1345   0 = 0.00% +/-0.00%  mean +0.000, sd 0.000
                                                              within-decision sd 0.000
live 3p champion, gen 1139         2281   10 = 0.44% +/-0.27% mean -0.012, sd 0.600
  (8 games, K=6)                          honest-eval loss of  within-decision sd 0.315
                                          the cheating pick
                                          -0.570 mean
live 4p champion, gen 308          2958   2 = 0.07% +/-0.09%  mean +0.000, sd 0.006
  (6 games, K=6)                          (CI includes zero)   within-decision sd 0.003
```

Read the **last column**, not the first. Under `DEFAULT_WEIGHTS` the `end_turn`
score is *identically* 0.000 across every determinization — the evaluator
cannot see the cards it peeked at, so the leak is provably inert, exactly as in
2026-07-27. Under the row-aware 3p champion the within-decision spread across
determinizations is **0.315 eval points**, which is not a noise question: it is
structurally non-zero because a term now reads the revealed cards. The
move-flip rate of 0.44% +/- 0.27% (n=2281, 95% CI 0.17%-0.71%) excludes zero,
but it is a *point* estimate on 10 events and should be treated as "small and
real", not as a magnitude. The 4p arm tracks its own tiny row scales exactly:
within-decision sd 0.003 and a move-flip rate whose CI still includes zero
(2/2958). **The size of the leak's effect scales with the row weights**, which
is the mechanism, and the 3p row weights are the ones the league is currently
driving upward.

**No BookBot anchor-leak fix has landed.** The commit named in the request,
`9794bd7` ("Desktop: make training invisible while the owner games, and stop
leaking drivers"), is a Windows scheduled-task and GPU-guard fix — the "leaking"
in its subject is leaked driver processes, not information. `6e5061e`
("Loop tuning: BookBot anchor + gentler fine-tune") is the neural loop's
opponent anchor and does not touch determinization either. Nothing in the repo
has changed `plan.determinize` or the trial-copy path since this audit was
written; the leak is unfixed and, as of the 3p arm, live.

**Required fix, and it is now urgent rather than prophylactic:** `end_turn`
candidates must be scored against a re-shuffled `civil_deck`, or the row terms
must be computed on the *root* row rather than the post-move row. The second is
cheaper and is arguably more correct anyway — `row_urgency` asks "what will the
sweep destroy before I act again", which is a question about the row I can see.

---

## 7. Ranked gaps and bounded proposals

Ordered by (value / implementation cost). Each is a feature-level change; none
requires a new search architecture.

### 7.0 Status and re-ranking, 2026-07-29

| gap | 2026-07-27 rank | status | evidence |
|---|---|---|---|
| GAP 1 — row depth priced with the wrong sign | 1 | **SHIPPED and fitted.** `take_cost_paid` exists (`weighted.py:500`, responds 24/24) and the live 3p `ca_left` has gone from -0.0974 to **+0.6072** | §0b.1 |
| GAP 2 — take now vs let it slide | 2 | **SHIPPED as code, fitted only at 3p.** `row_pressure` (`weighted.py:718-777`) implements the exact slide plus the legality gate; scales are 0.0 in DEFAULT, all frozen champions and the 2p arm, +0.106/+1.516 at 3p, ~0.002/0.024 at 4p | §0b.1-2 |
| GAP 3 — opponent hands and boards invisible | 3 | **PARTIALLY SHIPPED.** 4 of the 6 proposed terms exist (`rival_free_ca`, `rival_hand_civil`, `rival_wonders`, `rival_hand_potential`). `rival_best_tech_level` and `rival_happy_margin` were not built; nor was the desire model — `RIVAL_TAKE_P` is one flat 0.25 | §0b.3, §5 |
| GAP 4 — politics/event deck invisible | 4 | **UNSTARTED.** 0/24 positions, 0/35 evaluations, all seven weight vectors | §0b.4, §4.1 |
| GAP 5 — no civil discard record | 5 | **UNSTARTED.** No `civil_discard` field; deck composition moves nothing, 0/24 | §0b.4, §4 |
| GAP 6 — military hand identity | 6 | **UNSTARTED.** `hand_mil_value` is still a level sum | §3 |

**Re-ranked remaining work**, by expected value per unit of cost, given what is
now measured rather than what was predicted:

1. **Fix the `end_turn` row leak (§6.1).** New, and it now outranks everything
   because it is a *correctness* bug rather than a strength gap: the 3p arm is
   currently being trained against a signal it will not have against a human.
   Cost is a few lines (score the row terms on the root row, or shuffle
   `civil_deck` in the trial). Do this before adding any further row or deck
   feature.
2. **GAP 4 — events.** Still the largest untouched blind spot and the owner's
   first-listed skill. The evaluator has **zero** notion of any event, including
   ones it seeded itself, verified by measurement across all seven weight
   vectors. The neural encoder already demonstrates the legal masking
   (`neural_encode.py:182-188`), so the design question is settled and only the
   linear feature is missing. Must ship with the `current_events` shuffle in
   `plan.determinize`.
3. **Turn on GAP 2/GAP 3 at 2p and 4p.** The code is written and free; the
   scales are simply still at 0.0 in two of three arms, and at 4p the fitted
   scales are ~0.002 — i.e. the trainer has not yet found them. This is a
   training-schedule question, not an engineering one, and it costs nothing but
   generations. The 3p arm is the existence proof that the terms are reachable.
4. **The seven missing public rival board fields (§0b.3).** Their wonder-in-
   progress flag first — it is already snapshotted into `_RivalView.wonder` for
   the legality gate, so scoring it is one line, and it is the exact "is this
   wonder safe to let slide" signal in `docs/EXPERT_STRATEGY.md:546`. Then
   `rival_happy_margin` and `rival_best_tech_level` from the original GAP 3.
5. **GAP 5 — civil discard record**, which is the only prerequisite for any
   principled card counting. Nearly free (one state field), no immediate payoff.
6. **A rival *desire* model** to replace the flat `RIVAL_TAKE_P = 0.25`. Real
   value but the highest cost of anything here, and worth nothing until the
   arms have actually fitted the legality-gated version above it.
7. **GAP 6 — military hand identity.** Unchanged in rank; it is the one item
   that would make a *second* leak live (rival military hands in
   `determinize`), so it must not ship before item 1's discipline is in place.

*The original 2026-07-27 gap entries follow unchanged, for the proposals and
the derivations. Read them with the status table above.*

### GAP 1 — Row depth is priced by a single scalar with the wrong sign. *(highest value, lowest cost)* — SHIPPED

**Evidence.** §0, §2.2, §7.2. The only path from slot cost to evaluation is
`ca_left` (`engine/bots/weighted.py:439`); its 3p champion weight is -0.0974, so
paying 3 CA instead of 1 for the identical card *raises* the score by 0.195.
Measured downstream behaviour: 56.9% band-3 picks and 2.33 CA/card at 3p
(`docs/HEURISTICS_PROGRESS.md:119-121`) against 76%-at-1-CA in human tournaments
(`docs/EXPERT_STRATEGY.md:688`).

**Proposal — 1 feature, ~10 lines.** Add to `features()`:

```python
"take_cost_paid": <CA spent on row takes so far this turn>
```

The cleanest source is a per-turn counter set in `_h_take`
(`engine/actions.py:666-669`) — `p.ca_spent_taking += take_cost(...)`, reset in
`game.start_turn` next to `p.taken_this_turn = []`
(`engine/game.py:229`) — because `features()` is stateless and cannot see how a
CA was spent. Weight it freely and let the trainer fit it. Crucially this is a
*separate* channel from `ca_left`, so the trainer can price "a CA spent
upgrading" and "a CA spent reaching into the row" differently, which is exactly
the distinction the sources make
(`docs/EXPERT_STRATEGY.md:550`: "CAs which you spend for grabbing from the card
row increase in value as the game progresses, while it is the other way around
for CAs you spend to upgrade workers").

Guard: the field must be added to `PlayerState` (`engine/state.py`), to
`fastcopy`'s scalar list (`engine/bots/fastcopy.py`), and journalled via
`journal.touch` if it is a mutable — as an int it is journalled by the normal
scalar path. `bash tools/gate.sh` will catch a missed copy field.

**Why first:** it is the smallest diff in the audit, it has a measured wrong
sign in a live champion, and it needs no new information at all — only a second
place to hang a weight.

### GAP 2 — "Take now vs let it slide": no expected-value view of the row.

**Evidence.** §2.1-2.2. Slide is deterministic up to opponent takes; nothing
computes it.

**Proposal — 2 features + ~30 lines of pure arithmetic, no search.** Define, at
the root of a decision (once per decision, alongside `rival_context`, so the
cost is amortised over ~30 candidate moves exactly as `rival_context` already is,
`engine/bots/weighted.py:176-182`):

```python
SLIDE = num_players * game.SWEEP[live_count]      # 6 at 2p, 6 at 3p, 4 at 4p

for each occupied slot i:
    next_slot   = i - SLIDE                        # ignore opponent takes: a
                                                   # conservative UPPER bound on
                                                   # the future cost
    if next_slot < 0:   status = "dies"            # take it now or never
    else:               saving = row_cost(i) - row_cost(next_slot)
```

then two features:

* `row_urgency` = `sum(card_potential(name, w) for name in row if it dies before
  my next turn and I could legally take it)` — how much value the sweep is about
  to destroy. `card_potential` already exists and already prices a card through
  the same weight vector (`engine/bots/weighted.py:581-590`).
* `row_bargain_forgone` = `sum(saving * P_survive(i) * indicator(I want it))` —
  the civil actions I am about to overpay by not waiting.

`P_survive(i)` can start as the measured constants in §2.1 (a 7-entry table by
slot band and player count, from the run in §7.3 extended to more games) and be
replaced later by the contention model in GAP 3. Both features are computed on
the *post-move* state like everything else, so the 1-ply search will
automatically prefer the take that leaves the better row behind.

**Cost control:** the loop is ≤13 iterations and `card_potential` is
`lru_cache`d per name — but the weight vector is a dict argument, so cache it per
`(name, id(w))` or hoist the whole row valuation into `rival_context`, which is
computed once per decision, not once per candidate.

### GAP 3 — No model of opponent desire; opponent hands and boards are invisible.

**Evidence.** §3, §5, §0 tests C and D.

**What signal is actually available** (all of it legal, all of it already in
`GameState`, no sampling required):

| signal | source | what it tells you |
|---|---|---|
| they cannot take a wonder while one is unfinished | `p.wonder`, gate at `engine/actions.py:138` | wonders are safe to let slide |
| wonder surcharge = completed + destroyed wonders | `engine/actions.py:84-86` | a 3-CA wonder may be 5 CA *for them* |
| they cannot take a second leader of an age | `p.taken_leader_ages`, `engine/actions.py:147` | leaders are often safe to let slide — matches `docs/EXPERT_STRATEGY.md:546` verbatim |
| one-per-name: already in their hand or tableau | `engine/actions.py:155` | they literally cannot take it |
| their civil hand is at the limit | `engine/actions.py:122,144` | they cannot take *anything* |
| their remaining CA vs the slot cost | `p.civil_actions`, `spare_ca` `engine/actions.py:60-64` | can they reach that deep this turn |
| their board gaps (`best_farm`/`best_lab`/… computed for *me* at `weighted.py:342-362`) | `q.techs` | do they want this tech |
| their science stock vs the card's `techCost` | `q.science`, `card["techCost"]` | can they afford to play it |

**Proposal — reuse the existing machinery, ~40 lines.** `_take_gate` /
`_can_take_gated` (`engine/actions.py:112-157`) already answer "can player X
legally take slot i" for an arbitrary player and an arbitrary CA budget. So:

```python
def contention(state, idx, i):
    """P(some rival takes row slot i before my next turn), 0..1."""
    name = state.card_row[i]
    p_free = 1.0
    for q in rivals_in_turn_order_before_my_next_turn(state, idx):
        if not actions.can_take(state, q, i, budget=q_expected_ca):
            continue
        desire = card_potential(name, w) - <their board's existing best of that type>
        p_free *= 1.0 - squash(desire)
    return 1.0 - p_free
```

Feed `1 - contention(i)` into GAP 2's `P_survive`. Start with the legality gate
only (`can_take` for each rival) — that alone captures the three cases the expert
sources single out (wonders, leaders, full hands) and is *exact*, not a model.
Add the desire term second, measured separately.

Separately and independently, add the cheap opponent-board features the
evaluator is missing today, all one-liners over `rivals`:
`rival_best_tech_level`, `rival_wonders`, `rival_happy_margin`,
`rival_free_ca` (their remaining civil actions), `rival_hand_civil`
(their public civil hand size), `rival_hand_potential`
(`sum(card_potential(n, w) for n in q.hand_civil)` — legal, §3). The last one is
the direct answer to "keep track of what is in your opponents' hand": it costs
one line and reuses a function that already exists.

### GAP 4 — The politics (future events) deck is invisible, including my own seeds.

**Evidence.** §4.1, §0 test E.

**Proposal — 3 features, ~20 lines, no new state needed.** `seeded_by` already
attributes every seed. Add:

* `my_seeded_pending` = `sum(level(n) for n in current_events + future_events if
  seeded_by.get(n) == idx)` — events I planted that have not resolved yet.
* `event_threat` = the expected signed effect on *me* of the events I know are in
  the current deck, priced through `_YIELD_TO_FEATURE`
  (`engine/bots/weighted.py:78-98`), which already maps event effect keys onto
  feature keys. Restrict the sum to cards I am legally allowed to know: my own
  seeds, plus (optionally, and only under a flag) the full deck for symmetric
  self-play.
* `war_over_culture_pending` / `scoring_events_pending` — Age III scoring events
  are already broken out into `state.scoring_events` (`engine/state.py:130`) and
  are worth a feature of their own, since they change the endgame objective.

**This one must ship with a determinization change** (§6, item 2): shuffling
`current_events` in `plan.determinize` and masking `future_events` down to my own
seeds. Otherwise `event_threat` reads the true order and the bot cheats. The
change to `plan.determinize` is ~6 lines.

### GAP 5 — No civil discard record, so a legal card-counter cannot be built.

**Evidence.** §4. `engine/game.py:117-118` destroys swept cards silently.

**Proposal — 1 state field, ~4 lines.** `state.civil_discard: list` appended in
`_replenish` before nulling. Then `unseen_civil(age) = db.civil_deck(age, n) -
row - every hand - every tableau - civil_discard` is exactly computable from
public information, and the first feature to build on it is
`p_better_card_coming` (is the last copy of a critical tech still in the deck? —
`docs/EXPERT_STRATEGY.md:546`: "the last copy of some critical tech often just
has to be taken that turn, even for three CA"). Low value on its own, but it is a
prerequisite for any principled version of GAP 2's `P_survive` and it is nearly
free. Note it also makes the state strictly larger, so check `tools/bench_copy.py`
and the `fastcopy` field lists.

### GAP 6 — Military hand identity (already documented, listed for completeness).

`docs/BOT_ARCHITECTURE.md:241-257` measured four completely different military
hands producing a bit-identical evaluation, and identifies the fix as the
military mirror of `hand_potential`. Not re-derived here; it is the same class of
blindness and the same class of fix, and it is the one gap that turns the
existing measured leak live, so it must ship with rival-hand re-dealing in
`determinize` (§6, item 1).

---

## 8. Confirmation runs

All runs below were small and deliberately CPU-cheap (the box was saturated by
three training arms). Total ≈ 25 seconds of CPU. Scripts were throwaway, written
to `/tmp`, not committed.

* **§7.1 — `/tmp/invcheck.py`.** One 2p game advanced 60 plies with the default
  `WeightedBot`; then six perturbations of the resulting state and ten
  single-field rival perturbations, comparing `features()` dicts and
  `evaluate()` scalars. Results in §0. Runtime < 2 s.
* **§7.2 — `/tmp/slotprice.py`.** For each of the three frozen champions
  (`analysis/frozen/champion_{2,3,4}p.json`): advance 40 plies, then place the
  same real row card at slot 0 / 5 / 9 on a `fastcopy` of the state, apply
  `("take", slot)`, and report the evaluation delta. Results in §0. Runtime < 5 s.
* **§7.3 — `/tmp/rowfate.py`.** 6 games at 2p and 6 at 3p, frozen champions,
  tracking every row card from the start of a player's turn to the start of that
  same player's next turn. Results in §2.1. Runtime 15 s wall, 7 s CPU, run under
  `nice -n 19`. **n is small (≈210 observations per slot per count) and the
  opponents are themselves depth-blind bots, so the survival rates are
  directional, not final. A human field would take more cards and the survival
  rates would fall.**
* **Not run:** any league/arena batch, any `experiments/` job, `pickstats`. The
  behavioural numbers quoted (2.33 CA/card at 3p, 88.4% band-1 at 2p, the human
  tournament baseline) are cited from `docs/HEURISTICS_PROGRESS.md:119-121` and
  `docs/EXPERT_STRATEGY.md:688`, not re-measured.

### 8.1 The 2026-07-29 re-measurement

The original `/tmp/invcheck.py` no longer exists; it was **reconstructed from
this document's own §7.1 description** ("advance a game, apply six perturbations
plus ten single-field rival perturbations, compare `features()` dicts and
`evaluate()` scalars") and then generalised. All scripts were throwaway, written
to `/tmp`, not committed. Everything ran under `nice -n 19` alongside three live
training arms.

* **`/tmp/invcheck2.py`** — 35 positions: 2p/3p/4p x seeds 0/1/2 x four ply
  depths each, self-played by `WeightedBot` under `DEFAULT_WEIGHTS`, snapshotted
  with `fastcopy.copy_state`. Age distribution `{2p: 5 I / 3 II / 3 III;
  3p: 6 / 3 / 3; 4p: 6 I / 6 II}` — **no Age IV or Age A position was sampled**,
  and Age A in particular is where the row is shallowest, so the row results
  should be read as mid-game. Nine structural perturbations x seven weight
  vectors, plus the ten-field rival scalar sweep. Results in §0b.2-3. Runtime
  ~90 s.
* **`/tmp/invcheck3.py`** — 24 positions (seeds 0/1), one information source
  wiped at a time, recording *which* `features()` keys move. Results in §0b.4.
* Weight vectors were **snapshotted to `/tmp/snap/` before use** (live 2p gen
  16, 3p gen 1139, 4p gen 308) because the league rewrites
  `experiments/league_state/champion_*.json` continuously. Any re-run against
  the live files will see different numbers; the *frozen* and *default* columns
  are stable.
* **`tools/infoleak.py`** — re-run unmodified, 2p/8 games and 3p/5 games. It
  still works. Results in §6.1.
* **`tools/leak_impact.py`** — re-run unmodified under `DEFAULT_WEIGHTS` (2p, 10
  games), the live 3p champion (8 games) and the live 4p champion (6 games),
  K=6 determinizations each. Results in §6.1.

**Sample-size discipline.** The per-cell n in §0b.2 is 11-12 positions per
player count, which is enough to distinguish "structurally cannot respond"
(0/12 under a vector whose weight is literally 0.0) from "responds where the
term is non-zero" (8/12, with the other 4/12 explained position-by-position by
`row_pressure` returning `(0.0, 0.0)`). It is **not** enough to estimate a
magnitude precisely; the delete-row deltas are quoted with their sd for that
reason. The one number here that is a genuine small-n proportion is the 3p
move-flip rate (10 events in 2281 decisions) and it is quoted with its CI.

## 9. Things I could not verify

* Whether the military *discard pile* is public in the physical game.
  `docs/RULES_SPEC.md:188` says excess military cards are discarded "face down"
  and `docs/RULES_SPEC.md:125` says defence cards are discarded face down, which
  suggests the pile is *not* legible; but the spec does not say so explicitly.
  `state.discarded_military` should therefore be treated as hidden until this is
  settled, which affects how a military card-counter may use it.
* The size of the event-order leak (§6, item 2). `tools/infoleak.py` does not
  instrument event reveals, so this is unmeasured rather than measured-small.
  **Still true on 2026-07-29** — the tool is unchanged.
* Whether `rival_hand_potential` (GAP 3) is worth anything at 3p/4p. The civil
  `hand_potential` term itself was only validated at 2p
  (`engine/bots/weighted.py:673-677`). **Partial answer 2026-07-29:** the live
  4p champion has fitted it to **+1.329** and the live 3p champion to -0.020,
  i.e. two arms hill-climbing independently landed on opposite signs and very
  different magnitudes. That is suggestive of the 4p arm having found something
  and the 3p arm having found nothing, but a weight reached by hill climbing is
  not evidence of value — it needs a head-to-head A/B at each player count,
  which was **not run here**.
* Whether any of the shipped GAP 1/2/3 terms actually make the bot **stronger**.
  This audit measured only what the evaluator *reads*, never what it *wins*. The
  live 3p arm's advantage over its own lineage is a league statistic and is not
  attributable to these terms without an ablation.
* Whether the Age A and Age IV positions behave like the Ages I-III sampled
  here. Neither was sampled (§8.1).
