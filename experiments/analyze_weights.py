"""Cross-player-count analysis of what the hill climb actually learned.

    python3 -m experiments.analyze_weights            # human-readable report
    python3 -m experiments.analyze_weights --md       # markdown for PROGRESS.md
    python3 -m experiments.analyze_weights --json experiments/weights_analysis.json

`summarize.py` answers "how is the 4p run going?".  This answers the question
that a *human* wants answered: **which levers of the hand-written evaluation
were most wrong, in which direction, and do the three independent runs agree?**
Agreement across 2p/3p/4p is the whole point -- three separate searches over
noisy self-play will each chase their own noise, so a lever only becomes a
believable piece of Through the Ages advice when all three move it the same
way.  That is what the `consensus` section reports.

Notes on the arithmetic
-----------------------
* A linear evaluation is invariant to a positive rescale of *all* weights, so
  drifts would be meaningless if the overall scale could float.  It cannot:
  `hillclimb.py` freezes `culture` at 1.0 precisely so the units stay pinned.
* Relative drift is `(champ - default) / max(|default|, FLOOR)`.  The floor
  keeps a weight whose default is ~0 (e.g. `ca_left` at 0.05) from reporting a
  40x move when it shifted by 2 points of nothing.
* `PHASE_KEYS` features get an `_early`/`_late` pair on top of their base
  weight; reading those three numbers separately is useless.  The `phase`
  section instead reports the *effective* weight at each end of the game,
  `w + w_early` (Age A) and `w + w_late` (Age III+), which is what the bot
  actually applies.
"""
from __future__ import annotations

import argparse
import json
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine.bots.weighted import DEFAULT_WEIGHTS, PHASE_KEYS  # noqa: E402
from experiments.summarize import GROUPS, group_of  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))

# Denominator floor for relative drift: below this a default weight is "about
# zero" and a ratio stops meaning anything.
FLOOR = 0.25
# |relative drift| a lever must clear before it is worth a human's attention.
NOTABLE = 0.35


def load_champion(players, path=None):
    path = path or os.path.join(HERE, "champion_%dp.json" % players)
    if not os.path.exists(path):
        return None
    with open(path) as fh:
        d = json.load(fh)
    w = dict(DEFAULT_WEIGHTS)
    w.update(d.get("weights", d))
    return {"gen": d.get("gen"), "sigma": d.get("sigma"), "weights": w,
            "path": path}


def drift(key, champ_w):
    d = DEFAULT_WEIGHTS.get(key, 0.0)
    c = champ_w.get(key, d)
    denom = max(abs(d), FLOOR)
    return {"key": key, "group": group_of(key), "default": d, "champion": c,
            "abs": c - d, "rel": (c - d) / denom,
            "flipped": (d > 0 > c) or (d < 0 < c)}


def analyze_one(players, champ):
    rows = [drift(k, champ["weights"]) for k in sorted(DEFAULT_WEIGHTS)]
    rows.sort(key=lambda r: -abs(r["rel"]))
    by_group = {}
    for r in rows:
        by_group.setdefault(r["group"].split("/")[0], []).append(abs(r["rel"]))
    groups = sorted(((g, sum(v) / len(v)) for g, v in by_group.items()),
                    key=lambda t: -t[1])
    # effective early/late weight per phase key
    phase = []
    w = champ["weights"]
    for k in PHASE_KEYS:
        de = DEFAULT_WEIGHTS[k] + DEFAULT_WEIGHTS[k + "_early"]
        dl = DEFAULT_WEIGHTS[k] + DEFAULT_WEIGHTS[k + "_late"]
        ce = w[k] + w[k + "_early"]
        cl = w[k] + w[k + "_late"]
        phase.append({"key": k, "default_early": de, "default_late": dl,
                      "champion_early": ce, "champion_late": cl,
                      # >0 => the champion cares *more* late than early
                      "default_tilt": dl - de, "champion_tilt": cl - ce})
    return {"players": players, "gen": champ["gen"], "sigma": champ["sigma"],
            "rows": rows, "groups": groups, "phase": phase}


def consensus(per_count):
    """Per weight: do the runs agree on the direction of the move?"""
    out = []
    counts = sorted(per_count)
    for key in sorted(DEFAULT_WEIGHTS):
        rels = {k: next(r["rel"] for r in per_count[k]["rows"] if r["key"] == key)
                for k in counts}
        vals = list(rels.values())
        notable = [v for v in vals if abs(v) >= NOTABLE]
        if not notable:
            continue
        signs = {1 if v > 0 else -1 for v in notable}
        agree = len(signs) == 1 and len(notable) == len(vals)
        # partial agreement: every *notable* move points the same way, but at
        # least one player count barely moved the weight at all
        partial = len(signs) == 1 and not agree
        out.append({"key": key, "group": group_of(key), "rel": rels,
                    "default": DEFAULT_WEIGHTS[key],
                    "champion": {k: next(r["champion"] for r in per_count[k]["rows"]
                                         if r["key"] == key) for k in counts},
                    "mean_rel": sum(vals) / len(vals),
                    "n_notable": len(notable),
                    "verdict": "consensus" if agree else
                               ("leaning" if partial else "conflict")})
    out.sort(key=lambda r: (r["verdict"] != "consensus",
                            r["verdict"] != "leaning",
                            -abs(r["mean_rel"])))
    return out


# ------------------------------------------------------------------ output

def _fmt(v):
    return ("%+.3f" % v).rstrip("0").rstrip(".") if v else "0"


def render(per_count, cons, top, markdown=False):
    L = []
    p = L.append
    h1, h2 = ("## ", "### ") if markdown else ("== ", "-- ")
    counts = sorted(per_count)

    p(h1 + "Which weights the search moved most")
    p("")
    p("Champions: " + ", ".join(
        "%dp gen %s" % (k, per_count[k]["gen"]) for k in counts) + ".")
    p("Relative drift is `(champion - default) / max(|default|, %.2f)`; the "
      "overall scale is pinned because `culture` is frozen at 1.0." % FLOOR)
    p("")

    for k in counts:
        a = per_count[k]
        p(h2 + "%dp -- top %d movers (gen %s)" % (k, top, a["gen"]))
        p("")
        p("| weight | group | default | champion | rel | note |")
        p("|---|---|---|---|---|---|")
        for r in a["rows"][:top]:
            note = "SIGN FLIP" if r["flipped"] else ""
            p("| `%s` | %s | %s | %s | %+.0f%% | %s |"
              % (r["key"], r["group"], _fmt(r["default"]), _fmt(r["champion"]),
                 100 * r["rel"], note))
        p("")
        p("Mean |drift| by group: " +
          " > ".join("%s %.2f" % (g, v) for g, v in a["groups"]))
        p("")

    p(h1 + "Consistency across 2p / 3p / 4p")
    p("")
    p("A lever counts as *notable* at |rel| >= %.0f%%. `consensus` = every "
      "player count moved it notably and in the same direction; `leaning` = "
      "the ones that moved agree but at least one count left it alone; "
      "`conflict` = the counts disagree on the sign." % (100 * NOTABLE))
    p("")
    p("| weight | group | verdict | default | " +
      " | ".join("%dp" % k for k in counts) + " |")
    p("|---|---|---|---|" + "---|" * len(counts))
    for r in cons:
        p("| `%s` | %s | %s | %s | " % (r["key"], r["group"], r["verdict"],
                                        _fmt(r["default"]))
          + " | ".join("%s (%+.0f%%)" % (_fmt(r["champion"][k]),
                                         100 * r["rel"][k]) for k in counts)
          + " |")
    p("")
    n_con = sum(1 for r in cons if r["verdict"] == "consensus")
    n_lean = sum(1 for r in cons if r["verdict"] == "leaning")
    n_conf = sum(1 for r in cons if r["verdict"] == "conflict")
    p("%d consensus, %d leaning, %d conflicting out of %d notable levers."
      % (n_con, n_lean, n_conf, len(cons)))
    p("")

    p(h1 + "Early vs late game (effective phase weights)")
    p("")
    p("`w + w_early` is what the bot applies in Age A, `w + w_late` from Age "
      "III on. `tilt` = late - early; a *fall* means the lever matters most "
      "at the start of the game.")
    p("")
    p("| feature | default early -> late | " +
      " | ".join("%dp early -> late" % k for k in counts) + " |")
    p("|---|---|" + "---|" * len(counts))
    for i, pk in enumerate(PHASE_KEYS):
        cells = []
        for k in counts:
            ph = per_count[k]["phase"][i]
            cells.append("%s -> %s (%s)" % (_fmt(ph["champion_early"]),
                                            _fmt(ph["champion_late"]),
                                            _fmt(ph["champion_tilt"])))
        d0 = per_count[counts[0]]["phase"][i]
        p("| `%s` | %s -> %s (%s) | " % (pk, _fmt(d0["default_early"]),
                                         _fmt(d0["default_late"]),
                                         _fmt(d0["default_tilt"]))
          + " | ".join(cells) + " |")
    p("")
    return "\n".join(L)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--players", type=int, nargs="*", default=[2, 3, 4])
    ap.add_argument("--top", type=int, default=15)
    ap.add_argument("--md", action="store_true", help="markdown headings")
    ap.add_argument("--json", help="also dump the full analysis here")
    args = ap.parse_args()

    per_count = {}
    for k in args.players:
        champ = load_champion(k)
        if champ is None:
            print("no champion for %dp" % k, file=sys.stderr)
            continue
        per_count[k] = analyze_one(k, champ)
    if not per_count:
        return 1
    cons = consensus(per_count) if len(per_count) > 1 else []
    text = render(per_count, cons, args.top, markdown=args.md)
    print(text)
    if args.json:
        with open(args.json, "w") as fh:
            json.dump({"per_count": per_count, "consensus": cons}, fh, indent=1)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
