"""Pool the SUMMARY lines of several parallel neural_eval.py shards into one.

A beam-vs-beam gate costs ~18 cpu-s per game, so n=200 in a single process is
an hour.  The loop fans it out over disjoint `--seed0` ranges instead; each
shard is seat-balanced on its own (neural_eval alternates seats within a shard),
so pooling the *point estimate* is a plain n-weighted mean.

Pooling the *interval* is not, and this file got it wrong until 2026-07-30.
Seat balance makes a shard's mean unbiased; it does not make its games
independent, and it says nothing at all about whether the shards agree with
each other.  The old `1.96*sqrt(p(1-p)/n)` over pooled games is blind to both.
`ci_cluster=` is the shard-clustered replacement and `chi2=`/`overdispersed=`
report whether the shards were consistent enough for any pooled interval to
mean anything.  See experiments/paired_stats.py and docs/CARD_BLINDNESS.md.

Prints one line in exactly the format neural_eval.py emits, so the loop's
parsing is identical whether the gate ran in one process or twelve.
"""
from __future__ import annotations

import math
import os
import re
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from experiments import paired_stats  # noqa: E402  (needs the path insert)


def parse(path):
    out = None
    try:
        with open(path) as f:
            for line in f:
                if line.startswith("SUMMARY"):
                    d = dict(re.findall(r"(\w+)=(-?[\d.]+)", line))
                    out = {k: float(v) for k, v in d.items()}
    except OSError:
        return None
    return out


def main():
    rows = [r for r in (parse(p) for p in sys.argv[1:]) if r and r.get("n")]
    if not rows:
        # NO SHARDS IS NOT A SCORE OF ZERO.  This used to print
        # `win=0.0000 ci=1.0000` and exit 0, and experiments/neural_search_loop.sh
        # wrote that 0.0000 into curve.tsv as an observation -- see row 4 of the
        # desktop's loop2/curve.tsv, which records a reference match that never
        # ran as a 0-72 defeat.  A number that parses is the whole problem, so
        # the win rate and CI are emitted as `NA`: any caller scraping them with
        # a numeric pattern now gets nothing instead of a plausible lie.  The
        # n/errs/shards counters stay numeric because 0 is the true count and
        # callers test them.  Exit 3 so a caller that checks status sees it too.
        print("SUMMARY win=NA ci=NA ci_cluster=NA chi2=NA df=0 "
              "overdispersed=0 neural=NA opp=NA margin=NA "
              "n=0 errs=0 shards=0")
        return 3
    n = sum(r["n"] for r in rows)
    wm = sum(r["win"] * r["n"] for r in rows) / n
    cam = sum(r["neural"] * r["n"] for r in rows) / n
    cbm = sum(r["opp"] * r["n"] for r in rows) / n
    mm = sum(r["margin"] * r["n"] for r in rows) / n
    errs = sum(r.get("errs", 0) for r in rows)
    # LEGACY half-width: the independent-samples binomial formula over pooled
    # GAMES.  The claim in the docstring above -- that it "never optimistic"
    # -- is false, twice over:
    #
    #   1. The games are not independent.  neural_eval deals `seat = g % n`,
    #      `seed = seed0 + g // n`, so they come in seat-swapped pairs.
    #   2. More importantly here, it cannot see the shards at all.  Six shards
    #      whose spread is twice what binomial noise allows pool to exactly the
    #      same number as six shards that agree perfectly.
    #
    # Measured on the loop's own anchor (loop2/anchor_seed_{0..5}.log,
    # 6 x 40 games): shard win rates .325 .300 .3875 .5625 .425 .5875 give
    # chi2 = 11.76 on 5 df, p ~ 0.038.  This formula reported +/-6.27pp; the
    # honest shard-clustered interval is +/-12.6pp.  Optimistic by 2x.
    #
    # It is still emitted as `ci=` because a live loop parses that field into
    # its promotion gate and changing its meaning mid-experiment would move a
    # threshold without a human deciding to.  `ci_cluster=` is the correct one.
    half = 1.96 * math.sqrt(max(wm * (1 - wm), 1e-9) / n)

    # CORRECT: cluster on the shard.  Shards run disjoint `--seed0` ranges, so
    # their means are independent by construction and need no assumption about
    # what happens inside one.  With a handful of shards the variance estimate
    # is itself noisy, so this uses t_{k-1}, not 1.96 -- at k=6 that is 2.571.
    wins = [r["win"] for r in rows]
    est = paired_stats.cluster_ci(wins, unit="shard", n_games=int(n))
    chi2, df = float("nan"), max(0, len(rows) - 1)
    if df >= 1 and 0.0 < wm < 1.0:
        # Is the shard spread bigger than binomial noise within a shard allows?
        chi2 = sum((r["win"] - wm) ** 2 / (wm * (1 - wm) / max(1.0, r["n"]))
                   for r in rows)
    over = df >= 1 and chi2 == chi2 and chi2 > paired_stats._chi2_crit(df)

    print(f"SUMMARY win={wm:.4f} ci={half:.4f} ci_cluster={est.half:.4f} "
          f"chi2={chi2:.2f} df={df} overdispersed={int(bool(over))} "
          f"neural={cam:.1f} opp={cbm:.1f} "
          f"margin={mm:.1f} n={int(n)} errs={int(errs)} shards={len(rows)}")
    if over:
        print(f"# WARNING shards disagree beyond binomial noise "
              f"(chi2={chi2:.2f} on {df} df): quote ci_cluster="
              f"{est.half:.4f}, not ci={half:.4f}", file=sys.stderr)


if __name__ == "__main__":
    sys.exit(main() or 0)
