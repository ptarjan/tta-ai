"""Wonder completions per game, for two weight vectors on the same deals.

`experiments/evaluate.py` reports win rate and culture and cannot report this,
which matters for docs/CARD_BLINDNESS.md: pricing a wonder correctly is
supposed to change *whether the bot builds wonders*, and a win-rate move with
no change in wonder completions would mean the effect came from somewhere
else.  A flat wonder count with a moved win rate is a real and reportable
outcome -- most of the newly-priced keys are not on wonders at all -- but it
has to be visible rather than assumed.

    python3 -m tools.wonder_census --a A.json --b B.json --players 2 \
        --games 200 --seed 0 --workers 4

Paired exactly the way `experiments/arena.duel` pairs: game `g` uses seed
`seed0 + g // players` with A in seat `g % players`, so every deal is played
once from each seat and the two arms see identical deals.  Reports, per arm,
completions per game and the share of games with at least one wonder, plus
the per-wonder table -- "which wonder" is the question a tier list is about.
"""
from __future__ import annotations

import argparse
import collections
import json
import multiprocessing as mp
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine import game as G                 # noqa: E402
from experiments import arena                # noqa: E402

_CFG = None


def _init(a, b, n, cap):
    global _CFG
    _CFG = (a, b, n, cap)


def _play(task):
    """Byte-identical games to `arena._play`: same seeding, same bot order.

    Deliberately a copy of arena's four lines rather than a re-derivation --
    if these games were not the SAME games `experiments/evaluate.py` scores,
    the wonder counts would not describe the duel they are reported next to.
    """
    _gi, seed, seat = task
    a, b, n, cap = _CFG
    specs = [b] * (n - 1)
    specs = specs[:seat] + [a] + specs[seat:]
    bots = [arena.make_bot(s, seed * 97 + i * 13 + 1)
            for i, s in enumerate(specs)]
    st = G.play_game(bots, n, seed=seed, move_cap=cap)
    mine = list(st.players[seat].completed_wonders)
    theirs = [w for i, p in enumerate(st.players) if i != seat
              for w in p.completed_wonders]
    return mine, theirs


def run(a, b, players, games, seed0, workers):
    tasks = [(g, (seed0 + g // players) * 7919 + 17, g % players)
             for g in range(games)]
    ctx = mp.get_context("fork" if sys.platform != "win32" else "spawn")
    if workers <= 1:
        _init(a, b, players, 20000)
        out = [_play(t) for t in tasks]
    else:
        with ctx.Pool(workers, initializer=_init,
                      initargs=(a, b, players, 20000)) as pool:
            out = pool.map(_play, tasks, chunksize=4)
    res = {}
    for label, idx in (("arm_a", 0), ("arm_b", 1)):
        names = collections.Counter()
        tot = with_any = 0
        seats = 0
        for row in out:
            got = row[idx]
            names.update(got)
            tot += len(got)
            with_any += 1 if got else 0
            seats += 1 if idx == 0 else (players - 1)
        res[label] = {
            "per_game": tot / max(1, len(out)),
            "per_seat_game": tot / max(1, seats),
            "share_with_any": with_any / max(1, len(out)),
            "by_wonder": dict(names.most_common()),
        }
    res["games"] = len(out)
    return res


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--a", default="default")
    ap.add_argument("--b", default="default")
    ap.add_argument("--players", type=int, default=2, choices=(2, 3, 4))
    ap.add_argument("--games", type=int, default=200)
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--workers", type=int, default=2)
    ap.add_argument("--out", default="")
    args = ap.parse_args(argv)
    res = run(arena.load_spec(args.a), arena.load_spec(args.b),
              args.players, args.games, args.seed, args.workers)
    res["a"], res["b"] = args.a, args.b
    print(json.dumps(res, indent=2))
    if args.out:
        with open(args.out, "a") as fh:
            fh.write(json.dumps(res) + "\n")
    return res


if __name__ == "__main__":
    raise SystemExit(0 if main() else 0)
