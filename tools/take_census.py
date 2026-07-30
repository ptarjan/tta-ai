"""Which cards does a weight vector actually TAKE from the row?

docs/CARD_BLINDNESS.md §5.1 names the trap this exists for: giving a card a
weight does not help until the bot takes the card.  Three of the keys added
there sit at exactly 0.000 variance because the champion never takes Masonry
or Library of Alexandria, so their weights have no gradient at all and a
hill climb only drifts them.

Pricing leaders is worth nothing if leaders are never taken.  This plays
self-play games under one weight file and counts every card taken from the
civil row, by type and by name, so "the pricing did nothing" and "the bot
never saw the card" can be told apart.

    python3 tools/take_census.py --w analysis/laneC/on.json --games 40
    python3 tools/take_census.py --w analysis/laneC/off.json --games 40 \
        --type leader

`TTA_BOARD_TYPES` applies here exactly as it does to experiments.evaluate.
"""
from __future__ import annotations

import argparse
import collections
import json
import os
import random
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine import actions as A, cards as C, game as G   # noqa: E402
from engine.bots import WeightedBot                      # noqa: E402
from engine.bots.weighted import load_weights            # noqa: E402


def _take_name(state, move):
    """The card name a ("take", i) move would pull out of the row."""
    if not (isinstance(move, tuple) and move and move[0] == "take"):
        return None
    try:
        return state.card_row[move[1]]
    except (AttributeError, IndexError, TypeError):
        return None


def census(weights, players=2, games=40, seed0=0, max_plies=4000):
    db = C.db()
    by_type = collections.Counter()
    by_name = collections.Counter()
    rounds = 0
    for g in range(games):
        seed = seed0 + g
        st = G.new_game(players, seed)
        rng = random.Random(seed)
        bots = [WeightedBot(weights=weights, seed=seed + i)
                for i in range(players)]
        for _ in range(max_plies):
            if st.game_over:
                break
            mv = bots[st.decider()].pick(st, A.legal_moves(st))
            name = _take_name(st, mv)
            if name:
                by_type[db.type_of(name)] += 1
                by_name[name] += 1
            A.apply(st, mv, rng)
        rounds += st.round
    return {"games": games, "rounds": rounds,
            "by_type": dict(by_type), "by_name": dict(by_name)}


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--w", required=True, help="weight JSON file")
    ap.add_argument("--games", type=int, default=40)
    ap.add_argument("--players", type=int, default=2)
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--type", default="", help="detail one card type")
    ap.add_argument("--json", action="store_true")
    a = ap.parse_args(argv)

    res = census(load_weights(a.w), a.players, a.games, a.seed)
    if a.json:
        print(json.dumps(res, sort_keys=True))
        return 0
    n = res["games"]
    print(f"{a.w}: {n} games at {a.players}p, {res['rounds']} rounds")
    print(f"{'type':16s} {'taken':>7s} {'per game':>9s}")
    for t, c in sorted(res["by_type"].items(), key=lambda kv: -kv[1]):
        print(f"{t:16s} {c:7d} {c / n:9.2f}")
    if a.type:
        db = C.db()
        print()
        rows = [(k, v) for k, v in res["by_name"].items()
                if db.type_of(k) == a.type]
        for k, v in sorted(rows, key=lambda kv: -kv[1]):
            print(f"  {k:30s} {v:5d} {v / n:7.2f}")
        got = {k for k, _v in rows}
        never = sorted(c["name"] for c in db.of_type(a.type)
                       if c["name"] not in got)
        if never:
            print(f"\n  NEVER TAKEN ({len(never)}): {', '.join(never)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
