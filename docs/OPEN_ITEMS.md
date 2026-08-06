# Open items

This file should be empty; anything not listed below has been fixed, or is
moot because the Rust port deleted the Python code it depended on. Re-checked
against `rust/src/` directly (not against its own prior prose) on
2026-08-06. Git history is the record of what used to be here and why it was
closed -- there is deliberately no "recently closed" section in this file.

## 1. `gov_action_cost` is computed and then multiplied by zero

`government_cost` (`rust/src/bots/board_yields.rs:456-464`) pushes a real
`Feature::GovActionCost` triple -- the civil actions a government revolution
burns -- on the live board-aware path, and `feature_key` maps it to
`WeightKey::GovActionCost` (`rust/src/bots/weighted/cards.rs:789`). But
nothing ever sets that weight during `features()`'s accumulation, its coded
default is 0.0 (`weights.rs:335`), and it is absent from all three live
champions (`experiments/champion_{2,3,4}p.json`). So the quantity is
computed on every relevant decision and always multiplied by zero.

Next action: this is a one-line call, not an investigation -- either seed it
away from 0.0 and let a league run climb it, or delete the computation if
it's judged not worth pricing.

## 2. Three action-card coordinates are duplicated across two live paths, not dead

`ResourceDiscount`, `RestrictedResources`, `FreeCivilAction` are each
emitted from two independent places that can disagree: the board-aware
`action_value` (`rust/src/bots/weighted/cards.rs:950,951,976`), which fires
whenever `action_board_credit` is nonzero (default 1.0, `weights.rs:354`)
and so wins on every live champion today; and the static `card_yields`
fallback (`cards.rs:324-325,373`), reached only when `card_potential` falls
all the way through to it (`cards.rs:1157-1163`, i.e. when
`action_board_credit` happens to be exactly 0.0). Both walks are real,
independently-written Rust code today -- a champion or experiment that
zeroes `action_board_credit` does not fall back to "unpriced," it falls back
to a second implementation of the same three coordinates.

Next action: either delete the static path's handling of these three
(the board-aware path is strictly newer and already wins by default), or add
a test pinning that both paths agree, before anyone tunes
`action_board_credit` toward 0.0.

## 3. Five military-deck card classes have no pricing path at all

Tactic, Aggression, War, Pact and Event cards have no board-aware pricing
concept (`board_credit_key` returns `None` for all five, plus Territory,
`cards.rs:500-506`) and no static `card_yields` pricing either: Tactic is
explicitly skipped (`cards.rs:315`), and the other four have empty
`effects` blocks in the card data, so the generic walk finds nothing to
price. All five price at exactly 0.0 on every live champion. This is
harmless today only because `hand_mil_potential` is also 0.0 on every live
champion -- the first time the league prices the military hand, all five
classes go blind simultaneously with nothing to catch it.

Territory is NOT part of this group, despite being named alongside them in
the `board_credit_key` comment at `cards.rs:500-506`: that comment is about
the absence of a board-aware BONUS multiplier specifically. Territory gets
real static pricing through `card_yields`/`territory_credit` (default 1.0,
`cards.rs:244-275`), so it is not truly 0.0.

Next action: needs a mapping from a tactic's strength table / an
aggression's one-shot steal / a pact-in-hand onto some board feature before
there's anything to wire up. Single largest unpriced surface in the
evaluator; no small fix exists here.

## 4. `wonder_overrun` is unclimbed, not broken

The formula is live (`features.rs:489` sets it on every call) and tested
(`wonder_overrun_fires_for_a_constructed_near_completion_shortfall_state`,
`features.rs:641`, fires `> 0.0` on a constructed near-overrun state) -- an
earlier version of this item claimed the feature itself computes 0.0 as a
bug; that claim was checked, refuted with the constructed counterexample
above, and is closed. What's still true: the weight defaults to 0.0
(`weights.rs:324`) and stays there on all three live champions
(`experiments/champion_{2,3,4}p.json`), so it has simply never been climbed.

Next action: nothing broken to fix. If it's worth pricing, seed it away
from 0.0 and let a league run try it.

## 5. No Rust test enumerates the coordinate registries

Python's `tests/test_coordinate_registry.py` asserted, in both directions,
that every weight had a live reader and every reader had a declared weight,
with a `KNOWN_DEAD` allow-list ratchet for the exceptions. That test, and
the concept, died with the Python engine -- a repo grep for
`KNOWN_DEAD`/`coordinate_registry` in `rust/src/` today finds only two
prose mentions of the retired Python tool (`horizon.rs:77`,
`features.rs:230`), no actual enumeration. Per-coordinate guarantees now
live piecemeal in named unit tests (items 1-4 above are examples), which is
strictly weaker: nothing fails if a `WeightKey` variant is added with no
reader, or a reader is added with no matching variant. This is the repo's
headline recurring bug class (a guarantee asserted in one place, absent in
another, nothing failing when they disagree) and it is currently unguarded
here. `#![allow(dead_code)]` at `rust/src/lib.rs:14` (item 6) compounds it:
dead-code warnings are the compiler-level version of half this same check,
and they are switched off.

Next action: write a Rust test that walks every `WeightKey` variant and
asserts it has at least one call site (and, ideally, the reverse), mirroring
the shape of the Python ratchet rather than inventing a new design.

## 6. `#![allow(dead_code)]` at `rust/src/lib.rs:14` is ready to come off

Its own comment says "delete this line once `effects` and `actions` are
ported." `effects` is a real, substantial module (`rust/src/effects.rs`,
1839 lines) and the Python `actions.py` responsibilities now live across
`apply.rs`/`legal.rs`/`moves.rs`, none of them stubs. Both preconditions
read as met.

Next action: queued, not done here -- another agent may currently own
`rust/src/lib.rs`. Removing the line and letting the compiler's own
warnings surface whatever is actually unused is a good companion to item 5's
registry test (they'd likely turn up some of the same gaps from opposite
directions).
