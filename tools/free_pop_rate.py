"""What is one free population increase per turn actually worth?

Ocean Liners' whole card is `freePopIncreasePerTurn: True` -- no number at
all -- so `engine/bots/board_yields.py:_free_pop_increase` has to supply one, and
the constant it supplies (`FREE_POP_UTIL`) is the only hand-chosen number in
the board-aware pricing work.  This tool is what it was chosen from, so that
it is a measurement rather than an opinion.

    python3 -m tools.free_pop_rate --champ analysis/frozen/champion_2p.json \
        --players 2 --games 10

Three numbers, from the same self-play games:

  U_paid   the fraction of player-turns on which the bot spends a civil
           action and the food to increase population ANYWAY.  On those
           turns, and only those, Ocean Liners is a pure refund of exactly
           what the handler prices: one civil action and `pop_cost` food.

  want     the fraction of player-turns on which a FREE population increase
           would strictly improve the bot's own evaluation.  Much larger than
           U_paid, and the gap is the part of the card the refund model does
           not credit: increases the bot would take if they were free and
           does not take because they are not.

  gain     the mean, over every player-turn, of the evaluation improvement a
           free population increase would give, clipped below at zero because
           the card says "you MAY".  This is the quantity the handler is
           trying to approximate, measured directly and end-to-end.

The check that matters is the last line: the refund model's value, computed
under the champion's OWN weights across the plausible range of `pop_cost`,
against `gain`.  If those two are far apart the constant is wrong.

At `analysis/frozen/champion_2p.json`, 2p, 10 games / 410 player-turns:
U_paid 0.132, want 0.654, gain 0.646 pts/turn, refund model 0.51-0.98 -- so
the refund at U_paid brackets the measured value, and the far larger `want`
is the headroom deliberately left on the table (see the handler's docstring:
over-pricing a wonder is the specific bias docs/SCORE_VALIDATION.md 6.2
warns about).
"""
from __future__ import annotations

import argparse
import os
import random
import statistics
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine import actions as A, economy, effects, game as G   # noqa: E402
from engine.bots import board_yields as BY, weighted as W      # noqa: E402
from engine.bots.fastcopy import copy_state                    # noqa: E402


def measure(w, players=2, games=10, cap=20000):
    from engine.bots import WeightedBot
    gains, turns, paid, want = [], 0, 0, 0
    for seed in range(games):
        st = G.new_game(players, seed * 7919 + 17)
        rng = random.Random(seed)
        bots = [WeightedBot(weights=w, seed=seed * 97 + i)
                for i in range(players)]
        seen = set()
        for _ in range(cap):
            if st.game_over:
                break
            d = st.decider()
            mv = bots[d].pick(st, A.legal_moves(st))
            if mv[0] == "pop":
                paid += 1
            # one probe per player-turn, and not inside a pending decision
            # (the evaluation there is about somebody else's choice)
            if (d, st.turn) not in seen and not st.pending:
                seen.add((d, st.turn))
                turns += 1
                gains.append(_probe(st, d, w))
                want += 1 if gains[-1] > 0 else 0
            A.apply(st, mv, rng)
    return {
        "player_turns": turns,
        "U_paid": paid / max(1, turns),
        "want": want / max(1, turns),
        "gain": statistics.mean(gains) if gains else 0.0,
    }


def _probe(st, d, w):
    """Evaluation gain of a free population increase, clipped at zero."""
    if st.players[d].yellow_bank <= 0:
        return 0.0
    base = W.evaluate(st, d, w)
    tr = copy_state(st)
    if not economy.increase_population(tr, tr.players[d], free=True):
        return 0.0
    effects.invalidate(tr, tr.players[d])
    return max(0.0, W.evaluate(tr, d, w) - base)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--champ", default="analysis/frozen/champion_2p.json")
    ap.add_argument("--players", type=int, default=2, choices=(2, 3, 4))
    ap.add_argument("--games", type=int, default=10)
    a = ap.parse_args(argv)
    w = W.load_weights(a.champ)
    r = measure(w, a.players, a.games)
    print(f"player_turns  {r['player_turns']}")
    print(f"U_paid        {r['U_paid']:.3f}   "
          f"(engine/bots/board_yields.py:FREE_POP_UTIL = {BY.FREE_POP_UTIL})")
    print(f"want (free)   {r['want']:.3f}")
    print(f"gain          {r['gain']:.3f} eval pts/turn, measured directly")
    lo = hi = None
    for pc in (2, 3, 4, 5):
        v = BY.FREE_POP_UTIL * (w["civil_actions"] + pc * w["food_rate"])
        lo = v if lo is None else min(lo, v)
        hi = v if hi is None else max(hi, v)
    print(f"refund model  {lo:.3f}-{hi:.3f} eval pts/turn over pop_cost 2-5")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
