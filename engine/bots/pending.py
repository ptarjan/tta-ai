"""The ONE implementation of the "not my ordinary turn" short-circuit.

Both search bots -- :class:`engine.bots.plan.PlanBot` and
:class:`engine.bots.neural_plan.NeuralPlanBot` -- have the same three-way
decision to make at the top of ``pick``:

1. an ordinary turn of mine -> plan the whole turn with the beam;
2. somebody else's pending decision, or mine nested inside another player's
   turn -> there is no turn to plan, so price the candidates one ply deep at a
   common horizon of "now";
3. a pending decision that is **mine** -> price them one ply deep *after
   draining the stack*, because that is how each bot's own ``_beam`` already
   prices every node it searches (``apply -> _quiesce -> score``).

Case 3 is the bug this module exists to make unrepeatable.  Skipping the drain
at a real decision while performing it at every searched node means the bot
prices its own position differently from the way it prices the identical
position inside its own search.  Measured consequence, `docs/AGGRESSION_RATE.md`:
1,549 defences faced, 1,104 winnable by arithmetic, **0 ever held off**, with
cards burned in 335 arithmetically hopeless defences.  Nothing in
``weighted.features`` reads ``pend["atk"]``/``pend["dfn"]``, so an undrained
position cannot express whether a defence succeeds; and 588 of 589 winnable
defences need 2+ cards, so the first ``defend`` always leaves the outcome
invisible.  The drain is a rule-fact -- rule 5.4 step 5 is a *threshold*, so a
card spent below the attacker's strength buys nothing.

WHY IT LIVES HERE RATHER THAN IN EACH BOT.  It was written twice: `plan.py`
had it and `neural_plan.py` had it copied out, so the fix for one was not the
fix for the other.  That is the shape that has cost this repo repeatedly (the
build discount, the hand double-count, the population cost, the
``rankingCulture`` block).  The evaluator-specific part -- a linear dot product
scored serially versus a net scored in one batch per ply -- stays in each bot.
The *policy* is here, once, and `tests/test_pending_fallback_is_shared.py`
fails if either bot stops routing through it or resolves the default
differently.

THE DEFAULT IS THE JUDGEMENT CALL, and it lives here exactly once so the two
bots cannot disagree about it.  A bot may override per-instance
(``PlanBot(quiet_pending=True)``, ``plan:FILE,width=2,qp=1``); a class-level
``QUIET_PENDING`` of ``None`` means "whatever this module says".
"""
from __future__ import annotations

__all__ = ["QUIET_PENDING", "not_my_turn", "wants_quiet", "wants_determinize",
           "prepare_root", "fallback_pick", "counters", "reset_counters"]

#: Drain a pending decision of MINE before pricing the candidates.
#:
#: Measured at 3p and 4p against the live league references before being
#: flipped -- see `docs/AGGRESSION_RATE.md` 7 for the A/B and the behavioural
#: counters.  Changing this moves `tools/gate.sh`'s PNARROW/PWIDE digests,
#: which must be re-derived on two independent clean worktrees that agree.
#:
#: FLIPPED 2026-07-30.  It lands as a CONSISTENCY FIX, not on the strength
#: measurement: the beam already drains before scoring and the live decision
#: did not.  The A/B (docs/DEEPER_SEARCH.md §8) corroborates it and is uneven, so
#: quote it from there rather than from here -- there are two pools and they
#: say different things.  Six distinct blocks of 200 games against the frozen
#: league references, every one favouring the drain; 3p is decisive (pure-qp
#: pool 0.5217 own-win share against a 0.3333 null over 600 games, z = 9.26)
#: and 4p is NOT independently established (one pure-qp block, 0.3000 against
#: a 0.2500 null, z = 1.54, p ~ 0.12).
#:
#: The leak confound the note on `DETERMINIZE` below raises was the reason to
#: doubt this, and it was answered rather than argued away: the same seed run
#: as `qp=1` vs `qp=0` and as `qp=1,qd=1` vs plain returned the SAME numbers
#: to every printed digit (0.5325 win, +26.01 margin).  Determinizing the
#: pending root removes the peek; removing it changes nothing; so the win is
#: not the peek.  That is a different instrument from the 1,346-pick census
#: in `tools/pending_leak.py` and it agrees with it.
QUIET_PENDING = True

#: THE SECOND HALF OF THE SAME INCONSISTENCY.  `pick`'s beam path prices
#: candidates on a *determinized* root -- `fastcopy.copy_state` copies the
#: hidden piles verbatim, so without that step a trial `apply` that draws would
#: draw the REAL next card.  On the pending path `NeuralPlanBot` already
#: determinized and `PlanBot` did not: the copy had drifted from the original
#: before anyone noticed it was a copy.
#:
#: It matters here because the drain adds `apply` calls, and
#: `tools/pending_leak.py` measures the drain consuming real deck cards in
#: 34.7% of candidate evaluations at 3p (master's own apply: 24.0%).  So
#: turning the drain on without this turned some of the peek up as well, and a
#: win rate measured that way could not be attributed to play.
#:
#: FLIPPED 2026-07-30, and it lands as a CORRECTNESS FIX with a measured-zero
#: behavioural effect, which is the honest way to describe it.
#: `tools/pending_divergence.py --lever det` asked the only question that
#: matters at 1,346 of the bot's own pending decisions at 3p and the pick
#: changed **0 times**.  `docs/AGGRESSION_RATE.md` 9 explains why that is not a
#: fluke: the drain resolves the SAME pending stack for every candidate, so the
#: peeked cards enter every candidate's score as a common additive term and an
#: argmax is invariant to a common offset.  The leak is common-mode TODAY.  It
#: is one refactor away from being differential, and nothing failed when it was
#: open, which is the whole argument for closing it.
#:
#: Both bots now resolve this through `None` class attributes, so there is one
#: answer rather than two.  `tests/test_pending_fallback_is_shared.py` fails if
#: either grows a bool of its own.
DETERMINIZE = True

# Instrumentation the divergence test reads.  A call counter is a structural
# guarantee: if either bot re-inlines the short-circuit, these stop moving and
# the test fails.  Counting is not free but it is one integer add on a path
# that already does a `copy_state` per candidate move.
_CALLS = 0
_QUIET_CALLS = 0
_DET_CALLS = 0


def not_my_turn(state, me) -> bool:
    """True when there is no whole turn of ``me``'s to plan.

    Either somebody has a decision pending (which may be mine, nested inside
    another player's turn) or it is simply not my turn.
    """
    return bool(state.pending) or state.current != me


def wants_quiet(bot, state) -> bool:
    """Should ``bot`` drain the pending stack before pricing candidates?

    Only ever true when there is something TO drain.  Resolution order:
    the instance/class ``QUIET_PENDING`` when it is not ``None``, otherwise
    this module's :data:`QUIET_PENDING`.  Read through ``getattr`` with the
    module default so a bot that never mentions the flag still gets the one
    shared answer rather than a second, silently different one.
    """
    if not state.pending:
        return False
    own = getattr(bot, "QUIET_PENDING", None)
    return QUIET_PENDING if own is None else bool(own)


def wants_determinize(bot, state) -> bool:
    """Should ``bot`` re-shuffle the unseen piles before pricing candidates?

    Two gates, in this order:

    1. ``bot.determinize`` -- the bot-wide A/B switch (``plan:FILE,det=0``,
       ``nplan:FILE,det=0``).  It already gates the beam's determinization, and
       a run that sets it to measure the leak must get a bot that leaks
       *everywhere*, not one that leaks in the beam and not at pendings.
       ``NeuralPlanBot`` used to spell this gate itself, at the call site, and
       ``PlanBot`` did not spell it at all -- which is a third place for the two
       to drift.  It is here now, once.
    2. the instance/class ``PENDING_DETERMINIZE`` unless it is ``None``,
       otherwise this module's :data:`DETERMINIZE`.  Same resolution order as
       :func:`wants_quiet`.
    """
    if not getattr(bot, "determinize", True):
        return False
    own = getattr(bot, "PENDING_DETERMINIZE", None)
    return DETERMINIZE if own is None else bool(own)


def prepare_root(bot, state, copy_fn, determinize_fn, rng):
    """The state the fallback should price candidates from.

    Returns ``state`` itself when determinization is off, so the caller's
    behaviour is byte-for-byte unchanged; otherwise a determinized copy.  One
    copy per decision, not per candidate: the candidate loop copies again.
    """
    global _DET_CALLS
    _DET_CALLS += 1
    if not wants_determinize(bot, state):
        return state
    root = copy_fn(state)
    determinize_fn(root, rng)
    return root


def fallback_pick(bot, state, plain, quiet):
    """Price a non-ordinary-turn decision, draining first iff configured.

    ``plain`` and ``quiet`` are zero-argument callables the caller has already
    bound to its own one-ply scorer, so this function holds the policy and
    none of the scoring.  Call it only when :func:`not_my_turn`.
    """
    global _CALLS, _QUIET_CALLS
    _CALLS += 1
    if wants_quiet(bot, state):
        _QUIET_CALLS += 1
        return quiet()
    return plain()


def counters() -> dict:
    """Shared-path call counts since the last :func:`reset_counters`."""
    return {"calls": _CALLS, "quiet": _QUIET_CALLS, "roots": _DET_CALLS}


def reset_counters() -> None:
    global _CALLS, _QUIET_CALLS, _DET_CALLS
    _CALLS = 0
    _QUIET_CALLS = 0
    _DET_CALLS = 0
