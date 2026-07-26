"""Events, aggressions and wars (§5, §11, §12.5).

Card effects are read from the tag vocabulary used by
``data/cards_military_actions.json``.  Anything the engine does not
understand (free text values, unimplemented sub-decisions) is ignored, so
partial or prose-y data can never crash a game.
"""
from __future__ import annotations

from . import cards as C

# Module-level bindings for the singleton card DB: `C.db()` was ~734k calls
# per 60 4p games.  cards.py has no engine imports, so this is safe at import.
_DB = C.db()
_TYPE_BY_NAME = _DB.type_by_name
_BY_NAME = _DB.by_name
_LEVEL_BY_NAME = _DB.level_by_name
from . import economy
from . import effects


def _num(v):
    """Numeric effect values only; the data also stores prose in places."""
    return v if isinstance(v, (int, float)) and not isinstance(v, bool) else None


def _pkey(state):
    """Player-count key for card tables, after any resignations (§13)."""
    from . import game
    return f"{game.live_count(state)}p"


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
        elif key in ("decreasePopulation", "losePopulation"):
            # §6.5/FAQ p.15: the owner chooses which worker to lose
            if v:
                from . import interact
                interact.enqueue(state, {"player": p.idx, "tag": "lose_pop",
                                         "n": int(v)})
        elif key == "yellowTokens":
            if v:
                effects.grant_yellow(p, sign * int(v))
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
    from . import interact
    if not state.current_events:
        _recycle_future_events(state, rng)
        if not state.current_events:
            return None
    name = state.current_events.pop()
    db = _DB
    if name in db.by_name and db.type_of(name) == "territory":
        # §11.1: a territory starts a colonization auction instead
        interact.start_auction(state, name, state.current, rng)
    else:
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
    deck.sort(key=lambda n: -_DB.level_of(n))
    state.current_events = deck


def resolve_event(state, name, rng, revealer_idx):
    """Resolve one revealed event card (§5.3)."""
    db = _DB
    if name not in db.by_name:
        return
    card = db.get(name)
    if card["type"] == "territory":
        from . import interact
        interact.start_auction(state, name, revealer_idx, rng)
        return
    eff = card.get("effects") or {}
    order = _order_from(state, revealer_idx)
    if not order:
        return
    if state.last_round and isinstance(eff.get("lastRoundSubstitute"), dict):
        eff = eff["lastRoundSubstitute"]      # Politics of Strength

    if "allPlayers" in eff:
        for q in order:
            _apply_player_block(state, q, eff["allPlayers"], order, rng)

    if "target" in eff and "decreasePopulation" in eff:
        _conditional_target(state, eff, order, rng)

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
            count = (eff[key] or {}).get(_pkey(state), 1)
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
        table = block["rankingCulture"].get(_pkey(state)) or []
        stat = block.get("statistic", "strengthRating")
        rank = _rank(state, order, _STAT_ALIASES.get(stat, "strength"), True)
        if p in rank:
            i = rank.index(p)
            if i < len(table):
                p.culture = max(0, p.culture + table[i])
    _apply_extras(state, p, block, rng)
    _queue_decisions(state, p, block)


def _conditional_target(state, eff, order, rng):
    """Barbarians: 'the player with most culture, if among the weakest'."""
    from . import interact
    ranked = _rank(state, order, "culture", True)
    if not ranked:
        return
    q = ranked[0]
    among = ((eff.get("condition") or {}).get("amongWeakest")
             or {}).get(_pkey(state))
    if among and q not in _rank(state, order, "strength", False)[:among]:
        return
    interact.enqueue(state, {"player": q.idx, "tag": "lose_pop",
                             "n": int(eff.get("decreasePopulation", 1))})


def _apply_extras(state, p, block, rng):
    """Event effects with no decision that apply_gains does not cover."""
    db = _DB
    s = effects.state_stats(state, p)
    if block.get("produceFood"):
        effects.gain_food(p, s.food)
    if block.get("produceResources"):
        effects.gain_resources(p, s.resources)
    if isinstance(block.get("extraProduction"), dict):
        _extra_production(state, p, s)
    # "gain X equal to your Y production" (corruption/consumption never apply)
    if block.get("scienceEqualToScienceProduction"):
        p.science += s.science
    if block.get("cultureEqualToCultureProduction"):
        p.culture += s.culture
    if block.get("cultureEqualToScienceProduction"):
        p.culture += s.science
    if block.get("foodEqualToHappyFaces"):
        cap = _num(block.get("max"))
        n = s.happy if cap is None else min(s.happy, int(cap))
        effects.gain_food(p, max(0, n))
    if block.get("discardLeaderUnlessCurrentAge") and p.leader:
        if db.age_of(p.leader) != state.age_civil:
            effects.on_leave_play(state, p, p.leader)
            p.leader = None
    if block.get("takeYellowTokensFromWeakest"):
        n = int(block["takeYellowTokensFromWeakest"])
        order = _order_from(state, state.current)
        victims = [q for q in _rank(state, order, "strength", False)
                   if q.idx != p.idx]
        if victims:
            take = min(n, victims[0].yellow_bank)
            victims[0].yellow_bank -= take
            effects.grant_yellow(p, take)
    if block.get("decreasePopulationByHalfDiscontentWorkersRoundedUp"):
        from . import interact
        n = (economy.discontent(state, p) + 1) // 2
        if n:
            interact.enqueue(state, {"player": p.idx, "tag": "lose_pop",
                                     "n": n})
    if "civilActionsPerDiscontentWorker" in block:
        per = _num(block["civilActionsPerDiscontentWorker"]) or 0
        loss = int(-per * economy.discontent(state, p))
        if loss > 0:
            p.civil_actions = max(0, p.civil_actions - loss)
            p.ca_penalty_next_turn += loss
    if isinstance(block.get("oneTimeDiscount"), dict):
        p.one_time_discount = {k: v for k, v in block["oneTimeDiscount"].items()
                               if isinstance(v, dict)}
    if block.get("destroyOneUrbanBuildingOfEachOpponent"):
        from . import interact
        for q in state.players:
            if q.idx != p.idx and not q.resigned:
                interact.enqueue(state, {"player": p.idx, "tag": "raid",
                                         "victim": q.idx, "max_age": "IV",
                                         "no_loot": True})
    if block.get("optionalTakeCardsWithCivilActions"):
        from . import interact
        p.skip_next_politics = True
        interact.enqueue(state, {
            "player": p.idx, "tag": "take_row",
            "budget": int(block["optionalTakeCardsWithCivilActions"])})
    effects.invalidate(state, p)


def _extra_production(state, p, s):
    """Economic Progress: corruption, food, consumption, resources (2015)."""
    corr = economy.corruption(effects.blue_available(p))
    paid = effects.pay_resources(p, corr)
    p.food = max(0, p.food - (corr - paid))
    effects.gain_food(p, s.food)
    need = economy.consumption(p.yellow_bank)
    if p.food >= need:
        p.food -= need
    else:
        p.culture = max(0, p.culture - 4 * (need - p.food))
        p.food = 0
    effects.gain_resources(p, s.resources)


def _queue_decisions(state, p, block):
    """Event effects that require this player to choose (§5.3)."""
    from . import interact
    if isinstance(block.get("choose"), list) and block["choose"]:
        interact.enqueue(state, {"player": p.idx, "tag": "choose",
                                 "options": block["choose"]})
    if isinstance(block.get("freeBuild"), dict):
        interact.enqueue(state, {"player": p.idx, "tag": "free_build",
                                 "spec": block["freeBuild"]})
    if block.get("destroyOwnBuilding"):
        for _ in range(int(block["destroyOwnBuilding"])):
            interact.enqueue(state, {"player": p.idx, "tag": "destroy_own"})
    if block.get("loseColony"):
        for _ in range(int(block["loseColony"])):
            interact.enqueue(state, {"player": p.idx, "tag": "lose_colony"})
    if isinstance(block.get("flipCompletedWonder"), dict):
        interact.enqueue(state, {
            "player": p.idx, "tag": "flip_wonder",
            "ages": block["flipCompletedWonder"].get("ages") or ["A", "I"]})
    if block.get("discardMilitaryCards"):
        interact.enqueue(state, {"player": p.idx, "tag": "discard_military",
                                 "n": int(block["discardMilitaryCards"])})


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
    db = _DB
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
                     C.URBAN_OR_UNIT}
            kinds |= {n for n in p.techs if db.type_of(n) == "special-tech"}
            total += int(v or 0) * len(kinds)
    return total


def evaluate_final_events(state):
    """Age III events left in the current/future decks score at game end
    (§12.5.2); the starting player counts as the current player."""
    db = _DB
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
                    _pkey(state)) or []
                stat = _STAT_ALIASES.get(block.get("statistic",
                                                   "strengthRating"),
                                         "strength")
                rank = _rank(state, order, stat, True)
                if q in rank and rank.index(q) < len(table):
                    q.culture = max(0, q.culture + table[rank.index(q)])
        state.emit(f"final scoring event {name}")


# ------------------------------------------------------------ aggressions

def start_aggression(state, attacker, name, defender, rng):
    """§5.4.1-5.4.3, then hand the defense decision to the rival."""
    from . import interact
    db = _DB
    card = db.get(name)
    cost = (card.get("cost") or {}).get("militaryActions", 0)
    if defender.leader == "Mahatma Gandhi":
        cost *= 2
    attacker.military_actions -= cost
    attacker.hand_military.remove(name)
    economy.discard_military(state, name)
    atk = effects.attack_strength(state, attacker, defender)  # §5.4.2
    effects.cancel_attack_pacts(state, attacker, defender)    # §5.4.3
    interact.start_defense(state, attacker, defender, name, atk, rng)


def finish_aggression(state, ctx, rng):
    """§5.4.5-5.4.6: compare totals and resolve the card."""
    from . import interact
    db = _DB
    attacker = state.players[ctx["attacker"]]
    defender = state.players[ctx["player"]]
    name = ctx["card"]
    if ctx["dfn"] >= ctx["atk"]:
        state.emit(f"aggression {name} vs P{defender.idx} failed "
                   f"({ctx['dfn']} >= {ctx['atk']})")
        effects.invalidate(state)
        return False

    eff = db.get(name).get("effects") or {}
    apply_gains(state, attacker, eff, rng)
    for key, raw in (eff.get("takeFromOpponent") or {}).items():
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
        interact.enqueue(state, {
            "player": defender.idx, "tag": "lose_pop",
            "n": int(eff["opponentDecreasesPopulation"])})
    for spec in eff.get("destroyUrbanBuildings") or []:
        interact.enqueue(state, {"player": attacker.idx, "tag": "raid",
                                 "victim": defender.idx,
                                 "max_age": spec.get("maxAge", "A")})
    if eff.get("stealColony"):
        interact.enqueue(state, {"player": attacker.idx, "tag": "annex",
                                 "victim": defender.idx})
    if eff.get("removeFromGame"):
        interact.enqueue(state, {
            "player": attacker.idx, "tag": "infiltrate",
            "victim": defender.idx,
            "per": _num(eff.get("gainCulturePerLevelOfRemovedCard")) or 3})
    effects.invalidate(state)
    state.emit(f"aggression {name} vs P{defender.idx} succeeded")
    return True


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
    a = (effects.state_stats(state, attacker).strength
         + effects.pact_attack_bonus(state, attacker, defender))
    d = effects.state_stats(state, defender).strength
    economy.discard_military(state, name)
    if a == d:
        return
    victor, loser, adv = ((attacker, defender, a - d) if a > d
                          else (defender, attacker, d - a))
    base = _DB.get(name).get("baseName", name) if name in _DB.by_name \
        else name
    kind = WAR_SPOILS.get(base)
    if kind == "territory":
        take = min(1 + adv // 5, loser.yellow_bank)
        loser.yellow_bank -= take
        effects.grant_yellow(victor, take)
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
