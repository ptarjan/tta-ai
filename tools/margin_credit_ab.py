"""Is the gate's margin credit separable into "real strength" and "free margin"?

docs/CULTURE_GAP.md 19 measured a PERVERSE gradient in the trainer's own accept
statistic: inflating `culture_rate` 5 -> 35.574 buys +0.011 of pool-weighted
gate score and ZERO additional wins, because 5.5 of the 8.0 pool weight is
scored on `margin_share(culture margin)` at opponents the bot loses to 100% of
the time.  19c fix #2 proposes capping that credit or scoring the tiers on
margin RANK instead.

Neither remedy can be judged by whether it kills the perverse gradient -- ANY
monotone function of margin pays for margin, so a remedy that kills the
perverse gradient by brute force kills the real one with it.  The question is
whether the two are SEPARABLE, and the mechanism that could separate them is
CONVEXITY: a scoring function that saturates hard below the win boundary
responds sub-linearly to a small shift of a hopeless margin distribution
(+13 culture points, which is all the perverse axis buys) and super-linearly
to a large one (+100, which is what real strength buys).  A cap is therefore
not "less sensitive"; it is differently sensitive, and whether that helps is
an empirical question about the actual margin DISTRIBUTIONS.

So this tool separates data collection from scoring.  It plays a fixed set of
weight vectors against the real pool on identical seeds, dumps every game's
(win share, culture margin), and then re-scores the SAME games under every
candidate credit function.  Nothing is re-played when the candidate changes.

The vectors, and why each is here:

  base        DEFAULT_WEIGHTS -- the reference every edge is paired against
  perverse    culture_rate = 35.574 (the 4p champion's), everything else
              untouched: CULTURE_GAP 19's exact cell.  A remedy must shrink
              this edge toward zero.
  sci_neg     science = -6.089, the degenerate old-4p champion vector, measured
              at 9.7% +/- 2.7% win rate (hillclimb_league's guard comment).
              A remedy must still detect this, hugely.
  cr_zero     culture_rate = 0: the trainer's own ablation prices this at
              0.11-0.18 of win share (CULTURE_GAP 11).  Real, large.
  drift       a pure-drift sibling from tools/drift_sim.py -- same generation
              count and accept rate as a real arm, acceptance a coin flip.
              CULTURE_GAP 20 measures the champion beating these 0.94-0.99, so
              they are genuinely WORSE than DEFAULT_WEIGHTS.  Real, medium.
  flat_shape  the (base, early, late) -> (base + mean-lateness blend, 0, 0)
              rewrite from tools/shape_ab.py, worth 3.79 +/- 2.47 culture
              points = 0.21 sigma of one evaluation block (CULTURE_GAP 17).
              The borderline case: the gate is ALREADY blind to it, and a
              remedy must not make that worse in relative terms.

Usage:

    python3 tools/margin_credit_ab.py --players 4 --games 150 --workers 4 \
        --out /tmp/margin_ab_4p.json          # collect
    python3 tools/margin_credit_ab.py --score /tmp/margin_ab_4p.json   # score
"""
import argparse
import json
import math
import os
import random
import statistics
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine.bots.weighted import DEFAULT_WEIGHTS, PHASE_KEYS  # noqa: E402
from experiments import hillclimb_league as L  # noqa: E402
from experiments import hillclimb_pool as P  # noqa: E402
from experiments.hillclimb import mutate  # noqa: E402

#: mean of `lateness()` over real decisions, from tools/shape_ab.py
MEAN_LATENESS = {2: 0.6348, 3: 0.6672, 4: 0.6819}

#: CULTURE_GAP 19's cell: the 4p champion's `culture_rate`
PERVERSE_LEVEL = 35.574


def build_vectors(players, seed=20260726):
    v = {"base": dict(DEFAULT_WEIGHTS)}
    v["perverse"] = dict(DEFAULT_WEIGHTS, culture_rate=PERVERSE_LEVEL)
    v["sci_neg"] = dict(DEFAULT_WEIGHTS, science=-6.089)
    v["cr_zero"] = dict(DEFAULT_WEIGHTS, culture_rate=0.0)

    rng = random.Random(seed)
    w = dict(DEFAULT_WEIGHTS)
    for _ in range(120):                      # tools/drift_sim.py, one run
        cands = []
        for _ in range(2):
            m, _, _ = mutate(w, rng, 0.25)
            m, _ = L.guard_weights(m, "clamp")
            cands.append(m)
        if rng.random() < 0.20:
            w = cands[rng.randrange(len(cands))]
    v["drift"] = w

    lb = MEAN_LATENESS[players]
    flat = dict(DEFAULT_WEIGHTS)
    for k in PHASE_KEYS:
        e, l = flat.get(k + "_early", 0.0), flat.get(k + "_late", 0.0)
        flat[k] = flat.get(k, 0.0) + (1.0 - lb) * e + lb * l
        flat[k + "_early"] = 0.0
        flat[k + "_late"] = 0.0
    v["flat_shape"] = flat
    return v


def collect(players, games, workers, out_path, seed_base=20260726, only=None):
    pool = P.build_pool(players, ladder_dirs=(), past_k=0, log=lambda *_x: None)
    entries = [e for e in pool.sorted_entries() if not e.is_mirror]
    vectors = build_vectors(players)
    if only:
        vectors = {k: v for k, v in vectors.items() if k in only}

    rec = {"players": players, "games": games,
           "opponents": {e.label: {"tier": e.tier, "metric": e.metric,
                                   "weight": e.weight} for e in entries},
           "tier_weights": dict(P.DEFAULT_TIER_WEIGHTS),
           "series": {}}
    for name, w in vectors.items():
        rec["series"][name] = {}
        for e in entries:
            seed0 = (seed_base + L.label_seed(e.label) * 17) % 10_000_019
            # `metric` is irrelevant here: we keep the RAW win and margin
            # series and score them ourselves below.
            r = L._series(w, e.spec, players, games, seed0, workers, "winshare")
            rec["series"][name][e.label] = {"win": r["win"], "margin": r["margin"]}
            print(f"  {name:<11} {e.label:<14} win={_mean(r['win']):.3f} "
                  f"margin={_mean(r['margin']):+7.1f}", flush=True)
        with open(out_path, "w") as fh:
            json.dump(rec, fh)
    print(f"# wrote {out_path}")


def _mean(xs):
    xs = [x for x in xs if x is not None]
    return sum(xs) / max(1, len(xs))


# ------------------------------------------------------- credit functions
#
# Each takes the candidate's and the reference's per-game margin series for ONE
# opponent and returns the per-game paired credit series.  Keeping it paired
# (rather than "score each side, then subtract") is what lets `rank` exist at
# all, and it matches how `weighted_stats` consumes the data.

def _tanh(scale):
    def f(cand, ref):
        return [P.margin_share(c, scale) - P.margin_share(r, scale)
                for c, r in zip(cand, ref)]
    f.__name__ = f"tanh/{scale:g}"
    return f


def _reach_cap(cap, scale=P.MARGIN_SCALE):
    """`margin_share` on a margin floored at `-cap`.

    Below `cap` culture points behind, all losses score the same: the game was
    not winnable and narrowing it further earns nothing.  This is 19c fix #2's
    "cap the margin credit" read literally.
    """
    def f(cand, ref):
        return [P.margin_share(max(c, -cap), scale)
                - P.margin_share(max(r, -cap), scale) for c, r in zip(cand, ref)]
    f.__name__ = f"cap@{cap:g}"
    return f


def _rank(cand, ref):
    """Within-block margin RANK -- 19c fix #2's second proposal.

    Pool the candidate's and the reference's margins for this opponent, rank
    them together, and score each game by its normalised rank.  This is the
    Mann-Whitney statistic: the paired mean is P(cand > ref) - 0.5, bounded,
    and invariant to ANY monotone transform of the margin scale (so the choice
    of MARGIN_SCALE stops mattering).
    """
    pooled = sorted(x for x in list(cand) + list(ref) if x is not None)
    n = len(pooled)

    def q(x):
        if x is None:
            return None
        lo = _bisect_left(pooled, x)
        hi = _bisect_right(pooled, x)
        return ((lo + hi) / 2.0) / n           # mid-rank, in (0, 1)
    return [q(c) - q(r) for c, r in zip(cand, ref)]


_rank.__name__ = "rank"


def _bisect_left(a, x):
    lo, hi = 0, len(a)
    while lo < hi:
        mid = (lo + hi) // 2
        if a[mid] < x:
            lo = mid + 1
        else:
            hi = mid
    return lo


def _bisect_right(a, x):
    lo, hi = 0, len(a)
    while lo < hi:
        mid = (lo + hi) // 2
        if x < a[mid]:
            hi = mid
        else:
            lo = mid + 1
    return lo


CANDIDATES = [_tanh(120.0), _tanh(60.0), _tanh(30.0),
              _reach_cap(120.0), _reach_cap(60.0), _rank]


def _agg(rec, name, credit):
    """Pool-weighted accept statistic of `name` against `base`, and its SE.

    Mirrors `hillclimb_pool.weighted_stats`: each opponent contributes the mean
    of its per-game paired edges with its pool weight; the SE is the weighted
    combination of the per-opponent SEs.  `mirror` and `past` are absent, so
    like CULTURE_GAP 19 the denominator is the FULL 8.0 tier total and the
    missing 2.0 counts as zero edge.
    """
    total_w = sum(P.DEFAULT_TIER_WEIGHTS[t] for t in
                  ("book", "variant", "mirror", "past", "floor"))
    num, var = 0.0, 0.0
    per_op = {}
    for lab, meta in rec["opponents"].items():
        cand = rec["series"][name][lab]
        ref = rec["series"]["base"][lab]
        if meta["metric"] == "margin":
            edges = credit(cand["margin"], ref["margin"])
        else:
            edges = [c - r for c, r in zip(cand["win"], ref["win"])]
        edges = [e for e in edges if e is not None]
        m = statistics.fmean(edges)
        se = (statistics.stdev(edges) / math.sqrt(len(edges))
              if len(edges) > 1 else 0.0)
        w = meta["weight"]
        num += w * m
        var += (w * se) ** 2
        per_op[lab] = (m, se)
    return num / total_w, math.sqrt(var) / total_w, per_op


def score(path):
    with open(path) as fh:
        rec = json.load(fh)
    names = [n for n in rec["series"] if n != "base"]
    print(f"# {rec['players']}p, n={rec['games']}/cell, paired against `base` "
          f"(DEFAULT_WEIGHTS) on identical seeds")
    print("# pool-weighted accept statistic (edge vs base), and its detectability "
          "in sigma\n")
    hdr = f"  {'credit fn':<12}" + "".join(f"{n:>22}" for n in names)
    print(hdr)
    print("  " + "-" * (len(hdr) - 2))
    for credit in CANDIDATES:
        cells = []
        for n in names:
            m, se, _ = _agg(rec, n, credit)
            cells.append(f"{m:>+11.4f}({m / se if se else 0:>+5.1f}s)")
        print(f"  {credit.__name__:<12}" + "".join(f"{c:>22}" for c in cells))

    print("\n# the ratio that matters: |real weakening| / |perverse gain|, in "
          "raw statistic and in sigma")
    print(f"  {'credit fn':<12}{'perverse':>12}{'perverse s':>12}"
          f"{'sci_neg/perv':>14}{'cr_zero/perv':>14}{'drift/perv':>14}")
    for credit in CANDIDATES:
        p, pse, _ = _agg(rec, "perverse", credit)
        row = f"  {credit.__name__:<12}{p:>+12.4f}{(p / pse if pse else 0):>+12.2f}"
        for n in ("sci_neg", "cr_zero", "drift"):
            m, _se, _ = _agg(rec, n, credit)
            row += f"{(abs(m) / abs(p) if p else float('nan')):>14.1f}"
        print(row)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--players", type=int, default=4)
    ap.add_argument("--games", type=int, default=150)
    ap.add_argument("--workers", type=int, default=4)
    ap.add_argument("--out", default="/tmp/margin_ab.json")
    ap.add_argument("--score", default=None,
                    help="skip collection, score an existing dump")
    ap.add_argument("--only", default=None,
                    help="collect only these vectors (comma-separated).  The "
                         "seeds are a pure function of the opponent label, so "
                         "shards collected by separate processes are still "
                         "paired game-for-game and `--merge` can glue them")
    ap.add_argument("--merge", nargs="*", default=None,
                    help="combine shard dumps into --out and score it")
    a = ap.parse_args()
    if a.merge:
        base = None
        for p in a.merge:
            with open(p) as fh:
                r = json.load(fh)
            if base is None:
                base = r
            else:
                base["series"].update(r["series"])
        with open(a.out, "w") as fh:
            json.dump(base, fh)
        score(a.out)
    elif a.score:
        score(a.score)
    else:
        collect(a.players, a.games, a.workers, a.out,
                only=set(a.only.split(",")) if a.only else None)
        if not a.only:
            score(a.out)


if __name__ == "__main__":
    main()
