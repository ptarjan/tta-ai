"""Dump `engine/bots/weighted.py`'s DEFAULT_WEIGHTS table and its horizon
functions' answers on real game states, for the Rust port's differential
test (`rust/tests/weighted_horizon.rs`).

Exactly the same offline-oracle shape as `tools/dump_counting.py`/
`tools/dump_board_yields.py` (read either script's own doc comment for the
full rationale): this script loads states already recorded by
`dump_fixtures.py` (`rust/tests/fixtures/*.jsonl`, one `GameState.to_dict()`
per ply) and asks the real Python `weighted` module every question the Rust
port (`rust/src/bots/weighted/{weights,horizon}.rs`) answers, on a sample of
those states. The Rust side loads the SAME fixture states via
`GameState::from_json` and compares its own answers against this dump byte
for byte.

Two dumps, because `weights.rs` and `horizon.rs` are pinned differently:

* `default_weights.json` -- `DEFAULT_WEIGHTS` and `RETIRED_KEYS` verbatim,
  once. Not per-state: these are constants, not functions of a position.
* `<fixture>.jsonl`, one line per sampled ply -- every horizon function
  (`cards_unseen`, `take_rate`, `rounds_left`, `lateness`, `horizon_scale`,
  `rate_multiplier` at several credits). None of these take a player index --
  unlike `counting.py`'s functions, they are properties of the STATE alone --
  so there is one record per ply, not one per live player.

Two dead-code fixes landed in `engine/bots/weighted.py` alongside this port
(a genuinely unread `_ROW` constant, and `horizon_scale`'s unread `w`
parameter -- see that module's and `horizon.rs`'s own doc comments). Neither
changes any number this script dumps; this comment exists so a reader
diffing this file against an older `weighted.py` is not surprised that
`horizon_scale` here is called with two arguments, not three.

Usage:

    python3.13 tools/dump_weighted_horizon.py \\
        --fixtures rust/tests/fixtures --out rust/tests/weighted_horizon_fixtures \\
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

#: Credits sampled for `rate_multiplier`: 0.0 (must be exactly 1.0, the
#: no-op case), 1.0 (`DEFAULT_WEIGHTS`'s own shipped value), plus a fraction
#: and an over-1.0 value to pin the blend and the floor-at-0.0 clamp.
_CREDITS = (0.0, 0.5, 1.0, 2.0, -1.0)


def _weights_with_credit(c):
    w = dict(W.DEFAULT_WEIGHTS)
    w["rate_horizon"] = c
    return w


def _one_state(state):
    n = W._live(state)
    return {
        "n": n,
        "cards_unseen": W.cards_unseen(state, n),
        "take_rate": W.take_rate(state, n),
        "rounds_left": W.rounds_left(state, n),
        "lateness": W.lateness(state),
        "horizon_scale": W.horizon_scale(state, n),
        "rate_multiplier": {
            repr(c): W.rate_multiplier(state, _weights_with_credit(c), n)
            for c in _CREDITS
        },
    }


def dump_default_weights(out_dir):
    path = os.path.join(out_dir, "default_weights.json")
    with open(path, "w") as f:
        json.dump(
            {"weights": W.DEFAULT_WEIGHTS, "retired_keys": sorted(W.RETIRED_KEYS)},
            f, **_DUMP_JSON_KW,
        )
    return path


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
        records.append({"ply": rec["ply"], **_one_state(state)})

    with open(out_path, "w") as f:
        for r in records:
            f.write(json.dumps(r, **_DUMP_JSON_KW) + "\n")
    return len(records)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--fixtures", default="rust/tests/fixtures")
    ap.add_argument("--out", default="rust/tests/weighted_horizon_fixtures")
    ap.add_argument("--stride", type=int, default=15)
    args = ap.parse_args(argv)

    os.makedirs(args.out, exist_ok=True)
    wpath = dump_default_weights(args.out)
    print(f"default_weights -> {wpath}")

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
