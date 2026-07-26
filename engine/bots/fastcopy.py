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

## Generated per-class copiers

The generic copier spends most of its time *deciding* what to do: for each of
the ~209 dataclass fields and ~115 container elements in a mid-game 4p state
it calls ``type()`` and probes a frozenset, then walks an ``if`` chain.  All of
that is a pure function of the class, so it is decided ONCE, at import, and
baked into a straight-line ``exec``-generated function per class -- the same
trick ``dataclasses`` itself uses for ``__init__``.

Each field gets one of four plans, from its dataclass annotation plus the two
registries below:

  ``S``  scalar             -> ``d['x']``            (shared, immutable)
  ``A``  atomic container   -> ``list(d['x'])`` / ``dict(d['x'])``, one C call
  ``C``  container of a     -> a comprehension calling that class's generated
         known dataclass       copier, with one ``type() is`` guard per element
                              instead of the generic dispatch chain
  ``G``  anything else      -> ``_cv(d['x'])``       (generic, recursive)

Guards, because ``A``/``C`` are claims about *element* types that the
annotations cannot express (those fields are annotated bare ``list``/``dict``):

  * every generated copier checks ``len(obj.__dict__)`` against the class's
    field count and falls back to the fully generic ``_cdc`` if it differs, so
    a dynamically-added attribute can never be silently dropped;
  * ``C`` plans re-test each element's type and fall back to ``_cv`` per
    element, so a container that grows a new member type stays correct;
  * ``A`` plans are checked by **paranoid mode**: set ``FASTCOPY_PARANOID=1``
    in the environment and every atomic container is verified element by
    element to hold only immutable values, raising if not.  Run the 135-game
    fingerprint suite under it after touching ``_ATOMIC_CONTAINERS``.
"""
from __future__ import annotations

import copy as _copy
import os as _os

from ..state import (GameState as _GameState, PlayerState as _PlayerState,
                     TechCard as _TechCard, WonderInProgress as _WonderInProgress)

_ATOMIC = (str, int, float, bool, bytes, type(None))


_ATOMIC_SET = frozenset(_ATOMIC)

PARANOID = bool(_os.environ.get("FASTCOPY_PARANOID"))

# the one private attribute the engine hangs off a state (engine.effects); it
# is a pure cache and is never copied, but it does change len(__dict__)
_PRIVATE = "_stats_cache"


# ------------------------------------------------------------ the registries
#
# Fields whose CONTAINER is mutable but whose ELEMENTS are all immutable, so
# the whole thing can be copied with a single C-level `list()` / `dict()`
# instead of a Python-level per-element comprehension.  A mid-game 4p state
# holds ~115 such elements (decks + row 63, hands 27, event lists 16,
# seeded_by 9), and each of them used to cost a `type()` call plus a frozenset
# probe.  Verified against real play by paranoid mode -- see the docstring.
_ATOMIC_CONTAINERS = {
    "GameState": frozenset((
        "civil_deck", "military_deck", "card_row", "future_events",
        "current_events", "past_events", "scoring_events",
        "available_tactics", "seeded_by")),
    "PlayerState": frozenset((
        "completed_wonders", "colonies", "flipped_wonders", "hand_civil",
        "hand_military", "taken_leader_ages", "taken_this_turn",
        # elements are (war_name, attacker, defender) tuples of scalars:
        # immutable, therefore shareable
        "wars_declared_on_me")),
}

# Fields holding a container of one known dataclass, so the element copy can
# call that class's generated copier directly.
_DC_CONTAINERS = {
    ("GameState", "players"): _PlayerState,     # list, 4 elements
    ("PlayerState", "techs"): _TechCard,        # dict, ~26 elements per state
}

# Annotations denoting an immutable scalar.  `from __future__ import
# annotations` in state.py means they arrive as strings.
_SCALAR_ANNOTATIONS = frozenset((
    "int", "str", "bool", "float", "bytes",
    "str | None", "int | None", "bool | None", "float | None"))


# ------------------------------------------------------------ paranoid checks

def _shareable(x):
    """True if `x` may be shared between two states without aliasing risk."""
    t = type(x)
    if t in _ATOMIC_SET:
        return True
    if t is tuple or t is frozenset:
        return all(_shareable(i) for i in x)
    return False


def _checked_list(v):
    for x in v:
        if not _shareable(x):
            raise AssertionError(
                f"fastcopy paranoid: an _ATOMIC_CONTAINERS list holds a "
                f"{type(x).__name__} ({x!r}) -- it is not atomic, remove it "
                f"from the registry in engine/bots/fastcopy.py.")
    return list(v)


def _checked_dict(v):
    for x in v.values():
        if not _shareable(x):
            raise AssertionError(
                f"fastcopy paranoid: an _ATOMIC_CONTAINERS dict holds a "
                f"{type(x).__name__} ({x!r}) -- it is not atomic, remove it "
                f"from the registry in engine/bots/fastcopy.py.")
    return dict(v)


_LIST = _checked_list if PARANOID else list
_DICT = _checked_dict if PARANOID else dict


# ------------------------------------------------------------ generic copier

_COPIERS = {}       # class -> generated copy function


def _cv(v):
    t = type(v)
    if t in _ATOMIC_SET:
        return v
    f = _COPIERS.get(t)
    if f is not None:
        return f(v)
    if t is list:
        if not v:
            return []
        return [x if type(x) in _ATOMIC_SET else _cv(x) for x in v]
    if t is dict:
        if not v:
            return {}
        return {k: (x if type(x) in _ATOMIC_SET else _cv(x))
                for k, x in v.items()}
    if t is tuple:
        return tuple(_cv(x) for x in v)
    if hasattr(v, "__dataclass_fields__"):
        return _copier_for(t)(v)
    if t is set:
        return set(v)
    return _copy.deepcopy(v)


def _cdc(obj):
    """Fully generic dataclass copy: the fallback every generated copier drops
    back to when the instance does not match its class's schema."""
    cls = obj.__class__
    new = cls.__new__(cls)
    new.__dict__ = {k: (v if type(v) in _ATOMIC_SET else _cv(v))
                    for k, v in obj.__dict__.items() if k[0] != "_"}
    return new


# ------------------------------------------------------------ code generation

def _annotation(f):
    return f.type if isinstance(f.type, str) else getattr(
        f.type, "__name__", str(f.type))


def _field_exprs(cls, skip=()):
    """(source lines for the __dict__ display, extra globals) for `cls`."""
    cname = cls.__name__
    atomic = _ATOMIC_CONTAINERS.get(cname, frozenset())
    lines, glb = [], {}
    for fname, f in cls.__dataclass_fields__.items():
        if fname in skip:
            continue
        ann = _annotation(f)
        src = f"_cv(d[{fname!r}])"                       # G, the safe default
        dc = _DC_CONTAINERS.get((cname, fname))
        if dc is not None:                               # C
            tag, cpy = f"_t_{dc.__name__}", f"_c_{dc.__name__}"
            glb[tag], glb[cpy] = dc, _copier_for(dc)
            elem = f"({cpy}(x) if type(x) is {tag} else _cv(x))"
            if ann == "dict":
                src = f"{{k: {elem} for k, x in d[{fname!r}].items()}}"
            else:
                src = f"[{elem} for x in d[{fname!r}]]"
        elif fname in atomic and ann in ("list", "dict"):  # A
            src = (f"_DICT(d[{fname!r}])" if ann == "dict"
                   else f"_LIST(d[{fname!r}])")
        elif ann in _SCALAR_ANNOTATIONS:                 # S
            src = f"d[{fname!r}]"
        lines.append(f"        {fname!r}: {src},")
    return lines, glb


def _build(cls, name, skip=(), extra=()):
    lines, glb = _field_exprs(cls, skip)
    lines.extend(f"        {k!r}: {v}," for k, v in extra)
    n = len(cls.__dataclass_fields__)
    body = [
        f"def {name}(o):",
        "    d = o.__dict__",
        f"    if len(d) != {n}:",
        # the one tolerated deviation: engine.effects hangs a private stats
        # cache off the state.  Anything else -> fully generic path.
        f"        if len(d) != {n + 1} or _PRIVATE not in d:",
        "            return _slow(o)",
        "    n = _new(_cls)",
        "    n.__dict__ = {",
        *lines,
        "    }",
        "    return n",
    ]
    g = {"_cv": _cv, "_LIST": _LIST, "_DICT": _DICT, "_slow": _cdc,
         "_new": object.__new__, "_cls": cls, "_PRIVATE": _PRIVATE}
    g.update(glb)
    src = "\n".join(body)
    exec(compile(src, f"<fastcopy:{cls.__name__}>", "exec"), g)
    fn = g[name]
    fn._source = src
    return fn


def _copier_for(cls):
    fn = _COPIERS.get(cls)
    if fn is None:
        # register the generic copier first so a self-referential class cannot
        # recurse forever while its own copier is being generated
        _COPIERS[cls] = _cdc
        fn = _COPIERS[cls] = _build(cls, f"_copy_{cls.__name__}")
    return fn


for _c in (_TechCard, _WonderInProgress, _PlayerState):
    _copier_for(_c)

# GameState needs two variants because `log` is dropped by default: it holds up
# to 400 strings and search never reads it.
_copy_gs_nolog = _build(_GameState, "_copy_gs_nolog", skip=("log",),
                        extra=(("log", "[]"),))
_copy_gs_log = _build(_GameState, "_copy_gs_log")
_COPIERS[_GameState] = _copy_gs_nolog


def copy_state(state, keep_log=False):
    """Copy a GameState for search purposes."""
    if type(state) is _GameState:
        return _copy_gs_log(state) if keep_log else _copy_gs_nolog(state)
    # a subclass or a stand-in: stay fully generic
    new = _cdc(state)
    if not keep_log:
        new.log = []
    return new
