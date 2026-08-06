"""Dump `engine/bots/weighted.py`'s rival-pricing answers on real self-play
states, for the Rust port's differential test (`rust/tests/weighted_rivals.rs`).

Exactly the same offline-oracle shape as `tools/dump_counting.py`/
`tools/dump_weighted_horizon.py` (read either script's own doc comment for
the full rationale): this script loads states already recorded by
`dump_fixtures.py` (`rust/tests/fixtures/*.jsonl`, one `GameState.to_dict()`
per ply) and asks the real Python `weighted` module every question the Rust
port (`rust/src/bots/weighted/rivals.rs`) answers, for every live player, on
a sample of those states. The Rust side loads the SAME fixture states via
`GameState::from_json` and compares its own answers against this dump byte
for byte.

`deferred_credit`'s two interesting branches (`offer_pact` mid-decision,
a live colonization auction) only fire on a state whose `pending` stack
happens to be a `pact_offer` choice or an auction -- a plain stride sample
would miss most of them (they are a small fraction of all plies: 25
`pact_offer` and 50 auction states out of ~4900 sampled plies across the 13
existing fixture files, measured 2026-08-05). So sampling here is stride PLUS
every such state, not stride alone.

Usage:

    python3.13 tools/dump_weighted_rivals.py \\
        --fixtures rust/tests/fixtures --out rust/tests/weighted_rivals_fixtures \\
        --stride 15
"""
from __future__ import annotations

import argparse
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from engine.state import GameState                     # noqa: E402
from engine.bots import weighted as W                    # noqa: E402

_DUMP_JSON_KW = dict(sort_keys=True, separators=(",", ":"))

#: A representative sample of `feature_marginal` keys: a plain weight
#: (`colonies`), all four `PHASE_KEYS` members, all four `RATE_KEYS` members,
#: `strength` (must agree with `strength_marginal` -- the delegation this
#: function's own doc comment calls out), and one more plain weight
#: (`civil_actions`) that a card-pricing site actually calls
#: `feature_marginal` with (`action_value`'s `freeCivilAction` branch).
_FEATURE_KEYS = (
    "colonies", "workers", "strength_rel", "tech_levels", "hand_value",
    "culture_rate", "science_rate", "food_rate", "resource_rate",
    "strength", "civil_actions", "happy_margin",
)


def _pending_is_interesting(state):
    if not state.pending:
        return False
    top = state.pending[-1]
    if top.get("kind") == "choice" and top.get("tag") == "pact_offer":
        return True
    return top.get("kind") == "auction"


def _deferred_credit_json(state, idx):
    offers, committed, bid, blocks_attack, gains, partner_gains = W.deferred_credit(state, idx)
    return {
        "pact_offers": offers,
        "auction_committed": committed,
        "auction_bid": bid,
        "blocks_attack": blocks_attack,
        "gains": dict(gains),
        "partner_gains": {str(pi): dict(pg) for pi, pg in partner_gains.items()},
    }


def _one_player(state, idx, w):
    ctx = W.rival_context(state, idx)
    return {
        "rival_board": W.rival_board(state, idx),
        "deferred_credit": _deferred_credit_json(state, idx),
        "rival_strength": W.rival_strength(state, idx),
        "rival_context": {
            "rival_culture_rate": ctx["rival_culture_rate"],
            "rival_science_rate": ctx["rival_science_rate"],
            "rival_strength": ctx["rival_strength"],
            "rival_rates": {str(pi): list(rates) for pi, rates in ctx["rival_rates"].items()},
        },
        "strength_marginal": W.strength_marginal(state, idx, w),
        "feature_marginal": {k: W.feature_marginal(k, state, idx, w) for k in _FEATURE_KEYS},
    }


def _one_state(state, w):
    n = len(state.players)
    per_player = {}
    for idx in range(n):
        if state.players[idx].resigned:
            continue
        per_player[str(idx)] = _one_player(state, idx, w)
    return {"root_row": list(W.root_row_budget(state)), "players": per_player}


def dump_file(path, out_path, stride, w):
    with open(path) as f:
        lines = f.readlines()
    plies = []
    for line in lines:
        rec = json.loads(line)
        if "ply" in rec and rec.get("state") is not None:
            plies.append(rec)

    sampled = list(plies[::stride])
    if plies and (not sampled or sampled[-1] is not plies[-1]):
        sampled.append(plies[-1])
    sampled_ids = {id(r) for r in sampled}
    for rec in plies:
        if id(rec) in sampled_ids:
            continue
        if _pending_is_interesting(GameState.from_dict(rec["state"])):
            sampled.append(rec)
            sampled_ids.add(id(rec))
    sampled.sort(key=lambda r: r["ply"])

    records = []
    for rec in sampled:
        state = GameState.from_dict(rec["state"])
        records.append({"ply": rec["ply"], **_one_state(state, w)})

    with open(out_path, "w") as f:
        for r in records:
            f.write(json.dumps(r, **_DUMP_JSON_KW) + "\n")
    return len(records)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--fixtures", default="rust/tests/fixtures")
    ap.add_argument("--out", default="rust/tests/weighted_rivals_fixtures")
    ap.add_argument("--stride", type=int, default=15)
    args = ap.parse_args(argv)

    os.makedirs(args.out, exist_ok=True)
    w = dict(W.DEFAULT_WEIGHTS)

    total = 0
    for name in sorted(os.listdir(args.fixtures)):
        if not name.endswith(".jsonl"):
            continue
        src = os.path.join(args.fixtures, name)
        dst = os.path.join(args.out, name)
        n = dump_file(src, dst, args.stride, w)
        total += n
        print(f"{name}: {n} sampled states -> {dst}")
    print(f"total: {total} states")


if __name__ == "__main__":
    main()
