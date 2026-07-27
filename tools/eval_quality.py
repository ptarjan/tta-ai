"""How good is the evaluation function, as a *predictor of the outcome*?

This is the number that decides whether the bot needs a better search or a
better evaluation.  A search is an amplifier: it turns evaluation differences
into move choices.  If `evaluate(s)` barely predicts who wins from `s`, then
searching harder on it amplifies noise -- which is exactly what
docs/WASTED_ACTIONS.md §6 measured happening (five principled search fixes, all
significantly worse).

Reads the rows dumped by `tools/gen_value_data.py` and reports, by round:

  * R^2 of the champion's own `evaluate` against the realised culture margin
  * Spearman-style pairwise ranking accuracy within a round
  * the same for a trivial baseline (culture only), so the ~57 features have
    something to beat

    python3 tools/eval_quality.py /tmp/vdata_2p.jsonl \
        --weights experiments/arch_frozen/champ2p_gen344.json
"""
from __future__ import annotations

import argparse
import json
import math
import os
import sys
from collections import defaultdict

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine.bots.weighted import load_weights  # noqa: E402


def score(row, w):
    return row["off"] + sum(w.get(k, 0.0) * v for k, v in row["x"].items())


def r2(pairs):
    if len(pairs) < 3:
        return float("nan")
    ys = [y for _, y in pairs]
    xs = [x for x, _ in pairs]
    n = len(ys)
    mx, my = sum(xs) / n, sum(ys) / n
    sxy = sum((a - mx) * (b - my) for a, b in pairs)
    sxx = sum((a - mx) ** 2 for a in xs)
    syy = sum((b - my) ** 2 for b in ys)
    if sxx <= 0 or syy <= 0:
        return float("nan")
    r = sxy / math.sqrt(sxx * syy)
    return r * r


def pair_acc(groups, keys, by_round=False):
    """Within each game-turn, does a higher score mean a higher final margin?

    Every scorer named in `keys` is judged on **exactly the same pairs** -- a
    pair is used only if the margins differ AND every scorer separates it.
    Without that, a scorer with many ties (raw culture is 0 for everybody in
    the opening) is silently graded on a later, easier subset of the game.
    """
    ok = {k: 0 for k in keys}
    tot = 0
    per_round = defaultdict(lambda: [0, {k: 0 for k in keys}])
    for (seed, turn, rnd), g in groups.items():
        for i in range(len(g)):
            for j in range(i + 1, len(g)):
                a, b = g[i], g[j]
                if a["y"] == b["y"]:
                    continue
                if any(a[k] == b[k] for k in keys):
                    continue
                tot += 1
                per_round[rnd][0] += 1
                for k in keys:
                    if (a[k] > b[k]) == (a["y"] > b["y"]):
                        ok[k] += 1
                        per_round[rnd][1][k] += 1
    acc = {k: (ok[k] / tot if tot else float("nan")) for k in keys}
    return acc, tot, per_round


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("data")
    ap.add_argument("--weights", required=True)
    ap.add_argument("--compare", default=None)
    args = ap.parse_args()
    w = load_weights(args.weights)

    scorers = {"eval": lambda d: score(d, w),
               "culture": lambda d: d["x"].get("culture", 0.0),
               "cult+rate": lambda d: (d["x"].get("culture", 0.0)
                                       + 5.0 * d["x"].get("culture_rate", 0.0))}
    if args.compare:
        w2 = load_weights(args.compare)
        scorers["eval2"] = lambda d: score(d, w2)
    keys = list(scorers)

    by_round = {k: defaultdict(list) for k in keys}
    groups = defaultdict(list)
    n = 0
    with open(args.data) as fh:
        for line in fh:
            try:
                d = json.loads(line)
            except Exception:
                continue
            n += 1
            rec = {"y": d["margin"]}
            for k, f in scorers.items():
                rec[k] = f(d)
                by_round[k][d["round"]].append((rec[k], rec["y"]))
            groups[(d["seed"], d["turn"], d["round"])].append(rec)

    print(f"{n} rows, weights={os.path.basename(args.weights)}"
          + (f", compare={os.path.basename(args.compare)}" if args.compare else ""))
    hdr = f"{'round':>6} {'n':>7}" + "".join(f"{'R2 ' + k:>11}" for k in keys)
    print(hdr)
    allp = {k: [] for k in keys}
    for r in sorted(by_round[keys[0]]):
        line = f"{r:>6} {len(by_round[keys[0]][r]):>7}"
        for k in keys:
            allp[k] += by_round[k][r]
            line += f"{r2(by_round[k][r]):>11.4f}"
        if len(by_round[keys[0]][r]) >= 40:
            print(line)
    line = f"{'ALL':>6} {len(allp[keys[0]]):>7}"
    for k in keys:
        line += f"{r2(allp[k]):>11.4f}"
    print(line)

    acc, tot, per_round = pair_acc(groups, keys)
    print("\nwithin-turn pairwise ranking accuracy, MATCHED PAIRS "
          f"(every scorer judged on the identical {tot} pairs)")
    for k in keys:
        se = math.sqrt(acc[k] * (1 - acc[k]) / tot) if tot else 0.0
        print(f"  {k:10s} {acc[k]:.4f} +/- {1.96*se:.4f}")
    print("  0.500 = coin flip, 1.000 = perfect")
    print("\nby round:")
    print(f"{'round':>6} {'pairs':>7}" + "".join(f"{k:>11}" for k in keys))
    for r in sorted(per_round):
        t, o = per_round[r]
        if t < 30:
            continue
        print(f"{r:>6} {t:>7}" + "".join(f"{o[k]/t:>11.3f}" for k in keys))


if __name__ == "__main__":
    main()
