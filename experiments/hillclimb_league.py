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

from engine.bots.weighted import (DEFAULT_WEIGHTS, load_weights,  # noqa: E402
                                  save_weights)
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
# which is what keeps it correct as the vector grows: a term whose DEFAULT is
# strictly POSITIVE means "more of this is better", so a trained value below
# zero is a sign inversion, not a strategy.  Every legitimately negative term
# already has a negative default and is therefore untouched -- `rival_*`,
# the `*_late` phase multipliers, `discontent`, `uprising`, `pop_cost`,
# `wonder_remaining` and `end_turn_bias` included.
#
# NB `end_turn_bias` (-3.0) is one of those.  It looks like a bug and is NOT:
# removing it was measured twice, five ways, and makes the bot much weaker
# (see the comment on it in engine/bots/weighted.py, and docs/WASTED_ACTIONS.md
# section 6).  Nothing here should be read as licence to "fix" it.
NONNEG = frozenset(k for k, v in DEFAULT_WEIGHTS.items() if v > 0)

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
    for k in sorted(NONNEG):
        v = out.get(k, 0.0)
        if v < 0:
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


# ------------------------------------------------------------------ duels

def _shares(spec, opp_spec, players, games, seed0, workers):
    res = arena.duel(spec, opp_spec, players, games, seed0=seed0,
                     workers=workers)
    return res["per_game"], res


class RefCache:
    """The champion's per-game shares, computed once, reused by every mutant.

    Keyed on (opponent label, block index).  A mirror entry needs no games at
    all: champion-vs-a-table-of-itself is 1/players by construction.
    """

    def __init__(self, champion, players, workers, block, seed_base):
        self.champion = champion
        self.players = players
        self.workers = workers
        self.block = block
        self.seed_base = seed_base
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
            out = [1.0 / self.players] * self.block
        else:
            out, _ = _shares(self.champion, entry.spec, self.players,
                             self.block, self.seed_for(entry, b), self.workers)
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
                     "gate": e.tier in gate_tiers} for e in entries}
    games = 0
    blocks = 0
    for b in range(max_blocks):
        for e in entries:
            mine, _ = _shares(cand, e.resolve(cand, ref.champion),
                              ref.players, ref.block,
                              ref.seed_for(e, b), ref.workers)
            theirs = ref.get(e, b)
            games += ref.block
            d = per[e.label]
            for x, y in zip(mine, theirs):
                if x is None or y is None:
                    continue
                d["diffs"].append(x - y)
                d["mine"].append(x)
                d["theirs"].append(y)
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
        rec = {
            "tier": d["tier"], "weight": round(d["weight"], 4),
            "n": len(d["diffs"]), "edge": round(om, 4), "se": round(ose, 4),
            "win_rate": round(sum(d["mine"]) / len(d["mine"]), 4) if d["mine"] else None,
            "champ_rate": round(sum(d["theirs"]) / len(d["theirs"]), 4) if d["theirs"] else None,
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
        res = arena.duel(champion, e.spec, players, games,
                         seed0=(seed + label_seed(e.label)) % 10_000_019,
                         workers=workers)
        out[e.label] = {
            "tier": e.tier, "weight": round(e.weight, 4),
            "win_rate": round(res["win_rate"], 4), "ci": round(res["ci"], 4),
            "n": res["games"], "null": round(res["null"], 4),
        }
        log(f"    {e.label:<28} {res['win_rate']:6.1%} +/-{res['ci']:5.1%} "
            f"(null {res['null']:.0%}, n={res['games']}, tier={e.tier})")
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
           gate_only=True, max_opponents=3, mode="zero", log=print):
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
    ref = RefCache(champion, players, workers, games, seed)
    out = []
    for key in keys:
        w = dict(champion)
        w[key] = 0.0 if mode == "zero" else float(DEFAULT_WEIGHTS.get(key, 0.0))
        if w[key] == champion.get(key):
            continue                       # already at the ablated value
        per = {}
        for e in entries:
            mine, _ = _shares(w, e.spec, players, games, ref.seed_for(e, 0),
                              workers)
            theirs = ref.get(e, 0)
            diffs = [x - y for x, y in zip(mine, theirs)
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
            f"{'win%':>7}{'champ%':>8}{'edge':>9}")
    lines = [head]
    for label, r in rows:
        wr = "  --  " if r["win_rate"] is None else f"{r['win_rate']:6.1%}"
        cr = "  --  " if r["champ_rate"] is None else f"{r['champ_rate']:6.1%}"
        flag = " GATE" if r["gate"] else ""
        lines.append(f"      {label:<26}{r['tier']:<11}{r['weight']:5.2f} "
                     f"{r['n']:4d} {wr:>7}{cr:>8}{r['edge']:+9.4f}{flag}")
    return "\n".join(lines)


def run(players=2, hours=1.0, workers=3, lam=2, block=12, min_blocks=1,
        max_blocks=4, subset=4, seed=1, accept_z=1.2816, veto_z=1.0,
        sigma_floor=0.08, stall_kick=15, state_dir=DEFAULT_STATE,
        tier_weights=None, past_k=3, with_quiescent=False, init="default",
        full_check_every=10, check_games=48, ablate_every=25, ablate_k=3,
        ablate_games=24, ablate_mode="zero", max_gens=0, legacy_ladders=True,
        weight_guard="clamp", log=print):
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

    def build():
        return P.build_pool(players, ladder_dirs=ladders,
                            tier_weights=tier_weights, past_k=past_k,
                            with_quiescent=with_quiescent, log=log)

    pool = build()
    log(f"[{players}p] league trainer: {len(pool)} opponents, "
        f"gen={gen} sigma={sigma:.3f} state={state_dir}")

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
        ref = RefCache(champion, players, workers, block, seed_base)

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
                          mode=ablate_mode, log=log)
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
        log(f"    {'opponent':<28}{'tier':<11}{'win%':>8}{'+/-':>8}{'null':>7}{'n':>6}")
        for label, r in sorted(fc.items(),
                               key=lambda kv: (-kv[1]["weight"], kv[0])):
            log(f"    {label:<28}{r['tier']:<11}{r['win_rate']:8.1%}"
                f"{r['ci']:8.1%}{r['null']:7.0%}{r['n']:6d}")
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
    ap.add_argument("--past-k", type=int, default=3,
                    help="archived champions in the pool (spread oldest..newest)")
    ap.add_argument("--with-quiescent", action="store_true",
                    help="add the QuiescentBot search opponent (~1.2x cost)")
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
        legacy_ladders=not args.no_legacy_ladders)


if __name__ == "__main__":
    main()
