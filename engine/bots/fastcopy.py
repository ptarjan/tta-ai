"""Fast structural copy of a GameState for 1-ply search.

``copy.deepcopy`` accounts for ~78% of a lookahead bot's runtime: it memoises
every object, keeps an alive-list and reconstructs dataclasses through
``__reduce_ex__``.  The engine's state is a shallow tree of dataclasses,
lists, dicts and scalars, so a hand-rolled copier is far cheaper.

Rules:
  * dataclasses are rebuilt with ``cls.__new__`` + a copied ``__dict__``
  * lists/dicts are rebuilt element-wise
  * str/int/float/bool/None/tuple are shared (immutable in this engine)
  * private attributes (``_stats_cache``) are dropped -- they are pure caches
  * ``GameState.log`` is dropped: search never reads it and it can hold 400
    strings, which is the single biggest copy cost

Anything unexpected falls back to ``copy.deepcopy`` so the copier stays
correct if the engine grows new field types.
"""
from __future__ import annotations

import copy as _copy

from ..state import TechCard as _TechCard, WonderInProgress as _WonderInProgress

_ATOMIC = (str, int, float, bool, bytes, type(None))


_ATOMIC_SET = frozenset(_ATOMIC)


# ------------------------------------------------------------ leaf classes
#
# `TechCard` and `WonderInProgress` are the two dataclasses whose every field
# is an immutable scalar, and they are also by far the most numerous objects in
# a state: a mid-game 4p position holds ~31 TechCards, i.e. ~31 of the ~35
# dataclass copies per `copy_state`.  Copying one is therefore worth a fast
# path -- `cls.__new__` plus a single C-level `dict(...)` of its `__dict__`,
# with no Python-level loop, no per-field type test and no intermediate dict.
#
# The guard below fails loudly at import time if either class ever grows a
# field that is not a scalar, because sharing a mutable field between the real
# state and a search copy would be a silent correctness bug.
_SCALAR_ANNOTATIONS = frozenset(
    ("int", "str", "bool", "float", "str | None", "int | None",
     "bool | None", "float | None"))


def _check_leaf(cls):
    for name, f in cls.__dataclass_fields__.items():
        ann = f.type if isinstance(f.type, str) else getattr(
            f.type, "__name__", str(f.type))
        if ann not in _SCALAR_ANNOTATIONS:
            raise AssertionError(
                f"fastcopy: {cls.__name__}.{name} is annotated {ann!r}, which "
                "is not a scalar -- it can no longer use the leaf fast path. "
                "Remove it from _LEAF in engine/bots/fastcopy.py.")
    return cls


_LEAF = frozenset(_check_leaf(c) for c in (_TechCard, _WonderInProgress))


def _cv(v):
    t = type(v)
    if t is list:
        # inline the atomic test: most lists here are lists of card names
        if not v:
            return []
        return [x if type(x) in _ATOMIC_SET else _cv(x) for x in v]
    if t is dict:
        if not v:
            return {}
        return {k: (x if type(x) in _ATOMIC_SET else _cv(x))
                for k, x in v.items()}
    if t in _LEAF:
        new = t.__new__(t)
        new.__dict__ = dict(v.__dict__)
        return new
    if t in _ATOMIC_SET:
        return v
    if t is tuple:
        return tuple(_cv(x) for x in v)
    if hasattr(v, "__dataclass_fields__"):
        return _cdc(v)
    if t is set:
        return set(v)
    return _copy.deepcopy(v)


def _cdc(obj):
    cls = obj.__class__
    new = cls.__new__(cls)
    # A dict comprehension assigned straight onto `__dict__` beats building a
    # dict and calling `.update()` on the one `__new__` already made: one dict
    # allocation instead of two and no re-hashing of ~40 PlayerState keys.
    new.__dict__ = {k: (v if type(v) in _ATOMIC_SET else _cv(v))
                    for k, v in obj.__dict__.items() if k[0] != "_"}
    return new


def copy_state(state, keep_log=False):
    """Copy a GameState for search purposes."""
    cls = state.__class__
    new = cls.__new__(cls)
    d = {}
    for k, v in state.__dict__.items():
        if k[0] == "_":
            continue
        if type(v) in _ATOMIC_SET:
            d[k] = v
        elif k == "log" and not keep_log:
            # never copied: search never reads it and it holds up to 400 strings
            d[k] = []
        else:
            d[k] = _cv(v)
    new.__dict__ = d
    return new
