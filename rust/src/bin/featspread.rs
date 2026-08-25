//! `featspread` -- rebuild of the deleted candidate-set spread / eval-share
//! measurement tool that produced `analysis/spread_quantiles_2026-08-24.txt`
//! and `analysis/eval_share_2026-08-24.txt`. That tool was written and run
//! inside its own throwaway isolated clone and cleaned up afterward, per the
//! standing "isolated clone, never touch the live tree" convention -- it
//! never landed in the tracked repo, so those two published tables are
//! currently unreproducible. This is a from-scratch rewrite matching both
//! files' own METHOD sections, landed in the tracked tree this time so the
//! tables stop being unreproducible.
//!
//! # Method (restated from the two published files' own METHOD sections)
//!
//! For each player count in {2, 3, 4}: play `games` self-play games (deal
//! seed `base_seed + game_index`) using [`WeightedBot`] loaded from that
//! count's champion file. At every real decision (`legal_moves` filtered by
//! [`WeightedBot::choose`]'s own `allow_resign` policy still has more than
//! one candidate -- exactly what [`eval::candidate_features`] returns more
//! than one entry for), every legal move is applied to a scratch clone and
//! scored by [`eval::linear_features`] (via `candidate_features`, the SAME
//! function `WeightedBot::rank_moves`/`choose` trial-and-evaluate over), with
//! `freeze` pinned to that count's own champion vector -- never recomputed
//! per candidate weight, matching every other user of these functions in
//! this codebase. The move that actually advances the game is chosen
//! separately by `WeightedBot::choose` on the same state (not re-derived
//! from the candidate scores this binary computes), so self-play visits
//! exactly the decisions the champion actually reaches.
//!
//! Per [`WeightKey`] `k`, per decision: `spread(k) = max(candidate values) -
//! min(candidate values)`. Accumulated per player count across every
//! decision:
//!   fire_rate                = fraction of decisions where spread(k) > 0
//!   mean_spread               = mean spread(k) over ALL decisions
//!   mean_spread_when_firing   = mean spread(k) over FIRING decisions only
//!   p95_spread                = 95th percentile (nearest-rank) of spread(k)
//!                                over FIRING decisions only
//!   max_spread                = maximum spread(k) observed
//!
//! Per decision, also: `total_spread = max(dot(champ_w, phi)) -
//! min(dot(champ_w, phi))` over the candidate set (`dot` from `eval::dot`).
//! p50/p95 of `total_spread` are reported per player count.
//!
//! Finally, per (key, count): `eval_share(k) = abs(champ_w(k)) *
//! p95_spread_when_firing(k) / p95_total_spread(count)` -- each key's share
//! divides by ITS OWN player count's p95 total spread, never a cross-count
//! maximum (a prior version of this tool used a cross-count maximum by
//! mistake and reported a fake 107x outlier -- see this binary's own
//! `eval_share` computation below, which is scoped inside the per-count
//! loop specifically so that mistake cannot recur structurally).
//!
//! ```text
//! cargo run --release --bin featspread -- 40 0 ../experiments
//! ```
//!
//! Percentiles use nearest-rank (`rank = ceil(p/100 * n)`, 1-indexed),
//! matching both published files' own stated convention.

use std::path::Path;
use std::process::ExitCode;
use std::time::Instant;

use tta::bots::weighted::eval::{self, load_weights, WeightedBot};
use tta::bots::weighted::weights::{WeightKey, Weights};
use tta::game::{self, MOVE_CAP};

const PLAYER_COUNTS: [u8; 3] = [2, 3, 4];

struct Args {
    games: usize,
    seed: u64,
    champion_dir: String,
    /// Print the `p95_candidate_spread` match arms as compilable Rust after
    /// the report, so the clamp's ~486 constants are never transcribed by
    /// hand out of a text table.
    emit_rust: bool,
    /// `decisive` mode -- see [`play_decisive`]'s own doc comment. Mutually
    /// exclusive with `emit_rust`; restricted to 3p only (loads
    /// `champion_dir/rust_champion_3p.json`, same join `play_count` already
    /// uses for every other mode -- no new file-naming convention).
    decisive: bool,
}

const USAGE: &str = "usage: featspread <games_per_count> <seed> <champion_dir> [emit|decisive]";

fn parse_args(argv: &[String]) -> Result<Args, String> {
    if argv.len() != 3 && argv.len() != 4 {
        return Err(format!("{USAGE}\ngot {} argument(s), expected 3 or 4", argv.len()));
    }
    let (emit_rust, decisive) = match argv.get(3) {
        None => (false, false),
        Some(flag) if flag == "emit" => (true, false),
        Some(flag) if flag == "decisive" => (false, true),
        Some(flag) => return Err(format!("{USAGE}\nfourth argument must be \"emit\" or \"decisive\", got {flag:?}")),
    };
    let games: usize = argv[0].parse().map_err(|_| format!("games_per_count: {:?} is not a number", argv[0]))?;
    let seed: u64 = argv[1].parse().map_err(|_| format!("seed: {:?} is not a number", argv[1]))?;
    if games == 0 {
        return Err("games_per_count must be at least 1".to_string());
    }
    Ok(Args { games, seed, champion_dir: argv[2].clone(), emit_rust, decisive })
}

/// Nearest-rank percentile (`rank = ceil(p/100 * n)`, 1-indexed) over an
/// already-sorted-ascending slice. `0.0` on an empty slice (a key that never
/// fired has no firing-percentile to report).
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
    v.sort_by(|a, b| a.partial_cmp(b).expect("spread/total_spread values are never NaN"));
    v
}

/// Per-[`WeightKey`] running totals over one player count's whole self-play
/// sample -- `firing_spreads` is kept in full (not just a running sum)
/// because `p95_spread_when_firing` needs the actual distribution, not a
/// moment of it.
#[derive(Default)]
struct KeyAgg {
    firing: u64,
    sum_all: f64,
    sum_firing: f64,
    max_spread: f64,
    firing_spreads: Vec<f64>,
}

impl KeyAgg {
    fn record(&mut self, spread: f64) {
        self.sum_all += spread;
        if spread > 0.0 {
            self.firing += 1;
            self.sum_firing += spread;
            self.firing_spreads.push(spread);
            if spread > self.max_spread {
                self.max_spread = spread;
            }
        }
    }
}

/// One [`WeightKey`]'s fully reduced summary for one player count -- the
/// join point between the per-key table and the eval_share table, computed
/// once so both readers agree.
struct KeySummary {
    key: WeightKey,
    name: &'static str,
    champ_w: f64,
    fire_rate: f64,
    mean_spread: f64,
    mean_spread_firing: f64,
    p95_spread_firing: f64,
    max_spread: f64,
}

/// One player count's fully reduced self-play sample.
struct CountResult {
    players: u8,
    decisions: u64,
    keys: Vec<KeySummary>,
    total_spread_mean: f64,
    total_spread_p50: f64,
    total_spread_p95: f64,
    total_spread_max: f64,
}

/// Play `games` self-play games at `players` under `weights` (loaded from
/// that count's champion file), accumulating candidate-set spread per
/// [`WeightKey`] and per-decision `total_spread`, exactly per this file's
/// own top doc comment.
fn play_count(players: u8, games: usize, base_seed: u64, weights: &Weights) -> CountResult {
    let bot = WeightedBot::new(*weights);
    let n_keys = WeightKey::ALL.len();
    let mut key_agg: Vec<KeyAgg> = (0..n_keys).map(|_| KeyAgg::default()).collect();
    let mut total_spreads: Vec<f64> = Vec::new();
    let mut decisions: u64 = 0;

    // Reused scratch buffers across every decision, reset in place -- avoids
    // one allocation per decision per player count (tens of thousands of
    // decisions at 4p).
    let mut lo = vec![0.0f64; n_keys];
    let mut hi = vec![0.0f64; n_keys];

    for g in 0..games {
        let game_seed = base_seed.wrapping_add(g as u64);
        let mut state = game::new_game(players, game_seed);
        let outcome = game::play_game(&mut state, MOVE_CAP, |s, legal| {
            let candidates = eval::candidate_features(s, legal.as_slice(), bot.allow_resign, weights);
            if candidates.len() > 1 {
                decisions += 1;
                lo.iter_mut().for_each(|x| *x = f64::INFINITY);
                hi.iter_mut().for_each(|x| *x = f64::NEG_INFINITY);
                let mut dot_lo = f64::INFINITY;
                let mut dot_hi = f64::NEG_INFINITY;
                for (_, f) in &candidates {
                    let d = eval::dot(weights, f);
                    if d < dot_lo {
                        dot_lo = d;
                    }
                    if d > dot_hi {
                        dot_hi = d;
                    }
                    for (ki, &v) in f.iter().enumerate() {
                        if v < lo[ki] {
                            lo[ki] = v;
                        }
                        if v > hi[ki] {
                            hi[ki] = v;
                        }
                    }
                }
                for ki in 0..n_keys {
                    key_agg[ki].record(hi[ki] - lo[ki]);
                }
                total_spreads.push(dot_hi - dot_lo);
            }
            bot.choose(s, legal.as_slice())
        });
        if outcome.move_cap_hit {
            eprintln!("featspread: WARNING {players}p game (seed {game_seed}) hit the {MOVE_CAP}-move cap");
        }
    }

    let keys: Vec<KeySummary> = WeightKey::ALL
        .iter()
        .enumerate()
        .map(|(ki, &k)| {
            let agg = &key_agg[ki];
            let firing_sorted = sorted(agg.firing_spreads.clone());
            KeySummary {
                key: k,
                name: k.name(),
                champ_w: weights.get(k),
                fire_rate: if decisions > 0 { agg.firing as f64 / decisions as f64 } else { 0.0 },
                mean_spread: if decisions > 0 { agg.sum_all / decisions as f64 } else { 0.0 },
                mean_spread_firing: if agg.firing > 0 { agg.sum_firing / agg.firing as f64 } else { 0.0 },
                p95_spread_firing: percentile(&firing_sorted, 95.0),
                max_spread: agg.max_spread,
            }
        })
        .collect();

    let ts_sorted = sorted(total_spreads.clone());
    let total_spread_mean =
        if total_spreads.is_empty() { 0.0 } else { total_spreads.iter().sum::<f64>() / total_spreads.len() as f64 };

    CountResult {
        players,
        decisions,
        keys,
        total_spread_mean,
        total_spread_p50: percentile(&ts_sorted, 50.0),
        total_spread_p95: percentile(&ts_sorted, 95.0),
        total_spread_max: ts_sorted.last().copied().unwrap_or(0.0),
    }
}

/// One [`WeightKey`]'s fully reduced `decisive`-mode summary -- see
/// [`play_decisive`]'s own doc comment for what each field measures.
struct DecisiveKeySummary {
    name: &'static str,
    champ_w: f64,
    zero_frac: f64,
    fire_rate: f64,
    median_spread_firing: f64,
    p95_spread_firing: f64,
    mean_abs_level: f64,
    median_swing_firing: f64,
    p95_swing_firing: f64,
    capable_frac: f64,
}

/// Per-key running totals for `decisive` mode, mirroring [`KeyAgg`] but
/// carrying the extra distributions [`play_decisive`]'s report needs that
/// [`KeyAgg`]/[`KeySummary`] do not: the full spread distribution (for a
/// MEDIAN, not just mean/p95), the mean absolute LEVEL of the feature
/// itself (to tell an inert-but-alive key apart from a dead/never-populated
/// one when both show `zero_frac == 1.0`), and the `|w_k| * spread_k`
/// "swing" compared decision-by-decision against that SAME decision's
/// actual winning margin (best candidate score minus runner-up score) --
/// the quantity `p95_total_spread`/`eval_share` in the ordinary report
/// never computes, since `total_spread` there is max-minus-min over the
/// WHOLE candidate set, not best-vs-runner-up.
#[derive(Default)]
struct DecisiveKeyAgg {
    zero: u64,
    firing: u64,
    firing_spreads: Vec<f64>,
    firing_swings: Vec<f64>,
    sum_abs_level: f64,
    n_level_samples: u64,
    /// Count of ALL decisions (firing or not) where `|w_k| * spread_k >=
    /// margin` -- this key's own maximum possible score swing at that
    /// decision point was AT LEAST the gap between the actual winner and
    /// runner-up, i.e. this key alone was structurally capable of being the
    /// deciding factor (not that it WAS -- other keys may have agreed or
    /// disagreed with it; capability, not attribution).
    capable: u64,
}

/// One player count's `decisive`-mode reduced sample, plus the shared
/// (key-independent) winning-margin distribution every key's `capable_frac`
/// is compared against.
struct DecisiveResult {
    decisions: u64,
    keys: Vec<DecisiveKeySummary>,
    margin_mean: f64,
    margin_median: f64,
    margin_p95: f64,
    margin_max: f64,
}

/// `decisive` mode: at every real 3p self-play decision (same
/// `candidates.len() > 1` gate [`play_count`] uses), computes per
/// [`WeightKey`] the candidate-set spread exactly as [`play_count`] does,
/// PLUS two things [`play_count`]'s own report never computes:
///
/// First, `mean_abs_level(k)` -- the mean, over every decision, of the mean
/// `|candidate feature value|` for `k` at that decision. A key stuck at
/// `zero_frac == 1.0` with a near-zero level is simply unused; the same
/// `zero_frac == 1.0` with a large level is a STRUCTURAL constant that is
/// alive (computed, non-trivial) but cannot move that decision's argmax --
/// exactly the `BestMine`/`BestFarm` hypothesis this task exists to check,
/// and the two cases must not be conflated (task's own METHOD REQUIREMENTS
/// point 1).
///
/// Second, `capable_frac(k)` -- the fraction of ALL decisions where
/// `abs(champ_w(k)) times spread_k >= margin`, `margin` being THAT
/// decision's own actual winning margin (best candidate's `dot(w, phi)`
/// minus the runner-up's, both over every scored candidate, every key's
/// weight included, not just `k`'s). This is the task's "decisive
/// quantity": the maximum score swing `k` alone could produce at that
/// decision point, compared against the margin the real winner actually
/// needed to beat the runner-up. A key with `capable_frac` near 0 is
/// structurally incapable of having flipped that decision by itself, at ANY
/// weight on that key -- `spread_k` is fixed by the game state and moves,
/// only `abs(champ_w(k))` is a free dial, so `capable_frac` is monotonic in
/// `abs(champ_w(k))` and this reports the number at the CHAMPION's own
/// weight, not a hypothetical one.
///
/// `freeze` (the `horizon::rate_multiplier` input inside
/// `eval::linear_features`) is pinned to the SAME champion `weights` this
/// function scores candidates with, exactly like every other caller in this
/// binary and every real self-play bot -- see this file's top doc comment
/// and `eval::linear_features`'s own doc comment for why that is not a
/// second, independently-chosen vector.
fn play_decisive(games: usize, base_seed: u64, weights: &Weights) -> DecisiveResult {
    let bot = WeightedBot::new(*weights);
    let n_keys = WeightKey::ALL.len();
    let mut key_agg: Vec<DecisiveKeyAgg> = (0..n_keys).map(|_| DecisiveKeyAgg::default()).collect();
    let mut margins: Vec<f64> = Vec::new();
    let mut decisions: u64 = 0;

    let mut lo = vec![0.0f64; n_keys];
    let mut hi = vec![0.0f64; n_keys];
    let mut abs_sum = vec![0.0f64; n_keys];

    for g in 0..games {
        let game_seed = base_seed.wrapping_add(g as u64);
        let mut state = game::new_game(3, game_seed);
        let outcome = game::play_game(&mut state, MOVE_CAP, |s, legal| {
            let candidates = eval::candidate_features(s, legal.as_slice(), bot.allow_resign, weights);
            if candidates.len() > 1 {
                decisions += 1;
                lo.iter_mut().for_each(|x| *x = f64::INFINITY);
                hi.iter_mut().for_each(|x| *x = f64::NEG_INFINITY);
                abs_sum.iter_mut().for_each(|x| *x = 0.0);
                let mut dots: Vec<f64> = Vec::with_capacity(candidates.len());
                for (_, f) in &candidates {
                    dots.push(eval::dot(weights, f));
                    for (ki, &v) in f.iter().enumerate() {
                        if v < lo[ki] {
                            lo[ki] = v;
                        }
                        if v > hi[ki] {
                            hi[ki] = v;
                        }
                        abs_sum[ki] += v.abs();
                    }
                }
                let n_cand = candidates.len() as f64;
                let mut dots_sorted = dots.clone();
                dots_sorted.sort_by(|a, b| b.partial_cmp(a).expect("dot values are never NaN"));
                let margin = dots_sorted[0] - dots_sorted[1];
                margins.push(margin);
                for ki in 0..n_keys {
                    let spread = hi[ki] - lo[ki];
                    let agg = &mut key_agg[ki];
                    agg.sum_abs_level += abs_sum[ki] / n_cand;
                    agg.n_level_samples += 1;
                    let swing = weights.get(WeightKey::ALL[ki]).abs() * spread;
                    if spread > 0.0 {
                        agg.firing += 1;
                        agg.firing_spreads.push(spread);
                        agg.firing_swings.push(swing);
                    } else {
                        agg.zero += 1;
                    }
                    if swing >= margin {
                        agg.capable += 1;
                    }
                }
            }
            bot.choose(s, legal.as_slice())
        });
        if outcome.move_cap_hit {
            eprintln!("featspread: WARNING decisive 3p game (seed {game_seed}) hit the {MOVE_CAP}-move cap");
        }
    }

    let keys: Vec<DecisiveKeySummary> = WeightKey::ALL
        .iter()
        .enumerate()
        .map(|(ki, &k)| {
            let agg = &key_agg[ki];
            let spreads_sorted = sorted(agg.firing_spreads.clone());
            let swings_sorted = sorted(agg.firing_swings.clone());
            DecisiveKeySummary {
                name: k.name(),
                champ_w: weights.get(k),
                zero_frac: if decisions > 0 { agg.zero as f64 / decisions as f64 } else { 0.0 },
                fire_rate: if decisions > 0 { agg.firing as f64 / decisions as f64 } else { 0.0 },
                median_spread_firing: percentile(&spreads_sorted, 50.0),
                p95_spread_firing: percentile(&spreads_sorted, 95.0),
                mean_abs_level: if agg.n_level_samples > 0 { agg.sum_abs_level / agg.n_level_samples as f64 } else { 0.0 },
                median_swing_firing: percentile(&swings_sorted, 50.0),
                p95_swing_firing: percentile(&swings_sorted, 95.0),
                capable_frac: if decisions > 0 { agg.capable as f64 / decisions as f64 } else { 0.0 },
            }
        })
        .collect();

    let margins_sorted = sorted(margins.clone());
    let margin_mean = if margins.is_empty() { 0.0 } else { margins.iter().sum::<f64>() / margins.len() as f64 };

    DecisiveResult {
        decisions,
        keys,
        margin_mean,
        margin_median: percentile(&margins_sorted, 50.0),
        margin_p95: percentile(&margins_sorted, 95.0),
        margin_max: margins_sorted.last().copied().unwrap_or(0.0),
    }
}

/// Prints the `decisive`-mode report: method recap, the shared margin
/// distribution, then the per-key table sorted by `zero_frac` descending
/// (the task's own required sort order).
fn print_decisive_report(args: &Args, result: &DecisiveResult) {
    println!("================================================================================");
    println!("THROUGH THE AGES -- FEATURE DECISIVENESS (featspread --decisive, 3p only)");
    println!("================================================================================");
    println!("METHOD: at every real 3p self-play decision (candidates.len() > 1) reached by");
    println!("WeightedBot::choose under the champion vector, every legal move is applied to a");
    println!("scratch clone and eval::candidate_features()/eval::dot() scored, freeze pinned to");
    println!("the SAME champion vector. Per WeightKey k, per decision:");
    println!("  spread_k  = max(candidate value) - min(candidate value)      [same as play_count]");
    println!("  level_k   = mean(|candidate value|) over the candidate set   [NEW: alive-vs-dead]");
    println!("  swing_k   = |champ_w(k)| * spread_k                          [max score swing k can cause]");
    println!("  margin    = best candidate dot(w,phi) - runner-up dot(w,phi) [NEW: actual contest margin]");
    println!("capable_frac(k) = fraction of ALL decisions where swing_k >= margin, i.e. k ALONE");
    println!("was structurally capable of flipping that decision's argmax at the CHAMPION's own");
    println!("|w(k)| (spread_k is fixed by state+moves; only |w(k)| is a free dial, so this is");
    println!("evaluated at the champion's actual weight, not a hypothetical one).");
    println!();
    println!("games={} seed={} champion_dir={} decisions={}", args.games, args.seed, args.champion_dir, result.decisions);
    println!();
    println!("WINNING-MARGIN DISTRIBUTION (best - runner-up dot(w,phi), all keys included, per decision):");
    println!(
        "  n={} mean={:.4} median={:.4} p95={:.4} max={:.4}",
        result.decisions, result.margin_mean, result.margin_median, result.margin_p95, result.margin_max
    );
    println!();
    println!("PER-KEY TABLE -- sorted by zero_frac DESCENDING (task's required sort order)");
    println!(
        "{:<28} {:>10} {:>9} {:>9} {:>13} {:>10} {:>13} {:>13} {:>10} {:>13}",
        "key", "champ_w", "zero_frac", "fire_rate", "med_spread", "p95_spread", "mean_level", "med_swing", "p95_swing", "capable_frac"
    );
    let mut rows: Vec<&DecisiveKeySummary> = result.keys.iter().collect();
    rows.sort_by(|a, b| b.zero_frac.partial_cmp(&a.zero_frac).expect("zero_frac is never NaN"));
    for k in rows {
        println!(
            "{:<28} {:>10.4} {:>9.4} {:>9.4} {:>13.6} {:>10.6} {:>13.6} {:>13.6} {:>10.6} {:>13.4}",
            k.name,
            k.champ_w,
            k.zero_frac,
            k.fire_rate,
            k.median_spread_firing,
            k.p95_spread_firing,
            k.mean_abs_level,
            k.median_swing_firing,
            k.p95_swing_firing,
            k.capable_frac
        );
    }
}

fn print_method_header(args: &Args) {
    println!("================================================================================");
    println!("THROUGH THE AGES -- CANDIDATE-SET SPREAD / EVAL-SHARE (featspread rebuild)");
    println!("================================================================================");
    println!("Rebuild of the tool that produced analysis/spread_quantiles_2026-08-24.txt and");
    println!("analysis/eval_share_2026-08-24.txt -- the original was written in a throwaway");
    println!("isolated clone and deleted after that run, so those tables were unreproducible.");
    println!("This binary is landed in the tracked tree (rust/src/bin/featspread.rs) instead.");
    println!();
    println!("METHOD: at every real self-play decision (filter_resign'd legal-move count > 1)");
    println!("reached by WeightedBot::choose under a given player count's champion weights,");
    println!("every legal move is applied to a scratch clone and eval::candidate_features()");
    println!("(== eval::linear_features run once per candidate) is read for each, with");
    println!("`freeze` pinned to that count's champion vector. Per WeightKey, per decision:");
    println!("  spread(k) = max(candidate values) - min(candidate values)");
    println!("Accumulated per player count: fire_rate (fraction of decisions with spread>0),");
    println!("mean_spread (over ALL decisions), mean_spread_when_firing / p95_spread / ");
    println!("max_spread (over FIRING decisions only, p95 nearest-rank). Per decision also:");
    println!("  total_spread = max(dot(champ_w, phi)) - min(dot(champ_w, phi))");
    println!("over the candidate set, p50/p95 reported per player count. Per (key, count):");
    println!("  eval_share(k) = abs(champ_w(k)) * p95_spread_when_firing(k) / p95_total_spread");
    println!("using THAT key's OWN player count's p95_total_spread, never a cross-count max.");
    println!("The move that actually advances each game is chosen separately by");
    println!("WeightedBot::choose on the same state, not re-derived from these scores.");
    println!();
    println!("games_per_count={} seed={} champion_dir={}", args.games, args.seed, args.champion_dir);
    println!();
}

fn print_key_table(count: &CountResult) {
    println!("-- {}p --", count.players);
    println!(
        "{:<28} {:>8} {:>10} {:>14} {:>24} {:>14} {:>14}",
        "key", "n", "fire_rate", "mean_spread", "mean_spread_when_firing", "p95_spread", "max_spread"
    );
    for k in &count.keys {
        println!(
            "{:<28} {:>8} {:>10.4} {:>14.6} {:>24.6} {:>14.6} {:>14.6}",
            k.name, count.decisions, k.fire_rate, k.mean_spread, k.mean_spread_firing, k.p95_spread_firing, k.max_spread
        );
    }
    println!();
}

fn print_total_spread_quantiles(results: &[CountResult]) {
    println!("================================================================================");
    println!("TOTAL SPREAD QUANTILES -- max(dot(w,phi)) - min(dot(w,phi)) over the candidate");
    println!("set, per decision, per player count");
    println!("================================================================================");
    println!("{:<8} {:>12} {:>12} {:>12} {:>12} {:>12}", "count", "n_decisions", "mean", "p50", "p95", "max");
    for r in results {
        println!(
            "{:<8} {:>12} {:>12.3} {:>12.3} {:>12.3} {:>12.3}",
            format!("{}p", r.players),
            r.decisions,
            r.total_spread_mean,
            r.total_spread_p50,
            r.total_spread_p95,
            r.total_spread_max
        );
    }
    println!();
}

fn print_eval_share(results: &[CountResult]) {
    println!("================================================================================");
    println!("EVAL SHARE -- eval_share(k) = abs(champ_w(k)) * p95_spread_when_firing(k) /");
    println!("p95_total_spread(count), sorted descending");
    println!("================================================================================");
    println!(
        "{:<28} {:>6} {:>12} {:>10} {:>12} {:>18} {:>12}",
        "key", "count", "eval_share", "fire_rate", "champ_w", "p95_spread_firing", "p95_total"
    );

    let mut rows: Vec<(f64, String)> = Vec::new();
    for r in results {
        for k in &r.keys {
            if r.total_spread_p95 <= 0.0 {
                continue;
            }
            let share = k.champ_w.abs() * k.p95_spread_firing / r.total_spread_p95;
            let line = format!(
                "{:<28} {:>5}p {:>12.4} {:>10.4} {:>12.4} {:>18.4} {:>12.3}",
                k.name, r.players, share, k.fire_rate, k.champ_w, k.p95_spread_firing, r.total_spread_p95
            );
            rows.push((share, line));
        }
    }
    rows.sort_by(|a, b| b.0.partial_cmp(&a.0).expect("eval_share values are never NaN"));
    for (_, line) in &rows {
        println!("{line}");
    }
    println!();

    let over_one: Vec<&(f64, String)> = rows.iter().filter(|(share, _)| *share > 1.0).collect();
    println!("Keys (key, player-count) pairs with eval_share > 1.0: {}", over_one.len());
    for (share, line) in &over_one {
        println!("  {share:.4}  {line}");
    }
}

/// Print this run's per-key p95 firing spreads as the literal body of
/// `WeightKey::p95_candidate_spread`, plus the per-count total-spread
/// constants that go with them.
///
/// The clamp needs one bound per (key, player count) -- 486 numbers. Nobody
/// should ever type those out of a text table, and nobody should have to
/// trust that whoever did got the columns right: the previous hand-derived
/// bound divided per-count weights by a cross-count maximum and reported a
/// fake 107x outlier that survived into a message to Paul. Emitting the
/// arms straight from the same `KeySummary` values the report prints makes
/// that class of transcription error impossible.
///
/// A key whose spread is `0.0` at every count is not harmless, it is
/// INVISIBLE TO THIS INSTRUMENT: `linear_features` prices the multiplier and
/// credit keys at the caller's frozen vector, so their candidate-set spread
/// is zero by construction while they still move real move ranking. Their
/// arm is emitted as zeros and `clamp_bound` must fall back to the flat
/// historical rail for them rather than inventing a tighter or looser
/// number -- nine of them carry live champion weights between 6 and 27.
fn print_clamp_table(results: &[CountResult]) {
    println!();
    println!("================================================================================");
    println!("RUST TABLE -- paste as the body of WeightKey::p95_candidate_spread");
    println!("================================================================================");
    for r in results {
        println!("// {}p: decisions {} p95_total_spread {:.6}", r.players, r.decisions, r.total_spread_p95);
    }
    print!("const P95_TOTAL_SPREAD: [f64; 3] = [");
    let totals: Vec<String> = results.iter().map(|r| format!("{:.6}", r.total_spread_p95)).collect();
    println!("{}];", totals.join(", "));
    println!("match self {{");
    for k in WeightKey::ALL.iter().copied() {
        let cells: Vec<String> = results
            .iter()
            .map(|r| {
                let v = r.keys.iter().find(|s| s.key == k).map_or(0.0, |s| s.p95_spread_firing);
                format!("{v:.6}")
            })
            .collect();
        println!("    WeightKey::{:?} => [{}],", k, cells.join(", "));
    }
    println!("}}");
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_args(&argv) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("featspread: {e}");
            return ExitCode::FAILURE;
        }
    };

    if args.decisive {
        let path = Path::new(&args.champion_dir).join("rust_champion_3p.json");
        let weights = match load_weights(&path) {
            Ok(w) => w,
            Err(e) => {
                eprintln!("featspread: {e}");
                return ExitCode::FAILURE;
            }
        };
        let started = Instant::now();
        let result = play_decisive(args.games, args.seed, &weights);
        eprintln!(
            "featspread: decisive 3p done ({} games, {} decisions, {:.1}s)",
            args.games,
            result.decisions,
            started.elapsed().as_secs_f64()
        );
        print_decisive_report(&args, &result);
        return ExitCode::SUCCESS;
    }

    print_method_header(&args);

    let started = Instant::now();
    let mut results: Vec<CountResult> = Vec::new();
    for &players in &PLAYER_COUNTS {
        let path = Path::new(&args.champion_dir).join(format!("rust_champion_{players}p.json"));
        let weights = match load_weights(&path) {
            Ok(w) => w,
            Err(e) => {
                eprintln!("featspread: {e}");
                return ExitCode::FAILURE;
            }
        };
        let count_started = Instant::now();
        let result = play_count(players, args.games, args.seed, &weights);
        eprintln!(
            "featspread: {}p done ({} games, {} decisions, {:.1}s)",
            players,
            args.games,
            result.decisions,
            count_started.elapsed().as_secs_f64()
        );
        results.push(result);
    }

    println!("================================================================================");
    println!("PER-KEY TABLE (all {} WeightKeys, at 2p/3p/4p)", WeightKey::ALL.len());
    println!("================================================================================");
    println!("Columns: key | n (decisions sampled) | fire_rate | mean_spread |");
    println!("mean_spread_when_firing | p95_spread | max_spread");
    println!();
    for r in &results {
        print_key_table(r);
    }

    print_total_spread_quantiles(&results);
    print_eval_share(&results);
    if args.emit_rust {
        print_clamp_table(&results);
    }

    eprintln!("featspread: total elapsed {:.1}s", started.elapsed().as_secs_f64());
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_nearest_rank_p95_of_twenty_ascending_values_is_the_nineteenth() {
        let v: Vec<f64> = (1..=20).map(|x| x as f64).collect();
        // rank = ceil(0.95 * 20) = 19 -> 1-indexed 19th value = 19.0
        assert_eq!(percentile(&v, 95.0), 19.0);
    }

    #[test]
    fn percentile_of_empty_slice_is_zero() {
        assert_eq!(percentile(&[], 95.0), 0.0);
    }

    #[test]
    fn percentile_p50_of_single_value_is_that_value() {
        assert_eq!(percentile(&[42.0], 50.0), 42.0);
    }

    #[test]
    fn parse_args_rejects_wrong_argument_count() {
        assert!(parse_args(&["1".to_string(), "2".to_string()]).is_err());
    }

    #[test]
    fn parse_args_rejects_zero_games() {
        let argv = vec!["0".to_string(), "0".to_string(), "dir".to_string()];
        assert!(parse_args(&argv).is_err());
    }

    #[test]
    fn parse_args_reads_positional_fields() {
        let argv = vec!["40".to_string(), "7".to_string(), "../experiments".to_string()];
        let a = parse_args(&argv).expect("valid args");
        assert_eq!(a.games, 40);
        assert_eq!(a.seed, 7);
        assert_eq!(a.champion_dir, "../experiments");
    }

    #[test]
    fn key_agg_record_only_counts_positive_spreads_as_firing() {
        let mut agg = KeyAgg::default();
        agg.record(0.0);
        agg.record(5.0);
        agg.record(3.0);
        assert_eq!(agg.firing, 2);
        assert_eq!(agg.max_spread, 5.0);
        assert!((agg.sum_all - 8.0).abs() < 1e-12);
        assert!((agg.sum_firing - 8.0).abs() < 1e-12);
    }
}
