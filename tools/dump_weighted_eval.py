"""Dump `engine/bots/weighted.py::evaluate()`'s answer on real self-play
states, for the Rust port's differential test (`rust/tests/weighted_eval.rs`).

Exactly the same offline-oracle shape as `tools/dump_weighted_features.py`
(read that script's own doc comment for the full rationale): this script
loads states already recorded by `dump_fixtures.py`
(`rust/tests/fixtures/*.jsonl`, one `GameState.to_dict()` per ply) and asks
the real Python `weighted.evaluate()` for its ANSWER, for every live player,
on a sample of those states. The Rust side loads the SAME fixture states via
`GameState::from_json` and compares its own answer against this dump.

`evaluate()` returns one scalar, not a keyed vector like `features()` does --
there is no "every coordinate, both directions" check to make here the way
`weighted_features.rs` makes one. What plays that role instead is
`_WEIGHT_VECTORS`: several DIFFERENT weight vectors, each one dumped for
every sampled state, chosen so that between them every branch `evaluate` can
take is exercised on real boards, not just the linear body every vector hits:

* `"default"` -- `DEFAULT_WEIGHTS` verbatim, the vector every real caller
  starts from. `hand_potential` (0.125) and `rate_horizon` (1.0) are already
  nonzero here, so this alone already exercises the phase blend, the rate
  horizon's `hz != 1.0` branches, and the `hand_potential` identity-aware
  term.
* `"rate_horizon_off"` -- `DEFAULT_WEIGHTS` with `rate_horizon` forced to
  0.0, deterministically forcing the `hz == 1.0` short-circuit on every
  sampled state (under `"default"` alone, whether that branch fires depends
  on how close the state's `horizon_scale` happens to land to 1.0).
* `"all_optional_on"` -- every OTHER identity-aware/eval-only term
  `DEFAULT_WEIGHTS` prices at exactly 0.0 (`wonder_potential`,
  `hand_mil_potential`, `tactic_gain`, `tactic_short`, `rival_hand_potential`,
  `row_urgency`, `row_bargain_forgone`, `row_last_copy`, `my_event_threat`),
  turned on at a distinct nonzero probe value each -- so a bug that swapped
  two of these calls, or dropped one, moves a DIFFERENT amount on a real
  board than the correct wiring would, rather than two bugs cancelling out
  or a whole term going untested because a real trained champion happens
  never to price it.

`ctx` is computed once per `(state, idx)` via `rival_context`, matching every
real caller (`WeightedBot.pick`/`choose`) rather than the degraded
`ctx=None` path -- see `evaluate`'s own docstring on why a caller with a real
root context always supplies one.

Usage:

    python3.13 tools/dump_weighted_eval.py \\
        --fixtures rust/tests/fixtures --out rust/tests/weighted_eval_fixtures \\
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


def _all_optional_on():
    w = dict(W.DEFAULT_WEIGHTS)
    w.update(
        wonder_potential=0.7,
        hand_mil_potential=0.6,
        tactic_gain=0.5,
        tactic_short=0.4,
        rival_hand_potential=0.3,
        row_urgency=0.2,
        row_bargain_forgone=0.15,
        row_last_copy=0.1,
        my_event_threat=0.05,
    )
    return w


def _rate_horizon_off():
    return dict(W.DEFAULT_WEIGHTS, rate_horizon=0.0)


_WEIGHT_VECTORS = {
    "default": dict(W.DEFAULT_WEIGHTS),
    "rate_horizon_off": _rate_horizon_off(),
    "all_optional_on": _all_optional_on(),
}


def _one_player(state, idx):
    ctx = W.rival_context(state, idx)
    return {name: W.evaluate(state, idx, w, ctx) for name, w in _WEIGHT_VECTORS.items()}


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
    ap.add_argument("--out", default="rust/tests/weighted_eval_fixtures")
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
