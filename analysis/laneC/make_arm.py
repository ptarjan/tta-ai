"""Build the two weight vectors for one Lane C A/B arm.

The arm used to be an environment variable (`TTA_BOARD_TYPES`); it is a weight
configuration now (docs/CARD_BLINDNESS.md 13.10.2), which is the whole
point -- a configuration only a human could set became one the league can fit.
That makes "what did this arm actually run?" a question with a file for an
answer instead of a shell variable, so this writes both vectors to disk.

    python3 analysis/laneC/make_arm.py leader 0.0 /tmp/on.json /tmp/off.json

`arm` is `main` (every type, i.e. the shared credit alone) or one of
`leader` / `government` / `action` / `wonder`, which is expressed by
CANCELLING the shared credit on every other type with a -1.0 offset --
exactly what the environment variable used to do, so the arms stay comparable
to analysis/laneC/results.txt.

`extra` is `hand_swap_extra`: 0.0 is the shipped pricing (a hand of leaders is
its best single replacement), 1.0 is exactly the summing that preceded the
fix.  It is set on both vectors, and is inert on the off arm, so the two
vectors still differ in the board credits alone.

Separate from run_ab.sh because the desktop that has the cores has no bash.
"""
from __future__ import annotations

import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
TYPES = ("leader", "government", "action", "wonder")


def _load(name):
    """The weight vector, and a function that puts it back in its wrapper.

    `experiments/arena.py:_weights_of` accepts both a bare weight dict and one
    wrapped as `{"weights": {...}, ...}` (the champion files are wrapped, and
    analysis/laneC/on.json is one of those).  Writing a bare dict back where a
    wrapped one was read would silently drop whatever else the wrapper carried,
    so the shape is preserved."""
    with open(os.path.join(HERE, name)) as fh:
        doc = json.load(fh)
    if isinstance(doc.get("weights"), dict):
        return doc["weights"], (lambda w, d=doc: dict(d, weights=w))
    return doc, (lambda w: w)


def build(arm, extra):
    on, wrap_on = _load("on.json")
    off, wrap_off = _load("off.json")
    assert on["card_board_credit"] == 1.0, "on.json must have the credit up"
    assert off["card_board_credit"] == 0.0, "off.json must have it down"
    if arm != "main":
        assert arm in TYPES, f"unknown arm {arm!r}"
        for t in TYPES:
            if t != arm:
                on["card_board_" + t] = -1.0
    on["hand_swap_extra"] = off["hand_swap_extra"] = extra
    return wrap_on(on), wrap_off(off), on, off


def main(argv):
    if len(argv) != 5:
        print(__doc__)
        return 2
    arm, extra, on_path, off_path = argv[1], float(argv[2]), argv[3], argv[4]
    on_doc, off_doc, on, off = build(arm, extra)
    # printed, not assumed: the two vectors must differ in the board credits
    # and nothing else, which is the whole claim the duel rests on.
    diff = sorted(k for k in set(on) | set(off) if on.get(k) != off.get(k))
    print(f"arm={arm} hand_swap_extra={extra}")
    print(f"  {len(on)} weights; differing between the two vectors: {diff}")
    for path, doc in ((on_path, on_doc), (off_path, off_doc)):
        with open(path, "w") as fh:
            json.dump(doc, fh, indent=1)
        print(f"  wrote {path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
