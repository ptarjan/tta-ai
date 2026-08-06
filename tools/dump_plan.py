"""Dump `engine/bots/plan.py::determinize()`'s reordering of the three hidden
piles (`civil_deck`, `military_deck`, `current_events`), for the Rust port's
differential test (`rust/tests/plan.rs`).

`determinize` is the one piece of `PlanBot`'s search this dump CAN check
cross-engine on real states. Every other reachable code path in `plan.py`
runs a trial `actions.apply` under `trial.py`'s pooled `Random(0)` stream (a
FIXED, game-state-independent seed, reset fresh per candidate), while this
port's `crate::apply::apply` derives its own rng deterministically from
`state.seed`/`state.turn`/`state.round` (`game.rs::rng_for`) instead -- an
already-accepted divergence baked into every landed search bot (`quiescent.rs`
carries the identical gap and has no chosen-move differential test either;
see that module's own doc comment). A trial-apply node that happens to
consume randomness therefore draws a DIFFERENT card in the two engines, and
everything downstream of that node in a beam can legitimately disagree
without either side having a bug -- so there is no meaningful way to dump
`PlanBot.pick`'s own chosen move here.

`determinize`, by contrast, is handed an EXPLICIT rng by its caller (`pick`'s
own `drng`/`self.rng`, never `trial.py`'s pool), and `rust::rng::PyRandom` is
verified bit-exact against CPython's `random.Random` (`rust/src/rng.rs`'s own
fixture test) -- so for a SHARED seed, both engines must reorder the three
hidden piles identically. That is the one full cross-engine check this module
can make, and `rust/src/bots/plan.rs`'s own `#[cfg(test)]` module carries the
rest of the verification (structural properties: never mutates the real
state, always returns an offered move, drains a real pending decision,
prices a declared war through `quiescent::war_value`, routes a non-ordinary-
turn decision through the shared `pending` policy) the same way
`quiescent.rs`'s own test module does, with no Python counterpart needed.

Same offline-oracle shape as `tools/dump_weighted_eval.py`: loads states
already recorded by `dump_fixtures.py` (`rust/tests/fixtures/*.jsonl`) and
runs the real Python `determinize` on a fresh copy of each sampled state,
once per seed in `SEEDS`, dumping the resulting `civil_deck`/`military_deck`/
`current_events` (card names, in final order).

Usage:

    python3.13 tools/dump_plan.py \\
        --fixtures rust/tests/fixtures --out rust/tests/plan_fixtures --stride 25
"""
from __future__ import annotations

import argparse
import json
import os
import random
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from engine.state import GameState                     # noqa: E402
from engine.bots.fastcopy import copy_state              # noqa: E402
from engine.bots.plan import determinize                 # noqa: E402

_DUMP_JSON_KW = dict(sort_keys=True, separators=(",", ":"))

# A spread of seeds matching `rust/src/rng.rs`'s own fixture spread: zero,
# small, negative, and one straddling the 32/64-bit limb boundary.
SEEDS = [0, 1, 2, -1, 4294967296, 123456789]


def _one_seed(state, seed):
    trial = copy_state(state)
    determinize(trial, random.Random(seed))
    return {
        "civil_deck": list(trial.civil_deck),
        "military_deck": list(trial.military_deck),
        "current_events": list(trial.current_events),
    }


def _one_state(state):
    return {str(seed): _one_seed(state, seed) for seed in SEEDS}


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
        records.append({"ply": rec["ply"], "seeds": _one_state(state)})

    with open(out_path, "w") as f:
        for r in records:
            f.write(json.dumps(r, **_DUMP_JSON_KW) + "\n")
    return len(records)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--fixtures", default="rust/tests/fixtures")
    ap.add_argument("--out", default="rust/tests/plan_fixtures")
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
