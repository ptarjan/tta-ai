# Open items

Triaged 2026-08-05 against the landed Rust port (`rust/src/`: full rules
engine plus a substantial bot port — `weighted/` evaluator, `board_yields`,
`book`, `greedy`, `plan`, `quiescent`, `pending`, `counting`, `neural/`).
Every item below was checked against current source in both engines, not
against its own prior prose. About 40 items were closed or ruled obsolete —
see [`docs/OPEN_ITEMS_CLOSED.md`](OPEN_ITEMS_CLOSED.md) for the receipt.
**Headline finding: the Rust port has, so far, faithfully mirrored almost
every open pricing/evaluator defect rather than fixing it** — the same
weight names, the same 0.0-gated credits, often the same doc comment,
carried from `engine/bots/*.py` into `rust/src/bots/*.rs`. A handful of
structural engine bugs (the colony-sacrifice decision, the pact-offer
partner lookup, Churchill's ring-fenced discount, four leader abilities,
the Age-IV pact-legality gate) were genuinely fixed in Python before or
during the port and carried across correctly. Anything listed below is
believed genuinely open as of this triage; the codebase moves fast (three
agents were editing it concurrently while this was written), so re-check
before trusting a specific number.

## Needs a decision from Paul

**Wonder-programme abandonment tradeoff.** At `wonder_potential = 0.125`,
started-but-unfinished wonder programmes rose 0.032 → 0.271 per deal while
the finish *rate* improved 5% → 37%. Is a bot that starts eight times as
many wonders and finishes over a third of them better or worse than one
that barely starts any? The training objective has no term for this either
way — it needs one, and the shape of that term is a game-design call, not
an engineering one.

**What is a one-shot "produce your rate" event worth?** `_EVENT_YIELD`'s
`produceFood`/`produceResources` entries (`engine/bots/weighted.py:317-318`)
convert a boolean `true` into a magnitude via `float(True) == 1.0` —
`data/cards_military_actions.json:1870,1892` really does print
`"produceFood": true`. Rust's `event_block_value`
(`rust/src/bots/weighted/events.rs:229`) doesn't model these two fields at
all, and its doc comment confirms the same underlying fact: the only call
site that would price them is never actually invoked with an `allPlayers`
block that carries them, so the path is provably dead in both engines
today. If it's ever wired up, "one-shot equivalent of a rate" needs a real
definition and `state` threaded into `event_block_value`/`_event_block_value`
to compute it — not another flat constant.

**Is there a missing Bronze-over-Iron upgrade-timing rule?**
`V2_TUNABLES["iron_over_bronze"]` was deleted as provably dead in both
`engine/bots/book.py:1117-1122` and `rust/src/bots/book.rs:33-41` (read by
zero rules, in either language). Its name and the removed comment ("upgrade
to Iron only if it lands early in Age I") suggest a rule that was tunable
but never written, not just an unread tunable. Is this worth writing, or
was the knob genuinely never needed?

**Three stale/dead evaluator coordinates, no owner for the call**
([`docs/COVERAGE_AUDIT.md`](COVERAGE_AUDIT.md) §7): should
`rival_culture_rate`/`rival_science_rate`/`rival_strength` be made live or
deleted; should `wonder_remaining` be sign-locked or replaced; should
`p.resigned` get any evaluator term at all (currently read only for
list-filtering, never scored — `engine/bots/weighted.py`, no `Resigned`
`WeightKey` exists in Rust).

**Should the league's opponent-saturation rule use the lead column instead
of win-share?** `experiments/hillclimb_league.py:1390-1404` retires a
pooled opponent once it's saturated on win-share (`blend`'s tiebreak), but
an opponent can be saturated on win share while still discriminating on
culture lead. Using the check's own culture/lead columns for the
saturation rule instead was considered and deliberately not built.

**When should the never-climbed coordinates get priced?**
`wonder_stages_left`/`wonder_turns_to_finish`/`wonder_stages_per_action`,
`build_discount`, `colonize_bonus`, `event_scoring_margin`,
`hand_swap_extra`, `hand_mil_potential` are all seeded at exactly 0.0 in
both `engine/bots/weighted.py` and `rust/src/bots/weighted/weights.rs`,
deliberately, so the league would climb them. It never has, on any of the
three live champions. Is this worth a dedicated Python-league push now, or
does it wait for Rust to get its own self-play loop?

**Should the Rust evaluator get a `has_unit` cliff feature?** Commit
`2713037` on `origin/has-unit-ab` adds 9 lines to `engine/bots/weighted.py`
on the argument that owning your *first* military unit is a cliff, not a
slope: a player at zero units is dropped from every aggression auction
before they get a decision, so unit #1 is worth far more than unit #5, and
the linear `unit_workers` coordinate cannot express that. It was parked
pending a 3p/4p no-harm A/B — the offline-batch workflow that has since
been retired in favour of landing on master and reading the league runs.
It lives only in Python, so it dies with the `engine/` tree unless it is
ported. Three ways out: port it into `rust/src/bots/weighted/` seeded at
0.0 and let the league price it; port it live at the branch's value; or
drop the branch. This is a call about what the bot *values*, so it is not
an engineering decision.

## Open engineering items

### Card pricing (mirrored unfixed into Rust — cite is `engine/bots/weighted.py`
### + `rust/src/bots/weighted/*.rs` / `board_yields.rs` unless noted)

* **Military unit technologies (10 cards) price strictly negative.**
  `row_pressure` skips any card with `card_potential <= 0`, so holding a
  unit card lowers the evaluation; `unit_strength_credit` (the gate) is 0.0
  in both engines. Structural: only `strength_deficit`, a board query, can
  flip a unit card positive — no per-card table fixes this.
* **Twelve special technologies price net negative.** Benefit side
  (`build_discount`, `wonder_stages_per_action`, `colonize_bonus`) ships at
  0.0 in both engines; the science cost side is trained and live.
* **Military Bonus cards (`defenseBonus`/`colonizationBonus`) have no
  board-credit entry.** `board_credit_key`/`_board_credit_key` returns
  `None` for a military card in both engines (`rust/src/bots/weighted/cards.rs:473-477`),
  so it falls through to the bare `card_board_credit` gate instead of a
  dedicated credit the way leaders/actions/governments/wonders each got.
* **`cost.militaryActions` (54 cards) is read by no bot pricing path in
  either engine** — deliberate: pricing the cost alone, without also
  pricing the grant, would reproduce the historical negative-unit-card bug.
  No action needed unless a joint cost+grant scheme is designed.
* **`card_board_credit`'s leader and action thirds are still inert** by
  default in both engines; only the government third
  (`gov_board_credit = 1.0`) is wired and confirmed effective.
* **A wonder physically cannot enter `hand_civil`**
  (`rust/src/apply.rs:464-482`, `engine/actions.py:855`), so
  `hand_potential` never sees it — a structural fact independent of any
  weight. (Live champion `wonder_potential` values have since drifted off
  0.0 through evolution at all three player counts; that drift is
  unexplained noise on a low-gradient coordinate, not a fix.)
* **`copy_tactic` is a live, unassigned pathology and `tactic_gain` is a
  dead coordinate** — no evaluator term in either engine distinguishes
  "copied a tactic into an unused army" from a real gain. Re-measure both
  only after the unit-pricing defect above is fixed; they're confounded
  with it.
* **`resign` has no evaluator term** — `p.resigned` is read only for
  filtering in `engine/bots/weighted.py`, never scored; no `Resigned`
  `WeightKey` exists in Rust. Needs a policy decision (see "needs a
  decision," above) plus an n≥200 A/B.
* **`row_urgency` double-counts the row the same way `hand_potential` used
  to double-count the hand** — fixed for the hand
  ([`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md) §13.10), not for the row,
  in either engine (`rust/src/bots/weighted/row.rs:260-313`).
* **Aristotle is deliberately flat-zero pending a measured trigger rate**
  (`tests/test_board_yields.py`'s `STILL_FLAT` set — Hammurabi, Columbus and
  Barbarossa are there for rule reasons, not omission). Needs
  `tools/take_census.py` run for real, never has been.
* **Zero card takes during the Age IV phase is a vector/search artifact,
  not an engine hole.** The row is non-empty on 100% of Age IV decisions
  (`tools/age_iv_row.py`), and `DEFAULT_WEIGHTS` takes 2.00/seat-game —
  above the human 1.59 — so the exact-zero on live champions is about the
  trained vector and the search, not about coverage. Re-run
  `tools/age_iv_row.py` against current `champion_{2,3,4}p.json`, not
  `DEFAULT_WEIGHTS`, before concluding anything.
* **`ca_left` is a 1-ply pass-asymmetry artefact** — `end_turn` always has
  the highest `ca_left` of any candidate (165/165 measured), and the
  hillclimb `NONNEG` sign-guard can't correct it. Needs an n≥200 A/B.
* **A coordinate sweep is overdue in both engines**: `tech_levels`,
  `num_techs`, `special_techs`, `workers`, `prod_workers`, `urban_workers`
  and the `best_*` family are all read by `features()` but never charged
  for at take time, so they drift freely under training. The live 2p
  champion (checked today, 2026-08-05) has `tech_levels`/`culture_rate`/
  `best_library` drifted further than the 2026-07-31 snapshot that once
  called this "settled" — it isn't; a fresh inspection is needed before the
  sweep starts.
* **`build_fresh_credit` ships at 0.0 in both engines** — turning it on
  measured a 5.5pp loss on two independent 400-game A/Bs (44.1%/44.5% vs a
  50% null); it's a step, not a slope (0.5 and 1.0 measure the same loss),
  so tuning it down gently isn't available. Two unmeasured candidate causes:
  (a) the price sums over the whole civil hand, so every buildable tech
  double-credits one free worker; (b) nothing checks the build plan is
  still legal when it would be played (`workers_free` is 0 at 68% of 2p
  decisions). A third candidate: the uncharged build action may be worth
  more than `ca_left`/`ma_left` (≈0.05) says — same hypothesis as the
  civil-actions item below. Even with the credit off, two named sub-gaps
  stay unpriced in both engines: the plan isn't priced when the free-worker
  pool is empty, and only ONE fresh worker is ever priced.
* **`happy_margin` is priced linearly through its `min(3, margin)` clamp**
  in both engines — `feature_marginal`/`rivals::feature_marginal` already
  special-cases `Strength` this way, nobody has added the same for
  `HappyMargin`.
* **Nothing prices *which* build a `freeCivilAction` orders** (Rich Land vs
  Urban Growth price identically). The honest price is the best legal free
  build's own delta, which `tech_upgrade` can already compute — wiring it
  in is a performance question (`card_potential` runs per-row-card per-leaf)
  as much as a modelling one.
* **Engineering Genius / Frugality are under-played for reasons that
  aren't their price** (need a wonder-in-progress / population-increase
  context the bot rarely reaches). Blocked on the wonder-board-credit gap
  below; re-measure only after that lands.
* **The `civil_actions` weight (2.0 in `DEFAULT_WEIGHTS`, both engines) is
  the live suspect behind three separate measured gaps**, recorded together
  so nobody re-derives it a third time: `free_action_credit` ships at 0.0
  because turning it up is monotonically wrong (1.0→32.8%, 0.5→41.3%,
  0.0→47.7% vs a 50% null); the 2p civil-take budget is 19.2/seat-game
  against a human 34.3; and red cards get only 4.4% of takes against a
  human 11.2% share. All three read the same way: a civil action may be
  worth more than `w["civil_actions"]` says. Unowned in both engines.
* **A winning colony-auction `bid_pass` no longer prices the colony.**
  Since the colony sacrifice became its own `colonize` pending decision,
  `deferred_credit`'s `auction` branch stops matching (top of stack is
  `colonize`, not `auction`). Rust's own doc comment
  (`rust/src/bots/weighted/rivals.rs:380-385`) confirms this was left
  unhandled on purpose in both engines. Fix identified and small: a
  `colonize` sibling branch at share 1.0 (winner already decided).
* **`blue_free`/`corruption_loss` are unpriced on every technology plan**
  in both engines — neither is a `Stats` field, so `_delta_triples`/
  `board_yields` can't see either; upgrading Bronze→Iron moves both and
  prices neither. Ratchet-tested in Python
  (`tests/test_build_fresh.py:TestThePriceIsWhatFeaturesActuallyMove.UNPRICED`);
  no equivalent Rust ratchet exists yet.
* **Wonders never got the `*_board_credit`-defaults-to-1.0 treatment every
  other civil class got.** `wonder_completion_culture`
  (the four Age III wonders' one-shot culture bomb) is correctly priced by
  `_on_build_culture` in both engines, but it's gated behind
  `card_board_credit` (default 0.0) via `wonder_potential` (default 0.0) —
  unlike `tech_board_credit`/`gov_board_credit`/`action_board_credit`, no
  `wonder_board_credit` exists in either engine. Completion rates track it
  exactly (2p/3p/4p: 1.53/0.24/0.16 per seat-game). Not a free change —
  see the wonder-abandonment strategy question above; wants its own lane.
* **The rate horizon is applied to rate features and `feature_marginal`,
  never to the static `_sum_yields`/`sum_yields` table** — confirmed by
  identical function signatures in both engines (neither takes a state).
  Classes with no board handler still diverge from `evaluate` twice
  instead of once.
* **`rival_culture_rate`/`rival_science_rate` are deliberately excluded
  from the rate-horizon treatment** in both engines
  (`tests/test_coverage_tools.py:TestInertFeatures`,
  `rust/src/bots/weighted/horizon.rs:76`) because including them would make
  two coordinates the inertness test declares dead start varying. The
  wider horizon argument — an investment's cost side (`wonder_remaining`,
  science/resource costs) is flat while only wonders get a
  horizon-discounted payback term — is unowned in both engines.

### Evaluator information gaps

* **`PACT_OFFER_CREDIT = 0.5`** (`engine/bots/weighted.py:156`,
  `rust/src/bots/weighted/rivals.rs:378`) is a fitted prior with no measured
  value; can't cheaply become a weight because `features()`/`_pending_terms`
  have no weight vector on the hot path in either engine.
* **`_TAKE_PRIOR = {2: 0.30, 3: 0.35, 4: 0.40}`** (`weighted.py:1149`) — the
  3p/4p entries rest on one 10-game run of `tools/deal_rate.py`. Needs a
  bigger sample.
* **`rival_take_share` ships at its 0.5 prior, never climbed** — can't move
  until `row_bargain_forgone` goes non-zero, which it does on no live arm.
* **`FREE_POP_UTIL = 0.17`** (`board_yields.py:725`, ported identically to
  `rust/src/bots/board_yields.rs:587`) is 2p-only calibrated (~317
  player-turns) and currently unreachable in the live league (its handler
  gates on `card_board_credit`/`card_board_wonder`, both 0.0).
* **No "is there a job for this worker" board query exists** in either
  engine — the expensive urban-limit + per-technology build-cost query
  `card_potential` would need, deliberately left unbuilt.
* **`tools/deal_rate.py`'s "hungry" policy doesn't actually take more cards
  than the default** (both 1.88/round at 2p) — the §1.4 robustness claim
  rests on a weaker contrast than intended.
* **`event_scoring_margin` ships at weight 0.0 by design in both engines**
  (`rust/src/bots/weighted/events.rs:159`); its effect under `plan:width=2`
  (the search the league actually trains), 3p transfer, and whether it
  should feed the neural encoder's `seeded_n`/`seeded_lv` are all
  unmeasured.
* **GAP 6, military hand identity, is a loaded gun in both engines.** The
  moment either evaluator prices military-card identity,
  `plan::determinize`/`plan.determinize` becomes a live leak: both are
  faithful ports of each other and neither re-deals rival `hand_military`
  back into the deck. Any pricing change must ship together with that fix,
  in both languages at once.
* **The current-events leak (Joan of Arc) is fixed for `PlanBot`/
  `NeuralBot`/`NeuralPlanBot`, still open for `WeightedBot`/`QuiescentBot`.**
  `determinize` in both `engine/bots/plan.py:138-200` and
  `rust/src/bots/plan.rs:94` now shuffles `current_events` too, explicitly
  preserving a genuinely-peeked top card so Joan of Arc's ability stays
  worth something (`determinize_keeps_a_genuinely_peeked_top_event_on_top`
  in Rust). But `WeightedBot`/`QuiescentBot` never call `determinize` at
  all (`engine/bots/plan.py:45`), so any 1-ply trial that reads or resolves
  the top current event for those bot classes still sees the true card
  unconditionally — Joan of Arc's peek buys them nothing they didn't
  already "know."
* **`rival_hand_potential` has opposite fitted signs at 3p (−0.020) and 4p
  (+1.329)**, never head-to-head ablated, in either engine.
* **None of the shipped GAP1/2/3 terms** (`take_cost_paid`, `row_pressure`,
  `rival_free_ca`/`rival_hand_civil`/`rival_wonders`/`rival_hand_potential`)
  **has ever been win-rate ablation-tested**, only checked for reading
  correctly, in either engine. The one exception (de-leaking the row terms)
  measured strength-neutral, which says nothing about the terms' own worth.
* **The row-leak cursor is forward-only, not an identity** — it can't tell
  a genuinely new card from one reusing a name a *rival* took (as opposed
  to one swept), because that needs provenance on row slots. Pinned to
  fail-on-fix by `tests/test_row_features.py:717`; same shape mirrored
  into `rust/src/bots/weighted/row.rs`.
* **Pact pricing at 3p/4p via `deferred_credit` has never been quality-
  tested** — it exists (unlike at 2p, where pacts don't), but whether it's
  any good needs a 3p/4p experiment
  ([`docs/EVENT_SEEDING.md`](EVENT_SEEDING.md) §6.3).
* **`culture_rate_extra`/`science_rate_extra` are dead but entrenched.**
  Read by `effects.compute` (`engine/effects.py:462-463`,
  `rust/src/effects.rs:1242-1243`), written by nothing in either engine —
  event-granted per-turn culture/science channels that exist and are
  always zero. Used to also be pinned into the Python-parity fixture
  format (`rust/src/fixtures.rs`'s `req_num`, ~65MB of committed
  fixtures) as a reason not to touch the struct fields; that corpus and
  its reader were retired with the Python engine, so that constraint is
  gone. Either implement the event effect that should write them, or
  delete the two dead fields — neither has been started.
* **`one_time_discount` is in-flight, split across the two engines.** The
  Python bug ("one-time" was a lie — never cleared, so it silently applies
  to every future build/develop/population-increase) has been fixed:
  `engine/actions.py:927,1020` and `engine/economy.py:298` now call
  `effects.consume_one_time_discount`. Rust deliberately has NOT ported
  this yet — `rust/src/state.rs:520-541`'s doc comment says so explicitly,
  to keep differential fixture testing meaningful while the two engines
  briefly diverge. Port the consume calls into `rust/src/apply.rs`/
  `economy.rs` now that the Python side has landed.

### Training, league and objective (Python-only — Rust has no self-play
### runner, league, or training loop; see "Rust port status" below)

* **War/aggression rate under the new differential objective still needs a
  fresh post-relaunch measurement.** `tools/aggression_census.py` exists
  and works; the 6.6-7.9× human-rate figure is pre-relaunch. An uncommitted
  `docs/AGGRESSION_STATUS.md` re-measures 2p on the current champion but
  stops short of the human-rate comparison.
* **Nothing shows the new lead-over-best objective trains a *better* bot**
  — only that it ranks archived decisions in closer agreement with win
  rate. No post-relaunch league result confirms it yet
  ([`docs/LEAGUE_OBJECTIVE.md`](LEAGUE_OBJECTIVE.md) §9).
* **The lead's per-game dispersion is unmeasured** and expected to be
  noisier than plain own-culture was; if post-relaunch accept rates drop,
  this is the first thing to check (`--block` is the dial).
* **`LEAD_SCALE` (2p 145 / 3p 115 / 4p 135,
  `experiments/hillclimb_pool.py:1036`) is correctly derived but the 3p/4p
  corpus slices are thin (133/186 games) and the 2.5× multiplier has no
  independent justification.**
* **The objective's accept/reject throughput did not improve** — the
  reject-a-better-candidate rate is unchanged (16-20%) and 4p is slightly
  worse ([`docs/LEAGUE_OBJECTIVE.md`](LEAGUE_OBJECTIVE.md) §6b, a null).
* **3p/4p lead-over-best is a proxy (margin-over-mean), never validated
  against real logs** — the archives never recorded the best opponent's
  culture. The 2p column is exact.
* **`LEAGUE_POOL.md`'s saturation thresholds (0.70/0.95/0.15) are
  eyeballed, not derived** — direction is defensible, the exact knee isn't.
  Escape hatch: `--saturation 0,1,1`.
* **Post-training exploitability of the `hum:*` archetypes was never
  measured** — only pre-training. `experiments/human_exploit.py` exists as
  the probe but isn't wired into training. The take ceiling (~28 vs a
  human 33-40) survived four pre-training tuning attempts; take-backs (an
  8%-of-takes BGO affordance the bot lacks) is the leading unproven
  suspect.
* **Behaviour cloning's named follow-up — fit a value regression to human-
  corpus game *outcome* rather than move choice — has never been run.**
  Move-choice fitting provably can't learn a weight on a feature that never
  varies within a turn's candidates (culture stock, notably). Warning
  sign: the self-play analog of this experiment already ran and got
  *worse* as prediction accuracy improved
  ([`docs/BOT_ARCHITECTURE.md`](BOT_ARCHITECTURE.md) §3b).
* **`FOURP_GAP.md`'s best-supported mechanism (4p's accept gate is 2.8×
  less sensitive per generation) is unconfirmed** — one isolated test
  refuted a related hypothesis (reverting the negative `culture_early`
  coefficient did nothing, z=−1.5); nobody has measured the false-accept
  rate directly.
* **The culture matchup (`var:culture`) was never re-measured after the
  fixes** — what closed was the investigation's premise, not a measured
  win-rate recovery against that specific archetype.
* **Three unlanded `CULTURE_GAP.md` proposals**: decouple phase-multiplier
  mutation step size from base-weight magnitude; add a restoring force
  (log-prior toward defaults) to the geometric random walk; run a
  `--ladder 5,20,35.574,60` sweep on `culture_rate` — flagged as "the
  single most useful next experiment on this axis," queued, never run.
* **`deferred_credit()` has no `defense` branch** in either engine (only
  `pact_offer`/`auction`) — real, but an oracle that always attacks the
  culture leader gained nothing at n=48. Don't sell closing it as a fix;
  validate at n≥200 first.
* **The per-weight ablation ledger is currently disabled in the live
  trainer** — `experiments/watchdog.sh:194,231` passes `--ablate-every 0`.
  The `auction_committed` conflict (n=72 load-bearing vs n=24 harmful, both
  below n=200) can't resolve while it's off.
* **`guard_weights()`'s sign-clamp on science/culture has never been
  re-examined** despite non-monotonic evidence
  (`docs/HAZARDS.md:113-117`: helps at some 3p/4p configs, collapses
  others) — the clamp region itself, not just the constants, needs another
  look.
* **The trainer's accept-sample sizes are still small, and the doc's own
  numbers describing them are stale.** `--accept-z 1.2816` (one-sided 90%)
  is unchanged and still the root cause of this project's "confident
  result that later reverses" pattern. Current live blocks: 2p n=48
  (matches old docs), 3p n=96 (bumped from 12-game blocks 2026-08-02, not
  144 as both this doc and `docs/HAZARDS.md` §1 still say), 4p n=96 (not
  12). The full `--check-games` check defaults to 48 in argparse but live
  launches override it to 24. Fix the numbers in both docs together; "any
  result below n=200 is provisional" still stands, and is if anything
  stronger now.
* **`experiments/PROGRESS.md` is mechanically and methodologically stale**
  — wrong weight count, cites a horizon formula retired 2026-08-04, and
  draws un-ablated strategy conclusions from raw weight drift (the exact
  inference [`docs/OPENING_AUDIT.md`](OPENING_AUDIT.md) demolished).
* **`experiments/baselines.jsonl` rows carry no timestamp/generation/seed**
  — `experiments/evaluate.py` should stamp them; until it does, don't cite
  existing rows as dated evidence.
* **The evaluator's weight/feature count is wrong everywhere it's quoted.**
  `engine/bots/weighted.py`'s own docstring says 105; the actual count
  (`len(DEFAULT_WEIGHTS)`) is 130. Five docs still say 82/78/57
  (`docs/BOT_ARCHITECTURE.md`, `DEEPER_SEARCH.md`, `CULTURE_GAP.md`,
  `LEAGUE_TRAINING.md`, `TRAINING_RUN.md`). Fix the docstring and either
  grep-replace the docs or have them compute the count instead of
  hardcoding it.
* **The `has-unit` branch is parked, ready, and idle.** A 9-line fix
  (`has_unit` feature for the colony-auction rules cliff) sits unmerged in
  worktree `/Users/pt/tta-ai-hasunit`, branch `has-unit-ab`, tip `2713037`,
  forked ~10 days ago. `tools/guard_ab.py` exists specifically to run its
  3p/4p no-harm A/B against a post-horizon champion cheaply — just needs
  running, then merge or drop.
* **`docs/HEURISTICS.md`'s "zero-clamped weight" caveat is stale again.**
  Written 2026-07-30 citing 13 clamped weights and `end_turn_bias = -14.44`;
  the live champion today carries 33 clamped weights and
  `end_turn_bias = -39.65`. Given continuous unattended retraining, any
  hardcoded-numbers caveat goes stale within days — point the doc at a
  live-computed diff instead, or drop the numbers and keep only the
  confirmed/provisional framing.
* **`engine/PROGRESS.md:118,127` still states the pre-fix assumption**
  about action-card gain-ordering as an open question; it was resolved by
  a landed fix and is pinned by `tests/test_engine.py:496`. Delete the two
  stale lines.
* **The BGO scraper's 40-name expansion-exclusion whitelist has never been
  positively validated** against a known expansion-enabled game — none was
  found in-sample to test against. Internally consistent, not confirmed.
* **`docs/HUMAN_BASELINE.md`'s two proposed next steps are unimplemented**:
  hand-reconstruct one human position through `effects.end_of_game_bonus`
  to verify the scoring gap, and a scripted wonder-first A/B.
* **No real strength measurement exists — the bot has never played a
  human or the app AI.** `harness/` (the app-vs-bot measurement tool) is
  built and unit-tested but has never produced a reported win rate
  (`docs/APP_HARNESS.md:255`: "do not report a win rate from this
  harness"). Cost is ~11-18h for ten usable games. Standing side-benefit:
  if the league settles the `ca`/`hc`/`w` weights near zero, drop them
  from `mirror.RIVAL_ASK_KEYS` for a free 4-10%/game speedup. Also folds
  in: the BookBot-vs-champion benchmark was never re-run post the
  military-card-count fix, and `experiments/roster_behaviour.py` exists
  but was never run to fill `docs/BOT_ROSTER.md`'s blank 3p/4p
  reverse-direction cells.
* **`docs/DESKTOP_QUIET.md`'s two items are still unverified**: the
  arm-watchdog PID reap has never been observed actually reaping, and the
  12-worker generation path hasn't been window-checked under real load.
* **`docs/PYPY.md` §11.10's 3p/4p verdict needs re-measuring under
  `plan:width=2`.** All three arms have run `plan:width=2` since `1fbf128`
  (§11.4 measures it at 1.12-1.24× in PyPy's favour there); the §11.10
  verdict still quotes `quiescent:levels=1` ratios (0.82×/0.86×). The doc
  already flags this itself; the re-measurement hasn't happened.

### Search, neural and war pricing

* **The neural line has plateaued at rough parity with the linear champion
  since iteration 7** — the self-play accept gate's CI lower bound never
  clears 0.5 (`docs/NEURAL_LOOP_NULL.md` §5.1). Entirely Python; blocked on
  Rust getting a self-play loop before it matters there.
* **`NeuralPlanBot` still duplicates `_beam`/war-pricing logic** rather
  than sharing `plan.py`'s, and never reads `USE_JOURNAL` — deliberate,
  since nobody currently trains it. (Rust's `neural/plan.rs` shares more
  base-module code than Python does here, but that's moot: Rust has no
  training loop to run it in.)
* **Both engines still price a last-round war declaration as if it will
  resolve, when it never does before the game ends.** Identical guard, same
  missing rounds-remaining check, in `engine/bots/plan.py:555-585` and
  `rust/src/bots/plan.rs:393-399`. Deliberately unguarded so
  `docs/PLAN_WAR_LOOKAHEAD.md`'s measurements isolate "PlanBot prices wars"
  cleanly; flagged as the obvious next experiment, untried in either
  language.
* **`ctx` (rival aggregates) is never recomputed after a war resolves
  inside the lookahead**, identically in both engines — a known,
  deliberate imprecision left in place to avoid confounding the
  war-pricing measurement.
* **`settle_war_spoils` always takes the remainder as science — a
  permanent one-sided bias, byte-identical in both engines.**
  `engine/interact.py:420-451` and `rust/src/interact.rs:680-687` are
  functionally the same unconditional science branch; the Rust port
  carried the bias over rather than fixing it. Every bot in either
  language that prices a declared War-over-Technology sees only the floor
  of the card, never the ceiling. Fix (price the best affordable option
  from `war_tech_options` instead) is unimplemented in both.
* **`docs/TRANSFER_TEST.md` option (a), "train under `plan:width=1`," is
  still untried.** The war-lookahead fix only removed the proxy's sign
  inversion, not its magnitude miscalibration (the proxy still says one
  candidate is +36.3 better where real search finds no significant
  difference).
* **`docs/PLAN_WAR_LOOKAHEAD.md`'s entire measurement is 2p,
  `plan:width=8`, single-opponent (`book`).** 3p/4p, `width=1` (no beam to
  prune the war line out of), and a diverse opponent pool are all
  untested.

### Engine and rules

* **Plunder's food/resource split is hardcoded resources-first, not the
  attacker's choice**, identically in `engine/events.py:655-659` and
  `rust/src/events.rs:210-220` (FAQ p.7 says it should be the attacker's).
  Totals and cap are correct, only the mix isn't a real decision. Low
  impact.
* **Annex/Infiltrate are legal against a target that doesn't qualify** (no
  colony for Annex, no leader/unfinished wonder for Infiltrate) in both
  `engine/actions.py:326-340` and `rust/src/legal.rs:238-257` — the move
  resolves and does nothing. Deliberately left as an engine-vs-data
  inconsistency rather than a rules violation.
* **`Impact of Happiness`/`Impact of Strength` remain untestable against
  the BGO human corpus** — `engine/journal.py` never prints happy faces and
  the replayer models no tactics/armies. Python-only measurement
  limitation; needs richer journal output or accepting the gap.
  `Impact of Population`'s 73/88 residual is concentrated on
  discontent>0 rows; the "don't subtract discontent" alternative was tried
  and fits worse.
* **`state.scoring_events` is a genuinely dead field in both engines** —
  declared, never written, contributes a permanently-0 neural feature
  (`engine/bots/neural_encode.py:326`, `rust/src/bots/neural/encode.rs:466`).
  Safe to delete from both.

### The coordinate registry (`tests/test_coordinate_registry.py`'s
### `KNOWN_DEAD` allow-list, `docs/COORDINATE_REGISTRY.md`)

* **`gov_action_cost` stays dead** — the pricing *question* is answered
  (`ca_left` is the coordinate the live path correctly charges), but the
  coordinate itself is still emitted by the legacy static
  `card_board_credit` fallback in both engines
  (`engine/bots/board_yields.py:465`, `rust/src/bots/board_yields.rs:158`).
  Retiring it is the same task as the next item.
* **Retire the static action table.** `free_civil_action`,
  `resource_discount`, `restricted_resources` are dead on the live board
  path but `_card_yields`'s static fallback still emits them for any
  `action_board_credit = 0.0` caller — two of them visibly random-walk on
  live champions as a result. Python-only structural cleanup (Rust's
  board-aware path has no such fallback shape).
* **`defense_bonus` has no board mirror by construction** (a Military
  Bonus card's defence value only means something while still in hand) —
  it's also the only coordinate the three bonus cards carry, which is why
  `class:bonus` is entirely invisible to `row_pressure`. One defect, two
  symptoms, present identically in both engines.
* **Four whole card classes — `tactic` (15/15), `pact` (10/10),
  `aggression` (10/11), `war` (3/3) — price at exactly 0.0 in both engines**
  because pricing has no mapping from a tactic's strength table, an
  aggression's one-shot steal, or a pact-in-hand onto any board feature.
  Costs nothing today only because `hand_mil_potential` is 0.0 on every
  live champion — the moment the league prices the military hand, all four
  classes go blind simultaneously. Single largest item in the registry.
* **`used_leader_ability`** (`engine/state.py:49`, `rust/src/state.rs:387`)
  is a generic once-per-game/turn flag nothing sets or reads in either
  engine — every leader that needs one now carries its own dedicated flag.
  Probably safe to delete from both.
* **`wonder_overrun` is doubly dead** — weight 0.0 on every committed
  vector AND the feature itself computes exactly 0.0 on every corpus
  state, in both engines, so neither half can wake the other. Likely a bug
  in the feature computation (something that should fire near wonder
  completion never does), not just an unclimbed weight — needs tracing
  against a constructed near-overrun state.
* **The registry's own corpus is fragile.** It's pinned to a 6-game
  deterministic self-play corpus (~2000 states); any pricing change
  re-rolls all of it, and entries flip on unrelated changes (`uprising`
  flapped in and out of `KNOWN_DEAD` three times by 2026-08-04). Reports
  this week's policy, not the code. Two fixes proposed, neither started:
  widen `CORPUS_SEEDS` until rare shapes appear reliably, or reach them by
  construction the way `_probe_wonder_and_tactic` already does for the
  conduction probe (cheaper, and the pattern has already been hand-applied
  twice).
* **Two gaps the registry explicitly declares uncovered**, so nobody
  assumes otherwise: no check that a serialized neural checkpoint's
  weights still match the current `ENCODING_DIM`/feature layout (a stale
  checkpoint silently loads garbage into new slots); and
  `experiments/summarize.py`'s `GROUPS` dict names coordinates by string
  literal with nothing asserting the names still exist, so a renamed or
  retired coordinate silently stops being rescaled in reports.

### War-rate census

* **The live-league wiring this doc once said was "declined" actually
  landed 41 minutes later and has been running since 2026-07-31** —
  `engine/census.py` (opt-in `TTA_WAR_CENSUS` env var, off by default),
  wired into `experiments/run_league.sh:33`. `experiments/logs/census/`
  currently holds 3,567 JSONL files (227MB), far past the original "few
  hundred games" scope. Two things remain genuinely open: (1) the 3p arm
  still records nothing — `QuiescentBot`'s journalled path
  (`TTA_JOURNAL=1`) bypasses the recorder entirely, needs instrumenting;
  (2) nobody has re-run `tools/war_report.py` over the accumulated data to
  answer the real question — among decisions where war/aggression wins,
  how many would flip if every suppressed row card were priced at its true
  value? The published tables in `docs/WAR_RATE_CENSUS.md` §3-5 still only
  show the original n=129 partial sample from before live collection
  started.

### Rust port status

* **`#![allow(dead_code)]` at `rust/src/lib.rs:14` must come off when the
  port completes.**
* **Rust owns no self-play runner, league, or training loop yet** — no
  `[[bin]]` in `rust/Cargo.toml`. All training/league infrastructure above
  is Python-only until this exists.
