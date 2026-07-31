"""Generate (state, eventual-margin) training data by engine self-play.

Runs the PURE-PYTHON engine (no torch) so it can run anywhere; writes numpy
shards for the GPU trainer.  This is the Stage-1 bootstrap data source
(docs/NEURAL_EVAL.md): unlimited, free, and independent of the human-replay
fidelity problem (docs/SCORE_AUDIT.md) because the labels are the engine's
own final scores.

For every sampled state we emit one row PER active seat: the encoding from that
seat's perspective (`neural_encode.encode`) and that seat's eventual
final-culture MARGIN (its final culture minus the best rival's).  Sampling
every `--stride` plies keeps successive (highly-correlated) within-turn states
from dominating the set, and `--epsilon` mixes in random legal moves so the
data covers states an on-policy bot would never reach -- both matter for a
value net that will be asked to rank off-distribution siblings.

Usage (on the desktop, real python):
    python neural_selfplay.py --games 2000 --players 2 --spec default \
        --epsilon 0.1 --stride 2 --out data/sp2p --shard 50000
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
from experiments import arena


def _final_margins(state):
    sc = game.scores(state)
    out = []
    for i in range(len(sc)):
        others = [sc[j] for j in range(len(sc)) if j != i]
        out.append(float(sc[i] - (max(others) if others else 0)))
    return out


def play_and_record(spec, n, seed, epsilon, stride, move_cap=20000):
    """Play one game; return (list_of_encodings, list_of_perspective_indices,
    list_of_(game_id placeholder)) plus a callback to fill labels once the game
    ends.  We buffer (encoding, seat) and resolve the label at the end."""
    bots = [arena.make_bot(spec, seed * 97 + i * 13 + 1) for i in range(n)]
    rng = random.Random(seed * 7919 + 17)
    st = game.new_game(n, seed=seed)
    rows_enc, rows_seat = [], []
    ply = 0
    moves_played = 0
    while not st.game_over and moves_played < move_cap:
        moves = actions.legal_moves(st)
        if not moves:
            break
        dec = st.decider()
        if ply % stride == 0:
            for s in range(n):
                if not st.players[s].resigned:
                    rows_enc.append(E.encode(st, s))
                    rows_seat.append(s)
        # choose a move: epsilon-random exploration for state coverage
        if epsilon and rng.random() < epsilon:
            live = [m for m in moves if m[0] != "resign"] or moves
            mv = rng.choice(live)
        else:
            mv = bots[dec].pick(st, moves)
        try:
            actions.apply(st, mv, rng)
        except Exception:
            break
        moves_played += 1
        ply += 1
    margins = _final_margins(st)
    ys = [margins[s] for s in rows_seat]
    return rows_enc, ys


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--games", type=int, default=1000)
    ap.add_argument("--players", type=int, default=2)
    ap.add_argument("--spec", default="default",
                    help="arena bot spec for the data policy (default/greedy/"
                         "random/PATH/quiesce:PATH/plan:PATH)")
    ap.add_argument("--epsilon", type=float, default=0.1)
    ap.add_argument("--stride", type=int, default=2)
    ap.add_argument("--seed0", type=int, default=0)
    ap.add_argument("--out", default="data/selfplay")
    ap.add_argument("--shard", type=int, default=50000,
                    help="rows per output shard")
    args = ap.parse_args()

    spec = arena.load_spec(args.spec)
    os.makedirs(os.path.dirname(os.path.abspath(args.out)) or ".", exist_ok=True)

    dim = E.ENCODING_DIM
    Xbuf = np.empty((args.shard, dim), dtype=np.float16)
    Ybuf = np.empty((args.shard,), dtype=np.float32)
    fill = 0
    shard = 0
    total = 0

    def flush():
        nonlocal fill, shard
        if fill == 0:
            return
        path = f"{args.out}.{shard:04d}.npz"
        np.savez_compressed(path, X=Xbuf[:fill], y=Ybuf[:fill])
        print(f"  wrote {path}  ({fill} rows)", flush=True)
        shard += 1
        fill = 0

    for g in range(args.games):
        seed = args.seed0 + g
        encs, ys = play_and_record(spec, args.players, seed,
                                   args.epsilon, args.stride)
        for e, y in zip(encs, ys):
            Xbuf[fill] = e
            Ybuf[fill] = y
            fill += 1
            total += 1
            if fill == args.shard:
                flush()
        if (g + 1) % 50 == 0:
            print(f"game {g + 1}/{args.games}  total rows {total}", flush=True)
    flush()
    print(f"DONE  games={args.games}  rows={total}  dim={dim}", flush=True)


if __name__ == "__main__":
    main()
