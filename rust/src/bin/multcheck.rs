//! `multcheck` -- counterfactual argmax-flip measurement for the family of
//! `WeightKey`s that act purely as MULTIPLIERS inside other features'
//! computation (e.g. `cards::tech_value`'s `tb * tech_value(..)` at
//! `cards.rs`), and therefore never occupy a slot in the linear feature
//! vector `phi` that `eval::linear_features`/`candidate_features` returns.
//!
//! `bin/featspread.rs`'s candidate-set SPREAD instrument is structurally
//! blind to this family: a key with no `phi` slot has `spread == 0.0` by
//! construction at every decision, in every player count, no matter how much
//! it changes real move ranking through the keys it multiplies into. The
//! correct instrument for that family is a COUNTERFACTUAL ARGMAX FLIP: at
//! each real decision, recompute every candidate's score with the key
//! perturbed and see whether the move that would be CHOSEN changes.
//!
//! # Identifying the multiplier-only family (no literal names, ever)
//!
//! A `WeightKey` variant name is never typed as a string literal anywhere in
//! this file (repo-wide rule enforced by
//! `registry.rs::tests::every_weight_key_is_named_by_production_source_outside_its_own_declaration`).
//! The set is instead derived at RUNTIME, in two steps:
//!
//! 1. [`FEATURES_SRC`] embeds `features.rs`'s own source text (a file path,
//!    not a key name). For every `WeightKey::ALL` member `k`,
//!    `format!("f.set(WeightKey::{k:?}")` (built from `Debug`, never typed)
//!    is checked as a substring -- keys that never appear there are
//!    candidates (`not_in_fset`, ~54 of 160).
//! 2. `not_in_fset` still contains keys that DO get a `phi` slot some other
//!    way: eleven "identity-aware, freeze-priced gates" plus `StrengthRel`
//!    are written directly by `eval::linear_features` via
//!    `out[WeightKey::X as usize] = ...`, five more (the `.early()`/
//!    `.late()` phase-suffixed keys) are reached only through that
//!    indirection (so they have NO literal call site either, same as a true
//!    multiplier -- a pure text scan cannot tell them apart), and
//!    `EndTurnBias` is set in `candidate_features` itself. All of these DO
//!    get real, nonzero candidate-set spread. A true multiplier-only key's
//!    `phi` slot is never written by ANY code path, so its spread is
//!    STRUCTURALLY, deterministically `0.0` at every decision, forever.
//!    [`classify_multiplier_keys`] runs a small self-play classification
//!    pass and keeps only the `not_in_fset` keys whose spread was `0.0` at
//!    every sampled decision -- this is exactly the task's own stated
//!    fallback method ("p95_candidate_spread == [0,0,0] AND not set in
//!    features.rs"), just computed from the live candidate set instead of
//!    the pre-baked table (which is a private fn on `WeightKey` and cannot
//!    be reached from a `bin/`).
//!
//! # The measurement
//!
//! At every real 3p self-play decision (`candidates.len() > 1`, same gate
//! `featspread` uses) reached by [`WeightedBot::choose`] under the frozen
//! champion, for every key in the classified set and each of two
//! perturbations (ZERO: set to `0.0`; ABS: set to `champion_value.abs()`),
//! a COPY of the champion weights with only that one key changed is used as
//! `freeze` for a fresh [`eval::candidate_features`] call over the SAME
//! state and legal-move list the champion faced (determinization is a pure
//! function of `state`, never of `freeze`, so this is the SAME candidate set
//! the champion scored, just re-priced) -- see `candidate_features`'s own
//! doc comment. The perturbed argmax (first-candidate-wins ties, matching
//! [`WeightedBot::choose`]'s own tie-break) is compared to the champion's
//! own argmax over the SAME baseline candidate set: a mismatch is a FLIP.
//! `term_nonzero` (computed once, off the ZERO perturbation) counts
//! decisions where at least one candidate's score actually changed when the
//! key was zeroed, i.e. the quantity it multiplies was nonzero somewhere in
//! that decision's candidate set at all -- independent of whether it went on
//! to flip the argmax.
//!
//! The move that actually advances each game is chosen separately by
//! [`WeightedBot::choose`] on the unperturbed state (never re-derived from
//! any of these scores), so self-play visits exactly the decisions the
//! champion actually reaches -- same discipline as `featspread.rs`.
//!
//! ```text
//! cargo run --release --bin multcheck -- <games> <seed> <threads> <champion_json> <players 2|3|4>
//! ```

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use tta::bots::weighted::eval::{self, load_weights, WeightedBot};
use tta::bots::weighted::weights::{WeightKey, Weights};
use tta::game::{self, MOVE_CAP};

/// `features.rs`'s own source text, embedded at compile time -- a file
/// path, never a `WeightKey` variant name. Used only for a substring scan
/// against `Debug`-formatted key names built at runtime (see this file's
/// top doc comment, step 1).
const FEATURES_SRC: &str = include_str!("../bots/weighted/features.rs");

/// True if `k` appears as `f.set(WeightKey::<k>` in `features.rs` -- the
/// needle is built from `Debug` at runtime, never typed as a literal.
fn set_in_features_rs(k: WeightKey) -> bool {
    let needle = format!("f.set(WeightKey::{k:?}");
    FEATURES_SRC.contains(&needle)
}

struct Args {
    games: usize,
    seed: u64,
    threads: usize,
    champion_path: String,
    players: u8,
}

const USAGE: &str = "usage: multcheck <games> <seed> <threads> <champion_json> <players 2|3|4>";

fn parse_args(argv: &[String]) -> Result<Args, String> {
    if argv.len() != 5 {
        return Err(format!("{USAGE}\ngot {} argument(s), expected 5", argv.len()));
    }
    let games: usize = argv[0].parse().map_err(|_| format!("games: {:?} is not a number", argv[0]))?;
    let seed: u64 = argv[1].parse().map_err(|_| format!("seed: {:?} is not a number", argv[1]))?;
    let threads: usize = argv[2].parse().map_err(|_| format!("threads: {:?} is not a number", argv[2]))?;
    let players: u8 = argv[4].parse().map_err(|_| format!("players: {:?} is not a number", argv[4]))?;
    if games == 0 {
        return Err("games must be at least 1".to_string());
    }
    if threads == 0 {
        return Err("threads must be at least 1".to_string());
    }
    if !(2..=4).contains(&players) {
        return Err(format!("players must be 2, 3, or 4, got {players}"));
    }
    Ok(Args { games, seed, threads, champion_path: argv[3].clone(), players })
}

/// First-candidate-wins argmax over `scores`, matching
/// [`WeightedBot::choose`]'s own tie-break (`is_none_or(|(_, bv)| val >
/// bv)`, strict `>` only).
fn argmax(scores: &[f64]) -> usize {
    let mut best = 0usize;
    for (i, &v) in scores.iter().enumerate().skip(1) {
        if v > scores[best] {
            best = i;
        }
    }
    best
}

/// One key's fully reduced result.
struct KeyResult {
    key: WeightKey,
    champ_w: f64,
    flips_zero: u64,
    flips_abs: u64,
    term_nonzero: u64,
}

/// Per-key running totals, indexed in parallel with a caller-owned
/// `Vec<WeightKey>` (never `WeightKey::ALL` directly -- only the classified
/// multiplier-only subset is tracked here).
#[derive(Default, Clone)]
struct KeyAgg {
    flips_zero: u64,
    flips_abs: u64,
    term_nonzero: u64,
}

/// Small self-play classification pass (step 2 of this file's top doc
/// comment): plays `games` 3p games under `weights` and, for every key in
/// `candidates`, accumulates whether its candidate-set spread was EVER
/// nonzero. Returns the subset that stayed at exactly `0.0` spread across
/// every sampled decision -- the structural, deterministic signature of a
/// key with no `phi` slot at all.
fn classify_multiplier_keys(candidates: &[WeightKey], games: usize, seed: u64, players: u8, weights: &Weights) -> Vec<WeightKey> {
    let bot = WeightedBot::new(*weights);
    let mut ever_nonzero = vec![false; candidates.len()];
    let mut decisions = 0u64;

    for g in 0..games {
        let game_seed = seed.wrapping_add(g as u64);
        let mut state = game::new_game(players, game_seed);
        game::play_game(&mut state, MOVE_CAP, |s, legal| {
            let cf = eval::candidate_features(s, legal.as_slice(), bot.allow_resign, weights);
            if cf.len() > 1 {
                decisions += 1;
                for (ci, &k) in candidates.iter().enumerate() {
                    if ever_nonzero[ci] {
                        continue;
                    }
                    let mut lo = f64::INFINITY;
                    let mut hi = f64::NEG_INFINITY;
                    for (_, f) in &cf {
                        let v = f[k as usize];
                        if v < lo {
                            lo = v;
                        }
                        if v > hi {
                            hi = v;
                        }
                    }
                    if hi - lo != 0.0 {
                        ever_nonzero[ci] = true;
                    }
                }
            }
            bot.choose(s, legal.as_slice())
        });
    }

    eprintln!("multcheck: classification pass done ({games} games, {decisions} decisions)");
    candidates.iter().zip(ever_nonzero.iter()).filter(|(_, &nz)| !nz).map(|(&k, _)| k).collect()
}

/// One thread's share of the main measurement: plays whole games (claimed
/// off `next`), at every real decision scoring the baseline candidate set
/// once under the champion weights, then once per (key, perturbation) under
/// a copy of the champion weights with only that key changed -- see this
/// file's top doc comment for exactly what a "flip" and `term_nonzero`
/// mean.
fn play_shard(
    keys: &[WeightKey],
    games: usize,
    seed: u64,
    players: u8,
    next: &AtomicUsize,
    weights: &Weights,
) -> (Vec<KeyAgg>, u64) {
    let bot = WeightedBot::new(*weights);
    let mut agg: Vec<KeyAgg> = vec![KeyAgg::default(); keys.len()];
    let mut decisions = 0u64;

    loop {
        let g = next.fetch_add(1, Ordering::Relaxed);
        if g >= games {
            break;
        }
        let game_seed = seed.wrapping_add(g as u64);
        let mut state = game::new_game(players, game_seed);
        let outcome = game::play_game(&mut state, MOVE_CAP, |s, legal| {
            let baseline = eval::candidate_features(s, legal.as_slice(), bot.allow_resign, weights);
            if baseline.len() > 1 {
                decisions += 1;
                let base_scores: Vec<f64> = baseline.iter().map(|(_, f)| eval::dot(weights, f)).collect();
                let base_idx = argmax(&base_scores);
                let base_move = baseline[base_idx].0;

                for (ki, &k) in keys.iter().enumerate() {
                    let champ_w = weights.get(k);
                    for (is_abs, pert_val) in [(false, 0.0), (true, champ_w.abs())] {
                        let mut pw = *weights;
                        pw.set(k, pert_val);
                        let cand = eval::candidate_features(s, legal.as_slice(), bot.allow_resign, &pw);
                        let scores: Vec<f64> = cand.iter().map(|(_, f)| eval::dot(&pw, f)).collect();
                        let pidx = argmax(&scores);
                        let flipped = cand[pidx].0 != base_move;
                        if is_abs {
                            if flipped {
                                agg[ki].flips_abs += 1;
                            }
                        } else {
                            if flipped {
                                agg[ki].flips_zero += 1;
                            }
                            let touched = scores
                                .iter()
                                .zip(base_scores.iter())
                                .any(|(&a, &b)| (a - b).abs() > 1e-9);
                            if touched {
                                agg[ki].term_nonzero += 1;
                            }
                        }
                    }
                }
            }
            bot.choose(s, legal.as_slice())
        });
        if outcome.move_cap_hit {
            eprintln!("multcheck: WARNING {players}p game (seed {game_seed}) hit the {MOVE_CAP}-move cap");
        }
    }

    (agg, decisions)
}

fn main() -> std::process::ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_args(&argv) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("multcheck: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };

    let weights = match load_weights(Path::new(&args.champion_path)) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("multcheck: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };

    let not_in_fset: Vec<WeightKey> = WeightKey::ALL.iter().copied().filter(|&k| !set_in_features_rs(k)).collect();
    eprintln!("multcheck: {} of {} WeightKeys never appear in an f.set(...) in features.rs", not_in_fset.len(), WeightKey::ALL.len());

    let started = Instant::now();
    let mult_keys = classify_multiplier_keys(&not_in_fset, 60, args.seed, args.players, &weights);
    eprintln!(
        "multcheck: {} of those {} have candidate-set spread == 0.0 at every sampled decision -- \
         this is the multiplier-only family ({:.1}s)",
        mult_keys.len(),
        not_in_fset.len(),
        started.elapsed().as_secs_f64()
    );

    let main_started = Instant::now();
    let next = AtomicUsize::new(0);
    let threads = args.threads.min(args.games);
    let mut total_agg: Vec<KeyAgg> = vec![KeyAgg::default(); mult_keys.len()];
    let mut total_decisions = 0u64;

    std::thread::scope(|scope| {
        let (keys, games, seed, players, next, weights) = (&mult_keys, args.games, args.seed, args.players, &next, &weights);
        let mut handles = Vec::with_capacity(threads);
        for _ in 0..threads {
            handles.push(scope.spawn(move || play_shard(keys, games, seed, players, next, weights)));
        }
        for h in handles {
            let (agg, decisions) = h.join().expect("multcheck worker thread panicked");
            total_decisions += decisions;
            for (ti, a) in agg.into_iter().enumerate() {
                total_agg[ti].flips_zero += a.flips_zero;
                total_agg[ti].flips_abs += a.flips_abs;
                total_agg[ti].term_nonzero += a.term_nonzero;
            }
        }
    });

    let elapsed = main_started.elapsed().as_secs_f64();
    eprintln!(
        "multcheck: main pass done ({} games, {} decisions, {} keys, {:.1}s)",
        args.games,
        total_decisions,
        mult_keys.len(),
        elapsed
    );

    let mut results: Vec<KeyResult> = mult_keys
        .iter()
        .zip(total_agg.iter())
        .map(|(&key, a)| KeyResult {
            key,
            champ_w: weights.get(key),
            flips_zero: a.flips_zero,
            flips_abs: a.flips_abs,
            term_nonzero: a.term_nonzero,
        })
        .collect();
    results.sort_by(|a, b| {
        let ra = (a.flips_zero as f64 / total_decisions.max(1) as f64).max(a.flips_abs as f64 / total_decisions.max(1) as f64);
        let rb = (b.flips_zero as f64 / total_decisions.max(1) as f64).max(b.flips_abs as f64 / total_decisions.max(1) as f64);
        rb.partial_cmp(&ra).expect("flip rates are never NaN")
    });

    println!(
        "players={} games={} seed={} threads={} champion={} decisions={}",
        args.players, args.games, args.seed, args.threads, args.champion_path, total_decisions
    );
    println!("not_in_fset={} mult_keys={}", not_in_fset.len(), mult_keys.len());
    println!(
        "{:<28} {:>12} {:>14} {:>13} {:>12} {:>14}",
        "key", "champ_w", "flip_rate_zero", "flip_rate_abs", "flips_zero", "flips_abs"
    );
    for r in &results {
        let fz = r.flips_zero as f64 / total_decisions.max(1) as f64;
        let fa = r.flips_abs as f64 / total_decisions.max(1) as f64;
        let tn = r.term_nonzero as f64 / total_decisions.max(1) as f64;
        println!(
            "{:<28} {:>12.4} {:>14.6} {:>13.6} {:>12} {:>14} term_nonzero_rate={:.6} term_nonzero_n={}",
            r.key.name(),
            r.champ_w,
            fz,
            fa,
            r.flips_zero,
            r.flips_abs,
            tn,
            r.term_nonzero
        );
    }

    std::process::ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let argv =
            vec!["0".to_string(), "1".to_string(), "1".to_string(), "champ.json".to_string(), "3".to_string()];
        assert!(parse_args(&argv).is_err());
    }

    #[test]
    fn parse_args_rejects_zero_threads() {
        let argv =
            vec!["1".to_string(), "1".to_string(), "0".to_string(), "champ.json".to_string(), "3".to_string()];
        assert!(parse_args(&argv).is_err());
    }

    #[test]
    fn parse_args_rejects_out_of_range_players() {
        let argv =
            vec!["1".to_string(), "1".to_string(), "1".to_string(), "champ.json".to_string(), "5".to_string()];
        assert!(parse_args(&argv).is_err());
    }

    #[test]
    fn parse_args_reads_positional_fields() {
        let argv = vec![
            "10".to_string(),
            "1001".to_string(),
            "5".to_string(),
            "champ3p.json".to_string(),
            "3".to_string(),
        ];
        let a = parse_args(&argv).expect("valid args");
        assert_eq!(a.games, 10);
        assert_eq!(a.seed, 1001);
        assert_eq!(a.threads, 5);
        assert_eq!(a.champion_path, "champ3p.json");
        assert_eq!(a.players, 3);
    }

    #[test]
    fn set_in_features_rs_finds_a_key_known_to_be_set_there() {
        // Culture is set via `f.set(WeightKey::Culture, ...)` in features.rs
        // -- picked at runtime (first ALL member whose name, formatted via
        // Debug, is found in the embedded source) rather than typed as a
        // literal, per this file's own hard constraint.
        let found = WeightKey::ALL.iter().copied().find(|&k| set_in_features_rs(k));
        assert!(found.is_some(), "expected at least one WeightKey to be set via f.set(...) in features.rs");
    }

    #[test]
    fn not_every_weight_key_is_set_in_features_rs() {
        // The multiplier-only family must be non-empty for this binary's
        // classification step to have anything to classify.
        let missing = WeightKey::ALL.iter().copied().filter(|&k| !set_in_features_rs(k)).count();
        assert!(missing > 0, "expected at least one WeightKey absent from features.rs's f.set(...) call sites");
    }
}
