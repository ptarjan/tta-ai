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

3. **Hidden information.**  `civil_deck`, `military_deck` and
   `current_events` are full ordered lists inside `GameState`, and
   `fastcopy.copy_state` copies them verbatim, so a trial `apply` that draws
   would draw the *real* next card.  `tools/infoleak.py --true-card` measures
   it as 100% of draws on an undeterminized root -- not a rate, an identity.
   So `pick` re-shuffles the unseen piles into `root` *before* `_beam` sees
   it, which is the determinization step an information-set search needs
   anyway, and every trial `apply` in the search then draws a sample rather
   than the truth (measured 28.6% / 17.2% / 38.3% at 2p, against chance
   floors set by each pile's length).

   Read that as a claim about THIS bot only.  `WeightedBot` and
   `QuiescentBot` do not determinize at all and still draw the true card on
   100% of trial draws; `tools/infoleak.py`'s old headline number (94.9% of
   `end_turn` candidates at 2p) is measured on `WeightedBot` and counts
   candidates that DRAW, which is a number determinization cannot move.  It
   was quoted here for years as if it described the beam.  It never did.

Cost is bounded by ``WIDTH`` (beam) x branching x turn length; see
`docs/BOT_ARCHITECTURE.md` for the measured numbers.

4. **War priced as pure cost.**  ``_quiesce`` below drains ``state.pending``,
   so pacts, colony bids, aggressions and action cards all reach a quiet
   position before they are scored.  A **war** does not: it pushes nothing
   onto the pending stack and resolves at the start of the declarer's *next*
   turn, a full round past this search's horizon (end of my current turn).  So
   without ``WAR_LOOKAHEAD`` a war candidate is scored with its whole cost --
   a military card and 1-3 military actions gone -- and none of its loot,
   exactly the defect QuiescentBot's module docstring describes for pacts.

   `docs/TRANSFER_TEST.md` measured what that costs: a vector trained under
   ``QuiescentBot`` (which *does* price wars) is +36.3 +/- 4.8 margin better
   than a 1-ply-trained vector under quiescence and 32.5 +/- 6.9 *worse* under
   PlanBot -- the sign of the difference flips with the search, and ablating
   ``QuiescentBot.WAR_LOOKAHEAD`` alone accounts for 52.8 +/- 4.3 of it.  Two
   searches that disagree about one move class do not share a weight vector.
   ``WAR_LOOKAHEAD`` here calls the identical helper
   (``quiescent.war_value``) so the two price the move class the same way; see
   `docs/PLAN_WAR_LOOKAHEAD.md` for the before/after measurement.
"""
from __future__ import annotations

import random

from .. import actions, cards, census, journal
from . import pending
from .fastcopy import copy_state
from .quiescent import war_value
from .trial import USE_JOURNAL
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


#: Every field of `GameState` whose ORDER a player at the table cannot see,
#: and which some `actions.apply` can therefore consume during a trial.
#: :func:`determinize` re-orders exactly these and nothing else.
#:
#: It is a named tuple rather than three inline `if`s so that
#: `tests/test_search_root_is_determinized.py` can assert the set, and so that
#: adding a fourth hidden pile to the engine is a decision somebody has to
#: make in writing rather than an omission nobody notices.  The test also
#: asserts the COMPLEMENT: every other list/dict field of `GameState` is
#: identical across `determinize`, which is what stops a future "just shuffle
#: everything" from re-dealing the visible card row.
#:
#: WHY ``future_events`` IS NOT HERE, since it is the obvious fourth candidate.
#: Nothing ever pops it.  `events._recycle_future_events` is the only reader,
#: and it *already* shuffles the list before promoting it to
#: ``current_events``, so its order is not information -- re-ordering it here
#: would be re-ordering something the engine is about to re-order anyway, at
#: the cost of rng draws.  Its *contents* are hidden (a seeded event is placed
#: face down), and so are the rivals' hands; sampling those is a strictly
#: bigger job than this function does -- it permutes what is unseen, it does
#: not re-deal it -- and it is written down as an open item in
#: `docs/AGGRESSION_RATE.md` 9a rather than half-done here.
HIDDEN_ORDER = ("civil_deck", "military_deck", "current_events")


def determinize(state, rng):
    """Re-shuffle what the mover cannot see.

    Public: the card row, every board, culture/science, everyone's *count* of
    military cards, the two discard records, the tactics on offer, the events
    already resolved.  Hidden: the ORDER of the two draw decks **and of the
    current events deck**.  Rival military hands are hidden too, but
    `weighted.features` reads only public rival aggregates, so re-dealing them
    would change nothing that is read.

    ``current_events`` is the one that was missing, and it was the whole
    remaining leak on the beam path.  The two draw decks have been shuffled
    here since PlanBot was written, so the module docstring's defect 3 was
    fixed for cards and silently untrue for events: `engine/events.py`'s
    ``reveal_current_event`` pops the pile at the top of every turn, so every
    ``end_turn`` a beam ever expands revealed the REAL next event.  Measured
    on the instrument that can tell the difference (`tools/infoleak.py
    --true-card`), 2p/8 games: on a determinized root the trial drew the true
    top CIVIL card on 28.6% of civil draws and the true top EVENT on **100.0%**
    of event draws.  100% is not a leak rate, it is the signature of a field
    nobody was shuffling; it is 38.3% now, against a ~33% chance floor for a
    3-card pile.

    THE EVENT PILE IS AGE-ORDERED AND THAT ORDER IS PUBLIC.
    ``events._recycle_future_events`` shuffles the pile and then sorts it by
    descending age level, because ``pop()`` takes from the end, so the oldest
    age comes out first.  Everyone at the table knows an Age I event precedes
    an Age II one.  Shuffling the pile flat would therefore *destroy* public
    information as well as hiding private information, and would let the
    search see Age III events arrive early.  So this repeats the engine's own
    two lines -- shuffle, then stable-sort by descending level -- which
    randomises within each age band and leaves the bands where they were.
    """
    if state.civil_deck:
        rng.shuffle(state.civil_deck)
    if state.military_deck:
        rng.shuffle(state.military_deck)
    ev = state.current_events
    if len(ev) > 1:
        rng.shuffle(ev)
        # `list.sort` is stable, so this restores the age bands exactly as
        # `events._recycle_future_events` built them and permutes only within.
        # The key is character-for-character the engine's own.
        level_of = cards.db().level_of
        ev.sort(key=lambda n: -level_of(n))
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
    #: score an unresolved war of mine through the engine's own `resolve_war`
    #: (defect 4 in the module docstring).  `plan:FILE,war=0` turns it off.
    WAR_LOOKAHEAD = True
    #: fall back to a plain 1-ply pick when it is not my ordinary turn
    #: (a pending decision owned by somebody else has no turn to plan)
    #:
    #: ...and when that pending decision is MINE, drain the stack before
    #: scoring, exactly as `_child` already does for every node inside the
    #: beam.  See `_one_ply_quiet` for what this is for and what it costs.
    #: `plan:FILE,width=2,qp=1` turns it on per-instance.
    #:
    #: `None` means "the shared default in `engine.bots.pending`", which is
    #: where it lives so that this class and `NeuralPlanBot` -- which had this
    #: short-circuit copied out -- cannot answer the question differently.
    #: Do not put a bool here.
    QUIET_PENDING = None
    #: re-shuffle the unseen piles before pricing a non-ordinary-turn decision,
    #: as `pick`'s beam path a dozen lines below already does.
    #:
    #: `None` means "the shared default in `engine.bots.pending`", for exactly
    #: the reason `QUIET_PENDING` above is `None`: this used to be `False` here
    #: and `True` on `NeuralPlanBot`, which is the drift that made "the same
    #: short-circuit, copied out" not actually the same.  Both are `None` now
    #: and `pending.DETERMINIZE` is the one answer.  `plan:FILE,qd=0` turns it
    #: off per-instance for an A/B; `plan:FILE,det=0` turns off *all*
    #: determinization, this path included.  Do not put a bool here.
    PENDING_DETERMINIZE = None

    def __init__(self, weights=None, rng=None, seed=None, name=None,
                 width=None, samples=None, determinize=True,
                 war_lookahead=None, quiet_pending=None,
                 pending_determinize=None):
        self.weights = dict(weights) if weights else dict(DEFAULT_WEIGHTS)
        self.rng = rng or random.Random(seed)
        self.width = self.WIDTH if width is None else width
        self.samples = self.SAMPLES if samples is None else samples
        self.determinize = determinize
        if war_lookahead is not None:
            self.WAR_LOOKAHEAD = war_lookahead
        if quiet_pending is not None:
            self.QUIET_PENDING = quiet_pending
        if pending_determinize is not None:
            self.PENDING_DETERMINIZE = pending_determinize
        self.nodes = 0
        self.searches = 0
        self.wars_priced = 0
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
        # ...and when the pending decision is mine, drain it first.  The
        # policy is `engine.bots.pending`, shared with `NeuralPlanBot`; only
        # the two scorers below are this class's own.
        if pending.not_my_turn(state, me):
            root = pending.prepare_root(self, state, copy_state, determinize,
                                        self.rng)
            return pending.fallback_pick(
                self, state,
                plain=lambda: self._one_ply(root, moves, me, w, ctx),
                quiet=lambda: self._one_ply_quiet(root, moves, me, w, ctx))

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
        chosen = max(scored, key=lambda t: t[0])[1]
        if census.ENABLED:
            # After the decision, return value discarded: cannot alter play.
            census.record(state, me, w, moves,
                          [(mv, v) for v, mv in scored], chosen)
        return chosen

    def _one_ply_quiet(self, state, moves, me, w, ctx):
        """`_one_ply`, but drain the pending stack before scoring.

        THE INCONSISTENCY THIS EXISTS TO REMOVE.  `_child` scores every node
        inside the beam as `copy -> apply -> _quiesce -> _score`, so within its
        own search this bot always prices an aggression by *playing the defence
        out*.  At a REAL decision `pick` short-circuits to `_one_ply`, which is
        `copy -> apply -> evaluate` with no drain -- so the identical position
        is priced two different ways depending on whether the bot is the one
        being searched or the one deciding.

        Where that bites is the defender's own `kind="defense"` decision.
        `interact._defense_move` leaves the decision on `state.pending` while
        the defender still has room and cards, so after `("defend", card)` the
        aggression has NOT resolved: the position `evaluate` sees is one
        military card poorer with the attack still hanging.  `("defend_done",)`
        by contrast pops the stack and calls `events.finish_aggression`
        immediately, so its position shows the full loss.  Nothing in
        `features()` reads `pend["atk"]` or `pend["dfn"]`, so the choice cannot
        be about whether the defence SUCCEEDS.  What is left is a choice about
        whether to defer bad news, and it points the wrong way: measured over
        300 games of `plan:width=2` at 2p (`tools/aggression_census.py`), the
        defender spent cards in 34 of 39 arithmetically HOPELESS defences and
        in 3 of 52 WINNABLE ones, and held off 0 of 91 aggressions.

        Draining first makes the real decision agree with the searched one.
        It is not new knowledge and not a new weight: it is the same
        `_quiesce` this class already trusts, called at the one place it was
        being skipped.

        Cost is bounded: `_quiesce` caps at 12 drained decisions, and a
        pending stack is short, so this is a small constant multiple of
        `_one_ply` on the (rare) decisions where `state.pending` is non-empty.
        """
        best, bv = None, None
        for mv in moves:
            t = copy_state(state)
            try:
                actions.apply(t, mv, _rng())
                self._quiesce(t, w, root_row=ctx.get("root_row"))
                v = self._score(t, me, w, ctx)
            except Exception:
                continue
            if bv is None or v > bv:
                best, bv = mv, v
        return best if best is not None else moves[0]

    def _one_ply(self, state, moves, me, w, ctx):
        if USE_JOURNAL:
            return self._one_ply_journalled(state, moves, me, w, ctx)
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

    def _one_ply_journalled(self, state, moves, me, w, ctx):
        """`_one_ply` with the undo stack (docs/PYPY.md 10).

        Pure copy-apply-score-discard, so it needs nothing beyond `begin` /
        `rollback` -- but it is almost always *nested*, because `_quiesce`
        calls it from inside a beam node's journal.  That nesting is what
        section 10 added and what 9.13 wrongly read as a property of the bot
        rather than of the journal.
        """
        begin, rollback = journal.begin, journal.rollback
        best, bv = None, None
        for mv in moves:
            j = begin(state)
            try:
                try:
                    actions.apply(state, mv, _rng())
                    v = evaluate(state, me, w, ctx)
                except Exception:
                    continue        # the `finally` still rolls back
            finally:
                rollback(j)
            if bv is None or v > bv:
                best, bv = mv, v
        return best if best is not None else moves[0]

    def _beam(self, root, moves, me, w, ctx):
        """Return {first_move: best terminal score reachable through it}."""
        if USE_JOURNAL:
            return self._beam_journalled(root, moves, me, w, ctx)
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
                    self._quiesce(t, w, root_row=ctx.get("root_row"))
                    try:
                        v = self._score(t, me, w, ctx)
                    except Exception:
                        continue
                    if t.game_over or t.current != me:
                        if f not in best or v > best[f]:
                            best[f] = v
                    else:
                        nxt.append((v, t, f))
            if not nxt or budget <= 0:
                break
            nxt.sort(key=lambda e: -e[0])
            frontier = nxt[:self.width]
        return best

    def _beam_journalled(self, root, moves, me, w, ctx):
        """`_beam` with the undo stack, and a re-apply for the survivors.

        The beam is the one place in this repo where the flat
        copy-apply-score-discard shape does NOT hold, and it is worth being
        precise about why, because it is what section 6.3 meant by "design B's
        only advantage is holding many trial states alive simultaneously".
        A beam holds ``width`` states alive at once, so *some* of these copies
        are load-bearing.  How many is a measurement, not a guess:
        `tools/copy_census.py` counts, over whole games at the league's own
        ``plan:width=2``,

            2p   47790 `_beam` copies, 10.6% expanded again (89.4% discarded)
            4p   43101 `_beam` copies, 10.0% expanded again (90.0% discarded)

        so ~90% of them are journal-shaped and ~10% genuinely have to persist.

        The scheme: expand every child under a journal (no copy at all), keep
        only its score and the (parent, move) pair that produced it, and after
        the prune **re-apply** the ~``width`` winners onto fresh copies.  That
        trades ``width`` extra ``apply``+``_quiesce`` per ply for
        ``nodes - width`` copies, which the census says is a ~9:1 trade.  The
        alternative -- copy the survivor from inside the journal, before the
        rollback -- would work too now that `copy_state` detaches (section 10),
        but it copies at the same rate as this does and needs the copy to
        happen at a point where the state is half-unwound, so there is nothing
        to gain and a sharper edge to cut yourself on.

        Re-applying is exact, not approximate, and rests on one property:
        ``_rng()`` hands out a Mersenne Twister that is always at the start of
        the ``Random(0)`` stream (it re-seeds lazily, iff the previous consumer
        drew -- docs/PYPY.md 8.2).  So ``apply(child_of(parent, mv))`` is a
        deterministic function of ``(parent, mv)`` and calling it twice cannot
        diverge, however many rng calls happened in between.

        ``self.nodes``, ``self.searches``, ``self.wars_priced`` and ``budget``
        count expansions only; the re-apply is deliberately not counted,
        because a different budget would be a different search.
        """
        self.searches += 1
        budget = self.MAX_NODES
        begin, rollback = journal.begin, journal.rollback
        # frontier entries: (ordering_score, state, first_move)
        frontier = [(0.0, root, None)]
        best = {}
        for _ply in range(self.MAX_PLIES):
            # nxt entries: (ordering_score, PARENT state, move, first_move)
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
                    j = begin(st)
                    try:
                        try:
                            actions.apply(st, mv, _rng())
                        except Exception:
                            continue
                        f = mv if first is None else first
                        # resolve decisions owned by anybody, so the position
                        # is quiet before it is either scored or expanded
                        self._quiesce(st, w, root_row=ctx.get("root_row"))
                        try:
                            v = self._score(st, me, w, ctx)
                        except Exception:
                            continue
                        stop = st.game_over or st.current != me
                    finally:
                        rollback(j)
                    if stop:
                        if f not in best or v > best[f]:
                            best[f] = v
                    else:
                        nxt.append((v, st, mv, f))
            if not nxt or budget <= 0:
                break
            # `sort` is stable and the key is the score alone, so ties keep
            # the order they were appended in -- identical to the copy path,
            # where the tuples held states that were never compared either.
            nxt.sort(key=lambda e: -e[0])
            frontier = [(v, self._replay(parent, mv, w,
                                         ctx.get("root_row")), f)
                        for v, parent, mv, f in nxt[:self.width]]
        return best

    def _replay(self, parent, mv, w, root_row=None):
        """Rebuild the child of `parent` by `mv` as a real, persistent state.

        Runs with no journal of its own: the caller has rolled back, so
        `parent` is exactly what it was when the child was scored.
        """
        t = copy_state(parent)
        actions.apply(t, mv, _rng())
        self._quiesce(t, w, root_row=root_row)
        return t

    def _score(self, t, me, w, ctx):
        """Evaluate a quiet position, pricing an unresolved war of mine.

        Every node in the beam goes through here, not just the node where the
        ``war`` move was played, because the beam searches whole-turn
        *sequences*: the war is typically declared at ply 1 and the position
        that gets scored (and the positions that get ranked for the beam) are
        2-5 plies later.  Pricing only at the declaring node would let the war
        line be ranked as pure cost and pruned before it ever reached a
        terminal, which is the same bug in a different place.

        There is no double counting across plies.  ``war_value`` resolves on a
        scratch copy and returns a *replacement* score for the position, so the
        spoils enter each score exactly once and never enter the state that the
        next ply expands.  A player may hold at most one declared war
        (``actions.py`` refuses a second while ``war_declared_by_me`` is set),
        and the beam's horizon is the end of my own turn, so the engine can
        never resolve the war inside the search either.

        Skipped when ``t.game_over``: a war declared into a finished game never
        resolves, and the position is already scored on final culture, so
        adding spoils there would be inventing points.  The narrower case -- a
        war declared in the last round of a game that has not ended yet in this
        line -- is NOT handled; see docs/PLAN_WAR_LOOKAHEAD.md.
        """
        if (self.WAR_LOOKAHEAD and not t.game_over
                and t.players[me].war_declared_by_me is not None):
            wv = war_value(t, me, w, ctx)
            if wv is not None:
                self.wars_priced += 1
                return wv
        return evaluate(t, me, w, ctx)

    def _quiesce(self, st, w, cap=12, root_row=None):
        """Drain the pending stack with plain 1-ply picks for whoever decides.

        `root_row` is the search root's visible card row, threaded down from
        `pick` so the opponent picks made in here price the same row the real
        decider could see -- not the row a trial `end_turn` has already
        replenished with the deck's next real cards.  It has to arrive as an
        argument: this runs deep inside the beam, where the root context is
        long out of scope.
        """
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
                dctx = rival_context(st, d, root_row)
            except Exception:
                dctx = dict(_NO_CTX)
            mv = self._one_ply(st, mvs, d, w, dctx)
            try:
                actions.apply(st, mv, _rng())
            except Exception:
                return
