# Distilled evaluator history

Dated, terse entries carried forward from ten deleted docs (2026-08-06 doc
cull) that were narrative research journals over `engine/bots/` (the Python
engine, deleted 2026-08-06). Kept only what would cost real time to
re-derive: measurements with a sample size, and "do not fix this, it was
measured" rulings. Everything here was checked against the current tree; a
`rust/src/...` path in an entry exists as named. Architecture-level material
(bot roster, scoring structure, weight declaration, invariants) lives in
`docs/BOT_ARCHITECTURE.md` instead of here.

## Model constants: measure it, don't fit it (MODEL_CONSTANTS.md, 2026-07-30)

The standing principle ("proper always" — see the `model-it-properly-always`
memory note): a fitted constant standing in for a quantity the game state
already knows exactly or can measure live is always the wrong model, because
it silently goes stale the moment the policy it was fitted under changes.
Four instances, all now live in Rust as measured/exact quantities rather than
fitted tables:

* **Deal rate.** `n * SWEEP[n]` is exact (rule-derived); the take-rate half
  is measured in the game being played, not assumed. `rust/src/bots/weighted/horizon.rs::take_rate`
  (prior `TAKE_PRIOR = [0.30, 0.35, 0.40]`, shrunk away within ~2 replenishes).
* **Lateness.** `1 - cards_unseen/supply`, exact, no per-player-count fitted
  table. `rust/src/bots/weighted/horizon.rs::lateness`.
* **Rival take probability.** Replaced a flat `RIVAL_TAKE_P = 0.25` with a
  model over open information (rival's civil actions, hand fullness, what
  the card costs them, competing reachable slots), leaving one genuine prior
  (`share`, the fraction of a rival's actions spent on the row) exposed as
  the weight `rival_take_share`. `rust/src/bots/weighted/row.rs::rival_take_p`;
  `WeightKey::RivalTakeShare` in `rust/src/bots/weighted/weights.rs`.
* **`FREE_POP_UTIL`** (Ocean Liners' free-population-increase utility rate):
  measured at **0.17** (410 player-turns then, re-confirmed since), not the
  stale 0.13. `rust/src/bots/board_yields.rs::FREE_POP_UTIL = 0.17`, with the
  measurement note in that constant's own doc comment — it is the model
  every other constant in the port is held to.

## Rate horizon: a per-turn rate is worth `rate x rounds_left` (RATE_HORIZON.md, 2026-07-31)

A per-turn RATE feature (culture/science/food/resource) can never be worth
more than `rounds_left` times its value — you cannot collect a rate more
times than there are turns left. The old phase-blend shaping (`+early`/
`-late` pair) could not learn this ceiling; measured dynamic range 2.2x
against a true ~17x, and the live 2p champion of the time paid up to 25x its
own ceiling in Age IV. Fixed by multiplying the four rate weights by a
`rounds_left`-derived, mean-normalised horizon term rather than leaving the
climb to rediscover an affine approximation of it.

Live at `rust/src/bots/weighted/horizon.rs::{rounds_left, horizon_scale, rate_multiplier}`,
gated by weight `RateHorizon` (default **1.0**, i.e. shipped on — see that
weight's own comment in `weights.rs`). Applied identically to `evaluate` and
to `feature_marginal`, which is what makes every card-pricing site agree
with `evaluate` about the horizon by construction.

**Superseding update (2026-08-04, from `weights.rs`'s own comment):** the
older approach of also phase-blending the four rate keys plus `culture` and
`wonder_progress` through `_early`/`_late` weight pairs was retired outright
— those six are no longer in `PHASE_KEYS`. The rate horizon is now the only
mechanism pricing "how much game is left" for a rate; `culture`/
`wonder_progress` are pure numeraire/stock terms a phase blend must not
rescale. Only four keys remain phase-blended: `Workers`, `StrengthRel`,
`TechLevels`, `HandValue`.

## Government pricing: swap-diff plus the cheaper legal route (GOVERNMENT_PRICING.md, 2026-07-31)

A government was unpriced in the Python evaluator on both sides (no science
cost read, no civil/military-action/urban-limit gain read) because it prints
its cost as `peacefulCost`/`revolutionCost`, not the shared `techCost` field
every other technology uses. Fixed by pricing the level delta plus the
`effects.compute` swap diff as the gain, and taking the **cheaper of the two
legal routes** (peaceful: `peacefulCost` science + 1 civil action; revolution:
`revolutionCost` science, raw, + every civil action you have, gated on
`_can_revolt`) as the cost — read off the board every time, not a fitted rate
for "how often a revolution is worth it". The revolution's burnt civil-action
pool prices through `ca_left` (the actions-remaining coordinate), not
`civil_actions` (the allotment, which is the *gain* side already priced by
the swap diff) — charging the allotment would double-charge with the wrong
sign.

Live at `rust/src/bots/board_yields.rs::{government_plans, government_routes,
government_level, government_cost}`, gated by `WeightKey::GovBoardCredit`
(default **1.0**). The Rust port additionally closed the root cause
structurally rather than reproducing the Python workaround: `Card::peaceful_cost`/
`revolution_cost` are read directly, there is no shared `techCost` field to
miss in the first place.

## Theft must never help: the dominance guard (THEFT_IS_PRICED_BACKWARDS.md, 2026-08-04)

Two independent trained-vector defects found by playing a synthetic defence
to conclusion: a phase-multiplied term (`culture` + `culture_early`) whose
**net** sign was negative early, so losing 3 culture scored as a +0.55 gain;
and `resource_stock` sitting at 0.0 while `blue_free` (a dominated
sub-component — spending a resource returns the token AND buys the thing) was
positive, so being plundered of 4 resources scored as a +1.27 gain. The
per-key sign guard that existed could not see either: it checks individual
weights, never a sum, and never checks two terms against each other. Also
found: nine "benefit gate" weights (each the *only* per-card channel for its
class, e.g. `wonder_stages_per_action`) were negative on live champions even
though a printed benefit can, under the rules, never make a card worse.

Fixed by `dominance_repair`, applied both on load and by the trainer's own
guard, repairing to the boundary (never lowering what training already
measured): net-nonnegative phase terms, a `resource_stock >= blue_free`
dominance pair, and benefit gates floored at 0.0. Live at
`rust/src/bots/weighted/eval.rs::{dominance_repair, NET_NONNEG_PHASE, DOMINATES, BENEFIT_GATES}`,
pinned by that module's own unit tests (`repairing_twice_changes_nothing_the_second_time`,
`the_resource_pair_is_repaired_by_raising_the_dominant_side`, etc.). Note
`NET_NONNEG_PHASE` is currently **empty** — both entries that used to live
there (`culture`, `wonder_progress`) lost their phase pair entirely in the
2026-08-04 `PHASE_KEYS` retirement (see the rate-horizon entry above), so
there is no longer a multiplier on either that could go negative. The branch
is kept for the next phase-multiplied stock, not deleted.

## The 1-ply/quiescent-trained vector did not transfer to PlanBot's search — closed by giving PlanBot war lookahead (TRANSFER_TEST.md, 2026-07-27)

A vector trained under `quiesce:levels=1` (which prices a declared war
through its resolution via `WAR_LOOKAHEAD`) was the stronger vector under
1-ply and quiescent search, and the **weaker** one under PlanBot's beam
(2.5% win share head-to-head, own score collapsing from ~138 to 53) — because
PlanBot at the time had no war lookahead and scored a declared war as pure
cost. Measured two independent ways (head-to-head, and paired vs a common
opponent), both agreeing the flip was driven by that one flag (52.8 ± 4.3
margin points). The write-up's own recommended fix (§8, option b): "give
PlanBot a war lookahead... the cheapest and the one this document's own
data argues for directly."

**That fix shipped.** `rust/src/bots/plan.rs` prices a declared war of the
mover's own through `quiescent::war_value` (the same function `QuiescentBot`
uses) whenever `war_lookahead` is true, which is `PlanConfig`'s **default**
(`war_lookahead: true`). The general lesson stands as a standing caution
even though the specific instance is fixed: a vector's quality is a property
of the (vector, search) pair, not of the vector alone — do not assume a
weight vector transfers across a change of search policy without checking
which move classes each search actually prices.

## The book-bot yardstick and the self-play blind spot (STRENGTH_CHECK.md, 2026-07-30)

The methodological point, not the numbers (which were measured against a
long-superseded pre-league Python champion and are not reproducible): a
population trained only in self-play can report healthy win rates forever
against itself while being weak in an absolute sense, because nothing in
the loop ever asks the absolute question. `BookBot` — a hand-written,
rule-based external yardstick with no search and no learned weights, cited
to tournament data — was built to answer it, and at the time beat the
trained champion at every player count.

This is why `rust/src/bin/climb.rs`'s hill-climb gates promotion against a
fixed **anchor** vector (the untuned defaults, by default) in addition to
the self-play opponent, vetoing any promotion that is unambiguously weaker
against the anchor than the sitting champion — see `climb.rs`'s own top doc
comment, "every champion the Python league ever produced turned out to be
far WORSE than the untuned starting vector... while every single generation
had honestly beaten its own parent." Same failure mode, structural fix this
time instead of an external bot. `BookBot` itself is fully ported
(`rust/src/bots/book.rs`) but is not currently wired into any binary in
`rust/src/bin/` — it exists as an available module, not an active part of
today's training or gating loop.

## Superseded without independent content

`docs/CULTURE_GAP.md` (2026-07-26/27, ~2,000 lines, four self-correcting
research sessions) is where the need for both the dominance guard and the
rate horizon was first noticed. Both shipped, later and more precisely, as
the two entries above (`THEFT_IS_PRICED_BACKWARDS.md` → `eval.rs`;
`RATE_HORIZON.md` → `horizon.rs`); nothing in the doc's investigation of the
Python trainer's mutation step sizes, its gate-scoring (`tanh` margin
credit) experiments, or its multi-part self-corrections has a live
counterpart — `rust/src/bin/climb.rs` uses neither `tanh` margin credit nor
the Python trainer's group/rescale mutation bug the doc spent several
sections on. Nothing carried forward independently.

`docs/COORDINATE_REGISTRY.md`'s concept (every weight must have a live
reader, every reader a declared weight, checked in both directions) is
carried into `docs/BOT_ARCHITECTURE.md` directly, pointing at its live
replacement, `rust/src/bots/weighted/registry.rs`.

`docs/BOT_ROSTER.md` (2026-07-30) measured a round-robin of eleven
rule-based bots (`CultureBot`, `InfraBot`, `WonderBot`, `ScienceBot`,
`TempoBot`, `MilitaryBot`, two `BookBot` variants, plus the 1-ply
`champion`) — its own banner already flagged this as predating
`PlanBot`/`QuiescentBot` and never re-run. None of those rule-based variant
bots exist in `rust/src/bots/`; the current roster is `BotKind` in
`rust/src/bots/greedy.rs` (random/greedy/weighted/quiescent/plan) plus the
checkpoint-backed `neural`/`nplan` kinds in `rust/src/bots/neural/spec.rs`.
Do not resurrect the old names as if they describe a current pool.

## Closed items still worth knowing (from OPEN_ITEMS_CLOSED.md's 2026-08-05 triage)

Trimmed to the entries that name a durable code fact; the process/meta items
(dangling git commits, stale benchmark scripts, the PyPy re-test trigger)
are dropped as pure history with no current code path.

* **Sixteen of thirty-three action cards priced at exactly 0.000** because
  the static price table read coordinates `evaluate` never paid — fixed by
  routing through the feature marginal. `action_board_credit` is **1.0**
  live (not 0.0-gated) in `rust/src/bots/weighted/weights.rs`; the pricing
  path is `rust/src/bots/weighted/cards.rs::action_value`.
* **Production buildings were priced absolute instead of as the upgrade
  delta.** `rust/src/bots/board_yields.rs`'s tech-upgrade path prices via
  `upgrade_cost`, the diff, not the absolute yield.
* **Isaac Newton's leader ability** (regains 1 civil action after a
  revolution) is priced deterministically through the revolution-cost route
  rather than guessed — `rust/src/bots/board_yields.rs:528` (`leader_is(p,
  "Isaac Newton")` inside the government-routes pricing).
* **Knights/Cannon/Air Forces (never-upgradable red cards)** were
  permanently under-priced by the upgrade-table pattern; they get the
  build-fresh plan instead. `rust/src/bots/board_yields.rs::build_fresh`,
  gated by `WeightKey::BuildFreshCredit` (default 0.0 — tracked as an open
  item, not fully live).
* **Wonder culture-on-completion** (Hollywood/Internet/Fast Food
  Chains/First Space Flight/Ocean Liners) now prices through the same
  function `apply.rs` pays out for real (`wonder_completion_culture`), so
  the evaluator and the scorer cannot disagree. Reachable only through
  `WeightKey::CardBoardCredit` (default **0.0**) and `WeightKey::WonderPotential`
  (default **0.0**) — the code path is correct, but the credit that turns it
  on is off by default; see `docs/OPEN_ITEMS.md` if it still lists this as
  open.
* **Military discard pile legibility** — ruling from Paul, verbatim: "Card
  counting is legal. All public info can be used." Not a bug; both piles
  are readable.
