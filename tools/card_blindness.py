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
    python3 -m tools.card_blindness --board          # count the board-aware
                                                     # evaluator too

"zero visible gain" means `_card_yields` produced no non-cost pair at all,
excluding the generic `("wonders", 1.0)` every wonder gets just for being one
-- that term cannot tell Pyramids from Colossus, so counting it would hide
exactly the thing being measured.

WHAT THIS TOOL DOES **NOT** MEASURE, AND WHY THE SPLIT BELOW EXISTS
-------------------------------------------------------------------
`_card_yields` is reached only through `card_potential` <- `hand_potential`,
and `hand_potential` walks `p.hand_civil` ONLY.  **It is never called for a
card in the military deck** -- every event, aggression, war, pact, tactic,
territory and bonus card.  For those 109 cards a "dropped key" is not a
finding: mapping the key would change nothing, because nothing ever asks.

The first version of this tool pooled them with the civil cards, which
over-reported the blind spot by 109 cards and sent a whole work stream after
table entries that could not have helped.  An over-reporting measurement is
exactly as dangerous as an under-reporting one, so the two decks are now
counted separately and the military rows carry the reason.

Military-deck cards are not unpriced; they are priced somewhere else, and
the census cannot see that either:

  * aggressions -- `QuiescentBot` drains the defender's `defense` pending
    with real picks and scores the quiet position (docs/AGGRESSION_FIX.md)
  * wars        -- `quiescent.war_value` calls the engine's `resolve_war`
  * pacts       -- `weighted.deferred_credit`, and `count 2p: 0` besides
  * Age III scoring events -- `weighted.event_scoring_margin`, which calls
    `events.final_event_culture` (docs/EVENT_SEEDING.md)
  * tactics/territories/bonus -- genuinely unpriced, but the fix is a
    military sibling to `hand_potential`, not a table entry

See docs/CARD_BLINDNESS.md and docs/EVENT_SEEDING.md.
"""
from __future__ import annotations

import argparse
import collections
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine import cards as C                       # noqa: E402
from engine.bots import board_yields as BY          # noqa: E402
from engine.bots import weighted as W               # noqa: E402

# every effect key docs/CARD_BLINDNESS.md taught `_card_yields` to read.
# `--legacy` removes all of them, which reproduces master's tables exactly and
# is what the "before" column of the doc's census is generated from.
LEGACY_DROPPED = ("culture", "science", "civilHandLimit", "militaryHandLimit",
                  "colonizeBonus", "resourceDiscount",
                  # added by the leader/government/action work
                  "resourcesForMilitaryUnits")


def use_legacy_maps():
    """Restore the pre-fix `_EFF_TO_FEATURE`, for the before/after table.

    "Pre-fix" means master before docs/CARD_BLINDNESS.md, so everything any
    later pass added has to come back out -- the `_EFF_SPECIAL` handlers, the
    `_EFF_CHOICE` groups and the board-aware evaluator, not just the two
    mappings the original omission was about.  The check that this is
    complete is that `--legacy` still reproduces the doc's "master" column:
    171 cards with a dropped key, 168 with zero visible gain.
    """
    for k in LEGACY_DROPPED:
        W._EFF_TO_FEATURE.pop(k, None)
    W._EFF_SPECIAL.clear()
    W._EFF_CHOICE.clear()
    # the military-card pass: a unit's top-level `strength` and a territory's
    # immediateEffects/permanentEffects, neither of which lives in a block
    # this module's `_mapped` walks -- which is exactly why the census called
    # all 22 of them "zero dropped keys, zero visible gain".
    W._UNIT_TO_FEATURE.clear()
    W._TERR_TO_FEATURE.clear()
    W._card_yields.cache_clear()
    W._card_choices.cache_clear()


def _mapped(block, board=False):
    if block == "production":
        return set(W._PROD_TO_FEATURE)
    out = set(W._EFF_TO_FEATURE) | set(W._EFF_SPECIAL) | set(W._EFF_CHOICE)
    out -= set(LEGACY_DROPPED) - set(W._EFF_TO_FEATURE)
    if board:
        out |= set(BY.BOARD_PRICED)
    return out


def reachable(card):
    """Is `_card_yields` ever ASKED about this card?

    False for every military-deck card: `hand_potential` walks `hand_civil`
    only, so a dropped key on a war or an event is not a blind spot, it is a
    question nobody asks.  Wonders and leaders are not in `hand_civil` either,
    but they are taken FROM THE CIVIL ROW and `card_potential` prices them
    there, so they are reachable.
    """
    return card.get("deck") != "military"


def _board_state():
    """A two-player board carrying one staffed example of everything a card
    can be paid for, so that a leader counted as "zero gain" is one the
    evaluator cannot see rather than one this board gives nothing to.

    The census is otherwise a statement about a table; with `--board` it is a
    statement about the evaluator, and the evaluator needs a board.
    """
    import random
    from engine import actions as A, effects, game as G
    from engine.bots import WeightedBot
    from engine.state import TechCard
    st = G.new_game(2, 7)
    rng = random.Random(7)
    bots = [WeightedBot(seed=7 + i) for i in range(2)]
    for _ in range(60):
        if st.game_over:
            break
        A.apply(st, bots[st.decider()].pick(st, A.legal_moves(st)), rng)
    db = C.db()
    p = st.players[0]
    p.leader = None
    for typ in ("lab", "library", "theater", "temple", "mine", "farm",
                "infantry", "cavalry", "artillery"):
        pick = max(db.of_type(typ), key=lambda c: C.level(c["age"]))["name"]
        if pick not in p.techs:
            p.techs[pick] = TechCard(pick)
        p.techs[pick].workers = max(1, p.techs[pick].workers)
    if not p.colonies:
        p.colonies = [c["name"] for c in db.of_type("territory")][:2]
    # somebody ahead of us on culture and on strength, so the board-scaled
    # action cards (Endowment, Military Build-Up) have something to count
    st.players[1].culture = p.culture + 50
    effects.invalidate(st)
    return st


def scan(board=False):
    """(per-type counter table, per-key counter table, per-card detail)."""
    st = _board_state() if board else None
    types = collections.Counter()
    dropped = collections.Counter()
    zero = collections.Counter()
    keys = collections.Counter()
    keys_nonnum = collections.Counter()
    cards = {}
    for name, card in C.db().by_name.items():
        t = card["type"]
        types[t] += 1
        triples = W._card_yields(name)
        for group in W._card_choices(name):
            triples = triples + max(group, key=len)
        if st is not None:
            swap = BY.board_yields(name, st, 0)
            if swap is not None:
                triples = swap
            else:
                triples = triples + BY.board_extra(name, st, 0)
        gains = [(k, a) for k, a, kind in triples
                 if kind != W._Y_COST and k not in ("wonders", "leader")]
        drop = {}
        for block in ("production", "effects"):
            ok = _mapped(block, board)
            for k, v in (card.get(block) or {}).items():
                num = (isinstance(v, (int, float))
                       and v is not True and v is not False)
                # a key counts as priced if it is mapped AND carries a
                # number, or if it is one of the forms handled by code
                # rather than by a table lookup: `_EFF_SPECIAL` (a dict, an
                # offset, a presence flag) or, under --board, a key the
                # board-aware evaluator reads straight off the engine, whose
                # printed value is very often a bare `True`.
                if k in ok and (num or (block == "effects"
                                        and (k in W._EFF_SPECIAL
                                             or (board
                                                 and k in BY.BOARD_PRICED)))):
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
    ap.add_argument("--board", action="store_true",
                    help="count engine/bots/board_yields.py as well, on a "
                         "board stocked with one of everything")
    a = ap.parse_args(argv)
    if a.legacy:
        use_legacy_maps()

    types, dropped, zero, keys, nonnum, cards = scan(a.board)
    db = C.db()
    civil = [t for t in types if any(
        reachable(db.get(n)) for n, (tt, _, _) in cards.items() if tt == t)]
    mil = [t for t in types if t not in civil]

    def _rows(group):
        for t in sorted(group, key=lambda x: (-types[x], x)):
            print(f"{t:16s} {types[t]:4d} {dropped[t]:8d} {zero[t]:10d}")

    def _tot(group, label):
        n = sum(types[t] for t in group)
        d = sum(dropped[t] for t in group)
        z = sum(zero[t] for t in group)
        print(f"{label:16s} {n:4d} {d:8d} {z:10d}")
        return n, d, z

    print(f"{'type':16s} {'n':>4s} {'dropped':>8s} {'zero-gain':>10s}")
    print("-- CIVIL ROW: `_card_yields` is asked about these ------------")
    _rows(civil)
    _tot(civil, "SUBTOTAL")
    print()
    print("-- MILITARY DECK: `_card_yields` is NEVER asked --------------")
    print("   `hand_potential` walks hand_civil only.  A dropped key here")
    print("   is not a blind spot; mapping it would change nothing.")
    print("   Priced elsewhere -- see the module docstring.")
    _rows(mil)
    _tot(mil, "SUBTOTAL")
    print()
    _tot(list(types), "TOTAL (both)")

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
