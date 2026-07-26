"""Exact structural diff of two `GameState`s -- the oracle for the undo stack.

This is the safety net demanded by docs/PYPY.md section 6.5: the undo stack
(`engine/journal.py`) is only safe if a journalled `apply` followed by a
rollback leaves the state *structurally identical* to what it was before.  A
missed mutation site would silently corrupt the real game, so "did I find all
~470 sites?" has to become a test question rather than an audit question:

    before = copy_state(state)          # the oracle
    j = journal.begin(state)
    try:
        actions.apply(state, mv, rng)
    finally:
        journal.rollback(j)
    assert not statediff.diff(before, state)

Why not `==`?  Dataclass `__eq__` would get most of this, but it misses two
things that matter here and would produce exactly the bugs section 6.5 lists
as hazards 3 and 1:

  * **Key order.**  `{'a': 1, 'b': 2} == {'b': 2, 'a': 1}` is True, but the
    engine *iterates* `p.techs`, `state.seeded_by` and `p.one_time_discount`,
    so a rollback that restores the right values in the wrong insertion order
    changes real behaviour while comparing equal.  This differ compares key
    order explicitly.  (Same for lists, which `==` already handles.)
  * **Where.**  `==` returns False; debugging a missed mutation site needs the
    *path* to the difference.  This returns `state.players[2].techs['Bronze']
    .workers: 1 != 2`.

`log` and `_`-prefixed attributes are excluded by default, matching exactly
what `engine.bots.fastcopy.copy_state` excludes -- comparing them against a
`copy_state` oracle that never captured them would be meaningless.  Both are
handled separately by the journal (`emit` is suppressed during a trial; the
stats cache is cleared on rollback), and `include_private=True` /
`include_log=True` let a caller check them against a `deepcopy` oracle.
"""
from __future__ import annotations

_ATOMIC = (str, int, float, bool, bytes, type(None))

#: attributes `copy_state` drops, therefore ones a copy_state oracle cannot
#: be asked about.  Only skipped at the top level (`GameState`).
_SKIP_TOP = ("log",)

MAX_DIFFS = 40


class _Walker:
    __slots__ = ("out", "include_private", "include_log", "limit")

    def __init__(self, include_private, include_log, limit):
        self.out = []
        self.include_private = include_private
        self.include_log = include_log
        self.limit = limit

    def full(self):
        return len(self.out) >= self.limit

    def note(self, path, msg):
        if not self.full():
            self.out.append(f"{path}: {msg}")

    # -- container introspection -------------------------------------------
    def fields(self, v):
        """(kind, ordered [(key, value)]) or (None, None) for a leaf."""
        t = type(v)
        if t is list:
            return "list", list(enumerate(v))
        if t is tuple:
            return "tuple", list(enumerate(v))
        if t is dict:
            return "dict", list(v.items())
        if t is set or t is frozenset:
            return "set", None
        if hasattr(v, "__dataclass_fields__"):
            items = [(k, x) for k, x in v.__dict__.items()
                     if self.include_private or k[0] != "_"]
            return "obj", items
        return None, None

    # -- the walk -----------------------------------------------------------
    def walk(self, a, b, path, top=False):
        if self.full():
            return
        if a is b and isinstance(a, _ATOMIC):
            return
        ta, tb = type(a), type(b)
        if ta is not tb:
            self.note(path, f"type {ta.__name__} != {tb.__name__}")
            return

        kind, ia = self.fields(a)
        if kind is None:                       # scalar leaf
            if a != b:
                self.note(path, f"{a!r} != {b!r}")
            return

        if kind == "set":
            only_a, only_b = a - b, b - a
            if only_a:
                self.note(path, f"missing after rollback: {sorted(map(repr, only_a))}")
            if only_b:
                self.note(path, f"extra after rollback: {sorted(map(repr, only_b))}")
            return

        _, ib = self.fields(b)
        ka = [k for k, _ in ia]
        kb = [k for k, _ in ib]

        if top:
            drop = set() if self.include_log else set(_SKIP_TOP)
            if drop:
                ia = [(k, v) for k, v in ia if k not in drop]
                ib = [(k, v) for k, v in ib if k not in drop]
                ka = [k for k, _ in ia]
                kb = [k for k, _ in ib]

        if ka != kb:
            sa, sb = set(ka), set(kb)
            if sa != sb:
                if sa - sb:
                    self.note(path, f"keys lost: {sorted(map(repr, sa - sb))}")
                if sb - sa:
                    self.note(path, f"keys gained: {sorted(map(repr, sb - sa))}")
            else:
                # SAME keys, DIFFERENT order.  This is hazard 3 in section 6.5:
                # non-LIFO rollback reordering a dict the engine iterates.
                self.note(path, f"{kind} KEY ORDER changed: "
                                f"{_first_disorder(ka, kb)}")
            common = [k for k in ka if k in set(kb)]
            db = dict(ib)
            for k in common:
                self.walk(dict(ia)[k], db[k], _sub(path, kind, k))
            return

        for (k, va), (_, vb) in zip(ia, ib):
            self.walk(va, vb, _sub(path, kind, k))
            if self.full():
                return


def _first_disorder(ka, kb):
    for i, (x, y) in enumerate(zip(ka, kb)):
        if x != y:
            return f"at {i}: {x!r} -> {y!r}"
    return "?"


def _sub(path, kind, k):
    if kind == "obj":
        return f"{path}.{k}"
    return f"{path}[{k!r}]"


def diff(before, after, include_private=False, include_log=False,
         limit=MAX_DIFFS, path="state"):
    """Structural differences between `before` and `after`.

    Returns a list of human-readable `path: what` strings, empty when the two
    are structurally identical.  Truthiness is the assertion you want:

        assert not diff(oracle, rolled_back)
    """
    w = _Walker(include_private, include_log, limit)
    w.walk(before, after, path, top=True)
    return w.out


def assert_same(before, after, what="rollback", **kw):
    """Raise `AssertionError` with the paths if the two states differ."""
    d = diff(before, after, **kw)
    if d:
        extra = "" if len(d) < kw.get("limit", MAX_DIFFS) else f" (first {len(d)})"
        raise AssertionError(
            f"{what} did not restore the state{extra}:\n  " + "\n  ".join(d))
