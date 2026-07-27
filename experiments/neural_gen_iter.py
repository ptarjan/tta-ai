"""Self-play data generator for the Stage-2 loop: NEURAL policy, on-policy.

This is the data source for one iteration of the AlphaZero-style loop
(docs/NEURAL_EVAL.md Stage 2). It plays the current best value net in its own
1-ply search and records, per sampled decision from the mover's view:

  * a VALUE row: the pre-move state encoding + the mover's eventual final-culture
    margin (the on-policy return under the current greedy policy -- this is the
    generalized-policy-iteration signal: regressing V toward V^{pi_k} shifts the
    1-ply argmax and improves the policy), and
  * RANKING pairs: the net's own argmax child (chosen) vs a sample of the other
    children (rejected), which keep the value head's SIBLINGS discriminable so
    the greedy policy stays sharp (the Stage-1 MC net lost exactly this).

Exploration decouples behaviour from target: moves are SAMPLED by a softmax over
child values at temperature T (so the games explore), but the ranking "chosen"
label is always the GREEDY argmax (the improvement target). Torch runs on CPU by
default so many gen workers can run in parallel without contending for the GPU,
which is reserved for training.

Output npz per shard: Xa, Xb (ranking pairs), Xv, yv (value rows) -- the same
schema as neural_rankdata.py, so neural_train_rank.py consumes it unchanged.
"""
from __future__ import annotations

import argparse
import math
import os
import random
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

import numpy as np

from engine import actions, game
from engine.bots import neural_encode as E
from engine.bots.fastcopy import copy_state
from engine.bots.plan import determinize as _determinize
from engine.bots.neural_net import NeuralValue

_TRIAL = random.Random(0)


def _children(state, moves, seat, det_state, rng):
    """Encode every candidate's post-move child from `seat`'s view.
    Returns (encs, valid_moves)."""
    encs, valid = [], []
    for mv in moves:
        t = copy_state(det_state)
        try:
            actions.apply(t, mv, _TRIAL)
            encs.append(E.encode(t, seat))
        except Exception:
            continue
        valid.append(mv)
    return encs, valid


def _final_margins(state):
    sc = game.scores(state)
    out = []
    for i in range(len(sc)):
        others = [sc[j] for j in range(len(sc)) if j != i]
        out.append(float(sc[i] - (max(others) if others else 0)))
    return out


def play_and_record(value, n, seed, temp, stride, krej, epsilon=0.0, det=True):
    rng = random.Random(seed * 7919 + 17)
    st = game.new_game(n, seed=seed)
    pa, pb, vstates, vseat = [], [], [], []
    ply = moves_played = 0
    while not st.game_over and moves_played < 20000:
        moves = actions.legal_moves(st)
        if not moves:
            break
        dec = st.decider()
        live = [m for m in moves if m[0] != "resign"] or moves
        # determinize once per decision for an honest, leak-free child eval
        dstate = copy_state(st)
        if det:
            _determinize(dstate, rng)
        encs, valid = _children(st, live, dec, dstate, rng)
        if not encs:
            mv = rng.choice(live)
        else:
            vals = value.value(encs)
            gi = max(range(len(vals)), key=lambda i: vals[i])   # greedy argmax
            if ply % stride == 0 and len(valid) > 1:
                vstates.append(E.encode(st, dec))
                vseat.append(dec)
                rej = [j for j in range(len(valid)) if j != gi]
                rng.shuffle(rej)
                for j in rej[:krej]:
                    pa.append(encs[gi])
                    pb.append(encs[j])
            # exploration: epsilon-greedy (scale-INDEPENDENT, robust to the
            # ranking loss inflating value magnitudes) OR softmax over values.
            # The ranking target above is always the greedy argmax `gi`; only
            # the PLAYED move explores.
            if epsilon > 0 and rng.random() < epsilon and len(valid) > 1:
                ci = rng.randrange(len(valid))
            elif temp <= 0:
                ci = gi
            else:
                m = max(vals)
                w = [math.exp((v - m) / temp) for v in vals]
                tot = sum(w)
                r = rng.random() * tot
                ci = 0
                for k, wk in enumerate(w):
                    r -= wk
                    if r <= 0:
                        ci = k
                        break
            mv = valid[ci]
        try:
            actions.apply(st, mv, rng)
        except Exception:
            break
        moves_played += 1
        ply += 1
    margins = _final_margins(st)
    yv = [margins[s] for s in vseat]
    return pa, pb, vstates, yv


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--ckpt", required=True, help="current best value net")
    ap.add_argument("--games", type=int, default=100)
    ap.add_argument("--players", type=int, default=2)
    ap.add_argument("--temp", type=float, default=0.0,
                    help="softmax exploration temperature over child margins "
                         "(culture units); 0 = greedy (use --epsilon instead)")
    ap.add_argument("--epsilon", type=float, default=0.2,
                    help="epsilon-greedy exploration (scale-independent)")
    ap.add_argument("--stride", type=int, default=3)
    ap.add_argument("--krej", type=int, default=6)
    ap.add_argument("--seed0", type=int, default=0)
    ap.add_argument("--out", default="iterdata/it")
    ap.add_argument("--shard", type=int, default=200000)
    ap.add_argument("--device", default="cpu")
    args = ap.parse_args()

    value = NeuralValue.from_checkpoint(args.ckpt, args.device)
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
        a, b, vs, y = play_and_record(value, args.players, args.seed0 + g,
                                      args.temp, args.stride, args.krej,
                                      args.epsilon)
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
    flush()
    print(f"DONE games={args.games} pairs={tp} vals={tv}", flush=True)


if __name__ == "__main__":
    main()
