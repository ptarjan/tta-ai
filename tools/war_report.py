r"""Fold `tools/war_census.py` JSONL into the tables for
docs/WAR_RATE_CENSUS.md.  Every rate below prints its own denominator.

    python3.13 -m tools.war_report /tmp/war_2p.jsonl /tmp/war_3p.jsonl
"""
from __future__ import annotations

import argparse
import collections
import json
import sys

WAR_KINDS = ("war", "aggression")
TACTIC_KINDS = ("copy_tactic", "play_tactic")
ROUND_BUCKETS = (("early 1-6", range(1, 7)), ("mid 7-13", range(7, 14)),
                  ("late 14+", range(14, 100)))


def load(paths):
    out = []
    for p in paths:
        with open(p) as f:
            for line in f:
                line = line.strip()
                if line:
                    out.append(json.loads(line))
    return out


def round_bucket(r):
    for name, rg in ROUND_BUCKETS:
        if r in rg:
            return name
    return "?"


def section_availability(recs, players):
    rs = [r for r in recs if r["players"] == players]
    with_war = [r for r in rs if r["has_war_available"]]
    print(f"\n=== {players}p: war/aggression-available decisions ===")
    print(f"decisions with a war/aggression move OFFERED: {len(with_war)}  "
          f"(denominator for everything in this section)")
    if not with_war:
        return
    chosen_kind = collections.Counter(r["chosen"]["kind"] for r in with_war)
    for k, n in chosen_kind.most_common():
        print(f"  chosen == {k:14s} {n:6d}  ({100*n/len(with_war):.1f}%)")
    by_age = collections.Counter(r["age"] for r in with_war)
    chosen_war_by_age = collections.Counter(
        r["age"] for r in with_war if r["chosen"]["kind"] in WAR_KINDS)
    print("  by age: (war-chosen / offered)")
    for age in ("A", "I", "II", "III", "IV"):
        off = by_age.get(age, 0)
        ch = chosen_war_by_age.get(age, 0)
        if off:
            print(f"    {age:4s} {ch:5d} / {off:5d}  ({100*ch/off:.1f}%)")


def section_margin_horizon(recs, players):
    rs = [r for r in recs if r["players"] == players
          and r["chosen"]["kind"] in WAR_KINDS and r["margin"] is not None]
    print(f"\n=== {players}p: margin BY WHICH WAR/AGGRESSION BEAT ITS "
          f"RUNNER-UP (horizon-blindness check) ===")
    print(f"decisions where war/aggression WON: {len(rs)}  "
          f"(denominator for the rows below)")
    if not rs:
        return
    overall = sum(r["margin"] for r in rs) / len(rs)
    print(f"  overall mean margin: {overall:.2f}")
    print("  by age:")
    by_age = collections.defaultdict(list)
    for r in rs:
        by_age[r["age"]].append(r["margin"])
    for age in ("A", "I", "II", "III", "IV"):
        vs = by_age.get(age)
        if vs:
            print(f"    {age:4s} n={len(vs):5d}  mean margin={sum(vs)/len(vs):8.2f}")
    print("  by round-third:")
    by_rb = collections.defaultdict(list)
    for r in rs:
        by_rb[round_bucket(r["round"])].append(r["margin"])
    for name, _ in ROUND_BUCKETS:
        vs = by_rb.get(name)
        if vs:
            print(f"    {name:10s} n={len(vs):5d}  mean margin={sum(vs)/len(vs):8.2f}")


def section_suppression(recs, players):
    rs = [r for r in recs if r["players"] == players and r["has_war_available"]]
    print(f"\n=== {players}p: the row AT the war/aggression decision, priced "
          f"the way row_pressure prices it ===")
    print("NOTE: war/aggression is decided in the politics sub-phase, whose "
          "sibling moves are pol_pass/offer_pact/cancel_pact/prepare_event -- "
          "never `take`, which is a separate civil-action decision later the "
          "same turn (engine/actions.py:288-342 vs :401). So this is NOT "
          "'the take move that lost to war' -- it is the row's own opportunity "
          "cost as row_pressure would price it, gated the same way "
          "(actions._can_take_gated).")
    rows = [(r, c) for r in rs for c in r.get("row_alternatives", [])]
    print(f"row cards seen, gated legal-to-take: {len(rows)}  "
          f"(across {len(rs)} war/aggression-eligible decisions; denominator "
          f"below)")
    if not rows:
        return
    n_supp = sum(1 for _, c in rows if c["suppressed"])
    print(f"  suppressed (card_potential <= 0, invisible to row_pressure): "
          f"{n_supp}  ({100*n_supp/len(rows):.1f}%)")
    print(f"  merely outranked (priced > 0, so counts against declining to "
          f"take it): {len(rows)-n_supp}  ({100*(len(rows)-n_supp)/len(rows):.1f}%)")
    # rate-building skew
    rate_supp = sum(1 for _, c in rows if c["suppressed"] and c["rate_building"])
    n_rate = sum(1 for _, c in rows if c["rate_building"])
    n_onesh = len(rows) - n_rate
    supp_rate_of_rate = rate_supp / n_rate if n_rate else float("nan")
    supp_rate_of_onesh = (n_supp - rate_supp) / n_onesh if n_onesh else float("nan")
    print(f"  rate-building cards (farm/mine/lab/temple/library/arena/theater) "
          f"in the row: {n_rate}, of which suppressed: {rate_supp} "
          f"({100*supp_rate_of_rate:.1f}%)")
    print(f"  everything else (one-shot/other) in the row: {n_onesh}, of "
          f"which suppressed: {n_supp-rate_supp} ({100*supp_rate_of_onesh:.1f}%)")
    # decisions where EVERY legal row alternative was suppressed -- the
    # sharpest form of (B): war chosen against a row that offered nothing
    # visible at all.
    fully = [r for r in rs if r.get("row_alternatives")
             and all(c["suppressed"] for c in r["row_alternatives"])]
    print(f"  decisions where ALL gated row cards were suppressed: "
          f"{len(fully)} / {len(rs)} ({100*len(fully)/len(rs):.1f}% of "
          f"war-eligible decisions)")


def section_copy_tactic(recs, players):
    rs = [r for r in recs if r["players"] == players and r["has_tactic_available"]]
    print(f"\n=== {players}p: copy_tactic vs play_tactic ===")
    print(f"decisions with a copy_tactic/play_tactic move OFFERED: {len(rs)}  "
          f"(denominator below)")
    if not rs:
        return
    chosen_kind = collections.Counter(r["chosen"]["kind"] for r in rs)
    for k in ("copy_tactic", "play_tactic"):
        n = chosen_kind.get(k, 0)
        print(f"  chosen == {k:14s} {n:6d}  ({100*n/len(rs):.1f}%)")
    ratio_num = chosen_kind.get("copy_tactic", 0)
    ratio_den = chosen_kind.get("play_tactic", 0)
    if ratio_den:
        print(f"  copy_tactic : play_tactic ratio = {ratio_num/ratio_den:.1f} : 1"
              f"  (n={ratio_num}/{ratio_den})")
    # feature attribution, chosen == copy_tactic vs its own runner-up
    diffs = collections.defaultdict(list)
    n_with_diff = 0
    for r in rs:
        if r["chosen"]["kind"] != "copy_tactic":
            continue
        fd = r.get("feature_diff_chosen_vs_runnerup")
        if not fd:
            continue
        n_with_diff += 1
        for k, v in fd.items():
            diffs[k].append(v["weighted_diff"])
    print(f"  copy_tactic decisions with a usable feature diff "
          f"(QuiescentBot one-ply only): {n_with_diff}")
    if diffs:
        print("  mean weighted_diff by feature (chosen - runner-up; what the "
              "bot thinks copy_tactic buys):")
        rows = sorted(diffs.items(), key=lambda kv: -abs(sum(kv[1]) / len(kv[1])))
        for k, vs in rows[:15]:
            print(f"    {k:24s} n={len(vs):5d}  mean={sum(vs)/len(vs):8.4f}")


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("paths", nargs="+")
    a = ap.parse_args(argv)
    recs = load(a.paths)
    players_seen = sorted({r["players"] for r in recs})
    print(f"loaded {len(recs)} decision records from {len(a.paths)} file(s); "
          f"player counts present: {players_seen}")
    for players in players_seen:
        section_availability(recs, players)
        section_margin_horizon(recs, players)
        section_suppression(recs, players)
        section_copy_tactic(recs, players)
    return 0


if __name__ == "__main__":
    sys.exit(main())
