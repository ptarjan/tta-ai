"""Is `weighted.deferred_credit` still load-bearing under quiescence?

`weighted.features` applies the hand-priced deferred credit only
`if state.pending`.  A candidate that quiescence resolved to a quiet position
has an empty stack, so the credit contributes exactly zero to it.  That is an
argument from the code; this script is the experiment.

It plays the same seeds twice with the same bot -- once normally, once with
`deferred_credit` stubbed out to return zeros everywhere -- and compares the
final cultures move for move.  If the two runs are identical, every choice the
bot made was made without the hand-priced patches, and they can be deleted.

    nice -n 15 python3 tools/no_credit_check.py --players 4 --games 8 \
        --spec quiesce:experiments/champion_4p.json,levels=2
"""
from __future__ import annotations

import argparse
import json
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine import game                                    # noqa: E402
from engine.bots import weighted                           # noqa: E402
from experiments.arena import load_spec, make_bot          # noqa: E402

_ZERO = (0.0, 0.0, 0.0, 0.0, {})


def _no_credit(state, idx):
    return _ZERO


def run(spec, players, games, seed0):
    out = []
    for g in range(games):
        bots = [make_bot(spec, 1000 + i) for i in range(players)]
        st = game.play_game(bots, num_players=players,
                            seed=(seed0 + g) * 7919 + 17, move_cap=20000)
        out.append([p.culture for p in st.players])
    return out


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--players", type=int, default=4, choices=(2, 3, 4))
    ap.add_argument("--games", type=int, default=8)
    ap.add_argument("--seed", type=int, default=700)
    ap.add_argument("--spec", default="quiesce:experiments/champion_4p.json,levels=2")
    a = ap.parse_args(argv)

    spec = load_spec(a.spec)
    real = weighted.deferred_credit

    with_credit = run(spec, a.players, a.games, a.seed)
    weighted.deferred_credit = _no_credit
    try:
        without = run(spec, a.players, a.games, a.seed)
    finally:
        weighted.deferred_credit = real

    same = sum(1 for x, y in zip(with_credit, without) if x == y)
    print(json.dumps({
        "spec": a.spec, "players": a.players, "games": a.games,
        "identical_games": same,
        "differing_games": a.games - same,
        "verdict": ("deferred_credit is dead code for this bot"
                    if same == a.games else
                    "deferred_credit still changes this bot's play"),
        "with_credit": with_credit,
        "without_credit": without,
    }, indent=1))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
