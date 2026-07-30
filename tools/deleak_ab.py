"""Paired-seed A/B: did closing the `end_turn` row leak make the champion WORSE?

`0bec288` closed the leak documented in docs/INFORMATION_AUDIT.md 6.1-6.2:
`row_urgency` / `row_bargain_forgone` were pricing card-row slots that an
`end_turn` trial had just dealt from the real civil deck.  The fix budgets both
terms against the row visible at the SEARCH ROOT, and `tools/leak_impact.py`
measured it working (within-decision eval sd across determinizations 0.333 ->
0.005, move flips 11/2302 -> 0/2267 at 3p).

That is a correctness measurement, not a strength measurement, and the two are
different questions.  **The champions were hill-climbed WITH the leak**: every
accept decision that raised `row_bargain_forgone` (1.65 at 3p gen 1160) was
made by a bot whose row terms could see the future.  Removing the leak may
therefore make them play worse until the league re-adapts, and nobody had
measured it.  If it IS worse, the 3p arm's fitted row weights were partly
exploiting the leak and that bears on whether to roll that arm back.

The fix stays in either way -- playing on information the bot cannot legally
have is not an option regardless of what it scores -- so this tool exists to
report an honest number, not a favourable one.

HOW THE LEAKY ARM IS RECONSTRUCTED, and why it is exact
-------------------------------------------------------
`0bec288` touched three files.  `plan.py` and `quiescent.py` got *nothing but*
parameter threading (`_quiesce(..., root_row=ctx.get("root_row"))` and four
`rival_context(st, d, root_ctx.get("root_row"))` rebuilds -- verified by diff:
every changed line is either a new keyword argument or a docstring).  The whole
behavioural change lives in two places in `weighted.py`:

    rival_context: "root_row": root_row_budget(state) if root_row is None
                               else root_row
    row_pressure:  budget = ctx.get("root_row") if ctx else None
                   budget = dict(budget) if budget else None
                   ... if budget is not None: skip slots with no budget left

So making `root_row_budget` return `None` puts `ctx["root_row"] = None` at the
root, `dict(budget) if budget else None` turns that into `budget = None`, and
`row_pressure` prices every slot on the post-move row -- byte-for-byte the
pre-fix code path.  A threaded `None` recomputes to `None` at every mid-search
rebuild, so the deep nodes are leaky too, exactly as they were.  No engine file
is edited and no gate digest can move.

That argument is a *claim about code*, so `--verify-prefix DIR` checks it
against the real thing.  Point it at a checkout of the pre-fix commit
(`git archive 0bec288^ | tar -x -C /tmp/prefix`) and it plays the same seeds
three ways -- genuine pre-fix tree, this tool's leaky arm, de-leaked master --
against BookBot, which is the right opponent because `engine/bots/book.py` never
touches `row_pressure`, so the *defender* is identical in all three trees and
any difference is the challenger's.  Pass requires the first two to be
IDENTICAL per game and the third to differ.  Measured 2026-07-29, 24 games each,
both live architectures -- and note it must be run per architecture, because the
`plan.py` threading is a different code path from the `quiescent.py` one and is
where `0bec288` shipped its `NameError` (INFORMATION_AUDIT 6.3):

    3p, quiescent:levels=1   pre-fix == leaky 24/24   de-leaked differs 10/24
    2p, plan:width=2         pre-fix == leaky 24/24   de-leaked differs  2/24

WHY THE PATCH IS SCOPED TO ONE BOT
----------------------------------
The leaky arm wraps ONLY the challenger, and disables the budget only for the
duration of that bot's own `__call__`.  Patching the module globally would
de-leak the defenders too -- and the mirror/past/hall tiers are the same policy
family reading the same row weights, so a global patch measures "everyone lost
the leak simultaneously", under which a win share can stay flat while every bot
at the table gets worse.  Scoped, the defenders are byte-identical between the
two arms by construction and the only difference in the whole experiment is
whether the challenger can read cards it was not dealt yet.

THE STATISTIC IS PAIRED, AND THAT IS THE WHOLE POINT
----------------------------------------------------
Both arms play the same `seed0`, so game g of the leaky arm and game g of the
de-leaked arm are the same deal, the same seat rotation and the same opponent
draw.  Most pairs come out *identical* -- the leak only flips a move on ~0.5%
of decisions -- so the per-seed difference is exactly 0 on those and the
variance of the paired mean is driven only by the pairs that actually diverged.
Two independent means would throw that away and need an order of magnitude more
games for the same error bar.  Every number this tool prints is therefore
`mean(de-leaked - leaky)` over per-seed differences, with the SE of THAT
difference, plus the minimum detectable effect `1.96 * SE` so an underpowered
run reads as underpowered instead of as a finding.

`diverged` is the honesty check on the whole run: if it is 0 the two arms are
the same policy on these seeds and every effect size is trivially 0 with no
information in it.  A run with a small `diverged` count is reporting on a
handful of games no matter how many were played, and the tool says so.

Usage:

    # snapshot the LIVE champion first -- the arms rewrite it every generation,
    # and experiments/champion_3p.json (no league_state/) is a STALE gen-152
    # export with NO row weights, which would measure a fake zero
    cp experiments/league_state/champion_3p.json /tmp/champ_3p_snapshot.json

    python3 tools/deleak_ab.py --players 3 --champion /tmp/champ_3p_snapshot.json \
        --games 120 --workers 2 --out /tmp/deleak_3p.json
    python3 tools/deleak_ab.py --report /tmp/deleak_3p.json
"""
import argparse
import json
import math
import os
import subprocess
import sys
import tempfile
import time

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine.bots import weighted as W  # noqa: E402
from experiments import arena  # noqa: E402
from experiments import hillclimb_league as L  # noqa: E402
from experiments import hillclimb_pool as P  # noqa: E402

#: The default opponent set.  `mirror` is the champion itself, which is the
#: highest-powered row available: the opponent reads the same row weights, so
#: any strength the leak was buying shows up as a swing against a policy that
#: no longer has it.  `book`/`book2` are the external yardstick the pool uses
#: as its sanity floor and veto (hillclimb_pool.DEFAULT_TIER_WEIGHTS), i.e. the
#: opponents an operator actually reads a win rate against.
DEFAULT_OPPONENTS = ("mirror", "book", "book2")


def _no_budget(_state):
    """The pre-`0bec288` root row budget: there wasn't one."""
    return None


class LeakyBot:
    """Delegate to `inner`, with the root-row budget disabled while it thinks.

    The swap is per-decision rather than per-process so that only this bot's
    own search is leaky; the defenders in the same worker keep the fix.  Games
    are single-threaded inside a worker, so the window cannot overlap another
    bot's turn.  `finally` restores even on an engine exception, which
    `arena._play` catches and reports as a dead game.

    `placebo=True` performs the identical save/patch/restore dance with the
    REAL `root_row_budget`, which is the negative control: it exercises every
    line of this wrapper and the whole `("leaky", ...)` make_bot path without
    changing a single evaluation, so a placebo run that reports any divergence
    at all is a broken harness rather than a measurement of the leak.
    """

    name = "leaky"

    def __init__(self, inner, placebo=False):
        self.inner = inner
        self.swap = W.root_row_budget if placebo else _no_budget

    def __call__(self, state):
        saved = W.root_row_budget
        W.root_row_budget = self.swap
        try:
            return self.inner(state)
        finally:
            W.root_row_budget = saved


def install_leaky_make_bot(placebo=False):
    """Teach `arena.make_bot` the ``("leaky", inner_spec)`` wrapper spec.

    Chained over whatever is already installed -- importing
    `hillclimb_pool` replaces `arena.make_bot` with its own to add the `book`
    and `variant` kinds, so this must wrap that, not `arena`'s original.
    Forked workers inherit the patched global, so nothing is edited on disk.
    """
    base = arena.make_bot

    def make_bot(spec, seed):
        if isinstance(spec, tuple) and spec and spec[0] == "leaky":
            return LeakyBot(base(spec[1], seed), placebo=placebo)
        return base(spec, seed)

    arena.make_bot = make_bot
    return base


# ------------------------------------------------- is the leaky arm really leaky?

_XCHECK = r'''
import json, os, sys
sys.path.insert(0, os.getcwd())
from experiments import arena
from experiments import hillclimb_league as L
from experiments import hillclimb_pool as P  # noqa: F401  installs make_bot
champ_path, arch, players, seed0, games, out, mode = sys.argv[1:8]
players, seed0, games = int(players), int(seed0), int(games)
d = json.load(open(champ_path))
L.CANDIDATE_ARCH = L.parse_candidate_bot(arch)
spec = L.as_spec(d.get("weights", d))
if mode == "leaky":
    sys.path.insert(0, %(tools)r)
    import deleak_ab
    deleak_ab.install_leaky_make_bot()
    spec = ("leaky", spec)
res = arena.duel(spec, "book", players, games, seed0=seed0, workers=2)
json.dump({"culture": res["per_game_culture"], "err": res["errors"]},
          open(out, "w"))
'''


def verify_prefix(prefix_dir, champion_path, arch, players, games, seed0):
    """Play the same BookBot seeds in the pre-fix tree, leaky, and de-leaked.

    The whole tool rests on "disabling `root_row_budget` reproduces the code as
    it was before `0bec288`".  This checks it empirically instead of by reading
    the diff: three subprocesses, three trees, one seed set.  BookBot is the
    opponent because it never reads `row_pressure`, so the defender is
    identical in all three and every difference belongs to the challenger.
    """
    here = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    src = _XCHECK % {"tools": os.path.join(here, "tools")}
    script = os.path.join(tempfile.mkdtemp(), "xcheck.py")
    with open(script, "w") as fh:
        fh.write(src)
    runs = (("pre-fix tree", prefix_dir, "asis"),
            ("leaky arm", here, "leaky"),
            ("de-leaked master", here, "asis"))
    got = []
    for label, cwd, mode in runs:
        out = script + f".{len(got)}.json"
        cmd = [sys.executable, script, os.path.abspath(champion_path), arch,
               str(players), str(seed0), str(games), out, mode]
        subprocess.run(cmd, cwd=cwd, check=True)
        with open(out) as fh:
            d = json.load(fh)
        if d["err"]:
            raise SystemExit(f"{label}: {d['err']} games died; cannot compare")
        print(f"  {label:<18} {cwd}  mean own culture "
              f"{_mean(d['culture']):.4f}")
        got.append(d["culture"])
    same = sum(1 for a, b in zip(got[0], got[1]) if a == b)
    diff = sum(1 for a, b in zip(got[0], got[2]) if a != b)
    n = len(got[0])
    print(f"# pre-fix tree == leaky arm:        {same}/{n} games identical "
          f"({'PASS' if same == n else 'FAIL'})")
    print(f"# pre-fix tree vs de-leaked master: {diff}/{n} games differ "
          f"({'PASS' if diff else 'FAIL -- nothing to measure'})")
    if same != n:
        raise SystemExit("FAIL: the leaky arm is NOT the pre-fix code path. "
                         "Every A/B number this tool prints is invalid.")
    if not diff:
        raise SystemExit("FAIL: the fix changes nothing on these seeds, so "
                         "this champion/opponent pair cannot measure it.")
    print("# OK: the leaky arm is the pre-fix code path, and the fix is live.")


# ------------------------------------------------------------------ measuring

def load_champion(path):
    with open(path) as fh:
        d = json.load(fh)
    w = d.get("weights", d)
    return w, {"path": os.path.abspath(path), "gen": d.get("gen"),
               "players": d.get("players"), "n_keys": len(w),
               "row_urgency": w.get("row_urgency"),
               "row_bargain_forgone": w.get("row_bargain_forgone"),
               "hand_mil_value": w.get("hand_mil_value")}


def resolve_opponents(players, labels, champ_spec, log=print):
    """Pool label -> arena spec, with `mirror` resolved to the champion.

    Built from the real `hillclimb_pool.build_pool` so the specs are exactly
    the ones the league trains against, including the architecture wrapping:
    the mirror is the champion under the SAME searcher, because a mirror played
    1-ply would measure an architecture gap instead of the leak.
    """
    pool = P.build_pool(players, ladder_dirs=(), past_k=0, log=lambda *_a: None)
    by_label = {e.label: e for e in pool.sorted_entries()}
    out = []
    for lab in labels:
        if lab not in by_label:
            raise SystemExit(f"--opponents: {lab!r} is not a pool label; "
                             f"have {sorted(by_label)}")
        e = by_label[lab]
        # `hillclimb_league._series` wraps BOTH sides in `as_spec`, so a pool
        # entry that is a weight vector plays under the candidate architecture
        # too.  Doing the same here is what keeps "the opponents the league
        # trains against" true; `as_spec` is the identity on the `book`/
        # `variant`/`human` specs, which are not dicts.
        spec = champ_spec if e.is_mirror else L.as_spec(e.spec)
        out.append((lab, e.tier, spec))
    log("# opponents: " + ", ".join(f"{lab}[{tier}]" for lab, tier, _ in out))
    return out


def run(players, champion_path, games, workers, opponents, arch, seed_base,
        out_path, block=0, placebo=False):
    champ, meta = load_champion(champion_path)
    L.CANDIDATE_ARCH = L.parse_candidate_bot(arch)
    champ_spec = L.as_spec(champ)
    install_leaky_make_bot(placebo=placebo)

    # A complete seat rotation per base seed, always.  `arena.duel` assigns
    # `seat = g % players`, so a partial rotation leaves the challenger
    # over-represented in one seat -- and at the mirror that also destroys the
    # exact-zero reference (identical policies in every seat sum to 1 over a
    # full rotation, so the de-leaked mirror margin is 0.000 by construction
    # and every bit of the paired variance comes from the leaky arm).
    if games % players:
        raise SystemExit(f"--games must be a multiple of --players "
                         f"({games} % {players} != 0): a partial seat "
                         f"rotation is not a fair duel")
    block = block or games
    if block % players:
        raise SystemExit(f"--block must be a multiple of --players")

    print(f"# champion {meta['path']}")
    print(f"#   gen={meta['gen']} keys={meta['n_keys']} "
          f"row_urgency={meta['row_urgency']} "
          f"row_bargain_forgone={meta['row_bargain_forgone']} "
          f"hand_mil_value={meta['hand_mil_value']}")
    print(f"# architecture: {arch} -> {L.CANDIDATE_ARCH}")
    if placebo:
        print("# PLACEBO RUN: the 'leaky' arm keeps the real root-row budget. "
              "Anything other than 0 divergence here is a harness bug.")
    if not meta["row_urgency"] and not meta["row_bargain_forgone"]:
        print("# WARNING: this champion has NO row weights. The leak's effect "
              "scales with them, so this run can only measure a zero.")

    entries = resolve_opponents(players, opponents, champ_spec)
    rec = {"players": players, "games": games, "arch": arch,
           "seed_base": seed_base, "champion": meta, "placebo": placebo,
           "generated": time.strftime("%Y-%m-%dT%H:%M:%S"),
           "series": {}}
    arms = (("leaky", ("leaky", champ_spec)), ("fixed", champ_spec))
    for lab, tier, opp in entries:
        seed0 = (seed_base + L.label_seed(lab) * 17) % 10_000_019
        # Printed because `--seed-base` is NOT a run identifier: `arena.duel`
        # uses `seed0 + g // players`, so bumping --seed-base by 1 shifts the
        # deals by ONE BASE SEED and a "replication" at seed_base+1 replays
        # (games/players - 1) of the same deals.  Measured the hard way: two
        # 450-game 3p mirror runs one apart returned the same 175/450
        # divergence count and the same effect to 3 decimals.  A genuinely
        # independent replication needs --seed-base moved by at least
        # games/players.
        print(f"# {lab}: base seeds {seed0}..{seed0 + games // players - 1} "
              f"({games // players} deals x {players} seats)")
        row = {"tier": tier, "seed0": seed0, "errors": {}}
        for arm, _ in arms:
            row[arm] = {f: [] for f in FIELDS}
            row["errors"][arm] = 0
        rec["series"][lab] = row
        # Blocked so a long run checkpoints to disk every few minutes rather
        # than only at the end.  `duel` derives its base seed as
        # `seed0 + g // players`, so advancing seed0 by block/players gives the
        # next disjoint set of deals -- and both arms of a block share it, which
        # is what the pairing rests on.
        for done in range(0, games, block):
            k = min(block, games - done)
            s0 = seed0 + done // players
            for arm, spec in arms:
                t0 = time.time()
                res = arena.duel(spec, opp, players, k, seed0=s0,
                                 workers=workers)
                row[arm]["win"] += res["per_game"]
                row[arm]["margin"] += res["per_game_margin"]
                row[arm]["culture"] += res["per_game_culture"]
                row["errors"][arm] += res["errors"]
                print(f"  {lab:<10} {arm:<5} +{k:<4} tot={done + k:<5} "
                       f"win={res['win_rate']:.4f} "
                       f"margin={res['culture_a'] - res['culture_b']:+7.2f} "
                       f"own={res['culture_a']:6.2f} "
                       f"err={res['errors']} ({time.time() - t0:.0f}s)",
                      flush=True)
            if out_path:
                with open(out_path, "w") as fh:
                    json.dump(rec, fh)
    if out_path:
        print(f"# wrote {out_path}")
    return rec


# ------------------------------------------------------------------ reporting

def _paired(a, b):
    """Per-seed `b - a`, dropping any pair where either game died.

    Dropping the pair rather than the game is what keeps it paired: an
    unfinished game on one arm makes that seed uninformative on both.
    """
    return [float(y) - float(x) for x, y in zip(a, b)
            if x is not None and y is not None]


def _stats(d):
    n = len(d)
    if n == 0:
        return {"n": 0, "mean": 0.0, "se": float("nan"), "half": float("nan"),
                "nonzero": 0}
    mean = sum(d) / n
    if n > 1:
        var = sum((x - mean) ** 2 for x in d) / (n - 1)
        se = math.sqrt(var / n)
    else:
        se = float("nan")
    return {"n": n, "mean": mean, "se": se, "half": 1.96 * se,
            "nonzero": sum(1 for x in d if x != 0.0)}


def _merge(parts):
    """Equal-weight mean over opponents, with the SE of that mean.

    Each opponent is played on its own seeds, so the per-opponent paired means
    are independent and the SEs add in quadrature.  Equal weight, NOT the pool
    tier weights: the tier weights exist to shape a hill-climb gradient, and
    re-using them here would quietly turn "how much strength did the fix cost"
    into "how much gate score did it cost", which is a different question and
    one this tool does not have the mirror/past/hall coverage to answer.
    """
    live = [p for p in parts if p["n"] > 0]
    if not live:
        return {"n": 0, "mean": 0.0, "se": float("nan"), "half": float("nan"),
                "nonzero": 0}
    k = len(live)
    mean = sum(p["mean"] for p in live) / k
    se = math.sqrt(sum((p["se"] / k) ** 2 for p in live))
    return {"n": sum(p["n"] for p in live), "mean": mean, "se": se,
            "half": 1.96 * se, "nonzero": sum(p["nonzero"] for p in live)}


FIELDS = ("win", "margin", "culture")


def merge_recs(recs):
    """Several `--out` files into one report.

    n is chosen PER OPPONENT (a mirror duel at 3p costs 4x a BookBot duel and
    diverges 3x as often, so spending equal games on both wastes most of the
    budget), which means one run per opponent group and one merged report.
    Every statistic here is already per-opponent, so merging is a dict update;
    the only shared field that must agree is the champion and the architecture,
    and mixing those would be comparing different policies, so it raises.

    The SAME opponent may appear twice -- a replication on a fresh `--seed-base`
    -- and the two series are then concatenated, which is valid because disjoint
    deals give independent paired differences.  Disjointness is CHECKED, on the
    base-seed ranges rather than on `--seed-base`: `arena.duel` deals from
    `seed0 + g // players`, so two runs one apart share all but one of their
    deals and concatenating them would double-count the same games as if they
    were new evidence.  That is not hypothetical; it happened (INFORMATION_AUDIT
    6.4, "a trap worth recording").
    """
    out = dict(recs[0])
    out["series"] = {}
    seen = {}
    for r in recs:
        for key in ("players", "arch"):
            if r[key] != out[key]:
                raise SystemExit(f"cannot merge: {key} differs "
                                 f"({r[key]} vs {out[key]})")
        if r["champion"]["gen"] != out["champion"]["gen"]:
            raise SystemExit("cannot merge: different champion generations "
                             f"({r['champion']['gen']} vs "
                             f"{out['champion']['gen']})")
        if r.get("placebo") != out.get("placebo"):
            raise SystemExit("cannot merge: a placebo run and a real run")
        for lab, row in r["series"].items():
            lo = row["seed0"]
            hi = lo + len(row["fixed"]["win"]) // r["players"] - 1
            for plo, phi in seen.get(lab, ()):
                if lo <= phi and plo <= hi:
                    raise SystemExit(
                        f"cannot merge: two {lab!r} runs share deals -- base "
                        f"seeds {lo}..{hi} overlap {plo}..{phi}. Move "
                        f"--seed-base by at least games/players.")
            seen.setdefault(lab, []).append((lo, hi))
            if lab not in out["series"]:
                out["series"][lab] = row
                continue
            dst = out["series"][lab]
            for arm in ("leaky", "fixed"):
                for f in FIELDS:
                    dst[arm][f] = dst[arm][f] + row[arm][f]
                dst["errors"][arm] += row["errors"][arm]
    out["games"] = None
    return out


def _mean(xs):
    live = [x for x in xs if x is not None]
    return (sum(live) / len(live)) if live else float("nan")


def _defenders(arm):
    """Per-game MEAN DEFENDER culture, derived as ``own - margin``.

    `arena.duel` defines `per_game_margin` as A's culture minus the mean of the
    defenders', so this recovers the defenders' side exactly.  It is worth a
    column because "my bot scored less" and "the whole table scored less" are
    completely different findings and `d own culture` alone cannot tell them
    apart: total culture in a game of Through the Ages is not conserved (it
    depends on game length and on how much production everyone built), so a
    change that moves every seat by the same amount is a change in the SHAPE OF
    THE GAME, not a competitive loss.  The competitive quantity is the margin.
    """
    return [None if (c is None or m is None) else c - m
            for c, m in zip(arm["culture"], arm["margin"])]


def report(rec):
    games = rec.get("games")
    print(f"# {rec['players']}p  "
          f"n={games if games else 'per-opponent (see table)'}  "
          f"arch={rec['arch']}  champion gen={rec['champion']['gen']} "
          f"(row_bargain_forgone={rec['champion']['row_bargain_forgone']})")
    if rec.get("placebo"):
        print("# PLACEBO: the 'leaky' arm was never made leaky. Every number "
              "below MUST be exactly 0; anything else is a harness bug.")
    print("# LEVELS: the two arms' own absolute scores, for context.")
    print(f"# {'opponent':<10} {'n':>5} {'err':>4}  "
          f"{'leaky win':>10} {'fixed win':>10}  "
          f"{'leaky marg':>11} {'fixed marg':>11}")
    for lab, row in rec["series"].items():
        err = sum((row.get("errors") or {}).values())
        print(f"  {lab:<10} {len(row['fixed']['win']):>5} {err:>4}  "
              f"{_mean(row['leaky']['win']):>10.4f} "
              f"{_mean(row['fixed']['win']):>10.4f}  "
              f"{_mean(row['leaky']['margin']):>11.2f} "
              f"{_mean(row['fixed']['margin']):>11.2f}")
    print("#")
    print("# PAIRED EFFECT: de-leaked MINUS leaky on the SAME seed, mean of "
          "per-seed differences,")
    print("#   +/- 1.96*SE of that difference.  NEGATIVE = the fix cost "
          "strength.  z = mean/SE.")
    print("#   'd DEFENDER culture' is own-minus-margin: if it moves WITH "
          "'d own culture' the whole")
    print("#   table changed, which is a different finding from the challenger "
          "getting worse.")
    print(f"# {'opponent':<10} {'n':>5} {'diverged':>8}  "
          f"{'d win share':>21} {'z':>6}  {'d culture margin':>21} {'z':>6}  "
          f"{'d own culture':>21} {'z':>6}  {'d DEFENDER culture':>21} {'z':>6}")
    agg = {f: [] for f in FIELDS + ("defenders",)}
    total_div = total_n = 0
    for lab, row in rec["series"].items():
        cells, n = [], 0
        diffs = {}
        for f in FIELDS + ("defenders",):
            if f == "defenders":
                diffs[f] = _paired(_defenders(row["leaky"]),
                                   _defenders(row["fixed"]))
            else:
                diffs[f] = _paired(row["leaky"][f], row["fixed"][f])
            s = _stats(diffs[f])
            agg[f].append(s)
            n = max(n, s["n"])
            cell = (f"{s['mean']:+9.4f} +/-{s['half']:8.4f}"
                    if s["n"] else f"{'n/a':>21}")
            z = (s["mean"] / s["se"]) if s["n"] > 1 and s["se"] else 0.0
            cell += f" {z:>+6.2f}"
            cells.append(cell)
        # A pair "diverged" if the two arms produced a different GAME, which
        # `win` alone cannot see: the same player can win by a different
        # margin.  `culture`/`margin` catch every trajectory change.
        div = sum(1 for g in zip(*(diffs[f] for f in FIELDS if diffs[f]))
                  if any(x != 0.0 for x in g))
        total_div += div
        total_n += n
        print(f"  {lab:<10} {n:>5} {div:>8}  " + "  ".join(cells))
    pooled = {f: _merge(agg[f]) for f in FIELDS + ("defenders",)}
    cells = []
    for f in FIELDS + ("defenders",):
        cell = f"{pooled[f]['mean']:+9.4f} +/-{pooled[f]['half']:8.4f}"
        z = (pooled[f]["mean"] / pooled[f]["se"]) if pooled[f]["se"] else 0.
        cell += f" {z:>+6.2f}"
        cells.append(cell)
    print(f"  {'POOLED':<10} {total_n:>5} {total_div:>8}  " + "  ".join(cells))
    mw, mm = pooled["win"], pooled["margin"]
    print(f"# 95% CI half-width at this n: win share "
          f"+/-{100 * mw['half']:.2f} percentage points, culture margin "
          f"+/-{mm['half']:.2f} culture points.")
    # 1.96*SE is only the 50%-POWER detection threshold: an effect exactly that
    # big is significant half the time.  (z_{0.975} + z_{0.80}) * SE = 2.802*SE
    # is the effect this n would actually catch 80% of the time, and it is the
    # number to quote when the answer is "no difference", because quoting the
    # CI alone overstates how small an effect has been ruled out.
    print(f"# MINIMUM DETECTABLE EFFECT, 80% power, alpha=0.05 two-sided: "
          f"win share {100 * 2.802 * mw['se']:.2f} percentage points, "
          f"culture margin {2.802 * mm['se']:.2f} culture points.")
    print(f"#   A true effect smaller than that is INVISIBLE at this n; the "
          f"point estimate is not evidence for or against it.")
    print(f"# pairs that played a DIFFERENT game: {total_div} of {total_n} "
          f"({100.0 * total_div / max(1, total_n):.1f}%)")
    if total_div == 0 and not rec.get("placebo"):
        print("# NOTE: nothing diverged. The two arms are the same policy on "
              "these seeds; the zeros above carry no information about "
              "strength.")
    elif total_div < 10 and not rec.get("placebo"):
        print(f"# NOTE: only {total_div} pairs diverged, so the effect size "
              f"rests on that many games however large n is. Underpowered.")


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--players", type=int, default=3)
    ap.add_argument("--champion", help="path to a champion json SNAPSHOT "
                    "(copy it out of experiments/league_state first -- the "
                    "live file is rewritten every generation)")
    ap.add_argument("--games", type=int, default=120)
    ap.add_argument("--workers", type=int, default=2)
    ap.add_argument("--opponents", default=",".join(DEFAULT_OPPONENTS))
    ap.add_argument("--arch", default="quiescent:levels=1",
                    help="candidate architecture, as --candidate-bot; must "
                         "match the arm that FITTED the weights")
    ap.add_argument("--seed-base", type=int, default=20260729)
    ap.add_argument("--block", type=int, default=0,
                    help="checkpoint --out every this many games per arm "
                         "(default: only at the end). Must be a multiple of "
                         "--players.")
    ap.add_argument("--verify-prefix", metavar="DIR",
                    help="a checkout of the pre-fix commit "
                         "(git archive 0bec288^ | tar -x -C DIR): check that "
                         "the leaky arm reproduces it exactly, then exit")
    ap.add_argument("--verify-games", type=int, default=24)
    ap.add_argument("--placebo", action="store_true",
                    help="negative control: run the whole ('leaky', ...) path "
                         "with the REAL root-row budget. Divergence must be 0.")
    ap.add_argument("--out")
    ap.add_argument("--report", nargs="+", metavar="JSON",
                    help="re-print existing --out json files, merged")
    a = ap.parse_args(argv)

    if a.report:
        recs = []
        for path in a.report:
            with open(path) as fh:
                recs.append(json.load(fh))
        report(merge_recs(recs))
        return
    if not a.champion:
        ap.error("--champion is required (or --report)")
    if a.verify_prefix:
        verify_prefix(a.verify_prefix, a.champion, a.arch, a.players,
                      a.verify_games, a.seed_base)
        return
    rec = run(a.players, a.champion, a.games, a.workers,
              [s for s in a.opponents.split(",") if s], a.arch, a.seed_base,
              a.out, block=a.block, placebo=a.placebo)
    report(rec)


if __name__ == "__main__":
    main()
