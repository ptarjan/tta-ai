"""Bootstrap data for a BEAM LEAF evaluator, from the repo's strongest bot.

`PlanBot` + the linear champion is the strongest configuration this repo has
measured: culture 189.0 in a 2p mirror and 213.4 against BookBot
(docs/BEHAVIOUR_CLONE.md, docs/LEAGUE_OBJECTIVE.md), against a human mean of
159.5 -- the only configuration on record that clears the human.  The value net
has never been trained on anything that bot ever looks at.

Two distribution bugs in the old pipeline that this file fixes
--------------------------------------------------------------
1. **Wrong states.**  `neural_rankdata.py` / `neural_gen_iter.py` wrote
   *pre-move, mid-turn* encodings.  A beam leaf evaluator is asked about
   *quiet, end-of-my-turn* positions and nothing else.
   docs/BOT_ARCHITECTURE.md 3b: "PlanBot evaluates only at turn boundaries,
   which is exactly the distribution a boundary-only fit is trained on ... the
   fitted vector and PlanBot are a matched pair by construction."  Here the
   value rows ARE the positions the teacher's search scored, taken straight out
   of its `_score`, war-substitution included.
2. **Hidden information in the labels.**  `neural_rankdata.py` encoded children
   from the REAL state, with no determinization at all, so an `end_turn`
   candidate drew the real next card.  (`tools/infoleak.py` measures, on
   `WeightedBot`, that 94.9% of `end_turn` candidates draw a card -- a draw
   count, not by itself proof of a leak, since determinizing the root can't
   move it; but `neural_rankdata.py`'s root was never determinized either, so
   there a draw really is the real next card -- docs/AGGRESSION_RATE.md §9a.)
   The net was therefore taught to price `end_turn` off a card it cannot
   see when it plays, which is noise at play time.  Here every child is applied
   to a DETERMINIZED copy, the same one the teacher searched.

And the target contains the OUTCOME.  docs/BEHAVIOUR_CLONE.md 6.4, after the
human clone scored 7.4 culture at 0.361 move agreement: "cloning move choice
into a value function is a category error ... the objective has to contain the
outcome."  `yv` is the mover's eventual final culture margin.

Output schema is (Xa, Xb, Xv, yv), unchanged, so neural_train_rank.py consumes
it as-is.  Pure CPU, no torch -- run as many workers as you have cores.
"""
from __future__ import annotations

import argparse
import os
import random
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

import numpy as np

from engine import actions, game
from engine.bots import neural_encode as E
from engine.bots.fastcopy import copy_state
from engine.bots.plan import PlanBot, determinize as _determinize
from engine.bots.quiescent import war_value
from engine.bots.weighted import evaluate, load_weights

_TRIAL = random.Random(0)


class TeacherPlanBot(PlanBot):
    """PlanBot that hands back the encodings of the positions it scored."""

    def __init__(self, *a, **kw):
        super().__init__(*a, **kw)
        self.bank = []
        self.collect = False

    def _score(self, t, me, w, ctx):
        if (self.WAR_LOOKAHEAD and not t.game_over
                and t.players[me].war_declared_by_me is not None):
            wv = war_value(t, me, w, ctx)
            if wv is not None:
                self.wars_priced += 1
                if self.collect:
                    # encode what was SCORED: the war-resolved position, the
                    # same substitution NeuralPlanBot._leaf_enc makes.
                    from engine import events
                    scratch = copy_state(t)
                    try:
                        events.resolve_war(scratch, scratch.players[me], None)
                        self.bank.append(E.encode(scratch, me))
                    except Exception:
                        pass
                return wv
        if self.collect:
            try:
                self.bank.append(E.encode(t, me))
            except Exception:
                pass
        return evaluate(t, me, w, ctx)


def _final_margins(state):
    sc = game.scores(state)
    out = []
    for i in range(len(sc)):
        others = [sc[j] for j in range(len(sc)) if j != i]
        out.append(float(sc[i] - (max(others) if others else 0)))
    return out


def play_and_record(weights, n, seed, stride, krej, epsilon, width,
                    leaf_per_decision):
    rng = random.Random(seed * 7919 + 17)
    bots = [TeacherPlanBot(weights=weights, seed=seed * 97 + i * 13 + 1,
                           width=width) for i in range(n)]
    st = game.new_game(n, seed=seed)
    pa, pb, vstates, vseat = [], [], [], []
    ply = moves_played = 0
    while not st.game_over and moves_played < 20000:
        moves = actions.legal_moves(st)
        if not moves:
            break
        dec = st.decider()
        live = [m for m in moves if m[0] != "resign"] or moves
        bot = bots[dec]
        sample = (ply % stride == 0 and len(live) > 1
                  and not st.pending and st.current == dec)
        bot.bank = []
        bot.collect = sample
        chosen = bot.pick(st, live)
        bot.collect = False

        if sample:
            # ranking pairs: the teacher's SEARCH-BACKED root choice against
            # its rejected siblings, children applied to a determinized copy
            # so no candidate is priced against a card the mover cannot see.
            dstate = copy_state(st)
            _determinize(dstate, rng)
            encs, valid = [], []
            for mv in live:
                t = copy_state(dstate)
                try:
                    actions.apply(t, mv, _TRIAL)
                    encs.append(E.encode(t, dec))
                except Exception:
                    continue
                valid.append(mv)
            if encs and chosen in valid:
                ci = valid.index(chosen)
                rej = [j for j in range(len(valid)) if j != ci]
                rng.shuffle(rej)
                for j in rej[:krej]:
                    pa.append(encs[ci])
                    pb.append(encs[j])
            bank = bot.bank
            if bank:
                if len(bank) > leaf_per_decision:
                    bank = rng.sample(bank, leaf_per_decision)
                vstates.extend(bank)
                vseat.extend([dec] * len(bank))

        mv = rng.choice(live) if (epsilon and rng.random() < epsilon) else chosen
        try:
            actions.apply(st, mv, rng)
        except Exception:
            break
        moves_played += 1
        ply += 1
    margins = _final_margins(st)
    return pa, pb, vstates, [margins[s] for s in vseat]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--weights", default="analysis/frozen/champion_2p.json",
                    help="the teacher's linear vector")
    ap.add_argument("--games", type=int, default=60)
    ap.add_argument("--players", type=int, default=2)
    ap.add_argument("--width", type=int, default=8)
    ap.add_argument("--stride", type=int, default=3)
    ap.add_argument("--krej", type=int, default=4)
    ap.add_argument("--leaf-per-decision", type=int, default=12)
    ap.add_argument("--epsilon", type=float, default=0.05,
                    help="exploration on the PLAYED move only, for state "
                         "diversity; the recorded label is always the "
                         "teacher's searched choice at the visited state")
    ap.add_argument("--seed0", type=int, default=0)
    ap.add_argument("--out", default="teacherdata/tg")
    ap.add_argument("--shard", type=int, default=200000)
    args = ap.parse_args()

    w = None if args.weights in ("", "default") else load_weights(args.weights)
    os.makedirs(os.path.dirname(os.path.abspath(args.out)) or ".", exist_ok=True)
    dim = E.ENCODING_DIM
    Xa = np.empty((args.shard, dim), np.float16)
    Xb = np.empty((args.shard, dim), np.float16)
    Xv = np.empty((args.shard, dim), np.float16)
    yv = np.empty((args.shard,), np.float32)
    na = nv = shard = tp = tv = 0

    def flush():
        nonlocal na, nv, shard
        if na == 0 and nv == 0:
            return
        np.savez_compressed(f"{args.out}.{shard:04d}.npz",
                            Xa=Xa[:na], Xb=Xb[:na], Xv=Xv[:nv], yv=yv[:nv])
        shard += 1
        na = nv = 0

    for g in range(args.games):
        a, b, vs, y = play_and_record(w, args.players, args.seed0 + g,
                                      args.stride, args.krej, args.epsilon,
                                      args.width, args.leaf_per_decision)
        for ea, eb in zip(a, b):
            Xa[na] = ea
            Xb[na] = eb
            na += 1
            tp += 1
            if na == args.shard:
                flush()
        for ev, yy in zip(vs, y):
            Xv[nv] = ev
            yv[nv] = yy
            nv += 1
            tv += 1
            if nv == args.shard:
                flush()
        print(f"game {g+1}/{args.games} pairs={tp} vals={tv}", flush=True)
    flush()
    print(f"DONE games={args.games} pairs={tp} vals={tv} dim={dim}", flush=True)


if __name__ == "__main__":
    main()
