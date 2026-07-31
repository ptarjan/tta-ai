"""Does the TRAINER'S OWN GATE SCORE pay for inflating `culture_rate`?

docs/CULTURE_GAP.md section 15b established that the observed `culture_rate`
base (32.2 at 2p, 35.6 at 4p, against a default of 5.0) is far outside what
undirected drift produces, so something is selecting for it.  This measures the
obvious suspect directly.

The trainer does not score any opponent on win rate.  As of 2026-07-30
`hillclimb_pool` scores every tier on

    lead_share(m) = 0.5 * (1 + tanh(m / 120.0))       m = own culture - best

(when this sweep was written the same squash was applied to the culture margin
over the MEAN opponent, and only on the `book`/`variant` gate tiers; the
conclusions below are about the culture-production lever and are unaffected by
which opponent the differential is taken against).  The reward is a squashed
**culture differential**, used because win share carries no information at all
against opponents the champion loses to ~100% of the time.

Culture margin is the game's real score margin, so that is a defensible design.
But it means a candidate is paid for *losing by less*, and the single most
direct lever on culture margin is the weight on culture production.  If
inflating `culture_rate` raises the gate score while doing nothing for -- or
actively harming -- the win rate against opponents that can actually be beaten,
then the gate is buying margin it cannot convert, and that is the selection
pressure section 15b is looking for.

Every vector is played on the SAME seeds against the SAME opponents, so the
columns are directly comparable; `--baseline` additionally reports each vector
paired against the first one.

    python3 tools/level_sweep.py --players 4 --games 200 --levels 5,35.574
"""
import argparse
import math
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine.bots.weighted import DEFAULT_WEIGHTS, load_weights  # noqa
from experiments import hillclimb_league as L  # noqa
from experiments import hillclimb_pool as P  # noqa


def mean_ci(xs, z=1.96):
    xs = [x for x in xs if x is not None]
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
    ap.add_argument("--key", default="culture_rate")
    ap.add_argument("--levels", default="5,35.574")
    ap.add_argument("--opponents", default="var:culture,book,greedy")
    ap.add_argument("--workers", type=int, default=1)
    a = ap.parse_args()

    levels = [float(x) for x in a.levels.split(",")]
    pool = P.build_pool(a.players, ladder_dirs=(), past_k=0, log=lambda *_x: None)
    by = {e.label: e for e in pool.sorted_entries()}

    print(f"# level sweep {a.players}p  n={a.games}/cell  key={a.key}")
    print(f"# DEFAULT_WEIGHTS with {a.key} set to each level; everything else "
          f"untouched (including {a.key}_early/_late).")
    print(f"# gate_score is the trainer's own accept statistic for that "
          f"opponent: margin_share(culture margin) on margin tiers, win share "
          f"otherwise.\n")

    for lab in a.opponents.split(","):
        e = by[lab]
        seed0 = (20260726 + L.label_seed(lab) * 17) % 10_000_019
        print(f"  {lab}  (tier={e.tier}, trainer metric={e.metric})")
        print(f"    {'level':>9} {'win rate':>16} {'culture margin':>20} "
              f"{'gate score':>16}")
        for lv in levels:
            w = dict(DEFAULT_WEIGHTS)
            w[a.key] = lv
            r = L._series(w, e.spec, a.players, a.games, seed0, a.workers, e.metric)
            wm, wc = mean_ci(r["win"])
            mm, mc = mean_ci(r["margin"])
            gm, gc = mean_ci(r["score"])
            print(f"    {lv:>9.3f} {wm:>9.3f}+-{wc:.3f} {mm:>13.1f}+-{mc:5.1f} "
                  f"{gm:>9.4f}+-{gc:.4f}", flush=True)
        print()


if __name__ == "__main__":
    main()
