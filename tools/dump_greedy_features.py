"""Dump `engine/bots/__init__.py::features()`/`evaluate()`'s answers on real
self-play states, for the Rust port's differential test
(`rust/tests/greedy_features.rs`).

Exactly the same offline-oracle shape as `tools/dump_weighted_features.py`
(read that script's own doc comment for the full rationale): this loads
states already recorded by `dump_fixtures.py` (`rust/tests/fixtures/*.jsonl`,
one `GameState.to_dict()` per ply) and asks the real Python
`engine.bots.features()`/`engine.bots.evaluate()` for their ANSWER, for every
live player, on a sample of those states. The Rust side loads the SAME
fixture states via `GameState::from_json` and compares its own answer against
this dump.

Unlike `dump_weighted_features.py`, this dumps BOTH `features()` (the 19
`GreedyKey` coordinates) AND `evaluate()` (the scalar `GreedyBot` score) --
`GreedyBot`'s evaluator is small enough that checking the scalar too costs
nothing and catches a bug in the weighted-sum/rival-culture assembly that a
coordinate-only check could miss.

Usage:

    python3.13 tools/dump_greedy_features.py \\
        --fixtures rust/tests/fixtures --out rust/tests/greedy_features_fixtures \\
        --stride 15
"""
from __future__ import annotations

import argparse
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from engine.state import GameState                     # noqa: E402
from engine import bots as B                            # noqa: E402

_DUMP_JSON_KW = dict(sort_keys=True, separators=(",", ":"))


def _one_player(state, idx):
    return {"features": B.features(state, state.players[idx]), "evaluate": B.evaluate(state, idx)}


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
    ap.add_argument("--out", default="rust/tests/greedy_features_fixtures")
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
