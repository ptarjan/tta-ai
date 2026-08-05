"""Throughput benchmark: full games, uniformly-random LEGAL moves, single
threaded.  The Python-side counterpart to `rust/tests/bench_playout.rs` --
read that file's module doc comment first for why a "filtered" mode exists.

Two bot modes:

  filtered    Mirrors `rust/tests/common/mod.rs`'s `blocked_on`, which now
              blocks nothing but `resign` (kept out on purpose -- it would
              end the game early). This is the apples-to-apples number
              against the Rust bench: both sides play the same restricted
              move space, so the comparison is of engine speed, not of how
              much of the ruleset is implemented.

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


# ------------------------------------------------------- the shared filter
#
# THE LAST HAND-MIRRORED COPY.  The Rust side used to carry two copies of
# this filter, in `rust/tests/random_game.rs` and `rust/tests/
# bench_playout.rs`, and they drifted: the test unblocked events, pacts,
# aggression and the colonization responses as those modules landed, and the
# bench kept blocking them, so the benchmark was timing a smaller game than
# the suite played.  Those two are now one file, `rust/tests/common/mod.rs`.
# This one cannot share that code -- it filters Python's move list, not
# Rust's -- so it is kept in exact correspondence with `common::blocked_on`
# BY HAND.  If that function changes, change this.  It goes away when the
# Python engine does.

def _blocked(move):
    tag = move[0]
    # This list is now ONE entry long, `resign`, which is not a gap: see
    # `rust/tests/common/mod.rs::blocked_on`'s doc comment for why it stays
    # excluded on purpose (it would end the game early). Everything else
    # that used to be here came off on 2026-08-05: `offer_pact`,
    # `aggression` and `prepare_event`, plus the responses only they can
    # open (`bid`/`bid_pass`/`defend`/`defend_done`/`send_unit`/
    # `send_bonus`/`send_done`), when `interact::push_choice`/
    # `start_defense` and `events::reveal_current_event` landed on the Rust
    # side; `wonder_step` on Hollywood/Internet, when `effects::
    # building_output` landed; `play_action` on the `freeCivilAction`/
    # `gainFoodOrResources`/per-player-count cards, when `rust/src/apply.rs`
    # lost its last `unimplemented!`; and finally `war` on "War over
    # Technology", once `interact::war_tech_spoils` offered its spoils
    # choice through `ChoiceKind::WarTech` and `rust/tests/common/mod.rs`
    # confirmed random play at 2/3/4p reaches and resolves it.
    if tag == "resign":
        return True
    return False


class FilteredRandomBot:
    """Uniform-random over the same restricted move space the Rust
    random-game test driver plays.  NOT `engine.bots.RandomBot`: that bot
    excludes nothing at all, and so also plays `resign`, which the Rust
    driver holds out on purpose (`rust/tests/common/mod.rs::blocked_on`).
    """
    name = "filtered_random"

    def __init__(self, rng):
        self.rng = rng

    def __call__(self, state):
        moves = actions.legal_moves(state)
        playable = [m for m in moves if not _blocked(m)]
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
