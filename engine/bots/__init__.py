"""Bots.

A bot is any callable ``bot(state) -> move`` choosing among
``engine.actions.legal_moves(state)``.  Two are provided here:

* :class:`RandomBot` -- uniform over legal moves; the legality fuzzer.
* :class:`GreedyBot` -- 1-ply lookahead: applies every legal move to a copy
  of the state and keeps the one with the best static evaluation.
"""
from __future__ import annotations

import random

from .. import actions, effects

__all__ = ["RandomBot", "GreedyBot", "evaluate", "make_bots"]


class RandomBot:
    name = "random"

    def __init__(self, rng=None, seed=None):
        self.rng = rng or random.Random(seed)

    def __call__(self, state):
        moves = actions.legal_moves(state)
        return self.rng.choice(moves)

    def choose(self, state, moves, rng=None):
        """Adapter for experiments/harness.py."""
        return (rng or self.rng).choice(moves)


# ------------------------------------------------------------ evaluation

# Weights over the feature vector below.  Culture is the score, everything
# else is a proxy for future culture; the numbers are hand-set starting
# values for the self-play hill climb (experiments/harness.py).
WEIGHTS = {
    "culture": 1.0,
    "culture_rate": 6.0,
    "science_rate": 4.0,
    "science": 0.5,
    "strength": 0.8,
    "food_rate": 1.2,
    "resource_rate": 1.5,
    "food": 0.2,
    "resources": 0.3,
    "workers": 1.5,
    "free_workers": 0.3,
    "yellow_bank": 0.15,
    "civil_actions": 2.0,
    "military_actions": 0.7,
    "happy_margin": 1.5,
    "wonders": 3.0,
    "wonder_progress": 1.0,
    "hand": 0.4,
    "tech_levels": 1.5,
}


def features(state, p):
    from .. import cards as C
    from .. import economy
    s = effects.compute(state, p)
    db = C.db()
    workers = sum(t.workers for t in p.techs.values())
    # better technologies in play are future production, invisible to a
    # 1-ply search until a worker is on them
    tech_levels = sum(db.level_of(n) for n in p.techs
                      if db.type_of(n) in (C.WORKER_TYPES | {"special-tech"}))
    tech_levels += db.level_of(p.government)
    happy_margin = s.happy - economy.happy_required(p.yellow_bank)
    progress = 0
    if p.wonder is not None:
        stages = db.get(p.wonder.name)["stages"]
        progress = sum(stages[:p.wonder.steps_built])
    return {
        "culture": p.culture,
        "culture_rate": s.culture,
        "science_rate": s.science,
        "science": p.science,
        "strength": s.strength,
        "food_rate": s.food,
        "resource_rate": s.resources,
        "food": p.food,
        "resources": p.resources,
        "workers": workers,
        "free_workers": p.workers_free,
        "yellow_bank": p.yellow_bank,
        "civil_actions": s.civil_actions,
        "military_actions": s.military_actions,
        "happy_margin": min(2, happy_margin),
        "wonders": len(p.completed_wonders),
        "wonder_progress": progress,
        "hand": len(p.hand_civil),
        "tech_levels": tech_levels,
    }


def evaluate(state, idx, weights=None):
    """Static evaluation of `state` from player `idx`'s point of view."""
    w = weights or WEIGHTS
    f = features(state, state.players[idx])
    own = sum(w.get(k, 0.0) * v for k, v in f.items())
    # relative: being ahead of the best rival is what wins the game
    rivals = [q for q in state.players if q.idx != idx and not q.resigned]
    best_rival = max((q.culture for q in rivals), default=0)
    return own - 0.4 * best_rival


class GreedyBot:
    """1-ply lookahead over the legal moves of the current decision."""

    name = "greedy"

    def __init__(self, rng=None, seed=None, weights=None):
        self.rng = rng or random.Random(seed)
        self.weights = weights or WEIGHTS

    def choose(self, state, moves, rng=None):
        """Adapter for experiments/harness.py."""
        return self.pick(state, moves)

    def __call__(self, state):
        return self.pick(state, actions.legal_moves(state))

    def pick(self, state, moves):
        if len(moves) == 1:
            return moves[0]
        idx = state.current
        best, best_val = None, None
        # a fresh deterministic rng per candidate keeps the search from
        # consuming the game's rng stream
        for mv in moves:
            trial = state.copy()
            try:
                actions.apply(trial, mv, random.Random(0))
            except Exception:
                continue
            val = evaluate(trial, idx, self.weights)
            if mv[0] == "end_turn":
                # ending the turn is never rewarded for its own sake: it only
                # wins when nothing else improves the position
                val -= 0.01
            if best_val is None or val > best_val:
                best, best_val = mv, val
        return best if best is not None else moves[0]


def make_bots(spec, num_players, seed=0):
    """Build a bot list from names, e.g. make_bots("greedy,random", 2)."""
    kinds = spec.split(",") if isinstance(spec, str) else list(spec)
    out = []
    for i in range(num_players):
        kind = kinds[i % len(kinds)].strip()
        rng = random.Random(seed * 131 + i)
        out.append(RandomBot(rng) if kind == "random" else GreedyBot(rng))
    return out
