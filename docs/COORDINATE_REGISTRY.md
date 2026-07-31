# A coordinate must appear in every registry it needs

2026-07-30.  The bug, in one sentence: **a weight that exists in one registry
and is missing from another is silently dead, and the tree is green.**

The owner's reading of the lane that produced this file, verbatim:

> "You've made a huge number of mistakes with this. Can you structurally change
> the code so that all mistakes are impossible? It looks like you're forgetting
> a lot of 'in this list but not this one' errors."

That is the right diagnosis.  Every defect below is the same shape and none of
them was found by a test.  `tests/test_coordinate_registry.py` is the standing
guard; this document is what it means and what it currently allows.

---

## 0. The instances, in one table

| what | the registry it was in | the registry it was missing from | cost |
|---|---|---|---|
| `free_civil_action`, `resource_discount`, `restricted_resources` | `DEFAULT_WEIGHTS` **and** `card_potential` | `features()` | **16 of 33 action cards priced at exactly 0.000**; no game could produce a gradient on them.  Fixed `5ab4943` |
| `unit_strength_credit`, `tech_levels` | the feature vector, the evaluator | any non-zero champion | priced and inert for days; four audits walked past it |
| units, then yellow/technology cards | the card database | `card_potential > 0` | `row_pressure` skips `val <= 0.0`, so the bot could not see the class existed.  Fixed `d8a2172`, `8b972ef` |
| `MARGIN_SCALE` | `hillclimb_pool` (120.0) | — it was also in `neural_net` (100.0) | one name, two jobs; the reader who noticed would have "fixed" the disagreement |
| three hand-set weight tables | each other | — nothing compared them | `bots.WEIGHTS` `culture_rate` 6.0 against `BASE_WEIGHTS` 5.0, undocumented |
| `state.scoring_events` | `state.py` | every writer, and every reader but one | a dead field that is also a permanently-zero neural input |
| `state.current_events_age` | `state.py`, `neural_encode` | every writer | **found by this test**: five encoding slots frozen on age `A` forever |
| `gov_action_cost` | `DEFAULT_WEIGHTS`, `board_yields` | `features()` | **found by this test**: a revolution's burnt civil actions priced through a coordinate `evaluate` never pays, while `civil_actions` — the same quantity — sits right there |

---

## 1. The four registries

`encode()` is the one people forget, and it is the reason this file talks about
*four* registries rather than the obvious two.  It imports neither `features()`
nor `card_potential`; it builds its own fixed-length float vector.  So a
quantity can be live in the linear evaluator and absent from the encoding, or
encoded and permanently zero, and nothing fails either way.

| # | registry | what it is | how a coordinate dies in it |
|---|---|---|---|
| 1 | `weighted.features()` | the linear feature vector | the key is never emitted, so `evaluate` never multiplies it |
| 2 | `weighted.DEFAULT_WEIGHTS` + every `champion_*.json` / `hall_of_fame/*.json` / `analysis/frozen/*.json` | the coordinate it is paid through | 0.0 by default and 0.0 in every vector on disk |
| 3 | `weighted.card_potential` | what a card in hand claims to be worth | it spends a coordinate registry 1 does not emit, so playing the card cannot produce the points it was priced at |
| 4 | `neural_encode.encode()` | the value net's whole view | the slot is identical in every reachable state |

Each pair is asserted in **both** directions.  The missing direction is always
where the bug hides: "every feature has a weight" was true the whole time the
action cards were broken; "every weight is a feature" was not.

---

## 2. What the test asserts

Fifty tests, about eight seconds, no torch, no game batches.

**Bijection (`FeaturesAndWeightsAreABijection`).**
Every key `features()` emits is in `DEFAULT_WEIGHTS`; every key in
`DEFAULT_WEIGHTS` is emitted by `features()` **or** appears in `PARAMETERS`,
the declared list of weights that are legitimately not features.  A `PARAMETER`
carries what multiplies it and one of three readers — `evaluate`, `pricing`,
`search` — and each is checked (§3).

**Card pricing (`CardPricingSpendsRealCoordinates`).**  The coordinates
`card_potential` spends are collected two ways and unioned:

* an **AST call-graph walk** from `card_potential` through
  `engine/bots/weighted.py` and `engine/bots/board_yields.py`, pulling in every
  module-scope map those functions read by name — which is how `_EFFECT_KEYS`,
  `_BONUS_TO_FEATURE` and `_RESTRICTED_TO_FEATURE` get swept even though the
  keys they carry appear in no function body.  Membership is exact string
  equality against `DEFAULT_WEIGHTS`, so a docstring cannot match: prose is one
  long constant and a weight name is not.
* a **recording weight vector** that remembers every coordinate looked up while
  pricing all 236 cards, statelessly and on a board, under three different
  credit configurations.

`CorpusIsNotVacuous.test_the_ast_sweep_finds_the_historically_dead_coordinates`
is the sweep's own negative control: it names the five coordinates the sweep
must still find, so a graph walk that quietly stopped resolving would make
every assertion built on it vacuously green.

**Phase weights (`PhaseWeightsMatchTheirBases`).**  `PHASE_WEIGHTS` is disjoint
from `BASE_WEIGHTS`; every `PHASE_KEYS` stem has both an `_early` and a `_late`
half; every phase weight names a real stem; every stem is a real emitted
feature; the table is exactly twice `len(PHASE_KEYS)`.

**Vector files (`VectorFilesMatchTheRegistry`).**  No file carries a key
`DEFAULT_WEIGHTS` does not have (`load_weights` would drop it silently), and no
live champion is missing more than a handful.

**The three weight tables (`TheThreeWeightTablesAgreeOrSayWhyNot`).**
`bots.WEIGHTS` diverges from `DEFAULT_WEIGHTS` on eight keys and carries three
of its own; all eleven are pinned in `DELIBERATE_DIVERGENCE` / `GREEDY_ONLY`
with both values.  That is not tidiness — **GreedyBot is the fingerprint
control**: `NARROW` and `WIDE` are GreedyBot and nothing else, and their job is
to hold still while evaluator changes move the other six arms.  Syncing the two
tables, which is the obvious move on reading them, now fails a test first.
`book.V2_TUNABLES` is asserted to share **no** coordinate with the evaluator, so
the two namespaces cannot start disagreeing silently.

**Name collisions (`NoModuleConstantNameCollides`).**  No module-scope name in
`engine/`, `engine/bots/` or `experiments/` is bound to different **literal**
values in two files, unless declared.  Literal-valued only, exactly as
`tests/test_model_constants.py` scopes its sweep: two files computing the same
`HERE = os.path.dirname(...)` is not a collision.  `MARGIN_SCALE` is pinned by
name as well, so it cannot come back.

**Invisible card classes (`NoCardClassIsInvisible`).**  For every card,
`card_potential` must be strictly positive on at least one reachable state, and
the report is **grouped by class** because both historical instances took out a
whole class.  Two probe vectors, and the distinction is load-bearing:

* `DEFAULT_WEIGHTS` — a class at 0.0 here may simply be *configured* off;
* **credits on** — every declared credit at 1.0, no feature weight touched.  A
  class still at 0.0 here is **blind**.

`test_the_credit_gated_classes_are_not_counted_as_blind` pins the difference:
leaders and governments are dark under the defaults and lit with the credits on,
so if `_credits_on` ever stopped doing anything, that would fail rather than
turning the blindness check into a rubber stamp.

**Encoding slices (`NoEncodingSliceIsConstant`).**  §4.

**State fields (`StateFieldsExistInBothRepresentations`).**  §5.

---

## 3. `PARAMETERS` — the exemption list, and why it cannot rot

Not every weight is a feature.  Some are scales on a non-linear term `evaluate`
computes itself (`hand_potential`, `row_urgency`), some are credits on how much
of a board-derived card price to believe (`tech_board_credit`), and one is a
search knob read outside `evaluate` entirely (`end_turn_bias`).  Those are not
bugs — but "not a feature" is *exactly* the shape of the `free_civil_action`
bug, so the exemption must be declared and it must be checkable.

`EveryParameterConducts` checks each one by its declared reader:

* **`evaluate`** — perturb the coordinate and require the evaluation to move on
  at least one corpus state.  A declared parameter that can no longer move
  anything is dead, not exempt.
* **`pricing`** — `card_potential` must actually look the key up.
* **`search`** — the AST sweep must find the literal in `engine/bots/`.

Two details make the conduction probe honest rather than decorative:

* it runs against an **all-on** vector, every 0.0 coordinate lifted to 0.37,
  because half the parameters gate each other — `rival_take_share` multiplies
  nothing while `row_bargain_forgone` is 0.0, and calling it dead for that
  reason would be reporting the configuration rather than the code;
* it builds a real `rival_context` and also probes a **stacked civil hand**.
  `hand_swap_extra` only conducts when two cards of one single-slot class are in
  hand at once, which the corpus rarely deals; without that shape the probe
  would report a live coordinate dead, which is the same error in the other
  direction.

`test_most_of_the_vector_conducts` requires more than 80% of the vector to
conduct, so a probe that broke and started reporting nothing fails loudly.

---

## 4. The neural encoding, and how the zero padding is handled

`encode()` returns 1897 floats.  A slot identical in every reachable state is an
input the network can never learn from — the analogue of a weight `features()`
never emits.

**Named slices, not raw indices.**  "Component 7 of rival 2's government
`card_vec` is constant" is noise; "`rival.hand_military_vec` is constant" is the
information-legality guarantee in one line.  The 1897 indices are grouped into
**163 named slices**: the 14 `_global_block` fields (a one-hot is one slice),
the row's per-slot `present` / `card_vec` plus a single `row.cost_ladder`, and
the player blocks as 56 named scalars and 5 `card_vec` blocks under two roles,
`me` and `rival`.

`SCALAR_NAMES` is duplicated from `_player_block` because there is nothing to
derive it from, so `test_the_scalar_names_still_line_up` asserts its length
against `neural_encode._PLAYER_SCALARS`, asserts the globals sum to
`_GLOBAL_DIM`, and asserts the slice map **covers the whole 1897-wide vector** —
otherwise a slot could drop out of the map and stop being checked.

**Constancy is judged PER INDEX, not per slice.**  A one-hot is constant when
each of its five components is individually constant.  The first version of this
check compared min and max *across* the block, which calls a frozen one-hot live
— one component sits at 1.0 and the rest at 0.0, so min ≠ max.  That version
silently missed `current_events_age`.  It is recorded here because it is the
same vacuity failure this whole file exists to prevent, committed inside the
guard against it.

**The padding, which the request specifically asked about.**  `encode()` lays
the players out as `[me, rival0, rival1, rival2]` and zero-pads unused rival
blocks so the vector length is fixed across 2p/3p/4p.  Rival block *k* therefore
carries a real player only when `num_players >= k + 1`.  So:

* the corpus records min/max **segmented by player count** — three separate
  tables, not one;
* each index carries a `players_required`, and a slice is judged **only over the
  player counts where it is real**.  A slot in rival block 2 is never read from
  a 2p game.

Getting this wrong makes the check either vacuous (judge everything at 4p only)
or permanently red (judge 2p padding as dead data).  The premise itself is
asserted rather than assumed: `test_the_padded_rival_blocks_really_are_padded`
encodes a fresh 2p state and requires rival blocks 2 and 3 to be **all** zero.
If a 2p game ever started writing there, excluding those blocks would be hiding
data rather than excluding padding, and that test is what would say so.

One consequence to read carefully: `me.present` and `rival.present` are the
padding markers themselves, so they are 1.0 in every block this check is
*allowed* to look at.  They are listed as constant, and the reason on each entry
says they are informative precisely in the blocks the check excludes.

---

## 5. State fields: dead in both representations at once

A declared `GameState` / `PlayerState` field the engine never writes is dead
everywhere at once, so it is flagged **once, under the field**, not once per
registry.

*Written* is computed by AST over `engine/`, and the naive version gets it
wrong: `p.completed_wonders` is never a `Store` target, because the engine
writes it as `journal.touch(p.completed_wonders).append(name)`.  So the sweep
counts three shapes — `Store`/`Del` attributes, subscript-assignment receivers,
and **the whole of any call whose method is a mutator** — plus dataclass
constructor keyword arguments.

Nine fields survive that sweep as never written; they are in `KNOWN_DEAD` under
`never-written` and in §7.

**Every written field must be encoded or declared.**  `NOT_ENCODED` lists the 38
that are not, each with a reason, pinned by **set equality**.  That equality is
doing double duty and the second job is the important one: an entry *leaving*
this list means a field started being encoded, which needs an
information-legality read before it ships.

**The docstring is the allow-list of record.**  `neural_encode`'s own
"Deliberately NOT encoded (would be cheating or is unknowable)" list is asserted
to still carry each phrase that justifies an exclusion, so the code and its
documentation cannot drift apart.  On top of that:

* `civil_deck` and `military_deck` must not be read by the encoder **at all** —
  deck order is hidden, and only the count feeds `rounds_left`;
* `current_events` and `future_events` may be read **only inside `len()`**,
  asserted structurally on the AST rather than by reading the code, because "we
  only use `len()`" is exactly the sort of claim that rots;
* a rival's military hand and event seeds are zeroed by `_player_block`, and the
  constancy check in §4 is the machine-checkable form of that promise —
  `encode:rival.hand_military_vec` *ceasing* to be constant would mean the net
  had started seeing hidden information.

---

## 6. The ratchet

Dead coordinates exist today.  They are not fixed by weakening the test; they
are enumerated in `KNOWN_DEAD`, each entry carrying the coordinate, **which
registry it is missing from**, why it is dead, and where the open item lives.

Every check is a **set equality**, so it fails in both directions:

* something **new** joins the dead list — a regression, and the real guard;
* a listed entry is **no longer dead** — the entry must be deleted.

The second half is what makes this a ratchet and not a graveyard.  **The list
can only shrink.**  An entry may carry more than one kind: `wonder_overrun` is
both `inert-vector` and `never-nonzero`, and reading those two together is the
point — neither half can wake the other.

### The eight kinds

| kind | means |
|---|---|
| `unpriced-feature` | spent by `card_potential`, emitted by no feature, so `evaluate` never pays for what the card buys |
| `inert-vector` | 0.0 by default and 0.0 in every **committed** vector that carries it |
| `inert-live` | 0.0 by default and 0.0 on every **live league champion** |
| `never-nonzero` | emitted by `features()` but exactly 0.0 on every corpus state |
| `invisible-class` | a card class `card_potential` never prices above zero, so `row_pressure`'s `val <= 0.0` skip makes it unreachable |
| `stale-vector-key` | a key in a weight file that `DEFAULT_WEIGHTS` no longer has |
| `never-written` | a declared state field the engine never assigns |
| `constant-encoding` | an `encode()` slice identical on every corpus state |

### Why `inert-vector` and `inert-live` are two kinds and not one

`experiments/league_state/` and `experiments/hall_of_fame/` are the live
league's and are **untracked** — present on the training box, absent in a clone.
A ratchet whose answer depends on which tree it runs in means two different
things in two trees, so it is split: `inert-vector` runs over the committed
vectors and is portable, `inert-live` runs over the three vectors actually being
climbed and **skips** when they are absent.  Both are true statements; neither
is the other.  The committed `experiments/champion_*.json` are 78-key files
against a 124-key registry, which is also why "carried by at least one file" is
part of the definition — without it, 46 coordinates would read as inert for the
opposite reason to the one the check is about.

---

## 7. The current KNOWN_DEAD list — 45 entries

### Spent by `card_potential`, emitted by no feature (5)

| coordinate | why |
|---|---|
| `free_civil_action` | the **static** table's spelling.  `5ab4943` moved the live path onto the `civil_actions` marginal, but `_card_yields` still emits the key, so `action_board_credit = 0.0` or any caller with no board falls back onto it |
| `resource_discount` | same shape: `resourceDiscount`.  **Not** inert on the live champions — the climber scatters onto it (`mutate`'s 0.15 floor) and it random-walks, because no game can produce a gradient to pull it back |
| `restricted_resources` | same shape: `resourcesForMilitaryUnits`, superseded live by `restricted_resource_credit x resource_stock` |
| `defense_bonus` | a Military Bonus card's defence is worth something *because* it is still in hand, so there is genuinely no board mirror.  It is also the only coordinate the three bonus cards have — which is why `class:bonus` is invisible.  Two entries, one defect |
| `gov_action_cost` | **found by this test.**  `board_yields` prices a revolution's burnt civil-action pool through it; `features()` does not emit it, while it *does* emit `civil_actions`, the same quantity in the same units |

### 0.0 on every committed vector (4)

`wonder_stages_left`, `wonder_turns_to_finish`, `wonder_stages_per_action`,
`wonder_overrun` — the finish-discipline block plus the Masonry channel, all
deliberately 0.0 so trained vectors were unchanged when they landed, and the
league has never priced them.

### 0.0 on every live league champion (8)

`build_discount`, `colonize_bonus`, `defense_bonus`, `event_scoring_margin`,
`free_civil_action`, `hand_mil_potential`, `hand_swap_extra`,
`unit_strength_credit`.

`hand_mil_potential` at 0.0 is load-bearing: nothing calls `card_potential` on a
military card at all, which is what makes `territory_credit` and
`bonus_card_credit` cost nothing and what hides the four invisible military
classes below.  `unit_strength_credit` is the named historical case.

### Emitted but never non-zero (2)

`best_arena` — the bot builds no arena in any corpus game at any player count.
`wonder_overrun` — the feature never fires, so its 0.0 weight is doubly dead;
`docs/MODEL_CONSTANTS.md` §6.1 already leans on that to argue the deal-rate fix
is inert.

### Whole card classes invisible (5)

| class | count |
|---|---|
| `tactic` | 15/15 at exactly 0.0 |
| `aggression` | 10/11 |
| `war` | 3/3 |
| `pact` | 10/10 |
| `bonus` | 3/3 |

All five are still at 0.0 **with every credit at 1.0**, so this is blindness and
not configuration: `_card_yields` has no mapping for a tactic's strength table,
a one-shot steal has no printed production, and `pacts` / `pact_blocks_attack`
are *board* features that nothing maps a pact card in hand onto.  It costs
nothing today only because `hand_mil_potential` is 0.0 — the moment the league
prices the military hand, four of the game's card types are invisible to it.

### Declared state fields the engine never writes (9)

| field | why it matters |
|---|---|
| `scoring_events` | the recorded case; also a permanently-zero neural input |
| `current_events_age` | **found by this test**; also five frozen encoding slots |
| `caesar_double_politics_used` | **found by this test**; referenced nowhere else in the repo |
| `used_leader_ability` | **found by this test**; the generic leader flag, unused |
| `culture_rate_extra` | **read** by `effects.compute` and written by nothing |
| `science_rate_extra` | its exact twin |
| `destroyed_wonders` | already in OPEN_ITEMS: read by the take surcharge, never incremented |
| `hidden_civil`, `hidden_military` | **deliberate** — app-harness only, documented at `state.py`.  Listed so they are a stated exception rather than an unexamined zero |

### Constant encoding slices (14)

`global.current_events_age`, `global.scoring_events` — the two dead fields
above, seen from the other registry.
`row.cost_ladder` — `_ROW_COST[i]/3.0` is a positional constant, so 13 inputs
are compile-time constants the net absorbs into its bias.
`me.present`, `rival.present` — the padding markers, §4.
`me.discontent`, `rival.discontent`, `me.uprising`, `rival.uprising` — the bot
never goes discontent in any corpus game, so the whole unhappiness channel is a
column of zeros in *both* representations.
`me.best_arena`, `rival.best_arena` — **the cross-registry confirmation**: two
independent instruments over two independent representations agree the bot never
builds an arena.
`rival.seeded_events_n`, `rival.seeded_events_level`, `rival.hand_military_vec`
— **deliberate**, and they are the information-legality guarantee, not a bug.

### Registry drift (1)

`has_unit@analysis/frozen/default_weights.json` — an archived snapshot of a
`DEFAULT_WEIGHTS` that still had a `has_unit` coordinate.  The file is a frozen
artefact and must not be edited; it is listed so the sweep stays switched on for
every other file.

---

## 8. Anti-vacuity

A structural test that silently stops testing is worse than none, and this repo
has been bitten: `tests/test_row_features.py` once dealt its probe card at a
slot scored through a quantity that saturates at 1.0, so the guard could stop
guarding without anything failing.  So every floor here is a **number**:

* the corpus is 6 deterministic self-play games, one per (player count, seed),
  and `CorpusIsNotVacuous` asserts **more than 1200 states**, **at least 30**
  card-pricing probe states, **all five ages present**, **all three player
  counts with more than 100 states each**, and **more than 90% of states with a
  non-empty card row**;
* `DEFAULT_WEIGHTS` has more than 100 keys, `features()` emits more than 60, the
  AST sweep finds more than 30 pricing keys, the runtime sweep more than 30, and
  the constant sweep more than 100 constants — a registry that collapses to
  empty fails instead of making every set difference empty;
* the AST sweep must still find the five historically dead coordinates by name;
* more than 80% of the weight vector must conduct;
* more than 30% of the encoding must move, and the slice map must cover all
  1897 indices;
* the corpus deliberately does **not** retain states.  `actions.apply` mutates
  in place, so a list of states is a list of aliases of the last one — which is
  precisely the shape of the trap.  Every observation is taken inline.

The corpus is also stepped one ply past the loop to record the **terminal**
state: `game_over`, `final_scores` and `forced_winner` only ever move there, and
a corpus that stops one ply early reports all three dead.

---

## 9. Negative control

The guard was run against three simulated regressions in a throwaway clone.

| control | what was done | result |
|---|---|---|
| 1. the action-card bug | removed `civil_actions` from `features()`'s output — the coordinate `5ab4943` moved the free civil action onto, i.e. `free_civil_action` in its post-fix spelling | **6 failures**, led by `test_every_weight_is_emitted_or_declared` (the direction that would have caught it) and `test_the_dead_priced_coordinates_are_exactly_the_listed_ones` |
| 2. the `MARGIN_SCALE` collision | re-added `MARGIN_SCALE = 100.0` to `neural_net.py` and `MARGIN_SCALE = 120.0` to `hillclimb_pool.py` | **2 failures**, naming the constant and printing both values and both files |
| 3. an invisible card class | forced `card_potential` to return `-1.0` for every wonder | **1 failure**, naming `class:wonder` and reporting `16/16 at <= 0.0` |

---

## 10. What this does NOT cover — stated as gaps, not as completeness

* **`features()` emits a fixed key set.**  Every state in the corpus produces
  the same keys, so "emitted for at least one reachable state" is today a
  weaker claim than it sounds.  The test is written for the conditional case
  because that is the shape a future emitter will have; right now the floor is
  doing the work.
* **`never-nonzero` and `constant-encoding` are corpus-pinned.**  They are
  deterministic, so they are not flaky — but they are statements about *this
  policy*, not about the game.  A change that makes the bot build an arena
  will fail this test, and the correct response is to delete the entry.
* **The AST call graph is name-based.**  A call through a variable, a dict of
  handlers, or `getattr` is not followed.  The runtime recording sweep covers
  the paths it actually runs, and the two are unioned, but a pricing path that
  is neither statically named nor exercised by the corpus is invisible to both.
* **`_fields_the_encoder_reads` is name-based too.**  A field being *read* by
  `neural_encode` does not prove it reaches the output; the constancy check
  covers the output side, but the join between the two is by attribute name.
* **`NOT_ENCODED` is 38 entries of prose.**  The set equality is machine-checked;
  the *reasons* are not.  Only the four in `HIDDEN_BY_THE_RULES` are tied to an
  assertion (their justification must still appear in the encoder's docstring).
* **Nothing here checks the neural checkpoint files** the way it checks the
  weight vectors.  A serialised net trained against an older `ENCODING_DIM` is
  a fifth registry and is not covered.
* **`experiments/` and `tools/` weight-name literals are not swept.**
  `summarize.GROUPS` lists coordinates by name and a key that rots out of it
  silently stops being rescaled; that is the same bug class and it is not
  guarded here.
* **The invisible-class check uses `> 0.0` on the best state seen.**  A class
  that is positive only in states the corpus never reaches still reads as live.
