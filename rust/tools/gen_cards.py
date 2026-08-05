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
over `Special` is exhaustive; silently dropping a key would hand that
guarantee back.

## Every key must be classified

That principle applies to every key this generator reads, not just the ones
that become `Special` variants.  A card-level key (`data/*.json`'s per-card
dict) and an `effects`-dict key (including a key nested inside a dict-valued
effect, like a pact's `A` block) must each land in exactly one of:

  * a key this function reads structurally (STRUCTURAL_KEYS / EFFECT_FIELDS /
    PRODUCTION_FIELDS / PACT_BLOCK_FIELDS / COLONY_PERMANENT_FIELDS /
    COLONY_POOL_FIELDS / IMMEDIATE_EFFECT_FIELDS -- whichever loop is
    processing it);
  * IGNORED_KEYS, for provenance/prose the engine never reads;
  * IMPLEMENTED_TOP_KEYS, for the top-level keys this pass newly wires up;
  * a DEFERRED set (DEFERRED_TOP_KEYS / DEFERRED_DICT_EFFECT_KEYS), each
    entry carrying a one-line reason a human can check.

A key outside all of those stops the build.  A DEFERRED key still produces a
payload-less `Special` variant where that was already true before this pass
(so an unported module's rule is still visible in `special: &[...]`) -- it is
just no longer possible for a *new* key to fall through that path silently.
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

#: `CardEffects` fields not written through `EFFECT_FIELDS`'s top-level/
#: `effects`-dict pipeline -- only through a territory's `permanentEffects`
#: (see `COLONY_PERMANENT_FIELDS`/`COLONY_POOL_FIELDS` below). Listed here so
#: the initial `fields` dict (and therefore the emitted `CardEffects { ... }`
#: literal) always has a slot for them.
EXTRA_CARD_EFFECTS_FIELDS = ("food", "resources", "yellow_tokens")

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
#: individually so the list is auditable.  Read both at the top level and
#: inside any nested block (`effects.<pactBlock>.note` etc) -- provenance
#: keys can appear at either depth.
IGNORED_KEYS = {
    "note",       # prose annotation on the data, not a rule
    "source",     # provenance of the transcription
    "uncertain",  # transcription confidence, tracked in the JSON
    "aka",        # alternative printed name
    "text",       # the printed rules text
    "countSource",
}

# ------------------------------------------------- top-level key census -----
#
# Every key `load_cards()` can produce on a card dict must appear in exactly
# one of the four sets below (checked in `main()`).  See the module docstring.

STRUCTURAL_KEYS = {
    "name", "baseName", "type", "deck", "age", "count", "cost",
    "production", "effects", "techCost", "buildCost", "composition",
    "obsoleteStrength", "strength", "civilActions", "militaryActions",
    "urbanBuildingLimit",
}

#: New this pass -- see `cards.rs`'s `Card`/`CardEffects`/`ImmediateEffects`.
IMPLEMENTED_TOP_KEYS = {
    "peacefulCost", "revolutionCost", "stages",
    "permanentEffects", "immediateEffects",
}

#: Present in `data/*.json`, read by nothing ported yet. Confirmed against
#: the live data 2026-08-05 -- re-verify the reason if the census changes.
DEFERRED_TOP_KEYS = {
    "target": "event/aggression/war targeting text -- events.rs/combat.rs "
              "not ported",
    "sides": "pact A/B side hint (['A','B'] or null); redundant with which "
             "of effects.A/effects.B/effects.bothPlayers the card actually "
             "has -- not modeled separately",
    "urbanLimitCategory": "always equals the card's own `type` for every "
             "urban building in the base data (verified 2026-08-05) -- "
             "redundant with CardType, not modeled separately",
    "scoringEvent": "marks an end-game 'Impact of X' scoring event card -- "
             "events.rs scoring is not ported",
}

# ------------------------------------------------ pact blocks (A/B/...) -----
#
# `A`, `B`, `bothPlayers` and `onAttackBetweenParties` are the four
# dict-valued `effects` keys pact cards print (§5.9). Each becomes a
# `Special::<Name>(PactBlock)` -- see `cards::PactBlock`.

PACT_BLOCK_KEYS = {"A", "B", "bothPlayers", "onAttackBetweenParties"}

#: `data/*.json` pact-block sub-key -> `cards::PactBlock` field. Exhaustive
#: against the live data (10 pact cards, 2026-08-05): a sub-key outside this
#: map (other than `IGNORED_KEYS`) stops the build.
PACT_BLOCK_FIELDS = {
    "cultureProduction": "culture",
    "foodProduction": "food",
    "resourceProduction": "resources",
    "strength": "strength",
    "militaryActions": "military_actions",
    "technologyScienceDiscount": "tech_discount",
    "cannotBeDeclaredWarOnByAnyone": "war_immune",
    "mayUseFoodAsResource": "food_as_resource",
    "mayUseResourceAsFood": "resource_as_food",
    "otherPartyPaysScience": "other_party_pays_science",
    "cultureProductionPerCompletedWonderOfTheOtherParty":
        "culture_per_wonder_of_other_party",
    "attackerStrength": "attacker_strength",
}
#: `PactBlock` fields whose JSON value is a flag (Python ignores the
#: magnitude too -- `Stats.war_immune`/`Stats.science_partners` are booleans
#: in effects.py's own model, not accumulators).
PACT_BLOCK_BOOL_FIELDS = {"war_immune", "other_party_pays_science"}
#: Declaration order for the emitted `PactBlock { ... }` literal -- must name
#: every field in `cards::PactBlock` exactly once.
PACT_BLOCK_FIELD_ORDER = (
    "culture", "food", "resources", "strength", "military_actions",
    "tech_discount", "war_immune", "food_as_resource", "resource_as_food",
    "other_party_pays_science", "culture_per_wonder_of_other_party",
    "attacker_strength",
)

#: The 20 OTHER dict-valued `effects` keys in the base data (2026-08-05
#: census: 24 distinct dict-valued keys total, 4 of them the pact blocks
#: above). Each still becomes a payload-less `Special` variant -- exactly
#: what every dict-valued key silently did before this pass -- but doing so
#: now requires being named here, with a reason, rather than falling through
#: an `else` branch that could not tell "known, deferred" apart from "new key
#: nobody has looked at yet".
DEFERRED_DICT_EFFECT_KEYS = {
    "allPlayers": "event targeting -- events.rs not ported",
    "weakestPlayer": "event targeting -- events.rs not ported",
    "weakestPlayers": "event targeting -- events.rs not ported",
    "strongestPlayer": "event targeting -- events.rs not ported",
    "strongestPlayers": "event targeting -- events.rs not ported",
    "condition": "event targeting -- events.rs not ported",
    "playersWithMostHappyFaces": "event targeting -- events.rs not ported",
    "playerWithLeastCulture": "event targeting -- events.rs not ported",
    "playersWithMostDiscontentWorkers": "event targeting -- events.rs not "
        "ported",
    "playerWithMostCulture": "event targeting -- events.rs not ported",
    "lastRoundSubstitute": "event resolution -- events.rs not ported",
    "gain": "event resolution -- events.rs not ported",
    "lose": "event resolution -- events.rs not ported",
    "takeFromOpponent": "aggression resolution -- combat.rs not ported",
    "victorTakesYellowTokens": "war resolution -- combat.rs not ported",
    "victorTakesCulture": "war resolution -- combat.rs not ported",
    "resourcesForMilitaryUnitsPerStrongerCivilization":
        "per-player-count action-card bonus -- actions.rs not ported",
    "culturePerCivilizationWithMoreCulture":
        "per-player-count action-card bonus -- actions.rs not ported",
    "perTurnChoice": "Churchill's per-turn choice structure -- actions.rs "
        "not ported",
    "buildDiscount": "per-age discount dict; build_cost is out of this "
        "port's scope today -- see effects.rs's own KNOWN GAPS",
}

#: List-valued `effects` keys -- a target filter (age list) or a set of
#: target types, never a magnitude. Same "must be named, not caught by an
#: `else`" treatment as `DEFERRED_DICT_EFFECT_KEYS`, kept as its own set
#: because a list needs no sub-key validation the way a dict block does.
DEFERRED_LIST_EFFECT_KEYS = {
    "destroyUrbanBuildings": "aggression resolution, age-filtered target "
        "list -- combat.rs not ported",
    "removeFromGame": "aggression resolution, target-type list -- combat.rs "
        "not ported",
}

# ---------------------------------------------------------- colony keys -----
#
# A territory (colonization) card prints `permanentEffects` (per-turn, once
# claimed -- §11.5) and `immediateEffects` (one-shot, paid the moment it is
# claimed). Both route through the SAME `cards::CardEffects` struct as an
# ordinary card's `effects` dict for the permanent half -- colonies are a
# second PRINTER of that vocabulary, not a distinct one; `engine/effects.py`
# FLAT_KEYS's own comment calls these "aliases used by colony permanents and
# pact effects". `immediateEffects` gets its own struct (`ImmediateEffects`,
# see cards.rs) since a one-shot amount and a per-turn rate must never be
# summed into the same field by accident.

#: Mirrors `engine/effects.py:556` `COLONY_PERMANENT_KEYS` exactly -- the set
#: `_colony_permanents` filters a territory's `permanentEffects` through
#: before handing it to `_apply_flat`. Read that set, don't invent one.
COLONY_PERMANENT_FIELDS = {
    "strength": "strength",
    "happiness": "happy",
    "happy": "happy",
    "cultureProduction": "culture",
    "culture": "culture",
    "scienceProduction": "science",
    "science": "science",
    "foodProduction": "food",
    "food": "food",
    "resourceProduction": "resources",
    "resources": "resources",
    "colonizationBonus": "colonization_bonus",
    "civilActions": "civil_actions",
    "militaryActions": "military_actions",
}
#: The two `permanentEffects` keys the live data actually uses that are NOT
#: in `COLONY_PERMANENT_KEYS`: `compute()` never reads them (confirmed
#: against effects.py -- `_colony_permanents` filters them out), only
#: `gain_colony`/`lose_colony` (§11.5, combat.rs, not ported) do, as a
#: one-shot pool grant/loss when a colony is claimed/lost. Captured on
#: `CardEffects` anyway (same field `blueTokens` already uses elsewhere) so
#: they are not silently dropped while combat.rs is unwritten.
COLONY_POOL_FIELDS = {
    "yellowTokens": "yellow_tokens",
    "blueTokens": "blue_tokens",
}

#: `data/*.json` `immediateEffects` key -> `cards::ImmediateEffects` field.
#: Exhaustive against the live data (12 territory cards, 2026-08-05).
IMMEDIATE_EFFECT_FIELDS = {
    "food": "food",
    "resources": "resources",
    "culture": "culture",
    "science": "science",
    "population": "population",
    "drawMilitaryCards": "draw_military_cards",
}
IMMEDIATE_EFFECT_FIELD_ORDER = (
    "food", "resources", "culture", "science", "population",
    "draw_military_cards",
)

#: Max stages any base-game wonder prints (Internet: 5). `cards::Card.stages`
#: is a slice, not bounded by this constant, but a card wider than every
#: wonder seen so far is worth a human's eyes before silently accepting it.
MAX_WONDER_STAGES = 5


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


def rust_bool(v: bool) -> str:
    return "true" if v else "false"


def build_pact_block(name, block_key, block):
    """`effects.<A|B|bothPlayers|onAttackBetweenParties>` -> a `PactBlock {
    ... }` Rust literal. Every sub-key must be `PACT_BLOCK_FIELDS` or
    `IGNORED_KEYS` -- an unrecognized one stops the build rather than
    silently vanishing (`IGNORED_KEYS` covers `note`, seen on Scientific
    Cooperation's `bothPlayers`)."""
    values = {f: (False if f in PACT_BLOCK_BOOL_FIELDS else 0)
              for f in PACT_BLOCK_FIELDS.values()}
    for k, v in block.items():
        if k in IGNORED_KEYS:
            continue
        field = PACT_BLOCK_FIELDS.get(k)
        if field is None:
            raise ValueError(
                f"{name}: effects.{block_key}.{k} is not a recognized pact "
                f"block key -- add it to PACT_BLOCK_FIELDS (with the "
                f"PactBlock field it feeds) or IGNORED_KEYS in gen_cards.py")
        if field in PACT_BLOCK_BOOL_FIELDS:
            values[field] = bool(v)
        else:
            values[field] = as_int(v, f"{name}.effects.{block_key}.{k}")
    parts = []
    for f in PACT_BLOCK_FIELD_ORDER:
        v = values[f]
        parts.append(f"{f}: {rust_bool(v) if f in PACT_BLOCK_BOOL_FIELDS else v}")
    return "PactBlock { " + ", ".join(parts) + " }"


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

    # variant name -> "int" | "unit" | "pact_block" (carries an `(i16)`, no
    # payload, or a `(PactBlock)` payload respectively). A key that shows up
    # as both across different cards is a modelling question this generator
    # cannot resolve silently -- it must fail instead of picking one shape
    # and hiding the other card's value (the exact bug class this whole
    # generator exists to refuse).
    specials = {}
    rows = []
    eff_order = list(EFFECT_FIELDS.values()) + list(EXTRA_CARD_EFFECTS_FIELDS)
    for c in cards:
        name = c["name"]

        unknown_top = (set(c) - STRUCTURAL_KEYS - IGNORED_KEYS
                      - IMPLEMENTED_TOP_KEYS - set(DEFERRED_TOP_KEYS))
        if unknown_top:
            raise ValueError(
                f"{name}: unrecognized top-level key(s) {sorted(unknown_top)!r}"
                f" -- classify in gen_cards.py as STRUCTURAL/IGNORED/"
                f"IMPLEMENTED/DEFERRED")

        kind = TYPES.get(c["type"])
        if kind is None:
            raise ValueError(f"{name}: unknown card type {c['type']!r}; add it "
                             f"to CardType in cards.rs AND to TYPES here")
        age = AGES[c["age"]]

        fields = {v: 0 for v in eff_order}
        mine = []  # [(variant, payload_or_None), ...]

        # Top-level printed numbers that are effects in all but name.
        for key in ("strength", "civilActions", "militaryActions",
                    "urbanBuildingLimit"):
            if key in c and c[key] is not None:
                fields[EFFECT_FIELDS[key]] = as_int(c[key], f"{name}.{key}")

        # `effects.tacticBonus` / `tacticBonusObsolete` are a duplicate
        # spelling of the top-level `strength` / `obsoleteStrength` that the
        # engine actually reads (`effects.py:991`, and `bots/weighted.py:2029`
        # calls them a duplicate in so many words). Storing both would be the
        # same fact in two registries with nothing failing when they disagree.
        # So: verify and drop. If the data ever disagrees with itself, that is
        # a transcription error and it stops the build instead of silently
        # picking one.
        _eff = c.get("effects") or {}
        for dup, printed in (("tacticBonus", "strength"),
                             ("tacticBonusObsolete", "obsoleteStrength")):
            if dup in _eff and as_int(_eff[dup], f"{name}.effects.{dup}") != \
                    as_int(c.get(printed), f"{name}.{printed}"):
                raise ValueError(
                    f"{name}: effects.{dup}={_eff[dup]!r} disagrees with "
                    f"printed {printed}={c.get(printed)!r} -- one of them is a "
                    f"transcription error, fix data/*.json")

        for key, val in _eff.items():
            if key in IGNORED_KEYS or key in ("tacticBonus", "tacticBonusObsolete"):
                continue
            field = EFFECT_FIELDS.get(key)
            if field is not None and isinstance(val, (int, float)) and not isinstance(val, bool):
                fields[field] = as_int(val, f"{name}.effects.{key}")
                continue
            if isinstance(val, dict):
                if key in PACT_BLOCK_KEYS:
                    variant = camel(key)
                    shape = "pact_block"
                    payload = build_pact_block(name, key, val)
                elif key in DEFERRED_DICT_EFFECT_KEYS:
                    variant = camel(key)
                    shape = "unit"
                    payload = None
                else:
                    raise ValueError(
                        f"{name}: effects.{key} is a dict-valued key this "
                        f"generator does not recognize -- add it to "
                        f"PACT_BLOCK_KEYS (with real payload handling) or "
                        f"DEFERRED_DICT_EFFECT_KEYS (with a one-line reason) "
                        f"in gen_cards.py")
            elif isinstance(val, list):
                if key not in DEFERRED_LIST_EFFECT_KEYS:
                    raise ValueError(
                        f"{name}: effects.{key} is a list-valued key this "
                        f"generator does not recognize -- add it to "
                        f"DEFERRED_LIST_EFFECT_KEYS (with a one-line reason) "
                        f"in gen_cards.py")
                variant = camel(key)
                shape = "unit"
                payload = None
            elif isinstance(val, bool):
                # Bool-flag keys carry no magnitude in Python's own dispatch
                # either (`_apply_modifier`/`_apply_special` ignore `val` for
                # these) -- presence is the whole rule, so a bare unit variant
                # is correct, not a gap.
                variant = camel(key)
                shape = "unit"
                payload = None
            elif isinstance(val, str):
                # A free-text formula (Fast Food Chains'/Hollywood's/
                # Internet's `onBuildCulture`, Raid's `gainResources`, War
                # over Technology's `victorTakesScienceUpTo`) or an action
                # identifier (`freeCivilAction`'s `build_one_wonder_stage`
                # and friends). Python resolves every one of these by NAME
                # dispatch in a bespoke function
                # (`_one_time_culture`/aggression resolution/
                # `free_action_moves`), never by reading this string as a
                # magnitude -- there is no `(i16)` to carry, so a bare unit
                # variant is correct here too, not a gap.
                variant = camel(key)
                shape = "unit"
                payload = None
            elif isinstance(val, (int, float)):
                # A magnitude Python's own dispatch DOES read (Sid Meier's
                # sciencePerLab is -1, a reduction; Napoleon's
                # strengthPerUnitType is 2) -- must not be thrown away.
                variant = camel(key)
                shape = "int"
                payload = as_int(val, f"{name}.effects.{key}")
            else:
                raise ValueError(
                    f"{name}: effects.{key} has value {val!r}, not an int/"
                    f"float/bool/dict -- gen_cards.py has no shape for this")
            prev = specials.get(variant)
            if prev is not None and prev != shape:
                raise ValueError(
                    f"{name}: Special::{variant} used as both {prev!r} and "
                    f"{shape!r} across cards (key {key!r}) -- gen_cards.py "
                    f"cannot pick one shape silently, give it bespoke handling")
            specials[variant] = shape
            mine.append((variant, payload))

        # Colony permanent/immediate effects (territory cards, §11.5). Both
        # dicts are entirely absent (`{}`) on every non-territory card, so
        # merging them into `fields` additively can never collide with a
        # regular `effects` value in the base game today.
        perm = c.get("permanentEffects") or {}
        for k, v in perm.items():
            field = COLONY_PERMANENT_FIELDS.get(k) or COLONY_POOL_FIELDS.get(k)
            if field is None:
                raise ValueError(
                    f"{name}: permanentEffects.{k} is not a recognized "
                    f"colony key -- add it to COLONY_PERMANENT_FIELDS "
                    f"(engine/effects.py COLONY_PERMANENT_KEYS) or "
                    f"COLONY_POOL_FIELDS in gen_cards.py")
            fields[field] = fields.get(field, 0) + as_int(
                v, f"{name}.permanentEffects.{k}")

        imm = c.get("immediateEffects") or {}
        imm_fields = {v: 0 for v in IMMEDIATE_EFFECT_FIELD_ORDER}
        for k, v in imm.items():
            field = IMMEDIATE_EFFECT_FIELDS.get(k)
            if field is None:
                raise ValueError(
                    f"{name}: immediateEffects.{k} is not recognized -- add "
                    f"it to IMMEDIATE_EFFECT_FIELDS in gen_cards.py")
            imm_fields[field] = as_int(v, f"{name}.immediateEffects.{k}")
        if imm and c["type"] != "territory":
            raise ValueError(f"{name}: only territory cards may print "
                             f"immediateEffects")
        if perm and c["type"] != "territory":
            raise ValueError(f"{name}: only territory cards may print "
                             f"permanentEffects")

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

        # `cost` is a dict and, in the base game, only ever names military
        # actions.  Checked rather than assumed: a second key here would be a
        # cost the engine never charges.
        cost = c.get("cost") or {}
        unknown = set(cost) - {"militaryActions"}
        if unknown:
            raise ValueError(
                f"{name}: cost key(s) {sorted(unknown)!r} are not charged by "
                f"anything -- add them to cards::Card and to this check")
        mil_cost = as_int(cost.get("militaryActions"), f"{name}.cost")

        # Tactics (§10).  A composition is a LIST of unit type names in the
        # data but a multiset everywhere it is read, so it is counted here
        # once instead of on every stats recomputation.  An unknown member is
        # a hard error: an army silently missing a unit would make the tactic
        # cheaper to field than the card says.
        comp = {"infantry": 0, "cavalry": 0, "artillery": 0, "air": 0}
        for unit in (c.get("composition") or []):
            if unit not in comp:
                raise ValueError(
                    f"{name}: composition names {unit!r}, which is not a unit "
                    f"type -- add it to cards::Composition and here")
            comp[unit] += 1
        if comp != {"infantry": 0, "cavalry": 0, "artillery": 0, "air": 0} \
                and c["type"] != "tactic":
            raise ValueError(f"{name}: only tactics may print a composition")

        # Wonder build stages (§9): resources per stage, in printed order.
        raw_stages = c.get("stages") or []
        if raw_stages and c["type"] != "wonder":
            raise ValueError(f"{name}: only wonders may print stages")
        stages = [as_int(x, f"{name}.stages") for x in raw_stages]
        if len(stages) > MAX_WONDER_STAGES:
            raise ValueError(
                f"{name}: stages has {len(stages)} entries, more than any "
                f"base-game wonder seen so far ({MAX_WONDER_STAGES}) -- a "
                f"human should confirm this before widening the assumption")

        # Government peaceful/revolution costs (§8.3/§8.3.4) -- two DIFFERENT
        # science prices for the same government, both must be representable
        # at once (standing decision, not a simplification opportunity).
        peaceful_cost = as_int(c.get("peacefulCost"), f"{name}.peacefulCost")
        revolution_cost = as_int(c.get("revolutionCost"),
                                 f"{name}.revolutionCost")
        if (peaceful_cost or revolution_cost) and c["type"] != "government":
            raise ValueError(
                f"{name}: peacefulCost/revolutionCost only expected on "
                f"governments")

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
            "military_action_cost": mil_cost,
            "composition": comp,
            "obsolete_strength": as_int(c.get("obsoleteStrength"),
                                        f"{name}.obsoleteStrength"),
            "special": mine,
            "stages": stages,
            "peaceful_cost": peaceful_cost,
            "revolution_cost": revolution_cost,
            "immediate_effects": imm_fields,
        })

    ordered = sorted(specials)
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
    w("use crate::cards::{Age, Card, CardEffects, CardType, Composition, "
      "ImmediateEffects, PactBlock, Production};")
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
    w("/// / `_apply_special`); a `(PactBlock)` payload when the printed value is")
    w("/// one of the four pact blocks (§5.9); and stays a bare unit variant when")
    w("/// Python ignores the JSON value too (a bool-flag key, or a dict-valued")
    w("/// key this port has not modeled yet -- see gen_cards.py's")
    w("/// DEFERRED_DICT_EFFECT_KEYS for the reason on each).")
    w("#[derive(Clone, Copy, PartialEq, Eq, Debug)]")
    w("pub enum Special {")
    for v in ordered:
        shape = specials[v]
        if shape == "int":
            w(f"    {v}(i16),")
        elif shape == "pact_block":
            w(f"    {v}(PactBlock),")
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
        w(f"        military_action_cost: {r['military_action_cost']},")
        comp = ", ".join(f"{k}: {r['composition'][k]}"
                         for k in ("infantry", "cavalry", "artillery", "air"))
        w(f"        composition: Composition {{ {comp} }},")
        w(f"        obsolete_strength: {r['obsolete_strength']},")
        w(f"        special: &[{sp}],")
        stages = ", ".join(str(x) for x in r["stages"])
        w(f"        stages: &[{stages}],")
        w(f"        peaceful_cost: {r['peaceful_cost']},")
        w(f"        revolution_cost: {r['revolution_cost']},")
        imm = ", ".join(f"{k}: {r['immediate_effects'][k]}"
                        for k in IMMEDIATE_EFFECT_FIELD_ORDER)
        w(f"        immediate_effects: ImmediateEffects {{ {imm} }},")
        w("    },")
    w("];")
    w("")

    with open(OUT, "w") as fh:
        fh.write("\n".join(out))

    print(f"{OUT}: {len(rows)} cards, {len(ordered)} Special variants")
    return 0


if __name__ == "__main__":
    sys.exit(main())
