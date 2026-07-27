"""Direct evidence for the "strictly dominated" claim in docs/DEEPER_SEARCH.md.

Section 1 asserts that ``offer_pact`` / ``aggression`` / ``bid`` / ordered
``play_action`` are dominated *under any weight vector* because 1-ply ``apply``
leaves the whole cost and none of the gain in the trial state.  That is an
argument, not a measurement.  This measures it.

At every decision of a self-play game, whenever one of those move kinds is
legal, it records the mover's OWN evaluation of

  * the candidate scored the way ``WeightedBot`` scores it (apply, evaluate),
  * the candidate scored the way ``QuiescentBot`` scores it (apply, resolve the
    pending stack to quiet, evaluate),
  * the best score any OTHER legal candidate got at 1 ply,

and reports how often each scoring rule would have ranked the move first.

    nice -n 15 python3 tools/dominance_probe.py --players 2 --games 20 \
        --weights exp_quiesce/champ_2p.json
"""
from __future__ import annotations

import argparse
import json
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine import actions, game                          # noqa: E402
from engine.bots.fastcopy import copy_state               # noqa: E402
from engine.bots import quiescent as Q                    # noqa: E402
from engine.bots.weighted import (                        # noqa: E402
    DEFAULT_WEIGHTS, WeightedBot, evaluate, load_weights, rival_context)

WATCH = ("offer_pact", "aggression", "bid", "war", "play_action")


def _score(state, mv, idx, w, ctx, quiesce):
    trial = copy_state(state)
    try:
        actions.apply(trial, mv, Q._fresh(3))
    except Exception:
        return None
    c = ctx
    if quiesce and trial.pending:
        Q._resolve(trial, w, w.get("end_turn_bias", 0.0), 0, [600], 12)
        try:
            c = rival_context(trial, idx)
        except Exception:
            pass
    try:
        v = evaluate(trial, idx, w, c)
    except Exception:
        return None
    if quiesce and mv[0] == "war":
        # war pushes nothing onto the stack, so quiescence cannot see it;
        # QuiescentBot.WAR_LOOKAHEAD resolves it through the engine instead.
        wv = Q._war_value(trial, idx, w, c)
        if wv is not None:
            v = wv
    if mv[0] == "end_turn":
        v += w.get("end_turn_bias", 0.0)
    return v


class _Probe:
    """Plays as WeightedBot; records the ranking question at every decision."""

    def __init__(self, w, seed):
        self.inner = WeightedBot(weights=w, seed=seed)
        self.w = w
        self.rows = []

    def choose(self, state, moves, rng=None):
        self._probe(state, moves)
        return self.inner.choose(state, moves, rng)

    def __call__(self, state):
        moves = actions.legal_moves(state)
        self._probe(state, moves)
        return self.inner(state)

    def _probe(self, state, moves):
        watched = [m for m in moves if m[0] in WATCH]
        if not watched or len(moves) < 2:
            return
        idx = state.decider()
        try:
            ctx = rival_context(state, idx)
        except Exception:
            return
        w = self.w
        one = {}
        for mv in moves:
            v = _score(state, mv, idx, w, ctx, False)
            if v is not None:
                one[mv] = v
        if not one:
            return
        for mv in watched:
            if mv not in one:
                continue
            rest = [v for m, v in one.items() if m != mv]
            best_other = max(rest) if rest else float("-inf")
            q = _score(state, mv, idx, w, ctx, True)
            self.rows.append({
                "kind": mv[0],
                "one_ply": one[mv],
                "quiet": q,
                "best_other": best_other,
                "leaves_pending": q is not None and abs(q - one[mv]) > 1e-9,
            })


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--players", type=int, default=2, choices=(2, 3, 4))
    ap.add_argument("--games", type=int, default=20)
    ap.add_argument("--seed", type=int, default=77000)
    ap.add_argument("--weights", default="")
    a = ap.parse_args(argv)

    w = load_weights(a.weights) if a.weights else dict(DEFAULT_WEIGHTS)
    rows = []
    for g in range(a.games):
        bots = [_Probe(w, 1000 + i) for i in range(a.players)]
        game.play_game(bots, num_players=a.players,
                       seed=(a.seed + g) * 7919 + 17, move_cap=20000)
        for b in bots:
            rows.extend(b.rows)

    out = {}
    for kind in WATCH:
        sub = [r for r in rows if r["kind"] == kind]
        if not sub:
            continue
        pend = [r for r in sub if r["leaves_pending"]]
        wins1 = sum(1 for r in sub if r["one_ply"] > r["best_other"])
        winsq = sum(1 for r in sub
                    if r["quiet"] is not None and r["quiet"] > r["best_other"])
        gap1 = sum(r["one_ply"] - r["best_other"] for r in sub) / len(sub)
        gapq = sum((r["quiet"] if r["quiet"] is not None else r["one_ply"])
                   - r["best_other"] for r in sub) / len(sub)
        out[kind] = {
            "legal_at_n_decisions": len(sub),
            "leaves_pending_frac": round(len(pend) / len(sub), 3),
            "ranked_first_1ply": wins1,
            "ranked_first_quiet": winsq,
            "mean_gap_to_best_other_1ply": round(gap1, 2),
            "mean_gap_to_best_other_quiet": round(gapq, 2),
        }
    print(json.dumps({"players": a.players, "games": a.games,
                      "weights": a.weights or "default", "by_kind": out},
                     indent=1))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
