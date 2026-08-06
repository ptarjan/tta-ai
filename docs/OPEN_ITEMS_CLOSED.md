# Open items — closed/obsolete ledger

One line per item removed from `docs/OPEN_ITEMS.md` during the 2026-08-05
triage. Format: item — category — evidence. Categories: (1) closed by the
Rust port, (2) closed, period (fixed in source, doc not updated), (3)
obsolete.

* Wonder culture-on-completion pricing (Hollywood/Internet/Fast Food
  Chains/First Space Flight/Ocean Liners priced at 0.00) — (2) —
  `rust/src/bots/board_yields.rs:34-36` "BOTH NOW CLOSED"; `_on_build_culture`/
  `free_pop_increase` price all five in both engines.
* `tech_levels` phase-blend ratio bug (culture beats science by
  construction) — (2) — live champions carry the corrected `tech_levels`
  group; `card_potential` reads the same phase-blend `evaluate()` does.
* `free_civil_action`/`resource_discount`/`restricted_resources`
  unreachable-weight bug on action cards — (2) — `weighted.action_value`
  routes through `feature_marginal` in both engines.
* Production buildings priced absolute instead of delta — (2) —
  `board_yields.tech_upgrade`/`board_yields.rs` price the upgrade diff via
  `upgrade_cost` in both engines.
* Newton's leader ability guessed rather than measured — (2) — priced
  deterministically via `_government_routes` (revolution-cost route); no
  longer in `tests/test_board_yields.py`'s `STILL_FLAT` set.
* Colonization-sacrifice force picked by a hardcoded engine rule instead of
  a player decision — (1) — now a `colonize` pending decision
  (`send_unit`/`send_bonus`/`send_done`), ported correctly to
  `rust/src/moves.rs`/`interact.rs`/`apply.rs`; pinned by
  `tests/test_colony_sacrifice_choice.py`.
* `unit_upgrade` inventing upgrades not offered by legality (e.g. Warriors
  → Cavalrymen) — (2) — shares `_upgradable_onto`/`upgradable_onto` with
  `tech_upgrade` in both engines.
* Government pricing unreachable (`techCost` null on government cards;
  civil/military actions and urban limit not read at all) — (2) + (1) —
  `weighted.gov_value`/Rust `gov_value` price the swap diff correctly in
  both; Rust additionally fixed the root cause structurally
  (`Card::peaceful_cost`/`revolution_cost` read directly, no shared
  `techCost` field).
* Sixteen of thirty-three action cards priced at exactly 0.000 — (2) —
  `action_board_credit = 1.0` live (not 0.0-gated) in both engines.
* Knights/Cannon/Air Forces (never-upgradable red cards) permanently
  under-priced — (2) — `board_yields.build_fresh`/`board_yields.rs` give
  all three the build-fresh plan in both engines (residual capped by
  `build_fresh_credit = 0.0`, tracked as an open item).
* `hillclimb_pool.CULTURE_CENTRE`/`CULTURE_SCALE` flattening the 4p
  objective — (3) — both constants deleted, not re-fitted; replaced by
  per-player-count `LEAD_SCALE`.
* GAP 5, no civil-discard record — (2) — `state.civil_discard` exists in
  both engines, shaped like `discarded_military`; encoder reads both piles.
* Military discard pile legibility (hidden vs public) — (2) — ruling from
  Paul, verbatim: "Card counting is legal. All public info can be used."
* `tools/gate.sh`'s WeightedBot digests going stale silently — (2) —
  superseded by the standing "compute twice, require agreement" practice
  now documented in `docs/HAZARDS.md`.
* `quiesce_bench.py`/`no_credit_check.py`/`behaviour_counts.py` silently
  defaulting to the horizon-invalidated `champion_4p.json` — (2) — all
  three now require an explicit spec or refuse to run without one.
* `analysis/opening_order.py`'s `card_type()` always returning `"?"` — (2)
  — fixed, commit `c64971e`.
* Pact-legality gate checked a live player count instead of setup-time
  count (mid-game resignation could wrongly ban pacts) — (2) — fixed,
  commit `33bd1567`, `engine/actions.py:315` now reads `state.num_players`.
* Stale benchmark shell scripts + `bench_interp.py --kinds weighted`
  silently benchmarking GreedyBot — (2) — scripts deleted, dispatch fixed,
  commit `c64971e`.
* "Lost work" in dangling git commits — (3) — re-verified via `git fsck`;
  every dangling commit is ordinary rebase/amend debris already superseded
  on master, nothing recoverable.
* PyPy's re-test trigger ("once bots stop copying a whole GameState per
  move") — (2) — met by the journal rewrite; results already reported in
  `docs/PYPY.md` (3p/4p sub-question re-tracked as a separate open item).
* `experiments/summarize.py`'s `GROUPS` table missing four features
  (`pact_blocks_attack`, `auction_committed`, `auction_bid`,
  `hand_potential`) — (2) — fixed; `group_of()` now raises on an unmapped
  key instead of silently returning `"?"`.
* `experiments/behaviour.py` calling an undefined `all_snaps_iter` — (2) —
  fixed, commit `c64971e`.
* Two disagreeing `hand_potential` win-rate measurements (69.6%/72.5%)
  treated as a discrepancy — (2) — not a bug; both are correctly
  attributed to two different implementations in `docs/WASTED_ACTIONS.md`.
* `interact.py`'s `_c_pact_offer` doing `owner.pacts = [...]` (read as an
  accept-destroys-other-pacts bug) — (2) — this is the printed rule, not a
  bug (2015 rulebook: only one pact per play area, any existing pact is
  automatically cancelled); documented and tested as correct in both
  engines.
* `book.py`'s pact-offer response reading `pend["ctx"]["from"]` (always
  `None`, so BookBot accepted every pact offered) — (2) — fixed to read
  `"owner"` in both engines.
* Winston Churchill's military science/resource option unrestricted — (2)
  — ring-fenced via `mil_discount`/`mil_sci_discount` pools, spendable only
  on military technologies/units, in both engines.
* `state.current_events_age` never written — (2) — closed 2026-08-05 per
  the doc's own note; confirmed both engines now sync it on every reveal.
* `PlayerState.caesar_double_politics_used` never written (missing rule) —
  (2) — closed 2026-08-05; Julius Caesar's double political action is now
  implemented in both engines.
* `PlayerState.bach_upgrade_used` never written (missing rule) — (2) —
  closed 2026-08-05; J.S. Bach's theatre upgrade, Barbarossa's combined
  population/unit action, and Cook's military-card discard-for-colonization
  are all now implemented in both engines.
* `best_arena`/`discontent` constant-in-encoding findings — (2) —
  overturned within a day by the red-card/government pricing fixes; no
  longer in `tests/test_coordinate_registry.py`'s `KNOWN_DEAD`.
* Current-events order leak making Joan of Arc's peek worthless, for
  `PlanBot`/`NeuralBot`/`NeuralPlanBot` specifically — (2) —
  `determinize` now shuffles `current_events` with an explicit
  Joan-of-Arc-aware pin, in both `engine/bots/plan.py` and
  `rust/src/bots/plan.rs`. (`WeightedBot`/`QuiescentBot`, which never call
  `determinize` at all, stay open — tracked in the main doc.)
* Request for a standing hazards document — (2) — `docs/HAZARDS.md` now
  exists and covers exactly this ground.
* `PlayerState.destroyed_wonders` always zero — (2) — pre-closed
  2026-08-05 per the doc's own note; confirmed correct (nothing in the
  base game removes a completed wonder).
* Card-data provenance open items — (2) — pre-closed per the doc's own
  note; the appendix of `docs/RULES_SPEC.md` covers them.
* "The project has no external anchor of any kind" — (3) — superseded by
  the 1,011-game BGO corpus (`sources/bgo/index.tsv`), which is now a real
  external anchor (though the app harness half of the same item is still
  open — never run for an actual measurement).
* The "deliberately not open" list at the end of the card-pricing section
  (wars/aggressions repaired by search; Military Bonus cards have no move
  handler by design; pacts absent at 2p by rule; flooring `card_potential`
  at 0 was tried and rejected) — reviewed, still accurate, not re-listed as
  open items — never open to begin with, kept out of both documents.
