"""How many rounds are left?  Estimator error against replayed ground truth.

`weighted.rounds_left` used to divide the exact count of undealt civil cards by
a FITTED constant, `CARDS_PER_ROUND = {2: 6.29, 3: 6.73, 4: 5.71}`.  That
number is one exact quantity plus one guess: `n * SWEEP[n]` (6 / 6 / 4) cards
are swept and redealt per round by rule, and the remainder is cards players
*took*, which is policy.  The constant was fitted on a card-blind policy and
its own comment conceded that "a much more card-hungry policy would drain the
row faster and this would then run long".

This measures exactly that.  It replays self-play games, records at every
player-turn what each estimator says, and compares against the ground truth the
finished game reveals (`final round - this round + 1`).  Run it under two
policies with different appetites for the row and the fitted constant's error
should move with the appetite while the measured one should not:

    python3 tools/deal_rate.py --games 24 --players 2
    python3 tools/deal_rate.py --games 24 --players 2 --policy shy
    python3 tools/deal_rate.py --games 24 --players 2 --policy hungry

`--policy` picks the card appetite; see `POLICIES`.  Output is one row per
estimator: mean signed error (bias), sd of the error, and mean |error|, in
rounds, over pre-Age-IV decisions only (from Age IV on both estimators are
exact and identical, so including them only dilutes the comparison).
"""
from __future__ import annotations

import argparse
import json
import math
import os
import random
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine import actions as A, game as G                 # noqa: E402
from engine.bots import WeightedBot                        # noqa: E402
from engine.bots import weighted as W                      # noqa: E402

#: Two policies with deliberately different appetites for the card row, so the
#: estimators can be scored against a take rate the fitted constant never saw.
#: `hand_potential` prices the cards actually in hand through the same weight
#: vector, so raising it makes taking attractive; `take_cost_paid` charges the
#: civil actions a take costs, so a large negative makes it unattractive.
POLICIES = {
    "default": {},
    "hungry": {"take_cost_paid": 4.0, "hand_potential": 0.5},
    "shy": {"take_cost_paid": -6.0, "hand_value": -1.0},
}


def _snapshot(st):
    """Everything both estimators read, plus the round it was read on."""
    n = W._live(st)
    W.LEGACY_DEAL_RATE = True
    legacy = W.rounds_left(st, n)
    W.LEGACY_DEAL_RATE = False
    measured = W.rounds_left(st, n)
    return {"round": st.round, "n": n, "age": st.age_civil,
            "final_set": st.final_round_end is not None,
            "legacy": legacy, "measured": measured,
            "take_rate": W.take_rate(st, n),
            "unseen": W.cards_unseen(st, n)}


def play(weights, players, games, seed0, max_plies=6000):
    rows = []
    takes = rounds = 0
    for g in range(games):
        seed = seed0 + g
        st = G.new_game(players, seed)
        rng = random.Random(seed)
        bots = [WeightedBot(weights=weights, seed=seed + i)
                for i in range(players)]
        seen_turn = -1
        recs = []
        for _ in range(max_plies):
            if st.game_over:
                break
            if st.turn != seen_turn:
                seen_turn = st.turn
                recs.append(_snapshot(st))
            mv = bots[st.decider()].pick(st, A.legal_moves(st))
            if isinstance(mv, tuple) and mv and mv[0] == "take":
                takes += 1
            A.apply(st, mv, rng)
        # THE LAST ROUND ACTUALLY PLAYED, which is not `st.round`: `game.
        # _advance_turn` increments the round FIRST and only then notices it
        # has passed `final_round_end`, so a finished game sits one round past
        # its own end.  Using `st.round` overstates the ground truth by
        # exactly 1 and flatters every estimator by exactly 1 -- it made the
        # measured estimator look 0.45 rounds pessimistic when it is 0.55
        # optimistic.  `final_round_end` is the rule's own answer (12.3).
        last = st.final_round_end if st.final_round_end is not None else st.round
        rounds += last
        for r in recs:
            r["truth"] = float(last - r["round"] + 1)
            rows.append(r)
    return rows, {"games": games, "rounds": rounds, "takes": takes,
                  "takes_per_round": takes / max(1, rounds)}


def score(rows, key, pre_age_iv=True):
    errs = [r[key] - r["truth"] for r in rows
            if not (pre_age_iv and r["final_set"])]
    if not errs:
        return None
    m = sum(errs) / len(errs)
    var = sum((e - m) ** 2 for e in errs) / max(1, len(errs) - 1)
    return {"n": len(errs), "bias": m, "sd": math.sqrt(var),
            "mae": sum(abs(e) for e in errs) / len(errs)}


def by_age(rows, key):
    """Signed error by civil age -- where an estimator is wrong matters."""
    out = {}
    for r in rows:
        if r["final_set"]:
            continue
        out.setdefault(r["age"], []).append(r[key] - r["truth"])
    return {a: (len(v), sum(v) / len(v)) for a, v in sorted(out.items())}


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--games", type=int, default=24)
    ap.add_argument("--players", type=int, default=2)
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--policy", default="default", choices=sorted(POLICIES))
    ap.add_argument("--json", action="store_true")
    a = ap.parse_args(argv)

    w = dict(W.DEFAULT_WEIGHTS)
    w.update(POLICIES[a.policy])
    rows, meta = play(w, a.players, a.games, a.seed)
    out = {"policy": a.policy, "players": a.players, **meta,
           "legacy": score(rows, "legacy"),
           "measured": score(rows, "measured"),
           "mean_take_rate_seen": (sum(r["take_rate"] for r in rows)
                                   / max(1, len(rows)))}
    if a.json:
        print(json.dumps(out))
        return 0
    print(f"policy={a.policy} players={a.players} games={meta['games']} "
          f"rounds={meta['rounds']} takes/round={meta['takes_per_round']:.2f}")
    for k in ("legacy", "measured"):
        s = out[k]
        print(f"  {k:9s} n={s['n']:5d}  bias={s['bias']:+.3f}  "
              f"sd={s['sd']:.3f}  mae={s['mae']:.3f}   "
              + "  ".join(f"{a}{e:+.2f}" for a, (_, e)
                          in by_age(rows, k).items()))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
