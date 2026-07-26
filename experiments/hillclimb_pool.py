"""The opponent POOL: a diverse field a candidate has to prove itself against.

Why this module exists
----------------------
`experiments/hillclimb.py` scores a mutant against a *mirror* of its own
parent (plus a thin ladder of that parent's own ancestors).  That measures
"is this a better response to my lineage", which is not the same thing as
"is this stronger".  docs/STRENGTH_CHECK.md is the receipt: a hand-written
rule list, `engine/bots/book.py`, beats the resulting 2p champion 62.9% +/-
4.7%, while every internal metric reported steady progress.

A pool entry is ``(bot spec, weight, label)``.  The pool mixes, in ONE run:

===============  =========================================================
tier             members
===============  =========================================================
``book``         BookBot / BookBot v2 -- the external, expert-derived
                 yardstick.  Not produced by our training loop, so beating
                 it means something in absolute terms.
``variant``      the strategy archetypes in ``engine/bots/variants/``
                 (tempo, infrastructure, military, culture, science,
                 wonder-heavy...).  Discovered DYNAMICALLY: the pool grows
                 by itself as another agent lands them, and degrades
                 gracefully (empty tier, one log line) if the package is
                 missing.
``quiescent``    the search bot of docs/DEEPER_SEARCH.md, off by default
                 because it costs an order of magnitude more per game.
``mirror``       self-play: the candidate against a table of its own parent
                 (see ``PoolEntry.resolve`` for why not against itself).
                 Still a real signal -- it is just no longer the ONLY one.
``past``         champions archived from previous generations, and from the
                 legacy ``experiments/league_*p/`` ladders.  A historical
                 ladder is what guards against *cycling*, where a new
                 champion beats the current one but loses to an older one.
``floor``        GreedyBot / RandomBot / the untrained default weights:
                 cheap floor checks.  Cheap, and weighted like it.
===============  =========================================================

Weighting
---------
Each TIER carries a total weight (``DEFAULT_TIER_WEIGHTS``, overridable from
the CLI).  A tier's total is split evenly across its members, so an entry's
weight is ``tier_weight / len(tier)``.  That is deliberate: adding a seventh
strategy variant must not let the ``variant`` tier outvote BookBot; it splits
the variant tier's say seven ways instead.  The aggregate score is the
weight-weighted mean over per-game paired samples, and every per-opponent
number is reported alongside it so the aggregate can never quietly hide a
candidate that farms five weak bots and loses to the book.

Bot construction
----------------
``engine/bots/book.py``, ``quiescent.py`` and ``variants/`` are owned by other
agents, and ``experiments/arena.py`` is shared.  So, exactly as
``experiments/bookmatch.py`` already does, this module installs its own
``make_bot`` OVER ``arena.make_bot`` rather than editing anything: the
multiprocessing workers are forked, so they inherit the patch.
"""
from __future__ import annotations

import importlib
import json
import math
import os
import pkgutil
import random
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from experiments import arena  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)

MIRROR = "@mirror"          # placeholder spec: resolved to the candidate itself

# Tier totals.  Strong, external, non-self-play opponents carry the most.
# Everything here is a dial: --pool-weights book=4,variant=2.5,past=0.5 ...
DEFAULT_TIER_WEIGHTS = {
    "book": 3.0,            # the external yardstick we currently LOSE to
    "variant": 2.5,         # human strategy archetypes
    "quiescent": 2.0,       # deeper search (opt-in: expensive)
    "mirror": 1.0,          # self-play: kept, demoted below every real bot
    "past": 1.0,            # anti-cycling ladder
    "floor": 0.5,           # greedy / random / untrained default
}

# Tiers whose opponents can VETO an acceptance on their own: losing to these
# is not something a good aggregate is allowed to excuse.
DEFAULT_GATE_TIERS = ("book", "variant", "quiescent")

# Tier order used for display.
TIER_ORDER = ("book", "variant", "quiescent", "mirror", "past", "floor")


# --------------------------------------------------------------- bot specs
#
# Every spec below is a plain str/tuple/dict, i.e. picklable, because it has
# to survive the trip into an arena worker process.

_BASE_MAKE_BOT = arena.make_bot


def make_bot(spec, seed):
    """``arena.make_bot`` plus the pool's own bot kinds.

    Installed over ``arena.make_bot`` (see the module docstring): forked
    workers inherit it, so no shared file needs editing.
    """
    if spec == "book":
        from engine.bots.book import BookBot
        return BookBot(seed=seed)
    if spec == "book2":
        from engine.bots.book import BookBot
        return BookBot(seed=seed, version=2)
    if isinstance(spec, tuple) and spec:
        if spec[0] == "book-improved":
            from engine.bots.book import BookImprovedBot
            return BookImprovedBot(weights=spec[1], seed=seed)
        if spec[0] == "variant":
            return _make_variant(spec[1], spec[2], seed)
    return _BASE_MAKE_BOT(spec, seed)


arena.make_bot = make_bot


def _make_variant(module, cls_name, seed):
    """Build a bot from ``engine.bots.variants.<module>.<cls_name>``.

    The variants package is written by another agent, so the constructor
    signature is not ours to assume.  Try the conventions in order and take
    the first that works.
    """
    mod = importlib.import_module(f"engine.bots.variants.{module}")
    obj = getattr(mod, cls_name)
    for kwargs in ({"seed": seed}, {"rng": random.Random(seed)}, {}):
        try:
            return obj(**kwargs)
        except TypeError:
            continue
    return obj()


# ------------------------------------------------------------------ entries

class PoolEntry:
    """One opponent: ``(spec, weight, label)`` plus its tier."""

    __slots__ = ("label", "spec", "tier", "weight")

    def __init__(self, label, spec, tier, weight=0.0):
        self.label = label
        self.spec = spec
        self.tier = tier
        self.weight = weight

    @property
    def is_mirror(self):
        return self.spec is MIRROR or self.spec == MIRROR

    def resolve(self, candidate, champion):
        """The spec actually handed to the arena for this candidate.

        A mirror entry resolves to the CHAMPION, not to the candidate itself.
        Candidate-against-a-table-of-itself is worthless by construction: the
        seats hold the identical deterministic policy, so over a complete
        seat rotation the shares sum to 1 and the mean is exactly 1/players
        for every possible policy.  It measures the deal, not the bot.
        Candidate-against-parent is the real self-play signal, and its
        reference ("what would the champion score here?") is likewise exactly
        1/players, so it needs no reference games at all.
        """
        return champion if self.is_mirror else self.spec

    def __repr__(self):
        return f"<PoolEntry {self.label} tier={self.tier} w={self.weight:.3f}>"


class Pool:
    """A weighted, tiered collection of opponents."""

    def __init__(self, entries, tier_weights=None, gate_tiers=DEFAULT_GATE_TIERS):
        self.entries = list(entries)
        self.tier_weights = dict(tier_weights or DEFAULT_TIER_WEIGHTS)
        self.gate_tiers = tuple(gate_tiers)
        self.renormalise()

    def renormalise(self):
        """entry.weight = tier total / members of that tier (see docstring)."""
        counts = {}
        for e in self.entries:
            counts[e.tier] = counts.get(e.tier, 0) + 1
        for e in self.entries:
            total = self.tier_weights.get(e.tier, 0.0)
            e.weight = total / counts[e.tier] if counts[e.tier] else 0.0

    def __len__(self):
        return len(self.entries)

    def __iter__(self):
        return iter(self.entries)

    def by_label(self, label):
        for e in self.entries:
            if e.label == label:
                return e
        return None

    def tiers(self):
        out = {}
        for e in self.entries:
            out.setdefault(e.tier, []).append(e)
        return out

    def gates(self):
        return [e for e in self.entries if e.tier in self.gate_tiers]

    def sorted_entries(self):
        return sorted(self.entries,
                      key=lambda e: (TIER_ORDER.index(e.tier)
                                     if e.tier in TIER_ORDER else 99, e.label))

    # ------------------------------------------------------ anti-overfit
    def acceptance_subset(self, gen, size, rng=None):
        """The opponents THIS generation's accept/reject decision uses.

        Rotating the subset is the anti-overfit rule: a candidate cannot be
        tuned to the whole pool at once because it is never scored against
        the whole pool at once, and any opponent left out this generation is
        re-checked later (and by the periodic full-pool check).

        Two invariants:
          * mirror is always in, when present (it is nearly free -- the
            reference share is 1/players by construction, no champion games);
          * at least one GATE opponent is always in, rotating through them,
            so "beat five weak bots" can never be a winning strategy.
        """
        size = max(1, size)
        chosen, seen = [], set()

        def take(e):
            if e is not None and e.label not in seen:
                seen.add(e.label)
                chosen.append(e)

        mirrors = [e for e in self.entries if e.tier == "mirror"]
        for e in mirrors:
            take(e)
        gates = sorted(self.gates(), key=lambda e: e.label)
        if gates:
            take(gates[gen % len(gates)])
        # deterministic rotation over the rest: generation g starts reading
        # the (stable, sorted) remainder at offset g, so coverage is uniform.
        rest = [e for e in self.sorted_entries() if e.label not in seen]
        if rest:
            start = (gen * max(1, size - len(chosen))) % len(rest)
            i = 0
            while len(chosen) < size and i < len(rest):
                take(rest[(start + i) % len(rest)])
                i += 1
        return chosen


# ------------------------------------------------------------- discovery

def discover_variants(log=None):
    """Every bot in ``engine/bots/variants/``, found dynamically.

    Supports both shapes the variants agent might ship:

      * ``*.py`` modules exposing bot CLASSES.  An explicit ``BOTS`` (dict
        label -> class, or a list of classes) or ``BOT`` attribute wins;
        otherwise every public class *defined in that module* whose name
        ends in ``Bot`` is taken.
      * ``*.json`` weight files -- a strategy archetype expressed as a
        WeightedBot weight vector.

    Never raises: a missing package, a module that does not import, a class
    that will not construct -- each is logged and skipped, so training runs
    fine before the variants land and picks them up automatically after.
    """
    log = log or (lambda *_a: None)
    out = []
    try:
        pkg = importlib.import_module("engine.bots.variants")
    except Exception as exc:                       # not landed yet: fine
        log(f"[pool] no engine/bots/variants ({exc.__class__.__name__}: {exc})"
            " -- variant tier empty")
        return out
    paths = list(getattr(pkg, "__path__", []))
    for _finder, modname, _ispkg in pkgutil.iter_modules(paths):
        if modname.startswith("_"):
            continue
        full = f"engine.bots.variants.{modname}"
        try:
            mod = importlib.import_module(full)
        except Exception as exc:
            log(f"[pool] skip {full}: {exc.__class__.__name__}: {exc}")
            continue
        classes = []
        explicit = getattr(mod, "BOTS", None) or getattr(mod, "BOT", None)
        if isinstance(explicit, dict):
            classes = list(explicit.items())
        elif isinstance(explicit, (list, tuple)):
            classes = [(getattr(c, "name", c.__name__), c) for c in explicit]
        elif explicit is not None:
            classes = [(getattr(explicit, "name", explicit.__name__), explicit)]
        else:
            for nm in dir(mod):
                if nm.startswith("_") or not nm.endswith("Bot"):
                    continue
                obj = getattr(mod, nm)
                if isinstance(obj, type) and getattr(obj, "__module__", "") == full:
                    classes.append((getattr(obj, "name", nm), obj))
        for label, cls in classes:
            cls_name = cls.__name__ if isinstance(cls, type) else str(cls)
            try:                                   # prove it constructs+plays
                bot = _make_variant(modname, cls_name, 1)
            except Exception as exc:
                log(f"[pool] skip {full}.{cls_name}: "
                    f"{exc.__class__.__name__}: {exc}")
                continue
            if not (callable(bot) or hasattr(bot, "choose")):
                log(f"[pool] skip {full}.{cls_name}: not callable")
                continue
            out.append((f"var:{label}", ("variant", modname, cls_name)))
    # weight-vector variants
    vdir = os.path.join(ROOT, "engine", "bots", "variants")
    if os.path.isdir(vdir):
        for fn in sorted(os.listdir(vdir)):
            if not fn.endswith(".json"):
                continue
            try:
                from engine.bots.weighted import load_weights
                out.append((f"var:{fn[:-5]}", load_weights(os.path.join(vdir, fn))))
            except Exception as exc:
                log(f"[pool] skip {fn}: {exc.__class__.__name__}: {exc}")
    return out


def _spread(items, k):
    """`k` items spread evenly over `items`, endpoints always included."""
    if k <= 0 or not items:
        return []
    if len(items) <= k:
        return list(items)
    if k == 1:
        return [items[-1]]
    step = (len(items) - 1) / (k - 1)
    idx = sorted({int(round(i * step)) for i in range(k)})
    return [items[i] for i in idx]


def discover_past_champions(players, ladder_dirs, k=3, log=None):
    """Up to `k` archived champions, spread from oldest to newest.

    Oldest is always kept: the founder is the most *different* opponent in
    the archive, and difference is the whole point of a historical ladder.
    The selection is deterministic given the directory contents, so the
    same opponent set is faced by every candidate in a generation.
    """
    log = log or (lambda *_a: None)
    from engine.bots.weighted import DEFAULT_WEIGHTS
    files = []
    for d in ladder_dirs:
        if not os.path.isdir(d):
            continue
        for fn in sorted(os.listdir(d)):
            if fn.endswith(".json"):
                files.append(os.path.join(d, fn))
    files.sort(key=lambda p: (os.path.basename(p), p))
    out = []
    for path in _spread(files, k):
        try:
            with open(path) as fh:
                j = json.load(fh)
            w = dict(DEFAULT_WEIGHTS)
            w.update(j.get("weights", j))
            tag = os.path.splitext(os.path.basename(path))[0]
            parent = os.path.basename(os.path.dirname(path))
            out.append((f"past:{parent}/{tag}", w))
        except Exception as exc:
            log(f"[pool] skip {path}: {exc.__class__.__name__}: {exc}")
    return out


# ----------------------------------------------------------------- builder

def parse_tier_weights(s, base=None):
    """``"book=4,variant=2.5,floor=0"`` -> a tier-weight dict."""
    out = dict(base or DEFAULT_TIER_WEIGHTS)
    for part in (s or "").split(","):
        part = part.strip()
        if not part:
            continue
        k, _, v = part.partition("=")
        k = k.strip()
        if k not in out:
            raise SystemExit(f"unknown pool tier {k!r}; known: {sorted(out)}")
        out[k] = float(v)
    return out


def build_pool(players, ladder_dirs=(), tier_weights=None, past_k=3,
               with_quiescent=False, quiesce_opts=None, exclude=(),
               gate_tiers=DEFAULT_GATE_TIERS, log=None):
    """Assemble the full pool for one player count.

    Tiers whose weight is 0 are dropped entirely -- that is how you turn a
    tier off (``--pool-weights past=0``) without a second flag.
    """
    log = log or (lambda *_a: None)
    tw = dict(tier_weights or DEFAULT_TIER_WEIGHTS)
    exclude = set(exclude or ())
    entries = []

    def add(label, spec, tier):
        if tw.get(tier, 0.0) <= 0.0 or label in exclude:
            return
        entries.append(PoolEntry(label, spec, tier))

    add("book", "book", "book")
    add("book2", "book2", "book")
    for label, spec in discover_variants(log=log):
        add(label, spec, "variant")
    if with_quiescent:
        opts = dict(quiesce_opts or {})
        add("quiescent", ("quiescent", "default", opts), "quiescent")
    add("mirror", MIRROR, "mirror")
    for label, spec in discover_past_champions(players, ladder_dirs,
                                               k=past_k, log=log):
        add(label, spec, "past")
    add("greedy", "greedy", "floor")
    add("random", "random", "floor")
    add("default", "default", "floor")
    pool = Pool(entries, tier_weights=tw, gate_tiers=gate_tiers)
    log("[pool] " + ", ".join(f"{e.label}(w={e.weight:.2f})"
                              for e in pool.sorted_entries()))
    return pool


# ------------------------------------------------------------------ stats

def weighted_stats(samples, z=1.2816):
    """Weighted mean of paired per-game edges, its SE and a one-sided bound.

    `samples` is a list of ``(edge, weight)``.  Each OPPONENT contributes a
    fixed total weight regardless of how many games it played (the caller
    divides the entry weight by that opponent's game count), so playing an
    opponent more never silently increases its say in the aggregate -- it
    only sharpens that opponent's own estimate.

    SE is the standard weighted-mean estimator
    ``sqrt(sum(w_i^2 (x_i - m)^2)) / sum(w)`` with a small-sample
    ``n/(n-1)`` correction, which reduces to the ordinary SE when all
    weights are equal.
    """
    n = len(samples)
    if n < 2:
        return 0.0, 1.0, -1.0
    sw = sum(w for _x, w in samples)
    if sw <= 0:
        return 0.0, 1.0, -1.0
    m = sum(x * w for x, w in samples) / sw
    var = sum((w * (x - m)) ** 2 for x, w in samples) * n / (n - 1)
    se = math.sqrt(var) / sw
    return m, se, m - z * se


def mean_se(xs):
    """Plain mean and standard error of a list of numbers."""
    n = len(xs)
    if n == 0:
        return 0.0, 1.0
    m = sum(xs) / n
    if n < 2:
        return m, 1.0
    var = sum((x - m) ** 2 for x in xs) / (n - 1)
    return m, math.sqrt(var / n)
