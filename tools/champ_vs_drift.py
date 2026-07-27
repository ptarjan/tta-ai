"""Does the champion beat a weight vector that merely DRIFTED for as long?

docs/CULTURE_GAP.md section 18 is two facts that sound contradictory: the 2p and
3p champions' weight vectors are statistically indistinguishable from an
undirected random walk (marginal-by-marginal, KS p 0.14-0.80), and yet the 2p
arm's pool win rate climbed 0.20 -> 0.76 over the same 342 generations.

The resolution offered there is that the improvement lives in the JOINT
structure, and that section 18a's test -- one weight at a time -- is blind to
correlations by construction.  That is an argument.  This is the measurement:
play the champion head-to-head against vectors produced by `tools/drift_sim.py`
with the *same* generation count, accept rate and sigma, i.e. its own drift
siblings.  DEFAULT_WEIGHTS is played too, as the reference point both started
from.

If the champion beats its drift siblings decisively, training found something
real that no marginal test on individual weights can see, and section 18's
reading is correct.  If it does not, section 18b's improvement is coming from
somewhere other than the weight vector and something is badly wrong.

    python3 tools/champ_vs_drift.py --players 2 --games 200 --samples 3
"""
import argparse
import math
import os
import random
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine.bots.weighted import DEFAULT_WEIGHTS, load_weights  # noqa
from experiments import arena  # noqa

ARCHIVE = "/Users/pt/tta-ai/experiments/archive_prehorizon"
LIVE = "/Users/pt/tta-ai/experiments/league_state"
#: champion path, generations, accepts -- the run each champion actually had
ARMS = {
    2: (f"{LIVE}/champion_2p.json", 335, 47),
    3: (f"{ARCHIVE}/champion_3p.json", 212, 30),
    4: (f"{ARCHIVE}/champion_4p.json", 119, 22),
}


def mean_ci(xs, z=1.96):
    xs = [x for x in xs if x is not None]
    n = len(xs)
    if n < 2:
        return (sum(xs) / max(1, n)), 0.0
    m = sum(xs) / n
    var = sum((x - m) ** 2 for x in xs) / (n - 1)
    return m, z * math.sqrt(var / n)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--players", type=int, default=2)
    ap.add_argument("--games", type=int, default=200)
    ap.add_argument("--samples", type=int, default=3)
    ap.add_argument("--sigma", type=float, default=0.25)
    ap.add_argument("--workers", type=int, default=1)
    a = ap.parse_args()

    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    from drift_sim import one_run

    path, gens, acc = ARMS[a.players]
    champ = load_weights(path)
    null = 1.0 / a.players
    print(f"# champion {path}  ({gens} generations, {acc} accepts)")
    print(f"# {a.players}p head-to-head, n={a.games} per row, null = {null:.3f}")
    print(f"# drift siblings: real mutate() + real guard_weights(), {gens} gens,"
          f" accept {acc / gens:.1%}, sigma {a.sigma}, coin-flip acceptance\n")
    print(f"  {'opponent':<22}{'champion win rate':>22}{'culture margin':>18}")

    rng = random.Random(31337)
    rows = [("DEFAULT_WEIGHTS", dict(DEFAULT_WEIGHTS))]
    for i in range(a.samples):
        rows.append((f"drift sibling #{i + 1}", one_run(gens, a.sigma, acc / gens, rng)))

    for label, opp in rows:
        seed0 = (20260727 + abs(hash(label)) % 9973) % 10_000_019
        res = arena.duel(champ, opp, a.players, a.games, seed0=seed0,
                         workers=a.workers)
        wm, wc = mean_ci(res["per_game"])
        mm, mc = mean_ci(res.get("per_game_margin") or [])
        print(f"  {label:<22}{wm:>13.3f} +/-{wc:.3f}{mm:>12.1f} +/-{mc:.1f}",
              flush=True)


if __name__ == "__main__":
    main()
