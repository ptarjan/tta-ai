"""Dump `engine/bots/weighted.py`'s card-yield-plumbing AND valuation layers
for the Rust port's differential test (`rust/tests/weighted_cards.rs`).

Two dumps, two shapes, both written by this one script:

1. `dump()` -- the YIELD-PLUMBING layer (`_card_yields`, `_card_choices`,
   `_swap_type`, `_board_credit_key`, `_is_unit`, `_is_levelled_tech`,
   `_is_action`, `_is_government`). Same offline-oracle shape as
   `tools/dump_weighted_horizon.py`/`tools/dump_board_yields.py` (read either
   script's own doc comment for the full rationale). All of it is a PURE
   function of card identity -- no board, no weights except `_sum_yields`'s
   own credit -- so this dumps EVERY card in the database once, rather than
   sampling: full coverage of 236 cards is cheap for a per-card table, where
   sampling several hundred board STATES (below) is what a board-aware
   question needs. Written to `card_yields.json`, one JSON object.

2. `dump_valuation()` -- the VALUATION layer (`action_value`, `tech_value`,
   `gov_value`, `card_potential`, `hand_potential`, `wonder_potential`,
   `hand_mil_potential`, `rival_hand_potential`, `tactic_terms`), which
   genuinely IS board-aware (that is the whole point of the layer -- see
   `rust/src/bots/weighted/cards.rs`'s own top doc comment on the split).
   Same shape as `tools/dump_board_yields.py`/`tools/dump_weighted_row.py`:
   loads states already recorded by `dump_fixtures.py`
   (`rust/tests/fixtures/*.jsonl`), asks the real Python `weighted` module
   every valuation question for every live player on a STRIDE-sampled subset
   of those states, under several weight vectors (`_weight_vectors()` above,
   reused, PLUS `_valuation_vectors()` below -- the plumbing layer's four
   vectors never move the credits this layer's own dispatch/collapsing logic
   reads: `tech_board_credit`, `unit_tech_credit`, `gov_board_credit`,
   `action_board_credit`, `card_board_credit` and its four per-type offsets,
   `hand_swap_extra`, `free_action_credit`). Written to
   `<out>/<fixture-name>.jsonl`, one JSON object per sampled ply -- the same
   filename convention `dump_board_yields.py`/`dump_weighted_row.py` use, so
   `card_yields.json` (a single JSON object, no `.jsonl` sibling) and the
   per-fixture `.jsonl` files coexist in the same `--out` directory without
   colliding.

`_sum_yields` (part of dump 1) needs a weight vector too (but still no
board), so it is dumped once per card under a handful of representative
vectors chosen to exercise every `YieldKind`'s special-case arithmetic: the
default vector, one that drives the two COST weights negative (must clamp to
0, never read as a gain), one that zeroes every credit (must recover the
pre-fix pricing exactly -- byte-for-byte the same guarantee
`tests/test_card_pricing.py`'s `TestLaneBWeightsAreInert` already pins in
Python), and one that sets every credit away from its default and away from
1.0.

Usage:

    python3.13 tools/dump_weighted_cards.py --out rust/tests/weighted_cards_fixtures
    python3.13 tools/dump_weighted_cards.py --out rust/tests/weighted_cards_fixtures \\
        --fixtures rust/tests/fixtures --stride 15
"""
from __future__ import annotations

import argparse
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from engine import cards as C                          # noqa: E402
from engine.state import GameState                     # noqa: E402
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


# ================================================== the valuation layer dump


def _valuation_vectors():
    """Extra vectors exercising the VALUATION layer's own credits -- see this
    script's own doc comment. The plumbing layer's four `_weight_vectors()`
    never move `tech_board_credit`/`unit_tech_credit`/`gov_board_credit`/
    `action_board_credit`/`card_board_credit` (+ its four per-type offsets)/
    `hand_swap_extra`/`free_action_credit`, which `card_potential`'s dispatch
    and `_hand_total`'s slot-collapsing both depend on -- `credits_off` sends
    every dispatch branch back to the static table (the A/B control arm this
    whole layer is measured against); `board_on` turns every board-aware
    branch AND the swap/choice/board_extra paths on at once, with each
    per-type offset a DIFFERENT number so a transposition between two offsets
    would not cancel out."""
    default = dict(W.DEFAULT_WEIGHTS)

    board_on = dict(
        default,
        card_board_credit=1.0,
        card_board_leader=0.5,
        card_board_government=0.3,
        card_board_action=0.4,
        card_board_wonder=0.6,
        hand_swap_extra=0.5,
        free_action_credit=0.3,
    )

    credits_off = dict(
        default,
        tech_board_credit=0.0,
        gov_board_credit=0.0,
        action_board_credit=0.0,
        unit_tech_credit=0.0,
    )

    return {"board_on": board_on, "credits_off": credits_off}


def _card_names_of_interest(state, idx):
    """Every card name this ply's decision for `idx` could plausibly price --
    idx's own civil and military hands, plus everything currently in the row
    (a rival's `rival_hand_potential` call reprices these same functions from
    THEIR seat, covered by the outer per-player loop rather than duplicated
    here)."""
    p = state.players[idx]
    names = set(p.hand_civil) | set(p.hand_military)
    names |= {n for n in state.card_row if n is not None}
    return sorted(names)


def _one_state_valuation(state, idx, vectors):
    """Every valuation-layer answer for player `idx` on `state`, under every
    vector in `vectors`, as a plain-JSON dict. `tactic_terms` takes no weight
    vector (`engine/bots/weighted.py::tactic_terms(state, idx)`), so it is
    dumped once, outside the per-vector loop."""
    names = _card_names_of_interest(state, idx)
    out_vectors = {}
    for vname, w in vectors.items():
        out_vectors[vname] = {
            "card_potential": {n: W.card_potential(n, w, state, idx) for n in names},
            "action_value": {n: W.action_value(n, state, idx, w) for n in names if W._is_action(n)},
            "tech_value": {
                n: W.tech_value(n, state, idx, w)
                for n in names
                if W._is_unit(n) or W._is_levelled_tech(n)
            },
            "gov_value": {n: W.gov_value(n, state, idx, w) for n in names if W._is_government(n)},
            "hand_potential": W.hand_potential(state, idx, w),
            "wonder_potential": W.wonder_potential(state, idx, w),
            "hand_mil_potential": W.hand_mil_potential(state, idx, w),
            "rival_hand_potential": W.rival_hand_potential(state, idx, w),
        }
    gain, short = W.tactic_terms(state, idx)
    return {"tactic_terms": [gain, short], "vectors": out_vectors}


def dump_valuation_file(path, out_path, stride, vectors):
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
            per_player[str(idx)] = _one_state_valuation(state, idx, vectors)
        records.append({"ply": rec["ply"], "players": per_player})

    with open(out_path, "w") as f:
        for r in records:
            f.write(json.dumps(r, **_DUMP_JSON_KW) + "\n")
    return len(records)


def dump_valuation(fixtures_dir, out_dir, stride):
    os.makedirs(out_dir, exist_ok=True)
    vectors = dict(_weight_vectors(), **_valuation_vectors())
    total = 0
    files = 0
    for name in sorted(os.listdir(fixtures_dir)):
        if not name.endswith(".jsonl"):
            continue
        src = os.path.join(fixtures_dir, name)
        dst = os.path.join(out_dir, name)
        n = dump_valuation_file(src, dst, stride, vectors)
        total += n
        files += 1
        print(f"{name}: {n} sampled states -> {dst}")
    return files, total


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--out", default="rust/tests/weighted_cards_fixtures")
    ap.add_argument("--fixtures", default="rust/tests/fixtures")
    ap.add_argument("--stride", type=int, default=7)
    args = ap.parse_args(argv)

    path, n = dump(args.out)
    print(f"{n} cards -> {path}")

    files, total = dump_valuation(args.fixtures, args.out, args.stride)
    print(f"valuation: {files} files, {total} sampled states -> {args.out}")


if __name__ == "__main__":
    main()
