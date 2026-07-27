"""Distributions over the per-player rows produced by `tools/bgo_parse.py`
(human corpus) or `tools/bgo_botmatch.py` (our bot), and the A-vs-B comparison
between them.

    python3 tools/bgo_stats.py --tsv /tmp/human.tsv --players 2
    python3 tools/bgo_stats.py --tsv /tmp/human.tsv --players 2 --level Emperor,King
    python3 tools/bgo_stats.py --tsv /tmp/human.tsv --vs /tmp/bot_2p.tsv --players 2

Everything is reported as median [IQR] with a bootstrap 95% CI on the median,
because this repo has been burned by point estimates on small n.  The n on the
bot side is deliberately tiny (tens of games), so the CI is the headline, not
the median: a bot/human gap that does not clear both CIs is not a finding.

Rows are per PLAYER, not per game, and the two seats of one game are not
independent (one player's war is another's defence).  n_games is printed
alongside n_rows for exactly that reason -- read the CI as roughly "n_games
independent units", not n_rows.
"""
from __future__ import annotations

import argparse
import csv
import math
import random
import sys
from collections import Counter

METRICS = [
    ("rounds", "game length (rounds)"),
    ("score", "final culture (score)"),
    ("sci_final", "unspent science at end"),
    ("win_margin", "WINNER's margin over 2nd"),
    ("wars_declared", "wars declared"),
    ("wars_declared_won", "wars declared and won"),
    ("wars_defended", "wars defended against"),
    ("aggressions", "aggressions played"),
    ("colonies", "colonies taken"),
    ("bids", "colony bids made"),
    ("wonders_started", "wonders started"),
    ("wonders_completed", "wonders completed"),
    ("wonder_stages", "wonder stages built"),
    ("gov_changes", "government changes"),
    ("first_gov_round", "round of first government"),
    ("takes", "civil cards taken"),
    ("tier1", "takes at row tier 1 (1 CA)"),
    ("tier2", "takes at row tier 2 (2 CA)"),
    ("tier3", "takes at row tier 3 (3 CA)"),
    ("tier3_pct", "% of takes paid at 3 CA"),
    ("take_ageA", "age A cards taken"),
    ("take_ageI", "age I cards taken"),
    ("take_ageII", "age II cards taken"),
    ("take_ageIII", "age III cards taken"),
    ("take_ageIV", "age IV cards taken"),
    ("leaders_elected", "leaders elected"),
]


def load(path, players=None, levels=None):
    rows = []
    with open(path) as fh:
        for r in csv.DictReader(fh, delimiter="\t"):
            if players and int(r["players"] or 0) != players:
                continue
            if levels and r["level"] not in levels:
                continue
            rows.append(r)
    for r in rows:
        t = num(r.get("tier1")) + num(r.get("tier2")) + num(r.get("tier3"))
        r["tier3_pct"] = (100.0 * num(r.get("tier3")) / t) if t else ""
        # the winner's margin only means anything on the winner's row; on a
        # loser's row the same column is a (negative) deficit, and averaging
        # the two together is exactly zero by construction.
        r["win_margin"] = r["margin_vs_next"] if r.get("rank") == "1" else ""
    return rows


#: per-game totals, summed over seats.  "wars per game" in this repo's
#: champion measurements is a whole-table number, so the human side has to be
#: aggregated the same way before the two can be put side by side.
GAME_SUMS = ("wars_declared", "aggressions", "colonies", "wonders_completed")


def per_game(rows):
    """Collapse per-player rows to one row per game with summed counters."""
    games = {}
    for r in rows:
        g = games.setdefault(r["game_id"], {"game_id": r["game_id"],
                                            "players": r["players"],
                                            "level": r.get("level", "")})
        for k in GAME_SUMS:
            g[k] = g.get(k, 0) + (num(r.get(k)) or 0)
        g["rounds"] = num(r.get("rounds")) or 0
    return list(games.values())


def num(v):
    if v is None or v == "":
        return None
    try:
        f = float(v)
    except ValueError:
        return None
    return f


def col(rows, key):
    return [x for x in (num(r.get(key)) for r in rows) if x is not None]


def quantile(xs, q):
    if not xs:
        return float("nan")
    s = sorted(xs)
    i = q * (len(s) - 1)
    lo = int(math.floor(i))
    hi = min(lo + 1, len(s) - 1)
    return s[lo] + (s[hi] - s[lo]) * (i - lo)


def boot_median_ci(xs, iters=2000, seed=7):
    if len(xs) < 3:
        return (float("nan"), float("nan"))
    rng = random.Random(seed)
    n = len(xs)
    meds = []
    for _ in range(iters):
        samp = [xs[rng.randrange(n)] for _ in range(n)]
        meds.append(quantile(samp, 0.5))
    meds.sort()
    return (meds[int(0.025 * iters)], meds[int(0.975 * iters)])


def boot_mean_ci(xs, iters=1000, seed=7):
    if len(xs) < 3:
        return (float("nan"), float("nan"))
    rng = random.Random(seed)
    n = len(xs)
    ms = []
    for _ in range(iters):
        ms.append(sum(xs[rng.randrange(n)] for _ in range(n)) / n)
    ms.sort()
    return (ms[int(0.025 * iters)], ms[int(0.975 * iters)])


def cluster_mean_ci(rows, key, iters=1000, seed=7):
    """Bootstrap the mean of `key`, resampling GAMES not rows.

    The two seats of one game are not independent -- one player's war is the
    other's defence, and both share the same card row and event deck -- so a
    row-level bootstrap understates the interval.  Resampling whole games
    (all seats together, with replacement) is the standard cluster bootstrap
    and is what every CI printed here uses.
    """
    by_game = {}
    for r in rows:
        v = num(r.get(key))
        if v is not None:
            by_game.setdefault(r["game_id"], []).append(v)
    keys = list(by_game)
    if len(keys) < 3:
        return (float("nan"), float("nan"))
    rng = random.Random(seed)
    k = len(keys)
    ms = []
    for _ in range(iters):
        tot = 0.0
        cnt = 0
        for _ in range(k):
            vs = by_game[keys[rng.randrange(k)]]
            tot += sum(vs)
            cnt += len(vs)
        ms.append(tot / cnt)
    ms.sort()
    return (ms[int(0.025 * iters)], ms[int(0.975 * iters)])


def fmt(x):
    if x != x:
        return "  n/a"
    return ("%6.2f" % x).rstrip()


def describe(rows, label):
    ngames = len(set(r["game_id"] for r in rows))
    print("== %s  n_rows=%d  n_games=%d" % (label, len(rows), ngames))
    print("%-30s %8s %8s %8s %8s   %-18s %s"
          % ("metric", "median", "q25", "q75", "mean", "mean 95% CI", "n"))
    for key, name in METRICS:
        xs = col(rows, key)
        if not xs:
            print("%-30s %8s (no data)" % (name, "-"))
            continue
        lo, hi = cluster_mean_ci(rows, key)
        print("%-30s %8.2f %8.2f %8.2f %8.2f   [%6.2f, %6.2f]  %d"
              % (name, quantile(xs, .5), quantile(xs, .25), quantile(xs, .75),
                 sum(xs) / len(xs), lo, hi, len(xs)))


def compare(a_rows, b_rows, a_label, b_label):
    print("== %s (A) vs %s (B)" % (a_label, b_label))
    print("   A n_rows=%d n_games=%d   B n_rows=%d n_games=%d"
          % (len(a_rows), len(set(r["game_id"] for r in a_rows)),
             len(b_rows), len(set(r["game_id"] for r in b_rows))))
    print("%-30s %17s %17s  %s"
          % ("metric", "A mean [95% CI]", "B mean [95% CI]", "verdict"))
    for key, name in METRICS:
        xa, xb = col(a_rows, key), col(b_rows, key)
        if not xa or not xb:
            continue
        ma, mb = sum(xa) / len(xa), sum(xb) / len(xb)
        la, ha = cluster_mean_ci(a_rows, key)
        lb, hb = cluster_mean_ci(b_rows, key)
        if hb < la:
            verdict = "B LOWER"
        elif lb > ha:
            verdict = "B HIGHER"
        else:
            verdict = "overlap"
        print("%-30s %6.2f [%5.2f,%5.2f] %6.2f [%5.2f,%5.2f]  %s"
              % (name, ma, la, ha, mb, lb, hb, verdict))


def categorical(rows, key, label, top=8):
    c = Counter(r.get(key, "") or "(none)" for r in rows)
    tot = sum(c.values())
    print("-- %s (%s, n=%d)" % (label, key, tot))
    for v, n in c.most_common(top):
        print("     %-28s %5d  %5.1f%%" % (v, n, 100.0 * n / tot))


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--tsv", required=True)
    ap.add_argument("--vs", default="")
    ap.add_argument("--players", type=int, default=0)
    ap.add_argument("--level", default="")
    ap.add_argument("--cat", action="store_true",
                    help="also print categorical breakdowns")
    a = ap.parse_args(argv)
    levels = set(a.level.split(",")) if a.level else None
    rows = load(a.tsv, a.players or None, levels)
    lab = "%s players=%s level=%s" % (a.tsv, a.players or "all",
                                      a.level or "all")
    if a.vs:
        brows = load(a.vs, a.players or None, None)
        compare(rows, brows, lab, a.vs)
        print()
        print("-- per-GAME totals (summed over seats)")
        ga, gb = per_game(rows), per_game(brows)
        print("%-24s %17s %17s  %s"
              % ("metric/game", "A mean [95% CI]", "B mean [95% CI]", "verdict"))
        for k in GAME_SUMS:
            xa, xb = col(ga, k), col(gb, k)
            la, ha = cluster_mean_ci(ga, k)
            lb, hb = cluster_mean_ci(gb, k)
            v = "B LOWER" if hb < la else ("B HIGHER" if lb > ha else "overlap")
            print("%-24s %6.2f [%5.2f,%5.2f] %6.2f [%5.2f,%5.2f]  %s"
                  % (k, sum(xa) / len(xa), la, ha,
                     sum(xb) / len(xb), lb, hb, v))
    else:
        describe(rows, lab)
        ga = per_game(rows)
        print("-- per-GAME totals (summed over seats), n_games=%d" % len(ga))
        for k in GAME_SUMS:
            xa = col(ga, k)
            lo, hi = cluster_mean_ci(ga, k)
            print("%-24s median %5.2f  mean %5.2f [%5.2f, %5.2f]"
                  % (k, quantile(xa, .5), sum(xa) / len(xa), lo, hi))
    if a.cat:
        categorical(rows, "first_gov", "first government")
        categorical(rows, "final_age", "final age")
        categorical(rows, "gov_path", "government path")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
