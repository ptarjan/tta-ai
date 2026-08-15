"""Does a QUIESCENT-trained weight vector transfer to PlanBot?

`docs/TRAINING_RUN.md` names this test and never ran it.  The league trains
under `quiescent:levels=1` because PlanBot is 49-66x too expensive to train,
but PlanBot is the policy we would ship.  `docs/TWOP_PROFILE.md` 8 showed the
2p champion's whole *style* is produced by the search rather than the weights
(0.00 wars at 1 ply, 1.98/game under quiescence), so evaluation and search are
strongly coupled and a vector tuned under one policy need not transfer.

This driver plays two FROZEN vectors head to head under a chosen policy:

    Q = quiescent-trained   experiments/hall_of_fame/preinfo_2p_gen00188.json
    P = 1-ply-trained       experiments/archive_preplan/league_state_1ply_.../champion_2p.json

Both are 82-key vectors over the identical feature set, so `load_weights`
fills the same 7 post-hoc features from `DEFAULT_WEIGHTS` for each.

    python3 tools/transfer_ab.py h2h --policy plan:width=8 --deals 60

`h2h` runs one `arena.duel` of Q-under-policy against P-under-policy.  Because
`duel` rotates the challenger through every seat on the same game seed, games
come in K-tuples that share a deal; the reported n and SE are over DEALS, the
independent unit, not over games.

    python3 tools/transfer_ab.py vsfield --policy plan:width=8 --deals 40

`vsfield` runs each vector separately against the same opponent (default
`book`) on the same seeds and reports the PAIRED difference deal by deal --
the second, independent way of asking the same question.

Nothing here touches `engine/`.  Raw per-game records go to --out as JSON.
"""
from __future__ import annotations

import argparse
import json
import math
import os
import sys
import time

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from experiments import arena  # noqa: E402
from experiments import bookmatch  # noqa: E402,F401  (installs `book` specs)

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
#: Both vector families live only in the MAIN checkout -- `hall_of_fame/` and
#: `archive_preplan/` are untracked (the trainer's output is never committed),
#: so a worktree does not have them.  Absolute, as `tools/champ_vs_drift.py`
#: does for the same reason.
MAIN = "/Users/pt/tta-ai"
QUIESCENT_TRAINED = {
    2: f"{MAIN}/experiments/hall_of_fame/preinfo_2p_gen00188.json",
    3: f"{MAIN}/experiments/hall_of_fame/preinfo_3p_gen00205.json",
    4: f"{MAIN}/experiments/hall_of_fame/preinfo_4p_gen00102.json",
}
ONEPLY_TRAINED = {
    k: f"{MAIN}/experiments/archive_preplan/league_state_1ply_20260726/"
       f"champion_{k}p.json" for k in (2, 3, 4)
}


def policy_spec(policy, path):
    """`policy` is 'weighted', 'quiesce:levels=1' or 'plan:width=8'."""
    if policy in ("weighted", "1ply", ""):
        return arena.load_spec(path)
    kind, _, opts = policy.partition(":")
    return arena.load_spec(f"{kind}:{path}" + (f",{opts}" if opts else ""))


def mean_se(xs):
    xs = [x for x in xs if x is not None]
    n = len(xs)
    if n < 2:
        return (sum(xs) / max(1, n) if xs else 0.0), 0.0, n
    m = sum(xs) / n
    var = sum((x - m) ** 2 for x in xs) / (n - 1)
    return m, math.sqrt(var / n), n


def by_deal(per_game, players):
    """Collapse a task-ordered per-game series into one value per deal.

    `arena.duel` emits games in blocks of `players` that share a game seed and
    rotate the challenger's seat, so the block is one deal played from every
    seat.  A block with any engine error is dropped whole, which keeps the
    seat balance exact.
    """
    out = []
    for i in range(0, len(per_game) - players + 1, players):
        block = per_game[i:i + players]
        if any(v is None for v in block):
            out.append(None)
            continue
        out.append(sum(block) / len(block))
    return out


def paired(a, b):
    return [None if (x is None or y is None) else x - y for x, y in zip(a, b)]


def run_duel(a_spec, b_spec, players, deals, seed0, workers, cap):
    t0 = time.time()
    res = arena.duel(a_spec, b_spec, players, deals * players,
                     seed0=seed0, workers=workers, move_cap=cap)
    res["wall_s"] = time.time() - t0
    return res


def report(tag, per_deal_margin, per_deal_share, players, null=None):
    """`null` is the win-share expected under no difference: 1/K for a duel,
    0.0 for a PAIRED difference of two duels against a common opponent."""
    if null is None:
        null = 1.0 / players
    mm, ms, n = mean_se(per_deal_margin)
    sm, ss, _ = mean_se(per_deal_share)
    print(f"  {tag:<34} n={n:>4} deals   "
          f"margin {mm:+7.2f} +/- {ms:4.2f}   "
          f"win {sm:+.3f} +/- {ss:.3f}  (null {null:+.3f})")
    return {"tag": tag, "deals": n, "margin": mm, "margin_se": ms,
            "win": sm, "win_se": ss, "null": null}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("mode", choices=("h2h", "vsfield"))
    ap.add_argument("--players", type=int, default=2, choices=(2, 3, 4))
    ap.add_argument("--policy", default="plan:width=8")
    ap.add_argument("--policy-b", default="",
                    help="policy for B; default same as --policy. Setting it "
                         "makes the duel a SEARCH comparison (same vector, two "
                         "policies) rather than a vector comparison.")
    ap.add_argument("--deals", type=int, default=50)
    ap.add_argument("--seed", type=int, default=90210)
    ap.add_argument("--workers", type=int, default=2)
    ap.add_argument("--cap", type=int, default=20000)
    ap.add_argument("--opponent", default="book")
    ap.add_argument("--quiescent-vector", "--a", dest="quiescent_vector",
                    default="", help="vector A; default the quiescent-trained one")
    ap.add_argument("--oneply-vector", "--b", dest="oneply_vector",
                    default="", help="vector B; default the 1-ply-trained one")
    ap.add_argument("--label-a", default="Q (quiescent-trained)")
    ap.add_argument("--label-b", default="P (1-ply-trained)")
    ap.add_argument("--out", default="")
    args = ap.parse_args()

    K = args.players
    qpath = args.quiescent_vector or QUIESCENT_TRAINED[K]
    ppath = args.oneply_vector or ONEPLY_TRAINED[K]
    q = policy_spec(args.policy, qpath)
    p = policy_spec(args.policy_b or args.policy, ppath)
    print(f"policy={args.policy}/{args.policy_b or args.policy}  players={K}  deals={args.deals} "
          f"({args.deals * K} games)  workers={args.workers}")
    print(f"  A = {args.label_a}: {qpath}")
    print(f"  B = {args.label_b}: {ppath}")

    rows, raw = [], {"args": vars(args)}
    if args.mode == "h2h":
        res = run_duel(q, p, K, args.deals, args.seed, args.workers, args.cap)
        raw["h2h"] = res
        rows.append(report(f"{args.label_a} vs {args.label_b}",
                           by_deal(res["per_game_margin"], K),
                           by_deal(res["per_game"], K), K))
        print(f"  [{res['errors']} engine errors, {res['wall_s']:.0f}s wall]")
    else:
        opp = bookmatch.load_spec(args.opponent)
        rq = run_duel(q, opp, K, args.deals, args.seed, args.workers, args.cap)
        rp = run_duel(p, opp, K, args.deals, args.seed, args.workers, args.cap)
        raw["q_vs_field"], raw["p_vs_field"] = rq, rp
        qm = by_deal(rq["per_game_margin"], K)
        pm = by_deal(rp["per_game_margin"], K)
        qs = by_deal(rq["per_game"], K)
        ps = by_deal(rp["per_game"], K)
        rows.append(report(f"{args.label_a} vs {args.opponent}", qm, qs, K))
        rows.append(report(f"{args.label_b} vs {args.opponent}", pm, ps, K))
        rows.append(report("paired A - B", paired(qm, pm), paired(qs, ps), K,
                           null=0.0))
        print(f"  [{rq['errors'] + rp['errors']} engine errors, "
              f"{rq['wall_s'] + rp['wall_s']:.0f}s wall]")

    raw["rows"] = rows
    if args.out:
        with open(args.out, "w") as fh:
            json.dump(raw, fh)
        print(f"  raw -> {args.out}")


if __name__ == "__main__":
    main()
