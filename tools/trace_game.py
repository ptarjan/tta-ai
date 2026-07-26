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
_orig_new = game.new_game


def _traced(state, mv, rng=None, *a, **kw):
    if REAL and state is REAL[0]:
        TRACE.append(repr(mv))
    return _orig_apply(state, mv, rng, *a, **kw)


def probe(n, kind, seed, upto, out):
    """Replay the real game `upto` moves, then dump the decision detail:
    the legal-move list in order and GreedyBot's evaluation of each."""
    from engine.bots import evaluate
    from engine.bots.fastcopy import copy_state
    st = _orig_new(n, seed)
    rng = random.Random(seed ^ 0x5EED)
    bots = []
    for i in range(n):
        r = random.Random(seed * 131 + i)
        bots.append(RandomBot(r) if kind == "random" else GreedyBot(r))
    for _ in range(upto):
        mv = bots[st.decider()](st)
        _orig_apply(st, mv, rng)
    idx = st.decider()
    bot = bots[idx]
    moves = actions.legal_moves(st)
    moves = [m for m in moves if m[0] != "resign"] or moves
    rows = []
    for mv in moves:
        trial = copy_state(st)
        try:
            _orig_apply(trial, mv, random.Random(0))
        except Exception as e:
            rows.append([repr(mv), None, f"{type(e).__name__}: {e}"])
            continue
        val = evaluate(trial, idx, bot.weights)
        if mv[0] == "end_turn":
            val -= 0.01
        rows.append([repr(mv), repr(val), None])
    json.dump({"decider": idx, "rows": rows}, open(out, "w"), indent=1)
    print("probe wrote", out, "decider", idx, "moves", len(rows))


def main(argv):
    if argv[1] == "--probe":
        # --probe N kind seed upto out
        probe(int(argv[2]), argv[3], int(argv[4]), int(argv[5]), argv[6])
        return 0
    if argv[1] == "--pdiff":
        a=json.load(open(argv[2]))["rows"]; b=json.load(open(argv[3]))["rows"]
        print(f"{'move':52s} {'A(cpython)':24s} {'B(pypy)':24s}")
        for x,y in zip(a,b):
            flag = "" if x[1]==y[1] else "   <<< DIFFERS"
            print(f"{x[0]:52s} {str(x[1]):24s} {str(y[1]):24s}{flag}")
        return 0
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


