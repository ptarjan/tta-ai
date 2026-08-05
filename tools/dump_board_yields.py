"""Dump `engine/bots/board_yields.py`'s answers on real game states, for the
Rust port's differential test (`rust/tests/board_yields.rs`).

There is no in-process bridge between the two engines, so -- exactly like
`tools/dump_fixtures.py` -- the oracle is offline: this script loads states
already recorded by `dump_fixtures.py` (`rust/tests/fixtures/*.jsonl`, one
`GameState.to_dict()` per ply) and asks the real Python `board_yields`
module every question the Rust port answers, for every player, on a sample of
those states. The Rust side (`rust/tests/board_yields.rs`) loads the SAME
fixture states via `GameState::from_json` and compares its own answers
against this dump byte for byte.

Card names are NOT sampled: every card of a type any `board_yields.py` entry
point prices at all (leader/government/wonder for `board_yields`+
`government_plans`, every "levelled" type for `unit_upgrade`/`tech_upgrade`/
`build_fresh`, plus the 3 cards `board_extra` recognises) is asked about on
every sampled state, for every player -- 48 + 46 + 3 = 97 cards. States ARE
sampled (every `--stride`'th ply per fixture file, always including the
last) because 97 cards x 4 players x a fixture's full ply count is more
computation than this comparison needs to be confident; the stride is large
enough to cover many different boards per file without needing every ply.

Usage:

    python3.13 tools/dump_board_yields.py \\
        --fixtures rust/tests/fixtures --out rust/tests/board_yields_fixtures \\
        --stride 15
"""
from __future__ import annotations

import argparse
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from engine import cards as C                          # noqa: E402
from engine.state import GameState                     # noqa: E402
from engine.bots import board_yields as BY              # noqa: E402

_DUMP_JSON_KW = dict(sort_keys=True, separators=(",", ":"))

_DB = C.db()

#: Card types `board_yields`/`government_plans` price by a swap diff.
_SWAP_NAMES = sorted(n for n, c in _DB.by_name.items() if c["type"] in BY.SWAP_TYPES)

#: Card types `unit_upgrade`/`tech_upgrade`/`build_fresh` price.
_LEVELLED_NAMES = sorted(n for n, c in _DB.by_name.items() if c["type"] in BY.LEVELLED_TYPES)

#: The only cards `board_extra` has anything to say about (see that
#: function's own `_EXTRA_CARDS`).
_EXTRA_NAMES = sorted(BY._EXTRA_CARDS)

_GOV_NAMES = sorted(n for n, c in _DB.by_name.items() if c["type"] == "government")


def _triples_json(triples):
    return [[f, a, k] for f, a, k in triples]


def _routes_json(routes):
    return [_triples_json(r) for r in routes]


def _one_state(state, idx):
    """Every `board_yields.py` answer for player `idx` on `state`, as a
    plain-JSON dict. Deterministic key order via `_DUMP_JSON_KW` at write
    time, not here.
    """
    out = {"leader": {}, "government": {}, "wonder": {}, "gov_plans": {},
           "unit_upgrade": {}, "tech_upgrade": {}, "build_fresh": {},
           "board_extra": {}}
    for name in _SWAP_NAMES:
        got = BY.board_yields(name, state, idx)
        if got:
            out[_DB.type_of(name)][name] = _triples_json(got)
    for name in _GOV_NAMES:
        gained, routes = BY.government_plans(name, state, idx)
        if gained or routes:
            out["gov_plans"][name] = [_triples_json(gained), _routes_json(routes)]
    for name in _LEVELLED_NAMES:
        if _DB.type_of(name) in C.UNIT_TYPES:
            gained, sci, res = BY.unit_upgrade(name, state, idx)
            if gained or sci or res:
                out["unit_upgrade"][name] = [gained, sci, res]
        staff, dev, sci, res = BY.tech_upgrade(name, state, idx)
        if staff or dev or sci or res:
            out["tech_upgrade"][name] = [_triples_json(staff), _triples_json(dev), sci, res]
        triples, res = BY.build_fresh(name, state, idx)
        if triples or res:
            out["build_fresh"][name] = [_triples_json(triples), res]
    for name in _EXTRA_NAMES:
        got = BY.board_extra(name, state, idx)
        if got:
            out["board_extra"][name] = _triples_json(got)
    return out


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
            per_player[str(idx)] = _one_state(state, idx)
        records.append({"ply": rec["ply"], "players": per_player})

    with open(out_path, "w") as f:
        for r in records:
            f.write(json.dumps(r, **_DUMP_JSON_KW) + "\n")
    return len(records)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--fixtures", default="rust/tests/fixtures")
    ap.add_argument("--out", default="rust/tests/board_yields_fixtures")
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
