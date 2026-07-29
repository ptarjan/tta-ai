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
import platform
import random
import sys
import time

from . import game
from .bots import GreedyBot, RandomBot

# (num_players, bot-kind, seed) tuples covered by the fingerprint.
CASES = ([(n, "random", s) for n in (2, 3, 4) for s in range(8)]
         + [(n, "greedy", s) for n in (2, 3, 4) for s in range(3)])


def wide_cases(nrandom=24, ngreedy=10):
    """A bigger fingerprint set, for cross-interpreter verification.

    The default `CASES` is sized to stay a few seconds on CPython so it can be
    run after every optimisation.  `wide_cases()` is the belt-and-braces sweep
    used when signing off a whole interpreter (see docs/PYPY.md).
    """
    return ([(n, "random", s) for n in (2, 3, 4) for s in range(nrandom)]
            + [(n, "greedy", s) for n in (2, 3, 4) for s in range(ngreedy)])


def _bots(kind, n, seed):
    out = []
    for i in range(n):
        rng = random.Random(seed * 131 + i)
        if kind == "random":
            out.append(RandomBot(rng))
        elif kind == "weighted":
            from .bots import WeightedBot
            out.append(WeightedBot(rng=rng))
        elif kind == "quiescent":
            from .bots.quiescent import QuiescentBot
            out.append(QuiescentBot(rng=rng))
        elif kind == "plan":
            from .bots.plan import PlanBot
            # width=2 is what `experiments/run_league.sh` trains
            # (`--candidate-bot plan:width=2`), and a narrow beam is also the
            # interesting case for the prune: at width 8 almost every node
            # survives, at width 2 almost none does.
            out.append(PlanBot(rng=rng, width=PLAN_WIDTH))
        else:
            out.append(GreedyBot(rng))
    return out


# The default fingerprint plays GreedyBot only, so it is *structurally blind*
# to `WeightedBot` (docs/PYPY.md 9.0/9.6 rely on exactly that blindness to
# explain why four master rebases left the digests untouched).  That is fine as
# long as nobody changes WeightedBot -- and section 9.14 does.  These cases
# give WeightedBot a determinism gate of its own; without one, a change to
# `WeightedBot.pick` could not be caught by any digest in this project.
#: 11 seeds x 3 player counts = 33 games, 34 x 3 = 102 -- deliberately the same
#: 33/102 split as the greedy narrow/wide sets, so "the 135-game suite" means
#: the same amount of play whichever bot is searching.
def weighted_cases(nseeds=11):
    return [(n, "weighted", s) for n in (2, 3, 4) for s in range(nseeds)]


#: beam width for the `plan` fingerprint cases (see `_bots`)
PLAN_WIDTH = 2


# The same argument as `weighted_cases`, one bot further on.  `experiments/
# run_league.sh` trains `--candidate-bot plan:width=2` at 2p and
# `--candidate-bot quiescent:levels=1` at 3p/4p, and NEITHER of those bots is
# played by any of the four existing arms -- so before this, no digest in the
# project could catch a change to `PlanBot` or `QuiescentBot`.  That is the
# 9.14 hole exactly, one league re-target later.
#
# Sized by cost, not by symmetry with the 33/102 greedy split: a 2p PlanBot
# game is ~4 cpu-s and a 4p one ~16, against ~0.15 for a greedy game, so these
# sets are games where the others are seeds.  Both sets cover every player
# count in the wide form, because the copy sites they exercise
# (`_beam`/`_one_ply`/`war_value`, `pick`/`_pick`/`war_value`) fire at
# different rates per player count -- `tools/copy_census.py` measures
# `_pick` at 6% of QuiescentBot's copies at 2p and 50% at 4p.
def plan_cases(wide=False):
    if wide:
        return [(n, "plan", s) for n in (2, 3, 4) for s in range(2)]
    return [(2, "plan", 0), (2, "plan", 1), (3, "plan", 0)]


def quiescent_cases(wide=False):
    n = 8 if wide else 3
    return [(p, "quiescent", s) for p in (2, 3, 4) for s in range(n)]


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


def bench(kinds=("random", "greedy"), counts=(2, 3, 4), games=None,
          warmup=None, json_out=None):
    """Throughput in CPU-seconds of THIS process.

    Wall clock is useless here: the hill-climbing agents keep every core of
    this box busy, so `time.process_time` (our own CPU time) is the only
    stable measure.  Games per CPU-second is also the number that matters for
    self-play, which is CPU-bound and parallel.

    `warmup` games are played (and discarded) before the clock starts.  This
    matters enormously on PyPy, whose JIT needs to see the hot loops a few
    hundred thousand times before it compiles them; without it PyPy measures
    as *slower* than CPython.  Warm-up games use seeds 10_000+ so they never
    overlap the measured seeds, and both interpreters see identical work.
    """
    rows = []
    for kind in kinds:
        for n in counts:
            g = games if games else (30 if kind == "random" else 4)
            w = warmup if warmup is not None else max(2, g // 2)
            for s in range(w):
                _play(n, kind, 10_000 + s)
            t0 = time.process_time()
            w0 = time.perf_counter()
            moves = 0
            for s in range(g):
                st = _play(n, kind, s)
                moves += getattr(st, "moves_played", 0)
            dt = time.process_time() - t0
            wall = time.perf_counter() - w0
            rows.append({"kind": kind, "players": n, "games": g,
                         "warmup": w, "cpu_s": dt, "wall_s": wall,
                         "games_per_cpu_s": g / dt, "moves_per_cpu_s": moves / dt,
                         "games_per_wall_s": g / wall})
            print(f"{kind:7s} {n}p  {g/dt:8.2f} games/cpu-s  "
                  f"{moves/dt:10.0f} moves/cpu-s   (wall {g/wall:6.2f} g/s)")
    if json_out:
        with open(json_out, "w") as fh:
            json.dump({"impl": platform.python_implementation(),
                       "version": platform.python_version(),
                       "rows": rows}, fh, indent=1)
    return rows


def _opt(argv, name, default=None, cast=int):
    if name in argv:
        return cast(argv[argv.index(name) + 1])
    return default


def main(argv):
    cmd = argv[1] if len(argv) > 1 else "bench"
    wide = "--wide" in argv
    cases = wide_cases() if wide else CASES
    if "--weighted" in argv:
        cases = weighted_cases(_opt(argv, "--seeds", 34 if wide else 11))
    if "--plan" in argv:
        cases = plan_cases(wide)
    if "--quiescent" in argv:
        cases = quiescent_cases(wide)
    # Positional args = everything that is neither a `--flag` nor the VALUE of
    # one.  The old one-liner (`[a for a in argv[2:] if not a.startswith("--")]`)
    # swallowed flag values, so `bench --kinds weighted` crashed with
    # `int('weighted')` -- the default for --games is evaluated eagerly even
    # when --games is given.  Pre-existing; it only bites once anyone passes
    # --kinds or --players, which 9.14's A/B is the first thing to do.
    _VALUED = ("--games", "--warmup", "--json", "--kinds", "--players", "--seeds")
    pos, skip = [], False
    for a in argv[2:]:
        if skip:
            skip = False
            continue
        if a.startswith("--"):
            skip = a in _VALUED
            continue
        pos.append(a)
    if cmd == "bench":
        bench(games=_opt(argv, "--games", int(pos[0]) if pos else None),
              warmup=_opt(argv, "--warmup"),
              json_out=_opt(argv, "--json", None, str),
              kinds=tuple(_opt(argv, "--kinds", "random,greedy", str).split(",")),
              counts=tuple(int(x) for x in
                           _opt(argv, "--players", "2,3,4", str).split(",")))
    elif cmd == "hash":
        digest, _ = fingerprint(cases, verbose=True)
        print("FINGERPRINT", digest)
    elif cmd == "save":
        digest, per = fingerprint(cases)
        with open(pos[0], "w") as fh:
            json.dump({"digest": digest, "cases": per}, fh, indent=1)
        print("saved", digest, f"({len(cases)} cases)")
    elif cmd == "check":
        with open(pos[0]) as fh:
            want = json.load(fh)
        digest, per = fingerprint([tuple(c["case"]) for c in want["cases"]])
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
