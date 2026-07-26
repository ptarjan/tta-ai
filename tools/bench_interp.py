"""Steady-state throughput benchmark, CPython vs PyPy.

`engine.perf_check bench` warms up by *game count*, which is the wrong unit for
PyPy: a 4p GreedyBot game is ~2 CPU-seconds, so "4 warm-up games" is 8 seconds
of warm-up for one cell and 0.2 s for another.  PyPy's JIT needs to see the hot
loops for a few seconds of *time* before it has compiled and specialised them,
so this tool warms up for a fixed number of CPU-seconds and then measures for a
fixed number of CPU-seconds, reporting only the steady-state window.

    pypy3   tools/bench_interp.py --warmup 8 --measure 12 --json /tmp/b_pypy.json
    python3 tools/bench_interp.py --warmup 8 --measure 12 --json /tmp/b_cpy.json

It also reports a per-second trace of the warm-up so the JIT ramp is visible
rather than assumed.

NOTE: on this box the benchmark always runs against a fully loaded machine (the
hill climbs saturate all 6 cores), so `time.process_time` -- CPU time consumed
by *this* process -- is the only stable metric.  Wall-clock games/s would just
be measuring the scheduler.
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

from engine import game                     # noqa: E402
from engine.bots import GreedyBot, RandomBot  # noqa: E402


def _bots(kind, n, seed):
    return [(RandomBot if kind == "random" else GreedyBot)(
        random.Random(seed * 131 + i)) for i in range(n)]


def _play(n, kind, seed):
    return game.play_game(_bots(kind, n, seed), n, seed=seed)


def cell(kind, n, warmup_s, measure_s, ramp=False):
    """Play games until `warmup_s` CPU-seconds are burnt, then measure."""
    seed = 10_000
    ramp_trace = []
    t0 = time.process_time()
    mark, games_at_mark = t0, 0
    played = 0
    while True:
        el = time.process_time() - t0
        if el >= warmup_s:
            break
        _play(n, kind, seed)
        seed += 1
        played += 1
        if ramp and time.process_time() - mark >= 1.0:
            now = time.process_time()
            ramp_trace.append(round((played - games_at_mark) / (now - mark), 3))
            mark, games_at_mark = now, played

    seed = 0
    games = moves = 0
    t1 = time.process_time()
    while time.process_time() - t1 < measure_s:
        st = _play(n, kind, seed)
        seed += 1
        games += 1
        moves += getattr(st, "moves_played", 0)
    dt = time.process_time() - t1
    return {"kind": kind, "players": n, "warmup_s": warmup_s,
            "warmup_games": played, "games": games, "cpu_s": round(dt, 3),
            "games_per_cpu_s": round(games / dt, 4),
            "moves_per_cpu_s": round(moves / dt, 1),
            "ramp_games_per_s": ramp_trace}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--warmup", type=float, default=8.0)
    ap.add_argument("--measure", type=float, default=12.0)
    ap.add_argument("--kinds", default="random,greedy")
    ap.add_argument("--players", default="2,3,4")
    ap.add_argument("--ramp", action="store_true")
    ap.add_argument("--json")
    a = ap.parse_args()
    rows = []
    for kind in a.kinds.split(","):
        for n in (int(x) for x in a.players.split(",")):
            r = cell(kind, n, a.warmup, a.measure, a.ramp)
            rows.append(r)
            print(f"{kind:7s} {n}p  {r['games_per_cpu_s']:8.3f} games/cpu-s  "
                  f"{r['moves_per_cpu_s']:10.0f} moves/cpu-s  "
                  f"(warm {r['warmup_games']}g, meas {r['games']}g)", flush=True)
            if r["ramp_games_per_s"]:
                print("        ramp:", r["ramp_games_per_s"], flush=True)
    out = {"impl": platform.python_implementation(),
           "version": platform.python_version(), "rows": rows}
    if a.json:
        Path(a.json).write_text(json.dumps(out, indent=1))
    return 0


if __name__ == "__main__":
    sys.exit(main())
