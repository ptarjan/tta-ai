"""Interval estimators that respect how the arena actually deals its games.

Every duel this project runs is **seat-paired by construction**.
``experiments/arena.duel`` builds its task list as::

    for g in range(games):
        seat = g % num_players
        seed = seed0 + g // num_players

so a 3200-game 2p run is **1600 deals each played twice with the seats
swapped**, not 3200 independent trials.  ``experiments/neural_eval.py`` deals
the same way.  The independent unit is the *deal*, not the game.

Until 2026-07-30 every interval in the repo was
``1.96 * sqrt(var / n_games)`` (or, in ``pool_summary``, the even blunter
``1.96 * sqrt(p(1-p) / n_games)``).  That is the independent-samples formula
applied to a paired design, and it is wrong.  What it is *not* is uniformly
optimistic, which is the part that matters and the part that is easy to get
backwards.

The algebra, at P=2.  Let ``X_k0``/``X_k1`` be A's win share on deal ``k`` from
each seat, and let the estimator be the grand mean, i.e. the mean of the
per-deal means ``Y_k = (X_k0 + X_k1) / 2``::

    Var(Y_k)     = (p(1-p) + Cov(X_k0, X_k1)) / 2
    naive SE^2   = p(1-p) / (2K)
    correct SE^2 = (p(1-p) + Cov) / (2K)
    ratio^2      = 1 + rho,      rho = corr(X_k0, X_k1)

So the correction is a function of the **sign** of ``rho``:

* ``rho > 0`` — the deal favours one *strategy* whichever seat it sits in.
  The naive CI is too narrow, by at most sqrt(2) at ``rho = 1``.
* ``rho < 0`` — the deal favours one *seat*, so A tends to win one game of the
  pair and lose the other.  Swapping the seats cancels that nuisance variance
  and the correct CI is **narrower**, down to zero at ``rho = -1``.

Measured on this project's real data, ``rho`` is *negative almost everywhere*
(-0.04 to -0.72), because the deal-by-seat interaction in Through the Ages is
large.  The naive interval is therefore usually **conservative**, not
optimistic, and the paired estimator makes most results stronger.  The one
committed dataset that proves the point beyond argument is
``exp_quiesce/ab.jsonl``'s ``ctrl_2p`` row: the same deterministic bot on both
sides, n=800, reported ``ci=0.0346``, when the deal-level variance is *exactly
zero* -- every pair splits 1-1 and the true CI is 0.0.  All 3.46pp of that
interval was seat-assignment noise.

Do not, therefore, "correct" anything by multiplying by sqrt(2).  Measure it.

**A second, independent defect.** Runs are usually fanned out over disjoint
seed *blocks* (``--seed0`` shards).  Block means are independent by
construction, so they provide a check on the deal-level interval that assumes
nothing.  When the two disagree -- when the blocks are over-dispersed relative
to what deal-level noise predicts -- the deals are not exchangeable and the
honest interval is the coarser, block-clustered one.  ``pooled`` runs that test
and escalates automatically.  With only a handful of blocks the variance
estimate is itself noisy, so cluster intervals use a ``t_{K-1}`` critical value
rather than 1.96; at K=6 that is 2.571, not 1.96, and the difference is not
cosmetic.

Every estimator here also carries ``naive_half``, the legacy number, so results
can be compared against historical logs during the transition.
"""
from __future__ import annotations

import math
import random
from dataclasses import dataclass, field

# 95% two-sided Student-t critical values, df 1..30.  Small-K cluster
# intervals are badly anti-conservative with z=1.96 and this is the whole
# reason the anchor's six shards looked twice as sharp as they are.
_T95 = {
    1: 12.706, 2: 4.303, 3: 3.182, 4: 2.776, 5: 2.571, 6: 2.447, 7: 2.365,
    8: 2.306, 9: 2.262, 10: 2.228, 11: 2.201, 12: 2.179, 13: 2.160,
    14: 2.145, 15: 2.131, 16: 2.120, 17: 2.110, 18: 2.101, 19: 2.093,
    20: 2.086, 21: 2.080, 22: 2.074, 23: 2.069, 24: 2.064, 25: 2.060,
    26: 2.056, 27: 2.052, 28: 2.048, 29: 2.045, 30: 2.042,
}
Z95 = 1.959963984540054


def t_crit(df, z=Z95):
    """Two-sided 95% critical value on `df` degrees of freedom.

    Exact table to df=30, Cornish-Fisher expansion above it (error < 1e-4
    there), and a deliberately huge value at df<=0 so that a "confidence
    interval" from a single cluster cannot masquerade as a measurement.
    """
    if df <= 0:
        return float("inf")
    if df in _T95:
        return _T95[df]
    d = float(df)
    return (z + (z ** 3 + z) / (4 * d)
            + (5 * z ** 5 + 16 * z ** 3 + 3 * z) / (96 * d * d)
            + (3 * z ** 7 + 19 * z ** 5 + 17 * z ** 3 - 15 * z)
            / (384 * d * d * d))


@dataclass
class Estimate:
    """A win rate (or margin) with an interval that names its own unit."""

    mean: float
    half: float                 # the CORRECT half-width -- the headline
    se: float
    n_games: int
    n_clusters: int
    unit: str                   # "deal", "block", "game"
    crit: float
    naive_half: float = 0.0     # legacy 1.96*sqrt(var/n_games), for comparison
    rho: float = float("nan")   # within-deal correlation
    deff: float = float("nan")  # (half / naive_half)^2
    het_chi2: float = float("nan")
    het_df: int = 0
    escalated: bool = False
    notes: list = field(default_factory=list)

    @property
    def low(self):
        return self.mean - self.half

    @property
    def high(self):
        return self.mean + self.half

    def z_against(self, null):
        """Signed z of the estimate against a null, using the CORRECT se."""
        if self.se <= 0:
            return float("inf") if self.mean != null else 0.0
        return (self.mean - null) / self.se

    def p_against(self, null):
        z = abs(self.z_against(null))
        if z == float("inf"):
            return 0.0
        return math.erfc(z / math.sqrt(2))

    def fmt(self, pct=True):
        s = 100.0 if pct else 1.0
        u = "%" if pct else ""
        return (f"{self.mean * s:.2f}{u} +/- {self.half * s:.2f}"
                f"{'pp' if pct else ''} [{self.unit}-clustered, "
                f"K={self.n_clusters}]")


def _mean_var(xs):
    n = len(xs)
    m = sum(xs) / n
    if n < 2:
        return m, 0.0
    return m, sum((x - m) ** 2 for x in xs) / (n - 1)


def deal_means(per_game, players):
    """Task-ordered per-game values -> one value per COMPLETE deal.

    `per_game` must be in ``arena.duel`` task order, with ``None`` left in
    place for games that died -- that is what makes index ``g`` recoverable as
    ``(seed0 + g // players, g % players)``.  A deal missing any of its seats
    is dropped whole rather than half-counted, because half a mirrored pair is
    exactly the seat-biased observation the pairing exists to remove.
    """
    if players < 1:
        raise ValueError("players must be >= 1")
    out = []
    n = len(per_game) // players * players
    for k in range(0, n, players):
        blk = per_game[k:k + players]
        if any(v is None for v in blk):
            continue
        out.append(sum(blk) / players)
    return out


def naive_ci(per_game, z=Z95):
    """The legacy independent-samples interval.  Kept only for comparison.

    Never make this the headline of a paired design.  ``paired`` returns it as
    ``naive_half`` so both numbers can be printed side by side.
    """
    xs = [v for v in per_game if v is not None]
    if not xs:
        return 0.0, 0.0, 0
    if len(xs) < 2:
        return xs[0], 1.0, 1
    m, var = _mean_var(xs)
    return m, z * math.sqrt(var / len(xs)), len(xs)


def cluster_ci(values, use_t=True, unit="cluster", n_games=None):
    """Interval from independent cluster-level values, with a t correction."""
    k = len(values)
    if k == 0:
        return Estimate(0.0, 0.0, 0.0, 0, 0, unit, 0.0,
                        notes=["no clusters"])
    m, var = _mean_var(values)
    if k < 2:
        return Estimate(m, float("inf"), float("inf"), n_games or k, k, unit,
                        float("inf"),
                        notes=["a single cluster cannot bound itself"])
    se = math.sqrt(var / k)
    crit = t_crit(k - 1) if use_t else Z95
    return Estimate(mean=m, half=crit * se, se=se, n_games=n_games or k,
                    n_clusters=k, unit=unit, crit=crit)


def intra_deal_rho(per_game, players):
    """Within-deal correlation of the seat-swapped games.

    Negative means the deal favours a SEAT (pairing helps, CI shrinks);
    positive means it favours a STRATEGY (pairing costs, CI grows).
    """
    xs = [v for v in per_game if v is not None]
    if len(xs) < 2 or players < 2:
        return float("nan")
    _, tot = _mean_var(xs)
    if tot <= 0:
        return float("nan")
    ys = deal_means(per_game, players)
    if len(ys) < 2:
        return float("nan")
    _, between = _mean_var(ys)
    return (between * players / tot - 1) / (players - 1)


def paired(per_game, players, use_t=True):
    """THE estimator for a single seat-paired arena/neural_eval run.

    Clusters on the deal, which is the unit the design randomises.
    """
    ys = deal_means(per_game, players)
    n_games = sum(1 for v in per_game if v is not None)
    est = cluster_ci(ys, use_t=use_t, unit="deal", n_games=n_games)
    _, nh, _ = naive_ci(per_game)
    est.naive_half = nh
    est.rho = intra_deal_rho(per_game, players)
    est.deff = (est.half / nh) ** 2 if nh > 0 else float("nan")
    if players == 1:
        est.notes.append("players=1: no pairing exists, deal == game")
    return est


def pooled(blocks, players, use_t=True, alpha=0.05):
    """Pool runs on DISJOINT seed blocks, and check the blocks agree.

    `blocks` is a list of task-ordered per-game lists, one per ``--seed0``
    shard.  Returns the deal-clustered estimate over everything, unless the
    block means are over-dispersed relative to deal-level noise -- in which
    case the deals are demonstrably not exchangeable across blocks and the
    interval escalates to block-level clustering, which assumes strictly less.

    This is the defect that made the neural loop's anchor look twice as sharp
    as it is: six shards whose spread was 2x what 40 games of binomial noise
    predicts, summarised with a formula that could not see shards at all.
    """
    per_block = [deal_means(b, players) for b in blocks]
    per_block = [b for b in per_block if b]
    if not per_block:
        return Estimate(0.0, 0.0, 0.0, 0, 0, "deal", 0.0,
                        notes=["no complete deals"])
    alldeals = [y for b in per_block for y in b]
    n_games = sum(1 for b in blocks for v in b if v is not None)
    est = cluster_ci(alldeals, use_t=use_t, unit="deal", n_games=n_games)
    _, nh, _ = naive_ci([v for b in blocks for v in b])
    est.naive_half = nh
    est.rho = intra_deal_rho([v for b in blocks for v in b], players)
    est.deff = (est.half / nh) ** 2 if nh > 0 else float("nan")

    # Heterogeneity: do the block means scatter more than deal noise allows?
    b = len(per_block)
    if b >= 2:
        grand = sum(alldeals) / len(alldeals)
        _, deal_var = _mean_var(alldeals)
        if deal_var > 0:
            chi2 = sum(len(g) * (sum(g) / len(g) - grand) ** 2 / deal_var
                       for g in per_block)
            est.het_chi2 = chi2
            est.het_df = b - 1
            if chi2 > _chi2_crit(b - 1, alpha):
                bm = [sum(g) / len(g) for g in per_block]
                esc = cluster_ci(bm, use_t=use_t, unit="block",
                                 n_games=n_games)
                esc.naive_half = nh
                esc.rho = est.rho
                esc.deff = (esc.half / nh) ** 2 if nh > 0 else float("nan")
                esc.het_chi2, esc.het_df = chi2, b - 1
                esc.escalated = True
                esc.notes.append(
                    f"blocks over-dispersed (chi2={chi2:.2f} on {b - 1} df); "
                    f"deal-clustered half was {est.half:.4f}, escalated to "
                    f"block-clustered")
                return esc
    return est


# Upper 5% points of chi-square, df 1..30.  Same reason as the t table: no
# scipy anywhere in this repo and one dependency for one number is a bad trade.
_CHI2_95 = {
    1: 3.841, 2: 5.991, 3: 7.815, 4: 9.488, 5: 11.070, 6: 12.592, 7: 14.067,
    8: 15.507, 9: 16.919, 10: 18.307, 11: 19.675, 12: 21.026, 13: 22.362,
    14: 23.685, 15: 24.996, 16: 26.296, 17: 27.587, 18: 28.869, 19: 30.144,
    20: 31.410, 21: 32.671, 22: 33.924, 23: 35.172, 24: 36.415, 25: 37.652,
    26: 38.885, 27: 40.113, 28: 41.337, 29: 42.557, 30: 43.773,
}


def _chi2_crit(df, alpha=0.05):
    if df <= 0:
        return float("inf")
    if alpha == 0.05 and df in _CHI2_95:
        return _CHI2_95[df]
    # Wilson-Hilferty, good to ~1% for df>=5
    z = Z95 if alpha == 0.05 else Z95
    return df * (1 - 2.0 / (9 * df) + z * math.sqrt(2.0 / (9 * df))) ** 3


def block_bootstrap(per_game, players, reps=10000, seed=12345, alpha=0.05):
    """Percentile CI resampling whole DEALS.  A distribution-free cross-check.

    Nothing depends on this; it exists so the closed-form paired interval can
    be checked against something that makes no normality assumption at all.
    """
    ys = deal_means(per_game, players)
    k = len(ys)
    if k < 2:
        return (float("nan"), float("nan"))
    rng = random.Random(seed)
    means = []
    for _ in range(reps):
        means.append(sum(ys[rng.randrange(k)] for _ in range(k)) / k)
    means.sort()
    lo = means[int(alpha / 2 * reps)]
    hi = means[min(reps - 1, int((1 - alpha / 2) * reps))]
    return (lo, hi)


def from_duel(res, use_t=True, key="per_game"):
    """Convenience: an ``arena.duel`` result dict -> the correct Estimate."""
    return paired(res[key], res["players"], use_t=use_t)
