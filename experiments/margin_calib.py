"""Calibrate the league objective's culture-differential normalisation.

Plays DEFAULT_WEIGHTS against every non-mirror pool opponent at a player
count and dumps the per-game culture margins, so `LEAD_SCALE` in
`experiments/hillclimb_pool.py` is chosen from the measured distribution
instead of guessed.

NOTE (2026-07-30): the objective now scores the lead over the BEST opponent,
while this script still dumps `per_game_margin`, the margin over the MEAN.
At 2p they are the same number.  At 3p/4p the lead is at least as dispersed,
so a scale derived from this script is a lower bound there and should be
re-derived from `per_game_lead` -- see docs/LEAGUE_OBJECTIVE.md section 5.  Also reports the win-share distribution on the SAME
games, which is the before/after comparison the fix exists to justify:
where win share is a flat 0.0 the margin is not.

    python3 -m experiments.margin_calib --players 3 --games 24 \
        --out /tmp/calib_3p.json
"""
from __future__ import annotations

import argparse
import json
import os
import statistics
import sys
import time

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine.bots.weighted import DEFAULT_WEIGHTS  # noqa: E402
from experiments import arena  # noqa: E402
from experiments import hillclimb_pool as P  # noqa: E402


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--players", type=int, default=3)
    ap.add_argument("--games", type=int, default=24)
    ap.add_argument("--workers", type=int, default=2)
    ap.add_argument("--seed", type=int, default=4242)
    ap.add_argument("--out", default="")
    args = ap.parse_args(argv)

    pool = P.build_pool(args.players, log=lambda *_a: None)
    out = {"players": args.players, "games": args.games, "opponents": {}}
    for e in pool.sorted_entries():
        if e.is_mirror:
            continue
        t0 = time.time()
        res = arena.duel(dict(DEFAULT_WEIGHTS), e.spec, args.players,
                         args.games, seed0=args.seed, workers=args.workers)
        marg = [m for m in res["per_game_margin"] if m is not None]
        rec = {
            "tier": e.tier,
            "win_rate": round(res["win_rate"], 4),
            "n": len(marg),
            "margin_mean": round(statistics.fmean(marg), 2) if marg else None,
            "margin_sd": round(statistics.stdev(marg), 2) if len(marg) > 1 else None,
            "margin_min": round(min(marg), 1) if marg else None,
            "margin_max": round(max(marg), 1) if marg else None,
            "margins": [round(m, 1) for m in marg],
            "secs": round(time.time() - t0, 1),
        }
        out["opponents"][e.label] = rec
        print(f"{e.label:<16}{e.tier:<9} win={res['win_rate']:6.1%} "
              f"margin={rec['margin_mean']:>8} sd={rec['margin_sd']:>7} "
              f"[{rec['margin_min']}, {rec['margin_max']}] {rec['secs']}s",
              flush=True)

    gate = [m for lab, r in out["opponents"].items() if r["tier"] in ("book", "variant")
            for m in r["margins"]]
    if gate:
        out["gate_pooled"] = {
            "n": len(gate),
            "mean": round(statistics.fmean(gate), 2),
            "sd": round(statistics.stdev(gate), 2),
            "abs_mean": round(statistics.fmean([abs(m) for m in gate]), 2),
            "p10": round(sorted(gate)[len(gate) // 10], 1),
            "p90": round(sorted(gate)[9 * len(gate) // 10], 1),
        }
        print("GATE POOLED", json.dumps(out["gate_pooled"]), flush=True)
    if args.out:
        with open(args.out, "w") as fh:
            json.dump(out, fh, indent=1)
        print(f"wrote {args.out}", flush=True)


if __name__ == "__main__":
    main()
