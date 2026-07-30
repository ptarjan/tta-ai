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
| **`OPEN_ITEMS.md`** | *What is still open?*  The single register of unfinished work, deferred decisions and unanswered questions, grouped by area.  Everything that used to live in `OPEN_AFTER_THE_AUDIT.md`, `ARCHAEOLOGY.md` and `HEURISTICS_TODO.md` is here. |
| **`HAZARDS.md`** | *What will bite me?*  Standing traps, every one of which has already cost a real bug — training-loop traps 1-8 (cited by number from code), radioactive vectors, "do not fix these", measurement traps, engine/determinism rules, the Windows node, git and multi-agent working. |
| **`SYSTEM_COVERAGE.md`** | *How good is the bot right now, and what does it never do?*  The most recent whole-system census against the 1,011-game human corpus.  It is the current source of truth and explicitly supersedes older behavioural numbers elsewhere. |

## The game itself

| doc | answers |
|---|---|
| `RULES_SPEC.md` | The rules of 2015-edition base Through the Ages, 13 sections, every claim cited to the Handbook / Code of Laws / FAQ v1.5.  **There is no rulebook PDF in this repo; this is the only copy.**  Its appendix carries the card-data provenance and the rulings that produced `data/cards_*.json`. |
| `SOURCES.md` | Where the card data came from, which sources are trustworthy, and the two multi-source conflicts that were found and fixed.  Read before editing anything under `data/`. |
| `EXPERT_STRATEGY.md` | Published human expert consensus on how to play, gathered deliberately independent of our bots.  Openings, leader/wonder tiers, military doctrine, government doctrine, top mistakes, and a table of genuine expert disagreements left deliberately unresolved. |
| `HEURISTICS.md` | Human-facing playbook derived from our own self-play plus the book-bot benchmark.  Self-grades its own evidence; read the grades.  Carries a staleness caveat at the top. |

## The bot: architecture, search, evaluation

| doc | answers |
|---|---|
| `BOT_ARCHITECTURE.md` | *Why is the bot shaped like this?*  Engine cost census, why MCTS is ruled out, why the trained linear bot is weak, and the PlanBot beam that fixes it.  The long-lived architecture doc. |
| `DEEPER_SEARCH.md` | Quiescence: resolving the pending-decision stack before scoring.  Budgets, costs, why `LEVELS=1` and not 2, and why QuiescentBot cannot be the training challenger. |
| `INFORMATION_AUDIT.md` | *What can the evaluator actually see?*  Field-by-field measurement of the six information gaps, and the row-leak fix. |
| `EVENT_SEEDING.md` | Pricing the event/aggression/pact lane, and the `event_scoring_margin` feature. |
| `WASTED_ACTIONS.md` | Why the bot wastes civil actions, why the obvious fix makes it worse, and the `hand_potential` term that actually worked. |
| `DRAIN_AB.md` | The measurement behind `QUIET_PENDING = True`. |
| `PLAN_WAR_LOOKAHEAD.md` | Giving the beam a war lookahead, and what it did and did not fix. |
| `NEURAL_EVAL.md` | The value net and `NeuralBot`: why Monte-Carlo regression was the wrong objective and pairwise ranking was better. |
| `NEURAL_LOOP_NULL.md` | The v1 self-play loop: 74 iterations, 41 hours, zero promotions, and the precise diagnosis.  A durable negative result — read it before proposing another loop. |
| `NEURAL_SEARCH_LOOP.md` | The v2 loop that replaced it, its pre-registered kill conditions, and where it has plateaued. |

## Card pricing and coverage

*Six overlapping documents from one long investigation, plus
`PLAY_RATE_AUDIT.md`, which asks the behavioural question the other six do not.
`OPEN_ITEMS.md` §2 is the consolidated list of what is still unpriced; use
these for the reasoning.*

| doc | answers |
|---|---|
| `CARD_CENSUS.md` | The instrument: does the bot play each of the 236 cards, and can a card's value reach the policy at all?  Severed-pipe detection across all 23 card types. |
| `CARD_BLINDNESS.md` | The originating finding (`_card_yields` silently dropped culture/science), the fix, and a project-wide audit of confidence-interval methodology. |
| `CARD_PRICING_LEADERS.md` | Board-aware pricing for leaders, actions and governments by diffing the rules engine rather than copying it. |
| `CARD_BLINDNESS_MILITARY.md` | Territories, units and tactics — why units price *negative* and why no per-card table can fix it. |
| `UNCOVERED_TYPES.md` | Special technologies, production buildings and bonus cards; and the general rule that a half-priced card is biased, not neutral. |
| `COVERAGE_AUDIT.md` | The non-card axis: colonies, resign, farm-vs-mine degeneracy, and the dead-coordinate census of every evaluator feature. |
| `MILITARY_SEAM.md` | The plumbing that stopped board-aware pricing reaching military cards at all. |
| `PLAY_RATE_AUDIT.md` | The complement to all of the above: not *is the card priced* but **does the bot actually play it, and at what rate against a human?**  Per-card take/play rates for all 236 cards at 2p/3p/4p against the 1,011-game corpus, the ranked discrepancy table both ways, the never-played list, and the standing check (`tools/play_rate.py`, `tests/test_play_rate.py`) that makes a class priced-but-inert a test failure. |

## Rules conformance, scoring and combat

| doc | answers |
|---|---|
| `SCORE_AUDIT.md` | Do all 23 card types score exactly?  Nine bugs found and fixed; the "a corpus validates only what it varies" finding. |
| `SCORE_VALIDATION.md` | Engine scoring replayed against 1,011 human games — the method, the corpus-wide agreement rates, and what the corpus cannot decide. |
| `SCORE_BUGFIX.md` | The first four scoring fixes (Industry, Population, Hollywood/Internet, Chaplin) and their measurement. |
| `COMBAT_AUDIT.md` | Wars, aggressions and pacts checked against the printed rules.  Three bugs fixed, three gaps, and the most granular rules-to-code mapping in the repo. |
| `WAR_OVER_TECHNOLOGY.md` | The victor's choice between science and blue technologies — full rules citations, the implementation, and the permanent lower-bias it leaves in search. |
| `MILITARY_DISCARD.md` | Turning the end-of-turn excess-card discard from a hardcoded FIFO into a real decision. |
| `AGGRESSION_RATE.md` | *How often does the bot actually fight?*  Corrects the "aggressions are rare" reading as a 1-ply artefact, and fixes defences that were never won. |
| `AGGRESSION_FIX.md` | Superseded on its rates; kept for the refutation of "4p auctions never start because events are not seeded". |
| `PACTS_DIAGNOSIS.md` | Why the bot never offers pacts and almost never colonizes — a bot blind spot, not an engine bug. |

## Training, league and strength

| doc | answers |
|---|---|
| `TRAINING_RUN.md` | The running operational log of the live arms.  **Read newest entry first; later entries supersede earlier ones inline.**  This is where "what is training right now" is recorded. |
| `LEAGUE_TRAINING.md` | How the pool-based league trainer works: tiers, gate veto, ablation, restart safety, the weight guard.  The mechanism reference. |
| `LEAGUE_OBJECTIVE.md` | Why the accept metric is `blend` (own culture + a win-share tiebreak), and why gating on win rate would *not* have fixed the theft bug. |
| `LEAGUE_POOL.md` | Opponent-pool saturation weighting: "an opponent the champion beats 98% of the time is not an opponent, it is a bill." |
| `PROXY_GUARDRAIL.md` | The continuous monitor that asks whether the thing the league trains on predicts the strength of the thing we would ship. |
| `TRANSFER_TEST.md` | The decisive negative result: a quiescent-trained vector does not transfer to PlanBot, it *inverts*. |
| `STRENGTH_CHECK.md` | The first external yardstick — a hand-written book bot beating the trained champion.  Headline numbers stale; the method and the diagnosis are not. |
| `BOT_ROSTER.md` | The 47,520-game round-robin across 12 entrants.  Predates PlanBot and QuiescentBot; 4p rows quarantined. |
| `TWOP_PROFILE.md` | What the (superseded) margin-gated 2p champion actually did — it won by suppression, not by scoring. |
| `FOURP_GAP.md` | Why 4p training converged somewhere actively bad.  Lineage discarded; the matched-generation method survives. |
| `OPENING_AUDIT.md` | "4p champions open with a wonder" — real behaviour, but one hitchhiking weight, worth nothing.  The canonical demonstration that a moved weight is not evidence. |
| `CULTURE_GAP.md` | The culture matchup, and where chasing it led: multiplicative mutation steps, a one-sided weight guard, and a weak gate bias.  **Self-corrects three times — read to §23 or you will take away the opposite of its conclusion.** |

## Humans: corpus, baselines, imitation

| doc | answers |
|---|---|
| `HUMAN_BASELINE.md` | What strong humans actually do, from 1,011 BGO games.  The human side is the reference; the bot side is stale (see its banner). |
| `BGO_CORPUS.md` | The scrape: method, yield, limits, and what the journals can and cannot tell you. |
| `BEHAVIOUR_CLONE.md` | Fitting the weight vector to human move choices — and why behaviour cloning recovers *how* to play but can never recover *what for*. |
| `HUMAN_BOTS.md` | The four corpus-fitted human archetypes now in the pool, and the negative result underneath them (human play is not discrete archetypes). |
| `EXTERNAL_AIS.md` | Every external opponent and data source that was investigated, and why almost all of them are dead ends.  Mostly a record of *negative* results — read before re-attempting any of them. |
| `APP_HARNESS.md` | The operator's manual for playing the trained bot against the official app's Hard AI by hand.  The only externally calibrated anchor available. |

## Infrastructure and performance

| doc | answers |
|---|---|
| `PYPY.md` | Should the trainer run on PyPy, and the undo/journal stack that replaced copy-per-candidate.  Long; the methodology is the durable part.  Carries a dated correction at the top. |
| `DESKTOP_QUIET.md` | Keeping training invisible on the owner's gaming PC, and keeping the GPU guard alive.  The operational manual for that box. |

## Elsewhere in the repo

`engine/PROGRESS.md`, `experiments/PROGRESS.md`, `data/PROGRESS.md` and
`advisor/PROGRESS.md` are per-package build logs.  `analysis/frozen/README.md`
records which frozen weight vectors are trustworthy and which are quarantined —
read it before quoting any frozen-champion number.

## Housekeeping

* **61 documents on 2026-07-30, 54 after the consolidation** (53 topic docs plus
  this index); 55 once `PLAY_RATE_AUDIT.md` landed later the same day.  Nine were deleted and their live content migrated; one was
  renamed (`UNATTENDED.md` -> `HAZARDS.md`) and one superseded in place
  (`OPEN_AFTER_THE_AUDIT.md` -> `OPEN_ITEMS.md`); five stale-wrong documents got
  dated correction banners
  rather than deletion, because the wrong conclusions in them were the kind
  somebody would otherwise re-reach.
* Open work goes in `OPEN_ITEMS.md`.  Traps go in `HAZARDS.md`.  Neither is a
  place for narrative.
* If you delete a document, `grep -rn '<NAME>.md'` across the whole repo first —
  code comments cite these files heavily (over fifty comments point into
  `CARD_BLINDNESS.md` alone), and a dangling citation is exactly the debris this
  index exists to prevent.
* Still worth doing, and deliberately not done in this pass because the section
  numbers are cited from code: merging `SCORE_*` into one scoring document, the
  eight-document military/combat cluster into one, and the five card-pricing
  documents into one organised by card type rather than by investigation lane.
