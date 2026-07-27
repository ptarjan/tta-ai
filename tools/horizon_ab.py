"""A/B the game horizon: `lateness()` on rounds-left vs the old age bucket.

Thin driver over the trainer's OWN ablation machinery.  `hillclimb_league.
_series` is the function `ablate()` and `score_candidate()` both call: one
`arena.duel`, seat-rotated, returning the per-game win-share and culture-margin
series.  Two vectors that differ only in the `horizon_age` escape hatch are
duelled against the same opponents on the same seeds, so the comparison is
paired game by game exactly as an ablation is.

    OLD = the champion (or DEFAULT_WEIGHTS) with `horizon_age: 1.0`
    NEW = the same vector without it

`edge` is NEW - OLD, so edge > 0 means the rounds-left horizon is better.

Usage:
    python3 tools/horizon_ab.py --players 4 --games 200 \
        --weights /tmp/champ4p_snap.json --opponents var:culture,book
"""
import argparse
import json
import math
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine.bots.weighted import DEFAULT_WEIGHTS, load_weights  # noqa: E402
from experiments import hillclimb_pool as P  # noqa: E402
from experiments import hillclimb_league as L  # noqa: E402


def mean_ci(xs, z=1.96):
    n = len(xs)
    if n < 2:
        return (sum(xs) / max(1, n)), 0.0
    m = sum(xs) / n
    var = sum((x - m) ** 2 for x in xs) / (n - 1)
    return m, z * math.sqrt(var / n)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--players", type=int, default=4, choices=(2, 3, 4))
    ap.add_argument("--games", type=int, default=200)
    ap.add_argument("--workers", type=int, default=2)
    ap.add_argument("--seed", type=int, default=20260726)
    ap.add_argument("--weights", default="", help="weight file; blank = default")
    ap.add_argument("--opponents", default="var:culture,book")
    ap.add_argument("--label", default="")
    ap.add_argument("--out", default="")
    args = ap.parse_args()

    base = dict(DEFAULT_WEIGHTS) if not args.weights \
        else load_weights(args.weights)
    base.pop("horizon_age", None)
    new = dict(base)
    old = dict(base, horizon_age=1.0)

    pool = P.build_pool(args.players, ladder_dirs=(), past_k=0,
                        log=lambda *_a: None)
    by_label = {e.label: e for e in pool.sorted_entries()}
    want = [s.strip() for s in args.opponents.split(",") if s.strip()]
    missing = [s for s in want if s not in by_label]
    if missing:
        raise SystemExit(f"unknown opponents {missing}; have "
                         f"{sorted(by_label)}")

    games = max(args.players, (args.games // args.players) * args.players)
    label = args.label or (args.weights or "default")
    print(f"# horizon A/B  {args.players}p  n={games}/opponent  "
          f"weights={label}")
    print(f"# NEW = rounds-left lateness();  OLD = age bucket "
          f"(horizon_age=1.0).  edge = NEW - OLD, paired on seeds.")

    rows = []
    allwin, allmar = [], []
    for lab in want:
        e = by_label[lab]
        seed0 = (args.seed + L.label_seed(lab) * 17) % 10_000_019
        a = L._series(new, e.spec, args.players, games, seed0,
                      args.workers, e.metric)
        b = L._series(old, e.spec, args.players, games, seed0,
                      args.workers, e.metric)
        dw = [x - y for x, y in zip(a["win"], b["win"])
              if x is not None and y is not None]
        dm = [x - y for x, y in zip(a["margin"], b["margin"])
              if x is not None and y is not None]
        wa, _ = mean_ci([x for x in a["win"] if x is not None])
        wb, _ = mean_ci([x for x in b["win"] if x is not None])
        ma, _ = mean_ci([x for x in a["margin"] if x is not None])
        mb, _ = mean_ci([x for x in b["margin"] if x is not None])
        dwm, dwc = mean_ci(dw)
        dmm, dmc = mean_ci(dm)
        allwin += dw
        allmar += dm
        rows.append({"opponent": lab, "n": len(dw),
                     "win_new": wa, "win_old": wb,
                     "margin_new": ma, "margin_old": mb,
                     "d_win": dwm, "d_win_ci": dwc,
                     "d_margin": dmm, "d_margin_ci": dmc})
        print(f"  {lab:<20} n={len(dw):4d}  win {wa:6.3f} vs {wb:6.3f}  "
              f"edge {dwm:+.4f} +/-{dwc:.4f} | "
              f"culture margin {ma:+7.1f} vs {mb:+7.1f}  "
              f"edge {dmm:+7.2f} +/-{dmc:.2f}")

    dwm, dwc = mean_ci(allwin)
    dmm, dmc = mean_ci(allmar)
    print(f"  {'POOLED':<20} n={len(allwin):4d}  "
          f"win edge {dwm:+.4f} +/-{dwc:.4f} | "
          f"culture-margin edge {dmm:+7.2f} +/-{dmc:.2f}")
    out = {"players": args.players, "games": games, "weights": label,
           "seed": args.seed, "rows": rows,
           "pooled": {"n": len(allwin), "d_win": dwm, "d_win_ci": dwc,
                      "d_margin": dmm, "d_margin_ci": dmc}}
    if args.out:
        with open(args.out, "w") as fh:
            json.dump(out, fh, indent=1)


if __name__ == "__main__":
    main()
