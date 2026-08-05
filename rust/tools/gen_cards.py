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

#: Age order, i.e. the index `cards::Age as u8` gives each age.  Used to turn
#: an age-keyed effect dict into the fixed-size array `AGE_ARRAY_EFFECT_KEYS`
#: emits; must stay in step with `cards::Age`'s `#[repr(u8)]` discriminants.
AGE_ORDER = ("A", "I", "II", "III", "IV")

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
    # `buildDiscount` is NOT here: it is a per-age DICT on every card that
    # prints it, so it could never have reached a scalar `CardEffects` field
    # -- see `AGE_ARRAY_EFFECT_KEYS` below, which gives it a real
    # `Special::BuildDiscount([i16; 5])` payload instead. (It was listed here
    # until 2026-08-05, which made `CardEffects.build_discount` a field that
    # was zero on all 236 cards while looking, to a reader, like the printed
    # construction discount -- the same-fact-two-registries shape this
    # generator exists to refuse.)
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
#: `scoringEvent` (added for the §12.5.2 final-scoring port, 2026-08-05) is
#: read directly in the main loop below -- true on exactly the 15 base-game
#: Age III "Impact of ..." event cards, and is what selects the
#: `effects.allPlayers` -> `FinalScoringBlock` payload path over the
#: ordinary payload-less `DEFERRED_DICT_EFFECT_KEYS["allPlayers"]` one.
IMPLEMENTED_TOP_KEYS = {
    "peacefulCost", "revolutionCost", "stages",
    "permanentEffects", "immediateEffects", "scoringEvent",
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

#: The OTHER dict-valued `effects` keys in the base data still awaiting a
#: real port. Event targeting/resolution (`allPlayers` and its 12 siblings)
#: is handled below now (`EVENT_BLOCK_DICT_KEYS`/`build_event_block` etc,
#: 2026-08-05 events.rs pass) -- what is left here belongs to `combat.rs`
#: (war spoils) or `actions.rs` (per-player-count action-card bonuses,
#: Churchill's per-turn choice), neither in scope for that pass. Each
#: remaining key still becomes a payload-less `Special` variant -- exactly
#: what every dict-valued key silently did before this pass -- but doing so
#: now requires being named here, with a reason, rather than falling through
#: an `else` branch that could not tell "known, deferred" apart from "new key
#: nobody has looked at yet".
DEFERRED_DICT_EFFECT_KEYS = {
    "victorTakesYellowTokens": "war resolution -- combat.rs not ported",
    "victorTakesCulture": "war resolution -- combat.rs not ported",
    "resourcesForMilitaryUnitsPerStrongerCivilization":
        "per-player-count action-card bonus -- actions.rs not ported",
    "culturePerCivilizationWithMoreCulture":
        "per-player-count action-card bonus -- actions.rs not ported",
    "perTurnChoice": "Churchill's per-turn choice structure -- actions.rs "
        "not ported",
}

#: Dict-valued `effects` keys whose keys are AGES and whose values are a
#: magnitude per age (`{"I": 1, "II": 2, "III": 3}`).  Emitted as a
#: `[i16; 5]` payload indexed by `cards::Age as u8` -- a fixed-size array,
#: not a map (DESIGN.md rule 3), so the consumer indexes rather than looks up
#: and an age nobody printed is a real 0 rather than a missing key.
#:
#: `buildDiscount` (Masonry / Architecture / Engineering, the three
#: construction special-techs) is the only such key in the base game.
#: `engine/effects.py::_apply_special` accumulates it into
#: `Stats.build_discount` by SUMMING per age across every source, which
#: `effects.rs`'s `Special::BuildDiscount` arm mirrors; `costs.rs::
#: build_cost_for` then subtracts the entry for the card's own age.
AGE_ARRAY_EFFECT_KEYS = {"buildDiscount"}

#: `effects`-dict keys whose VALUE Python confirms it never reads -- prose,
#: not a rule -- despite the KEY sometimes mattering (Barbarians' `target` is
#: checked for PRESENCE, `events.py:182`: `"target" in eff and
#: "decreasePopulation" in eff`, alongside `condition`/`decreasePopulation`,
#: both independently captured by their OWN Special variants on the same
#: card, so nothing is lost by not modelling `target`'s text too). Verified
#: against `engine/*.py` 2026-08-05: `duration` does not appear in a single
#: `.get(...)` call anywhere in `engine/`. Scoped to the nested `effects`
#: dict only (not `IGNORED_KEYS`): the TOP-LEVEL `target` key on aggression/
#: war cards is a real, structured value combat.rs will need, and belongs in
#: `DEFERRED_TOP_KEYS` instead -- conflating the two under one name would
#: bury that distinction.
IGNORED_NESTED_EFFECT_KEYS = {"target", "duration"}

#: List-valued `effects` keys -- a target filter (age list) or a set of
#: target types, never a magnitude. Same "must be named, not caught by an
#: `else`" treatment as `DEFERRED_DICT_EFFECT_KEYS`, split into two sets
#: because the two base-game list keys need different shapes.
#:
#: `destroyUrbanBuildings`: a list of per-raid `{"maxAge": "<Age>"}` specs,
#: one raid per entry, in printed order -- `engine/events.py::
#: finish_aggression` (672-675) loops this list reading `spec.get("maxAge",
#: "A")` from each entry, so `combat.rs` needs the actual ages, not just the
#: key's presence. Given a real payload below: `&'static [Age]`, built by
#: `build_age_list`.
LIST_AGE_EFFECT_KEYS = {"destroyUrbanBuildings"}

#: `removeFromGame`: a list of target-TYPE strings (`["leader",
#: "unfinishedWonder"]` on the only base-game printing, Infiltrate).
#: `engine/events.py::finish_aggression` (line 679) only ever tests
#: `if eff.get("removeFromGame"):` -- the list's CONTENTS are never read
#: (which targets are actually offered is derived structurally in
#: `interact.rs`'s `QueueItem::Infiltrate` handler, from which of the
#: victim's leader/wonder exist) -- so this stays the payload-less unit
#: variant it already was. Not "deferred" any more, though:
#: `combat.rs::finish_aggression` reads its PRESENCE now.
LIST_PRESENCE_EFFECT_KEYS = {"removeFromGame"}

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

# --------------------------------------------------- string-valued keys -----
#
# A handful of `effects` keys print a STRING rather than a number. Some of
# those strings are pure human prose Python never reads (see
# `IGNORED_NESTED_EFFECT_KEYS` above). The rest are a real dispatch key --
# `engine/actions.py:566-618`: `kind = eff.get("freeCivilAction")`, then
# `kind == "increase_population"` / `"build_one_wonder_stage"` / etc reads
# THE VALUE directly, and `_one_time_culture`/aggression-resolution/war-
# resolution each hand-dispatch a DIFFERENT formula per card for
# `onBuildCulture`/`gainResources`/`victorTakesScienceUpTo`. A payload-less
# `Special::FreeCivilAction` could not tell those apart -- SIX different
# cards' ordered actions all became the exact same variant, the textbook
# case of this project's "present in one registry, absent from the other,
# nothing fails when they disagree" bug class (confirmed 2026-08-05: 18
# cards print `freeCivilAction`, across 6 distinct values, all 18 collapsed
# into one `Special::FreeCivilAction` before this fix).
#
# Fixed the same way `PACT_BLOCK_FIELDS` fixes the equivalent problem for
# dict-valued keys: every OBSERVED value is named explicitly, and a value
# outside this map stops the build rather than silently becoming "some
# string, which one is anyone's guess". Hand-mapped rather than
# `camel()`-generated from the value directly -- unlike an effect KEY,
# these VALUES are not guaranteed to make a legal Rust identifier
# (`onBuildCulture`'s Fast Food Chains/Internet formulas both start with a
# digit) or a nameable one (Internet's formula is a full sentence).
STRING_EFFECT_VALUES = {
    "freeCivilAction": {
        "build_or_upgrade_farm_or_mine": "BuildOrUpgradeFarmOrMine",
        "build_or_upgrade_urban_building": "BuildOrUpgradeUrbanBuilding",
        "increase_population": "IncreasePopulation",
        "build_one_wonder_stage": "BuildOneWonderStage",
        "develop_technology": "DevelopTechnology",
        "upgrade_farm_mine_or_urban_building": "UpgradeFarmMineOrUrbanBuilding",
    },
    "onBuildCulture": {
        "2*workers(farm,mine)+1*workers(urban,military)": "FastFoodChains",
        "2*(cultureProduction of theaters+libraries)": "Hollywood",
        "sum over urban buildings of (culture + science + strength) they "
        "give, including leader modifications to those buildings' output":
            "Internet",
    },
    "gainResources": {
        "half of each destroyed building's printed build cost, rounded up":
            "HalfDestroyedBuildingCostRoundedUp",
    },
    "victorTakesScienceUpTo": {
        "strengthAdvantage": "StrengthAdvantage",
    },
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


def build_age_array(name, key, block):
    """An age-keyed effect dict -> a `[i16; 5]` Rust literal indexed by
    `cards::Age as u8`.  A key that is not an age stops the build: an
    unrecognised one would silently price every age at zero, which is
    exactly the failure mode this generator refuses everywhere else."""
    values = [0] * len(AGE_ORDER)
    for k, v in block.items():
        if k in IGNORED_KEYS:
            continue
        if k not in AGES:
            raise ValueError(
                f"{name}: effects.{key} is keyed by {k!r}, which is not an age "
                f"-- an age-array effect ({sorted(AGE_ARRAY_EFFECT_KEYS)}) may "
                f"only be keyed by {list(AGE_ORDER)}")
        values[AGE_ORDER.index(k)] = as_int(v, f"{name}.effects.{key}.{k}")
    return "[" + ", ".join(str(x) for x in values) + "]"


# ------------------------------------------- §12.5.2 final scoring events --
#
# The 15 base-game "Impact of ..." Age III event cards print
# `scoringEvent: true` and an `effects.allPlayers` block holding the actual
# formula (`engine/events.py::scoring_culture`'s key vocabulary). Every
# other `allPlayers` block in the data (event targeting during PLAY, not
# final scoring) stays the payload-less `Special::AllPlayers` unit variant
# `DEFERRED_DICT_EFFECT_KEYS` already gives it -- this is scoped to the 15
# `scoringEvent` cards specifically, verified 2026-08-05 to be exactly the
# same 15 cards `age == "III" and "allPlayers" in effects` selects.

#: `effects.allPlayers` sub-key -> `cards::FinalScoringBlock` field, for the
#: plain-magnitude keys. `culturePerCompletedWonderByAge` (age-keyed dict),
#: `rankingCulture`/`statistic` (the ranking table) are handled separately
#: below, not through this map. Exhaustive against the live data (2026-08-05
#: census of all 15 `scoringEvent` cards' `allPlayers` blocks).
SCORING_BLOCK_FIELDS = {
    "culturePerResourceProducedByMines": "culture_per_resource_produced_by_mines",
    "culturePerFoodProducedByFarms": "culture_per_food_produced_by_farms",
    "bonusIfProductionExceedsConsumption": "bonus_if_production_exceeds_consumption",
    "culturePerLevelOfMilitaryUnitsAndArenas":
        "culture_per_level_of_military_units_and_arenas",
    "culturePerLevelOfSpecialTechsAndGovernment":
        "culture_per_level_of_special_techs_and_government",
    "culturePerContentWorkerAbove10": "culture_per_content_worker_above_10",
    "culturePerColony": "culture_per_colony",
    "culturePerCivilAction": "culture_per_civil_action",
    "culturePerMilitaryAction": "culture_per_military_action",
    "culturePerLevelOfUrbanBuildings": "culture_per_level_of_urban_buildings",
    "culturePerHappyFace": "culture_per_happy_face",
    "maxCultureFromHappyFaces": "max_culture_from_happy_faces",
    "culturePerDiscontentWorker": "culture_per_discontent_worker",
    "culturePerAgeIIITechnology": "culture_per_age_iii_technology",
    "cultureTimesLowestProduction": "culture_times_lowest_production",
    "culturePerDistinctTypeOfUnitUrbanBuildingAndSpecialTech":
        "culture_per_distinct_type_of_unit_urban_building_and_special_tech",
}

#: `allPlayers` sub-keys `engine/events.py::scoring_culture` never reads at
#: all -- decorative/documentation keys, only ever seen on "Impact of
#: Balance" (`statistics`, `ignore`: a human-readable restatement of what
#: `cultureTimesLowestProduction` already means). Verified against
#: `scoring_culture`'s own `elif` chain 2026-08-05: neither key has a
#: matching branch. Scoped to this block only -- NOT the same set as the
#: top-level `IGNORED_KEYS`, which is also checked first.
SCORING_BLOCK_IGNORED_KEYS = {"statistics", "ignore"}

#: `effects.allPlayers.statistic` -> `cards::FinalScoringStat` variant.
#: Mirrors `engine/events.py::_STAT_ALIASES`.
SCORING_STAT_ALIASES = {
    "strengthRating": "Strength",
    "scienceProduction": "Science",
    "cultureProduction": "CultureRate",
    "foodProduction": "Food",
    "resourceProduction": "Resources",
}

SCORING_BLOCK_FIELD_ORDER = (
    "culture_per_resource_produced_by_mines",
    "culture_per_food_produced_by_farms",
    "bonus_if_production_exceeds_consumption",
    "culture_per_level_of_military_units_and_arenas",
    "culture_per_level_of_special_techs_and_government",
    "culture_per_completed_wonder_by_age",
    "culture_per_content_worker_above_10",
    "culture_per_colony",
    "culture_per_civil_action",
    "culture_per_military_action",
    "culture_per_level_of_urban_buildings",
    "culture_per_happy_face",
    "max_culture_from_happy_faces",
    "culture_per_discontent_worker",
    "culture_per_age_iii_technology",
    "culture_times_lowest_production",
    "culture_per_distinct_type_of_unit_urban_building_and_special_tech",
    "has_ranking",
    "ranking_stat",
    "ranking_2p",
    "ranking_3p",
    "ranking_4p",
)
SCORING_BLOCK_BOOL_FIELDS = {"has_ranking"}


def scoring_stat_variant(name, statistic):
    """`effects.allPlayers.statistic` -> a `FinalScoringStat::<Variant>`
    literal.

    Python's own lookup (`_STAT_ALIASES.get(block.get("statistic",
    "strengthRating"), "strength")`) silently falls back to `"strength"` for
    a `statistic` value it does not recognize, rather than erroring --
    unlike almost everything else in this generator, which fails loud on an
    unrecognized value. That is a real quirk of the Python it is faithfully
    reproducing, not hardened away here (this project's standing rule:
    reproduce a found gap, do not paper over it). No base-game card
    exercises the fallback -- verified 2026-08-05, only `strengthRating`/
    `scienceProduction` are ever printed -- so this only ever returns
    `Strength` by the documented default path, never the silent one.
    """
    return SCORING_STAT_ALIASES.get(statistic, "Strength")


def build_final_scoring_block(name, block):
    """`effects.allPlayers` on a `scoringEvent` card -> a `FinalScoringBlock
    { ... }` Rust literal (see cards.rs)."""
    values = {f: 0 for f in SCORING_BLOCK_FIELD_ORDER}
    values["culture_per_completed_wonder_by_age"] = "[0, 0, 0, 0, 0]"
    values["has_ranking"] = False
    values["ranking_stat"] = "FinalScoringStat::Strength"
    values["ranking_2p"] = "[0, 0]"
    values["ranking_3p"] = "[0, 0, 0]"
    values["ranking_4p"] = "[0, 0, 0, 0]"

    ranking_table = None
    statistic = None
    for k, v in block.items():
        if k in IGNORED_KEYS or k in SCORING_BLOCK_IGNORED_KEYS:
            continue
        if k == "culturePerCompletedWonderByAge":
            values["culture_per_completed_wonder_by_age"] = build_age_array(
                name, "allPlayers.culturePerCompletedWonderByAge", v)
            continue
        if k == "rankingCulture":
            ranking_table = v
            continue
        if k == "statistic":
            statistic = v
            continue
        field = SCORING_BLOCK_FIELDS.get(k)
        if field is None:
            raise ValueError(
                f"{name}: effects.allPlayers.{k} is not a recognized final-"
                f"scoring key -- add it to SCORING_BLOCK_FIELDS (with the "
                f"FinalScoringBlock field it feeds), SCORING_BLOCK_IGNORED_KEYS "
                f"(if events.py::scoring_culture truly never reads it) or "
                f"IGNORED_KEYS in gen_cards.py")
        values[field] = as_int(v, f"{name}.effects.allPlayers.{k}")

    if ranking_table is not None:
        values["has_ranking"] = True
        values["ranking_stat"] = (
            f"FinalScoringStat::{scoring_stat_variant(name, statistic or 'strengthRating')}")
        unknown_pkeys = set(ranking_table) - {"2p", "3p", "4p"}
        if unknown_pkeys:
            raise ValueError(
                f"{name}: effects.allPlayers.rankingCulture has unrecognized "
                f"player-count key(s) {sorted(unknown_pkeys)!r}")
        for pkey, field, width in (("2p", "ranking_2p", 2),
                                    ("3p", "ranking_3p", 3),
                                    ("4p", "ranking_4p", 4)):
            table = ranking_table.get(pkey)
            if table is None:
                raise ValueError(
                    f"{name}: effects.allPlayers.rankingCulture is missing "
                    f"{pkey!r}")
            if len(table) != width:
                raise ValueError(
                    f"{name}: effects.allPlayers.rankingCulture.{pkey} has "
                    f"{len(table)} entries, expected {width}")
            values[field] = "[" + ", ".join(
                str(as_int(x, f"{name}.rankingCulture.{pkey}")) for x in table
            ) + "]"
    elif statistic is not None:
        raise ValueError(
            f"{name}: effects.allPlayers.statistic printed without "
            f"rankingCulture")

    parts = []
    for f in SCORING_BLOCK_FIELD_ORDER:
        v = values[f]
        parts.append(f"{f}: {rust_bool(v) if f in SCORING_BLOCK_BOOL_FIELDS else v}")
    return "FinalScoringBlock { " + ", ".join(parts) + " }"


#: `effects.takeFromOpponent` sub-key -> `cards::TakeFromOpponentBlock`
#: field. Exhaustive against the live data (2026-08-05: three base-game
#: aggression cards -- Plunder/Spy/Armed Intervention -- one field each):
#: an unrecognized sub-key stops the build rather than vanishing, same
#: treatment as `PACT_BLOCK_FIELDS` above.
TAKE_FROM_OPPONENT_FIELDS = {
    "foodAndOrResources": "food_and_or_resources",
    "science": "science",
    "culture": "culture",
}
TAKE_FROM_OPPONENT_FIELD_ORDER = (
    "food_and_or_resources", "science", "culture",
)


def build_take_from_opponent(name, block):
    """`effects.takeFromOpponent` -> a `TakeFromOpponentBlock { ... }` Rust
    literal (see cards.rs). `engine/events.py::finish_aggression` (651-667)
    reads exactly these three keys off this dict and ignores any other --
    but an unrecognized key here still stops the build, on the theory that a
    key finish_aggression doesn't read is far more likely to be a
    transcription surprise than a deliberate no-op (same posture as
    `build_pact_block`)."""
    values = {f: 0 for f in TAKE_FROM_OPPONENT_FIELDS.values()}
    for k, v in block.items():
        if k in IGNORED_KEYS:
            continue
        field = TAKE_FROM_OPPONENT_FIELDS.get(k)
        if field is None:
            raise ValueError(
                f"{name}: effects.takeFromOpponent.{k} is not a recognized "
                f"key -- add it to TAKE_FROM_OPPONENT_FIELDS (with the real "
                f"handling combat.rs::finish_aggression needs) or "
                f"IGNORED_KEYS in gen_cards.py")
        values[field] = as_int(v, f"{name}.effects.takeFromOpponent.{k}")
    parts = [f"{f}: {values[f]}" for f in TAKE_FROM_OPPONENT_FIELD_ORDER]
    return "TakeFromOpponentBlock { " + ", ".join(parts) + " }"


def build_age_list(name, key, val):
    """`destroyUrbanBuildings`'s list of per-raid `{"maxAge": "<Age>"}`
    specs -> a `&[Age]` Rust slice literal, one element per raid, in printed
    order. `engine/events.py::finish_aggression` (674-675) loops this list
    calling `spec.get("maxAge", "A")` per entry, so a spec printing no
    `maxAge` at all defaults to `Age::A` here too. Any OTHER key inside a
    spec stops the build -- nothing in `finish_aggression` reads one, so a
    spec that had one would be a transcription surprise, not a deliberate
    extra."""
    ages = []
    for i, spec in enumerate(val):
        unknown = set(spec) - {"maxAge"}
        if unknown:
            raise ValueError(
                f"{name}: effects.{key}[{i}] has key(s) {sorted(unknown)!r} "
                f"besides maxAge -- gen_cards.py only knows how to read "
                f"maxAge from a {key} entry")
        age = spec.get("maxAge", "A")
        if age not in AGES:
            raise ValueError(
                f"{name}: effects.{key}[{i}].maxAge = {age!r} is not a "
                f"recognized age")
        ages.append(f"Age::{AGES[age]}")
    return "&[" + ", ".join(ages) + "]"


# ------------------------------------------------------ event targeting -----
#
# `allPlayers` and its 12 siblings (§5.3, `engine/events.py::resolve_event`/
# `_apply_player_block`/`_apply_extras`/`_queue_decisions`): the dict-valued
# `effects` keys that name WHO an event targets and WHAT happens to them.
# Every one of the 7 player-targeting keys (`allPlayers`, `weakestPlayer`,
# `strongestPlayer`, `playerWithMostCulture`, `playerWithLeastCulture`,
# `playersWithMostHappyFaces`, `playersWithMostDiscontentWorkers`) plus the
# `gain`/`lose` blocks `strongestPlayers`/`weakestPlayers` apply shares ONE
# payload shape, `cards::EventBlock` -- see that struct's own doc comment for
# why one shape serves all nine.

EVENT_BLOCK_DICT_KEYS = {
    "allPlayers", "weakestPlayer", "strongestPlayer", "playerWithMostCulture",
    "playerWithLeastCulture", "playersWithMostHappyFaces",
    "playersWithMostDiscontentWorkers", "gain", "lose",
}

#: `effects.<targeting key>` sub-key -> `cards::EventBlock` field, for the
#: plain int/bool-magnitude keys. `choose`/`freeBuild`/`flipCompletedWonder`/
#: `oneTimeDiscount`/`extraProduction` (structured values) are handled
#: separately in `build_event_block`, not through this map. Exhaustive
#: against the live data (2026-08-05 census of every non-`scoringEvent`
#: event card's targeting/`gain`/`lose` sub-dicts, 40 cards).
EVENT_BLOCK_FIELDS = {
    "science": "science",
    "culture": "culture",
    "food": "food",
    "resources": "resources",
    "foodAndOrResources": "food_and_or_resources",
    "blueTokens": "blue_tokens",
    "drawMilitaryCards": "draw_military_cards",
    "decreasePopulation": "decrease_population",
    "increasePopulation": "increase_population",
    "takeYellowTokensFromWeakest": "take_yellow_tokens_from_weakest",
    "civilActionsPerDiscontentWorker": "civil_actions_per_discontent_worker",
    "optionalTakeCardsWithCivilActions": "optional_take_cards_with_civil_actions",
    "culturePerDiscontentWorker": "culture_per_discontent_worker",
    "destroyOwnBuilding": "destroy_own_building",
    "loseColony": "lose_colony",
    "discardMilitaryCards": "discard_military_cards",
    "loseAllStoredFood": "lose_all_stored_food",
    "produceFood": "produce_food",
    "produceResources": "produce_resources",
    "scienceEqualToScienceProduction": "science_equal_to_science_production",
    "cultureEqualToCultureProduction": "culture_equal_to_culture_production",
    "cultureEqualToScienceProduction": "culture_equal_to_science_production",
    "discardLeaderUnlessCurrentAge": "discard_leader_unless_current_age",
    "decreasePopulationByHalfDiscontentWorkersRoundedUp":
        "decrease_population_by_half_discontent_workers_rounded_up",
    "destroyOneUrbanBuildingOfEachOpponent":
        "destroy_one_urban_building_of_each_opponent",
    # `foodEqualToHappyFaces`'s own cap -- only ever printed alongside it, on
    # the one base-game card that prints either ("Prosperity"), same pairing
    # reasoning `FinalScoringBlock`'s own doc comment gives for
    # `bonusIfProductionExceedsConsumption`.
    "foodEqualToHappyFaces": "food_equal_to_happy_faces",
    "max": "food_equal_to_happy_faces_max",
}
#: `EventBlock` fields (not JSON keys) whose value is a bool flag, not a
#: magnitude -- Python's own dispatch ignores the printed value for these too
#: (`_apply_extras`'s `if block.get("produceFood"):` etc never reads it past
#: truthiness).
EVENT_BLOCK_BOOL_FIELDS = {
    "lose_all_stored_food", "produce_food", "produce_resources",
    "extra_production", "science_equal_to_science_production",
    "culture_equal_to_culture_production",
    "culture_equal_to_science_production", "food_equal_to_happy_faces",
    "discard_leader_unless_current_age",
    "decrease_population_by_half_discontent_workers_rounded_up",
    "destroy_one_urban_building_of_each_opponent",
}
#: Keys nested INSIDE a targeting/`gain`/`lose` dict that Python's own
#: dispatch confirmed never reads the VALUE of (unlike `IGNORED_KEYS`, this
#: is scoped to this one nesting level -- see `IGNORED_NESTED_EFFECT_KEYS`'s
#: own doc comment for why a sibling scope is kept separate rather than
#: merged). `ignoreConsumption`/`ignoreCorruption` (Good Harvest/New
#: Deposits): `_apply_extras`'s `produceFood`/`produceResources` branches
#: never test either flag. `ruinsCultureProduction` (Ravages of Time): not a
#: single `.get(...)` call anywhere in `engine/`. `chosenBy` (Independence
#: Declaration): prose restating that `_q_lose_colony` already lets the
#: LOSING player choose which colony -- `interact.rs`'s existing
#: `QueueItem::LoseColony` handler already does. `cost` (International
#: Agreement): prose restating `optionalTakeCardsWithCivilActions`'s own
#: hardcoded `p.skip_next_politics = True`.
EVENT_BLOCK_IGNORED_KEYS = {
    "ignoreConsumption", "ignoreCorruption", "ruinsCultureProduction",
    "chosenBy", "cost",
}
#: `freeBuild.card` values that name a card TYPE rather than a printed card
#: name (Development of Religion: `"card": "Temple"`, no such card is ever
#: printed -- every base-game temple is named after its own age instead,
#: "Religion"/"Theology"/"Organized Religion"). Scoped to the civil-row types
#: a free build could plausibly ever name.
FREE_BUILD_TYPE_NAMES = {
    "Farm": "Farm", "Mine": "Mine", "Lab": "Lab", "Temple": "Temple",
    "Library": "Library", "Arena": "Arena", "Theater": "Theater",
    "Infantry": "Infantry", "Cavalry": "Cavalry", "Artillery": "Artillery",
    "Air": "Air",
}

EVENT_BLOCK_FIELD_ORDER = (
    "science", "culture", "food", "resources", "food_and_or_resources",
    "blue_tokens", "lose_all_stored_food", "draw_military_cards",
    "decrease_population", "increase_population", "produce_food",
    "produce_resources", "extra_production",
    "science_equal_to_science_production",
    "culture_equal_to_culture_production",
    "culture_equal_to_science_production", "food_equal_to_happy_faces",
    "food_equal_to_happy_faces_max", "discard_leader_unless_current_age",
    "take_yellow_tokens_from_weakest",
    "decrease_population_by_half_discontent_workers_rounded_up",
    "civil_actions_per_discontent_worker",
    "one_time_discount_build_resources", "one_time_discount_develop_science",
    "one_time_discount_pop_food", "destroy_one_urban_building_of_each_opponent",
    "optional_take_cards_with_civil_actions", "culture_per_discontent_worker",
    "choose_food", "choose_resources", "free_build_card", "free_build_age",
    "free_build_kind", "free_build_cost", "destroy_own_building",
    "lose_colony", "flip_completed_wonder_ages", "discard_military_cards",
)


def build_event_block(name, block, name_index):
    """One event's targeting/`gain`/`lose` sub-dict -> an `EventBlock { ... }`
    Rust literal (see cards.rs). Exhaustive against the live data: a sub-key
    outside this vocabulary stops the build."""
    values = {f: (False if f in EVENT_BLOCK_BOOL_FIELDS else 0)
              for f in EVENT_BLOCK_FIELD_ORDER}
    values["free_build_card"] = "CardId::NONE"
    values["free_build_age"] = "None"
    values["free_build_kind"] = "None"
    values["flip_completed_wonder_ages"] = "&[]"

    for k, v in block.items():
        if k in IGNORED_KEYS or k in EVENT_BLOCK_IGNORED_KEYS:
            continue
        if k == "extraProduction":
            if not isinstance(v, dict) or (set(v) - {"order"} - IGNORED_KEYS):
                raise ValueError(
                    f"{name}: effects.<block>.extraProduction has an "
                    f"unexpected shape {v!r}")
            values["extra_production"] = True
            continue
        if k == "oneTimeDiscount":
            if not isinstance(v, dict):
                raise ValueError(
                    f"{name}: effects.<block>.oneTimeDiscount must be a dict")
            sub_map = {
                "increasePopulation": ("food", "one_time_discount_pop_food"),
                "build": ("resources", "one_time_discount_build_resources"),
                "developTechnology": ("science", "one_time_discount_develop_science"),
            }
            for sk, sv in v.items():
                if sk in IGNORED_KEYS:
                    continue
                entry = sub_map.get(sk)
                if entry is None:
                    raise ValueError(
                        f"{name}: effects.<block>.oneTimeDiscount.{sk} is "
                        f"not recognized -- add it to gen_cards.py's "
                        f"build_event_block sub_map and cards::EventBlock")
                want_key, field = entry
                if not isinstance(sv, dict) or set(sv) - {want_key}:
                    raise ValueError(
                        f"{name}: effects.<block>.oneTimeDiscount.{sk} = "
                        f"{sv!r}, expected {{{want_key!r}: <int>}}")
                values[field] = as_int(
                    sv.get(want_key), f"{name}.oneTimeDiscount.{sk}.{want_key}")
            continue
        if k == "choose":
            if not (isinstance(v, list) and len(v) == 2
                    and set(v[0]) == {"food"} and set(v[1]) == {"resources"}):
                raise ValueError(
                    f"{name}: effects.<block>.choose must be exactly "
                    f"[{{'food': N}}, {{'resources': M}}] (cards::EventBlock's "
                    f"own doc comment narrows to this shape) -- got {v!r}")
            values["choose_food"] = as_int(v[0]["food"], f"{name}.choose[0].food")
            values["choose_resources"] = as_int(
                v[1]["resources"], f"{name}.choose[1].resources")
            continue
        if k == "freeBuild":
            if not isinstance(v, dict):
                raise ValueError(f"{name}: effects.<block>.freeBuild must be a dict")
            unknown = (set(v) - {"card", "age", "cost", "requiresAvailableWorker"}
                       - IGNORED_KEYS)
            if unknown:
                raise ValueError(
                    f"{name}: effects.<block>.freeBuild has unrecognized "
                    f"key(s) {sorted(unknown)!r}")
            if v.get("requiresAvailableWorker") is not True:
                raise ValueError(
                    f"{name}: effects.<block>.freeBuild.requiresAvailableWorker "
                    f"is not True -- interact.rs's existing FreeBuild resolver "
                    f"always requires one unconditionally; a card that omits "
                    f"or falsifies this needs real handling, not a silent "
                    f"assumption")
            values["free_build_cost"] = as_int(v.get("cost"), f"{name}.freeBuild.cost")
            age_str = v.get("age")
            values["free_build_age"] = f"Some(Age::{AGES[age_str]})" if age_str else "None"
            card_name = v.get("card")
            if card_name is None:
                raise ValueError(f"{name}: effects.<block>.freeBuild has no 'card'")
            if card_name in name_index:
                values["free_build_card"] = f"CardId({name_index[card_name]})"
            elif card_name in FREE_BUILD_TYPE_NAMES:
                values["free_build_kind"] = f"Some(CardType::{FREE_BUILD_TYPE_NAMES[card_name]})"
            else:
                raise ValueError(
                    f"{name}: effects.<block>.freeBuild.card = {card_name!r} "
                    f"is neither a printed card name nor a recognized card "
                    f"type -- add it to FREE_BUILD_TYPE_NAMES in "
                    f"gen_cards.py if it names a type")
            continue
        if k == "flipCompletedWonder":
            if not isinstance(v, dict):
                raise ValueError(
                    f"{name}: effects.<block>.flipCompletedWonder must be a dict")
            unknown = set(v) - {"ages"} - IGNORED_KEYS
            if unknown:
                raise ValueError(
                    f"{name}: effects.<block>.flipCompletedWonder has "
                    f"unrecognized key(s) {sorted(unknown)!r}")
            ages = v.get("ages") or ["A", "I"]
            values["flip_completed_wonder_ages"] = (
                "&[" + ", ".join(f"Age::{AGES[a]}" for a in ages) + "]")
            continue
        field = EVENT_BLOCK_FIELDS.get(k)
        if field is None:
            raise ValueError(
                f"{name}: effects.<block>.{k} is not a recognized event-block "
                f"key -- add it to EVENT_BLOCK_FIELDS (with the "
                f"cards::EventBlock field it feeds), EVENT_BLOCK_IGNORED_KEYS "
                f"or IGNORED_KEYS in gen_cards.py")
        if field in EVENT_BLOCK_BOOL_FIELDS:
            if not isinstance(v, bool):
                raise ValueError(
                    f"{name}: effects.<block>.{k} expected a bool flag, got {v!r}")
            values[field] = v
        else:
            values[field] = as_int(v, f"{name}.effects.<block>.{k}")

    parts = []
    for f in EVENT_BLOCK_FIELD_ORDER:
        v = values[f]
        parts.append(f"{f}: {rust_bool(v) if f in EVENT_BLOCK_BOOL_FIELDS else v}")
    return "EventBlock { " + ", ".join(parts) + " }"


def build_count_table(name, path, val):
    """A per-player-count magnitude dict (`{"2p": 1, "3p": 2, "4p": 2}`) --
    `strongestPlayers`/`weakestPlayers`'s own value, or `condition.
    amongWeakest` -- -> a `[i16; 3]` Rust literal (index 0/1/2 = 2p/3p/4p).
    All three player counts must be present, same strictness as
    `FinalScoringBlock`'s own `rankingCulture` tables."""
    if not isinstance(val, dict):
        raise ValueError(f"{name}: {path} must be a per-player-count dict")
    unknown = set(val) - {"2p", "3p", "4p"} - IGNORED_KEYS
    if unknown:
        raise ValueError(f"{name}: {path} has unrecognized key(s) {sorted(unknown)!r}")
    out = []
    for pkey in ("2p", "3p", "4p"):
        if pkey not in val:
            raise ValueError(f"{name}: {path} is missing {pkey!r}")
        out.append(as_int(val[pkey], f"{name}.{path}.{pkey}"))
    return "[" + ", ".join(str(x) for x in out) + "]"


def build_condition(name, val):
    """Barbarians' `condition` key -> a `[i16; 3]` `amongWeakest` table
    (`cards::Special::Condition`). Exhaustive against the live data
    (2026-08-05: one card)."""
    if not isinstance(val, dict):
        raise ValueError(f"{name}: effects.condition must be a dict")
    unknown = set(val) - {"amongWeakest"} - IGNORED_KEYS
    if unknown:
        raise ValueError(
            f"{name}: effects.condition has unrecognized key(s) {sorted(unknown)!r}")
    among = val.get("amongWeakest")
    if among is None:
        raise ValueError(f"{name}: effects.condition has no 'amongWeakest'")
    return build_count_table(name, "effects.condition.amongWeakest", among)


def build_last_round_substitute(name, val, name_index):
    """`lastRoundSubstitute` (Politics of Strength, the only base-game card)
    -> a `LastRoundSubstituteBlock { ... }` Rust literal. Exhaustive against
    the live data: this one card's substitute dict prints only
    `strongestPlayer`/`weakestPlayer`, so those are the only two targeting
    keys recognized here -- any other stops the build (see
    `cards::LastRoundSubstituteBlock`'s own doc comment)."""
    if not isinstance(val, dict):
        raise ValueError(f"{name}: effects.lastRoundSubstitute must be a dict")
    key_to_field = {"strongestPlayer": "strongest_player", "weakestPlayer": "weakest_player"}
    parts = {"strongest_player": "EventBlock::EMPTY", "weakest_player": "EventBlock::EMPTY"}
    for k, v in val.items():
        if k in IGNORED_KEYS:
            continue
        field = key_to_field.get(k)
        if field is None:
            raise ValueError(
                f"{name}: effects.lastRoundSubstitute.{k} is not a "
                f"recognized targeting key -- add it to "
                f"cards::LastRoundSubstituteBlock and gen_cards.py's "
                f"build_last_round_substitute (only strongestPlayer/"
                f"weakestPlayer are exhaustive against the live data)")
        if not isinstance(v, dict):
            raise ValueError(
                f"{name}: effects.lastRoundSubstitute.{k} must be a dict")
        parts[field] = build_event_block(name, v, name_index)
    return ("LastRoundSubstituteBlock { "
            f"strongest_player: {parts['strongest_player']}, "
            f"weakest_player: {parts['weakest_player']} }}")


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
    # Final printed name -> index into `cards`/`rows`/`CARDS` -- computed
    # once, up front, since `build_event_block`'s `freeBuild.card` handling
    # (a card referring to ANOTHER card by identity, a new pattern this
    # generator has not needed before) must resolve a name to a `CardId`
    # that is only meaningful once every card's final (possibly
    # disambiguated) name and position are both fixed.
    name_index = {c["name"]: i for i, c in enumerate(cards)}

    # variant name -> "int" | "unit" | "pact_block" | "age_array" |
    # "string_enum" (carries an `(i16)`, no payload, a `(PactBlock)`, an
    # `([i16; 5])` indexed by age, or a small generated enum respectively).
    # A key that shows up
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
            if key in IGNORED_KEYS or key in IGNORED_NESTED_EFFECT_KEYS \
                    or key in ("tacticBonus", "tacticBonusObsolete"):
                continue
            if key == "allPlayers" and c.get("scoringEvent"):
                # One of the 15 "Impact of ..." final-scoring events (see
                # this file's "§12.5.2 final scoring events" section) -- a
                # REAL `FinalScoringBlock` payload. UNLIKE the 9 ordinary
                # `allPlayers`-printing event cards, this does NOT also get a
                # `Special::AllPlayers(EventBlock)` entry: `SCORING_BLOCK_FIELDS`
                # is exhaustive against all 15 of these cards' `allPlayers`
                # blocks, so none of them ever prints an EventBlock-shaped
                # key, and `Special::AllPlayers` cannot carry two different
                # payload shapes across different cards anyway. `events.rs`
                # derives "this card has an allPlayers key" from
                # `Special::FinalScoring`'s own presence for these 15 instead
                # -- see that module's top doc comment.
                variant, shape, payload = (
                    "FinalScoring", "final_scoring_block",
                    build_final_scoring_block(name, val))
                prev = specials.get(variant)
                if prev is not None and prev != shape:
                    raise ValueError(
                        f"{name}: Special::{variant} used as both "
                        f"{prev!r} and {shape!r} across cards (key "
                        f"{key!r}) -- gen_cards.py cannot pick one shape "
                        f"silently, give it bespoke handling")
                specials[variant] = shape
                mine.append((variant, payload))
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
                elif key in AGE_ARRAY_EFFECT_KEYS:
                    variant = camel(key)
                    shape = "age_array"
                    payload = build_age_array(name, key, val)
                elif key == "takeFromOpponent":
                    variant = camel(key)
                    shape = "take_from_opponent_block"
                    payload = build_take_from_opponent(name, val)
                elif key in EVENT_BLOCK_DICT_KEYS:
                    variant = camel(key)
                    shape = "event_block"
                    payload = build_event_block(name, val, name_index)
                elif key == "condition":
                    variant = camel(key)
                    shape = "count_table"
                    payload = build_condition(name, val)
                elif key in ("strongestPlayers", "weakestPlayers"):
                    variant = camel(key)
                    shape = "count_table"
                    payload = build_count_table(name, f"effects.{key}", val)
                elif key == "lastRoundSubstitute":
                    variant = camel(key)
                    shape = "last_round_substitute_block"
                    payload = build_last_round_substitute(name, val, name_index)
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
                if key in LIST_AGE_EFFECT_KEYS:
                    variant = camel(key)
                    shape = "age_list"
                    payload = build_age_list(name, key, val)
                elif key in LIST_PRESENCE_EFFECT_KEYS:
                    variant = camel(key)
                    shape = "unit"
                    payload = None
                else:
                    raise ValueError(
                        f"{name}: effects.{key} is a list-valued key this "
                        f"generator does not recognize -- add it to "
                        f"LIST_AGE_EFFECT_KEYS or LIST_PRESENCE_EFFECT_KEYS "
                        f"(with a one-line reason) in gen_cards.py")
            elif isinstance(val, bool):
                # Bool-flag keys carry no magnitude in Python's own dispatch
                # either (`_apply_modifier`/`_apply_special` ignore `val` for
                # these) -- presence is the whole rule, so a bare unit variant
                # is correct, not a gap.
                variant = camel(key)
                shape = "unit"
                payload = None
            elif isinstance(val, str):
                # See STRING_EFFECT_VALUES above: a real dispatch value
                # (`freeCivilAction`) or a per-card formula
                # (`onBuildCulture`/`gainResources`/`victorTakesScienceUpTo`)
                # -- either way, Python's behaviour DOES depend on which
                # string this is, so it needs a real payload, not a bare
                # flag every such card would otherwise share indistinguishably.
                if key not in STRING_EFFECT_VALUES:
                    raise ValueError(
                        f"{name}: effects.{key} is a string-valued key this "
                        f"generator does not recognize -- add it to "
                        f"STRING_EFFECT_VALUES (every expected value named) "
                        f"or IGNORED_NESTED_EFFECT_KEYS (if Python truly "
                        f"never reads the value, verified against engine/) "
                        f"in gen_cards.py")
                values = STRING_EFFECT_VALUES[key]
                if val not in values:
                    raise ValueError(
                        f"{name}: effects.{key} = {val!r} is not in "
                        f"STRING_EFFECT_VALUES[{key!r}] -- a new value here "
                        f"is exactly the case this generator exists to "
                        f"catch, not paper over; add it with a real name")
                variant = camel(key)
                shape = "string_enum"
                enum_type = variant + "Value"
                payload = f"{enum_type}::{values[val]}"
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
    w("use crate::cards::{Age, Card, CardEffects, CardId, CardType, "
      "Composition, EventBlock, FinalScoringBlock, FinalScoringStat, "
      "ImmediateEffects, LastRoundSubstituteBlock, PactBlock, Production, "
      "TakeFromOpponentBlock};")
    w("")
    w(f"pub const NUM_CARDS: usize = {len(rows)};")
    w("")

    # One small enum per string-valued effects key actually seen in the data
    # (`STRING_EFFECT_VALUES`) -- emitted before `Special` since `Special`'s
    # variants reference them as payloads.
    for key, values in STRING_EFFECT_VALUES.items():
        variant = camel(key)
        if specials.get(variant) != "string_enum":
            continue  # key defined but never observed in this data revision
        enum_type = variant + "Value"
        w(f"/// Every value `effects.{key}` prints in the base game -- see")
        w("/// gen_cards.py's `STRING_EFFECT_VALUES` for why this is a closed,")
        w("/// hand-named enum rather than a `&'static str`.")
        w("#[derive(Clone, Copy, PartialEq, Eq, Debug)]")
        w(f"pub enum {enum_type} {{")
        for name_ in values.values():
            w(f"    {name_},")
        w("}")
        w("")

    w("/// One card's unique rule. Generated: one variant per effect key that is")
    w("/// not a recurring numeric field. The `match` over this in `effects.rs`")
    w("/// is exhaustive, so a card the engine cannot interpret is a COMPILE")
    w("/// ERROR -- which is the guarantee the Python name-dispatch cannot give.")
    w("///")
    w("/// A variant carries an `(i16)` payload when the printed effect has a")
    w("/// magnitude Python's own dispatch reads (`val` used in `_apply_modifier`")
    w("/// / `_apply_special`); a `(PactBlock)` payload when the printed value is")
    w("/// one of the four pact blocks (§5.9); a `(TakeFromOpponentBlock)`")
    w("/// payload for `takeFromOpponent` (§5.4.6); a `(FinalScoringBlock)`")
    w("/// payload for `allPlayers` on one of the 15 `scoringEvent` Age III")
    w("/// cards (§12.5.2); an `(EventBlock)` payload for the 7 player-targeting")
    w("/// keys plus `gain`/`lose` (§5.3 event resolution -- see")
    w("/// `cards::EventBlock`'s own doc comment for why one shape serves all")
    w("/// nine); a `([i16; 3])` payload, indexed by live player count minus 2,")
    w("/// for `strongestPlayers`/`weakestPlayers`'s own per-count table and for")
    w("/// `condition`'s `amongWeakest` table; a `(LastRoundSubstituteBlock)`")
    w("/// payload for `lastRoundSubstitute`; an `([i16; 5])` payload,")
    w("/// indexed by `Age as u8`, when the printed value is a per-age magnitude")
    w("/// dict (`buildDiscount`); a `(&'static [Age])` payload for")
    w("/// `destroyUrbanBuildings`, one entry per raid; a `(<Key>Value)` payload")
    w("/// when the printed value is a STRING Python's own dispatch reads")
    w("/// (`freeCivilAction` et al -- see `STRING_EFFECT_VALUES` in")
    w("/// gen_cards.py); and stays a bare unit variant when Python ignores the")
    w("/// JSON value too (a bool-flag key, or a dict/list-valued key this port")
    w("/// has not modeled yet -- see gen_cards.py's DEFERRED_DICT_EFFECT_KEYS/")
    w("/// LIST_PRESENCE_EFFECT_KEYS for the reason on each).")
    w("#[derive(Clone, Copy, PartialEq, Eq, Debug)]")
    w("pub enum Special {")
    for v in ordered:
        shape = specials[v]
        if shape == "int":
            w(f"    {v}(i16),")
        elif shape == "pact_block":
            w(f"    {v}(PactBlock),")
        elif shape == "age_array":
            w(f"    {v}([i16; 5]),")
        elif shape == "age_list":
            w(f"    {v}(&'static [Age]),")
        elif shape == "take_from_opponent_block":
            w(f"    {v}(TakeFromOpponentBlock),")
        elif shape == "final_scoring_block":
            w(f"    {v}(FinalScoringBlock),")
        elif shape == "event_block":
            w(f"    {v}(EventBlock),")
        elif shape == "count_table":
            w(f"    {v}([i16; 3]),")
        elif shape == "last_round_substitute_block":
            w(f"    {v}(LastRoundSubstituteBlock),")
        elif shape == "string_enum":
            w(f"    {v}({v}Value),")
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
