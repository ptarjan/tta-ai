"""Generate (design row, outcome) pairs from self-play, for value regression.

Motivation
----------
`experiments/hillclimb_league.py` optimises 82 weights with a signal of roughly
ONE BIT PER BATCH of games (did the challenger win the duel).  A regression
extracts a real-valued target from every state of every game -- ~185 states per
2p game, times one row per player.  That is three to four orders of magnitude
more signal for the same CPU.

The design matrix is *exactly* the parameterisation `weighted.evaluate` already
uses, so a fitted coefficient vector is a drop-in weight file:

    evaluate(s, i, w) = sum_k w[k] * f[k]
                      + sum_{k in PHASE_KEYS} w[k+"_early"] * (1-L) * f[k]
                      + sum_{k in PHASE_KEYS} w[k+"_late"]  *   L   * f[k]
                      + w["hand_potential"] * hand_potential(s, i, w)

The last term is priced *through w itself*, so it is not linear and cannot be
fitted here.  It is held fixed at the reference vector's value and its
contribution is subtracted from the target as an offset, which keeps the rest
of the fit honest.  (`end_turn_bias` is not part of `evaluate` at all -- it is
added by `WeightedBot.pick` to one move kind -- so it is not fitted either.)

Rows are emitted at TURN BOUNDARIES rather than at every decision: consecutive
mid-turn states differ by one action and are massively autocorrelated, so they
inflate n without adding information.

    nice -n 15 python3 tools/gen_value_data.py --players 2 --games 500 \
        --weights experiments/arch_frozen/champ2p_gen344.json \
        --out /tmp/vdata_2p.jsonl --workers 4
"""
from __future__ import annotations

import argparse
import json
import multiprocessing as mp
import os
import random
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine import actions, game  # noqa: E402
from engine.bots.weighted import (DEFAULT_WEIGHTS, PHASE_KEYS,  # noqa: E402
                                  features, hand_potential, lateness,
                                  load_weights, rival_context)

_W = {}


def columns(sample_feat):
    """Stable column order: base features, then _early, then _late."""
    base = sorted(sample_feat)
    return (base + [k + "_early" for k in PHASE_KEYS]
            + [k + "_late" for k in PHASE_KEYS])


def row_for(state, idx, w):
    """The design row and the fixed (non-linear) offset for player `idx`."""
    try:
        ctx = rival_context(state, idx)
    except Exception:
        ctx = {"rival_culture_rate": 0, "rival_science_rate": 0,
               "rival_strength": 0}
    f = features(state, idx, ctx)
    L = lateness(state)
    early = 1.0 - L
    r = dict(f)
    for k in PHASE_KEYS:
        v = f.get(k, 0.0)
        r[k + "_early"] = early * v
        r[k + "_late"] = L * v
    hp = w.get("hand_potential") or 0.0
    off = hp * hand_potential(state, idx, w) if hp else 0.0
    return r, off


def _init(spec, n, mode, every=False):
    _W["w"] = spec
    _W["n"] = n
    _W["mode"] = mode
    _W["every"] = every


def _one(seed):
    from engine.bots.weighted import WeightedBot
    from engine.bots.plan import PlanBot
    w = _W["w"]
    n = _W["n"]
    rng = random.Random(seed)
    st = game.new_game(n, seed)
    if _W["mode"] == "plan":
        bots = [PlanBot(weights=w, seed=seed + i) for i in range(n)]
    else:
        bots = [WeightedBot(weights=w, seed=seed + i) for i in range(n)]
    rows = []
    last_turn = None
    moves = 0
    every = _W.get("every")
    while not game.is_over(st):
        # `--rows turn` samples only turn boundaries: consecutive mid-turn
        # states differ by one action and are massively autocorrelated.  But a
        # 1-ply WeightedBot *compares* mid-turn states, so a value function fit
        # only on boundaries is being asked to extrapolate off its own training
        # distribution.  `--rows every` covers that at the price of correlation.
        take = (st.turn != last_turn) if not every else (moves % 3 == 0)
        if take and not st.pending:
            last_turn = st.turn
            snap = []
            for i in range(n):
                try:
                    r, off = row_for(st, i, w)
                except Exception:
                    r, off = None, 0.0
                snap.append((r, off))
            rows.append((st.turn, st.round, snap))
        mv = actions.legal_moves(st)
        st = game.apply(st, bots[game.current_player(st)].choose(st, mv, rng), rng)
        moves += 1
        if moves > 100000:
            break
    sc = game.scores(st)
    out = []
    for turn, rnd, snap in rows:
        for i, (r, off) in enumerate(snap):
            if r is None:
                continue
            others = [sc[j] for j in range(n) if j != i]
            best_other = max(others)
            out.append({
                "seed": seed, "turn": turn, "round": rnd, "p": i,
                "margin": sc[i] - best_other,
                "culture": sc[i],
                "win": 1.0 if sc[i] >= max(sc) else 0.0,
                "off": off,
                "x": r,
            })
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--players", type=int, default=2)
    ap.add_argument("--games", type=int, default=200)
    ap.add_argument("--weights", default=None)
    ap.add_argument("--mode", default="weighted", choices=("weighted", "plan"))
    ap.add_argument("--out", required=True)
    ap.add_argument("--seed0", type=int, default=100000)
    ap.add_argument("--workers", type=int, default=4)
    ap.add_argument("--rows", default="turn", choices=("turn", "every"))
    args = ap.parse_args()

    w = load_weights(args.weights) if args.weights else dict(DEFAULT_WEIGHTS)
    seeds = [args.seed0 + g for g in range(args.games)]
    written = 0
    with open(args.out, "a") as fh:
        if args.workers <= 1:
            _init(w, args.players, args.mode, args.rows == "every")
            it = (_one(s) for s in seeds)
        else:
            ctx = mp.get_context("fork")
            pool = ctx.Pool(args.workers, initializer=_init,
                            initargs=(w, args.players, args.mode, args.rows == "every"))
            it = pool.imap_unordered(_one, seeds, chunksize=2)
        for rows in it:
            for r in rows:
                fh.write(json.dumps(r) + "\n")
                written += 1
            fh.flush()
    print(f"wrote {written} rows from {args.games} games -> {args.out}")


if __name__ == "__main__":
    main()
