"""Measure the raw costs that decide whether search is affordable.

Everything here is `time.process_time` (cpu seconds), because the box is
usually saturated by hill climbs and wall clock is meaningless.

    nice -n 15 python3 tools/cost_census.py --players 2 --games 20

Reports, per game:
  * decisions (root moves asked of a bot), branching factor histogram
  * cpu-s per full game for RandomBot / WeightedBot
  * cpu-s per `copy_state` at a sampled mid-game state
  * cpu-s per `apply` (random move)
  * cpu-s of a full random *playout* from a sampled mid-game state
  * cpu-s of one `evaluate` call
"""
from __future__ import annotations

import argparse
import os
import random
import statistics
import sys
import time

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

from engine import actions, game  # noqa: E402
from engine.bots import RandomBot, WeightedBot, evaluate  # noqa: E402
from engine.bots.fastcopy import copy_state  # noqa: E402
from engine.bots.weighted import load_weights  # noqa: E402


def play(nplayers, seed, botf, collect=None):
    rng = random.Random(seed)
    st = game.new_game(nplayers, seed)
    bots = [botf() for _ in range(nplayers)]
    n = 0
    while not game.is_over(st):
        mv = actions.legal_moves(st)
        if collect is not None:
            collect.append((len(mv), st.turn, len(st.pending)))
        p = game.current_player(st)
        st = game.apply(st, bots[p].choose(st, mv, rng), rng)
        n += 1
        if n > 100000:
            raise RuntimeError("no termination")
    return st, n


def snap_states(nplayers, seed, k=6):
    """Sample k mid-game states along a random-bot game."""
    rng = random.Random(seed)
    st = game.new_game(nplayers, seed)
    bot = RandomBot(seed=seed)
    seq = []
    n = 0
    while not game.is_over(st):
        mv = actions.legal_moves(st)
        st = game.apply(st, bot.choose(st, mv, rng), rng)
        n += 1
        if n % 40 == 0:
            seq.append(copy_state(st))
        if n > 100000:
            break
    if not seq:
        return []
    step = max(1, len(seq) // k)
    return seq[::step][:k]


def timed(fn, reps):
    t0 = time.process_time()
    for _ in range(reps):
        fn()
    return (time.process_time() - t0) / reps


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--players", type=int, default=2)
    ap.add_argument("--games", type=int, default=10)
    ap.add_argument("--weights", default=None)
    args = ap.parse_args()
    N = args.players

    w = load_weights(args.weights) if args.weights else None

    # ---- 1. decision census + weighted-bot game cost
    census = []
    t0 = time.process_time()
    lens = []
    for g in range(args.games):
        st, n = play(N, 5000 + g, lambda: WeightedBot(weights=w, seed=1), census)
        lens.append(n)
    wt = (time.process_time() - t0) / args.games

    # ---- 2. random-bot game cost
    t0 = time.process_time()
    rn = []
    for g in range(args.games):
        _, n = play(N, 7000 + g, lambda: RandomBot(seed=1))
        rn.append(n)
    rt = (time.process_time() - t0) / args.games

    bf = [c[0] for c in census]
    turns = max(c[1] for c in census)

    # ---- 3. micro costs on sampled mid-game states
    states = snap_states(N, 99)
    st = states[len(states) // 2]
    c_copy = timed(lambda: copy_state(st), 2000)
    mvs = actions.legal_moves(st)
    rr = random.Random(3)

    def one_apply():
        t = copy_state(st)
        actions.apply(t, rr.choice(actions.legal_moves(t)), rr)

    c_applycopy = timed(one_apply, 1000)
    c_eval = timed(lambda: evaluate(st, 0, w), 2000)
    c_legal = timed(lambda: actions.legal_moves(st), 2000)

    # ---- 4. full random playout from sampled states
    pl_t, pl_n = [], []
    for s in states:
        for k in range(3):
            base = copy_state(s)
            r = random.Random(k)
            bot = RandomBot(seed=k)
            t0 = time.process_time()
            n = 0
            while not game.is_over(base):
                base = game.apply(base, bot.choose(base, actions.legal_moves(base), r), r)
                n += 1
                if n > 100000:
                    break
            pl_t.append(time.process_time() - t0)
            pl_n.append(n)

    print(f"== {N}p, {args.games} games, weights={args.weights or 'DEFAULT'} ==")
    print(f"moves/game   weighted={statistics.mean(lens):.0f}  random={statistics.mean(rn):.0f}")
    print(f"max turn     {turns}")
    print(f"cpu-s/game   weighted={wt:.3f}  random={rt:.4f}   "
          f"-> {1/wt:.2f} wgames/cpu-s, {1/rt:.1f} rgames/cpu-s")
    print(f"branching    mean={statistics.mean(bf):.2f} median={statistics.median(bf)} "
          f"p90={sorted(bf)[int(.9*len(bf))]} max={max(bf)} "
          f"frac==1: {sum(1 for x in bf if x==1)/len(bf):.3f}")
    print(f"pending-decisions frac: {sum(1 for c in census if c[2])/len(census):.4f}")
    print(f"copy_state   {c_copy*1e6:.1f} us")
    print(f"legal_moves  {c_legal*1e6:.1f} us")
    print(f"copy+apply   {c_applycopy*1e6:.1f} us")
    print(f"evaluate     {c_eval*1e6:.1f} us")
    print(f"random playout from midgame: {statistics.mean(pl_t)*1000:.1f} ms "
          f"({statistics.mean(pl_n):.0f} moves)  -> {1/statistics.mean(pl_t):.0f} playouts/cpu-s")
    # decisions where the bot has a real choice
    real = sum(1 for x in bf if x > 1)
    print(f"real decisions/game: {real/args.games:.0f}  (of {len(bf)/args.games:.0f} total)")


if __name__ == "__main__":
    main()
