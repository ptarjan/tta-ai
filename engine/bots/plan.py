"""PlanBot: beam search over whole-turn action *sequences*, scored at one
fixed horizon, on a determinized state.

Three separate defects of `WeightedBot` are addressed at once, because they
are all consequences of the same thing -- scoring a candidate on the child
state that `apply` happens to leave behind.

1. **Horizon asymmetry.**  `apply(("end_turn",))` runs my whole end-of-turn
   economy *and* advances the turn, so `end_turn`'s child state has banked a
   production phase that no other candidate has.  `docs/WASTED_ACTIONS.md` §1
   measures the resulting flattery at +12.6 evaluation points at 2p (+26.3 in
   Age IV), against real moves worth fractions of a point, and the trained
   `end_turn_bias` (-12.9 at 2p) is a *constant* fighting a term that scales
   with the economy.  Here every candidate is scored at the same horizon --
   the state immediately after my turn ends -- so the asymmetry does not
   exist and no bias constant is needed.  Sequences and `end_turn` alone are
   the same kind of object: "end turn now" is just the length-1 sequence.

2. **One action of lookahead inside a turn that has four.**  A civil action
   is rarely worth anything on its own: taking a card pays off when it is
   played, increasing population pays off when the worker is placed, building
   a lab pays off at production.  A greedy bot commits to action 1 before it
   knows what actions 2..4 will be.  Beam search over sequences prices the
   *plan*, which is the unit the rules actually meter (civil actions).

   `docs/WASTED_ACTIONS.md` §6 measured two earlier attempts at defect 1 and
   both made the bot much weaker (29.8% and 38.4% against a 50% null, n=400).
   Neither of them did this: `HorizonBot` rolled each *single* candidate
   through production but still chose one action at a time.  Fixing the
   horizon without gaining the lookahead removes a filter and adds nothing,
   which is exactly what those numbers look like.

3. **Hidden information.**  `civil_deck` and `military_deck` are full ordered
   lists inside `GameState`, and `fastcopy.copy_state` copies them verbatim,
   so a trial `apply` that draws draws the *real* next card.
   `tools/infoleak.py` measures 94.9% of `end_turn` candidates doing exactly
   that at 2p (71.1% of all decisions have at least one such candidate), so
   the single most-evaluated move in the game is priced against a peek at the
   future.  Here the unseen decks are re-shuffled before the search, which is
   the determinization step an information-set search needs anyway.

Cost is bounded by ``WIDTH`` (beam) x branching x turn length; see
`docs/BOT_ARCHITECTURE.md` for the measured numbers.
"""
from __future__ import annotations

import random

from .. import actions
from .fastcopy import copy_state
from .weighted import DEFAULT_WEIGHTS, evaluate, rival_context

__all__ = ["PlanBot", "determinize"]

_NO_CTX = {"rival_culture_rate": 0, "rival_science_rate": 0,
           "rival_strength": 0}


class _TrialRandom(random.Random):
    used = False

    def random(self):
        self.used = True
        return super().random()

    def getrandbits(self, k):
        self.used = True
        return super().getrandbits(k)


_TRIAL = _TrialRandom(0)
_PRISTINE = _TRIAL.getstate()


def _rng():
    if _TRIAL.used:
        _TRIAL.setstate(_PRISTINE)
        _TRIAL.used = False
    return _TRIAL


def determinize(state, rng):
    """Re-shuffle what the mover cannot see.

    Public: the card row, every board, culture/science, everyone's *count* of
    military cards.  Hidden: the ORDER of the two draw decks.  Rival military
    hands are hidden too, but `weighted.features` reads only public rival
    aggregates, so re-dealing them would change nothing that is read.
    """
    if state.civil_deck:
        rng.shuffle(state.civil_deck)
    if state.military_deck:
        rng.shuffle(state.military_deck)
    return state


class PlanBot:
    """Beam search to the end of my own turn, scored on a determinized state."""

    name = "plan"

    #: beam width kept between plies
    WIDTH = 8
    #: hard cap on sequence length (a turn is ~2-7 moves; 16 is slack)
    MAX_PLIES = 16
    #: hard cap on `apply` calls per root decision
    MAX_NODES = 4000
    #: how many determinizations to average the search over (1 = one sample)
    SAMPLES = 1
    #: fall back to a plain 1-ply pick when it is not my ordinary turn
    #: (a pending decision owned by somebody else has no turn to plan)

    def __init__(self, weights=None, rng=None, seed=None, name=None,
                 width=None, samples=None, determinize=True):
        self.weights = dict(weights) if weights else dict(DEFAULT_WEIGHTS)
        self.rng = rng or random.Random(seed)
        self.width = self.WIDTH if width is None else width
        self.samples = self.SAMPLES if samples is None else samples
        self.determinize = determinize
        self.nodes = 0
        self.searches = 0
        if name:
            self.name = name

    # -- harness adapters -------------------------------------------------
    def choose(self, state, moves, rng=None):
        return self.pick(state, moves)

    def __call__(self, state):
        return self.pick(state, actions.legal_moves(state))

    # -- search -----------------------------------------------------------
    def pick(self, state, moves):
        if len(moves) == 1:
            return moves[0]
        me = state.decider()
        w = self.weights
        try:
            ctx = rival_context(state, me)
        except Exception:
            ctx = dict(_NO_CTX)

        # Not my ordinary turn (someone else's pending decision, or mine but
        # nested inside another player's turn): there is no turn to plan, so
        # score the candidates one ply deep at a common horizon of "now".
        if state.pending or state.current != me:
            return self._one_ply(state, moves, me, w, ctx)

        totals = {mv: 0.0 for mv in moves}
        seen = {mv: 0 for mv in moves}
        drng = random.Random(state.seed * 7919 + state.turn * 31 + me)
        for _s in range(self.samples):
            root = copy_state(state)
            if self.determinize:
                determinize(root, drng)
            best = self._beam(root, moves, me, w, ctx)
            for mv, v in best.items():
                totals[mv] += v
                seen[mv] += 1
        scored = [(totals[mv] / seen[mv], mv) for mv in moves if seen[mv]]
        if not scored:
            return moves[0]
        return max(scored, key=lambda t: t[0])[1]

    def _one_ply(self, state, moves, me, w, ctx):
        best, bv = None, None
        for mv in moves:
            t = copy_state(state)
            try:
                actions.apply(t, mv, _rng())
                v = evaluate(t, me, w, ctx)
            except Exception:
                continue
            if bv is None or v > bv:
                best, bv = mv, v
        return best if best is not None else moves[0]

    def _beam(self, root, moves, me, w, ctx):
        """Return {first_move: best terminal score reachable through it}."""
        self.searches += 1
        budget = self.MAX_NODES
        # frontier entries: (ordering_score, state, first_move)
        frontier = [(0.0, root, None)]
        best = {}
        for _ply in range(self.MAX_PLIES):
            nxt = []
            for _, st, first in frontier:
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
                        actions.apply(t, mv, _rng())
                    except Exception:
                        continue
                    f = mv if first is None else first
                    # resolve decisions owned by anybody, so the position is
                    # quiet before it is either scored or expanded
                    self._quiesce(t, w)
                    if t.game_over or t.current != me:
                        try:
                            v = evaluate(t, me, w, ctx)
                        except Exception:
                            continue
                        if f not in best or v > best[f]:
                            best[f] = v
                    else:
                        try:
                            v = evaluate(t, me, w, ctx)
                        except Exception:
                            continue
                        nxt.append((v, t, f))
            if not nxt or budget <= 0:
                break
            nxt.sort(key=lambda e: -e[0])
            frontier = nxt[:self.width]
        return best

    def _quiesce(self, st, w, cap=12):
        """Drain the pending stack with plain 1-ply picks for whoever decides."""
        n = 0
        while st.pending and n < cap and not st.game_over:
            n += 1
            d = st.decider()
            mvs = actions.legal_moves(st)
            if not mvs:
                return
            if len(mvs) == 1:
                actions.apply(st, mvs[0], _rng())
                continue
            try:
                dctx = rival_context(st, d)
            except Exception:
                dctx = dict(_NO_CTX)
            mv = self._one_ply(st, mvs, d, w, dctx)
            try:
                actions.apply(st, mv, _rng())
            except Exception:
                return
