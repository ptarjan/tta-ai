"""Aggregate analysis/wasted_actions.py event dumps into the numbers we report."""
from __future__ import annotations

import json
import sys
from collections import Counter, defaultdict

AGES = ["A", "I", "II", "III", "IV"]


def mean(xs):
    xs = [x for x in xs if x is not None]
    return sum(xs) / len(xs) if xs else 0.0


def summarize(path):
    d = json.load(open(path))
    ev = [e for e in d["events"] if "error" not in e]
    n = len(ev)
    out = {"players": d["players"], "games": d["games"], "events": n}

    # 1. was there ANY legal civil-action move at all?
    none_legal = [e for e in ev if e["n_alts"] == 0]
    some_legal = [e for e in ev if e["n_alts"] > 0]
    out["no_legal_ca_move"] = len(none_legal)
    out["no_legal_ca_move_share"] = round(len(none_legal) / n, 4)
    out["ca_wasted_total"] = sum(e["ca_left"] for e in ev)
    out["ca_wasted_with_no_option"] = sum(e["ca_left"] for e in none_legal)

    # 2. of the ones with an option, how good was the best declined move?
    pos = [e for e in some_legal if e["best_alt"]["delta"] > 0]
    out["declined_a_move"] = len(some_legal)
    out["declined_an_eval_POSITIVE_move"] = len(pos)
    out["declined_positive_share_of_all"] = round(len(pos) / n, 4)

    # 3. the horizon artifact: end_turn's child has run a production phase
    out["mean_flattery"] = round(mean([e["flattery"] for e in ev]), 3)
    out["mean_bias"] = round(mean([e["bias"] for e in ev]), 3)
    out["mean_best_alt_delta"] = round(
        mean([e["best_alt"]["delta"] for e in some_legal]), 3)
    # would the best alternative have been chosen if end_turn were scored on
    # the UNMOVED board (base) instead of its post-production child?
    flip = [e for e in some_legal
            if e["best_alt"]["delta"] > e["bias"]]
    out["would_flip_if_flattery_removed"] = len(flip)
    out["would_flip_share_of_all"] = round(len(flip) / n, 4)
    # and the strictly conservative version: only moves the eval already likes
    out["would_flip_and_positive"] = len(
        [e for e in pos if e["best_alt"]["delta"] > e["bias"]])

    # 4. by age
    by_age = {}
    for a in AGES:
        sub = [e for e in ev if e["age"] == a]
        if not sub:
            continue
        s2 = [e for e in sub if e["n_alts"] > 0]
        by_age[a] = {
            "turns": len(sub),
            "ca_wasted": sum(e["ca_left"] for e in sub),
            "no_option_share": round(
                sum(1 for e in sub if e["n_alts"] == 0) / len(sub), 3),
            "mean_flattery": round(mean([e["flattery"] for e in sub]), 2),
            "mean_best_alt_delta": round(
                mean([e["best_alt"]["delta"] for e in s2]), 3) if s2 else None,
            "mean_alts_available": round(mean([e["n_alts"] for e in sub]), 1),
            "take_legal_share": round(
                sum(1 for e in sub if e["take_legal"] > 0) / len(sub), 3),
            "hand_full_share": round(
                sum(1 for e in sub if e["hand_civil"] >= e["hand_limit"])
                / len(sub), 3),
        }
    out["by_age"] = by_age

    # 5. what kind of move was declined, and what did the eval say about it
    kinds = Counter()
    deltas = defaultdict(list)
    for e in some_legal:
        for a in e["alts"]:
            kinds[a["kind"]] += 1
            deltas[a["kind"]].append(a["delta"])
    out["declined_kinds"] = {
        k: {"n": v, "mean_delta": round(mean(deltas[k]), 3)}
        for k, v in kinds.most_common()}

    # 6. yellow (action) cards specifically
    y_hand = [e for e in ev if e["yellow_in_hand"] > 0]
    y_play = [e for e in ev if e["yellow_playable"] > 0]
    ydelta = [a["delta"] for e in some_legal for a in e["alts"]
              if a["move"][0] == "play_action"]
    tdelta_yellow = [a["delta"] for e in some_legal for a in e["alts"]
                     if a["move"][0] == "take" and a["ctype"] == "action"]
    tdelta_other = [a["delta"] for e in some_legal for a in e["alts"]
                    if a["move"][0] == "take" and a["ctype"] != "action"]
    out["yellow"] = {
        "turns_with_yellow_in_hand": len(y_hand),
        "turns_with_yellow_playable": len(y_play),
        "share_of_wasted_turns_with_playable_yellow": round(len(y_play) / n, 4),
        "mean_delta_play_action": round(mean(ydelta), 3),
        "n_play_action_declined": len(ydelta),
        "mean_delta_take_yellow": round(mean(tdelta_yellow), 3),
        "mean_delta_take_nonyellow": round(mean(tdelta_other), 3),
    }
    return out


if __name__ == "__main__":
    res = [summarize(p) for p in sys.argv[1:]]
    print(json.dumps(res, indent=1))
