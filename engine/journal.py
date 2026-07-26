"""Undo stack for trial moves -- docs/PYPY.md section 6, design A.

`GreedyBot`'s use of the state is `copy -> apply(mv) -> evaluate -> discard`.
It never holds two trial states at once, so it does not need a copy; it needs
`apply` to be *reversible*:

    j = journal.begin(state)
    try:
        actions.apply(state, mv, rng)
        val = evaluate(state, idx)
    finally:
        journal.rollback(j)          # state is structurally identical again

Section 4b measured 6.43 mutated scalar slots per candidate against 395.4
copied, so the journal is O(mutation) where the copy is O(state).

Two mechanisms, chosen for *different* reasons.

**Attributes: a journalling `__setattr__`.**  300 of the engine's 470 mutation
sites are attribute writes.  Hand-converting them would be 300 chances to miss
one, and a miss silently corrupts the real game.  A `__setattr__` hook cannot
be forgotten, so that entire class of bug stops existing.  It is affordable
because a trial `apply` performs only ~3.8 attribute writes: 6.4x on a 0.5 us
operation is ~2 us of a ~107 us candidate.

**Containers: snapshot-on-first-touch.**  `touch(obj)` shallow-copies a list /
dict / set the first time a journal sees it, and rollback restores the whole
thing.  The obvious alternative -- one undo record per `append`/`pop`/`insert`
-- is faster and *much* easier to get subtly wrong: it only restores dict key
order if replay is strictly LIFO (hazard 3 of section 6.5), and key order is
load-bearing because the engine iterates `p.techs`, `seeded_by` and
`one_time_discount`.  `d.clear(); d.update(snapshot)` restores insertion order
by construction, with no ordering argument to get wrong.  At ~5.4 mutated
container nodes per candidate the snapshots are small; if they ever show up in
a profile, that is the moment to trade this safety for records, not before.

Journalling is **opt-in and off by default**: `_J is None` is the entire test,
so `play_game`, `experiments/` and `analysis/` are unaffected.
"""
from __future__ import annotations

import os

from . import state as _state
from . import statediff

#: The active journal (a plain list of undo records), or None when off.
_J = None

_MISSING = object()

# record kinds
_ATTR = 0        # (_ATTR, obj_dict, key, old_or_MISSING)
_LIST = 1        # (_LIST, lst, snapshot)
_DICT = 2        # (_DICT, dct, snapshot)
_SET = 3         # (_SET, st, snapshot)

#: classes whose attribute writes are journalled
JOURNALLED_CLASSES = (_state.GameState, _state.PlayerState,
                      _state.TechCard, _state.WonderInProgress)

#: set JOURNAL_PARANOID=1 to check every rollback against a copy_state oracle
PARANOID = os.environ.get("JOURNAL_PARANOID") == "1"


class JournalError(RuntimeError):
    """A journalled operation that cannot be undone correctly."""


# --------------------------------------------------------------------------
# attribute writes -- installed on the state dataclasses, active only when a
# journal is open.
# --------------------------------------------------------------------------
def _journalling_setattr(self, name, value):
    j = _J
    if j is not None:
        d = self.__dict__
        if name == "__dict__":
            # Wholesale __dict__ replacement is what the generated copiers in
            # bots/fastcopy do.  Copying a state *while* a trial journal is
            # open would mean the copy is journalled and the trial is aliased
            # into it; refuse rather than corrupt.
            raise JournalError(
                "__dict__ assigned while a journal is open -- copy_state must "
                "not run inside a journalled apply")
        j.append((_ATTR, d, name, d[name] if name in d else _MISSING))
    object.__setattr__(self, name, value)


_installed = False


def install():
    """Put the journalling `__setattr__` on the state dataclasses.

    Idempotent.  Safe to leave installed forever: with no journal open the
    hook is one global load and one `is not None` test.
    """
    global _installed
    if _installed:
        return
    for cls in JOURNALLED_CLASSES:
        cls.__setattr__ = _journalling_setattr
    _installed = True


def uninstall():
    """Restore the default `__setattr__` (for benchmarking the hook's cost)."""
    global _installed
    if not _installed:
        return
    if _J is not None:
        raise JournalError("cannot uninstall while a journal is open")
    for cls in JOURNALLED_CLASSES:
        del cls.__setattr__
    _installed = False


# --------------------------------------------------------------------------
# container writes -- call `touch(c)` immediately BEFORE mutating `c`.
# --------------------------------------------------------------------------
def touch(c):
    """Record `c`'s contents so rollback can restore them.  Returns `c`.

    Call it before the mutation, and it is safe (a cheap `id` lookup) to call
    it again for the same container:

        journal.touch(p.hand_civil).append(name)
        del journal.touch(state.seeded_by)[ev]
    """
    j = _J
    if j is None:
        return c
    seen = j.seen
    i = id(c)
    if i in seen:
        return c
    seen.add(i)
    t = type(c)
    if t is list:
        j.append((_LIST, c, c[:]))
    elif t is dict:
        j.append((_DICT, c, dict(c)))
    elif t is set:
        j.append((_SET, c, set(c)))
    else:
        raise JournalError(f"cannot journal a {t.__name__}")
    return c


# --------------------------------------------------------------------------
# begin / rollback
# --------------------------------------------------------------------------
class _Journal(list):
    """A list of undo records, plus the id-set that keeps `touch` idempotent."""
    __slots__ = ("seen", "oracle", "state")

    def __init__(self):
        super().__init__()
        self.seen = set()
        self.oracle = None
        self.state = None


def begin(state=None):
    """Open a journal.  Nesting is not supported (GreedyBot never nests)."""
    global _J
    if _J is not None:
        raise JournalError("a journal is already open (nesting unsupported)")
    if not _installed:
        install()
    j = _Journal()
    j.state = state
    if PARANOID and state is not None:
        from .bots.fastcopy import copy_state
        # keep_log=True: `emit` is suppressed below, so the log must come back
        # unchanged and the oracle has to be able to prove it.
        j.oracle = copy_state(state, keep_log=True)
    _state.SUPPRESS_LOG = True     # see GameState.emit
    _J = j
    return j


def rollback(j):
    """Undo every record in `j`, newest first, and close the journal.

    Correct from a *partial* journal: an exception part-way through `apply`
    leaves the records written so far, and replaying those in reverse is
    exactly the right thing (hazard 5 of section 6.5).  Always call from a
    `finally`.
    """
    global _J
    if _J is not j:
        raise JournalError("rollback of a journal that is not the open one")
    _J = None                                  # restores are NOT journalled
    _state.SUPPRESS_LOG = False
    for rec in reversed(j):
        kind = rec[0]
        if kind == _ATTR:
            _, d, k, old = rec
            if old is _MISSING:
                d.pop(k, None)
            else:
                d[k] = old
        elif kind == _LIST:
            rec[1][:] = rec[2]
        elif kind == _DICT:
            d = rec[1]
            d.clear()
            d.update(rec[2])                   # restores insertion order
        else:
            s = rec[1]
            s.clear()
            s.update(rec[2])
    del j[:]
    j.seen.clear()
    st = j.state
    if st is not None:
        # The stats cache is `_`-prefixed and so is not copied today: every
        # trial gets a clean one.  Under undo the REAL state's cache would be
        # polluted by trial computes, so drop it.  `invalidate` is 1.4% of
        # runtime; restoring it exactly is not worth the risk (6.5 hazard 4).
        st.__dict__.pop("_stats_cache", None)
    if j.oracle is not None:
        # include_log=True is affordable here (paranoid mode only) and is the
        # only mechanical proof that `emit` suppression really holds -- a
        # single un-suppressed `emit` inside a trial `apply` shows up as an
        # extra log line rather than as a fingerprint mismatch 30 minutes later.
        statediff.assert_same(j.oracle, st, what="journal rollback",
                              include_log=True)
        j.oracle = None


class scope:
    """`with journal.scope(state):` -- begin, and roll back however we leave."""
    __slots__ = ("state", "j")

    def __init__(self, state):
        self.state = state
        self.j = None

    def __enter__(self):
        self.j = begin(self.state)
        return self.j

    def __exit__(self, *exc):
        rollback(self.j)
        return False


def active():
    return _J is not None
