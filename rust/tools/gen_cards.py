#!/usr/bin/env python3
"""Generate `rust/src/card_table.rs` from the same `data/*.json` the Python
engine loads.

DESIGN.md rule 2: static data is generated and checked in, not parsed at
start-up.  The Rust engine therefore has no dependencies, no start-up cost and
no `serde`, and a change to the card data shows up as a reviewable diff.

Run from anywhere:

    python3.13 rust/tools/gen_cards.py

It is deliberately strict.  Every failure mode here is the project's recurring
bug class -- a value present in one registry and absent from another, with
nothing that fails when they disagree -- so an unrecognised card type, a
non-integer cost or an effect key that maps nowhere is a hard error, never a
default.  The whole point of generating this table is that the Rust `match`
over `Special` is exhaustive; silently dropping a key would hand that guarantee
back.
"""
from __future__ import annotations

import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.normpath(os.path.join(HERE, "..", ".."))
DATA = os.path.join(ROOT, "data")
OUT = os.path.join(ROOT, "rust", "src", "card_table.rs")

PART_FILES = [
    "cards_civil.json",
    "cards_wonders_leaders.json",
    "cards_military_actions.json",
]

CIVIL_ROW_TYPES = {
    "farm", "mine", "lab", "temple", "arena", "library", "theater",
    "infantry", "cavalry", "artillery", "air", "government", "special-tech",
    "wonder", "leader", "action",
}

#: JSON `type` -> Rust `CardType` variant.  Exhaustive: a new type is an error,
#: because `CardType` is hand-written and the two must not drift.
TYPES = {
    "farm": "Farm", "mine": "Mine", "lab": "Lab", "temple": "Temple",
    "library": "Library", "arena": "Arena", "theater": "Theater",
    "infantry": "Infantry", "cavalry": "Cavalry", "artillery": "Artillery",
    "air": "Air", "government": "Government", "special-tech": "SpecialTech",
    "wonder": "Wonder", "leader": "Leader", "action": "Action",
    "tactic": "Tactic", "aggression": "Aggression", "war": "War",
    "pact": "Pact", "bonus": "Bonus", "territory": "Territory",
    "event": "Event",
}

AGES = {"A": "A", "I": "I", "II": "II", "III": "III", "IV": "IV"}

#: Effect keys that RECUR, mapped to their `CardEffects` field.  These are read
#: on every stats recomputation, so they are field loads.  Keys NOT listed here
#: become `Special` variants -- see the module docstring.
#:
#: Sources are two: a top-level card key (governments print their civil actions
#: on the card) and a key inside `effects`.  Both land in the same field, which
#: is the point: the consumer should not care where the printer put the number.
EFFECT_FIELDS = {
    "culture": "culture",
    "science": "science",
    "strength": "strength",
    "happy": "happy",
    "civilActions": "civil_actions",
    "militaryActions": "military_actions",
    "gainCulture": "gain_culture",
    "gainScience": "gain_science",
    "gainFood": "gain_food",
    "gainResources": "gain_resources",
    "buildDiscount": "build_discount",
    "resourceDiscount": "resource_discount",
    "resourcesForMilitaryUnits": "resources_for_military_units",
    "tacticBonus": "tactic_bonus",
    "tacticBonusObsolete": "tactic_bonus_obsolete",
    "defenseBonus": "defense_bonus",
    "colonizationBonus": "colonization_bonus",
    "colonizeBonus": "colonize_bonus",
    "blueTokens": "blue_tokens",
    "onBuildCulture": "on_build_culture",
    "wonderStagesPerAction": "wonder_stages_per_action",
    "civilHandLimit": "civil_hand_limit",
    "militaryHandLimit": "military_hand_limit",
    "freeCivilAction": "free_civil_action",
    "urbanBuildingLimit": "urban_building_limit",
}

#: The six keys `production` dicts print, mapped onto `cards::Production`
#: fields (the mapping is the identity today, but is spelled out so a JSON
#: key rename is a one-line change here rather than an assumption baked into
#: the loop below).  A card may print SEVERAL of these AT ONCE -- Religion is
#: `{culture: 1, happy: 1}`, Printing Press is `{science: 1, culture: 1}` --
#: which is exactly why `Card.production` is a struct and not the single
#: scalar this generator used to emit.
#:
#: Found incomplete during the effects.rs port (2026-08-05): the original
#: generator only ever read `prod.get("food") or prod.get("resources") or 0`,
#: so every urban building -- Philosophy's science, Religion's culture and
#: happy, every lab/temple/library/arena/theater in the game -- silently
#: generated as production-less, and a card printing BOTH food and resources
#: (never happens today, but nothing checked) would have silently kept only
#: one.  `_unknown_production_keys` below is the regression test: it fails
#: generation outright if `data/*.json` ever grows a 7th key.
PRODUCTION_FIELDS = {
    "food": "food",
    "resources": "resources",
    "culture": "culture",
    "science": "science",
    "happy": "happy",
    "strength": "strength",
}

#: Keys carried for humans, not the engine.  Dropping these is the ONLY place
#: this generator is allowed to ignore something, and each is listed
#: individually so the list is auditable.
IGNORED_KEYS = {
    "note",       # prose annotation on the data, not a rule
    "source",     # provenance of the transcription
    "uncertain",  # transcription confidence, tracked in the JSON
    "aka",        # alternative printed name
    "text",       # the printed rules text
    "countSource",
}


def camel(key: str) -> str:
    """`gainCulturePerLevelOfRemovedCard` -> `GainCulturePerLevelOfRemovedCard`."""
    s = re.sub(r"[^0-9a-zA-Z]+", " ", key).strip()
    if not s:
        raise ValueError(f"un-nameable effect key {key!r}")
    return s[0].upper() + s[1:].replace(" ", "")


def as_int(v, what):
    """Costs and yields must be integers.  A bool is not an integer here: JSON
    `true` for a numeric field means the transcription is unfinished, and
    letting it through as 1 would price a card off a typo."""
    if v is None:
        return 0
    if isinstance(v, bool):
        raise TypeError(f"{what}: expected a number, got {v!r}")
    if isinstance(v, int):
        return v
    raise TypeError(f"{what}: expected a number, got {v!r}")


def load_cards():
    cards = []
    for fn in PART_FILES:
        with open(os.path.join(DATA, fn)) as fh:
            part = json.load(fh)
        if part.get("scope") != "base-2015":
            raise ValueError(f"{fn}: wrong scope {part.get('scope')!r} -- this "
                             f"port is base-2015 only, the expansion is out of "
                             f"scope by standing decision")
        for c in part["cards"]:
            c.setdefault("deck",
                         "civil" if c["type"] in CIVIL_ROW_TYPES else "military")
            c.setdefault("effects", {})
            c.setdefault("count", {"2p": 1, "3p": 1, "4p": 1})
            c["baseName"] = c["name"]
            cards.append(c)
    # Mirrors `cards._disambiguate`: a few military cards share a printed name
    # across ages, and the name is the identity used by decks/hands/the row.
    seen = {}
    for c in cards:
        seen[c["name"]] = seen.get(c["name"], 0) + 1
    for c in cards:
        if seen[c["name"]] > 1:
            c["name"] = f"{c['baseName']} ({c['age']})"
    return cards


def main():
    cards = load_cards()

    # variant name -> "int" (carries an `(i16)` payload) or "unit" (bare).
    # A key that shows up as both across different cards is a modelling
    # question this generator cannot resolve silently -- it must fail instead
    # of picking one shape and hiding the other card's value (the exact bug
    # class this whole generator exists to refuse).
    specials = {}
    rows = []
    for c in cards:
        name = c["name"]
        kind = TYPES.get(c["type"])
        if kind is None:
            raise ValueError(f"{name}: unknown card type {c['type']!r}; add it "
                             f"to CardType in cards.rs AND to TYPES here")
        age = AGES[c["age"]]

        fields = {v: 0 for v in EFFECT_FIELDS.values()}
        mine = []  # [(variant, payload_or_None), ...]

        # Top-level printed numbers that are effects in all but name.
        for key in ("strength", "civilActions", "militaryActions",
                    "urbanBuildingLimit"):
            if key in c and c[key] is not None:
                fields[EFFECT_FIELDS[key]] = as_int(c[key], f"{name}.{key}")

        for key, val in (c.get("effects") or {}).items():
            if key in IGNORED_KEYS:
                continue
            field = EFFECT_FIELDS.get(key)
            if field is not None and isinstance(val, (int, float)) and not isinstance(val, bool):
                fields[field] = as_int(val, f"{name}.effects.{key}")
                continue
            # Either a one-off rule, or a recurring key whose value is a
            # structure (event targeting).  Both are code, not data -- but a
            # plain int/float (not bool) magnitude must not be thrown away:
            # Sid Meier's sciencePerLab is -1 (a REDUCTION), Napoleon's
            # strengthPerUnitType is 2, Shakespeare's culturePerLibraryTheaterPair
            # is 2, James Cook's cultureFirstColony is 2 -- a bare unit variant
            # would silently read all four as their Rust match arm's assumed
            # 1, which for Sid Meier is not just imprecise, it is the wrong
            # SIGN.  Bool-flag keys (`sciencePerBestLabOrLibraryLevel: true`
            # and friends) carry no magnitude in Python either -- the matching
            # `_apply_modifier` branch ignores `val` for those -- so they stay
            # bare unit variants.
            variant = camel(key)
            if isinstance(val, bool):
                shape = "unit"
                payload = None
            elif isinstance(val, (int, float)):
                shape = "int"
                payload = as_int(val, f"{name}.effects.{key}")
            else:
                shape = "unit"
                payload = None
            prev = specials.get(variant)
            if prev is not None and prev != shape:
                raise ValueError(
                    f"{name}: Special::{variant} used as both {prev!r} and "
                    f"{shape!r} across cards (key {key!r}) -- gen_cards.py "
                    f"cannot pick one shape silently, give it bespoke handling")
            specials[variant] = shape
            mine.append((variant, payload))

        prod = c.get("production") or {}
        unknown = set(prod) - set(PRODUCTION_FIELDS)
        if unknown:
            raise ValueError(
                f"{name}: production key(s) {sorted(unknown)!r} not in "
                f"PRODUCTION_FIELDS -- add them to cards::Production and to "
                f"PRODUCTION_FIELDS, do not let them fall on the floor "
                f"(this is exactly the bug the urban buildings had)")
        production = {field: as_int(prod.get(key), f"{name}.production.{key}")
                     for key, field in PRODUCTION_FIELDS.items()}

        count = c["count"]
        rows.append({
            "name": name,
            "base_name": c["baseName"],
            "kind": kind,
            "age": age,
            "science_cost": as_int(c.get("techCost"), f"{name}.techCost"),
            "resource_cost": as_int(c.get("buildCost"), f"{name}.buildCost"),
            "count": [as_int(count.get(f"{n}p", 0), f"{name}.count") for n in (2, 3, 4)],
            "production": production,
            "effects": fields,
            "special": mine,
        })

    ordered = sorted(specials)
    eff_order = list(EFFECT_FIELDS.values())
    prod_order = list(PRODUCTION_FIELDS.values())

    out = []
    w = out.append
    w("// @generated by rust/tools/gen_cards.py -- DO NOT EDIT BY HAND.")
    w("//")
    w("// Regenerate with `python3.13 rust/tools/gen_cards.py` whenever")
    w("// `data/*.json` changes.  Checked in deliberately (DESIGN.md rule 2):")
    w("// the engine parses no JSON and has no dependencies, and a card-data")
    w("// change arrives as a reviewable diff rather than a runtime surprise.")
    w("")
    w("use crate::cards::{Age, Card, CardEffects, CardType, Production};")
    w("")
    w(f"pub const NUM_CARDS: usize = {len(rows)};")
    w("")
    w("/// One card's unique rule. Generated: one variant per effect key that is")
    w("/// not a recurring numeric field. The `match` over this in `effects.rs`")
    w("/// is exhaustive, so a card the engine cannot interpret is a COMPILE")
    w("/// ERROR -- which is the guarantee the Python name-dispatch cannot give.")
    w("///")
    w("/// A variant carries an `(i16)` payload when the printed effect has a")
    w("/// magnitude Python's own dispatch reads (`val` used in `_apply_modifier`")
    w("/// / `_apply_special`); it stays a bare unit variant when Python ignores")
    w("/// the JSON value too (a bool-flag key: presence is the whole rule).")
    w("#[derive(Clone, Copy, PartialEq, Eq, Debug)]")
    w("pub enum Special {")
    for v in ordered:
        if specials[v] == "int":
            w(f"    {v}(i16),")
        else:
            w(f"    {v},")
    w("}")
    w("")
    w("pub static CARDS: [Card; NUM_CARDS] = [")
    for r in rows:
        eff = ", ".join(f"{k}: {r['effects'][k]}" for k in eff_order)
        parts = []
        for variant, payload in r["special"]:
            if payload is None:
                parts.append(f"Special::{variant}")
            else:
                parts.append(f"Special::{variant}({payload})")
        sp = ", ".join(parts)
        w("    Card {")
        w(f"        name: {json.dumps(r['name'])},")
        w(f"        base_name: {json.dumps(r['base_name'])},")
        w(f"        kind: CardType::{r['kind']},")
        w(f"        age: Age::{r['age']},")
        w(f"        science_cost: {r['science_cost']},")
        w(f"        resource_cost: {r['resource_cost']},")
        w(f"        count: [{', '.join(str(x) for x in r['count'])}],")
        prod = ", ".join(f"{k}: {r['production'][k]}" for k in prod_order)
        w(f"        production: Production {{ {prod} }},")
        w(f"        effects: CardEffects {{ {eff} }},")
        w(f"        special: &[{sp}],")
        w("    },")
    w("];")
    w("")

    with open(OUT, "w") as fh:
        fh.write("\n".join(out))

    print(f"{OUT}: {len(rows)} cards, {len(ordered)} Special variants")
    return 0


if __name__ == "__main__":
    sys.exit(main())
