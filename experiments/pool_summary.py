"""Pool the SUMMARY lines of several parallel neural_eval.py shards into one.

A beam-vs-beam gate costs ~18 cpu-s per game, so n=200 in a single process is
an hour.  The loop fans it out over disjoint `--seed0` ranges instead; each
shard is seat-balanced on its own (neural_eval alternates seats within a shard),
so pooling is a plain n-weighted mean and the CI comes from the pooled n.

Prints one line in exactly the format neural_eval.py emits, so the loop's
parsing is identical whether the gate ran in one process or twelve.
"""
from __future__ import annotations

import math
import re
import sys


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
        print("SUMMARY win=0.0000 ci=1.0000 neural=0.0 opp=0.0 margin=0.0 "
              "n=0 errs=0 shards=0")
        return
    n = sum(r["n"] for r in rows)
    wm = sum(r["win"] * r["n"] for r in rows) / n
    cam = sum(r["neural"] * r["n"] for r in rows) / n
    cbm = sum(r["opp"] * r["n"] for r in rows) / n
    mm = sum(r["margin"] * r["n"] for r in rows) / n
    errs = sum(r.get("errs", 0) for r in rows)
    # win share is in [0,1]; the binomial-style normal-approx CI the loop's
    # promotion rule expects.  Recovering the exact per-game variance from the
    # shard CIs is possible but the shares are 0/0.5/1 so this is tight enough
    # and never optimistic (share variance <= p(1-p) only when no ties).
    half = 1.96 * math.sqrt(max(wm * (1 - wm), 1e-9) / n)
    print(f"SUMMARY win={wm:.4f} ci={half:.4f} neural={cam:.1f} opp={cbm:.1f} "
          f"margin={mm:.1f} n={int(n)} errs={int(errs)} shards={len(rows)}")


if __name__ == "__main__":
    main()
