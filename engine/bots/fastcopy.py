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

_ATOMIC = (str, int, float, bool, bytes, type(None))


_ATOMIC_SET = frozenset(_ATOMIC)


def _cv(v):
    t = type(v)
    if t is list:
        # inline the atomic test: most lists here are lists of card names
        return [x if type(x) in _ATOMIC_SET else _cv(x) for x in v]
    if t is dict:
        return {k: (x if type(x) in _ATOMIC_SET else _cv(x))
                for k, x in v.items()}
    if t in _ATOMIC:
        return v
    if t is tuple:
        return tuple(_cv(x) for x in v)
    if hasattr(v, "__dataclass_fields__"):
        return _cdc(v)
    if t is set:
        return set(v)
    return _copy.deepcopy(v)


def _cdc(obj):
    new = obj.__class__.__new__(obj.__class__)
    d = {}
    for k, v in obj.__dict__.items():
        if k[0] == "_":
            continue
        t = type(v)
        if t in _ATOMIC:
            d[k] = v
        else:
            d[k] = _cv(v)
    new.__dict__.update(d)
    return new


def copy_state(state, keep_log=False):
    """Copy a GameState for search purposes."""
    new = state.__class__.__new__(state.__class__)
    d = {}
    for k, v in state.__dict__.items():
        if k[0] == "_":
            continue
        if k == "log" and not keep_log:
            d[k] = []
            continue
        t = type(v)
        if t in _ATOMIC:
            d[k] = v
        else:
            d[k] = _cv(v)
    new.__dict__.update(d)
    return new
