"""What does one training generation cost under each candidate architecture?

The league trainer's budget is games, and `--candidate-bot` changes what a game
costs by an order of magnitude.  This measures that cost so the choice of which
architecture to train is made on numbers.

It reports CPU-seconds per game, not wall-clock, and runs `workers=1`.  That is
deliberate: the box is shared, and wall-clock on a contended box measures the
neighbours.  `time.process_time` of a single-worker duel measures only this
process, so the numbers are comparable across runs made hours apart and under
different load.  Convert with

    games/hour = 3600 * cores / cpu_s_per_game

Two opponent shapes are measured because they cost very differently:

  book    an EXTERNAL opponent.  Only the candidate searches, so the cost is
          (searching seat) + (players-1) cheap seats.  Eight of the twelve pool
          entries are this shape.
  mirror  the candidate against ITSELF.  Every seat searches, so this is the
          worst case, and it is also the shape of the `past` ladder.

`TTA_JOURNAL` is forced on, matching `experiments/run_league.sh`.  It matters:
docs/DEEPER_SEARCH.md 3.1 measured the quiescent cost ratio at 1.2x with the
journal off and 1.65-2.65x with it on, because QuiescentBot holds several live
trial states and cannot use the journal's fast path.

    python3 tools/arch_cost.py --players 4 --games 8
"""
import argparse
import json
import os
import sys
import time

os.environ.setdefault("TTA_JOURNAL", "1")

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine.bots.weighted import DEFAULT_WEIGHTS  # noqa: E402
from experiments import arena  # noqa: E402
from experiments import hillclimb_league as L  # noqa: E402
from experiments import hillclimb_pool as P  # noqa: E402  (installs make_bot)

ARCHES = ("weighted", "quiescent:levels=1", "plan:width=8")


def bench(arch, opp, players, games, seed=4242, weights=None):
    L.CANDIDATE_ARCH = L.parse_candidate_bot(arch)
    w = dict(weights or DEFAULT_WEIGHTS)
    spec = L.as_spec(w)
    opp_spec = spec if opp == "mirror" else opp
    t0, w0 = time.process_time(), time.time()
    arena.duel(spec, opp_spec, players, games, seed0=seed, workers=1)
    return (time.process_time() - t0) / games, (time.time() - w0) / games


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--players", type=int, default=4)
    ap.add_argument("--games", type=int, default=8)
    ap.add_argument("--plan-games", type=int, default=0,
                    help="fewer games for the expensive arch (0 = same)")
    ap.add_argument("--opponents", default="book,mirror")
    ap.add_argument("--arches", default=",".join(ARCHES))
    ap.add_argument("--cores", type=float, default=5.0,
                    help="cores this run would get, for the games/hour column")
    ap.add_argument("--json", default=None)
    # DEFAULT_WEIGHTS systematically UNDER-states the cost of the search bots:
    # docs/DEEPER_SEARCH.md 3.1 shows quiescent cost rises with how much the
    # vector attacks, and a trained champion attacks far more than the default
    # vector does.  Budgeting a retarget from the default-vector column is how
    # you pick a width you cannot afford, so point this at the champion the arm
    # would actually resume from.
    ap.add_argument("--weights", default=None,
                    help="weight JSON to benchmark (default DEFAULT_WEIGHTS)")
    a = ap.parse_args()
    weights = None
    if a.weights:
        from engine.bots.weighted import load_weights
        weights = load_weights(a.weights)

    out = {"players": a.players, "cells": {}, "weights": a.weights or "default"}
    print(f"# {a.players}p  TTA_JOURNAL={os.environ['TTA_JOURNAL']}  workers=1  "
          f"cpu-seconds per game (contention-immune)  "
          f"weights={a.weights or 'DEFAULT_WEIGHTS'}")
    print(f"  {'architecture':<20}{'opponent':<9}{'n':>4}{'cpu_s/game':>12}"
          f"{'x 1-ply':>10}{'games/h @'+format(a.cores, '.0f')+'c':>14}")
    base = {}
    for arch in a.arches.split(","):
        for opp in a.opponents.split(","):
            n = (a.plan_games or a.games) if arch.startswith("plan") else a.games
            cpu, wall = bench(arch, opp, a.players, n, weights=weights)
            base.setdefault(opp, cpu)
            gph = 3600.0 * a.cores / cpu
            print(f"  {arch:<20}{opp:<9}{n:>4}{cpu:>12.3f}{cpu / base[opp]:>9.1f}x"
                  f"{gph:>14.0f}", flush=True)
            out["cells"][f"{arch}|{opp}"] = {"cpu_s": cpu, "wall_s": wall, "n": n}
    if a.json:
        with open(a.json, "w") as fh:
            json.dump(out, fh, indent=1)


if __name__ == "__main__":
    main()
