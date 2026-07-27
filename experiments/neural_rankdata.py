"""Generate SIBLING-PREFERENCE data for a learning-to-rank objective.

docs/BOT_ARCHITECTURE.md 3b: a value net fit by Monte-Carlo regression makes a
weak greedy policy because the greedy argmax needs the *difference between
sibling states one action apart*, which squared-error spends ~no capacity on.
docs/NEURAL_EVAL.md's Stage-1 MC result confirmed that survives a nonlinear
card-aware net (0.07 vs the linear champion).  The fix it prescribes is a
pairwise / learning-to-rank objective over the siblings the policy chose
between.  This script produces exactly that data.

At each sampled decision of a STRONG teacher (BookBot, which beats the linear
champion 62.9%), we record, from the mover's perspective:
  * the encoding of the CHOSEN post-move child state, and
  * the encodings of up to K REJECTED post-move child states,
as ranking pairs (chosen should out-value each rejected), PLUS a value row
(pre-move state encoding + the mover's eventual final-culture margin) to keep
the net's outputs calibrated toward the actual objective.

Output npz per shard: Xa (chosen children), Xb (rejected children) -- the
ranking pairs; and Xv (pre-move states), yv (mover final margin) -- the value
anchor.
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
from engine.bots.book import BookBot
from engine.bots.fastcopy import copy_state

_TRIAL_RNG = random.Random(0)


def _child_enc(state, mv, seat):
    t = copy_state(state)
    try:
        actions.apply(t, mv, _TRIAL_RNG)
        return E.encode(t, seat)
    except Exception:
        return None


def _final_margins(state):
    sc = game.scores(state)
    out = []
    for i in range(len(sc)):
        others = [sc[j] for j in range(len(sc)) if j != i]
        out.append(float(sc[i] - (max(others) if others else 0)))
    return out


def play_and_record(n, seed, version, stride, krej, epsilon):
    bots = [BookBot(seed=seed * 97 + i * 13 + 1, version=version)
            for i in range(n)]
    rng = random.Random(seed * 7919 + 17)
    st = game.new_game(n, seed=seed)
    pairs_a, pairs_b = [], []
    vstates, vseat = [], []
    ply = 0
    moves_played = 0
    while not st.game_over and moves_played < 20000:
        moves = actions.legal_moves(st)
        if not moves:
            break
        dec = st.decider()
        live = [m for m in moves if m[0] != "resign"] or moves
        chosen = bots[dec].pick(st, moves) if hasattr(bots[dec], "pick") \
            else bots[dec](st)
        if chosen[0] == "resign" and len(live) < len(moves):
            chosen = bots[dec].pick(st, live)
        if ply % stride == 0 and len(live) > 1:
            # value anchor: pre-move state from the mover's view
            vstates.append(E.encode(st, dec))
            vseat.append(dec)
            # ranking pairs: chosen child vs a sample of rejected children
            ce = _child_enc(st, chosen, dec)
            if ce is not None:
                rejected = [m for m in live if m != chosen]
                rng.shuffle(rejected)
                for mv in rejected[:krej]:
                    re = _child_enc(st, mv, dec)
                    if re is not None:
                        pairs_a.append(ce)
                        pairs_b.append(re)
        # advance (small epsilon for state diversity; label above is still
        # book's preferred move at the visited state)
        mv = rng.choice(live) if (epsilon and rng.random() < epsilon) else chosen
        try:
            actions.apply(st, mv, rng)
        except Exception:
            break
        moves_played += 1
        ply += 1
    margins = _final_margins(st)
    yv = [margins[s] for s in vseat]
    return pairs_a, pairs_b, vstates, yv


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--games", type=int, default=800)
    ap.add_argument("--players", type=int, default=2)
    ap.add_argument("--version", type=int, default=1, help="BookBot version")
    ap.add_argument("--stride", type=int, default=3)
    ap.add_argument("--krej", type=int, default=6, help="rejected siblings/pair")
    ap.add_argument("--epsilon", type=float, default=0.05)
    ap.add_argument("--seed0", type=int, default=0)
    ap.add_argument("--out", default="rankdata/rk")
    ap.add_argument("--shard", type=int, default=120000)
    args = ap.parse_args()

    os.makedirs(os.path.dirname(os.path.abspath(args.out)) or ".", exist_ok=True)
    dim = E.ENCODING_DIM
    Xa = np.empty((args.shard, dim), np.float16)
    Xb = np.empty((args.shard, dim), np.float16)
    Xv = np.empty((args.shard, dim), np.float16)
    yv = np.empty((args.shard,), np.float32)
    pa = pv = 0
    shard = 0
    tot_p = tot_v = 0

    def flush():
        nonlocal pa, pv, shard
        if pa == 0 and pv == 0:
            return
        path = f"{args.out}.{shard:04d}.npz"
        np.savez_compressed(path, Xa=Xa[:pa], Xb=Xb[:pa], Xv=Xv[:pv], yv=yv[:pv])
        print(f"  wrote {path}  pairs={pa} vals={pv}", flush=True)
        shard += 1
        pa = pv = 0

    for g in range(args.games):
        a, b, vs, y = play_and_record(args.players, args.seed0 + g,
                                      args.version, args.stride, args.krej,
                                      args.epsilon)
        for ea, eb in zip(a, b):
            Xa[pa] = ea
            Xb[pa] = eb
            pa += 1
            tot_p += 1
            if pa == args.shard:
                flush()
        for ev, yy in zip(vs, y):
            Xv[pv] = ev
            yv[pv] = yy
            pv += 1
            tot_v += 1
            if pv == args.shard:
                flush()
        if (g + 1) % 50 == 0:
            print(f"game {g+1}/{args.games}  pairs {tot_p}  vals {tot_v}",
                  flush=True)
    flush()
    print(f"DONE games={args.games} pairs={tot_p} vals={tot_v} dim={dim}",
          flush=True)


if __name__ == "__main__":
    main()
