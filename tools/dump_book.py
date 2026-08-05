"""Dump `engine/bots/book.py`'s `BookBot` decisions on real game states, for
the Rust port's differential test (`rust/tests/book.rs`).

Same offline-oracle shape as `tools/dump_counting.py`/`tools/dump_board_
yields.py` (read either script's own doc comment for the full rationale):
this script loads states already recorded by `dump_fixtures.py` (`rust/
tests/fixtures/*.jsonl`, one `GameState.to_dict()` per ply) and asks the
real Python `BookBot` -- both `version=1` and `version=2` -- what it would
play at each sampled state, given the real `engine.actions.legal_moves`
output for that state. The Rust side (`rust/tests/book.rs`) loads the SAME
fixture states via `GameState::from_json`, computes its own
`crate::legal::legal_moves`, runs `bots::book::BookBot::choose`, and
compares the CHOSEN MOVE against this dump.

Every `state.pending` ply is dumped UNCONDITIONALLY, on top of a strided
sample of the rest -- `BookBot`'s pending-decision branches (`_choice`'s 15
tags, `_auction`, `_defense`, `_colonize`) are rare in ordinary self-play
(one fixture file measured at ~15% of plies), and a plain stride sample
would under-cover them badly.

A move is dumped as a plain JSON array: the move's own tag string, followed
by its fields in the same order `engine.actions`/`engine.interact`/
`engine.game` construct the move tuple in (a card/government/tech name
stays a name, a row slot / bid amount / choose index stays an int) -- e.g.
`["take", 3]`, `["upgrade", "Bronze", "Iron"]`, `["aggression", "Legion", 2]`,
`["end_turn"]`. `rust/tests/book.rs` matches this shape directly against its
own `Move` rather than deserializing a generic move type.

Usage:

    python3.13 tools/dump_book.py \\
        --fixtures rust/tests/fixtures --out rust/tests/book_fixtures \\
        --stride 20
"""
from __future__ import annotations

import argparse
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from engine.state import GameState                     # noqa: E402
from engine import actions as A                         # noqa: E402
from engine.bots import book as B                        # noqa: E402

_DUMP_JSON_KW = dict(sort_keys=True, separators=(",", ":"))

_BOT_V1 = B.BookBot(version=1)
_BOT_V2 = B.BookBot(version=2)


def _move_json(mv):
    if mv is None:
        return None
    return list(mv)


def _one_state(state):
    moves = A.legal_moves(state)
    if not moves:
        return None
    return {
        "phase": state.phase,
        "pending": bool(state.pending),
        "v1": _move_json(_BOT_V1.choose(state, moves)),
        "v2": _move_json(_BOT_V2.choose(state, moves)),
    }


def dump_file(path, out_path, stride):
    with open(path) as f:
        lines = f.readlines()
    plies = []
    for line in lines:
        rec = json.loads(line)
        if "ply" in rec and rec.get("state") is not None:
            plies.append(rec)

    sampled = []
    seen = set()
    for i, rec in enumerate(plies):
        state = GameState.from_dict(rec["state"])
        is_pending = bool(state.pending)
        if is_pending or i % stride == 0:
            sampled.append((rec["ply"], state))
            seen.add(rec["ply"])
    if plies and plies[-1]["ply"] not in seen:
        sampled.append((plies[-1]["ply"], GameState.from_dict(plies[-1]["state"])))

    records = []
    for ply, state in sampled:
        if state.game_over:
            continue
        one = _one_state(state)
        if one is None:
            continue
        one["ply"] = ply
        records.append(one)

    with open(out_path, "w") as f:
        for r in records:
            f.write(json.dumps(r, **_DUMP_JSON_KW) + "\n")
    return len(records)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--fixtures", default="rust/tests/fixtures")
    ap.add_argument("--out", default="rust/tests/book_fixtures")
    ap.add_argument("--stride", type=int, default=20)
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
