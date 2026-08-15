"""Card effects: ratings, production, costs and triggered abilities.

Everything that reads a card's mechanical `effects` dict lives here.
Effects the data can't express mechanically are handled by name dispatch
(see LEADER_TRIGGERS / the *_TRIGGER tables at the bottom).

Two-phase computation of a player's statistics (`compute`):
  phase 1  government + workers on technologies + colonies (base output)
  phase 2  flat bonuses from special techs, completed wonders, leader
  phase 3  multiplicative / per-X modifiers that must see the base output
           (St. Peter's, Chaplin, Transcontinental Railroad, Michelangelo...)
"""
from __future__ import annotations

import os
from dataclasses import dataclass, field

from . import cards as C

# Module-level bindings for the singleton card DB: `C.db()` was ~734k calls
# per 60 4p games.  cards.py has no engine imports, so this is safe at import.
_DB = C.db()
_TYPE_BY_NAME = _DB.type_by_name
_BY_NAME = _DB.by_name
_LEVEL_BY_NAME = _DB.level_by_name
_DENOM_BY_NAME = _DB.denom_by_name
_IS_UNIT_NAME = _DB.is_unit_name

# TTA_PARANOID=1 makes `state_stats` recompute on every call and assert the
# cached answer is identical -- the guard rail for the stats-cache key below.
_PARANOID = os.environ.get("TTA_PARANOID", "") not in ("", "0", "false", "no")

# ---------------------------------------------------------------- stats


@dataclass
class Stats:
    science: int = 0
    culture: int = 0
    strength: int = 0
    happy: int = 0
    civil_actions: int = 4
    military_actions: int = 2
    urban_limit: int = 2
    food: int = 0                # food production per turn
    resources: int = 0           # resource production per turn
    colonize: int = 0
    civil_hand_limit: int = 0    # bonus on top of civil action total
    military_hand_limit: int = 0
    wonder_stages: int = 1       # stages per "build a wonder step" action
    pop_food_discount: int = 0
    build_discount: dict = field(default_factory=dict)   # age -> resources
    free_pop_per_turn: bool = False
    no_aggression: bool = False
    # pact-derived (§5.9)
    tech_discount: int = 0               # science off every technology
    war_immune: bool = False             # nobody may declare war on me
    food_as_resource: int = 0            # food spendable as resources
    resource_as_food: int = 0
    science_partners: list = field(default_factory=list)  # must pay 1 science


FLAT_KEYS = {
    "civilActions": "civil_actions",
    "militaryActions": "military_actions",
    "culture": "culture",
    "science": "science",
    "strength": "strength",
    "happy": "happy",
    "colonizeBonus": "colonize",
    "civilHandLimit": "civil_hand_limit",
    "militaryHandLimit": "military_hand_limit",
    "food": "food",
    "resources": "resources",
    # aliases used by colony permanents and pact effects
    "happiness": "happy",
    "colonizationBonus": "colonize",
    "cultureProduction": "culture",
    "scienceProduction": "science",
    "foodProduction": "food",
    "resourceProduction": "resources",
}

# keys applied in phase 3 (need the base output first)
MODIFIER_KEYS = {
    "strengthPerMilitaryUnit", "strengthPerInfantry", "strengthPerArtillery",
    "strengthPerUnitType", "strengthPerTempleOrGovernmentHappy",
    "sciencePerBestLabOrLibraryLevel", "culturePerTheater",
    "culturePerLabEqualToLevel", "sciencePerLab", "bestTheaterDoubleCulture",
    "culturePerHappyFromTemplesTheatersWonders", "culturePerLibraryTheaterPair",
    "extraHappyPerHappySource", "doubleBestMine", "resourcesPerLabEqualToLevel",
    "cultureFirstColony", "culturePerAdditionalColony",
}


def _cards_of(p, types):
    db = _DB
    return [n for n in p.techs if db.type_of(n) in types]


def workers_on_types(p, types):
    db = _DB
    return sum(t.workers for n, t in p.techs.items() if db.type_of(n) in types)


def best_card(p, types, require_workers=False):
    """Highest-level technology card of the given types (None if none)."""
    db = _DB
    best, best_lv = None, -1
    for n, t in p.techs.items():
        if db.type_of(n) not in types:
            continue
        if require_workers and t.workers <= 0:
            continue
        lv = db.level_of(n)
        if lv > best_lv:
            best, best_lv = n, lv
    return best


# --------------------------------------------------- per-building output
#
# Three cards score what a *particular set of the player's buildings*
# produces, rather than a rating:
#
#   Impact of Industry  "culture equal to the amount of resources its mines
#                        produce.  (Ignore any production from other
#                        sources.)"                                  [§12.5.2]
#   Hollywood           "culture equal to twice the total culture production
#                        of your theaters and libraries"              [§9.2]
#   Internet            "culture equal to the combined culture, science and
#                        strength your urban buildings give"          [§9.2]
#
# What they score is the *effective* output of those buildings, i.e. printed
# production plus every card effect in play that changes what those buildings
# themselves produce -- Chaplin doubling a theater, Sid Meier turning lab
# science into culture, Einstein/Newton adding science to the best lab or
# library, Shakespeare's library/theater pairs, Bach's theaters, the
# Transcontinental Railroad's doubled mine worker (FAQ v1.5 p.9).  Anything
# that adds the same *rating* from somewhere that is not one of those
# buildings does not count: Bill Gates' labs make resources but "labs
# affected by Bill Gates are not mines" (the card's own text), colonies and
# governments produce from themselves, and Michelangelo pays for happy faces
# rather than for a building's output.
#
# The mapping below is what makes that distinction mechanical instead of
# hand-written per card.  `_BUILDING_OUTPUT` maps a modifier key to
# (building types it modifies, rating it modifies); a modifier counts only
# when the caller asked about those types AND that rating.
_BUILDING_OUTPUT = {
    "bestTheaterDoubleCulture":        (frozenset({"theater"}), "culture"),
    "culturePerTheater":               (frozenset({"theater"}), "culture"),
    "culturePerLabEqualToLevel":       (frozenset({"lab"}), "culture"),
    "sciencePerLab":                   (frozenset({"lab"}), "science"),
    "resourcesPerLabEqualToLevel":     (frozenset({"lab"}), "resources"),
    "sciencePerBestLabOrLibraryLevel": (frozenset({"lab", "library"}),
                                        "science"),
    # FAQ v1.5, "Internet": "Sid Meier does affect the amount of Culture
    # which the Internet Wonder gives you, as do William Shakespeare,
    # J. S. Bach, Charlie Chaplin, Isaac Newton, and Albert Einstein."
    # The Hollywood note names only Bach/Shakespeare/Chaplin, so the two
    # lab/library-science entries above count for the Internet but not for
    # Hollywood -- the `mtypes <= types` check in `building_output` enforces
    # exactly that split.
    "culturePerLibraryTheaterPair":    (frozenset({"library", "theater"}),
                                        "culture"),
    "doubleBestMine":                  (frozenset({"mine"}), "resources"),
}


def building_output(p, types, attrs):
    """Effective output of the player's buildings of `types`, over `attrs`.

    Printed per-worker production plus the in-play modifiers that change what
    those buildings produce, and nothing else.  See `_BUILDING_OUTPUT`.
    """
    db = _DB
    tot = 0
    for n, t in p.techs.items():
        if db.type_of(n) not in types or not t.workers:
            continue
        prod = db.get(n).get("production") or {}
        for a in attrs:
            v = prod.get(a, 0)
            if v:
                tot += v * t.workers
    for key, val in _output_modifiers(p):
        mtypes, attr = _BUILDING_OUTPUT[key]
        # a modifier counts only if EVERY building it reads is one the caller
        # asked about (Shakespeare's pair straddles a library and a theater,
        # so it counts for Hollywood but would not for a theaters-only card)
        if attr in attrs and mtypes <= types:
            tot += _building_modifier(p, key, val)
    return tot


def _output_modifiers(p):
    """(key, value) for every `_BUILDING_OUTPUT` modifier this player has."""
    db = _DB
    out = []
    srcs = [p.government]
    srcs.extend(p.techs)
    srcs.extend(p.colonies)
    # a wonder flipped by Ravages of Time is ruins: its effects are gone
    srcs.extend(w for w in p.completed_wonders if w not in p.flipped_wonders)
    if p.leader:
        srcs.append(p.leader)
    for n in srcs:
        if n not in _BY_NAME:
            continue
        for k, v in (db.get(n).get("effects") or {}).items():
            if k in _BUILDING_OUTPUT:
                out.append((k, v))
    return out


def _building_modifier(p, key, val):
    """How much one `_BUILDING_OUTPUT` modifier adds, in its own rating.

    Deliberately the same arithmetic as the matching branch of
    `_apply_modifier`: a card that scores a building's output and the rating
    that building feeds must never disagree.
    """
    db = _DB
    if key == "bestTheaterDoubleCulture":
        b = best_card(p, {"theater"}, require_workers=True)
        if b:
            return (db.get(b).get("production") or {}).get("culture", 0)
    elif key == "culturePerTheater":
        return val * workers_on_types(p, {"theater"})
    elif key == "culturePerLabEqualToLevel":
        return sum(db.level_of(n) * t.workers for n, t in p.techs.items()
                   if db.type_of(n) == "lab")
    elif key == "sciencePerLab":
        return val * workers_on_types(p, {"lab"})
    elif key == "resourcesPerLabEqualToLevel":
        return sum(db.level_of(n) * t.workers for n, t in p.techs.items()
                   if db.type_of(n) == "lab")
    elif key == "sciencePerBestLabOrLibraryLevel":
        # Leonardo / Newton / Einstein: "your best lab or library PRODUCES
        # extra science".  An unstaffed technology card is not a building and
        # produces nothing, so the best lab is the best STAFFED one -- the
        # same reading `bestTheaterDoubleCulture` and `doubleBestMine` below
        # already used (FAQ v1.5 p.9: "one worker on the best mine technology
        # card that has workers").  Measured against BGO on 150 human games:
        # 7303/7600 per-turn science rows exact with this reading against
        # 7275/7600 without it (docs/SCORE_AUDIT.md 3.9).
        b = best_card(p, {"lab", "library"}, require_workers=True)
        if b:
            return db.level_of(b)
    elif key == "culturePerLibraryTheaterPair":
        return val * min(workers_on_types(p, {"library"}),
                         workers_on_types(p, {"theater"}))
    elif key == "doubleBestMine":
        b = best_card(p, {"mine"}, require_workers=True)
        if b:
            return (db.get(b).get("production") or {}).get("resources", 0)
    return 0


def mine_resources(p):
    """Resources produced BY THIS PLAYER'S MINES (Impact of Industry)."""
    return building_output(p, frozenset({"mine"}), ("resources",))


def farm_food(p):
    """Food produced BY THIS PLAYER'S FARMS (Impact of Agriculture).

    The twin of `mine_resources`, and it exists for the same reason: the card
    scores "the food produced by their farms", not the food RATING, which also
    carries a pact's food symbol (`International Trade Agreement`, side B).
    `docs/SCORE_AUDIT.md` 10.3.1 found and fixed that on the mine side and
    left it standing here -- and the human corpus could not catch it, because
    the replayer models no pacts.  See docs/SCORE_AUDIT.md 3.1.
    """
    return building_output(p, frozenset({"farm"}), ("food",))


# Effect keys that need bespoke handling in `_apply_special` (everything
# outside FLAT_KEYS / MODIFIER_KEYS that touches Stats at all).
SPECIAL_KEYS = frozenset((
    "buildDiscount", "wonderStagesPerAction", "popIncreaseFoodDiscount",
    "freePopIncreasePerTurn", "cannotPlayAggressionOrWar",
    "technologyScienceDiscount", "cannotBeDeclaredWarOnByAnyone",
    "mayUseFoodAsResource", "mayUseResourceAsFood",
))

# --------------------------------------------------------------------------
# Compiled effect programs.
#
# Card data is loaded once and never mutated, so the classification of every
# `effects` / `production` dict (which keys are flat Stats fields, which are
# phase-3 modifiers, which need bespoke code) is a pure function of the dict
# and can be done exactly once instead of on every `compute`.  The cache is
# keyed by `id(dict)` and keeps a strong reference to the dict itself, so an
# id can never be recycled underneath us.
# --------------------------------------------------------------------------

_EFF_PROG = {}       # id(eff) -> (eff, flat_tuple, mods_tuple, special_tuple)
_PROD_PROG = {}      # id(prod) -> (prod, items_tuple)

_EMPTY = ()


def _compile_effects(eff):
    flat, mods, special = [], [], []
    for k, v in eff.items():
        if k in MODIFIER_KEYS:
            mods.append((k, v))
        else:
            attr = FLAT_KEYS.get(k)
            if attr is not None:
                flat.append((attr, v))
            elif k in SPECIAL_KEYS:
                special.append((k, v))
            # everything else is action-time / trigger-time, elsewhere
    prog = (eff, tuple(flat), tuple(mods), tuple(special))
    _EFF_PROG[id(eff)] = prog
    return prog


def _compile_production(prod):
    items = []
    for k, v in prod.items():
        attr = FLAT_KEYS.get(k)
        if attr is not None:
            items.append((attr, v))
    prog = (prod, tuple(items))
    _PROD_PROG[id(prod)] = prog
    return prog


def _apply_special(s, special):
    for k, v in special:
        if k == "buildDiscount":
            bd = s.build_discount
            for age, d in v.items():
                bd[age] = bd.get(age, 0) + d
        elif k == "wonderStagesPerAction":
            if v > s.wonder_stages:
                s.wonder_stages = v
        elif k == "popIncreaseFoodDiscount":
            s.pop_food_discount += v
        elif k == "freePopIncreasePerTurn":
            s.free_pop_per_turn = True
        elif k == "cannotPlayAggressionOrWar":
            s.no_aggression = True
        elif k == "technologyScienceDiscount":
            s.tech_discount += v
        elif k == "cannotBeDeclaredWarOnByAnyone":
            s.war_immune = True
        elif k == "mayUseFoodAsResource":
            s.food_as_resource += v
        elif k == "mayUseResourceAsFood":
            s.resource_as_food += v


def _add_production(s, prod, mult=1):
    if not prod:
        return
    try:
        items = _PROD_PROG[id(prod)][1]
    except KeyError:
        items = _compile_production(prod)[1]
    if not items:
        return
    d = s.__dict__
    for attr, v in items:
        d[attr] += v * mult


def _apply_flat(s, eff, mods):
    """Apply flat effect keys; queue modifier keys for phase 3."""
    if not eff:
        return
    try:
        prog = _EFF_PROG[id(eff)]
    except KeyError:
        prog = _compile_effects(eff)
    _, flat, mkeys, special = prog
    if flat:
        d = s.__dict__
        for attr, v in flat:
            d[attr] += v
    if mkeys:
        mods.extend(mkeys)
    if special:
        _apply_special(s, special)


# name -> (per_worker_items, special_tech_effects_or_None).  Phase 1 of
# `compute` walks every technology in the tableau on every recomputation;
# what each card contributes per worker is fixed by the card data.
_TECH_PROG = {}


def _tech_prog(name):
    db = _DB
    card = db.by_name[name]
    typ = card["type"]
    eff = None
    items = ()
    if typ == "special-tech":
        eff = card.get("effects") or {}
    elif typ in C.URBAN_TYPES:
        items = _compile_production(card.get("production") or {})[1]
    elif typ in C.UNIT_TYPES:
        st = card.get("strength") or 0
        items = (("strength", st),) if st else ()
    elif typ == "farm":
        v = (card.get("production") or {}).get("food", 0)
        items = (("food", v),) if v else ()
    elif typ == "mine":
        v = (card.get("production") or {}).get("resources", 0)
        items = (("resources", v),) if v else ()
    prog = _TECH_PROG[name] = (items, eff)
    return prog


def compute(state, p):
    """Full statistics for a player (ratings, action totals, production)."""
    db = _DB
    s = Stats(build_discount={})
    mods = []

    gov = db.get(p.government)
    s.civil_actions = gov.get("civilActions") or 4
    s.military_actions = gov.get("militaryActions") or 2
    s.urban_limit = gov.get("urbanBuildingLimit") or 2
    _add_production(s, gov.get("production"))
    _apply_flat(s, gov.get("effects"), mods)

    # --- phase 1: technologies
    sd = s.__dict__
    for name, t in p.techs.items():
        try:
            per_worker, eff = _TECH_PROG[name]
        except KeyError:
            per_worker, eff = _tech_prog(name)
        if eff is not None:
            _apply_flat(s, eff, mods)
        elif per_worker:
            w = t.workers
            if w:
                for attr, v in per_worker:
                    sd[attr] += v * w

    # --- phase 2: wonders, leader, colonies
    for w in p.completed_wonders:
        if w in p.flipped_wonders:
            # Ravages of Time: effects gone, ruins produce culture instead
            s.culture += 2
            continue
        _apply_flat(s, db.get(w).get("effects"), mods)
        if p.homer_wonder == w:
            s.happy += 1
    if p.leader:
        _apply_flat(s, db.get(p.leader).get("effects"), mods)
        _add_production(s, db.get(p.leader).get("production"))
    for col in p.colonies:
        card = db.get(col) if col in db.by_name else None
        if card:
            _apply_flat(s, card.get("effects"), mods)
            _add_production(s, card.get("permanent"))
            _apply_flat(s, _colony_permanents(card), mods)
    _apply_pacts(state, s, p, mods)

    # event-granted permanents
    s.culture += p.culture_rate_extra
    s.science += p.science_rate_extra
    s.strength += p.strength_extra
    s.happy += p.happy_extra

    # --- phase 3: modifiers
    for key, val in mods:
        _apply_modifier(s, p, key, val)

    s.strength += army_strength(state, p)
    # "Limits on Ratings" (rulebook, Civilization Statistics): *none* of your
    # ratings can ever go below zero -- if the total contribution of your cards
    # and workers is negative, that rating is 0.  Only happiness was clamped
    # here before, which let Age III Fundamentalism (production science -2)
    # drive a player's science *stock* negative once its penalty outweighed the
    # player's labs/libraries.
    s.science = max(0, s.science)
    s.culture = max(0, s.culture)
    s.food = max(0, s.food)
    s.resources = max(0, s.resources)
    s.strength = max(0, s.strength)
    s.happy = max(0, min(8, s.happy))
    return s


def _apply_modifier(s, p, key, val):
    db = _DB
    if key == "strengthPerMilitaryUnit":
        s.strength += val * workers_on_types(p, C.UNIT_TYPES)
    elif key == "strengthPerInfantry":
        s.strength += val * workers_on_types(p, {"infantry"})
    elif key == "strengthPerArtillery":
        s.strength += val * workers_on_types(p, {"artillery"})
    elif key == "strengthPerUnitType":
        types = {db.type_of(n) for n, t in p.techs.items()
                 if db.type_of(n) in C.UNIT_TYPES and t.workers}
        s.strength += val * len(types)
    elif key == "strengthPerTempleOrGovernmentHappy":
        happy = _happy_from(p, {"temple"}) + (
            (db.get(p.government).get("production") or {}).get("happy", 0))
        s.strength += val * happy
    elif key == "sciencePerBestLabOrLibraryLevel":
        # see `_building_modifier`: the best lab or library is the best
        # STAFFED one (docs/SCORE_AUDIT.md 3.9).  These two branches are
        # deliberately the same arithmetic and must not drift apart.
        b = best_card(p, {"lab", "library"}, require_workers=True)
        if b:
            s.science += db.level_of(b)
    elif key == "culturePerTheater":
        s.culture += val * workers_on_types(p, {"theater"})
    elif key == "culturePerLabEqualToLevel":
        for n, t in p.techs.items():
            if db.type_of(n) == "lab":
                s.culture += db.level_of(n) * t.workers
    elif key == "sciencePerLab":
        s.science += val * workers_on_types(p, {"lab"})
    elif key == "resourcesPerLabEqualToLevel":
        for n, t in p.techs.items():
            if db.type_of(n) == "lab":
                s.resources += db.level_of(n) * t.workers
    elif key == "bestTheaterDoubleCulture":
        b = best_card(p, {"theater"}, require_workers=True)
        if b:
            s.culture += (db.get(b).get("production") or {}).get("culture", 0)
    elif key == "culturePerHappyFromTemplesTheatersWonders":
        happy = _happy_from(p, {"temple", "theater"})
        for w in p.completed_wonders:
            # a wonder flipped by Ravages of Time is ruins: "its effects no
            # longer apply", so it provides no happy face for Michelangelo to
            # be paid for.  `compute` phase 2 and `_output_modifiers` already
            # filter it; this reader did not (docs/SCORE_AUDIT.md 3.3).
            if w in p.flipped_wonders:
                continue
            happy += (db.get(w).get("effects") or {}).get("happy", 0)
        s.culture += val * max(0, happy)
    elif key == "culturePerLibraryTheaterPair":
        pairs = min(workers_on_types(p, {"library"}),
                    workers_on_types(p, {"theater"}))
        s.culture += val * pairs
    elif key == "extraHappyPerHappySource":
        s.happy += val * _happy_source_count(p)
    elif key == "doubleBestMine":
        b = best_card(p, {"mine"}, require_workers=True)
        if b:
            s.resources += (db.get(b).get("production") or {}).get("resources", 0)
    elif key == "cultureFirstColony":
        if p.colonies:
            s.culture += val
    elif key == "culturePerAdditionalColony":
        s.culture += val * max(0, len(p.colonies) - 1)


# ------------------------------------------------------------- pacts (§5.9)

COLONY_PERMANENT_KEYS = {"strength", "happiness", "cultureProduction",
                         "scienceProduction", "foodProduction",
                         "resourceProduction", "colonizationBonus",
                         "civilActions", "militaryActions", "culture",
                         "science", "food", "resources", "happy"}


_COLONY_PERM_CACHE = {}


def _colony_permanents(card):
    """Rating symbols on a territory card (token grants are applied once).

    Memoized by card name: the result feeds `_apply_flat`, whose compiled
    program cache is keyed by dict identity, so a fresh dict per call would
    defeat it (and leak).
    """
    name = card["name"]
    out = _COLONY_PERM_CACHE.get(name)
    if out is None:
        perm = card.get("permanentEffects") or {}
        out = _COLONY_PERM_CACHE[name] = {
            k: v for k, v in perm.items() if k in COLONY_PERMANENT_KEYS}
    return out


def pacts_for(state, idx):
    """Every pact `idx` is party to, wherever it physically sits (§5.9)."""
    out = []
    for q in state.players:
        for pact in q.pacts:
            if idx in (pact["owner"], pact["partner"]):
                out.append(pact)
    return out


def pact_partner(pact, idx):
    return pact["partner"] if pact["owner"] == idx else pact["owner"]


def _pact_blocks(pact, idx):
    eff = _DB.get(pact["name"]).get("effects") or {}
    blocks = []
    if isinstance(eff.get("bothPlayers"), dict):
        blocks.append(eff["bothPlayers"])
    if idx == pact.get("a") and isinstance(eff.get("A"), dict):
        blocks.append(eff["A"])
    if idx == pact.get("b") and isinstance(eff.get("B"), dict):
        blocks.append(eff["B"])
    return blocks


def _apply_pacts(state, s, p, mods):
    for pact in pacts_for(state, p.idx):
        other = state.players[pact_partner(pact, p.idx)]
        for block in _pact_blocks(pact, p.idx):
            _apply_flat(s, block, mods)
            per = block.get(
                "cultureProductionPerCompletedWonderOfTheOtherParty")
            if per:
                s.culture += per * len(other.completed_wonders)
            if block.get("otherPartyPaysScience"):
                s.science_partners.append(other.idx)


def _pact_effects(pact):
    return _DB.get(pact["name"]).get("effects") or {}


def pact_forbids_attack(state, attacker, defender):
    """§5.4.2 / §5.6: a pact may make an attack illegal."""
    if state_stats(state, defender).war_immune:
        pass       # only blocks wars; checked separately by war_forbidden()
    for pact in pacts_for(state, attacker.idx):
        if pact_partner(pact, attacker.idx) != defender.idx:
            continue
        if _pact_effects(pact).get("noAttacksBetweenParties"):
            return True
    return False


def war_forbidden(state, attacker, defender):
    return (pact_forbids_attack(state, attacker, defender)
            or state_stats(state, defender).war_immune)


def pact_attack_bonus(state, attacker, defender):
    """Strength a pact grants the attacker when the parties fight (§5.4.2)."""
    tot = 0
    for pact in pacts_for(state, attacker.idx):
        if pact_partner(pact, attacker.idx) != defender.idx:
            continue
        block = _pact_effects(pact).get("onAttackBetweenParties") or {}
        tot += block.get("attackerStrength", 0) or 0
    return tot


def _doomed_pact_strength(state, one, other, idx):
    """Strength `idx` draws from a pact between `one` and `other` that ends
    the moment either of them attacks (§5.4.3).

    FAQ p.11: "The Military Strength given by either Pact will not affect
    any War or Aggression which is declared between the two civilizations
    -- for the Pact is cancelled immediately."  That is true of BOTH
    parties, so the same helper serves the attacker and the defender.
    """
    total = 0
    for pact in pacts_for(state, one.idx):
        if pact_partner(pact, one.idx) != other.idx:
            continue
        if not _pact_effects(pact).get("cancelledIfPartiesAttackEachOther"):
            continue
        for block in _pact_blocks(pact, idx):
            total += block.get("strength", 0) or 0
    return total


def attack_strength(state, attacker, defender):
    """§5.4.2: the attacker's strength for an attack on `defender`.

    Includes bonuses that trigger when attacking them and EXCLUDES strength
    granted by a pact between the two that ends the moment they attack --
    so `legal_moves` and the resolution in events.py agree.
    """
    total = state_stats(state, attacker).strength
    total += pact_attack_bonus(state, attacker, defender)
    total -= _doomed_pact_strength(state, attacker, defender, attacker.idx)
    return max(0, total)


def defense_strength(state, attacker, defender):
    """§5.4.2: the defender's strength for the legality comparison.

    The mirror image of `attack_strength`.  A pact that ends when the two
    attack each other is removed BEFORE the aggression is resolved
    (`cancel_attack_pacts`), so `start_defense` already sees a defender
    without it; the "may I attack at all?" test in `legal_moves` has to
    agree, or a legal attack looks illegal by up to the pact's strength.
    """
    total = state_stats(state, defender).strength
    total -= _doomed_pact_strength(state, attacker, defender, defender.idx)
    return max(0, total)


def cancel_attack_pacts(state, attacker, defender):
    """§5.4.3: a pact that ends on attack is removed before resolving."""
    changed = False
    for q in state.players:
        keep = []
        for pact in q.pacts:
            parties = (pact["owner"], pact["partner"])
            if (attacker.idx in parties and defender.idx in parties
                    and _pact_effects(pact).get(
                        "cancelledIfPartiesAttackEachOther")):
                changed = True
                continue
            keep.append(pact)
        q.pacts = keep
    if changed:
        invalidate(state)


def drop_pacts_of(state, idx):
    """Remove every pact `idx` is party to (resignation, §5.11)."""
    for q in state.players:
        q.pacts = [pact for pact in q.pacts
                   if idx not in (pact["owner"], pact["partner"])]
    invalidate(state)


# ------------------------------------------------- resource substitution

def avail_resources(state, p):
    """Resources spendable, counting a Trade Routes pact's food (§5.9)."""
    s = state_stats(state, p)
    return p.resources + min(s.food_as_resource, p.food)


def spend_resources(state, p, n):
    take = min(p.resources, n)
    p.resources -= take
    rest = n - take
    if rest > 0:
        p.food = max(0, p.food - rest)


def avail_food(state, p):
    s = state_stats(state, p)
    return p.food + min(s.resource_as_food, p.resources)


def spend_food(state, p, n):
    take = min(p.food, n)
    p.food -= take
    rest = n - take
    if rest > 0:
        p.resources = max(0, p.resources - rest)


def _happy_from(p, types):
    db = _DB
    tot = 0
    for n, t in p.techs.items():
        if db.type_of(n) in types:
            tot += (db.get(n).get("production") or {}).get("happy", 0) * t.workers
    return tot


def _happy_source_count(p):
    """Number of cards/buildings providing happy faces (St. Peter's).

    "every building/CARD providing happy faces provides one additional happy
    face" -- so the government card, the leader card and a colony card all
    count, not only buildings.  Two of those three were already counted; the
    colony was not (docs/SCORE_AUDIT.md 3.7).  A wonder ruined by Ravages of
    Time provides no happy face and is therefore not a source (3.3).
    """
    db = _DB
    n = 0
    for name, t in p.techs.items():
        if (db.get(name).get("production") or {}).get("happy", 0) > 0:
            n += t.workers
    for w in p.completed_wonders:
        if w in p.flipped_wonders:
            continue
        if (db.get(w).get("effects") or {}).get("happy", 0) > 0:
            n += 1
    if p.leader and (db.get(p.leader).get("effects") or {}).get("happy", 0) > 0:
        n += 1
    if (db.get(p.government).get("production") or {}).get("happy", 0) > 0:
        n += 1
    for col in p.colonies:
        if col not in _BY_NAME:
            continue
        perm = _BY_NAME[col].get("permanentEffects") or {}
        if (perm.get("happiness", 0) or perm.get("happy", 0)) > 0:
            n += 1
    return n


# ---------------------------------------------------------------- armies


def army_strength(state, p):
    """Tactical strength from armies formed by the current tactic (§10).

    Hot path: called once per `compute`.  The tactic is checked FIRST (a
    player without one always scores 0) and the unit multiset is accumulated
    as type->count / type->fresh-count dicts instead of materialising one
    tuple per worker.
    """
    tactic = p.tactic
    if not tactic:
        return 0
    db = _DB
    if tactic not in db.by_name:
        return 0
    card = db.by_name[tactic]
    comp = card.get("composition") or []
    if not comp:
        return 0
    tactic_lv = db.level_by_name[tactic]
    type_of = db.type_by_name
    level_of = db.level_by_name
    unit_types = C.UNIT_TYPES
    avail = {}
    fresh = {}
    for n, t in p.techs.items():
        w = t.workers
        if not w:
            continue
        typ = type_of[n]
        if typ not in unit_types:
            continue
        avail[typ] = avail.get(typ, 0) + w
        if level_of[n] >= tactic_lv - 1:
            fresh[typ] = fresh.get(typ, 0) + w
    return _army_strength_counts(p, card, comp, tactic_lv, avail, fresh,
                                 avail.get("air", 0))


def army_strength_units(state, p, units):
    """§10.3-10.5 for an explicit (type, level) unit multiset.

    Also used for colonization forces, where only the sacrificed units
    form armies (§10.7).
    """
    db = _DB
    if not p.tactic or p.tactic not in db.by_name:
        return 0
    card = db.get(p.tactic)
    comp = card.get("composition") or []
    if not comp:
        return 0
    tactic_lv = db.level_of(p.tactic)
    avail = {}
    fresh = {}
    for typ, lv in units:
        avail[typ] = avail.get(typ, 0) + 1
        if lv >= tactic_lv - 1:
            fresh[typ] = fresh.get(typ, 0) + 1
    return _army_strength_counts(p, card, comp, tactic_lv, avail, fresh,
                                 avail.get("air", 0))


def tactic_outlook(state, p, names):
    """(best_strength, units_short) over the tactics `p` could switch to.

    Answers the two questions `army_strength` cannot, because it only ever
    looks at the tactic already in play:

    * **best_strength** -- the army strength `p` would have if it played the
      best tactic it has access to right now.  `army_strength(state, p)`
      subtracted from this is the strength sitting unclaimed on the table.
    * **units_short** -- how many more unit workers that same tactic needs
      before it forms ONE MORE army.  0 when the next army is already there.

    Why both.  A tactic is worthless without units and units are worth much
    more with a tactic, and the 1-ply search cannot see either half from the
    other side: playing a tactic with no army for it is +0 strength for a
    military action and a card, and building the unit that would complete an
    army is +printed-strength only, because the tactic is not in play yet.
    So the bot does not play the tactic (no army) and does not build the unit
    (no tactic).  `best_strength` breaks the deadlock in the play direction --
    it goes up the moment the units exist -- and `units_short` is the gradient
    that gets you there, since `best_strength` alone is a step function that
    is flat at 0 for the first two of the three cavalry.

    Candidates are the caller's business (`weighted.tactic_terms` passes the
    tactics in hand plus `state.available_tactics`, i.e. everything reachable
    with one military action or two).  The chosen candidate maximizes
    `(strength, -shortfall, printed bonus)` so that with nothing formable yet
    the shortfall reported is the nearest tactic's, not an arbitrary one's.
    """
    db = _DB
    type_of = db.type_by_name
    level_of = db.level_by_name
    unit_types = C.UNIT_TYPES
    counts = []
    for n, t in p.techs.items():
        w = t.workers
        if not w:
            continue
        typ = type_of[n]
        if typ in unit_types:
            counts.append((typ, level_of[n], w))
    best = None
    for name in names:
        card = db.by_name.get(name)
        if card is None:
            continue
        comp = card.get("composition") or []
        if not comp:
            continue
        lv = level_of[name]
        avail = {}
        fresh = {}
        for typ, ulv, w in counts:
            avail[typ] = avail.get(typ, 0) + w
            if ulv >= lv - 1:
                fresh[typ] = fresh.get(typ, 0) + w
        val = _army_strength_counts(p, card, comp, lv, avail, fresh,
                                    avail.get("air", 0))
        need = _tactic_need(comp)
        armies = min((avail.get(t, 0) // c for t, c in need.items()),
                     default=0)
        short = sum(max(0, c * (armies + 1) - avail.get(t, 0))
                    for t, c in need.items())
        key = (val, -short, card.get("strength") or 0)
        if best is None or key > best[0]:
            best = (key, val, short)
    if best is None:
        return 0, 0
    return best[1], best[2]


_TACTIC_NEED = {}     # id(composition list) -> (comp, need dict)


def _tactic_need(comp):
    try:
        return _TACTIC_NEED[id(comp)][1]
    except KeyError:
        need = {}
        for typ in comp:
            need[typ] = need.get(typ, 0) + 1
        _TACTIC_NEED[id(comp)] = (comp, need)
        return need


def _army_strength_counts(p, card, comp, tactic_lv, avail, fresh, air):
    """Shared core of the two entry points above, on type->count dicts."""
    need = _tactic_need(comp)
    if p.leader != "Genghis Khan":
        # fast path: armies = min over required types of avail // needed
        total_armies = min([avail.get(t, 0) // c for t, c in need.items()],
                           default=0)
        if not total_armies:
            return 0
        fresh_armies = min(
            min([fresh.get(t, 0) // c for t, c in need.items()], default=0),
            total_armies)
        return _army_value(card, total_armies, fresh_armies, air)
    genghis = True

    def count_armies(avail):
        if genghis:
            # infantry may fill cavalry slots
            inf = avail.get("infantry", 0)
            cav = avail.get("cavalry", 0)
            need_inf, need_cav = need.get("infantry", 0), need.get("cavalry", 0)
            if need_inf or need_cav:
                total = inf + cav
                best = 0
                for k in range(total + 1):
                    # k armies need k*need_inf infantry-ish + k*need_cav
                    if k * (need_inf + need_cav) <= total and \
                       k * need_cav <= total and \
                       all(k * need[t] <= avail.get(t, 0)
                           for t in need if t not in ("infantry", "cavalry")):
                        best = k
                return best
        return min((avail.get(t, 0) // c for t, c in need.items()), default=0)

    # armies whose units are all recent enough are not outdated
    total_armies = count_armies(avail)
    if not total_armies:
        return 0
    fresh_armies = min(count_armies(fresh), total_armies)
    return _army_value(card, total_armies, fresh_armies, air)


def _army_value(card, total_armies, fresh_armies, air):
    outdated_armies = total_armies - fresh_armies
    val = card.get("strength") or 0
    old_val = card.get("obsoleteStrength")
    if old_val is None:
        old_val = val
    total = fresh_armies * val + outdated_armies * old_val
    # an air force doubles the tactical bonus of one army (§10.5), and each
    # air unit may be assigned to at most one army.  DOUBLING AN OUTDATED
    # ARMY IS WORTH `obsoleteStrength`, not `strength`: with one fresh and
    # one outdated army, two air units are worth val + old_val, not 2 * val.
    # Counted from `units`, so a colonization force only benefits from the
    # air units actually sacrificed into it (§11.3).
    if air:
        assigned = min(air, total_armies)
        on_fresh = min(assigned, fresh_armies)
        total += on_fresh * val + (assigned - on_fresh) * old_val
    return total


# ------------------------------------------------------- blue token math


def _denoms(p, typ, key):
    """Blue-token denominations available to `p` for a farm/mine resource.

    `key` is kept for the call sites' readability; the (type, value) pair is
    precomputed per card in the DB, so this is one dict probe per tech.
    """
    denom = _DENOM_BY_NAME
    ds = {1}
    for n in p.techs:
        d = denom.get(n)
        if d is not None and d[0] == typ:
            ds.add(d[1])
    return sorted(ds, reverse=True)


def tokens_for(amount, denoms):
    """Minimal number of blue tokens holding `amount` (greedy; 1 present)."""
    n = 0
    for d in denoms:
        if d <= 0:
            continue
        while amount >= d:
            amount -= d
            n += 1
    return n + max(0, amount)


def blue_used(p):
    used = tokens_for(p.food, _denoms(p, "farm", "food"))
    used += tokens_for(p.resources, _denoms(p, "mine", "resources"))
    if p.wonder:
        used += p.wonder.steps_built
    return used


def blue_available(p):
    return max(0, p.blue_total - blue_used(p))


def gain_food(p, n):
    """Gain up to n food, limited by the blue bank (§6.4). Returns gained."""
    return _gain(p, n, "food")


def gain_resources(p, n):
    return _gain(p, n, "resources")


def _gain(p, n, attr):
    if n <= 0:
        return 0
    free = blue_available(p)
    if free <= 0:
        return 0
    cur = getattr(p, attr)
    denoms = _denoms(p, "farm" if attr == "food" else "mine",
                     "food" if attr == "food" else "resources")
    base = tokens_for(cur, denoms)
    for want in range(n, 0, -1):
        if tokens_for(cur + want, denoms) - base <= free:
            setattr(p, attr, cur + want)
            return want
    return 0


def pay_resources(p, n):
    """Pay n resources, food covering any shortfall is NOT done here."""
    paid = min(p.resources, n)
    p.resources -= paid
    return paid


# ------------------------------------------------------------- costs


def build_cost(state, p, name):
    """Resource cost to build a worker onto technology `name`."""
    card = _BY_NAME[name]
    cost = card.get("buildCost")
    if cost is None:
        return None
    typ = card["type"]
    # `state_stats` and the one-time-discount lookup are only consulted on the
    # branches that can actually use them (hot: called once per buildable card
    # per move-generation pass).
    if p.one_time_discount and typ in C.URBAN_OR_PRODUCTION:
        cost -= (p.one_time_discount.get("build") or {}).get("resources", 0)
    if typ in C.URBAN_TYPES:
        bd = state_stats(state, p).build_discount
        if bd:
            cost -= bd.get(card["age"], 0)
        if p.leader == "William Shakespeare":
            type_of = _TYPE_BY_NAME
            if typ == "theater" and any(type_of[n] == "library"
                                        for n in p.techs):
                cost -= 1
            elif typ == "library" and any(type_of[n] == "theater"
                                          for n in p.techs):
                cost -= 1
    return cost if cost > 0 else 0


def tech_cost(state, p, name):
    """Science cost to develop technology `name`."""
    card = _BY_NAME[name]
    typ = card["type"]
    if typ == "government":
        cost = card.get("peacefulCost")
    else:
        cost = card.get("techCost")
    if cost is None:
        return None
    cost -= state_stats(state, p).tech_discount
    if p.one_time_discount:
        cost -= (p.one_time_discount.get("developTechnology") or {}).get(
            "science", 0)
    if typ == "theater":
        if p.leader == "J. S. Bach":
            cost -= 2
        if p.leader == "William Shakespeare" and \
                any(_TYPE_BY_NAME[n] == "library" for n in p.techs):
            cost -= 1
    if typ == "library" and p.leader == "William Shakespeare" and \
            any(_TYPE_BY_NAME[n] == "theater" for n in p.techs):
        cost -= 1
    return max(0, cost)


_STATS_CACHE_KEY = "_stats_cache"


def stats_key(state, p):
    """Everything `compute(state, p)` reads, as a cheap comparable tuple.

    `invalidate()` only marks a cached Stats *dirty*; the next read rebuilds
    this key and, if it is unchanged, keeps the cached object.  ~83% of the
    invalidations in self-play are spurious (the move touched a pool the
    ratings do not depend on), and building the key costs about a tenth of a
    `compute`.

    INVARIANT: every field `compute` (and everything it calls: `_apply_flat`,
    `_apply_modifier`, `_apply_pacts`, `army_strength`) reads off `p` or
    `state` must appear here.  Run the suite with TTA_PARANOID=1 to have every
    cache hit verified against a fresh `compute`.
    """
    techs = p.techs
    pacts = ()
    idx = p.idx
    for q in state.players:
        for pact in q.pacts:
            owner = pact["owner"]
            partner = pact["partner"]
            if owner == idx or partner == idx:
                other = state.players[partner if owner == idx else owner]
                if not pacts:
                    pacts = []
                pacts.append((pact["name"], owner, partner,
                              pact.get("a"), pact.get("b"),
                              len(other.completed_wonders)))
    return (p.government, p.leader, p.tactic, p.homer_wonder,
            tuple(techs), tuple([t.workers for t in techs.values()]),
            tuple(p.completed_wonders), tuple(p.flipped_wonders),
            tuple(p.colonies),
            p.culture_rate_extra, p.science_rate_extra,
            p.strength_extra, p.happy_extra,
            p.yellow_bank,
            tuple(pacts))


def state_stats(state, p):
    """Cached per-mutation stats (invalidated by engine.actions.apply).

    Hot: ~10 calls per generated move.  A cache entry is `[stats, key,
    dirty]`; the common case (clean entry) is one dict probe and two list
    indexings.

    `GameState._stats_cache` is a class-level `None` (see state.py), so a cold
    state -- which is what every `copy_state` and every `journal.begin`
    produces, since the cache is `_`-prefixed and never copied -- reads it
    without raising.  This used to be a caught `AttributeError` and was 23,305
    of the 44,020 exceptions raised per 4p 4-game batch.
    """
    cache = state._stats_cache
    if cache is None:
        cache = state._stats_cache = {}
    idx = p.idx
    ent = cache.get(idx)
    if ent is not None:
        if not ent[2]:
            return ent[0]
        key = stats_key(state, p)
        if key == ent[1]:
            ent[2] = False
            return ent[0]
    else:
        key = stats_key(state, p)
    st = compute(state, p)
    cache[idx] = [st, key, False]
    return st


def _state_stats_paranoid(state, p):
    """TTA_PARANOID=1: verify every cache hit against a fresh computation."""
    st = _state_stats_fast(state, p)
    fresh = compute(state, p)
    if fresh.__dict__ != st.__dict__:
        raise AssertionError(
            f"stale stats for player {p.idx}: cached={st!r} fresh={fresh!r}")
    return st


_state_stats_fast = state_stats
if _PARANOID:
    state_stats = _state_stats_paranoid


def invalidate(state, p=None):
    cache = getattr(state, _STATS_CACHE_KEY, None)
    if not cache:
        return
    if p is None:
        for ent in cache.values():
            ent[2] = True
    else:
        ent = cache.get(p.idx)
        if ent is not None:
            ent[2] = True


# ------------------------------------------------------ enter/leave play


def grant_yellow(p, n):
    """Move `n` yellow tokens into `p`'s supply from a card or a rival."""
    if n > 0:
        p.yellow_granted += n
    p.yellow_bank = max(0, p.yellow_bank + n)


def on_enter_play(state, p, name):
    """Immediate one-time effects when a card enters play."""
    db = _DB
    eff = db.get(name).get("effects") or {}
    if "blueTokens" in eff:
        p.blue_total += eff["blueTokens"]
    if "yellowTokens" in eff:
        grant_yellow(p, eff["yellowTokens"])
    invalidate(state, p)


def on_leave_play(state, p, name):
    db = _DB
    eff = db.get(name).get("effects") or {}
    if "blueTokens" in eff:
        p.blue_total = max(0, p.blue_total - eff["blueTokens"])
    if "yellowTokens" in eff:
        p.yellow_bank = max(0, p.yellow_bank - eff["yellowTokens"])
    if "cultureOnLeaveEqualToLabResourceProduction" in eff:
        # Bill Gates: "when Bill Gates is removed from the game OR the game
        # ends".  `end_of_game_bonus` implements the second half; this is the
        # first, and without it replacing him (or Iconoclasm discarding him)
        # threw the whole bonus away (docs/SCORE_AUDIT.md 3.2).
        p.culture += _lab_level_workers(p)
    invalidate(state, p)


# ---------------------------------------------------------- triggers


def on_take_card(state, p, name):
    """Aristotle: 1 science per technology card taken from the row."""
    db = _DB
    if p.leader == "Aristotle" and _is_technology(db.get(name)):
        p.science += 1


def _is_technology(card):
    return card["type"] in C.DEVELOPABLE_TYPES


def on_develop(state, p, name):
    """Leader triggers when a technology card is played (§ leaders)."""
    if p.leader == "Leonardo da Vinci":
        gain_resources(p, 1)
    elif p.leader == "Albert Einstein":
        p.culture += 3
    elif p.leader == "Isaac Newton":
        s = state_stats(state, p)
        p.civil_actions = min(s.civil_actions, p.civil_actions + 1)


def on_build_unit(state, p, name):
    if p.leader == "Homer":
        gain_resources(p, 1)


def wonder_completion_culture(state, p, name):
    """Culture an Age III wonder would score if completed NOW, without paying it.

    THE SINGLE IMPLEMENTATION.  `on_wonder_complete` below pays this out for
    real and `engine/bots/board_yields.py:_on_build_culture` asks it what a
    wonder in hand would be worth, so the scorer and the evaluator cannot
    disagree about Hollywood.  That is not a stylistic preference: lines
    1197-1202 of this file are the note left by the last person who kept two
    implementations of one rule, and the evaluator's copy is the one that
    drifts silently.  `tests/test_card_pricing.py:TestOneImplementation`
    fails if `on_wonder_complete` ever stops routing through here.

    Pure: reads `p`, returns a number, touches nothing.
    """
    db = _DB
    card = db.get(name)
    eff = card.get("effects") or {}
    if eff.get("onBuildCulturePerTechLevelSum"):
        gained = sum(db.level_of(n) for n in p.techs
                     if _is_technology(db.get(n)))
        return gained + db.level_of(p.government)
    if "onBuildCulture" in eff:
        return _one_time_culture(state, p, name)
    return 0


def on_wonder_complete(state, p, name):
    """Age III wonders score a one-time culture bonus (§9.2)."""
    gained = wonder_completion_culture(state, p, name)
    p.culture += gained
    return gained


def _one_time_culture(state, p, name):
    if name == "Fast Food Chains":
        return (2 * workers_on_types(p, C.PRODUCTION_TYPES)
                + workers_on_types(p, C.URBAN_OR_UNIT))
    # Hollywood and the Internet score what the buildings ACTUALLY produce,
    # not their printed production: see `_BUILDING_OUTPUT`.  Before that these
    # two summed printed values with an ad-hoc Sid Meier special case, which
    # under-scored every Chaplin, Shakespeare, Newton and Einstein completion.
    if name == "Hollywood":
        return 2 * building_output(p, frozenset({"theater", "library"}),
                                   ("culture",))
    if name == "Internet":
        return building_output(p, frozenset(C.URBAN_TYPES),
                               ("culture", "science", "strength"))
    return 0


def _lab_level_workers(p):
    """Sum of (lab level x workers) -- Bill Gates' extra resource production.

    One implementation, because the card pays it twice: once per turn as
    resources (`resourcesPerLabEqualToLevel`) and once as culture when he
    leaves play or the game ends.
    """
    db = _DB
    return sum(db.level_of(n) * t.workers for n, t in p.techs.items()
               if db.type_of(n) == "lab")


def end_of_game_bonus(state, p):
    """Bill Gates and friends (§12.5.3)."""
    if p.leader == "Bill Gates":
        return _lab_level_workers(p)
    return 0
