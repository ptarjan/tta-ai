"""Does 1-ply search read cards the player cannot legally know?

`engine/state.py` keeps `civil_deck`, `military_deck` and `current_events` as
full ordered lists inside the state, and `fastcopy.copy_state` copies them
verbatim.  So a trial `apply` that draws a card draws the REAL next card, not a
sample from the distribution.  If that happens often, the bot is searching
against a cheat and every weight trained on top of it is tuned against a cheat.

    nice -n 15 python3 tools/infoleak.py --players 2 --games 20
    nice -n 15 python3 tools/infoleak.py --players 2 --games 8 --true-card

## Two modes, and the difference between them is the whole point

**Default (the original instrument).**  Per root decision, for every candidate
move, apply it on a trial copy and record whether `civil_deck` /
`military_deck` shrank, whether the player's own `hand_military` grew (drawn
cards land there), and whether the trial's `card_row` gained a card that was
not visible at the root.

**`--true-card` (added 2026-07-30, and it is the one to quote).**  The default
mode counts candidates that DRAW.  That is not the same question as "did the
search read the future", and the difference is not academic: **determinization
cannot move the default number at all.**  A determinized root draws exactly as
often; it just draws a different card.  So the headline this file has been
quoted for --

    94.9% of `end_turn` candidates at 2p

-- says only that `end_turn` nearly always draws something.  It was cited in
`engine/bots/plan.py`'s docstring and in `docs/AGGRESSION_RATE.md` 9 as though
it described PlanBot's beam, which determinizes its root and has since it was
written.  It describes `WeightedBot`, which does not.

`--true-card` asks the question that separates the two: was the card the trial
consumed the card that was really on top?  It runs every candidate TWICE, once
from the real state and once from a determinized copy, so the two columns are
the leak and the control side by side on the same decisions.  On an
un-determinized root the answer is 100% -- an identity, not a rate -- and that
is what a leak looks like.  Anything near 1/len(pile) is a sample.

The measurement that found the event leak (2p, 8 games, 18,762 candidates):

    pile              draws   true-top on real root   true-top on det. root
    civil_deck          800            100.0%                   23.6%
    military_deck       639            100.0%                   15.8%
    current_events      209            100.0%                  100.0%   <-- leak

`plan.determinize` shuffled the two decks and never touched the event pile.
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


#: The hidden piles, named once, imported from the production registry so this
#: instrument cannot measure a different set from the one `determinize` fixes.
from engine.bots.plan import HIDDEN_ORDER, determinize  # noqa: E402


def true_card(args, w):
    """Did the trial consume the card that was really on top?  See the header.

    Every candidate is applied twice from the same decision: once from the real
    state and once from a determinized copy of it.  The two columns are the
    leak and its control, on identical decisions, so no seed argument is needed
    to compare them.
    """
    draws = {f: [0, 0] for f in HIDDEN_ORDER}       # field -> [real, det]
    trues = {f: [0, 0] for f in HIDDEN_ORDER}
    dec = cand = 0

    for g in range(args.games):
        rng = random.Random(9000 + g)
        st = game.new_game(args.players, 9000 + g)
        bots = [WeightedBot(weights=w, seed=1) for _ in range(args.players)]
        n = 0
        while not game.is_over(st):
            moves = actions.legal_moves(st)
            if len(moves) > 1:
                dec += 1
                det = copy_state(st)
                determinize(det, random.Random(g * 7919 + n))
                for mv in moves:
                    cand += 1
                    for col, base in ((0, st), (1, det)):
                        t = copy_state(base)
                        try:
                            actions.apply(t, mv, fresh_trial_rng())
                        except Exception:
                            continue
                        for f in HIDDEN_ORDER:
                            before, after = getattr(base, f), getattr(t, f)
                            if len(after) >= len(before):
                                continue
                            draws[f][col] += 1
                            # `pop()` takes from the end, so the cards this
                            # candidate consumed are the tail of `before`.
                            # Compare against the REAL state's top card, which
                            # is what a peek would hand it.
                            eaten = before[len(after):]
                            real_top = getattr(st, f)
                            if real_top and real_top[-1] in eaten:
                                trues[f][col] += 1
            p = game.current_player(st)
            st = game.apply(st, bots[p].choose(st, moves, rng), rng)
            n += 1
            if n > 100000:
                break

    print(f"== TRUE-CARD  {args.players}p, {args.games} games, "
          f"weights={args.weights or 'DEFAULT'}")
    print(f"decisions (>1 move): {dec}   candidates: {cand}")
    print(f"{'pile':16s} {'draws':>8s}  {'true-top REAL root':>20s}  "
          f"{'true-top DET root':>20s}")
    for f in HIDDEN_ORDER:
        d = draws[f][0]
        if not d:
            print(f"{f:16s} {0:8d}  (never drawn in this sample)")
            continue
        a = 100.0 * trues[f][0] / d
        b = 100.0 * trues[f][1] / max(1, draws[f][1])
        print(f"{f:16s} {d:8d}  {a:19.1f}%  {b:19.1f}%")
    print("\nA REAL-root column below 100% would mean the copier stopped "
          "copying;\na DET-root column at 100% is a pile `determinize` is not "
          "shuffling.")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--players", type=int, default=2)
    ap.add_argument("--games", type=int, default=20)
    ap.add_argument("--weights", default=None)
    ap.add_argument("--true-card", action="store_true",
                    help="measure whether the card drawn was the TRUE top "
                         "card, with a determinized control column -- the "
                         "only mode that can tell a leak from a draw")
    args = ap.parse_args()
    w = load_weights(args.weights) if args.weights else None
    if args.true_card:
        return true_card(args, w)

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
