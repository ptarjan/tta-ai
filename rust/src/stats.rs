//! Confidence intervals for a seat-paired duel.
//!
//! Port of `experiments/paired_stats.py`. One estimator, not the module's
//! four: `paired` is the one a single arena run needs, and the pooled /
//! escalating variants exist there to reconcile the halted league's historical
//! logs, which the native trainer does not read.
//!
//! # Why the interval is not the obvious one
//!
//! [`crate::bots::greedy::Seat`]'s duel deals every seed `players` times with
//! the challenger rotated through every seat, so the games are *not*
//! independent samples -- the same deal appears in all of them. Treating them
//! as independent gets the interval wrong in a direction that depends on the
//! data:
//!
//! * `rho > 0` -- the deal favours a *strategy* whichever seat it sits in.
//!   The naive interval is too NARROW, by at most `sqrt(2)` at `rho = 1`.
//! * `rho < 0` -- the deal favours a *seat*, so the challenger tends to win
//!   one game of the mirrored pair and lose the other. The naive interval is
//!   too WIDE, down to zero at `rho = -1`.
//!
//! Measured on this project's real data `rho` is negative nearly everywhere,
//! so the correction usually TIGHTENS the interval -- which is why it is not
//! a blanket `sqrt(2)` safety factor, and why getting it right buys games.
//!
//! The fix is to cluster on the deal: average each deal's `players` games into
//! one number and put the interval on those, with a t critical value because
//! the cluster count is small. [`naive_half`](Estimate::naive_half) is kept
//! alongside so both numbers can be printed and a run reconciled against the
//! Python league's logs.

/// Two-sided 95% normal critical value.
pub const Z95: f64 = 1.959_963_984_540_054;

/// Exact two-sided 95% t critical values, `df` 1..=30, indexed `df - 1`.
const T95: [f64; 30] = [
    12.706, 4.303, 3.182, 2.776, 2.571, 2.447, 2.365, 2.306, 2.262, 2.228, 2.201, 2.179, 2.160,
    2.145, 2.131, 2.120, 2.110, 2.101, 2.093, 2.086, 2.080, 2.074, 2.069, 2.064, 2.060, 2.056,
    2.052, 2.048, 2.045, 2.042,
];

/// Two-sided 95% critical value on `df` degrees of freedom.
///
/// Table to `df = 30`, Cornish-Fisher expansion above it (error < 1e-4
/// there), and `INFINITY` at `df <= 0` so that an "interval" derived from a
/// single cluster cannot masquerade as a measurement.
pub fn t_crit(df: usize) -> f64 {
    if df == 0 {
        return f64::INFINITY;
    }
    if df <= T95.len() {
        return T95[df - 1];
    }
    let d = df as f64;
    let z = Z95;
    z + (z.powi(3) + z) / (4.0 * d)
        + (5.0 * z.powi(5) + 16.0 * z.powi(3) + 3.0 * z) / (96.0 * d * d)
        + (3.0 * z.powi(7) + 19.0 * z.powi(5) + 17.0 * z.powi(3) - 15.0 * z)
            / (384.0 * d * d * d)
}

/// A win rate (or a culture lead) with an interval that names its own unit.
#[derive(Clone, Debug)]
pub struct Estimate {
    pub mean: f64,
    /// The CORRECT half-width -- the headline. Clustered on the deal.
    pub half: f64,
    pub se: f64,
    pub n_games: usize,
    /// Complete deals; the unit the design actually randomises.
    pub n_deals: usize,
    /// The legacy independent-samples half-width, for comparison only. Never
    /// make this the headline of a paired design.
    pub naive_half: f64,
    /// Within-deal correlation. Negative: the deal favours a seat (pairing
    /// helps). Positive: it favours a strategy (pairing costs). `NaN` when
    /// there is not enough data to estimate it.
    pub rho: f64,
    /// `(half / naive_half)^2` -- the design effect. Below 1 means pairing
    /// bought precision.
    pub deff: f64,
}

impl Estimate {
    pub fn lo(&self) -> f64 {
        self.mean - self.half
    }

    pub fn hi(&self) -> f64 {
        self.mean + self.half
    }

    /// Two-sided p-value that the mean differs from `null`.
    ///
    /// Computed from the clustered standard error, so it agrees with
    /// [`Estimate::half`] rather than with the naive interval.
    pub fn p_against(&self, null: f64) -> f64 {
        if !(self.se > 0.0) {
            return 1.0;
        }
        erfc((self.mean - null).abs() / self.se / std::f64::consts::SQRT_2)
    }

    /// Is the mean above `null` by more than the interval?
    ///
    /// This is the accept test a hill climber applies to a challenger: a
    /// point estimate above the null is not a result, an interval clear of it
    /// is.
    pub fn beats(&self, null: f64) -> bool {
        self.half.is_finite() && self.lo() > null
    }
}

/// Sample mean and unbiased variance. Variance is `0.0` for `n < 2` -- a
/// single point has no spread, and callers all special-case `n < 2` anyway.
fn mean_var(xs: &[f64]) -> (f64, f64) {
    let n = xs.len();
    if n == 0 {
        return (0.0, 0.0);
    }
    let m = xs.iter().sum::<f64>() / n as f64;
    if n < 2 {
        return (m, 0.0);
    }
    let var = xs.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / (n - 1) as f64;
    (m, var)
}

/// Task-ordered per-game values -> one value per COMPLETE deal.
///
/// `per_game` must be in duel task order, with `None` left in place for games
/// that died -- that is what keeps index `g` recoverable as
/// `(seed0 + g / players, g % players)`. A deal missing any of its seats is
/// dropped whole rather than half-counted, because half a mirrored pair is
/// exactly the seat-biased observation the pairing exists to remove.
pub fn deal_means(per_game: &[Option<f64>], players: usize) -> Vec<f64> {
    assert!(players >= 1, "players must be >= 1");
    per_game
        .chunks_exact(players)
        .filter(|blk| blk.iter().all(Option::is_some))
        .map(|blk| blk.iter().map(|v| v.unwrap()).sum::<f64>() / players as f64)
        .collect()
}

/// The legacy independent-samples half-width. Kept only for comparison.
fn naive_half(per_game: &[Option<f64>]) -> f64 {
    let xs: Vec<f64> = per_game.iter().filter_map(|v| *v).collect();
    if xs.len() < 2 {
        return if xs.is_empty() { 0.0 } else { 1.0 };
    }
    let (_, var) = mean_var(&xs);
    Z95 * (var / xs.len() as f64).sqrt()
}

/// Within-deal correlation of the seat-swapped games.
fn intra_deal_rho(per_game: &[Option<f64>], players: usize) -> f64 {
    let xs: Vec<f64> = per_game.iter().filter_map(|v| *v).collect();
    if xs.len() < 2 || players < 2 {
        return f64::NAN;
    }
    let (_, total) = mean_var(&xs);
    if total <= 0.0 {
        return f64::NAN;
    }
    let ys = deal_means(per_game, players);
    if ys.len() < 2 {
        return f64::NAN;
    }
    let (_, between) = mean_var(&ys);
    (between * players as f64 / total - 1.0) / (players as f64 - 1.0)
}

/// THE estimator for one seat-paired duel. Clusters on the deal.
///
/// `per_game` is in task order with `None` for a game that did not finish;
/// see [`deal_means`] for why that placeholder has to be there rather than
/// the value simply being absent.
pub fn paired(per_game: &[Option<f64>], players: usize) -> Estimate {
    let ys = deal_means(per_game, players);
    let n_games = per_game.iter().filter(|v| v.is_some()).count();
    let k = ys.len();
    let nh = naive_half(per_game);
    let rho = intra_deal_rho(per_game, players);

    if k == 0 {
        return Estimate {
            mean: 0.0,
            half: f64::INFINITY,
            se: 0.0,
            n_games,
            n_deals: 0,
            naive_half: nh,
            rho,
            deff: f64::NAN,
        };
    }
    let (m, var) = mean_var(&ys);
    // A single cluster cannot bound itself: an infinite half-width is the
    // honest answer, and it makes `Estimate::beats` false by construction.
    let (se, half) = if k < 2 {
        (f64::INFINITY, f64::INFINITY)
    } else {
        let se = (var / k as f64).sqrt();
        (se, t_crit(k - 1) * se)
    };
    let deff = if nh > 0.0 && half.is_finite() { (half / nh).powi(2) } else { f64::NAN };
    Estimate { mean: m, half, se, n_games, n_deals: k, naive_half: nh, rho, deff }
}

/// `erfc`, to about 1e-7 relative -- Numerical Recipes' Chebyshev fit.
///
/// Hand-rolled because `f64::erfc` is not in Rust's standard library and
/// `Cargo.toml`'s `[dependencies]` is deliberately empty. Its only caller is
/// [`Estimate::p_against`], a printed diagnostic: the accept decision is
/// [`Estimate::beats`], which never touches this.
fn erfc(x: f64) -> f64 {
    let z = x.abs();
    let t = 2.0 / (2.0 + z);
    let ty = 4.0 * t - 2.0;
    const C: [f64; 28] = [
        -1.3026537197817094,
        6.419_697_923_564_902e-1,
        1.9476473204185836e-2,
        -9.561_514_786_808_631e-3,
        -9.46595344482036e-4,
        3.66839497852761e-4,
        4.2523324806907e-5,
        -2.0278578112534e-5,
        -1.624290004647e-6,
        1.303655835580e-6,
        1.5626441722e-8,
        -8.5238095915e-8,
        6.529054439e-9,
        5.059343495e-9,
        -9.91364156e-10,
        -2.27365122e-10,
        9.6467911e-11,
        2.394038e-12,
        -6.886027e-12,
        8.94487e-13,
        3.13092e-13,
        -1.12708e-13,
        3.81e-16,
        7.106e-15,
        -1.523e-15,
        -9.4e-17,
        1.21e-16,
        -2.8e-17,
    ];
    let (mut d, mut dd) = (0.0f64, 0.0f64);
    for &c in C.iter().skip(1).rev() {
        let tmp = d;
        d = ty * d - dd + c;
        dd = tmp;
    }
    let ans = t * (-z * z + 0.5 * (C[0] + ty * d) - dd).exp();
    if x >= 0.0 {
        ans
    } else {
        2.0 - ans
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn some(xs: &[f64]) -> Vec<Option<f64>> {
        xs.iter().map(|x| Some(*x)).collect()
    }

    #[test]
    fn t_crit_matches_the_table_and_tends_to_z() {
        assert_eq!(t_crit(1), 12.706);
        assert_eq!(t_crit(30), 2.042);
        assert!((t_crit(1000) - Z95).abs() < 3e-3, "{}", t_crit(1000));
        assert!(t_crit(0).is_infinite());
    }

    /// A deal missing any of its seats is dropped whole -- half a mirrored
    /// pair is the seat-biased observation the pairing exists to remove.
    #[test]
    fn an_incomplete_deal_is_dropped_rather_than_half_counted() {
        let per_game = vec![Some(1.0), None, Some(0.0), Some(0.0), Some(1.0), Some(0.5)];
        assert_eq!(deal_means(&per_game, 3), vec![0.5]);
    }

    /// A trailing partial deal is not a deal.
    #[test]
    fn a_run_that_stops_mid_deal_drops_the_stub() {
        assert_eq!(deal_means(&some(&[1.0, 0.0, 1.0, 1.0]), 3), vec![2.0 / 3.0]);
    }

    /// The case the whole module exists for: the challenger wins from seat 0
    /// and loses from seat 1 in every deal, so every deal-mean is exactly
    /// 0.5. The naive interval sees wild per-game variance; the clustered one
    /// sees a constant.
    #[test]
    fn a_pure_seat_effect_collapses_the_interval() {
        let per_game = some(&[1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0]);
        let est = paired(&per_game, 2);
        assert_eq!(est.mean, 0.5);
        assert_eq!(est.n_deals, 4);
        assert!(est.half < 1e-12, "half was {}", est.half);
        assert!(est.naive_half > 0.35, "naive was {}", est.naive_half);
        assert!(est.rho < -0.99, "rho was {}", est.rho);
    }

    /// The other direction: the deal decides the game whichever seat the
    /// challenger sits in, so pairing buys nothing and must not pretend to.
    #[test]
    fn a_pure_deal_effect_does_not_shrink_the_interval() {
        let per_game = some(&[1.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0]);
        let est = paired(&per_game, 2);
        assert!(est.rho > 0.99, "rho was {}", est.rho);
        assert!(est.half > est.naive_half, "half {} naive {}", est.half, est.naive_half);
    }

    /// One deal is not a measurement, and must not be allowed to look like
    /// one -- `beats` has to be false however good the point estimate is.
    #[test]
    fn a_single_deal_cannot_bound_itself() {
        let est = paired(&some(&[1.0, 1.0]), 2);
        assert_eq!(est.mean, 1.0);
        assert!(est.half.is_infinite());
        assert!(!est.beats(0.5));
    }

    #[test]
    fn no_complete_deals_is_not_a_win() {
        let est = paired(&[Some(1.0), None], 2);
        assert_eq!(est.n_deals, 0);
        assert!(!est.beats(0.5));
    }

    /// `beats` is the accept test: a point estimate above the null is not a
    /// result, an interval clear of it is.
    #[test]
    fn beats_needs_the_whole_interval_clear_of_the_null() {
        let clear = paired(&some(&[1.0, 1.0, 1.0, 1.0, 1.0, 0.9, 1.0, 1.0]), 2);
        assert!(clear.beats(0.5), "{clear:?}");
        let noisy = paired(&some(&[1.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0]), 2);
        assert!(!noisy.beats(0.5), "{noisy:?}");
    }

    #[test]
    fn erfc_matches_known_values() {
        assert!((erfc(0.0) - 1.0).abs() < 1e-9);
        assert!((erfc(1.0) - 0.157_299_207_050_285).abs() < 1e-7);
        assert!((erfc(-1.0) - 1.842_700_792_949_715).abs() < 1e-7);
        assert!((erfc(3.0) - 2.209_049_699_858_544e-5).abs() < 1e-9);
    }

    /// A mean sitting exactly on the null is p=1; a mean far from it is
    /// small. The p-value is a printed diagnostic, so this pins the shape,
    /// not a digit.
    #[test]
    fn p_against_the_null_moves_the_right_way() {
        let est = paired(&some(&[0.6, 0.4, 0.55, 0.45, 0.5, 0.5, 0.52, 0.48]), 2);
        assert!((est.p_against(0.5) - 1.0).abs() < 1e-9, "{est:?}");
        let strong = paired(&some(&[0.9, 0.8, 0.85, 0.9, 0.8, 0.9, 0.9, 0.85]), 2);
        assert!(strong.p_against(0.5) < 1e-6, "{strong:?}");
    }
}
