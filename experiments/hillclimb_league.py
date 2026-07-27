"""(1+lambda) hill climbing scored against a DIVERSE OPPONENT POOL.

    python3 -m experiments.hillclimb_league --players 2 --hours 8 --workers 3

What is different from `experiments/hillclimb.py`
------------------------------------------------
The old loop scores a mutant against a mirror of its own parent (plus that
parent's ancestors).  That answers "is this a better response to my lineage",
which is not "is this stronger": docs/STRENGTH_CHECK.md shows a hand-written
rule list beating the resulting 2p champion 62.9% +/- 4.7% while every
internal metric reported progress.  Beating a weak baseline by 88% told us
nothing.

Here a candidate is scored against `experiments/hillclimb_pool.py`: BookBot,
the strategy variants in `engine/bots/variants/`, mirror self-play, past
champions, and greedy/random floors -- all mixed in one run, each opponent
carrying an explicit configurable weight.  Every decision reports the
PER-OPPONENT table as well as the aggregate, and an opponent in the gate
tiers can VETO an accept on its own, so a candidate that farms five weak bots
and loses to the book is rejected no matter how good the aggregate looks.

The statistic is unchanged in spirit and still paired: for every (opponent,
seed, seat) the candidate plays, the CHAMPION plays the byte-identical game,
and we accumulate the difference in win share.  The null is therefore exactly
0 whatever the pool contains, and seed luck cancels.  The champion's
reference games are computed once per generation and shared by all lambda
candidates, so a pool of P opponents costs (lambda + 1) * P duels, not
2 * lambda * P.

Anti-overfit
------------
1. The acceptance subset ROTATES (`Pool.acceptance_subset`): a generation
   scores against a few opponents, never all of them, so weights cannot be
   fitted to the whole pool at once.  Mirror and one rotating gate opponent
   are always in.
2. Every `--full-check-every` generations the champion is re-measured
   against the FULL pool, and any opponent whose win rate dropped since the
   previous full check is logged -- loudly if the champion was never tested
   against it in between (`untested_regressions`).

Weight credit attribution
-------------------------
Accepting a mutant that moved 20 weights at once teaches you nothing about
any single weight -- `wonder_remaining` is a trained weight that measures as
worthless.  So every `--ablate-every` generations the trainer takes the next
`--ablate-k` weights off a rotating cursor, zeroes each one in the champion,
and plays that single-weight ablation against the pool.  A weight whose
removal measurably hurts is doing work; a weight whose removal changes
nothing inside a tight CI is noise; a weight whose removal HELPS is worse
than noise.  Verdicts accumulate in `weight_credit_{K}p.json`.

Everything is restart-safe (the supervisor `experiments/run_league.sh`
restarts this process hourly and the Discord bridge kills agents constantly):
the champion, the run state, the generation log, the full-check log and the
ablation log are all written to disk every generation, atomically.
"""
from __future__ import annotations

import argparse
import json
import os
import random
import sys
import time
import zlib

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine.bots.weighted import (DEFAULT_WEIGHTS, PHASE_KEYS,  # noqa: E402
                                  load_weights, save_weights)
from experiments import arena  # noqa: E402
from experiments import hillclimb_pool as P  # noqa: E402
# Reuse -- not fork -- the mutation operators and the accept-z machinery the
# old climber already tuned.  hillclimb.py is owned by other agents' running
# jobs; importing it keeps one implementation of `mutate`.
from experiments.hillclimb import FROZEN, mutate  # noqa: E402,F401

HERE = os.path.dirname(os.path.abspath(__file__))
DEFAULT_STATE = os.path.join(HERE, "league_state")


# ------------------------------------------------------- degeneracy guard
#
# The 4p champion produced by the old loop has `science` = -6.09.  A negative
# science weight INVERTS a cost term: expensive cards start looking like
# bargains (Alchemy priced at +67.04 at 4p against +5.86 at 2p), and 4p play
# collapsed to 9.7% +/- 2.7% until it was clamped by hand.  Nothing in the
# old loop noticed, because acceptance only ever compared a bundle against a
# mirror of itself -- a mirror is equally happy to be wrong in the same way.
#
# The rule below is derived from the weight vector rather than hand-listed,
# which is what keeps it correct as the vector grows: a VALUE term's default
# sign says which direction of the feature is good for its owner, so a trained
# value on the far side of zero is a sign inversion, not a strategy.
#
# It used to be ONE-SIDED -- only defaults `> 0` were checked -- on the stated
# grounds that "every legitimately negative term already has a negative default
# and is therefore untouched".  That was the bug.  25 of the 82 weights have a
# negative default and could invert freely with nothing logged, and one of them
# did: the 4p champion reached `rival_culture` = +5.611 against a default of
# -0.35 in a single accepted `kick` at gen 30, which prices one culture point in
# the leading rival's hands at +5.6 of the bot's own and makes stealing 10
# culture off the culture leader score -41.25.  See docs/CULTURE_GAP.md section
# 2c for the reproduction.  The guard below is two-sided: a value term may not
# cross zero away from its default sign, in either direction.
#
# NB `end_turn_bias` (-3.0) is one of the terms this newly protects.  It looks
# like a bug and is NOT: removing it was measured twice, five ways, and makes
# the bot much weaker (see the comment on it in engine/bots/weighted.py, and
# docs/WASTED_ACTIONS.md section 6), and "pass MORE instead" -- i.e. driving it
# positive -- measured 11.0%.  Locking its sign is protecting it, not fixing it.
#
# EXEMPTION -- the early/late PHASE MULTIPLIERS are deliberately not sign-locked
# in the newly-added direction.  They are not value terms.  A feature in
# PHASE_KEYS contributes `(w[k] + (1-L)*w[k_early] + L*w[k_late]) * v`, so the
# triple (base, early, late) is over-parameterised by exactly one degree of
# freedom: adding c to BOTH phase weights and subtracting c from the base is the
# identical policy.  The sign of a phase multiplier is therefore not even
# gauge-invariant -- `culture_rate_late` can be made positive without changing a
# single move -- and "the sign flipped" carries no information about whether the
# policy inverted.  What a phase weight's sign does encode is a strategic
# hypothesis about WHEN a thing matters ("strength_rel_early < 0" = being ahead
# on army early is not worth paying for), and the search is entitled to
# disagree with the default.
#
# THE EXEMPTION IS NOW SYMMETRIC (docs/CULTURE_GAP.md 19c fix #1).  It used to
# apply only to the ten phase multipliers whose default is NEGATIVE, on the
# stated grounds that dropping the clamp on the ten POSITIVE-default ones
# (`culture_late`, `culture_rate_early`, ...) was "a behaviour change to the
# search, not a correctness fix", and belonged in its own measured commit.
# CULTURE_GAP 15a is that measurement, and it says the clamp is NECESSARY AND
# SUFFICIENT for a spurious attractor at exactly 0.000:
#
#   pure-drift simulation (real mutate + real guard_weights, coin-flip accept,
#   120 gens, 300 runs, tools/drift_sim.py), rate of landing on EXACTLY 0.000:
#
#     positive-default phase multipliers, guard ON   15.7%   (20.0% observed)
#     negative-default phase multipliers, guard off   0.0%   ( 0.0% observed)
#     both halves, guard turned OFF                   0.0%
#
# So the asymmetry in the champions is manufactured by the guard, not by the
# game, and the gauge argument above says the clamp could not have been
# protecting anything in the first place.  Both halves are exempt now.
_PHASE_MULT = frozenset(k + suf for k in PHASE_KEYS
                        for suf in ("_early", "_late"))

#: value terms whose default is > 0: a trained value below zero is inverted.
#: Phase multipliers excluded -- see the EXEMPTION note above.
NONNEG = frozenset(k for k, v in DEFAULT_WEIGHTS.items()
                   if v > 0 and k not in _PHASE_MULT)
#: value terms whose default is < 0: a trained value above zero is inverted.
#: Phase multipliers excluded -- see the EXEMPTION note above.
NONPOS = frozenset(k for k, v in DEFAULT_WEIGHTS.items()
                   if v < 0 and k not in _PHASE_MULT)

# Applied ONLY when a run starts from DEFAULT_WEIGHTS (a clean restart).
# `hand_potential` (the card-identity fix, 72.5% +/- 4.4% at 2p) defaults to
# 0.125 for every player count and 4p currently regresses with it, so 4p
# starts the term switched off and lets the pool decide its value.
INIT_OVERRIDES = {4: {"hand_potential": 0.0}}


def guard_weights(w, mode="clamp"):
    """Flag (and by default clamp) sign-inverted value terms.

    Returns ``(weights, violations)``.  `mode` is ``clamp`` (set to 0 and
    carry on), ``flag`` (log only, change nothing) or ``reject`` (the caller
    throws the candidate away).
    """
    out = dict(w)
    viol = []
    for k in sorted(NONNEG | NONPOS):
        v = out.get(k, 0.0)
        if (v < 0) if k in NONNEG else (v > 0):
            viol.append({"weight": k, "value": round(float(v), 4),
                         "default": DEFAULT_WEIGHTS[k]})
            if mode == "clamp":
                out[k] = 0.0
    return out, viol


# ------------------------------------------------------------------- paths

def paths(state_dir, k):
    return {
        "champion": os.path.join(state_dir, f"champion_{k}p.json"),
        "state": os.path.join(state_dir, f"state_{k}p.json"),
        "gens": os.path.join(state_dir, f"generations_{k}p.jsonl"),
        "ladder": os.path.join(state_dir, f"ladder_{k}p"),
        "fullcheck": os.path.join(state_dir, f"fullcheck_{k}p.jsonl"),
        "ablation": os.path.join(state_dir, f"ablation_{k}p.jsonl"),
        "credit": os.path.join(state_dir, f"weight_credit_{k}p.json"),
        "guard": os.path.join(state_dir, f"guard_{k}p.jsonl"),
    }


def label_seed(label):
    """A stable per-opponent seed offset.

    `hash()` on a str is salted per process (PYTHONHASHSEED), so using it
    here would give a restarted supervisor different seeds for the same
    generation and quietly break the "same seeds across runs" guarantee.
    crc32 is stable forever.
    """
    return zlib.crc32(label.encode()) % 9973


def write_json(path, obj):
    os.makedirs(os.path.dirname(os.path.abspath(path)), exist_ok=True)
    tmp = path + ".tmp"
    with open(tmp, "w") as fh:
        json.dump(obj, fh, indent=1, sort_keys=True)
        fh.flush()
        os.fsync(fh.fileno())
    os.replace(tmp, path)


def append_jsonl(path, rec):
    os.makedirs(os.path.dirname(os.path.abspath(path)), exist_ok=True)
    with open(path, "a") as fh:
        fh.write(json.dumps(rec) + "\n")
        fh.flush()
        os.fsync(fh.fileno())


DEFAULT_STATE_REC = {
    "gen": 0, "sigma": 0.25, "since_accept": 0,
    "ablate_cursor": 0, "tested_since_check": [], "last_full_check": None,
}


def load_state(pp):
    rec = dict(DEFAULT_STATE_REC)
    if os.path.exists(pp["state"]):
        try:
            with open(pp["state"]) as fh:
                rec.update(json.load(fh))
        except Exception:
            pass
    return rec


def load_champion(pp, init, players, overrides=True, log=print):
    """Champion weights: resume if we have one, else `--init`.

    A CLEAN RESTART (no state, ``--init default``) is the intended way to run
    this: the old champions carry the degeneracy this loop exists to catch,
    and 4p's in particular is known bad.  `INIT_OVERRIDES` is applied only on
    that path.
    """
    if os.path.exists(pp["champion"]):
        return load_weights(pp["champion"]), "resume"
    if init and init != "default":
        w = load_weights(init)
        log(f"[{players}p] WARM START from {init} -- note that champions "
            f"trained by the old mirror loop can carry sign-inverted weights; "
            f"a clean start from DEFAULT_WEIGHTS is preferred")
        return w, f"init:{init}"
    w = dict(DEFAULT_WEIGHTS)
    if overrides:
        for k, v in INIT_OVERRIDES.get(players, {}).items():
            if k in w and w.get(k) != v:
                log(f"[{players}p] init override {k}: {w.get(k)} -> {v} "
                    f"(known {players}p regression; the pool decides its value "
                    f"from here)")
                w[k] = v
    return w, "default"


# -------------------------------------------------- the trained architecture
#
# The climber trains a WEIGHT VECTOR, but a weight vector is not a policy -- it
# only becomes one when some bot reads it.  Every trained vector reached the
# arena as a bare dict, and `arena.make_bot`'s fallthrough turns a bare dict
# into a 1-ply `WeightedBot`.  So the loop could only ever train weights for
# greedy 1-ply play, which is a problem now that two searching policies
# (`engine/bots/plan.py`, `engine/bots/quiescent.py`) read the SAME vector and
# beat the 1-ply bot with it: an evaluation function has to be tuned with the
# search it will run under, because the searcher's blind spots are what the
# weights have to cover.  docs/DEEPER_SEARCH.md.
#
# `arena.make_bot` already accepts `("plan", weights, opts)` and
# `("quiescent", weights, opts)`, so all that was missing was the wrapping.
# `as_spec` is applied at every point a trained vector crosses into the arena
# and NOWHERE else -- `mutate`, `guard_weights`, the champion file and the
# ladder all keep seeing plain dicts, so nothing about the search or the
# on-disk format changes.
#
# It wraps any plain dict, which deliberately includes the MIRROR opponent
# (the champion) and the PAST-ladder opponents (older champions): those are the
# same policy family and must be played by the same architecture or the
# self-play and anti-cycling tiers would be measuring an architecture gap
# instead of a weight gap.  It never wraps a str or tuple spec, which is every
# external opponent (`book`, `var:*`, `greedy`, ...), so those stay untouched
# and stay cheap.
#
# NOT persisted: the architecture is a property of the RUN, not of the vector,
# so it must be re-supplied on every restart.  `run_league.sh` forwards its
# extra args verbatim to each hourly restart, so passing it once is enough --
# but resuming an arm by hand without the flag silently reverts it to 1-ply.
CANDIDATE_ARCH = None       # None = 1-ply WeightedBot, the historical default


def parse_candidate_bot(text):
    """``"plan:width=8,samples=2"`` -> ``("plan", {"width": 8, "samples": 2})``."""
    if not text or text in ("weighted", "1ply", "default"):
        return None
    kind, _, rest = text.partition(":")
    if kind not in ("plan", "quiescent"):
        raise SystemExit(f"--candidate-bot: unknown architecture {kind!r} "
                         f"(want weighted, quiescent or plan)")
    opts = {}
    for part in filter(None, rest.split(",")):
        k, _, v = part.partition("=")
        opts[k.strip()] = float(v) if "." in v else int(v)
    return (kind, opts)


def as_spec(w):
    """A trained weight vector -> the arena spec for the architecture in play."""
    if CANDIDATE_ARCH is None or not isinstance(w, dict):
        return w
    kind, opts = CANDIDATE_ARCH
    return (kind, dict(w), dict(opts))


# ------------------------------------------------------------------ duels

def _series(spec, opp_spec, players, games, seed0, workers, metric,
            margin_scale=P.MARGIN_SCALE):
    """One duel -> the per-game series needed to score AND to report it.

    ``score`` is the series the accept decision uses (win share or normalised
    culture margin, per the opponent's metric); ``win`` and ``margin`` are
    always the raw win share and raw culture margin of THE SAME GAMES.  Both
    are kept whichever metric is in play, because the per-opponent table is a
    diagnostic an operator reads: switching the gate tier to margin must not
    cost us the win-rate column that tells us whether the bot can actually
    beat BookBot yet.
    """
    res = arena.duel(as_spec(spec), as_spec(opp_spec), players, games,
                     seed0=seed0, workers=workers)
    return {
        "score": P.score_series(res, metric, margin_scale),
        "win": res["per_game"],
        "margin": res["per_game_margin"],
    }


class RefCache:
    """The champion's per-game series, computed once, reused by every mutant.

    Keyed on (opponent label, block index).  A mirror entry needs no games at
    all: champion-vs-a-table-of-itself is 1/players by construction, and its
    culture margin is exactly 0 by the same symmetry -- over a complete seat
    rotation of one identical deterministic policy the seat's culture and the
    mean of the others' sum to the same total, so the margins cancel.
    """

    def __init__(self, champion, players, workers, block, seed_base,
                 margin_scale=P.MARGIN_SCALE):
        self.champion = champion
        self.players = players
        self.workers = workers
        self.block = block
        self.seed_base = seed_base
        self.margin_scale = margin_scale
        self.cache = {}
        self.games = 0

    def seed_for(self, entry, b):
        return (self.seed_base + b * 100_003
                + label_seed(entry.label) * 17) % 10_000_019

    def get(self, entry, b):
        key = (entry.label, b)
        if key in self.cache:
            return self.cache[key]
        if entry.is_mirror:
            share = 1.0 / self.players
            out = {"score": [share] * self.block, "win": [share] * self.block,
                   "margin": [0.0] * self.block}
            if entry.metric == "margin":
                out["score"] = [P.margin_share(0.0, self.margin_scale)] * self.block
        else:
            out = _series(self.champion, entry.spec, self.players, self.block,
                          self.seed_for(entry, b), self.workers, entry.metric,
                          self.margin_scale)
            self.games += self.block
        self.cache[key] = out
        return out


def score_candidate(cand, entries, ref, z, min_blocks, max_blocks,
                    veto_z, gate_tiers, log_games=None):
    """Play `cand` against `entries`, paired with the champion's reference.

    Returns ``(agg_mean, agg_se, agg_lo, per_opponent, games, veto)`` where
    `per_opponent` maps label -> dict(edge, se, win_rate, ref_rate, n, tier,
    weight, gate).

    Sequential: after each block the aggregate is re-tested; a candidate that
    has already cleared the accept bound stops early, and one whose mean edge
    is negative after the screening block stops paying for itself.  The
    per-opponent numbers are always kept, whichever way it ends, because a
    rejection that says WHICH opponent killed the candidate is the diagnostic
    the old loop never produced.
    """
    per = {e.label: {"tier": e.tier, "weight": e.weight, "diffs": [],
                     "mine": [], "theirs": [],
                     "mine_margin": [], "theirs_margin": [],
                     "metric": e.metric,
                     "gate": e.tier in gate_tiers} for e in entries}
    games = 0
    blocks = 0
    for b in range(max_blocks):
        for e in entries:
            mine = _series(cand, e.resolve(cand, ref.champion),
                           ref.players, ref.block, ref.seed_for(e, b),
                           ref.workers, e.metric, ref.margin_scale)
            theirs = ref.get(e, b)
            games += ref.block
            d = per[e.label]
            # `diffs` is the SCORED series (win share, or normalised culture
            # margin for a margin-tier opponent); the win/margin lists are the
            # same games' raw numbers, carried for the report only.
            for x, y in zip(mine["score"], theirs["score"]):
                if x is None or y is None:
                    continue
                d["diffs"].append(x - y)
            for x, y in zip(mine["win"], theirs["win"]):
                if x is not None and y is not None:
                    d["mine"].append(x)
                    d["theirs"].append(y)
            for x, y in zip(mine["margin"], theirs["margin"]):
                if x is not None and y is not None:
                    d["mine_margin"].append(x)
                    d["theirs_margin"].append(y)
        blocks += 1
        m, se, lo = _aggregate(per, z)
        if lo > 0.0 and blocks >= min_blocks:
            break
        if m < 0.0 and blocks >= 1:
            break
    m, se, lo = _aggregate(per, z)

    out, veto = {}, []
    for label, d in per.items():
        om, ose = P.mean_se(d["diffs"])
        def _avg(xs, nd=4):
            return round(sum(xs) / len(xs), nd) if xs else None

        rec = {
            "tier": d["tier"], "weight": round(d["weight"], 4),
            "n": len(d["diffs"]), "edge": round(om, 4), "se": round(ose, 4),
            "metric": d["metric"],
            "win_rate": _avg(d["mine"]),
            "champ_rate": _avg(d["theirs"]),
            # Raw culture margins, always reported.  On a margin-tier row
            # these are what the edge is actually computed from, and they are
            # the numbers that stay readable when the win rate is 0.0% on both
            # sides -- which at 4p is every gate opponent at the clean start.
            "margin": _avg(d["mine_margin"], 2),
            "champ_margin": _avg(d["theirs_margin"], 2),
            "gate": d["gate"],
        }
        out[label] = rec
        # VETO: a gate opponent whose edge is significantly negative sinks
        # the candidate no matter what the aggregate says.  This is the
        # explicit answer to "do not let the aggregate hide a candidate that
        # beats five weak bots and loses to BookBot".
        if d["gate"] and len(d["diffs"]) >= 2 and om + veto_z * ose < 0.0:
            veto.append(label)
    return m, se, lo, out, games, veto


def _aggregate(per, z):
    """Weight-weighted aggregate over per-game paired edges.

    Each opponent contributes a FIXED total weight (its pool weight) split
    over its own games, so an opponent that happened to play more games does
    not thereby get a bigger vote -- only a sharper estimate.
    """
    samples = []
    for d in per.values():
        n = len(d["diffs"])
        if not n or d["weight"] <= 0:
            continue
        w = d["weight"] / n
        samples.extend((x, w) for x in d["diffs"])
    return P.weighted_stats(samples, z)


# ------------------------------------------------------------- full check

def full_check(champion, pool, players, games, workers, seed, log=print):
    """The champion against EVERY pool opponent.  Absolute win rates.

    Mirror is skipped: a champion against a table of itself is 1/players by
    construction and measures nothing.
    """
    out = {}
    for e in pool.sorted_entries():
        if e.is_mirror:
            continue
        t0 = time.time()
        res = arena.duel(as_spec(champion), as_spec(e.spec), players, games,
                         seed0=(seed + label_seed(e.label)) % 10_000_019,
                         workers=workers)
        # Wall clock per opponent, recorded because it is a budget decision:
        # opponents differ by more than an order of magnitude in cost, and a
        # long run spends its hours wherever this number is largest.
        secs = time.time() - t0
        out[e.label] = {
            "tier": e.tier, "weight": round(e.weight, 4),
            "win_rate": round(res["win_rate"], 4), "ci": round(res["ci"], 4),
            # The gate tier sits at a flat 0.0% win rate at the clean start
            # (all eight opponents at 4p).  Without this column the full check
            # cannot tell "closed the gap by 30 culture" from "no change".
            "margin": round(res.get("margin", 0.0), 2),
            "metric": e.metric,
            "n": res["games"], "null": round(res["null"], 4),
            "secs": round(secs, 2),
            "secs_per_game": round(secs / max(1, res["games"]), 3),
        }
        log(f"    {e.label:<28} {res['win_rate']:6.1%} +/-{res['ci']:5.1%} "
            f"margin {res.get('margin', 0.0):+7.1f} "
            f"(null {res['null']:.0%}, n={res['games']}, tier={e.tier}, "
            f"{secs:.1f}s)")
    return out


def compare_checks(now, prev, tested, min_drop=0.03):
    """Regressions since the previous full check.

    `tested` is the set of labels the acceptance subsets actually used in
    between.  A regression against an opponent we DID test on is ordinary
    noise or a trade-off we chose; a regression against one we did NOT is
    exactly the overfitting this design exists to catch, so it is reported
    separately.
    """
    regs, untested = [], []
    for label, cur in now.items():
        old = (prev or {}).get(label)
        if not old:
            continue
        delta = cur["win_rate"] - old["win_rate"]
        noise = max(min_drop, (cur["ci"] ** 2 + old["ci"] ** 2) ** 0.5)
        if delta < -noise:
            rec = {"opponent": label, "tier": cur["tier"],
                   "was": old["win_rate"], "now": cur["win_rate"],
                   "delta": round(delta, 4), "noise": round(noise, 4)}
            regs.append(rec)
            if label not in tested:
                untested.append(rec)
    return regs, untested


# --------------------------------------------------------------- ablation

def ablate(champion, keys, pool, players, games, workers, seed, z,
           gate_only=True, max_opponents=3, mode="zero",
           margin_scale=P.MARGIN_SCALE, log=print):
    """Single-weight ablation of the champion against the pool.

    For each key: set it to 0 (or to its DEFAULT_WEIGHTS value) and play the
    result against a small fixed slice of the pool, paired against the
    unablated champion on the same seeds.  The champion's reference games are
    played ONCE and shared by every key in the cycle.

      edge < 0 significantly  -> the weight is load-bearing (removing hurts)
      edge > 0 significantly  -> the weight is HARMFUL (removing helps)
      CI covers 0             -> no measurable effect at this sample size
    """
    entries = [e for e in pool.sorted_entries()
               if not e.is_mirror and (e.tier in pool.gate_tiers or not gate_only)]
    if not entries:
        entries = [e for e in pool.sorted_entries() if not e.is_mirror]
    entries = entries[:max_opponents]
    if not entries:
        return []
    ref = RefCache(champion, players, workers, games, seed,
                   margin_scale=margin_scale)
    out = []
    for key in keys:
        w = dict(champion)
        w[key] = 0.0 if mode == "zero" else float(DEFAULT_WEIGHTS.get(key, 0.0))
        if w[key] == champion.get(key):
            continue                       # already at the ablated value
        per = {}
        for e in entries:
            # Same metric the accept decision uses.  Ablation slices the GATE
            # tier by default, so on win share it had exactly the flat-signal
            # problem this module's margin scoring exists to fix: every
            # ablation verdict against an unbeaten opponent was
            # "no-measurable-effect" by construction, not by measurement.
            mine = _series(w, e.spec, players, games, ref.seed_for(e, 0),
                           workers, e.metric, ref.margin_scale)
            theirs = ref.get(e, 0)
            diffs = [x - y for x, y in zip(mine["score"], theirs["score"])
                     if x is not None and y is not None]
            per[e.label] = diffs
        alld = [x for v in per.values() for x in v]
        m, se = P.mean_se(alld)
        if m + z * se < 0:
            verdict = "load-bearing"
        elif m - z * se > 0:
            verdict = "harmful"
        else:
            verdict = "no-measurable-effect"
        rec = {"weight": key, "value": round(champion.get(key, 0.0), 4),
               "ablated_to": w[key], "edge": round(m, 4), "se": round(se, 4),
               "n": len(alld), "verdict": verdict,
               "opponents": {k: round(P.mean_se(v)[0], 4) for k, v in per.items()}}
        out.append(rec)
        log(f"    ablate {key:<24} edge={m:+.4f} +/-{z * se:.4f} "
            f"n={len(alld)} -> {verdict}")
    return out


def update_credit(path, recs):
    """Accumulate ablation evidence per weight across cycles."""
    cur = {}
    if os.path.exists(path):
        try:
            with open(path) as fh:
                cur = json.load(fh)
        except Exception:
            cur = {}
    for r in recs:
        c = cur.setdefault(r["weight"], {"cycles": 0, "n": 0, "sum_edge": 0.0,
                                         "verdicts": {}})
        c["cycles"] += 1
        c["n"] += r["n"]
        c["sum_edge"] += r["edge"] * r["n"]
        c["verdicts"][r["verdict"]] = c["verdicts"].get(r["verdict"], 0) + 1
        c["mean_edge"] = round(c["sum_edge"] / max(1, c["n"]), 5)
        c["last"] = r["verdict"]
    write_json(path, cur)
    return cur


# -------------------------------------------------------------------- run

def format_table(per):
    rows = sorted(per.items(),
                  key=lambda kv: (-kv[1]["weight"], kv[0]))
    head = (f"      {'opponent':<26}{'tier':<11}{'w':>5} {'n':>4} "
            f"{'win%':>7}{'champ%':>8}{'marg':>8}{'cmarg':>8}{'edge':>9}")
    lines = [head]
    for label, r in rows:
        wr = "  --  " if r["win_rate"] is None else f"{r['win_rate']:6.1%}"
        cr = "  --  " if r["champ_rate"] is None else f"{r['champ_rate']:6.1%}"
        mg = "   --  " if r.get("margin") is None else f"{r['margin']:+7.1f}"
        cm = "   --  " if r.get("champ_margin") is None else f"{r['champ_margin']:+7.1f}"
        flag = " GATE" if r["gate"] else ""
        # A margin-scored row is marked, because its edge is NOT in win-share
        # units and reading it as one would be wrong.
        flag += "*" if r.get("metric") == "margin" else ""
        lines.append(f"      {label:<26}{r['tier']:<11}{r['weight']:5.2f} "
                     f"{r['n']:4d} {wr:>7}{cr:>8}{mg:>8}{cm:>8}"
                     f"{r['edge']:+9.4f}{flag}")
    lines.append("      (* = scored on culture margin, not win share; "
                 "marg/cmarg are raw culture points)")
    return "\n".join(lines)


def run(players=2, hours=1.0, workers=3, lam=2, block=12, min_blocks=1,
        max_blocks=4, subset=4, seed=1, accept_z=1.2816, veto_z=1.0,
        sigma_floor=0.08, stall_kick=15, state_dir=DEFAULT_STATE,
        tier_weights=None, past_k=2, with_quiescent=False, init="default",
        full_check_every=10, check_games=48, ablate_every=25, ablate_k=3,
        ablate_games=24, ablate_mode="zero", max_gens=0, legacy_ladders=True,
        hall_dirs=(),
        weight_guard="clamp", gate_metric="margin",
        margin_scale=P.MARGIN_SCALE, candidate_bot=None, log=print):
    global CANDIDATE_ARCH
    CANDIDATE_ARCH = candidate_bot
    # A block must be a whole number of seat rotations, or the seats are not
    # balanced and the "same seeds, same seats" pairing quietly stops being
    # an apples-to-apples comparison at 3p and 4p.
    want = max(players, (block // players) * players)
    if want != block:
        log(f"[{players}p] --block {block} is not a multiple of {players}; "
            f"using {want} so seat rotation stays exact")
        block = want
    pp = paths(state_dir, players)
    os.makedirs(state_dir, exist_ok=True)
    st = load_state(pp)
    champion, origin = load_champion(pp, init, players, log=log)
    # A resumed or warm-started champion is guarded too: the degeneracy may
    # already be baked into the file we just loaded.
    champion, viol = guard_weights(champion, weight_guard)
    if viol:
        log(f"[{players}p] !! WEIGHT GUARD on the {origin} champion: "
            + ", ".join(f"{v['weight']}={v['value']} (default "
                        f"{v['default']})" for v in viol)
            + (f" -- clamped to 0" if weight_guard == "clamp" else ""))
        append_jsonl(pp["guard"], {"gen": st["gen"], "players": players,
                                   "where": f"champion:{origin}",
                                   "mode": weight_guard, "violations": viol,
                                   "at": time.strftime("%F %T")})
        # `reject` means "refuse a sign-inverted vector".  It used to mean
        # that only for MUTANTS: guard_weights rewrites the vector in `clamp`
        # mode only, so a warm start under `reject` logged eight violations
        # and then trained happily from the 4p champion's science=-6.089.  The
        # flag name lied.  It is fatal at champion load now.
        if weight_guard == "reject":
            raise SystemExit(
                f"[{players}p] --weight-guard reject: the {origin} champion "
                f"has {len(viol)} sign-inverted weight(s) "
                + ", ".join(f"{v['weight']}={v['value']} (default "
                            f"{v['default']})" for v in viol)
                + ".  Refusing to train from it.  Use --weight-guard clamp to "
                  "neutralise them, --weight-guard flag to train from it "
                  "anyway, or --init default for a clean start.")
    gen = int(st["gen"])
    sigma = max(float(st["sigma"]), sigma_floor)
    since_accept = int(st["since_accept"])
    tested_since_check = set(st.get("tested_since_check") or [])
    hold_sigma = 0
    recent = []
    rng = random.Random(seed * 7919 + players * 101 + gen)
    t_end = time.time() + hours * 3600

    ladders = [pp["ladder"]]
    if legacy_ladders:
        legacy = os.path.join(HERE, f"league_{players}p")
        if os.path.isdir(legacy):
            ladders.append(legacy)

    if not os.path.exists(pp["champion"]):
        save_weights(pp["champion"], champion, gen=gen, sigma=sigma,
                     players=players)
    if not os.path.isdir(pp["ladder"]):
        save_weights(os.path.join(pp["ladder"], f"gen{gen:05d}.json"),
                     champion, gen=gen, players=players)

    margin_tiers = P.DEFAULT_MARGIN_TIERS if gate_metric == "margin" else ()

    def build():
        return P.build_pool(players, ladder_dirs=ladders,
                            tier_weights=tier_weights, past_k=past_k,
                            with_quiescent=with_quiescent,
                            hall_dirs=hall_dirs,
                            margin_tiers=margin_tiers, log=log)

    pool = build()
    log(f"[{players}p] league trainer: {len(pool)} opponents, "
        f"gen={gen} sigma={sigma:.3f} state={state_dir}")
    log(f"[{players}p] trained architecture: "
        + ("WeightedBot (1-ply greedy)" if CANDIDATE_ARCH is None else
           f"{CANDIDATE_ARCH[0]} {CANDIDATE_ARCH[1] or '(defaults)'} -- the "
           f"champion, the mirror and the past ladder all play this; external "
           f"opponents are unchanged"))
    log(f"[{players}p] gate metric: {gate_metric}"
        + (f" (scale {margin_scale:g} culture points; tiers "
           f"{','.join(margin_tiers)})" if margin_tiers else
           " -- WARNING: win share is flat 0.0 against opponents the champion "
           "never beats, so gate rows may return edge=0.0000 and neither "
           "reward nor veto"))

    start_gen = gen
    while time.time() < t_end and (not max_gens or gen < start_gen + max_gens):
        gen += 1
        t0 = time.time()
        entries = pool.acceptance_subset(gen, subset)
        tested_since_check.update(e.label for e in entries)
        # snapshot BEFORE any accept rebuilds the pool, so the log records the
        # field this generation was actually scored against
        pool_snapshot = [{"label": e.label, "tier": e.tier,
                          "weight": round(e.weight, 4)}
                         for e in pool.sorted_entries()]
        seed_base = (gen * 1_000_003 + seed * 7717) % 10_000_019
        ref = RefCache(champion, players, workers, block, seed_base,
                       margin_scale=margin_scale)

        forced = None
        if stall_kick and since_accept and since_accept % stall_kick == 0:
            forced = "kick"
            sigma = min(0.8, max(sigma, 0.25) * 2.0)
            hold_sigma = stall_kick // 3

        best, tried = None, []
        guard_hits = []
        for j in range(lam):
            mutant, moved, op = mutate(champion, rng, sigma, op=forced)
            # Catch the sign inversion at the moment it is proposed, not
            # hundreds of generations later.
            mutant, viol = guard_weights(mutant, weight_guard)
            if viol:
                rec_v = {"gen": gen, "players": players, "where": f"mutant:{j}",
                         "op": op, "mode": weight_guard, "violations": viol,
                         "at": time.strftime("%F %T")}
                guard_hits.extend(viol)
                append_jsonl(pp["guard"], rec_v)
                log(f"[{players}p] gen {gen} cand {j} weight guard "
                    f"({weight_guard}): "
                    + ", ".join(f"{v['weight']}={v['value']}" for v in viol))
                if weight_guard == "reject":
                    tried.append({"op": op, "moved": len(moved),
                                  "rejected_by_guard": viol})
                    continue
            m, se, lo, per, games, veto = score_candidate(
                mutant, entries, ref, accept_z, min_blocks, max_blocks,
                veto_z, pool.gate_tiers)
            tried.append({"edge": round(m, 4), "lo": round(lo, 4),
                          "games": games, "moved": len(moved), "op": op,
                          "veto": veto, "per_opponent": per})
            log(f"[{players}p] gen {gen} cand {j} op={op} "
                f"edge={m:+.4f} lo={lo:+.4f} games={games}"
                + (f" VETO={veto}" if veto else ""))
            log(format_table(per))
            if lo > 0.0 and not veto and (best is None or lo > best[2]):
                best = (mutant, m, lo, games, moved, op, per)

        accepted = best is not None
        if accepted:
            champion = best[0]
            since_accept = 0
            save_weights(os.path.join(pp["ladder"], f"gen{gen:05d}.json"),
                         champion, gen=gen, players=players)
            pool = build()            # the new champion joins the past tier
        else:
            since_accept += 1
        recent = (recent + [accepted])[-12:]
        if hold_sigma > 0:
            hold_sigma -= 1
        elif len(recent) >= 6:
            rate = sum(recent) / len(recent)
            if rate > 0.25:
                sigma = min(0.8, sigma * 1.25)
            elif rate < 0.12:
                sigma = max(sigma_floor, sigma * 0.85)

        rec = {"gen": gen, "players": players, "accepted": accepted,
               "sigma": round(sigma, 4), "secs": round(time.time() - t0, 1),
               "at": time.strftime("%F %T"), "mode": "league-pool",
               "subset": [e.label for e in entries],
               "pool": pool_snapshot,
               "ref_games": ref.games, "since_accept": since_accept,
               "tried": tried}
        if forced:
            rec["forced"] = forced
        if guard_hits:
            rec["weight_guard"] = {"mode": weight_guard, "hits": guard_hits}
        if accepted:
            rec.update({"edge": round(best[1], 4), "ci_low": round(best[2], 4),
                        "games": best[3], "moved": best[4], "op": best[5],
                        "per_opponent": best[6]})

        # ------------------------------------------------ periodic checks
        if full_check_every and gen % full_check_every == 0:
            log(f"[{players}p] gen {gen} FULL POOL CHECK "
                f"({check_games} games x {len(pool) - 1} opponents)")
            now = full_check(champion, pool, players, check_games, workers,
                             seed_base, log=log)
            regs, untested = compare_checks(now, st.get("last_full_check"),
                                            tested_since_check)
            fc = {"gen": gen, "players": players, "at": time.strftime("%F %T"),
                  "games": check_games, "results": now,
                  "tested_since_last": sorted(tested_since_check),
                  "regressions": regs, "untested_regressions": untested}
            append_jsonl(pp["fullcheck"], fc)
            rec["full_check"] = now
            rec["regressions"] = regs
            rec["untested_regressions"] = untested
            for r in untested:
                log(f"[{players}p] !! REGRESSION vs {r['opponent']} "
                    f"({r['tier']}), NOT tested on since last check: "
                    f"{r['was']:.1%} -> {r['now']:.1%} "
                    f"(delta {r['delta']:+.1%}, noise {r['noise']:.1%})")
            for r in regs:
                if r not in untested:
                    log(f"[{players}p] .. regression vs {r['opponent']}: "
                        f"{r['was']:.1%} -> {r['now']:.1%} (tested on)")
            st["last_full_check"] = now
            tested_since_check = set()

        if ablate_every and gen % ablate_every == 0:
            keys = [k for k in sorted(DEFAULT_WEIGHTS) if k not in FROZEN]
            cur = int(st.get("ablate_cursor", 0)) % max(1, len(keys))
            todo = [keys[(cur + i) % len(keys)] for i in range(ablate_k)]
            log(f"[{players}p] gen {gen} ABLATION cycle: {todo}")
            recs = ablate(champion, todo, pool, players, ablate_games,
                          workers, seed_base + 77, accept_z,
                          mode=ablate_mode, margin_scale=margin_scale, log=log)
            for r in recs:
                append_jsonl(pp["ablation"],
                             dict(r, gen=gen, players=players,
                                  at=time.strftime("%F %T")))
            credit = update_credit(pp["credit"], recs)
            st["ablate_cursor"] = (cur + ablate_k) % len(keys)
            rec["ablation"] = recs
            work = [k for k, v in credit.items() if v.get("last") == "load-bearing"]
            noise = [k for k, v in credit.items()
                     if v.get("last") == "no-measurable-effect"]
            log(f"[{players}p] weight credit so far: {len(work)} load-bearing, "
                f"{len(noise)} no measurable effect")

        # --------------------------------------------------- checkpoint
        append_jsonl(pp["gens"], rec)
        st.update({"gen": gen, "sigma": round(sigma, 4),
                   "since_accept": since_accept,
                   "tested_since_check": sorted(tested_since_check)})
        write_json(pp["state"], st)
        save_weights(pp["champion"], champion, gen=gen, sigma=sigma,
                     players=players, since_accept=since_accept)
        log(f"[{players}p] gen {gen} "
            + ("ACCEPT" if accepted else "reject")
            + f" sigma={sigma:.3f} {rec['secs']}s "
            + (f"edge={rec.get('edge'):+.4f} lo={rec.get('ci_low'):+.4f} "
               f"op={rec.get('op')}" if accepted
               else f"best_lo={max((t['lo'] for t in tried), default=0):+.4f}"))
        sys.stdout.flush()
    return champion


def report(state_dir, players, log=print):
    """Print the run's current standing: last full-pool check + weight credit.

    The generation log is a large JSONL and the interesting rows are buried in
    it, so this is what an operator actually wants to look at.
    """
    pp = paths(state_dir, players)
    st = load_state(pp)
    log(f"[{players}p] gen={st['gen']} sigma={st['sigma']} "
        f"since_accept={st['since_accept']} state={state_dir}")
    fc = st.get("last_full_check") or {}
    if fc:
        log(f"\n  full-pool check (champion vs every opponent):")
        log(f"    {'opponent':<28}{'tier':<11}{'win%':>8}{'+/-':>8}"
            f"{'margin':>9}{'null':>7}{'n':>6}")
        for label, r in sorted(fc.items(),
                               key=lambda kv: (-kv[1]["weight"], kv[0])):
            mg = r.get("margin")
            log(f"    {label:<28}{r['tier']:<11}{r['win_rate']:8.1%}"
                f"{r['ci']:8.1%}"
                + (f"{mg:+9.1f}" if mg is not None else f"{'--':>9}")
                + f"{r['null']:7.0%}{r['n']:6d}")
    else:
        log("  no full-pool check yet")
    if os.path.exists(pp["credit"]):
        with open(pp["credit"]) as fh:
            credit = json.load(fh)
        buckets = {}
        for k, v in credit.items():
            buckets.setdefault(v.get("last", "?"), []).append(
                (v.get("mean_edge", 0.0), k, v.get("n", 0)))
        log(f"\n  weight credit ({len(credit)} weights measured):")
        for verdict in ("load-bearing", "harmful", "no-measurable-effect"):
            rows = sorted(buckets.get(verdict, []))
            log(f"    {verdict} ({len(rows)}):")
            for edge, k, n in rows:
                log(f"      {k:<28} mean_edge={edge:+.4f} n={n}")
    else:
        log("\n  no ablation cycles yet")


def main(argv=None):
    ap = argparse.ArgumentParser(
        description="TtA weight hill climbing against a diverse opponent pool")
    ap.add_argument("--players", type=int, default=2, choices=(2, 3, 4))
    ap.add_argument("--hours", type=float, default=1.0)
    ap.add_argument("--workers", type=int, default=3)
    ap.add_argument("--lambda", dest="lam", type=int, default=2)
    ap.add_argument("--block", type=int, default=12,
                    help="games per opponent per evaluation block "
                         "(rounded down to a multiple of --players by arena "
                         "seat rotation, so use a multiple of 12)")
    ap.add_argument("--min-blocks", type=int, default=1)
    ap.add_argument("--max-blocks", type=int, default=4,
                    help="cap on evaluation blocks spent on one candidate")
    ap.add_argument("--subset", type=int, default=4,
                    help="opponents used for THIS generation's accept "
                         "decision (rotates; mirror + one gate always in)")
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--accept-z", type=float, default=1.2816,
                    help="z for the one-sided accept CI (1.2816=90%%)")
    ap.add_argument("--veto-z", type=float, default=1.0,
                    help="a gate opponent vetoes when edge + z*se < 0")
    ap.add_argument("--sigma-floor", type=float, default=0.08)
    ap.add_argument("--stall-kick", type=int, default=15)
    ap.add_argument("--state-dir", default=DEFAULT_STATE)
    ap.add_argument("--init", default="default",
                    help="'default' (DEFAULT_WEIGHTS) or a weights JSON path; "
                         "ignored once the state dir has a champion")
    ap.add_argument("--pool-weights", default="",
                    help="tier totals, e.g. 'book=4,variant=2.5,past=0.5,"
                         "floor=0.25'; 0 removes a tier")
    ap.add_argument("--hall-dir", action="append", default=[],
                    metavar="DIR",
                    help="directory of frozen champion weight files that join "
                         "the pool permanently and are never rotated out, "
                         "unlike the --past-k ladder.  Repeatable.")
    ap.add_argument("--past-k", type=int, default=2,
                    help="archived champions in the pool (spread oldest.."
                         "newest).  Default 2, not 3: a past:* duel is the "
                         "wall-clock hog at 3.5-4.4x a BookBot duel (21.9s vs "
                         "4.9s at 4p), ~30%% of a full check, and the tier is "
                         "a regression tripwire rather than a gradient.  "
                         "_spread keeps the ENDPOINTS, so k=2 is the founder "
                         "(the most different archived opponent) plus the "
                         "newest -- which is exactly the cycling test the "
                         "tier exists for")
    ap.add_argument("--gate-metric", choices=("margin", "winshare"),
                    default="margin",
                    help="how the gate tiers are scored.  'margin' (default) "
                         "uses the normalised culture-point differential; "
                         "'winshare' restores the old behaviour, which "
                         "returns edge=0.0000 against any opponent the "
                         "champion never beats")
    ap.add_argument("--margin-scale", type=float, default=P.MARGIN_SCALE,
                    help="culture points per unit of margin score "
                         "(default %(default)g, measured -- see "
                         "experiments/margin_calib.py)")
    ap.add_argument("--with-quiescent", action="store_true",
                    help="add the QuiescentBot search opponent (~1.2x cost)")
    ap.add_argument("--candidate-bot", default="weighted",
                    help="the architecture the trained weights are played by: "
                         "weighted (1-ply, default), quiescent[:levels=1,...] "
                         "or plan[:width=8,samples=1,det=1].  An evaluation "
                         "must be tuned with the search it runs under, so "
                         "this is what makes the run train the policy we ship "
                         "rather than the one we are replacing.  Applies to "
                         "the champion, the mirror and the past ladder; "
                         "external opponents are unaffected")
    ap.add_argument("--no-legacy-ladders", action="store_true",
                    help="ignore the pre-existing experiments/league_Np dirs")
    ap.add_argument("--full-check-every", type=int, default=10)
    ap.add_argument("--check-games", type=int, default=48)
    ap.add_argument("--ablate-every", type=int, default=25,
                    help="0 disables single-weight credit attribution")
    ap.add_argument("--ablate-k", type=int, default=3,
                    help="weights ablated per cycle (rotating cursor)")
    ap.add_argument("--ablate-games", type=int, default=24)
    ap.add_argument("--ablate-mode", choices=("zero", "default"), default="zero")
    ap.add_argument("--max-gens", type=int, default=0,
                    help="stop after N generations this process (0 = time only)")
    ap.add_argument("--weight-guard", choices=("clamp", "flag", "reject"),
                    default="clamp",
                    help="what to do when a value term whose default is "
                         "positive goes negative (a sign inversion: the 4p "
                         "champion's science=-6.09 made expensive cards look "
                         "like bargains).  Every occurrence is logged to "
                         "guard_Np.jsonl either way")
    ap.add_argument("--report", action="store_true",
                    help="print the run's standing (last full-pool check and "
                         "weight-credit ledger) and exit")
    args = ap.parse_args(argv)
    if args.report:
        return report(args.state_dir, args.players)
    tw = P.parse_tier_weights(args.pool_weights)
    run(players=args.players, hours=args.hours, workers=args.workers,
        lam=args.lam, block=args.block, min_blocks=args.min_blocks,
        max_blocks=args.max_blocks, subset=args.subset, seed=args.seed,
        accept_z=args.accept_z, veto_z=args.veto_z,
        sigma_floor=args.sigma_floor, stall_kick=args.stall_kick,
        state_dir=args.state_dir, tier_weights=tw, past_k=args.past_k,
        with_quiescent=args.with_quiescent, init=args.init,
        full_check_every=args.full_check_every, check_games=args.check_games,
        ablate_every=args.ablate_every, ablate_k=args.ablate_k,
        ablate_games=args.ablate_games, ablate_mode=args.ablate_mode,
        max_gens=args.max_gens, weight_guard=args.weight_guard,
        gate_metric=args.gate_metric, margin_scale=args.margin_scale,
        candidate_bot=parse_candidate_bot(args.candidate_bot),
        legacy_ladders=not args.no_legacy_ladders,
        hall_dirs=tuple(args.hall_dir))


if __name__ == "__main__":
    main()
