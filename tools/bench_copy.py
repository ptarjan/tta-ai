"""Micro-benchmark for `engine.bots.fastcopy.copy_state`.

Profiling 4p GreedyBot self-play shows `copy_state` is ~64% of total runtime
(the bot copies the whole GameState once per candidate move), so it gets its
own benchmark: the full-game benchmark is too noisy to see a 10% copy win.

    nice -n 10 python3 tools/bench_copy.py
    nice -n 10 /usr/local/bin/pypy3 tools/bench_copy.py

States are captured mid-game (deep into age II/III, so the tableaux, decks and
hands are realistic rather than the tiny opening setup) and then copied in a
tight loop for a fixed number of CPU-seconds.
"""
from __future__ import annotations

import argparse
import json
import platform
import random
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from engine import actions, game                  # noqa: E402
from engine.bots import GreedyBot, RandomBot      # noqa: E402
from engine.bots.fastcopy import copy_state       # noqa: E402


def sample_states(n_players=4, seed=7, every=40, want=12):
    """Play a RandomBot game, snapshotting the state every `every` moves."""
    bots = [RandomBot(random.Random(seed * 131 + i)) for i in range(n_players)]
    st = game.new_game(n_players, seed=seed)
    out = []
    moves = 0
    while not game.is_over(st) and moves < 4000:
        legal = actions.legal_moves(st)
        if not legal:
            break
        mv = bots[st.decider() % len(bots)].choose(st, legal)
        actions.apply(st, mv, random.Random(moves))
        moves += 1
        if moves % every == 0:
            out.append(copy_state(st, keep_log=True))
            if len(out) >= want:
                break
    return out


def size_of(st):
    """Object counts in a state, so the benchmark reports what it is copying."""
    techs = sum(len(p.techs) for p in st.players)
    lists = sum(len(p.hand_civil) + len(p.hand_military) + len(p.colonies)
                + len(p.completed_wonders) for p in st.players)
    decks = len(st.civil_deck) + len(st.military_deck) + len(st.card_row)
    return {"players": len(st.players), "techcards": techs,
            "player_list_items": lists, "deck_items": decks}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--seconds", type=float, default=6.0)
    ap.add_argument("--warmup", type=float, default=3.0)
    ap.add_argument("--players", type=int, default=4)
    ap.add_argument("--json")
    a = ap.parse_args()

    states = sample_states(a.players)
    if not states:
        print("no states sampled", file=sys.stderr)
        return 1
    ns = len(states)

    t0 = time.process_time()
    i = 0
    while time.process_time() - t0 < a.warmup:
        copy_state(states[i % ns])
        i += 1

    t1 = time.process_time()
    n = 0
    while time.process_time() - t1 < a.seconds:
        for st in states:
            copy_state(st)
        n += ns
    dt = time.process_time() - t1

    per_us = dt / n * 1e6
    print(f"{platform.python_implementation()} {platform.python_version()}: "
          f"{n/dt:9.0f} copies/cpu-s   {per_us:7.2f} us/copy   "
          f"({ns} sampled states, {a.players}p)")
    for st in states[-1:]:
        print("   last state:", size_of(st))
    if a.json:
        Path(a.json).write_text(json.dumps(
            {"impl": platform.python_implementation(),
             "copies_per_cpu_s": n / dt, "us_per_copy": per_us,
             "states": ns, "players": a.players}, indent=1))
    return 0


if __name__ == "__main__":
    sys.exit(main())
