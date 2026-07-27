"""Fit `weighted.evaluate`'s 80 linear coefficients by ridge regression on
self-play outcomes, and write them out as a drop-in weight file.

This is the same hypothesis class the hill climb searches -- identical
features, identical parameterisation -- so a head-to-head between a fitted
vector and a climbed one is a clean test of the TRAINER, holding the bot fixed.

Stdlib only (normal equations + Cholesky).  80 columns makes that trivial; the
cost is accumulating X^T X, which is O(rows * 80^2 / 2).

    python3 tools/fit_value.py /tmp/vdata_2p.jsonl \
        --ref experiments/arch_frozen/champ2p_gen344.json \
        --out /tmp/fit_2p.json --lam 1.0 --target margin
"""
from __future__ import annotations

import argparse
import json
import math
import os
import random
import sys


def load(paths, target, holdout=0.15, seed=0):
    rows = []
    cols = None
    for p in paths:
        with open(p) as fh:
            for line in fh:
                d = json.loads(line)
                if cols is None:
                    cols = sorted(d["x"])
                rows.append(d)
    rng = random.Random(seed)
    # hold out whole GAMES, never rows: rows from one game share an outcome
    seeds = sorted({d["seed"] for d in rows})
    rng.shuffle(seeds)
    ho = set(seeds[:max(1, int(len(seeds) * holdout))])
    tr = [d for d in rows if d["seed"] not in ho]
    te = [d for d in rows if d["seed"] in ho]
    return cols, tr, te


def target_of(d, target, scale, prior=None, cols=None):
    """The regression target.

    With `--prior`, fit the RESIDUAL against a reference vector instead of
    against zero.  That turns the ridge penalty into shrinkage toward a known
    working policy rather than toward the origin, which matters here for a
    concrete reason: the design matrix is exactly rank-deficient (for each
    PHASE_KEY, `(base+c, early-c, late-c)` is the identical function), so a
    zero-centred ridge picks a minimum-norm solution with large cancelling
    coefficients.  Those cancel *on the data manifold* and not off it -- and
    a greedy bot's argmax deliberately searches off it.  See
    docs/BOT_ARCHITECTURE.md §3b for the duel that this explains.

    lam -> infinity recovers the prior exactly; lam -> 0 recovers the plain
    fit.  So `--lam` sweeps a straight line between the champion and the data.
    """
    y = d["margin"] if target == "margin" else d["win"]
    if target == "margin" and scale:
        y = math.tanh(y / scale) * scale
    y -= d["off"]
    if prior:
        x = d["x"]
        y -= sum(prior.get(c, 0.0) * x.get(c, 0.0) for c in cols)
    return y


def cholesky_solve(A, b):
    n = len(b)
    L = [[0.0] * n for _ in range(n)]
    for i in range(n):
        for j in range(i + 1):
            s = A[i][j] - sum(L[i][k] * L[j][k] for k in range(j))
            if i == j:
                if s <= 0:
                    s = 1e-9
                L[i][i] = math.sqrt(s)
            else:
                L[i][j] = s / L[j][j]
    y = [0.0] * n
    for i in range(n):
        y[i] = (b[i] - sum(L[i][k] * y[k] for k in range(i))) / L[i][i]
    x = [0.0] * n
    for i in range(n - 1, -1, -1):
        x[i] = (y[i] - sum(L[k][i] * x[k] for k in range(i + 1, n))) / L[i][i]
    return x


def fit(cols, rows, target, lam, scale, prior=None):
    n = len(cols)
    # standardise so one ridge penalty means the same thing on every column
    mean = [0.0] * n
    for d in rows:
        x = d["x"]
        for j, c in enumerate(cols):
            mean[j] += x.get(c, 0.0)
    m = len(rows)
    mean = [v / m for v in mean]
    sd = [0.0] * n
    for d in rows:
        x = d["x"]
        for j, c in enumerate(cols):
            sd[j] += (x.get(c, 0.0) - mean[j]) ** 2
    sd = [math.sqrt(v / m) or 1.0 for v in sd]

    A = [[0.0] * (n + 1) for _ in range(n + 1)]   # +1 intercept
    b = [0.0] * (n + 1)
    for d in rows:
        x = d["x"]
        z = [(x.get(c, 0.0) - mean[j]) / sd[j] for j, c in enumerate(cols)]
        z.append(1.0)
        y = target_of(d, target, scale, prior, cols)
        for i in range(n + 1):
            zi = z[i]
            if zi:
                Ai = A[i]
                for j in range(i + 1):
                    Ai[j] += zi * z[j]
                b[i] += zi * y
    for i in range(n + 1):
        for j in range(i + 1, n + 1):
            A[i][j] = A[j][i]
        if i < n:
            A[i][i] += lam * m / 1000.0
    w = cholesky_solve(A, b)
    # un-standardise
    raw = {cols[j]: w[j] / sd[j] for j in range(n)}
    intercept = w[n] - sum(w[j] * mean[j] / sd[j] for j in range(n))
    return raw, intercept, mean, sd


def r2(cols, rows, raw, intercept, target, scale, prior=None):
    ys, ps = [], []
    for d in rows:
        x = d["x"]
        p = intercept + sum(raw[c] * x.get(c, 0.0) for c in cols)
        ys.append(target_of(d, target, scale, prior, cols))
        ps.append(p)
    my = sum(ys) / len(ys)
    ss_t = sum((y - my) ** 2 for y in ys)
    ss_r = sum((y - p) ** 2 for y, p in zip(ys, ps))
    return 1.0 - ss_r / ss_t if ss_t else 0.0


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("data", nargs="+")
    ap.add_argument("--ref", required=True, help="reference weight file (for the keys it cannot fit)")
    ap.add_argument("--out", required=True)
    ap.add_argument("--lam", type=float, default=1.0)
    ap.add_argument("--target", default="margin", choices=("margin", "win"))
    ap.add_argument("--scale", type=float, default=0.0,
                    help="tanh-squash the margin at this scale (0 = raw)")
    ap.add_argument("--end-turn-bias", type=float, default=0.0)
    ap.add_argument("--prior", default=None,
                    help="shrink toward this weight file instead of toward 0")
    args = ap.parse_args()

    cols, tr, te = load(args.data, args.target)
    prior = None
    if args.prior:
        pj = json.load(open(args.prior))
        prior = pj.get("weights", pj)
    print(f"{len(tr)} train rows, {len(te)} holdout rows, {len(cols)} columns"
          + (f", prior={os.path.basename(args.prior)}" if args.prior else ""))
    raw, intercept, _, _ = fit(cols, tr, args.target, args.lam, args.scale, prior)
    print(f"residual train R2 {r2(cols, tr, raw, intercept, args.target, args.scale, prior):.4f}   "
          f"holdout R2 {r2(cols, te, raw, intercept, args.target, args.scale, prior):.4f}")
    if prior:
        raw = {c: raw.get(c, 0.0) + prior.get(c, 0.0) for c in cols}

    ref = json.load(open(args.ref))
    refw = ref.get("weights", ref)
    out = dict.fromkeys(refw, 0.0)
    out.update({k: round(v, 6) for k, v in raw.items()})
    # not linear in the design matrix -> carried over, not fitted
    out["hand_potential"] = refw.get("hand_potential", 0.0)
    out["end_turn_bias"] = args.end_turn_bias
    json.dump({"gen": -1, "source": "ridge fit on self-play outcomes",
               "lam": args.lam, "target": args.target, "scale": args.scale,
               "weights": out}, open(args.out, "w"), indent=1)
    print("wrote", args.out)
    top = sorted(raw.items(), key=lambda kv: -abs(kv[1]))[:18]
    for k, v in top:
        print(f"  {k:26s} {v:+10.4f}   (was {refw.get(k, 0.0):+.4f})")


if __name__ == "__main__":
    main()
