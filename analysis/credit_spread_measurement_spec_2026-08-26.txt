# credit_spread_measurement_spec_2026-08-26.txt
#
# SPECIFICATION (not code) for the credit-half spread measurement: a
# p95-slope-shaped quantity for the credit class of zero-spread-row keys
# (Section 7, class 3, rust/src/bots/weighted/weights.rs). READ-ONLY
# spec written 2026-08-26; nothing here has been built or run.
# Citations verified against the tree; Section 9 lists corrections.

================================================================================
1. WHAT IS BEING MEASURED, AND WHY FEATSPREAD CANNOT
================================================================================

The credit keys are the ~20 WeightKeys whose only readers are the card
pricers in rust/src/bots/weighted/cards.rs. They enter the evaluation
NOT through the linear feature vector but INSIDE the frozen-vector
sub-pricing: eval.rs:689-698 computes HandPotential, WonderPotential,
WonderPromise, HandMilPotential, RivalHandPotential, RowUrgency,
RowBargainForgone, RowLastCopy and MyEventThreat by calling the pricers
with the FREEZE vector (the `freeze: &Weights` parameter of
linear_features, eval.rs:612), and each pricer resolves its own credit
weight from that same frozen vector.

Because the credit coefficient is frozen OUT of the dot product, a probe
that moves ONLY the credit key changes nothing in phi' -- the spread of
dot(w, phi') over the candidate set is exactly 0.0 by construction, and
clamp_bound falls back to CLAMP_BLIND (weights.rs:2045). Section 7 class
3 said these are NOT a category error: with the probe in the sub-pricing,
the per-card value DOES swing across the candidate set, in the same units
(eval points) as every measured featspread row.

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
  relative to the argmax gap, the flip rate is zero no matter how large
  the lever. The spec REQUIRES flip rates alongside S_d (section 6), or
  the number will be misread as importance. A key can have a large S_d
  and a near-zero flip rate, and that is a genuine, informative result.

================================================================================
2. THE 20 CREDIT KEYS
================================================================================

From the `Credit =>` declarations in weights.rs (19 with the "Credit"
suffix; the per-type board offsets are the rest). The measurement set is:

  1  CardRateCredit         (cards.rs:2031, :2314; threaded to sum_yields)
  2  UnitStrengthCredit     (cards.rs:544, via sum_yields)
  3  TerritoryCredit        (cards.rs:560, via sum_yields)
  4  BonusCardCredit        (cards.rs:561, via sum_yields)
  5  CardBoardCredit        (cards.rs:2032, the `base` in credit_board)
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
  20 CardBoardLeader        (weights.rs:945; sole per-type offset,
                             cards.rs:2033 credit_board = base + offset)

NOT IN THE SET (documented, so a future reader does not re-derive it):
`board_credit_key` (cards.rs:597-662) returns `Some(WeightKey::
CardBoardLeader)` for Leader type only; every other type returns
`None`. The per-type offset keys `card_board_government` /
`card_board_action` / `card_board_wonder` / `card_board_bonus`
were retired (RETIRED_KEYS) because their dedicated pricer branches
dispatch before `board_credit_key` is ever called, so the offsets
are structurally dead on every champion. `CardBoardLeader` is the
sole per-type offset still live, additive with `CardBoardCredit` at
cards.rs:2033. Probing it is unmasked: a Leader card's `credit_board`
equals `CardBoardCredit + CardBoardLeader`, and a single-key probe
moves only Leader cards' `credit_board`, not the `base`. The
dedicated gates (6-14) are measured individually for the same
reason: each gate fires exclusively per type.

  NOTE ON COUNT: the relay said "21 credit keys"; the coordinator
  confirmed 20 (19 *Credit + CardBoardLeader). Resolved.

================================================================================
3. THE FREEZE (CITATION VERIFIED)
================================================================================

The relay cited "multcheck.rs:242" as the freeze site. VERIFIED: the
freeze is the call at multcheck.rs:242
  `eval::candidate_features(s, legal.as_slice(), bot.allow_resign, &pw)`
where `pw = *weights; pw.set(k, pert_val)` (multcheck.rs:240-241). The
freeze is the fourth argument, `freeze: &Weights`, which threads to
linear_features (eval.rs:726) and then to every pricer call at
eval.rs:689-698. Line 235 is the `base_move` line. The probe vector
pw = champion with ONLY the credit key k set to c; every other key at
the champion value.
CRITICAL CORRECTION TO THE DESIGN SKETCH: the sketch said "S_d =
max - min of dot(w_probe, phi')" with phi' built under the probe
freeze. That is WRONG for the credit keys: they have no
linear_feature coordinate, so dot(w_probe, phi') differs from
dot(champion, phi') ONLY by w_probe[credit] * phi'[credit] = c *
0.0 = 0.0. The probe has NO EFFECT on the dot product; the swing
comes ENTIRELY from the pricers re-running under the probe freeze.
(Building phi' with the CHAMPION freeze and dotting the probe in is
also wrong -- phi'[HandPotential] would be the champion's number.)
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

4.1 Monotonicity. Is S_d monotone in c? NO. For a dedicated gate
(say TechBoardCredit, cards.rs:1947-1952), the branch fires only when
`tb != 0.0`; tech_value does not read TechBoardCredit, so on the
firing interval the pricer output is LINEAR in c and S_d = |c| * R
(R = max_i tv_i - min_i tv_i). THE GATE THRESHOLD: at c = 0.0 the
branch does NOT fire; the pricer falls through to the `sum_yields`
fallback, which reads CardRateCredit, not TechBoardCredit, so it is
INDEPENDENT of c. Hence S_d(c) = |c| * R for c != 0, S_d(0) = 0:
linear in |c| with a kink at 0. For the ADDITIVE credits
(CardRateCredit, etc.) there is no gate: sum_yields multiplies the
rate yield by `credit` (cards.rs:543) unconditionally, so S_d is
linear in c across all c, passing through 0 with no kink.
4.2 Sensitivity to c, and the normalization that removes it. Because S_d
is |c|-linear in c for a fixed d, p95_d(S_d) is |c|-linear in c:
p95_d(|c| * R_d) = |c| * p95_d(R_d). Read the RAW S_d as the bound's
denominator and the bound scales as 1/|c|: doubling c halves it.

That is an artifact of not normalizing, not a property of the game. The
quantity to measure is the per-key SLOPE

    s_k = S_d(c) / |c|

which is what |c|-linearity says exists: a pure function of state and
legal moves, with no weight in it. The bound is then

    bound = min(CLAMP_BLIND, CLAMP_T * T / p95(firing s_k))

and IS c-invariant. The linearity finding of 4.1 is precisely what makes
it invariant; the 1/|c| scaling above is what is left when the division
by |c| is skipped.

THIS IS NOT COSMETIC. It is what makes a credit row commensurable with
the other 169 rows at all. For a linear-feature key, spread_k = max-min
of the phi_k coordinate over the candidate set: a state+move property
with no weight in it, so the bound answers "at what weight could this key
alone swing T points". s_k is the same shape of quantity for a credit
key. A bound built from an un-normalized S_d would carry the probe
magnitude inside it and would not mean the same thing as the rest of the
table, even though it would paste into the same column.
4.3 How to pin c. Under the 4.2 normalization any nonzero c gives the
same bound wherever S_d is linear, so c is a numerical-conditioning
choice, not a modelling one. USE c = 1.0, FIXED, FOR EVERY KEY, recorded
in the output header alongside the frozen champion md5 (the multcheck
convention, analysis/multiplier_flips_2026-08-25.txt lines 16-22).

  REJECTED: c = |champion_value(k)|. It was this spec's own earlier
  recommendation and it is wrong twice over. First, under normalization
  it buys nothing, since the c cancels. Second, it is undefined exactly
  where it is needed: measured against the champion carried on
  2026-08-26, TechBoardCredit and AggressionBoardCredit both sit at
  weight EXACTLY 0.0000, giving c = 0, S_d = 0 and no bound at all --
  and four more sit under 0.05 (TacticBoardCredit 0.0072,
  RestrictedResourceCredit 0.0192, UnitStrengthCredit 0.0249,
  PactBoardCredit 0.0320). Read through the un-normalized 1/|c| rule
  those six get the LOOSEST rails in the table. That is backwards for a
  safety rail: it is most permissive precisely where the climb has
  already driven the key to nothing, which is where a random walk on
  noise is most likely and a wide clamp is least affordable. A rail must
  not be a function of how much the incumbent happens to use the key.

  REJECTED: c = default_weight(k). A hand-picked prior, and several
  credit keys default to 0.0.

  LINEARITY TEST REQUIRED, PER KEY -- this is the surviving reason to
  vary c, and it tests rather than assumes. Measure S_d at c = 1.0 and
  c = 2.0 and check S_d doubles within tolerance.
    - Doubles: the key is in a linear segment, emit s_k = S_d(1.0) and
      its bound.
    - Does not double: a gate threshold lies between the two probes
      (section 4.1). The key is GATED; report both raw readings in a
      separate section and emit NO normalized bound for it, leaving it
      at CLAMP_BLIND. Do not emit a slope that cannot be defended.
  If most keys come back non-linear, stop and report that: it is a
  finding about the credit pricers, not a failed measurement.

================================================================================
5. THE MEASUREMENT, PRECISELY
================================================================================

For each of the 20 credit keys k (section 2) and each player count in
{2,3,4}:
  1. Load the champion for that player count (frozen md5, recorded).
  2. c = 1.0, FIXED (section 4.3). No per-key skip: normalization makes
     the bound c-invariant, so even keys at champion weight 0.0 get a
     bound (the slope is a property of state and legal moves, not of the
     incumbent's weight).
  3. Play `games` games (recommend 40, matching featspread's
     analysis/clamp_spread_2026-08-25.txt calibration) at that player
     count, seed 0, threaded, under the champion.
  4. At every decision d with candidate set C (|C| > 1, the same gate
     featspread and multcheck use):
       a. Build phi_c = candidate_features(s, legal, allow_resign,
          freeze=champion). (This is the SAME call featspread makes.)
       b. Build pw = champion with k set to c = 1.0.
       c. Build phi_p = candidate_features(s, legal, allow_resign,
          freeze=pw).
       d. S_d = max_m dot(pw, phi_p(m)) - min_m dot(pw, phi_p(m)).
          (S_d = 0.0 trivially when |C| < 2, but the |C| > 1 gate
          already excludes those, so no special case is needed.)
       e. Build pw2 = champion with k set to c = 2.0, phi_p2 with the
          same freeze protocol, S_d(2.0) the same way. Linearity test
          (section 4.3): S_d(2.0) should be ~2 * S_d(1.0) within
          tolerance. If it is not, the key is GATED (a gate threshold
          lies between the two probes); report both raw readings in a
          separate section and emit NO normalized bound for it (it
          stays at CLAMP_BLIND).
       f. Also build phi_0 and phi_abs (k set to 0.0 and
          champion.get(k).abs() respectively, same freeze protocol) and
          record the flip indicators and the argmax gap
          G_d = score_p(argmax) - score_p(runner_up) (section 6).
  5. s_k(d) = S_d(1.0) / 1.0 = S_d(1.0). For linear keys this is the
     per-key slope (section 4.2), a state+move quantity with no weight
     in it. p95_slope(k, players) = p95 over d of s_k(d) (nearest-rank,
     same as featspread's percentile at featspread.rs:105), over the
     FIRING decisions only (the same gate featspread uses for
     p95_spread_firing, featspread.rs:242).
  6. bound(k, players) = (CLAMP_T * T_players) / p95_slope(k, players),
     .min(CLAMP_BLIND), where T_players = p95 of the TOTAL spread over
     the SAME run (re-measured, not reused from featspread's
     P95_TOTAL_SPREAD constant at weights.rs:2280). T_players is the
     max-min of dot(champion, phi_c) over C at each d, p95 over d.
     Re-measuring T in the same run is REQUIRED by the spec: featspread's
     P95_TOTAL_SPREAD was measured in a different run with a different
     sample, and dividing a new S_d by a stale T mixes two samples.
     GATED keys (failed the 4e linearity test) emit no bound; they stay
     at CLAMP_BLIND.

COST NOTE (the relay's question 4): step 4c is a FULL re-run of
candidate_features per decision per key -- the pricers (eval.rs:689-698,
each building its Baseline) are the dominant cost, so 4c costs ~ the same
as 4a. With the linearity test (step 4e, c=2.0) the honest count is
1 + 20 + 20 (linearity) + 2 (flip vectors) = 43 candidate_features calls
per decision. OPTIMIZATION (allowed, must be documented in the
output): for a fixed d, phi_c is shared across all 20 keys (it depends
only on the champion freeze), so 4a runs ONCE per decision. The
linearity phi_p2 (c=2.0) is shared across keys too, so it is 1 pass,
not 20: the true count is 1 + 20 + 1 + 2 = 24 candidate_features calls
per decision. There is no cheap incremental way to get phi_p from phi_c
without re-running the pricers (they are not linear in their inputs in
a way that supports a delta), so this is the floor. Section 8 gives the
wall-clock.

================================================================================
6. WHAT MUST BE REPORTED (LEVER, NOT INFLUENCE)
================================================================================

For each key and player count, the output must include:
  1. p95_slope(k, players)     -- the normalized slope s_k = S_d(c)/|c|
                                  (section 4.2), p95 over firing decisions.
  2. bound(k, players)         -- CLAMP_T * T / p95_slope, capped at
                                  CLAMP_BLIND. GATED keys (failed the
                                  linearity test) emit no bound; report
                                  both raw S_d(1.0) and S_d(2.0) readings
                                  in a separate section instead.
  3. T_players (re-measured)   -- the p95 total spread for this run.
  4. flip_rate_zero(k)         -- the fraction of decisions where
     setting k to 0.0 (multcheck's ZERO perturbation, multcheck.rs:240)
     changes the champion's argmax. The INFLUENCE measure, computed in
     the SAME run: at each decision also compute
     phi_0 = candidate_features(s, legal, allow_resign, freeze=champion
     with k set to 0.0) and check whether argmax(dot(champion, phi_0))
     differs from argmax(dot(champion, phi_c)).
  5. flip_rate_abs(k)          -- the same, with k set to
     champion.get(k).abs() (multcheck's ABS perturbation).
  6. term_nonzero(k)           -- the fraction of decisions where the
     probe changes ANY candidate's score (multcheck.rs:258-264).

The flip rates MUST be printed alongside p95_slope and bound in the
same table. The table is emitted in the SAME shape as featspread's
print_clamp_table (featspread.rs:622-646): one match arm per key,
[2p, 3p, 4p] cells, splicing into WeightKey::p95_candidate_spread
(weights.rs:2166-2188), replacing the 0.000000 rows.

The REQUIRED CAVEAT (print it in the table header, verbatim):
  "S_d is a LEVER (max-min of the probe-perturbed score over the
   candidate set), not INFLUENCE. A large S_d does not imply the key
   changes the chosen move. Read flip_rate_zero and flip_rate_abs for
   influence; read p95_slope for the scale of the bound."

================================================================================
7. THE RUN (INVOCATION, THREADING, GATES)
================================================================================

A new binary (or a new mode of featspread) taking
  <games> <seed> <threads> <champion_dir>
as featspread's non-decisive mode (featspread.rs:648-677), emitting
the section 6 table plus the caveat. champion_dir holds
rust_champion_{2,3,4}p.json, frozen by md5 (the multcheck convention,
analysis/multiplier_flips_2026-08-25.txt lines 16-22).

GATES (this spec is analysis-only and exempt; the implementing
binary is not): cargo clippy --all-targets -- -D warnings (exit 0),
cargo test (exit 0), from rust/, exit codes reported verbatim. A
unit test is REQUIRED before the binary ships: a synthetic decision
whose HandPotential differs by a known amount under a known probe,
asserting S_d equals the expected value and the bound formula
reproduces a hand-computed number. The test must FAIL if the probe
is applied to the dot vector instead of the freeze (section 3), so
it pins the correct formulation.

================================================================================
8. WALL-CLOCK COST
================================================================================

Calibration (verified in-tree): featspread, 40 games: 38.5s total
(2p 3.2s, 3p 11.0s, 4p 24.3s), analysis/clamp_spread_2026-08-25.txt
lines 21-22, 1 candidate_features per decision. multcheck, 150 games
3p 35 keys 4 threads: 182.2s main pass, ~3.5 s/game, 4p ~7.05 s/game
(analysis/multiplier_flips_2026-08-25.txt lines 26-28, 63).

The credit measurement is ~24x the plain featspread candidate_features
cost per decision (1 shared phi_c + 20 per-key phi_p + 1 shared
linearity phi_p2 + 2 flip vectors, section 5; phi_c and phi_p2 are
shared across keys). At 40 games:
  - 2p: 3.2s * 24  = ~77s   - 3p: 11.0s * 24 = ~264s   - 4p: 24.3s * 24 = ~583s
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

1. "multcheck.rs:242 does it [the freeze]" -- VERIFIED CORRECT.
   The freeze call is at multcheck.rs:242
   (candidate_features(s, legal, bot.allow_resign, &pw)); line 235
   is the base_move line. MORE IMPORTANTLY, the sketch's
   formulation (probe in the dot vector, S_d = max-min of
   dot(w_probe, phi')) is WRONG for the credit keys: they have no
   linear_feature coordinate, so the probe in the dot vector
   changes nothing; the swing comes from re-running the pricers
   under the probe freeze. Section 3 corrects this.
2. "featspread's print_clamp_table (featspread.rs:623-646)" --
   VERIFIED. Function starts at featspread.rs:622; table spans
   623-646. Correct as a line range.
3. "T re-measured in the SAME run" -- AGREED; required (section 5,
   step 6). Reusing P95_TOTAL_SPREAD (weights.rs:2280) would mix
   two samples.
4. "the 21 credit keys" -- RESOLVED: 20 (coordinator confirmed).
   19 *Credit-suffixed keys + CardBoardLeader. The other per-type
   offsets are dead in the current tree (section 2).

================================================================================
10. IS IT WORTH BUILDING?
================================================================================

YES, with two conditions. It fills 20 zero rows with defensible
p95-slope numbers in the same units as every other measured row, at
~15 minutes single-core-equivalent (section 8), and retires the
CLAMP_BLIND fallback for the credit class.
  (1) the bound built from the NORMALIZED slope s_k = S_d(c)/|c|
      (section 4.2) at a fixed probe c = 1.0 (section 4.3), with the
      per-key linearity test at c = 1.0 and c = 2.0, and no bound
      emitted for any key that fails it.
  (2) flip rates (section 6) in the same table as p95_slope and
      bound, or the number is misread as importance.
This is the one class of the four that Section 7 said is
"unmeasured-but-definable"; this spec is the definition. It stands on
its own, not argued into existence by momentum. If the cost is not
worth it against the running arms, defer it; the spec costs nothing to hold.
END OF SPEC.
