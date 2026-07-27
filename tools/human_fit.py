"""Measure a bot on the human-corpus axes, and FIT a bot's knobs to a target.

    # just measure
    python3 tools/human_fit.py measure --spec book --players 2 --games 30
    python3 tools/human_fit.py measure --spec human:builder --players 2 --games 40

    # fit an archetype's knobs (coordinate descent on the corpus loss)
    python3 tools/human_fit.py fit --arch builder --players 2 --games 24 --rounds 3

Why this file exists
--------------------
`tools/bgo_botmatch.py` already emits bot behaviour in the *same TSV schema*
as the human parse, so "how human is this bot" is a distance between two rows
of the table `docs/HUMAN_BASELINE.md` prints.  That makes it an objective
function, and this is the optimiser for it.

Two things it is careful about:

* **Noise.**  A 24-game measurement has a real standard error on every axis
  (the war rate especially: sd ~1 war/game means +-0.2 at n=24).  The fitter
  therefore re-measures the incumbent on the SAME seed block as each
  challenger, so a knob change is judged on paired games rather than against a
  remembered number from a luckier block.  It also only accepts an improvement
  that clears a margin, so it cannot ratchet on noise.
* **Scale.**  Axes are in wildly different units (score ~160, wars ~0.25), so
  every residual is divided by the axis's own human standard deviation, i.e.
  the loss is in units of "human sigmas off".  An axis weight above 1 is
  reserved for the axes that DEFINE an archetype.
"""
from __future__ import annotations

import argparse
import json
import math
import os
import sys
import tempfile

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from experiments import hillclimb_pool                      # noqa: E402,F401
from tools import bgo_botmatch, bgo_stats                    # noqa: E402

#: axis -> (human sd at 2p, default weight).  The sd values are the per-player
#: standard deviations measured over the 1,383 2p corpus rows; they are the
#: unit the loss is expressed in.  Recomputed by `--corpus`, defaults here so
#: the tool runs without the tarball unpacked.
AXES = {
    "score":             (46.0, 0.6),
    "wonders_completed": (1.30, 1.5),
    "wonder_stages":     (4.40, 1.5),
    "takes":             (6.20, 1.5),
    "tier3_pct":         (4.30, 1.0),
    "wars_declared":     (0.62, 1.2),
    "aggressions":       (1.40, 1.0),
    "first_gov_round":   (3.60, 1.0),
    "sci_final":         (9.50, 0.4),
    "bids":              (2.60, 0.5),
    "leaders_elected":   (0.90, 0.5),
    "colonies":          (1.05, 0.5),
    "rounds":            (1.10, 0.3),
}


#: The corpus SEGMENTATION, as an explicit auditable rule rather than a
#: k-means label.  `tools/bgo_cluster.py` is the evidence that decided the
#: shape of this: k-means silhouette on the 2p rows barely clears a
#: permutation null (ratio 1.03-1.37 for k=2..6), so the corpus is a
#: CONTINUUM, not a set of types, and any "cluster" is a slice through a
#: blob.  Given that, a rule you can read beats a centroid you cannot:
#: k-means at k=3 and k=5 recovers the same three directions (economy size,
#: cards-vs-wonders, and a discrete militarist minority), so the cut below is
#: k-means' answer written down in the units of the game.
#:
#: Thresholds are the corpus's own quantiles at 2p: 11 stages is the ~78th
#: percentile of wonder_stages, 37 takes the ~72nd of takes, and
#: wars_declared >= 1 is a genuinely bimodal split (83% of 2p players never
#: declare one).
def segment(r, num=None):
    num = num or bgo_stats.num

    def f(k):
        v = num(r.get(k))
        return 0.0 if v is None else v
    if f("wars_declared") >= 1:
        return "warlord"
    ws, tk = f("wonder_stages"), f("takes")
    if ws >= 11:
        return "wonder"
    if tk >= 37 and ws <= 9:
        return "tempo"
    if ws <= 6 and tk <= 31:
        return "passive"
    return "builder"


SEGMENTS = ("builder", "wonder", "tempo", "warlord", "passive")


def spec_of(name):
    """`book` / `book2` / `var:tempo` / `human:builder` / a weight-file spec."""
    if name.startswith("human:"):
        from engine.bots.human import HUMANS
        cls = HUMANS[name.split(":", 1)[1]]
        return ("human", cls.__module__.rsplit(".", 1)[-1], cls.__name__)
    if name.startswith("var:"):
        from engine.bots.variants import VARIANTS
        cls = VARIANTS[name.split(":", 1)[1]]
        return ("variant", cls.__module__.rsplit(".", 1)[-1], cls.__name__)
    if name in ("book", "book2"):
        return name
    from experiments.arena import load_spec
    return load_spec(name)


def measure(spec, players=2, games=24, seed0=0, quiet=True):
    """Bot behaviour on the corpus axes: {axis: mean}, plus per-GAME rates."""
    fd, path = tempfile.mkstemp(suffix=".tsv")
    os.close(fd)
    err = sys.stderr
    try:
        if quiet:
            sys.stderr = open(os.devnull, "w")
        bgo_botmatch.run(spec, players, games, seed0, path)
    finally:
        if quiet:
            sys.stderr.close()
            sys.stderr = err
    rows = bgo_stats.load(path, players)
    os.unlink(path)
    out = {}
    for k in AXES:
        xs = bgo_stats.col(rows, k)
        out[k] = sum(xs) / len(xs) if xs else float("nan")
    out["_n"] = len(rows)
    out["_games"] = len(set(r["game_id"] for r in rows))
    for k in ("wars_declared", "aggressions", "wonders_completed"):
        out[k + "_per_game"] = out[k] * players
    return out


def human_target(tsv, players=2, rows_filter=None):
    """Target vector from the corpus, optionally restricted to a subset."""
    rows = bgo_stats.load(tsv, players)
    if rows_filter:
        rows = [r for r in rows if rows_filter(r)]
    out = {}
    for k in AXES:
        xs = bgo_stats.col(rows, k)
        out[k] = sum(xs) / len(xs) if xs else float("nan")
    out["_n"] = len(rows)
    return out


def loss(meas, target, weights=None):
    """Weighted mean squared residual, in units of human sigmas."""
    w = dict(weights or {})
    tot, sw = 0.0, 0.0
    for k, (sd, dw) in AXES.items():
        if k not in target or target[k] != target[k]:
            continue
        ww = w.get(k, dw)
        if ww <= 0:
            continue
        r = (meas[k] - target[k]) / sd
        tot += ww * r * r
        sw += ww
    return tot / sw if sw else float("nan")


def table(meas, target, weights=None):
    w = dict(weights or {})
    out = ["%-20s %10s %10s %8s %6s" % ("axis", "target", "bot", "sigmas", "w")]
    for k, (sd, dw) in AXES.items():
        if k not in target:
            continue
        ww = w.get(k, dw)
        out.append("%-20s %10.2f %10.2f %8.2f %6.1f"
                   % (k, target[k], meas[k], (meas[k] - target[k]) / sd, ww))
    out.append("loss = %.4f  (n=%d rows / %d games)"
               % (loss(meas, target, weights), meas["_n"], meas["_games"]))
    return "\n".join(out)


# ------------------------------------------------------------------- fitting

def fit(arch, players, games, rounds, seed0, tsv, out_path, margin=0.04,
        log=print):
    """Coordinate descent over an archetype's FIT_KNOBS.

    Paired evaluation: every candidate and the incumbent are measured on the
    same seed block, and the block rotates each pass so the fit cannot lock
    onto one set of deals.
    """
    from engine.bots.human import HUMANS
    cls = HUMANS[arch]
    target = dict(cls.TARGET)
    weights = dict(getattr(cls, "FIT_WEIGHTS", {}))
    knobs = dict(cls.FIT_KNOBS)          # knob -> list of candidate values
    cur = {k: cls.PROFILE.get(k, v[0]) for k, v in knobs.items()}

    def evaluate(prof, block):
        spec = ("human", cls.__module__.rsplit(".", 1)[-1], cls.__name__,
                json.dumps(prof, default=list))
        m = measure(spec, players, games, block)
        return loss(m, target, weights), m

    best_l, best_m = evaluate(cur, seed0)
    log("start loss %.4f  knobs=%s" % (best_l, cur))
    for r in range(rounds):
        block = seed0 + 1000 * (r + 1)
        best_l, best_m = evaluate(cur, block)      # re-measure on THIS block
        for key, values in knobs.items():
            for v in values:
                if v == cur[key]:
                    continue
                trial = dict(cur)
                trial[key] = v
                l, m = evaluate(trial, block)
                if l < best_l - margin:            # margin: no ratcheting
                    log("  pass %d  %s: %r -> %r   loss %.4f -> %.4f"
                        % (r, key, cur[key], v, best_l, l))
                    cur, best_l, best_m = trial, l, m
        log("pass %d done: loss %.4f knobs=%s" % (r, best_l, cur))
    if out_path:
        with open(out_path, "w") as fh:
            json.dump({"arch": arch, "players": players, "knobs": cur,
                       "loss": best_l, "measured": best_m, "target": target},
                      fh, indent=1, default=list)
    log(table(best_m, target, weights))
    return cur, best_l


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("cmd", choices=("measure", "fit", "target"))
    ap.add_argument("--spec", default="book")
    ap.add_argument("--arch", default="builder")
    ap.add_argument("--players", type=int, default=2)
    ap.add_argument("--games", type=int, default=24)
    ap.add_argument("--rounds", type=int, default=3)
    ap.add_argument("--margin", type=float, default=0.04,
                    help="loss improvement a knob change must clear to be "
                         "accepted.  Must exceed the block-to-block noise of "
                         "a --games measurement or the fit ratchets on noise: "
                         "at 24 games the same knobs re-measure 0.14-0.29.")
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--tsv", default="/tmp/human.tsv")
    ap.add_argument("--clusters", default="", help="cluster dump from bgo_cluster")
    ap.add_argument("--cluster", default="", help="cluster id to target")
    ap.add_argument("--seg", default="", help="corpus segment: %s, or all"
                    % ",".join(SEGMENTS))
    ap.add_argument("--out", default="")
    a = ap.parse_args(argv)

    if a.cmd == "target":
        if a.seg:
            for s in (SEGMENTS if a.seg == "all" else (a.seg,)):
                t = human_target(a.tsv, a.players,
                                 lambda r, s=s: segment(r) == s)
                print("# seg=%s players=%d" % (s, a.players))
                print(json.dumps(t, indent=1, sort_keys=True))
            return 0
        keep = None
        if a.clusters and a.cluster:
            keep = set()
            with open(a.clusters) as fh:
                next(fh)
                for line in fh:
                    g, c, cl = line.rstrip("\n").split("\t")
                    if cl == a.cluster:
                        keep.add((g, c))
            t = human_target(a.tsv, a.players,
                             lambda r: (r["game_id"], r["colour"]) in keep)
        else:
            t = human_target(a.tsv, a.players)
        print(json.dumps(t, indent=1, sort_keys=True))
        return 0

    if a.cmd == "measure":
        from engine.bots.human import HUMANS
        m = measure(spec_of(a.spec), a.players, a.games, a.seed)
        tgt = None
        if a.spec.startswith("human:"):
            tgt = HUMANS[a.spec.split(":", 1)[1]].TARGET
        elif a.arch in HUMANS:
            tgt = HUMANS[a.arch].TARGET
        if tgt:
            print(table(m, tgt, getattr(HUMANS.get(a.arch), "FIT_WEIGHTS", {})))
        else:
            print(json.dumps(m, indent=1, sort_keys=True))
        return 0

    fit(a.arch, a.players, a.games, a.rounds, a.seed, a.tsv, a.out,
        margin=a.margin)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
