#!/usr/bin/env python3
"""Compare the horizon-fix probe arm's trajectory against the live 4p arm's own
first-N-generation history, matched on GENERATION, not wall clock.

Both arms write `fullcheck_4p.jsonl` -- a 48-game duel against every pool member
every 10 generations.  The pool-weighted win rate and culture margin over that
file is the only trajectory metric both arms produce on the same schedule.

    python3 tools/probe_compare.py [PROBE_STATE] [LIVE_STATE]

Read-only.  Never writes to either state dir.
"""
import json
import os
import sys

PROBE = sys.argv[1] if len(sys.argv) > 1 else "experiments/probe_state_4p"
LIVE = sys.argv[2] if len(sys.argv) > 2 else "/Users/pt/tta-ai/experiments/league_state"


def load(state_dir):
    p = os.path.join(state_dir, "fullcheck_4p.jsonl")
    if not os.path.exists(p):
        return []
    return [json.loads(l) for l in open(p)]


def pooled(rec):
    """Pool-weight-averaged win rate and culture margin for one fullcheck.

    The standard error treats each opponent's `win_rate` as a binomial over its
    own n (48) and the opponents as independent, which is optimistic -- the same
    candidate vector plays all of them, so its own strength is a shared term
    that this does not price.  Read it as a floor on the error, not the error.
    """
    res = rec["results"]
    tw = sum(v["weight"] for v in res.values())
    win = sum(v["win_rate"] * v["weight"] for v in res.values()) / tw
    mar = sum(v["margin"] * v["weight"] for v in res.values()) / tw
    var = 0.0
    for v in res.values():
        p, n = v["win_rate"], max(1, v["n"])
        var += (v["weight"] / tw) ** 2 * p * (1 - p) / n
    return win, mar, var ** 0.5


def gens(state_dir):
    p = os.path.join(state_dir, "generations_4p.jsonl")
    if not os.path.exists(p):
        return []
    return [json.loads(l) for l in open(p)]


def main():
    pr, lv = load(PROBE), load(LIVE)
    pg, lg = gens(PROBE), gens(LIVE)
    print(f"probe  {PROBE}")
    print(f"  generations {len(pg)}  accepts {sum(1 for r in pg if r['accepted'])}"
          f"  wall {sum(r['secs'] for r in pg) / 3600:.2f}h"
          f"  {sum(r['secs'] for r in pg) / max(1, len(pg)):.0f}s/gen")
    print(f"live   {LIVE}")
    print(f"  generations {len(lg)}  accepts {sum(1 for r in lg if r['accepted'])}"
          f"  wall {sum(r['secs'] for r in lg) / 3600:.2f}h"
          f"  {sum(r['secs'] for r in lg) / max(1, len(lg)):.0f}s/gen")
    print()
    lvm = {r["gen"]: r for r in lv}
    print("  gen |     PROBE win  margin |      LIVE win  margin |   d(win)+/-se  d(marg)")
    print("  ----+-----------------------+-----------------------+----------------------")
    for r in pr:
        g = r["gen"]
        pw, pm, pse = pooled(r)
        if g in lvm:
            lw, lm, lse = pooled(lvm[g])
            dse = (pse ** 2 + lse ** 2) ** 0.5
            print(f"  {g:>3} | {pw:.3f}+/-{pse:.3f} {pm:>7.1f} | {lw:.3f}+/-{lse:.3f} {lm:>7.1f} |"
                  f"  {pw - lw:+.3f}+/-{dse:.3f} {pm - lm:+6.1f}")
        else:
            print(f"  {g:>3} | {pw:.3f}+/-{pse:.3f} {pm:>7.1f} |            -       - |"
                  f"        -           -")
    if not pr:
        print("  (probe has not reached its first fullcheck yet)")
    # per-opponent detail at the last matched generation
    matched = [r for r in pr if r["gen"] in lvm]
    if matched:
        r = matched[-1]
        g = r["gen"]
        print(f"\nper-opponent at matched gen {g} (n=48 each, so +/-0.12 on a win rate):")
        pres, lres = r["results"], lvm[g]["results"]
        print(f"  {'opponent':<26} {'probe':>7} {'live':>7} {'d':>7} | "
              f"{'probeM':>8} {'liveM':>8}")
        for k in sorted(set(pres) | set(lres)):
            a, b = pres.get(k), lres.get(k)
            if not a or not b:
                print(f"  {k:<26} {'-' if not a else a['win_rate']:>7} "
                      f"{'-' if not b else b['win_rate']:>7}   (only one arm)")
                continue
            print(f"  {k:<26} {a['win_rate']:>7.3f} {b['win_rate']:>7.3f} "
                  f"{a['win_rate'] - b['win_rate']:>+7.3f} | "
                  f"{a['margin']:>8.1f} {b['margin']:>8.1f}")


if __name__ == "__main__":
    main()
