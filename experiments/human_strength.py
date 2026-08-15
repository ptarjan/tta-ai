"""Are the human archetypes worth training against, or are they decoration?

    nice -n 19 python3 -m experiments.human_strength --games 200 \
        --bots hum:builder,hum:wonder,hum:tempo,hum:warlord \
        --foes "book;book2;quiesce:/tmp/champ_2p_snapshot.json,levels=1"

`docs/BOT_ROSTER.md`'s honest read is the standard this has to meet: a pool
member below par is a "sparring partner", not a gate, and saying which is
which up front is the whole value of that document.  A human-imitating bot
that loses to everything is a perfectly publishable result -- it just must not
be shipped into the pool silently.

Seat-rotated and seed-paired via `experiments.arena.duel`, so the numbers are
comparable with `docs/BOT_ROSTER.md`'s.  Null is 1/players.
"""
from __future__ import annotations

import argparse
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from experiments import hillclimb_pool as P                  # noqa: E402,F401
from experiments import arena                                # noqa: E402
from experiments.human_exploit import _spec                  # noqa: E402


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--bots", default="hum:builder,hum:wonder,hum:tempo,"
                                      "hum:warlord")
    ap.add_argument("--foes", default="book;book2")
    ap.add_argument("--games", type=int, default=120)
    ap.add_argument("--players", type=int, default=2)
    ap.add_argument("--workers", type=int, default=1)
    ap.add_argument("--seed", type=int, default=3000)
    a = ap.parse_args(argv)
    null = 100.0 / a.players
    print("null = %.1f%%   n=%d games, seat-rotated, seed-paired" % (null,
                                                                    a.games))
    print("%-14s %-38s %16s %14s" % ("bot", "vs", "win share", "culture margin"))
    for s in a.bots.split(","):
        for f in a.foes.split(";"):
            if not f:
                continue
            r = arena.duel(_spec(s), _spec(f), a.players, a.games,
                           seed0=a.seed, workers=a.workers)
            m = r.get("per_game_margin") or []
            mm = sum(m) / len(m) if m else float("nan")
            sd = (sum((x - mm) ** 2 for x in m) / max(1, len(m) - 1)) ** 0.5 \
                if len(m) > 2 else float("nan")
            print("%-14s %-38s %6.1f%% +-%4.1f%%   %7.1f +-%4.1f"
                  % (s, f[:38], 100 * r["win_rate"], 100 * r["ci"],
                     mm, 1.96 * sd / max(1, len(m)) ** 0.5))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
