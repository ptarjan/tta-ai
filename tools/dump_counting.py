"""Dump `engine/bots/counting.py`'s answers on real game states, for the
Rust port's differential test (`rust/tests/counting.rs`).

Exactly the same offline-oracle shape as `tools/dump_board_yields.py` (read
that script's own doc comment for the full rationale): this script loads
states already recorded by `dump_fixtures.py` (`rust/tests/fixtures/*.jsonl`,
one `GameState.to_dict()` per ply) and asks the real Python `counting` module
every question the Rust port answers, for every live player, on a sample of
those states. The Rust side (`rust/tests/counting.rs`) loads the SAME fixture
states via `GameState::from_json` and compares its own answers against this
dump byte for byte.

`GameState.from_dict` reconstructs `state.seeded_by` from the fixture's own
recorded dict (`dataclasses.asdict` on the real dataclass field) -- unlike
`board_yields.py`, `counting.event_pool` reads that field, so this dump is
only meaningful once the Rust side actually loads it too (`rust/src/
fixtures.rs::seeded_by_field`, added alongside this script and `counting.rs`
-- see `rust/src/bots/counting.rs`'s top doc comment for why that field had
no home in `state.rs` before this port).

Usage:

    python3.13 tools/dump_counting.py \\
        --fixtures rust/tests/fixtures --out rust/tests/counting_fixtures \\
        --stride 15
"""
from __future__ import annotations

import argparse
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from engine.state import GameState                     # noqa: E402
from engine.bots import counting as CT                  # noqa: E402

_DUMP_JSON_KW = dict(sort_keys=True, separators=(",", ":"))


def _one_player(state, idx):
    outlook = CT.civil_outlook(state, idx)
    unknown, p_in_pile = CT.event_pool(state, idx)
    return {
        "live_ages": list(CT.live_ages(state)),
        "civil_outlook": dict(outlook),
        "event_pool_unknown": dict(unknown),
        "event_pool_p": p_in_pile,
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
    ap.add_argument("--out", default="rust/tests/counting_fixtures")
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
