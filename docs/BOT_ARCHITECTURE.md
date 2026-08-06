# Bot architecture

What an agent needs to know about the bot layer before touching it: the
roster, how a position is scored, how weights are declared/read/persisted,
how search is structured, and the invariants that must not be broken. Every
path below is in `rust/src/` and was checked against the current tree; the
Python engine (`engine/`) was deleted 2026-08-06 and no longer exists.
History, measurements and superseded prose live in `docs/EVALUATOR_HISTORY.md`.

## 1. The bot roster

`rust/src/bots/greedy.rs::BotKind` is the classical roster, five kinds:

| kind | what it is | file |
|---|---|---|
| `random` | picks a legal move uniformly | `greedy.rs` |
| `greedy` | 1-ply search over `greedy.rs`'s own small, frozen `GreedyKey` weight table (19 keys) — the gate arms' **fingerprint control**, see §7 | `greedy.rs` |
| `weighted` | 1-ply search over the full linear evaluator, §2 below | `weighted/eval.rs::WeightedBot` |
| `quiescent` | resolves `state.pending` before scoring, §5 | `quiescent.rs` |
| `plan` | beam search over whole-turn move sequences, §5 | `plan.rs` |

Two checkpoint-backed kinds sit alongside these, parsed by the same grammar
(`rust/src/bots/neural/spec.rs::Spec`, `KIND[:PATH][,KEY=VALUE]...`,
e.g. `plan:champion_2p.json,width=8` or `nplan:best.ckpt,width=8,nodes=1200`):

* **`neural:CKPT`** — the value net's own 1-ply argmax (`bots/neural/bot.rs`).
* **`nplan:CKPT`** — `plan`'s whole-turn beam with the net as its leaf
  evaluator instead of the linear evaluator (`bots/neural/plan.rs`). This is
  the policy the neural search loop ships.

`Spec::parse` refuses a knob a kind does not read (e.g. `weighted,width=8`
is an error, not a silently-ignored width) — see that module's own doc
comment, "a knob that does not apply is an ERROR". Do not add a case there
by guessing; the exhaustive `Knob::applies_to` match is what makes an
unread knob a compile error the day a new kind or knob is added.

**`BookBot`** (`rust/src/bots/book.rs`) is a hand-written, rule-based
external yardstick — an ordered priority list cited to tournament data, no
search, no learned weights — fully ported but **not currently wired into
any binary** in `rust/src/bin/`. It exists as an available module for
whoever next needs an opponent that isn't a self-play product. See
`docs/EVALUATOR_HISTORY.md` for why it was built.

There is no rule-based variant pool (`CultureBot`/`InfraBot`/`MilitaryBot`/
etc.) in this port. An older Python generation had one; it predated
`PlanBot`/`QuiescentBot` and does not exist here — the table above is the
whole roster.

## 2. How a position is scored: `evaluate`

`rust/src/bots/weighted/eval.rs::evaluate(state, idx, weights, ctx, f)` is a
linear function of a feature vector (`rust/src/bots/weighted/features.rs::features`),
computed for player `idx` (the *decider*, not necessarily the turn player —
a pact accept/refuse is scored for whoever is deciding it, not who owns the
turn). Four passes, in order:

1. **The flat body.** For every `WeightKey` (see §3), `total += w[k] * f[k]`,
   skipped when `w[k] == 0.0` (which is also correct for every key
   `features()` never wrote — `Features::get`'s documented zero default
   makes the two cases arithmetically identical).
2. **The phase-blended body.** Four keys only (`Workers`, `StrengthRel`,
   `TechLevels`, `HandValue` — `weights::PHASE_KEYS`) additionally carry an
   `_early`/`_late` pair, blended as `w[k] + (1-L)*w[k_early] + L*w[k_late]`
   where `L = horizon::lateness(state)` (exact — fraction of the civil card
   supply already dealt, §4). Six other keys used to be phase-blended this
   way (`culture`, `culture_rate`, `science_rate`, `food_rate`,
   `resource_rate`, `wonder_progress`); that was retired 2026-08-04 in favor
   of the rate horizon (§4) and `culture`/`wonder_progress` being pure
   numeraire/stock terms a phase blend must not rescale. Do not add a phase
   pair for a rate key or a stock key without re-reading why those six were
   pulled out — `weights.rs`'s own comment on `PHASE_KEYS` has the citation
   trail.
3. **The rate horizon.** The four `RATE_KEYS` (`CultureRate`, `ScienceRate`,
   `FoodRate`, `ResourceRate`) — whichever pass they appear in, flat or
   phase-blended — are additionally scaled by `horizon::rate_multiplier`,
   gated by weight `RateHorizon` (default **1.0**). See §4 and
   `docs/EVALUATOR_HISTORY.md`'s rate-horizon entry for why.
4. **Identity-aware terms**, each gated by its own weight defaulting to
   0.0 and each skipped entirely when that weight is 0.0 (so a champion
   trained before one existed evaluates exactly as it did): `hand_potential`,
   `wonder_potential`, `hand_mil_potential`, `tactic_terms` (gain/short),
   `rival_hand_potential`, `row_pressure` (urgency/bargain), `row_last_copy`,
   `my_event_threat`. These price *what the cards in hand would be worth if
   played*, not a linear function of board state, so they are computed by
   `bots/weighted/cards.rs`/`row.rs`/`events.rs` and multiplied in here
   directly rather than folded into `features()`.

**Card and board pricing (leaders, governments, wonders, technologies)**
goes through a different, shared mechanism: `rust/src/bots/board_yields.rs`
computes what a card is worth by **swapping it into a cloned player and
diffing `effects::compute` before/after** — never a hand-written value
table, because a table drifts from the rules the moment a leader like
Michelangelo is involved. `feature_marginal` (in `bots/weighted/rivals.rs`)
is the single definition of "what one unit of a feature is worth" and is
what every card-pricing site goes through, which is what keeps card prices
and `evaluate` from ever disagreeing about the phase blend or the rate
horizon — see that function's own doc comment. Government pricing
specifically prices the **cheaper of the two legal routes** (peaceful vs
revolution) every time, gated by `WeightKey::GovBoardCredit` (default 1.0);
see `docs/EVALUATOR_HISTORY.md`.

Most of these board-credit weights (`TechBoardCredit`, `ActionBoardCredit`,
`GovBoardCredit`, `RestrictedResourceCredit`, `TerritoryCredit`,
`BonusCardCredit`) default to **1.0** (live); a few (`CardBoardCredit`,
`WonderBoardCredit`, `BuildFreshCredit`, `UnitStrengthCredit`,
`FreeActionCredit`) default to **0.0** (the code path exists and is
correct, but the league has not been given it to price). Check
`weight_key_table!` in `weights.rs` for the current default of any specific
credit before assuming either way.

### The `end_turn` bias — do not "fix" this

`WeightedBot::choose` (`eval.rs`) scores `Move::EndTurn` on the **unmoved**
trial state, plus `WeightKey::EndTurnBias` (default −3.0). This looks like
an asymmetry with every other candidate, which is scored on the state
*after* applying it. **It is not a bug.** It was measured, twice, two
different ways, against every alternative, and is strictly stronger — see
the comment at that exact line in `eval.rs`: "DO NOT 'fix' this asymmetry."
If you find this and it looks wrong, it has already been checked; look for
a different bug.

## 3. Weights: declared, read, persisted

`rust/src/bots/weighted/weights.rs::WeightKey` is a fieldless enum, one
variant per coordinate, generated by the `weight_key_table!` macro
invocation (name, JSON string, default value, in one place — currently
**133 keys**; count `WeightKey::ALL.len()` rather than trusting a number in
prose, including this one, since it moves). Being a fieldless enum buys two
guarantees the Python dict-of-floats version never had:

* **A reader with no declared key is a compile error** (`WeightKey::Typo`
  fails `cargo build`), not a runtime surprise.
* **`Weights` is `[f64; N]` indexed by `key as usize`** (`N = WeightKey::ALL.len()`)
  — no `HashMap`, no allocation or hash on the hot 1-ply-search path.

What a fieldless enum does **not** buy: a *declared* key with no live
reader. `WeightKey::ALL` happily lists a variant nothing reads, and
`evaluate`'s own generic loop reads every key uniformly, so "was `get` ever
called with this key" is true of all 133 by construction and catches
nothing. `rust/src/bots/weighted/registry.rs` is this port's replacement
for the retired Python `tests/test_coordinate_registry.py`
(`docs/OPEN_ITEMS.md` item 5) and closes this the way a fieldless enum
cannot: a source-text scan requiring every variant to appear as a literal
`WeightKey::X` somewhere outside its own declaration, **and** runtime
instrumentation over real self-play games requiring every key `features()`
sets to actually go nonzero on some sampled position (this second check is
the one that would have caught a coordinate with a call site that never
evaluates non-zero — read that file's own top doc comment for the exact
precedent it cites). Both carry a `KNOWN_DEAD`-shaped ratchet for real,
named exceptions. **If you add a `WeightKey` variant, it must be read by
name somewhere in production code, or `registry.rs`'s tests fail.**

**`WeightGroup`** (`weights.rs`) buckets every key into one of 14 strategic
axes (economy, military, tech, wonders, row, ...) so a hill-climb mutation
can move a whole axis at once (`rescale`, §7) instead of scattering onto
unrelated coefficients. `WeightGroup::keys` derives its membership from
`WeightKey::group` rather than keeping a second, parallel list — the two
cannot drift apart because there is only one.

**`RETIRED_KEYS`** (`weights.rs`) is a named list of weight-name strings
that used to be `WeightKey` variants and were deliberately removed (the six
`_early`/`_late` rate/stock pairs from §2). Every champion JSON on disk
still carries them; `load_weights` drops them silently on read, and
`registry.rs`'s check knows the difference between a retired name and a
typo because this list says which retired names to expect.

**Persistence.** `rust/src/bots/weighted/eval.rs::{parse_weights, load_weights, save_weights}`
are the only I/O. Loading: missing keys keep their `WeightKey::default_weight`,
retired keys are dropped, an **unknown** key is a loud `Err` (not silently
ignored — a typo in a hand-edited champion costs you the weight you thought
you set, so this port refuses rather than repeats that), and
`dominance_repair` (§6) is applied on every load, not only inside the
trainer. Saving: **every** `WeightKey` is written, not just the keys that
happened to be present in whatever was loaded — a champion file is always a
complete vector.

Two files matter for "which weights are actually being played" and they
are **not interchangeable**: the retired Python trainer's last snapshot (78
keys, July, moved 2026-08-06 to `analysis/frozen/python_champion_{2,3,4}p_
..._2026-07-26.json` — see that directory's `README.md`) is a **stale
snapshot**. The **live** champions are `experiments/rust_champion_{2,3,4}p.json`
(130 keys, gitignored — they exist only on the training box, not in a fresh
clone; see [`docs/RUST_LEAGUE.md`](RUST_LEAGUE.md#which-champion-file-is-live)).
If a doc, tool or your own assumption conflates the two, it is wrong; check
the file that is actually being read by whatever you're running.

## 4. The horizon: how much game is left

`rust/src/bots/weighted/horizon.rs` answers "how many rounds are left" and
"how far through the game are we" from exact and measured state, not fitted
constants:

* `rounds_left` — exact once Age IV begins (`state.final_round_end` is
  pinned); before that, the exact count of undealt civil cards divided by a
  take rate **measured in the game being played** (`take_rate`, shrunk off
  a small fitted prior within a couple of rounds — the one number in this
  module that is still fitted, and it is labelled as such).
* `lateness` — `1 - cards_unseen/supply`, exact, clamped to `[0, 1]`. The
  clamp is load-bearing, not decorative: an earlier unclamped version let
  `1 - L` go negative near the end of the game, which flips the sign of
  every early-phase weight (measured cost: a 4p champion falling to 19.9%
  against a 25% null). Do not remove the clamp to "simplify" this.
* `horizon_scale` / `rate_multiplier` — `rounds_left` normalised so an
  average-moment decision scores 1.0, blended against flat (no-op) pricing
  by weight `RateHorizon`. This is what §2's rate-horizon scaling reads.

## 5. Search structure

**`quiescent.rs`** is generic over the evaluator (`eval: &impl Fn(&GameState, u8) -> f64`).
Its reason to exist: a candidate move that leaves a *pending* decision (a
pact offer, an aggression, a colony bid, most action cards) shows its full
cost and none of its gain in a 1-ply trial, so 1-ply search ranks it as
dominated by passing under any weight vector. Quiescence keeps resolving
`state.pending` — whoever the decider is — until the stack is empty, then
scores. `war_value` (declared-war-of-mine pricing, "played out as already
fought" because a war's loot resolves a full round later, outside the
pending stack) lives here and is reused by `plan.rs` rather than
reimplemented — "two searches that disagree about one move class do not
share a weight vector."

**`plan.rs`** (`PlanBot`) is a beam search over **whole-turn move
sequences**, scored at one fixed horizon, on a **determinized** root (hidden
piles reshuffled before search, so the search cannot read cards the player
hasn't legally seen). It fixes three defects of 1-ply `WeightedBot` at once:
the `end_turn` horizon asymmetry (§2, though that asymmetry is now itself
believed correct and kept), one ply of lookahead inside a turn that has
several, and hidden information leaking through an undeterminized root.
`PlanConfig::war_lookahead` defaults to **true** — it prices a declared war
through `quiescent::war_value` rather than as pure cost, which closed a real
transfer-failure bug (see `docs/EVALUATOR_HISTORY.md`'s transfer-test entry:
a vector trained under a war-aware search used to be measurably *worse*
under a war-blind `PlanBot`). If you ever see `war_lookahead: false`
somewhere, that is a deliberate A/B arm, not a normal configuration — it
measures the hidden-information/war-blindness leak, "never how a bot should
play for real" (`spec.rs`'s own comment on the `det`/`war` knobs).

**`bots/neural/{bot.rs,plan.rs}`** are the checkpoint-backed twins: `neural:`
is a 1-ply argmax over the value net, `nplan:` is the same beam as `plan.rs`
with the net as leaf evaluator instead of the linear one. Both live under
`Kind::Neural`/`Kind::NeuralPlan` in `spec.rs` (§1).

Every search bot in this crate treats an invariant violation from
`crate::apply::apply` as a **panic**, not a caught exception — a candidate
that would have silently narrowed the search in the Python port instead
stops the program loudly at the point of the actual bug. Do not wrap a
search loop's `apply` call in a result-swallowing pattern; that reintroduces
exactly the failure mode this port deliberately removed.

## 6. The dominance guard: theft, and any other pure loss, must never help

`rust/src/bots/weighted/eval.rs::dominance_repair(w) -> (Weights, Vec<Violation>)`
is a **pure, idempotent** repair applied on every load and again by the
trainer's own guard (`climb.rs`), closing three rule-level orderings a
per-key sign guard cannot see because it never looks at a *sum* or compares
two *different* keys:

* **`NET_NONNEG_PHASE`** — currently empty (see §2: the two entries that
  used to live here, `culture` and `wonder_progress`, lost their phase pair
  in the 2026-08-04 retirement, so there is no multiplier left to drag their
  net weight negative). Kept, and empty, for the next phase-multiplied stock
  — deleting the loop would delete the argument with it.
* **`DOMINATES = [(ResourceStock, BlueFree)]`** — a stocked resource is
  worth at least the free token it sits on, because spending it returns the
  token to the bank *and* buys what it paid for. Repaired by **raising** the
  dominant side, never lowering the dominated one.
* **`BENEFIT_GATES`** — nine weights that each scale a printed grant on
  exactly one card class (e.g. `wonder_stages_per_action`) and may not be
  negative, because a card that prints a benefit is, under the rules, never
  worse than the same card without it. Repaired to `0.0`.

This was found by measuring a synthetic defence where a trained champion
preferred being robbed (losing 3 culture scored as +0.55; losing 4 resources
scored as +1.27) — see `docs/EVALUATOR_HISTORY.md`. **If you find a term where
a pure gain scores as a loss or vice versa, check `dominance_repair` first
before assuming it's a new bug** — it may belong in one of these three
lists rather than needing a bespoke fix.

## 7. How weights are trained, briefly

`rust/src/bin/climb.rs` hill-climbs one weight vector per player count
against **itself**: a generation mutates the champion `lambda` ways, duels
each mutant against the champion at the same table on the same deals, and
promotes the best mutant that clears the null. Two things make this safe
against the classic self-play failure (a chain of honest pairwise
improvements walking somewhere absolutely worse than where it started —
measured on the old Python league: 22.8% at 2p/3p, 13.7% at 4p against a
freshly-initialized vector, after every generation had honestly beaten its
parent):

* **The anchor gate.** A promotion is vetoed when the candidate's win share
  against a fixed **anchor** vector (the built-in defaults, unless
  `--anchor` says otherwise — never updated for the life of the climb) is
  unambiguously below the sitting champion's own. This is the *structural*
  descendant of "measure against an external yardstick" (§1's `BookBot`
  note, `docs/EVALUATOR_HISTORY.md`) — the gate no longer needs a second bot
  to catch a slide, because it asks the absolute question directly.
* **`FROZEN = [WeightKey::Culture]`.** `culture` is the numeraire every
  other weight is denominated in; scaling it rescales the whole objective
  without reordering any preference, so it is the one weight no mutation
  operator (`scatter`/`group`/`rescale`/`kick`) may touch. Enforced once, in
  `movable`, not duplicated across the operators the way the Python version
  split it — "the same rule written in two lists is precisely the bug class
  this port exists to remove."

The value net (`rust/src/bots/neural/{train.rs,net.rs}`, driven by
`rust/src/bin/neuraltrain.rs`) is trained separately, on data
`rust/src/bin/rankdata.rs` collects from `PlanBot`/`nplan` beam leaves —
a distinct system from the linear `WeightedBot` vector this document
otherwise describes. `neuraltrain` runs entirely in Rust, no `torch`.

## 8. Invariants — do not change these without re-reading why

* The `end_turn_bias` asymmetry in `WeightedBot::choose` (§2) is deliberate
  and measured twice. Do not score `EndTurn` on the post-move state to "fix"
  it.
* `culture` is frozen in the trainer (`climb.rs::FROZEN`) — it is the
  numeraire, not a preference.
* Every `WeightKey` must be read by name in production code somewhere
  outside `weights.rs`, or `registry.rs`'s tests fail (§3).
* `dominance_repair` (§6) runs on every load. A pure gain must never price
  as a loss; check the three guard lists before adding a bespoke fix for
  what looks like a sign bug.
* `WeightedBot::allow_resign` defaults to `false` and filters `Move::Resign`
  out of the candidate set whenever a non-resign move is legal — a fitted
  value vector has been measured to resign mid-game, silently contaminating
  a duel with games that ended early at a `[0, 0]` score.
* `PlanBot`/`nplan` determinize the root before searching (hidden piles
  reshuffled) so a beam cannot read cards the player has not legally seen;
  `det=0` exists only as a leak-measuring A/B, never a real playing
  configuration.
* The retired Python trainer's last committed snapshot (78 keys, now
  `analysis/frozen/python_champion_*_..._2026-07-26.json`) is a stale
  Python-era snapshot, not the live vector. `experiments/rust_champion_*.json`
  (130 keys, gitignored, training-box only) is live. Do not conflate them.
* Card/board pricing goes through `board_yields.rs`'s swap-and-diff pattern
  and `feature_marginal`, never a hand-written value table — a table drifts
  from the rules the moment a leader or wonder effect changes.
