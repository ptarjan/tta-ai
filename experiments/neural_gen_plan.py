"""SEARCH-BACKED self-play data for the value net.  Replaces neural_gen_iter.py.

Why this file exists
--------------------
`neural_gen_iter.py` labelled each ranking pair with **the net's own 1-ply
argmax**:

    gi = max(range(len(vals)), key=lambda i: vals[i])   # greedy argmax
    ...
    pa.append(encs[gi]); pb.append(encs[j])

so the training target was a fixed point of the model being trained.  Measured
on the 41-hour run's own shards, before a single gradient step, the incumbent
already ordered **97.6%** of those pairs correctly (docs/NEURAL_LOOP_NULL.md).
The loss therefore carried almost no information and its gradient did only one
thing: inflate the margin between what the net already preferred and everything
else.  Generalised policy iteration with the *identity* as its improvement
operator cannot improve.  74 iterations, zero promotions, candidate win rate
0.4413 pooled over 20,700 gate games.

The fix is the improvement operator AlphaZero uses and this repo has already
measured: **search**.  `engine/bots/neural_plan.py` runs PlanBot's whole-turn
beam with this same net as the leaf, so its root choice is several plies deeper
than the net's own argmax.  On identical LINEAR weights that beam beats the
1-ply bot 88.6% +/- 3.1 (docs/BOT_ARCHITECTURE.md 3).

What gets recorded, and why each label is informative
----------------------------------------------------
Per sampled decision, from the mover's view:

* **Ranking pairs (Xa, Xb)** -- the 1-ply child encoding of the move the BEAM
  chose, against the 1-ply child encodings of the moves it rejected.  The label
  is the beam's, not the net's, so it is not a fixed point: the pairs where the
  two disagree are exactly the gradient.  `DISAGREE=` on the DONE line reports
  that fraction and is the loop's health meter -- if it goes to zero the net has
  absorbed the search and the target has gone vacuous again.  Only decisions
  where beam and 1-ply DISAGREE are written by default (`--all-pairs` keeps the
  rest), because the agreeing ones are the vacuous 97.6% all over again.

* **Value rows (Xv, yv)** -- the encodings of the states the beam ACTUALLY
  SCORES: quiet, end-of-my-turn leaf positions on a determinized state, with
  the war-resolved substitution applied, targeted at the mover's eventual final
  culture margin.  docs/BOT_ARCHITECTURE.md 3b: "PlanBot evaluates only at turn
  boundaries, which is exactly the distribution a boundary-only fit is trained
  on ... the fitted vector and PlanBot are a matched pair by construction."
  `neural_gen_iter.py` instead wrote *pre-move, mid-turn, NON-determinized*
  states -- a distribution the leaf evaluator is never asked about at play time,
  encoded with hidden-information the bot does not have when it plays.

Output schema is unchanged (Xa, Xb, Xv, yv), so neural_train_rank.py consumes
it as-is.  Torch runs on CPU with one thread per worker so many generators can
run in parallel without oversubscribing or touching the GPU.
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
from engine.bots.neural_net import NeuralValue
from engine.bots.neural_plan import NeuralPlanBot
from engine.bots.plan import determinize as _determinize

_TRIAL = random.Random(0)


def _final_margins(state):
    sc = game.scores(state)
    out = []
    for i in range(len(sc)):
        others = [sc[j] for j in range(len(sc)) if j != i]
        out.append(float(sc[i] - (max(others) if others else 0)))
    return out


class _Recorder(NeuralPlanBot):
    """NeuralPlanBot that also hands back the leaf encodings it scored.

    Overriding `_score_many` is the whole hook: every position the beam prices
    passes through it, already war-substituted and already determinized, which
    is precisely the distribution the leaf evaluator is served at play time.
    """

    def __init__(self, *a, **kw):
        super().__init__(*a, **kw)
        self.leaf_bank = []          # encodings scored during the last pick()
        self.collect = False

    def _score_many(self, states, me):
        encs, where = [], []
        for i, t in enumerate(states):
            e = self._leaf_enc(t, me)
            if e is not None:
                encs.append(e)
                where.append(i)
        out = [None] * len(states)
        if not encs:
            return out
        self.evals += len(encs)
        if self.collect:
            self.leaf_bank.extend(encs)
        vals = self.value.value(encs)
        for i, v in zip(where, vals):
            out[i] = v
        return out


def play_and_record(value, n, seed, stride, krej, epsilon, width, nodes,
                    leaf_per_decision, all_pairs):
    rng = random.Random(seed * 7919 + 17)
    bots = [_Recorder(value, seed=seed * 97 + i * 13 + 1, width=width,
                      max_nodes=nodes) for i in range(n)]
    st = game.new_game(n, seed=seed)
    pa, pb = [], []
    vstates, vseat = [], []
    ply = moves_played = 0
    decisions = disagree = 0
    while not st.game_over and moves_played < 20000:
        moves = actions.legal_moves(st)
        if not moves:
            break
        dec = st.decider()
        live = [m for m in moves if m[0] != "resign"] or moves
        bot = bots[dec]
        sample = (ply % stride == 0 and len(live) > 1
                  and not st.pending and st.current == dec)
        bot.leaf_bank = []
        bot.collect = sample
        chosen = bot.pick(st, live)
        bot.collect = False

        if sample:
            # 1-ply children on ONE determinization, shared by the pair labels
            # and by the 1-ply reference choice, so the comparison is honest.
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
                vals = value.value(encs)
                gi = max(range(len(vals)), key=lambda i: vals[i])   # 1-ply pick
                ci = valid.index(chosen)                            # beam pick
                decisions += 1
                if ci != gi:
                    disagree += 1
                if all_pairs or ci != gi:
                    rej = [j for j in range(len(valid)) if j != ci]
                    # the 1-ply argmax is the single most informative negative:
                    # it is what the net would have played and the search says
                    # not to.  Always keep it, then sample the rest.
                    rng.shuffle(rej)
                    if gi != ci and gi in rej:
                        rej.remove(gi)
                        rej.insert(0, gi)
                    for j in rej[:krej]:
                        pa.append(encs[ci])
                        pb.append(encs[j])
            # leaf value rows: subsample what the beam priced this decision
            bank = bot.leaf_bank
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
    yv = [margins[s] for s in vseat]
    return pa, pb, vstates, yv, decisions, disagree


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--ckpt", required=True)
    ap.add_argument("--games", type=int, default=20)
    ap.add_argument("--players", type=int, default=2)
    ap.add_argument("--epsilon", type=float, default=0.08,
                    help="epsilon-greedy exploration on the PLAYED move only; "
                         "the recorded label is always the beam's choice")
    ap.add_argument("--stride", type=int, default=3)
    ap.add_argument("--krej", type=int, default=4)
    ap.add_argument("--width", type=int, default=None)
    ap.add_argument("--nodes", type=int, default=None)
    ap.add_argument("--leaf-per-decision", type=int, default=12,
                    help="value rows kept per searched decision (the beam "
                         "prices ~280 positions; keeping them all would swamp "
                         "the ranking pairs and correlate the batch)")
    ap.add_argument("--all-pairs", action="store_true",
                    help="also write pairs where the beam agrees with the "
                         "1-ply argmax (these are the vacuous ones)")
    ap.add_argument("--seed0", type=int, default=0)
    ap.add_argument("--out", default="iterdata/it")
    ap.add_argument("--shard", type=int, default=200000)
    ap.add_argument("--device", default="cpu")
    ap.add_argument("--threads", type=int, default=1,
                    help="torch CPU threads; 1 so N parallel workers do not "
                         "oversubscribe the box")
    args = ap.parse_args()

    import torch
    if args.threads:
        torch.set_num_threads(args.threads)
    value = NeuralValue.from_checkpoint(args.ckpt, args.device)
    os.makedirs(os.path.dirname(os.path.abspath(args.out)) or ".", exist_ok=True)
    dim = E.ENCODING_DIM
    Xa = np.empty((args.shard, dim), np.float16)
    Xb = np.empty((args.shard, dim), np.float16)
    Xv = np.empty((args.shard, dim), np.float16)
    yv = np.empty((args.shard,), np.float32)
    na = nv = shard = tp = tv = 0
    tot_dec = tot_dis = 0

    def flush():
        nonlocal na, nv, shard
        if na == 0 and nv == 0:
            return
        np.savez_compressed(f"{args.out}.{shard:04d}.npz",
                            Xa=Xa[:na], Xb=Xb[:na], Xv=Xv[:nv], yv=yv[:nv])
        shard += 1
        na = nv = 0

    for g in range(args.games):
        a, b, vs, y, dec, dis = play_and_record(
            value, args.players, args.seed0 + g, args.stride, args.krej,
            args.epsilon, args.width, args.nodes, args.leaf_per_decision,
            args.all_pairs)
        tot_dec += dec
        tot_dis += dis
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
        print(f"game {g+1}/{args.games} pairs={tp} vals={tv} "
              f"disagree={tot_dis}/{tot_dec}", flush=True)
    flush()
    rate = (tot_dis / tot_dec) if tot_dec else 0.0
    # DISAGREE is the health meter: it is the fraction of decisions where the
    # beam overrules the 1-ply argmax, i.e. the fraction of the target that is
    # NOT a fixed point of the model.  A loop whose DISAGREE has decayed to ~0
    # has nothing left to learn from this operator and must be told so loudly.
    print(f"DONE games={args.games} pairs={tp} vals={tv} "
          f"decisions={tot_dec} disagree={tot_dis} DISAGREE={rate:.4f}",
          flush=True)


if __name__ == "__main__":
    main()
