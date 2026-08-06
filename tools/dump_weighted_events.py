"""Dump `engine/bots/weighted.py` lines 159-591's answers on real game
states, for the Rust port's differential test (`rust/tests/
weighted_events.rs`).

Exactly the same offline-oracle shape as `tools/dump_counting.py`/
`tools/dump_weighted_horizon.py` (read either script's own doc comment for
the full rationale): this script loads states already recorded by
`dump_fixtures.py` (`rust/tests/fixtures/*.jsonl`, one `GameState.to_dict()`
per ply) and asks the real Python `weighted` module every question the Rust
port (`rust/src/bots/weighted/events.rs`) answers, for every live player, on
a sample of those states. The Rust side loads the SAME fixture states via
`GameState::from_json` and compares its own answers against this dump byte
for byte.

Every function dumped here is `(state, idx)` (or `(state, idx, w)`), exactly
like `counting.py`'s functions -- so this follows `dump_counting.py`'s
per-player record shape rather than `dump_weighted_horizon.py`'s
one-record-per-ply shape.

`ctx`/the pre-computed event pool is deliberately left `None` on both sides
(Rust's `pool: Option<&EventPool>` is also passed `None` in the Rust test) --
`_scoring_weights`/`event_scoring_margin` then each compute their own
`counting.event_pool` internally, which is already pinned byte-for-byte by
`rust/tests/counting.rs`'s own differential test, so there is nothing extra
to plumb through here.

`my_event_threat` needs a weight vector; `DEFAULT_WEIGHTS` is used, since
every `_EVENT_YIELD`-reachable feature (`science`/`culture`/`food_stock`/
`resource_stock`/`blue_free`/`free_workers`/`hand_military`) defaults
nonzero, so a real dump exercises real pricing rather than a vector of
zeros.

Usage:

    python3.13 tools/dump_weighted_events.py \\
        --fixtures rust/tests/fixtures --out rust/tests/weighted_events_fixtures \\
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


def _one_player(state, idx):
    w = W.DEFAULT_WEIGHTS
    scoring_weights = W._scoring_weights(state, idx, None)
    lead, weakness = W.attack_target_terms(state, idx)
    return {
        "my_seeds": sorted(W.my_seeds(state, idx)),
        "my_seeded_pending": W.my_seeded_pending(state, idx),
        "scoring_weights": dict(scoring_weights),
        "event_scoring_margin": W.event_scoring_margin(state, idx, None),
        "my_event_threat": W.my_event_threat(state, idx, w),
        "attack_targets": W.attack_targets(state, idx),
        "attack_target_lead": lead,
        "attack_target_weakness": weakness,
        "pact_partner_lead": W.pact_partner_lead(state, idx),
    }


def dump_file(path, out_path, stride):
    with open(path) as f:
        lines = f.readlines()
    plies = []
    for line in lines:
        rec = json.loads(line)
        if "ply" in rec and rec.get("state") is not None:
            plies.append(rec)
    sampled = plies[::stride]
    if plies and (not sampled or sampled[-1] is not plies[-1]):
        sampled.append(plies[-1])

    records = []
    for rec in sampled:
        state = GameState.from_dict(rec["state"])
        n = len(state.players)
        per_player = {}
        for idx in range(n):
            if state.players[idx].resigned:
                continue
            per_player[str(idx)] = _one_player(state, idx)
        records.append({"ply": rec["ply"], "players": per_player})

    with open(out_path, "w") as f:
        for r in records:
            f.write(json.dumps(r, **_DUMP_JSON_KW) + "\n")
    return len(records)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--fixtures", default="rust/tests/fixtures")
    ap.add_argument("--out", default="rust/tests/weighted_events_fixtures")
    ap.add_argument("--stride", type=int, default=15)
    args = ap.parse_args(argv)

    os.makedirs(args.out, exist_ok=True)
    total = 0
    for name in sorted(os.listdir(args.fixtures)):
        if not name.endswith(".jsonl"):
            continue
        src = os.path.join(args.fixtures, name)
        dst = os.path.join(args.out, name)
        n = dump_file(src, dst, args.stride)
        total += n
        print(f"{name}: {n} sampled states -> {dst}")
    print(f"total: {total} states")


if __name__ == "__main__":
    main()
