"""Where does the champion lose to BookBot?

Plays BookBot against the trained champion and records, once per round, the
handful of numbers a human would look at over a player's shoulder: culture,
science rate, strength, workers, civil actions, wonders, happiness margin,
cards taken and actions left unspent.  Then prints the per-round difference.

    python3 -m experiments.book_diag --players 2 --games 20
"""
from __future__ import annotations

import argparse
import os
import statistics
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine import cards as C  # noqa: E402
from engine import economy, effects, game  # noqa: E402
from engine.bots.book import BookBot  # noqa: E402
from engine.bots.weighted import WeightedBot, load_weights  # noqa: E402

FIELDS = ("culture", "culture_rate", "science_rate", "science", "strength",
          "food_rate", "res_rate", "workers", "civil_actions", "wonders",
          "happy_margin", "hand", "techs", "colonies")


def snapshot(state, p):
    s = effects.state_stats(state, p)
    db = C.db()
    return {
        "culture": p.culture,
        "culture_rate": s.culture,
        "science_rate": s.science,
        "science": p.science,
        "strength": s.strength,
        "food_rate": s.food,
        "res_rate": s.resources,
        "workers": sum(t.workers for t in p.techs.values()),
        "civil_actions": s.civil_actions,
        "wonders": len(p.completed_wonders),
        "happy_margin": s.happy - economy.happy_required(p.yellow_bank),
        "hand": len(p.hand_civil),
        "techs": sum(1 for n in p.techs if db.type_of(n) in C.WORKER_TYPES),
        "colonies": len(p.colonies),
    }


def play_traced(bots, n, seed, book_seat):
    """Play one game, snapshotting every player at the top of each round."""
    import random
    state = game.new_game(n, seed)
    rng = random.Random(seed ^ 0x5EED)
    trace = {}          # round -> [per-player snapshot]
    moves = 0
    last_round = -1
    while not state.game_over and moves < 20000:
        if state.round != last_round:
            last_round = state.round
            trace[state.round] = [snapshot(state, q) for q in state.players]
        mv = bots[state.decider()](state)
        game.apply(state, mv, rng)
        moves += 1
    return trace, game.scores(state)


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--players", type=int, default=2)
    ap.add_argument("--games", type=int, default=20)
    ap.add_argument("--seed", type=int, default=5000)
    args = ap.parse_args(argv)

    n = args.players
    champ = load_weights(os.path.join(os.path.dirname(os.path.abspath(__file__)),
                                      f"champion_{n}p.json"))
    rows = {}           # round -> field -> [book - champ deltas]
    book_wins = 0
    games = 0
    for g in range(args.games):
        seat = g % n
        seed = args.seed + g
        bots = [BookBot(seed=seed * 97 + i) if i == seat
                else WeightedBot(weights=champ, seed=seed * 97 + i)
                for i in range(n)]
        trace, scores = play_traced(bots, n, seed, seat)
        games += 1
        book_wins += (scores[seat] == max(scores))
        for rnd, snaps in trace.items():
            book = snaps[seat]
            others = [snaps[i] for i in range(n) if i != seat]
            for f in FIELDS:
                rival = sum(o[f] for o in others) / len(others)
                rows.setdefault(rnd, {}).setdefault(f, []).append(book[f] - rival)

    print(f"{n}p, {games} games, BookBot won {book_wins}/{games}")
    print(f"per-round mean (BookBot - champion); positive = BookBot ahead\n")
    hdr = "rnd  n  " + "".join(f"{f[:9]:>10}" for f in FIELDS)
    print(hdr)
    for rnd in sorted(rows):
        vals = rows[rnd]
        cnt = len(vals["culture"])
        if cnt < max(3, games // 4):
            continue
        line = f"{rnd:>3} {cnt:>3}  "
        for f in FIELDS:
            line += f"{statistics.mean(vals[f]):>10.1f}"
        print(line)


if __name__ == "__main__":
    main()
