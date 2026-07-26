"""Fast A-vs-B evaluation.

    python3 -m experiments.evaluate --a champion_4p.json --b greedy \
        --games 120 --players 4

`--a` / `--b` are either a JSON weight file or one of the built-in bots
``random`` / ``greedy`` / ``default`` (WeightedBot at default weights).
The challenger A is rotated through every seat, so the reported win rate is
seat-balanced; the null hypothesis is 1/players.
"""
from __future__ import annotations

import argparse
import json
import os
import sys
import time

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from experiments import arena  # noqa: E402


def main(argv=None):
    ap = argparse.ArgumentParser(description="TtA head-to-head evaluation")
    ap.add_argument("--a", default="default", help="challenger: file|random|greedy|default")
    ap.add_argument("--b", default="greedy", help="defenders: file|random|greedy|default")
    ap.add_argument("--games", type=int, default=120)
    ap.add_argument("--players", type=int, default=4, choices=(2, 3, 4))
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--workers", type=int, default=0)
    ap.add_argument("--json", action="store_true", help="print the raw result dict")
    ap.add_argument("--out", default="", help="append the result to this JSONL file")
    args = ap.parse_args(argv)

    a = arena.load_spec(args.a)
    b = arena.load_spec(args.b)
    t0 = time.time()
    res = arena.duel(a, b, args.players, args.games, seed0=args.seed,
                     workers=args.workers or None)
    res["secs"] = round(time.time() - t0, 1)
    res["a"] = args.a
    res["b"] = args.b
    if args.json:
        print(json.dumps(res))
    else:
        print(arena.fmt(res, os.path.basename(args.a), os.path.basename(args.b)),
              f"[{res['secs']}s]")
    if args.out:
        os.makedirs(os.path.dirname(os.path.abspath(args.out)), exist_ok=True)
        with open(args.out, "a") as fh:
            fh.write(json.dumps(res) + "\n")
    return res


if __name__ == "__main__":
    main()
