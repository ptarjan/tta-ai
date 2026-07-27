"""Fit `engine/bots/weighted.py`'s weight vector to predict the human's move.

Input is what `tools/bgo_moves.py --emit` writes: one line per reconstructed
human decision, carrying every legal candidate's post-move feature vector as a
sparse delta against candidate 0 and the index of the one the human played.

The model is exactly `weighted.evaluate`, no more and no less.  `evaluate` is
linear in the weights over 64 features, 20 phase copies and `end_turn_bias`,
so "make the evaluator's argmax agree with the human" is a **conditional
logit** -- a convex problem with a closed-form gradient -- rather than a
search.  The four terms `evaluate` prices through `w` itself
(`hand_potential`, `rival_hand_potential`, `row_urgency`,
`row_bargain_forgone`) are emitted priced through `DEFAULT_WEIGHTS` and fitted
as ordinary scales; see `bgo_moves._expand`.

Why softmax and not "count argmax agreements and hill climb":

* the argmax objective is piecewise constant, which is the one thing hill
  climbing is bad at and gradient descent cannot do at all;
* the log-loss is a proper scoring rule, so a vector that ranks the human's
  move second everywhere beats one that ranks it last everywhere, which
  argmax agreement cannot see;
* it is convex, so the fit has one answer and no seed dependence.

**Split by GAME.**  Two decisions in the same game share a tableau, a card row
and an opponent; splitting by decision would let the test set memorise the
training set's positions and would report a beautiful meaningless number.

    python3 tools/bgo_fit.py --data /tmp/bc2p_*.jsonl --epochs 8 \
        --out /tmp/clone_2p.json
    python3 tools/bgo_fit.py --data /tmp/bc2p_*.jsonl --eval-only \
        --weights experiments/league_state/champion_2p.json
"""
from __future__ import annotations

import argparse
import glob
import json
import math
import os
import random
import sys
from collections import Counter, defaultdict

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine.bots import weighted as W                          # noqa: E402
from tools.bgo_moves import PARAMS, PARAM_IX                   # noqa: E402

NP = len(PARAMS)


# ------------------------------------------------------------------- data

class Ex:
    __slots__ = ("y", "u", "c", "m", "g", "r", "q", "n")

    def __init__(self, d):
        self.y = d["y"]
        self.n = d["n"]
        self.u = [_pairs(v) for v in d["u"]]
        self.c = d["c"]
        self.m = d["m"]
        self.g = d.get("g", "?")
        self.r = d.get("r", 0)
        self.q = d.get("q", 2)


def _pairs(flat):
    return [(int(flat[i]), flat[i + 1]) for i in range(0, len(flat), 2)]


def load(paths, quality=2, limit=0):
    out = []
    for path in paths:
        with open(path) as fh:
            for line in fh:
                d = json.loads(line)
                if d.get("q", 2) < quality:
                    continue
                out.append(Ex(d))
                if limit and len(out) >= limit:
                    return out
    return out


def round_weights(rows):
    """Per-example weights that flatten the ROUND distribution.

    The clean-turn gate passes 46% of round 1-4 turns and 2% of round 17+
    turns (docs/BEHAVIOUR_CLONE.md), so the training set is roughly twice as
    opening-heavy as human play is.  A vector fitted on it is partly an
    opening book being asked to play 21 rounds; these weights let the fit be
    run both ways so that hypothesis is measured rather than argued about.
    """
    n = defaultdict(int)
    for e in rows:
        n[min((e.r - 1) // 4, 5)] += 1
    tot = sum(n.values())
    k = len(n)
    return {b: (tot / k) / c for b, c in n.items()}


def split_by_game(rows, frac=0.2, seed=11):
    games = sorted({e.g for e in rows})
    rng = random.Random(seed)
    rng.shuffle(games)
    k = max(1, int(len(games) * frac))
    test = set(games[:k])
    return ([e for e in rows if e.g not in test],
            [e for e in rows if e.g in test], len(games) - k, k)


# ------------------------------------------------------------------ model

def scores(e, w):
    return [sum(w[i] * v for i, v in e.u[j]) for j in e.c]


def rank_of(s, y):
    """Position of candidate `y` in the score order, first-index tie-break.

    `WeightedBot.pick` keeps a candidate only on a strict `>`, so a tie goes
    to the EARLIEST candidate.  Scoring has to use the same rule or it flatters
    every model that produces ties -- and ties are the normal case here,
    because `features()` cannot tell two cards in the same row tier apart.
    """
    sy = s[y]
    better = 0
    for j, v in enumerate(s):
        if v > sy or (v == sy and j < y):
            better += 1
    return better


def evaluate(rows, w, tag=""):
    top1 = top3 = 0
    ll = 0.0
    n = 0
    by_kind = defaultdict(lambda: [0, 0])
    by_round = defaultdict(lambda: [0, 0])
    tie_credit = 0
    klass = 0
    for e in rows:
        s = scores(e, w)
        r = rank_of(s, e.y)
        top1 += int(r == 0)
        top3 += int(r < 3)
        # the model cannot do better than the tie group of identical vectors
        best = max(s)
        tie_credit += int(s[e.y] == best)
        mx = best
        tot = sum(math.exp(v - mx) for v in s)
        ll += (s[e.y] - mx) - math.log(tot)
        n += 1
        k = e.m[e.y]
        by_kind[k][0] += int(r == 0)
        by_kind[k][1] += 1
        # did it at least pick the right KIND of move?  Which card to take is
        # a different question from whether to take one at all, and the
        # evaluator is much better at the second.
        pick = min(range(len(s)), key=lambda j: (-s[j], j))
        klass += int(e.m[pick] == k)
        b = min((e.r - 1) // 4, 5)
        by_round[b][0] += int(r == 0)
        by_round[b][1] += 1
    return {"n": n, "top1": top1 / max(1, n), "top3": top3 / max(1, n),
            "kind": klass / max(1, n),
            "tie": tie_credit / max(1, n), "ll": ll / max(1, n),
            "by_kind": dict(by_kind), "by_round": dict(by_round), "tag": tag}


def se(p, n):
    return math.sqrt(max(1e-12, p * (1 - p) / max(1, n)))


def game_bootstrap(rows, w, reps=200, seed=5):
    """Cluster bootstrap over GAMES, the only independent unit here."""
    by_game = defaultdict(list)
    for e in rows:
        by_game[e.g].append(e)
    hits = {}
    for g, es in by_game.items():
        h = 0
        for e in es:
            h += int(rank_of(scores(e, w), e.y) == 0)
        hits[g] = (h, len(es))
    games = list(hits)
    rng = random.Random(seed)
    out = []
    for _ in range(reps):
        h = t = 0
        for _ in range(len(games)):
            a, b = hits[rng.choice(games)]
            h += a
            t += b
        out.append(h / max(1, t))
    out.sort()
    return out[int(0.025 * reps)], out[int(0.975 * reps)]


# ------------------------------------------------------------------- fit

def fit(train, dev, epochs=8, lr=0.5, l2=1e-5, seed=3, init=None, quiet=False,
        anchor=None, rw=None):
    """Conditional logit, optionally regularised TOWARD `anchor` rather than 0.

    This matters more than it looks.  A weight is only identified by move data
    if the feature it multiplies VARIES between the candidates of a decision,
    and several of the most important ones barely do: a player's culture
    STOCK is the same number whichever of this turn's moves they make, so the
    likelihood is flat in `culture` and an L2-to-zero penalty drives it to
    zero.  The resulting vector predicts humans well and does not know that
    culture wins the game -- measured at 54 final culture against a human
    159.5 (docs/BEHAVIOUR_CLONE.md).  Anchoring the penalty on a sane prior
    leaves the unidentified directions where the prior put them and spends
    the data on the identified ones, which is the whole point of a prior.
    """
    w = [0.0] * NP
    if init:
        for k, v in init.items():
            i = PARAM_IX.get(k)
            if i is not None:
                w[i] = float(v)
    anc = list(anchor) if anchor is not None else [0.0] * NP
    g2 = [1e-8] * NP
    rng = random.Random(seed)
    order = list(range(len(train)))
    best_w, best_ll = list(w), -1e18
    for ep in range(epochs):
        rng.shuffle(order)
        for oi in order:
            e = train[oi]
            s = scores(e, w)
            mx = max(s)
            ex = [math.exp(v - mx) for v in s]
            tot = sum(ex)
            grad = {}
            for j, u in enumerate(e.c):
                p = ex[j] / tot
                if p < 1e-6:
                    continue
                for i, v in e.u[u]:
                    grad[i] = grad.get(i, 0.0) + p * v
            for i, v in e.u[e.c[e.y]]:
                grad[i] = grad.get(i, 0.0) - v
            scale = rw.get(min((e.r - 1) // 4, 5), 1.0) if rw else 1.0
            for i, gv in grad.items():
                gv = gv * scale + l2 * (w[i] - anc[i])
                g2[i] += gv * gv
                w[i] -= lr * gv / math.sqrt(g2[i])
        m = evaluate(dev, w)
        if not quiet:
            print("  epoch %d  dev top1 %.4f  top3 %.4f  logloss %.4f"
                  % (ep + 1, m["top1"], m["top3"], -m["ll"]))
        if m["ll"] > best_ll:
            best_ll, best_w = m["ll"], list(w)
    return best_w


# ---------------------------------------------------------------- baselines

def baseline_uniform(rows):
    return sum(1.0 / e.n for e in rows) / max(1, len(rows))


def baseline_kind(rows, kind):
    """Always play the first candidate of this move kind, else candidate 0."""
    hit = 0
    for e in rows:
        pick = 0
        for j, k in enumerate(e.m):
            if k == kind:
                pick = j
                break
        hit += int(pick == e.y)
    return hit / max(1, len(rows))


def to_weights(w):
    d = dict(W.DEFAULT_WEIGHTS)
    for i, k in enumerate(PARAMS):
        d[k] = round(w[i], 5)
    return d


def from_file(path):
    with open(path) as fh:
        d = json.load(fh)
    return d.get("weights", d)


def vec(weights):
    w = [0.0] * NP
    for k, v in weights.items():
        i = PARAM_IX.get(k)
        if i is not None:
            w[i] = float(v)
    return w


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--data", nargs="+", required=True)
    ap.add_argument("--quality", type=int, default=2)
    ap.add_argument("--limit", type=int, default=0)
    ap.add_argument("--epochs", type=int, default=8)
    ap.add_argument("--lr", type=float, default=0.5)
    ap.add_argument("--l2", type=float, default=1e-5)
    ap.add_argument("--init", default=None)
    ap.add_argument("--flat-rounds", action="store_true",
                    help="reweight examples to a flat round distribution")
    ap.add_argument("--anchor", default=None,
                    help="regularise toward this weight file (or 'default')")
    ap.add_argument("--out", default=None)
    ap.add_argument("--compare", action="append", default=[],
                    help="name=path of a weight file to score as a baseline")
    ap.add_argument("--eval-only", action="store_true")
    a = ap.parse_args(argv)

    paths = []
    for pat in a.data:
        paths.extend(sorted(glob.glob(pat)) or [pat])
    rows = load(paths, quality=a.quality, limit=a.limit)
    train, test, ng_tr, ng_te = split_by_game(rows)
    # a dev slice out of TRAIN games, for early stopping; the test games are
    # never scored during fitting
    dtr, dev, _a, _b = split_by_game(train, frac=0.15, seed=77)
    print("examples %d (train %d / test %d)  games %d/%d  quality>=%d"
          % (len(rows), len(train), len(test), ng_tr, ng_te, a.quality))
    kinds = Counter(e.m[e.y] for e in rows)
    print("human move kinds:", ", ".join("%s %d" % kv
                                         for kv in kinds.most_common(12)))
    print("mean candidates %.1f   mean distinct vectors %.1f"
          % (sum(e.n for e in rows) / max(1, len(rows)),
             sum(len(e.u) for e in rows) / max(1, len(rows))))

    print("\n--- baselines on the held-out games (n=%d) ---" % len(test))
    print("  uniform over legal moves      top1 %.4f" % baseline_uniform(test))
    for k in ("end_turn", "take", "build", "pop"):
        print("  always %-22s top1 %.4f" % (k, baseline_kind(test, k)))
    named = [("DEFAULT_WEIGHTS", dict(W.DEFAULT_WEIGHTS))]
    for spec in a.compare:
        name, _, path = spec.rpartition("=")
        named.append((name, from_file(path)))
    for name, ws in named:
        m = evaluate(test, vec(ws))
        lo, hi = game_bootstrap(test, vec(ws))
        print("  %-29s top1 %.4f [%.4f,%.4f]  top3 %.4f  kind %.4f  "
              "logloss %.4f"
              % (name, m["top1"], lo, hi, m["top3"], m["kind"], -m["ll"]))

    if a.eval_only:
        return 0

    init = from_file(a.init) if a.init else None
    anchor = None
    if a.anchor:
        aw = (dict(W.DEFAULT_WEIGHTS) if a.anchor == "default"
              else from_file(a.anchor))
        anchor = vec(aw)
        if init is None:
            init = aw
    print("\n--- fitting (train %d, dev %d) l2=%g anchor=%s ---"
          % (len(dtr), len(dev), a.l2, a.anchor or "zero"))
    w = fit(dtr, dev, epochs=a.epochs, lr=a.lr, l2=a.l2, init=init,
            anchor=anchor, rw=round_weights(dtr) if a.flat_rounds else None)
    m = evaluate(test, w)
    lo, hi = game_bootstrap(test, w)
    print("\n=== CLONE on held-out games ===")
    print("  top1 %.4f [%.4f,%.4f]  top3 %.4f  move-kind %.4f  "
          "tie-group %.4f  logloss %.4f"
          % (m["top1"], lo, hi, m["top3"], m["kind"], m["tie"], -m["ll"]))
    print("  by human move kind:")
    for k, (h, n) in sorted(m["by_kind"].items(), key=lambda kv: -kv[1][1]):
        print("    %-14s %5d  top1 %.3f +- %.3f" % (k, n, h / n, se(h / n, n)))
    print("  by round bucket:")
    for b in sorted(m["by_round"]):
        h, n = m["by_round"][b]
        print("    rounds %2d-%2d  %5d  top1 %.3f" % (b * 4 + 1, b * 4 + 4, n,
                                                      h / n))
    if a.out:
        W.save_weights(a.out, to_weights(w), source="bgo_fit",
                       examples=len(rows), test_top1=round(m["top1"], 4))
        print("wrote", a.out)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
