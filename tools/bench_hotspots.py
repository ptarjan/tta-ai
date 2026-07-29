"""Per-hotspot CPU cost, so an interpreter comparison can be *explained*.

`tools/bench_interp.py` says which interpreter plays more games per CPU-second.
It cannot say why one bot flips the answer and another does not -- and
docs/PYPY.md section 10 found exactly that: PyPy is much faster than CPython on
GreedyBot and slower on WeightedBot, on the same engine, in the same process
shape.  A whole-game number cannot resolve that; this can.

Each row is one hot operation timed on its own, warmed up first (PyPy's JIT
needs to see the loop), measured in `time.process_time` because the box is
never idle:

    nice -n 10 python3 tools/bench_hotspots.py --json /tmp/h_cpy.json
    nice -n 10 pypy3   tools/bench_hotspots.py --json /tmp/h_pypy.json

Ratio the two files and every row is an ops/cpu-s speed-up for that operation
alone.
"""
from __future__ import annotations

import argparse
import json
import math
import platform
import random
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from engine import actions, game, journal            # noqa: E402
from engine.bots import GreedyBot, RandomBot         # noqa: E402
from engine.bots import evaluate as greedy_evaluate  # noqa: E402
from engine.bots.fastcopy import copy_state          # noqa: E402
from engine.bots import weighted as W                # noqa: E402


def mid_game_states(n_players=4, seed=7, every=40, want=6):
    """Snapshots deep enough to have real tableaux, decks and hands."""
    bots = [RandomBot(random.Random(seed * 131 + i)) for i in range(n_players)]
    st = game.new_game(n_players, seed=seed)
    out, moves = [], 0
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


#: Everything each benchmarked call returns is stored here.  This is NOT
#: decoration: PyPy's escape analysis removes allocations whose result never
#: leaves the loop, so timing `copy_state(st)` with the copy discarded measures
#: a copy PyPy is partly allowed not to make.  Storing into a module-level list
#: forces the object to escape and makes the two interpreters time the same
#: work.  Section 10 has the before/after -- it is worth up to 2x on PyPy.
SINK = [None]


def timed(fn, warm_s, meas_s):
    """(ops/cpu-s, ops) for `fn`, after `warm_s` CPU-seconds of warm-up."""
    sink = SINK
    t0 = time.process_time()
    while time.process_time() - t0 < warm_s:
        for _ in range(50):
            sink[0] = fn()
    n = 0
    t1 = time.process_time()
    while time.process_time() - t1 < meas_s:
        for _ in range(50):
            sink[0] = fn()
        n += 50
    dt = time.process_time() - t1
    return n / dt, n


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--warm", type=float, default=2.0)
    ap.add_argument("--meas", type=float, default=4.0)
    ap.add_argument("--vary", action="store_true",
                    help="cycle over several different states instead of "
                         "hammering one -- see the caveat in section 10")
    ap.add_argument("--json")
    a = ap.parse_args()

    states = mid_game_states()
    st = states[-1]
    if a.vary:
        pool = [(s, s.decider() % len(s.players)) for s in states]
        pool = [(s, i, W.rival_context(s, i)) for s, i in pool]
        turn = [0]

        def _next():
            turn[0] = (turn[0] + 1) % len(pool)
            return pool[turn[0]]

        rows_v = {}
        for name, fn in (
            ("weighted.features (varied)",
             lambda: (lambda t: W.features(t[0], t[1], t[2]))(_next())),
            ("weighted.evaluate (varied)",
             lambda: (lambda t: W.evaluate(t[0], t[1],
                                           dict(W.DEFAULT_WEIGHTS), t[2]))(
                 _next())),
            ("copy_state (varied)",
             lambda: copy_state(_next()[0])),
            ("legal_moves (varied)",
             lambda: actions.legal_moves(_next()[0])),
        ):
            ops, n = timed(fn, a.warm, a.meas)
            rows_v[name] = round(ops, 1)
            print(f"{name:32s} {ops:12.1f} ops/cpu-s  (n={n})", flush=True)
        out = {"impl": platform.python_implementation(),
               "version": platform.python_version(), "rows": rows_v}
        if a.json:
            Path(a.json).write_text(json.dumps(out, indent=1))
        return 0
    idx = st.decider() % len(st.players)
    moves = actions.legal_moves(st)
    mv = moves[0]
    w = dict(W.DEFAULT_WEIGHTS)
    ctx = W.rival_context(st, idx)
    feats = W.features(st, idx, ctx)
    gw = {k: 1.0 for k in feats}
    names = [p for p in st.players[idx].hand_civil] or ["Bronze"]
    counter = [0]

    def apply_on_copy():
        c = copy_state(st)
        actions.apply(c, mv, random.Random(0))

    def journal_apply():
        j = journal.begin(st)
        try:
            actions.apply(st, mv, random.Random(0))
        finally:
            journal.rollback(j)

    def setattr_hot():
        p = st.players[idx]
        counter[0] += 1
        p.science = counter[0] & 7

    rows = {}
    bench = [
        ("copy_state", lambda: copy_state(st)),
        ("copy_state+apply", apply_on_copy),
        ("weighted.features", lambda: W.features(st, idx, ctx)),
        ("weighted.evaluate", lambda: W.evaluate(st, idx, w, ctx)),
        ("weighted.rival_context", lambda: W.rival_context(st, idx)),
        ("greedy.evaluate", lambda: greedy_evaluate(st, idx, gw)),
        ("math.fsum(80 floats)",
         lambda: math.fsum([i * 0.5 for i in range(80)])),
        ("lru_cache hit (_card_yields)", lambda: W._card_yields(names[0])),
        ("legal_moves", lambda: actions.legal_moves(st)),
        ("attr write, hook OFF", setattr_hot),
    ]
    for name, fn in bench:
        ops, n = timed(fn, a.warm, a.meas)
        rows[name] = round(ops, 1)
        print(f"{name:32s} {ops:12.1f} ops/cpu-s  (n={n})", flush=True)

    # The same two writes with the journalling __setattr__ installed but no
    # journal open -- what every bot in a mixed league worker pays once any
    # WeightedBot search has run in the process.
    journal.install()
    for name, fn in (("attr write, hook ON (no journal)", setattr_hot),
                     ("copy_state, hook ON", lambda: copy_state(st)),
                     ("copy_state+apply, hook ON", apply_on_copy)):
        ops, n = timed(fn, a.warm, a.meas)
        rows[name] = round(ops, 1)
        print(f"{name:32s} {ops:12.1f} ops/cpu-s  (n={n})", flush=True)
    # journalled apply needs the hook installed, so it goes here
    ops, n = timed(journal_apply, a.warm, a.meas)
    rows["journal begin+apply+rollback"] = round(ops, 1)
    print(f"{'journal begin+apply+rollback':32s} {ops:12.1f} ops/cpu-s "
          f"(n={n})", flush=True)

    out = {"impl": platform.python_implementation(),
           "version": platform.python_version(), "rows": rows}
    if a.json:
        Path(a.json).write_text(json.dumps(out, indent=1))
    return 0


if __name__ == "__main__":
    sys.exit(main())
