"""How much does the hidden-information leak actually change the bot's move?

`tools/infoleak.py`'s default mode shows 94.9% of `end_turn` candidates draw a
card.  That is a draw count, not a leak measurement by itself -- it is
identical whether or not the root is determinized, and it is measured on
`WeightedBot`, not on `PlanBot`'s beam (which determinizes and has since it
was written; see `engine/bots/plan.py`).  `WeightedBot` itself never
determinizes at all, though, so its draws really are of the true next card
(`tools/infoleak.py --true-card` puts it at 100.0%, docs/AGGRESSION_RATE.md
§9a.1) -- that is the leak this script measures the impact of, by comparing:

  cheat  -- the bot's actual pick (trial applies read the true deck order)
  det    -- the same pick after re-shuffling the unseen decks, averaged over
            K determinizations (the honest, ISMCTS-style evaluation)

Reported: disagreement rate on the chosen move, and the eval delta on the
`end_turn` candidate specifically (cheat score minus mean determinized score),
which is the term `end_turn_bias` has been fighting.

CAVEAT, added 2026-07-30: `determinize` below only re-shuffles `civil_deck`
and `military_deck`, never `current_events` -- the pile that turned out to be
the beam's real leak (docs/AGGRESSION_RATE.md §9a).  So the disagreement rate
this script reports never exercises that component and is a lower bound.

    nice -n 15 python3 tools/leak_impact.py --players 2 --games 12 --k 8
"""
from __future__ import annotations

import argparse
import math
import os
import random
import statistics
import sys

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

from engine import actions, game  # noqa: E402
from engine.bots import WeightedBot  # noqa: E402
from engine.bots.fastcopy import copy_state  # noqa: E402
from engine.bots.trial import fresh_trial_rng  # noqa: E402
from engine.bots.weighted import evaluate, load_weights, rival_context  # noqa: E402


def score_all(st, moves, idx, w, ctx, end_bias):
    """Score every candidate on the state as given."""
    out = {}
    for mv in moves:
        t = copy_state(st)
        try:
            actions.apply(t, mv, fresh_trial_rng())
            v = evaluate(t, idx, w, ctx)
        except Exception:
            continue
        if mv[0] == "end_turn":
            v += end_bias
        out[mv] = v
    return out


def determinize(st, rng):
    """Re-shuffle the parts of the state the mover cannot see.

    Observable: the card row, everyone's board, everyone's culture/science.
    Hidden:     the order of `civil_deck` and `military_deck`, and every
                other player's `hand_military`.
    """
    rng.shuffle(st.civil_deck)
    rng.shuffle(st.military_deck)
    return st


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--players", type=int, default=2)
    ap.add_argument("--games", type=int, default=12)
    ap.add_argument("--k", type=int, default=8, help="determinizations")
    ap.add_argument("--weights", default=None)
    args = ap.parse_args()
    w = load_weights(args.weights) if args.weights else None

    agree = dis = 0
    deltas = []
    et_cheat, et_det, et_spread = [], [], []
    for g in range(args.games):
        rng = random.Random(4100 + g)
        st = game.new_game(args.players, 4100 + g)
        bots = [WeightedBot(weights=w, seed=1) for _ in range(args.players)]
        n = 0
        while not game.is_over(st):
            moves = actions.legal_moves(st)
            if len(moves) > 1 and (st.civil_deck or st.military_deck):
                idx = st.decider()
                try:
                    ctx = rival_context(st, idx)
                except Exception:
                    ctx = {"rival_culture_rate": 0, "rival_science_rate": 0,
                           "rival_strength": 0}
                ww = bots[idx].weights
                eb = ww.get("end_turn_bias", 0.0)
                cheat = score_all(st, moves, idx, ww, ctx, eb)
                if cheat:
                    cheat_pick = max(cheat, key=cheat.get)
                    acc = {mv: 0.0 for mv in cheat}
                    dr = random.Random(g * 7919 + n)
                    per_k_et = []
                    for _k in range(args.k):
                        d = copy_state(st)
                        determinize(d, dr)
                        s = score_all(d, moves, idx, ww, ctx, eb)
                        for mv, v in s.items():
                            if mv in acc:
                                acc[mv] += v / args.k
                        for mv, v in s.items():
                            if mv[0] == "end_turn":
                                per_k_et.append(v)
                    det_pick = max(acc, key=acc.get)
                    if det_pick == cheat_pick:
                        agree += 1
                    else:
                        dis += 1
                        deltas.append(acc[cheat_pick] - acc[det_pick])
                    for mv in cheat:
                        if mv[0] == "end_turn" and per_k_et:
                            et_cheat.append(cheat[mv])
                            et_det.append(acc[mv])
                            et_spread.append(
                                statistics.pstdev(per_k_et) if len(per_k_et) > 1 else 0.0)
            p = game.current_player(st)
            st = game.apply(st, bots[p].choose(st, moves, rng), rng)
            n += 1
            if n > 100000:
                break

    tot = agree + dis
    if not tot:
        print("no comparable decisions")
        return
    p = dis / tot
    ci = 1.96 * math.sqrt(p * (1 - p) / tot)
    print(f"== {args.players}p, {args.games} games, K={args.k} determinizations")
    print(f"comparable decisions: {tot}")
    print(f"MOVE CHANGED by honest determinization: {dis} = {100*p:.2f}% "
          f"+/- {100*ci:.2f}%")
    if deltas:
        print(f"  mean honest-eval loss of the cheating pick: "
              f"{statistics.mean(deltas):.3f}")
    if et_cheat:
        d = [c - m for c, m in zip(et_cheat, et_det)]
        print(f"end_turn candidates compared: {len(et_cheat)}")
        print(f"  cheat score - determinized mean: mean {statistics.mean(d):+.3f}, "
              f"sd {statistics.pstdev(d):.3f}")
        print(f"  within-decision sd across determinizations: "
              f"{statistics.mean(et_spread):.3f}")


if __name__ == "__main__":
    main()
