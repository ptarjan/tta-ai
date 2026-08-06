"""Dump `engine/bots/neural_encode.py::encode()`'s full flat vector on real
self-play states, for the Rust port's differential test
(`rust/tests/neural.rs`).

Same offline-oracle shape as `tools/dump_weighted_features.py` (read that
script's own doc comment for the full rationale): this script loads states
already recorded by `dump_fixtures.py` (`rust/tests/fixtures/*.jsonl`, one
`GameState.to_dict()` per ply) and asks the real Python `neural_encode.encode()`
for its ANSWER, for every live player, on a sample of those states. The Rust
side loads the SAME fixture states via `GameState::from_json` and compares its
own answer against this dump byte for byte, coordinate by coordinate.

Unlike `dump_weighted_features.py`'s named `WeightKey` coordinates,
`encode()`'s output is POSITIONAL (a flat `list[float]`, no names) -- so this
dump is a flat JSON array per player, and the Rust side compares index by
index. A length mismatch is caught first (and would make every subsequent
index compare against the wrong coordinate), so the Rust test checks the
LENGTH before walking the array.

`neural_encode.py` has NO torch/numpy dependency on purpose (its own module
doc comment), so unlike `neural_net.py` this script runs on the same
torch-less machine as everything else in this repo.

Usage:

    python3.13 tools/dump_neural.py \\
        --fixtures rust/tests/fixtures --out rust/tests/neural_fixtures \\
        --stride 25
"""
from __future__ import annotations

import argparse
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from engine.state import GameState                     # noqa: E402
from engine.bots import neural_encode as NE             # noqa: E402

_DUMP_JSON_KW = dict(sort_keys=True, separators=(",", ":"))


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
            per_player[str(idx)] = NE.encode(state, idx)
        records.append({"ply": rec["ply"], "players": per_player})

    with open(out_path, "w") as f:
        for r in records:
            f.write(json.dumps(r, **_DUMP_JSON_KW) + "\n")
    return len(records)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--fixtures", default="rust/tests/fixtures")
    ap.add_argument("--out", default="rust/tests/neural_fixtures")
    ap.add_argument("--stride", type=int, default=25)
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
