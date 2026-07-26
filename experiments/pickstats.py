"""What does a bot actually pay for cards, and which cards does it take?

Two hard numbers from the BGG empirical tournament tier list (39 games across
3 International Championships + 3 Intermezzo seasons,
https://boardgamegeek.com/thread/2494200, summarised in docs/EXPERT_STRATEGY.md):

  * **76% of Age I card picks happen at 1 civil action, only 2.5% at 3 CA.**
  * **Theology was selected exactly 0 times in 39 games.**

Both are cheap to measure on our bots, and a bot that habitually overpays for
cards is burning the civil actions it then does not have.

    python3 -m experiments.pickstats --bot champion --games 40 --players 2
"""
from __future__ import annotations

import argparse
import collections
import os
import random
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine import actions, cards as C, game  # noqa: E402


def make(spec, seed, players):
    from engine import bots as B
    if spec == "greedy":
        return B.GreedyBot(seed=seed)
    if spec == "book":
        from engine.bots.book import BookBot
        return BookBot(seed=seed)
    if spec == "book2":
        from engine.bots.book import BookBot
        return BookBot(seed=seed, version=2)
    from engine.bots.weighted import WeightedBot, load_weights
    if spec == "champion":
        here = os.path.dirname(os.path.abspath(__file__))
        path = os.path.join(here, "frozen",
                            f"champion_{players}p_strengthcheck.json")
        if not os.path.exists(path):
            path = os.path.join(here, f"champion_{players}p.json")
        return WeightedBot(weights=load_weights(path), seed=seed)
    return WeightedBot(seed=seed)


def run(spec, players, games, seed0):
    """Play `games` games of an all-`spec` table, logging every take."""
    db = C.db()
    # cost -> count, per civil age at the moment of the pick
    by_age = collections.defaultdict(collections.Counter)
    names = collections.Counter()
    picks_per_game = []
    for g in range(games):
        seed = seed0 + g
        bots = [make(spec, seed * 97 + i, players) for i in range(players)]
        state = game.new_game(players, seed)
        rng = random.Random(seed ^ 0x5EED)
        n_moves = takes = 0
        while not state.game_over and n_moves < 20000:
            mv = bots[state.decider()](state)
            if mv is not None and mv[0] == "take" and not state.pending:
                p = state.actor()
                idx = mv[1]
                name = state.card_row[idx]
                if name is not None:
                    cost = actions.take_cost(state, p, idx)
                    by_age[state.age_civil][cost] += 1
                    names[name] += 1
                    takes += 1
            game.apply(state, mv, rng)
            n_moves += 1
        picks_per_game.append(takes / players)
    return by_age, names, picks_per_game


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--bot", default="champion")
    ap.add_argument("--players", type=int, default=2)
    ap.add_argument("--games", type=int, default=40)
    ap.add_argument("--seed", type=int, default=9000)
    args = ap.parse_args(argv)

    by_age, names, ppg = run(args.bot, args.players, args.games, args.seed)
    print(f"bot={args.bot} {args.players}p games={args.games}")
    print(f"picks per player per game: {sum(ppg)/max(1,len(ppg)):.1f}\n")
    print("civil-action cost of each card pick, by the age showing in the row:")
    print(f"{'age':>4} {'n':>6} {'1 CA':>8} {'2 CA':>8} {'3 CA':>8} {'4+ CA':>8}")
    for age in ("A", "I", "II", "III"):
        c = by_age.get(age)
        if not c:
            continue
        tot = sum(c.values())
        hi = sum(v for k, v in c.items() if k >= 4)
        print(f"{age:>4} {tot:>6} {c[1]/tot:>7.1%} {c[2]/tot:>7.1%} "
              f"{c[3]/tot:>7.1%} {hi/tot:>7.1%}")
    tot_all = sum(sum(c.values()) for c in by_age.values())
    print(f"\ntournament reference for Age I: 76.0% at 1 CA, 2.5% at 3 CA")

    print("\nTheology (0 picks in 39 tournament games):")
    th = names.get("Theology", 0)
    print(f"  taken {th} times in {args.games} games "
          f"= {th/max(1,args.games):.2f} per game "
          f"({th/max(1,tot_all):.1%} of all picks)")
    print("\nmost-taken cards:")
    for n, c in names.most_common(12):
        print(f"  {c:>4}  {n}")


if __name__ == "__main__":
    main()
