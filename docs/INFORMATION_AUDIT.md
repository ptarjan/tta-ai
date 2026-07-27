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

**The hypothesis was right, and it is stronger than stated.** Almost all of that
information is already sitting in `GameState` in full, and
`engine/bots/weighted.py:features()` reads *none* of it. The bot is not failing
to track hidden information; it is failing to look at information the engine has
already handed it.

---

## 0. Headline result

`features()` (`engine/bots/weighted.py:332-481`) returns a 60-key dict. Measured
invariance on a real mid-game 2p position (turn ~60, `/tmp/invcheck.py`, see
§7.1):

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

Only five of the 60 features are rival-derived at all — `rival_culture`,
`rival_mean_culture`, `rival_culture_rate`, `rival_science_rate`,
`rival_strength` (`engine/bots/weighted.py:476-480`, aggregates built in
`rival_context`, `engine/bots/weighted.py:176-192`).

And the single most damaging consequence, measured in §7.2: pricing the **same
card** at slot 0 (1 CA) versus slot 9 (3 CA),

```
2p champion   slot 0 +1.44621   slot 5 +1.38226   slot 9 +1.31831   (spread 0.128)
3p champion   slot 0 +1.55058   slot 5 +1.64798   slot 9 +1.74538   (spread -0.195)
4p champion   slot 0 +0.06640   slot 5 +0.03276   slot 9 -0.00088   (spread 0.067)
```

**The 3p champion prefers to pay 3 civil actions.** That is not a metaphor: the
only channel by which row depth reaches the evaluation is the `ca_left` feature
(`engine/bots/weighted.py:439`), whose 3p champion weight is **-0.0974**
(`analysis/frozen/champion_3p.json`), so spending two extra civil actions is
scored as a *gain* of 0.195. This is the mechanism behind an already-measured
behaviour that nobody had explained: the 3p champion takes **56.9% of its cards
from cost band 3 at 2.33 CA/card** while the 2p champion takes 88.4% from band 1
at 1.15 CA/card (`docs/HEURISTICS_PROGRESS.md:119-121`), against a human
tournament baseline of **76% of Age I picks at 1 CA and 2.5% at 3 CA**
(`docs/EXPERT_STRATEGY.md:688`).

---

## 1. Master table

"Engine represents it" = the fact is recoverable from `GameState` today.
"A feature reads it" = it changes the output of `weighted.features()` /
`weighted.evaluate()`.

| # | Information the rules make available to a player | Engine represents it? | Any feature reads it? |
|---|---|---|---|
| 1 | Which 13 cards are in the civil row | YES `engine/state.py:124` | **NO** (§0 test B) |
| 2 | Each row card's slot, hence its CA cost | YES `engine/actions.py:36-45,79-89` | **NO** — only via `ca_left`, worth 0.05 (default) / **-0.097 (3p champ)** per CA `engine/bots/weighted.py:439` |
| 3 | Where a card will slide to next turn | Derivable (exact sweep constant) `engine/game.py:41,104-121,219-220` | **NO** |
| 4 | Whether a card will be swept before I act again | Derivable, same source | **NO** |
| 5 | Whether an opponent can/wants to take a given row card | Derivable (their CA, hand limit, techs, wonder-in-progress, leader ages) | **NO** |
| 6 | Opponents' civil cards in hand (PUBLIC, `docs/RULES_SPEC.md:71`) | YES `engine/state.py:60` | **NO** (§0 test C) |
| 7 | Opponents' civil hand *size* | YES same field | **NO** |
| 8 | Opponents' military hand *size* (public) | YES `engine/state.py:61` | **NO** (own hand only, `weighted.py:473`) |
| 9 | Opponents' military hand *contents* (HIDDEN by rules) | YES, truthfully — no info-set abstraction | **NO** by features; **YES** by QuiescentBot's defence resolution `docs/DEEPER_SEARCH.md:507-512` |
| 10 | Age I/II/III deck composition (fixed, public) | YES `engine/cards.py:155-175` | Only as a **count**, for the game-length horizon `engine/bots/weighted.py:243-276` |
| 11 | Which civil cards have already been seen (row/hands/boards) | Partially — **swept row cards are destroyed with no record** `engine/game.py:117-120` | **NO** |
| 12 | Which military cards have been discarded | YES `engine/state.py:132`, `engine/economy.py:186-197` | **NO** |
| 13 | **What I put into the politics (future events) deck** | YES, with owner attribution `engine/state.py:125,129`; written at `engine/actions.py:992` | **NO** (§0 test E) |
| 14 | The current-events deck contents/order (hidden) | YES, in the clear `engine/state.py:126` | **NO** by features; **readable by any deeper search** — `plan.determinize` does not touch it `engine/bots/plan.py:82-94` |
| 15 | Events already resolved (`past_events`, public) | YES `engine/state.py:127` | **NO** |
| 16 | Opponent boards: techs, workers per card, government, leader, wonders, tactic, colonies, pacts, happiness, food/resources/science, CA/MA | YES `engine/state.py:43-101` | **Only 3 derived scalars**: their culture *rate*, science *rate*, strength (`weighted.py:183-191`), plus raw `culture`. Pacts are counted only when I am a party (`weighted.py:368-376`). |
| 17 | Turn order / how many opponent turns before mine | YES `engine/state.py:117-119` | **NO** (only aggregate `rounds_left`, `weighted.py:264-276`) |
| 18 | Civil deck order (HIDDEN) | YES, in the clear `engine/state.py:122` | **NO** by features; read by `end_turn` trials — 94.9% leaky, measured `docs/BOT_ARCHITECTURE.md:208-231` |

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
* **No feature reads any opponent hand — contents or size.** `hand_civil`,
  `hand_value`, `hand_military`, `hand_mil_value`
  (`engine/bots/weighted.py:471-474`) are all `p = state.players[idx]`, i.e. self
  only; `hand_potential` likewise (`engine/bots/weighted.py:593-601`).
  Confirmed empirically, §0 tests C and D.
* The one place opponent military hands *are* read is QuiescentBot resolving a
  defender's `defense` decision — `docs/DEEPER_SEARCH.md:507-512` states this
  openly and calls it indefensible for play against a human.

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
* **Nothing reads any of it.** Grep across `engine/bots/`: `future_events`,
  `current_events`, `past_events`, `seeded_by`, `scoring_events` appear *only* in
  `engine/bots/fastcopy.py:85-87` as field names to copy. §0 test E confirms:
  deleting both event decks changes nothing.
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

So: **three derived rates plus one raw score.** Their happiness, food balance,
worker count, government, leader, wonder progress, tactic, colonies, science
stock, resource stock, civil/military action counts and hand are all invisible —
confirmed field by field in §0. Their techs are read only insofar as they roll up
into those three rates. It is aggregate strength and score and essentially
nothing else.

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

---

## 7. Ranked gaps and bounded proposals

Ordered by (value / implementation cost). Each is a feature-level change; none
requires a new search architecture.

### GAP 1 — Row depth is priced by a single scalar with the wrong sign. *(highest value, lowest cost)*

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

## 9. Things I could not verify

* Whether the military *discard pile* is public in the physical game.
  `docs/RULES_SPEC.md:188` says excess military cards are discarded "face down"
  and `docs/RULES_SPEC.md:125` says defence cards are discarded face down, which
  suggests the pile is *not* legible; but the spec does not say so explicitly.
  `state.discarded_military` should therefore be treated as hidden until this is
  settled, which affects how a military card-counter may use it.
* The size of the event-order leak (§6, item 2). `tools/infoleak.py` does not
  instrument event reveals, so this is unmeasured rather than measured-small.
* Whether `rival_hand_potential` (GAP 3) is worth anything at 3p/4p. The civil
  `hand_potential` term itself was only validated at 2p
  (`engine/bots/weighted.py:673-677`).
