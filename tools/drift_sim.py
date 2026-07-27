"""Does the FLATTENING need selection pressure, or does the move generator do it?

docs/CULTURE_GAP.md section 12 argued that `culture_rate_early` is driven to
exactly 0.000 by `guard_weights`' one-sided clamp plus the fact that
`hillclimb.mutate`'s step is proportional to the weight's own magnitude.  That
argument is about the SEARCH'S MOVE GENERATOR, not about the game -- so it can
be tested without playing a single game, by running the real `mutate()` and the
real `guard_weights()` under a NULL acceptance model: accept a proposal at
random, with no reference to whether it is any good.

If the observed pattern (early pinned at exactly 0, shape collapsed, base
inflated) appears under pure drift, then no selection pressure is needed to
explain it and the culture-rate flattening is an artefact of the optimiser
rather than a finding about Through the Ages.  If it does not appear, the
flattening needs selection and the gate metric is implicated.

    python3 tools/drift_sim.py --gens 200 --runs 400
"""
import argparse
import json
import os
import random
import statistics
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine.bots.weighted import DEFAULT_WEIGHTS, PHASE_KEYS  # noqa
from experiments.hillclimb import mutate  # noqa
from experiments.hillclimb_league import guard_weights  # noqa


def one_run(gens, sigma, accept_p, rng, guard=True, key="culture_rate"):
    w = dict(DEFAULT_WEIGHTS)
    for _ in range(gens):
        # the trainer proposes `lambda`=2 mutants per generation and takes at
        # most one; with no selection, pick uniformly among them and accept
        # with probability `accept_p` (the live arms' measured 14-26%).
        cands = []
        for _ in range(2):
            m, _, _ = mutate(w, rng, sigma)
            if guard:
                m, _ = guard_weights(m, "clamp")
            cands.append(m)
        if rng.random() < accept_p:
            w = cands[rng.randrange(len(cands))]
    return w


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--gens", type=int, default=200)
    ap.add_argument("--runs", type=int, default=400)
    ap.add_argument("--sigma", type=float, default=0.25)
    ap.add_argument("--accept", type=float, default=0.20)
    ap.add_argument("--seed", type=int, default=20260726)
    a = ap.parse_args()

    print(f"# pure-drift simulation: real mutate() + real guard_weights(), "
          f"acceptance is a COIN FLIP (p={a.accept}), no game is played.")
    print(f"# {a.runs} independent runs x {a.gens} generations, sigma={a.sigma}")
    print()
    for guard in (True, False):
        rng = random.Random(a.seed)
        outs = [one_run(a.gens, a.sigma, a.accept, rng, guard) for _ in range(a.runs)]
        tag = "guard ON (as the trainer runs)" if guard else "guard OFF (counterfactual)"
        print(f"--- {tag} ---")
        print(f"  {'phase key':<17}{'early==0':>9}{'|shape| med':>12}"
              f"{'kept med':>10}{'base med':>10}{'base p90':>10}")
        for k in PHASE_KEYS:
            e = [w[k + "_early"] for w in outs]
            sh = [abs(w[k + "_late"] - w[k + "_early"]) for w in outs]
            b = [abs(w[k]) for w in outs]
            d_sh = abs(DEFAULT_WEIGHTS[k + "_late"] - DEFAULT_WEIGHTS[k + "_early"])
            zero = sum(1 for x in e if x == 0.0) / len(e)
            print(f"  {k:<17}{zero:>8.1%}{statistics.median(sh):>12.3f}"
                  f"{statistics.median(sh) / d_sh:>10.2f}"
                  f"{statistics.median(b):>10.2f}"
                  f"{sorted(b)[int(0.9 * len(b))]:>10.2f}")
        # how many of the 10 POSITIVE-default multipliers land exactly on 0,
        # against the 10 negative-default ones the guard exempts
        pos = [k + s for k in PHASE_KEYS for s in ("_early", "_late")
               if DEFAULT_WEIGHTS[k + s] > 0]
        neg = [k + s for k in PHASE_KEYS for s in ("_early", "_late")
               if DEFAULT_WEIGHTS[k + s] < 0]
        zp = sum(1 for w in outs for k in pos if w[k] == 0.0) / (len(outs) * len(pos))
        zn = sum(1 for w in outs for k in neg if w[k] == 0.0) / (len(outs) * len(neg))
        print(f"  exactly-zero rate: positive-default multipliers {zp:.1%}, "
              f"negative-default (guard-exempt) {zn:.1%}")
        print()


if __name__ == "__main__":
    main()
