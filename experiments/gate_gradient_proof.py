"""Does margin scoring actually produce a gradient where win share does not?

This is the acceptance test for the gate-metric change, and it is deliberately
a MEASUREMENT rather than an assertion that the code runs.  A fix that
compiles and still returns a flat signal is a failure.

Method.  From the clean `DEFAULT_WEIGHTS` start, play a set of candidates
against every gate opponent, paired against the champion on byte-identical
seeds -- exactly what one generation of `hillclimb_league` does.  Each duel is
played ONCE and scored BOTH ways: win share (the old metric) and normalised
culture margin (the new one).  So the before/after comparison is not two
samples, it is the same games read two ways, and any difference is the metric
and nothing else.

Candidates:

  mut:*        ordinary mutants off the mutation operator.  These test that
               the metric VARIES -- different candidates must get different
               edges, or there is no gradient to climb.
  worse:*      deliberately sabotaged vectors (science negated, science
               zeroed).  These test the DIRECTION.  A worse vector must score
               WORSE.  If the sign comes out backwards the gradient is
               inverted and a long run would train toward garbage, which is a
               far more expensive failure than a flat signal.

    python3 -m experiments.gate_gradient_proof --players 3 --block 24
"""
from __future__ import annotations

import argparse
import json
import os
import random
import sys
import time

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine.bots.weighted import DEFAULT_WEIGHTS  # noqa: E402
from experiments import arena  # noqa: E402
from experiments import hillclimb_pool as P  # noqa: E402
from experiments.hillclimb import mutate  # noqa: E402


def candidates(champion, seed=99, which="all"):
    """The candidate set: ordinary mutants plus deliberate sabotage.

    On the sabotage vectors -- MEASURED, not assumed.  The obvious choice,
    negating `science`, turned out NOT to be sabotage at 3p: it scores
    +0.0599 on win share and +0.0875 on margin, taking BookBot from 0.0% to
    12.5%.  Both metrics agree it is better, so it validates nothing about
    direction.  (It is a real finding about the weight vector, not about the
    metric, and it is why the direction check needs a vector that is worse
    beyond argument.)

    `culture_negative` is that vector.  Culture points ARE the score -- the
    game is won by having the most of them -- so a bot that values them
    negatively is playing to lose by construction, and no correct metric may
    call it an improvement.  `all_zero` is the second: a bot indifferent to
    every feature, i.e. effectively random play.
    """
    rng = random.Random(seed)
    out = []
    if which in ("all", "mutants"):
        for i in range(2):
            m, moved, op = mutate(champion, rng, 0.25)
            out.append((f"mut:{i}({op})", m))

    if which in ("all", "sabotage"):
        bad = dict(champion)
        bad["science"] = -abs(champion.get("science", 0.5)) * 4.0
        out.append(("probe:science_negative", bad))

        # Playing to lose: culture is the win condition itself.
        cn = dict(champion)
        cn["culture"] = -abs(champion.get("culture", 1.0))
        out.append(("worse:culture_negative", cn))

        # Indifferent to everything -> effectively random play.
        az = {k: 0.0 for k in champion}
        out.append(("worse:all_zero", az))

        # Every preference reversed.
        neg = {k: -v for k, v in champion.items()}
        out.append(("worse:negate_all", neg))
    return out


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--players", type=int, default=3)
    ap.add_argument("--block", type=int, default=24)
    ap.add_argument("--workers", type=int, default=3)
    ap.add_argument("--seed", type=int, default=20260726)
    ap.add_argument("--scale", type=float, default=P.MARGIN_SCALE)
    ap.add_argument("--veto-z", type=float, default=1.0)
    ap.add_argument("--out", default="")
    ap.add_argument("--which", choices=("all", "mutants", "sabotage"),
                    default="all")
    args = ap.parse_args(argv)

    champion = dict(DEFAULT_WEIGHTS)
    pool = P.build_pool(args.players, log=lambda *_a: None)
    gate = [e for e in pool.sorted_entries()
            if e.tier in pool.gate_tiers and not e.is_mirror]
    cands = candidates(champion, which=args.which)
    print(f"=== {args.players}p  gate={len(gate)} opponents  "
          f"block={args.block}  scale={args.scale:g}  "
          f"candidates={[c[0] for c in cands]}", flush=True)

    def seed_for(e):
        from experiments.hillclimb_league import label_seed
        return (args.seed + label_seed(e.label) * 17) % 10_000_019

    # champion reference, once per opponent
    ref = {}
    for e in gate:
        r = arena.duel(champion, e.spec, args.players, args.block,
                       seed0=seed_for(e), workers=args.workers)
        ref[e.label] = r
        print(f"  ref {e.label:<16} win={r['win_rate']:6.1%} "
              f"margin={r['margin']:+8.1f}", flush=True)

    out = {"players": args.players, "block": args.block, "scale": args.scale,
           "candidates": {}}
    for name, cand in cands:
        rows, agg = {}, {"winshare": [], "margin": []}
        t0 = time.time()
        for e in gate:
            r = arena.duel(cand, e.spec, args.players, args.block,
                           seed0=seed_for(e), workers=args.workers)
            rr = ref[e.label]
            ws, mg = [], []
            for cw, rw, cm, rm in zip(r["per_game"], rr["per_game"],
                                      r["per_game_margin"],
                                      rr["per_game_margin"]):
                if cw is not None and rw is not None:
                    ws.append(cw - rw)
                if cm is not None and rm is not None:
                    mg.append(P.margin_share(cm, args.scale)
                              - P.margin_share(rm, args.scale))
            wm, wse = P.mean_se(ws)
            mm, mse = P.mean_se(mg)
            rows[e.label] = {
                "n": len(ws),
                "cand_win": round(r["win_rate"], 4),
                "champ_win": round(rr["win_rate"], 4),
                "cand_margin": round(r["margin"], 2),
                "champ_margin": round(rr["margin"], 2),
                "edge_winshare": round(wm, 4), "se_winshare": round(wse, 4),
                "edge_margin": round(mm, 4), "se_margin": round(mse, 4),
                # would this row have been able to veto?
                "veto_winshare": bool(len(ws) >= 2 and wm + args.veto_z * wse < 0),
                "veto_margin": bool(len(mg) >= 2 and mm + args.veto_z * mse < 0),
                "dead_winshare": bool(abs(wm) < 1e-12 and abs(wse) < 1e-12),
            }
            agg["winshare"].extend(ws)
            agg["margin"].extend(mg)
        awm, awse = P.mean_se(agg["winshare"])
        amm, amse = P.mean_se(agg["margin"])
        rec = {"rows": rows,
               "gate_edge_winshare": round(awm, 4), "gate_se_winshare": round(awse, 4),
               "gate_edge_margin": round(amm, 4), "gate_se_margin": round(amse, 4),
               "n": len(agg["margin"]), "secs": round(time.time() - t0, 1)}
        out["candidates"][name] = rec

        print(f"\n--- {name}", flush=True)
        print(f"    {'opponent':<16}{'cand%':>7}{'champ%':>8}{'cmarg':>9}"
              f"{'chmarg':>9}{'edge_ws':>10}{'edge_marg':>11}  flags", flush=True)
        for lab, r in rows.items():
            flags = []
            if r["dead_winshare"]:
                flags.append("WS-DEAD")
            if r["veto_margin"] and not r["veto_winshare"]:
                flags.append("veto-only-on-margin")
            print(f"    {lab:<16}{r['cand_win']:7.1%}{r['champ_win']:8.1%}"
                  f"{r['cand_margin']:+9.1f}{r['champ_margin']:+9.1f}"
                  f"{r['edge_winshare']:+10.4f}{r['edge_margin']:+11.4f}"
                  f"  {','.join(flags)}", flush=True)
        print(f"    GATE AGGREGATE  winshare {awm:+.4f} +/-{awse:.4f}   "
              f"margin {amm:+.4f} +/-{amse:.4f}   n={rec['n']}", flush=True)

    # ------------------------------------------------------------ verdicts
    dead = sum(1 for c in out["candidates"].values()
               for r in c["rows"].values() if r["dead_winshare"])
    total = sum(len(c["rows"]) for c in out["candidates"].values())
    marg_edges = [c["gate_edge_margin"] for c in out["candidates"].values()]
    ws_edges = [c["gate_edge_winshare"] for c in out["candidates"].values()]
    worse = [(k, v) for k, v in out["candidates"].items() if k.startswith("worse:")]
    muts = [(k, v) for k, v in out["candidates"].items() if k.startswith("mut:")]
    # A worse vector must not merely be negative, it must be negative with
    # confidence -- the whole point is that the sign is not a coin flip.
    direction_ok = bool(worse) and all(
        v["gate_edge_margin"] + 2.0 * v["gate_se_margin"] < 0 for _k, v in worse)
    out["verdict"] = {
        "rows_total": total,
        "rows_dead_on_winshare": dead,
        "rows_dead_on_margin": sum(
            1 for c in out["candidates"].values() for r in c["rows"].values()
            if abs(r["edge_margin"]) < 1e-12),
        "distinct_margin_edges": len(set(marg_edges)),
        "distinct_winshare_edges": len(set(ws_edges)),
        "margin_edge_spread": round(max(marg_edges) - min(marg_edges), 4),
        "winshare_edge_spread": round(max(ws_edges) - min(ws_edges), 4),
        "sabotage_scores_worse_on_margin": direction_ok,
        "sabotage": {k: v["gate_edge_margin"] for k, v in worse},
        "mutants": {k: v["gate_edge_margin"] for k, v in muts},
    }
    print("\n=== VERDICT " + json.dumps(out["verdict"], indent=1), flush=True)
    if args.out:
        with open(args.out, "w") as fh:
            json.dump(out, fh, indent=1)
        print(f"wrote {args.out}", flush=True)


if __name__ == "__main__":
    main()
