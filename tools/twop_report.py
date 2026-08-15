#!/usr/bin/env python3
"""Aggregate `tools/twop_profile.py` game records into a behavioural profile.

    python3 tools/twop_report.py /tmp/twop_main

Every number printed is a per-GAME mean with the standard error of that mean
(n = games), except where marked.  Champion and opponent are measured on the
SAME games, so every "vs" is a paired within-game comparison and the paired SE
is the one reported for a difference.
"""
from __future__ import annotations

import argparse
import glob
import json
import math
import os
import sys


def mean_se(xs):
    n = len(xs)
    if n == 0:
        return 0.0, 0.0
    m = sum(xs) / n
    if n < 2:
        return m, 0.0
    v = sum((x - m) ** 2 for x in xs) / (n - 1)
    return m, math.sqrt(v / n)


def fmt(xs, d=1):
    m, s = mean_se(xs)
    return f"{m:.{d}f}+/-{s:.{d}f}"


def paired(a, b, d=1):
    """Mean of (a_i - b_i) with its paired SE."""
    return fmt([x - y for x, y in zip(a, b)], d)


def load(path):
    with open(path) as fh:
        return json.load(fh)


# ------------------------------------------------------------- extractors

def per_game(recs, fn, default=0.0):
    return [fn(r) for r in recs]


def ledger_sum(r, prefix):
    return sum(v for k, v in r["ledger"].items() if k.startswith(prefix))


def snaps(r):
    return r["snaps"]


def last_snap(r, key, default=0):
    return r["snaps"][-1][key] if r["snaps"] else default


def turn_stat(r, key):
    """Mean over that player's own turns in one game."""
    ss = r["snaps"]
    return (sum(s[key] for s in ss) / len(ss)) if ss else 0.0


def age_stat(r, key, age):
    ss = [s for s in r["snaps"] if s["age"] == age]
    return (sum(s[key] for s in ss) / len(ss)) if ss else None


def eoa(r, key, age):
    """Value at the LAST turn played inside `age` (None if never reached)."""
    ss = [s for s in r["snaps"] if s["age"] == age]
    return ss[-1][key] if ss else None


def moves(r, kind):
    return r["moves"].get(kind, 0)


# ------------------------------------------------------------------ report

CULTURE_GROUPS = [
    ("rate (production phase)", ("rate:",)),
    ("events resolved in play", ("event:",)),
    ("end-game Age III events", ("endgame:final_event",)),
    ("end-game wonder/tech bonus", ("endgame:bonus",)),
    ("preparing events (seeding)", ("military:prepare_event",)),
    ("one-off card/build culture", ("card:", "build:", "gov:", "leader:")),
    ("aggression / war transfers", ("aggression:", "war:")),
    ("penalties (food/war)", ("penalty:",)),
    ("other", ("opponent_resigned", "unmapped:")),
]


def group_of(k):
    for name, pres in CULTURE_GROUPS:
        if any(k.startswith(p) for p in pres):
            return name
    return "other"


def report(path, out=sys.stdout):
    d = load(path)
    A, B = d["recs_a"], d["recs_b"]
    n = len(A)
    p = lambda *a: print(*a, file=out)
    p("=" * 78)
    p(f"{d['a']}  vs  {d['b']}   n={n} games (seat-balanced), "
      f"{d['error_count']} engine errors")
    bad = sum(1 for r in A if not r["ledger_ok"])
    p(f"culture ledger reconciles to final score in {n - bad}/{n} games")
    p("-" * 78)
    p(f"  final score      A {fmt([r['culture'] for r in A])}   "
      f"B {fmt([r['culture'] for r in B])}   "
      f"margin {fmt([r['margin'] for r in A])}")
    p(f"  win share        {fmt([r['win'] for r in A], 3)}  (null 0.5)")
    p(f"  game length      {fmt([r['rounds'] for r in A], 2)} rounds")

    # ---- score composition
    p("\n  SCORE COMPOSITION (culture points per game, paired)")
    p(f"    {'source':<30}{'champion':>16}{'opponent':>16}{'difference':>16}")
    tot_a = tot_b = 0.0
    rows = []
    for name, pres in CULTURE_GROUPS:
        a = [sum(v for k, v in r["ledger"].items() if group_of(k) == name)
             for r in A]
        b = [sum(v for k, v in r["ledger"].items() if group_of(k) == name)
             for r in B]
        if not any(a) and not any(b):
            continue
        rows.append((name, a, b, mean_se([x - y for x, y in zip(a, b)])[0]))
        tot_a += mean_se(a)[0]
        tot_b += mean_se(b)[0]
    for name, a, b, _ in sorted(rows, key=lambda r: -abs(r[3])):
        p(f"    {name:<30}{fmt(a):>16}{fmt(b):>16}{paired(a, b):>16}")
    p(f"    {'TOTAL':<30}{tot_a:>16.1f}{tot_b:>16.1f}{tot_a - tot_b:>16.1f}")

    p("\n  ...the same ledger, by raw engine site (nothing is pooled here)")
    keys = sorted({k for r in A + B for k in r["ledger"]})
    rows = []
    for k in keys:
        a = [r["ledger"].get(k, 0) for r in A]
        b = [r["ledger"].get(k, 0) for r in B]
        rows.append((abs(mean_se([x - y for x, y in zip(a, b)])[0]), k, a, b))
    for _, k, a, b in sorted(rows, reverse=True):
        if abs(mean_se(a)[0]) < 0.05 and abs(mean_se(b)[0]) < 0.05:
            continue
        p(f"    {k:<30}{fmt(a, 2):>16}{fmt(b, 2):>16}{paired(a, b, 2):>16}")

    # ---- where the margin is made, by age
    p("\n  SCORE GAP BY ROUND (champion culture - opponent culture, "
      "at end of champion's turn)")
    by_r = {}
    for r in A:
        for rnd, gap in r["gap_by_round"]:
            by_r.setdefault(rnd, []).append(gap)
    p(f"    {'round':>6}{'games':>7}{'gap':>16}{'age':>6}")
    for rnd in sorted(by_r):
        if len(by_r[rnd]) < max(5, n // 20):
            continue
        ages = [s["age"] for r in A for s in r["snaps"] if s["round"] == rnd]
        a = round(sum(ages) / len(ages)) if ages else 0
        p(f"    {rnd:>6}{len(by_r[rnd]):>7}{fmt(by_r[rnd]):>16}"
          f"{['A', 'I', 'II', 'III', 'IV'][a]:>6}")

    # ---- opening: when each side first does a thing (median round, and the
    #      share of games it happens at all)
    p("\n  FIRST TIME IT DOES X (median round | share of games)")
    keys = ["take_leader", "leader", "government", "take_wonder",
            "wonder_start", "upgrade_production", "upgrade_urban",
            "take_special-tech", "take_action", "aggression", "war", "pact"]
    for k in keys:
        ga = sorted(r["first"][k] for r in A if k in r["first"])
        gb = sorted(r["first"][k] for r in B if k in r["first"])
        if not ga and not gb:
            continue
        f = lambda g: (f"{g[len(g)//2]:>3} | {len(g)/n:.0%}" if g
                       else "  - |  0%")
        p(f"    {k:<30}{f(ga):>16}{f(gb):>16}")

    # ---- tempo
    p("\n  TEMPO / ACTIONS  (per own turn)")
    for lbl, key in (("civil actions available", "ca_total"),
                     ("civil actions LEFT unspent", "ca_left"),
                     ("military actions available", "ma_total"),
                     ("military actions LEFT unspent", "ma_left")):
        a = [turn_stat(r, key) for r in A]
        b = [turn_stat(r, key) for r in B]
        p(f"    {lbl:<30}{fmt(a, 2):>16}{fmt(b, 2):>16}{paired(a, b, 2):>16}")
    a = [sum(1 for s in r["snaps"] if s["ca_left"] > 0) / max(1, len(r["snaps"]))
         for r in A]
    b = [sum(1 for s in r["snaps"] if s["ca_left"] > 0) / max(1, len(r["snaps"]))
         for r in B]
    p(f"    {'share of turns w/ CA left':<30}{fmt(a, 3):>16}{fmt(b, 3):>16}"
      f"{paired(a, b, 3):>16}")

    # ---- economy by age
    p("\n  ECONOMY, at the END of each age (rate at the last turn in that age)")
    for age, an in ((1, "I"), (2, "II"), (3, "III")):
        reach_a = sum(1 for r in A if eoa(r, "culture_rate", age) is not None)
        p(f"    -- age {an}  (champion reaches it in {reach_a}/{n} games)")
        for lbl, key in (("culture/turn", "culture_rate"),
                         ("science/turn", "science_rate"),
                         ("resources/turn", "resource_rate"),
                         ("food/turn", "food_rate"),
                         ("workers total", "workers_total"),
                         ("techs known", "techs"),
                         ("wonders done", "wonders"),
                         ("strength", "strength")):
            pa = [(eoa(r, key, age), eoa(rb, key, age)) for r, rb in zip(A, B)]
            pa = [(x, y) for x, y in pa if x is not None and y is not None]
            if not pa:
                continue
            a = [x for x, _ in pa]
            b = [y for _, y in pa]
            p(f"       {lbl:<27}{fmt(a, 2):>16}{fmt(b, 2):>16}"
              f"{paired(a, b, 2):>16}")

    # ---- military / conflict
    p("\n  MILITARY & CONFLICT (per game)")
    for lbl, fn in (
        ("aggressions started", lambda r: sum(1 for _, k, _ in r["attacks_made"]
                                              if k == "aggression")),
        ("wars declared", lambda r: sum(1 for _, k, _ in r["attacks_made"]
                                        if k == "war")),
        ("aggressions suffered", lambda r: sum(1 for _, k, _ in r["attacked_by"]
                                               if k == "aggression")),
        ("wars suffered", lambda r: sum(1 for _, k, _ in r["attacked_by"]
                                        if k == "war")),
        ("pacts offered", lambda r: moves(r, "offer_pact")),
        ("events prepared", lambda r: len(r["prepared"])),
        ("  of them age III", lambda r: sum(1 for x in r["prepared"]
                                            if x[2] >= 3)),
        ("resolved events I seeded", lambda r: r["events_resolved_seeded_by_me"]),
        ("leftover age-III I seeded",
         lambda r: r["leftover_age3_seeded_by_me"]),
    ):
        a, b = [fn(r) for r in A], [fn(r) for r in B]
        p(f"    {lbl:<30}{fmt(a, 2):>16}{fmt(b, 2):>16}{paired(a, b, 2):>16}")
    p(f"    {'culture from MY seeded events':<30}"
      f"{fmt([r['event_culture_by_seeder'].get('mine', 0) for r in A], 2):>16}"
      f"{fmt([r['event_culture_by_seeder'].get('mine', 0) for r in B], 2):>16}")
    p(f"    {'culture from RIVAL seeded ev.':<30}"
      f"{fmt([r['event_culture_by_seeder'].get('rival', 0) for r in A], 2):>16}"
      f"{fmt([r['event_culture_by_seeder'].get('rival', 0) for r in B], 2):>16}")

    # when the fighting happens, and what a fight is worth
    for kind in ("war", "aggression"):
        rnds = [rr for r in A for rr, k, _ in r["attacks_made"] if k == kind]
        if not rnds:
            continue
        rnds.sort()
        firsts = [min(rr for rr, k, _ in r["attacks_made"] if k == kind)
                  for r in A if any(k == kind for _, k, _ in r["attacks_made"])]
        n_cnt = [sum(1 for _, k, _ in r["attacks_made"] if k == kind) for r in A]
        got = ([ledger_sum(r, "war:") for r in A] if kind == "war"
               else [ledger_sum(r, "aggression:") for r in A])
        per = [g / c for g, c in zip(got, n_cnt) if c]
        p(f"\n    {kind}s: in {sum(1 for c in n_cnt if c)}/{n} games, "
          f"{fmt(n_cnt, 2)} per game, first at round "
          f"{fmt(firsts, 2) if firsts else 'n/a'} "
          f"(p10 {rnds[len(rnds)//10]}, median {rnds[len(rnds)//2]}, "
          f"p90 {rnds[9*len(rnds)//10]}); "
          f"{fmt(per, 2)} culture each")

    # --- deterrence check: the strength curve round by round, and how big the
    #     gap is relative to the thresholds a rule-based opponent gates on
    #     (engine/bots/variants/military.py: war_lead 5, agg_lead 3-4).
    p("\n    STRENGTH BY ROUND (A, B, and the A-B gap; B's offence in "
      "variants/military.py needs a lead of 3-5 to fire)")
    p(f"      {'round':>6}{'games':>7}{'A':>13}{'B':>13}{'gap':>15}"
      f"{'B ahead by 3+':>15}")
    byr = {}
    for r, rb in zip(A, B):
        pa = {s["round"]: s["strength"] for s in r["snaps"]}
        pb = {s["round"]: s["strength"] for s in rb["snaps"]}
        for rnd in set(pa) & set(pb):
            byr.setdefault(rnd, []).append((pa[rnd], pb[rnd]))
    for rnd in sorted(byr):
        v = byr[rnd]
        if len(v) < max(5, n // 20):
            continue
        a = [x for x, _ in v]
        b = [y for _, y in v]
        lead = sum(1 for x, y in v if y - x >= 3) / len(v)
        p(f"      {rnd:>6}{len(v):>7}{fmt(a, 1):>13}{fmt(b, 1):>13}"
          f"{paired(a, b, 1):>15}{lead:>14.1%}")

    # --- what B spent on military and never fired
    p("\n    MILITARY INVESTMENT (per game / per own turn)")
    for lbl, fn in (
        ("unit workers (per turn)", lambda r: turn_stat(r, "w_units")),
        ("military actions avail (per turn)", lambda r: turn_stat(r, "ma_total")),
        ("military actions unspent (turn)", lambda r: turn_stat(r, "ma_left")),
        ("military units taken", lambda r: sum(
            r["take_types"].get(t, 0)
            for t in ("infantry", "cavalry", "artillery", "air"))),
        ("special-tech cards taken", lambda r: r["take_types"].get(
            "special-tech", 0)),
        ("tactic plays", lambda r: moves(r, "play_tactic")
         + moves(r, "copy_tactic")),
        ("defends made", lambda r: moves(r, "defend")),
    ):
        a, b = [fn(r) for r in A], [fn(r) for r in B]
        p(f"      {lbl:<32}{fmt(a, 2):>14}{fmt(b, 2):>14}"
          f"{paired(a, b, 2):>14}")

    # strength curve
    p("\n    strength at end of age:")
    for age, an in ((1, "I"), (2, "II"), (3, "III")):
        pa = [(eoa(r, "strength", age), eoa(rb, "strength", age))
              for r, rb in zip(A, B)]
        pa = [(x, y) for x, y in pa if x is not None and y is not None]
        if not pa:
            continue
        a = [x for x, _ in pa]
        b = [y for _, y in pa]
        ahead = sum(1 for x, y in pa if x > y) / len(pa)
        p(f"       age {an:<23}{fmt(a, 2):>16}{fmt(b, 2):>16}"
          f"{paired(a, b, 2):>16}   ahead {ahead:.0%}")

    # ---- move mix
    p("\n  MOVE MIX (per game)")
    kinds = sorted({k for r in A + B for k in r["moves"]},
                   key=lambda k: -sum(r["moves"].get(k, 0) for r in A))
    for k in kinds:
        a = [r["moves"].get(k, 0) for r in A]
        b = [r["moves"].get(k, 0) for r in B]
        if mean_se(a)[0] < 0.05 and mean_se(b)[0] < 0.05:
            continue
        p(f"    {k:<30}{fmt(a, 2):>16}{fmt(b, 2):>16}{paired(a, b, 2):>16}")

    # ---- what it takes / builds
    p("\n  CARDS TAKEN BY TYPE (per game)")
    types = {}
    for r in A:
        for t, c in r["take_types"].items():
            types[t] = types.get(t, 0) + c
    for t in sorted(types, key=lambda t: -types[t]):
        a = [r["take_types"].get(t, 0) for r in A]
        b = [r["take_types"].get(t, 0) for r in B]
        p(f"    {t:<30}{fmt(a, 2):>16}{fmt(b, 2):>16}{paired(a, b, 2):>16}")

    p("\n  TOP TECHS DEVELOPED (share of games, champion | opponent)")
    ca, cb = {}, {}
    for r in A:
        for _, nm in r["dev_names"]:
            ca[nm] = ca.get(nm, 0) + 1
    for r in B:
        for _, nm in r["dev_names"]:
            cb[nm] = cb.get(nm, 0) + 1
    for nm in sorted(set(ca) | set(cb),
                     key=lambda k: -(ca.get(k, 0) - cb.get(k, 0))):
        if max(ca.get(nm, 0), cb.get(nm, 0)) < n * 0.10:
            continue
        p(f"    {nm:<30}{ca.get(nm, 0) / n:>15.2f}{cb.get(nm, 0) / n:>16.2f}"
          f"{(ca.get(nm, 0) - cb.get(nm, 0)) / n:>16.2f}")

    p("\n  WONDERS (per game)")
    a = [len(r["wonders_done"]) for r in A]
    b = [len(r["wonders_done"]) for r in B]
    p(f"    {'completed':<30}{fmt(a, 2):>16}{fmt(b, 2):>16}{paired(a, b, 2):>16}")
    a = [len(r["wonders_started"]) for r in A]
    b = [len(r["wonders_started"]) for r in B]
    p(f"    {'started':<30}{fmt(a, 2):>16}{fmt(b, 2):>16}{paired(a, b, 2):>16}")
    wa = {}
    for r in A:
        for _, nm in r["wonders_done"]:
            wa[nm] = wa.get(nm, 0) + 1
    wb = {}
    for r in B:
        for _, nm in r["wonders_done"]:
            wb[nm] = wb.get(nm, 0) + 1
    for nm in sorted(set(wa) | set(wb), key=lambda k: -wa.get(k, 0)):
        if max(wa.get(nm, 0), wb.get(nm, 0)) < n * 0.08:
            continue
        p(f"      {nm:<28}{wa.get(nm, 0) / n:>15.2f}{wb.get(nm, 0) / n:>16.2f}")
    p("")


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("dir")
    ap.add_argument("--only", default=None)
    a = ap.parse_args(argv)
    for f in sorted(glob.glob(os.path.join(a.dir, "*.json"))):
        if a.only and a.only not in f:
            continue
        report(f)


if __name__ == "__main__":
    main()
