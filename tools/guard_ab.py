"""Fix #1's only measurable consequence: what the guard would DO to a champion.

Same pairing as the trainer's `ablate()` -- `hillclimb_league._series`, one
`arena.duel` per arm on identical seeds -- at n>=200, which is the instrument
docs/CULTURE_GAP.md section 6 asked for and the resolution its own n=48
counterfactual could not reach.

    python3 tools/guard_ab.py 4 200 /tmp/champ4p_snap.json rival_culture=0.0
"""
import math
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine.bots.weighted import load_weights  # noqa
from experiments import hillclimb_pool as P  # noqa
from experiments import hillclimb_league as L  # noqa


def mean_ci(xs, z=1.96):
    n = len(xs)
    if n < 2:
        return (sum(xs) / max(1, n)), 0.0
    m = sum(xs) / n
    var = sum((x - m) ** 2 for x in xs) / (n - 1)
    return m, z * math.sqrt(var / n)


players = int(sys.argv[1])
games = int(sys.argv[2])
champ_path = sys.argv[3]
sets = [s.split("=") for s in sys.argv[4:]]
champ = load_weights(champ_path)
patched = dict(champ)
for k, v in sets:
    patched[k] = float(v)

pool = P.build_pool(players, ladder_dirs=(), past_k=0, log=lambda *_a: None)
by = {e.label: e for e in pool.sorted_entries()}
want = ["var:culture", "book"]

print(f"# guard A/B {players}p  n={games}/opponent   "
      + ", ".join(f"{k}: {champ.get(k, 0.0):+.3f} -> {float(v):+.3f}"
                  for k, v in sets))
print("# edge = PATCHED - CHAMPION, paired on seeds")
allw, allm = [], []
for lab in want:
    e = by[lab]
    seed0 = (20260726 + L.label_seed(lab) * 17) % 10_000_019
    a = L._series(patched, e.spec, players, games, seed0, 2, e.metric)
    b = L._series(champ, e.spec, players, games, seed0, 2, e.metric)
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
    allw += dw
    allm += dm
    print(f"  {lab:<14} n={len(dw):4d}  win {wa:6.3f} vs {wb:6.3f} "
          f"edge {dwm:+.4f} +/-{dwc:.4f} | margin {ma:+7.1f} vs {mb:+7.1f} "
          f"edge {dmm:+7.2f} +/-{dmc:.2f}")
dwm, dwc = mean_ci(allw)
dmm, dmc = mean_ci(allm)
print(f"  {'POOLED':<14} n={len(allw):4d}  win edge {dwm:+.4f} +/-{dwc:.4f} | "
      f"margin edge {dmm:+7.2f} +/-{dmc:.2f}")
