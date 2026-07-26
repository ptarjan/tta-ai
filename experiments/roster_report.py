"""Render the roster round-robin into the tables docs/BOT_ROSTER.md publishes.

Kept separate from ``roster_match.py`` so the doc can be rebuilt from the
JSONL without replaying a single game.

TWO STATISTICAL FIXES ARE APPLIED HERE
--------------------------------------
1. **Boundary cells.**  ``arena.mean_ci`` is a normal approximation over the
   per-game win shares.  When a bot wins (or loses) *every* game the sample
   variance is exactly zero, so the harness reports "100.0% +/- 0.0%,
   p=1.0000".  That p-value is an artefact of dividing by a zero standard
   error, not a finding of "no difference" -- a 240-0 sweep is the most
   significant result in the table, not the least.  Those cells get a Wilson
   score interval instead, which is well behaved at the boundary.

2. **Elo is a summary, not the evidence.**  A single rating hides
   non-transitivity, which is exactly what a *diverse* pool is supposed to
   have.  Elo is reported, but the per-pairing matrix is the primary output
   and the doc says so.

    python3 -m experiments.roster_report --out docs/BOT_ROSTER.md
"""
from __future__ import annotations

import argparse
import json
import math
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)),
                                ".."))

ORDER = ["culture", "book", "book2", "bookimp", "champion", "infra", "wonder",
         "science", "tempo", "military", "greedy", "random"]

LABELS = {
    "culture": "CultureBot", "infra": "InfraBot", "military": "MilitaryBot",
    "science": "ScienceBot", "tempo": "TempoBot", "wonder": "WonderBot",
    "book": "BookBot v1", "book2": "BookBot v2", "bookimp": "BookImprovedBot",
    "champion": "champion", "greedy": "GreedyBot", "random": "RandomBot",
}


def wilson(p, n, z=1.96):
    """Wilson score interval; stable when p is 0 or 1."""
    if n <= 0:
        return 0.0, 1.0
    d = 1.0 + z * z / n
    centre = (p + z * z / (2 * n)) / d
    half = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / d
    return max(0.0, centre - half), min(1.0, centre + half)


def interval(row):
    """(lo, hi, flag) -- normal CI, or Wilson where the normal one collapses."""
    p, n, half = row["win_rate"], row["games"], row["ci"]
    if half > 0:
        return p - half, p + half, ""
    lo, hi = wilson(p, n)
    return lo, hi, "*"


def significant(row):
    """Does the interval exclude the 1/N null?"""
    lo, hi, _ = interval(row)
    return row["null"] < lo or row["null"] > hi


def load(path):
    rows = [json.loads(l) for l in open(path)]
    # last write wins, so a re-run of a pairing supersedes the earlier one
    dedup = {}
    for r in rows:
        dedup[(r["players"], r["a"], r["b"])] = r
    return list(dedup.values())


def cells(rows, n):
    """(a, b) -> win rate of a vs a table of b, both directions filled in."""
    out = {}
    for r in rows:
        if r["players"] != n:
            continue
        out[(r["a"], r["b"])] = r["win_rate"]
    return out


def names_at(rows, n):
    seen = set()
    for r in rows:
        if r["players"] == n:
            seen.add(r["a"])
            seen.add(r["b"])
    return [e for e in ORDER if e in seen]


def elo(rows, n, iters=3000, lr=8.0):
    """Bradley-Terry ratings on the Elo scale, anchored so the mean is 1500.

    Only a summary: see the module docstring.  At 3p/4p a "win rate" is a
    share of a table, so it is rescaled to a 2-player-equivalent probability
    (share / (share + mean opponent share)) before fitting, otherwise every
    rating is dragged toward the 1/N null.
    """
    ns = names_at(rows, n)
    idx = {e: i for i, e in enumerate(ns)}
    R = [1500.0] * len(ns)
    games = []
    for r in rows:
        if r["players"] != n:
            continue
        share = min(max(r["win_rate"], 1e-6), 1 - 1e-6)
        # share of the winner's seat vs the average defender seat
        opp = (1.0 - share) / (n - 1)
        p = share / (share + opp)
        games.append((idx[r["a"]], idx[r["b"]], p, r["games"]))
    for _ in range(iters):
        grad = [0.0] * len(ns)
        for i, j, p, w in games:
            exp = 1.0 / (1.0 + 10 ** ((R[j] - R[i]) / 400.0))
            grad[i] += w * (p - exp)
            grad[j] -= w * (p - exp)
        for k in range(len(ns)):
            R[k] += lr * grad[k] / max(1, len(games))
        mean = sum(R) / len(R)
        R = [x - mean + 1500.0 for x in R]
    return {e: R[idx[e]] for e in ns}


def matrix(rows, n):
    ns = names_at(rows, n)
    c = cells(rows, n)
    lines = []
    head = [LABELS[e].replace("Bot", "") for e in ns]
    lines.append("| A \\ table of B | " + " | ".join(head) + " | mean |")
    lines.append("|---|" + "---|" * (len(ns) + 1))
    for a in ns:
        vals, out = [], []
        for b in ns:
            if a == b:
                out.append("–")
                continue
            v = c.get((a, b))
            if v is None and (b, a) in c:
                v = 1.0 - c[(b, a)] if n == 2 else None
            if v is None:
                out.append("")
                continue
            vals.append(v)
            out.append(f"{v:.0%}")
        m = f"**{sum(vals) / len(vals):.1%}**" if vals else ""
        lines.append(f"| **{LABELS[a]}** | " + " | ".join(out) + f" | {m} |")
    return "\n".join(lines)


def mean_share(rows, n, e):
    vals = []
    for r in rows:
        if r["players"] != n:
            continue
        if r["a"] == e:
            vals.append(r["win_rate"])
        elif r["b"] == e:
            vals.append((1.0 - r["win_rate"]) / (n - 1) * 1.0
                        if n > 2 else 1.0 - r["win_rate"])
    return sum(vals) / len(vals) if vals else float("nan")


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--match", default="experiments/roster_match.jsonl")
    ap.add_argument("--behaviour", default="experiments/roster_behaviour.jsonl")
    args = ap.parse_args(argv)

    rows = load(args.match)
    for n in (2, 3, 4):
        if not names_at(rows, n):
            continue
        print(f"\n### {n} players (null = {1.0 / n:.1%})\n")
        print(matrix(rows, n))
        print()
        r = elo(rows, n)
        for e, v in sorted(r.items(), key=lambda kv: -kv[1]):
            print(f"  {LABELS[e]:<18s} {v:7.0f}   mean share "
                  f"{mean_share(rows, n, e):.1%}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
