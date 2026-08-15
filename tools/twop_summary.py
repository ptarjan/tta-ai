#!/usr/bin/env python3
"""One row per matchup: the headline behavioural contrasts, side by side.

    python3 tools/twop_summary.py /tmp/twop_main [/tmp/twop_ctl ...]

Every cell is a per-game mean over that matchup's n games; `+/-` is the
standard error of that mean.  A "champ-opp" cell is a PAIRED difference (the
two players are measured on the same games), so its SE is the paired one.
"""
from __future__ import annotations

import argparse
import glob
import json
import math
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
from tools.twop_report import (mean_se, fmt, paired, ledger_sum,  # noqa: E402
                               turn_stat, eoa)


def row(d):
    A, B = d["recs_a"], d["recs_b"]
    n = len(A)

    def both(fn):
        return [fn(r) for r in A], [fn(r) for r in B]

    war_a, war_b = both(lambda r: sum(1 for _, k, _ in r["attacks_made"]
                                      if k == "war"))
    agg_a, agg_b = both(lambda r: sum(1 for _, k, _ in r["attacks_made"]
                                      if k == "aggression"))
    conf_a, conf_b = both(lambda r: ledger_sum(r, "war:")
                          + ledger_sum(r, "aggression:"))
    rate_a, rate_b = both(lambda r: ledger_sum(r, "rate:"))
    ev_a, ev_b = both(lambda r: ledger_sum(r, "event:")
                      + ledger_sum(r, "endgame:")
                      + ledger_sum(r, "military:prepare_event"))
    won_a, won_b = both(lambda r: len(r["wonders_done"]))
    ca_a, ca_b = both(lambda r: turn_stat(r, "ca_left"))
    st3 = [(eoa(r, "strength", 3), eoa(rb, "strength", 3))
           for r, rb in zip(A, B)]
    st3 = [(x, y) for x, y in st3 if x is not None and y is not None]
    return {
        "matchup": f"{d['a']} vs {d['b']}",
        "n": n,
        "win": fmt([r["win"] for r in A], 3),
        "margin": fmt([r["margin"] for r in A], 1),
        "score": f"{mean_se([r['culture'] for r in A])[0]:.0f}/"
                 f"{mean_se([r['culture'] for r in B])[0]:.0f}",
        "wars": fmt(war_a, 2),
        "aggs": fmt(agg_a, 2),
        "opp_wars": fmt(war_b, 2),
        "conflict_swing": paired(conf_a, conf_b, 1),
        "rate_swing": paired(rate_a, rate_b, 1),
        "event_swing": paired(ev_a, ev_b, 1),
        "wonders": f"{mean_se(won_a)[0]:.2f}/{mean_se(won_b)[0]:.2f}",
        "ca_left": f"{mean_se(ca_a)[0]:.2f}/{mean_se(ca_b)[0]:.2f}",
        "str_a3": (f"{mean_se([x for x, _ in st3])[0]:.1f}/"
                   f"{mean_se([y for _, y in st3])[0]:.1f}" if st3 else "-"),
    }


COLS = [("matchup", 30), ("n", 5), ("win", 14), ("margin", 13),
        ("score", 9), ("wars", 11), ("opp_wars", 11), ("aggs", 11),
        ("conflict_swing", 13), ("rate_swing", 12), ("event_swing", 12),
        ("str_a3", 11), ("wonders", 10), ("ca_left", 10)]


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("dirs", nargs="+")
    a = ap.parse_args(argv)
    rows = []
    for dd in a.dirs:
        for f in sorted(glob.glob(os.path.join(dd, "*.json"))):
            with open(f) as fh:
                rows.append(row(json.load(fh)))
    print("  ".join(f"{c:<{w}}" for c, w in COLS))
    print("-" * (sum(w + 2 for _, w in COLS)))
    for r in rows:
        print("  ".join(f"{str(r[c]):<{w}}" for c, w in COLS))
    print("\nlegend: win/margin are the A player's, null 0.5 and 0.0.")
    print("        *_swing = paired (A - B) culture from that source per game.")
    print("        str_a3 = strength at the last turn of age III, A/B.")
    print("        wonders = completed per game A/B; ca_left = unspent civil")
    print("        actions per own turn A/B.")


if __name__ == "__main__":
    main()
