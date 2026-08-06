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
