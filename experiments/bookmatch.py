"""Benchmark BookBot (engine/bots/book.py) against the trained champion.

The point of this script is an EXTERNAL yardstick.  Everything else in
experiments/ is self-play, which only ever shows that the champion is good
relative to itself.  BookBot is hand-written from published human strategy
advice, so a duel against it is the first measurement of our bot against a
standard that our own training loop did not produce.

Usage::

    python3 -m experiments.bookmatch --games 240            # all of 2p/3p/4p
    python3 -m experiments.bookmatch --players 2 --games 400

Fairness: every duel is seat-rotated over the same seed set (arena.duel plays
each seed once per seat), and the *same* seeds are used for every matchup at a
given player count, so BookBot-vs-champion and greedy-vs-champion are paired
game-for-game on identical deals.
"""
from __future__ import annotations

import argparse
import json
import os
import sys
import time

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from experiments import arena  # noqa: E402

_BASE_MAKE_BOT = arena.make_bot


def make_bot(spec, seed):
    """arena.make_bot plus the ``book`` bot.

    Installed over ``arena.make_bot`` so the multiprocessing workers (forked,
    hence inheriting this module's patch) can build BookBot without editing
    the shared arena module.
    """
    if spec == "book":
        from engine.bots.book import BookBot
        return BookBot(seed=seed)
    if spec == "book2":
        from engine.bots.book import BookBot
        return BookBot(seed=seed, version=2)
    if isinstance(spec, tuple) and spec and spec[0] == "book-improved":
        # ("book-improved", weights): the champion, overruled by the book in
        # the specific move kinds the strength check says it gets wrong.
        from engine.bots.book import BookImprovedBot
        return BookImprovedBot(weights=spec[1], seed=seed)
    return _BASE_MAKE_BOT(spec, seed)


arena.make_bot = make_bot


def load_spec(spec):
    if spec in ("book", "book2"):
        return spec
    if isinstance(spec, str) and spec.startswith("book-improved:"):
        return ("book-improved", arena.load_spec(spec.split(":", 1)[1]))
    return arena.load_spec(spec)


def champion(players):
    """The champion weights to benchmark against.

    Prefers `experiments/frozen/champion_Np_strengthcheck.json`, a snapshot
    taken when this benchmark started.  The live `champion_Np.json` files are
    rewritten continuously by the running hill climbs, so measuring against
    them would compare different opponents between runs and make the numbers
    unreproducible.
    """
    here = os.path.dirname(os.path.abspath(__file__))
    frozen = os.path.join(here, "frozen",
                          f"champion_{players}p_strengthcheck.json")
    if os.path.exists(frozen):
        return frozen
    return os.path.join(here, f"champion_{players}p.json")


def run(a, b, players, games, seed, workers=0, label=""):
    t0 = time.time()
    res = arena.duel(load_spec(a), load_spec(b), players, games, seed0=seed,
                     workers=workers or None)
    res["a"], res["b"], res["secs"] = a, b, round(time.time() - t0, 1)
    print(arena.fmt(res, label or a, os.path.basename(b)), f"[{res['secs']}s]",
          flush=True)
    return res


def main(argv=None):
    ap = argparse.ArgumentParser(description="BookBot vs the trained champion")
    ap.add_argument("--games", type=int, default=240,
                    help="games per matchup per player count")
    ap.add_argument("--players", type=int, default=0, choices=(0, 2, 3, 4),
                    help="0 = run 2p, 3p and 4p")
    ap.add_argument("--seed", type=int, default=1000)
    ap.add_argument("--workers", type=int, default=0)
    ap.add_argument("--out", default="experiments/bookmatch.jsonl")
    ap.add_argument("--matchups", default="book_vs_champ,greedy_vs_champ,book_vs_greedy")
    args = ap.parse_args(argv)

    counts = (2, 3, 4) if args.players == 0 else (args.players,)
    want = [m.strip() for m in args.matchups.split(",") if m.strip()]
    out = []
    for n in counts:
        champ = champion(n)
        # identical seeds across matchups => paired comparison on the same deals
        table = {
            "book_vs_champ": ("book", champ, f"book@{n}p"),
            "greedy_vs_champ": ("greedy", champ, f"greedy@{n}p"),
            "book_vs_greedy": ("book", "greedy", f"book@{n}p"),
            "champ_vs_greedy": (champ, "greedy", f"champ@{n}p"),
            "champ_vs_book": (champ, "book", f"champ@{n}p"),
            # step 5 ablation: does champion + book overrides beat both?
            "hybrid_vs_champ": ("book-improved:" + champ, champ, f"hybrid@{n}p"),
            "hybrid_vs_book": ("book-improved:" + champ, "book", f"hybrid@{n}p"),
            # v2 = the empirical tournament tier list
            "book2_vs_champ": ("book2", champ, f"book2@{n}p"),
            "book2_vs_book": ("book2", "book", f"book2@{n}p"),
        }
        for key in want:
            a, b, label = table[key]
            res = run(a, b, n, args.games, args.seed, args.workers, label)
            res["matchup"] = key
            out.append(res)
    if args.out:
        with open(args.out, "a") as fh:
            for r in out:
                slim = {k: v for k, v in r.items()
                        if k not in ("shares", "per_game")}
                fh.write(json.dumps(slim) + "\n")
    return out


if __name__ == "__main__":
    main()
