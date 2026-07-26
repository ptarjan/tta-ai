"""Determinism + throughput harness for engine optimisation work.

Two jobs, both run from the command line:

    python3 -m engine.perf_check bench          # games/second table
    python3 -m engine.perf_check hash           # determinism fingerprint
    python3 -m engine.perf_check save FILE      # write a fingerprint file
    python3 -m engine.perf_check check FILE     # compare against one

The fingerprint is a SHA-256 over the full game log, the final scores, the
winners and the move count of a fixed set of (player-count, bot, seed) games.
Any optimisation that changes a single byte of a game's behaviour changes the
fingerprint, so `check` is the guard rail for "no behaviour change".
"""
from __future__ import annotations

import hashlib
import json
import random
import sys
import time

from . import game
from .bots import GreedyBot, RandomBot

# (num_players, bot-kind, seed) tuples covered by the fingerprint.
CASES = ([(n, "random", s) for n in (2, 3, 4) for s in range(8)]
         + [(n, "greedy", s) for n in (2, 3, 4) for s in range(3)])


def _bots(kind, n, seed):
    out = []
    for i in range(n):
        rng = random.Random(seed * 131 + i)
        out.append(RandomBot(rng) if kind == "random" else GreedyBot(rng))
    return out


def _play(n, kind, seed):
    return game.play_game(_bots(kind, n, seed), n, seed=seed)


def fingerprint(cases=CASES, verbose=False):
    h = hashlib.sha256()
    per_case = []
    for n, kind, seed in cases:
        st = _play(n, kind, seed)
        blob = json.dumps({
            "log": st.log,
            "scores": game.scores(st),
            "winners": game.winners(st),
            "moves": getattr(st, "moves_played", None),
            "turn": st.turn,
            "round": st.round,
        }, sort_keys=True)
        d = hashlib.sha256(blob.encode()).hexdigest()
        per_case.append({"case": [n, kind, seed], "digest": d})
        h.update(d.encode())
        if verbose:
            print(f"  {n}p {kind:7s} seed={seed}  {d[:16]}  "
                  f"scores={game.scores(st)}")
    return h.hexdigest(), per_case


def bench(kinds=("random", "greedy"), counts=(2, 3, 4), games=None):
    """Throughput in CPU-seconds of THIS process.

    Wall clock is useless here: the hill-climbing agents keep every core of
    this box busy, so `time.process_time` (our own CPU time) is the only
    stable measure.  Games per CPU-second is also the number that matters for
    self-play, which is CPU-bound and parallel.
    """
    rows = []
    for kind in kinds:
        for n in counts:
            g = games if games else (30 if kind == "random" else 4)
            t0 = time.process_time()
            w0 = time.perf_counter()
            moves = 0
            for s in range(g):
                st = _play(n, kind, s)
                moves += getattr(st, "moves_played", 0)
            dt = time.process_time() - t0
            wall = time.perf_counter() - w0
            rows.append((kind, n, g / dt, moves / dt))
            print(f"{kind:7s} {n}p  {g/dt:8.2f} games/cpu-s  "
                  f"{moves/dt:10.0f} moves/cpu-s   (wall {g/wall:6.2f} g/s)")
    return rows


def main(argv):
    cmd = argv[1] if len(argv) > 1 else "bench"
    if cmd == "bench":
        bench(games=int(argv[2]) if len(argv) > 2 else None)
    elif cmd == "hash":
        digest, _ = fingerprint(verbose=True)
        print("FINGERPRINT", digest)
    elif cmd == "save":
        digest, per = fingerprint()
        with open(argv[2], "w") as fh:
            json.dump({"digest": digest, "cases": per}, fh, indent=1)
        print("saved", digest)
    elif cmd == "check":
        with open(argv[2]) as fh:
            want = json.load(fh)
        digest, per = fingerprint()
        if digest == want["digest"]:
            print("OK  identical behaviour:", digest)
            return 0
        print("MISMATCH", digest, "!=", want["digest"])
        old = {tuple(c["case"]): c["digest"] for c in want["cases"]}
        for c in per:
            k = tuple(c["case"])
            if old.get(k) != c["digest"]:
                print("  differs:", k, old.get(k), "->", c["digest"])
        return 1
    else:
        print(__doc__)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
