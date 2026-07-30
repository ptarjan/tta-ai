"""How often does one weight actually change the move that gets played?

The cheap, exact version of an A/B, and the right thing to run FIRST.  A
weight can only change the outcome of a game through an argmax: if flipping it
never once wins an argmax, no number of games will resolve an effect, and
`docs/CARD_BLINDNESS.md` section 5.1 wasted 1200 games discovering that about
`wonder_overrun` the expensive way.  Reference play is always `--base`, so the
two arms see the SAME states and the count is a paired, deterministic
divergence rate rather than a noisy win rate.

    python3 tools/argmax_divergence.py --base analysis/laneb/base.json \
        --arm analysis/laneb/terr_0.125.json --games 4

Reports the divergence rate and, when it is non-zero, which move kinds the two
disagree about -- because "changed 3% of decisions" is only interesting
alongside "and they were all `take`".
"""
from __future__ import annotations

import argparse
import json
import os
import random
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine import actions as A, game as G          # noqa: E402
from experiments.arena import load_spec, make_bot   # noqa: E402


def _pick_with(bot, weights, state, moves):
    old = bot.weights
    bot.weights = weights
    try:
        return bot.pick(state, moves)
    finally:
        bot.weights = old


def run(base_spec, arm_spec, players, games, seed0):
    base_w = make_bot(load_spec(base_spec), 1).weights
    arm_w = make_bot(load_spec(arm_spec), 1).weights
    changed = {k: (base_w.get(k), v) for k, v in arm_w.items()
               if base_w.get(k) != v}
    decisions = diverged = 0
    kinds = {}
    for g in range(games):
        st = G.new_game(players, 700 + g)
        rng = random.Random(700 + g)
        bots = [make_bot(load_spec(base_spec), 20 + i) for i in range(players)]
        while not st.game_over:
            i = st.decider()
            moves = A.legal_moves(st)
            ref = _pick_with(bots[i], base_w, st, moves)
            if len(moves) > 1:
                decisions += 1
                alt = _pick_with(bots[i], arm_w, st, moves)
                if alt != ref:
                    diverged += 1
                    k = f"{ref[0]} -> {alt[0]}"
                    kinds[k] = kinds.get(k, 0) + 1
            A.apply(st, ref, rng)
    return {"arm": os.path.basename(arm_spec), "weights_changed": changed,
            "games": games, "decisions": decisions, "diverged": diverged,
            "rate": round(diverged / max(decisions, 1), 5),
            "by_kind": dict(sorted(kinds.items(), key=lambda kv: -kv[1])[:8])}


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--base", required=True)
    ap.add_argument("--arm", required=True)
    ap.add_argument("--players", type=int, default=2, choices=(2, 3, 4))
    ap.add_argument("--games", type=int, default=4)
    ap.add_argument("--seed", type=int, default=0)
    a = ap.parse_args(argv)
    print(json.dumps(run(a.base, a.arm, a.players, a.games, a.seed)))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
