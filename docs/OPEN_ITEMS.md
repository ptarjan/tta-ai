# Open items

This file should be empty; anything not listed below has been fixed, or is
moot because the Rust port deleted the Python code it depended on. Re-checked
against `rust/src/` directly (not against its own prior prose) on
2026-08-06. Git history is the record of what used to be here and why it was
closed -- there is deliberately no "recently closed" section in this file.

## 1. `gov_action_cost` has been climbed repeatedly, not multiplied by zero -- corrected 2026-08-06

`government_cost` (`rust/src/bots/board_yields.rs:456-464`) pushes a real
`Feature::GovActionCost` triple -- the civil actions a government revolution
burns -- on the live board-aware path (`gov_value`, gated on
`gov_board_credit`; `feature_key` at `cards.rs:817` maps it to
`WeightKey::GovActionCost`). The coded default is 0.0 (`weights.rs:335`), and
this item previously claimed it was "absent from all three live champions,"
citing `experiments/champion_{2,3,4}p.json` (since moved to `analysis/
frozen/python_champion_{2,3,4}p_..._2026-07-26.json`, see that directory's
`README.md`) -- but that file is a stale
Jul-26 snapshot (78 weight keys, no `gov_action_cost` entry at all, untouched
since) from before the coordinate existed in the schema, not the live
champion. The Rust league's actual output (`experiments/rust_champion_
{2,3,4}p.json`, written by `climb` per `experiments/rust_league.sh`,
currently at generation 1083/777/241) has `gov_board_credit` substantially
nonzero on all three (1.03/7.07/-2.66) and `gov_action_cost` drifted well off
0.0 under real training pressure: 0.351/-0.201/0.029. The climb log confirms
the mutation operator reaches it regularly: a `board`-group op was tried in
98 of 1069 logged 2p generations and present in 18 of the 188 accepted ones
(`experiments/logs/rust_climb_2p.jsonl`; similarly 63/762 tried and 15/143
accepted at 3p, 20/236 tried and 8/50 accepted at 4p). So it has been climbed
many times over, not left at its default.

Next action: not a seed-and-climb task any more -- what's actually missing
is an ablation to tell whether the drift on this coordinate is signal or
noise.

## 2. `wonder_overrun` has been climbed too -- corrected 2026-08-06

The formula is live (`features.rs:489` sets it on every call) and tested
(`wonder_overrun_fires_for_a_constructed_near_completion_shortfall_state`,
`features.rs:641`, fires `> 0.0` on a constructed near-overrun state) -- an
earlier version of this item claimed the feature itself computes 0.0 as a
bug; that claim was checked, refuted with the constructed counterexample
above, and is closed. The weight's default is 0.0 (`weights.rs:324`), and
this item then claimed it "stays there on all three live champions,"
citing `experiments/champion_{2,3,4}p.json` (see item 1's note above -- since
moved to `analysis/frozen/python_champion_{2,3,4}p_..._2026-07-26.json`) --
the same stale Jul-26 snapshot item 1 was corrected against, not the live
champion. The Rust
league's actual output (`experiments/rust_champion_{2,3,4}p.json`, gen
1083/777/241) reads -0.073/-0.249/0.101 -- drifted well off 0.0, same as
`gov_action_cost`. The `wonders`-group mutation op was tried in 85 of 1069
logged 2p generations and present in 16 of the 188 accepted ones
(`experiments/logs/rust_climb_2p.jsonl`; similarly 63/762 tried and 15/143
accepted at 3p, 18/236 tried and 6/50 accepted at 4p).

That said, this is weaker evidence of a *working* coordinate than item 1's:
`overrun` is 0.0 whenever no wonder is in progress or none is at risk of
overrunning, which the 6-game/~2000-state coordinate-registry corpus
apparently never samples outside the constructed test case above -- so a
champion carrying -0.25 there could be riding pure noise the win-rate gate
had no live signal to reject or confirm.

Next action: nothing broken to fix, and it is not unclimbed -- what's
missing is a real-game frequency measurement (how often does `overrun`
actually go nonzero in league or corpus play?) before trusting either the
sign or the magnitude of the drift.

## 3. `evaluate()` has no opinion of `Move::Resign` because nothing ever trained it one -- opened 2026-08-06

Every `allow_resign` field in this crate (`WeightedBot`, `RandomBot`,
`NeuralBotConfig`, `NeuralPlanConfig`, `QuiescentBot` as of this commit) is a
workaround for the same underlying defect, stated plainly in
`WeightedBot::allow_resign`'s own doc comment
(`rust/src/bots/weighted/eval.rs:263-271`): a value vector fitted by
regression has been measured to resign mid-game, because the 1-ply score of
the post-resign trial state can beat the score of playing on. Measured fresh
for this commit: `QuiescentBot`, which lacked the guard entirely, resigned in
43%/67%/75% of 2p/3p/4p games on trained champion weights and won 1.25%/
0.42%/0.00% of its games against `WeightedBot` on shared deals (nulls
50%/33.3%/25%) -- it was losing on purpose, not on merit.

Traced one level deeper than the existing doc comments go: this is not a
sign error or a missing term waiting to be added to `evaluate`
(`rust/src/bots/weighted/eval.rs:133-252`), which is a plain dot product of
`WeightKey::ALL` against `features::features` -- it has no `state.game_over`/
`resigned` special case at all, by design (§2 of `docs/BOT_ARCHITECTURE.md`:
the whole evaluator is "the same rule written once"). The real cause is that
`Move::Resign` is categorically absent from the data the weight vector was
ever fit or validated against:

* `h_resign` (`rust/src/apply.rs:1177-1231`) empties both hands, drops every
  pact, and tears down every war to or from the resigning player, then calls
  `game::after_resign`. This is a large, one-shot swing in exactly the raw
  material (`hand_civil`, `hand_military`, `pacts`, `war_declared_by_me`,
  `wars_declared_on_me`) several `features()` terms read -- but which
  direction that swings `evaluate` depends on the fitted sign of each
  affected `WeightKey`, and nothing has ever checked which way it actually
  goes for a live champion.
* Nothing that fits or validates those signs ever samples a resigned state to
  check: `WeightedBot::choose`/`RandomBot::choose`/`neural::bot::pick`/
  `neural::plan::pick_collecting` all filter `Move::Resign` out of their own
  candidate sets by default (this is `filter_resign`'s whole job, see
  `rust/src/bots/mod.rs`); the self-play driver `climb`/`rankdata` actually
  train and validate against
  (`rust/src/bots/weighted/registry.rs:262-264`'s `sample_nonzero_feature_keys`
  helper, and every other corpus-driving loop in this crate) explicitly
  filters `Move::Resign` out of ITS random walk too, with the comment "this
  test's sampling of it... on purpose." Grepping `rust/src/bin/*.rs` for
  `allow_resign` returns nothing: no training or league binary ever
  constructs a bot with it `true`.

So the fitted vector has never been shown a single post-resign position, in
training or in any coordinate-registry check, and nothing in its objective
has ever penalized whatever `evaluate` happens to say about one. That a
constructed-to-avoid-Resign search occasionally trips over a Resign candidate
that outscores everything else is not a bug in any one formula; it is every
formula's blind spot on an input they were never fit against, discovered only
because a fifth bot (`QuiescentBot`) forgot to keep filtering it out. This
also means the current fix (filter it everywhere) is not a stopgap for a
formula that will get corrected later -- there is no planned correction; as
long as `Move::Resign` stays outside every training/validation corpus,
`allow_resign: false` is the only thing keeping this class of bot from
losing on purpose, on any future weight vector, forever.

Next action: not a formula fix -- `evaluate` cannot be trusted to score
`Move::Resign` correctly by construction, so there is nothing to "fix" in
`eval.rs` itself. Two real options, neither attempted here (out of scope for
this item; this is a characterisation, not a design doc): (a) keep
`Move::Resign` permanently out of every candidate set a linear evaluator ever
scores, which is already true today and should stay a documented invariant
rather than five independent opt-outs that a sixth bot can still forget --
`filter_resign` (`rust/src/bots/mod.rs`) is that invariant now, shared; or
(b) if a bot ever needs to decide WHETHER to resign for real (down a lost
game, salvaging tournament time), that decision needs its own model trained
on actual resign/no-resign outcomes, not `evaluate`'s board-position vector
repurposed for a question it was never fit to answer.
