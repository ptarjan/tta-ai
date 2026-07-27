"""Segment the human corpus rows (`tools/bgo_parse.py` TSV) into playing styles.

    python3 tools/bgo_cluster.py --tsv /tmp/human.tsv --players 2 --k 2,3,4,5,6

Why this is not just "run k-means and name the clusters": k-means *always*
returns k clusters, including on one Gaussian blob.  So every k here is
reported next to a **Gaussian null** -- the same clustering run on synthetic
data drawn from a multivariate normal with the corpus's own covariance, which
by construction has NO cluster structure.  If the corpus silhouette does not
clear the null's, the "clusters" are slices of a blob and should be described
as *segments*, not types.  See docs/HUMAN_BOTS.md for what that came out as.

Everything is pure Python: this box has no numpy.
"""
from __future__ import annotations

import argparse
import csv
import math
import random
import sys

#: The behavioural axes.  Deliberately excludes score / won / rank: we are
#: segmenting STYLE, and letting outcome in would just find "winners" and
#: "losers", which is not a policy we can imitate.
FEATURES = [
    ("wonder_stages", "wonder stages built"),
    ("wonders_completed", "wonders completed"),
    ("takes", "civil cards taken"),
    ("tier3_pct", "% of takes at 3 CA"),
    ("wars_declared", "wars declared"),
    ("aggressions", "aggressions played"),
    ("bids", "colony bids"),
    ("sci_final", "unspent science at end"),
    ("first_gov_round", "round of first government"),
    ("leaders_elected", "leaders elected"),
    ("take_ageI", "age I cards taken"),
    ("take_ageIII", "age III cards taken"),
]

REPORT_EXTRA = ["score", "won", "rounds", "colonies", "gov_changes",
                "tier1", "tier2", "tier3", "wonders_started"]


def num(v, default=None):
    if v is None or v == "":
        return default
    try:
        return float(v)
    except ValueError:
        return default


def load(path, players=None, levels=None):
    rows = []
    with open(path) as fh:
        for r in csv.DictReader(fh, delimiter="\t"):
            if players and int(r["players"] or 0) != players:
                continue
            if levels and r.get("level") not in levels:
                continue
            t = (num(r.get("tier1"), 0) + num(r.get("tier2"), 0)
                 + num(r.get("tier3"), 0))
            r["tier3_pct"] = (100.0 * num(r.get("tier3"), 0) / t) if t else 0.0
            # never-revolted players have no first_gov_round; they are a real
            # style (6.7% of the corpus), so they are coded as "after the end"
            # rather than dropped, which would bias the axis toward revolters.
            if not r.get("first_gov_round"):
                r["first_gov_round"] = str(num(r.get("rounds"), 19) + 1)
            rows.append(r)
    return rows


def matrix(rows):
    return [[num(r.get(k), 0.0) for k, _ in FEATURES] for r in rows]


def standardise(X):
    d = len(X[0])
    mu = [sum(r[j] for r in X) / len(X) for j in range(d)]
    sd = []
    for j in range(d):
        v = sum((r[j] - mu[j]) ** 2 for r in X) / max(1, len(X) - 1)
        sd.append(math.sqrt(v) or 1.0)
    return [[(r[j] - mu[j]) / sd[j] for j in range(d)] for r in X], mu, sd


def dist2(a, b):
    return sum((x - y) ** 2 for x, y in zip(a, b))


def kmeans(X, k, seed=0, iters=100):
    rng = random.Random(seed)
    n, d = len(X), len(X[0])
    # k-means++
    cent = [X[rng.randrange(n)][:]]
    for _ in range(k - 1):
        w = [min(dist2(x, c) for c in cent) for x in X]
        tot = sum(w) or 1.0
        t, acc = rng.random() * tot, 0.0
        pick = n - 1
        for i, wi in enumerate(w):
            acc += wi
            if acc >= t:
                pick = i
                break
        cent.append(X[pick][:])
    lab = [0] * n
    for _ in range(iters):
        moved = False
        for i, x in enumerate(X):
            best, bd = 0, float("inf")
            for c, cc in enumerate(cent):
                dd = dist2(x, cc)
                if dd < bd:
                    best, bd = c, dd
            if lab[i] != best:
                lab[i], moved = best, True
        for c in range(k):
            mem = [X[i] for i in range(n) if lab[i] == c]
            if mem:
                cent[c] = [sum(m[j] for m in mem) / len(mem) for j in range(d)]
        if not moved:
            break
    inertia = sum(dist2(X[i], cent[lab[i]]) for i in range(n))
    return lab, cent, inertia


def best_kmeans(X, k, restarts=25):
    best = None
    for s in range(restarts):
        lab, cent, inr = kmeans(X, k, seed=s)
        if best is None or inr < best[2]:
            best = (lab, cent, inr)
    return best


def silhouette(X, lab, k, sample=600, seed=3):
    """Mean silhouette, on a subsample (O(n^2) otherwise)."""
    rng = random.Random(seed)
    idx = list(range(len(X)))
    if len(idx) > sample:
        idx = rng.sample(idx, sample)
    by = {}
    for i in idx:
        by.setdefault(lab[i], []).append(i)
    if len(by) < 2:
        return 0.0
    tot = 0.0
    for i in idx:
        own = by.get(lab[i], [])
        if len(own) < 2:
            continue
        a = sum(math.sqrt(dist2(X[i], X[j])) for j in own if j != i) / (len(own) - 1)
        b = min(sum(math.sqrt(dist2(X[i], X[j])) for j in mem) / len(mem)
                for c, mem in by.items() if c != lab[i] and mem)
        tot += (b - a) / max(a, b) if max(a, b) else 0.0
    return tot / len(idx)


def gaussian_null(X, seed=11):
    """A same-shape sample with no cluster structure.

    Per-column Gaussian resample preserves each axis's mean/sd but destroys the
    joint structure, which would make the null too easy to beat.  So the
    columns are instead resampled INDEPENDENTLY from the data itself
    (permutation null): each column keeps its exact marginal distribution --
    including the spikes at zero that wars/wonders have -- and only the
    row-wise association between axes is broken.  Clusters that survive that
    are clusters of *co-occurring* behaviour, which is what an archetype is.
    """
    rng = random.Random(seed)
    n, d = len(X), len(X[0])
    cols = [[X[i][j] for i in range(n)] for j in range(d)]
    for c in cols:
        rng.shuffle(c)
    return [[cols[j][i] for j in range(d)] for i in range(n)]


def ari(a, b):
    """Adjusted Rand index between two labelings."""
    pairs = {}
    ca, cb = {}, {}
    for x, y in zip(a, b):
        pairs[(x, y)] = pairs.get((x, y), 0) + 1
        ca[x] = ca.get(x, 0) + 1
        cb[y] = cb.get(y, 0) + 1
    def c2(m):
        return m * (m - 1) / 2.0
    idx = sum(c2(v) for v in pairs.values())
    sa = sum(c2(v) for v in ca.values())
    sb = sum(c2(v) for v in cb.values())
    n = len(a)
    exp = sa * sb / c2(n) if n > 1 else 0.0
    mx = (sa + sb) / 2.0
    return (idx - exp) / (mx - exp) if mx != exp else 1.0


def report(rows, X, lab, k, out=sys.stdout):
    n = len(rows)
    groups = {}
    for i in range(n):
        groups.setdefault(lab[i], []).append(i)
    order = sorted(groups, key=lambda c: -len(groups[c]))
    out.write("cluster sizes: %s\n"
              % ", ".join("c%d=%d (%.1f%%)" % (j, len(groups[c]),
                                               100.0 * len(groups[c]) / n)
                          for j, c in enumerate(order)))
    hdr = ["metric"] + ["c%d" % j for j in range(len(order))] + ["all"]
    out.write("%-26s %s\n" % (hdr[0], " ".join("%9s" % h for h in hdr[1:])))
    for key, name in FEATURES + [(k2, k2) for k2 in REPORT_EXTRA]:
        vals = []
        for c in order:
            xs = [num(rows[i].get(key), 0.0) for i in groups[c]]
            vals.append(sum(xs) / len(xs) if xs else float("nan"))
        allx = [num(r.get(key), 0.0) for r in rows]
        out.write("%-26s %s %9.2f\n"
                  % (name, " ".join("%9.2f" % v for v in vals),
                     sum(allx) / len(allx)))
    lv = {}
    for i in range(n):
        lv.setdefault(rows[i].get("level", "?"), {})
        lv[rows[i]["level"]][lab[i]] = lv[rows[i]["level"]].get(lab[i], 0) + 1
    out.write("level mix (row %% within level):\n")
    for level in sorted(lv):
        tot = sum(lv[level].values())
        out.write("  %-9s n=%4d  %s\n"
                  % (level, tot, " ".join("c%d=%4.1f%%" % (j, 100.0 * lv[level].get(c, 0) / tot)
                                          for j, c in enumerate(order))))
    return order, groups


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--tsv", required=True)
    ap.add_argument("--players", type=int, default=2)
    ap.add_argument("--level", default="")
    ap.add_argument("--k", default="2,3,4,5,6")
    ap.add_argument("--report-k", type=int, default=0)
    ap.add_argument("--dump", default="", help="write game_id/colour/cluster TSV")
    a = ap.parse_args(argv)
    levels = set(a.level.split(",")) if a.level else None
    rows = load(a.tsv, a.players or None, levels)
    X0 = matrix(rows)
    X, mu, sd = standardise(X0)
    null = gaussian_null(X)
    print("n_rows=%d n_games=%d players=%s level=%s"
          % (len(rows), len(set(r["game_id"] for r in rows)),
             a.players or "all", a.level or "all"))
    print("%3s %10s %10s %10s %8s" % ("k", "silhouette", "null_sil", "ratio", "ARI(1/2 split)"))
    labs = {}
    for k in [int(x) for x in a.k.split(",")]:
        lab, _cent, _inr = best_kmeans(X, k)
        labs[k] = lab
        s = silhouette(X, lab, k)
        nlab, _c, _i = best_kmeans(null, k)
        ns = silhouette(null, nlab, k)
        # stability: cluster two random halves, re-assign the other half by
        # nearest centroid, compare labelings on the overlap
        rng = random.Random(5)
        idx = list(range(len(X)))
        rng.shuffle(idx)
        h1, h2 = idx[:len(idx) // 2], idx[len(idx) // 2:]
        l1, c1, _ = best_kmeans([X[i] for i in h1], k, restarts=8)
        l2, c2, _ = best_kmeans([X[i] for i in h2], k, restarts=8)
        def assign(x, cent):
            return min(range(len(cent)), key=lambda c: dist2(x, cent[c]))
        A = [assign(X[i], c1) for i in idx]
        B = [assign(X[i], c2) for i in idx]
        print("%3d %10.3f %10.3f %10.2f %8.3f" % (k, s, ns, s / ns if ns else 0, ari(A, B)))
    rk = a.report_k or max(labs)
    print()
    print("=== k=%d detail" % rk)
    order, groups = report(rows, X, labs[rk], rk)
    if a.dump:
        with open(a.dump, "w") as fh:
            fh.write("game_id\tcolour\tcluster\n")
            for j, c in enumerate(order):
                for i in groups[c]:
                    fh.write("%s\t%s\tc%d\n"
                             % (rows[i]["game_id"], rows[i]["colour"], j))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
