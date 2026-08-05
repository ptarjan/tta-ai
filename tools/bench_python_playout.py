"""Throughput benchmark: full games, uniformly-random LEGAL moves, single
threaded.  The Python-side counterpart to `rust/tests/bench_playout.rs` --
read that file's module doc comment first for why a "filtered" mode exists.

Two bot modes:

  filtered    Mirrors `rust/tests/random_game.rs`'s `blocked_on`: skips
              event/pact/aggression moves, War over Technology declarations,
              a few ordered-choice action cards, Hollywood/Internet wonder
              completion, and the pending-decision responses only those
              moves can open (bid/defend/send_*).  This is the
              apples-to-apples number against the Rust bench: both sides
              play the same restricted move space, so the comparison is of
              engine speed, not of how much of the ruleset is implemented.

  unfiltered  `engine.bots.RandomBot` -- the whole ported ruleset (Rust has
              no equivalent yet).  Reported separately, never blended into
              the headline speedup, so the size of the gap the filter opens
              is visible rather than hidden.

Usage:

    nice -n 19 python3.13 -m tools.bench_python_playout \\
        --games 200 --seed0 0 --mode both --players 2,3,4

Timing follows `engine/perf_check.py`'s convention: `time.process_time()`
(this process's own CPU time, immune to the rest of the box's load) is the
primary number, wall clock is reported alongside it as a sanity check.
"""
from __future__ import annotations

import argparse
import statistics
import sys
import time

from engine import game
from engine import actions
from engine.cards import db as _card_db

_DB = _card_db()

# ------------------------------------------------------- the shared filter
#
# Kept in exact correspondence with `rust/tests/random_game.rs`'s
# `blocked_on`/`action_card_is_blocked`.  If that file's filter changes,
# mirror the change here -- see this module's docstring.

_BLOCKED_ACTION_EFFECT_KEYS = (
    "freeCivilAction",
    "gainFoodOrResources",
    "culturePerCivilizationWithMoreCulture",
    "resourcesForMilitaryUnitsPerStrongerCivilization",
)

# Responses to decisions only prepare_event/aggression/offer_pact can open
# (auctions, aggression defense, colonization).  Blocking those three moves
# already makes these unreachable; listed too, so a divergence fails loudly
# (see FilteredRandomBot) instead of silently playing an unfiltered move.
_BLOCKED_PENDING_TAGS = ("bid", "bid_pass", "defend", "defend_done",
                          "send_unit", "send_bonus", "send_done")


def _action_card_blocked(name):
    eff = _DB.get(name).get("effects") or {}
    return any(k in eff for k in _BLOCKED_ACTION_EFFECT_KEYS)


def _blocked(p, move):
    tag = move[0]
    if tag in ("offer_pact", "aggression", "prepare_event", "resign"):
        return True
    if tag == "war" and move[1] == "War over Technology":
        return True
    if tag == "play_action" and _action_card_blocked(move[1]):
        return True
    if tag == "wonder_step" and p.wonder is not None \
            and p.wonder.name in ("Hollywood", "Internet"):
        return True
    if tag in _BLOCKED_PENDING_TAGS:
        return True
    return False


class FilteredRandomBot:
    """Uniform-random over the same restricted move space the Rust
    random-game test driver plays.  NOT `engine.bots.RandomBot`: that bot
    excludes only `resign` and sees every other move -- events, pacts,
    aggression, War over Technology, the blocked action cards -- which is
    exactly the extra work the Rust port does not do yet.
    """
    name = "filtered_random"

    def __init__(self, rng):
        self.rng = rng

    def __call__(self, state):
        moves = actions.legal_moves(state)
        p = state.players[state.decider()]
        playable = [m for m in moves if not _blocked(p, m)]
        if not playable:
            # Fail loudly rather than silently falling back to the full
            # move list -- that would quietly break the apples-to-apples
            # comparison this bot exists for.
            raise RuntimeError(
                f"no playable (unblocked) move: phase={state.phase!r} "
                f"pending={bool(state.pending)} all_moves={moves}")
        return self.rng.choice(playable)


# ------------------------------------------------------------- the bench

def _play(bot_factory, n, seed):
    bots = [bot_factory(i, seed) for i in range(n)]
    return game.play_game(bots, n, seed=seed)


def _filtered_factory(i, seed):
    import random as _random
    return FilteredRandomBot(_random.Random(seed * 131 + i))


def _unfiltered_factory(i, seed):
    import random as _random
    from engine.bots import RandomBot
    return RandomBot(_random.Random(seed * 131 + i))


def mean_std(xs):
    if not xs:
        return 0.0, 0.0
    return statistics.mean(xs), (statistics.pstdev(xs) if len(xs) > 1 else 0.0)


def bench_mode(label, factory, counts, n_games, seed0, warmup):
    rows = []
    for n in counts:
        for s in range(warmup):
            _play(factory, n, 20_000 + seed0 + s)
        plies = []
        t0 = time.process_time()
        w0 = time.perf_counter()
        for s in range(n_games):
            st = _play(factory, n, seed0 + s)
            plies.append(getattr(st, "moves_played", 0))
        cpu_s = time.process_time() - t0
        wall_s = time.perf_counter() - w0
        mean_p, std_p = mean_std(plies)
        total_plies = sum(plies)
        print(f"[{label}] {n}p  games={n_games}  cpu={cpu_s:.3f}s wall={wall_s:.3f}s  "
              f"games/cpu-s={n_games / cpu_s:.3f}  plies/cpu-s={total_plies / cpu_s:.1f}  "
              f"games/wall-s={n_games / wall_s:.3f}  plies/wall-s={total_plies / wall_s:.1f}  "
              f"mean_plies={mean_p:.1f} std_plies={std_p:.1f}")
        rows.append({"mode": label, "players": n, "games": n_games,
                     "cpu_s": cpu_s, "wall_s": wall_s,
                     "games_per_cpu_s": n_games / cpu_s,
                     "plies_per_cpu_s": total_plies / cpu_s,
                     "games_per_wall_s": n_games / wall_s,
                     "plies_per_wall_s": total_plies / wall_s,
                     "mean_plies": mean_p, "std_plies": std_p})
    return rows


def main(argv):
    ap = argparse.ArgumentParser()
    ap.add_argument("--games", type=int, default=200)
    ap.add_argument("--seed0", type=int, default=0)
    ap.add_argument("--warmup", type=int, default=None)
    ap.add_argument("--mode", choices=["filtered", "unfiltered", "both"], default="both")
    ap.add_argument("--players", default="2,3,4")
    args = ap.parse_args(argv)
    counts = tuple(int(x) for x in args.players.split(","))
    warmup = args.warmup if args.warmup is not None else max(2, args.games // 10)

    print(f"python {sys.version.split()[0]}  games={args.games} seed0={args.seed0} "
          f"warmup={warmup} players={counts}", file=sys.stderr)

    if args.mode in ("filtered", "both"):
        bench_mode("filtered", _filtered_factory, counts, args.games, args.seed0, warmup)
    if args.mode in ("unfiltered", "both"):
        bench_mode("unfiltered", _unfiltered_factory, counts, args.games, args.seed0, warmup)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
