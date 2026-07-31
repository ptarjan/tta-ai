"""Dead-coordinate census: which weights can never change a decision?

`WeightedBot` is an argmax over a linear score.  A weight only matters if
its feature actually *differs between the candidate moves of a decision*.
A feature that is constant across every candidate at a decision adds the
same number to every candidate and cancels out of the argmax exactly --
so its weight has no gradient there, drifts under the hill climb, and any
value it ends up with means nothing.  `unit_workers` = 0.000 next to
`strength_lead` = 6.392 in the 2p champion is one instance of this; this
tool finds all of them.

Three numbers per feature, all measured on the positions the bot really
evaluated (candidate states, i.e. after applying each legal move):

``varying``     fraction of decisions where the feature is not identical
                across all candidates.  This is the gradient: at 0 the
                weight is invisible, full stop.
``mean_range``  mean over decisions of (max - min) across candidates.
``flip``        fraction of decisions where zeroing this weight (and its
                ``_early`` / ``_late`` copies) changes the chosen move.
                The direct, end-to-end answer to "does this weight do
                anything", under the weight vector supplied.

``flip`` is the one to read.  ``varying`` > 0 with ``flip`` == 0 means the
feature moves but the weight is too small to ever win an argmax; both zero
means the coordinate is dead.

Usage:
    python3 tools/feature_variance.py --players 2 --games 6 \\
        --champ experiments/league_state/champion_2p.json --out /tmp/v2.json
    python3 tools/feature_variance.py --players 2 --games 6 --bot default

One worker.  Roughly 3x the cost of a plain self-play game (the feature
vector of every candidate is kept, not just its score).
"""
from __future__ import annotations

import argparse
import json
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine import actions, game, journal                        # noqa: E402
from engine.bots import weighted as W                            # noqa: E402
from engine.bots.fastcopy import copy_state                      # noqa: E402
from engine.bots.trial import USE_JOURNAL, fresh_trial_rng       # noqa: E402


def score_from(f, w, late, hand_pot, hz=1.0):
    """`weighted.evaluate` recomputed from a cached feature vector.

    Kept in step with `evaluate` by `test_feature_variance.py`, which asserts
    the two agree to 1e-9 on real candidate states.

    `hz` is `weighted.rate_multiplier` for the state the features came from --
    the rate horizon is a property of the PRICE, so a cached feature vector
    does not carry it and the caller has to hand it over with the vector.
    """
    total = 0.0
    get = w.get
    for k, v in f.items():
        wk = get(k)
        if wk:
            total += wk * v * hz if (hz != 1.0 and k in W.RATE_KEYS) \
                else wk * v
    early = 1.0 - late
    for k in W.PHASE_KEYS:
        v = f[k]
        if not v:
            continue
        if hz != 1.0 and k in W.RATE_KEYS:
            v = v * hz
        we = get(k + "_early")
        if we:
            total += we * early * v
        wl = get(k + "_late")
        if wl:
            total += wl * late * v
    hp = get("hand_potential")
    if hp:
        total += hp * hand_pot
    return total


class Probe:
    """Plays exactly like `WeightedBot` and records every candidate's features.

    The move returned is the same argmax `WeightedBot.pick` would return
    (same order, same strict `>` tie-break, same `end_turn_bias`), so the
    game trajectory measured is the real one.
    """

    def __init__(self, weights, acc, seed=None):
        self.w = dict(weights)
        self.acc = acc
        import random
        self.rng = random.Random(seed)

    def __call__(self, state):
        moves = actions.legal_moves(state)
        if len(moves) == 1:
            return moves[0]
        idx = state.decider()
        try:
            ctx = W.rival_context(state, idx)
        except Exception:                                  # noqa: BLE001
            ctx = {"rival_culture_rate": 0, "rival_science_rate": 0,
                   "rival_strength": 0}
        w = self.w
        end_bias = w.get("end_turn_bias", 0.0)
        rows = []          # (move, features, lateness, hand_potential, bias)
        for mv in moves:
            if USE_JOURNAL:
                j = journal.begin(state)
                try:
                    try:
                        actions.apply(state, mv, fresh_trial_rng())
                        f = W.features(state, idx, ctx)
                        late = W.lateness(state)
                        hp = W.hand_potential(state, idx, w)
                    except Exception:                      # noqa: BLE001
                        continue
                finally:
                    journal.rollback(j)
            else:
                trial = copy_state(state)
                try:
                    actions.apply(trial, mv, fresh_trial_rng())
                    f = W.features(trial, idx, ctx)
                    late = W.lateness(trial)
                    hz = W.rate_multiplier(trial, w)
                    hp = W.hand_potential(trial, idx, w)
                except Exception:                          # noqa: BLE001
                    continue
            rows.append((mv, f, late, hp,
                         end_bias if mv[0] == "end_turn" else 0.0, hz))
        if not rows:
            return self.rng.choice(moves)
        self.acc.note(rows, w)
        best, best_val = None, None
        for mv, f, late, hp, bias, hz in rows:
            val = score_from(f, w, late, hp, hz) + bias
            if best_val is None or val > best_val:
                best, best_val = mv, val
        return best


def _argmax(rows, w):
    best, best_val = None, None
    for mv, f, late, hp, bias, hz in rows:
        val = score_from(f, w, late, hp, hz) + bias
        if best_val is None or val > best_val:
            best, best_val = mv, val
    return best


class Acc:
    def __init__(self, flip=True):
        self.n = 0
        self.varying = {}
        self.range_sum = {}
        self.flip = {}
        self.do_flip = flip
        self.keys = None

    def note(self, rows, w):
        if len(rows) < 2:
            return
        self.n += 1
        if self.keys is None:
            self.keys = sorted(rows[0][1])
        for k in self.keys:
            vals = [r[1][k] for r in rows]
            lo, hi = min(vals), max(vals)
            if hi != lo:
                self.varying[k] = self.varying.get(k, 0) + 1
                self.range_sum[k] = self.range_sum.get(k, 0.0) + (hi - lo)
        if not self.do_flip:
            return
        base = _argmax(rows, w)
        for k in self.keys + ["hand_potential", "end_turn_bias"]:
            names = [k]
            if k in W.PHASE_KEYS:
                names += [k + "_early", k + "_late"]
            if not any(w.get(nm) for nm in names):
                continue
            w2 = dict(w)
            for nm in names:
                w2[nm] = 0.0
            if k == "end_turn_bias":
                rows2 = [(mv, f, late, hp, 0.0)
                         for mv, f, late, hp, _ in rows]
                alt = _argmax(rows2, w2)
            else:
                alt = _argmax(rows, w2)
            if alt != base:
                self.flip[k] = self.flip.get(k, 0) + 1

    def table(self, w):
        out = []
        for k in (self.keys or []) + ["hand_potential", "end_turn_bias"]:
            n = max(1, self.n)
            out.append({
                "feature": k,
                "weight": round(float(w.get(k, 0.0)), 5),
                "varying": round(self.varying.get(k, 0) / n, 5),
                "mean_range": round(self.range_sum.get(k, 0.0) / n, 4),
                "flip": round(self.flip.get(k, 0) / n, 5),
                "flip_n": self.flip.get(k, 0),
            })
        out.sort(key=lambda r: (r["flip"], r["varying"]))
        return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--players", type=int, default=2)
    ap.add_argument("--games", type=int, default=6)
    ap.add_argument("--champ", default=None)
    ap.add_argument("--bot", default="champ", choices=("champ", "default"))
    ap.add_argument("--seed0", type=int, default=91000)
    ap.add_argument("--no-flip", action="store_true")
    ap.add_argument("--out", default=None)
    a = ap.parse_args()

    if a.bot == "default":
        w, src = dict(W.DEFAULT_WEIGHTS), "default"
    else:
        src = a.champ or f"experiments/league_state/champion_{a.players}p.json"
        w = W.load_weights(src)

    acc = Acc(flip=not a.no_flip)
    for gi in range(a.games):
        bots = [Probe(w, acc, seed=(a.seed0 + gi) * 97 + i)
                for i in range(a.players)]
        st = game.play_game(bots, a.players, seed=a.seed0 + gi)
        print(f"  game {gi}: scores={game.scores(st)} decisions={acc.n}",
              file=sys.stderr, flush=True)

    out = {"players": a.players, "games": a.games, "weights": src,
           "decisions": acc.n, "features": acc.table(w)}
    print(json.dumps(out, indent=1))
    if a.out:
        with open(a.out, "w") as fh:
            json.dump(out, fh, indent=1)


if __name__ == "__main__":
    main()
