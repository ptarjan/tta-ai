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
over-pricing a wonder is the specific bias docs/SCORE_AUDIT.md 10.6.2
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
    """Per player-turn: what a free increase is worth, what the handler says
    it is worth, and which of the board's own gates were open at the time."""
    from engine.bots import WeightedBot
    rows, paid_rows = [], []
    for seed in range(games):
        st = G.new_game(players, seed * 7919 + 17)
        rng = random.Random(seed)
        bots = [WeightedBot(weights=w, seed=seed * 97 + i)
                for i in range(players)]
        seen = set()
        pending_probe = {}
        for _ in range(cap):
            if st.game_over:
                break
            d = st.decider()
            mv = bots[d].pick(st, A.legal_moves(st))
            if (d, st.turn) not in seen and not st.pending:
                seen.add((d, st.turn))
                rows.append(_snapshot(st, d, w))
                pending_probe[(d, st.turn)] = len(rows) - 1
            if mv[0] == "pop":
                # credit the PAY to the probe taken on the same player-turn,
                # so `U_paid` can be conditioned on the same board facts the
                # handler gates on rather than only averaged over everything
                i = pending_probe.get((d, st.turn))
                if i is not None:
                    rows[i]["paid"] = 1
            A.apply(st, mv, rng)
    return rows


def _snapshot(st, d, w):
    """One probed player-turn: the truth, the handler, and the gates."""
    p = st.players[d]
    s = effects.state_stats(st, p)
    req_after = economy.happy_required(max(0, p.yellow_bank - 1))
    priced = BY._free_pop_increase(st, p, "Ocean Liners")
    food = economy.pop_food_cost(s, p.yellow_bank)
    # What the card is worth THIS turn if the player pays for an increase
    # anyway: exactly the civil action and the food they would have spent,
    # priced through their own weights.  Exact for this position -- `food` is
    # the real pop cost here, not a 2-5 range.
    refund = (w.get("civil_actions", 0.0)
              + (food or 0) * w.get("food_rate", 0.0))
    return {
        "paid": 0,
        "refund": refund,
        "gain": _probe(st, d, w),
        "handler": sum(c * w.get(k, 0.0) for k, c, _kind in priced),
        "bank": p.yellow_bank,
        "idle": p.workers_free,
        "stays_happy": 1 if s.happy - req_after >= 0 else 0,
        "can": 0 if economy.pop_food_cost(s, p.yellow_bank) is None else 1,
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


def _rate(rows, key, sub=None):
    xs = [r for r in rows if sub is None or sub(r)]
    if not xs:
        return 0.0, 0
    return sum(r[key] for r in xs) / len(xs), len(xs)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--champ", default="analysis/frozen/champion_2p.json")
    ap.add_argument("--players", type=int, default=2, choices=(2, 3, 4))
    ap.add_argument("--games", type=int, default=10)
    a = ap.parse_args(argv)
    w = W.load_weights(a.champ)
    rows = measure(w, a.players, a.games)
    n = len(rows)
    print(f"weights       {a.champ}   {a.players}p, {a.games} games")
    print(f"player_turns  {n}")
    up, _ = _rate(rows, "paid")
    print(f"U_paid        {up:.3f}   "
          f"(engine/bots/board_yields.py:FREE_POP_UTIL = {BY.FREE_POP_UTIL})")
    want, _ = _rate(rows, "gain", lambda r: True)
    wf = sum(1 for r in rows if r["gain"] > 0) / max(1, n)
    print(f"want (free)   {wf:.3f}")
    print(f"gain          {want:.3f} eval pts/turn, measured directly")

    # THE CHECK THAT DECIDES THE SHAPE, and the target is NOT `gain` alone.
    # `gain` is the value of a FREE WORKER, which is what the card is worth on
    # a turn the player would not have bought one.  On a turn they WOULD have
    # bought one they were getting the worker either way, so the card is worth
    # the refund instead -- the civil action and the food they now keep.  So
    # the marginal value of the card on any given turn is one of two things
    # and the replay knows WHICH, per position:
    #
    #     truth(turn) = refund(turn)  if the player paid for an increase
    #                   gain(turn)    if they did not
    #
    # No constant anywhere in that, which is what makes it a ground truth
    # rather than a second model.  Comparing the handler against `gain` alone
    # (an earlier version of this tool did) scores the refund branch against
    # the wrong quantity and reports a 2.5x over-price that is not real.
    truth = sum(r["refund"] if r["paid"] else r["gain"] for r in rows) / max(1, n)
    hv, _ = _rate(rows, "handler")
    print(f"truth         {truth:.3f} eval pts/turn "
          f"(refund on paid turns, free worker on the rest)")
    print(f"handler       {hv:.3f} eval pts/turn, at the real pop_cost")
    print(f"handler/truth {hv / truth if truth else float('inf'):.2f}x   "
          f"(1.00 = calibrated; >1 over-prices, the bias "
          f"docs/SCORE_AUDIT.md 10.6.2 measured as costly)")

    # ...and the same rates conditioned on the board facts the handler could
    # gate on, which is what turns a flat U into a query.
    print("conditioned on the board:")
    for lab, sub in (("bank empty (worth 0)", lambda r: not r["can"]),
                     ("would go unhappy", lambda r: not r["stays_happy"]),
                     ("has idle workers", lambda r: r["idle"] > 0),
                     ("no idle workers", lambda r: r["idle"] == 0),
                     ("all gates open",
                      lambda r: r["can"] and r["stays_happy"]
                      and r["idle"] == 0)):
        u, k = _rate(rows, "paid", sub)
        g, _ = _rate(rows, "gain", sub)
        print(f"  {lab:<22} n={k:4d} ({k / max(1, n):5.1%})  "
              f"U_paid={u:.3f}  gain={g:.3f}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
