"""Dump `engine/bots/weighted.py`'s row-section answers (`_rival_take_cost`,
`rival_take_p`, `_rival_desire`, `row_pressure`, `row_last_copy`,
`RIVAL_TAKE_SHARE`) on real game states, for the Rust port's differential
test (`rust/tests/weighted_row.rs`).

Same offline-oracle shape as `tools/dump_weighted_horizon.py`/
`tools/dump_counting.py` (read either script's own doc comment for the full
rationale): this script loads states already recorded by `dump_fixtures.py`
(`rust/tests/fixtures/*.jsonl`, one `GameState.to_dict()` per ply) and asks
the real Python `weighted` module every question `rust/src/bots/weighted/
row.rs` answers, for every live player, on a sample of those states, under
several weight variants. The Rust side loads the SAME fixture states via
`GameState::from_json`, independently rebuilds `rivals::rival_context`
(already landed and differentially tested on its own, `rust/tests/
weighted_rivals.rs`) and compares its `row_pressure`/`row_last_copy` against
this dump byte for byte.

## `card_potential` is dumped as DATA, not reimplemented

`card_potential` (`weighted.py` lines 1730-3211) is `cards.rs`'s port, not
`row.rs`'s, and is not finished yet (only its yield-plumbing layer had
landed as of this dump -- the VALUATION layer, `card_potential` itself, has
not). `row.rs`'s `row_pressure`/`row_last_copy` therefore take it as an
INJECTED closure rather than calling a Rust port of it -- see that module's
own top doc comment for why this is exactly the case this port's house
style reserves dependency injection for. For the differential test to
exercise the REAL row/bargain/last-copy arithmetic (not a stand-in), this
script dumps Python's own `card_potential` value for every (viewer, row-
card-name) pair a sampled decision could query, and the Rust test's injected
closure is a lookup into that table rather than a second implementation of
card pricing. This tests everything `row.rs` actually owns -- the masking,
the take-gating, the desire-weighted competition, the slide/bargain
arithmetic -- against real numbers, without requiring `cards::
card_potential` to exist first. Once it does, `eval.rs` wires the real thing
in; this dump (and the closure-shaped test harness reading it) stops being
needed at that point, not before.

Weight variants only ever move `rival_desire`/`rival_take_share`: no other
`DEFAULT_WEIGHTS` key changes `card_potential`'s OWN value (it reads
neither), so one `card_potential` table per (ply, idx) is dumped and reused
under every weight variant's `row_pressure`/`row_last_copy`.

Usage:

    python3.13 tools/dump_weighted_row.py \\
        --fixtures rust/tests/fixtures --out rust/tests/weighted_row_fixtures \\
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

#: Weight overrides layered onto `DEFAULT_WEIGHTS` -- see this script's own
#: doc comment for why only these two keys ever need to vary: `rival_desire`
#: at 0.0 (the default -- `_rival_desire` never runs), a fraction (blends the
#: uniform `reach` against the desire-weighted `eff`), and 1.0 (`compete ==
#: eff` exactly, the branch this port's own top doc comment on the removed
#: `eff` floor is about); `rival_take_share` away from its 0.5 default to
#: pin `rival_take_p`'s own sensitivity to it independently of `row_pressure`
#: (already unit-pinned in `rust/src/bots/weighted/row.rs`'s own tests, but
#: exercised here end-to-end too).
_WEIGHT_VARIANTS = {
    "default": {},
    "desire_half": {"rival_desire": 0.5},
    "desire_one": {"rival_desire": 1.0},
    "share_low": {"rival_take_share": 0.1},
    "share_high_desire": {"rival_take_share": 0.9, "rival_desire": 0.7},
}


def _weights(overrides):
    w = dict(W.DEFAULT_WEIGHTS)
    w.update(overrides)
    return w


def _card_potential_table(state, idx, ctx, late):
    """`{viewer_idx: {name: card_potential(name, DEFAULT_WEIGHTS, state,
    viewer_idx, late)}}` for every viewer `row_pressure`/`_rival_desire`
    could ever price a row card from (the mover, plus every live rival) and
    every name currently sitting in `state.card_row`."""
    viewers = {idx}
    for view, _gate in ctx["rival_views"]:
        viewers.add(view.idx)
    names = sorted({name for name in state.card_row if name is not None})
    return {
        str(v_idx): {
            name: W.card_potential(name, W.DEFAULT_WEIGHTS, state, v_idx, late)
            for name in names
        }
        for v_idx in viewers
    }


def _one_player(state, idx):
    ctx = W.rival_context(state, idx)
    late = W.lateness(state)
    results = {}
    for tag, overrides in _WEIGHT_VARIANTS.items():
        w = _weights(overrides)
        urgency, bargain = W.row_pressure(state, idx, w, ctx)
        last_copy = W.row_last_copy(state, idx, w, ctx)
        results[tag] = {
            "row_urgency": urgency,
            "row_bargain_forgone": bargain,
            "row_last_copy": last_copy,
        }
    return {
        "card_potential": _card_potential_table(state, idx, ctx, late),
        "results": results,
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
    ap.add_argument("--out", default="rust/tests/weighted_row_fixtures")
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
