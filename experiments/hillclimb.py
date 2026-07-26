"""(1+lambda) evolution strategy over WeightedBot's weight dict.

    python3 -m experiments.hillclimb --players 4 --hours 8 --workers 3

Each generation:
  1. build `lambda` mutants of the champion (~25% of the weights perturbed by
     a gaussian step proportional to the weight's own magnitude, with an
     occasional 4x jump to escape local optima);
  2. screen each mutant against a table of champions over `--screen` games
     (seat-rotated); mutants below the null win rate are dropped immediately;
  3. play the survivor up to `--max-games` more games and accept it only if
     the lower bound of a one-sided 90% confidence interval is still above
     the null win rate (1/players) -- i.e. a clear, not a lucky, win;
  4. checkpoint.

Everything restart-safe: the champion is written atomically to
experiments/champion_{K}p.json after every generation and every generation
appends one line to experiments/generations_{K}p.jsonl, so a kill costs at
most the generation in flight.  Re-running the same command resumes.

Step size adapts with the 1/5th success rule, bounded to [0.05, 0.8].
"""
from __future__ import annotations

import argparse
import json
import math
import os
import random
import sys
import time

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine.bots.weighted import DEFAULT_WEIGHTS, save_weights  # noqa: E402
from experiments import arena  # noqa: E402
from experiments.summarize import GROUPS, group_of  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))

# weights that must not change sign or scale wildly: culture IS the score,
# it anchors the whole evaluation's units.
FROZEN = {"culture"}

# Coherent mutation groups.  `summarize.GROUPS` names the base features;
# here each group also owns its `_early` / `_late` phase copies, so a group
# move is "care more/less about this whole strategic axis", at every age,
# rather than a scatter of unrelated coefficients.
GROUP_KEYS = {}
for _k in DEFAULT_WEIGHTS:
    if _k in FROZEN:
        continue
    GROUP_KEYS.setdefault(group_of(_k).split("/")[0], []).append(_k)
GROUP_KEYS = {g: ks for g, ks in GROUP_KEYS.items() if ks}
GROUP_NAMES = sorted(GROUP_KEYS)


def champ_path(k):
    return os.path.join(HERE, f"champion_{k}p.json")


def gen_path(k):
    return os.path.join(HERE, f"generations_{k}p.jsonl")


def league_dir(k):
    return os.path.join(HERE, f"league_{k}p")


# --------------------------------------------------------------- checkpoint

def load_champion(k):
    path = champ_path(k)
    if os.path.exists(path):
        try:
            with open(path) as fh:
                d = json.load(fh)
            w = dict(DEFAULT_WEIGHTS)
            w.update(d["weights"])
            return (w, d.get("gen", 0), d.get("sigma", 0.25),
                    d.get("since_accept", 0))
        except Exception:
            pass
    return dict(DEFAULT_WEIGHTS), 0, 0.25, 0


def append_gen(k, rec):
    with open(gen_path(k), "a") as fh:
        fh.write(json.dumps(rec) + "\n")
        fh.flush()
        os.fsync(fh.fileno())


# -------------------------------------------------------------------- league
#
# A pure mirror ladder (mutant vs a table of its own parent) finds the best
# *response to the parent*, which is not the same thing as strong play: the
# 3p run visibly drifted into a policy that beat `default` while losing
# ground against `greedy`.  Keeping an archive of past champions and making
# every mutant prove itself against a mixed field of them -- plus the two
# fixed reference bots -- is the standard fix.

LEAGUE_KEEP = 8            # archived champions retained (plus the founder)


def load_league(k):
    d = league_dir(k)
    if not os.path.isdir(d):
        return []
    out = []
    for fn in sorted(os.listdir(d)):
        if not fn.endswith(".json"):
            continue
        try:
            with open(os.path.join(d, fn)) as fh:
                j = json.load(fh)
            w = dict(DEFAULT_WEIGHTS)
            w.update(j["weights"] if "weights" in j else j)
            out.append(w)
        except Exception:
            pass
    return out


def archive_champion(k, weights, gen):
    d = league_dir(k)
    os.makedirs(d, exist_ok=True)
    save_weights(os.path.join(d, f"gen{gen:05d}.json"), weights, gen=gen,
                 players=k)
    files = sorted(fn for fn in os.listdir(d) if fn.endswith(".json"))
    # Keep the founder (oldest -- the most diverse opponent we have) and the
    # most recent LEAGUE_KEEP-1; drop the middle.
    for fn in files[1:-LEAGUE_KEEP]:
        try:
            os.remove(os.path.join(d, fn))
        except OSError:
            pass


def build_field(champion, league, rng, mode, n_past=3):
    """The table a mutant has to beat.

    Returns either a single spec (classic mirror) or a list of specs from
    which `arena.duel` draws each defender seat.
    """
    if mode == "mirror" or not league:
        return champion
    picks = league if len(league) <= n_past else rng.sample(league, n_past)
    # ~4/9 parent, ~3/9 ancestors, ~2/9 fixed references.  The references
    # keep the ladder honest: a champion cannot win by being a specialised
    # counter to its own lineage if it also has to beat `greedy`.
    return [champion] * 4 + list(picks) + ["default", "greedy"]


# ----------------------------------------------------------------- mutation

def _clamp(x):
    return math.copysign(60.0, x) if abs(x) > 60 else x


def mutate(w, rng, sigma, frac=0.25, op=None):
    """Return (mutant, {key: [old, new]}, operator_name).

    Four operators.  `scatter` is the original random-subset move; the other
    three exist because the evaluation's weights are not independent -- the
    interesting moves are "value this whole strategic axis more" and "escape
    this basin", neither of which a 25% random scatter reaches often.
    """
    out = dict(w)
    keys = [k for k in out if k not in FROZEN]
    if op is None:
        r = rng.random()
        op = ("scatter" if r < 0.45 else
              "group" if r < 0.78 else
              "rescale" if r < 0.90 else "kick")
    moved = {}

    if op == "rescale":
        g = rng.choice(GROUP_NAMES)
        factor = math.exp(rng.gauss(0.0, max(0.20, sigma)))
        for k in GROUP_KEYS[g]:
            new = _clamp(out[k] * factor)
            moved[k] = [round(out[k], 4), round(new, 4)]
            out[k] = round(new, 5)
        return out, moved, f"rescale:{g}"

    scale = sigma
    if op == "scatter":
        picks = rng.sample(keys, max(1, int(round(len(keys) * frac))))
    elif op == "group":
        gs = rng.sample(GROUP_NAMES, 1 if rng.random() < 0.6 else 2)
        picks = [k for g in gs for k in GROUP_KEYS[g]]
        op = "group:" + "+".join(sorted(gs))
    else:                                    # kick -- deliberate big restart
        picks = rng.sample(keys, max(1, int(round(len(keys) * 0.6))))
        scale = sigma * 3.0

    for k in picks:
        s = scale * (4.0 if rng.random() < 0.10 else 1.0)
        new = _clamp(out[k] + rng.gauss(0.0, s) * (abs(out[k]) + 0.15))
        moved[k] = [round(out[k], 4), round(new, 4)]
        out[k] = round(new, 5)
    return out, moved, op


# --------------------------------------------------------------- evaluation

def combine(shares, null, z=1.2816):
    """Mean, standard error and the lower bound of a one-sided CI."""
    n = len(shares)
    if n < 2:
        return 0.0, 1.0, -1.0
    m = sum(shares) / n
    var = sum((x - m) ** 2 for x in shares) / (n - 1)
    se = math.sqrt(var / n)
    return m, se, m - z * se


def challenge(mutant, champion, field, players, screen, max_games, seed0,
              workers, min_games=0, accept_z=1.2816):
    """Two-stage sequential test of `mutant` against `field`.

    The statistic is always the mutant's *edge over the champion on identical
    games*: for every (seed, seat) the mutant plays, the champion plays the
    byte-identical game -- same seed, same opponents drawn from the same
    field -- and we accumulate the difference in win share.  The null is
    therefore exactly 0 regardless of how strong or weird the field is, and
    pairing removes the large game-to-game variance that seed luck
    contributes, so far fewer games are needed per decision.

    When `field` is the champion itself (mirror mode) the champion's own
    share is 1/players by construction -- both sides are the same
    deterministic policy -- so the reference games are skipped and the test
    degenerates to the original one against the 1/players null.

    Accept when the lower bound of a one-sided CI on the mean edge (default
    90%, `accept_z`) is still above 0 after at least `min_games` games;
    abandon as soon as the running edge is negative and the screening batch
    is spent.  `accept_z` is the *whole* strictness knob: at n paired samples
    the mutant has to beat its parent by `accept_z * se` win share, so a
    player count whose generations keep rejecting is either short of games or
    short of a smaller z, and both are now dials rather than edits.
    """
    mirror = not isinstance(field, list)
    cost = 1 if mirror else 2            # games spent per paired sample
    diffs, mut = [], []
    played = 0
    batch = max(players, screen)
    budget = max(cost * players, max_games)
    min_played = min(cost * (min_games or 2 * screen), budget)
    res = None
    while played < budget:
        a = arena.duel(mutant, field, players, batch,
                       seed0=seed0 + played * 131, workers=workers)
        if mirror:
            ref = [1.0 / players] * len(a["per_game"])
        else:
            b = arena.duel(champion, field, players, batch,
                           seed0=seed0 + played * 131, workers=workers)
            ref = b["per_game"]
        for x, y in zip(a["per_game"], ref):
            if x is not None and y is not None:
                diffs.append(x - y)
                mut.append(x)
        res = a
        played += batch * cost
        m, se, lo = combine(diffs, 0.0, accept_z)
        if lo > 0.0 and played >= min_played:      # a clear win, not a lucky one
            break
        if m < 0.0 and played >= cost * screen:    # stop paying for a loser
            break
        batch = max(players, min((budget - played) // cost, screen))
        if batch <= 0:
            break
    m, se, lo = combine(diffs, 0.0, accept_z)
    wr = (sum(mut) / len(mut)) if mut else 0.0
    return m, se, lo, len(diffs), res, wr


# --------------------------------------------------------------------- run

def run(players, hours, workers, lam, screen, max_games, seed, anchor_every,
        min_games=0, mode="league", log=print, stall_kick=15,
        accept_z=1.2816, sigma_floor=0.08):
    # `since_accept` is restored from the checkpoint on purpose.  The
    # supervisor restarts this process every hour; a counter that reset on
    # every restart could never reach `stall_kick` on a player count whose
    # generations are slow, so the anti-stagnation kick would silently never
    # fire on exactly the runs that need it most.
    champion, gen, sigma, since_accept = load_champion(players)
    rng = random.Random(seed * 7919 + players * 101 + gen)
    t_end = time.time() + hours * 3600
    recent = []
    if not os.path.exists(champ_path(players)):
        save_weights(champ_path(players), champion, gen=gen, sigma=sigma,
                     players=players, since_accept=since_accept)
    league = load_league(players)
    if mode == "league" and not league:
        # Seed the league so the very first generation already faces a mixed
        # field rather than a pure mirror.
        archive_champion(players, champion, gen)
        league = load_league(players)

    while time.time() < t_end:
        gen += 1
        t0 = time.time()
        best = None
        tried = []
        broken = 0
        field = build_field(champion, league, rng, mode)
        # A long rejection streak means the current sigma cannot reach
        # anything better from here.  Force a large restart-style jump
        # instead of grinding the same neighbourhood -- and re-open sigma in
        # the SAME generation, so the big jump is actually taken at a big step
        # size.  (These used to be an off-by-one apart: the kick fired on the
        # generation where `since_accept % stall_kick == 0` before the
        # increment, the sigma re-open on the one after it, so the kick was
        # always drawn at the annealed step size it was meant to escape.)
        forced = None
        if stall_kick and since_accept and since_accept % stall_kick == 0:
            forced = "kick"
            sigma = min(0.8, max(sigma, 0.25) * 2.0)
            hold_sigma = stall_kick // 3        # let the re-opened step breathe
        for j in range(lam):
            mutant, moved, op = mutate(champion, rng, sigma, op=forced)
            m, se, lo, n, res, wr = challenge(
                mutant, champion, field, players, screen, max_games,
                seed0=(gen * 1_000_003 + j * 7717 + seed) % 10_000_019,
                workers=workers, min_games=min_games, accept_z=accept_z)
            tried.append({"edge": round(m, 4), "lo": round(lo, 4), "n": n,
                          "wr": round(wr, 4), "moved": len(moved), "op": op})
            if n == 0 or (res and res.get("errors", 0) > res.get("games", 0)):
                broken += 1
                tried[-1]["engine_errors"] = (res or {}).get("errors", 0)
                tried[-1]["error_sample"] = (res or {}).get("error_sample", [])
            if lo > 0.0 and (best is None or lo > best[2]):
                best = (mutant, m, lo, n, moved, se, op)

        accepted = best is not None
        if accepted:
            champion = best[0]
            since_accept = 0
            archive_champion(players, champion, gen)
            league = load_league(players)
        else:
            since_accept += 1
        recent.append(accepted)
        recent = recent[-12:]
        # 1/5th success rule.  Held for a few generations after a stall kick:
        # the shrink is x0.85 per rejected generation, so an un-held sigma
        # decays from a 0.5 kick back to the floor inside one stall cycle and
        # the kick buys nothing.
        if hold_sigma > 0:
            hold_sigma -= 1
        elif len(recent) >= 6:
            rate = sum(recent) / len(recent)
            if rate > 0.25:
                sigma = min(0.8, sigma * 1.25)
            elif rate < 0.12:
                sigma = max(sigma_floor, sigma * 0.85)

        rec = {
            "gen": gen, "players": players, "accepted": accepted,
            "sigma": round(sigma, 4), "secs": round(time.time() - t0, 1),
            "tried": tried, "at": time.strftime("%F %T"),
            "mode": mode, "field": len(field) if isinstance(field, list) else 1,
        }
        if broken:
            rec["broken"] = broken
        if accepted:
            rec.update({"win_rate": round(best[1], 4),
                        "ci_low": round(best[2], 4),
                        "games": best[3], "moved": best[4], "op": best[6]})
        # periodic sanity check against fixed anchors the climb never trains
        # on directly -- the guard against chasing noise.
        if anchor_every and gen % anchor_every == 0:
            # Big enough to be worth reading: at 48 games the CI is +/-14%,
            # which cannot distinguish "the champion is drifting" from noise.
            n_anchor = max(96, 2 * screen)
            for tag, opp, s in (("default", "default", 13),
                                ("greedy", "greedy", 17),
                                ("random", "random", 23)):
                a = arena.duel(champion, opp, players, n_anchor,
                               seed0=gen * s, workers=workers)
                rec[f"vs_{tag}"] = round(a["win_rate"], 4)
                rec[f"vs_{tag}_ci"] = round(a["ci"], 4)
            rec["anchor_games"] = n_anchor
        append_gen(players, rec)
        save_weights(champ_path(players), champion, gen=gen, sigma=sigma,
                     players=players, since_accept=since_accept)
        log(f"[{players}p] gen {gen} "
            + ("ACCEPT" if accepted else "reject ")
            + f" sigma={sigma:.3f} {rec['secs']}s "
            + (f"edge={rec.get('win_rate')} lo={rec.get('ci_low')} "
               f"n={rec.get('games')} op={best[6]}"
               if accepted else str(tried))
            + (f" | vs_default={rec.get('vs_default')} "
               f"vs_greedy={rec.get('vs_greedy')} "
               f"vs_random={rec.get('vs_random')}" if "vs_default" in rec else ""))
        sys.stdout.flush()
        # The engine is edited under us by another process.  If a generation
        # produced no playable games at all, back off instead of burning the
        # budget (and the log) at hundreds of empty generations per minute.
        if broken == lam:
            log(f"[{players}p] no playable games this generation "
                f"-- engine likely mid-edit, sleeping 60s")
            sys.stdout.flush()
            time.sleep(60)
    return champion


def main(argv=None):
    ap = argparse.ArgumentParser(description="TtA weight hill climbing")
    ap.add_argument("--players", type=int, default=4, choices=(2, 3, 4))
    ap.add_argument("--hours", type=float, default=6.0)
    ap.add_argument("--workers", type=int, default=3)
    ap.add_argument("--lambda", dest="lam", type=int, default=2)
    ap.add_argument("--screen", type=int, default=48,
                    help="games in the first screening batch")
    ap.add_argument("--max-games", type=int, default=240,
                    help="cap on games spent on one mutant")
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--min-games", type=int, default=0,
                    help="games required before an accept (default 2*screen)")
    ap.add_argument("--anchor-every", type=int, default=10,
                    help="every N generations, measure vs default/greedy/random")
    ap.add_argument("--mode", choices=("league", "mirror"), default="league",
                    help="league: mutants face a mixed field of past champions "
                         "plus default/greedy, scored as a paired edge over the "
                         "champion on identical games.  mirror: the old "
                         "self-play-only ladder.")
    ap.add_argument("--stall-kick", type=int, default=15,
                    help="after N consecutive rejections force a large jump "
                         "and re-open sigma (0 disables)")
    ap.add_argument("--accept-z", type=float, default=1.2816,
                    help="z for the one-sided accept CI (1.2816=90%%, "
                         "0.8416=80%%); lower accepts more, faster, noisier")
    args = ap.parse_args(argv)
    run(args.players, args.hours, args.workers, args.lam, args.screen,
        args.max_games, args.seed, args.anchor_every, args.min_games,
        mode=args.mode, stall_kick=args.stall_kick,
        accept_z=args.accept_z)


if __name__ == "__main__":
    main()
