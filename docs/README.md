# docs/ — what is here and which document answers which question

This index exists so the tree does not grow back to sixty files.  **Before
writing a new document, check whether the answer belongs in one of these.**
Investigation write-ups whose question has been answered and whose fix has
landed should be folded into the relevant topic doc and deleted, not left
lying around — see "Housekeeping" at the bottom.

The three documents at the top are the ones to read first.

---

## Start here

| doc | answers |
|---|---|
| **[`OPEN_ITEMS.md`](OPEN_ITEMS.md)** | *What is still open?*  The single register of unfinished work, deferred decisions and unanswered questions, grouped by area.  Everything that used to live in `OPEN_AFTER_THE_AUDIT.md`, `ARCHAEOLOGY.md` and `HEURISTICS_TODO.md` is here. |
| **[`HAZARDS.md`](HAZARDS.md)** | *What will bite me?*  Standing traps, every one of which has already cost a real bug — training-loop traps 1-8 (cited by number from code), radioactive vectors, "do not fix these", measurement traps, engine/determinism rules, the Windows node, git and multi-agent working. |
| **[`SYSTEM_COVERAGE.md`](SYSTEM_COVERAGE.md)** | *How good is the bot right now, and what does it never do?*  The most recent whole-system census against the 1,011-game human corpus.  It is the current source of truth and explicitly supersedes older behavioural numbers elsewhere. |

## The game itself

| doc | answers |
|---|---|
| [`RULES_SPEC.md`](RULES_SPEC.md) | The rules of 2015-edition base Through the Ages, 13 sections, every claim cited to the Handbook / Code of Laws / FAQ v1.5.  **There is no rulebook PDF in this repo; this is the only copy.**  Its appendix carries the card-data provenance and the rulings that produced `data/cards_*.json`. |
| [`SOURCES.md`](SOURCES.md) | Where the card data came from, which sources are trustworthy, and the two multi-source conflicts that were found and fixed.  Read before editing anything under `data/`. |
| [`EXPERT_STRATEGY.md`](EXPERT_STRATEGY.md) | Published human expert consensus on how to play, gathered deliberately independent of our bots.  Openings, leader/wonder tiers, military doctrine, government doctrine, top mistakes, and a table of genuine expert disagreements left deliberately unresolved. |
| [`HEURISTICS.md`](HEURISTICS.md) | Human-facing playbook derived from our own self-play plus the book-bot benchmark.  Self-grades its own evidence; read the grades.  Carries a staleness caveat at the top. |

## The bot: architecture, search, evaluation

| doc | answers |
|---|---|
| [`BOT_ARCHITECTURE.md`](BOT_ARCHITECTURE.md) | *Why is the bot shaped like this?*  Engine cost census, why MCTS is ruled out, why the trained linear bot is weak, and the PlanBot beam that fixes it.  The long-lived architecture doc. |
| [`DEEPER_SEARCH.md`](DEEPER_SEARCH.md) | Quiescence: resolving the pending-decision stack before scoring.  Budgets, costs, why `LEVELS=1` and not 2, why QuiescentBot cannot be the training challenger, and (§8, merged from the former `DRAIN_AB.md`) the measurement behind `QUIET_PENDING = True`. |
| [`INFORMATION_AUDIT.md`](INFORMATION_AUDIT.md) | *What can the evaluator actually see?*  Field-by-field measurement of the six information gaps, and the row-leak fix. |
| [`EVENT_SEEDING.md`](EVENT_SEEDING.md) | Pricing the event/aggression/pact lane, and the `event_scoring_margin` feature. |
| [`WASTED_ACTIONS.md`](WASTED_ACTIONS.md) | Why the bot wastes civil actions, why the obvious fix makes it worse, and the `hand_potential` term that actually worked. |
| [`PLAN_WAR_LOOKAHEAD.md`](PLAN_WAR_LOOKAHEAD.md) | Giving the beam a war lookahead, and what it did and did not fix. |
| [`NEURAL_EVAL.md`](NEURAL_EVAL.md) | The value net and `NeuralBot`: why Monte-Carlo regression was the wrong objective and pairwise ranking was better. |
| [`NEURAL_LOOP_NULL.md`](NEURAL_LOOP_NULL.md) | The v1 self-play loop: 74 iterations, 41 hours, zero promotions, and the precise diagnosis.  A durable negative result — read it before proposing another loop. |
| [`NEURAL_SEARCH_LOOP.md`](NEURAL_SEARCH_LOOP.md) | The v2 loop that replaced it, its pre-registered kill conditions, and where it has plateaued. |

## Card pricing and coverage

*One document now, `CARD_BLINDNESS.md`, absorbing what used to be seven
overlapping write-ups from one long investigation (see "Housekeeping" for the
merge). [`OPEN_ITEMS.md`](OPEN_ITEMS.md#2-card-pricing-and-coverage) §2 is the consolidated list of what is still unpriced; use
these for the reasoning.*

| doc | answers |
|---|---|
| [`CARD_BLINDNESS.md`](CARD_BLINDNESS.md) | The originating finding (`_card_yields` silently dropped culture/science), the fix, and a project-wide audit of confidence-interval methodology.  §11 (merged from the former `CARD_BLINDNESS_MILITARY.md`): territories, units and tactics — why units price *negative* and why no per-card table can fix it.  §12 (merged from the former `CARD_CENSUS.md`): the instrument — does the bot play each of the 236 cards, and can a card's value reach the policy at all?  Severed-pipe detection across all 23 card types.  §13 (merged from the former `CARD_PRICING_LEADERS.md`): board-aware pricing for leaders, actions and governments by diffing the rules engine rather than copying it.  §14 (merged from the former `UNIT_TECH_PRICING.md`): a unit technology priced by a board query (the engine's own upgrade cost against a `compute` strength diff, valued at d(`evaluate`)/d(strength)).  Unit takes 0.20 → 1.06 at 2p and 0.08 → 4.16 at 3p; A/B null at 2p and on the defaults at 3p, and a 14.6% **regression** on the archived 3p champion attributed to an unconstrained `strength` weight — read §14.5 before warm-starting anything from that archive.  §15 (merged from the former `YELLOW_TECH_PRICING.md`): the other half of that fix — `card_potential` read `w[k]` where `evaluate` reads `w[k] + (1−L)w[k_early] + L·w[k_late]`, and `tech_levels` was mapped to nothing at all on every technology card.  `feature_marginal` + `board_yields.tech_upgrade` price all fifteen technology types by one board query.  Labs 0.02 → 1.77 per seat-game at 2p against a human 1.62, mines 0.03 → 0.85 — **and the blue over-play is cured by the same change** (theatres 2.23 → 0.82).  A/B **+20.5pp at 2p and +8.3pp at 3p on `DEFAULT_WEIGHTS`**, and a −37.8pp **regression** on the live 2p champion that reverses to +13.0pp when one stale weight group is reset — read §15.4 before trusting `champion_2p.json`.  §16 (merged from the former `ACTION_CARD_PRICING.md`): **sixteen of the thirty-three yellow action cards priced at exactly 0.000**, because `free_civil_action`, `resource_discount` and `restricted_resources` are weights `features()` never emits, and because Reserves' "food OR resources" choice was multiplied by `card_board_credit`, 0.0 on every champion.  `weighted.action_value` prices all thirty-three through `feature_marginal` instead.  Takes 7.30 → 9.00 per seat-game at 2p against a human 12.98 and 5.83 → 7.53 at 3p against 10.25 — read §16.5 for the modelling error the first A/B caught.  §17 (merged from the former `PLAY_RATE_AUDIT.md`): the behavioural complement to all of the above — not *is the card priced* but **does the bot actually play it, and at what rate against a human?**  Per-card take/play rates for all 236 cards at 2p/3p/4p against the 1,011-game corpus, the ranked discrepancy table both ways, the never-played list, and the standing check (`tools/play_rate.py`, `tests/test_play_rate.py`) that makes a class priced-but-inert a test failure. |
| **[`GOVERNMENT_PRICING.md`](GOVERNMENT_PRICING.md)** | The fourth instance, and the last civil type left on the static table: a government prints **two** science costs (`peacefulCost` / `revolutionCost`) and keeps its civil actions, military actions and urban limit in top-level fields, so `_card_yields` — which reads `techCost`, `production` and `effects` — saw **none of it**.  Five of the seven takeable governments were unreachable: three priced at exactly 0.000 and two strictly negative.  `weighted.gov_value` prices the swap diff plus the `tech_levels` / `gov_level` delta at `feature_marginal` and charges the cheaper of the two routes RULES_SPEC 8.2/8.3 offers, the revolution branch gated on the engine's own `_can_revolt` — which also settles [`OPEN_ITEMS.md`](OPEN_ITEMS.md) §9.1 from the rules: the burn lands on `ca_left`.  Government takes 1.05 → 1.63 per seat-game at 2p against a human 1.37, and seats ending the game still on Despotism fall from 10 of 40 to 1 of 40.  Gated by `gov_board_credit`, 1.0. |
| **[`MODEL_CONSTANTS.md`](MODEL_CONSTANTS.md)** | The other side of the same coin: not *is the card priced* but **is this number a rule, a measurement or a guess?**  Three fitted constants in the evaluator replaced by quantities the state already knows — the deal rate (fitted at 0.29 takes/round, actually **1.88**, leaving the horizon **1.80 rounds long**), the lateness gauge (now the exact civil-supply fraction, bounded [0,1] by construction), and the flat rival take probability (now read off each rival's open board, with one prior left as a fittable weight).  Plus the standing check (`tests/test_model_constants.py`) that fails when any module-scope constant in `engine/` or `experiments/` is not classified as rule-derived / numerical guard / measured / fitted prior / training policy / enum-or-sentinel. |
| **[`COORDINATE_REGISTRY.md`](COORDINATE_REGISTRY.md)** | The generalisation of the three above, and the guard that makes them a closed bug class: **a coordinate that exists in one registry and is missing from another is silently dead, and the tree is green.**  Four registries — `features()`, `DEFAULT_WEIGHTS` plus every weight vector on disk, `card_potential`, and `neural_encode.encode()` — with the bijection asserted in **both** directions, because the missing direction is always where the bug hides.  `tests/test_coordinate_registry.py` (50 tests, ~8s, no game batches) plus a frozen `KNOWN_DEAD` ratchet that fails when something new joins **and** when a listed entry stops being dead, so the list can only shrink.  It found two new instances on its first run: `gov_action_cost` (a revolution's burnt civil actions priced through a coordinate `evaluate` never pays, beside the `civil_actions` that is the same quantity) and `state.current_events_age` (declared, never written, and five permanently frozen neural inputs). |
| [`UNCOVERED_TYPES.md`](UNCOVERED_TYPES.md) | Special technologies, production buildings and bonus cards; and the general rule that a half-priced card is biased, not neutral. |
| [`COVERAGE_AUDIT.md`](COVERAGE_AUDIT.md) | The non-card axis: colonies, resign, farm-vs-mine degeneracy, and the dead-coordinate census of every evaluator feature. |

## Rules conformance, scoring and combat

| doc | answers |
|---|---|
| [`SCORE_AUDIT.md`](SCORE_AUDIT.md) | Do all 23 card types score exactly?  Nine bugs found and fixed; the "a corpus validates only what it varies" finding.  §10 (merged from the former `SCORE_VALIDATION.md`): engine scoring replayed against 1,011 human games — the method, the corpus-wide agreement rates, and what the corpus cannot decide.  §11 (merged from the former `SCORE_BUGFIX.md`): the first four scoring fixes (Industry, Population, Hollywood/Internet, Chaplin) and their measurement. |
| [`COMBAT_AUDIT.md`](COMBAT_AUDIT.md) | Wars, aggressions and pacts checked against the printed rules.  Three bugs fixed, three gaps, and the most granular rules-to-code mapping in the repo.  §1 (merged from the former `MILITARY_SEAM.md`): the plumbing that stopped board-aware pricing reaching military cards at all.  §2 (merged from the former `MILITARY_DISCARD.md`): turning the end-of-turn excess-card discard from a hardcoded FIFO into a real decision.  §3 (merged from the former `WAR_OVER_TECHNOLOGY.md`): the victor's choice between science and blue technologies — full rules citations, the implementation, and the permanent lower-bias it leaves in search.  §4 (merged from the former `PACTS_DIAGNOSIS.md`): why the bot never offers pacts and almost never colonizes — a bot blind spot, not an engine bug. |
| [`AGGRESSION_RATE.md`](AGGRESSION_RATE.md) | *How often does the bot actually fight?*  Corrects the "aggressions are rare" reading as a 1-ply artefact, and fixes defences that were never won.  Appendix (merged from the former `AGGRESSION_FIX.md`): the refutation of "4p auctions never start because events are not seeded", and the payoff-lands-in-the-defender's-decision mechanism. |

## Training, league and strength

| doc | answers |
|---|---|
| [`TRAINING_RUN.md`](TRAINING_RUN.md) | The running operational log of the live arms.  **Read newest entry first; later entries supersede earlier ones inline.**  This is where "what is training right now" is recorded. |
| [`LEAGUE_TRAINING.md`](LEAGUE_TRAINING.md) | How the pool-based league trainer works: tiers, gate veto, ablation, restart safety, the weight guard.  The mechanism reference. |
| [`LEAGUE_OBJECTIVE.md`](LEAGUE_OBJECTIVE.md) | Why the accept metric is `blend` (own culture + a win-share tiebreak), and why gating on win rate would *not* have fixed the theft bug. |
| [`LEAGUE_POOL.md`](LEAGUE_POOL.md) | Opponent-pool saturation weighting: "an opponent the champion beats 98% of the time is not an opponent, it is a bill." |
| [`PROXY_GUARDRAIL.md`](PROXY_GUARDRAIL.md) | The continuous monitor that asks whether the thing the league trains on predicts the strength of the thing we would ship. |
| [`TRANSFER_TEST.md`](TRANSFER_TEST.md) | The decisive negative result: a quiescent-trained vector does not transfer to PlanBot, it *inverts*. |
| [`STRENGTH_CHECK.md`](STRENGTH_CHECK.md) | The first external yardstick — a hand-written book bot beating the trained champion.  Headline numbers stale; the method and the diagnosis are not. |
| [`BOT_ROSTER.md`](BOT_ROSTER.md) | The 47,520-game round-robin across 12 entrants.  Predates PlanBot and QuiescentBot; 4p rows quarantined. |
| [`TWOP_PROFILE.md`](TWOP_PROFILE.md) | What the (superseded) margin-gated 2p champion actually did — it won by suppression, not by scoring. |
| [`FOURP_GAP.md`](FOURP_GAP.md) | Why 4p training converged somewhere actively bad.  Lineage discarded; the matched-generation method survives. |
| [`OPENING_AUDIT.md`](OPENING_AUDIT.md) | "4p champions open with a wonder" — real behaviour, but one hitchhiking weight, worth nothing.  The canonical demonstration that a moved weight is not evidence. |
| [`CULTURE_GAP.md`](CULTURE_GAP.md) | The culture matchup, and where chasing it led: multiplicative mutation steps, a one-sided weight guard, and a weak gate bias.  **Self-corrects three times — read to §23 or you will take away the opposite of its conclusion.** |

## Humans: corpus, baselines, imitation

| doc | answers |
|---|---|
| [`HUMAN_BASELINE.md`](HUMAN_BASELINE.md) | What strong humans actually do, from 1,011 BGO games.  The human side is the reference; the bot side is stale (see its banner). |
| [`BGO_CORPUS.md`](BGO_CORPUS.md) | The scrape: method, yield, limits, and what the journals can and cannot tell you. |
| [`BEHAVIOUR_CLONE.md`](BEHAVIOUR_CLONE.md) | Fitting the weight vector to human move choices — and why behaviour cloning recovers *how* to play but can never recover *what for*. |
| [`HUMAN_BOTS.md`](HUMAN_BOTS.md) | The four corpus-fitted human archetypes now in the pool, and the negative result underneath them (human play is not discrete archetypes). |
| [`EXTERNAL_AIS.md`](EXTERNAL_AIS.md) | Every external opponent and data source that was investigated, and why almost all of them are dead ends.  Mostly a record of *negative* results — read before re-attempting any of them. |
| [`APP_HARNESS.md`](APP_HARNESS.md) | The operator's manual for playing the trained bot against the official app's Hard AI by hand.  The only externally calibrated anchor available. |

## Infrastructure and performance

| doc | answers |
|---|---|
| [`PYPY.md`](PYPY.md) | Should the trainer run on PyPy, and the undo/journal stack that replaced copy-per-candidate.  Long; the methodology is the durable part.  Carries a dated correction at the top. |
| [`DESKTOP_QUIET.md`](DESKTOP_QUIET.md) | Keeping training invisible on the owner's gaming PC, and keeping the GPU guard alive.  The operational manual for that box. |

## Elsewhere in the repo

[`engine/PROGRESS.md`](../engine/PROGRESS.md), [`experiments/PROGRESS.md`](../experiments/PROGRESS.md), [`data/PROGRESS.md`](../data/PROGRESS.md) and
[`advisor/PROGRESS.md`](../advisor/PROGRESS.md) are per-package build logs.  [`analysis/frozen/README.md`](../analysis/frozen/README.md)
records which frozen weight vectors are trustworthy and which are quarantined —
read it before quoting any frozen-champion number.

## Housekeeping

* **61 documents on 2026-07-30, 54 after the consolidation** (53 topic docs plus
  this index); 55 once [`CARD_BLINDNESS.md`](CARD_BLINDNESS.md) landed later the same day.  Nine were deleted and their live content migrated; one was
  renamed (`UNATTENDED.md` -> [`HAZARDS.md`](HAZARDS.md)) and one superseded in place
  (`OPEN_AFTER_THE_AUDIT.md` -> [`OPEN_ITEMS.md`](OPEN_ITEMS.md)); five stale-wrong documents got
  dated correction banners
  rather than deletion, because the wrong conclusions in them were the kind
  somebody would otherwise re-reach.
* **44 topic docs on 2026-07-31** (45 with this index), down from 56 (57 with
  the index) earlier the same day: the scoring, military/combat and
  card-pricing clusters below removed thirteen more files by merging their
  live content into their topic doc. Two more one-question
  write-ups whose fix had landed were folded into their topic doc and deleted:
  the former `AGGRESSION_FIX.md` into [`AGGRESSION_RATE.md`](AGGRESSION_RATE.md) (as an
  appendix, headings and all — its own §A/§B numbering was untouched so every
  existing `§A`/`§B` citation still resolves, just against the new filename),
  and the former `DRAIN_AB.md` into [`DEEPER_SEARCH.md`](DEEPER_SEARCH.md#8-the-drain-ab-why-quiet_pending-was-flipped-to-true-merged-from-the-former-drain_abmd-2026-07-31) §8 (renumbered
  1-4 -> 8.1-8.4 to avoid colliding with `DEEPER_SEARCH.md`'s own §1-§7; every
  citing line, in docs and in code (`engine/bots/pending.py`, `tools/gate.sh`),
  was updated in the same commit). Cross-references elsewhere that named
  either file by number (`AGGRESSION_FIX §B`, `docs/DRAIN_AB.md 1/2`) were
  repointed at the new file and section, not deleted.
* **The scoring cluster merged into `SCORE_AUDIT.md` on 2026-07-31.** The
  former `SCORE_VALIDATION.md` (which found the bugs, 2026-07-27) became §10,
  its own §1-§8 renumbered §10.1-§10.8 to avoid colliding with
  `SCORE_AUDIT.md`'s own §1-§9. The former `SCORE_BUGFIX.md` (which fixed
  them, 2026-07-27) became §11, its own §1-§6 renumbered §11.1-§11.6.
  Cross-references between the two merged docs (`SCORE_BUGFIX.md` cited
  `SCORE_VALIDATION.md`'s own §2 and §3 by number, calling it "the previous
  document") were repointed at the new §10.x/§11.x locations. Every citing
  line, in docs and in code (`engine/effects.py`,
  `engine/bots/board_yields.py`, `tools/gate.sh`, `tools/bgo_moves.py`,
  `tools/free_pop_rate.py`, `tools/objective_ab.py`, and the test suite), was
  updated in the same commit. [`CULTURE_GAP.md`](CULTURE_GAP.md) was evaluated for this
  cluster and deliberately left out — despite the similar subject (scoring /
  culture margin) it is a training-loop and weight-search narrative about the
  culture-rate objective, not a scoring-correctness document like the other
  three.
* **The military/combat cluster merged into `COMBAT_AUDIT.md` on 2026-07-31.**
  `COMBAT_AUDIT.md` itself has no numbered headings (`Method`, `Bugs found`,
  `BUG 1`/`2`/`3`, `Verdict`, ...), so the four incoming docs got fresh top-level
  numbers: the former `MILITARY_SEAM.md` is §1 (its own §1-§5 renumbered
  §1.1-§1.5), the former `MILITARY_DISCARD.md` is §2 (§1-§6 renumbered
  §2.1-§2.6), the former `WAR_OVER_TECHNOLOGY.md` is §3 (§1-§8 renumbered
  §3.1-§3.8), and the former `PACTS_DIAGNOSIS.md` is §4 (it had no numbered
  headings of its own, so nothing to renumber). Cross-references between the
  merged docs (`MILITARY_SEAM.md` and `WAR_OVER_TECHNOLOGY.md` each cite
  `MILITARY_DISCARD.md` by name) were repointed at §2. Every citing line, in
  docs (`AGGRESSION_RATE.md`, `HEURISTICS.md`, `BOT_ARCHITECTURE.md`,
  `DEEPER_SEARCH.md`, `CARD_BLINDNESS.md`, `UNCOVERED_TYPES.md`,
  `OPEN_ITEMS.md`, `SCORE_AUDIT.md`) and in code
  (`engine/bots/weighted.py`, `engine/bots/quiescent.py`, `engine/PROGRESS.md`,
  `tools/gate.sh`, and half a dozen census/probe tools), was updated in the
  same commit.
* **The card-pricing cluster merged into `CARD_BLINDNESS.md` on 2026-07-31.**
  Seven satellite docs, each cited from code by filename and section number
  (52 comments into `CARD_BLINDNESS.md` alone before this pass), became new
  top-level sections §11-§17, each satellite's own numbering renumbered under
  its new prefix to avoid colliding with `CARD_BLINDNESS.md`'s own §1-§10:
  the former `CARD_BLINDNESS_MILITARY.md` is §11, `CARD_CENSUS.md` is §12,
  `CARD_PRICING_LEADERS.md` is §13, `UNIT_TECH_PRICING.md` is §14,
  `YELLOW_TECH_PRICING.md` is §15, `ACTION_CARD_PRICING.md` is §16, and
  `PLAY_RATE_AUDIT.md` is §17. Every citing line, in docs and in code
  (`engine/bots/weighted.py`, `engine/bots/board_yields.py`,
  `engine/effects.py`, and a couple dozen tools/tests), was updated in the
  same commit — including several two-line and mid-sentence cross-references
  between the satellites themselves that a naive per-line regex would have
  missed (e.g. "this is the §5.1 finding of `docs/CARD_BLINDNESS.md`", where
  the filename follows rather than precedes the section number). `MODEL_CONSTANTS.md`
  and `COORDINATE_REGISTRY.md` were deliberately left out of this merge —
  they ask "is this a rule, a measurement or a guess" and "does a coordinate
  reach every registry it needs", not "is this card priced", and stand fine
  on their own.
* Open work goes in [`OPEN_ITEMS.md`](OPEN_ITEMS.md).  Traps go in [`HAZARDS.md`](HAZARDS.md).  Neither is a
  place for narrative.
* If you delete a document, `grep -rn '<NAME>.md'` across the whole repo first —
  code comments cite these files heavily (over fifty comments point into
  [`CARD_BLINDNESS.md`](CARD_BLINDNESS.md) alone), and a dangling citation is exactly the debris this
  index exists to prevent.
