#!/usr/bin/env python3
"""Re-score every ARCHIVED league evaluation under the new objective.

Offline only: no games, no engine, just the structured `generations_*.jsonl`
the arms already wrote.  It answers the one question a paired A/B cannot --
"would the new objective have made better accept/reject calls on the decisions
we actually faced" -- against the old objective, on identical data.

WHAT IS EXACT AND WHAT IS A PROXY.  Read this before quoting a number.

* The old objective is recomputed from the logged per-opponent MEAN culture
  through the parent tree's `own_share`, and validated against the logged
  `edge` (Spearman ~0.995+).  `own_share` is gone from the tree, so it is
  restated here, once, clearly labelled, purely to reproduce the thing being
  replaced.
* The new objective is `lead_share` imported from `experiments.hillclimb_pool`
  -- not reimplemented.
* **At 2p the recomputation is EXACT in the quantity**: with one defender,
  "margin over the mean defender" and "lead over the best defender" are the
  same number, so the logged `margin` column IS the lead.
* **At 3p/4p it is a PROXY**: the archives never recorded the best opponent's
  culture, only the mean, so the 3p/4p rows use margin-over-mean in place of
  lead-over-best.  That tests the main change (a differential instead of an
  absolute own score) but NOT the best-vs-mean refinement, which is
  unmeasurable from these logs.  Every 3p/4p number below is marked.
* Both objectives are aggregated by averaging per-opponent means and then
  applying the squash, which is `tanh(mean) != mean(tanh)`.  The original
  analysis validated that approximation at Spearman 0.995-0.998 against the
  logged edge; the same validation is printed here.
* `lo` (the actual accept bound) is computed by the trainer from PER-GAME
  variance, which the logs do not contain.  Where an accept RULE is needed,
  both objectives get the same opponent-level proxy bound, so the comparison
  between them is apples-to-apples even though neither matches the trainer's
  absolute accept rate.  It is labelled `lo_hat` everywhere.
"""
from __future__ import annotations

import json
import math
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from experiments.hillclimb_pool import (lead_share, lead_scale_for,  # noqa: E402
                                        DEFAULT_ALPHA, LEAD_SCALE)

TTA = os.environ.get("TTA_ROOT", "/Users/pt/tta-ai")

# The REPLACED objective, restated here and nowhere else in the tree.  This is
# the thing being measured against, not a thing anyone should call.
OLD_CULTURE_CENTRE = 100.0
OLD_CULTURE_SCALE = 120.0


def own_share(c, centre=OLD_CULTURE_CENTRE, scale=OLD_CULTURE_SCALE):
    if c is None:
        return None
    return 0.5 * (1.0 + math.tanh((float(c) - centre) / scale))


CHAINS = {
    2: ["experiments/generations_2p.jsonl",
        "experiments/archive_preplan/league_state_1ply_20260726/generations_2p.jsonl",
        "experiments/archive_2p_quiescent_20260729/generations_2p.jsonl",
        "experiments/league_state/generations_2p.jsonl"],
    3: ["experiments/generations_3p.jsonl",
        "experiments/archive_prehorizon/generations_3p.jsonl",
        "experiments/archive_preplan/league_state_1ply_20260726/generations_3p.jsonl",
        "experiments/league_state/archive_prequiescent_20260730/generations_3p.jsonl",
        "experiments/league_state/generations_3p.jsonl"],
    4: ["experiments/generations_4p.jsonl",
        "experiments/archive_prehorizon/generations_4p.jsonl",
        "experiments/archive_preplan/league_state_1ply_20260726/generations_4p.jsonl",
        "experiments/league_state/archive_4p_cold/generations_4p.jsonl",
        "experiments/league_state/archive_prequiescent_20260730/generations_4p.jsonl"],
}

Z = 1.2816                              # the trainer's accept z


# ------------------------------------------------------------------ stats

def _rank(vals):
    order = sorted(range(len(vals)), key=lambda i: vals[i])
    ranks = [0.0] * len(vals)
    i = 0
    while i < len(order):
        j = i
        while j + 1 < len(order) and vals[order[j + 1]] == vals[order[i]]:
            j += 1
        for k in range(i, j + 1):
            ranks[order[k]] = (i + j) / 2.0 + 1
        i = j + 1
    return ranks


def pearson(xs, ys):
    pairs = [(x, y) for x, y in zip(xs, ys) if x is not None and y is not None]
    if len(pairs) < 3:
        return None
    n = len(pairs)
    mx = sum(p[0] for p in pairs) / n
    my = sum(p[1] for p in pairs) / n
    sxy = sum((p[0] - mx) * (p[1] - my) for p in pairs)
    sxx = sum((p[0] - mx) ** 2 for p in pairs)
    syy = sum((p[1] - my) ** 2 for p in pairs)
    if sxx <= 0 or syy <= 0:
        return None
    return sxy / math.sqrt(sxx * syy)


def spearman(xs, ys):
    pairs = [(x, y) for x, y in zip(xs, ys) if x is not None and y is not None]
    if len(pairs) < 3:
        return None, len(pairs)
    a = _rank([p[0] for p in pairs])
    b = _rank([p[1] for p in pairs])
    return pearson(a, b), len(pairs)


def wmean(pairs):
    num = den = 0.0
    for v, w in pairs:
        if v is None or w is None or w <= 0:
            continue
        num += v * w
        den += w
    return (num / den) if den > 0 else None


def wstats(pairs):
    """Weighted mean, SE and one-sided bound over OPPONENT-level samples.

    Same shape as `hillclimb_pool.weighted_stats`, applied one level up.  See
    the module docstring: this is `lo_hat`, a proxy, applied identically to
    both objectives.
    """
    pairs = [(v, w) for v, w in pairs if v is not None and w and w > 0]
    n = len(pairs)
    if n < 2:
        return None, None, None
    sw = sum(w for _v, w in pairs)
    m = sum(v * w for v, w in pairs) / sw
    var = sum((w * (v - m)) ** 2 for v, w in pairs) * n / (n - 1)
    se = math.sqrt(var) / sw
    return m, se, m - Z * se


# ------------------------------------------------------------------ load

def evaluations(players):
    """Every blend-era candidate evaluation for one player count."""
    out = []
    for rel in CHAINS[players]:
        p = os.path.join(TTA, rel)
        if not os.path.exists(p):
            continue
        for line in open(p):
            line = line.strip()
            if not line:
                continue
            try:
                g = json.loads(line)
            except json.JSONDecodeError:
                continue
            for t in g.get("tried", []):
                per = t.get("per_opponent") or {}
                if not per:
                    continue
                if {e.get("metric") for e in per.values()} != {"blend"}:
                    continue          # only the objective being replaced
                out.append({"logged_edge": t.get("edge"),
                            "logged_lo": t.get("lo"),
                            "scale": lead_scale_for(players),
                            "veto": t.get("veto") or [], "per": per})
    return out


def score(rec, alpha, kind):
    """Per-opponent paired edges under one objective -> (edges, weights).

    `kind` is "old" (own culture through own_share) or "new" (culture
    differential through lead_share).
    """
    pairs = []
    for e in rec["per"].values():
        w = e.get("weight")
        wr, cr = e.get("win_rate"), e.get("champ_rate")
        if not w or w <= 0 or wr is None or cr is None:
            continue
        if kind == "old":
            c, cc = e.get("culture"), e.get("champ_culture")
            if c is None or cc is None:
                continue
            cul = own_share(c) - own_share(cc)
        else:
            m, cm = e.get("margin"), e.get("champ_margin")
            if m is None or cm is None:
                continue
            sc = rec["scale"]
            cul = lead_share(m, sc) - lead_share(cm, sc)
        pairs.append(((1.0 - alpha) * cul + alpha * (wr - cr), w))
    return pairs


def win_diff(rec):
    a = wmean([(e.get("win_rate"), e.get("weight")) for e in rec["per"].values()])
    b = wmean([(e.get("champ_rate"), e.get("weight")) for e in rec["per"].values()])
    return None if a is None or b is None else a - b


def derive_scale():
    """Re-derive `hillclimb_pool.LEAD_SCALE` from the human BGO corpus.

    For every seat of every completed human game, the culture lead is
    `own final score - max(other seats' final scores)` -- the exact quantity
    `lead_share` squashes, taken from `sources/bgo/index.tsv`'s `results`
    column.  The rule is `scale ~= 2.5 x sd(lead)`, per player count.

    The corpus is EXTERNAL and FIXED, which is the entire point: a scale
    derived from our own logs would go stale every time the bot improved,
    which is the failure that killed CULTURE_CENTRE.
    """
    import csv
    import statistics
    path = os.path.join(TTA, "sources", "bgo", "index.tsv")
    leads = {}
    games = {}
    for r in csv.DictReader(open(path), delimiter="\t"):
        try:
            n = int(r["players"])
        except (TypeError, ValueError):
            continue
        sc = []
        for part in (r.get("results") or "").split("|"):
            if ":" not in part:
                continue
            try:
                sc.append(float(part.rsplit(":", 1)[1]))
            except ValueError:
                pass
        if n < 2 or len(sc) != n:
            continue                      # unfinished or unparsable
        games[n] = games.get(n, 0) + 1
        for seat in range(n):
            best_other = max(sc[i] for i in range(n) if i != seat)
            leads.setdefault(n, []).append(sc[seat] - best_other)

    print("# LEAD_SCALE re-derived from the human BGO corpus "
          "(external and fixed -- it does not move when our bot improves)")
    print(f"# {'n':>3} {'games':>6} {'seats':>6} {'sd':>8} {'mean':>8} "
          f"{'p10':>8} {'p90':>8} {'2.5*sd':>8} {'in code':>8}")
    for n in sorted(leads):
        v = sorted(leads[n])
        sd = statistics.pstdev(v)
        print(f"  {n}p {games[n]:6d} {len(v):6d} {sd:8.1f} "
              f"{statistics.mean(v):+8.1f} {v[len(v) // 10]:+8.1f} "
              f"{v[9 * len(v) // 10]:+8.1f} {2.5 * sd:8.1f} "
              f"{lead_scale_for(n):8.1f}")
    print("\n# The dispersion is NOT ordered by player count -- 2p is the")
    print("# widest.  Score LEVEL and lead DISPERSION are different things.")
    print(f"# LEAD_SCALE in code: {LEAD_SCALE}")


def main():
    if "--derive-scale" in sys.argv[1:]:
        return derive_scale()
    a = DEFAULT_ALPHA
    print(f"# Archived league evaluations re-scored under the new objective")
    print(f"\nalpha = {a} for both objectives.  2p is EXACT (one defender, so "
          f"margin-over-mean IS lead-over-best);\n3p/4p use margin-over-mean as "
          f"a PROXY for lead-over-best -- the archives never recorded the best\n"
          f"opponent's culture.  See the module docstring.\n")

    for players in (2, 3, 4):
        recs = evaluations(players)
        tag = "EXACT" if players == 2 else "PROXY (margin over the MEAN)"
        print(f"\n## {players}p   n={len(recs)} blend-era evaluations   [{tag}]")

        old_e, new_e, wd, logged, old_lo, new_lo, llo = [], [], [], [], [], [], []
        for r in recs:
            om, _ose, olo = wstats(score(r, a, "old"))
            nm, _nse, nlo = wstats(score(r, a, "new"))
            old_e.append(om)
            new_e.append(nm)
            old_lo.append(olo)
            new_lo.append(nlo)
            wd.append(win_diff(r))
            logged.append(r["logged_edge"])
            llo.append(r["logged_lo"])

        rho_s, n_s = spearman(logged, old_e)
        print(f"  sanity: recomputed OLD edge vs the LOGGED edge   "
              f"Spearman {rho_s:+.3f}  Pearson {pearson(logged, old_e):+.3f}  n={n_s}")
        rho_l, _ = spearman(llo, old_lo)
        print(f"  sanity: lo_hat vs the trainer's logged lo        "
              f"Spearman {rho_l:+.3f}  (proxy bound, see docstring)")

        rlog, _ = spearman(logged, wd)
        print(f"  reference: the LOGGED edge vs win-rate diff              "
              f"Spearman {rlog:+.3f}   (the number /tmp/objective_analysis.md "
              f"quotes)")

        ro, no = spearman(old_e, wd)
        rn, nn = spearman(new_e, wd)
        print(f"\n  Spearman vs win-rate diff:  OLD {ro:+.3f} (n={no})   "
              f"NEW {rn:+.3f} (n={nn})   delta {rn - ro:+.3f}")
        print(f"  Pearson  vs win-rate diff:  OLD {pearson(old_e, wd):+.3f}   "
              f"NEW {pearson(new_e, wd):+.3f}")

        # sign flips
        flip = comp = 0
        for o, n in zip(old_e, new_e):
            if o is None or n is None:
                continue
            comp += 1
            if (o > 0) != (n > 0):
                flip += 1
        print(f"\n  edge-sign decisions that FLIP new vs old: {flip}/{comp} "
              f"({flip / comp:.1%})")

        # the conservative-bias table, both objectives, same proxy rule
        for name, edges, los in (("OLD", old_e, old_lo), ("NEW", new_e, new_lo)):
            fa = fr = ok = tot = 0
            for r, lo, d in zip(recs, los, wd):
                if lo is None or d is None or d == 0:
                    continue
                tot += 1
                acc = lo > 0 and not r["veto"]
                if acc and d < 0:
                    fa += 1
                elif (not acc) and d > 0:
                    fr += 1
                else:
                    ok += 1
            print(f"  {name} accept rule (lo_hat>0, no veto), n={tot}:  "
                  f"accepted-but-WORSE-on-winning {fa} ({fa / tot:.1%})   "
                  f"rejected-but-BETTER-on-winning {fr} ({fr / tot:.1%})   "
                  f"agree {ok / tot:.1%}")

        # THE RISK, measured offline: where does a lead gain come from?
        up = down = both = 0
        all_rows = opp_down_all = 0
        for r in recs:
            for e in r["per"].values():
                c, cc = e.get("culture"), e.get("champ_culture")
                m, cm = e.get("margin"), e.get("champ_margin")
                if None in (c, cc, m, cm):
                    continue
                d_opp_all = (c - m) - (cc - cm)
                all_rows += 1
                opp_down_all += 1 if d_opp_all < 0 else 0
                if m - cm <= 0:
                    continue                    # only rows that gained lead
                d_own = c - cc
                d_opp = (c - m) - (cc - cm)     # opponent culture, backed out
                if d_own > 0 and d_opp >= 0:
                    up += 1                     # out-produced them
                elif d_own <= 0 and d_opp < 0:
                    down += 1                   # pure suppression
                else:
                    both += 1
        tot = up + down + both
        if tot:
            print(f"\n  rows where the candidate GAINED culture differential "
                  f"over its parent (n={tot}):")
            print(f"    out-produced (own up, opponents not down): {up} "
                  f"({up / tot:.1%})")
            print(f"    pure suppression (own flat/down, opponents down): "
                  f"{down} ({down / tot:.1%})")
            print(f"    mixed (own up AND opponents down): {both} "
                  f"({both / tot:.1%})")
            print(f"    NOISE CONTROL -- opponents' culture fell on "
                  f"{opp_down_all}/{all_rows} ({opp_down_all / all_rows:.1%}) "
                  f"of ALL rows,\n      unconditionally.  Conditioning on a "
                  f"differential GAIN mechanically raises that, so the split\n"
                  f"      above overstates deliberate suppression by an "
                  f"unknown amount.  The informative\n      cell is the "
                  f"middle one: own culture flat-or-down while the "
                  f"differential rose.")


if __name__ == "__main__":
    main()
