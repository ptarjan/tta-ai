"""Ground truth for tools/margin_credit_ab.py: who actually beats whom?

`margin_credit_ab.py` measures what the trainer's GATE thinks of each vector.
That is a proxy.  This measures the thing the proxy is supposed to track --
head-to-head win rate against the reference vector, on the same seeds -- so
"the gate rewards this vector" and "this vector is stronger" can be compared
instead of assumed.

Without this column a claim like "the perverse cell buys accept statistic and
no strength" rests on the gate opponents' win rates being 0.000, which only
says the vector cannot beat BOOK; it says nothing about whether it beats
DEFAULT_WEIGHTS.

    python3 tools/margin_credit_truth.py --players 4 --games 150 --workers 3
"""
import argparse
import json
import os
import sys

os.environ.setdefault("TTA_JOURNAL", "1")

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from experiments import arena  # noqa: E402
from experiments import hillclimb_pool as P  # noqa: E402  (installs make_bot)
from tools.margin_credit_ab import build_vectors  # noqa: E402


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--players", type=int, default=4)
    ap.add_argument("--games", type=int, default=150)
    ap.add_argument("--workers", type=int, default=3)
    ap.add_argument("--seed", type=int, default=771013)
    ap.add_argument("--out", default="/tmp/margin_truth.json")
    ap.add_argument("--ladder", default=None,
                    help="instead of the vector set, play a ladder of "
                         "`culture_rate` levels head-to-head, each against the "
                         "one below it.  This is the question the level sweep "
                         "cannot answer: 5 -> 35 being a real gain says nothing "
                         "about whether 35 -> 60 is, and the ratchet lives "
                         "above 35, not below it")
    a = ap.parse_args()

    if a.ladder:
        lv = [float(x) for x in a.ladder.split(",")]
        base0 = dict(__import__("engine.bots.weighted", fromlist=["x"])
                     .DEFAULT_WEIGHTS)
        null = 1.0 / a.players
        print(f"# {a.players}p culture_rate ladder, n={a.games}, "
              f"null={null:.3f}: each level as ONE seat against a table of the "
              f"level below")
        for lo, hi in zip(lv, lv[1:]):
            wl = dict(base0, culture_rate=lo)
            wh = dict(base0, culture_rate=hi)
            res = arena.duel(wh, wl, a.players, a.games, seed0=a.seed,
                             workers=a.workers)
            wr, ci = res["win_rate"], res["ci"]
            verdict = ("BETTER" if wr - ci > null else
                       "worse" if wr + ci < null else "n.s.")
            print(f"  {hi:>8.3f} vs {lo:<8.3f}{wr:>10.3f}+-{ci:.3f}"
                  f"{res.get('margin', 0.0):>12.1f}{verdict:>10}", flush=True)
        return

    vec = build_vectors(a.players)
    base = vec["base"]
    null = 1.0 / a.players
    print(f"# {a.players}p head-to-head vs base (DEFAULT_WEIGHTS), n={a.games}, "
          f"null={null:.3f}")
    print(f"  {'vector':<12}{'win rate':>18}{'culture margin':>18}"
          f"{'stronger?':>12}")
    out = {}
    for name, w in vec.items():
        if name == "base":
            continue
        res = arena.duel(w, base, a.players, a.games, seed0=a.seed,
                         workers=a.workers)
        wr, ci, mg = res["win_rate"], res["ci"], res.get("margin", 0.0)
        verdict = ("STRONGER" if wr - ci > null else
                   "weaker" if wr + ci < null else "n.s.")
        print(f"  {name:<12}{wr:>10.3f}+-{ci:.3f}{mg:>18.1f}{verdict:>12}",
              flush=True)
        out[name] = {"win_rate": wr, "ci": ci, "margin": mg, "null": null}
    with open(a.out, "w") as fh:
        json.dump(out, fh, indent=1)


if __name__ == "__main__":
    main()
