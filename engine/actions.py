"""Legal move generation and application.

Moves are small JSON-serializable tuples, e.g. ("take", 3),
("build", "Bronze"), ("upgrade", "Bronze", "Iron"), ("wonder_step", 2).

`legal_moves(state)` is the single source of truth: `apply()` asserts the
move it is given is legal (set STRICT = False to skip the check for speed).
"""
from __future__ import annotations

from . import cards as C
from . import economy
from . import effects
from .state import TechCard, WonderInProgress

STRICT = True

ROW_SIZE = 13


def row_cost(idx):
    """Civil actions to take the card in row slot idx (0-based) (§2.3)."""
    if idx < 5:
        return 1
    if idx < 9:
        return 2
    return 3


# ------------------------------------------------------------- helpers

def ca_total(state, p):
    s = effects.state_stats(state, p)
    return s.civil_actions


def civil_hand_limit(state, p):
    s = effects.state_stats(state, p)
    return s.civil_actions + s.civil_hand_limit


def spare_ca(state, p):
    """Civil actions available, counting Hammurabi's MA-as-CA conversion."""
    extra = 1 if (p.leader == "Hammurabi" and not p.hammurabi_used
                  and p.military_actions > 0) else 0
    return p.civil_actions + extra


def pay_ca(state, p, n):
    use = min(p.civil_actions, n)
    p.civil_actions -= use
    n -= use
    if n > 0 and p.leader == "Hammurabi" and not p.hammurabi_used \
            and p.military_actions > 0:
        p.military_actions -= 1
        p.hammurabi_used = True
        n -= 1
    assert n == 0, "paid more civil actions than available"


def take_cost(state, p, idx):
    db = C.db()
    name = state.card_row[idx]
    cost = row_cost(idx)
    card = db.get(name)
    if card["type"] == "wonder":
        if p.leader != "Michelangelo":
            cost += len(p.completed_wonders) + p.destroyed_wonders
    elif card["type"] == "leader" and p.leader == "Hammurabi":
        cost -= 1
    return max(0, cost)


def special_icon(card):
    """Blue special technology category (max one per icon, §7.6)."""
    eff = card.get("effects") or {}
    if "buildDiscount" in eff:
        return "construction"
    if "colonizeBonus" in eff:
        return "exploration"
    if "militaryActions" in eff:
        return "warfare"
    if "civilActions" in eff:
        return "law"
    return "other"


def urban_count(p, urban_type):
    db = C.db()
    return sum(t.workers for n, t in p.techs.items()
               if db.type_of(n) == urban_type)


def can_take(state, p, idx, budget=None):
    """§2.5 taking limits. `budget` overrides the civil-action check."""
    db = C.db()
    name = state.card_row[idx]
    if name is None:
        return False
    card = db.get(name)
    typ = card["type"]
    have = spare_ca(state, p) if budget is None else budget
    if take_cost(state, p, idx) > have:
        return False
    if typ == "wonder":
        return p.wonder is None
    # hand limit (§2.5) applies to everything that goes to hand
    if len(p.hand_civil) >= civil_hand_limit(state, p):
        return False
    if typ == "leader":
        return card["age"] not in p.taken_leader_ages
    if name in p.hand_civil or name in p.techs or name == p.government:
        return False
    return True


def build_cost_for(state, p, name):
    return effects.build_cost(state, p, name)


def upgrade_cost(state, p, lo, hi):
    a = build_cost_for(state, p, lo) or 0
    b = build_cost_for(state, p, hi) or 0
    return max(0, b - a)


def wonder_stage_cost(state, p, k):
    db = C.db()
    stages = db.get(p.wonder.name)["stages"]
    done = p.wonder.steps_built
    return sum(stages[done:done + k])


def build_cost_net(state, p, name):
    """Build cost after the per-turn military discount pool (§3.11)."""
    cost = build_cost_for(state, p, name)
    if cost is None:
        return None
    if is_unit(name):
        cost = max(0, cost - p.mil_discount)
    return cost


def upgrade_cost_net(state, p, lo, hi):
    cost = upgrade_cost(state, p, lo, hi)
    if is_unit(lo):
        cost = max(0, cost - p.mil_discount)
    return cost


def _spend_mil_discount(p, name, raw):
    """Consume as much of the discount pool as this build/upgrade uses."""
    if not is_unit(name) or p.mil_discount <= 0:
        return raw
    used = min(p.mil_discount, raw)
    p.mil_discount -= used
    return raw - used


def is_unit(name):
    return C.db().type_of(name) in C.UNIT_TYPES


# ------------------------------------------------------- move generation

def legal_moves(state):
    if state.game_over:
        return []
    if state.pending:
        from . import interact
        return interact.pending_moves(state)
    p = state.me()
    if state.phase == "politics":
        return _politics_moves(state, p)
    return _action_moves(state, p)


def _politics_moves(state, p):
    db = C.db()
    moves = [("pol_pass",)]
    if not state.has_military:
        return moves
    s = effects.state_stats(state, p)
    # §5.11 resign: not in age IV, and never the last player standing
    if state.age_civil != "IV":
        moves.append(("resign",))
    for pact in effects.pacts_for(state, p.idx):        # §5.10
        moves.append(("cancel_pact", pact["owner"]))
    for name in sorted(set(p.hand_military)):
        card = db.get(name)
        typ = card["type"]
        cost = (card.get("cost") or {}).get("militaryActions", 0)
        if typ in ("event", "territory"):
            moves.append(("prepare_event", name))
        elif typ == "pact":
            if len(state.active_players()) < 3:          # §13: no pacts in 2p
                continue
            sides = card.get("sides") or []
            for q in state.players:
                if q.idx == p.idx or q.resigned:
                    continue
                if sides:
                    moves.append(("offer_pact", name, q.idx, "A"))
                    moves.append(("offer_pact", name, q.idx, "B"))
                else:
                    moves.append(("offer_pact", name, q.idx, ""))
        elif typ == "aggression" and cost <= p.military_actions:
            if s.no_aggression:
                continue
            for q in state.players:
                if q.idx == p.idx or q.resigned:
                    continue
                mult = 2 if q.leader == "Mahatma Gandhi" else 1
                if cost * mult > p.military_actions:
                    continue
                if effects.pact_forbids_attack(state, p, q):     # §5.4.2
                    continue
                bonus = effects.pact_attack_bonus(state, p, q)
                if effects.state_stats(state, q).strength >= s.strength + bonus:
                    continue
                moves.append(("aggression", name, q.idx))
        elif typ == "war" and cost <= p.military_actions and not state.last_round:
            if s.no_aggression or p.war_declared_by_me:
                continue
            for q in state.players:
                if q.idx == p.idx or q.resigned:
                    continue
                mult = 2 if q.leader == "Mahatma Gandhi" else 1
                if cost * mult > p.military_actions:
                    continue
                if effects.war_forbidden(state, p, q):           # §5.6
                    continue
                moves.append(("war", name, q.idx))
    return moves


def _action_moves(state, p):
    db = C.db()
    moves = [("end_turn",)]
    ca = spare_ca(state, p)

    # take a card from the row
    for idx, name in enumerate(state.card_row):
        if name is not None and can_take(state, p, idx):
            moves.append(("take", idx))

    if state.round == 1:
        return moves          # §1.9: taking cards is the only legal action

    s = effects.state_stats(state, p)

    # increase population
    cost = economy.pop_cost(state, p)
    if cost is not None and ca >= 1 and p.food >= cost:
        moves.append(("pop",))
    if s.free_pop_per_turn and not p.ocean_liners_used and p.yellow_bank > 0:
        moves.append(("pop_free",))

    # build / upgrade / destroy
    if p.workers_free > 0:
        for name in sorted(p.techs):
            typ = db.type_of(name)
            if typ not in C.WORKER_TYPES:
                continue
            cost = build_cost_net(state, p, name)
            if cost is None or p.resources < cost:
                continue
            if typ in C.UNIT_TYPES:
                if p.military_actions < 1:
                    continue
            else:
                if ca < 1:
                    continue
                if typ in C.URBAN_TYPES and urban_count(p, typ) >= s.urban_limit:
                    continue
            moves.append(("build", name))

    for lo in sorted(p.techs):
        if p.techs[lo].workers <= 0:
            continue
        typ = db.type_of(lo)
        if typ not in C.WORKER_TYPES:
            continue
        if typ in C.UNIT_TYPES:
            if p.military_actions < 1:
                continue
        elif ca < 1:
            continue
        for hi in sorted(p.techs):
            if hi == lo or db.type_of(hi) != typ:
                continue
            if db.level_of(hi) <= db.level_of(lo):
                continue
            if p.resources >= upgrade_cost_net(state, p, lo, hi):
                moves.append(("upgrade", lo, hi))

    # destroy / disband (§3.6, §4.3)
    for name in sorted(p.techs):
        if p.techs[name].workers <= 0:
            continue
        if is_unit(name):
            if p.military_actions >= 1:
                moves.append(("destroy", name))
        elif ca >= 1 and db.type_of(name) in C.WORKER_TYPES:
            moves.append(("destroy", name))

    # wonder stages
    if p.wonder is not None:
        stages = db.get(p.wonder.name)["stages"]
        left = len(stages) - p.wonder.steps_built
        if ca >= 1:
            for k in range(1, min(left, s.wonder_stages) + 1):
                if p.resources >= wonder_stage_cost(state, p, k):
                    moves.append(("wonder_step", k))

    # hand: leaders, technologies, governments, action cards
    for name in sorted(set(p.hand_civil)):
        card = db.get(name)
        typ = card["type"]
        if typ == "leader":
            if ca >= 1:
                moves.append(("play_leader", name))
        elif typ == "government":
            if ca >= 1 and p.science >= (effects.tech_cost(state, p, name) or 0):
                moves.append(("develop", name))
            if _can_revolt(state, p, name):
                moves.append(("revolution", name))
        elif typ == "action":
            if ca >= 1 and name not in p.taken_this_turn \
                    and _action_card_playable(state, p, name):
                moves.append(("play_action", name))
        elif typ in C.WORKER_TYPES or typ == "special-tech":
            if ca >= 1 and p.science >= (effects.tech_cost(state, p, name) or 0):
                moves.append(("develop", name))

    # tactics
    if state.has_military and not p.tactic_action_used:
        if p.military_actions >= 1:
            for name in sorted(set(p.hand_military)):
                if db.type_of(name) == "tactic":
                    moves.append(("play_tactic", name))
        if p.military_actions >= 2:
            for name in sorted(set(state.available_tactics)):
                if name != p.tactic:
                    moves.append(("copy_tactic", name))

    # Churchill's once-per-turn choice
    if p.leader == "Winston Churchill" and not p.churchill_used:
        moves.append(("churchill", "culture"))
        moves.append(("churchill", "military"))

    return moves


def _can_revolt(state, p, name):
    db = C.db()
    card = db.get(name)
    cost = card.get("revolutionCost")
    if cost is None or p.science < cost:
        return False
    if p.leader == "Maximilien Robespierre":
        s = effects.state_stats(state, p)
        return p.military_actions == s.military_actions and s.military_actions > 0
    return p.civil_actions == ca_total(state, p) and p.civil_actions > 0


def _action_card_playable(state, p, name):
    """§3.11: a yellow card that orders an action needs that action to be legal.

    Note the ordered action is checked AFTER the card's own gains, because
    the gains are what make it affordable (Breakthrough's +science pays for
    the technology it develops, Frugality's +food for the population).
    """
    eff = C.db().get(name).get("effects") or {}
    if not eff:
        return False
    kind = eff.get("freeCivilAction")
    if not kind:
        return any(k in ACTION_CARD_KEYS for k in eff)
    probe = _with_card_gains(state, p, eff)
    return bool(free_action_moves(state, probe, kind,
                                  eff.get("resourceDiscount", 0)))


ACTION_CARD_KEYS = {
    "gainScience", "gainCulture", "gainFood", "gainResources",
    "gainPopulation", "extraCivilActions", "extraMilitaryActions",
    "militaryActions", "gainFoodOrResources", "resourcesForMilitaryUnits",
    "resourcesForMilitaryUnitsPerStrongerCivilization",
    "culturePerCivilizationWithMoreCulture",
}


def _with_card_gains(state, p, eff):
    """A throwaway clone of `p` with the card's immediate gains applied.

    Only the scalar pools that gate the ordered action are moved, so this
    stays cheap enough to call from `legal_moves`.
    """
    import copy as _copy
    probe = _copy.copy(p)
    probe.techs = p.techs                   # read-only in the probe
    probe.food = p.food + eff.get("gainFood", 0)
    probe.resources = p.resources + eff.get("gainResources", 0)
    probe.science = p.science + eff.get("gainScience", 0)
    n = eff.get("gainFoodOrResources", 0)
    probe.food += n                         # best case for either choice
    probe.resources += n
    return probe


# ------------------------------------------------- ordered (free) actions

def free_action_moves(state, p, kind, discount=0, revolt_ok=False):
    """Concrete moves satisfying an action card's ordered action (§3.11).

    The action is performed under normal rules but pays no civil/military
    action, and `discount` resources come off its cost (floor 0).
    """
    db = C.db()
    out = []
    if kind == "increase_population":
        cost = economy.pop_cost(state, p)          # at full price
        if cost is not None and p.food >= cost:
            out.append(("pop",))
        return out
    if kind == "build_one_wonder_stage":
        if p.wonder is not None:
            stages = db.get(p.wonder.name)["stages"]
            if p.wonder.steps_built < len(stages):
                if p.resources >= max(0, wonder_stage_cost(state, p, 1) - discount):
                    out.append(("wonder_step", 1))
        return out
    if kind == "develop_technology":
        for name in sorted(set(p.hand_civil)):
            card = db.get(name)
            if card["type"] not in (C.WORKER_TYPES | {"special-tech",
                                                      "government"}):
                continue
            if p.science >= (effects.tech_cost(state, p, name) or 0):
                out.append(("develop", name))
            # RB p.15: Breakthrough may also pay for a revolution
            if revolt_ok and card["type"] == "government" \
                    and (card.get("revolutionCost") is not None) \
                    and p.science >= card["revolutionCost"]:
                out.append(("revolution", name))
        return out

    types = _FREE_BUILD_TYPES.get(kind)
    if types is None:
        return out
    upgrade_only = kind.startswith("upgrade_")
    s = effects.state_stats(state, p)
    if not upgrade_only and p.workers_free > 0:
        for name in sorted(p.techs):
            typ = db.type_of(name)
            if typ not in types:
                continue
            cost = build_cost_for(state, p, name)
            if cost is None or p.resources < max(0, cost - discount):
                continue
            if typ in C.URBAN_TYPES and urban_count(p, typ) >= s.urban_limit:
                continue
            out.append(("build", name))
    for lo in sorted(p.techs):
        if p.techs[lo].workers <= 0 or db.type_of(lo) not in types:
            continue
        typ = db.type_of(lo)
        for hi in sorted(p.techs):
            if hi == lo or db.type_of(hi) != typ:
                continue
            if db.level_of(hi) <= db.level_of(lo):
                continue
            if p.resources >= max(0, upgrade_cost(state, p, lo, hi) - discount):
                out.append(("upgrade", lo, hi))
    return out


_FREE_BUILD_TYPES = {
    "build_or_upgrade_farm_or_mine": C.PRODUCTION_TYPES,
    "build_or_upgrade_urban_building": C.URBAN_TYPES,
    "upgrade_farm_mine_or_urban_building": C.PRODUCTION_TYPES | C.URBAN_TYPES,
}


def apply_free_action(state, p, move, discount=0):
    """Perform an action-card's ordered action: no action cost, discounted."""
    kind = move[0]
    if kind == "pop":
        economy.increase_population(state, p)
    elif kind == "build":
        do_build(state, p, move[1], discount=discount, free=True)
    elif kind == "upgrade":
        do_upgrade(state, p, move[1], move[2], discount=discount, free=True)
    elif kind == "wonder_step":
        do_wonder_step(state, p, 1, discount=discount, free=True)
    elif kind == "develop":
        _h_develop(state, p, ("develop", move[1]), None, free=True)
    elif kind == "revolution":
        _h_revolution(state, p, ("revolution", move[1]), None)
    effects.invalidate(state, p)


# ------------------------------------------------------------- apply

def apply(state, move, rng=None):
    from . import interact
    if STRICT:
        legal = legal_moves(state)
        assert move in legal or list(move) in [list(m) for m in legal], (
            f"illegal move {move!r} in phase {state.phase}")
    if state.pending:
        interact.apply_pending(state, move, rng)
        return state
    p = state.me()
    kind = move[0]
    handler = _HANDLERS.get(kind)
    if handler is None:
        raise ValueError(f"unknown move {move!r}")
    handler(state, p, move, rng)
    effects.invalidate(state, p)
    interact.run_queue(state, rng)
    return state


# --- action phase handlers

def _h_take(state, p, move, rng):
    idx = move[1]
    pay_ca(state, p, take_cost(state, p, idx))
    take_card(state, p, idx)


def take_card(state, p, idx):
    """Move row card `idx` into `p`'s hand/play area (actions already paid)."""
    db = C.db()
    name = state.card_row[idx]
    state.card_row[idx] = None
    card = db.get(name)
    effects.on_take_card(state, p, name)
    if card["type"] == "wonder":
        p.wonder = WonderInProgress(name)
    else:
        p.hand_civil.append(name)
        if card["type"] == "leader":
            p.taken_leader_ages.append(card["age"])
        elif card["type"] == "action":
            p.taken_this_turn.append(name)
    state.emit(f"took {name}")


def _h_pop(state, p, move, rng):
    pay_ca(state, p, 1)
    ok = economy.increase_population(state, p)
    assert ok


def _h_pop_free(state, p, move, rng):
    economy.increase_population(state, p, free=True)
    p.ocean_liners_used = True


def _h_build(state, p, move, rng):
    do_build(state, p, move[1])


def do_build(state, p, name, discount=0, free=False):
    cost = max(0, (build_cost_for(state, p, name) or 0) - discount)
    if not free:
        cost = _spend_mil_discount(p, name, cost)
        if is_unit(name):
            p.military_actions -= 1
        else:
            pay_ca(state, p, 1)
    p.resources -= cost
    p.techs[name].workers += 1
    p.workers_free -= 1
    if is_unit(name):
        effects.on_build_unit(state, p, name)
    state.emit(f"built {name} for {cost}")


def _h_upgrade(state, p, move, rng):
    do_upgrade(state, p, move[1], move[2])


def do_upgrade(state, p, lo, hi, discount=0, free=False):
    cost = max(0, upgrade_cost(state, p, lo, hi) - discount)
    if not free:
        cost = _spend_mil_discount(p, lo, cost)
        if is_unit(lo):
            p.military_actions -= 1
        else:
            pay_ca(state, p, 1)
    if is_unit(lo):
        effects.on_build_unit(state, p, hi)
    p.resources -= cost
    p.techs[lo].workers -= 1
    p.techs[hi].workers += 1


def _h_destroy(state, p, move, rng):
    name = move[1]
    if is_unit(name):
        p.military_actions -= 1
    else:
        pay_ca(state, p, 1)
    p.techs[name].workers -= 1
    p.workers_free += 1


def _h_wonder_step(state, p, move, rng):
    do_wonder_step(state, p, move[1])


def do_wonder_step(state, p, k, discount=0, free=False):
    db = C.db()
    cost = max(0, wonder_stage_cost(state, p, k) - discount)
    if not free:
        pay_ca(state, p, 1)
    p.resources -= cost
    p.wonder.steps_built += k
    name = p.wonder.name
    if p.wonder.steps_built >= len(db.get(name)["stages"]):
        p.wonder = None
        p.completed_wonders.append(name)
        effects.on_enter_play(state, p, name)
        gained = effects.on_wonder_complete(state, p, name)
        state.emit(f"completed wonder {name} (+{gained} culture)")


def _h_play_leader(state, p, move, rng):
    name = move[1]
    pay_ca(state, p, 1)
    p.hand_civil.remove(name)
    if p.leader:
        old = p.leader
        effects.on_leave_play(state, p, old)
        if old == "Homer" and p.completed_wonders and p.homer_wonder is None:
            p.homer_wonder = p.completed_wonders[0]
        # replacing refunds one civil action (§9.1)
        p.civil_actions = min(ca_total(state, p), p.civil_actions + 1)
    p.leader = name
    effects.on_enter_play(state, p, name)
    state.emit(f"played leader {name}")


def _h_develop(state, p, move, rng, free=False):
    db = C.db()
    name = move[1]
    card = db.get(name)
    cost = effects.tech_cost(state, p, name) or 0
    if not free:
        pay_ca(state, p, 1)
    p.science -= cost
    p.hand_civil.remove(name)
    if card["type"] == "government":
        _set_government(state, p, name)
    elif card["type"] == "special-tech":
        _develop_special(state, p, name)
    else:
        p.techs[name] = TechCard(name)
        effects.on_enter_play(state, p, name)
    effects.on_develop(state, p, name)
    state.emit(f"developed {name} for {cost} science")


def _develop_special(state, p, name):
    db = C.db()
    icon = special_icon(db.get(name))
    existing = [n for n in p.techs
                if db.type_of(n) == "special-tech"
                and special_icon(db.get(n)) == icon]
    for old in existing:
        if db.level_of(old) >= db.level_of(name):
            return                      # the new (lower) card is removed
    for old in existing:
        effects.on_leave_play(state, p, old)
        del p.techs[old]
    p.techs[name] = TechCard(name)
    effects.on_enter_play(state, p, name)


def _set_government(state, p, name):
    spent_c = ca_total(state, p) - p.civil_actions
    spent_m = effects.state_stats(state, p).military_actions - p.military_actions
    p.government = name
    effects.invalidate(state, p)
    s = effects.state_stats(state, p)
    p.civil_actions = max(0, s.civil_actions - spent_c)
    p.military_actions = max(0, s.military_actions - spent_m)


def _h_revolution(state, p, move, rng):
    db = C.db()
    name = move[1]
    card = db.get(name)
    p.science -= card["revolutionCost"]
    p.hand_civil.remove(name)
    robespierre = (p.leader == "Maximilien Robespierre")
    if robespierre:
        p.military_actions = 0
    else:
        p.civil_actions = 0
    p.government = name
    effects.invalidate(state, p)
    s = effects.state_stats(state, p)
    if robespierre:
        p.civil_actions = min(p.civil_actions, s.civil_actions)
        p.culture += 3
    else:
        p.civil_actions = 0
        p.military_actions = min(p.military_actions, s.military_actions)
    if p.leader == "Isaac Newton":
        p.civil_actions = min(s.civil_actions, p.civil_actions + 1)
    state.emit(f"revolution -> {name}")


def _h_churchill(state, p, move, rng):
    p.churchill_used = True
    if move[1] == "culture":
        p.culture += 3
    else:
        p.science += 3
        effects.gain_resources(p, 3)


def _h_play_tactic(state, p, move, rng):
    name = move[1]
    p.military_actions -= 1
    p.hand_military.remove(name)
    p.tactic = name
    p.tactic_exclusive = True
    p.tactic_action_used = True


def _h_copy_tactic(state, p, move, rng):
    p.military_actions -= 2
    p.tactic = move[1]
    p.tactic_exclusive = False
    p.tactic_action_used = True


def _h_play_action(state, p, move, rng):
    """§3.11 play a yellow action card: 1 CA, resolve, discard (leaves game).

    Gains resolve first, then the ordered action (which pays no action and
    takes the card's resource discount).
    """
    from . import game, interact
    name = move[1]
    revolt_ok = (p.civil_actions == ca_total(state, p))
    pay_ca(state, p, 1)
    p.hand_civil.remove(name)
    eff = C.db().get(name).get("effects") or {}
    if "gainScience" in eff:
        p.science += eff["gainScience"]
    if "gainCulture" in eff:
        p.culture += eff["gainCulture"]
    if "gainFood" in eff:
        effects.gain_food(p, eff["gainFood"])
    if "gainResources" in eff:
        effects.gain_resources(p, eff["gainResources"])
    if "gainPopulation" in eff:
        for _ in range(eff["gainPopulation"]):
            if p.yellow_bank > 0:
                p.yellow_bank -= 1
                p.workers_free += 1
    if "extraCivilActions" in eff:
        p.civil_actions += eff["extraCivilActions"]
    for key in ("extraMilitaryActions", "militaryActions"):
        if key in eff:                      # virtual MAs, not carried over
            p.military_actions += eff[key]
    if "culturePerCivilizationWithMoreCulture" in eff:
        per = _per_player(state, eff["culturePerCivilizationWithMoreCulture"])
        n = sum(1 for q in state.active_players()
                if q.idx != p.idx and q.culture > p.culture)
        p.culture += per * n
    if "resourcesForMilitaryUnits" in eff:
        p.mil_discount += eff["resourcesForMilitaryUnits"]
    if "resourcesForMilitaryUnitsPerStrongerCivilization" in eff:
        per = _per_player(
            state, eff["resourcesForMilitaryUnitsPerStrongerCivilization"])
        mine = effects.state_stats(state, p).strength
        n = sum(1 for q in state.active_players()
                if q.idx != p.idx
                and effects.state_stats(state, q).strength > mine)
        p.mil_discount += per * n
    effects.invalidate(state, p)
    if "gainFoodOrResources" in eff:
        n = eff["gainFoodOrResources"]
        interact.push_choice(state, p.idx, "food_or_res", ["food", "resources"],
                             {"n": n}, auto=False)
    if eff.get("freeCivilAction"):
        interact.enqueue(state, {"player": p.idx, "tag": "free_civil",
                                 "kind": eff["freeCivilAction"],
                                 "discount": eff.get("resourceDiscount", 0),
                                 "revolt_ok": revolt_ok})
    state.emit(f"played action card {name}")


def _per_player(state, value):
    """Action-card values printed per player count, e.g. {2p:6,3p:3,4p:2}."""
    from . import game
    if isinstance(value, dict):
        return value.get(f"{game.live_count(state)}p", 0)
    return value or 0


def _h_end_turn(state, p, move, rng):
    from . import game
    game.end_turn(state, rng)


# --- politics handlers

def _h_pol_pass(state, p, move, rng):
    p.politics_done = True
    state.phase = "actions"


def _h_prepare_event(state, p, move, rng):
    from . import events
    name = move[1]
    p.hand_military.remove(name)
    p.culture += C.db().level_of(name)
    state.future_events.append(name)
    state.seeded_by[name] = p.idx
    events.reveal_current_event(state, rng)
    p.politics_done = True
    state.phase = "actions"


def _h_aggression(state, p, move, rng):
    from . import events
    p.politics_done = True
    state.phase = "actions"
    events.start_aggression(state, p, move[1], state.players[move[2]], rng)


def _h_offer_pact(state, p, move, rng):
    """§5.9: reveal the pact, name the partner and the sides."""
    from . import interact
    name, target, side = move[1], move[2], move[3]
    p.hand_military.remove(name)
    ctx = {"owner": p.idx, "name": name}
    if side == "B":
        ctx["a"], ctx["b"] = target, p.idx
    else:
        ctx["a"], ctx["b"] = p.idx, target
    p.politics_done = True
    state.phase = "actions"
    interact.push_choice(state, target, "pact_offer", ["accept", "refuse"],
                         ctx, auto=False)


def _h_cancel_pact(state, p, move, rng):
    """§5.10: remove any pact you are party to from play."""
    owner = state.players[move[1]]
    owner.pacts = [pact for pact in owner.pacts
                   if p.idx not in (pact["owner"], pact["partner"])]
    effects.invalidate(state)
    p.politics_done = True
    state.phase = "actions"
    state.emit(f"P{p.idx} cancelled a pact")


def _h_resign(state, p, move, rng):
    """§5.11: leave the game; wars against you score their declarer 7."""
    from . import game
    p.resigned = True
    p.hand_civil = []
    for n in p.hand_military:
        economy.discard_military(state, n)
    p.hand_military = []
    effects.drop_pacts_of(state, p.idx)
    for war in list(p.wars_declared_on_me):
        name, atk_idx, _ = tuple(war)
        atk = state.players[atk_idx]
        atk.culture += 7
        if atk.war_declared_by_me and tuple(atk.war_declared_by_me)[0] == name:
            atk.war_declared_by_me = None
        economy.discard_military(state, name)
    p.wars_declared_on_me = []
    if p.war_declared_by_me:
        name, _, tgt = tuple(p.war_declared_by_me)
        d = state.players[tgt]
        d.wars_declared_on_me = [w for w in d.wars_declared_on_me
                                 if tuple(w)[0] != name]
        economy.discard_military(state, name)
        p.war_declared_by_me = None
    p.politics_done = True
    effects.invalidate(state)
    state.emit(f"P{p.idx} resigned")
    game.after_resign(state, rng)


def _h_war(state, p, move, rng):
    db = C.db()
    name, target = move[1], move[2]
    cost = (db.get(name).get("cost") or {}).get("militaryActions", 0)
    if state.players[target].leader == "Mahatma Gandhi":
        cost *= 2
    p.military_actions -= cost
    p.hand_military.remove(name)
    p.war_declared_by_me = (name, p.idx, target)
    state.players[target].wars_declared_on_me.append((name, p.idx, target))
    p.politics_done = True
    state.phase = "actions"


_HANDLERS = {
    "take": _h_take,
    "pop": _h_pop,
    "pop_free": _h_pop_free,
    "build": _h_build,
    "upgrade": _h_upgrade,
    "destroy": _h_destroy,
    "wonder_step": _h_wonder_step,
    "play_leader": _h_play_leader,
    "develop": _h_develop,
    "revolution": _h_revolution,
    "churchill": _h_churchill,
    "play_tactic": _h_play_tactic,
    "copy_tactic": _h_copy_tactic,
    "play_action": _h_play_action,
    "end_turn": _h_end_turn,
    "pol_pass": _h_pol_pass,
    "prepare_event": _h_prepare_event,
    "aggression": _h_aggression,
    "war": _h_war,
    "offer_pact": _h_offer_pact,
    "cancel_pact": _h_cancel_pact,
    "resign": _h_resign,
}
