#!/usr/bin/env python3
"""Aggregate `agreement`'s per-decision TSV output into the summary numbers
`docs/AGREEMENT.md` reports.

Stdlib only (matches this project's own "no new dependency for a one-off
analysis" posture -- the Rust crate's zero-dependency rule doesn't strictly
bind a throwaway Python script, but there is no reason to add one here
either). Reads `rust/src/bin/agreement.rs`'s documented 15-column TSV
schema; see that binary's own module doc for the exact column meanings.

Usage:
    python3 tools/aggregate_agreement.py agreement.tsv

Regeneration of the input TSV this consumes:
    cd rust
    tar -xzf ../sources/bgo/journals.tar.gz -C /tmp/bgo-journals   # once
    IDS=$(for n in 2 3 4; do awk -F'\t' -v n=$n \
        'NR>1 && $3==n{print $1}' ../sources/bgo/index.tsv | head -50; done)
    cargo run --profile difftest --bin agreement -- \
        ../sources/bgo/index.tsv /tmp/bgo-journals/journals ../experiments \
        $IDS > ../agreement.tsv 2> ../agreement.stderr
"""
import math
import sys
from collections import Counter, defaultdict

COLUMNS = [
    "game_id", "tier", "players", "age", "round", "lineno", "category",
    "agreed", "human_rank", "legal_count", "discard_tainted", "human_move",
    "bot_top_move", "human_score", "bot_top_score",
]


def load(path):
    rows = []
    with open(path) as f:
        for line in f:
            fields = line.rstrip("\n").split("\t")
            if len(fields) != len(COLUMNS):
                raise ValueError(f"expected {len(COLUMNS)} columns, got {len(fields)}: {fields!r}")
            row = dict(zip(COLUMNS, fields))
            row["agreed"] = row["agreed"] == "true"
            row["discard_tainted"] = row["discard_tainted"] == "true"
            row["players"] = int(row["players"])
            row["human_rank"] = None if row["human_rank"] == "uncounted" else int(row["human_rank"])
            rows.append(row)
    return rows


def wilson_ci(k, n, z=1.96):
    """95% Wilson score interval for a binomial proportion -- more honest
    than a normal approximation at the n's some breakdowns below have
    (a handful of category x player-count cells are well under 100)."""
    if n == 0:
        return (float("nan"), float("nan"))
    p = k / n
    denom = 1 + z * z / n
    centre = p + z * z / (2 * n)
    half = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n))
    return ((centre - half) / denom, (centre + half) / denom)


def rate_line(label, rows):
    n = len(rows)
    k = sum(1 for r in rows if r["agreed"])
    if n == 0:
        return f"{label}: n=0"
    lo, hi = wilson_ci(k, n)
    return f"{label}: {k}/{n} = {k/n:.1%}  (95% CI {lo:.1%}-{hi:.1%})"


def group_by(rows, key):
    out = defaultdict(list)
    for r in rows:
        out[key(r)].append(r)
    return out


def print_breakdown(title, rows, key, order=None):
    print(f"\n### {title}\n")
    groups = group_by(rows, key)
    keys = order if order else sorted(groups.keys())
    for k in keys:
        if k in groups:
            print("- " + rate_line(str(k), groups[k]))


def rank_distribution(rows):
    disagreements = [r for r in rows if not r["agreed"]]
    c = Counter(r["human_rank"] for r in disagreements)
    n = len(disagreements)
    print(f"n={n} disagreements")
    for rank in sorted(c, key=lambda x: (x is None, x)):
        label = "uncounted" if rank is None else f"rank {rank}"
        print(f"  {label}: {c[rank]} ({c[rank]/n:.1%})" if n else f"  {label}: {c[rank]}")


def main():
    path = sys.argv[1] if len(sys.argv) > 1 else "agreement.tsv"
    rows = load(path)

    print("=" * 70)
    print(f"TOTAL decision points: n={len(rows)} across {len(set(r['game_id'] for r in rows))} games")
    print("=" * 70)

    print("\n## Overall top-1 agreement\n")
    print("- " + rate_line("all decisions", rows))
    clean = [r for r in rows if not r["discard_tainted"]]
    tainted = [r for r in rows if r["discard_tainted"]]
    print("- " + rate_line("excluding discard_tainted", clean))
    print("- " + rate_line("discard_tainted only", tainted))

    print_breakdown("By player count", rows, lambda r: r["players"], order=[2, 3, 4])
    print_breakdown("By game age", rows, lambda r: r["age"], order=["A", "I", "II", "III", "IV"])
    print_breakdown("By move category", rows, lambda r: r["category"])
    print_breakdown("By BGO skill tier", rows, lambda r: r["tier"], order=["Prince", "King", "Warlord", "Emperor"])

    print("\n## discard_tainted, by player count (included vs excluded)\n")
    for p in [2, 3, 4]:
        sub = [r for r in rows if r["players"] == p]
        sub_clean = [r for r in sub if not r["discard_tainted"]]
        print(f"- {p}p: " + rate_line("all", sub) + "  |  " + rate_line("clean", sub_clean))

    print("\n## Human-rank distribution among ALL disagreements\n")
    rank_distribution(rows)

    print("\n## Human-rank distribution, top categories by disagreement volume\n")
    cat_groups = group_by(rows, lambda r: r["category"])
    by_disagree_volume = sorted(
        cat_groups.items(), key=lambda kv: sum(1 for r in kv[1] if not r["agreed"]), reverse=True
    )
    for cat, sub in by_disagree_volume[:5]:
        n_dis = sum(1 for r in sub if not r["agreed"])
        print(f"\n### {cat} (n={len(sub)}, {n_dis} disagreements)")
        rank_distribution(sub)

    print("\n## Category volume table (n, agreement rate)\n")
    for cat, sub in sorted(cat_groups.items(), key=lambda kv: len(kv[1]), reverse=True):
        print("- " + rate_line(cat, sub))

    print("\n## discard_tainted share overall and by player count\n")
    print(f"- overall: {len(tainted)}/{len(rows)} = {len(tainted)/len(rows):.1%}")
    for p in [2, 3, 4]:
        sub = [r for r in rows if r["players"] == p]
        sub_t = [r for r in sub if r["discard_tainted"]]
        print(f"- {p}p: {len(sub_t)}/{len(sub)} = {len(sub_t)/len(sub):.1%}" if sub else f"- {p}p: n=0")


if __name__ == "__main__":
    main()
