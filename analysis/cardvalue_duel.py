"""Prototype of fix #1 in docs/WASTED_ACTIONS.md: value a card by what it DOES.

`WeightedBot.features()` reduces the whole civil hand to `hand_civil` (a count)
and `hand_value` (sum of age level + 1).  Nothing about card identity, so the
1-ply search cannot prefer a good card to a bad one, and taking any card scores
about zero.  That is the root cause of the wasted-action behaviour: the flattery
on `end_turn` is only the thing that makes the indifference visible.

`CardValueBot` adds one term to the evaluation: for every card still in hand, a
discounted estimate of the position it would reach if it were played, priced
through the SAME weight vector (so it needs no new hand-tuned constants).  A
lab's science production is worth `science_rate` weights; an action card's
`gainScience` is worth `science` weights; a wonder's `civilActions` is worth
`civil_actions` weights.  The estimate is scaled by `--disc` because the card
still has to be paid for in actions, science and resources.

    python3 analysis/cardvalue_duel.py --players 2 --games 400 \
        --champion analysis/frozen/champion_2p.json --disc 0.5 --mode horizon
"""
from __future__ import annotations

import argparse
import multiprocessing as mp
import os
import random
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from engine import actions, cards as C, game              # noqa: E402
from engine.bots.fastcopy import copy_state               # noqa: E402
from engine.bots.weighted import (                        # noqa: E402
    WeightedBot, evaluate, load_weights, rival_context)
from analysis.passfix_duel import HorizonBot              # noqa: E402

# production dict key -> feature key (a card's per-turn yield once staffed)
_PROD_TO_FEATURE = {
    "culture": "culture_rate",
    "science": "science_rate",
    "food": "food_rate",
    "resources": "resource_rate",
    "happy": "happy_margin",
    "strength": "strength",
}

# effect-block key -> feature key (one-shot gains and permanent modifiers)
_EFF_TO_FEATURE = {
    "gainScience": "science",
    "gainCulture": "culture",
    "gainFood": "food_stock",
    "gainResources": "resource_stock",
    "gainPopulation": "free_workers",
    "civilActions": "civil_actions",
    "militaryActions": "military_actions",
    "extraCivilActions": "civil_actions",
    "extraMilitaryActions": "military_actions",
    "cultureProduction": "culture_rate",
    "scienceProduction": "science_rate",
    "foodProduction": "food_rate",
    "resourceProduction": "resource_rate",
    "strength": "strength",
    "happy": "happy_margin",
    "yellowTokens": "yellow_bank",
    "blueTokens": "blue_free",
}

_CACHE = {}


def card_potential(name, w):
    """Eval-points a card would be worth if it were played, before its cost."""
    key = (name, id(w))
    hit = _CACHE.get(key)
    if hit is not None:
        return hit
    db = C.db()
    card = db.by_name.get(name)
    if card is None:
        _CACHE[key] = 0.0
        return 0.0
    v = 0.0
    typ = card["type"]

    # a staffed worker card yields its production every turn
    for k, amt in (card.get("production") or {}).items():
        fk = _PROD_TO_FEATURE.get(k)
        if fk and isinstance(amt, (int, float)):
            v += w.get(fk, 0.0) * amt

    for k, amt in (card.get("effects") or {}).items():
        if amt is True:
            continue
        if not isinstance(amt, (int, float)):
            continue
        fk = _EFF_TO_FEATURE.get(k)
        if fk:
            v += w.get(fk, 0.0) * amt

    # what it still costs you to get there
    tc = card.get("techCost") or 0
    bc = card.get("buildCost") or 0
    v -= tc * w.get("science", 0.0)
    v -= bc * w.get("resource_stock", 0.0)
    if typ == "wonder":
        stages = card.get("stages") or []
        v += w.get("wonders", 0.0)
        v -= sum(stages) * w.get("resource_stock", 0.0)
    _CACHE[key] = v
    return v


def hand_potential(state, idx, w):
    p = state.players[idx]
    return sum(card_potential(n, w) for n in p.hand_civil)


class CardValueMixin:
    """Adds the identity-aware hand term to whatever scorer it is mixed into."""

    disc = 0.5

    def _score(self, trial, idx, w, ctx):
        return (evaluate(trial, idx, w, ctx)
                + self.disc * hand_potential(trial, idx, w))


class CardValueHorizonBot(CardValueMixin, HorizonBot):
    name = "cardvalue_horizon"

    def __init__(self, *a, disc=0.5, **kw):
        super().__init__(*a, **kw)
        self.disc = disc

    def pick(self, state, moves):
        if len(moves) == 1:
            return moves[0]
        idx = state.decider()
        try:
            ctx = rival_context(state, idx)
        except Exception:
            ctx = {"rival_culture_rate": 0, "rival_science_rate": 0,
                   "rival_strength": 0}
        w = self.weights
        best, best_val = None, None
        for mv in moves:
            trial = copy_state(state)
            try:
                actions.apply(trial, mv, random.Random(0))
                if mv[0] != "end_turn":
                    if not trial.game_over and not trial.pending \
                            and trial.decider() == idx \
                            and trial.phase == "actions":
                        actions.apply(trial, ("end_turn",), random.Random(0))
                val = self._score(trial, idx, w, ctx)
                if mv[0] == "end_turn":
                    val += self.eps
            except Exception:
                continue
            if best_val is None or val > best_val:
                best, best_val = mv, val
        if best is None:
            return self.rng.choice(moves)
        return best


class CardValueBot(CardValueMixin, WeightedBot):
    """The champion's own search, with only the hand term added."""

    name = "cardvalue"

    def __init__(self, *a, disc=0.5, **kw):
        super().__init__(*a, **kw)
        self.disc = disc

    def pick(self, state, moves):
        if len(moves) == 1:
            return moves[0]
        idx = state.decider()
        try:
            ctx = rival_context(state, idx)
        except Exception:
            ctx = {"rival_culture_rate": 0, "rival_science_rate": 0,
                   "rival_strength": 0}
        w = self.weights
        end_bias = w.get("end_turn_bias", 0.0)
        best, best_val = None, None
        for mv in moves:
            trial = copy_state(state)
            try:
                actions.apply(trial, mv, random.Random(0))
                val = self._score(trial, idx, w, ctx)
            except Exception:
                continue
            if mv[0] == "end_turn":
                val += end_bias
            if best_val is None or val > best_val:
                best, best_val = mv, val
        if best is None:
            return self.rng.choice(moves)
        return best


_MODES = {"horizon": CardValueHorizonBot, "plain": CardValueBot}

_W = {}


def _init(weights, n, disc, mode, eps):
    _W.update(w=weights, n=n, disc=disc, mode=mode, eps=eps)


def _play(task):
    seed, seat = task
    n, w, disc, mode, eps = (_W["n"], _W["w"], _W["disc"], _W["mode"],
                             _W["eps"])
    cls = _MODES[mode]
    bots = []
    for i in range(n):
        s = seed * 97 + i * 13 + 1
        if i == seat:
            kw = {"disc": disc}
            if mode == "horizon":
                kw["eps"] = eps
            bots.append(cls(weights=w, seed=s, **kw))
        else:
            bots.append(WeightedBot(weights=w, seed=s))
    try:
        st = game.play_game(bots, n, seed=seed)
    except Exception as e:
        return (None, repr(e), 0, 0)
    sc = game.scores(st)
    best = max(sc)
    tied = [i for i, v in enumerate(sc) if v == best]
    share = (1.0 / len(tied)) if seat in tied else 0.0
    others = [sc[i] for i in range(n) if i != seat]
    return (share, sc[seat], sum(others) / len(others), 0)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--players", type=int, default=2)
    ap.add_argument("--games", type=int, default=400)
    ap.add_argument("--champion", required=True)
    ap.add_argument("--disc", type=float, default=0.5)
    ap.add_argument("--eps", type=float, default=-0.01)
    ap.add_argument("--mode", default="horizon", choices=tuple(_MODES))
    ap.add_argument("--workers", type=int, default=0)
    a = ap.parse_args()

    w = load_weights(a.champion)
    tasks = [(s // a.players * 7919 + 17, s % a.players)
             for s in range(a.games)]
    workers = a.workers or max(1, (os.cpu_count() or 2) - 1)
    ctx = mp.get_context("fork")
    with ctx.Pool(workers, initializer=_init,
                  initargs=(w, a.players, a.disc, a.mode, a.eps)) as pool:
        out = pool.map(_play, tasks, chunksize=2)

    shares = [o[0] for o in out if o[0] is not None]
    errs = [o[1] for o in out if o[0] is None]
    ca = [o[1] for o in out if o[0] is not None]
    cb = [o[2] for o in out if o[0] is not None]
    n = len(shares)
    m = sum(shares) / n
    var = sum((x - m) ** 2 for x in shares) / max(1, n - 1)
    half = 1.96 * (var / n) ** 0.5
    print(f"cardvalue/{a.mode}(disc={a.disc},eps={a.eps}) vs champion "
          f"@{a.players}p: win rate {m:.1%} +/- {half:.1%} "
          f"(null {1/a.players:.1%}, n={n}, errors={len(errs)})")
    print(f"  mean culture: cardvalue {sum(ca)/n:.1f}  champion {sum(cb)/n:.1f}")
    if errs:
        print("  error sample:", errs[:2])


if __name__ == "__main__":
    main()
