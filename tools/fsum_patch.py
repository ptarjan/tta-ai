"""Candidate determinism fix, applied by monkeypatch so it can be measured
without touching engine/ (owned by other agents).

`engine.bots.evaluate` totals the weighted feature vector with builtin
`sum()`. CPython >=3.12 uses Neumaier compensated summation for float
arguments; PyPy 3.11 uses naive accumulation. The results differ by ~1 ULP,
which flips exact ties in GreedyBot.pick and diverges the game.

`math.fsum` is *exactly* rounded on every interpreter, so it is a single
cross-interpreter answer. This module swaps it in.

    python3 tools/fsum_patch.py hash
    pypy3    tools/fsum_patch.py hash
"""
import math
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from engine import bots as B          # noqa: E402
from engine import perf_check         # noqa: E402


def evaluate(state, idx, weights=None):
    w = weights or B.WEIGHTS
    f = B.features(state, state.players[idx])
    own = math.fsum([w.get(k, 0.0) * v for k, v in f.items()])
    rivals = [q for q in state.players if q.idx != idx and not q.resigned]
    best_rival = max((q.culture for q in rivals), default=0)
    return own - 0.4 * best_rival


def apply_patch():
    B.evaluate = evaluate
    for mod in list(sys.modules.values()):
        if getattr(mod, "__name__", "").startswith("engine") and \
                getattr(mod, "evaluate", None) is not None and \
                getattr(mod, "evaluate", None) is not evaluate:
            try:
                mod.evaluate = evaluate
            except Exception:
                pass


if __name__ == "__main__":
    apply_patch()
    digest, per = perf_check.fingerprint()
    print("FSUM FINGERPRINT", digest)
