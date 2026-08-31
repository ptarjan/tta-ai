#!/usr/bin/env python3
"""Join the R2 feature screen with the within-decision discrimination screen.

    python3 analysis/feature_discrimination.py \
        --disc /tmp/featdisc_2p_400.tsv \
        --families analysis/feature_screen_families_2026-08-31.txt

The two screens answer different questions and neither answers the other's.
``feature_screen.py`` ranks a candidate column by held-out R2 gain over phi
when predicting the game's outcome -- variation ACROSS DECISION POINTS. The
leaf eval is ``dot(w, phi(candidate))``, so what actually moves an argmax is
variation ACROSS THE LEGAL MOVES AT ONE DECISION POINT, which ``featdisc``
measures. This script puts both on one row per column.

It reads the families report's own ranked table rather than re-deriving any
R2 number, so nothing here can disagree with the file it cites.
"""

import argparse
import re
import sys

# `rank  FAMILY  name  +gain  Nx  spanned  games  folds` in the families
# report's section 4. The two `---` marker rows (the positive control and the
# floor) do not match, which is the point.
RANK_ROW = re.compile(
    r"^\s*(\d+)\s+(REL|GRAN|HAND)\s+(\S+)\s+([+-][\d.]+)\s+(\d+)x\s+([\d.]+)\s+(\d+)\s+(\S.*?)\s*$"
)


def read_r2_table(path):
    """(name -> {family, gain, spanned, games, folds}) from the families report."""
    out = {}
    with open(path) as fh:
        for line in fh:
            m = RANK_ROW.match(line)
            if m:
                out[m.group(3)] = {
                    "rank": int(m.group(1)),
                    "family": m.group(2),
                    "gain": float(m.group(4)),
                    "spanned": float(m.group(6)),
                    "games": int(m.group(7)),
                    "folds": m.group(8),
                }
    return out


def read_disc(path):
    """(name -> row) plus the run's header line, from a featdisc TSV."""
    header = ""
    rows = {}
    cols = None
    with open(path) as fh:
        for line in fh:
            if line.startswith("#"):
                header = line[1:].strip()
                continue
            parts = line.rstrip("\n").split("\t")
            if cols is None:
                cols = parts
                continue
            r = dict(zip(cols, parts))
            for k in (
                "const_frac",
                "allzero_frac",
                "mean_spread",
                "max_spread",
                "sd_chosen",
                "spread_ratio",
                "mean_distinct",
            ):
                r[k] = float(r[k])
            r["n_dec"] = int(r["n_dec"])
            r["median_distinct"] = int(r["median_distinct"])
            r["p90_distinct"] = int(r["p90_distinct"])
            rows[r["column"]] = r
    return header, rows


def fmt(rank, name, r2, disc, flag):
    return (
        f"{rank:5d}  {r2['family']:5s} {name:34s} {r2['gain']:+.6f} {r2['rank']:5d} "
        f"{disc['spread_ratio']:8.4f} {disc['const_frac']:7.4f} "
        f"{disc['median_distinct']:4d} {disc['mean_distinct']:6.2f} "
        f"{disc['p90_distinct']:4d} {disc['mean_spread']:9.4f}  {flag}"
    )


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--disc", required=True, help="featdisc TSV")
    ap.add_argument("--families", required=True, help="feature_screen families report")
    ap.add_argument(
        "--r2-yardstick",
        type=float,
        default=0.003732,
        help="R2 gain of one live decision-relevant feature (CultureRate)",
    )
    ap.add_argument(
        "--spread-yardstick-column",
        default="CultureRate",
        help="phi column whose spread ratio sets the low-spread line",
    )
    ap.add_argument(
        "--spread-fraction",
        type=float,
        default=0.25,
        help="a column is LOW spread below this fraction of the yardstick's ratio",
    )
    args = ap.parse_args(argv)

    r2 = read_r2_table(args.families)
    header, disc = read_disc(args.disc)
    if not r2:
        sys.exit(f"no ranked rows parsed from {args.families}")

    yard = disc.get(args.spread_yardstick_column)
    if yard is None:
        sys.exit(f"{args.spread_yardstick_column} is not a column in {args.disc}")
    low_line = args.spread_fraction * yard["spread_ratio"]

    # Wrapped: the run line is 185 characters and this file is read in a
    # terminal.
    cut = header.index("decisions=")
    print(f"# {header[:cut].rstrip()}")
    print(f"#   {header[cut:]}")
    print(f"# r2 yardstick {args.r2_yardstick:+.6f}   spread yardstick "
          f"{args.spread_yardstick_column} {yard['spread_ratio']:.4f}   "
          f"low-spread line {low_line:.4f}")
    print(f"{'rank':>5s}  {'fam':5s} {'column':34s} {'r2gain':>9s} {'r2rk':>5s} "
          f"{'spread':>8s} {'const':>7s} {'dmed':>4s} {'dmean':>6s} "
          f"{'d90':>4s} {'rawspr':>9s}  flag")

    missing = [n for n in r2 if n not in disc]
    ranked = sorted(
        (kv for kv in r2.items() if kv[0] in disc),
        key=lambda kv: -disc[kv[0]]["spread_ratio"],
    )
    for i, (name, row) in enumerate(ranked, start=1):
        d = disc[name]
        low = d["spread_ratio"] < low_line
        if row["gain"] >= args.r2_yardstick and low:
            flag = "HIGH-R2/LOW-SPREAD"
        elif d["const_frac"] >= 0.999:
            flag = "CANNOT-DECIDE"
        elif low:
            flag = "low spread"
        else:
            flag = ""
        print(fmt(i, name, row, d, flag))
    if missing:
        print(f"# NOT IN THE DISCRIMINATION RUN: {', '.join(sorted(missing))}")


if __name__ == "__main__":
    main()
