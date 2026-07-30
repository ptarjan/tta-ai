"""Pool `experiments.evaluate --out` JSONL blocks into one result with CIs.

`evaluate` prints a per-block win rate and a pair of mean cultures.  Two things
that matters for reading a small experiment are not in that line:

* the **culture margin** with its own confidence interval.  `arena.duel`
  returns `per_game_margin` -- A's final culture minus the mean of the
  defenders', on EVERY game -- which is a dense signal where win share is one
  bit per game.  `docs/CARD_BLINDNESS.md` section 5 got z = 17.8 on the margin
  against z = 14.4 on the win rate from the same games for this reason.
* the **per-block spread**, so a result carried by one lucky block is visible
  rather than hidden inside a pooled mean.

Intervals are clustered on the **deal**, not the game.  `arena.duel` plays
every deal `players` times with the seats swapped (`seat = g % P`,
`seed = seed0 + g // P`), so games come in mirrored groups and the
independent-samples formula does not apply to them.  On this project's data
that correction is usually a *tightening*, not a widening -- see
`experiments/paired_stats.py` for why, and why it is not a blanket sqrt(2).
The legacy per-game number is printed underneath each corrected one so this
can still be reconciled against reports written before 2026-07-30.

Usage:
    python3 tools/ab_summary.py /tmp/ab_events.jsonl
"""
from __future__ import annotations

import json
import math
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from experiments import paired_stats as PS  # noqa: E402


def _stats(xs):
    n = len(xs)
    if n < 2:
        return (float(xs[0]) if xs else 0.0), 0.0, n
    m = sum(xs) / n
    var = sum((x - m) ** 2 for x in xs) / (n - 1)
    return m, math.sqrt(var / n), n


def main():
    paths = sys.argv[1:]
    if not paths:
        print(__doc__)
        return 1
    blocks = []
    for p in paths:
        with open(p) as fh:
            for line in fh:
                line = line.strip()
                if line:
                    blocks.append(json.loads(line))
    if not blocks:
        print("no blocks")
        return 1

    players = blocks[0].get("players", 2)
    if any(b.get("players", players) != players for b in blocks):
        print("refusing to pool blocks with different player counts")
        return 1

    margins, cultures, wins = [], [], []
    share_blocks, margin_blocks = [], []
    per_block = []
    for b in blocks:
        pm = b.get("per_game_margin") or []
        pc = b.get("per_game_culture") or []
        pg = b.get("per_game")
        margins += pm
        cultures += pc
        if pg:
            share_blocks.append(pg)
        if pm:
            margin_blocks.append(pm)
        w = b.get("win_rate")
        if w is None and b.get("games"):
            w = b.get("wins", 0) / b["games"]
        wins.append((w, b.get("games", len(pm))))
        mb, _, _ = _stats(pm) if pm else (0.0, 0, 0)
        per_block.append((b.get("seed"), b.get("games"), w, round(mb, 2)))

    tot = sum(g for _, g in wins if g)
    wr = sum((w or 0) * g for w, g in wins if g) / tot if tot else 0.0
    # The legacy interval, kept only so older reports can be reconciled.
    wr_se = math.sqrt(max(wr * (1 - wr), 1e-9) / tot) if tot else 0.0
    mm, mse, mn = _stats(margins)
    cm, cse, _ = _stats(cultures)
    null = 1.0 / players

    print(f"blocks           {len(blocks)}   games {tot}   players {players}")
    if share_blocks:
        est = PS.pooled(share_blocks, players)
        print(f"win rate         {est.mean*100:6.2f}%  "
              f"+/- {est.half*100:.2f}pp  (z = {est.z_against(null):.2f}, "
              f"p = {est.p_against(null):.3g})")
        print(f"                 [{est.unit}-clustered, K={est.n_clusters}, "
              f"rho={est.rho:+.3f}, vs null {null*100:.1f}%]")
        print(f"                 legacy per-game: "
              f"+/- {1.96*wr_se*100:.2f}pp (z = {(wr-null)/wr_se:.2f})"
              if wr_se else "")
        if est.het_df:
            flag = " *** OVER-DISPERSED" if est.escalated else ""
            print(f"                 block agreement: chi2 = "
                  f"{est.het_chi2:.2f} on {est.het_df} df{flag}")
        for n in est.notes:
            print(f"                 note: {n}")
        eff_se = est.se
    else:
        print(f"win rate         {wr*100:6.2f}%  +/- {1.96*wr_se*100:.2f}pp "
              f"(NO per_game array -- legacy per-game interval only)")
        eff_se = wr_se

    if margin_blocks:
        me = PS.pooled(margin_blocks, players)
        print(f"culture margin   {me.mean:+6.2f}   +/- {me.half:.2f}  "
              f"(z = {me.z_against(0.0):.2f}, {me.unit}s = {me.n_clusters})")
        print(f"                 legacy per-game: +/- {1.96*mse:.2f} "
              f"(z = {mm/mse:.2f}, n = {mn})" if mse else "")
    print(f"own culture      {cm:6.2f}")
    print(f"MDE (80% power)  {2.8*eff_se*100:.2f}pp on the win rate")
    print()
    print(" seed  games  win%   margin")
    for s, g, w, mb in per_block:
        print(f"{str(s):>5} {str(g):>6}  {(w or 0)*100:5.1f}  {mb:+7.2f}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
