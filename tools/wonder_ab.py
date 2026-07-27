"""Force a bot to build wonders and see which way its score moves.

`docs/HUMAN_BASELINE.md` proposal 2: humans complete 2.74 wonders a game and
every bot we have completes 0.3-0.8.  Two causes are distinguishable:

(a) wonders are genuinely bad value in our engine (a rules or data bug), or
(b) wonders are fine and a `levels=1` evaluator cannot price a multi-turn
    investment -- the horizon problem `docs/TRANSFER_TEST.md` measures for war.

Under (a) forcing a bot to build wonders makes it WORSE.  Under (b) it makes it
BETTER (or at worst costs little), because the search was leaving value on the
table.  This runs that A/B.

`WonderFirst` wraps a policy and overrides it in exactly two situations:

* a `("wonder_step", k)` move is legal -- take the largest `k`;
* no wonder is in progress and a wonder sits in the card row -- take it, the
  cheapest slot first.

`--force` is the probability of overriding, so `--force 0` is the unmodified
bot and the run is a dose-response curve rather than one point.  Seats are
mirrored across the same seed (the wrapper plays seat 0, then seat 1, on the
same deal), so the reported margin is paired and the deal is the unit of
error.

    nice -n 19 python3 tools/wonder_ab.py --spec /tmp/P2p.json \
        --deals 40 --force 0.25 --force 0.5 --force 1.0
"""
from __future__ import annotations

import argparse
import math
import os
import random
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine import cards, game                                # noqa: E402
from experiments.arena import load_spec, make_bot             # noqa: E402

_DB = cards.db()


class WonderFirst:
    """Wrap a policy; prefer wonder moves with probability `force`."""

    def __init__(self, inner, idx, force, rng):
        self.inner = inner
        self.idx = idx
        self.force = force
        self.rng = rng
        self.overrides = 0

    def _override(self, state, moves):
        if self.force <= 0 or not moves:
            return None
        if self.rng.random() >= self.force:
            return None
        steps = [m for m in moves if m and m[0] == "wonder_step"]
        if steps:
            self.overrides += 1
            return max(steps, key=lambda m: m[1])
        p = state.players[self.idx]
        if getattr(p, "wonder", None) is None:
            takes = [m for m in moves
                     if m and m[0] == "take"
                     and state.card_row[m[1]] in _DB.by_name
                     and _DB.get(state.card_row[m[1]])["type"] == "wonder"]
            if takes:
                self.overrides += 1
                return min(takes, key=lambda m: m[1])
        return None

    def choose(self, state, moves, rng=None):
        mv = self._override(state, moves)
        return mv if mv is not None else self.inner.choose(state, moves, rng)

    def __call__(self, state):
        # The 1-ply/quiescent bots are called as `bot(state)`; regenerate the
        # legal moves the same way the engine does so the override sees them.
        from engine import actions
        moves = actions.legal_moves(state)
        mv = self._override(state, moves)
        return mv if mv is not None else self.inner(state)


def _mean_se(xs):
    n = len(xs)
    if n < 2:
        return (xs[0] if xs else 0.0), 0.0
    m = sum(xs) / n
    var = sum((x - m) ** 2 for x in xs) / (n - 1)
    return m, math.sqrt(var / n)


def run(spec, deals, force, seed0, players=2):
    """Wrapper in every seat in turn; returns per-deal paired records."""
    own, opp, marg, won, wond, stg = [], [], [], [], [], []
    for d in range(deals):
        seed = (seed0 + d) * 7919 + 17
        for seat in range(players):
            rng = random.Random(seed * 31 + seat)
            bots = []
            for i in range(players):
                b = make_bot(spec, 1000 + i)
                bots.append(WonderFirst(b, i, force, rng) if i == seat else b)
            st = game.new_game(players, seed)
            game.play_game(bots, num_players=players, seed=seed,
                           move_cap=20000, state=st)
            sc = list(st.final_scores or [p.culture for p in st.players])
            best_other = max(sc[i] for i in range(players) if i != seat)
            own.append(sc[seat])
            opp.append(best_other)
            marg.append(sc[seat] - best_other)
            won.append(1.0 if sc[seat] >= best_other else 0.0)
            wond.append(len(st.players[seat].completed_wonders))
            stg.append(bots[seat].overrides)
    return own, opp, marg, won, wond, stg


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--spec", required=True)
    ap.add_argument("--deals", type=int, default=30)
    ap.add_argument("--players", type=int, default=2)
    ap.add_argument("--seed", type=int, default=5000)
    ap.add_argument("--force", type=float, action="append", default=[])
    a = ap.parse_args(argv)
    forces = a.force or [0.0, 0.25, 0.5, 1.0]
    spec = load_spec(a.spec)
    print(f"spec={a.spec} players={a.players} deals={a.deals} "
          f"(n = {a.deals * a.players} games per row, seats mirrored)")
    print(f"{'force':>6s} {'own culture':>18s} {'rival':>18s} "
          f"{'margin':>18s} {'winshare':>14s} {'wonders':>13s} {'overrides':>10s}")
    for f in forces:
        own, opp, marg, won, wond, ov = run(spec, a.deals, f, a.seed,
                                            a.players)
        rows = []
        for xs in (own, opp, marg, won, wond):
            m, se = _mean_se(xs)
            rows.append(f"{m:8.1f} +-{se:5.1f}" if m > 1.5
                        else f"{m:8.3f} +-{se:5.3f}")
        print(f"{f:6.2f} {rows[0]:>18s} {rows[1]:>18s} {rows[2]:>18s} "
              f"{rows[3]:>14s} {rows[4]:>13s} {sum(ov)/len(ov):10.1f}")
        sys.stdout.flush()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
