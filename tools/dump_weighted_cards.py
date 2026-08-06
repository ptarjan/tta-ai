"""Dump `engine/bots/weighted.py`'s card-yield-plumbing layer for the Rust
port's differential test (`rust/tests/weighted_cards.rs`).

Same offline-oracle shape as `tools/dump_weighted_horizon.py`/
`tools/dump_board_yields.py` (read either script's own doc comment for the
full rationale). Unlike those two, most of what this script dumps is a PURE
function of card identity -- `_card_yields`, `_card_choices`, `_swap_type`,
`_board_credit_key`, `_is_unit`, `_is_levelled_tech`, `_is_action`,
`_is_government` all take a card name alone, no board, no weights -- so this
dumps EVERY card in the database once, rather than sampling: full coverage of
236 cards is cheap for a per-card table, where sampling several hundred board
STATES is what the `dump_fixtures.py`-based scripts do for board-aware
questions.

`_sum_yields` needs a weight vector too (but still no board), so it is dumped
once per card under a handful of representative vectors chosen to exercise
every `YieldKind`'s special-case arithmetic: the default vector, one that
drives the two COST weights negative (must clamp to 0, never read as a
gain), one that zeroes every credit (must recover the pre-fix pricing exactly
-- byte-for-byte the same guarantee `tests/test_card_pricing.py`'s
`TestLaneBWeightsAreInert` already pins in Python), and one that sets every
credit away from its default and away from 1.0.

`_yield_marginal` is NOT dumped here. It calls `feature_marginal`, which is
`engine/bots/weighted.py` lines 1004-1055 -- outside this port's line range
(1730-3211) and not yet ported to Rust (`rust/src/bots/weighted/rivals.rs` is
still a stub). The Rust side unit-tests `yield_marginal`'s own arithmetic (the
ring-fenced credit lookup and the two independent `max(0.0, ...)` clamps)
against a mock closure instead; a real differential test needs
`rivals::feature_marginal` to exist first.

Usage:

    python3.13 tools/dump_weighted_cards.py --out rust/tests/weighted_cards_fixtures
"""
from __future__ import annotations

import argparse
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from engine import cards as C                          # noqa: E402
from engine.bots import weighted as W                   # noqa: E402

_DUMP_JSON_KW = dict(sort_keys=True, separators=(",", ":"), indent=None)

_DB = C.db()

_KIND_NAME = {
    W._Y_GAIN: "gain",
    W._Y_COST: "cost",
    W._Y_RATE: "rate",
    W._Y_UNIT: "unit",
    W._Y_TERR: "territory",
    W._Y_BONUS: "bonus",
}


def _triples_json(triples):
    return [[k, a, _KIND_NAME[kind]] for k, a, kind in triples]


def _weight_vectors():
    """The representative vectors `sum_yields` is checked under -- see this
    script's own doc comment for why each one exists."""
    default = dict(W.DEFAULT_WEIGHTS)

    neg_cost = dict(default)
    neg_cost["science"] = -3.0
    neg_cost["resource_stock"] = -2.0

    zero_credit = dict(
        default,
        card_rate_credit=0.0,
        unit_strength_credit=0.0,
        territory_credit=0.0,
        bonus_card_credit=0.0,
    )

    boosted_credit = dict(
        default,
        card_rate_credit=2.0,
        unit_strength_credit=3.0,
        territory_credit=0.5,
        bonus_card_credit=4.0,
        restricted_resource_credit=0.7,
    )

    return {
        "default": default,
        "neg_cost": neg_cost,
        "zero_credit": zero_credit,
        "boosted_credit": boosted_credit,
    }


def dump(out_dir):
    os.makedirs(out_dir, exist_ok=True)
    names = sorted(_DB.by_name)
    vectors = _weight_vectors()

    card_yields = {}
    card_choice = {}
    sum_yields = {}
    board_credit_key = {}
    swap_type = {}
    is_unit = {}
    is_levelled_tech = {}
    is_action = {}
    is_government = {}

    for name in names:
        triples = W._card_yields(name)
        card_yields[name] = _triples_json(triples)

        choices = W._card_choices(name)
        # Python's shape is a tuple of groups, each a tuple of triple-tuples.
        # Every group in the base game today is exactly two branches of one
        # triple each (see `rust/src/bots/weighted/cards.rs::card_choice`'s
        # own doc comment on why the Rust side narrows this to
        # `Option<(CardYield, CardYield)>`) -- dumped as `null` when there is
        # no choice, else the one group's two branches.
        if choices:
            assert len(choices) == 1, f"{name}: more than one choice group, Rust's card_choice needs widening"
            group = choices[0]
            assert len(group) == 2, f"{name}: choice group is not a pair, Rust's card_choice needs widening"
            for branch in group:
                assert len(branch) == 1, f"{name}: a choice branch has more than one triple, Rust's card_choice needs widening"
            card_choice[name] = [_triples_json(branch)[0] for branch in group]
        else:
            card_choice[name] = None

        sum_yields[name] = {
            vname: W._sum_yields(triples, w, w.get("card_rate_credit", 1.0))
            for vname, w in vectors.items()
        }

        board_credit_key[name] = W._board_credit_key(name)
        swap_type[name] = W._swap_type(name)
        is_unit[name] = W._is_unit(name)
        is_levelled_tech[name] = W._is_levelled_tech(name)
        is_action[name] = W._is_action(name)
        is_government[name] = W._is_government(name)

    payload = {
        "card_yields": card_yields,
        "card_choice": card_choice,
        "sum_yields": sum_yields,
        "board_credit_key": board_credit_key,
        "swap_type": swap_type,
        "is_unit": is_unit,
        "is_levelled_tech": is_levelled_tech,
        "is_action": is_action,
        "is_government": is_government,
        "deliberately_unpriced": dict(W.DELIBERATELY_UNPRICED),
        "unpriced_values": sorted(
            [name, key, reason] for (name, key), reason in W.UNPRICED_VALUES.items()
        ),
    }

    path = os.path.join(out_dir, "card_yields.json")
    with open(path, "w") as f:
        json.dump(payload, f, **_DUMP_JSON_KW)
    return path, len(names)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--out", default="rust/tests/weighted_cards_fixtures")
    args = ap.parse_args(argv)

    path, n = dump(args.out)
    print(f"{n} cards -> {path}")


if __name__ == "__main__":
    main()
