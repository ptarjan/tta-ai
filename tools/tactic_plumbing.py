"""Which term actually carries a tactic's value into the policy?

The wonder lane showed that repricing 8 wonders moved wonder completions by a
measured zero, because a wonder never enters `hand_civil` and reaches the
policy only through a take-timing heuristic.  Correct pricing on a term the
search does not optimise is worth nothing, and a null measured on top of it
looks exactly like "the price did not matter" when the truth is "the price
never arrived".  So: before believing any tactic result, ask what the
evaluator is actually comparing when a tactic move is on the table.

At every decision where `play_tactic` or `copy_tactic` is legal, this builds
the trial state for every candidate move and reports, per feature, how often
it differs across candidates and its mean `|weight| x range` -- a feature can
only change a decision through that product (the measure
`tools/feature_variance.py` uses).

    python3 tools/tactic_plumbing.py --spec analysis/frozen/champion_2p.json

What it found for the frozen 2p champion, over 374 such decisions: the card
COUNT in the military hand (0.464), the sum of its age levels (0.283) and the
tactic's own age (0.267) outweigh the army strength the tactic actually forms
(0.066) by about 11 to 1.  See docs/CARD_BLINDNESS_MILITARY.md section 5.4.
"""
from __future__ import annotations

import argparse
import collections
import json
import os
import random
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine import actions as A, game as G                 # noqa: E402
from engine.bots import weighted as W                      # noqa: E402
from engine.bots.fastcopy import copy_state                # noqa: E402
from experiments.arena import load_spec, make_bot          # noqa: E402

#: the terms that can plausibly separate one tactic move from another
KEYS = ("strength", "strength_rel", "strength_lead", "tactic_level",
        "hand_military", "hand_mil_value", "ma_left", "military_actions")

#: the move kinds whose decisions we are auditing
TACTIC_MOVES = ("play_tactic", "copy_tactic")


def run(spec, players, games, seed0):
    w = make_bot(load_spec(spec), 1).weights
    varies = collections.Counter()
    contrib = collections.Counter()
    n_dec = 0
    for g in range(games):
        st = G.new_game(players, seed0 + g)
        rng = random.Random(seed0 + g)
        bots = [make_bot(load_spec(spec), 30 + i) for i in range(players)]
        while not st.game_over:
            i = st.decider()
            moves = A.legal_moves(st)
            if len(moves) > 1 and any(m[0] in TACTIC_MOVES for m in moves):
                n_dec += 1
                ctx = W.rival_context(st, i)
                feats = []
                for m in moves:
                    trial = copy_state(st)
                    try:
                        A.apply(trial, m, random.Random(0))
                    except Exception:
                        continue
                    feats.append(W.features(trial, i, ctx))
                for k in KEYS:
                    vs = [f.get(k, 0.0) for f in feats]
                    if vs and max(vs) - min(vs) > 1e-9:
                        varies[k] += 1
                        contrib[k] += (max(vs) - min(vs)) * abs(w.get(k, 0.0))
            A.apply(st, bots[i].pick(st, moves), rng)
    d = float(max(n_dec, 1))
    return {"decisions": n_dec, "games": games, "players": players,
            "per_feature": {k: {"varies": round(varies[k] / d, 4),
                                "w_times_range": round(contrib[k] / d, 4)}
                            for k in KEYS}}


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--spec", required=True)
    ap.add_argument("--players", type=int, default=2, choices=(2, 3, 4))
    ap.add_argument("--games", type=int, default=4)
    ap.add_argument("--seed", type=int, default=900)
    a = ap.parse_args(argv)
    print(json.dumps(run(a.spec, a.players, a.games, a.seed), indent=1))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
