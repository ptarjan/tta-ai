"""Is the early/late phase SHAPE load-bearing, with the LEVEL held fixed?

This is the experiment docs/CULTURE_GAP.md section 11 could not settle.  The
trainer's own `--ablate` zeroes a phase multiplier and reports "no measurable
effect" -- but zeroing `culture_rate_early` does not just remove the shape, it
also moves the average price of a culture rate.  A null there is ambiguous
between "the shape does not matter" and "the two changes cancelled".

Here the contrast is built to be **mean-price matched**, so the only thing that
differs is the shape:

    price(L) = w[k] + (1-L)*w[k_early] + L*w[k_late]

    shaped:  (base, early, late)  -- as given
    flat:    (base + (1-Lbar)*early + Lbar*late,  0,  0)

with `Lbar` the mean of `lateness()` over real decisions at this player count,
measured by `--measure-lbar`.  Both vectors price a rate identically *on
average over the game*; the shaped one is expensive early and cheap late and
the flat one is neither.

Pairing is the trainer's own: `hillclimb_league._series`, one `arena.duel` per
arm on identical seeds, so seed luck cancels.

    python3 tools/shape_ab.py --players 4 --games 200 --keys culture_rate
    python3 tools/shape_ab.py --players 4 --games 200 --keys ALL
    python3 tools/shape_ab.py --players 4 --measure-lbar
"""
import argparse
import math
import os
import statistics
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine.bots.weighted import DEFAULT_WEIGHTS, PHASE_KEYS, load_weights  # noqa
from experiments import hillclimb_league as L  # noqa
from experiments import hillclimb_pool as P  # noqa

#: mean of `lateness()` over real decisions, per player count.  Produced by
#: --measure-lbar (6 self-play games, every candidate scoring recorded).
LBAR = {2: 0.6348, 3: 0.6672, 4: 0.6819}


def measure_lbar(players, games=6):
    """Mean lateness over every candidate scoring in `games` self-play games."""
    from engine import game as G
    from engine.bots import WeightedBot
    import engine.bots.weighted as w2
    seen = []
    orig = w2.lateness

    def spy(state):
        v = orig(state)
        seen.append(v)
        return v

    w2.lateness = spy
    try:
        for seed in range(games):
            G.play_game([WeightedBot(seed=seed * 10 + i) for i in range(players)],
                        players, seed=seed)
    finally:
        w2.lateness = orig
    return statistics.mean(seen), len(seen)


def flatten(w, keys, lbar):
    """Mean-price-matched removal of the phase shape on `keys`."""
    out = dict(w)
    for k in keys:
        e, la = out.get(k + "_early", 0.0), out.get(k + "_late", 0.0)
        out[k] = out.get(k, 0.0) + (1.0 - lbar) * e + lbar * la
        out[k + "_early"] = 0.0
        out[k + "_late"] = 0.0
    return out


def mean_ci(xs, z=1.96):
    n = len(xs)
    if n < 2:
        return (sum(xs) / max(1, n)), 0.0
    m = sum(xs) / n
    var = sum((x - m) ** 2 for x in xs) / (n - 1)
    return m, z * math.sqrt(var / n)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--players", type=int, default=4)
    ap.add_argument("--games", type=int, default=200)
    ap.add_argument("--weights", default="default")
    ap.add_argument("--keys", default="culture_rate",
                    help="comma-separated PHASE_KEYS, or ALL")
    ap.add_argument("--opponents", default="var:culture,book,book2")
    ap.add_argument("--workers", type=int, default=1)
    ap.add_argument("--measure-lbar", action="store_true")
    a = ap.parse_args()

    if a.measure_lbar:
        for n in (2, 3, 4):
            m, c = measure_lbar(n)
            print(f"{n}p  Lbar = {m:.4f}   ({c} scorings)")
        return

    base = (dict(DEFAULT_WEIGHTS) if a.weights == "default"
            else load_weights(a.weights))
    keys = list(PHASE_KEYS) if a.keys == "ALL" else a.keys.split(",")
    lbar = LBAR[a.players]
    flat = flatten(base, keys, lbar)

    print(f"# shape A/B {a.players}p  n={a.games}/opponent  weights={a.weights}"
          f"  Lbar={lbar}")
    print(f"# flattening {len(keys)} phase key(s): {','.join(keys)}")
    for k in keys:
        print(f"#   {k:<16} shaped ({base.get(k, 0):+.3f}, "
              f"{base.get(k + '_early', 0):+.3f}, {base.get(k + '_late', 0):+.3f})"
              f"  ->  flat ({flat[k]:+.3f}, 0, 0)")
    print("# edge = FLAT - SHAPED, paired on identical seeds.  "
          "A negative edge means the shape was earning its keep.")

    pool = P.build_pool(a.players, ladder_dirs=(), past_k=0, log=lambda *_x: None)
    by = {e.label: e for e in pool.sorted_entries()}
    allw, allm = [], []
    for lab in a.opponents.split(","):
        e = by[lab]
        seed0 = (20260726 + L.label_seed(lab) * 17) % 10_000_019
        fa = L._series(flat, e.spec, a.players, a.games, seed0, a.workers, e.metric)
        sh = L._series(base, e.spec, a.players, a.games, seed0, a.workers, e.metric)
        dw = [x - y for x, y in zip(fa["win"], sh["win"])
              if x is not None and y is not None]
        dm = [x - y for x, y in zip(fa["margin"], sh["margin"])
              if x is not None and y is not None]
        wa, _ = mean_ci([x for x in fa["win"] if x is not None])
        wb, _ = mean_ci([x for x in sh["win"] if x is not None])
        dwm, dwc = mean_ci(dw)
        dmm, dmc = mean_ci(dm)
        allw += dw
        allm += dm
        print(f"  {lab:<14} n={len(dw):4d}  win flat {wa:6.3f} vs shaped {wb:6.3f}"
              f"  edge {dwm:+.4f} +/-{dwc:.4f} | margin edge {dmm:+7.2f} +/-{dmc:.2f}",
              flush=True)
    dwm, dwc = mean_ci(allw)
    dmm, dmc = mean_ci(allm)
    print(f"  {'POOLED':<14} n={len(allw):4d}  win edge {dwm:+.4f} +/-{dwc:.4f}"
          f" | margin edge {dmm:+7.2f} +/-{dmc:.2f}")


if __name__ == "__main__":
    main()
