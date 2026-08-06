"""Dump `engine/bots/weighted.py::features()`'s full coordinate vector on
real self-play states, for the Rust port's differential test
(`rust/tests/weighted_features.rs`).

Exactly the same offline-oracle shape as `tools/dump_weighted_events.py`/
`tools/dump_weighted_rivals.py` (read either script's own doc comment for
the full rationale): this script loads states already recorded by
`dump_fixtures.py` (`rust/tests/fixtures/*.jsonl`, one `GameState.to_dict()`
per ply) and asks the real Python `weighted.features()` for its ANSWER, for
every live player, on a sample of those states. The Rust side loads the
SAME fixture states via `GameState::from_json` and compares its own answer
against this dump byte for byte.

Unlike `dump_weighted_rivals.py`'s `_FEATURE_KEYS` (a representative sample
of `feature_marginal` keys), this dumps the WHOLE `features()` dict every
time -- no coordinate sampling. `features()` is the module `docs/
OPEN_ITEMS.md`'s `wonder_stages_per_action` warns about (a coordinate
silently stuck at zero), so the differential test built off this dump checks
every key `features()` can emit against every real `WeightKey`, in both
directions, on every sampled state -- a sampled subset of COORDINATES is not
good enough here even though a sampled subset of STATES still is.

`ctx`/`w`/`priced_only` are left at their defaults (`None`/`None`/`False`) on
both sides -- the complete-vector INSTRUMENT reading `features()`'s own
docstring insists on for anything that is not literally the search's own
speed switch (see that function's docstring on why a `priced_only` vector
must never be fed to something that reads the vector as an instrument).

Usage:

    python3.13 tools/dump_weighted_features.py \\
        --fixtures rust/tests/fixtures --out rust/tests/weighted_features_fixtures \\
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
    return W.features(state, idx)


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
    ap.add_argument("--out", default="rust/tests/weighted_features_fixtures")
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
