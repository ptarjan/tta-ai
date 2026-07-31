"""Bots.

A bot is any callable ``bot(state) -> move`` choosing among
``engine.actions.legal_moves(state)``.  Two are provided here:

* :class:`RandomBot` -- uniform over legal moves; the legality fuzzer.
* :class:`GreedyBot` -- 1-ply lookahead: applies every legal move to a copy
  of the state and keeps the one with the best static evaluation.
"""
from __future__ import annotations

import math
import random

from .. import actions, effects, journal
from .fastcopy import copy_state

#: Search with the undo stack (docs/PYPY.md section 6) instead of `copy_state`.
#: OPT-IN and off by default; honoured by `GreedyBot` and `WeightedBot`, and
#: deliberately NOT by `QuiescentBot` (docs/PYPY.md 9.15).  Defined in
#: `bots/trial.py` so `bots.weighted` can see it without importing this
#: half-initialised package; re-exported here because that is where every
#: existing caller (tools/mutation_coverage.py, tests) looks for it.
from .trial import (USE_JOURNAL, TrialRandom as _TrialRandom,
                    TRIAL_RNG as _TRIAL_RNG,
                    TRIAL_RNG_STATE as _TRIAL_RNG_STATE)
from .weighted import DEFAULT_WEIGHTS, WeightedBot, load_weights, save_weights

__all__ = ["RandomBot", "GreedyBot", "WeightedBot", "DEFAULT_WEIGHTS",
           "evaluate", "make_bots", "load_weights", "save_weights"]

# `_TrialRandom` / `_TRIAL_RNG` moved to `bots/trial.py` (same objects, same
# semantics, aliased above) so `WeightedBot` can share the one instance.  The
# reasoning for reusing a Mersenne Twister instead of re-seeding one per
# candidate is written out there; docs/PYPY.md 5a/8 is the measurement.


class RandomBot:
    name = "random"

    def __init__(self, rng=None, seed=None, allow_resign=False):
        self.rng = rng or random.Random(seed)
        self.allow_resign = allow_resign

    def __call__(self, state):
        return self.choose(state, actions.legal_moves(state))

    def choose(self, state, moves, rng=None):
        """Adapter for experiments/harness.py."""
        if not self.allow_resign:
            # resigning (§5.11) is legal every turn; a uniform-random bot
            # would end most games in round 2, so it is opt-in
            live = [m for m in moves if m[0] != "resign"]
            moves = live or moves
        return (rng or self.rng).choice(moves)


# ------------------------------------------------------------ evaluation

# GreedyBot's weights.  FROZEN ON PURPOSE -- do not "sync" these with
# `weighted.BASE_WEIGHTS`, which they deliberately disagree with (culture_rate
# 6.0 here against 5.0 there, and a dozen keys that exist on one side only).
#
# The comment this replaces said they were "hand-set starting values for the
# self-play hill climb (experiments/harness.py)", which has not been true for a
# long time: the trainer is `experiments/hillclimb_league.py`, it starts from
# `weighted.DEFAULT_WEIGHTS`, and nothing has warm-started from this table in
# the life of the current league.  Read as a stale default, the obvious next
# move is to make the two agree.  That would be a mistake.
#
# GreedyBot is the fingerprint CONTROL (docs/PYPY.md 9.0).  Two of the eight
# gate arms, NARROW and WIDE, are GreedyBot and nothing else, and their whole
# job is to hold still while evaluator changes move the other six -- a
# GreedyBot arm moving is the signal that a change has leaked out of the
# evaluator and into the rules.  A control whose weights drift with the thing
# it is controlling for is not a control.  So these numbers are frozen, and
# changing any of them is a deliberate act that moves NARROW and WIDE and needs
# its own derivation.  (They are also `features(state, p)` keys in THIS module,
# not `weighted.features` keys; the two vocabularies only overlap by accident.)
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
    # math.fsum, not sum(): CPython >=3.12 sums floats with Neumaier
    # compensation while PyPy 3.11 accumulates naively, and the resulting
    # ~1 ULP gap flips exact ties in GreedyBot.pick, diverging whole games
    # between interpreters. fsum is exactly rounded everywhere, so this is
    # one answer on every interpreter -- and it is bit-identical to what
    # CPython 3.14 already produced (perf_check fingerprint unchanged).
    own = math.fsum([w.get(k, 0.0) * v for k, v in f.items()])
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

    ALLOW_RESIGN = False

    def __call__(self, state):
        return self.pick(state, actions.legal_moves(state))

    def pick(self, state, moves):
        if not self.ALLOW_RESIGN:
            moves = [m for m in moves if m[0] != "resign"] or moves
        if len(moves) == 1:
            return moves[0]
        idx = state.decider()
        if USE_JOURNAL:
            return self._pick_journalled(state, moves, idx)
        best, best_val = None, None
        # a fresh deterministic rng per candidate keeps the search from
        # consuming the game's rng stream
        for mv in moves:
            trial = copy_state(state)
            if _TRIAL_RNG.used:                         # rare: ~0.4% of moves
                _TRIAL_RNG.setstate(_TRIAL_RNG_STATE)   # == fresh Random(0)
                _TRIAL_RNG.used = False
            try:
                actions.apply(trial, mv, _TRIAL_RNG)
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

    def _pick_journalled(self, state, moves, idx):
        """`pick` with the undo stack instead of `copy_state` (docs/PYPY.md 6).

        Line-for-line the same search as above; the only difference is that
        the candidate is applied to the REAL state and undone, rather than to
        a copy that is thrown away.  Kept as a separate method rather than a
        branch inside the loop so the copy path stays exactly as it was and
        can go on being the paranoid oracle.
        """
        begin, rollback = journal.begin, journal.rollback
        best, best_val = None, None
        for mv in moves:
            if _TRIAL_RNG.used:
                _TRIAL_RNG.setstate(_TRIAL_RNG_STATE)
                _TRIAL_RNG.used = False
            j = begin(state)
            try:
                try:
                    actions.apply(state, mv, _TRIAL_RNG)
                except Exception:
                    continue            # the `finally` still rolls back
                val = evaluate(state, idx, self.weights)
            finally:
                rollback(j)
            if mv[0] == "end_turn":
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
        if kind == "random":
            out.append(RandomBot(rng))
        elif kind == "weighted":
            out.append(WeightedBot(rng=rng))
        elif kind == "quiescent":
            from .quiescent import QuiescentBot
            out.append(QuiescentBot(rng=rng))
        elif kind == "plan":
            # width 2, as `experiments/run_league.sh` trains it and as
            # `engine/perf_check.py`'s plan cases play it
            from .plan import PlanBot
            out.append(PlanBot(rng=rng, width=2))
        else:
            out.append(GreedyBot(rng))
    return out
