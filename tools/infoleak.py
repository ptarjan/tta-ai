"""Does 1-ply search read cards the player cannot legally know?

`engine/state.py` keeps `civil_deck` and `military_deck` as full ordered lists
inside the state, and `fastcopy.copy_state` copies them verbatim.  So a trial
`apply` that draws a card draws the REAL next card, not a sample from the
distribution.  If that happens often, the bot is searching against a cheat and
every weight trained on top of it is tuned against a cheat.

This script measures it directly, over real WeightedBot self-play:

  * per root decision, for every candidate move, apply it on a trial copy and
    record whether `civil_deck` / `military_deck` shrank, and whether the
    player's own `hand_military` grew (drawn cards land there);
  * separately, whether the trial's `card_row` gained a card that was not
    visible at the root (row replenishment reveals future civil cards).

    nice -n 15 python3 tools/infoleak.py --players 2 --games 20
"""
from __future__ import annotations

import argparse
import os
import random
import sys
from collections import Counter

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

from engine import actions, game  # noqa: E402
from engine.bots import WeightedBot  # noqa: E402
from engine.bots.fastcopy import copy_state  # noqa: E402
from engine.bots.trial import fresh_trial_rng  # noqa: E402
from engine.bots.weighted import load_weights  # noqa: E402


def row_names(st):
    return [c["name"] if isinstance(c, dict) else getattr(c, "name", c)
            for c in st.card_row if c is not None]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--players", type=int, default=2)
    ap.add_argument("--games", type=int, default=20)
    ap.add_argument("--weights", default=None)
    args = ap.parse_args()
    w = load_weights(args.weights) if args.weights else None

    tally = Counter()
    per_move = Counter()
    per_move_leak = Counter()

    for g in range(args.games):
        rng = random.Random(9000 + g)
        st = game.new_game(args.players, 9000 + g)
        bots = [WeightedBot(weights=w, seed=1) for _ in range(args.players)]
        n = 0
        while not game.is_over(st):
            moves = actions.legal_moves(st)
            if len(moves) > 1:
                tally["decisions"] += 1
                cdeck, mdeck = len(st.civil_deck), len(st.military_deck)
                base_row = set(row_names(st))
                idx = st.decider()
                mh = len(st.players[idx].hand_military)
                leaky_here = False
                for mv in moves:
                    tally["candidates"] += 1
                    per_move[mv[0]] += 1
                    t = copy_state(st)
                    try:
                        actions.apply(t, mv, fresh_trial_rng())
                    except Exception:
                        tally["apply_error"] += 1
                        continue
                    leak = False
                    if len(t.civil_deck) < cdeck:
                        tally["cand_civil_deck_drawn"] += 1
                        leak = True
                    if len(t.military_deck) < mdeck:
                        tally["cand_mil_deck_drawn"] += 1
                        leak = True
                    if len(t.players[idx].hand_military) > mh:
                        tally["cand_own_mil_hand_grew"] += 1
                        leak = True
                    new_row = set(row_names(t)) - base_row
                    if new_row:
                        tally["cand_row_revealed"] += 1
                        leak = True
                    if leak:
                        tally["cand_leaky"] += 1
                        per_move_leak[mv[0]] += 1
                        leaky_here = True
                if leaky_here:
                    tally["decisions_with_a_leaky_candidate"] += 1
            p = game.current_player(st)
            st = game.apply(st, bots[p].choose(st, moves, rng), rng)
            n += 1
            if n > 100000:
                break

    print(f"== {args.players}p, {args.games} games, weights={args.weights or 'DEFAULT'}")
    d, c = tally["decisions"], tally["candidates"]
    print(f"decisions (>1 move): {d}   candidates: {c}")
    for k in ("cand_leaky", "cand_civil_deck_drawn", "cand_mil_deck_drawn",
              "cand_own_mil_hand_grew", "cand_row_revealed", "apply_error"):
        v = tally[k]
        print(f"  {k:28s} {v:7d}  ({100.0*v/max(1,c):.3f}% of candidates)")
    v = tally["decisions_with_a_leaky_candidate"]
    print(f"  decisions with >=1 leaky cand {v:7d}  ({100.0*v/max(1,d):.2f}% of decisions)")
    print("\nleak rate by move kind (leaky / total):")
    for k, tot in per_move.most_common():
        lk = per_move_leak[k]
        if lk:
            print(f"  {k:18s} {lk:6d}/{tot:6d}  {100.0*lk/tot:5.1f}%")


if __name__ == "__main__":
    main()
