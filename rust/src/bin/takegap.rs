//! `takegap` -- exact per-[`WeightKey`] decomposition of the margin by which
//! the champion's CHOSEN move beats the best legal `Move::Take`, for the
//! "the bot takes 17.5 cards a game where humans take 26.9" defect.
//!
//! Same construction as `bin/farmgap.rs`, and for the same reason: the leaf
//! eval is exactly `score = w . phi(state, w)`, so at one decision the score
//! gap between two candidates decomposes EXACTLY into per-key terms
//! `w_k * (phi_A[k] - phi_B[k])`. The feature vectors come from
//! [`eval::candidate_features`] -- the SAME vector `WeightedBot::choose`/
//! `rank_moves` dot against `w` -- so there is no second arithmetic path to
//! get wrong.
//!
//! # Method
//!
//! 2p self-play mirror matches with a frozen champion snapshot at
//! `/tmp/takegap/base.json` (the live `experiments/rust_champion_2p.json` is
//! rewritten by the climb mid-run and must never be read directly here). At
//! every own-turn decision (`Move::EndTurn` legal -- `openerprobe`'s
//! convention for "the player's own choice" as opposed to answering a
//! pending sub-decision) where at least one `Move::Take` is legal:
//!
//! * score every candidate as `dot(w, phi)`, argmax first-wins (exactly
//!   `choose`'s tie-break);
//! * if the argmax is NOT a `Take`, record the per-key decomposition of
//!   `score(chosen) - score(best legal Take)`.
//!
//! Every recorded point is cross-checked two ways: the summed contributions
//! against `dot(w,phi_chosen) - dot(w,phi_take)`, and every candidate's
//! `dot(w,phi)` against `rank_moves`' own independently-computed `evaluate`
//! score. Both maxima are printed. They are judged against the ULP of the
//! largest score seen, not against a fixed absolute bound: scores here reach
//! ~1e3, where one double ULP is already ~2e-13, so "residual < 1e-13" is
//! below the precision of exactly-correct arithmetic and cannot be a test of
//! anything. A residual of a few ULP is float reassociation; a residual
//! orders of magnitude above that means the feature vectors are not the ones
//! the bot ranks with and the table below it is meaningless.
//!
//! Diagnosis only -- no weight is written, and nothing outside this file is
//! touched.
//!
//! ```text
//! cargo run --release --bin takegap
//! ```
use std::collections::HashMap;
use std::path::Path;

use tta::bots::weighted::eval::{candidate_features, dot, load_weights, WeightedBot};
use tta::bots::weighted::weights::{WeightKey, Weights};
use tta::game;
use tta::legal;
use tta::moves::Move;

/// Contributions below this in absolute value are counted as "zero" for the
/// nonzero-share column -- float noise from `evaluate`, not a real term.
const ZERO_EPS: f64 = 1e-12;

/// One recorded decision: the bot had a legal `Take` and preferred something
/// else.
struct Point {
    round: u16,
    /// 1-based rank of the best legal `Take` in the full candidate ordering.
    take_rank: usize,
    /// Candidates at this decision (post-`filter_resign`).
    candidates: usize,
    /// `gap` as a fraction of the decision's own best-minus-worst score
    /// spread -- the only scale on which "is the margin tiny?" has an
    /// answer, since raw scores here are ~1e3 and differ by ~1e1.
    gap_frac: f64,
    /// `w_k * (phi_chosen[k] - phi_take[k])`, indexed by `WeightKey as usize`.
    contrib: Vec<f64>,
    /// `score(chosen) - score(best legal Take)`, always > 0 by construction.
    gap: f64,
}

/// Running per-key totals over a set of points.
#[derive(Default)]
struct Bucket {
    points: usize,
    gap_sum: f64,
    sum: Vec<f64>,
    nonzero: Vec<usize>,
}

impl Bucket {
    fn new() -> Bucket {
        Bucket { points: 0, gap_sum: 0.0, sum: vec![0.0; WeightKey::ALL.len()], nonzero: vec![0; WeightKey::ALL.len()] }
    }

    fn add(&mut self, p: &Point) {
        self.points += 1;
        self.gap_sum += p.gap;
        for (i, &c) in p.contrib.iter().enumerate() {
            self.sum[i] += c;
            if c.abs() > ZERO_EPS {
                self.nonzero[i] += 1;
            }
        }
    }

    fn report(&self, label: &str, weights: &Weights, top: usize) {
        println!("\n=== {label}: {} decisions, mean gap {:.4} ===", self.points, self.mean_gap());
        if self.points == 0 {
            return;
        }
        let mut rows: Vec<(WeightKey, f64, f64, f64)> = WeightKey::ALL
            .iter()
            .enumerate()
            .map(|(i, &k)| {
                let mean = self.sum[i] / self.points as f64;
                let share = if self.gap_sum == 0.0 { 0.0 } else { self.sum[i] / self.gap_sum };
                let nz = self.nonzero[i] as f64 / self.points as f64;
                (k, mean, share, nz)
            })
            .collect();
        rows.sort_by(|a, b| b.1.abs().partial_cmp(&a.1.abs()).expect("no NaN contributions"));
        rows.truncate(top);
        println!("{:<26} {:>12} {:>14} {:>10} {:>10}", "WeightKey", "w_k", "mean contrib", "share", "nonzero%");
        for (k, mean, share, nz) in &rows {
            println!(
                "{:<26} {:>12.5} {:>14.5} {:>9.1}% {:>9.1}%",
                format!("{k:?}"),
                weights.get(*k),
                mean,
                share * 100.0,
                nz * 100.0
            );
        }
    }

    fn mean_gap(&self) -> f64 {
        if self.points == 0 {
            0.0
        } else {
            self.gap_sum / self.points as f64
        }
    }
}

/// Totals over the whole sweep, not per-bucket.
#[derive(Default)]
struct Counts {
    own_turn_decisions: usize,
    with_legal_take: usize,
    take_was_rank1: usize,
    take_chosen_by_round: HashMap<u16, usize>,
}

fn argmax_first_wins(scores: &[f64]) -> usize {
    let mut best = 0usize;
    for (i, &s) in scores.iter().enumerate() {
        if s > scores[best] {
            best = i;
        }
    }
    best
}

/// Walk one self-play game, recording every own-turn decision at which a
/// `Take` was legal but not chosen. Returns
/// `(max decomposition residual, max dot-vs-evaluate residual, max |score|)`.
fn scan_game(
    seed: u64,
    weights: &Weights,
    bot: &WeightedBot,
    counts: &mut Counts,
    out: &mut Vec<Point>,
) -> (f64, f64, f64) {
    // (absolute decomposition residual, absolute dot-vs-`evaluate`
    // residual, largest |score| seen). The scale travels with the residuals
    // because it is what makes them readable: an absolute residual only
    // means something next to the ULP of the numbers it came out of.
    let mut max_decomp: f64 = 0.0;
    let mut max_eval: f64 = 0.0;
    let mut max_scale: f64 = 0.0;
    let mut state = game::new_game(2, seed);
    while !game::is_over(&state) {
        let move_list = legal::legal_moves(&state);
        let moves = move_list.as_slice();
        if moves.is_empty() {
            break;
        }
        let is_own_turn = moves.iter().any(|m| matches!(m, Move::EndTurn));
        let has_take = moves.iter().any(|m| matches!(m, Move::Take { .. }));
        let mv = if is_own_turn && has_take {
            counts.own_turn_decisions += 1;
            counts.with_legal_take += 1;
            let cf = candidate_features(&state, moves, false, weights);
            let scores: Vec<f64> = cf.iter().map(|(_, f)| dot(weights, f)).collect();
            let ci = argmax_first_wins(&scores);
            let ti = cf
                .iter()
                .enumerate()
                .filter(|(_, (m, _))| matches!(m, Move::Take { .. }))
                .max_by(|a, b| scores[a.0].partial_cmp(&scores[b.0]).expect("no NaN scores"))
                .map(|(i, _)| i)
                .expect("has_take implies a Take candidate");

            // Independent path: `rank_moves` re-runs `evaluate` itself.
            // If `dot(w, phi)` and that disagree, the vectors below are not
            // the ones the bot ranks with.
            let ranked = bot.rank_moves(&state, moves);
            for (m, s) in &ranked {
                if let Some(i) = cf.iter().position(|(cm, _)| cm == m) {
                    max_eval = max_eval.max((scores[i] - s).abs());
                    max_scale = max_scale.max(s.abs());
                }
            }

            if matches!(cf[ci].0, Move::Take { .. }) {
                counts.take_was_rank1 += 1;
                *counts.take_chosen_by_round.entry(state.round).or_insert(0) += 1;
            } else {
                let (_, phi_c) = &cf[ci];
                let (_, phi_t) = &cf[ti];
                let contrib: Vec<f64> = WeightKey::ALL
                    .iter()
                    .enumerate()
                    .map(|(i, &k)| weights.get(k) * (phi_c[i] - phi_t[i]))
                    .collect();
                let summed: f64 = contrib.iter().sum();
                let gap = scores[ci] - scores[ti];
                let take_rank = 1 + scores.iter().filter(|&&s| s > scores[ti]).count();
                let lo = scores.iter().copied().fold(f64::INFINITY, f64::min);
                let spread = scores[ci] - lo;
                let gap_frac = if spread > 0.0 { gap / spread } else { 0.0 };
                max_decomp = max_decomp.max((summed - gap).abs());
                max_scale = max_scale.max(scores[ci].abs().max(scores[ti].abs()));
                out.push(Point {
                    round: state.round,
                    take_rank,
                    candidates: scores.len(),
                    contrib,
                    gap,
                    gap_frac,
                });
            }
            cf[ci].0
        } else {
            if is_own_turn {
                counts.own_turn_decisions += 1;
            }
            bot.choose(&state, moves)
        };
        game::step(&mut state, mv);
    }
    (max_decomp, max_eval, max_scale)
}

fn pct(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    let idx = ((sorted.len() - 1) as f64 * q).round() as usize;
    sorted[idx]
}

fn main() {
    let path = Path::new("/tmp/takegap/base.json");
    let weights = load_weights(path).expect("load frozen champion snapshot");
    let bot = WeightedBot::new(weights);

    // Enough seeds for a few hundred recorded points; stops early once the
    // target is met so a long run is not left to finish pointlessly.
    let want = 2000usize;
    let mut points: Vec<Point> = Vec::new();
    let mut counts = Counts::default();
    let mut max_decomp: f64 = 0.0;
    let mut max_eval: f64 = 0.0;
    let mut max_scale: f64 = 0.0;
    let mut games = 0usize;
    for seed in 1u64..=400 {
        if points.len() >= want {
            break;
        }
        let (d, e, s) = scan_game(seed, &weights, &bot, &mut counts, &mut points);
        max_decomp = max_decomp.max(d);
        max_eval = max_eval.max(e);
        max_scale = max_scale.max(s);
        games += 1;
    }

    println!("frozen champion snapshot: {}", path.display());
    println!("games: {games}   recorded points (Take legal, not chosen): {}", points.len());
    println!("own-turn decisions: {}", counts.own_turn_decisions);
    println!(
        "  ... with >=1 legal Take: {} ({:.1}%)",
        counts.with_legal_take,
        100.0 * counts.with_legal_take as f64 / counts.own_turn_decisions.max(1) as f64
    );
    println!(
        "  ... of those, Take was rank 1: {} ({:.1}%)",
        counts.take_was_rank1,
        100.0 * counts.take_was_rank1 as f64 / counts.with_legal_take.max(1) as f64
    );
    let ulp = f64::EPSILON * max_scale;
    println!("MAX DECOMPOSITION RESIDUAL |sum_k w_k*dphi - (dot_c - dot_t)|: {max_decomp:.3e} ({:.1} ULP)", max_decomp / ulp);
    println!("MAX |dot(w,phi) - rank_moves' evaluate()|:                    {max_eval:.3e} ({:.1} ULP)", max_eval / ulp);
    println!("largest |score| seen: {max_scale:.1}; one double ULP there is {ulp:.3e}");
    // 64 ULP of the largest score: generous for reassociation across ~1e2
    // summed terms, tiny next to any real disagreement in the vectors.
    if max_decomp > 64.0 * ulp || max_eval > 64.0 * ulp {
        println!("RESIDUAL TOO LARGE -- the decomposition below would be meaningless. Stopping.");
        return;
    }

    let mut gaps: Vec<f64> = points.iter().map(|p| p.gap).collect();
    gaps.sort_by(|a, b| a.partial_cmp(b).expect("no NaN gaps"));
    println!(
        "\nchosen-minus-best-Take margin: min {:.4}  p25 {:.4}  median {:.4}  p75 {:.4}  max {:.4}",
        pct(&gaps, 0.0),
        pct(&gaps, 0.25),
        pct(&gaps, 0.5),
        pct(&gaps, 0.75),
        pct(&gaps, 1.0)
    );

    let mut fracs: Vec<f64> = points.iter().map(|p| p.gap_frac).collect();
    fracs.sort_by(|a, b| a.partial_cmp(b).expect("no NaN fractions"));
    println!(
        "same margin as a fraction of the decision's own score spread: min {:.3}  p25 {:.3}  median {:.3}  p75 {:.3}  max {:.3}",
        pct(&fracs, 0.0),
        pct(&fracs, 0.25),
        pct(&fracs, 0.5),
        pct(&fracs, 0.75),
        pct(&fracs, 1.0)
    );
    for t in [0.5f64, 1.0, 2.0, 5.0] {
        let n = gaps.iter().filter(|&&g| g < t).count();
        println!("  margin < {t:.1}: {n} of {} ({:.1}%)", gaps.len(), 100.0 * n as f64 / gaps.len().max(1) as f64);
    }
    let mut ranks: Vec<usize> = points.iter().map(|p| p.take_rank).collect();
    ranks.sort_unstable();
    let mean_cands = points.iter().map(|p| p.candidates).sum::<usize>() as f64 / points.len().max(1) as f64;
    println!(
        "best-Take rank among candidates (rank 1 excluded by construction): p25 {} median {} p75 {} max {}; mean candidate count {:.1}",
        ranks[ranks.len() / 4],
        ranks[ranks.len() / 2],
        ranks[3 * ranks.len() / 4],
        ranks[ranks.len() - 1],
        mean_cands
    );
    for t in [2usize, 3, 5] {
        let n = ranks.iter().filter(|&&r| r <= t).count();
        println!("  best Take in top {t}: {n} of {} ({:.1}%)", ranks.len(), 100.0 * n as f64 / ranks.len().max(1) as f64);
    }

    let mut all = Bucket::new();
    let mut opener = Bucket::new();
    let mut mid = Bucket::new();
    for p in &points {
        all.add(p);
        if (1..=3).contains(&p.round) {
            opener.add(p);
        }
        if p.round >= 6 {
            mid.add(p);
        }
    }
    all.report("ALL ROUNDS", &weights, 12);
    opener.report("OPENER (rounds 1-3)", &weights, 12);
    mid.report("MID-GAME (rounds 6+)", &weights, 12);

    let mut by_round: Vec<(u16, usize)> = counts.take_chosen_by_round.into_iter().collect();
    by_round.sort_unstable();
    println!("\nrounds at which a Take WAS rank 1 (round: count): {by_round:?}");
}
