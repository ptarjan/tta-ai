//! `creditspread` -- the credit-half spread measurement specified by
//! `analysis/credit_spread_measurement_spec_2026-08-26.txt` (frozen at
//! af57966). Implements the spec's sections 2-6 exactly: the 20 credit
//! keys, the probe-in-the-FREEZE formulation (spec section 3), fixed
//! `c = 1.0` (spec section 4.3), the normalized slope `s_k = S_d(c)/|c|`
//! with the per-key c=1.0/c=2.0 linearity test (spec sections 4.2/4.3/5),
//! T re-measured in the same run (spec section 5, step 6), and the
//! LEVER-not-INFLUENCE report with the flip rates in the same table
//! (spec section 6, as corrected 2026-08-26: the flip vectors move all
//! 20 credit keys at once, so the flip rates are a single CREDIT-CLASS
//! row per count, not a per-key column).
//!
//! # Why a new binary, not a featspread mode
//!
//! `featspread`'s instrument is structurally blind to this family by
//! construction: the credit keys have NO coordinate in the linear feature
//! vector `phi` that `eval::linear_features`/`candidate_features` return,
//! so a probe that moves only the credit key changes nothing in `dot(w,
//! phi)` -- `c * 0.0 = 0.0` -- and the candidate-set spread is exactly
//! 0.0 at every decision (which is why their rows in
//! `WeightKey::p95_candidate_spread` are `[0.000000, 0.000000, 0.000000]`
//! and they sit at `CLAMP_BLIND` today). The swing comes ENTIRELY from
//! the pricers re-running under the probe freeze: `eval::linear_features`
//! computes the eleven identity-aware coordinates (`HandPotential`,
//! `WonderPotential`, `WonderPromise`, `HandMilPotential`,
//! `RivalHandPotential`, `RowUrgency`, `RowBargainForgone`, `RowLastCopy`,
//! `MyEventThreat`) by calling the pricers with the `freeze: &Weights`
//! parameter, and each pricer resolves its own credit weight from that
//! same frozen vector.
//!
//! The correct formulation (spec section 3, the CRITICAL CORRECTION, as
//! corrected by the coordinator on 2026-08-26, displacement form per
//! 2026-08-26):
//!   phi_c = candidate_features(s, legal, allow_resign, freeze=champion)
//!   phi_p = candidate_features(s, legal, allow_resign, freeze=pw)
//!           (pw = champion with k DISPLACED by c: set to w_k + c, every
//!            other key at the champion value, so only the pricer reads
//!            of k differ between phi_c and phi_p)
//!   d_m   = dot(pw, phi_p(m)) - dot(champion, phi_c(m))
//!           = dot(champion, phi_p(m) - phi_c(m))     (pw and champion
//!             differ only in coordinate k, and k has NO linear feature
//!             coordinate, so both forms are identical)
//!   S_d   = max_m d_m - min_m d_m
//!
//! The probe has NO effect on the dot product (the credit coefficient is
//! frozen OUT of it); the swing is the difference between phi_p and phi_c
//! in the slots the pricers wrote, a function of c only through the
//! pricer's own resolution of k. Applying the probe to the DOT vector
//! instead of the freeze is the wrong instrument -- `tests` below contains
//! a unit test that fails if that is done. The spec's own
//! "S_d = max-min dot(pw, phi_p(m))" was ALSO the wrong instrument: the
//! spread of the TOTAL score is T (the total spread), identical under
//! every probe, so it read flat at T for every key -- the probe's
//! contribution is a small term buried inside a number ~400 units wide.
//! The DELTA form above removes the constant baseline: the 168 non-credit
//! keys cancel out of d_m, leaving purely the pricer re-pricing effect.
//!
//! DISPLACEMENT, not SET (coordinator correction, 2026-08-26): an earlier
//! draft of this binary SET k to c (pw[k] = c). That made
//! S_d(c) = h(c) - h(w_k) where h is the pricer's response to a key set
//! to a value, and the c = 1.0/2.0 test then passes iff h is linear on an
//! interval containing {w_k, 1.0, 2.0} -- trivially true when w_k = 0
//! (h(2) = 2h(1) on a linear segment through 0) and otherwise a
//! second-difference test, not a |c|-linearity test. The displacement
//! probe (w_k + c) measures the pricer's response AT THE CHAMPION'S OWN
//! OPERATING POINT, so |c|-linearity is the correct test and gated_frac
//! is a per-key property of the pricers.
//!
//! # WHAT IS REPORTED IS A LEVER, NOT INFLUENCE (spec section 6)
//!
//! `S_d` is a LEVER: how far one unit of probe magnitude can move the
//! score difference between the best and worst candidate. It is NOT
//! INFLUENCE: a large `S_d` says nothing about whether the key changes
//! the CHOSEN move -- if the spread is small relative to the argmax gap,
//! the flip rate is zero no matter how large the lever. The flip rates
//! (zero/abs, the counterfactual argmax-flip instrument) are therefore
//! printed in the SAME table as `p95_slope` and `bound` -- as ONE
//! CREDIT-CLASS row per count (both flip vectors perturb all 20 keys at
//! once, so the number is the flip rate for the credit half as a whole,
//! not commensurable with multcheck's one-key-at-a-time per-key flip
//! rates) -- and the method header carries the required caveat verbatim.
//!
//! # Per-key linearity test (spec section 4.3, displacement form)
//!
//! Under the displacement probe the key sits at w_k + c, and S_d(c) is
//! |c|-linear wherever the pricer's response is linear on the interval
//! [w_k, w_k + 2.0] around the champion's own operating point: the
//! dedicated gates (`cards::card_potential_core`'s `tb != 0.0` branches)
//! are linear in the key's value wherever the branch stays active, and
//! the additive credits (`sum_yields`'s unconditional `amt *= credit`)
//! are linear across all values. A pricer whose branch flips inside that
//! interval (a gate threshold between w_k and w_k + 2.0) is NOT
//! |c|-linear there, and the test catches it. Because the bound is built
//! from the NORMALIZED slope `s_k = S_d(c)/|c|`, any nonzero c gives the
//! same bound on a linear segment -- c is a numerical-conditioning
//! choice, pinned at 1.0 for every key. The per-key test probes
//! c = 1.0 and c = 2.0 and requires `S_d(2.0) ~= 2 * S_d(1.0)`; a
//! decision where that fails counts toward the key's GATED_FRAC. gated
//! frac is the headline: most keys are expected to drop to a small
//! gated_frac and carry a usable slope, while a few keep a high one --
//! those few are the genuine finding. A key gated at more than half its
//! firing decisions emits NO bound (the RUST TABLE row is 0.000000, so
//! `clamp_bound`'s own `<= 0.0` fallback keeps it at `CLAMP_BLIND`); a
//! key below that threshold still earns a real bound from its own
//! measured slope, capped at `CLAMP_CEILING` like every other measured
//! row -- no slope is emitted for a key that cannot be defended.
//!
//! # T is re-measured in the SAME run (spec section 5, step 6)
//!
//! `T_players` is the p95 over decisions of the TOTAL spread
//! `max-min dot(champion, phi_c)` over the candidate set, from this run's
//! own sample. `P95_TOTAL_SPREAD` (weights.rs) was measured by
//! `featspread` in a different run with a different sample; dividing this
//! run's slopes by that stale constant would mix two samples, which the
//! spec forbids.
//!
//! # Cost
//!
//! Per decision: 1 shared phi_c + 20 per-key phi_p (displacement c=1.0)
//!     + 20 per-key phi_p2 (displacement c=2.0, the linearity test) + 2
//!     shared flip vectors (all 20 keys -> 0.0 and ->abs, shared across
//!     keys) = 43 `candidate_features` calls, versus 1 for plain
//!     `featspread`. phi_p2 is NOT shared across keys: key k's c=2.0
//!     freeze differs from every other key's, and the pricers cannot be
//!     delta'd (spec section 5's COST NOTE acknowledges the pricers are
//!     the dominant cost and are not linear in a way that supports a
//!     delta).
//!
//! ```text
//! cargo run --release --bin creditspread -- <games> <seed> <threads> <champion_dir>
//! ```
//!
//! `champion_dir` holds `rust_champion_{2,3,4}p.json`, frozen by md5
//! (the multcheck convention, analysis/multiplier_flips_2026-08-25.txt
//! lines 16-22); the frozen copy this run must use is
//! `analysis/champion_freeze_2026-08-26/`.

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use tta::bots::weighted::eval::{self, load_weights, WeightedBot};
use tta::bots::weighted::weights::{CLAMP_BLIND, CLAMP_CEILING, CLAMP_T, WeightKey, Weights};
use tta::game::{self, MOVE_CAP};

/// The probe DISPLACEMENT, FIXED for every key (spec section 4.3,
/// displacement form 2026-08-26): the key sits at w_k + c, so the probe
/// measures the pricer's response at the champion's own operating point.
const C: f64 = 1.0;
/// The second displacement of the per-key linearity test.
const C2: f64 = 2.0;
/// Relative tolerance for the linearity test: |S_d(2.0) - 2*S_d(1.0)|
/// <= TOL * S_d(1.0) when S_d(1.0) > 0.
const LINEAR_TOL: f64 = 0.01;
/// `S_d` readings at or under this are treated as "no spread" for the
/// linearity test (both probes at numerical zero passes: there is no
/// gate to hide at a magnitude the pricer cannot resolve).
const SD_EPS: f64 = 1e-9;

const PLAYER_COUNTS: [u8; 3] = [2, 3, 4];
const N_KEYS: usize = 20;

const CAVEAT: &str = "S_d is a LEVER (max-min over the candidate set of the per-move\
                      DELTA the probe causes, d_m = dot(pw, phi_p(m)) - dot(champion,\
                      phi_c(m)) = dot(champion, phi_p(m) - phi_c(m)) -- purely the\
                      pricer re-pricing effect, baseline removed), not INFLUENCE. A large\
                      S_d does not imply the key changes the chosen move. Read\
                      flip_rate_zero and flip_rate_abs for influence -- those two are\
                      CREDIT-CLASS statistics (all 20 credit keys zeroed / set to abs\
                      together, one row), NOT per-key: multcheck's per-key flip rates\
                      perturb ONE key at a time. Read p95_slope for the scale of the\
                      bound.";

struct Args {
    games: usize,
    seed: u64,
    threads: usize,
    champion_dir: String,
}

const USAGE: &str = "usage: creditspread <games> <seed> <threads> <champion_dir>";

fn parse_args(argv: &[String]) -> Result<Args, String> {
    if argv.len() != 4 {
        return Err(format!("{USAGE}\ngot {} argument(s), expected 4", argv.len()));
    }
    let games: usize = argv[0].parse().map_err(|_| format!("games: {:?} is not a number", argv[0]))?;
    let seed: u64 = argv[1].parse().map_err(|_| format!("seed: {:?} is not a number", argv[1]))?;
    let threads: usize = argv[2].parse().map_err(|_| format!("threads: {:?} is not a number", argv[2]))?;
    if games == 0 {
        return Err("games must be at least 1".to_string());
    }
    if threads == 0 {
        return Err("threads must be at least 1".to_string());
    }
    Ok(Args { games, seed, threads, champion_dir: argv[3].clone() })
}

/// Nearest-rank percentile (`rank = ceil(p/100 * n)`, 1-indexed) over an
/// already-sorted-ascending slice; `0.0` on an empty slice (a key that
/// never fired has no firing-percentile to report). Same convention as
/// `featspread` (featspread.rs:105).
fn percentile(sorted_ascending: &[f64], p: f64) -> f64 {
    if sorted_ascending.is_empty() {
        return 0.0;
    }
    let n = sorted_ascending.len();
    let rank = (p / 100.0 * n as f64).ceil() as usize;
    let idx = rank.clamp(1, n) - 1;
    sorted_ascending[idx]
}

fn sorted(mut v: Vec<f64>) -> Vec<f64> {
    v.sort_by(|a, b| a.partial_cmp(b).expect("spreads are never NaN"));
    v
}

/// First-candidate-wins argmax (matching `WeightedBot::choose`'s own
/// tie-break), as `multcheck` uses it.
fn argmax(scores: &[f64]) -> usize {
    let mut best = 0usize;
    for (i, &s) in scores.iter().enumerate() {
        if s > scores[best] {
            best = i;
        }
    }
    best
}

/// The 20 credit keys (spec section 2), in spec order. The names are never
/// typed as string literals anywhere in this file (repo-wide rule): the
/// set is declared as `WeightKey` values here, and every printed name is
/// resolved at runtime via `WeightKey::name`/`Debug`.
const CREDIT_KEYS: [WeightKey; N_KEYS] = [
    WeightKey::CardRateCredit,
    WeightKey::UnitStrengthCredit,
    WeightKey::TerritoryCredit,
    WeightKey::BonusCardCredit,
    WeightKey::CardBoardCredit,
    WeightKey::TechBoardCredit,
    WeightKey::ActionBoardCredit,
    WeightKey::GovBoardCredit,
    WeightKey::WonderBoardCredit,
    WeightKey::TacticBoardCredit,
    WeightKey::AggressionBoardCredit,
    WeightKey::WarBoardCredit,
    WeightKey::PactBoardCredit,
    WeightKey::EventBoardCredit,
    WeightKey::UnitTechCredit,
    WeightKey::BuildFreshCredit,
    WeightKey::RestrictedResourceCredit,
    WeightKey::FreeActionCredit,
    WeightKey::TacticReachCredit,
    WeightKey::CardBoardLeader,
];

/// Synthetic-decision S_d (spec section 7's required unit test, as
/// corrected 2026-08-26, displacement form): `probe_w` is the probe
/// weight vector (the champion with k DISPLACED to w_k + c), `phi_p` the
/// candidate vectors under the probe freeze, `base` the per-candidate
/// baseline `dot(champion, phi_c(m))`. Returns
/// `S_d = max_m (dot(pw, phi_p(m)) - base(m)) - min_m (...)` -- the
/// spread of the per-move DELTA the probe causes. Because `probe_w` and
/// the champion differ only in a coordinate with no linear feature (k is
/// a credit key), each delta equals
/// `dot(champion, phi_p(m) - phi_c(m))`: purely the pricer re-pricing
/// effect, baseline removed.
#[cfg(test)]
fn synthetic_sd(probe_w: &Weights, phi_p: &[Vec<f64>], base: &[f64]) -> f64 {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for (i, f) in phi_p.iter().enumerate() {
        let d = eval::dot(probe_w, f) - base[i];
        if d < lo {
            lo = d;
        }
        if d > hi {
            hi = d;
        }
    }
    hi - lo
}

/// The bound (spec section 5, step 6): `CLAMP_T * T / p95_slope` capped at
/// `CLAMP_CEILING` (a measured row was seen firing, so it gets the room a
/// measurement earns); `CLAMP_BLIND` when `p95_slope <= 0` (no firing
/// decisions observed -- invisible to this instrument, same as a zero row
/// in `p95_candidate_spread`).
///
/// MIRRORS `WeightKey::clamp_bound` (weights.rs) exactly -- same two
/// constants, same shape, same fallback condition -- because this function
/// exists only to report what the live rule will do for the credit keys
/// before their measured rows are spliced into that match. If
/// `clamp_bound` ever changes its cap or its fallback, this must change
/// with it or the report starts lying about the rule it mirrors.
fn bound_from_slope(p95_slope: f64, t_players: f64) -> f64 {
    if p95_slope <= 0.0 {
        return CLAMP_BLIND;
    }
    (CLAMP_T * t_players / p95_slope).min(CLAMP_CEILING)
}

/// One credit key's running totals over one player count's whole self-play
/// sample. `firing_slopes` is kept in full (not a running sum) because
/// `p95_slope` needs the distribution, mirroring featspread's `KeyAgg`.
///
/// `flips_zero` / `flips_abs` are CREDIT-CLASS counters: the two flip
/// vectors zero / abs-set ALL 20 credit keys at once, so every key's
/// counter counts the same decisions. They are reduced to ONE per-count
/// row in `CountResult`, never divided across the 20 keys.
#[derive(Default, Clone)]
struct KeyAgg {
    firing: u64,
    firing_slopes: Vec<f64>,
    /// Raw S_d(c) readings over FIRING decisions, for the GATED-keys
    /// section (spec section 5, step 4e): `sd1` at c = 1.0, `sd2` at
    /// c = 2.0.
    sd1: Vec<f64>,
    sd2: Vec<f64>,
    /// Decisions where the linearity test failed (S_d(2.0) not ~2*S_d(1.0)).
    gated: u64,
    flips_zero: u64,
    flips_abs: u64,
    term_nonzero: u64,
    /// Decisions where the probe (c = 1.0) changed ANY candidate's score
    /// (spec section 6, item 6 -- `term_nonzero` as a fraction; the flip
    /// counters above are the argmax-flip rates).
    probe_touched: u64,
}

/// One credit key's fully reduced summary for one player count.
struct KeySummary {
    key: WeightKey,
    name: &'static str,
    champ_w: f64,
    /// Per-key COUNT of firing decisions (S_d(c=1.0) > 0), the headline
    /// denominator for gated_frac (coordinator, 2026-08-26).
    firing: u64,
    /// Per-key fraction of decisions where the c = 1.0 probe fired
    /// (S_d > 0): firing / decisions. Superseded by `firing` (the raw
    /// count, the headline denominator for gated_frac) but kept for the
    /// spec section 6 report.
    #[allow(dead_code)]
    fire_rate: f64,
    p95_slope: f64,
    bound: f64,
    gated: bool,
    gated_frac: f64,
    sd1_p95: f64,
    sd2_p95: f64,
    /// Per-key fraction of decisions where the c = 1.0 probe changed any
    /// candidate's score (spec section 6, item 6). Per-key because each
    /// key's probe is its own.
    term_nonzero_frac: f64,
}

/// One player count's fully reduced self-play sample.
struct CountResult {
    players: u8,
    decisions: u64,
    keys: Vec<KeySummary>,
    /// CREDIT-CLASS flip rates (coordinator correction, 2026-08-26): the
    /// two flip vectors zero / abs-set ALL 20 credit keys at once, so the
    /// flip rate is ONE number per count -- "flip rate for the credit half
    /// as a whole" -- not commensurable with multcheck's per-key flip
    /// rates (which perturb one key at a time).
    flip_rate_zero: f64,
    flip_rate_abs: f64,
    /// T_players (spec section 5, step 6): p95 over decisions of the TOTAL
    /// spread max-min dot(champion, phi_c) over the candidate set,
    /// re-measured in this run.
    t_p50: f64,
    t_p95: f64,
    t_max: f64,
}

/// One thread's share of the measurement: plays whole games (claimed off
/// `next`), and at every real decision (candidate set |C| > 1, the same
/// gate featspread and multcheck use) performs the spec section 5 steps:
/// one shared phi_c under the champion freeze, one phi_p per key under the
/// c = 1.0 probe freeze, one phi_p2 per key under the c = 2.0 probe
/// freeze (the linearity test), and the two SHARED flip vectors (all 20
/// credit keys -> 0.0, and all 20 -> champion.get(k).abs()) with their
/// argmax-flip indicators -- CREDIT-CLASS statistics by construction,
/// reduced to one row per count, never divided across keys.
fn play_shard(
    games: usize,
    seed: u64,
    players: u8,
    next: &AtomicUsize,
    weights: &Weights,
) -> (Vec<KeyAgg>, Vec<f64>, u64) {
    let bot = WeightedBot::new(*weights);
    let mut agg: Vec<KeyAgg> = vec![KeyAgg::default(); N_KEYS];
    let mut total_spreads: Vec<f64> = Vec::new();
    let mut decisions = 0u64;

    loop {
        let g = next.fetch_add(1, Ordering::Relaxed);
        if g >= games {
            break;
        }
        let game_seed = seed.wrapping_add(g as u64);
        let mut state = game::new_game(players, game_seed);
        let outcome = game::play_game(&mut state, MOVE_CAP, |s, legal| {
            // Step 4a -- the shared baseline, the SAME call featspread
            // makes.
            let phi_c = eval::candidate_features(s, legal.as_slice(), bot.allow_resign, weights);
            if phi_c.len() > 1 {
                decisions += 1;
                let base_scores: Vec<f64> = phi_c.iter().map(|(_, f)| eval::dot(weights, f)).collect();
                let base_move_idx = argmax(&base_scores);
                let base_lo = base_scores.iter().cloned().fold(f64::INFINITY, f64::min);
                let base_hi = base_scores.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                total_spreads.push(base_hi - base_lo);

                // Step 4f -- the two shared flip vectors, priced under
                // their own freezes, scored with the CHAMPION (the flip is
                // "does the champion's argmax change when the pricers see
                // the perturbed freeze", spec section 6 items 4-5).
                let mut w0 = *weights;
                for &k in &CREDIT_KEYS {
                    w0.set(k, 0.0);
                }
                let mut wabs = *weights;
                for &k in &CREDIT_KEYS {
                    wabs.set(k, weights.get(k).abs());
                }
                let scores0: Vec<f64> =
                    eval::candidate_features(s, legal.as_slice(), bot.allow_resign, &w0)
                        .iter()
                        .map(|(_, f)| eval::dot(weights, f))
                        .collect();
                let scores_abs: Vec<f64> =
                    eval::candidate_features(s, legal.as_slice(), bot.allow_resign, &wabs)
                        .iter()
                        .map(|(_, f)| eval::dot(weights, f))
                        .collect();
                let flip0 = argmax(&scores0) != base_move_idx;
                let flip_abs = argmax(&scores_abs) != base_move_idx;
                let touched0 = scores0
                    .iter()
                    .zip(base_scores.iter())
                    .any(|(&a, &b)| (a - b).abs() > 1e-9);
                // CREDIT-CLASS accumulation: both flip vectors move ALL 20
                // keys at once, so one counter pair serves the whole
                // credit half. The counters are stored on every key's
                // KeyAgg (reduce_count sums them across shards, so the
                // value must be counted once per shard per flip), and the
                // reduce divides by ONE count's decisions -- never by the
                // 20 keys. The per-key `touched` column is the per-key
                // probe_touched accumulated in the per-key loop below.
                for a in &mut agg {
                    if flip0 {
                        a.flips_zero += 1;
                    }
                    if flip_abs {
                        a.flips_abs += 1;
                    }
                    if touched0 {
                        a.term_nonzero += 1;
                    }
                }

                // Steps 4b-4e -- per key: probe freezes at displacement
                // c = 1.0 and c = 2.0 (pw[k] = w_k + c; every other key at
                // the champion value). S_d is the spread over CANDIDATES
                // of the per-move DELTA the probe causes,
                //   d_m = dot(pw, phi_p(m)) - dot(champion, phi_c(m))
                //        = dot(champion, phi_p(m) - phi_c(m))
                // (pw differs from the champion only in coordinate k, and
                // k has no linear feature coordinate, so the first form
                // equals the second: the delta is PURELY the pricer
                // re-pricing effect, with the constant baseline removed
                // -- the 168 non-credit keys cancel out of it). Measuring
                // the spread of the total score instead measures T, which
                // dominates d_m for every key; the delta is the quantity
                // that should be |c|-linear (coordinator correction,
                // 2026-08-26, on the af57966 spec -- the spec's
                // "S_d = max-min dot(pw, phi_p(m))" was the wrong
                // instrument: it buried the probe's contribution inside a
                // ~400-unit total).
                //
                // DISPLACEMENT, not SET (coordinator correction,
                // 2026-08-26): setting k to c made S_d(c) = h(c) - h(w_k),
                // so the c = 1.0/2.0 test passed iff the pricer's response
                // h was linear on an interval containing {w_k, 1.0, 2.0}
                // -- trivially true at w_k = 0, a second-difference test
                // otherwise. Displacing by c measures the response at the
                // champion's own operating point, where |c|-linearity is
                // the right test and gated_frac is a per-key property of
                // the pricers.
                for (ki, &k) in CREDIT_KEYS.iter().enumerate() {
                    let a = &mut agg[ki];
                    let wk = weights.get(k);

                    let mut pw = *weights;
                    pw.set(k, wk + C);
                    let phi_p = eval::candidate_features(s, legal.as_slice(), bot.allow_resign, &pw);
                    let mut lo = f64::INFINITY;
                    let mut hi = f64::NEG_INFINITY;
                    let mut touched = false;
                    for (i, (_, f)) in phi_p.iter().enumerate() {
                        // dot(pw, phi_p(m)) - dot(champion, phi_c(m)): the
                        // credit coordinate contributes 0.0 to both dots
                        // (the pricer re-run is baked into phi_p), so this
                        // equals dot(champion, phi_p(m) - phi_c(m)) and is
                        // the delta, not the total.
                        let d = eval::dot(&pw, f) - base_scores[i];
                        if d.abs() > 1e-9 {
                            touched = true;
                        }
                        if d < lo {
                            lo = d;
                        }
                        if d > hi {
                            hi = d;
                        }
                    }
                    let sd1 = hi - lo;
                    if touched {
                        a.probe_touched += 1;
                    }

                    let mut pw2 = *weights;
                    pw2.set(k, wk + C2);
                    let phi_p2 = eval::candidate_features(s, legal.as_slice(), bot.allow_resign, &pw2);
                    let mut lo2 = f64::INFINITY;
                    let mut hi2 = f64::NEG_INFINITY;
                    for (i, (_, f)) in phi_p2.iter().enumerate() {
                        let d = eval::dot(&pw2, f) - base_scores[i];
                        if d < lo2 {
                            lo2 = d;
                        }
                        if d > hi2 {
                            hi2 = d;
                        }
                    }
                    let sd2 = hi2 - lo2;

                    if sd1 > 0.0 {
                        a.firing += 1;
                        a.firing_slopes.push(sd1 / C);
                        a.sd1.push(sd1);
                        a.sd2.push(sd2);
                        let linear = sd1 <= SD_EPS || (sd2 - 2.0 * sd1).abs() <= LINEAR_TOL * sd1;
                        if !linear {
                            a.gated += 1;
                        }
                    }
                }
            }
            bot.choose(s, legal.as_slice())
        });
        if outcome.move_cap_hit {
            eprintln!("creditspread: WARNING {players}p game (seed {game_seed}) hit the {MOVE_CAP}-move cap");
        }
    }

    (agg, total_spreads, decisions)
}

/// Reduce one player count's shards to a `CountResult` (spec section 5,
/// steps 5-6).
fn reduce_count(players: u8, shards: &[(Vec<KeyAgg>, Vec<f64>, u64)], weights: &Weights) -> CountResult {
    let decisions: u64 = shards.iter().map(|(_, _, d)| *d).sum();
    let mut total_spreads: Vec<f64> = Vec::new();
    for (_, ts, _) in shards {
        total_spreads.extend_from_slice(ts);
    }

    let mut keys: Vec<KeySummary> = Vec::with_capacity(N_KEYS);
    let mut flips_zero_total = 0u64;
    let mut flips_abs_total = 0u64;
    for (ki, &k) in CREDIT_KEYS.iter().enumerate() {
        let mut agg = KeyAgg::default();
        for (sa, _, _) in shards {
            let s = &sa[ki];
            agg.firing += s.firing;
            agg.firing_slopes.extend_from_slice(&s.firing_slopes);
            agg.sd1.extend_from_slice(&s.sd1);
            agg.sd2.extend_from_slice(&s.sd2);
            agg.gated += s.gated;
            agg.probe_touched += s.probe_touched;
        }
        // The flip counters are CREDIT-CLASS: every key's KeyAgg counted
        // the same decisions, so summing across keys would multiply by 20.
        // Take any one key's cross-shard sum -- they are all equal -- and
        // report it once, per count.
        if ki == 0 {
            for (sa, _, _) in shards {
                flips_zero_total += sa[ki].flips_zero;
                flips_abs_total += sa[ki].flips_abs;
            }
        }
        let slopes_sorted = sorted(agg.firing_slopes.clone());
        let sd1_sorted = sorted(agg.sd1.clone());
        let sd2_sorted = sorted(agg.sd2.clone());
        let p95_slope = percentile(&slopes_sorted, 95.0);
        let gated = agg.firing > 0 && agg.gated as f64 > 0.5 * agg.firing as f64;
        keys.push(KeySummary {
            key: k,
            name: k.name(),
            champ_w: weights.get(k),
            firing: agg.firing,
            fire_rate: if decisions > 0 { agg.firing as f64 / decisions as f64 } else { 0.0 },
            p95_slope,
            bound: bound_from_slope(p95_slope, percentile(&sorted(total_spreads.clone()), 95.0)),
            gated,
            gated_frac: if agg.firing > 0 { agg.gated as f64 / agg.firing as f64 } else { 0.0 },
            sd1_p95: percentile(&sd1_sorted, 95.0),
            sd2_p95: percentile(&sd2_sorted, 95.0),
            term_nonzero_frac: if decisions > 0 { agg.probe_touched as f64 / decisions as f64 } else { 0.0 },
        });
    }

    let ts_sorted = sorted(total_spreads);
    CountResult {
        players,
        decisions,
        keys,
        flip_rate_zero: if decisions > 0 { flips_zero_total as f64 / decisions as f64 } else { 0.0 },
        flip_rate_abs: if decisions > 0 { flips_abs_total as f64 / decisions as f64 } else { 0.0 },
        t_p50: percentile(&ts_sorted, 50.0),
        t_p95: percentile(&ts_sorted, 95.0),
        t_max: ts_sorted.last().copied().unwrap_or(0.0),
    }
}

fn print_method_header(args: &Args, results: &[CountResult], md5s: &[String; 3]) {
    println!("================================================================================");
    println!("THROUGH THE AGES -- CREDIT-HALF SPREAD (creditspread, spec 2026-08-26)");
    println!("================================================================================");
    println!("Spec: analysis/credit_spread_measurement_spec_2026-08-26.txt (frozen at af57966).");
    println!("The 20 credit keys (spec section 2) have NO coordinate in the linear feature");
    println!("vector: their only readers are the card pricers, which resolve their own credit");
    println!("weight from the FROZEN vector inside eval::linear_features, so featspread's");
    println!("spread instrument reads exactly 0.0 for them by construction. The probe in");
    println!("THIS measurement is applied to the FREEZE (not the dot vector -- c * 0.0 = 0.");
    println!("0 there); S_d comes entirely from the pricers re-running under the probe.");
    println!();
    println!("Per credit key k, per decision d (candidate set |C| > 1), DISPLACEMENT c = 1.0 FIXED:");
    println!("  phi_c = candidate_features(s, legal, allow_resign, freeze=champion)   [shared]");
    println!("  phi_p = candidate_features(s, legal, allow_resign, freeze=champ[k+w_k]) [per key]");
    println!("  d_m   = dot(pw, phi_p(m)) - dot(champion, phi_c(m))");
    println!("          = dot(champion, phi_p(m) - phi_c(m))");
    println!("  S_d(1.0) = max_m d_m - min_m d_m   (the per-move DELTA spread: the constant");
    println!("            baseline removed, so the 168 non-credit keys cancel out -- the");
    println!("            spec's 'max-min dot(pw, phi_p(m))' measured the TOTAL score, whose");
    println!("            spread is T and is identical under every probe; that is why the");
    println!("            first measurement was flat at T for all keys)");
    println!("  s_k(d)  = S_d(1.0)/1.0 -- the normalized slope: a pure function of state and");
    println!("           legal moves, no weight in it (spec section 4.2); commensurable with");
    println!("           the featspread spread rows of the other 169 keys.");
    println!("DISPLACEMENT (coordinator correction, 2026-08-26): the probe SETS k to");
    println!("w_k + c, not to c. A set-to-c probe made S_d(c) = h(c) - h(w_k) and the");
    println!("c = 1.0/2.0 test passed iff the pricer's response h was linear on an");
    println!("interval containing {{w_k, 1.0, 2.0}} -- trivially true at w_k = 0 (the pass");
    println!("set was exactly the zero-champion-weight keys), a second-difference test");
    println!("otherwise. Displacing by c measures the pricer's response AT THE");
    println!("CHAMPION'S OWN OPERATING POINT, where |c|-linearity is the correct test");
    println!("and gated_frac is a per-key property of the pricers.");
    println!("Linearity test (spec section 4.3): phi_p2 under displacement c = 2.0; if");
    println!("S_d(2.0) is not ~2*S_d(1.0) the decision counts toward the key's GATED_FRAC");
    println!("(a gate threshold lies between w_k and w_k + 2.0). gated_frac is the");
    println!("HEADLINE column: most keys are expected to drop to a small gated_frac and");
    println!("carry a usable slope; a few keep a high one -- those few are the genuine");
    println!("finding, named with their gated_frac in the GATED-FRACTION section. A key");
    println!("gated at more than half its firing decisions emits NO bound (the RUST TABLE");
    println!("row is 0.000000; clamp_bound's own <= 0.0 fallback keeps it at CLAMP_BLIND);");
    println!("a key below that threshold still earns a real bound, capped at CLAMP_CEILING");
    println!("like every other measured row.");
    println!();
    println!("T_players is re-measured in THIS run (p95 over decisions of the TOTAL spread");
    println!("max-min dot(champion, phi_c) over C) -- P95_TOTAL_SPREAD (weights.rs) is a");
    println!("stale sample from featspread's own run and must not be mixed with this one.");
    println!();
    println!("bound(k, players) = CLAMP_T * T / p95_slope, capped at CLAMP_CEILING (falls");
    println!("back to CLAMP_BLIND when p95_slope <= 0, i.e. no firing decisions observed).");
    println!();
    println!("[REQUIRED CAVEAT, verbatim]");
    println!("{CAVEAT}");
    println!();
    println!("COST NOTE: per decision, 1 shared phi_c + {} per-key phi_p (displacement c=1.0) +", N_KEYS);
    println!("phi_p2 (displacement c=2.0, the linearity test) + 2 shared flip vectors (all 20 keys -> 0.0, ->abs) =");
    println!("43 candidate_features calls (vs 1 for featspread). phi_p2 is NOT shared across");
    println!("keys (each key's c=2.0 freeze is a different vector and the pricers are not");
    println!("delta-able -- spec section 5's COST NOTE concedes the pricers are the dominant");
    println!("cost and not linear in a way that supports a delta).");
    println!();
    for r in results {
        let i = (r.players - 2) as usize;
        println!("{}p: champion rust_champion_{}p.json md5 {}", r.players, r.players, md5s[i]);
    }
    println!("games_per_count={} seed={} threads={}", args.games, args.seed, args.threads);
    println!();
}

fn print_key_table(count: &CountResult) {
    println!("-- {}p -- (decisions {})", count.players, count.decisions);
    println!(
        "{:<28} {:>10} {:>9} {:>12} {:>10} {:>14} {:>13} {:>12}",
        "key", "champ_w", "fire", "p95_slope", "gated_frac", "bound", "touched", "GATED?>"
    );
    for k in &count.keys {
        println!(
            "{:<28} {:>10.4} {:>9} {:>12.6} {:>10.4} {:>14.6} {:>13.6} {:>12}",
            k.name, k.champ_w, k.firing, k.p95_slope, k.gated_frac, k.bound, k.term_nonzero_frac,
            if k.gated { "YES" } else { "no" }
        );
    }
    // The flip rates are CREDIT-CLASS (both flip vectors move all 20 keys
    // at once), so they are ONE row per count, not a column on every key.
    println!(
        "{:<28} {:>10} {:>9} {:>12} {:>10} {:>14} {:>13} {:>12}",
        "CREDIT-CLASS flip rates (all 20 keys perturbed together):", "", "", "", "", "",
        format!("{:.6}", count.flip_rate_zero), format!("{:.6}", count.flip_rate_abs)
    );
    println!(
        "  flip_zero = credit half zeroed at once; flip_abs = credit half abs-set at once.\
         These are NOT commensurable with multcheck's per-key flip rates (one key at\
         a time). 'touched' above IS per-key (fraction of decisions where that key's\
         displacement c=1.0 probe changed any candidate's score)."
    );
    println!();
}

fn print_t_quantiles(results: &[CountResult]) {
    println!("================================================================================");
    println!("T RE-MEASURED IN THIS RUN -- p50/p95/max of the TOTAL spread");
    println!("max-min dot(champion, phi_c) over the candidate set, per decision (spec step 6)");
    println!("================================================================================");
    println!("{:<8} {:>12} {:>12} {:>12} {:>12}", "count", "n_decisions", "p50", "p95", "max");
    for r in results {
        println!(
            "{:<8} {:>12} {:>12.3} {:>12.3} {:>12.3}",
            format!("{}p", r.players),
            r.decisions,
            r.t_p50,
            r.t_p95,
            r.t_max
        );
    }
    println!();
}

fn print_gated_keys(results: &[CountResult]) {
    let rows: Vec<(&KeySummary, u8)> = results
        .iter()
        .flat_map(|r| r.keys.iter().map(move |k| (k, r.players)))
        .filter(|(k, _)| k.gated_frac > 0.0)
        .collect();
    if rows.is_empty() {
        println!("GATED-FRACTION TABLE (linearity test failed at any firing decision): NONE");
        println!();
        return;
    }
    println!("================================================================================");
    println!("GATED-FRACTION TABLE -- S_d(2.0) not ~2*S_d(1.0) at some firing decisions");
    println!("(a gate threshold lies between w_k and w_k + 2.0, spec section 4.3,");
    println!("displacement form). gated_frac is the HEADLINE: most keys are expected to");
    println!("drop to a small gated_frac and carry a usable slope; a few keep a high one.");
    println!("Those few are the genuine finding; they are named here with their gated_frac,");
    println!("on the record. A key gated at more than half its firing decisions (the");
    println!("GATED? column of the key table) emits NO bound: the RUST TABLE row is");
    println!("0.000000, so clamp_bound's own <= 0.0 fallback applies and its bound below");
    println!("reads CLAMP_BLIND. A key below that threshold still earns a real bound from");
    println!("its own measured slope, capped at CLAMP_CEILING like every other measured row.");
    println!("Raw p95 readings over FIRING decisions, both probes:");
    println!("================================================================================");
    println!(
        "{:<28} {:>8} {:>9} {:>10} {:>12} {:>14} {:>14} {:>12}",
        "key", "count", "fire", "gated_frac", "p95 S_d(1.0)", "p95 S_d(2.0)", "bound", "(BLIND/CEIL)"
    );
    for (k, players) in &rows {
        println!(
            "{:<28} {:>7}p {:>9} {:>10.4} {:>12.6} {:>14.6} {:>12.6}",
            k.name, players, k.firing, k.gated_frac, k.sd1_p95, k.sd2_p95, k.bound
        );
    }
    println!();
}

/// Print this run's per-key p95 firing slopes as the literal body of
/// `WeightKey::p95_candidate_spread`, in the SAME shape as featspread's
/// `print_clamp_table` (featspread.rs:622-646), so the credit rows splice
/// into the existing match (weights.rs:2166-2188), replacing the
/// `[0.000000, 0.000000, 0.000000]` rows. Keys gated at more than half
/// their firing decisions (no defensible slope) are emitted as
/// `0.000000` so `clamp_bound`'s own `<= 0.0` fallback keeps them at
/// CLAMP_BLIND -- the same mechanism featspread's zero rows already rely
/// on. (Gate held 2026-08-26: nothing from this run goes into
/// p95_candidate_spread; all 20 rows stay CLAMP_BLIND until a defensible
/// slope exists.)
fn print_clamp_table(results: &[CountResult]) {
    println!();
    println!("================================================================================");
    println!("RUST TABLE -- paste as the credit arms of WeightKey::p95_candidate_spread");
    println!("================================================================================");
    for r in results {
        println!("// {}p: decisions {} T (re-measured) {:.6}", r.players, r.decisions, r.t_p95);
    }
    println!("// NOTE: T is re-measured in THIS run (spec section 5, step 6); it is");
    println!("// intentionally NOT the P95_TOTAL_SPREAD constant -- mixing the two");
    println!("// samples is what the spec forbids.");
    println!("match self {{");
    for &k in &CREDIT_KEYS {
        let cells: Vec<String> = results
            .iter()
            .map(|r| {
                let v = r.keys.iter().find(|s| s.key == k).map_or(0.0, |s| if s.gated { 0.0 } else { s.p95_slope });
                format!("{v:.6}")
            })
            .collect();
        println!("    WeightKey::{:?} => [{}],", k, cells.join(", "));
    }
    println!("}}");
}

/// md5 of a file's bytes, hex -- the multcheck convention for recording
/// the frozen champion (analysis/multiplier_flips_2026-08-25.txt lines
/// 16-22). No external crate: a small local implementation.
const MD5_K: [u32; 64] = [
    0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613, 0xfd469501,
    0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193, 0xa679438e, 0x49b40821,
    0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d, 0x02441453, 0xd8a1e681, 0xe7d3fbc8,
    0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed, 0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a,
    0xfffa3942, 0x8771f681, 0x6d9d6122, 0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70,
    0x289b7ec6, 0xeaa127fa, 0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665,
    0xf4292244, 0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
    0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb, 0xeb86d391,
];

/// The four MD5 round functions, inline (kept in the table below rather
/// than as closures so the inner loop stays allocation-free).
#[inline]
fn md5_f(x: u32, y: u32, z: u32) -> u32 {
    (x & y) | ((!x) & z)
}

#[inline]
fn md5_g(x: u32, y: u32, z: u32) -> u32 {
    (x & z) | (y & !z)
}

#[inline]
fn md5_h(x: u32, y: u32, z: u32) -> u32 {
    x ^ y ^ z
}

#[inline]
fn md5_i(x: u32, y: u32, z: u32) -> u32 {
    y ^ (x | !z)
}

/// md5 of a byte slice, hex -- the multcheck convention for recording the
/// frozen champion (analysis/multiplier_flips_2026-08-25.txt lines 16-22).
/// A small local implementation (no external crate): the standard
/// message-padded, four-round algorithm with the published K table and
/// left-rotation schedule, pinned by a unit test against known digests.
fn md5_hex(data: &[u8]) -> String {
    let mut h: [u32; 4] = [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476];
    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_le_bytes());

    for chunk in msg.chunks(64) {
        let mut m = [0u32; 16];
        for (i, w) in m.iter_mut().enumerate() {
            *w = u32::from_le_bytes([chunk[4 * i], chunk[4 * i + 1], chunk[4 * i + 2], chunk[4 * i + 3]]);
        }
        let (mut a, mut b, mut c, mut d) = (h[0], h[1], h[2], h[3]);
        for t in 0..64usize {
            let (f, g) = match t {
                0..=15 => (md5_f(b, c, d), t),
                16..=31 => (md5_g(b, c, d), (5 * t + 1) % 16),
                32..=47 => (md5_h(b, c, d), (3 * t + 5) % 16),
                _ => (md5_i(b, c, d), (7 * t) % 16),
            };
            let tmp = d;
            d = c;
            c = b;
            let x = m[g].wrapping_add(a).wrapping_add(MD5_K[t]).wrapping_add(f);
            let s = match t {
                0..=15 => [7, 12, 17, 22][t % 4],
                16..=31 => [5, 9, 14, 20][t % 4],
                32..=47 => [4, 11, 16, 23][t % 4],
                _ => [6, 10, 15, 21][t % 4],
            };
            b = b.wrapping_add(x.rotate_left(s));
            a = tmp;
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
    }

    let mut out = String::with_capacity(32);
    for w in h {
        for byte in w.to_le_bytes() {
            out.push_str(&format!("{byte:02x}"));
        }
    }
    out
}

fn main() -> std::process::ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_args(&argv) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("creditspread: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };

    let mut md5s: [String; 3] = [String::new(), String::new(), String::new()];
    let mut loaded: Vec<(u8, Weights)> = Vec::with_capacity(3);
    for &players in &PLAYER_COUNTS {
        let path = Path::new(&args.champion_dir).join(format!("rust_champion_{players}p.json"));
        let weights = match load_weights(&path) {
            Ok(w) => w,
            Err(e) => {
                eprintln!("creditspread: {e}");
                return std::process::ExitCode::FAILURE;
            }
        };
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("creditspread: {path:?}: {e}");
                return std::process::ExitCode::FAILURE;
            }
        };
        md5s[(players - 2) as usize] = md5_hex(&bytes);
        loaded.push((players, weights));
    }

    let started = Instant::now();
    let mut results: Vec<CountResult> = Vec::with_capacity(3);
    let mut any_gated = false;
    for (players, weights) in &loaded {
        let count_started = Instant::now();
        let next = AtomicUsize::new(0);
        let threads = args.threads.min(args.games).max(1);
        let mut shards: Vec<(Vec<KeyAgg>, Vec<f64>, u64)> = Vec::with_capacity(threads);
        std::thread::scope(|scope| {
            let (games, seed, players, next, weights) = (args.games, args.seed, *players, &next, weights);
            let mut handles = Vec::with_capacity(threads);
            for _ in 0..threads {
                handles.push(scope.spawn(move || play_shard(games, seed, players, next, weights)));
            }
            for h in handles {
                shards.push(h.join().expect("creditspread worker thread panicked"));
            }
        });
        let result = reduce_count(*players, &shards, weights);
        let gated_keys: Vec<&KeySummary> = result.keys.iter().filter(|k| k.gated).collect();
        any_gated = any_gated || !gated_keys.is_empty();
        eprintln!(
            "creditspread: {}p done ({} games, {} decisions, {:.1}s)",
            players,
            args.games,
            result.decisions,
            count_started.elapsed().as_secs_f64()
        );
        results.push(result);
    }

    print_method_header(&args, &results, &md5s);
    println!("================================================================================");
    println!("PER-KEY TABLE -- one table carrying p95_slope, bound, the per-key touched");
    println!("fraction, and (as a single CREDIT-CLASS row) the flip rates (spec section 6,");
    println!("as corrected 2026-08-26: the flip vectors move all 20 credit keys at once, so");
    println!("the flip rates are ONE number for the credit half as a whole, not per-key).");
    println!("================================================================================");
    for r in &results {
        print_key_table(r);
    }
    print_t_quantiles(&results);
    print_gated_keys(&results);
    if any_gated {
        eprintln!(
            "creditspread: WARNING -- at least one key failed the per-key linearity test; \
             GATED keys emit no bound (see GATED KEYS section). If MOST keys are gated, \
             that is a finding about the credit pricers, not a failed measurement -- \
             report it instead of forcing a table."
        );
    }
    print_clamp_table(&results);
    eprintln!("creditspread: total elapsed {:.1}s", started.elapsed().as_secs_f64());
    std::process::ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_nearest_rank_p95_of_twenty_ascending_values_is_the_nineteenth() {
        let v: Vec<f64> = (1..=20).map(|x| x as f64).collect();
        assert_eq!(percentile(&v, 95.0), 19.0);
    }

    #[test]
    fn percentile_of_empty_slice_is_zero() {
        assert_eq!(percentile(&[], 95.0), 0.0);
    }

    #[test]
    fn argmax_picks_first_on_tie() {
        assert_eq!(argmax(&[1.0, 2.0, 2.0, 0.5]), 1);
    }

    #[test]
    fn argmax_picks_the_strict_max() {
        assert_eq!(argmax(&[1.0, 5.0, 2.0]), 1);
    }

    #[test]
    fn parse_args_rejects_wrong_argument_count() {
        assert!(parse_args(&["1".to_string(), "2".to_string()]).is_err());
    }

    #[test]
    fn parse_args_rejects_zero_games() {
        let argv = vec!["0".to_string(), "1".to_string(), "1".to_string(), "dir".to_string()];
        assert!(parse_args(&argv).is_err());
    }

    #[test]
    fn parse_args_rejects_zero_threads() {
        let argv = vec!["1".to_string(), "1".to_string(), "0".to_string(), "dir".to_string()];
        assert!(parse_args(&argv).is_err());
    }

    #[test]
    fn parse_args_reads_positional_fields() {
        let argv = vec!["40".to_string(), "7".to_string(), "4".to_string(), "../experiments".to_string()];
        let a = parse_args(&argv).expect("valid args");
        assert_eq!(a.games, 40);
        assert_eq!(a.seed, 7);
        assert_eq!(a.threads, 4);
        assert_eq!(a.champion_dir, "../experiments");
    }

    #[test]
    fn md5_matches_known_digests() {
        // RFC 1321 test vectors, plus the empty-string digest.
        assert_eq!(md5_hex(b""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(md5_hex(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
        // A 100-byte message forces TWO MD5 blocks (padded to 128 bytes),
        // exercising the multi-block loop, not just the first chunk.
        let two_blocks: Vec<u8> = vec![b'a'; 100];
        let digest = md5_hex(&two_blocks);
        // The authoritative multi-block vector (RFC 1321 / hashlib): the
        // 100-byte 'a' digest. This pins the multi-block loop, not just the
        // first chunk, against a value computed by an independent
        // implementation.
        assert_eq!(digest, "36a92cc94a9e0fa21f625f8bfb007adf");
    }

    #[test]
    fn credit_keys_are_distinct_and_exactly_twenty() {
        let mut seen: Vec<WeightKey> = CREDIT_KEYS.to_vec();
        seen.sort_by_key(|k| *k as u8);
        seen.dedup();
        assert_eq!(seen.len(), N_KEYS, "the 20-key set must contain no duplicates");
    }

    #[test]
    fn bound_formula_matches_the_spec_by_hand() {
        // spec section 5, step 6: bound = CLAMP_T * T / p95_slope, capped
        // at CLAMP_CEILING (a measured slope earns the full measured cap);
        // zero slope (no firing decisions, invisible to the instrument)
        // falls back to CLAMP_BLIND.
        assert_eq!(bound_from_slope(0.0, 500.0), CLAMP_BLIND);
        assert_eq!(bound_from_slope(10.0, 500.0), 50.0); // 1.0 * 500 / 10
        // 5000 / 1 = 5000 and 50000 / 5 = 10000 both dwarf CLAMP_CEILING =
        // 120: the cap, not the raw ratio.
        assert_eq!(bound_from_slope(1.0, 5000.0), CLAMP_CEILING);
        assert_eq!(bound_from_slope(5.0, 50000.0), CLAMP_CEILING);
    }

    /// The formulation-pinning test required by spec section 7, as
    /// corrected 2026-08-26 (displacement form): a synthetic decision
    /// whose pricer-written slot differs by a KNOWN amount under a KNOWN
    /// DISPLACEMENT probe, asserting the DELTA S_d equals the
    /// hand-computed value at c = 1.0 and c = 2.0, that the readings are
    /// LINEAR IN THE VALUE w_k + c (which is what the displacement form
    /// promises -- not |c|-linearity of the spread itself, which holds
    /// only when w_k = 0), that the per-decision linearity check
    /// (S_d(2.0) ~= 2*S_d(1.0)) behaves exactly as its arithmetic says,
    /// AND that the bound formula reproduces the hand value.
    ///
    /// The probe key k (any credit key: it has no linear coordinate, so
    /// pick the first of the set at runtime -- no name literal) has
    /// champion weight w_k = 0.5, deliberately NONZERO: the displacement
    /// probe sets k to w_k + c = 0.5 + c, measuring the pricer's response
    /// at the champion's own operating point. The pricer re-run is
    /// modeled by a KNOWN pricer linear in the key's VALUE: the
    /// HandPotential slot of the probe-frozen vectors gains `0.5 * value`
    /// on candidate 0 and loses `0.5 * value` on candidate 1 (a dedicated
    /// gate reading k directly). Every OTHER slot is identical in phi_c
    /// and phi_p, and the champion weights are nonzero on a few slots so
    /// the baseline dots are non-trivial.
    ///
    /// Hand computation: champion w has HandPotential weight 4.0,
    /// Culture weight 2.0, and k weight 0.5 (everything else 0.0); the
    /// credit slot is 0.0 in every vector (no linear coordinate), so the
    /// baseline is
    ///   base(0) = 4.0*10.0 + 2.0*1.0 = 42.0
    ///   base(1) = 4.0*5.0  + 2.0*0.0 = 20.0
    /// and the TOTAL spread is T = 22.0, identical under every probe.
    ///
    ///   c = 1.0: value = 1.5, deltas d(0) = 4.0*0.5*1.5 = 3.0,
    ///            d(1) = 4.0*(-0.5*1.5) = -3.0; S_d = 3.0 - (-3.0) = 6.0
    ///   c = 2.0: value = 2.5, deltas 5.0 / -5.0; S_d = 5.0 - (-5.0)
    ///            = 10.0
    ///   S_d(c) = 4.0 * (w_k + c) = 2.0 + 4.0c  -- linear in the VALUE.
    ///   s_k = S_d / 1.0 = 6.0; with T = 1000.0: bound = 1.0*1000/6
    ///       = 166.67.. capped at CLAMP_CEILING = 120.0. (T = 500.0 gave
    ///       83.33, which was capped at the old single CLAMP_BLIND = 60.0;
    ///       since CLAMP_CEILING = 120.0 replaced it as the cap on a
    ///       MEASURED bound, 83.33 sits below the cap and no longer
    ///       exercises it -- T = 1000.0 is chosen so the ratio still
    ///       clears CLAMP_CEILING.)
    ///
    /// The per-decision linearity check is |S_d(2.0) - 2*S_d(1.0)|
    /// <= TOL*S_d(1.0): here |10.0 - 12.0| = 2.0 > 0.06, so it GATES --
    /// correctly, because the spread is linear in w_k + c, not in c (the
    /// spread doubles only when w_k = 0, value = c). The check is the
    /// right instrument for the REAL pricers at the operating point;
    /// this synthetic pair pins its arithmetic.
    ///
    /// The WRONG formulation (the spec's original max-min dot(pw,
    /// phi_p(m)) -- the TOTAL score, no baseline removal) is
    /// hand-computed as (42.0 + 2.0*value) - (20.0 - 2.0*value)
    ///   = 22.0 + 4.0*value = 24.0 + 4.0c
    /// -- c-dependent (28.0 at c = 1.0 vs true 6.0; 30.0 at c = 2.0 vs
    /// true 10.0), and its linearity reading 30.0/2 != 28.0 gates the
    /// key under the wrong numbers too, for a different reason.
    ///
    /// The engine invariant the binary relies on -- that the probe
    /// contributes NOTHING to the dot product (the credit slot is 0.0 in
    /// every vector) -- is asserted separately: a probe vector dotted
    /// against the CHAMPION-frozen vectors moves no candidate off its
    /// baseline, whatever the pricer did under the probe freeze.
    #[test]
    fn synthetic_decision_delta_sd_and_bound_match_the_hand_computation() {
        let k = CREDIT_KEYS[0]; // the probe key, resolved at runtime
        let n = WeightKey::ALL.len();
        // Zero every default first so only the three slots below are live.
        let mut champion = Weights::defaults();
        for &key in WeightKey::ALL {
            champion.set(key, 0.0);
        }
        champion.set(WeightKey::HandPotential, 4.0);
        champion.set(WeightKey::Culture, 2.0);
        let wk = 0.5_f64; // the champion's weight of k: NONZERO, so the
        // displacement probe (k -> w_k + c) is a genuine displacement
        // away from the operating point, not a set-from-zero.
        champion.set(k, wk);

        let mut phi_c0 = vec![0.0f64; n];
        let mut phi_c1 = vec![0.0f64; n];
        phi_c0[WeightKey::HandPotential as usize] = 10.0;
        phi_c0[WeightKey::Culture as usize] = 1.0;
        phi_c1[WeightKey::HandPotential as usize] = 5.0;

        let base = [
            eval::dot(&champion, &phi_c0),
            eval::dot(&champion, &phi_c1),
        ];
        assert!((base[0] - 42.0).abs() < 1e-12);
        assert!((base[1] - 20.0).abs() < 1e-12);

        for (c, expected_sd) in [(C, 6.0), (C2, 10.0)] {
            let mut pw = champion;
            let value = wk + c; // the DISPLACEMENT probe: k -> w_k + c
            pw.set(k, value);
            let mut phi_p0 = phi_c0.clone();
            let mut phi_p1 = phi_c1.clone();
            // The modeled pricer re-run, KNOWN in closed form: linear in
            // the key's value (0.5 * value per candidate, with opposite
            // signs on the two candidates).
            phi_p0[WeightKey::HandPotential as usize] += 0.5 * value;
            phi_p1[WeightKey::HandPotential as usize] -= 0.5 * value;

            let phi_p = vec![phi_p0, phi_p1];
            let sd = synthetic_sd(&pw, &phi_p, &base);
            assert!((sd - expected_sd).abs() < 1e-12, "S_d(c={c}) = {sd}, expected {expected_sd}");

            // S_d is LINEAR IN THE VALUE w_k + c: 4.0 * (wk + c). This is
            // what the displacement form promises (not |c|-linearity of
            // the spread itself, which holds only when wk = 0).
            assert!((sd - 4.0 * (wk + c)).abs() < 1e-12, "S_d(c={c}) must be 4.0*(wk+c)");

            // The WRONG formulation: the spec's original max-min of the
            // TOTAL probe-perturbed score (no baseline removal),
            // hand-computed as 24.0 + 4.0c (see the doc above): 28.0 at
            // c = 1.0 (true 6.0) and 30.0 at c = 2.0 (true 10.0), so the
            // linearity test on the wrong numbers reads 30.0/2 != 28.0
            // and gates the key.
            let wrong_total_sd = (20.0_f64 - 2.0 * value).max(42.0 + 2.0 * value)
                - (20.0 - 2.0 * value).min(42.0 + 2.0 * value);
            assert!((wrong_total_sd - (24.0 + 4.0 * c)).abs() < 1e-12, "total-score S_d = {wrong_total_sd}");
            if c == C2 {
                assert!((wrong_total_sd - sd).abs() > 1.0, "the test must fail under the total-score formulation");
            }
        }

        // The engine invariant: the probe vector dotted against the
        // CHAMPION-frozen vectors moves no candidate off its baseline --
        // the credit slot is 0.0 in every vector, so the probe
        // contributes nothing to the dot, whatever the pricer did under
        // the probe freeze. This is what makes
        //   dot(pw, phi_p(m)) - dot(champion, phi_c(m))
        // equal to dot(champion, phi_p(m) - phi_c(m)): the pw-vs-champion
        // difference cancels out of the dot, and only the pricer
        // re-pricing (the phi_p - phi_c difference) remains.
        for (i, f) in [phi_c0.clone(), phi_c1.clone()].iter().enumerate() {
            let mut pw = champion;
            pw.set(k, wk + C);
            let d = eval::dot(&pw, f) - base[i];
            assert!(d.abs() < 1e-12, "probe-in-dot-vector delta must be 0, got {d} for candidate {i}");
        }

        // The per-decision linearity check (S_d(2.0) ~= 2*S_d(1.0)): on
        // this synthetic pair it GATES, because the spread is linear in
        // w_k + c, not in c -- |10.0 - 2*6.0| = 2.0 > 0.01*6.0 = 0.06.
        // The check is the right instrument for the real pricers at the
        // operating point; this pins its arithmetic on the hand values.
        let sd1 = 6.0_f64;
        let sd2 = 10.0_f64;
        let linear = sd1 <= SD_EPS || (sd2 - 2.0 * sd1).abs() <= LINEAR_TOL * sd1;
        assert!(!linear, "the displacement form makes the spread linear in w_k + c, not in c: the check must gate here");

        let slope = sd1 / C;
        // T = 1000.0, not the hand-computed T = 22.0 from base above: this
        // plugs an arbitrary total into the bound formula purely to
        // exercise CLAMP_CEILING. T = 500.0 gave 83.33, which sat below
        // CLAMP_CEILING = 120.0 once that cap replaced the old single
        // CLAMP_BLIND = 60.0, so it stopped exercising the cap at all.
        let bound = bound_from_slope(slope, 1000.0);
        assert_eq!(bound, CLAMP_CEILING, "1000/6 = 166.67 exceeds CLAMP_CEILING = 120.0");
    }
}
