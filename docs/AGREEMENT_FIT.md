# Is the champion's weakness in its weights or its features?

**Verdict: WEIGHTS, with one named FEATURES exception (`take_card`).**

Directly fitting `WeightedBot`'s ~140 weights to strong-human move choices
by supervised softmax (`rust/src/bin/agreefit.rs`) very nearly doubles
top-1 agreement over the hill-climbed champion on held-out games (21.1% ->
38.9%, comfortably past the "~32%+ means co-adaptation, not a blind
basis" bar this project set itself) with almost no train/held-out gap
(38.8% train vs 38.9% held-out). The SAME feature basis the champion
already climbs on can express a much more human-like policy than 1,455
generations of hill-climbing ever found — most of the champion's weakness
was a bad optimum in weight-space, not a missing feature. The one loud
exception: `take_card`, the single worst category in the original corpus
census, barely moves (3.9% -> 6.4%) even with every weight in the basis
free to fit it directly on the very games being measured. That specific
category needs a new feature, not a better number on an old one.

## Method (see `rust/src/bin/agreefit.rs`'s own doc comment for the full account)

1. Reused `tta::replay_common::replay_game(..., record_decisions: true)`
   — the same decision-point walker `bin/agreement.rs` uses. No second
   replay walker.
2. Reused the same feature-extraction code path `WeightedBot::rank_moves`
   itself scores through: `eval::candidate_features`/`linear_features`
   (new, `rust/src/bots/weighted/eval.rs`), which share `rank_moves`' own
   root/`RivalContext`/`determinize_current_events`/trial-apply/
   `end_turn_bias` machinery and read the exact same `features::features`/
   `cards`/`row`/`events` calls `evaluate` calls. Pinned exact (bit-level)
   equivalence to `evaluate` by a unit test
   (`linear_features_dotted_with_a_weight_vector_reproduces_evaluate_exactly`)
   — confirmed empirically too: the champion vector, dotted against this
   file's own cached features, reproduces 21.1% top-1 agreement, matching
   the known corpus-wide 21.4% (`docs/CHAMPION_VS_HUMANS.md`) within the
   noise of measuring 120 games instead of 716.
3. **One documented linearity gap.** Ten of the ~140 coordinates
   (`hand_potential`, `wonder_potential`, `hand_mil_potential`,
   `rival_hand_potential`, `row_urgency`, `row_bargain_forgone`,
   `row_last_copy`, `my_event_threat`, plus `rate_horizon`'s own scaling of
   the four rate features) are **bilinear** in the real `evaluate`: each is
   priced by a function that takes the FULL weight vector and reprices its
   own internal sub-terms through it, so `evaluate(state, w)` is not
   expressible as `w . f(state)` for any single fixed `f` on these ten. This
   fit freezes those sub-computations at the CHAMPION's numbers (so the
   frozen scalar is a fixed, w-independent feature) while leaving each
   coordinate's own OUTER gate weight fully free and fit — a documented
   approximation, not a hidden one (`eval.rs`'s own doc comment on
   `linear_features`). The other ~130 coordinates, including all four
   phase-blended pairs, are exactly linear and fit with no approximation.
4. Multinomial softmax cross-entropy (`score_i = w . f_i`, loss =
   `-log softmax(score_human)`), Adam, standardized features (mean/std
   from the train set only), a modest L2 (3e-4), 6 epochs, streamed one
   decision (one softmax over its own candidate list) at a time.
5. Split BY GAME: 250 train / 120 held-out Warlord+Emperor BGO games,
   selected by hashing each numeric game id (decorrelates the split from
   `index.tsv`'s chronological order) — never measured on a game trained
   on. 59,867 train decisions, 27,837 held-out decisions.
6. One fit. No tuning to a target number — this is the one honest pass the
   brief asked for.
7. Weights fit as ONE POOLED vector across all three player counts (not
   three separately-starved ~90-125-game fits) — see
   `analysis/frozen/agreefit/README.md`. Written to
   `analysis/frozen/agreefit/fitted_{2,3,4}p_agreefit_2026-08-08.json`
   (same JSON shape as `analysis/frozen/gauntlet/*.json`, loadable by the
   same `load_weights`/gauntlet tooling — not yet run through a gauntlet
   match as of this commit).

## Headline numbers, held-out (27,837 decisions, 120 games)

| weights | top-1 agreement |
|---|---|
| champion (reproduction check) | **21.1%** (5,870/27,837) — matches the known 21.4% corpus number |
| zero/uniform | 24.6% (6,849/27,837) — inflated; see "Reading the zero floor" below |
| **fitted** | **38.9%** (10,824/27,837) |

Fitted weights' own **train-set** agreement: **38.8%** (23,207/59,867) —
essentially identical to held-out, so this is not overfitting; the basis
genuinely supports this level of agreement, not just memorization of the
250 training games.

### By category (held-out) — the four `docs/CHAMPION_VS_HUMANS.md` weak categories in bold

| category | n | zero | champion | **fitted** |
|---|---|---|---|---|
| **take_card** | 7,028 | 0.0% | 3.9% | **6.4%** |
| build | 5,481 | 0.0% | 21.3% | 32.3% |
| **increase_population** | 1,876 | 0.0% | 10.0% | **36.6%** |
| **leader_or_wonder_step** | 2,215 | 0.0% | 18.0% | **49.8%** |
| political_action | 1,888 | 98.3% | 40.0% | 82.5% |
| **aggression_or_war** | 122 | 0.0% | 5.7% | **59.8%** |
| pact | 66 | 48.5% | 51.5% | 56.1% |
| tactics | 475 | 0.0% | 22.7% | 22.7% |
| bid | 935 | 39.7% | 23.6% | 42.8% |
| end_turn | 4,000 | 100.0% | 41.5% | 90.2% |
| other | 3,751 | 15.7% | 28.1% | 27.5% |

Three of the four named weak categories close most of the gap to human
play once the SAME basis is fit directly: `leader_or_wonder_step` (18.0% ->
49.8%), `aggression_or_war` (5.7% -> 59.8%, n=122 — small, treat the exact
number loosely, but the direction is stark), `increase_population` (10.0%
-> 36.6%). `take_card` alone stays badly blind (3.9% -> 6.4%) — see
"Blind-spot scan" below for why.

### By age (held-out)

| age | n | zero | champion | fitted |
|---|---|---|---|---|
| A | 711 | 40.8% | 56.4% | 40.8% |
| I | 10,660 | 26.3% | 22.9% | 41.9% |
| II | 9,213 | 25.0% | 19.1% | 39.3% |
| III | 6,056 | 20.7% | 16.8% | 34.2% |
| IV | 1,197 | 16.8% | 20.9% | 31.3% |

Fitted improves every age past the opening; Age A is a wash (the champion
was already unusually human-like there, 56.4%, likely because the opening
is close to forced anyway — few real alternatives that early).

### Reading the zero/uniform floor (INSTRUMENT ARTIFACT, not a bug)

Zero weights score every candidate 0.0, so the tie-break (first candidate
in `legal_moves`' own order wins — the same first-wins convention
`WeightedBot::choose` itself uses) decides everything. `end_turn` (100%)
and `political_action` (98.3%) are so high under zero purely because a
large share of those decisions have exactly ONE legal move (a forced
end-of-turn, a politics phase with nothing else to do) — "the floor agrees"
is really "there was nothing else it could have picked." Every category
with genuine choice (`take_card`, `build`, `increase_population`,
`leader_or_wonder_step`, `aggression_or_war`, `tactics`) scores exactly
0.0% under zero. This is why the honest floor-vs-fitted comparison is
per-category, not the blended 24.6% headline number, which is inflated by
how many decisions in this corpus are forced rather than by anything zero
weights "know."

## Blind-spot scan (train set, fitted weights, worst-first per weak category)

Failing on the TRAIN set is the stronger signal: this fit had every chance
to price these decisions correctly (they are exactly what it optimized
against) and still could not rank the human's move well.

- **take_card** — game 7521971, line 300: human's move ranked **6th of 29**
  candidates, 23.7 points behind the fitted top pick.
- **increase_population** — game 7521987, line 311: human's move ranked
  5th of 35, 19.4 points behind.
- **leader_or_wonder_step** — game 7521971, line 331: human's move ranked
  **dead last, 32nd of 32**, 32.2 points behind.
- **aggression_or_war** — game 7521666, line 507: human's move ranked 3rd
  of 19, 48.1 points behind.

`take_card`'s held-out number (6.4%, barely above champion's 3.9%) plus
its still-bad worst-case training example is the clearest single finding
here: with 43,086 corpus-wide take_card decisions and the WORST agreement
of any category before this fit, and STILL the worst after giving the
identical basis every opportunity to reprice it optimally, this category's
failure is not a weight problem. Two live hypotheses this project did not
chase further (out of scope for "one honest fit"): (a) `take_card`'s legal
list is often 10-40+ cards wide (the row plus deck-draw slots) — this is
the single highest-branching category by a wide margin, so getting the
`arg-1-of-30` exactly right is a harder target even for a perfect linear
model than a binary choice would be; (b) the deliberately-unpriced set
(`cards.rs::DELIBERATELY_UNPRICED`) already documents that several
take-worthy card properties (triggered/future-firing effects, event
addressing, several rule-change flags) are not represented as a feature
at ANY weight setting — a card whose value is invisible to every
coordinate in the basis cannot be correctly ranked by any `w`, which is
exactly the FEATURES failure mode this project set out to distinguish
from a WEIGHTS one.

## Largest champion -> fitted weight moves (2p champion as reference)

| weight | champion | fitted | \|delta\| |
|---|---|---|---|
| `best_theater` | 51.704 | 0.861 | 50.843 |
| `rival_hand_civil` | -28.046 | -0.000 | 28.046 |
| `best_unit` | -27.401 | -0.279 | 27.122 |
| `end_turn_bias` | -20.353 | 0.198 | 20.551 |
| `pact_blocks_attack` | -0.169 | -19.924 | 19.755 |
| `no_aggression` | 3.775 | -10.402 | 14.177 |
| `rival_culture_rate` | -13.345 | -0.000 | 13.345 |
| `best_farm` | 12.378 | -0.369 | 12.747 |
| `wonder_overrun` | 0.318 | -7.931 | 8.250 |
| `culture_rate` | 8.741 | 0.862 | 7.879 |
| `discontent` | -8.753 | -1.025 | 7.728 |
| `wonders` | -1.180 | 5.874 | 7.054 |

Mixed picture, not a clean "champion underprices X" story. Some moves
match the "champion underprices building/development" hypothesis loosely
(`wonders` moves from slightly negative to clearly positive), but the
biggest movers (`best_theater`, `best_unit`, `best_farm`,
`rival_hand_civil`, `rival_culture_rate`) are the champion pricing a
"best tech level seen" or "rival's civil hand" term far more aggressively
than a direct human-choice fit ever wants — read as the hill-climb having
found large values for these because they helped WIN GAMES (the climb's
actual objective) that a human-agreement objective does not reward at all.
This is the expected shape for two different objectives (win-rate vs.
human-mimicry) landing on different vectors over the same basis, not
evidence either objective is somehow wrong.

## Scope notes and limitations (read before reusing these numbers)

- **One pooled weight vector across all player counts**, not three
  separately fit ones — see `analysis/frozen/agreefit/README.md`.
- **Ten coordinates are fit as a frozen-inner-pricing approximation**, not
  a fully faithful nonlinear fit — see "Method" step 3 above. This is a
  deliberate, documented scope reduction the linear-softmax formulation
  this project specified requires; a fully faithful fit of those ten would
  need a nonlinear (bilinear) optimizer, out of scope for "one honest fit."
- **Agreement is a blind-spot PROBE, not the target metric.** A vector that
  agrees with humans more is not necessarily a vector that wins more games
  — the fitted vector has not been run through a gauntlet match against the
  champion, and per Paul's own steer on this task, that comparison is
  deliberately left for later, not run here.
- No ENGINE or REPLAYER bug was found. The champion-reproduction check
  (21.1% vs. the known 21.4%) matches within the expected noise of a
  120-game sample vs. the full 716-game corpus, confirming the harness
  (and the new shared `linear_features`/`candidate_features` extraction)
  are faithful to `WeightedBot::evaluate`. The one thing worth flagging as
  an **INSTRUMENT ARTIFACT** (not a bug, but a number that misleads if read
  naively) is the inflated 24.6% "zero/uniform" headline — see "Reading
  the zero floor" above.
