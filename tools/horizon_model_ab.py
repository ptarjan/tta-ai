"""A/B the horizon MODEL: the measured deal rate + exact gauge vs the fitted
constants it replaces.

`docs/CULTURE_GAP.md`'s `tools/horizon_ab.py` A/Bs `horizon_age`, the switch
between the rounds-left gauge and the age bucket that preceded it.  This is the
next one along: `horizon_legacy` restores `CARDS_PER_ROUND = {2: 6.29, ...}`
AND the fitted `(z - rounds_left) / (z - 5)` map, both at once, for one weight
vector -- so the two horizon MODELS can be seated at the same table.

    NEW = the vector as it ships (measured take rate, exact supply gauge)
    OLD = the same vector with `horizon_legacy: 1.0`

Head-to-head, seat-rotated, so the null is exactly `1/players`: 50.0% at 2p,
33.3% at 3p, 25.0% at 4p.  A win share above the null means the new model
plays better; the point of running it is that it might not, and the answer is
reported either way.

    python3 tools/horizon_model_ab.py --players 2 --games 120
    python3 tools/horizon_model_ab.py --players 2 --games 120 \
        --weights experiments/champion_2p.json

The `--weights` run is the one that matters for a TRAINED vector: that vector
was fitted under the OLD gauge, so it is the arm with something to lose.  See
docs/MODEL_CONSTANTS.md section 5.
"""
from __future__ import annotations

import argparse
import json
import math
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine.bots.weighted import DEFAULT_WEIGHTS, load_weights  # noqa: E402
from experiments import arena  # noqa: E402


def wilson(k, n, z=1.959963984540054):
    """Win share with a stderr, and the interval that does not fall off the
    end of [0, 1] at the extremes."""
    if n == 0:
        return 0.0, 0.0, (0.0, 1.0)
    p = k / n
    se = math.sqrt(max(0.0, p * (1.0 - p)) / n)
    d = 1.0 + z * z / n
    c = (p + z * z / (2 * n)) / d
    h = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / d
    return p, se, (c - h, c + h)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--players", type=int, default=2, choices=(2, 3, 4))
    ap.add_argument("--games", type=int, default=120)
    ap.add_argument("--workers", type=int, default=1)
    ap.add_argument("--seed", type=int, default=20260730)
    ap.add_argument("--bot", default="weighted")
    ap.add_argument("--weights", default="", help="weight file; blank=default")
    ap.add_argument("--out", default="")
    a = ap.parse_args(argv)

    base = dict(DEFAULT_WEIGHTS) if not a.weights else load_weights(a.weights)
    base.pop("horizon_legacy", None)
    new = {"bot": a.bot, "weights": dict(base)}
    old = {"bot": a.bot, "weights": dict(base, horizon_legacy=1.0)}

    res = arena.duel(new, old, a.players, a.games, seed0=a.seed,
                     workers=a.workers)
    per = [x for x in res["per_game"] if x is not None]
    k = sum(per)
    p, se, ci = wilson(k, len(per))
    null = 1.0 / a.players
    label = a.weights or "DEFAULT_WEIGHTS"
    print(f"# horizon MODEL A/B  {a.players}p  bot={a.bot}  weights={label}")
    print(f"# NEW (measured rate + exact gauge) vs OLD (horizon_legacy=1.0), "
          f"head to head.  null = {null:.3f}")
    print(f"  n={len(per)}  win={p:.4f} +/-{se:.4f} (stderr)  "
          f"95% CI [{ci[0]:.4f}, {ci[1]:.4f}]  null={null:.4f}  "
          f"z={(p - null) / se if se else 0.0:+.2f}")
    marg = [x for x in res.get("per_game_margin") or [] if x is not None]
    if marg:
        m = sum(marg) / len(marg)
        sd = math.sqrt(sum((x - m) ** 2 for x in marg) / max(1, len(marg) - 1))
        print(f"  culture margin (NEW - OLD) {m:+.2f} +/-"
              f"{sd / math.sqrt(len(marg)):.2f}")
    out = {"players": a.players, "weights": label, "n": len(per), "win": p,
           "stderr": se, "ci": ci, "null": null}
    if a.out:
        with open(a.out, "w") as fh:
            json.dump(out, fh, indent=1)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
