"""Paired analysis of exp_quiesce/ab.jsonl.

`arena.duel` lays its tasks out as `seat = g % players`, `seed = seed0 +
g // players`, so games `[k*P, (k+1)*P)` are the SAME game seed played once
with the challenger in each seat.  Call that a *seed group*.

The control arm is the 1-ply bot challenging a table of itself, i.e. pure
self-play with deterministic bots: the game does not depend on which seat is
labelled "the challenger", so the challenger wins exactly one of the P
rotations and every control seed group scores exactly 1/P with zero variance.
That makes the seed group the natural paired unit -- the difference of group
means against the control is just `group_mean - 1/P`, and its spread across
groups is the only noise there is.  Reporting the CI of the GROUP means rather
than of the individual games is what removes the seat-assignment variance,
which is pure nuisance here.
"""
from __future__ import annotations

import json
import math
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))


def groups(per_game, players):
    out = []
    for k in range(0, len(per_game) - players + 1, players):
        chunk = per_game[k:k + players]
        ok = [x for x in chunk if x is not None]
        if len(ok) == players:
            out.append(sum(ok) / players)
    return out


def mean_ci(xs, z=1.96):
    n = len(xs)
    if n < 2:
        return (xs[0] if xs else 0.0), 0.0
    m = sum(xs) / n
    var = sum((x - m) ** 2 for x in xs) / (n - 1)
    return m, z * math.sqrt(var / n)


def paired(a, b):
    """Mean and CI of a-b, elementwise."""
    d = [x - y for x, y in zip(a, b)]
    return mean_ci(d) + (len(d),)


def main():
    recs = {}
    with open(os.path.join(HERE, "ab.jsonl")) as fh:
        for line in fh:
            r = json.loads(line)
            recs[r["label"]] = r

    rows = []
    for p in (2, 3, 4):
        ctrl = recs.get(f"ctrl_{p}p")
        if ctrl:
            cg = groups(ctrl["per_game"], p)
            cm = groups(ctrl["per_game_margin"], p)
            assert all(abs(x - 1.0 / p) < 1e-12 for x in cg), "control drifted"
            assert all(abs(x) < 1e-12 for x in cm), "control margin drifted"
        else:
            # provably 1/P and 0.0 in every group -- see the module docstring;
            # verified exactly at 2p (400 groups) and 3p (267 groups).
            ctrl = {"secs": None}
            cg = [1.0 / p] * 10000
            cm = [0.0] * 10000
        for lvl in (1, 2, "nw"):
            r = recs.get(f"q{lvl}_{p}p")
            if not r:
                continue
            qg = groups(r["per_game"], p)
            qm = groups(r["per_game_margin"], p)
            n = min(len(qg), len(cg))
            wm, wci = mean_ci(qg[:n])
            dm, dci, _ = paired(qg[:n], cg[:n])
            mm, mci, _ = paired(qm[:n], cm[:n])
            rows.append({
                "players": p, "levels": lvl,
                "games": r["games"], "seed_groups": n,
                "win_rate": wm, "win_ci": wci,
                "null": 1.0 / p,
                "d_win": dm, "d_win_ci": dci,
                "d_margin": mm, "d_margin_ci": mci,
                "culture_a": r["culture_a"], "culture_b": r["culture_b"],
                "ctrl_win": mean_ci(cg[:n])[0],
                "errors": r["errors"], "secs": r.get("secs"),
                "ctrl_secs": ctrl.get("secs"),
            })

    print(f"{'P':>2} {'lvl':>3} {'n games':>7} {'grp':>4} "
          f"{'win rate':>16} {'null':>6} {'Δ win (paired)':>18} "
          f"{'Δ culture margin':>20}  err")
    for r in rows:
        print(f"{r['players']:>2} {str(r['levels']):>3} {r['games']:>7} "
              f"{r['seed_groups']:>4} "
              f"{r['win_rate']:>8.1%} +/-{r['win_ci']:>5.1%} "
              f"{r['null']:>6.1%} "
              f"{r['d_win']:>+9.1%} +/-{r['d_win_ci']:>5.1%} "
              f"{r['d_margin']:>+11.2f} +/-{r['d_margin_ci']:>6.2f}  "
              f"{r['errors']}")
    # LEVELS=2 vs LEVELS=1 directly: same seeds, so this is paired too and is
    # a much tighter test than comparing each arm's interval to the null.
    print()
    for p, la, lb in [(x, a, b) for x in (2, 3, 4)
                      for a, b in (("q2", "q1"), ("qnw", "q1"))]:
        a, b = recs.get(f"{la}_{p}p"), recs.get(f"{lb}_{p}p")
        if not (a and b):
            continue
        ag, bg = groups(a["per_game"], p), groups(b["per_game"], p)
        am, bm = groups(a["per_game_margin"], p), groups(b["per_game_margin"], p)
        n = min(len(ag), len(bg))
        d, dci, _ = paired(ag[:n], bg[:n])
        m, mci, _ = paired(am[:n], bm[:n])
        print(f"{p}p  {la} - {lb} (paired, {n} seed groups): "
              f"win {d:+.1%} +/-{dci:.1%}   culture {m:+.2f} +/-{mci:.2f}")
    with open(os.path.join(HERE, "ab_summary.json"), "w") as fh:
        json.dump(rows, fh, indent=1)
    return 0


if __name__ == "__main__":
    sys.exit(main())
