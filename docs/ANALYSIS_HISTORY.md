# Distilled analysis docs: search architecture, card pricing, training history

Distilled 2026-08-06 from eight investigation/analysis docs (CARD_BLINDNESS,
INFORMATION_AUDIT, DEEPER_SEARCH, FOURP_GAP, OPENING_AUDIT, TRAINING_RUN,
TWOP_PROFILE, PLAN_WAR_LOOKAHEAD; ~10,291 lines, all `git rm`'d). Full
narrative, per-run numbers and champion-specific weight tables are in git
history — search commit messages for the doc names above.

**Provenance note.** All eight source docs were written against the Python
engine (`engine/*.py`, `tools/*.py`), deleted 2026-08-06 in favour of the
Rust port under `rust/src/`. Every claim below was re-checked against current
`rust/src/` (grepped, not assumed) and is kept only where the equivalent
mechanism still exists there. Behavioural numbers (win rates, weight values,
census tables) describe specific long-gone Python-era bot generations —
several of the champions named in these docs are explicitly marked
superseded or discarded in the docs' own banners — and are **not** carried
forward.

---

## The headline finding, checked directly against `rust/src/`

Two evaluator/search limitations were the load-bearing claims across these
eight docs. Both were re-verified here rather than trusted from prose.

**1. A 1-ply evaluator cannot see a move whose payoff is deferred to another
player's decision (pact offers, aggressions, colony bids, most action
cards).** `rust/src/bots/quiescent.rs` (663 lines) and `rust/src/bots/plan.rs`
exist, are wired into the bot registry (`greedy.rs::BotKind::Quiescent`,
`neural/spec.rs::Kind::Plan`), and are the real fix — `quiescent.rs`'s own
module doc comment restates the mechanism accurately. **But the fix is not
the default anywhere that matters**: `rust/src/bin/climb.rs:414` (the live
trainer) defaults `kind: BotKind::Weighted` — plain 1-ply — and
`rust/src/advisor/advisor.rs::load_bot` (what `harness`/`advisor` use to play
the app's Hard AI, per `docs/APP_HARNESS.md`) constructs a bare `WeightedBot`
with no quiescence or beam at all. So: **the deferred-payoff blindness is
still live in what is actually trained and actually played today**; the
fix exists as an opt-in (`--kind quiescent`/`plan:...`) that nothing turns on
by default. Confirm before relying on this changing: `grep -n
"BotKind::Weighted" rust/src/bin/climb.rs` and `rust/src/advisor/advisor.rs::load_bot`.

**2. Card-identity blindness (two different cards in hand price the same).**
Partially fixed, verified from `rust/src/bots/weighted/weights.rs`:
`HandPotential` (own civil hand, the historically dominant case) defaults to
**0.125 — live, not gated to zero**. `RivalHandPotential`, `WonderPotential`
and `HandMilPotential` all default to **0.0** — present in code
(`bots/weighted/cards.rs`) but inert unless a trainer has fitted them
nonzero. Whether the actual champion files (`experiments/rust_champion_*.json`,
gitignored, training-box only, not present in a fresh clone) have moved
these off their defaults is **unverified** — check the file being played
before assuming either way (`docs/BOT_ARCHITECTURE.md` §3 explains why a
0.0-default weight is common).

Both points are elaborated, with the current-state architecture and the
general lesson, in `docs/BOT_ARCHITECTURE.md` §2 (`evaluate`'s four passes,
identity-aware terms) and `docs/AUDIT_HISTORY.md` §3 (both risks named
explicitly as "standing architectural risks to check for, not closed
Python-era findings"). Read those two before this file for anything current.

---

## Per-doc verdict

### CARD_BLINDNESS.md (5,614 lines) — SUPERSEDED

Originating finding: the Python evaluator's `_card_yields` silently dropped
any card effect keyed on `culture`/`science` (ten of sixteen wonders priced
at nothing beyond "it is a wonder"), plus absorbed sections on military/unit
pricing, a per-card play-rate census across all 236 cards, board-aware
pricing for leaders/actions/governments, and technology pricing. The fix —
give the evaluator identity-aware, weighted terms instead of a flat lookup
table — **shipped and is the current Rust architecture**:
`rust/src/bots/weighted/cards.rs`/`row.rs`/`events.rs` compute
`hand_potential`, `wonder_potential`, `hand_mil_potential`, `tactic_terms`,
`rival_hand_potential`, `row_pressure`, `row_last_copy`, `my_event_threat` —
documented as current in `docs/BOT_ARCHITECTURE.md` §2 point 4. Card and
board pricing generally now goes through `rust/src/bots/board_yields.rs`'s
swap-and-diff pattern (put the card on a cloned player, diff `effects::compute`
before/after) rather than any hand-written lookup table — a structural
improvement the Python doc's own §3 called for and did not have. The
general lesson (a linear evaluator with a bag-of-numbers card table cannot
tell a good card from a bad one until it's given an identity-bearing
feature) is preserved in `docs/AUDIT_HISTORY.md` §3. **DEAD**: every specific
number in the doc (the 236-card census, per-wonder win-rate A/Bs, the "unit
of analysis" statistical-interval correction in its §10, all fingerprint
digests) was measured against `experiments/champion_{2,3,4}p.json` — the
file since renamed `analysis/frozen/python_champion_{2,3,4}p_..._2026-07-26.json`
and marked a stale, 78-key, Python-era snapshot (`docs/RUST_LEAGUE.md`
"Which champion file is live?"). None of it is reproducible against the
live `experiments/rust_champion_*.json` lineage. Do not resurrect any of its
win-rate or census tables as facts about the current bot.

### INFORMATION_AUDIT.md (1,736 lines) — SUPERSEDED, one caution

Field-by-field census of what the rules make knowable (row contents/slot
cost, opponent hands, deck composition, opponent boards, the politics/event
deck) versus what `engine/bots/weighted.py:features()` actually read, plus
the "card counting is legal, all public info can be used" project ruling and
the row-leak/event-scoring-leak fixes. The ruling and its citation are
already preserved verbatim in `docs/EVALUATOR_HISTORY.md` ("Military discard
pile legibility"). The shipped fixes (row pressure, rival hand/board
scalars, event-pool-based scoring instead of an omniscient forecast,
civil-discard tracking) are the same Rust features listed above under
CARD_BLINDNESS and in `docs/BOT_ARCHITECTURE.md` §2. **DEAD**: every
`engine/bots/weighted.py:LINE` citation, every per-position sensitivity
table, and all 4p numbers (the doc's own banner: measured against
`analysis/frozen/champion_4p.DEGENERATE.json`, 20.1% against a 25% null — a
vector that loses to random seating). **Caution, unverified against
current Rust**: the doc's central unresolved item — the politics/future-events
deck (what a player seeded and when it resolves) — was, per the doc, read
correctly-and-legally only by the neural encoder, never by the linear
evaluator. `rust/src/bots/weighted/eval.rs` does have `my_event_threat`
(listed under identity-aware terms in `BOT_ARCHITECTURE.md`), which appears
to be the ported version of this fix, but this was not traced line-by-line
here; treat "does the linear evaluator price its own seeded events" as
unverified rather than settled.

### DEEPER_SEARCH.md (867 lines) — SUPERSEDED, mechanism LIVE-but-opt-in

Design/cost/strength case for quiescence (resolve `state.pending` before
scoring a candidate) plus a war lookahead for declared wars, and the
after-the-fact record of trying `QuiescentBot` as the training challenger at
3p/4p and reverting it (`1fbf128`, 2026-07-30) because it resolves a
defender's `defense` decision by reading the defender's real hidden
military hand — fine for self-play, indefensible for a bot that will play a
human. Both mechanisms ported: see the headline section above.
`rust/src/bots/quiescent.rs`'s own module doc comment cites this Python doc
as the design rationale and restates the hidden-information caution in its
`levels` discussion. **LIVE, opt-in only**: `BotKind::Quiescent` and
`Kind::Plan`/`Kind::NeuralPlan` all exist and work; neither the trainer
(`climb.rs`) nor the app-play path (`advisor.rs`) uses them by default —
see the headline section. The Stockfish-comparison discussion (§7: why
alpha-beta/deeper classical search doesn't port to an n-player, stochastic,
imperfect-information, positive-sum game) is a durable architectural
argument with no Python dependency; it is not restated in any current-state
doc and is worth preserving in one line: **deeper classical search is not
the next move here** — n-player minimax has no usable analogue,
the game is stochastic and imperfect-information, and the payoff horizon
(10-20 turns for a wonder) is far longer than any affordable ply count
reaches; the same argument that killed this idea in 2026-07 has not been
invalidated by the Rust port. **DEAD**: every win-rate/cost-ratio A/B
(quiescence's +5.8pp/+9.5pp/+16.7pp at 2p/3p/4p, the LEVELS=2 regression,
the `TTA_JOURNAL` cost multipliers) — Python engine, `TTA_JOURNAL`
environment variable and `tools/quiesce_bench.py`/`tools/dominance_probe.py`
do not exist in the Rust tree.

### FOURP_GAP.md (510 lines) — DEAD (subject discarded)

Diagnosed why a specific Python-era 4p lineage converged somewhere actively
bad (matched-generation control: 2p arm's vector beat `book` at 4p 57.4%
against the 4p arm's own vector's 27.6%). The doc's own banner says the
lineage was discarded 2026-07-30 when the 4p arm was reset to gen 0 after
reverting off `QuiescentBot` (see DEEPER_SEARCH above), so nothing about
`champion_4p` here describes any vector that exists. The one durable
methodological point — a matched-generation control is what separates "the
training is broken" from "it just needs more generations," and 4p is
structurally noisier than 2p (per-game culture-margin sd 107.2 vs 38.8, so
4p needs roughly 7.6x the games for equal statistical resolution) — is
worth one line for whoever next investigates a stalled arm, but the specific
mechanism was never confirmed (the doc says so itself) and `docs/OPEN_ITEMS.md`
is the place to check whether it was later resolved by a different route.

### OPENING_AUDIT.md (465 lines) — DEAD (subject reverted)

Showed that a "4p champion opens with a wonder" behaviour some Python-era
champion exhibited was not a real 4-player strategy but one hitchhiking
weight (`wonder_remaining` flipped sign by a single generation-5 mutation
that moved 19 weights at once) that never mattered (reverting it changed
the win rate by nothing, 0.276 vs a 0.25 null). The specific champion,
weight name and generation history are Python-era
(`experiments/hillclimb_league`, gen 5 of 138) and not reproducible.
**The durable lesson, worth keeping in one line**: a behaviour that moved
because a mutation happened to touch a group of weights together is not
evidence the behaviour is *strategy* — check whether reverting the one
plausible weight changes the win rate before writing up a "the bot
learned X" finding. This is the same shape as the "attribute, don't reason"
methodology lesson already in `docs/AUDIT_HISTORY.md` §2; no new content to
carry beyond that one line.

### TRAINING_RUN.md (383 lines) — DEAD

Operational log of the Python `experiments/hillclimb_league`/`run_league.sh`
supervisors, resume commands, and per-arm `--candidate-bot` flags. None of
this exists in the Rust tree — no `hillclimb_league`, no `run_league.sh`, no
`league_state/` directory, no `--candidate-bot` flag. `docs/RUST_LEAGUE.md`
is the current, correct operational reference for the live three-arm
`climb.rs`/`rust_league.sh` system (which champion file is live, how a
generation is scored, the anchor-drift veto, the stop sentinel) and already
supersedes this doc's role per `docs/README.md`'s own index. Nothing here
needs to be carried forward; the "do not commit `experiments/champion_*.json`
while a run is live" operational caution is restated for the Rust arms in
`docs/RUST_LEAGUE.md` directly ("Do not run git commands ... in the live
checkout while the arms are running").

### TWOP_PROFILE.md (360 lines) — DEAD (subject deleted)

Profiled a specific gen-181 2p champion as "a war bot" (declares its first
war ~round 15, wins by suppression — removing the war/aggression move class
cost it 59.0 of its 85.5-point margin — not by out-scoring). The doc's own
banner says that champion "no longer exists": the gate metric it was
exploiting was deliberately changed to kill this exact behaviour, and the
whole Python pool/objective mechanism it trained under has since been
replaced by `climb.rs`'s mirror-plus-anchor-veto design. Current numbers
cited in the banner itself (from `docs/AUDIT_HISTORY.md`) already show a
different profile for whatever 2p champion is live now (1.10 wars/game,
1.53 wonders completed) — check that doc, not this one, for anything about
current behaviour. The causal method (ban a move class, re-measure the
margin drop) is generically reusable but is not itself new information;
it is the same "attribute, don't reason" methodology already generalized in
`docs/AUDIT_HISTORY.md` §2.

### PLAN_WAR_LOOKAHEAD.md (356 lines) — SUPERSEDED

Gave `PlanBot` a war lookahead (price a declared war through the engine's
own `resolve_war` on a scratch copy rather than as pure cost), which closed
a measured transfer-test inversion: a vector trained under quiescent search
scored better under quiescent evaluation but *worse* under `PlanBot`'s beam
until this fix landed. **Shipped and current**: `rust/src/bots/plan.rs:532`
calls `quiescent::war_value` for exactly this, `PlanConfig::war_lookahead`
defaults to `true`, and `docs/EVALUATOR_HISTORY.md` ("The 1-ply/quiescent
-trained vector did not transfer to PlanBot's search") already records this
with the verified `rust/src/bots/plan.rs` pointer — this doc's entire
finding is duplicated there in already-distilled form. **DEAD**: the
specific before/after margin numbers (−97.4 → +1.4 win margin) describe a
Python-era vector-vs-vector transfer test with no current equivalent.

---

## Housekeeping

If a future audit wants the Python-era investigation detail behind any
verdict above — full census tables, digest hashes, per-generation weight
dumps — `git log --oneline -- docs/<NAME>.md` on this repository's history
finds the deleted file's full text. Nothing in this file is a rules fact;
for those see `docs/AUDIT_HISTORY.md` §1. For the current, code-checked
architecture (roster, scoring, weights, search, training), see
`docs/BOT_ARCHITECTURE.md`.
