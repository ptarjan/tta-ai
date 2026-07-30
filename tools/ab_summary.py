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

Usage:
    python3 tools/ab_summary.py /tmp/ab_events.jsonl
"""
from __future__ import annotations

import json
import math
import sys


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

    margins, cultures, wins = [], [], []
    per_block = []
    for b in blocks:
        pm = b.get("per_game_margin") or []
        pc = b.get("per_game_culture") or []
        margins += pm
        cultures += pc
        w = b.get("win_rate")
        if w is None and b.get("games"):
            w = b.get("wins", 0) / b["games"]
        wins.append((w, b.get("games", len(pm))))
        mb, _, _ = _stats(pm) if pm else (0.0, 0, 0)
        per_block.append((b.get("seed"), b.get("games"), w, round(mb, 2)))

    tot = sum(g for _, g in wins if g)
    wr = sum((w or 0) * g for w, g in wins if g) / tot if tot else 0.0
    wr_se = math.sqrt(max(wr * (1 - wr), 1e-9) / tot) if tot else 0.0
    mm, mse, mn = _stats(margins)
    cm, cse, _ = _stats(cultures)

    print(f"blocks           {len(blocks)}   games {tot}")
    print(f"win rate         {wr*100:6.2f}%  +/- {1.96*wr_se*100:.2f}pp "
          f"(z = {(wr-0.5)/wr_se:.2f})" if wr_se else "")
    print(f"culture margin   {mm:+6.2f}   +/- {1.96*mse:.2f} "
          f"(z = {mm/mse:.2f}, n = {mn})" if mse else "")
    print(f"own culture      {cm:6.2f}")
    print(f"MDE (80% power)  {2.8*wr_se*100:.2f}pp on the win rate")
    print()
    print(" seed  games  win%   margin")
    for s, g, w, mb in per_block:
        print(f"{str(s):>5} {str(g):>6}  {(w or 0)*100:5.1f}  {mb:+7.2f}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
