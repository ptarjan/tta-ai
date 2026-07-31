"""A/B the RATE HORIZON: is a per-turn rate worth `rate x turns remaining`?

DO NOT RUN THIS AS A MATTER OF COURSE.  Paired A/B batches compete with the
training league for the same six cores, and the project's standing validation
rule (docs/SYSTEM_COVERAGE.md, "How changes are validated on this project") is
to land a change and read the real league runs plus logging instead.  This tool
is kept because docs/RATE_HORIZON.md section 5b quotes numbers it produced and
those must be reproducible, and for the rare case where the box is idle.

`engine/bots/weighted.py` prices every per-turn rate through a flat weight plus
a [0, 1] phase shape.  `rate_horizon` (default 0.0) scales the rate features by
`rounds_left / mean rounds_left` instead -- see `weighted.rate_multiplier` for
the derivation, which contains no fitted constant.

This is a head-to-head at ONE table: the challenger is `DEFAULT_WEIGHTS` with
`rate_horizon = c` and every defender seat is `DEFAULT_WEIGHTS` itself, so the
two vectors differ in exactly one key, the null win rate is exactly `1/players`,
and no opponent changes between rungs.  Seats are rotated and `--seed` is shared
across rungs, so the ladder is paired game by game with itself.

    python3.13 tools/rate_horizon_ab.py --players 2 --games 300 \
        --ladder 0,0.25,0.5,0.75,1.0

`--base` duels a different reference vector (a champion snapshot) instead of
`DEFAULT_WEIGHTS`; the challenger is then that same vector plus the credit, so
it stays a one-key comparison.

The ladder is the point, not the endpoint.  A credit that helps at 1.0 and
nowhere below it is a step, and docs/OPEN_ITEMS.md warns against shipping a
positive value on a step in the hope that "the league will find the level" --
the league climbs by small perturbations and cannot cross one.
"""
from __future__ import annotations

import argparse
import json
import math
import os
import sys
import tempfile
import time

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine.bots.weighted import DEFAULT_WEIGHTS, load_weights   # noqa: E402
from experiments import arena                                    # noqa: E402


def _binom_p(k, n, p0):
    """Two-sided exact binomial p-value for k successes in n at rate p0."""
    if n <= 0:
        return 1.0
    from math import comb
    obs = comb(n, k) * p0 ** k * (1 - p0) ** (n - k)
    tot = 0.0
    for i in range(n + 1):
        pr = comb(n, i) * p0 ** i * (1 - p0) ** (n - i)
        if pr <= obs * (1 + 1e-9):
            tot += pr
    return min(1.0, tot)


def _mean_ci(xs, z=1.96):
    n = len(xs)
    if n < 2:
        return (sum(xs) / max(1, n)), 0.0
    m = sum(xs) / n
    var = sum((x - m) ** 2 for x in xs) / (n - 1)
    return m, z * math.sqrt(var / n)


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--players", type=int, default=2, choices=(2, 3, 4))
    ap.add_argument("--games", type=int, default=200)
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--workers", type=int, default=0)
    ap.add_argument("--ladder", default="0,0.25,0.5,0.75,1.0")
    ap.add_argument("--base", default="", help="reference weight file "
                    "(default: DEFAULT_WEIGHTS)")
    ap.add_argument("--out", default="")
    args = ap.parse_args(argv)

    base = dict(load_weights(args.base)) if args.base else dict(DEFAULT_WEIGHTS)
    tmp = tempfile.mkdtemp(prefix="ratehz_")
    bpath = os.path.join(tmp, "base.json")
    with open(bpath, "w") as fh:
        json.dump(base, fh)

    null = 1.0 / args.players
    rows = []
    print(f"{args.players}p, n={args.games}, null={null:.3f}, "
          f"base={args.base or 'DEFAULT_WEIGHTS'}")
    print(f"{'rate_horizon':>13} {'win':>8} {'+/-95%':>8} {'p':>9} "
          f"{'cult A':>8} {'cult B':>8} {'margin':>8}")
    for tok in args.ladder.split(","):
        c = float(tok)
        w = dict(base)
        w["rate_horizon"] = c
        cpath = os.path.join(tmp, f"c{tok}.json")
        with open(cpath, "w") as fh:
            json.dump(w, fh)
        t0 = time.time()
        res = arena.duel(arena.load_spec(cpath), arena.load_spec(bpath),
                         args.players, args.games, seed0=args.seed,
                         workers=args.workers or None)
        per = [x for x in res.get("per_game") or [] if x is not None]
        m, ci = _mean_ci(per)
        wins = sum(1 for x in per if x > 0)
        p = _binom_p(wins, len(per), null)
        mg = res.get("per_game_margin") or []
        mg = [x for x in mg if x is not None]
        margin = sum(mg) / len(mg) if mg else float("nan")
        rows.append({"rate_horizon": c, "win": m, "ci": ci, "p": p,
                     "n": len(per), "margin": margin,
                     "culture_a": res.get("culture_a"),
                     "culture_b": res.get("culture_b"),
                     "secs": round(time.time() - t0, 1)})
        print(f"{c:>13.2f} {m:>8.3f} {ci:>8.3f} {p:>9.4f} "
              f"{res.get('culture_a', 0):>8.1f} {res.get('culture_b', 0):>8.1f} "
              f"{margin:>8.1f}")
    if args.out:
        with open(args.out, "a") as fh:
            for r in rows:
                r["players"] = args.players
                r["base"] = args.base or "default"
                fh.write(json.dumps(r) + "\n")
    return rows


if __name__ == "__main__":
    main()
