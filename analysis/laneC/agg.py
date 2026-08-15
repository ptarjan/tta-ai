"""Pool the blocks of a Lane C A/B and report the paired statistics.

`experiments.arena.duel` returns `per_game` (challenger's share of the win,
task-ordered) and `per_game_margin` (culture_a - culture_b).  At 2 players
each deal is played twice with the seats swapped, so games 2k and 2k+1 of a
block are the SAME deal: averaging the pair removes the deal, which is what
makes the comparison paired and is why the block-level CI below is narrower
than a naive binomial on the same n.

    python3 analysis/laneC/agg.py /tmp/laneC_ab_main.jsonl
"""
from __future__ import annotations

import json
import math
import sys


def _pairs(seq):
    """Deal-level means: (g, g+1) are the same deal with the seats swapped."""
    out = []
    for i in range(0, len(seq) - 1, 2):
        a, b = seq[i], seq[i + 1]
        if a is None or b is None:
            continue
        out.append((a + b) / 2.0)
    return out


def _stat(vals, null):
    n = len(vals)
    if n < 2:
        return 0.0, 0.0, 0.0
    m = sum(vals) / n
    var = sum((v - m) ** 2 for v in vals) / (n - 1)
    se = math.sqrt(var / n)
    return m, 1.96 * se, ((m - null) / se if se else 0.0)


def main(path):
    blocks = [json.loads(ln) for ln in open(path) if ln.strip()]
    if not blocks:
        print("no blocks")
        return 1
    wins, margins = [], []
    print(f"{'block':>5s} {'n':>5s} {'win rate':>9s} {'margin':>8s}")
    for i, b in enumerate(blocks, 1):
        w = _pairs(b["per_game"])
        g = _pairs(b["per_game_margin"])
        wins += w
        margins += g
        bw, _, _ = _stat(w, b["null"])
        bg, _, _ = _stat(g, 0.0)
        print(f"{i:5d} {len(b['per_game']):5d} {bw * 100:8.1f}% {bg:8.2f}")
    null = blocks[0]["null"]
    mw, cw, zw = _stat(wins, null)
    mg, cg, zg = _stat(margins, 0.0)
    ca = sum(b["culture_a"] for b in blocks) / len(blocks)
    cb = sum(b["culture_b"] for b in blocks) / len(blocks)
    ng = sum(b["games"] for b in blocks)
    err = sum(b["errors"] for b in blocks)
    print(f"\npooled: {ng} games / {len(wins)} deals   (errors {err})")
    print(f"  win rate {mw * 100:.2f}% +/- {cw * 100:.2f}pp "
          f"(null {null * 100:.1f}%, z = {zw:.1f})")
    print(f"  culture margin {mg:+.2f} +/- {cg:.2f} (z = {zg:.1f})")
    print(f"  own culture {ca:.1f} vs {cb:.1f}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1]))
