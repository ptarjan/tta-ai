"""Score the SAME two vectors under every league objective, on the same games.

    nice -n 19 python3 tools/objective_ab.py --players 2 --games 48 --workers 2 \
        --a experiments/archive_preplan/league_state_1ply_20260726/champion_2p.json \
        --b experiments/league_state/champion_2p.json \
        --ladder-dir experiments/league_state/ladder_2p \
        --hall-dir experiments/hall_of_fame

Why this exists
---------------
Changing what a 45-hour hill climb maximises is not a change you make on
faith.  The cheap, decisive check is not "does the new objective train
something good" (that takes the 45 hours) -- it is **"does the new objective
rank two vectors we already know the answer for the right way round?"**

We have exactly that pair, both measured against the 1,011-game human corpus
on an engine whose scoring is validated exact (docs/SCORE_VALIDATION.md):

    humans                                              159.5 final culture
    P, the 1-ply-lineage vector the league replaced      139.8
    Q, the margin-trained champion the league selected    64.7

The old objective preferred Q.  If the new one does not prefer P, it is not
an improvement and nothing should be restarted on it.

What it does
------------
Builds the real pool, plays A and B against every opponent on **byte-identical
seeds**, and then re-scores that one set of games under each objective
(`winshare`, `lead`, `blend`) and each tier-weight preset.  Because
the games are shared, the objectives are compared with zero sampling noise
between them: any difference in the verdict is the objective, not the deal.

`--a` is the CANDIDATE and `--b` the REFERENCE, matching the trainer: the
reported aggregate is the paired weighted mean of (A - B) per game, which is
the exact statistic `hillclimb_league.score_candidate` accepts on.  A mirror
entry resolves to B for both sides, again matching the trainer.

Also reports the per-game paired standard deviation of each objective's score
series, which is what sets `--objective-alpha`: win share is a 0/1 step and
carries several times the variance of the culture term, so a large alpha buys
noise rather than objective-alignment.
"""
from __future__ import annotations

import argparse
import json
import os
import statistics
import sys
import time

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine.bots.weighted import load_weights            # noqa: E402
from experiments import arena                            # noqa: E402
from experiments import hillclimb_pool as P              # noqa: E402
from experiments.hillclimb_league import (label_seed, parse_candidate_bot)  # noqa: E402

PRESETS = {
    "new": None,                       # P.DEFAULT_TIER_WEIGHTS
    "legacy": P.LEGACY_TIER_WEIGHTS,
}


def wrap(w, arch):
    """`hillclimb_league.as_spec`, reproduced exactly.

    It wraps any PLAIN DICT, which deliberately includes the mirror opponent
    and every archived `past:`/`hall:` vector -- those are the same policy
    family as the candidate and must be played by the same architecture or the
    self-play tiers measure an architecture gap instead of a weight gap.  It
    never wraps a str/tuple spec, so `book`, `var:*`, `greedy` ... stay as they
    are.  Getting this wrong makes the 51%-of-the-weight self-play rows a full
    search level weaker than the trainer plays them.
    """
    if arch is None or not isinstance(w, dict):
        return w
    kind, opts = arch
    return (kind, dict(w), dict(opts))


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--players", type=int, default=2)
    ap.add_argument("--games", type=int, default=48)
    ap.add_argument("--workers", type=int, default=2)
    ap.add_argument("--seed", type=int, default=20260727)
    ap.add_argument("--a", required=True, help="candidate weights JSON")
    ap.add_argument("--b", required=True, help="reference weights JSON")
    ap.add_argument("--arch", default="quiescent:levels=1",
                    help="architecture both vectors are played under "
                         "(default matches the live arms)")
    ap.add_argument("--ladder-dir", action="append", default=[])
    ap.add_argument("--hall-dir", action="append", default=[])
    ap.add_argument("--past-k", type=int, default=2)
    ap.add_argument("--alpha", type=float, default=P.DEFAULT_ALPHA)
    ap.add_argument("--out", default="")
    a = ap.parse_args(argv)

    arch = parse_candidate_bot(a.arch)
    A, B = load_weights(a.a), load_weights(a.b)
    params = P.ScoreParams(alpha=a.alpha)

    # One pool, built with the NEW weights; the legacy preset is applied
    # afterwards by re-weighting the same opponents, so both presets score the
    # identical games.  `floor` is forced on so the saturated-dummy question
    # can be answered from the same run.
    tw = dict(P.DEFAULT_TIER_WEIGHTS)
    tw["floor"] = max(tw.get("floor", 0.0), 0.5)
    pool = P.build_pool(a.players, ladder_dirs=a.ladder_dir,
                        tier_weights=tw, past_k=a.past_k,
                        hall_dirs=a.hall_dir, metric="blend", log=print)

    rows = {}
    for e in pool.sorted_entries():
        opp = e.resolve(B, B)          # mirror -> the reference champion
        s0 = (a.seed + label_seed(e.label) * 17) % 10_000_019
        t0 = time.time()
        ra = arena.duel(wrap(A, arch), wrap(opp, arch),
                        a.players, a.games, seed0=s0, workers=a.workers)
        rb = arena.duel(wrap(B, arch), wrap(opp, arch),
                        a.players, a.games, seed0=s0, workers=a.workers)
        rows[e.label] = {"tier": e.tier, "a": ra, "b": rb,
                         "secs": round(time.time() - t0, 1)}
        print(f"  {e.label:<40} A cult {ra['culture_a']:6.1f} win {ra['win_rate']:5.1%} "
              f"| B cult {rb['culture_a']:6.1f} win {rb['win_rate']:5.1%} "
              f"| opp {ra['culture_b']:6.1f}/{rb['culture_b']:6.1f} "
              f"[{rows[e.label]['secs']}s]")
        sys.stdout.flush()

    # ---------------------------------------------------------- verdicts
    print(f"\n{'objective':<10}{'weights':<9}{'aggregate edge (A-B)':>24}"
          f"{'z':>8}{'per-game sd':>14}   verdict")
    out = {"argv": vars(a), "rows": {}, "verdicts": []}
    for label, r in rows.items():
        out["rows"][label] = {
            "tier": r["tier"],
            "a": {k: r["a"][k] for k in ("win_rate", "ci", "culture_a",
                                         "culture_b", "margin", "games")},
            "b": {k: r["b"][k] for k in ("win_rate", "ci", "culture_a",
                                         "culture_b", "margin", "games")},
        }
    for objective in ("winshare", "margin", "own", "blend"):
        for preset, weights in PRESETS.items():
            wmap = dict(weights or P.DEFAULT_TIER_WEIGHTS)
            counts = {}
            for e in pool.entries:
                counts[e.tier] = counts.get(e.tier, 0) + 1
            samples, alldiffs = [], []
            for e in pool.entries:
                tot = wmap.get(e.tier, 0.0)
                if tot <= 0:
                    continue
                # One metric for every tier -- the per-tier override is gone
                # (docs/LEAGUE_OBJECTIVE.md).
                metric = objective
                sa = P.score_series(rows[e.label]["a"], metric, params)
                sb = P.score_series(rows[e.label]["b"], metric, params)
                d = [x - y for x, y in zip(sa, sb)
                     if x is not None and y is not None]
                if not d:
                    continue
                w = (tot / counts[e.tier]) / len(d)
                samples.extend((x, w) for x in d)
                alldiffs.extend(d)
            if not samples:
                continue
            m, se, lo = P.weighted_stats(samples)
            sd = statistics.pstdev(alldiffs) if len(alldiffs) > 1 else 0.0
            z = m / se if se else 0.0
            verdict = ("prefers A" if lo > 0 else
                       "prefers B" if m + 1.2816 * se < 0 else "null")
            print(f"{objective:<10}{preset:<9}{m:+16.4f} +/-{se:.4f}"
                  f"{z:+8.1f}{sd:14.4f}   {verdict}")
            out["verdicts"].append({"objective": objective, "weights": preset,
                                    "edge": round(m, 5), "se": round(se, 5),
                                    "z": round(z, 2), "sd": round(sd, 5),
                                    "verdict": verdict})
    if a.out:
        with open(a.out, "w") as fh:
            json.dump(out, fh, indent=1)
        print(f"\nwrote {a.out}")


if __name__ == "__main__":
    main()
