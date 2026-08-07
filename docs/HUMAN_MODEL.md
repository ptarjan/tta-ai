# Human-imitation dataset and baseline policy (Stage One only)

Companion to [`REPLAY.md`](REPLAY.md) (the reconstruction machinery) and
[`AGREEMENT.md`](AGREEMENT.md) (the move-agreement analysis this project
reuses ~90% of the extraction machinery from). This doc covers a DIFFERENT
goal from either: not "does the hand-tuned bot resemble a human" but "can we
build a DATASET and a baseline MODEL that imitates STRONG human play," as the
first stage of a sparring-partner project (see "Why," below). **This is
Stage One only** — the dataset and an honest baseline policy with measured
held-out accuracy. Nothing here is wired into the training league or the
accept gate (`rust/src/bin/climb.rs::challenge`); that is a separate,
unmade decision.

## Why this exists (read before judging the accuracy number below)

The self-play training league (`RUST_LEAGUE.md`) only ever plays copies of
itself, so it only ever learns to counter its own blind spots — it never
declares war, so it never learns to defend one; it rarely reaches a
colonization auction, so it never learns to contest one
(`AGREEMENT.md`'s own `bid`/`aggression_or_war` rows: thin, but pointing at
"doesn't reach the decision" rather than "declines once there"). A bot
trained to IMITATE strong humans is not meant to become a strong player
itself — an imitation model averages across incompatible human strategies
and caps out below the source players' own strength, a well-known limitation
of behaviour cloning restated here because it is the reason this document
does not report "is the resulting model a good player." It is meant to be a
SPARRING PARTNER: something that reliably generates the curriculum (real
aggression, real auction contests) self-play structurally cannot.

## Deliverable 1: the dataset

### Regeneration (exact command)

```text
tar -xzf sources/bgo/journals.tar.gz -C /tmp/bgo-journals   # once
cd rust
cargo run --profile difftest --bin humandata -- \
    ../sources/bgo/index.tsv /tmp/bgo-journals/journals Warlord \
    > ../human_dataset.tsv 2> ../human_dataset_report.txt
```

`Warlord` is the minimum BGO skill tier (inclusive) — `corpus::GameMeta::tier`
parses `sources/bgo/index.tsv`'s own `level` column
(`Prince < King < Warlord < Emperor`, a real ladder, not a proxy). This
selects every game tiered Warlord or Emperor — no subsample, no
cherry-picking, unlike `AGREEMENT.md`'s own 150-game subsample (which only
took one because that analysis needed no persisted dataset file).

### How much strong-tier data actually exists — not thin

**716 of the corpus's 1,011 games (71%) are Warlord or Emperor tier** (252 +
464). Every one of the 716 has a journal on disk and was processed — zero
skipped for a missing file. `docs/AGREEMENT.md`/`HUMAN_PLAY.md` already
found BGO's own tier ladder barely moves move-level agreement (24–28% across
all four tiers, overlapping CIs) — so filtering to the top two tiers mainly
buys "these are the players closest to what a sparring partner should
imitate" by construction, not a materially different data DISTRIBUTION; this
is stated here so nobody re-derives it expecting tier to be a strong
confound.

Full-corpus extraction actually run (2026-08-06):

| | count |
|---|---|
| games matching tier filter | 716 |
| games processed (journal found) | 716 |
| decisions recorded | 41,214 |
| dataset file size | 234 MB (`human_dataset.tsv`, sparse encoding) |

**Verdict: strong-tier data is NOT thin.** 41,214 decisions across 716 games
is MORE volume than `AGREEMENT.md`'s own 9,428-decision, all-tier, 150-game
reference sample — the reference sample is simply a smaller subsample of a
much larger available corpus, not evidence that strong-tier data runs out.

### Breakdown

By player count:

| players | decisions |
|---|---|
| 2p | 25,318 |
| 3p | 5,728 |
| 4p | 10,168 |

By game age at the decision point (`GameState::age_civil`, structural, not a
journal re-parse):

| age | decisions | share |
|---|---|---|
| A | 4,351 | 10.6% |
| I | 36,629 | 88.9% |
| II | 234 | 0.6% |
| III / IV | 0 | 0% |

By move category (same bucket definitions as `AGREEMENT.md`'s `Category`,
restated in `human_policy::categorize` — see that function's own doc comment
for the one deliberate narrowing: a `Move::Choose` resolving a pact
offer/refusal is bucketed `other` here rather than `AGREEMENT.md`'s finer
`pact` split, because this extraction path does not thread the pre-move
pending snapshot through; `OfferPact` itself, the human PROPOSING a pact,
still lands in `pact` correctly):

| category | decisions |
|---|---|
| take_card | 12,595 |
| end_turn | 7,642 |
| leader_or_wonder_step | 5,203 |
| build | 4,259 |
| other | 4,391 |
| increase_population | 3,790 |
| political_action | 2,649 |
| tactics | 516 |
| bid | 69 |
| pact | 54 |
| aggression_or_war | 46 |

### The late-game sampling bias — quantified

`replay.rs`'s reconstruction stops on genuinely unrecoverable hidden
information (mostly an interleaving/action-budget edge case now, per
`REPLAY.md`'s fourth pass) well before most real games finish. Two ways to
see exactly how much of a real game this captures, both measured on the
716-game strong-tier run:

1. **By age**: 99.4% of recorded decisions are Age A or Age I; Age II is
   0.6%; Age III/IV — where a 2015-base-game score is actually decided — is
   **0%**. None of this dataset's accuracy numbers can speak to late-game
   play at all.
2. **By round, against `index.tsv`'s own ground truth**: the 716 real,
   COMPLETED games this sample draws from averaged **19.3 rounds**
   (`GameMeta::rounds`). The highest round any recorded decision in the SAME
   game reached averaged **5.1 rounds** — **26.4% of the true game length**.

Both point the same way: this dataset is an early-game (and to a much
smaller extent, early-mid-game) dataset. A model trained on it has no
evidence about late-game play — colonization, endgame scoring pushes, or
late wars — and its accuracy numbers below must be read as "does this
imitate early-game human play," not "does this imitate human play."

### Discard taint — quantified, not silently dropped

Every forced military discard in this corpus is resolved by an ARBITRARY
pick among legal candidates (`discard_solver.rs`; BGO never names a military
card at draw time — see `REPLAY.md`'s discard-solver section: 0 of the
sampled discards were ever uniquely "Solved," all were "Chosen" among
several valid candidates). Every decision downstream of one has a partly
fictional simulated military hand.

**13,404 of 41,214 decisions (32.5%) are discard-tainted**
(`ExtractedDecision::discard_tainted`, carried straight off
`replay_common::Decision::after_arbitrary_discard`) — close to
`AGREEMENT.md`'s own 29.5% on its all-tier sample, so tier filtering does not
materially change this. Every row in `human_dataset.tsv` carries this flag
so a consumer can filter or weight it; this project makes no judgement about
which is "more correct" to train on. The baseline model below is trained
and evaluated on BOTH (with-and-without-taint accuracy reported separately).

### Legality audit: no private information read

The card row, played cards, boards (culture/science/resource/food stock,
tech, wonder progress, workers — everything physically face-up in the 2015
base game), and discard piles are PUBLIC and used freely. Rivals' HAND
CONTENTS and the unshuffled deck order are PRIVATE and must never be read on
any evaluation or feature path — this is the exact bug that poisoned the
earlier neural work (`NEURAL.md`).

**Audited, explicitly, for this project**: `human_policy::candidate_features`
calls `bots::weighted::features::features`, the SAME raw board encoding
`WeightedBot` (the real, currently-fielded evaluator) is scored with — not a
parallel encoding invented for this project. Reading `features()`'s full
body (`bots/weighted/features.rs`) confirms every `Features::set` call
reads either the ACTING player's own board/hand, a rival's PUBLIC board
field (`rivals::rival_board` — stock counts, colony/wonder/leader counts,
all physically face-up per that module's own doc comment citing the standing
"public info can be used" rule), a rival's hand SIZE
(`PlayerState::hand_size_civil`, a count, not an identity), or a
`RivalContext`'s rate/strength SUMMARY fields (`ctx.rival_culture_rate`,
`rival_science_rate`, `rival_strength`, `rival_rates`, `ctx.event_pool`) —
never a rival's `RivalView.hand_civil` (which `rival_context` DOES build
internally, for OTHER callers this project never invokes — `row::
row_pressure`, `cards::rival_hand_potential`). `candidate_features` calls
`features()` with `w: None, priced_only: true`, which as a side effect
always skips `event_scoring_margin` (an expensive, unrelated Age III
computation) rather than computing it — a compute-cost simplification (that
coordinate is a constant 0.0 across this whole dataset), not a privacy
decision.

No code path in this project reads a game's true civil/military deck order,
or any player's hand OTHER than the acting player's own. `candidate_features`
also reuses `WeightedBot::rank_moves`'s own `determinize_current_events`
guard, so a `Move::PrepareEvent` candidate is never scored against the TRUE
top of an unpeeked event pile either.

## Deliverable 2: the baseline policy model

### What it is, and what it deliberately is not

A single [`WeightKey`]-indexed linear vector (139-wide, the same
representation `WeightedBot`'s own champion weights use), fit by full-batch
gradient descent on a multinomial-logistic ("softmax over the legal-move
list") loss — the standard conditional-logit model for "which of these
labelled alternatives did the agent pick." **Not a deep net.** The brief for
this stage was explicit: keep it simple and honest. `rust/src/bots/neural/`
was not touched, reused, or audited for this pass — the known pre-2026-08-06
data leak documented in `NEURAL.md` is a separate, orthogonal problem for
whoever next works on that stack, not something this baseline depends on or
inherits.

Score of a candidate move = `w · features(trial_state)`, a plain dot
product over the SAME raw `Features` vector the dataset stores — no phase
blending, no rate horizon, no `hand_potential`/`wonder_potential`/
`tactic_gain`/`row_pressure`/`rival_hand_potential` identity-aware extra
terms (`WeightedBot::evaluate`'s own extra machinery, deliberately not
reused here: several of those terms build a `RivalView` off a rival's hand,
and while `evaluate()` itself only reads their PUBLIC-facing summaries, the
extra terms unavailable here — `hand_potential` and friends — would add
complexity for a first pass without adding to the legality guarantee this
document already had to establish). A future pass could extend the score to
the full `evaluate()` shape if this baseline's accuracy justifies the extra
complexity.

### Regeneration

```text
cargo run --profile difftest --bin humantrain -- human_dataset.tsv
```

Methodology: split BY GAME, never by decision (`human_policy::is_held_out`,
a deterministic FNV-1a hash of the game id, ~20% held out) — positions from
the same game are correlated (the same evolving board), so a decision-level
split would leak the held-out evaluation and produce a meaningless number.
A per-coordinate [`Normalizer`] is fit on the TRAIN split only and applied
to both. 300 epochs, learning rate 0.3, L2 = 1e-4 — untuned beyond a single
sanity pass (a real hyperparameter search was out of scope for "keep it
simple"); the toy-data and normalizer unit tests in `human_policy.rs` are
what actually pin the optimizer's correctness, not this run's specific
numbers.

### Result: 48.8% held-out top-1 accuracy — nearly double the 26.4% reference

| split | k/n | rate |
|---|---|---|
| **this baseline, held-out** | **4,172/8,548** | **48.8%** |
| reference (`AGREEMENT.md`, trained `WeightedBot` vs. human, all tiers) | 2,493/9,428 | 26.4% (95% CI 25.6–27.3%) |

**Read this comparison carefully — it is not apples-to-apples on every
axis**, though the direction and size of the gap are real:

- The reference number comes from a DIFFERENT sample (all four tiers,
  150 games, decision-level, no train/held-out split — it was never fit to
  the data at all, since `WeightedBot`'s weights come from self-play, not
  from this corpus). This baseline's number is genuinely held-out (by game).
- The reference bot is optimized to WIN games; this baseline is optimized
  to IMITATE the exact human move. They are not the same objective, and a
  higher imitation-accuracy number is exactly what training FOR that
  objective should produce — it is not evidence the baseline is a
  stronger PLAYER (see "Why this exists" above).
- `bin/humandata.rs`'s defensive "human_move not found in legal_moves" skip
  branch never fired on the full 716-game extraction run — every recorded
  decision's `human_move` was confirmed present in its own `legal_moves`
  list, so no decisions were silently dropped for that reason.

**Verdict: this baseline models human play more usefully than the reference
bot does, on this metric.** It beats the 26.4% reference by a wide,
credible margin.

### Per discard-taint

| slice | k/n | rate |
|---|---|---|
| untainted | 2,954/5,824 | 50.7% |
| tainted | 1,218/2,724 | 44.7% |

A real but modest gap (~6 points) — tainted decisions are harder, as
expected (the simulated military hand is partly fictional), but not so much
harder that the overall number is being carried entirely by the untainted
slice.

### Per player count and BGO tier

| players | k/n | rate |
|---|---|---|
| 2p | 2,667/5,527 | 48.3% |
| 3p | 686/1,365 | 50.3% |
| 4p | 819/1,656 | 49.5% |

| tier | k/n | rate |
|---|---|---|
| Emperor | 2,587/5,299 | 48.8% |
| Warlord | 1,585/3,249 | 48.8% |

Flat across both — consistent with `AGREEMENT.md`/`HUMAN_PLAY.md`'s standing
finding that neither player count nor tier is a strong confound here.

### Per move category — where it actually works, and where it does not

| category | k/n | rate | reference (`AGREEMENT.md`) |
|---|---|---|---|
| end_turn | 1,517/1,580 | 96.0% | 66.3% |
| political_action | 544/564 | 96.5% | 30.4% |
| increase_population | 521/730 | 71.4% | 11.7% |
| leader_or_wonder_step | 760/1,101 | 69.0% | 14.0% |
| other | 405/951 | 42.6% | 36.9% |
| build | 205/822 | 24.9% | 22.4% |
| take_card | 211/2,646 | 8.0% | 7.5% |
| tactics | 5/125 | 4.0% | 38.5% |
| aggression_or_war | 2/11 | 18.2% | 37.5% |
| bid | 2/11 | 18.2% | 50.0% |
| pact | 0/7 | 0.0% | 39.5% |

**This is the single most important table in this document — read it before
trusting the 48.8% headline.** The gains are concentrated in exactly the
categories `AGREEMENT.md`'s own headline finding calls the reference bot's
weakest spot: `end_turn`/`political_action`/`increase_population`/
`leader_or_wonder_step` are the "when to stop building and do something with
deferred payoff" decisions a 1-ply hand-tuned evaluator systematically gets
wrong (`AGREEMENT.md`'s "the dominant pattern" section) — this baseline,
fit directly to human choices, closes most of that gap. `take_card`
(8.0% vs 7.5%) is essentially UNCHANGED — both models are near-blind to
WHICH row card a human takes, for the documented reason
(`HUMAN_PLAY.md`'s "the clone still misses badly on takes... the evaluator
has no feature that distinguishes one row card from another" — a limitation
of the shared `features()` encoding this baseline inherits verbatim, not a
new one). **`tactics`/`aggression_or_war`/`bid`/`pact` are all substantially
WORSE than the reference, at very thin n (7–125 held-out decisions each)** —
this is exactly the population `AGREEMENT.md` already flagged as thin and
heavily discard-tainted (100% tainted for `bid`/`aggression_or_war` in that
sample; not re-measured per-category here, but nothing about this dataset's
extraction changes that structural fact), and this baseline's card-identity
blindness likely hits WHICH tactic/pact/target card even harder than it hits
`take_card`, since these categories are lower-volume to begin with. **Read
these four rows as a real, unresolved weakness, not statistical noise to be
waved away** — and note they are precisely the categories the "why this
exists" motivation cares about most (a sparring partner needs to actually
declare war and contest auctions, not just imitate the aggregate rate
correctly).

## Verdict: worth continuing to Stage Two, with two named caveats

**Yes, on balance** — the data volume is not the constraint many similar
projects hit (716 games / 41,214 decisions, not thin), the legality audit is
clean, and the baseline beats the reference by a wide, credible, mostly
well-explained margin on the categories that matter for general play.

Two caveats a Stage Two decision must weigh, both already fully quantified
above, neither hidden:

1. **This is an early-game model.** 99.4% of the data is Age A/I; Age III/IV
   (where a base-game score is decided) is entirely absent. Nothing here
   licenses a claim about late-game imitation.
2. **The exact categories the "why this exists" motivation cares about most
   — `aggression_or_war`, `bid`, `pact`, and (for reaching them at all)
   `tactics` — are this baseline's weakest, on its thinnest and most tainted
   data.** A sparring partner built on this baseline as-is would likely still
   under-declare war and under-contest auctions relative to a genuinely
   strong human, for a different reason than the self-play league does today
   (card-identity blindness plus data thinness, not "never learns to," but
   the practical symptom — a bot that rarely fights — could look similar).
   Before wiring this into the training league, either accept this
   limitation explicitly, or invest in the two things most likely to move
   these four rows: a card-identity-aware feature (closing the same gap
   `take_card` already has) and/or importance-weighting the rare categories
   during training so a full-batch gradient descent does not implicitly
   treat 46 aggression decisions as noise against 12,595 take_card ones.
