"""QuiescentBot: 1-ply search that resolves the pending stack before scoring.

Why this exists
---------------
``WeightedBot`` scores a candidate move by applying it to a trial copy and
evaluating the result immediately.  That is correct only for moves whose
whole effect lands in the trial state.  A large and strategically important
class of TTA moves does NOT satisfy that:

    move            what apply() leaves in the trial state       the real payoff
    ------------    ------------------------------------------   --------------
    offer_pact      card gone from hand, a ``choice`` pending     the pact, if the
                    for the partner                               partner accepts
    aggression      military card gone, MA spent, a ``defense``   loot / raid /
                    pending for the defender                      annex / culture
    bid             one integer in the auction dict              the colony, and
                                                                  its price in units
    action card     card gone, a ``free_civil`` choice pending    the free action
                    for the mover                                 and the gains

In every one of those rows the trial state shows **the entire cost and none of
the gain**, so the move is strictly dominated by passing under *any* weight
vector.  docs/PACTS_DIAGNOSIS.md proves this for pacts and colony bids; the
same argument covers aggressions and action cards.  It is not a weighting
problem and no weight can fix it -- hill climbing cannot select for a move it
can never rank first.

The fix here is the game-tree analogue of quiescence search: do not evaluate a
position while a decision is still hanging.  After applying a candidate move,
keep resolving ``state.pending`` -- whoever the decider is, including rivals --
until the stack is empty, and only then evaluate.  ``interact.run_queue`` drains
``state.queue`` automatically whenever the stack empties, so "pending is empty"
is exactly the quiet position.

Opponent model
--------------
Pending decisions belonging to rivals are resolved with a plain 1-ply weighted
pick **maximising that rival's own evaluation** -- i.e. the current champion.
In self-play that is not an approximation of the opponent, it *is* the
opponent, so the line the search reads is the line that will actually be
played.  The inner pick is 1-ply (never recursive), which bounds the cost.

Cost
----
Quiescence runs only when a candidate move leaves the stack non-empty.  The
overwhelming majority of moves (build, take, pop, upgrade, end_turn) resolve
inside ``apply`` and cost exactly what they cost today.  The expensive shapes
are auctions, whose resolution is itself a chain of rival bid decisions.  Two
budgets keep that bounded:

``MAX_DEPTH``   pending decisions resolved per candidate move
``MAX_NODES``   total ``apply`` calls spent on quiescence per root decision

When a budget is exhausted the stack is left as it is and the position is
scored as it stands -- which falls back to exactly the hand-priced
``deferred_credit`` path of ``WeightedBot``, so a budget miss degrades to
today's behaviour rather than to nonsense.

War
---
A war declaration is NOT fixed by quiescence: it pushes nothing onto the stack
and resolves at the start of the declarer's *next* turn (``game.start_turn`` ->
``events.resolve_war``), a full round away.  ``WAR_LOOKAHEAD`` handles it
separately by calling the engine's own ``resolve_war`` on a scratch copy, which
is a pure deterministic function of the two players' current strengths.  That
is a lookahead, not a hand-priced weight: the number it produces is the spoils
the engine itself would award.
"""
from __future__ import annotations

import random

from .. import actions
from .fastcopy import copy_state
from .weighted import DEFAULT_WEIGHTS, evaluate, rival_context

__all__ = ["QuiescentBot"]


# ------------------------------------------------------------------ rng
#
# Same trick as GreedyBot (docs/PYPY.md 5a/8): a Mersenne Twister that has not
# been drawn from is byte-identical to a fresh Random(0), so the object is
# reused and re-seeded only when a trial actually consumed it.  Two separate
# instances: the outer one drives the quiescence line, the inner one drives the
# opponent-model picks, so an inner pick can never advance the outer line's
# stream and make a candidate's resolution depend on how many moves the
# opponent model happened to consider.
class _TrialRandom(random.Random):
    used = False

    def random(self):
        self.used = True
        return super().random()

    def getrandbits(self, k):
        self.used = True
        return super().getrandbits(k)


_OUTER = _TrialRandom(0)
_INNER = _TrialRandom(0)
_PRISTINE = _OUTER.getstate()


def _fresh(r):
    if r.used:
        r.setstate(_PRISTINE)
        r.used = False
    return r


# -------------------------------------------------------- opponent model

def _pick_1ply(state, moves, idx, weights, end_bias):
    """The current champion's choice, for whoever owns this decision."""
    if len(moves) == 1:
        return moves[0]
    try:
        ctx = rival_context(state, idx)
    except Exception:
        ctx = {"rival_culture_rate": 0, "rival_science_rate": 0,
               "rival_strength": 0}
    best, best_val = None, None
    for mv in moves:
        trial = copy_state(state)
        try:
            actions.apply(trial, mv, _fresh(_INNER))
            val = evaluate(trial, idx, weights, ctx)
        except Exception:
            continue
        if mv[0] == "end_turn":
            val += end_bias
        if best_val is None or val > best_val:
            best, best_val = mv, val
    return best if best is not None else moves[0]


# --------------------------------------------------------- war lookahead

def _war_value(state, idx, weights, ctx):
    """Value of the position with the declarer's pending war already fought.

    ``events.resolve_war`` is deterministic and consumes no rng, so this is a
    real (if optimistic -- the defender gets a turn in between) resolution
    rather than a priced guess.
    """
    from .. import events
    scratch = copy_state(state)
    try:
        events.resolve_war(scratch, scratch.players[idx], None)
    except Exception:
        return None
    try:
        return evaluate(scratch, idx, weights, ctx)
    except Exception:
        return None


# -------------------------------------------------------------- the bot

class QuiescentBot:
    """1-ply search that resolves ``state.pending`` to quiescence first."""

    name = "quiescent"

    #: pending decisions resolved per candidate move
    MAX_DEPTH = 12
    #: total quiescence ``apply`` calls per root decision
    MAX_NODES = 600
    #: resolve a declared-but-unfought war through the engine before scoring
    WAR_LOOKAHEAD = True

    def __init__(self, weights=None, rng=None, seed=None, name=None,
                 max_depth=None, max_nodes=None, war_lookahead=None):
        self.weights = dict(weights) if weights else dict(DEFAULT_WEIGHTS)
        self.rng = rng or random.Random(seed)
        if name:
            self.name = name
        if max_depth is not None:
            self.MAX_DEPTH = max_depth
        if max_nodes is not None:
            self.MAX_NODES = max_nodes
        if war_lookahead is not None:
            self.WAR_LOOKAHEAD = war_lookahead
        # instrumentation, read by tools/quiesce_bench.py
        self.stats = {"decisions": 0, "candidates": 0, "quiesced": 0,
                      "qnodes": 0, "truncated": 0}

    # -- harness adapters
    def choose(self, state, moves, rng=None):
        return self.pick(state, moves)

    def __call__(self, state):
        return self.pick(state, actions.legal_moves(state))

    # -- search
    def _quiesce(self, trial, weights, end_bias, left):
        """Resolve pending decisions until the position is quiet.

        Returns (nodes spent, True if it reached a quiet position)."""
        n = 0
        depth = self.MAX_DEPTH
        while trial.pending:
            if n >= depth or n >= left or trial.game_over:
                return n, False
            moves = actions.legal_moves(trial)
            if not moves:
                return n, False
            mv = _pick_1ply(trial, moves, trial.decider(), weights, end_bias)
            try:
                actions.apply(trial, mv, _fresh(_OUTER))
            except Exception:
                return n, False
            n += 1
        return n, True

    def pick(self, state, moves):
        if len(moves) == 1:
            return moves[0]
        idx = state.decider()
        try:
            root_ctx = rival_context(state, idx)
        except Exception:
            root_ctx = {"rival_culture_rate": 0, "rival_science_rate": 0,
                        "rival_strength": 0}
        w = self.weights
        end_bias = w.get("end_turn_bias", 0.0)
        st = self.stats
        st["decisions"] += 1
        budget = self.MAX_NODES
        best, best_val = None, None
        for mv in moves:
            st["candidates"] += 1
            trial = copy_state(state)
            try:
                actions.apply(trial, mv, _fresh(_OUTER))
            except Exception:
                continue
            ctx = root_ctx
            if trial.pending:
                st["quiesced"] += 1
                spent, quiet = self._quiesce(trial, w, end_bias, budget)
                budget -= spent
                st["qnodes"] += spent
                if not quiet:
                    st["truncated"] += 1
                # rivals moved inside the resolution, so the root's cached
                # rival aggregates are stale
                try:
                    ctx = rival_context(trial, idx)
                except Exception:
                    ctx = root_ctx
            try:
                val = evaluate(trial, idx, w, ctx)
            except Exception:
                continue
            if self.WAR_LOOKAHEAD and mv[0] == "war":
                wv = _war_value(trial, idx, w, ctx)
                if wv is not None:
                    val = wv
            if mv[0] == "end_turn":
                val += end_bias
            if best_val is None or val > best_val:
                best, best_val = mv, val
        if best is None:
            return self.rng.choice(moves)
        return best
