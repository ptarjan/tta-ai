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

#: docs/TRAINING_RUN.md:39-44 -- this file holds the pre-horizon-fix 4p
#: champion (science=-6.089) and says explicitly "never warm-start from it".
#: docs/CULTURE_GAP.md Sec 8f measured it at 20.1% against a 25% null once the
#: turns-remaining horizon fix (`e990920`) landed.  Three tools (quiesce_bench,
#: no_credit_check, behaviour_counts) used to default or example their
#: --weights/--spec argument straight to this file and printed numbers for it
#: without warning.  `refuse_if_degenerate_champion` below is the one place
#: any of them -- or anything written later that loads a spec through
#: `load_spec` -- can be routed through so this cannot recur through a
#: different path.
DEGENERATE_CHAMPION_PATH = os.path.join(
    os.path.dirname(os.path.abspath(__file__)), "champion_4p.json")


def _weights_of(path):
    with open(path) as fh:
        d = json.load(fh)
    return d.get("weights", d)


def _spec_weight_path(spec):
    """Pull the on-disk weights path out of a raw --weights/--spec string,
    unwrapping a ``quiesce:PATH,opt=1,...`` prefix the same way `load_spec`
    does. Returns None for a builtin ('random'/'greedy'/'default') or an
    empty/absent spec -- neither can be the degenerate file."""
    if not spec or spec in BUILTINS:
        return None
    if spec.startswith("quiesce:"):
        spec = spec[len("quiesce:"):].split(",")[0]
    if not spec or spec in BUILTINS:
        return None
    return spec


def refuse_if_degenerate_champion(spec, tool_name):
    """Hard-refuse (``SystemExit``) if `spec` resolves to the known-degenerate
    ``experiments/champion_4p.json`` vector -- checked by PATH and by CONTENT,
    so a copy or rename of the file is still caught. See
    `DEGENERATE_CHAMPION_PATH`'s comment for why this exists. A no-op for
    builtins, empty specs, and any path that isn't that vector."""
    path = _spec_weight_path(spec)
    if path is None or not os.path.exists(path):
        return
    known_path = DEGENERATE_CHAMPION_PATH
    if not os.path.exists(known_path):
        return
    same_path = os.path.samefile(path, known_path)
    same_content = False
    if not same_path:
        try:
            mine = _weights_of(path)
            known = _weights_of(known_path)
            same_content = bool(known) and all(
                mine.get(k) == v for k, v in known.items())
        except (OSError, ValueError):
            same_content = False
    if same_path or same_content:
        sys.stderr.write(
            "\n" + "!" * 70 + "\n"
            f"! {tool_name}: REFUSING to load {path!r}\n"
            "! This is (or byte-matches) experiments/champion_4p.json, the\n"
            "! pre-horizon-fix vector docs/TRAINING_RUN.md says never to\n"
            "! warm-start from (science=-6.089; docs/CULTURE_GAP.md Sec 8f\n"
            "! measured it at 20.1% against a 25% null after the horizon\n"
            "! fix landed). Pass a different --weights/--spec -- e.g. a file\n"
            "! under experiments/league_state/ (the live league champion) --\n"
            "! or omit the flag entirely for DEFAULT_WEIGHTS.\n"
            + "!" * 70 + "\n\n")
        raise SystemExit(
            f"{tool_name}: refusing degenerate weights vector {path!r}")


# ------------------------------------------------------------ bot specs

def load_spec(spec):
    """Turn a CLI bot spec into something picklable.

    ``random`` / ``greedy`` / ``default`` are the built-in bots; anything
    else is a path to a JSON weight file (either a bare dict or a
    ``{"weights": {...}}`` checkpoint).

    A ``quiesce:`` prefix runs the SAME weights under
    :class:`engine.bots.quiescent.QuiescentBot` instead of the 1-ply
    ``WeightedBot``, so ``--a quiesce:experiments/league_state/champion_4p.json
    --b experiments/league_state/champion_4p.json`` is an exact search-only
    A/B.  Optional
    tuning follows the path, comma-separated: ``quiesce:FILE,depth=8,nodes=300,
    war=0``.  The returned spec is a plain tuple/dict, still picklable.
    """
    if spec in BUILTINS:
        return spec
    if spec.startswith("plan:"):
        # `plan:FILE,width=8,samples=1,det=1,war=1` -- whole-turn beam search under
        # the SAME weights, so `--a plan:champ.json --b champ.json` is an
        # exact search-only A/B (engine/bots/plan.py).
        rest = spec[len("plan:"):].split(",")
        path, opts = rest[0], {}
        for kv in rest[1:]:
            if not kv:
                continue
            k, _, v = kv.partition("=")
            opts[k.strip()] = int(v)
        inner = "default" if path in ("", "default") else load_spec(path)
        return ("plan", inner, opts)
    if spec.startswith("quiesce:"):
        rest = spec[len("quiesce:"):].split(",")
        path, opts = rest[0], {}
        for kv in rest[1:]:
            if not kv:
                continue
            k, _, v = kv.partition("=")
            opts[k.strip()] = int(v)
        inner = "default" if path in ("", "default") else load_spec(path)
        return ("quiescent", inner, opts)
    from engine.bots.weighted import load_weights
    return load_weights(spec)


def make_bot(spec, seed):
    from engine import bots as B
    if isinstance(spec, tuple) and spec and spec[0] == "plan":
        from engine.bots.plan import PlanBot
        _, inner, opts = spec
        w = None if inner == "default" else inner
        return PlanBot(weights=w, seed=seed,
                       width=opts.get("width"),
                       samples=opts.get("samples"),
                       determinize=bool(opts.get("det", 1)),
                       war_lookahead=(None if "war" not in opts
                                      else bool(opts["war"])))
    if isinstance(spec, tuple) and spec and spec[0] == "quiescent":
        from engine.bots.quiescent import QuiescentBot
        _, inner, opts = spec
        w = None if inner == "default" else inner
        return QuiescentBot(weights=w, seed=seed,
                            levels=opts.get("levels"),
                            max_depth=opts.get("depth"),
                            max_nodes=opts.get("nodes"),
                            war_lookahead=(None if "war" not in opts
                                           else bool(opts["war"])))
    if spec == "random":
        return B.RandomBot(seed=seed)
    if spec == "greedy":
        return B.GreedyBot(seed=seed)
    if spec == "default":
        return B.WeightedBot(seed=seed)
    return B.WeightedBot(weights=spec, seed=seed)


def spec_name(spec, fallback):
    if isinstance(spec, tuple) and spec and spec[0] in ("quiescent", "plan"):
        return spec[0]
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

    `per_game_margin` is the same list in the same order for the CULTURE
    MARGIN, ``A's final culture - the mean of the defenders'``.  Win share is
    a step function -- against an opponent nobody beats it is 0.0 on every
    game and two policies that lose by 8 and by 90 are indistinguishable --
    whereas the margin is a dense signal that exists on every single game.
    `experiments/hillclimb_league.py` scores its gate tier on it for exactly
    that reason (docs/LEAGUE_TRAINING.md, "The pool is too hard at the
    bottom").  The mean of this list is `culture_a - culture_b`; it is
    returned per game because pairing against a reference duel has to happen
    game by game.

    `per_game_culture` is the same list again for **A's own final culture**,
    which is the quantity you actually win Through the Ages on.  It is not
    derivable from the other two: `per_game_margin` is a DIFFERENCE, and a
    difference cannot tell "I scored 140, they scored 60" from "I scored 80,
    they scored 0".  War and aggression move culture from the victim to the
    attacker, so a stolen point moves the margin by two and a produced point
    by one; `experiments/hillclimb_league.py --objective own|blend` scores on
    this list instead, so theft is paid for exactly once (docs/LEAGUE_OBJECTIVE.md).
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
    per_game_margin = []               # ditto, culture_a - culture_b
    per_game_culture = []              # ditto, culture_a on its own
    for share, x, y, m in out:
        per_game.append(share)
        per_game_margin.append(None if share is None else float(x - y))
        per_game_culture.append(None if share is None else float(x))
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
        "margin": ((sum(ca) / len(ca)) - (sum(cb) / len(cb))) if ca else 0.0,
        "shares": shares,
        "per_game": per_game,
        "per_game_margin": per_game_margin,
        "per_game_culture": per_game_culture,
    }


def fmt(res, name_a="A", name_b="B"):
    return (f"{name_a} vs {name_b} @{res['players']}p: "
            f"win rate {res['win_rate']:.1%} +/- {res['ci']:.1%} "
            f"(null {res['null']:.1%}, p={res['p']:.4f}, n={res['games']}) "
            f"culture {res['culture_a']:.0f} vs {res['culture_b']:.0f}"
            + (f" [{res['errors']} engine errors]" if res["errors"] else ""))
