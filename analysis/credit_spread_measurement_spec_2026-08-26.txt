# credit_spread_measurement_spec_2026-08-26.txt
#
# SPECIFICATION (not code) for the credit-half spread measurement: a
# p95-spread-shaped quantity for the credit class of zero-spread-row keys
# (Section 7, class 3 of the four classes documented on
# WeightKey::clamp_bound in rust/src/bots/weighted/weights.rs). READ-ONLY
# spec written 2026-08-26; nothing here has been built or run.
#
# Every citation below was verified against the tree on 2026-08-26 before
# being used. Section 9 lists the corrections found.

================================================================================
1. WHAT IS BEING MEASURED, AND WHY FEATSPREAD CANNOT
================================================================================

The credit keys are the ~20 WeightKeys whose only readers are the card
pricers in rust/src/bots/weighted/cards.rs (`w.get(credit)` reads inside
card_potential_core, sum_yields, gains_only_sum, and the dedicated
pricers). They enter the evaluation NOT through the linear feature vector
but INSIDE the frozen-vector sub-pricing: eval.rs:689-698 computes
HandPotential, WonderPotential, WonderPromise, HandMilPotential,
RivalHandPotential, RowUrgency, RowBargainForgone, RowLastCopy and
MyEventThreat by calling the pricers with the FREEZE vector (the
`freeze: &Weights` parameter of linear_features, eval.rs:612), and each
pricer resolves its own credit weight from that same frozen vector.

Because the credit coefficient is frozen OUT of the dot product, a probe
that moves ONLY the credit key changes nothing in phi' -- the spread of
dot(w, phi') over the candidate set is exactly 0.0 by construction, and
clamp_bound falls back to CLAMP_BLIND (weights.rs:2045). This is the zero
row. Section 7 class 3 said these are NOT a category error: with the probe
in the sub-pricing (not the dot product), the per-card value DOES swing
across the candidate set, in the same units (eval points) as every
measured featspread row.

WHAT THIS MEASURES THAT FEATSPREAD CANNOT:
  The candidate-set spread of the probe-perturbed score, S_d = max_m
  dot(pw, phi_p(m)) - min_m dot(pw, phi_p(m)) (section 3), where the
  swing comes entirely from the probe's effect inside the frozen
  sub-pricing. featspread cannot produce this number because its probe is
  in the dot product, and the credit keys are invisible to it by
  construction.

WHAT IT DOES NOT MEASURE (say so in every report of this number):
  S_d is a LEVER, not INFLUENCE. A large S_d says "this credit can move
  the score difference between the best and worst candidate by S_d." It
  does NOT say the key changes the CHOSEN move: if the spread is small
  relative to the gap between the top two candidates (the gap that
  decides the argmax), the flip rate is zero no matter how large the
  lever. This is the same lever-vs-influence distinction multcheck's flip
  rates measure, and the spec REQUIRES the flip rates reported alongside
  S_d (section 6), or the number will be misread as importance. A key can
  have a large S_d and a near-zero flip rate, and that is a genuine,
  informative, non-contradictory result.

================================================================================
2. THE 20 CREDIT KEYS
================================================================================

From the `Credit =>` declarations in weights.rs (19 with the "Credit"
suffix; the per-type board offsets are the rest). The measurement set is:

  1  CardRateCredit         (w.get at cards.rs:2031, :2314; resolved once
                             by card_potential_core, threaded as the
                             `credit` argument to sum_yields)
  2  UnitStrengthCredit     (cards.rs:544, via sum_yields)
  3  TerritoryCredit        (cards.rs:560, via sum_yields)
  4  BonusCardCredit        (cards.rs:561, via sum_yields)
  5  CardBoardCredit        (cards.rs:2032, the `base` in
                             credit_board = base + per-type offset)
  6  TechBoardCredit        (cards.rs:1947, card_potential_core)
  7  ActionBoardCredit      (cards.rs:1961, card_potential_core)
  8  GovBoardCredit         (cards.rs:1956, card_potential_core)
  9  WonderBoardCredit      (cards.rs:1991, card_potential_core)
  10 TacticBoardCredit      (cards.rs:2004, card_potential_core)
  11 AggressionBoardCredit  (cards.rs:2007, card_potential_core)
  12 WarBoardCredit         (cards.rs:2012, card_potential_core)
  13 PactBoardCredit        (cards.rs:2017, card_potential_core)
  14 EventBoardCredit       (cards.rs:2022, card_potential_core)
  15 UnitTechCredit         (cards.rs:1949, card_potential_core)
  16 BuildFreshCredit       (cards.rs:1328, tech_value)
  17 RestrictedResourceCredit (cards.rs:766)
  18 FreeActionCredit       (cards.rs:1287, action_value)
  19 TacticReachCredit      (cards.rs:1478, tactic_value)
  20 CardBoardLeader        (weights.rs:945; the sole per-type offset
                             in cards.rs:597-601 board_credit_key, read
                             at cards.rs:2033 via
                             credit_board = base + offset)

NOT IN THE SET (documented, so a future reader does not re-derive it):
`board_credit_key` (cards.rs:597-662) returns `Some(WeightKey::
CardBoardLeader)` for Leader type only; every other type
(Government, Action, Wonder, Bonus, Tactic, Aggression, War, Pact,
Territory, Event, build types) returns `None`. The per-type offset
keys `card_board_government` / `card_board_action` / `card_board_
wonder` / `card_board_bonus` were retired (they moved into
`RETIRED_KEYS`) because their dedicated pricer branches dispatch
before `board_credit_key` is ever called, so the offsets are
structurally dead on every champion. `CardBoardLeader` is the sole
per-type offset still live, and it is additive with `CardBoardCredit`
at cards.rs:2033 (`credit_board = base + offset`). Probing it is
unmasked: a Leader card's `credit_board` equals `CardBoardCredit +
CardBoardLeader`, and a single-key probe on `CardBoardLeader` moves
only Leader cards' `credit_board`, not the `base` (CardBoardCredit)
that the other 19 keys in this set read. The dedicated gates
(6-14) are measured individually for the same reason: each gate
fires exclusively per type, so probing one gate moves only that
type's prices.

  NOTE ON COUNT: the relay says "the 21 credit keys"; this spec's count
  is 20 (section 9, item 4). The set is the keys whose reader is a
  `w.get` inside a card pricer, with the per-type offsets collapsed into
  CardBoardLeader because of the masking problem above. If the
  coordinator's 21 counts the per-type offsets as separate keys, the
  masking argument still holds and the set stays 20; if it counts
  something else, resolve before the spec is frozen.

================================================================================
3. THE FREEZE (CITATION VERIFIED, CORRECTION NOTED)
================================================================================

The relay cited "multcheck.rs:242" as the freeze site. VERIFIED: the
freeze is the call at multcheck.rs:246
  `eval::candidate_features(s, legal.as_slice(), bot.allow_resign, &pw)`
where `pw = *weights; pw.set(k, pert_val)` (multcheck.rs:244-245). The
freeze is the fourth argument, `freeze: &Weights`, which threads to
linear_features (eval.rs:726) and then to every pricer call at
eval.rs:689-698. Line 242 is the `base_move` line; the freeze call is
at 246 (off by 4). The probe vector pw = champion with ONLY the credit
key k set to c; every other key at the champion value.
CRITICAL CORRECTION TO THE DESIGN SKETCH: the sketch said "phi' is
built with the PROBE vector as the freeze" and "S_d = max - min of
dot(w_probe, phi')". That is WRONG for the credit keys, because the
credit keys are NOT in the dot product: the credit keys have no
linear_feature coordinate (frozen OUT), so dot(w_probe, phi') differs
from dot(champion, phi') ONLY by w_probe[credit] * phi'[credit] = c *
0.0 = 0.0. The probe has NO EFFECT on the dot product at all; the
swing comes ENTIRELY from the pricers re-running under the probe
freeze. (Building phi' with the CHAMPION freeze and dotting the probe
in is also wrong -- phi'[HandPotential] would be the champion's number
and the probe's effect would not be captured.)
The CORRECT formulation:
  phi_c = candidate_features(s, legal, allow_resign, freeze=champion)
  phi_p = candidate_features(s, legal, allow_resign, freeze=pw)
          (pw = champion with k set to c; every other key at champion, so
           only the pricer reads of k change between phi_c and phi_p)
  score_p(m) = dot(pw, phi_p(m))
  S_d = max_m score_p(m) - min_m score_p(m)
The probe's effect is the difference between phi_p and phi_c in the
slots the pricers wrote (HandPotential, etc.), a function of c only
through the pricer's own resolution of k. Definable and p95-spread-shaped.

================================================================================
4. THE FREE PARAMETER c: HOW TO PIN IT, SENSITIVITY, MONOTONICITY
================================================================================

4.1 Monotonicity. Is S_d monotone in c? NO, and the reason is specific
and checkable. For a dedicated gate (say TechBoardCredit), the pricer
branch is (cards.rs:1947-1952):
    let tb = w.get(TechBoardCredit);
    if kind.is_unit() { ... }
    else if tb != 0.0 && board_yields::is_levelled_type(kind) {
        return tb * tech_value(id, st, ix, w, 1.0, late);
    }
tech_value itself does not read TechBoardCredit (it reads ResourceStock,
Science, dev_credit, all frozen at champion), so on the interval where
the branch fires, the pricer output is LINEAR in c and S_d is linear in
|c| (S_d = |c| * R, R = max_i tv_i - min_i tv_i over the firing cards).
THE GATE THRESHOLD: the branch fires only when `tb != 0.0`. At c = 0.0 it
does NOT fire; the pricer falls through to the
`sum_yields(scratch, w, credit) + type_bonus` fallback, which reads
CardRateCredit, not TechBoardCredit, so it is INDEPENDENT of c on this
probe. Hence S_d(c) = |c| * R for c != 0 and S_d(0) = 0: linear in |c|
with a kink at 0, not linear in c.
For the ADDITIVE credits (CardRateCredit, UnitStrengthCredit, etc.) there
is no gate: sum_yields multiplies the rate yield by `credit`
(cards.rs:543) unconditionally, so the pricer output is LINEAR in c
across all c and S_d is linear in c, passing through 0 with no kink.
4.2 Sensitivity to c. Because S_d is linear (or |c|-linear) in c for a
fixed d, p95_d(S_d) is linear (or |c|-linear) in c:
p95_d(|c| * R_d) = |c| * p95_d(R_d). The bound is
  bound = CLAMP_T * T / p95(S_d) = T / (|c| * p95(R))
so the bound scales as 1/|c|: doubling c halves the bound. The sketch's
hope that "if S_d scales linearly in c the whole bound is c-invariant"
is WRONG: linearity in c makes the BOUND scale as 1/c, not invariant, for
both gate and additive credits. The free parameter is NOT free; it sets
the scale of the bound directly.
4.3 How to pin c. Because the bound is 1/c, c must be pinned to a
documented, defensible magnitude. Options, in order of preference:
  (a) c = |champion_value(k)|. The bound then answers "how large can this
      credit get before it commands more than CLAMP_T typical decisions,
      measured at the scale the champion ALREADY uses." The pricers are
      tuned around champ_w; measuring at champ_w keeps the bound
      comparable to every other key's bound (featspread measures the
      spread at the champion's state, not at a probe).
  (b) c = 1.0. A unit probe; defensible but arbitrary (a credit at 0.3
      is measured 3.3x above its operating scale, one at 15.0 is 15x
      below).
  (c) c = default_weight(k). Rejected: a hand-picked prior, and several
      credit keys have default 0.0, which makes c = 0 and the
      measurement undefined.
  RECOMMENDATION: (a), with the champion md5 frozen and recorded in the
  output (the multcheck convention, analysis/multiplier_flips_2026-08-25.txt
  lines 16-22). For keys whose champion value is 0.0 the measurement is
  undefined and the key stays at CLAMP_BLIND; documented, not a defect,
  matching the existing "spread <= 0.0 -> CLAMP_BLIND" fallback at
  weights.rs:2045.
  SENSITIVITY REPORT REQUIRED: for each key, report S_d at c =
  |champ_w|, c = |champ_w|/2, and c = 2*|champ_w|, and confirm the
  predicted scaling (S_d proportional to |c|, hence the bound
  proportional to 1/|c|). If it does not hold (e.g. a gate threshold is
  crossed between the three c values), flag it and report the
  nonlinearity. Cheap check (3x the pricer calls) and the only way to
  catch the gate-threshold nonlinearity of section 4.1 empirically.

================================================================================
5. THE MEASUREMENT, PRECISELY
================================================================================

For each of the 20 credit keys k (section 2) and each player count in
{2,3,4}:
  1. Load the champion for that player count (frozen md5, recorded).
  2. c_k = |champion.get(k)|. If c_k == 0.0, skip (document, CLAMP_BLIND).
  3. Play `games` games (recommend 40, matching featspread's
     analysis/clamp_spread_2026-08-25.txt calibration) at that player
     count, seed 0, threaded, under the champion.
  4. At every decision d with candidate set C (|C| > 1, the same gate
     featspread and multcheck use):
       a. Build phi_c = candidate_features(s, legal, allow_resign,
          freeze=champion). (This is the SAME call featspread makes.)
       b. Build pw = champion with k set to c_k.
       c. Build phi_p = candidate_features(s, legal, allow_resign,
          freeze=pw).
       d. S_d = max_m dot(pw, phi_p(m)) - min_m dot(pw, phi_p(m)).
          (S_d = 0.0 trivially when |C| < 2, but the |C| > 1 gate
          already excludes those, so no special case is needed.)
       e. Also build phi_0 and phi_abs (k set to 0.0 and
          champion.get(k).abs() respectively, same freeze protocol) and
          record the flip indicators and the argmax gap
          G_d = score_p(argmax) - score_p(runner_up) (section 6).
  5. p95_spread(k, players) = p95 over d of S_d (nearest-rank, same
     as featspread's percentile at featspread.rs:105).
  6. bound(k, players) = (CLAMP_T * T_players) / p95_spread(k, players),
     .min(CLAMP_BLIND), where T_players = p95 of the TOTAL spread over
     the SAME run (re-measured, not reused from featspread's
     P95_TOTAL_SPREAD constant at weights.rs:2280). T_players is the
     max-min of dot(champion, phi_c) over C at each d, p95 over d.
     Re-measuring T in the same run is REQUIRED by the spec: featspread's
     P95_TOTAL_SPREAD was measured in a different run with a different
     sample, and dividing a new S_d by a stale T mixes two samples.

COST NOTE (the relay's question 4): step 4c is a FULL re-run of
candidate_features per decision per key -- the pricers (eval.rs:689-698,
each building its Baseline) are the dominant cost, so 4c costs ~ the same
as 4a. Naively that is 20x (plus 2x for the flip-rate phi_0/phi_abs) per
decision. OPTIMIZATION (allowed, must be documented in the output): for
a fixed d, phi_c is shared across all 20 keys (it depends only on the
champion freeze), so 4a runs ONCE per decision and the honest count is
1 + 20 (+2 for the flips) candidate_features calls per decision, not 42.
There is no cheap incremental way to get phi_p from phi_c without
re-running the pricers (they are not linear in their inputs in a way that
supports a delta), so this is the floor. Section 8 gives the wall-clock.

================================================================================
6. WHAT MUST BE REPORTED (LEVER, NOT INFLUENCE)
================================================================================

For each key and player count, the output must include:
  1. p95_spread(k, players)   -- the S_d quantity above.
  2. bound(k, players)         -- CLAMP_T * T / p95_spread, capped.
  3. T_players (re-measured)   -- the p95 total spread for this run.
  4. flip_rate_zero(k)         -- the fraction of decisions where setting
     k to 0.0 (multcheck's ZERO perturbation, multcheck.rs:244) changes
     the champion's argmax. This is the INFLUENCE measure, computed in
     the SAME run: at each decision also compute
     phi_0 = candidate_features(s, legal, allow_resign, freeze=champion
     with k set to 0.0) and check whether argmax(dot(champion, phi_0))
     differs from argmax(dot(champion, phi_c)). The probe-vs-zero
     contrast is the same one multcheck uses, so the numbers are
     directly comparable.
  5. flip_rate_abs(k)          -- the same, with k set to
     champion.get(k).abs() (multcheck's ABS perturbation).
  6. term_nonzero(k)           -- the fraction of decisions where the
     probe changes ANY candidate's score (multcheck.rs:258-264),
     separating "moves the scores but not the argmax" from "moves
     nothing."

The flip rates MUST be printed alongside p95_spread and bound in the
same table, or the spec is not satisfied. The table is emitted in the
SAME shape as featspread's print_clamp_table (featspread.rs:622-646):
a "RUST TABLE -- paste as the body of ..." block, one match arm per key,
[2p, 3p, 4p] cells, so the output splices into
WeightKey::p95_candidate_spread (weights.rs:2166-2188, the credit arms)
directly, replacing the 0.000000 rows.

The REQUIRED CAVEAT (print it in the table header, verbatim):
  "S_d is a LEVER (max-min of the probe-perturbed score over the
   candidate set), not INFLUENCE. A large S_d does not imply the key
   changes the chosen move. Read flip_rate_zero and flip_rate_abs for
   influence; read p95_spread for the scale of the bound."

================================================================================
7. THE RUN (INVOCATION, THREADING, GATES)
================================================================================

A new binary (or a new mode of featspread, the coordinator's call) taking
  <games> <seed> <threads> <champion_dir>
exactly as featspread's non-decisive mode (featspread.rs:648-677) does,
and emitting the section 6 table plus the caveat. The champion_dir holds
the three rust_champion_{2,3,4}p.json, frozen by md5 and recorded in the
output (the multcheck convention, analysis/multiplier_flips_2026-08-25.txt
lines 16-22).

GATES (standing rule; this spec is analysis-only and exempt, the
implementing binary is not):
  cargo clippy --all-targets -- -D warnings   (exit 0)
  cargo test                                  (exit 0)
run from rust/, exit codes reported verbatim. A unit test is REQUIRED
before the binary ships, in the style of multcheck's tests (multcheck.rs:
340+): a synthetic decision whose HandPotential differs by a known amount
under a known probe, asserting S_d equals the expected value and the bound
formula reproduces a hand-computed number. RED-confirmation discipline: the
test must FAIL if the probe is applied to the dot vector instead of the
freeze (the section 3 correction), so it pins the correct formulation.

================================================================================
8. WALL-CLOCK COST
================================================================================

Calibration data (all verified in-tree, from the analysis files):
  - featspread, 40 games, all three player counts: 38.5s total (2p 3.2s,
    3p 11.0s, 4p 24.3s), analysis/clamp_spread_2026-08-25.txt lines
    21-22. This is 1 candidate_features per decision (the plain sample).
  - multcheck, 150 games 3p, 35 keys, 4 threads: 182.2s main pass
    (analysis/multiplier_flips_2026-08-25.txt line 63), ~3.5 s/game
    including its 2 perturbations; 4p ~7.05 s/game (same file, lines
    26-28). (The 60-game classification pass is a fixed ~15-30s; it is
    not part of this design.)

The credit measurement is ~23x the plain featspread candidate_features
cost per decision (1 shared phi_c + 20 per-key phi_p + 2 flip vectors,
section 5; phi_c is shared across keys, so it is NOT 42x). At 40 games:
  - 2p: 3.2s * 23  = ~74s   - 3p: 11.0s * 23 = ~253s   - 4p: 24.3s * 23 = ~559s
  TOTAL, all three counts: ~15 minutes single-core-equivalent; with 4
  threads ~4 minutes wall, bounded by the 4p shard.

This is the trade: ~15 minutes single-core-equivalent (~4 minutes
wall on 4 threads) fills the credit arms in weights.rs:2166-2188
with defensible numbers, retiring the CLAMP_BLIND = 60.0 fallback
(weights.rs:2271) for the whole credit class. If the arms are
mid-climb, the same deferral logic the featspread rerun used
applies: document the deferral and do not build.

================================================================================
9. CITATION VERIFICATION LOG (THE RELAY'S FOUR CITATIONS)
================================================================================

1. "multcheck.rs:242 does it [the freeze]" -- VERIFIED WITH CORRECTION.
   The freeze call is at multcheck.rs:246
   (candidate_features(s, legal, bot.allow_resign, &pw)); line 242 is
   the base_move line (mechanism correct, line off by 4). MORE
   IMPORTANTLY, the sketch's formulation (probe in the dot vector, S_d
   = max-min of dot(w_probe, phi')) is WRONG for the credit keys: they
   have no linear_feature coordinate, so the probe in the dot vector
   changes nothing; the swing comes from re-running the pricers under
   the probe freeze. Section 3 corrects this.
2. "featspread's print_clamp_table (featspread.rs:623-646)" -- VERIFIED.
   The function `print_clamp_table` starts at featspread.rs:622 (doc
   from 604); the table it prints spans 623-646. Correct as a line
   range, off by 1 as a function start. No substantive issue.
3. "T re-measured in the SAME run" -- AGREED; the spec makes it
   required (section 5, step 6). Reusing P95_TOTAL_SPREAD
   (weights.rs:2280) would divide a new sample's S_d by a stale
   sample's T.
4. "the 21 credit keys" -- DISCREPANCY. The tree has 19
   `*Credit`-suffixed keys plus CardBoardLeader (the sole per-type
   board offset still live; `board_credit_key` at cards.rs:597-662
   returns it for Leader only, the other per-type offsets were
   retired): 20 in the set this spec measures. The other per-type
   offsets are not measured because they are dead in the current
   tree (section 2). 21 does not match; flagged, not silently
   resolved. The spec measures 20.

================================================================================
10. IS IT WORTH BUILDING?
================================================================================

YES, with two conditions. It fills 20 zero rows with defensible,
p95-spread-shaped numbers in the same units as every other measured
row, at ~15 minutes single-core-equivalent (section 8), and retires
the CLAMP_BLIND fallback for the credit class.
  (1) c pinned to |champion_value(k)| (section 4.3), with the
      3-point sensitivity check (c/2, c, 2c).
  (2) flip rates (section 6) in the same table as p95_spread and
      bound, or the number is misread as importance.
This is the one class of the four that Section 7 said is
"unmeasured-but-definable"; this spec is the definition. It stands
on its own, not argued into existence by momentum. If the cost is
not worth it against the running arms, defer it; the spec costs nothing to hold.
END OF SPEC.
