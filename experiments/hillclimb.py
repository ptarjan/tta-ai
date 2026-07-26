"""(1+lambda) evolution strategy over WeightedBot's weight dict.

    python3 -m experiments.hillclimb --players 4 --hours 8 --workers 3

Each generation:
  1. build `lambda` mutants of the champion (~25% of the weights perturbed by
     a gaussian step proportional to the weight's own magnitude, with an
     occasional 4x jump to escape local optima);
  2. screen each mutant against a table of champions over `--screen` games
     (seat-rotated); mutants below the null win rate are dropped immediately;
  3. play the survivor up to `--max-games` more games and accept it only if
     the lower bound of a one-sided 90% confidence interval is still above
     the null win rate (1/players) -- i.e. a clear, not a lucky, win;
  4. checkpoint.

Everything restart-safe: the champion is written atomically to
experiments/champion_{K}p.json after every generation and every generation
appends one line to experiments/generations_{K}p.jsonl, so a kill costs at
most the generation in flight.  Re-running the same command resumes.

Step size adapts with the 1/5th success rule, bounded to [0.05, 0.8].
"""
from __future__ import annotations

import argparse
import json
import math
import os
import random
import sys
import time

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine.bots.weighted import DEFAULT_WEIGHTS, save_weights  # noqa: E402
from experiments import arena  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))

# weights that must not change sign or scale wildly: culture IS the score,
# it anchors the whole evaluation's units.
FROZEN = {"culture"}


def champ_path(k):
    return os.path.join(HERE, f"champion_{k}p.json")


def gen_path(k):
    return os.path.join(HERE, f"generations_{k}p.jsonl")


# --------------------------------------------------------------- checkpoint

def load_champion(k):
    path = champ_path(k)
    if os.path.exists(path):
        try:
            with open(path) as fh:
                d = json.load(fh)
            w = dict(DEFAULT_WEIGHTS)
            w.update(d["weights"])
            return w, d.get("gen", 0), d.get("sigma", 0.25)
        except Exception:
            pass
    return dict(DEFAULT_WEIGHTS), 0, 0.25


def append_gen(k, rec):
    with open(gen_path(k), "a") as fh:
        fh.write(json.dumps(rec) + "\n")
        fh.flush()
        os.fsync(fh.fileno())


# ----------------------------------------------------------------- mutation

def mutate(w, rng, sigma, frac=0.25):
    out = dict(w)
    keys = [k for k in out if k not in FROZEN]
    n = max(1, int(round(len(keys) * frac)))
    moved = {}
    for k in rng.sample(keys, n):
        scale = sigma * (4.0 if rng.random() < 0.10 else 1.0)
        step = rng.gauss(0.0, scale) * (abs(out[k]) + 0.15)
        new = out[k] + step
        if abs(new) > 60:                    # keep the eval numerically sane
            new = math.copysign(60.0, new)
        moved[k] = [round(out[k], 4), round(new, 4)]
        out[k] = round(new, 5)
    return out, moved


# --------------------------------------------------------------- evaluation

def combine(shares, null, z=1.2816):
    """Mean, standard error and the lower bound of a one-sided CI."""
    n = len(shares)
    if n < 2:
        return 0.0, 1.0, -1.0
    m = sum(shares) / n
    var = sum((x - m) ** 2 for x in shares) / (n - 1)
    se = math.sqrt(var / n)
    return m, se, m - z * se


def challenge(mutant, champion, players, screen, max_games, seed0, workers,
              min_games=0):
    """Two-stage sequential test of `mutant` against a table of `champion`.

    `min_games` is the floor on the sample before an accept is allowed.  The
    screening batch is cheap but its 90% lower bound clears the null by luck
    often enough that, over hundreds of generations, the champion would drift
    on noise; requiring a second disjoint block of games before accepting is
    the cheapest available fix.
    """
    shares = []
    games = 0
    null = 1.0 / players
    batch = max(players, screen)
    min_games = min(min_games or 2 * screen, max_games)
    while games < max_games:
        res = arena.duel(mutant, champion, players, batch,
                         seed0=seed0 + games * 131, workers=workers)
        shares.extend(res["shares"])
        games += batch
        m, se, lo = combine(shares, null)
        if lo > null and games >= min_games:   # a clear win on enough games
            return m, se, lo, len(shares), res
        if m < null and games >= screen:   # not promising, stop paying for it
            return m, se, lo, len(shares), res
        batch = max(players, min(max_games - games, screen * 2))
        if batch <= 0:
            break
    m, se, lo = combine(shares, null)
    return m, se, lo, len(shares), res


# --------------------------------------------------------------------- run

def run(players, hours, workers, lam, screen, max_games, seed, anchor_every,
        min_games=0, log=print):
    champion, gen, sigma = load_champion(players)
    rng = random.Random(seed * 7919 + players * 101 + gen)
    t_end = time.time() + hours * 3600
    recent = []
    if not os.path.exists(champ_path(players)):
        save_weights(champ_path(players), champion, gen=gen, sigma=sigma,
                     players=players)

    while time.time() < t_end:
        gen += 1
        t0 = time.time()
        best = None
        tried = []
        broken = 0
        for j in range(lam):
            mutant, moved = mutate(champion, rng, sigma)
            m, se, lo, n, res = challenge(
                mutant, champion, players, screen, max_games,
                seed0=(gen * 1_000_003 + j * 7717 + seed) % 10_000_019,
                workers=workers, min_games=min_games)
            tried.append({"wr": round(m, 4), "lo": round(lo, 4), "n": n,
                          "moved": len(moved)})
            if n == 0 or (res and res.get("errors", 0) > res.get("games", 0)):
                broken += 1
                tried[-1]["engine_errors"] = (res or {}).get("errors", 0)
                tried[-1]["error_sample"] = (res or {}).get("error_sample", [])
            if lo > 1.0 / players and (best is None or lo > best[2]):
                best = (mutant, m, lo, n, moved, se)

        accepted = best is not None
        if accepted:
            champion = best[0]
        recent.append(accepted)
        recent = recent[-12:]
        # 1/5th success rule
        if len(recent) >= 6:
            rate = sum(recent) / len(recent)
            if rate > 0.25:
                sigma = min(0.8, sigma * 1.25)
            elif rate < 0.12:
                sigma = max(0.05, sigma * 0.85)

        rec = {
            "gen": gen, "players": players, "accepted": accepted,
            "sigma": round(sigma, 4), "secs": round(time.time() - t0, 1),
            "tried": tried, "at": time.strftime("%F %T"),
        }
        if broken:
            rec["broken"] = broken
        if accepted:
            rec.update({"win_rate": round(best[1], 4),
                        "ci_low": round(best[2], 4),
                        "games": best[3], "moved": best[4]})
        # periodic sanity check against a fixed anchor (default weights)
        if anchor_every and gen % anchor_every == 0:
            a = arena.duel(champion, "default", players, max(24, screen),
                           seed0=gen * 13, workers=workers)
            rec["vs_default"] = round(a["win_rate"], 4)
            g = arena.duel(champion, "greedy", players, max(24, screen),
                           seed0=gen * 17, workers=workers)
            rec["vs_greedy"] = round(g["win_rate"], 4)
        append_gen(players, rec)
        save_weights(champ_path(players), champion, gen=gen, sigma=sigma,
                     players=players)
        log(f"[{players}p] gen {gen} "
            + ("ACCEPT" if accepted else "reject ")
            + f" sigma={sigma:.3f} {rec['secs']}s "
            + (f"wr={rec.get('win_rate')} lo={rec.get('ci_low')} "
               f"n={rec.get('games')} moved={list(best[4])[:6]}"
               if accepted else str(tried))
            + (f" | vs_default={rec.get('vs_default')} "
               f"vs_greedy={rec.get('vs_greedy')}" if "vs_default" in rec else ""))
        sys.stdout.flush()
        # The engine is edited under us by another process.  If a generation
        # produced no playable games at all, back off instead of burning the
        # budget (and the log) at hundreds of empty generations per minute.
        if broken == lam:
            log(f"[{players}p] no playable games this generation "
                f"-- engine likely mid-edit, sleeping 60s")
            sys.stdout.flush()
            time.sleep(60)
    return champion


def main(argv=None):
    ap = argparse.ArgumentParser(description="TtA weight hill climbing")
    ap.add_argument("--players", type=int, default=4, choices=(2, 3, 4))
    ap.add_argument("--hours", type=float, default=6.0)
    ap.add_argument("--workers", type=int, default=3)
    ap.add_argument("--lambda", dest="lam", type=int, default=2)
    ap.add_argument("--screen", type=int, default=48,
                    help="games in the first screening batch")
    ap.add_argument("--max-games", type=int, default=240,
                    help="cap on games spent on one mutant")
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--min-games", type=int, default=0,
                    help="games required before an accept (default 2*screen)")
    ap.add_argument("--anchor-every", type=int, default=10,
                    help="every N generations, measure vs default and greedy")
    args = ap.parse_args(argv)
    run(args.players, args.hours, args.workers, args.lam, args.screen,
        args.max_games, args.seed, args.anchor_every, args.min_games)


if __name__ == "__main__":
    main()
