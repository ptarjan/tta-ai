#!/usr/bin/env python3
"""Compare 4p league arms' trajectories, matched on GENERATION, not wall clock.

    python3 tools/probe_compare.py

Arms are configured in `ARMS` below.  Read-only: never writes to any state dir.

WHY THERE IS A "HORIZON-INVARIANT" METRIC HERE
----------------------------------------------
The obvious statistic -- the pool-weight-averaged win rate over the whole
`fullcheck_4p.jsonl` -- is NOT comparable between an arm running the new
`lateness()` and an arm running the old one, because three of the thirteen full
check opponents are themselves `WeightedBot`s and therefore change when the
horizon changes:

    default                   arena.make_bot: WeightedBot(seed)  -> DEFAULT_WEIGHTS
    past:ladder_4p/gen00000   WeightedBot(weights=...)
    past:league_4p/gen00103   WeightedBot(weights=...)

and `docs/CULTURE_GAP.md` 8d/8f measured that the new horizon makes
DEFAULT_WEIGHTS *stronger* (+7.5 points at 4p) and makes an already-trained
champion *weaker* (20.1% against a 25% null).  So an arm on the new horizon
faces a harder `default` and a crippled `past:league_4p/gen00103` -- movement
that says nothing about the arm itself.  It showed up immediately: at matched
gen 10 the probe read +0.396 against `past:league_4p/gen00103` and -0.198
against `default`, which is the artifact, not a result.

The other ten opponents never call `lateness()` and are byte-identical across
the two builds:

    book, book2                       BookBot v1 / v2      (rule-based)
    var:{culture,infra,military,      VariantBot           (BookBot subclass)
         science,tempo,wonder}
    greedy                            GreedyBot            (its own WEIGHTS,
                                                            its own evaluate())
    random                            RandomBot

Those ten are the fixed yardstick and `HORIZON_INVARIANT` below is exactly that
set.  Both metrics are printed; the invariant one is the one to read.
"""
import json
import os
import sys

LIVE = "/Users/pt/tta-ai/experiments/league_state"
ARMS = [
    ("probe  (new horizon)", "/tmp/tta-probe/experiments/probe_state_4p"),
    ("control(old horizon)", "/tmp/tta-control/experiments/control_state_4p"),
    ("live   (old horizon)", LIVE),
]

#: opponents whose play does not depend on `weighted.lateness()` -- see module
#: docstring.  Everything else in the full check is a WeightedBot.
HORIZON_INVARIANT = frozenset((
    "book", "book2", "greedy", "random",
    "var:culture", "var:infra", "var:military",
    "var:science", "var:tempo", "var:wonder",
))


def load(state_dir, name):
    p = os.path.join(state_dir, name)
    if not os.path.exists(p):
        return []
    with open(p) as fh:
        return [json.loads(l) for l in fh]


def pooled(rec, only=None):
    """Pool-weight-averaged win rate, culture margin, and a standard error.

    The se treats each opponent as an independent binomial over its own n.
    That is optimistic -- one candidate vector plays all of them, so its own
    strength is a shared term this does not price -- so read it as a floor.
    """
    res = {k: v for k, v in rec["results"].items()
           if only is None or k in only}
    if not res:
        return None
    tw = sum(v["weight"] for v in res.values())
    win = sum(v["win_rate"] * v["weight"] for v in res.values()) / tw
    mar = sum(v["margin"] * v["weight"] for v in res.values()) / tw
    var = sum((v["weight"] / tw) ** 2 * v["win_rate"] * (1 - v["win_rate"])
              / max(1, v["n"]) for v in res.values())
    return win, mar, var ** 0.5


def main():
    arms = []
    for label, sd in ARMS:
        fc = load(sd, "fullcheck_4p.jsonl")
        gens = load(sd, "generations_4p.jsonl")
        arms.append((label, sd, fc, gens))

    print("=" * 78)
    for label, sd, fc, gens in arms:
        if not gens:
            print(f"{label}  {sd}\n    (not started)")
            continue
        secs = sum(r["secs"] for r in gens)
        print(f"{label}  {sd}")
        print(f"    gen {len(gens):>4}   accepts {sum(1 for r in gens if r['accepted']):>3}"
              f"   {secs / max(1, len(gens)):>5.0f}s/gen (in-generation)"
              f"   fullchecks {len(fc)}")

    live_fc = {r["gen"]: r for r in arms[-1][2]}
    for metric_name, subset in (("HORIZON-INVARIANT (10 opponents)", HORIZON_INVARIANT),
                                ("all 13 opponents (confounded, see docstring)", None)):
        print("\n" + "-" * 78)
        print(metric_name)
        hdr = "  gen |"
        for label, _, fc, _ in arms:
            if fc:
                hdr += f" {label.split()[0]:>17} |"
        print(hdr)
        print("      | " + " | ".join("      win   margin" for l, _, f, _ in arms if f))
        allgens = sorted({r["gen"] for _, _, fc, _ in arms for r in fc})
        for g in allgens:
            row = f"  {g:>3} |"
            for label, _, fc, _ in arms:
                if not fc:
                    continue
                rec = next((r for r in fc if r["gen"] == g), None)
                if rec is None:
                    row += "        -        - |"
                    continue
                w, m, se = pooled(rec, subset)
                row += f"  {w:.3f}+-{se:.3f} {m:>7.1f} |"
            print(row)
    print()


if __name__ == "__main__":
    main()
