# docs/ — what is here and which document answers which question

This index exists so the tree does not grow back to sixty files. **Before
writing a new document, check whether the answer belongs in one of these.**
Investigation write-ups whose question has been answered and whose fix has
landed should be folded into the relevant topic doc and deleted, not left
lying around.

The audience for every doc here is a future AI agent working on this repo,
not a human reader. Terseness and accuracy beat narrative.

---

## Start here

| doc | answers |
|---|---|
| [`OPEN_ITEMS.md`](OPEN_ITEMS.md) | *What is still open?* The single register of unfinished work, deferred decisions and unanswered questions. |
| [`HAZARDS.md`](HAZARDS.md) | *What will bite me?* Standing traps, every one of which has already cost a real bug — training-loop traps 1-8 (cited by number from code), radioactive vectors, measurement traps, git and multi-agent working rules. Predates the Rust port; read the banner at the top before trusting a `.py` path. |
| [`BOT_ARCHITECTURE.md`](BOT_ARCHITECTURE.md) | *Why is the bot shaped like this?* The roster, how a position is scored, how weights are declared/read/persisted, search structure, and the invariants that must not be broken — checked against current `rust/src/`. The long-lived architecture doc; read before touching the evaluator. |

There is no current whole-system behavioural census (the old `SYSTEM_COVERAGE.md`
was deleted 2026-08-06 as a point-in-time census of a bot generation that no
longer exists, with nothing durable to carry forward — see
[`AUDIT_HISTORY.md`](AUDIT_HISTORY.md)'s verdict on it). If you need to know
"how good is the bot right now", run it — there is no doc that answers that
as of this writing.

## The game itself

| doc | answers |
|---|---|
| [`RULES_SPEC.md`](RULES_SPEC.md) | The rules of 2015-edition base Through the Ages, 13 sections, every claim cited to the Handbook / Code of Laws / FAQ v1.5. **There is no rulebook PDF in this repo; this is the only copy.** |
| [`EXPERT_STRATEGY.md`](EXPERT_STRATEGY.md) | Published human expert consensus on how to play, gathered deliberately independent of our bots. Openings, leader/wonder tiers, military doctrine, government doctrine, top mistakes. |
| [`HEURISTICS.md`](HEURISTICS.md) | Human-facing playbook derived from our own self-play plus the book-bot benchmark. Self-grades its own evidence; read the grades. Carries a staleness caveat at the top. |

## The bot: architecture, search, evaluation

| doc | answers |
|---|---|
| [`DEEPER_SEARCH.md`](DEEPER_SEARCH.md) | Quiescence: resolving the pending-decision stack before scoring. Budgets, costs, why `LEVELS=1` and not 2, why QuiescentBot cannot be the training challenger. |
| [`INFORMATION_AUDIT.md`](INFORMATION_AUDIT.md) | *What can the evaluator actually see?* Field-by-field measurement of information gaps, and the row-leak fix. Carries a 4p-numbers-quarantined banner; read it. |
| [`PLAN_WAR_LOOKAHEAD.md`](PLAN_WAR_LOOKAHEAD.md) | Giving the beam a war lookahead, and what it did and did not fix. |
| [`AUDIT_HISTORY.md`](AUDIT_HISTORY.md) | Distilled 2026-08-06 from ten deleted rules/combat/coverage audit docs (~7,000 lines). §1: rules bugs found and fixed, verified live against `rust/src/`. §2: validation methodology lessons (what a corpus can and cannot prove). §3: standing evaluator/search architectural blind spots (1-ply deferred-payoff blindness, card-identity blindness, `end_turn` flattery, linear-evaluator limits) — read before assuming a Python-era bug is still live or still fixed. §4: per-source-doc verdict, for finding what was dropped and why. |
| [`EVALUATOR_HISTORY.md`](EVALUATOR_HISTORY.md) | Distilled 2026-08-06 from ten deleted evaluator-history docs. Dated entries on: fitted-vs-measured model constants, the rate-horizon fix, government pricing, the dominance guard (theft must never help), the transfer-test negative result (a vector trained under one search can invert under another), the book-bot external yardstick and the anchor-gate it motivated, and a closed-items list of coordinate-registry fixes. Companion to `BOT_ARCHITECTURE.md`: that doc is the current-state reference, this one is how each piece got that way. |
| [`NEURAL.md`](NEURAL.md) | The value net: what it is, the encoder and its versioning, how training data is generated (and the pre-2026-08-06 leak that invalidates older results), the two self-play loop generations found so far (v1's 41-hour null, v2's search-backed design with its promotion arms and kill conditions), and the desktop training box's operating notes. |

## Card pricing and coverage

| doc | answers |
|---|---|
| [`CARD_BLINDNESS.md`](CARD_BLINDNESS.md) | The originating finding (`_card_yields` silently dropped culture/science) plus absorbed sections on military/unit pricing, the per-card play-rate census across all 236 cards, board-aware pricing for leaders/actions/governments, and technology pricing. The largest doc in the tree; use its own internal section headers to jump around rather than reading start to end. |

## Training, league and strength

| doc | answers |
|---|---|
| [`TRAINING_RUN.md`](TRAINING_RUN.md) | The running operational log of the live arms. **Read newest entry first; later entries supersede earlier ones inline.** This is where "what is training right now" is recorded. |
| [`RUST_LEAGUE.md`](RUST_LEAGUE.md) | The current mechanism reference: what `rust/src/bin/climb.rs` and `experiments/rust_league.sh` actually run today — three arms, mirror-vs-champion duels, the anchor-drift veto, the stop sentinel, and which champion file on disk is actually live vs. a stale Python-era snapshot. |
| [`TWOP_PROFILE.md`](TWOP_PROFILE.md) | What a since-superseded 2p champion actually did (won by suppression, not scoring) — carries a banner marking the specific champion gone; the causal method survives. |
| [`FOURP_GAP.md`](FOURP_GAP.md) | Why an earlier 4p lineage (since discarded) converged somewhere actively bad, and the matched-generation control method used to show it wasn't just "needs more generations." |
| [`OPENING_AUDIT.md`](OPENING_AUDIT.md) | "4p champions open with a wonder" — real behaviour, but one hitchhiking weight, worth nothing. The canonical demonstration that a moved weight is not evidence. |

## Humans: corpus, baselines, imitation, outside sources

| doc | answers |
|---|---|
| [`HUMAN_PLAY.md`](HUMAN_PLAY.md) | Distilled 2026-08-06 from six deleted docs. The 1,011-game BGO corpus (what it is, where it lives, its one permanent blind spot); measured human baselines by player count; the negative result that human play does not cluster into archetypes; behaviour cloning (human-likeness and playing strength are anti-correlated at 1-ply, but the ordering flips under a deeper search); the survey of external AI/data sources (nothing usable exists); and, at the end, card-data provenance and the one real card-count bug that process found and fixed. |
| [`APP_HARNESS.md`](APP_HARNESS.md) | The operator's manual for playing the trained bot against the official app's Hard AI by hand. The only externally calibrated anchor available; there is no automated version of this measurement. |

## Elsewhere in the repo

[`data/PROGRESS.md`](../data/PROGRESS.md) is a per-package build log.
[`analysis/frozen/README.md`](../analysis/frozen/README.md) records which
frozen weight vectors are trustworthy and which are quarantined — read it
before quoting any frozen-champion number.

**There is no Python left in this repo.** `engine/`, `tests/`, the Python
half of `experiments/` and `tools/gate.sh` were deleted 2026-08-06 once the
last thing depending on them — the neural self-play loop — had every stage
ported to a Rust binary; `advisor/` and `harness/` had gone earlier for the
same reason. Docs that quote a `python3 ...` command in a "how to reproduce"
block are recording how a past measurement *was* taken and have been left as
written; they are history, not instructions.

## Housekeeping

**2026-08-06: the big cull.** Three agents distilled roughly 19,500 lines
across ~32 documents (`SCORE_AUDIT.md`, `COMBAT_AUDIT.md`, `COVERAGE_AUDIT.md`,
`SYSTEM_COVERAGE.md`, `UNCOVERED_TYPES.md`, `WAR_RATE_CENSUS.md`,
`AGGRESSION_RATE.md`, `AGGRESSION_STATUS.md`, `WASTED_ACTIONS.md`,
`EVENT_SEEDING.md`, `MODEL_CONSTANTS.md`, `RATE_HORIZON.md`,
`GOVERNMENT_PRICING.md`, `THEFT_IS_PRICED_BACKWARDS.md`, `TRANSFER_TEST.md`,
`STRENGTH_CHECK.md`, `CULTURE_GAP.md`, `COORDINATE_REGISTRY.md`,
`BOT_ROSTER.md`, `OPEN_ITEMS_CLOSED.md`, `NEURAL_SEARCH_LOOP.md`,
`NEURAL_EVAL.md`, `NEURAL_LOOP_NULL.md`, `PROXY_GUARDRAIL.md`,
`DESKTOP_QUIET.md`, `HUMAN_BASELINE.md`, `HUMAN_BOTS.md`,
`BEHAVIOUR_CLONE.md`, `EXTERNAL_AIS.md`, `BGO_CORPUS.md`, `SOURCES.md`) down
to four new files — [`AUDIT_HISTORY.md`](AUDIT_HISTORY.md),
[`EVALUATOR_HISTORY.md`](EVALUATOR_HISTORY.md), [`NEURAL.md`](NEURAL.md),
[`HUMAN_PLAY.md`](HUMAN_PLAY.md) — deleting all thirty-two originals. The
specific per-run numbers, digest hashes and multi-page investigation
narratives those docs carried are **not** reproduced in the new files;
recover them from git history (`git log --oneline -- docs/<OLD_NAME>.md`) if
you need them. What was kept: rules facts still true of the current engine,
methodology lessons, and durable architectural findings. A follow-up pass the
same day renamed the four from their provisional `_distilled_*.md` names and
repointed every dangling cross-reference this cull created, across `docs/`,
`rust/src/`, `experiments/*.sh`, and `experiments/deploy/*.ps1`.

`docs/PYPY.md`, `docs/LEAGUE_TRAINING.md` and `experiments/PROGRESS.md` were
deleted in earlier 2026-08-06 passes for the same reason (pure Python-era
history, no live code path); see git history.

Older housekeeping entries from the 2026-07-30/31 doc consolidation (which
merged card-pricing, scoring and combat satellite docs into
`CARD_BLINDNESS.md`, `SCORE_AUDIT.md` and `COMBAT_AUDIT.md` respectively) are
themselves now only in git history — the two latter target files no longer
exist post-cull; see `AUDIT_HISTORY.md` for what survived from them.

* Open work goes in [`OPEN_ITEMS.md`](OPEN_ITEMS.md). Traps go in [`HAZARDS.md`](HAZARDS.md). Neither is a place for narrative.
* If you delete a document, `grep -rn '<NAME>.md'` across the whole repo first — code comments cite these files heavily, and a dangling citation is exactly the debris this index exists to prevent.
