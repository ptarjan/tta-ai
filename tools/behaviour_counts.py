"""Per-game counts of the moves the 1-ply search is blind to.

`experiments/behaviour.py` reports wars and aggressions but has no counter for
pact offers or colony bids, which are two of the four move classes
docs/PACTS_DIAGNOSIS.md identifies as strictly dominated at 1 ply.  This
counts all four, plus what they actually produced (pacts signed, colonies
taken), by wrapping every bot at the table and tallying the moves it picks.

Mirror tables: every seat runs the same bot, so the counts are "what this
search does when everyone uses it", not "what it does against a 1-ply field".
That is the right comparison for the emergence question -- a bot that attacks
only because its opponents cannot retaliate has not learned anything.

    nice -n 15 python3 tools/behaviour_counts.py --players 4 --games 40 \
        --spec quiesce:experiments/champion_4p.json,levels=2
"""
from __future__ import annotations

import argparse
import json
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine import game                                    # noqa: E402
from experiments.arena import load_spec, make_bot          # noqa: E402

# move kinds we care about, in report order
KINDS = ("offer_pact", "war", "aggression", "bid", "cancel_pact",
         "prepare_event")


class _Counting:
    """Wraps a bot and tallies the move kinds it chooses."""

    def __init__(self, bot):
        self.bot = bot
        self.counts = {}

    def _note(self, mv):
        if mv:
            k = mv[0]
            if k in KINDS:
                self.counts[k] = self.counts.get(k, 0) + 1
        return mv

    def choose(self, state, moves, rng=None):
        return self._note(self.bot.choose(state, moves, rng))

    def __call__(self, state):
        return self._note(self.bot(state))


def run(spec, players, games, seed0):
    tot = {k: 0 for k in KINDS}
    colonies = pacts = 0
    offers_accepted = 0
    for g in range(games):
        bots = [_Counting(make_bot(spec, 1000 + i)) for i in range(players)]
        st = game.play_game(bots, num_players=players,
                            seed=(seed0 + g) * 7919 + 17, move_cap=20000)
        for b in bots:
            for k, v in b.counts.items():
                tot[k] += v
        for p in st.players:
            colonies += len(getattr(p, "colonies", ()) or ())
            pacts += len(getattr(p, "pacts", ()) or ())
    n = float(games)
    out = {k: round(tot[k] / n, 3) for k in KINDS}
    out["colonies_held_end"] = round(colonies / n, 3)
    out["pacts_live_end"] = round(pacts / n, 3)
    return out


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--players", type=int, default=4, choices=(2, 3, 4))
    ap.add_argument("--games", type=int, default=40)
    ap.add_argument("--seed", type=int, default=55000)
    ap.add_argument("--spec", required=True)
    ap.add_argument("--label", default="")
    a = ap.parse_args(argv)

    res = run(load_spec(a.spec), a.players, a.games, a.seed)
    print(json.dumps({"label": a.label or a.spec, "players": a.players,
                      "games": a.games, "per_game": res}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
