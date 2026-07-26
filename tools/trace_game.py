"""Trace every move actually applied to the real game state, for
CPython-vs-PyPy divergence bisection.

    python3 tools/trace_game.py 4 greedy 2 /tmp/t_cpy.json
    pypy3    tools/trace_game.py 4 greedy 2 /tmp/t_pypy.json
    python3 tools/trace_game.py --diff /tmp/t_cpy.json /tmp/t_pypy.json

Bots call actions.apply() on *copies* during search, so we only record calls
whose target state is the one real GameState object.
"""
import json
import os
import random
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from engine import actions, game                # noqa: E402
from engine.bots import GreedyBot, RandomBot    # noqa: E402

TRACE = []
REAL = []
_orig_apply = actions.apply


def _traced(state, mv, rng=None, *a, **kw):
    if REAL and state is REAL[0]:
        TRACE.append(repr(mv))
    return _orig_apply(state, mv, rng, *a, **kw)


def main(argv):
    if argv[1] == "--diff":
        a = json.load(open(argv[2]))["trace"]
        b = json.load(open(argv[3]))["trace"]
        for i in range(max(len(a), len(b))):
            x = a[i] if i < len(a) else "<end>"
            y = b[i] if i < len(b) else "<end>"
            if x != y:
                print(f"first differing applied move: index {i} "
                      f"(lens {len(a)}/{len(b)})")
                for j in range(max(0, i - 8), i):
                    print(f"   same {j}: {a[j]}")
                print(f"   A    {i}: {x}")
                print(f"   B    {i}: {y}")
                return 1
        print(f"traces identical ({len(a)} moves)")
        return 0

    n, kind, seed, out = int(argv[1]), argv[2], int(argv[3]), argv[4]
    bots = []
    for i in range(n):
        rng = random.Random(seed * 131 + i)
        bots.append(RandomBot(rng) if kind == "random" else GreedyBot(rng))

    # capture the real state object as soon as play_game creates it
    orig_new = game.new_game

    def spy(*a, **kw):
        st = orig_new(*a, **kw)
        REAL.append(st)
        return st

    game.new_game = spy
    actions.apply = _traced
    # engine modules that imported apply by name
    for mod in list(sys.modules.values()):
        if getattr(mod, "__name__", "").startswith("engine") and \
                getattr(mod, "apply", None) is _orig_apply:
            mod.apply = _traced
    st = game.play_game(bots, n, seed=seed)
    json.dump({"trace": TRACE, "scores": game.scores(st)},
              open(out, "w"), indent=1)
    print("wrote", out, "moves", len(TRACE), "scores", game.scores(st))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
