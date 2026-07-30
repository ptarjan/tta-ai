"""How much of each card can the evaluator actually see?

`engine/bots/weighted.py:_card_yields` turns a card into (feature, amount)
pairs by looking its `production` and `effects` keys up in two tables.  Any
key not in those tables, and any key whose value is `True` or a string rather
than a number, is dropped on the floor without a word.  This tool counts how
much gets dropped, by card type and by key.

    python3 -m tools.card_blindness              # the tree as it stands
    python3 -m tools.card_blindness --legacy     # ...with the `culture` and
                                                 # `science` mappings removed,
                                                 # i.e. the pre-fix numbers
    python3 -m tools.card_blindness --keys       # per-key detail
    python3 -m tools.card_blindness --cards wonder   # per-card, one type

"zero visible gain" means `_card_yields` produced no non-cost pair at all,
excluding the generic `("wonders", 1.0)` every wonder gets just for being one
-- that term cannot tell Pyramids from Colossus, so counting it would hide
exactly the thing being measured.

See docs/CARD_BLINDNESS.md.
"""
from __future__ import annotations

import argparse
import collections
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine import cards as C            # noqa: E402
from engine.bots import weighted as W    # noqa: E402

# every effect key docs/CARD_BLINDNESS.md taught `_card_yields` to read.
# `--legacy` removes all of them, which reproduces master's tables exactly and
# is what the "before" column of the doc's census is generated from.
LEGACY_DROPPED = ("culture", "science", "civilHandLimit", "militaryHandLimit",
                  "colonizeBonus", "resourceDiscount")


def use_legacy_maps():
    """Restore the pre-fix `_EFF_TO_FEATURE`, for the before/after table."""
    for k in LEGACY_DROPPED:
        W._EFF_TO_FEATURE.pop(k, None)
    W._EFF_SPECIAL.clear()
    W._card_yields.cache_clear()


def _mapped(block):
    if block == "production":
        return set(W._PROD_TO_FEATURE)
    return set(W._EFF_TO_FEATURE) | set(W._EFF_SPECIAL)


def scan():
    """(per-type counter table, per-key counter table, per-card detail)."""
    types = collections.Counter()
    dropped = collections.Counter()
    zero = collections.Counter()
    keys = collections.Counter()
    keys_nonnum = collections.Counter()
    cards = {}
    for name, card in C.db().by_name.items():
        t = card["type"]
        types[t] += 1
        gains = [(k, a) for k, a, kind in W._card_yields(name)
                 if kind != W._Y_COST and k != "wonders"]
        drop = {}
        for block in ("production", "effects"):
            ok = _mapped(block)
            for k, v in (card.get(block) or {}).items():
                num = (isinstance(v, (int, float))
                       and v is not True and v is not False)
                if k in ok and (num or block == "effects"
                                and k in W._EFF_SPECIAL):
                    continue
                drop[k] = v
                keys[k] += 1
                if not num:
                    keys_nonnum[k] += 1
        if drop:
            dropped[t] += 1
        if not gains:
            zero[t] += 1
        cards[name] = (t, gains, drop)
    return types, dropped, zero, keys, keys_nonnum, cards


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--legacy", action="store_true",
                    help="drop the `culture`/`science` mappings first")
    ap.add_argument("--keys", action="store_true", help="per-key table")
    ap.add_argument("--cards", default="", help="per-card table for one type")
    a = ap.parse_args(argv)
    if a.legacy:
        use_legacy_maps()

    types, dropped, zero, keys, nonnum, cards = scan()
    print(f"{'type':16s} {'n':>4s} {'dropped':>8s} {'zero-gain':>10s}")
    for t in sorted(types, key=lambda x: (-types[x], x)):
        print(f"{t:16s} {types[t]:4d} {dropped[t]:8d} {zero[t]:10d}")
    print(f"{'TOTAL':16s} {sum(types.values()):4d} "
          f"{sum(dropped.values()):8d} {sum(zero.values()):10d}")

    if a.keys:
        print(f"\n{'dropped key':52s} {'cards':>6s} {'non-numeric':>12s}")
        for k, n in keys.most_common():
            print(f"{k:52s} {n:6d} {nonnum[k]:12d}")

    if a.cards:
        print()
        for name, (t, gains, drop) in sorted(cards.items()):
            if t != a.cards:
                continue
            g = ", ".join(f"{k}{amt:+g}" for k, amt in gains) or "-- NOTHING --"
            print(f"{name:28s} seen: {g}")
            if drop:
                print(f"{'':28s} dropped: {sorted(drop)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
