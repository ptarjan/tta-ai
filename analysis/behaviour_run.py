"""Run experiments.behaviour with a missing helper patched in.

`experiments/behaviour.py` calls a module-level `all_snaps_iter(recs)` inside
`_summarize_group` but never defines it, so every run dies at the aggregation
step *after* playing all the games.  That file is owned by another agent, so
rather than edit it this wrapper injects the one-line helper and calls `main()`.

Usage is identical to `python3 -m experiments.behaviour`:

    python3 analysis/behaviour_run.py --players 3 --games 120 \
        --champion experiments/champion_3p.json --out experiments/behaviour_3p.json

Delete this file once behaviour.py defines the helper itself.
"""
from __future__ import annotations

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from experiments import behaviour  # noqa: E402


def all_snaps_iter(recs):
    """Every end-of-turn snapshot across a list of per-game records."""
    for r in recs:
        for s in r["snaps"]:
            yield s


if not hasattr(behaviour, "all_snaps_iter"):
    behaviour.all_snaps_iter = all_snaps_iter

if __name__ == "__main__":
    behaviour.main()
