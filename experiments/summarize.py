"""Summarize a hill-climbing run: progress, anchors, and which levers moved.

    python3 -m experiments.summarize            # all player counts
    python3 -m experiments.summarize --players 4 --top 20

Reads `experiments/champion_{K}p.json` and `experiments/generations_{K}p.jsonl`
and prints, per player count:

  * generations run / accepted, current sigma, wall time;
  * the anchor series (`vs_default` / `vs_greedy` measured every N gens);
  * the weights that drifted furthest from `DEFAULT_WEIGHTS`, which is the
    readable answer to "what strategic levers is the search favoring?".

Drift is reported both absolutely and as a ratio, and each weight is tagged
with its feature group so the output can be pasted into docs/HEURISTICS.md.
"""
from __future__ import annotations

import argparse
import json
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine.bots.weighted import DEFAULT_WEIGHTS  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))

GROUPS = {
    "economy": ("culture", "culture_rate", "science", "science_rate",
                "food_rate", "resource_rate", "food_stock", "resource_stock",
                "blue_free", "corruption_loss", "consumption", "pop_cost",
                "yellow_bank", "free_workers", "workers", "prod_workers",
                "urban_workers", "unit_workers"),
    "happiness": ("happy_margin", "discontent", "uprising"),
    "actions": ("civil_actions", "military_actions", "ca_left", "ma_left"),
    "military": ("strength", "strength_rel", "strength_deficit",
                 "strength_lead", "tactic_level", "colonies", "pacts"),
    "tech": ("tech_levels", "gov_level", "best_farm", "best_mine", "best_lab",
             "best_temple", "best_theater", "best_library", "best_arena",
             "best_unit", "num_techs", "special_techs"),
    "wonders": ("wonders", "wonder_progress", "wonder_remaining", "leader"),
    "cards": ("hand_civil", "hand_value", "hand_military", "hand_mil_value"),
    "rivals": ("rival_culture", "rival_mean_culture", "rival_culture_rate",
               "rival_science_rate", "rival_strength"),
    "search": ("end_turn_bias",),
}


def group_of(key):
    base = key
    for suf in ("_early", "_late"):
        if key.endswith(suf):
            base = key[: -len(suf)]
            break
    for g, keys in GROUPS.items():
        if base in keys:
            return g + ("/phase" if base != key else "")
    return "?"


def load(k):
    cpath = os.path.join(HERE, f"champion_{k}p.json")
    gpath = os.path.join(HERE, f"generations_{k}p.jsonl")
    champ = None
    if os.path.exists(cpath):
        with open(cpath) as fh:
            champ = json.load(fh)
    rows = []
    if os.path.exists(gpath):
        with open(gpath) as fh:
            for line in fh:
                line = line.strip()
                if not line:
                    continue
                try:
                    rows.append(json.loads(line))
                except ValueError:      # torn last line after a kill
                    pass
    return champ, rows


def report(k, top, out=print):
    champ, rows = load(k)
    if champ is None and not rows:
        out(f"== {k}p: no run on disk ==")
        return
    marks = [r for r in rows if r.get("event")]
    rows = [r for r in rows if not r.get("event")]
    acc = [r for r in rows if r.get("accepted")]
    anchors = [r for r in rows if "vs_default" in r]
    secs = sum(r.get("secs", 0) for r in rows)
    out(f"== {k}p ==")
    out(f"  generations: {len(rows)}  accepted: {len(acc)} "
        f"({(len(acc) / len(rows) * 100) if rows else 0:.0f}%)  "
        f"sigma: {rows[-1].get('sigma') if rows else '-'}  "
        f"wall: {secs / 3600:.2f}h")
    for m in marks:
        out(f"  ! after gen {m.get('gen')}: {m.get('note')} ({m.get('at')})")
    if rows:
        out(f"  first gen {rows[0].get('at')}   last gen {rows[-1].get('at')}")
    broken = sum(r.get("broken", 0) for r in rows)
    if broken:
        out(f"  generations with no playable games: {broken} "
            f"(engine was mid-edit)")
    if anchors:
        out("  anchors (champion as challenger, null = "
            f"{1.0 / k:.0%}):")
        for r in anchors[-8:]:
            ci_d = r.get("vs_default_ci")
            ci_g = r.get("vs_greedy_ci")
            out(f"    gen {r['gen']:>4}  vs_default {r['vs_default']:.1%}"
                + (f" +/-{ci_d:.1%}" if ci_d is not None else "")
                + f"  vs_greedy {r['vs_greedy']:.1%}"
                + (f" +/-{ci_g:.1%}" if ci_g is not None else "")
                + (f"  vs_random {r['vs_random']:.1%}"
                   if "vs_random" in r else "")
                + (f"  (n={r['anchor_games']})" if "anchor_games" in r else ""))

    # Which mutation operator actually pays.  `op` is only recorded by the
    # league-mode climber, so this block is silent for older runs.
    ops = {}
    for r in rows:
        for t in r.get("tried", ()):
            o = (t.get("op") or "").split(":")[0]
            if not o:
                continue
            ops.setdefault(o, [0, 0])[0] += 1
    for r in acc:
        o = (r.get("op") or "").split(":")[0]
        if o in ops:
            ops[o][1] += 1
    if ops:
        out("  mutation operators (tried -> accepted):")
        for o, (n, a) in sorted(ops.items(), key=lambda kv: -kv[1][1]):
            out(f"    {o:<10} {n:>4} tried  {a:>3} accepted  "
                f"{(a / n * 100) if n else 0:.0f}%")
    ldir = os.path.join(HERE, f"league_{k}p")
    if os.path.isdir(ldir):
        out(f"  league: {len([f for f in os.listdir(ldir) if f.endswith('.json')])}"
            " archived champions in the field")

    if champ is None:
        return
    w = champ["weights"]
    drift = []
    for key, base in DEFAULT_WEIGHTS.items():
        now = w.get(key, base)
        d = now - base
        if abs(d) < 1e-9:
            continue
        rel = abs(d) / max(0.15, abs(base))
        drift.append((rel, abs(d), key, base, now))
    drift.sort(reverse=True)
    out(f"  weights moved: {len(drift)}/{len(DEFAULT_WEIGHTS)}; "
        f"largest relative drifts:")
    out(f"    {'weight':<24} {'group':<14} {'default':>8} {'champ':>9} {'x':>7}")
    for rel, _, key, base, now in drift[:top]:
        out(f"    {key:<24} {group_of(key):<14} {base:>8.2f} {now:>9.3f} "
            f"{rel:>6.2f}x")

    # group-level summary: mean |relative drift| per group
    per = {}
    for rel, _, key, _, _ in drift:
        per.setdefault(group_of(key).split("/")[0], []).append(rel)
    out("  mean relative drift by group:")
    for g, xs in sorted(per.items(), key=lambda kv: -sum(kv[1]) / len(kv[1])):
        out(f"    {g:<12} {sum(xs) / len(xs):.2f}x over {len(xs)} weights")


def main(argv=None):
    ap = argparse.ArgumentParser(description="summarize hill-climbing runs")
    ap.add_argument("--players", type=int, default=0, choices=(0, 2, 3, 4),
                    help="0 = all")
    ap.add_argument("--top", type=int, default=15)
    args = ap.parse_args(argv)
    ks = (2, 3, 4) if args.players == 0 else (args.players,)
    for k in ks:
        report(k, args.top)
        print()


if __name__ == "__main__":
    main()
