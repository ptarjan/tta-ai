"""A/B the proposed fix for the wasted-civil-action bug (docs/WASTED_ACTIONS.md).

`WeightedBot` scores `("end_turn",)` on its child state, which has already run
a production phase, so ending the turn is flattered by a whole turn's income.
`PassFixBot` scores it on the *unmoved* board instead — the honest "what is my
position worth if I do nothing else" — plus a small epsilon so an action is
only spent when it strictly improves the position.

Nothing in engine/ is touched; this subclasses the bot.

    python3 analysis/passfix_duel.py --players 2 --games 400 \
        --champion experiments/champion_2p.json --eps -0.05
"""
from __future__ import annotations

import argparse
import multiprocessing as mp
import os
import random
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from engine import actions, game                       # noqa: E402
from engine.bots.fastcopy import copy_state            # noqa: E402
from engine.bots.weighted import (                     # noqa: E402
    WeightedBot, evaluate, load_weights, rival_context)


class PassFixBot(WeightedBot):
    """WeightedBot, but `end_turn` is priced on the board as it stands."""

    name = "passfix"

    def __init__(self, *a, eps=-0.05, **kw):
        super().__init__(*a, **kw)
        self.eps = eps

    def pick(self, state, moves):
        if len(moves) == 1:
            return moves[0]
        idx = state.decider()
        try:
            ctx = rival_context(state, idx)
        except Exception:
            ctx = {"rival_culture_rate": 0, "rival_science_rate": 0,
                   "rival_strength": 0}
        w = self.weights
        best, best_val = None, None
        base = None
        for mv in moves:
            if mv[0] == "end_turn":
                # the honest value of ending the turn: the position unmoved.
                # The production phase in end_turn's child arrives whether or
                # not the action is spent, so it must not be a reason to pass.
                if base is None:
                    try:
                        base = evaluate(state, idx, w, ctx)
                    except Exception:
                        continue
                val = base + self.eps
            else:
                trial = copy_state(state)
                try:
                    actions.apply(trial, mv, random.Random(0))
                    val = evaluate(trial, idx, w, ctx)
                except Exception:
                    continue
            if best_val is None or val > best_val:
                best, best_val = mv, val
        if best is None:
            return self.rng.choice(moves)
        return best


_W = {}


def _init(weights, n, eps):
    _W["w"], _W["n"], _W["eps"] = weights, n, eps


def _play(task):
    seed, seat = task
    n, w, eps = _W["n"], _W["w"], _W["eps"]
    bots = []
    for i in range(n):
        s = seed * 97 + i * 13 + 1
        bots.append(PassFixBot(weights=w, seed=s, eps=eps) if i == seat
                    else WeightedBot(weights=w, seed=s))
    try:
        st = game.play_game(bots, n, seed=seed)
    except Exception as e:
        return (None, repr(e), 0, 0)
    sc = game.scores(st)
    best = max(sc)
    tied = [i for i, v in enumerate(sc) if v == best]
    share = (1.0 / len(tied)) if seat in tied else 0.0
    others = [sc[i] for i in range(n) if i != seat]
    return (share, sc[seat], sum(others) / len(others), 0)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--players", type=int, default=2)
    ap.add_argument("--games", type=int, default=400)
    ap.add_argument("--champion", required=True)
    ap.add_argument("--eps", type=float, default=-0.05)
    ap.add_argument("--workers", type=int, default=0)
    a = ap.parse_args()

    w = load_weights(a.champion)
    tasks = [(s // a.players * 7919 + 17, s % a.players)
             for s in range(a.games)]
    workers = a.workers or max(1, (os.cpu_count() or 2) - 1)
    ctx = mp.get_context("fork")
    with ctx.Pool(workers, initializer=_init,
                  initargs=(w, a.players, a.eps)) as pool:
        out = pool.map(_play, tasks, chunksize=2)

    shares = [o[0] for o in out if o[0] is not None]
    errs = [o[1] for o in out if o[0] is None]
    ca = [o[1] for o in out if o[0] is not None]
    cb = [o[2] for o in out if o[0] is not None]
    n = len(shares)
    m = sum(shares) / n
    var = sum((x - m) ** 2 for x in shares) / max(1, n - 1)
    half = 1.96 * (var / n) ** 0.5
    print(f"passfix(eps={a.eps}) vs champion @{a.players}p: "
          f"win rate {m:.1%} +/- {half:.1%} (null {1/a.players:.1%}, n={n}, "
          f"errors={len(errs)})")
    print(f"  mean culture: passfix {sum(ca)/n:.1f}  champion {sum(cb)/n:.1f}")
    if errs:
        print("  error sample:", errs[:2])


if __name__ == "__main__":
    main()
