"""QuiescentBot: search that resolves the pending stack before scoring.

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
until the stack is empty, and only then evaluate.  ``interact.run_queue``
drains ``state.queue`` automatically whenever the stack empties, so "pending is
empty" is exactly the quiet position.

Levels
------
``LEVELS`` says how many nested quiescence resolutions the search performs.

``LEVELS = 1``  the root's candidates are resolved to quiet; the rivals'
                decisions inside that resolution are answered with a plain
                1-ply pick, i.e. the current champion.  In self-play that is
                not an approximation of the opponent, it *is* the opponent.

``LEVELS = 2``  the rivals' decisions inside the resolution are themselves
                resolved to quiet.  This matters for one question specifically:
                a rival deciding whether to raise a colony bid is, at 1 ply,
                blind in exactly the way described above, and today only
                ``weighted.deferred_credit`` stops it modelling every bid as
                worthless.  At ``LEVELS = 2`` the modelled rival plays the
                auction out instead, so the hand-priced credit stops being
                load-bearing anywhere in the search.

``LEVELS = 0`` is plain ``WeightedBot`` and exists for A/B control.

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
today's behaviour rather than to nonsense.  The converse is the load-bearing
fact for retiring those patches: ``weighted.features`` only applies the credit
``if state.pending``, so a candidate that reached quiescence is scored with the
credit contributing exactly zero.

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

from .. import actions, journal
from .fastcopy import copy_state
from .trial import USE_JOURNAL
from .weighted import DEFAULT_WEIGHTS, evaluate, rival_context

__all__ = ["QuiescentBot", "war_value"]

_NO_CTX = {"rival_culture_rate": 0, "rival_science_rate": 0,
           "rival_strength": 0}


# ------------------------------------------------------------------ rng
#
# Same trick as GreedyBot (docs/PYPY.md 5a/8): a Mersenne Twister that has not
# been drawn from is byte-identical to a fresh Random(0), so the object is
# reused and re-seeded only when a trial actually consumed it.  One instance
# per search level, so a deeper level's trials can never advance a shallower
# level's stream -- otherwise a candidate's resolution would depend on how many
# moves the opponent model happened to consider.
class _TrialRandom(random.Random):
    used = False

    def random(self):
        self.used = True
        return super().random()

    def getrandbits(self, k):
        self.used = True
        return super().getrandbits(k)


# Two streams per level: `2*L` drives the line _resolve actually walks, `2*L+1`
# drives the trial applies of the picks made along it.  If they shared one
# stream, which move a pick considered first would perturb the line it was
# choosing for.
_RNGS = [_TrialRandom(0) for _ in range(8)]
_PRISTINE = _RNGS[0].getstate()


def _fresh(i):
    r = _RNGS[i] if i < len(_RNGS) else _RNGS[-1]
    if r.used:
        r.setstate(_PRISTINE)
        r.used = False
    return r


# ------------------------------------------------------------- the search
#
# `box` is a one-element list holding the quiescence node budget remaining for
# the whole root decision; every level decrements the same counter, so a single
# pathological auction cannot make one root decision cost unboundedly more than
# another.

def _resolve(state, weights, end_bias, level, box, max_depth):
    """Play out ``state.pending`` until the position is quiet.

    Returns True if it reached a quiet position, False if a budget ran out.
    """
    n = 0
    while state.pending:
        if n >= max_depth or box[0] <= 0 or state.game_over:
            return False
        moves = actions.legal_moves(state)
        if not moves:
            return False
        mv = _pick(state, moves, state.decider(), weights, end_bias, level,
                   box, max_depth)
        try:
            actions.apply(state, mv, _fresh(2 * level))
        except Exception:
            return False
        n += 1
        box[0] -= 1
    return True


def _pick(state, moves, idx, weights, end_bias, level, box, max_depth):
    """Best move for `idx`.  ``level <= 0`` is a plain 1-ply pick."""
    if len(moves) == 1:
        return moves[0]
    if USE_JOURNAL:
        return _pick_journalled(state, moves, idx, weights, end_bias, level,
                                box, max_depth)
    try:
        root_ctx = rival_context(state, idx)
    except Exception:
        root_ctx = _NO_CTX
    best, best_val = None, None
    for mv in moves:
        trial = copy_state(state)
        try:
            actions.apply(trial, mv, _fresh(2 * level + 1))
        except Exception:
            continue
        ctx = root_ctx
        if level > 0 and trial.pending:
            _resolve(trial, weights, end_bias, level - 1, box, max_depth)
            # rivals moved inside the resolution, so the cached aggregates
            # computed at this node are stale
            try:
                ctx = rival_context(trial, idx)
            except Exception:
                ctx = root_ctx
        try:
            val = evaluate(trial, idx, weights, ctx)
        except Exception:
            continue
        if mv[0] == "end_turn":
            val += end_bias
        if best_val is None or val > best_val:
            best, best_val = mv, val
    return best if best is not None else moves[0]


def _pick_journalled(state, moves, idx, weights, end_bias, level, box,
                     max_depth):
    """`_pick` with the undo stack instead of `copy_state` (docs/PYPY.md 10).

    Line-for-line the copy path above, with the candidate applied to the real
    state and undone.  This one is *nested by construction*: `_resolve` calls
    it from inside a journal that `QuiescentBot.pick` (or an outer `_pick`)
    already opened, and it opens another for each candidate, which is exactly
    the strict-LIFO nesting `journal.begin` grew in section 10.  Every copy
    QuiescentBot makes is discard-shaped -- `tools/copy_census.py` measures
    100.0% at both 2p and 4p -- so there is nothing here the copy path buys.

    The copy path is deliberately left intact: it is the paranoid oracle.
    """
    try:
        root_ctx = rival_context(state, idx)
    except Exception:
        root_ctx = _NO_CTX
    best, best_val = None, None
    for mv in moves:
        j = journal.begin(state)
        try:
            try:
                actions.apply(state, mv, _fresh(2 * level + 1))
            except Exception:
                continue            # the `finally` still rolls back
            ctx = root_ctx
            if level > 0 and state.pending:
                _resolve(state, weights, end_bias, level - 1, box, max_depth)
                try:
                    ctx = rival_context(state, idx)
                except Exception:
                    ctx = root_ctx
            try:
                val = evaluate(state, idx, weights, ctx)
            except Exception:
                continue
        finally:
            journal.rollback(j)
        if mv[0] == "end_turn":
            val += end_bias
        if best_val is None or val > best_val:
            best, best_val = mv, val
    return best if best is not None else moves[0]


# --------------------------------------------------------- war lookahead

def war_value(state, idx, weights, ctx):
    """Value of the position with the declarer's pending war already fought.

    ``events.resolve_war`` is deterministic and consumes no rng, so this is a
    real (if optimistic -- the defender gets a turn in between) resolution
    rather than a priced guess.

    Public because ``engine/bots/plan.py`` reuses it verbatim: PlanBot has the
    same blind spot for the same reason (a war resolves on the declarer's next
    turn, which is past both searches' horizons), and two searches that price
    the same move class differently produce weight vectors that do not transfer
    between them -- see docs/TRANSFER_TEST.md and docs/PLAN_WAR_LOOKAHEAD.md.
    ``state`` is never mutated; the resolution happens on a scratch copy, so a
    caller may score the same position with and without it.
    """
    from .. import events
    if USE_JOURNAL:
        # Same resolution, on the state itself, undone afterwards.  The
        # "never mutated" promise in the docstring is kept by the rollback
        # rather than by the copy -- and it is checked, not asserted:
        # JOURNAL_PARANOID=1 diffs the rolled-back state against a
        # `copy_state` oracle here as everywhere else.
        j = journal.begin(state)
        try:
            events.resolve_war(state, state.players[idx], None)
            return evaluate(state, idx, weights, ctx)
        except Exception:
            return None
        finally:
            journal.rollback(j)
    scratch = copy_state(state)
    try:
        events.resolve_war(scratch, scratch.players[idx], None)
        return evaluate(scratch, idx, weights, ctx)
    except Exception:
        return None


#: historical private name, kept so nothing that imported it breaks
_war_value = war_value


# -------------------------------------------------------------- the bot

class QuiescentBot:
    """Search that resolves ``state.pending`` to quiescence before scoring."""

    name = "quiescent"

    #: nested quiescence resolutions (see the module docstring)
    LEVELS = 1
    #: pending decisions resolved per candidate move
    MAX_DEPTH = 12
    #: total quiescence ``apply`` calls per root decision
    MAX_NODES = 600
    #: resolve a declared-but-unfought war through the engine before scoring
    WAR_LOOKAHEAD = True

    def __init__(self, weights=None, rng=None, seed=None, name=None,
                 levels=None, max_depth=None, max_nodes=None,
                 war_lookahead=None):
        self.weights = dict(weights) if weights else dict(DEFAULT_WEIGHTS)
        self.rng = rng or random.Random(seed)
        if name:
            self.name = name
        if levels is not None:
            self.LEVELS = levels
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

    def pick(self, state, moves):
        if len(moves) == 1:
            return moves[0]
        idx = state.decider()
        try:
            root_ctx = rival_context(state, idx)
        except Exception:
            root_ctx = _NO_CTX
        w = self.weights
        end_bias = w.get("end_turn_bias", 0.0)
        st = self.stats
        st["decisions"] += 1
        box = [self.MAX_NODES]
        depth = self.MAX_DEPTH
        levels = self.LEVELS
        if USE_JOURNAL:
            return self._pick_journalled(state, moves, idx, root_ctx, w,
                                         end_bias, box, depth, levels)
        best, best_val = None, None
        for mv in moves:
            st["candidates"] += 1
            trial = copy_state(state)
            try:
                actions.apply(trial, mv, _fresh(2 * levels + 1))
            except Exception:
                continue
            ctx = root_ctx
            if levels > 0 and trial.pending:
                st["quiesced"] += 1
                before = box[0]
                quiet = _resolve(trial, w, end_bias, levels - 1, box, depth)
                st["qnodes"] += before - box[0]
                if not quiet:
                    st["truncated"] += 1
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

    def _pick_journalled(self, state, moves, idx, root_ctx, w, end_bias,
                         box, depth, levels):
        """`pick`'s candidate loop with the undo stack (docs/PYPY.md 10).

        The root of the nest: this opens journal depth 1, `_resolve` ->
        `_pick_journalled` opens depth 2 for each rival decision it prices, and
        `war_value` opens one inside that.  `journal.begin` refused to nest
        until section 10, which is the whole reason QuiescentBot was pinned to
        the copy path even though 100% of its copies are discard-shaped.

        Two things are deliberately NOT inside the journal and must not be:

        * `box` (the node budget) and `self.stats` are plain Python objects
          owned by the bot, not reachable from the state, so nothing journals
          them and budget consumption is identical to the copy path -- which
          matters, because a different budget is a different search.
        * `root_ctx` is computed once by `pick`, on the unmutated state,
          exactly as the copy path does.  Under the journal the root state is
          the object the candidates mutate, so capturing it before the loop is
          load-bearing rather than incidental (the same point as WeightedBot's
          `ctx`, docs/PYPY.md 9.14).
        """
        st = self.stats
        begin, rollback = journal.begin, journal.rollback
        best, best_val = None, None
        for mv in moves:
            st["candidates"] += 1
            j = begin(state)
            try:
                try:
                    actions.apply(state, mv, _fresh(2 * levels + 1))
                except Exception:
                    continue            # the `finally` still rolls back
                ctx = root_ctx
                if levels > 0 and state.pending:
                    st["quiesced"] += 1
                    before = box[0]
                    quiet = _resolve(state, w, end_bias, levels - 1, box,
                                     depth)
                    st["qnodes"] += before - box[0]
                    if not quiet:
                        st["truncated"] += 1
                    try:
                        ctx = rival_context(state, idx)
                    except Exception:
                        ctx = root_ctx
                try:
                    val = evaluate(state, idx, w, ctx)
                except Exception:
                    continue
                if self.WAR_LOOKAHEAD and mv[0] == "war":
                    wv = _war_value(state, idx, w, ctx)
                    if wv is not None:
                        val = wv
            finally:
                rollback(j)
            if mv[0] == "end_turn":
                val += end_bias
            if best_val is None or val > best_val:
                best, best_val = mv, val
        if best is None:
            return self.rng.choice(moves)
        return best
