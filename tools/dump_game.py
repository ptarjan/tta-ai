"""Dump the full log of one (players, bot, seed) game to JSON.

Used to bisect a CPython-vs-PyPy divergence down to the first differing
log line:

    python3 tools/dump_game.py 4 greedy 2 /tmp/g_cpy.json
    pypy3    tools/dump_game.py 4 greedy 2 /tmp/g_pypy.json
    python3 tools/dump_game.py --diff /tmp/g_cpy.json /tmp/g_pypy.json
"""
import json
import os
import random
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from engine import game                      # noqa: E402
from engine.bots import GreedyBot, RandomBot  # noqa: E402


def play(n, kind, seed):
    bots = []
    for i in range(n):
        rng = random.Random(seed * 131 + i)
        bots.append(RandomBot(rng) if kind == "random" else GreedyBot(rng))
    return game.play_game(bots, n, seed=seed)


def main(argv):
    if argv[1] == "--diff":
        a = json.load(open(argv[2]))
        b = json.load(open(argv[3]))
        la, lb = a["log"], b["log"]
        for i in range(max(len(la), len(lb))):
            x = la[i] if i < len(la) else "<end>"
            y = lb[i] if i < len(lb) else "<end>"
            if x != y:
                print(f"first divergence at log index {i} of "
                      f"{len(la)}/{len(lb)}")
                for j in range(max(0, i - 6), i):
                    print(f"  ctx {j}: {la[j]}")
                print(f"  A  {i}: {x}")
                print(f"  B  {i}: {y}")
                return 1
        print("logs identical; scores", a["scores"], b["scores"])
        return 0

    n, kind, seed, out = int(argv[1]), argv[2], int(argv[3]), argv[4]
    st = play(n, kind, seed)
    json.dump({"log": st.log, "scores": game.scores(st),
               "winners": game.winners(st),
               "moves": getattr(st, "moves_played", None)},
              open(out, "w"), indent=1)
    print("wrote", out, "loglen", len(st.log), "scores", game.scores(st))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
