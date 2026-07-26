"""Events, aggressions and wars (§5, §11, §12.5).

Card effects are read from the tag vocabulary used by
``data/cards_military_actions.json``.  Anything the engine does not
understand (free text values, unimplemented sub-decisions) is ignored, so
partial or prose-y data can never crash a game.
"""
from __future__ import annotations

from . import cards as C
from . import economy
from . import effects


def _num(v):
    """Numeric effect values only; the data also stores prose in places."""
    return v if isinstance(v, (int, float)) and not isinstance(v, bool) else None


def _order_from(state, first_idx):
    """Players in clockwise order starting at `first_idx` (§5.3 tie-break)."""
    n = state.num_players
    return [state.players[(first_idx + i) % n] for i in range(n)
            if not state.players[(first_idx + i) % n].resigned]


# ------------------------------------------------------------- gain blocks

def apply_gains(state, p, block, rng=None, sign=1):
    """Apply one effect block to one player. `sign=-1` inverts (lose blocks)."""
    for key, raw in (block or {}).items():
        v = _num(raw)
        if key in ("science", "gainScience"):
            if v:
                p.science = max(0, p.science + sign * int(v))
        elif key == "loseScience":
            if v:
                p.science = max(0, p.science - int(v))
        elif key in ("culture", "gainCulture"):
            if v:
                p.culture = max(0, p.culture + sign * int(v))
        elif key == "loseCulture":
            if v:
                p.culture = max(0, p.culture - int(v))
        elif key in ("food", "gainFood", "produceFood"):
            if v:
                if sign > 0:
                    effects.gain_food(p, int(v))
                else:
                    p.food = max(0, p.food - int(v))
        elif key in ("resources", "gainResources", "produceResources"):
            if v:
                if sign > 0:
                    effects.gain_resources(p, int(v))
                else:
                    p.resources = max(0, p.resources - int(v))
        elif key == "foodAndOrResources":
            if v:
                _food_or_resources(p, int(v), sign)
        elif key in ("population", "gainPopulation"):
            for _ in range(int(v or 0)):
                if p.yellow_bank > 0:
                    p.yellow_bank -= 1
                    p.workers_free += 1
        elif key == "increasePopulation":
            for _ in range(int(v or 0)):
                economy.increase_population(state, p)
        elif key in ("decreasePopulation", "losePopulation",
                     "opponentDecreasesPopulation"):
            for _ in range(int(v or 0)):
                economy.lose_population(state, p)
        elif key == "yellowTokens":
            if v:
                p.yellow_bank = max(0, p.yellow_bank + sign * int(v))
        elif key == "blueTokens":
            if v:
                p.blue_total = max(0, p.blue_total + sign * int(v))
        elif key == "strength":
            if v:
                p.strength_extra += sign * int(v)
        elif key in ("happiness", "happy"):
            if v:
                p.happy_extra += sign * int(v)
        elif key == "loseAllStoredFood":
            p.food = 0
        elif key == "drawMilitaryCards":
            _draw_military(state, p, int(v or 0))
    effects.invalidate(state, p)


def _food_or_resources(p, amount, sign):
    if sign > 0:
        got = effects.gain_resources(p, amount)
        effects.gain_food(p, amount - got)
    else:
        take = min(p.resources, amount)
        p.resources -= take
        p.food = max(0, p.food - (amount - take))


def _draw_military(state, p, n):
    if not state.has_military or state.age_military == "IV":
        return
    for _ in range(n):
        card = economy.draw_military(state)
        if card is None:
            return
        p.hand_military.append(card)


# ------------------------------------------------------------ event decks

def reveal_current_event(state, rng):
    """Reveal and resolve the top card of the current events deck (§5.2)."""
    if not state.current_events:
        _recycle_future_events(state, rng)
        if not state.current_events:
            return None
    name = state.current_events.pop()
    state.past_events.append(name)
    resolve_event(state, name, rng, state.current)
    if not state.current_events:
        _recycle_future_events(state, rng)
    return name


def _recycle_future_events(state, rng):
    """Future events deck becomes the new current events deck (§5.2)."""
    if not state.future_events:
        return
    deck = list(state.future_events)
    state.future_events = []
    rng.shuffle(deck)
    # pop() takes from the end, so earlier ages must sit last
    deck.sort(key=lambda n: -C.db().level_of(n))
    state.current_events = deck


def resolve_event(state, name, rng, revealer_idx):
    """Resolve one revealed event card (§5.3)."""
    db = C.db()
    if name not in db.by_name:
        return
    card = db.get(name)
    if card["type"] == "territory":
        # §11 colonization auctions are not implemented: the territory goes
        # to the past events pile without being colonized.
        state.emit(f"territory {name} revealed (auction not implemented)")
        return
    eff = card.get("effects") or {}
    order = _order_from(state, revealer_idx)
    if not order:
        return

    if "allPlayers" in eff:
        for q in order:
            _apply_player_block(state, q, eff["allPlayers"], order, rng)

    for key, stat, best in (("strongestPlayer", "strength", True),
                            ("weakestPlayer", "strength", False),
                            ("playerWithMostCulture", "culture", True),
                            ("playerWithLeastCulture", "culture", False),
                            ("playersWithMostHappyFaces", "happy", True),
                            ("playersWithMostDiscontentWorkers",
                             "discontent", True)):
        if key in eff and isinstance(eff[key], dict):
            targets = _rank(state, order, stat, best)[:1]
            for q in targets:
                _apply_player_block(state, q, eff[key], order, rng)

    for key, best in (("strongestPlayers", True), ("weakestPlayers", False)):
        if key in eff:
            count = (eff[key] or {}).get(f"{state.num_players}p", 1)
            block = eff.get("gain") if best else eff.get("lose")
            sign = 1 if best else -1
            if key == "weakestPlayers" and eff.get("gain"):
                block, sign = eff["gain"], 1
            for q in _rank(state, order, "strength", best)[:count]:
                apply_gains(state, q, block, rng, sign=sign)

    state.emit(f"event {name} resolved")


def _apply_player_block(state, p, block, order, rng):
    apply_gains(state, p, block, rng)
    culture = scoring_culture(state, p, block, order)
    if culture:
        p.culture = max(0, p.culture + culture)
    if "rankingCulture" in block:
        table = block["rankingCulture"].get(f"{state.num_players}p") or []
        stat = block.get("statistic", "strengthRating")
        rank = _rank(state, order, _STAT_ALIASES.get(stat, "strength"), True)
        if p in rank:
            i = rank.index(p)
            if i < len(table):
                p.culture = max(0, p.culture + table[i])


_STAT_ALIASES = {
    "strengthRating": "strength",
    "scienceProduction": "science",
    "cultureProduction": "culture_rate",
    "foodProduction": "food",
    "resourceProduction": "resources",
}


def _stat_value(state, p, stat):
    s = effects.state_stats(state, p)
    if stat == "strength":
        return s.strength
    if stat == "science":
        return s.science
    if stat == "culture_rate":
        return s.culture
    if stat == "food":
        return s.food
    if stat == "resources":
        return s.resources
    if stat == "happy":
        return s.happy
    if stat == "discontent":
        return economy.discontent(state, p)
    if stat == "culture":
        return p.culture
    return 0


def _rank(state, order, stat, best_first):
    """Players ranked by a statistic; ties broken by turn order (§5.3)."""
    idx = {q.idx: i for i, q in enumerate(order)}
    return sorted(order,
                  key=lambda q: (-_stat_value(state, q, stat) if best_first
                                 else _stat_value(state, q, stat), idx[q.idx]))


# ------------------------------------------------- Age III scoring events

def scoring_culture(state, p, block, order):
    """Culture awarded by the 'Impact of ...' Age III events (§12.5.2)."""
    db = C.db()
    s = effects.state_stats(state, p)
    total = 0
    for key, raw in block.items():
        v = _num(raw)
        if key == "culturePerResourceProducedByMines":
            total += int(v or 0) * s.resources
        elif key == "culturePerFoodProducedByFarms":
            total += int(v or 0) * s.food
            bonus = _num(block.get("bonusIfProductionExceedsConsumption"))
            if bonus and s.food > economy.consumption(p.yellow_bank):
                total += int(bonus)
        elif key == "culturePerLevelOfMilitaryUnitsAndArenas":
            total += int(v or 0) * sum(
                db.level_of(n) * t.workers for n, t in p.techs.items()
                if db.type_of(n) in C.UNIT_TYPES or db.type_of(n) == "arena")
        elif key == "culturePerLevelOfSpecialTechsAndGovernment":
            lv = sum(db.level_of(n) for n in p.techs
                     if db.type_of(n) == "special-tech")
            lv += db.level_of(p.government)
            total += int(v or 0) * lv
        elif key == "culturePerCompletedWonderByAge":
            for w in p.completed_wonders:
                total += int(raw.get(db.age_of(w), 0))
        elif key == "culturePerContentWorkerAbove10":
            workers = sum(t.workers for t in p.techs.values())
            content = max(0, workers - economy.discontent(state, p))
            total += int(v or 0) * max(0, content - 10)
        elif key == "culturePerColony":
            total += int(v or 0) * len(p.colonies)
        elif key == "culturePerCivilAction":
            total += int(v or 0) * s.civil_actions
        elif key == "culturePerMilitaryAction":
            total += int(v or 0) * s.military_actions
        elif key == "culturePerLevelOfUrbanBuildings":
            total += int(v or 0) * sum(
                db.level_of(n) * t.workers for n, t in p.techs.items()
                if db.type_of(n) in C.URBAN_TYPES)
        elif key == "culturePerHappyFace":
            gained = int(v or 0) * s.happy
            cap = _num(block.get("maxCultureFromHappyFaces"))
            if cap is not None:
                gained = min(gained, int(cap))
            total += gained
        elif key == "culturePerDiscontentWorker":
            total += int(v or 0) * economy.discontent(state, p)
        elif key == "culturePerAgeIIITechnology":
            n_iii = sum(1 for n in p.techs if db.age_of(n) == "III")
            if db.age_of(p.government) == "III":
                n_iii += 1
            total += int(v or 0) * n_iii
        elif key == "cultureTimesLowestProduction":
            total += int(v or 0) * min(s.food, s.resources, s.science, s.culture)
        elif key == "culturePerDistinctTypeOfUnitUrbanBuildingAndSpecialTech":
            kinds = {db.type_of(n) for n, t in p.techs.items()
                     if t.workers and db.type_of(n) in
                     (C.UNIT_TYPES | C.URBAN_TYPES)}
            kinds |= {n for n in p.techs if db.type_of(n) == "special-tech"}
            total += int(v or 0) * len(kinds)
    return total


def evaluate_final_events(state):
    """Age III events left in the current/future decks score at game end
    (§12.5.2); the starting player counts as the current player."""
    db = C.db()
    order = _order_from(state, state.start_player)
    for name in list(state.current_events) + list(state.future_events):
        if name not in db.by_name or db.age_of(name) != "III":
            continue
        eff = db.get(name).get("effects") or {}
        block = eff.get("allPlayers")
        if not block:
            continue
        for q in order:
            gained = scoring_culture(state, q, block, order)
            if gained:
                q.culture = max(0, q.culture + gained)
            if "rankingCulture" in block:
                table = block["rankingCulture"].get(
                    f"{state.num_players}p") or []
                stat = _STAT_ALIASES.get(block.get("statistic",
                                                   "strengthRating"),
                                         "strength")
                rank = _rank(state, order, stat, True)
                if q in rank and rank.index(q) < len(table):
                    q.culture = max(0, q.culture + table[rank.index(q)])
        state.emit(f"final scoring event {name}")


# ------------------------------------------------------------ aggressions

def resolve_aggression(state, attacker, name, defender, rng):
    db = C.db()
    card = db.get(name)
    cost = (card.get("cost") or {}).get("militaryActions", 0)
    if defender.leader == "Mahatma Gandhi":
        cost *= 2
    attacker.military_actions -= cost
    attacker.hand_military.remove(name)
    economy.discard_military(state, name)

    atk = effects.state_stats(state, attacker).strength
    dfn = effects.state_stats(state, defender).strength
    # §5.4.4 defender plays bonus cards / discards military cards, at most
    # their military action total in cards
    budget = effects.state_stats(state, defender).military_actions
    spent = 0
    while dfn < atk and spent < budget and defender.hand_military:
        card_name = defender.hand_military.pop()
        eff = (db.get(card_name).get("effects") or {}
               if card_name in db.by_name else {})
        dfn += _num(eff.get("defenseBonus")) or 1
        economy.discard_military(state, card_name)
        spent += 1
    if dfn >= atk:
        state.emit(f"aggression {name} vs P{defender.idx} failed")
        return False

    eff = card.get("effects") or {}
    apply_gains(state, attacker, eff, rng)
    take = (eff.get("takeFromOpponent") or {})
    for key, raw in take.items():
        v = _num(raw)
        if not v:
            continue
        if key == "foodAndOrResources":
            before_f, before_r = defender.food, defender.resources
            _food_or_resources(defender, int(v), -1)
            moved = (before_f - defender.food) + (before_r - defender.resources)
            _food_or_resources(attacker, moved, 1)
        elif key == "science":
            moved = min(int(v), defender.science)
            defender.science -= moved
            attacker.science += moved
        elif key == "culture":
            moved = min(int(v), defender.culture)
            defender.culture -= moved
            attacker.culture += moved
    if eff.get("opponentDecreasesPopulation"):
        for _ in range(int(eff["opponentDecreasesPopulation"])):
            economy.lose_population(state, defender)
    _destroy_buildings(state, defender, attacker, eff)
    effects.invalidate(state)
    state.emit(f"aggression {name} vs P{defender.idx} succeeded")
    return True


def _destroy_buildings(state, victim, attacker, eff):
    """Raid: destroy urban buildings up to the listed ages (§5.5)."""
    db = C.db()
    specs = eff.get("destroyUrbanBuildings") or []
    for spec in specs:
        max_lv = C.level(spec.get("maxAge", "A"))
        cands = [n for n, t in victim.techs.items()
                 if t.workers and db.type_of(n) in C.URBAN_TYPES
                 and db.level_of(n) <= max_lv]
        if not cands:
            continue
        # destroy the most valuable one
        target = max(cands, key=lambda n: db.get(n).get("buildCost") or 0)
        victim.techs[target].workers -= 1
        victim.workers_free += 1
        printed = db.get(target).get("buildCost") or 0
        effects.gain_resources(attacker, (printed + 1) // 2)
    if specs:
        effects.invalidate(state, victim)


# ------------------------------------------------------------------ wars

WAR_SPOILS = {
    "War over Territory": "territory",
    "War over Technology": "technology",
    "War over Culture": "culture",
}


def resolve_war(state, attacker, rng):
    """Resolve the war declared by `attacker` last turn (§5.7)."""
    war = attacker.war_declared_by_me
    if not war:
        return
    name, _, target = war
    defender = state.players[target]
    attacker.war_declared_by_me = None
    defender.wars_declared_on_me = [
        w for w in defender.wars_declared_on_me if tuple(w) != tuple(war)]
    a = effects.state_stats(state, attacker).strength
    d = effects.state_stats(state, defender).strength
    economy.discard_military(state, name)
    if a == d:
        return
    victor, loser, adv = ((attacker, defender, a - d) if a > d
                          else (defender, attacker, d - a))
    base = C.db().get(name).get("baseName", name) if name in C.db().by_name \
        else name
    kind = WAR_SPOILS.get(base)
    if kind == "territory":
        take = min(1 + adv // 5, loser.yellow_bank)
        loser.yellow_bank -= take
        victor.yellow_bank += take
    elif kind == "technology":
        take = min(adv, loser.science)
        loser.science -= take
        victor.science += take
    elif kind == "culture":
        take = min(5 + adv, loser.culture)
        loser.culture -= take
        victor.culture += take
    effects.invalidate(state)
    state.emit(f"war {name}: P{victor.idx} beat P{loser.idx} by {adv}")
