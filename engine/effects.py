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

from dataclasses import dataclass, field

from . import cards as C

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
    db = C.db()
    return [n for n in p.techs if db.type_of(n) in types]


def workers_on_types(p, types):
    db = C.db()
    return sum(t.workers for n, t in p.techs.items() if db.type_of(n) in types)


def best_card(p, types, require_workers=False):
    """Highest-level technology card of the given types (None if none)."""
    db = C.db()
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


def _add_production(s, prod, mult=1):
    for k, v in (prod or {}).items():
        attr = FLAT_KEYS.get(k)
        if attr:
            setattr(s, attr, getattr(s, attr) + v * mult)


def _apply_flat(s, eff, mods):
    """Apply flat effect keys; queue modifier keys for phase 3."""
    for k, v in (eff or {}).items():
        if k in MODIFIER_KEYS:
            mods.append((k, v))
        elif k in FLAT_KEYS:
            setattr(s, FLAT_KEYS[k], getattr(s, FLAT_KEYS[k]) + v)
        elif k == "buildDiscount":
            for age, d in v.items():
                s.build_discount[age] = s.build_discount.get(age, 0) + d
        elif k == "wonderStagesPerAction":
            s.wonder_stages = max(s.wonder_stages, v)
        elif k == "popIncreaseFoodDiscount":
            s.pop_food_discount += v
        elif k == "freePopIncreasePerTurn":
            s.free_pop_per_turn = True
        elif k == "cannotPlayAggressionOrWar":
            s.no_aggression = True
        # everything else is action-time / trigger-time, handled elsewhere


def compute(state, p):
    """Full statistics for a player (ratings, action totals, production)."""
    db = C.db()
    s = Stats(build_discount={})
    mods = []

    gov = db.get(p.government)
    s.civil_actions = gov.get("civilActions") or 4
    s.military_actions = gov.get("militaryActions") or 2
    s.urban_limit = gov.get("urbanBuildingLimit") or 2
    _add_production(s, gov.get("production"))
    _apply_flat(s, gov.get("effects"), mods)

    # --- phase 1: technologies
    for name, t in p.techs.items():
        card = db.get(name)
        typ = card["type"]
        if typ == "special-tech":
            _apply_flat(s, card.get("effects"), mods)
        elif t.workers:
            if typ in C.URBAN_TYPES:
                _add_production(s, card.get("production"), t.workers)
            elif typ in C.UNIT_TYPES:
                s.strength += (card.get("strength") or 0) * t.workers
            elif typ == "farm":
                s.food += (card.get("production") or {}).get("food", 0) * t.workers
            elif typ == "mine":
                s.resources += (card.get("production") or {}).get(
                    "resources", 0) * t.workers

    # --- phase 2: wonders, leader, colonies
    for w in p.completed_wonders:
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

    # event-granted permanents
    s.culture += p.culture_rate_extra
    s.science += p.science_rate_extra
    s.strength += p.strength_extra
    s.happy += p.happy_extra

    # --- phase 3: modifiers
    for key, val in mods:
        _apply_modifier(s, p, key, val)

    s.strength += army_strength(state, p)
    s.happy = max(0, min(8, s.happy))
    return s


def _apply_modifier(s, p, key, val):
    db = C.db()
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
        b = best_card(p, {"lab", "library"})
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
            cult = (db.get(b).get("production") or {}).get("culture", 0)
            s.culture += cult * p.worker_count(b)
    elif key == "culturePerHappyFromTemplesTheatersWonders":
        happy = _happy_from(p, {"temple", "theater"})
        for w in p.completed_wonders:
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


def _happy_from(p, types):
    db = C.db()
    tot = 0
    for n, t in p.techs.items():
        if db.type_of(n) in types:
            tot += (db.get(n).get("production") or {}).get("happy", 0) * t.workers
    return tot


def _happy_source_count(p):
    """Number of cards/buildings providing happy faces (St. Peter's)."""
    db = C.db()
    n = 0
    for name, t in p.techs.items():
        if (db.get(name).get("production") or {}).get("happy", 0) > 0:
            n += t.workers
    for w in p.completed_wonders:
        if (db.get(w).get("effects") or {}).get("happy", 0) > 0:
            n += 1
    if p.leader and (db.get(p.leader).get("effects") or {}).get("happy", 0) > 0:
        n += 1
    if (db.get(p.government).get("production") or {}).get("happy", 0) > 0:
        n += 1
    return n


# ---------------------------------------------------------------- armies


def army_strength(state, p):
    """Tactical strength from armies formed by the current tactic (§10)."""
    db = C.db()
    if not p.tactic or p.tactic not in db.by_name:
        return 0
    card = db.get(p.tactic)
    comp = card.get("composition") or []
    if not comp:
        return 0
    tactic_lv = db.level_of(p.tactic)
    genghis = (p.leader == "Genghis Khan")

    # units as (type, level) multiset
    units = []
    for n, t in p.techs.items():
        typ = db.type_of(n)
        if typ in C.UNIT_TYPES and t.workers:
            units.extend([(typ, db.level_of(n))] * t.workers)

    need = {}
    for typ in comp:
        need[typ] = need.get(typ, 0) + 1

    def count_armies(pool):
        avail = {}
        for typ, lv in pool:
            avail[typ] = avail.get(typ, 0) + 1
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
    fresh = [u for u in units if u[1] >= tactic_lv - 1]
    total_armies = count_armies(units)
    fresh_armies = min(count_armies(fresh), total_armies)
    outdated_armies = total_armies - fresh_armies

    val = card.get("strength") or 0
    old_val = card.get("obsoleteStrength")
    if old_val is None:
        old_val = val
    total = fresh_armies * val + outdated_armies * old_val
    # an air force doubles the tactical bonus of one army (§10.5)
    air = workers_on_types(p, {"air"})
    if air and total_armies:
        total += min(air, total_armies) * (val if fresh_armies else old_val)
    return total


# ------------------------------------------------------- blue token math


def _denoms(p, typ, key):
    db = C.db()
    ds = {1}
    for n in p.techs:
        if db.type_of(n) == typ:
            v = (db.get(n).get("production") or {}).get(key, 0)
            if v > 0:
                ds.add(v)
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
    db = C.db()
    card = db.get(name)
    cost = card.get("buildCost")
    if cost is None:
        return None
    s = state_stats(state, p)
    typ = card["type"]
    if typ in C.URBAN_TYPES:
        cost -= s.build_discount.get(card["age"], 0)
        if typ == "theater" and p.leader == "William Shakespeare" and \
                any(db.type_of(n) == "library" for n in p.techs):
            cost -= 1
        if typ == "library" and p.leader == "William Shakespeare" and \
                any(db.type_of(n) == "theater" for n in p.techs):
            cost -= 1
    return max(0, cost)


def tech_cost(state, p, name):
    """Science cost to develop technology `name`."""
    db = C.db()
    card = db.get(name)
    if card["type"] == "government":
        cost = card.get("peacefulCost")
    else:
        cost = card.get("techCost")
    if cost is None:
        return None
    typ = card["type"]
    if typ == "theater":
        if p.leader == "J. S. Bach":
            cost -= 2
        if p.leader == "William Shakespeare" and \
                any(db.type_of(n) == "library" for n in p.techs):
            cost -= 1
    if typ == "library" and p.leader == "William Shakespeare" and \
            any(db.type_of(n) == "theater" for n in p.techs):
        cost -= 1
    return max(0, cost)


_STATS_CACHE_KEY = "_stats_cache"


def state_stats(state, p):
    """Cached per-mutation stats (invalidated by engine.actions.touch)."""
    cache = getattr(state, _STATS_CACHE_KEY, None)
    if cache is None:
        cache = {}
        setattr(state, _STATS_CACHE_KEY, cache)
    if p.idx not in cache:
        cache[p.idx] = compute(state, p)
    return cache[p.idx]


def invalidate(state, p=None):
    cache = getattr(state, _STATS_CACHE_KEY, None)
    if cache is None:
        return
    if p is None:
        cache.clear()
    else:
        cache.pop(p.idx, None)


# ------------------------------------------------------ enter/leave play


def on_enter_play(state, p, name):
    """Immediate one-time effects when a card enters play."""
    db = C.db()
    eff = db.get(name).get("effects") or {}
    if "blueTokens" in eff:
        p.blue_total += eff["blueTokens"]
    if "yellowTokens" in eff:
        p.yellow_bank += eff["yellowTokens"]
    invalidate(state, p)


def on_leave_play(state, p, name):
    db = C.db()
    eff = db.get(name).get("effects") or {}
    if "blueTokens" in eff:
        p.blue_total = max(0, p.blue_total - eff["blueTokens"])
    if "yellowTokens" in eff:
        p.yellow_bank = max(0, p.yellow_bank - eff["yellowTokens"])
    invalidate(state, p)


# ---------------------------------------------------------- triggers


def on_take_card(state, p, name):
    """Aristotle: 1 science per technology card taken from the row."""
    db = C.db()
    if p.leader == "Aristotle" and _is_technology(db.get(name)):
        p.science += 1


def _is_technology(card):
    return card["type"] in (C.WORKER_TYPES | {"special-tech", "government"})


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


def on_wonder_complete(state, p, name):
    """Age III wonders score a one-time culture bonus (§9.2)."""
    db = C.db()
    card = db.get(name)
    eff = card.get("effects") or {}
    gained = 0
    if eff.get("onBuildCulturePerTechLevelSum"):
        gained = sum(db.level_of(n) for n in p.techs
                     if _is_technology(db.get(n)))
        gained += db.level_of(p.government)
    elif "onBuildCulture" in eff:
        gained = _one_time_culture(state, p, name)
    p.culture += gained
    return gained


def _one_time_culture(state, p, name):
    db = C.db()
    if name == "Fast Food Chains":
        return (2 * workers_on_types(p, C.PRODUCTION_TYPES)
                + workers_on_types(p, C.URBAN_TYPES | C.UNIT_TYPES))
    if name == "Hollywood":
        tot = 0
        for n, t in p.techs.items():
            if db.type_of(n) in ("theater", "library"):
                tot += (db.get(n).get("production") or {}).get(
                    "culture", 0) * t.workers
        return 2 * tot
    if name == "Internet":
        tot = 0
        for n, t in p.techs.items():
            if db.type_of(n) in C.URBAN_TYPES:
                prod = db.get(n).get("production") or {}
                per = (prod.get("culture", 0) + prod.get("science", 0)
                       + prod.get("strength", 0))
                tot += per * t.workers
                if p.leader == "Sid Meier" and db.type_of(n) == "lab":
                    tot += (db.level_of(n) - 1) * t.workers
        return tot
    return 0


def end_of_game_bonus(state, p):
    """Bill Gates and friends (§12.5.3)."""
    db = C.db()
    bonus = 0
    if p.leader == "Bill Gates":
        for n, t in p.techs.items():
            if db.type_of(n) == "lab":
                bonus += db.level_of(n) * t.workers
    return bonus
