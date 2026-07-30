"""How often is rival strength MANDATORY, across many sampled positions?

tests/test_harness_fields.py:test_rival_strength_is_decision_relevant samples
three positions (rounds 8/14/20 of one 3p WeightedBot self-play game) and
requires at least one of them to have rival strength change the argmax.  Any
change to the move stream re-rolls which positions those are, so a failure
there is ambiguous: it can mean "the property is gone" or "these three
samples moved".  This scan disambiguates by measuring the RATE over a grid.

    python3 -m tools.rival_strength_scan            # default grid
"""
from __future__ import annotations

import sys
from collections import Counter

from advisor.advisor import load_bot
from harness import fields as F
from tests.test_harness_mirror import midgame


def main(argv):
    stops = [int(x) for x in argv[1:]] or [6, 8, 10, 12, 14, 16, 18, 20]
    w = load_bot(3).weights
    tally = Counter()
    for stop in stops:
        b = midgame(stop=stop)
        v = F.probe_position(b.state, b.me, w)["rival.strength"]
        tally[v] += 1
        print(f"stop={stop:3d}  round={b.state.round:3d}  rival.strength={v}"
              f"  mandatory={v in set(F.MANDATORY)}")
    print("---")
    for k, n in sorted(tally.items()):
        print(f"{k:12s} {n}")
    mand = sum(n for k, n in tally.items() if k in set(F.MANDATORY))
    print(f"MANDATORY {mand}/{sum(tally.values())}")


if __name__ == "__main__":
    main(sys.argv)
