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

### Result: 48.8% held-out top-1 accuracy — a PAIRED comparison against `WeightedBot`

**"Paired" means this**: `WeightedBot::rank_moves` (the real, currently-fielded
champion evaluator — `experiments/rust_champion_{2,3,4}p.json`, the same
weights the training league plays with) was scored, via `bin/agreement.rs`
unmodified, on the EXACT SAME 8,548 held-out decisions from the EXACT SAME
152 held-out games this baseline was evaluated on (`human_policy::
is_held_out`'s game-id split) — not `AGREEMENT.md`'s own headline number,
which comes from a different 150-game, all-tier sample. Both models see the
identical decision set, decision for decision; the two counts below are
directly comparable, and this is a stronger check than the doc originally
reported (2026-08-06, see the retraction note under "Per move category"
below for the mistake this caught).

| model | k/n | rate |
|---|---|---|
| **this baseline, held-out** | **4,172/8,548** | **48.8%** |
| `WeightedBot`, PAIRED on the identical 8,548 held-out decisions | 2,491/8,548 | 29.1% |
| (`AGREEMENT.md`'s own headline, different 150-game all-tier sample — kept for context only, NOT paired) | 2,493/9,428 | 26.4% (95% CI 25.6–27.3%) |

The reference bot is optimized to WIN games; this baseline is optimized to
IMITATE the exact human move. They are not the same objective, and a higher
imitation-accuracy number is exactly what training FOR that objective should
produce — it is not evidence the baseline is a stronger PLAYER (see "Why
this exists" above).

`bin/humandata.rs`'s defensive "human_move not found in legal_moves" skip
branch never fired on the full 716-game extraction run — every recorded
decision's `human_move` was confirmed present in its own `legal_moves` list,
so no decisions were silently dropped for that reason.

**Verdict: this baseline models human play more usefully than the reference
bot does, on this metric.** It beats `WeightedBot`, PAIRED on the identical
held-out decisions, by a wide, credible margin (48.8% vs 29.1%) — the
headline holds up, and slightly more cleanly than the original unpaired
26.4% comparison suggested.

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

**RETRACTION (2026-08-06), read this before the table**: the first version
of this section compared this baseline's per-category accuracy against
`AGREEMENT.md`'s quoted per-category numbers — a DIFFERENT, easier, 150-game
all-tier sample, not the same decisions this baseline was evaluated on. On
that unpaired comparison, `tactics`/`aggression_or_war`/`bid`/`pact` all
looked substantially WORSE than the reference. Once `WeightedBot` was scored
PAIRED, on this baseline's exact 8,548 held-out decisions (see "Result,"
above), that claim did not survive for `aggression_or_war` and `bid`
specifically — both are within noise of each other at n=11, and the
`AGREEMENT.md` numbers those two rows were originally compared to (37.5%,
50.0%) turn out to come from an easier sample, not a fact about
`WeightedBot` that held on this baseline's own held-out games. This mistake
is left visible rather than quietly fixed, because the underlying lesson —
an unpaired comparison against a differently-sampled reference number can
invert a per-category finding, even when the overall headline direction
survives — is worth the next agent seeing directly. `build`, `tactics` and
`pact` DO hold up as real (if thin) baseline weaknesses under the paired
comparison — see below.

| category | baseline k/n | baseline rate | `WeightedBot`, PAIRED k/n | paired rate |
|---|---|---|---|---|
| end_turn | 1,517/1,580 | 96.0% | 1,177/1,580 | 74.5% |
| political_action | 544/564 | 96.5% | 203/564 | 36.0% |
| increase_population | 521/730 | 71.4% | 110/730 | 15.1% |
| leader_or_wonder_step | 760/1,101 | 69.0% | 215/1,101 | 19.5% |
| other | 405/951 | 42.6% | 327/945\* | 34.6% |
| build | 205/822 | 24.9% | 283/822 | 34.4% |
| take_card | 211/2,646 | 8.0% | 155/2,646 | 5.9% |
| tactics | 5/125 | 4.0% | 18/125 | 14.4% |
| pact | 0/7\* | 0.0% | 2/13\* | 15.4% |
| aggression_or_war | 2/11 | 18.2% | 1/11 | 9.1% |
| bid | 2/11 | 18.2% | 0/11 | 0.0% |

\* `other`/`pact` `n` differs by a handful of decisions between the two
columns because the two binaries' category boundaries are not quite
identical: `human_policy::categorize` (this baseline's categorizer) folds a
`Move::Choose` that accepts/refuses a pact offer into `other`, since this
extraction path does not thread the pre-move `Pending` snapshot needed to
recover that context (documented in `categorize`'s own doc comment);
`bin/agreement.rs::categorize_choice` DOES thread that snapshot and buckets
the same decision as `pact`. Same underlying decisions, different bucket —
not a data error, just two categorizers that were never required to agree
down to this one edge case.

**Read the table like this.** The gains are concentrated in exactly the
categories `AGREEMENT.md`'s own headline finding calls the reference bot's
weakest spot: `end_turn`/`political_action`/`increase_population`/
`leader_or_wonder_step` are the "when to stop building and do something with
deferred payoff" decisions a 1-ply hand-tuned evaluator systematically gets
wrong (`AGREEMENT.md`'s "the dominant pattern" section) — this baseline,
fit directly to human choices, closes most of that gap, and the paired
`WeightedBot` numbers confirm the gap is real (74.5%/36.0%/15.1%/19.5%, not
the higher unpaired reference numbers). `take_card` (8.0% vs paired 5.9%) is
essentially UNCHANGED between the two models — both are near-blind to WHICH
row card a human takes, the documented `features()`-encoding limitation
(`HUMAN_PLAY.md`'s "the clone still misses badly on takes... the evaluator
has no feature that distinguishes one row card from another") this baseline
inherits verbatim, not a new gap.

**What survives as a real baseline weakness, paired**: `build` (24.9% vs
34.4%, n=822), `tactics` (4.0% vs 14.4%, n=125), and `pact` (0.0% vs 15.4%,
n=7 vs 13, see the category-boundary note above) are all genuinely WORSE
than `WeightedBot` on the identical held-out decisions. All three are worth
taking seriously despite `build`'s and `tactics`' larger n — but `pact`'s
n=7-13 is thin enough that a couple of decisions either way would move the
rate substantially.

**What does NOT survive, and must be read as near-noise, not a finding**:
`aggression_or_war` (2/11 vs 1/11) and `bid` (2/11 vs 0/11) are both n=11 —
a single decision flipping either way changes the rate by ~9 points. Do not
read "baseline 18.2% vs bot 9.1%/0.0%" as "the baseline is better at war/
auctions"; read it as "this dataset does not contain enough
`aggression_or_war`/`bid` decisions, paired or not, to say anything about
either model's relative skill there." These are exactly the categories the
"why this exists" motivation cares about most (a sparring partner needs to
actually declare war and contest auctions) — and the honest finding is that
neither this baseline nor the reference bot's behaviour on them can be
distinguished from noise on this corpus, not that either one is clearly
better or worse.

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
2. **`build`, `tactics`, and `pact` are genuinely worse than `WeightedBot`,
   paired on identical held-out decisions** (24.9% vs 34.4%, n=822; 4.0% vs
   14.4%, n=125; 0.0% vs 15.4%, n=7–13 — see "Per move category," above, and
   its retraction note on what did NOT hold up: `aggression_or_war` and
   `bid` are n=11 each and must be read as noise, not a finding, in either
   direction). `tactics`/`pact` most likely share `take_card`'s
   card-identity blindness (the shared `features()` encoding has no
   coordinate that distinguishes one specific tactic/pact card from
   another), and their thinness (125/54 decisions total, corpus-wide) means
   this baseline has seen very little of either to begin with. `bid`/
   `aggression_or_war` cannot currently be judged AT ALL on this corpus —
   not "the baseline is fine there," but "there is not enough data, paired
   or not, to know." Before wiring this into the training league: either
   accept `build`/`tactics`/`pact` as known, real gaps, or invest in a
   card-identity-aware feature (closing the same gap `take_card` already
   has) and/or importance-weighting the rare categories during training so a
   full-batch gradient descent does not implicitly treat 46 aggression
   decisions as noise against 12,595 take_card ones — and separately, treat
   `bid`/`aggression_or_war` as simply UNMEASURED rather than weak, until a
   larger or differently-sampled corpus can move their n past the
   noise floor.
