"""Head-to-head match machinery shared by evaluate.py and hillclimb.py.

A "duel" puts one challenger (A) at a table of defenders (B) and rotates the
challenger through every seat, so seat order is exactly balanced.  With K
players the null hypothesis is a win rate of 1/K.

Ties share the win: a 2-way tie for first counts 0.5 for each.

Everything is process-parallel (`multiprocessing`): a worker plays one whole
game and returns only the final scores, so nothing large crosses the pipe.
"""
from __future__ import annotations

import json
import math
import multiprocessing as mp
import os
import random
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

BUILTINS = ("random", "greedy", "default")


# ------------------------------------------------------------ bot specs

def load_spec(spec):
    """Turn a CLI bot spec into something picklable.

    ``random`` / ``greedy`` / ``default`` are the built-in bots; anything
    else is a path to a JSON weight file (either a bare dict or a
    ``{"weights": {...}}`` checkpoint).
    """
    if spec in BUILTINS:
        return spec
    from engine.bots.weighted import load_weights
    return load_weights(spec)


def make_bot(spec, seed):
    from engine import bots as B
    if spec == "random":
        return B.RandomBot(seed=seed)
    if spec == "greedy":
        return B.GreedyBot(seed=seed)
    if spec == "default":
        return B.WeightedBot(seed=seed)
    return B.WeightedBot(weights=spec, seed=seed)


def spec_name(spec, fallback):
    return spec if isinstance(spec, str) else fallback


# --------------------------------------------------------------- worker

_W = {}


def _init(a, b, num_players, move_cap):
    _W["a"], _W["b"] = a, b
    _W["n"] = num_players
    _W["cap"] = move_cap


def _play(task):
    """task = (game_index, seed, seat_of_A) -> (share_a, culture_a, culture_b, moves)"""
    from engine import game
    gi, seed, seat = task
    n = _W["n"]
    b = _W["b"]
    if isinstance(b, list):
        # A *field*: the defender seats are drawn from a pool.  The draw is
        # keyed only on `seed`, never on the challenger, so two duels run with
        # the same seeds face byte-identical opposition and can be paired.
        r = random.Random(seed * 31 + 7)
        others = [b[r.randrange(len(b))] for _ in range(n - 1)]
    else:
        others = [b] * (n - 1)
    specs = others[:seat] + [_W["a"]] + others[seat:]
    bots = [make_bot(s, seed * 97 + i * 13 + 1) for i, s in enumerate(specs)]
    try:
        st = game.play_game(bots, n, seed=seed, move_cap=_W["cap"])
        sc = game.scores(st)
        moves = getattr(st, "moves_played", 0)
    except Exception as e:  # engine bug: report, do not kill the tournament
        return (None, repr(e), seed, 0)
    best = max(sc)
    tied = [i for i, v in enumerate(sc) if v == best]
    share = (1.0 / len(tied)) if seat in tied else 0.0
    others = [sc[i] for i in range(n) if i != seat]
    return (share, sc[seat], sum(others) / len(others), moves)


# ----------------------------------------------------------------- stats

def mean_ci(xs, z=1.96):
    """Mean and half-width of a normal-approximation confidence interval."""
    n = len(xs)
    if n == 0:
        return 0.0, 0.0
    m = sum(xs) / n
    if n < 2:
        return m, 1.0
    var = sum((x - m) ** 2 for x in xs) / (n - 1)
    return m, z * math.sqrt(var / n)


def p_value(mean, half, null):
    """Two-sided normal p-value that the observed mean differs from `null`."""
    if half <= 0:
        return 1.0
    se = half / 1.96
    z = abs(mean - null) / se
    return math.erfc(z / math.sqrt(2))


# ------------------------------------------------------------------ duel

def duel(a, b, num_players, games, seed0=0, workers=None, move_cap=20000,
         chunk=4):
    """Play `games` games of A-vs-table-of-B, seat-rotated.

    `b` may be a single spec (every defender seat plays it -- then the null
    win rate really is 1/num_players) or a *list* of specs, a "field", from
    which each defender seat is drawn.  Against a field 1/num_players is no
    longer the right null, so a field duel is only meaningful when compared
    against a second duel run on the same seeds: see `hillclimb.challenge`.

    Returns a dict with the win share, its confidence interval, mean
    cultures and the number of games actually completed.  `per_game` is the
    task-ordered share list (None for a game the engine could not finish),
    which is what makes two duels on the same seeds pairable.
    """
    tasks = []
    for g in range(games):
        seat = g % num_players
        seed = seed0 + g // num_players
        tasks.append((g, seed * 7919 + 17, seat))

    workers = workers or max(1, min(len(os.sched_getaffinity(0))
                                    if hasattr(os, "sched_getaffinity")
                                    else (os.cpu_count() or 2),
                                    (os.cpu_count() or 2)) - 1)
    out = []
    if workers <= 1:
        _init(a, b, num_players, move_cap)
        out = [_play(t) for t in tasks]
    else:
        ctx = mp.get_context("fork" if sys.platform != "win32" else "spawn")
        with ctx.Pool(workers, initializer=_init,
                      initargs=(a, b, num_players, move_cap)) as pool:
            out = pool.map(_play, tasks, chunksize=chunk)

    shares, ca, cb, moves, errors = [], [], [], [], []
    per_game = []                      # task-ordered, None where the game died
    for share, x, y, m in out:
        per_game.append(share)
        if share is None:
            errors.append(x)
            continue
        shares.append(share)
        ca.append(x)
        cb.append(y)
        moves.append(m)
    m, half = mean_ci(shares)
    null = 1.0 / num_players
    return {
        "players": num_players,
        "games": len(shares),
        "requested": games,
        "win_rate": m,
        "ci": half,
        "null": null,
        "p": p_value(m, half, null),
        "culture_a": (sum(ca) / len(ca)) if ca else 0.0,
        "culture_b": (sum(cb) / len(cb)) if cb else 0.0,
        "moves": (sum(moves) / len(moves)) if moves else 0.0,
        "errors": len(errors),
        "error_sample": errors[:3],
        "shares": shares,
        "per_game": per_game,
    }


def fmt(res, name_a="A", name_b="B"):
    return (f"{name_a} vs {name_b} @{res['players']}p: "
            f"win rate {res['win_rate']:.1%} +/- {res['ci']:.1%} "
            f"(null {res['null']:.1%}, p={res['p']:.4f}, n={res['games']}) "
            f"culture {res['culture_a']:.0f} vs {res['culture_b']:.0f}"
            + (f" [{res['errors']} engine errors]" if res["errors"] else ""))
