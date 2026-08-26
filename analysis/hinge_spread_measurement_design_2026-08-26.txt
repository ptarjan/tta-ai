Design: measuring spread bounds for the key classes featspread cannot reach
===========================================================================
2026-08-26. Read-only design analysis (no code changes). Clone: /Users/pt/tta-scratch.
Follows analysis/clamped_keys_2026-08-26.txt (the 32 pre-existing all-zero arms).

1. WHY THE CLASSES COME OUT ZERO -- PRECISELY
---------------------------------------------
The relay's three candidate explanations are not co-equal; the code decides
which one applies to which class. All three mechanisms exist in this tree.

M1 -- the perturbation point does not reach them (frozen vector).
    featspread scores every candidate with eval::candidate_features (eval.rs:
    708-733), which builds one linear_features(trial, idx, Some(&ctx), freeze)
    per candidate (eval.rs:726) with freeze pinned to the CHAMPION (featspread
    .rs:193, 201). Inside linear_features, eleven slots are filled by
    functions that take the FULL vector and reprice their internals through
    it -- the section's own top doc comment calls them out (eval.rs:575-600):

        out[WeightKey::HandPotential as usize] = cards::hand_potential(state, idx, freeze);   // 677
        out[WeightKey::WonderPotential as usize] = cards::wonder_potential(state, idx, freeze); // 678
        ... (WonderPromise 679, HandMilPotential 680, RivalHandPotential 684,
             RowUrgency/RowBargainForgone 685-687, RowLastCopy 688, MyEventThreat 689)

    The section's own words (eval.rs:580-593): "eleven coordinates ... are
    NOT linear in w in evaluate itself -- each is priced by calling a function
    that takes the FULL weight vector and reprices its own internal sub-terms
    through it ... the true evaluate(state, w) is BILINEAR in w on these
    eleven dimensions, not expressible as w . f(state) for any single fixed
    f. linear_features resolves this by freezing those eleven sub-computations
    at a caller-supplied freeze vector." Perturbing w outside the dot product
    never moves these eleven slots: they are constants of the candidate set.
    (All eleven currently carry MEASURED spreads because featspread's freeze
    is the champion and its INTERNAL sub-pricing happens to swing; but a
    perturbation of w cannot change them -- see the implication in (3).)
    The RATE_KEYS scale is the same mechanism one level up: linear_features
    computes hz = horizon::rate_multiplier(state, freeze, n) ONCE (eval.rs:
    622) and multiplies every RATE_KEYS slot by it (eval.rs:625-627). freeze
    is the champion, so perturbing w moves no rate slot either.
    Verdict: M1 is the mechanism for the rate-key RateHorizon (its own slot is
    never written by features() -- no f.set(WeightKey::RateHorizon) in
    features.rs -- and the hz it controls is frozen), and it is the
    documented reason linear_features is a LINEAR APPROXIMATION of evaluate,
    not a representation of it.

M2 -- the contribution is computed downstream of the perturbation point, in a
    function that reads the weight DIRECTLY, not from the phi vector.
    The credit keys (TechBoardCredit, CardBoardCredit, UnitTechCredit, ...)
    are read as w.get(...) INSIDE the per-card pricers: cards.rs:1947-2019
    (tb = w.get(WeightKey::TechBoardCredit); return uc * tech_value(...),
    tb * tech_value(...), gb * gov_value(...), ab * action_value(...),
    wb * sum_board_triples(...), tc * tactic_value(...), ac * ..., wac * ...,
    pc * ...), plus the yield_marginal route (cards.rs:722-734) and the
    ring-fenced Priced::RestrictedResources arm (cards.rs:765-772). These
    pricers are called by the hand/wonder/row functions that fill M1's
    eleven slots AND by the search path's evaluate (eval.rs:330-394), so a
    credit key genuinely moves candidate scores in evaluate -- but its
    contribution never appears in ANY slot of the phi vector linear_features
    returns. features() (w=None, priced_only=false at eval.rs:621) computes
    the raw board-read vector and never reads a credit weight. No filter
    excludes them; they are simply absent from the vector's domain.
    multcheck.rs states this class in its own header (lines 1-13): "A
    WeightKey variant ... that acts purely as MULTIPLIERS inside other
    features' computation ... and therefore never occupy a slot in the linear
    feature vector phi that eval::linear_features/candidate_features returns.
    bin/featspread.rs's candidate-set SPREAD instrument is structurally blind
    to this family: a key with no phi slot has spread == 0.0 by construction
    at every decision, in every player count, no matter how much it changes
    real move ranking through the keys it multiplies into."
    Verdict: M2 is the mechanism for the 21 credit/discount/multiplier keys.

M3 -- the key is never written into phi at all; its only live read is inside
    feature_marginal, which linear_features never calls.
    The hinge keys (ScienceRateTrailing, CultureRateTrailing, FoodStockNeeded,
    ResourceStockNeeded, ScienceNeeded, FreeWorkersNeeded) are read exactly
    once in production: w.get(key.trailing()) at rivals.rs:977 and
    w.get(key.needed()) at rivals.rs:996, inside feature_marginal (rivals.rs:
    950-1035), multiplied by trailing_fraction/need_fraction in [0,1].
    feature_marginal's callers are the CARD PRICERS (cards.rs:724, 730, 770,
    1289), i.e. the same M2/M1 call graph -- and the registry pins the
    provenance: registry.rs:310-311 names both *Trailing keys
    "rivals.rs standing-hinge marginal", and features() never sets them (no
    f.set arm). linear_features (eval.rs:612-692) calls features() and the
    eleven freeze-priced functions; it never calls feature_marginal, so a
    hinge key's slot stays at the vec![0.0; ...] initial value (eval.rs:613)
    at every candidate, in every decision.
    Verdict: M3 is the mechanism for the 6 hinge keys.

    So: it is NOT one uniform "perturbing a frozen vector never moves them".
    Hinges: never written (M3, a domain gap, not a freeze gap).
    Credits: written by NO code path -- absent from phi by construction (M2).
    RateHorizon: the hz it controls is frozen at the champion (M1).
    The distinction matters downstream because the FIX for each is a
    different perturbation, and because M2/M3 keys are invisible even to
    featspread's P95_TOTAL_SPREAD (the dot at featspread.rs:201 dots w against
    phi, and phi carries none of their contribution) -- total_spread is
    therefore an INCOMPLETE measure of one decision's worth while any credit
    or hinge weight is nonzero.

2. WHAT EXISTING INSTRUMENTS COULD BE BENT, AND WHAT EACH IS MISSING
--------------------------------------------------------------------
(a) multcheck (rust/src/bin/multcheck.rs) is the closest existing instrument.
    Its measurement (header lines 46-64, code at 210-273): at every real 3p
    decision, recompute every candidate's score under a copy of the champion
    with ONE key perturbed (ZERO, and ABS = champion_value.abs()), using the
    perturbed vector as BOTH freeze and dot weight (play_shard, multcheck
    .rs:240-243: pw.set(k, pert_val); candidate_features(s, legal, allow,
    &pw); dot(&pw, f)), and count argmax FLIPS. Its runtime classification
    (lines 15-44, 164-202) derives the multiplier-only family exactly as the
    task's fallback method prescribes: keys never appearing as
    f.set(WeightKey::...) in features.rs (include_str! source scan, lines
    87-94) AND whose phi spread stayed 0.0 across a whole self-play sample
    (lines 164-202) -- "p95_candidate_spread == [0,0,0] AND not set in
    features.rs, just computed from the live candidate set".
    What it measures: flip rates and term_nonzero (the decision-fraction
    where zeroing the key changed ANY candidate's score, lines 254-261).
    What it is missing for a SPREAD BOUND:
      * a MAGNITUDE per decision. A flip is binary; the bound formula needs
        the p95 of a per-decision score range, and multcheck never computes
        max-min of the perturbed scores.
      * per (key, PLAYER COUNT) rows: it runs ONE player count per
        invocation (usage line 72: <games> <seed> <threads> <champion_json>
        <players 2|3|4>), while clamp_bound is per (key, count) and
        "never collapsed across counts" (weights.rs:1905-1910).
      * an EMIT path that splices into p95_candidate_spread + P95_TOTAL_SPREAD
        as compilable Rust (featspread.rs:604-646 has it; multcheck has only
        a text table).
      * a total_spread numerator consistent with the perturbed scoring.
    The 2026-08-24 analysis files already carry partial evidence
    (analysis/multcheck_raw_4p_2026-08-24.txt:22: culture_rate_trailing
    term_nonzero_rate=0.2045 -- the hinge fires on ~1 in 5 decisions, so it
    is not dead, just unmeasured).

(b) featspread (rust/src/bin/featspread.rs) has the per-count loop, the
    P95_TOTAL_SPREAD, the emit path, and the nearest-rank percentile
    (featspread.rs:105-113) -- but its scoring primitive (candidate_features
    + dot, lines 193-201) is exactly the M1/M2/M3-blind one. Re-running it
    re-emits zeros for all 32 keys forever (weights.rs:2101-2120 document the
    convention: a 0.0 arm is the "unmeasured" state).

3. THE DESIGN: A PER-KEY COUNTERFACTUAL SPREAD, ONE BINARY
-----------------------------------------------------------
Core quantity. For a key k that does not occupy a phi slot (M2/M3), define,
at each real decision d (candidates.len() > 1, the same gate featspread
uses), a per-decision score range under a SINGLE-KEY perturbation delta:

    S_d(k, delta) = max_i dot(w^delta_k, phi_i) - min_i dot(w^delta_k, phi_i)

where w^delta_k is the champion with ONLY k set to delta, phi_i are the
candidate feature vectors, and dot is eval::dot (eval.rs:740-746). The bound
then reuses clamp_bound's own formula with S's p95 in the denominator:

    bound(k, players) = min(CLAMP_BLIND, CLAMP_T * T_players / p95_d S_d(k, delta))

where T_players is the per-count total decision spread. Two sub-decisions:

  (i) NUMERATOR T_players must be re-measured under the SAME perturbed
      scoring, or the ratio compares two different samples (the exact
      discipline weights.rs:1924-1928 enforces for the current table, and the
      107x cross-count error featspread.rs:43-48 documents). For M1 keys
      (RateHorizon) and the eleven M1 slots, the numerator is also
      champion-only today; using T from the baseline (unperturbed) champion
      scoring is defensible as a first cut because the clamp is a CEILING
      (weights.rs:2138-2142) and an under-estimated T only loosens the bound
      toward CLAMP_BLIND, never tightens past it. State this in the report.

  (ii) PERTURBATION delta. Options, in order of preference:
      A. delta = champion value (baseline vs champion-own is degenerate for
         an M2/M3 key: phi is invariant under k, so S_d at delta=champ_w is
         0 for a pure M3 key -- the contribution lives in the pricer's
         internals, which candidate_features' phi never exposes). REJECTED
         as the sole perturbation: it measures nothing for M3 keys.
      B. delta = +c (a fixed positive probe, c chosen so the probe's
         induced swing is comparable in scale to the champion's own |w_k|,
         e.g. c = max(|champ_w(k)|, 1.0)). Measures the key's LEVER ARM: how
         far one unit of k moves the candidate scores at decision d.
         p95_d of S_d(k, c) is then "the typical decision's score range per
         unit of k", and the bound formula reads exactly like the measured
         keys' bound: "k may command at most CLAMP_T whole typical decisions
         on its own". This is the one that makes the bound MEANINGFUL.
      C. multcheck's existing pair (ZERO / ABS) is the decision-relevance
         instrument (flips), not a magnitude instrument; keep it as the
         companion check (a key with huge S but ~0 flip rate is lever
         without influence -- see (5) below).
      The binary should run B per decision and accumulate S_d(k, B) per
      (key, count), exactly parallel to featspread's KeyAgg (featspread
      .rs:124-145).

  (iii) WHICH DECISIONS: the same champion self-play sample featspread uses
      (base_seed + game_index, per count, MOVE_CAP) so the rows are
      comparable with the 36c7c06 table; the move that advances each game
      stays WeightedBot::choose on the UNperturbed champion (multcheck
      .rs:265, featspread.rs:222) -- perturbations price, they never drive.

Per-class plan:
  - Hinge keys (M3, 6 keys: ScienceRateTrailing, CultureRateTrailing,
    FoodStockNeeded, ResourceStockNeeded, ScienceNeeded, FreeWorkersNeeded):
    S_d is nonzero only when the hinge fires (trailing_fraction/need_fraction
    > 0 on at least one candidate -- rivals.rs:898-901, 944-947), so the
    firing-only p95 (featspread.rs:134-144) is the right statistic and
    fire_rate becomes a first-class reported column (multcheck's
    term_nonzero_rate is the existing proxy: 0.2045 for culture_rate_
    trailing at 4p). A hinge key that never fires in the sample gets 0.0
    again -- correctly, because a key that never fires cannot move a
    decision's argmax at any weight, and CLAMP_BLIND is then the honest
    bound (nothing to tighten).
  - Credit keys (M2, 21 keys): same S_d under probe B. Their contribution
    reaches phi indirectly through the eleven M1 slots (hand_potential et
    al. reprice with the perturbed vector when the perturbed vector is
    passed as freeze -- see the gap below), so S_d captures exactly the
    candidate-score movement the key causes.
  - RateHorizon (M1): S_d under probe B on RateHorizon itself; hz is
    recomputed from the perturbed freeze (eval.rs:622), so the probe
    genuinely moves the four RATE_KEYS slots.
  - The eleven M1 slots themselves (HandPotential, WonderPotential, ...):
    already carry measured spreads in the 36c7c06 table (their freeze-priced
    values swing between candidates even though w-perturbations cannot move
    them); they are NOT among the 32 and need no new rows. Their BOUND
    question is a separate, tighter one (see (5)).
  - Rival-context keys (RivalDesire, RivalTakeShare, RivalWonders,
    RivalScienceRate): written by features() through ctx (not M1/M2/M3);
    their zero rows are a SAMPLE property. A plain featspread re-run with
    more games at each count is the fix; no design change needed. If a
    larger sample still shows zero, run featspread decisive mode
    (featspread.rs:318-449, 3p only) to separate "structurally dead"
    (zero_frac 1.0 AND mean_abs_level ~ 0) from "alive but quiet"
    (zero_frac 1.0 with a large level -- the BestMine/BestFarm hypothesis
    the decisive mode exists to check, featspread.rs:318-330).

THE ONE GAP THAT MUST BE CLOSED FOR M2 TO WORK: candidate_features currently
takes a SINGLE freeze and uses it for both the phi construction and (in the
caller) the dot. For a credit key's S_d the probe must reach the pricer
internals, i.e. the perturbed vector must be passed as the freeze that
linear_features uses for the eleven M1 slots (eval.rs:677-689) -- which is
exactly what multcheck already does (pw as freeze, multcheck.rs:242), and it
is exactly why multcheck's flips are real. The new binary must do the same:
build phi_i once with the CHAMPION freeze (the baseline), and per (k, probe)
build phi'_i with the PROBE freeze and dot the PROBE w against phi'_i.
(Do NOT dot probe-w against champion-phi: that is M1's frozen-approximation
and would measure the lever through the wrong phi.) Cost: one extra
candidate_features call per (key, probe) per decision -- multcheck pays the
same cost per (key, perturbation) and runs it threaded (multcheck.rs:307-
327); with 27 probe keys (21 credits + 6 hinges; RateHorizon optional) and
a probe pair, that is ~2x multcheck's per-decision work, which multcheck's
threading already absorbs.

EMIT. Mirror featspread's print_clamp_table (featspread.rs:623-646): print
the regenerated p95_candidate_spread arms (measured keys carry their 36c7c06
values verbatim; the 27 new arms carry the probe-S p95 values) plus the
P95_TOTAL_SPREAD line, as compilable Rust to splice into weights.rs -- no
hand transcription (weights.rs:1927-1928). Keep the per-(key, count) shape;
do not collapse across counts (weights.rs:1905-1910).

4. HOW THE BOUND FORMULA AND EMIT WOULD SPICE -- SUMMARY
---------------------------------------------------------
- Denominator: p95 (nearest-rank, firing-only) of S_d(k, probe) over the
  champion's own self-play decisions at that count.
- Numerator: P95_TOTAL_SPREAD[players] from the SAME run (re-measured, not
  carried, so the ratio compares one sample).
- Cap: CLAMP_BLIND (60.0) unchanged; the new rows can only tighten.
- Fallback: a key with zero firing decisions in the sample emits 0.0 again
  (the documented unmeasured state) and keeps CLAMP_BLIND -- same contract
  as today, no invented numbers (weights.rs:2101-2120).
- DOMINATES interactions (climb.rs:219-225): a tightened dominated key's
  bound is min(own, dominators) -- unchanged; a newly-measured dominator can
  only tighten further.

5. HONEST LIMITS OF THIS DESIGN (state them in any report)
-----------------------------------------------------------
- S_d measures LEVER, not INFLUENCE. A key can swing candidate scores a lot
  without ever flipping the argmax (every candidate moves together), or flip
  often with a tiny swing (a near-tie it tips). The bound's own doc
  (weights.rs:1868-1881) frames the rail in terms of "how far the feature
  swings between the moves on offer" -- lever is the faithful reading -- but
  the companion metric must be multcheck's flip/term_nonzero rates, reported
  side by side, so a row can be read as "big lever, no influence" or the
  reverse. The 2026-08-24 files already show the pair diverging (culture_
  rate_trailing: term_nonzero 0.2045 at 4p vs flip_rate_zero 0.000073,
  analysis/multcheck_raw_4p_2026-08-24.txt:22).
- The probe magnitude c is a CHOICE, not a measurement, and enters the
  denominator linearly (S_d scales with |delta|). Any c is defensible for a
  RAIL (the ratio bound(k) is proportional to 1/c, and the cap keeps the
  answer inside [tightest, 60.0]), but the emitted table is only comparable
  across c values if c is fixed and documented in the arm comment, exactly
  like CLAMP_T is documented (weights.rs:2125-2136). This is the one new
  "free parameter" the design introduces; it should be named, justified, and
  pinned by a test the way CLAMP_T is.
- Hinge keys at CLAMP_BLIND (like science_rate_trailing at 60.0 in the 2p
  log) stay at CLAMP_BLIND after this design IF they fire rarely enough that
  p95 S_d is tiny -- in which case the measured bound is LOOSER than 60.0 and
  the cap binds, i.e. the design confirms the current behaviour is the
  ceiling, not a measurement. That is a valid outcome and should be reported
  as such, not papered over.
- The eleven M1 slots' own bounds (already measured) are a DIFFERENT
  question: their phi is freeze-priced, so their "spread" is the champion's
  internal swing, not a perturbation lever; tightening them would mean
  probing the sub-weights inside hand_potential et al., which is a second
  design, out of scope here.

6. EXACT SHAPE OF THE DELIVERABLE BINARY (spec, no code)
---------------------------------------------------------
Name: a new bin (suggested: hinge spread, i.e. a featspread sibling), or a
new MODE of featspread (suggested: better -- reuses PLAYER_COUNTS, load_
weights, MOVE_CAP, the percentiles, the emit; adds a scoring path).
Invocation (mode): featspread <games> <seed> <champion_dir> hinges
  - games/seed/champion_dir exactly as today (featspread.rs:82-100).
  - Per count in {2,3,4}: champion self-play, same gate (candidates.len() >
    1), baseline phi from candidate_features(state, legal, allow, champ);
    per probe key k (the 21 M2 + 6 M3 keys, derived at RUNTIME by the
    multcheck classification: not_in_fset AND phi-spread 0.0, multcheck
    .rs:15-44, 293-301 -- never typed as literals, per the repo rule at
    multcheck.rs:17-19): probe vector = champ with k := c (the documented
    probe magnitude), phi' from candidate_features(state, legal, allow,
    probe), S_d = max-min of dot(probe, phi') over candidates; accumulate
    firing-only S_d, fire_rate, and per-count total spread T (max-min of
    dot(probe, phi') with probe := champ, i.e. the baseline, for the
    numerator) .
  - Report: per (key, count): champ_w, fire_rate, p95 S_d, bound =
    min(CLAMP_BLIND, CLAMP_T * T / p95 S_d); plus the multcheck-style
    flip/term_nonzero companion (reuse play_shard, multcheck.rs:210-273, or
    its numbers from a sibling run).
  - Emit: regenerated p95_candidate_spread arms + P95_TOTAL_SPREAD as
    compilable Rust (featspread.rs:623-646 shape), measured rows carried
    verbatim, probe rows filled, unmeasured rows 0.0.
- Tests to pin (mirroring featspread's own test style, featspread.rs:724-
  776): percentile reuse; parse-args shape; S_d == 0 for a probe key on a
  decision where the hinge provably cannot fire (a level/leading state,
  rivals.rs:898-899); S_d > 0 on a contrived trailing state; emit output
  round-trips through parse_weights' table expectations.

RULES OBSERVED
--------------
Read-only: no edits under rust/src, no build, no git in /Users/pt or
/Users/pt/tta-ai. All file:line references verified in /Users/pt/tta-scratch.


======================================================================
SECTION 7 -- REBUTTAL OF QUESTION (2): IS THE p95 ROW A CATEGORY ERROR?
(Appended 2026-08-26 in response to the coordinator's question (2),
which the earlier sections skipped. This section does NOT revise or
supersede Sections 1-6; it answers the question they were written to
avoid, and where the answer lands on the Section 6 design is said
plainly at the end.)
======================================================================

7.1 What the clamp actually operates on
---------------------------------------
The clamp operates on the WEIGHT COEFFICIENT, never on a phi entry and
never on a contribution:

  climb.rs:235-242
    fn clamp(x: f64, key: WeightKey, players: u8) -> f64 {
        let bound = effective_bound(key, players);
        if x.abs() > bound {
            bound.copysign(x)
        } else {
            x
        }
    }

  climb.rs:219-225   effective_bound = min(own clamp_bound, dominators')
  climb.rs:455,493   applied at both mutation operators
  climb.rs:267-280   repair_to_bounds pulls an incumbent in at load

So "science_rate_trailing is clamped at 60.0" is true in the only sense
the machinery has: the search is forbidden from pushing that coordinate
past +/-60.0, and a coordinate sitting there is pinned. The p95 row is
not what is clamped. The question is what the row is FOR, and the honest
answer is that it is not a single thing. The bound formula

  bound(k, players) = min(CLAMP_BLIND, CLAMP_T * T_players / spread_k)

  weights.rs:1911-1918, with CLAMP_BLIND = 60.0 (weights.rs:2143)

uses the key's phi spread as a UNIT CONVERSION: T (the total-score
swing the instrument observed across the candidate set) divided by
"one unit of this weight's feature" gives "how far may the coefficient
walk before one unit of it can move the total by one total-swing". That
conversion is only meaningful for weights whose contribution to the
score IS w_k * phi_k. For the zero-row classes, that is not how they
enter the score:

  * Hinge keys (M3): w[ScienceRateTrailing] is read at rivals.rs:977
    (w.get(key.trailing())) and added to the SCIENCE_RATE marginal:
    m += hinge * trailing_fraction(key, state, idx)  (rivals.rs:976-983)
  * Credit keys (M2): w[TechBoardCredit] is read inside the per-card
    pricers (cards.rs:1947-2019: tb * tech_value(...), etc.), scaling
    hand/wonder/action values that evaluate then books under the OUTER
    gates (hand_potential, wonder_potential, ...).

In both cases the feature value phi_k is not a quantity that exists,
because the coefficient multiplies a state-dependent factor (a
fraction in [0,1], a card value) that the instrument never records.
So the answer to (1) is: the clamp applies, and applies to the
coefficient; the zero p95 row records that the instrument cannot see
that coefficient's own feature, not that the coefficient is unbounded.
"Clamped at 60.0" means "held at the fallback ceiling" and nothing
more.

7.2 The p95 row for the hinge class: category error, and the honest
    move is to stop pretending
------------------------------
For the six hinge keys the row IS a category error, and the design in
Section 6 is the wrong response. Reason:

  (a) The row is not "unmeasured" in the sense featspread's own docs
      use for the class it was built for. Those docs say a stored 0.0
      is a documented unmeasured state and the flat rail is the
      fallback (weights.rs:1896-1903). For the hinge keys, by contrast,
      the spread is 0.0 for a structural reason that no measurement
      run can change: linear_features never writes their slots
      (eval.rs:613 zero-init, no f.set arm, confirmed by the multcheck
      runtime classification) and evaluate never dots them
      (evaluate's linear body, eval.rs:162-185, dots features() output,
      which never carries them either). A measurement binary would
      produce the same zeros forever. Filling the row would require
      inventing a DIFFERENT quantity -- p95 over hinge *
      trailing_fraction across decisions -- and labelling it
      p95_candidate_spread. That is not filling a row, it is redefining
      the column to fit one key, and the bound formula would then be
      dividing T by a number whose units are "hinge-times-fraction",
      not "one unit of the feature the weight prices".

  (b) The meaningful quantity for a hinge is not a spread at all, it is
      the fire rate. The hinge fires on exactly the decisions where
      trailing_fraction > 0, and its leverage on the score is
      hinge * trailing_fraction * (candidate spread of the hinged
      marginal) -- which the existing instrument already measures,
      without any new binary: multcheck's term_nonzero_rate
      (multcheck.rs:254-261) IS the fraction of decisions where the
      hinge moves a candidate score. The 2026-08-24 4p run
      (analysis/multcheck_raw_4p_2026-08-24.txt) already recorded
      culture_rate_trailing at 0.2045 term_nonzero with
      flip_rate_zero 0.000073. That pair -- fires in one decision in
      five, flips the choice almost never -- is the complete honest
      summary of the lever, and it is already on disk.

  (c) So the honest move is: remove the pretence. The zero row for a
      hinge key should not be read as "measured zero" nor "unmeasured",
      it is "not a quantity this instrument defines for this key".
      That is a documentation change to weights.rs's p95_candidate_
      spread table comment (and the clamp_bound doc at 1896-1903), and
      at most a marker on those six rows, NOT a measurement tool. A new
      binary that fills them with a differently-defined quantity would
      add a thing that then has to be maintained, defended, and whose
      output would be mistaken for the column's existing meaning. The
      coordinator's proposed outcome -- "the row is meaningless for
      these classes, delete the pretence" -- is the right one for the
      hinge class, and the Section 6 plan's hinge half should be
      dropped.

  One correction to my own earlier claim while answering this: I wrote
  in Section 1 (M3) that the hinge "contribution never appears in ANY
  phi slot" in a way that implied the weight had no score effect. It
  DOES have a score effect -- it reaches evaluate through the card
  pricers (feature_marginal's callers, cards.rs:724, 730, 770, 1289,
  pricing science-yielding cards' marginals), so the 2p champion's
  +20.619 does move real move ranking. The correct statement is: the
  weight's effect exists and is measurable in flip/term_nonzero terms,
  but it is NOT of the form w_k * phi_k for any phi the instrument
  records, which is exactly why no spread row exists for it and none
  should be forced into existence.

7.3 Same question, credit class: the row is not a category error
-----------------------------
The credit keys are different, and the answer is that a bound IS
meaningful for them, for the specific reason that their contribution
IS of the form w_k * (something the instrument can record). The credit
weight multiplies a per-card value that depends on the board state, not
on w: tb * tech_value(...) where tech_value is arithmetic on the card's
printed yields priced by OTHER weights' base values. With the pricers'
internal sub-pricing frozen at the champion (the same freeze discipline
linear_features already applies, eval.rs:677-689), the credit's
candidate-set swing IS a well-defined, state-dependent, w-independent
quantity: for each candidate move, the hand/wonder/action value that
the credit scales, and its spread across candidates. That is a
p95-candidate-spread-shaped quantity in the literal sense: "the
candidate-set spread of the thing one unit of this weight prices,
under frozen sub-pricing". The Section 6 probe-as-freeze measurement
measures exactly that.

The distinction from the hinge class is real, not cosmetic: the hinge's
multiplier (trailing_fraction) is a STATE-only factor in [0,1] that
gates a contribution to ANOTHER key's marginal -- it has no candidate-
set of its own, and its "spread" across candidates would just be
trailing_fraction spread, a number with no relation to the total-score
swing T. The credit's multiplier (the card value) is precisely the
thing the candidate moves change, so its candidate spread converts T
into a coefficient bound with the same unit logic as every measured row.

Two honest limits on the credit reading:
  (i) The measured quantity is conditional on the freeze: it is "the
      spread under champion sub-pricing", which is also what the
      existing measured rows are (weights.rs:1930-1935: "These are a
      property of the CHAMPION as much as of the game"). Consistent with
      the column's existing semantics, so it is a legitimate row value,
      not a redefinition.
  (ii) The credit keys are doubly present: they scale the outer gates'
      inner values (M2) AND some are themselves outer gates. The
      measured spread covers the inner-scaling role only; the gate role
      belongs to the gate key's own row. The bound is a rail on the
      inner-scaling leverage, and that is the role the rail is for.

7.4 What the blind 60.0 ceiling costs the credit class in practice
------------------------------------------------------------------
The question: what does a WRONG bound actually do to these keys during
a climb, observably?

  * It costs nothing when it is too LOOSE. A 60.0 ceiling is a ceiling:
    the measured bounds are all below 60.0 (the min at weights.rs:1917),
    so 60.0 is never tighter than the rail would be. While a credit key
    sits below 60.0, the blind ceiling is inactive.
  * It costs something when a key WALKS to it. A credit key reaching
    +/-60.0 means the fitness gradient still points at the wall after
    thousands of generations (the pinned-coordinate pathology the
    runaway guard exists for, climb.rs:99-115, 282-294). The guard logs
    it (climb.rs:1352-1360) but cannot distinguish "the true optimal is
    past 60 and the rail is wrong" from "overfit to the pool, pinned
    coordinate". With a measured bound in place, a key at its measured
    bound is a different report than a key at the blind one: the former
    says the instrument saw the leverage and the search pushed to it;
    the latter says the instrument could not see the leverage at all.
    That is the observable difference the row is for.
  * It costs something structural: P95_TOTAL_SPREAD (the numerator T)
    was measured from an evaluate that does not carry the credits'
    contributions (featspread's phi, featspread.rs:201, dots
    linear_features, which prices the eleven identity-aware gates at
    the freeze and carries no credit inner-scaling). So T is the
    total-score swing of a PARTIAL score. Any bound computed from that
    T -- measured or blind -- divides a partial numerator by a
    key-specific denominator. The measured rows are already affected
    by this in principle (whenever any credit weight is nonzero, the
    true total swing is larger than T). The practical magnitude is
    unknown and was not measured in this session; stating it that way
    rather than waving at it.
  * What it does NOT cost: it cannot make a credit key misbehave below
    the ceiling. The ceiling only binds at 60.0. So "what does the
    blind 60.0 do to a credit key during a climb" has a short answer:
    nothing until the key reaches 60.0, and at 60.0 it is an
    indistinguishable-pinned-coordinate log line, not a wrong decision.
    The value of the measured row is in the REPORTING (pinned-at-
    measured-bound vs pinned-at-blind-ceiling) and in the numerator
    discipline (a T re-measured under the same run), not in preventing
    a wrong move today.

7.5 Verdict on the Section 6 design, plainly
--------------------------------------------
  * Hinge half (the 6 M3 keys): unnecessary. The row is a category
    error; the honest instrument already exists (multcheck
    term_nonzero/flip, run on 2026-08-24); the right move is a
    documentation correction that says the zero row for these keys is
    "not defined for this key", not "unmeasured". Drop the plan.
  * RateHorizon: as written in Section 1, its slot is never written by
    features(); its effect is a scale on the four RATE_KEYS. Same
    category-error logic as the hinges applies to its own row: the
    meaningful quantity is the fire rate of hz != 1.0, not a spread.
    Drop from the plan too.
  * Credit half (the 21 M2 keys): the row is meaningful, the
    measurement is the right way to fill it, and the Section 6
    mechanism (probe-as-freeze, per-decision candidate spread, same
    bound formula, same emit path) stands. This is the part of the
    design worth building, and it is smaller than the design as
    written because the hinge and RateHorizon rows no longer need
    rows.
  * The one gap named in Section 6 (probe must be passed as freeze)
    remains the critical implementation point for the surviving half.

In the coordinator's framing: the design as a whole was ~50%
unnecessary. The hinge and RateHorizon halves should be withdrawn in
favour of "delete the pretence" -- a doc change that removes a false
promise from the table -- and the credit half is the real work, with
its cost/benefit stated honestly in 7.4 (reporting quality and
numerator discipline, not prevention of a wrong move).
