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
``human``        the corpus-fitted archetypes in ``engine/bots/human/``.
                 These are the only opponents in the pool whose behaviour was
                 FITTED to something outside this repo -- the 1,011 BGO human
                 games -- and the only stochastic ones.  See
                 docs/HUMAN_BOTS.md.
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
``hall``         frozen champions from ``--hall-dir``, which -- unlike
                 ``past`` -- are never rotated out.  Its own tier since the
                 2026-07-27 rebalance, so it can be weighted separately.
``floor``        GreedyBot / RandomBot / the untrained default weights:
                 cheap floor checks.  **Weight 0 by default** -- they are
                 saturated (see the comment in ``build_pool``).
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

# Tier totals.  Everything here is a dial:
#   --pool-weights book=4,variant=2.5,past=0.5,hall=0,floor=0 ...
#
# THE 2026-07-27 REBALANCE (docs/LEAGUE_OBJECTIVE.md section 3).  The previous
# totals were book 3.0 / variant 2.5 / mirror 1.0 / past 1.0 / floor 0.5, i.e.
# 5.5 of 8.0 = **69% of the training signal came from hand-written static bots
# that never improve**, and every one of them is `BookBot` or a `BookBot`
# subclass (docs/TWOP_PROFILE.md section 9: the pool is a monoculture).  The
# champion learned to farm that monoculture's hard-coded numeric triggers --
# `var:military` reaches the +3 strength lead its offence is gated on for 5.5%
# of turns against the champion versus 41-44% against the rest of its own
# family.  That is exploiting an implementation artefact, not learning the game.
#
# So the static tiers are cut to 24% of the total: enough to be a SANITY FLOOR
# (they still hold the veto, see DEFAULT_GATE_TIERS) and a source of
# behavioural diversity, not enough to be the gradient.  The majority now sits
# on opponents that move: `mirror` (the candidate's own parent), `past` (the
# rotating archived ladder) and `hall` (frozen champions that never age out).
DEFAULT_TIER_WEIGHTS = {
    "book": 0.6,            # external yardstick: sanity floor + veto, not gradient
    "variant": 0.6,         # human strategy archetypes: diversity, not gradient
    # Corpus-fitted, stochastic, no hand-written trigger to hold shut
    # (docs/HUMAN_BOTS.md).  This is the non-exploitable replacement for the
    # `variant` monoculture, so it takes the same veto/diversity role and the
    # same weight -- the 2026-07-27 rebalance moved the gradient onto the
    # self-play tiers, and these bots are external anchors, not gradient.
    "human": 0.6,
    "quiescent": 2.0,       # deeper search (opt-in: expensive; off by default)
    "mirror": 1.0,          # self-play against the immediate parent
    "past": 1.2,            # the rotating anti-cycling ladder
    "hall": 1.6,            # frozen champions, the least exploitable opponents
    "floor": 0.0,           # greedy / random / untrained default -- OFF, see below
}

#: The weights every champion before 2026-07-27 was selected under.  Pass
#: `--pool-weights "$(python3 -c 'import experiments.hillclimb_pool as P;
#: print(P.legacy_weight_string())')"` (or just the literal string below) to
#: reproduce a historical run.  NB `hall` did not exist as a tier then -- the
#: hall-of-fame files were added to `past` and diluted it.
LEGACY_TIER_WEIGHTS = {
    "book": 3.0, "variant": 2.5, "quiescent": 2.0,
    "mirror": 1.0, "past": 1.0, "hall": 1.0, "floor": 0.5,
}

# Tiers whose opponents can VETO an acceptance on their own: losing to these
# is not something a good aggregate is allowed to excuse.  Deliberately still
# the STATIC tiers even though their weight was cut by 5x: their job changed
# from "supply the gradient" to "stop the climber walking off a cliff", and a
# veto is exactly that job.  A self-play tier cannot do it -- "do not regress
# against your own parent" is a statement about the lineage, not about play.
# `human` joins them: an unexploitable external opponent is exactly the kind
# of cliff-guard the monoculture could not be (docs/HUMAN_BOTS.md).
DEFAULT_GATE_TIERS = ("book", "variant", "quiescent", "human")

#: Tiers that `acceptance_subset` guarantees a representative of every
#: generation, alongside `mirror` and one rotating gate.  Without this the
#: rotation can hand a generation mirror + three 0.10-weight variants, and
#: mirror alone then carries 77% of that generation's accept decision -- which
#: is the mirror-only training loop this whole module exists to replace.
DEFAULT_LADDER_TIERS = ("hall", "past")

# Tiers scored on CULTURE MARGIN instead of win share.  See `margin_share`.
# Only consulted by the LEGACY `--objective margin` mode; the own/blend
# objectives apply one metric to the whole pool.
DEFAULT_MARGIN_TIERS = ("book", "variant", "quiescent", "human")

# Tier order used for display.
TIER_ORDER = ("book", "human", "variant", "quiescent", "mirror", "past",
              "hall", "floor")


def legacy_weight_string(base=None):
    """The `--pool-weights` string that restores the pre-2026-07-27 pool."""
    return ",".join(f"{k}={v:g}" for k, v in
                    sorted((base or LEGACY_TIER_WEIGHTS).items()))


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
        if spec[0] == "human":
            return _make_human(spec, seed)
    return _BASE_MAKE_BOT(spec, seed)


arena.make_bot = make_bot


def _make_human(spec, seed):
    """Build ``engine.bots.human.<module>.<cls>``, optionally re-profiled.

    Spec shape is ``("human", module, cls_name)`` or, for the fitter,
    ``("human", module, cls_name, profile_json)`` -- a JSON string rather than
    a dict because the spec has to survive pickling into an arena worker and a
    frozenset knob (``tech_veto``) does not round-trip through JSON as itself.
    Only ``tools/human_fit.py`` uses the four-element form; the pool always
    ships the fitted class as written.
    """
    mod = importlib.import_module(f"engine.bots.human.{spec[1]}")
    cls = getattr(mod, spec[2])
    prof = json.loads(spec[3]) if len(spec) > 3 and spec[3] else None
    return cls(seed=seed, profile=prof)


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

    __slots__ = ("label", "spec", "tier", "weight", "metric")

    def __init__(self, label, spec, tier, weight=0.0, metric="winshare"):
        self.label = label
        self.spec = spec
        self.tier = tier
        self.weight = weight
        # "winshare" or "margin" -- which per-game series this opponent is
        # scored on.  Set by the owning Pool from its `margin_tiers`.
        self.metric = metric

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

    def __init__(self, entries, tier_weights=None, gate_tiers=DEFAULT_GATE_TIERS,
                 margin_tiers=DEFAULT_MARGIN_TIERS, metric="winshare",
                 ladder_tiers=DEFAULT_LADDER_TIERS):
        self.entries = list(entries)
        self.tier_weights = dict(tier_weights or DEFAULT_TIER_WEIGHTS)
        self.gate_tiers = tuple(gate_tiers)
        self.ladder_tiers = tuple(ladder_tiers)
        # The pool-wide default metric.  `margin_tiers` overrides it per tier
        # and exists only so the LEGACY objective (win share everywhere except
        # a margin-scored gate) stays reproducible bit for bit.  Under the
        # own/blend objectives `margin_tiers` is empty and every opponent is
        # scored on the same thing, which is the point: the objective is a
        # property of the RUN, not of which tier an opponent happens to sit in.
        self.metric = metric
        # Which tiers score on culture margin rather than win share.  Only the
        # tiers where win share is DEGENERATE need it: the champion beats
        # `floor`, plays `past` and `mirror` roughly evenly, and win share is
        # both meaningful and the thing we actually care about there.  The
        # gate tiers are the ones it loses to ~100% of the time, where win
        # share carries no information at all.
        self.margin_tiers = tuple(margin_tiers)
        self.renormalise()

    def renormalise(self):
        """entry.weight = tier total / members of that tier (see docstring)."""
        counts = {}
        for e in self.entries:
            counts[e.tier] = counts.get(e.tier, 0) + 1
        for e in self.entries:
            total = self.tier_weights.get(e.tier, 0.0)
            e.weight = total / counts[e.tier] if counts[e.tier] else 0.0
            e.metric = "margin" if e.tier in self.margin_tiers else self.metric

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

        Three invariants:
          * mirror is always in, when present (under win share it is nearly
            free -- the reference share is 1/players by construction and needs
            no champion games; under own/blend the reference IS played);
          * at least one GATE opponent is always in, rotating through them,
            so "beat five weak bots" can never be a winning strategy;
          * at least one LADDER opponent (`hall`/`past`) is always in,
            rotating.  This one is new and it is load-bearing after the weight
            rebalance: the gate tiers now carry 0.10-0.30 each against
            mirror's 1.0, so a generation whose two free slots both land on
            variants would be decided ~77% by mirror alone.
        """
        size = max(1, size)
        chosen, seen = [], set()

        def take(e):
            if e is not None and e.label not in seen:
                seen.add(e.label)
                chosen.append(e)

        def rotate(pool_):
            if pool_:
                take(sorted(pool_, key=lambda e: e.label)[gen % len(pool_)])

        mirrors = [e for e in self.entries if e.tier == "mirror"]
        for e in mirrors:
            take(e)
        rotate(self.gates())
        rotate([e for e in self.entries if e.tier in self.ladder_tiers])
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
    seen = set()

    def emit(label, modname, cls_name, full):
        """Record one variant, with a label that is UNIQUE in the pool.

        Uniqueness is not cosmetic.  Labels are the identity of an opponent
        everywhere downstream: `acceptance_subset` de-duplicates by label
        (so a colliding tier collapses to ONE member per generation), the
        per-opponent stats are keyed by label, and `by_label` returns the
        first match.  The roster classes all inherit BookBot's ``name``
        attribute -- it is literally ``"book"`` on every one of them -- so a
        label taken straight off the class silently turned the seven-member
        variant tier into one opponent named ``var:book``.
        """
        try:                                       # prove it constructs+plays
            bot = _make_variant(modname, cls_name, 1)
        except Exception as exc:
            log(f"[pool] skip {full}.{cls_name}: "
                f"{exc.__class__.__name__}: {exc}")
            return
        if not (callable(bot) or hasattr(bot, "choose")):
            log(f"[pool] skip {full}.{cls_name}: not callable")
            return
        lab = f"var:{label}"
        if lab in seen:                            # fall back to a name that
            lab = f"var:{modname}.{cls_name}"      # cannot collide
            i = 2
            while lab in seen:
                lab, i = f"var:{modname}.{cls_name}#{i}", i + 1
            log(f"[pool] variant label var:{label} already taken -> {lab}")
        seen.add(lab)
        out.append((lab, ("variant", modname, cls_name)))

    # The package naming its own roster is the reliable source of labels;
    # `VARIANTS` keys are distinct by construction and it omits the abstract
    # base class that a blind module scan would otherwise enrol as a bot.
    registry = getattr(pkg, "VARIANTS", None)
    paths = [] if isinstance(registry, dict) and registry else \
        list(getattr(pkg, "__path__", []))
    if not paths:
        for label, cls in registry.items():
            mod = getattr(cls, "__module__", "")
            emit(str(label), mod.rsplit(".", 1)[-1], cls.__name__, mod)
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
                    # NAME before name: the roster's own per-class identifier
                    # before the one every class inherits from BookBot.
                    classes.append((getattr(obj, "NAME", None)
                                    or getattr(obj, "name", None) or nm, obj))
        for label, cls in classes:
            cls_name = cls.__name__ if isinstance(cls, type) else str(cls)
            emit(str(label), modname, cls_name, full)
    # weight-vector variants
    vdir = os.path.join(ROOT, "engine", "bots", "variants")
    if os.path.isdir(vdir):
        for fn in sorted(os.listdir(vdir)):
            if not fn.endswith(".json"):
                continue
            try:
                from engine.bots.weighted import load_weights
                lab = f"var:{fn[:-5]}"
                if lab in seen:
                    continue
                seen.add(lab)
                out.append((lab, load_weights(os.path.join(vdir, fn))))
            except Exception as exc:
                log(f"[pool] skip {fn}: {exc.__class__.__name__}: {exc}")
    return out


def discover_humans(names=("all",), log=None):
    """The corpus-fitted archetypes in ``engine/bots/human/``.

    `names` is ``("all",)``, ``("none",)`` or an explicit list of short names.
    Never raises: like `discover_variants`, a missing or broken package logs
    one line and leaves the tier empty, so a training run started against an
    older checkout still works.
    """
    log = log or (lambda *_a: None)
    if not names or "none" in names:
        return []
    try:
        from engine.bots.human import HUMANS
    except Exception as exc:
        log(f"[pool] no engine/bots/human ({exc.__class__.__name__}: {exc})"
            " -- human tier empty")
        return []
    want = sorted(HUMANS) if "all" in names else [n for n in names]
    out = []
    for n in want:
        cls = HUMANS.get(n)
        if cls is None:
            log(f"[pool] unknown human archetype {n!r};"
                f" known: {sorted(HUMANS)}")
            continue
        spec = ("human", cls.__module__.rsplit(".", 1)[-1], cls.__name__)
        try:                                     # prove it constructs
            _make_human(spec, 1)
        except Exception as exc:
            log(f"[pool] skip human:{n}: {exc.__class__.__name__}: {exc}")
            continue
        out.append((f"hum:{n}", spec))
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


def build_pool(players, ladder_dirs=(), tier_weights=None, past_k=2,
               with_quiescent=False, quiesce_opts=None, exclude=(),
               gate_tiers=DEFAULT_GATE_TIERS, hall_dirs=(),
               margin_tiers=None, metric="winshare",
               ladder_tiers=DEFAULT_LADDER_TIERS, human_bots=("all",),
               log=None):
    """Assemble the full pool for one player count.

    Tiers whose weight is 0 are dropped entirely -- that is how you turn a
    tier off (``--pool-weights past=0``) without a second flag.

    `margin_tiers` defaults to the legacy gate-tier list when `metric` is the
    legacy ``winshare`` and to *nothing* otherwise.  A caller that asks for
    ``own``/``blend`` means it for the whole pool; silently leaving the gate
    tiers on margin would reproduce the exact bug this change exists to fix,
    in the half of the pool that used to carry 69% of the weight.
    """
    log = log or (lambda *_a: None)
    if margin_tiers is None:
        margin_tiers = DEFAULT_MARGIN_TIERS if metric == "winshare" else ()
    tw = dict(tier_weights or DEFAULT_TIER_WEIGHTS)
    exclude = set(exclude or ())
    entries = []

    def add(label, spec, tier):
        if tw.get(tier, 0.0) <= 0.0 or label in exclude:
            return
        entries.append(PoolEntry(label, spec, tier))

    add("book", "book", "book")
    add("book2", "book2", "book")
    for label, spec in discover_humans(human_bots, log=log):
        add(label, spec, "human")
    for label, spec in discover_variants(log=log):
        add(label, spec, "variant")
    if with_quiescent:
        opts = dict(quiesce_opts or {})
        add("quiescent", ("quiescent", "default", opts), "quiescent")
    add("mirror", MIRROR, "mirror")
    for label, spec in discover_past_champions(players, ladder_dirs,
                                               k=past_k, log=log):
        add(label, spec, "past")
    # The hall of fame: frozen champions that are NEVER rotated out, unlike
    # the `past` ladder whose k slots are re-spread every generation so a
    # champion is eventually aged out by its own descendants.  Every file in
    # these dirs joins the pool, so keep them small and deliberate.
    #
    # These are the least exploitable opponents we have.  Every other trained
    # opponent here is BookBot or a BookBot subclass with hand-written numeric
    # triggers, and docs/TWOP_PROFILE.md measured how much of the champion's
    # margin comes from holding those triggers shut rather than from playing
    # well -- MilitaryBot reaches its required +3 lead on 5.5% of turns against
    # the champion versus 41-44% against the rest of the family.  A frozen
    # trained vector has no such switch to find.
    hall = []
    for d in hall_dirs:
        if os.path.isdir(d):
            hall.extend(discover_past_champions(players, [d],
                                                k=len(os.listdir(d)), log=log))
    for label, spec in hall:
        add(label.replace("past:", "hall:", 1), spec, "hall")
    # The floor tier defaults to weight 0 and is therefore usually absent.
    # Under win share it was inert-by-construction (the champion beats all
    # three 97.9-100%, so candidate and reference both score 1.0 and every
    # paired diff is exactly 0.0 with se 0.0 -- docs/UNATTENDED.md trap 2).
    # Under `own`/`blend` it is NOT inert any more: a punching bag that never
    # competes for the card row and never attacks lets a candidate farm
    # culture in a way no real opponent does, so it would start actively
    # pulling the vector toward a policy tuned for an opponent that does not
    # play.  Turning it on under those objectives is a deliberate act:
    # `--pool-weights floor=0.5`.
    add("greedy", "greedy", "floor")
    add("random", "random", "floor")
    add("default", "default", "floor")
    pool = Pool(entries, tier_weights=tw, gate_tiers=gate_tiers,
                margin_tiers=margin_tiers, metric=metric,
                ladder_tiers=ladder_tiers)
    log("[pool] " + ", ".join(
        f"{e.label}(w={e.weight:.2f},{e.metric})"
        for e in pool.sorted_entries()))
    tot = sum(e.weight for e in pool.entries) or 1.0
    share = {}
    for e in pool.entries:
        share[e.tier] = share.get(e.tier, 0.0) + e.weight
    static = sum(v for k, v in share.items() if k in ("book", "variant"))
    log("[pool] tier share: " + ", ".join(
        f"{t}={share[t] / tot:.0%}" for t in TIER_ORDER if t in share)
        + f"  (static book+variant {static / tot:.0%}, "
        f"self-play {(tot - static) / tot:.0%})")
    return pool


# ------------------------------------------------------- margin scoring
#
# WHY.  Win share is a step function.  Against an opponent the champion never
# beats it is 0.0 on every game, so the paired edge (candidate - champion) is
# exactly 0.0 with se exactly 0.0, and that row can neither reward nor veto.
# Measured on the clean DEFAULT_WEIGHTS start (docs/LEAGUE_TRAINING.md, "The
# pool is too hard at the bottom"): 0-11% at 3p and 0-2.8% at 4p against the
# whole gate tier, seven of eight gate rows a flat 0.0% at 4p.  The strongest
# and highest-weighted half of the pool was invisible to the gradient
# PRECISELY BECAUSE IT IS STRONG, and the accept decision fell back on
# mirror/past/floor -- the weak-baseline problem the league exists to replace.
#
# Culture margin is dense: it exists on every game, and "lost by 8" is real
# information that "lost by 40" is not.
#
# THE NORMALISATION.  A margin cannot be averaged into the same aggregate as
# a win share as-is -- it is measured in culture points, tens of them, and
# would swamp every win-share tier and make the tier weights meaningless.  So
# a margin is mapped onto a win-share-LIKE number in (0, 1):
#
#     margin_share(m) = 0.5 * (1 + tanh(m / MARGIN_SCALE))
#
# The properties that make this safe to mix with win share in one aggregate:
#
#   range      (0, 1), the same interval win share lives in, so a PAIRED edge
#              (candidate - champion) lands in (-1, +1) for both metrics and
#              `weighted_stats` can average them together untouched.  A tier
#              weight therefore still buys the same share of the decision it
#              bought before.
#   null       equal play scores margin 0 -> 0.5, and the paired difference of
#              two equal policies is 0 -- the same null the win-share pairing
#              has.  `_aggregate`'s "the null is exactly 0 whatever the pool
#              contains" guarantee is preserved.
#   monotone   strictly increasing in m, so MORE CULTURE IS ALWAYS A BETTER
#              SCORE.  There is no region where the gradient inverts; a
#              deliberately-worse vector must score worse.
#   bounded    saturating, not linear.  Margin has fat tails (blowouts past
#              200 culture are measured below), and an unbounded linear score
#              would let one lucky blowout dominate a weighted mean AND its
#              SE, so a candidate could be accepted on a single outlier game.
#              tanh bounds each game's influence exactly as win share does.
#
# MARGIN_SCALE sets how many culture points count as "one decisive game", and
# it is MEASURED, not guessed -- `experiments/margin_calib.py` dumps the
# per-game margin distribution of DEFAULT_WEIGHTS against every gate opponent:
#
#     3p   gate pooled n=192  mean -60.3  sd 49.6  p10 -129.5  p90 -1.5
#     4p   gate per-opponent means -56 (var:military) .. -144 (var:culture),
#          per-game extremes to -224
#
# Getting this constant wrong re-creates the bug it fixes.  The champion does
# not sit near margin 0 -- it sits 60 (3p) to 120 (4p) culture points BEHIND.
# A small scale would put the entire operating region deep in tanh's flat
# tail: at scale 45 a 4p margin of -120 maps to -0.996, where a 15-point
# improvement moves the score by 0.0004 and the gradient is dead again, just
# more quietly than before.
#
# So the rule is: the scale must be large enough that the MEASURED operating
# band sits in tanh's near-linear core.  120 is ~2.5x the measured per-game sd
# (~50 at 3p, ~45 at 4p) and keeps the whole band inside |m/scale| <~ 1.8,
# where tanh keeps a usable slope, while still bounding the -224 extreme at
# -0.94 instead of letting it dominate.  As the bot improves its margins move
# TOWARD zero, i.e. toward the most linear part of the curve, so the constant
# does not need re-tuning as the run progresses.
#
# Larger = gentler and more linear, more outlier influence; smaller = closer
# to a win/lose step function (as scale -> 0 it degenerates to the sign of the
# margin, which is roughly the win-share behaviour we are replacing).
# Overridable per run with --margin-scale.
MARGIN_SCALE = 120.0


def margin_share(margin, scale=MARGIN_SCALE):
    """Culture margin -> a win-share-like score in (0, 1).  See above."""
    if margin is None:
        return None
    return 0.5 * (1.0 + math.tanh(float(margin) / float(scale)))


# -------------------------------------------------- own-culture scoring
#
# WHY THIS REPLACED MARGIN AS THE DEFAULT (docs/LEAGUE_OBJECTIVE.md).
#
# You win Through the Ages by having the most culture.  You do not win it by
# having the biggest gap.  Those are the same objective in a two-player game
# ONLY if culture is conserved -- and it is not: war and aggression MOVE
# culture from the victim to the attacker.  A stolen point moves
# (mine - theirs) by TWO; a produced point moves it by ONE.  So a margin gate
# pays double for theft, and a hill climber will find that.
#
# It did.  docs/TWOP_PROFILE.md measures the resulting 2p champion: 69% of its
# 85.5-point margin against `book` is the war/aggression move class, banning
# that class barely moves its own score (131.0 -> 119.8) while nearly doubling
# `book`'s (45.5 -> 93.8), and it is BEHIND on tech and wonders while doing it.
# On the same engine, with scoring validated exact against 1,011 human BGO
# journals (docs/SCORE_VALIDATION.md), final own culture reads:
#
#     humans                          159.5  [156.0, 163.0]
#     the 1-ply vector we replaced    139.8  [131.6, 148.3]
#     the margin-trained champion      64.7
#
# It holds its rival to 26 and scores 65.  It wins 97.9% of its pool and would
# be crushed by a competent player.  Scoring on `per_game_culture` pays a
# stolen point exactly once, which is what the rules do.
#
# THE SQUASH.  Same three requirements as `margin_share`: land in (0, 1) so a
# paired edge lands in (-1, +1) and can be averaged in one aggregate with win
# share; be strictly monotone in culture so more culture is always a better
# score; be bounded so one blowout cannot carry an accept.  One difference:
# a margin is centred on 0 by construction, whereas own culture is strictly
# positive and lives around 40-200, so the squash has to be OFFSET or the
# whole operating band sits on one side of the curve where the slope decays.
#
#     own_share(c) = 0.5 * (1 + tanh((c - CULTURE_CENTRE) / CULTURE_SCALE))
#
# CULTURE_CENTRE = 100 sits between where we are (65) and where humans are
# (160); with CULTURE_SCALE = 120 the marginal value of one culture point is
# 0.00383 at c=65 and 0.00327 at c=160, i.e. **flat to 17% across the entire
# band we care about**.  Uncentred (the naive `tanh(c/120)`) the same ratio is
# 3.1x, which would have priced a point of culture at a human score at a third
# of a point at our current score -- a built-in bias against ever getting
# there.  The 40-200 band maps to |u| <= 0.83, comfortably inside tanh's
# near-linear core, while a 400-point outlier still saturates at 0.99.
CULTURE_SCALE = 120.0
CULTURE_CENTRE = 100.0

#: Weight on the WIN-SHARE component of the `blend` objective; 1 - alpha goes
#: on own culture.  See `score_series` and docs/LEAGUE_OBJECTIVE.md section 2.
DEFAULT_ALPHA = 0.15


def own_share(culture, scale=CULTURE_SCALE, centre=CULTURE_CENTRE):
    """Own final culture -> a win-share-like score in (0, 1).  See above."""
    if culture is None:
        return None
    return 0.5 * (1.0 + math.tanh((float(culture) - centre) / float(scale)))


class ScoreParams:
    """The constants a per-game score needs, carried as one object.

    Threading five floats through `RefCache`, `_series`, `score_candidate` and
    `ablate` individually is how a run ends up scoring the candidate on one
    objective and the reference on another.  One object, passed everywhere.
    """

    __slots__ = ("margin_scale", "culture_scale", "culture_centre", "alpha")

    def __init__(self, margin_scale=MARGIN_SCALE, culture_scale=CULTURE_SCALE,
                 culture_centre=CULTURE_CENTRE, alpha=DEFAULT_ALPHA):
        self.margin_scale = float(margin_scale)
        self.culture_scale = float(culture_scale)
        self.culture_centre = float(culture_centre)
        self.alpha = float(alpha)

    def __repr__(self):
        return (f"ScoreParams(margin_scale={self.margin_scale:g}, "
                f"culture_scale={self.culture_scale:g}, "
                f"culture_centre={self.culture_centre:g}, "
                f"alpha={self.alpha:g})")


DEFAULT_SCORE_PARAMS = ScoreParams()

#: Metrics whose champion reference against a MIRROR opponent is known
#: analytically and therefore costs no games.  See `RefCache.get`: a champion
#: at a table of itself takes 1/players of the wins and a culture margin of
#: exactly 0 by symmetry, but its own CULTURE is an ordinary unknown quantity
#: that has to be played for.
ANALYTIC_MIRROR_METRICS = ("winshare", "margin")


def score_series(res, metric, params=None):
    """Per-game scoring series from an `arena.duel` result.

    `metric` is one of:

      ``winshare``  the task-ordered per-game share list (the historical
                    default; flat 0.0 against an opponent nobody beats, and
                    saturated at 0.94-0.97 against `book` under PlanBot, so it
                    cannot discriminate at either end);
      ``margin``    the same games' culture margins through `margin_share`
                    (the historical GATE metric -- kept so every vector
                    selected under it stays reproducible, and because it is
                    still the right thing when you genuinely want a
                    differential);
      ``own``       the same games' OWN final culture through `own_share`:
                    dense, continuous, and the thing the rules score;
      ``blend``     ``(1 - alpha) * own + alpha * winshare``.  Both components
                    are already in (0, 1) with a paired null of exactly 0, so
                    a convex combination of them is too.

    Every branch is task-ordered and None-preserving, so a candidate series and
    a champion series played on the same seeds pair element by element.
    """
    p = params or DEFAULT_SCORE_PARAMS
    if metric == "margin":
        return [margin_share(m, p.margin_scale)
                for m in res.get("per_game_margin") or []]
    if metric == "own":
        return [own_share(c, p.culture_scale, p.culture_centre)
                for c in res.get("per_game_culture") or []]
    if metric == "blend":
        own = [own_share(c, p.culture_scale, p.culture_centre)
               for c in res.get("per_game_culture") or []]
        win = res["per_game"]
        a = p.alpha
        return [None if (o is None or w is None) else (1.0 - a) * o + a * w
                for o, w in zip(own, win)]
    return res["per_game"]


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
