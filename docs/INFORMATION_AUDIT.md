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

### 6.2 Fixed — the root-row budget, measured 2026-07-29

The second option above shipped. `weighted.root_row_budget(state)` snapshots the
card row **as a multiset of names** at the search root; `rival_context` carries
it in `ctx["root_row"]`; `row_pressure` keeps a decremented local copy and skips
any slot whose name has no budget left.

Three details that are the whole correctness argument:

* **A multiset, not a set.** The civil decks contain duplicate card names, so a
  set-based mask would still price a freshly dealt *second* copy of a card the
  root row held once. The budget is decremented as slots are accepted.
* **Cards that merely slid left are still priced**, at their new cheaper slot.
  The slide is public arithmetic every player at the table can do, and the card
  keeps its name, so masking it would make the bot *worse than legal*.
* **The budget is threaded, never recomputed.** `quiescent.py` rebuilds `ctx`
  at four mid-search sites and `plan.py` at one; all five now pass the root
  budget down. Recomputing it from the trial state would re-open the leak for
  exactly the deep nodes that have one.

Measured against the **live** 3p champion (`experiments/league_state/champion_3p.json`,
gen 1149, `row_bargain_forgone` 1.5164 — *not* the stale `experiments/champion_3p.json`
export, gen 152, which has no row weights at all and would have shown a fake zero):

```
tools/leak_impact.py, 3p, K=6, 8 games
                  decisions   move changed         within-decision eval sd
master (leaky)         2302   11 = 0.48% +/-0.28%  0.333
root-row budget        2267    0 = 0.00%           0.005
```

`tools/infoleak.py` is **unchanged** at 92.0% `end_turn` leaky, exactly as it
should be: it counts trials that *deal* a real card, and the fix does not stop
the dealing, it stops the *reading*. Only `leak_impact.py` can measure this fix.
Anyone re-checking with `infoleak.py` alone will wrongly conclude nothing
happened.

**Residual, unfixed:** the post-fix within-decision sd is 0.005, not 0. An
`end_turn` trial also draws my own next **military** card, and the champion
prices `hand_mil_value` at 0.15. That is ~1.5% of the row terms' effect and is a
separate follow-up, not a loose end in this fix.

> **That guess was wrong.** `hand_mil_value` varies in **0 of 1583** `end_turn`
> candidates and structurally cannot vary. The residual was still the row.
> Measured and fixed in **6.4** below; read that instead.

### 6.3 What actually caught the bug in the fix, and the three layers now guarding it

Worth writing down because the fix above shipped with a bug in it, and the way
it was found says more than the fix does.

`plan.py`'s `_quiesce` got the line `rival_context(st, d, ctx.get("root_row"))`.
`_quiesce(self, st, w, cap=12)` has **no `ctx` in scope** — it runs deep inside
the beam, where the root context is long gone. So every call raised `NameError`,
the pre-existing `except Exception: dctx = dict(_NO_CTX)` two lines below
swallowed it, and PlanBot silently played *every* quiesced opponent decision
with no rival aggregates at all. 1112 raises in a single 4p game.

* **550 unit tests passed.**
* Three plausible diagnoses were chased and refuted first: a stale digest on
  master (master reproduced `441cd256` exactly), the new mask firing (turning it
  off changed nothing, and a counter wrapped around `rival_context` read **zero**
  — the `NameError` fires *before* `rival_context` is entered, so that counter
  could never have seen it), and 4p non-determinism (six master runs including
  three `PYTHONHASHSEED` values were byte-identical).
* What found it was a **file-level bisect** — revert `plan.py` alone and master's
  scores return; revert `weighted.py` alone and they do not.
* What *caught* it in the first place was the **`plan wide` fingerprint arm**,
  added one commit earlier for precisely the reason docs/PYPY.md 9.14 gives: a
  digest can only catch a change to a bot it actually plays. No other arm moved.
  It was also one decision away from being laundered: the honest-looking move
  when a fingerprint fails is to re-derive it and write "fingerprints moved, as
  expected" in the commit message.

The lesson is not "write fewer `except Exception`". The ~56 of them in `engine/`
are mostly load-bearing — a search speculatively `apply`s a move that turns out
illegal, and skipping the candidate rather than crashing is what makes a 40-hour
unattended league run survivable. The lesson is that a *swallowed* `NameError`
is never a legitimate game-state failure, it is always a bug, and nothing in the
repo could see one. Three layers now can, cheapest first, and none subsumes the
others:

| layer | file | cost | catches |
|---|---|---|---|
| static | `ruff.toml`, `ruff` arm in `tools/gate.sh` | ~200ms, no game | F821 undefined name — *this* bug, before a reproducer is even run |
| dynamic, thorough | `tools/bug_audit.py`, `bug audit` arm in the gate | ~20s, 4p x 4 bots | anything in `BUG_TYPES` raised anywhere under the repo, caught or not |
| dynamic, cheap | `tests/test_no_swallowed_bugs.py` | ~4s, in the suite | same, 2p x weighted+plan, plus the negative control |

`bug_audit.py` uses `sys.monitoring`'s `RAISE` event, which fires *before* any
`except` runs. That is what lets it see swallowed exceptions without editing a
single one of the 56 sites.

`BUG_TYPES` membership is **measured, not guessed**. `--all` over 36 games
(2p/3p/4p x seeds 0,1,2 x greedy/weighted/quiescent/plan) swallows ~44k
exceptions per 4p batch and **every one is `KeyError` or `AttributeError`** —
nothing else raises at all, so `NameError`, `TypeError`, `ImportError`,
`IndexError`, `ZeroDivisionError` and `ValueError` all sit in the strict set for
free. `AttributeError` and `KeyError` are deliberately excluded and
`tests/test_no_swallowed_bugs.py` pins that exclusion, because both are
load-bearing control flow here: `effects.state_stats` initialises
`_stats_cache` off a caught `AttributeError` (23,305 per 4p batch) and
`actions.cost_of` probes card names with a caught `KeyError` (~15k). Including
either would fail on every clean tree — a gate that cries wolf, which
docs/PYPY.md 9.0 records as expensive here.

Both layers were verified with the real bug re-introduced: `ruff` names
`plan.py:432:45`, and the unit test fails with
`NameError in engine/bots/plan.py:411 _quiesce() -- name 'ctx' is not defined (x256)`.

**Two performance findings fell out of the audit and are NOT bugs**, recorded
here because 38k exceptions per four games is not free: `effects.state_stats`
raising `AttributeError` 23,305 times to lazily initialise a cache (a class-level
default or `getattr` would remove it), and `actions.cost_of` raising `KeyError`
~15k times as name-probe flow control.

---

### 6.4 The residual closed — and the hypothesis that was wrong, measured 2026-07-29

6.2 left a within-decision eval sd of 0.005 and *guessed* at the cause: the
`end_turn` trial also draws my own next military card, and the champion prices
`hand_mil_value`. **The guess was wrong.** It was tested before it was fixed,
and the test is the most valuable thing in this subsection.

#### The hypothesis is refuted, n=1583

`evaluate` was decomposed into its additive parts — every linear feature key
times its weight, both early/late phase channels, and the four non-linear terms
priced through `w` — and each part's spread across K=6 honest re-shuffles was
recorded for every `end_turn` candidate. The decomposition was checked to sum
to `evaluate` to 1.1e-13 before it was trusted. 3p, 8 games, live champion
`experiments/league_state/champion_3p.json` at gen 1166:

```
component                  varies    rate    mean sd
row_bargain_forgone         11/1583  0.70%    0.4478      <- the entire residual
(every other component)      0/1583  0.00%    0.0000
```

`hand_mil_value` moves in **0 of 1583**, and cannot move at all. The reason is
structural, not statistical: `_meta()` maps a card to `(type, level(age))`, so
`hand_mil_value` prices only a drawn card's **age**, and a military deck holds
exactly one age — 1633 live `military_deck` snapshots across 6 games at 3p, **0
with mixed age levels**. Shuffling the military deck therefore cannot change
the feature. `hand_military` is a count, and a count is legally knowable. The
military draw is a real *deal* of hidden information; the evaluator simply
cannot read it.

**The answer was already written down in this repo.**
`docs/BOT_ARCHITECTURE.md:243-252` measured it directly and years-old-style
plainly: four completely different determinized military hands, one identical
`hand_mil_value 6` and one identical eval `37.517925`, because "`Crusades` and
`Rats` are literally the same feature vector". 6.2 proposed a cause that an
existing measurement in a neighbouring doc had already ruled out. The cheapest
step in this whole subsection was grepping for the feature name before writing
code — worth more than the fix.

**This is a loaded gun, not a closed one.** The moment anything prices military
card *identity* — strength, type, `Aggression` vs `Defence` — the `end_turn`
military draw becomes a live leak with no mask in front of it, exactly as §6
predicted for the row. GAP 6 is where that belongs.

#### The real cause: the multiset budget's swept-donor hole

6.2's mask is a **multiset**, and the leak survived in the cards that *leave*
the row. `_replenish` destroys the leftmost `_sweep_count` slots, so a swept
root card never spends its own budget entry — and a freshly dealt card with the
same name spends it instead and gets priced. Only `row_bargain_forgone` was
affected, and that is mechanistic too: dealt cards land in the **rightmost**
slots, so `i - slide >= 0` and they can never reach the `row_urgency` branch,
which is reserved for cards the next sweep destroys.

The fix follows 6.2's principle. `root_row_budget` now returns the root row's
names **in row order** as a tuple, and `row_pressure` walks it with a
**forward-only cursor**: skipping root names to find a match tolerates swept
cards, taken cards and the holes they leave (all public arithmetic), while
consuming each name at most once *in order* stops a dealt card reusing the name
of a card that was swept off the left. It rests on an engine invariant —
`_replenish` and `interact._finish_take_row` both compact survivors to a prefix
in order and `_deal` fills only the slots behind them, so **every dealt card is
strictly right of every survivor** — which is now pinned directly by
`test_replenish_keeps_survivors_as_a_prefix` at 2p/3p/4p, so a future change
that deals into a middle hole fails loudly instead of silently over-masking.

#### Measured, before and after

Both runs are the same command against the same **snapshot** of the live
champion (gen 1166, copied to a fixed file — the league rewrites that file
mid-run, and 6.2's 0.005 was measured at gen 1149, which is why the "before"
here reads 0.003: the residual's size is a function of the row weights, and
they move). `experiments/champion_3p.json` remains a stale export with no row
weights that would show a fake zero.

```
tools/leak_impact.py --players 3 --games 8 --k 6, champion gen 1166
                     decisions  move changed              within-decision eval sd
multiset (6.2)            2264  0 = 0.00%  (0/2264)       0.003
ordered cursor (6.4)      2264  0 = 0.00%  (0/2264)       0.000  exactly
                                end_turn candidates: 1583 in both runs
                                cheat - determinized mean: -0.003 -> -0.000
                                                      sd:  0.039 -> 0.000
```

Read this honestly:

* **The move-flip rate did not improve, because it was already 0.** At this
  residual size no move flipped either way: 0 of 2264, whose one-sided 95%
  bound is 0.13% (rule of three), so anything under ~3 flips is invisible at
  this n. The 0.48% flip rate in 6.2's table belonged to the *unmasked* leak,
  which was ~100x larger. Claiming a move-quality win here would be reading
  small-n noise, the mistake this repo has made six times.
* **What did improve is provable, not statistical.** The eval spread is now
  *identically* 0.000 across every determinization, with the per-component
  breakdown confirming 0 of 1583 candidates move any component at all. At 1 ply
  the evaluator is now demonstrably a function of legal information only —
  the same standing this file gave `DEFAULT_WEIGHTS` in 6.1, now earned with
  real row weights fitted.
* `tools/infoleak.py` is unchanged and irrelevant here, for 6.2's reason: it
  counts trials that *deal* a card, not trials that *read* one.

#### A second, smaller behaviour change in the same edit

6.2's mask was disabled by a *falsy* budget (`dict(budget) if budget else None`),
so a genuinely **empty** root row masked nothing and every dealt card was
priced — the worst case, since with an empty root row *every* card present was
dealt. The tuple form distinguishes the two: `None` (a caller-built ctx or the
degraded no-ctx path) still masks nothing by design, while `()` masks
everything. Pinned by `test_empty_root_row_masks_everything`. An empty row is
rare mid-game, which is why it did not show up in the numbers above.

#### One hole knowingly left open, and pinned

The cursor is an **upper bound** on the survivors, not an identity. It retires
a root name only by accepting a slot to that name's right, so it sees
departures from the **left** (swept) but not from the **right** (a card a rival
took). A card dealt later sharing a *taken* card's name is still priced —
demonstrated, and pinned by
`test_known_hole_a_taken_card_can_still_lend_its_name`, which is written to
fail if someone closes it so the caveat cannot rot.

It needs a take *and* a turn boundary in the same trial, so it is unreachable
at 1 ply, invisible to `leak_impact.py`, and reachable only in quiescent/plan
search. Closing it exactly needs **provenance on the row slots** — names alone
cannot tell a survivor from a duplicate of itself, and the two cheap proxies
both fail: a deck-size delta miscounts once a dealt card is itself swept, and
widening the mask over-masks public cards, which 6.2 records as being as much a
bug as leaking (rival civil hands are public, RULES_SPEC.md:71).

#### Inert, as required

The change is inert under `DEFAULT_WEIGHTS`, which price `row_urgency` and
`row_bargain_forgone` at 0.0 and skip `row_pressure` entirely. `bash
tools/gate.sh` prints GATE PASS with all 14 digests unchanged — narrow
`0a6ed6ad`, wide `4a8c6ca6`, weighted `302c546c`/`4e40a58c`, quiescent
`0e90a7e6`/`41f078e5`, plan `ad64a55b`/`441cd256`. No digest was re-derived;
6.3 records what re-blessing a moved digest costs.

### 6.5 Did closing the leak make the champion WORSE? — paired A/B, 2026-07-29

§6.2 measured that the fix *works*. That is a correctness measurement, and
strength is a different question, because **the champions were hill-climbed WITH
the leak**: every accept decision that drove `row_bargain_forgone` to 1.65 at 3p
was taken by a bot whose row terms could see cards it had not been dealt. If any
of that weight was fitted to the leak rather than to the game, removing the leak
should cost strength until the league re-adapts — and that bears on a pending
decision about rolling the 3p arm back. `tools/deleak_ab.py` measures it.

The result stated up front: **no detectable loss of strength at either player
count, at the n run here.** Numbers and the size of "detectable" below.

#### The design

* **Paired seeds.** Both arms play the same `seed0`, so game *g* of the leaky arm
  and game *g* of the de-leaked arm are the same deal, the same seat rotation and
  the same opponent draw. Every statistic is `mean(de-leaked − leaky)` over
  **per-seed differences**, with the SE of that difference — not the difference
  of two independent means, which would throw the pairing away and need roughly
  an order of magnitude more games for the same error bar.
* **The leaky arm wraps only the challenger**, and only for the duration of its
  own `__call__`. A global patch would de-leak the *defenders* too, and the
  mirror tier is the same policy family reading the same row weights, so a global
  patch measures "everyone lost the leak at once" — under which a win share can
  stay flat while every bot at the table gets worse.
* **Complete seat rotations only** (`--games` must be a multiple of `--players`).
  At the mirror this also gives an exact reference: identical policies in every
  seat sum to 1 over a rotation, so the de-leaked mirror margin is **0.000 by
  construction** and all of the paired variance comes from the leaky arm.
* **Live champions, snapshotted first.** `experiments/league_state/champion_2p.json`
  (**gen 24**) and `champion_3p.json` (**gen 1169**), copied to `/tmp` before use
  because the arms rewrite them every generation. *Not*
  `experiments/champion_3p.json`, which is a stale gen-152 export with **no row
  weights at all** and would have shown a fake zero.
* **The architecture must match the arm that fitted the weights**, or the run
  measures a searcher gap instead of the leak. Read off the live processes: the
  3p arm is `--candidate-bot quiescent:levels=1`, the 2p arm is
  `--candidate-bot plan:width=2`.

#### Two validations, because the whole thing rests on the leaky arm being real

1. **`--verify-prefix`** plays the same BookBot seeds in three trees — an actual
   checkout of the pre-fix commit (`git archive 0bec288^`), this tool's leaky
   arm, and de-leaked master. BookBot never touches `row_pressure`, so the
   defender is identical in all three and any difference belongs to the
   challenger. Run **per architecture**, because `plan.py`'s threading is a
   different code path from `quiescent.py`'s and is exactly where `0bec288`
   shipped the `NameError` of §6.3:

   ```
   24 games each                  pre-fix == leaky arm    de-leaked differs
   3p, quiescent:levels=1              24/24                   10/24
   2p, plan:width=2                    24/24                    2/24
   ```

   So the leaky arm is not an approximation of the pre-fix code, it *is* the
   pre-fix code, and the fix is live on these seeds.
2. **`--placebo`** runs the entire `("leaky", ...)` wrapper path with the *real*
   `root_row_budget` restored. **0 of 24 pairs diverged**, confirming the harness
   itself is deterministic and that divergence in the real arms is the leak
   rather than run-to-run noise.

**Which version of the fix was measured.** The duels were played against the
**multiset** mask of 6.2 (`f0e8b1e`). 6.4 above then replaced it with the
ordered cursor, which raises the obvious question of whether these numbers still
describe master. They do, and this was checked rather than assumed: the
de-leaked arm's **first 150 games of the real A/B seed set** (3p vs `book`,
`seed0` 279816) were replayed under the ordered cursor and came back
**150/150 byte-identical** to the recorded series. `--verify-prefix` re-run on
master likewise reproduces the pre-fix tree 24/24 with the same 10/24 differing
and the same mean own culture 198.6250. The two fixes are the same policy on
these games, so every table below applies to master unchanged. Consistent with
6.4's own measurement that the ordered cursor changed 0 of 2,264 chosen moves.

#### Result — 3p, `quiescent:levels=1`, champion gen 1169 (`row_bargain_forgone` 1.65171)

Every cell is `de-leaked − leaky` on the same seed. **Negative = the fix cost
strength.** `±` is 1.96·SE of the paired difference; `z` is mean/SE.

| opponent | n pairs | diverged | Δ win share | Δ culture margin | Δ own culture | Δ defender culture |
|---|---|---|---|---|---|---|
| mirror | 900 | 359 (39.9%) | −0.0056 ±0.0184 (z −0.59) | −0.78 ±1.76 (−0.87) | −1.98 ±1.57 (−2.47) | −1.20 ±1.16 (−2.03) |
| book | 600 | 219 (36.5%) | −0.0033 ±0.0080 (−0.82) | +0.07 ±1.48 (+0.09) | −0.36 ±1.33 (−0.52) | −0.43 ±1.01 (−0.83) |
| book2 | 600 | 190 (31.7%) | −0.0017 ±0.0098 (−0.33) | +0.26 ±1.26 (+0.41) | **+1.25 ±1.17 (+2.10)** | +0.99 ±0.95 (+2.04) |
| **POOLED** | **2100** | **768 (36.6%)** | **−0.0035 ±0.0075 (−0.93)** | **−0.15 ±0.88 (−0.33)** | **−0.36 ±0.79 (−0.90)** | −0.21 ±0.60 (−0.70) |

Absolute levels, for scale: mirror win share 0.3389 leaky / 0.3333 de-leaked;
book 0.9408 / 0.9375; book2 0.9417 / 0.9400.

The mirror row is **two disjoint 450-pair runs** (base seeds 279765..279914 and
779765..779914), because the first one produced the only cell in the whole
experiment that looked like a finding — Δ own culture −2.59 ±2.21, z = −2.30 —
and a single flagged cell selected out of many is exactly the thing that has to
be re-shot on fresh seeds before it is believed. **It did not replicate.** On the
independent seeds the same cell came back at **−1.37 ±2.24, z = −1.20**, half the
size and not significant, and the Δ defender culture that had been z = −2.57
collapsed to z = −0.33. The pooled n=900 figure above (−1.98, z = −2.47) still
sits below zero, but it *contains* the run that motivated the replication, so its
z is inflated by selection and should not be read as a two-sigma result. The
clean, pre-specified test of that cell is the replication alone, and it is null.

**Minimum detectable effect** (80% power, α=0.05 two-sided, pooled n=2100):
**1.07 percentage points of win share** and **1.25 culture points of margin**.
The 95% CI half-widths are 0.75pp and 0.88 points. So this run *can* distinguish
"no change" from "2% worse" on both competitive metrics — a 2pp loss would have
been caught with >99% probability — and it did not find one.

#### Result — 2p, `plan:width=2`, champion gen 24 (`row_bargain_forgone` 0.04841)

| opponent | n pairs | diverged | Δ win share | Δ culture margin | Δ own culture | Δ defender culture |
|---|---|---|---|---|---|---|
| mirror | 180 | 20 (11.1%) | +0.0000 ±0.0218 (0.00) | −0.34 ±2.49 (−0.27) | +0.26 ±2.23 (+0.23) | +0.60 ±1.59 (+0.74) |
| book | 240 | 38 (15.8%) | +0.0042 ±0.0142 (+0.58) | +1.01 ±2.26 (+0.88) | +0.89 ±2.65 (+0.66) | −0.12 ±2.07 (−0.11) |
| book2 | 240 | 27 (11.2%) | +0.0042 ±0.0183 (+0.45) | **−2.97 ±2.57 (−2.27)** | −2.28 ±2.05 (−2.18) | +0.69 ±1.66 (+0.81) |
| **POOLED** | **660** | **85 (12.9%)** | **+0.0028 ±0.0106 (+0.51)** | **−0.77 ±1.41 (−1.07)** | **−0.38 ±1.34 (−0.55)** | +0.39 ±1.03 (+0.74) |

MDE at 80% power: 1.52pp of win share, 2.01 culture points of margin.

#### Reading the cells that crossed |z| = 1.96

There are **24** of them (2 player counts x 3 opponents x 4 metrics) and **no
multiplicity correction was applied**, so ~1.2 crossings are expected under a
true null. Two survived into the final tables, and both are noise for reasons
visible in the tables rather than only in the arithmetic:

* **They point in opposite directions.** At 3p the de-leaked bot produced *less*
  own culture against the mirror and **+1.25 more** against book2 (z = +2.10).
  At 2p the culture margin is **−2.97 against book2** (z = −2.27) and **+1.01
  against book** (z = +0.88). A real cost of losing the leak cannot flip sign
  between two opponents at the same player count.
* **The one that was re-shot did not replicate** (above): −2.59 → −1.37, z −2.30
  → −1.20.

The `Δ defender culture` column is what makes the mirror cell interpretable
rather than just dismissible. It is `own − margin`, i.e. the mean culture of the
*defenders*, who are byte-identical de-leaked champions in both arms. In the
first mirror run it moved −2.12, essentially the same as the challenger's −2.59,
while the competitive margin between them stayed flat at −0.47: on those seeds
the whole table produced about 2 culture per seat less and **nobody was
outplayed**. That is a change in the shape of the game, not a strength loss —
total culture in Through the Ages is not conserved, it depends on game length and
on how much production everyone built. **The mechanism was not investigated**;
game length is the obvious candidate and `arena.duel` does not return per-game
move counts, so this is unmeasured rather than explained. On the replication
seeds the whole-table shift was absent (−0.28, z = −0.33), so even the shape
effect is not established.

One caveat that survives all of the above: the league's live objective is
`--objective blend --objective-alpha 0.15`, which is **85% own final culture**
and only 15% win share (`hillclimb_pool.own_share`, `CULTURE_CENTRE` 100,
`CULTURE_SCALE` 120). Absolute own culture is therefore not a bystander metric
here, it is most of the accept gradient — which is why it is reported with the
same error bars as the rest rather than dropped as "not competitive". Pooled, it
is −0.36 ±0.79 at 3p and −0.38 ±1.34 at 2p: consistent with zero at both.

#### Headline

**No detectable loss of strength from closing the leak, at either player count.**
Pooled Δ win share is **−0.35 ±0.75 pp** at 3p (n=2100 pairs) and **+0.28 ±1.06
pp** at 2p (n=660); pooled Δ culture margin is **−0.15 ±0.88** and **−0.77
±1.41**. Nothing is significant on any metric, and the n is large enough that a
2% effect would not have been missed at either count. The 3p arm's fitted
`row_bargain_forgone` = 1.65 does **not** appear to have been buying its strength
from the leak — so the de-leak is not, by itself, a reason to roll that arm back.
(Read that together with the first bullet below, which is the part it does not
cover.)

#### What this does NOT establish

Kept separate from the result on purpose, because all five are easy to read into
the table above and none of them is in it.

* **It does not say the 1,169 generations were well spent.** What is measured is
  the *immediate* cost of removing the leak from a **fixed** weight vector. A
  league that had never had the leak might have hill-climbed to a different and
  better vector — the accept decisions themselves were taken under the leak, so
  the *search path* was contaminated even if the endpoint's score is not. Testing
  that means re-running the arm from scratch, which is a days-long experiment and
  was **not run**. "No immediate strength loss" is therefore *not* an argument
  against rolling the 3p arm back; it only removes the de-leak as a *reason* to.
* **It does not cover the whole pool.** Only `mirror`, `book` and `book2` were
  played — 3 of the 6 live tiers. `past`, `hall`, `human` and `variant` were
  **not** measured, and the pool's own gate weighting (`past=1.2`, `hall=1.6`)
  puts more weight on precisely the tiers left out. The mirror is the strongest
  available probe *of the leak specifically* (the opponent reads the same row
  weights), but it is not a substitute for the gate score.
* **The 2p arm cannot answer the question at all**, and its numbers should not be
  read as evidence of a null. Its champion is **gen 24** of a freshly restarted
  `plan:width=2` arm with `row_bargain_forgone` = **0.048**, i.e. **34x smaller**
  than 3p's 1.65. The effect of the leak scales with the row weights (§6.1), and
  correspondingly only **2/24** games diverged at 2p against 10/24 at 3p. The 2p
  run is a *consistency check* on the harness, not a measurement: it is a null
  because there was almost nothing there to remove.
* **No multiplicity correction was applied.** Four metrics x three opponents are
  reported per player count. With effects this close to zero it does not matter,
  but a reader treating any single cell as a discovery should account for it.
* **The residual leak of §6.2 is untouched.** `hand_mil_value` still reads the
  bot's own freshly drawn military card (within-decision sd 0.005). Both arms
  here have it, so it cancels out of every paired difference — it is *invisible*
  to this experiment, not shown to be harmless.

#### Run log

Unlike §8, these runs were **not** cheap: 5,520 duel games for the two tables
above (3p mirror 1,800 / book 1,200 / book2 1,200; 2p mirror 360 / book 480 /
book2 480), plus 144 for the two validations and 900 for the discarded
replication described below. All under `nice -n 10` alongside three live
training arms and another agent. Wall clock is not quoted anywhere because
docs/PYPY.md records ~9% between-run wall-clock sd on this box under load.

* `tools/deleak_ab.py` — committed, not a throwaway. `--placebo` and
  `--verify-prefix` are the two validations; `--report a.json b.json c.json`
  re-prints and merges saved runs without replaying anything.
* Champions were **snapshotted to `/tmp` before use** (`cp
  experiments/league_state/champion_{2,3}p.json /tmp/`), for the reason §8.1
  gives: the arms rewrite them continuously. Any re-run will see a different
  generation.
* Architectures were read off the **live processes**, not guessed:
  `ps ax | grep hillclimb_league` shows `--candidate-bot plan:width=2` on the 2p
  arm and `--candidate-bot quiescent:levels=1` on 3p and 4p. **4p was not run.**
* `experiments/champion_3p.json` (no `league_state/`) was **not** used. It is a
  gen-152 export with 78 keys and no row weights at all; running against it would
  have produced a confident zero that meant nothing.

**A trap worth recording, found the hard way.** `--seed-base` is not a run
identifier. `arena.duel` derives each game's deal as `seed0 + g // players`, so
a "replication" at `--seed-base` **+1** replays `games/players − 1` of the same
deals. A 450-game 3p mirror run at `20260730` returned the same 175/450
divergence count and the same effect to three decimals as the run at `20260729`,
because 149 of its 150 deals were the same deals. An independent replication has
to move `--seed-base` by at least `games/players`. The tool now prints the base
seed **range** it is about to play so the overlap is visible before the CPU is
spent, and `--report` refuses to concatenate two runs of the same opponent whose
base-seed ranges overlap. The 900 games of the bad replication are not in any
table above; the valid replication at `--seed-base 20760729` is.

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
  attributable to these terms without an ablation. **Still true 2026-07-29**, with
  one narrow exception: §6.5 is a strength measurement, but of *closing the leak*
  in the row terms, not of the row terms themselves. It says the 3p champion is no
  weaker without the leak; it says nothing about whether `row_urgency` /
  `row_bargain_forgone` are worth their weights at all. That still needs an
  ablation, which was not run.
* Whether the Age A and Age IV positions behave like the Ages I-III sampled
  here. Neither was sampled (§8.1).
