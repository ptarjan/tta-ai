"""NeuralPlanBot: PlanBot's whole-turn beam with the value net as the leaf.

This is docs/NEURAL_EVAL.md "Stage 3" and the thing every prior measurement in
this repo points at.  Two independent lines of evidence:

* Search-only A/B on identical linear weights (docs/BOT_ARCHITECTURE.md 3):
  ``PlanBot`` beats the same vector at 1 ply **88.6% +/- 3.1** (n=400), culture
  194.5 vs 125.8.  The width ablation splits that: ``width=1`` -- horizon
  equalisation + determinization + quiescence, *no lookahead* -- is already
  62.3%, and the multi-action beam adds the other ~23 points.
* The 1-ply-lineage vector scores 139.8 culture at 1 ply and **189.0** under
  ``plan:width=8`` (docs/BEHAVIOUR_CLONE.md), above the human 159.5.

``NeuralBot`` is 1 ply, so it eats *both* defects PlanBot fixes and has no
``end_turn_bias`` to paper over the first one.  This class is the same search
as :class:`engine.bots.plan.PlanBot` with exactly one thing swapped: the leaf
scalar is the network's predicted final-culture margin instead of
``weighted.evaluate``.

Three deliberate differences from ``PlanBot``, all forced by the evaluator:

1. **The beam is expanded ply-at-a-time and scored in ONE batch per ply.**
   PlanBot scores each node the moment it is generated, which costs 30us with
   a linear evaluator and a whole forward pass with a net.  Here every node of
   a ply is encoded, then scored in a single batched call.  This changes no
   decision -- the same nodes get the same scores -- only the wall clock.
2. **War lookahead substitutes the ENCODING, not the score.**  ``PlanBot``
   calls ``quiescent.war_value``, which resolves the declared war on a scratch
   copy and returns ``evaluate(scratch)``.  The neural analogue encodes that
   same scratch copy and lets the net score it, so both searches price the move
   class the same way (docs/PLAN_WAR_LOOKAHEAD.md, docs/TRANSFER_TEST.md: two
   searches that disagree about one move class do not share an evaluator).
3. **The pending stack is drained with the LINEAR 1-ply pick**, exactly as
   ``PlanBot._quiesce`` does, using ``DEFAULT_WEIGHTS``.  It touches ~1.8% of
   candidates at 2p (docs/DEEPER_SEARCH.md 3.1) and keeping it identical to
   PlanBot's makes ``nplan:CKPT`` vs ``plan:VECTOR`` an honest evaluator-only
   A/B.  It is also the same on both sides of any candidate-vs-best gate.
   This DOES include threading the search root's ``root_row``/``root_counts``
   down into every quiesce-drain ``rival_context`` call, the same information-
   leak guard ``PlanBot._quiesce`` documents on its own ``root_row`` parameter
   -- fixed 2026-08-05: this class used to call ``rival_context(st, d)`` with
   neither, which is not what "identical to PlanBot's" a few lines up
   promised, and let a quiesce-drain deep in the beam price an opponent's pick
   off a row a trial ``end_turn`` had already replenished with the real
   deck's next cards, rather than the row the actual decider could see.  See
   ``tests/test_neural_plan.py::test_quiesce_forwards_root_row_and_root_counts``.

The evaluator is injected (anything with ``.value(list_of_encodings) ->
list[float]``, i.e. an :class:`engine.bots.neural_net.NeuralValue`), so this
module never imports torch.
"""
from __future__ import annotations

import random

from .. import actions
from . import pending
from .fastcopy import copy_state
from .neural_encode import encode
from .plan import determinize as _determinize
from .weighted import DEFAULT_WEIGHTS, evaluate, rival_context

__all__ = ["NeuralPlanBot"]

_NO_CTX = {"rival_culture_rate": 0, "rival_science_rate": 0,
           "rival_strength": 0}

_TRIAL = random.Random(0)


class NeuralPlanBot:
    """Beam search to the end of my own turn, leaves scored by a value net."""

    name = "nplan"

    #: beam width kept between plies (PlanBot's tuned value)
    WIDTH = 8
    #: hard cap on sequence length
    MAX_PLIES = 16
    #: hard cap on `apply` calls per root decision.  Lower than PlanBot's 4000
    #: because a node costs an encode + a forward instead of a 30us dot
    #: product; PlanBot measured ~280 nodes actually expanded per decision
    #: (docs/BOT_ARCHITECTURE.md 4.4), so 1200 is still slack, not a squeeze.
    MAX_NODES = 1200
    #: determinizations averaged over
    SAMPLES = 1
    WAR_LOOKAHEAD = True
    #: drain a pending decision of MINE before pricing candidates, exactly as
    #: `_beam` already does for every node it scores.  `None` means "the shared
    #: default in `engine.bots.pending`", which is where it lives so that this
    #: class and `PlanBot` -- which is where this short-circuit was copied FROM
    #: -- cannot answer the question differently.  Do not put a bool here.
    QUIET_PENDING = None
    #: re-shuffle the unseen piles before pricing a non-ordinary-turn decision.
    #: This used to be `True` here and `False` on `PlanBot` -- the drift that
    #: made "the same short-circuit, copied out" not actually the same.  This
    #: class was the one that was right, so `PlanBot` moved to match and both
    #: are now `None`: ask `engine.bots.pending`.  Do not put a bool here.
    PENDING_DETERMINIZE = None

    def __init__(self, value, seed=None, rng=None, name=None, width=None,
                 samples=None, determinize=True, war_lookahead=None,
                 max_nodes=None, drain_weights=None, allow_resign=False,
                 quiet_pending=None):
        self.value = value
        self.rng = rng or random.Random(seed)
        self.width = self.WIDTH if width is None else width
        self.samples = self.SAMPLES if samples is None else samples
        self.determinize = determinize
        self.allow_resign = allow_resign
        if war_lookahead is not None:
            self.WAR_LOOKAHEAD = war_lookahead
        if max_nodes is not None:
            self.MAX_NODES = max_nodes
        if quiet_pending is not None:
            self.QUIET_PENDING = quiet_pending
        self.drain_weights = dict(drain_weights or DEFAULT_WEIGHTS)
        if name:
            self.name = name
        # instrumentation
        self.decisions = 0
        self.nodes = 0
        self.evals = 0
        self.searches = 0
        self.wars_priced = 0

    # -- harness adapters ---------------------------------------------------
    def choose(self, state, moves, rng=None):
        return self.pick(state, moves)

    def __call__(self, state):
        return self.pick(state, actions.legal_moves(state))

    # -- leaf ---------------------------------------------------------------
    def _leaf_enc(self, t, me):
        """Encoding the net should score for position ``t`` from ``me``'s view.

        Substitutes the war-resolved position when I hold a declared war, the
        neural mirror of ``quiescent.war_value`` -- including its settlement
        of ``War over Technology``'s spoils choice, without which the net
        would be handed a position where the war has been fought and has paid
        NOTHING (the decision would still be sitting on ``scratch.pending``).
        Same lower-bound caveat as there; see ``interact.settle_war_spoils``.
        """
        if (self.WAR_LOOKAHEAD and not t.game_over
                and t.players[me].war_declared_by_me is not None):
            from .. import events, interact
            scratch = copy_state(t)
            try:
                events.resolve_war(scratch, scratch.players[me], None)
                interact.settle_war_spoils(scratch, None)
                self.wars_priced += 1
                t = scratch
            except Exception:
                pass
        try:
            return encode(t, me)
        except Exception:
            return None

    def _score_many(self, states, me):
        """Batched leaf values.  Returns a list aligned with ``states``, with
        ``None`` where the position could not be encoded."""
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
        vals = self.value.value(encs)
        for i, v in zip(where, vals):
            out[i] = v
        return out

    # -- search -------------------------------------------------------------
    def pick(self, state, moves):
        if not self.allow_resign and len(moves) > 1:
            live = [m for m in moves if m[0] != "resign"]
            if live:
                moves = live
        if len(moves) == 1:
            return moves[0]
        me = state.decider()
        self.decisions += 1
        # Computed once at the root (from `state`, never from a determinized
        # copy) and threaded down to every quiesce-drain inside `_beam`/
        # `_one_ply_neural` below -- `rival_context`'s own doc comment: this
        # is what stops a quiesce-drain deep in the beam from pricing an
        # opponent's pick off a row a trial `end_turn` has already
        # replenished. Mirrors `PlanBot.pick`'s identical `ctx` computation;
        # this class did not have it until 2026-08-05 (see this module's top
        # doc comment, point 3).
        try:
            ctx = rival_context(state, me)
        except Exception:
            ctx = dict(_NO_CTX)

        # Not my ordinary turn: there is no turn to plan, so fall back to the
        # 1-ply neural pick at a common horizon of "now" (this is exactly what
        # NeuralBot does, and what PlanBot._one_ply does for the linear bot)
        # -- and when the pending decision is MINE, drain it first, because
        # `_beam` below already quiesces every node it scores.  The policy is
        # `engine.bots.pending`, shared with PlanBot so that the two cannot
        # answer this differently again; only the scorer here is ours.
        if pending.not_my_turn(state, me):
            # `prepare_root` does what the three lines here used to do, and it
            # reads `self.determinize` itself now (`pending.wants_determinize`
            # gate 1), so the bot-wide A/B switch is honoured in ONE place
            # rather than spelled here and forgotten in `PlanBot`.
            root = pending.prepare_root(self, state, copy_state,
                                        _determinize, self.rng)
            return pending.fallback_pick(
                self, state,
                plain=lambda: self._one_ply_neural(root, moves, me),
                quiet=lambda: self._one_ply_neural(
                    root, moves, me, quiet=True,
                    root_row=ctx.get("root_row"),
                    root_counts=ctx.get("root_counts")))

        totals, seen = {}, {}
        drng = random.Random((state.seed or 0) * 7919 + state.turn * 31 + me)
        for _s in range(self.samples):
            root = copy_state(state)
            if self.determinize:
                _determinize(root, drng)
            beam = self._beam(root, moves, me,
                               root_row=ctx.get("root_row"),
                               root_counts=ctx.get("root_counts"))
            for mv, v in beam.items():
                totals[mv] = totals.get(mv, 0.0) + v
                seen[mv] = seen.get(mv, 0) + 1
        if not totals:
            return moves[0]
        # index-keyed argmax: move tuples are not orderable against each other,
        # so never let `max` fall through to comparing them on a tie.
        bi, bv = 0, None
        for i, mv in enumerate(moves):
            if mv not in totals:
                continue
            v = totals[mv] / seen[mv]
            if bv is None or v > bv:
                bi, bv = i, v
        return moves[bi]

    def _one_ply_neural(self, state, moves, me, quiet=False,
                         root_row=None, root_counts=None):
        """One ply, batched.  ``quiet`` drains the pending stack per candidate.

        ``quiet=True`` is the neural mirror of ``PlanBot._one_ply_quiet``: it
        calls the same ``_quiesce`` this class already runs on every node of
        its own beam (see ``_beam``), so a real decision is priced the way a
        searched one is.  See `engine.bots.pending` for why that matters and
        what skipping it measured.

        ``root_row``/``root_counts`` are threaded straight through to
        ``_quiesce`` -- see that method's own doc comment.
        """
        states, valid = [], []
        for mv in moves:
            t = copy_state(state)
            try:
                actions.apply(t, mv, _TRIAL)
                if quiet:
                    self._quiesce(t, root_row=root_row,
                                  root_counts=root_counts)
            except Exception:
                continue
            states.append(t)
            valid.append(mv)
        if not states:
            return moves[0]
        vals = self._score_many(states, me)
        best, bv = None, None
        for mv, v in zip(valid, vals):
            if v is None:
                continue
            if bv is None or v > bv:
                best, bv = mv, v
        return best if best is not None else moves[0]

    def _beam(self, root, moves, me, root_row=None, root_counts=None):
        """{first_move: best terminal leaf value reachable through it}.

        ``root_row``/``root_counts`` are threaded straight through to
        ``_quiesce`` -- see that method's own doc comment.
        """
        self.searches += 1
        budget = self.MAX_NODES
        frontier = [(root, None)]
        best = {}
        for _ply in range(self.MAX_PLIES):
            gen_states, gen_first = [], []
            for st, first in frontier:
                mvs = moves if first is None else actions.legal_moves(st)
                for mv in mvs:
                    if mv[0] == "resign":
                        continue
                    if budget <= 0:
                        break
                    budget -= 1
                    self.nodes += 1
                    t = copy_state(st)
                    try:
                        actions.apply(t, mv, _TRIAL)
                    except Exception:
                        continue
                    self._quiesce(t, root_row=root_row,
                                  root_counts=root_counts)
                    gen_states.append(t)
                    gen_first.append(mv if first is None else first)
                if budget <= 0:
                    break
            if not gen_states:
                break
            vals = self._score_many(gen_states, me)
            nxt = []
            for t, f, v in zip(gen_states, gen_first, vals):
                if v is None:
                    continue
                if t.game_over or t.current != me:
                    if f not in best or v > best[f]:
                        best[f] = v
                else:
                    nxt.append((v, t, f))
            if not nxt or budget <= 0:
                break
            nxt.sort(key=lambda e: -e[0])
            frontier = [(t, f) for _v, t, f in nxt[:self.width]]
        return best

    def _quiesce(self, st, cap=12, root_row=None, root_counts=None):
        """Drain the pending stack with plain LINEAR 1-ply picks (see 3 above).

        ``root_row``/``root_counts`` are the search ROOT's visible card row
        and counting outlook, threaded down from ``pick`` (via ``_beam``/
        ``_one_ply_neural``) so the opponent picks made in here price the
        same information the real decider could see -- not information a
        trial ``end_turn`` deep in the beam has already changed. Mirrors
        ``PlanBot._quiesce``'s identical parameters and identical reasoning;
        this method did not have them until 2026-08-05 (see this module's
        top doc comment, point 3) -- a plain ``rival_context(st, d)`` here
        let a quiesce-drain price an opponent's pick off a row this search
        should not be able to see yet.
        """
        w = self.drain_weights
        n = 0
        while st.pending and n < cap and not st.game_over:
            n += 1
            d = st.decider()
            mvs = actions.legal_moves(st)
            if not mvs:
                return
            if len(mvs) == 1:
                try:
                    actions.apply(st, mvs[0], _TRIAL)
                except Exception:
                    return
                continue
            try:
                dctx = rival_context(st, d, root_row, root_counts)
            except Exception:
                dctx = dict(_NO_CTX)
            best, bv = None, None
            for mv in mvs:
                t = copy_state(st)
                try:
                    actions.apply(t, mv, _TRIAL)
                    v = evaluate(t, d, w, dctx)
                except Exception:
                    continue
                if bv is None or v > bv:
                    best, bv = mv, v
            try:
                actions.apply(st, best if best is not None else mvs[0], _TRIAL)
            except Exception:
                return
