"""Legal move generation and application.

Moves are small JSON-serializable tuples, e.g. ("take", 3),
("build", "Bronze"), ("upgrade", "Bronze", "Iron"), ("wonder_step", 2).

`legal_moves(state)` is the single source of truth: `apply()` can assert the
move it is given is legal.  That assert doubles the cost of every move (it
regenerates the whole move list), so STRICT is OFF by default and ON in the
test suite; set the env var TTA_STRICT=1 to turn the legality fuzzer back on
for a self-play smoke run.
"""
from __future__ import annotations

import copy as _copy
import os
from functools import lru_cache as _lru_cache

from . import cards as C
from . import economy
from . import effects
from . import journal
from .state import TechCard, WonderInProgress

# Module-level bindings for the singleton card DB: `C.db()` was ~734k calls
# per 60 4p games.  cards.py has no engine imports, so this is safe at import.
_DB = C.db()
_TYPE_BY_NAME = _DB.type_by_name
_BY_NAME = _DB.by_name
_LEVEL_BY_NAME = _DB.level_by_name

STRICT = os.environ.get("TTA_STRICT", "") not in ("", "0", "false", "no")

ROW_SIZE = 13


ROW_COST = (1, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3)


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
    db = _DB
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
    type_of = _TYPE_BY_NAME
    return sum(t.workers for n, t in p.techs.items()
               if type_of[n] == urban_type)


def _take_gate(state, p, budget=None):
    """Loop invariants of `can_take`, computed once per move-generation pass.

    Returns (have, hand_full, wonder_surcharge, leader_discount) -- everything
    in §2.5 that does not depend on the row slot being tested.
    """
    have = spare_ca(state, p) if budget is None else budget
    surcharge = (0 if p.leader == "Michelangelo"
                 else len(p.completed_wonders) + p.destroyed_wonders)
    leader_discount = 1 if p.leader == "Hammurabi" else 0
    return (have, len(p.hand_civil) >= civil_hand_limit(state, p),
            surcharge, leader_discount)


def _can_take_gated(state, p, idx, gate, name=None):
    have, hand_full, surcharge, leader_discount = gate
    if name is None:
        name = state.card_row[idx]
        if name is None:
            return False
    typ = _TYPE_BY_NAME[name]
    cost = ROW_COST[idx] if idx < 13 else row_cost(idx)
    if typ == "wonder":
        cost += surcharge
        if cost > have:
            return False
        return p.wonder is None
    if typ == "leader":
        cost -= leader_discount
    if cost > have:                      # cost is floored at 0 == max(0, ...)
        return False
    # hand limit (§2.5) applies to everything that goes to hand
    if hand_full:
        return False
    if typ == "leader":
        return _BY_NAME[name]["age"] not in p.taken_leader_ages
    if name in p.hand_civil or name in p.techs or name == p.government:
        return False
    return True


def can_take(state, p, idx, budget=None):
    """§2.5 taking limits. `budget` overrides the civil-action check."""
    return _can_take_gated(state, p, idx, _take_gate(state, p, budget))


def build_cost_for(state, p, name):
    return effects.build_cost(state, p, name)


def upgrade_cost(state, p, lo, hi):
    a = build_cost_for(state, p, lo) or 0
    b = build_cost_for(state, p, hi) or 0
    return max(0, b - a)


def wonder_stage_cost(state, p, k):
    db = _DB
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
    return _TYPE_BY_NAME[name] in C.UNIT_TYPES


# `engine.interact` and `engine.game` import `engine.actions` back, so the
# import cycle used to be broken by a `from . import interact` INSIDE the hot
# functions -- ~60 k `_handle_fromlist` calls per 60 4p games.  A lazy
# module-global costs one global load plus an `is None` test instead.
_interact = None
_game = None


def _load_interact():
    global _interact
    from . import interact as _m
    _interact = _m
    return _m


def _load_game():
    global _game
    from . import game as _m
    _game = _m
    return _m


# ------------------------------------------------------- move generation

def legal_moves(state):
    if state.game_over:
        return []
    if state.pending:
        interact = _interact or _load_interact()
        return interact.pending_moves(state)
    p = state.me()
    if state.phase == "politics":
        return _politics_moves(state, p)
    return _action_moves(state, p)


def _politics_moves(state, p):
    db = _DB
    moves = [("pol_pass",)]
    if not state.has_military:
        return moves
    s = effects.state_stats(state, p)
    # §5.11 resign: not in age IV, and never the last player standing
    if state.age_civil != "IV":
        moves.append(("resign",))
    for pact in effects.pacts_for(state, p.idx):        # §5.10
        moves.append(("cancel_pact", pact["owner"]))
    for name in _sorted_unique(tuple(p.hand_military)):
        card = db.get(name)
        typ = card["type"]
        cost = (card.get("cost") or {}).get("militaryActions", 0)
        if typ in ("event", "territory"):
            moves.append(("prepare_event", name))
        elif typ == "pact":
            # §13 / CoL p.2: pacts are removed from the military DECKS when
            # the game is set up for two players -- a setup rule, not a live
            # one.  FAQ p.11 on resigning: "Do not remove any Pacts or 3+ or
            # 4-player cards from the current-Age decks; but do remove them
            # from any future-Age decks."  So the survivors of a resignation
            # keep drawing pacts from the current deck and may still play
            # them; only `game._advance_age` re-trims.
            if state.num_players < 3:
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
                if effects.defense_strength(state, p, q) >= \
                        effects.attack_strength(state, p, q):
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


@_lru_cache(maxsize=4096)
def _tableau(names):
    """Everything move generation derives from the SET of tableau card names.

    Keyed by `tuple(p.techs)` (dict key order, cheap to build and to hash --
    interned strings cache their hash), so it needs no invalidation: a
    different tableau is a different key.  Nothing returned here may be
    mutated by callers.

    Returns (names_sorted, worker_names, by_type, urban_names, higher):
      by_type      type -> worker names of that type, in sorted order
      urban_names  worker names whose type is urban, in sorted order
      higher       name -> the same-type names of strictly higher level,
                   in sorted order (the legal upgrade targets)
    """
    db = _DB
    type_of = db.type_by_name
    level_of = db.level_by_name
    worker_types = C.WORKER_TYPES
    urban_types = C.URBAN_TYPES
    names_sorted = sorted(names)
    worker_names = [n for n in names_sorted if type_of[n] in worker_types]
    by_type = {}
    urban_names = []
    for n in worker_names:
        typ = type_of[n]
        lst = by_type.get(typ)
        if lst is None:
            lst = by_type[typ] = []
        lst.append(n)
        if typ in urban_types:
            urban_names.append(n)
    higher = {}
    for n in worker_names:
        lv = level_of[n]
        higher[n] = tuple(h for h in by_type[type_of[n]]
                          if h != n and level_of[h] > lv)
    return names_sorted, worker_names, by_type, urban_names, higher


@_lru_cache(maxsize=2048)
def _sorted_unique(items):
    """Cached `sorted(set(seq))` for hands / the tactic pool (read-only)."""
    return tuple(sorted(set(items)))


def _action_moves(state, p):
    db = _DB
    moves = [("end_turn",)]
    ca = spare_ca(state, p)

    # take a card from the row
    gate = _take_gate(state, p)
    for idx, name in enumerate(state.card_row):
        if name is not None and _can_take_gated(state, p, idx, gate, name):
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

    # --- loop invariants for build / upgrade / destroy.  The tableau does not
    # change inside move generation, so the sorted name list, each card's type
    # and level, the per-type urban worker counts and every build cost are
    # derived exactly once.
    type_of = _TYPE_BY_NAME
    techs = p.techs
    names, worker_names, by_type, urban_names, higher = _tableau(tuple(techs))
    urban_workers = {}
    for n in urban_names:
        typ = type_of[n]
        urban_workers[typ] = urban_workers.get(typ, 0) + techs[n].workers
    costs = {}                          # name -> build cost, memoized lazily
    build_cost = effects.build_cost

    def cost_of(n):
        try:
            return costs[n]
        except KeyError:
            c = costs[n] = build_cost(state, p, n)
            return c

    have_ma = p.military_actions >= 1
    have_ca = ca >= 1
    res = p.resources
    disc = p.mil_discount

    # build / upgrade / destroy
    if p.workers_free > 0:
        for name in worker_names:
            typ = type_of[name]
            cost = cost_of(name)
            if cost is None:
                continue
            if typ in C.UNIT_TYPES:
                if res < max(0, cost - disc) or not have_ma:
                    continue
            else:
                if res < cost or not have_ca:
                    continue
                if typ in C.URBAN_TYPES and \
                        urban_workers.get(typ, 0) >= s.urban_limit:
                    continue
            moves.append(("build", name))

    for lo in worker_names:
        if techs[lo].workers <= 0:
            continue
        typ = type_of[lo]
        unit = typ in C.UNIT_TYPES
        if unit:
            if not have_ma:
                continue
        elif not have_ca:
            continue
        lo_cost = cost_of(lo) or 0
        for hi in higher[lo]:
            cost = max(0, (cost_of(hi) or 0) - lo_cost)
            if unit:
                cost = max(0, cost - disc)
            if res >= cost:
                moves.append(("upgrade", lo, hi))

    # destroy / disband (§3.6, §4.3)
    for name in names:
        if techs[name].workers <= 0:
            continue
        if db.is_unit_name[name]:
            if have_ma:
                moves.append(("destroy", name))
        elif have_ca and type_of[name] in C.WORKER_TYPES:
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
    for name in _sorted_unique(tuple(p.hand_civil)):
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
            for name in _sorted_unique(tuple(p.hand_military)):
                if db.type_of(name) == "tactic":
                    moves.append(("play_tactic", name))
        if p.military_actions >= 2:
            for name in _sorted_unique(tuple(state.available_tactics)):
                if name != p.tactic:
                    moves.append(("copy_tactic", name))

    # Churchill's once-per-turn choice
    if p.leader == "Winston Churchill" and not p.churchill_used:
        moves.append(("churchill", "culture"))
        moves.append(("churchill", "military"))

    return moves


def _can_revolt(state, p, name):
    db = _DB
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

    The ordered action is checked against the player's CURRENT pools, before
    the card's own gains — Breakthrough's "at full price ... gain 2 science"
    means the science arrives too late to pay for the technology it just
    developed, which is the whole point of that wording (OPEN_QUESTIONS 20).
    """
    eff = _DB.get(name).get("effects") or {}
    if not eff:
        return False
    kind = eff.get("freeCivilAction")
    if not kind:
        return any(k in ACTION_CARD_KEYS for k in eff)
    # RB p.15: Breakthrough may spend its order on a revolution, which is only
    # available while every civil action is still unspent (playing the card
    # itself has not been paid for yet at this point).
    revolt_ok = (p.civil_actions == ca_total(state, p))
    return bool(free_action_moves(state, p, kind,
                                  eff.get("resourceDiscount", 0), revolt_ok))


ACTION_CARD_KEYS = {
    "gainScience", "gainCulture", "gainFood", "gainResources",
    "gainPopulation", "extraCivilActions", "extraMilitaryActions",
    "militaryActions", "gainFoodOrResources", "resourcesForMilitaryUnits",
    "resourcesForMilitaryUnitsPerStrongerCivilization",
    "culturePerCivilizationWithMoreCulture",
}



# ------------------------------------------------- ordered (free) actions

def free_action_moves(state, p, kind, discount=0, revolt_ok=False):
    """Concrete moves satisfying an action card's ordered action (§3.11).

    The action is performed under normal rules but pays no civil/military
    action, and `discount` resources come off its cost (floor 0).
    """
    db = _DB
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
        for name in _sorted_unique(tuple(p.hand_civil)):
            card = db.get(name)
            if card["type"] not in C.DEVELOPABLE_TYPES:
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
    "upgrade_farm_mine_or_urban_building": C.URBAN_OR_PRODUCTION,
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
    interact = _interact or _load_interact()
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
    db = _DB
    name = state.card_row[idx]
    journal.touch(state.card_row)[idx] = None
    card = db.get(name)
    effects.on_take_card(state, p, name)
    if card["type"] == "wonder":
        p.wonder = WonderInProgress(name)
    else:
        journal.touch(p.hand_civil).append(name)
        if card["type"] == "leader":
            journal.touch(p.taken_leader_ages).append(card["age"])
        elif card["type"] == "action":
            journal.touch(p.taken_this_turn).append(name)
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
    db = _DB
    cost = max(0, wonder_stage_cost(state, p, k) - discount)
    if not free:
        pay_ca(state, p, 1)
    p.resources -= cost
    p.wonder.steps_built += k
    name = p.wonder.name
    if p.wonder.steps_built >= len(db.get(name)["stages"]):
        p.wonder = None
        journal.touch(p.completed_wonders).append(name)
        effects.on_enter_play(state, p, name)
        gained = effects.on_wonder_complete(state, p, name)
        state.emit(f"completed wonder {name} (+{gained} culture)")


def _h_play_leader(state, p, move, rng):
    name = move[1]
    pay_ca(state, p, 1)
    journal.touch(p.hand_civil).remove(name)
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
    db = _DB
    name = move[1]
    card = db.get(name)
    cost = effects.tech_cost(state, p, name) or 0
    if not free:
        pay_ca(state, p, 1)
    p.science -= cost
    journal.touch(p.hand_civil).remove(name)
    if card["type"] == "government":
        _set_government(state, p, name)
    elif card["type"] == "special-tech":
        _develop_special(state, p, name)
    else:
        journal.touch(p.techs)[name] = TechCard(name)
        effects.on_enter_play(state, p, name)
    effects.on_develop(state, p, name)
    state.emit(f"developed {name} for {cost} science")


def _develop_special(state, p, name):
    db = _DB
    icon = special_icon(db.get(name))
    existing = [n for n in p.techs
                if db.type_of(n) == "special-tech"
                and special_icon(db.get(n)) == icon]
    for old in existing:
        if db.level_of(old) >= db.level_of(name):
            return                      # the new (lower) card is removed
    for old in existing:
        effects.on_leave_play(state, p, old)
        del journal.touch(p.techs)[old]
    journal.touch(p.techs)[name] = TechCard(name)
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
    db = _DB
    name = move[1]
    card = db.get(name)
    p.science -= card["revolutionCost"]
    journal.touch(p.hand_civil).remove(name)
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
    journal.touch(p.hand_military).remove(name)
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

    The ordered action resolves FIRST and the card's gains after it, in printed
    order — Breakthrough's science and Frugality's food arrive too late to pay
    for the very action the card just ordered (OPEN_QUESTIONS 20, ruled by the
    user 2026-07-26). Cards with no ordered action gain immediately.
    """
    game = _game or _load_game()
    interact = _interact or _load_interact()
    name = move[1]
    revolt_ok = (p.civil_actions == ca_total(state, p))
    pay_ca(state, p, 1)
    journal.touch(p.hand_civil).remove(name)
    eff = _DB.get(name).get("effects") or {}
    ordered = eff.get("freeCivilAction")
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
    if ordered:
        # FIFO: the order resolves, then the gains land behind it.
        interact.enqueue(state, {"player": p.idx, "tag": "free_civil",
                                 "kind": ordered,
                                 "discount": eff.get("resourceDiscount", 0),
                                 "revolt_ok": revolt_ok})
        interact.enqueue(state, {"player": p.idx, "tag": "card_gains",
                                 "gains": {k: eff[k] for k in _GAIN_KEYS
                                           if k in eff}})
    else:
        apply_card_gains(state, p, eff)
    state.emit(f"played action card {name}")


_GAIN_KEYS = ("gainScience", "gainCulture", "gainFood", "gainResources",
              "gainPopulation", "gainFoodOrResources")


def apply_card_gains(state, p, eff):
    """The gain half of a yellow action card (§3.11)."""
    interact = _interact or _load_interact()
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
    effects.invalidate(state, p)
    if "gainFoodOrResources" in eff:
        interact.push_choice(state, p.idx, "food_or_res", ["food", "resources"],
                             {"n": eff["gainFoodOrResources"]}, auto=False)


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
    journal.touch(p.hand_military).remove(name)
    p.culture += _DB.level_of(name)
    journal.touch(state.future_events).append(name)
    journal.touch(state.seeded_by)[name] = p.idx
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
    journal.touch(p.hand_military).remove(name)
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
    """§5.6 / CoL p.4: reveal, pay, name the rival, drop the pact, place it."""
    db = _DB
    name, target = move[1], move[2]
    cost = (db.get(name).get("cost") or {}).get("militaryActions", 0)
    if state.players[target].leader == "Mahatma Gandhi":
        cost *= 2
    p.military_actions -= cost
    journal.touch(p.hand_military).remove(name)
    # CoL p.4: "If you and your rival have a pact that says it ends if you
    # attack, remove that pact from play."  FAQ p.11 spells out that this
    # covers "either by Aggression or by declaring War", and that the
    # strength such a pact granted therefore never applies to the war.
    effects.cancel_attack_pacts(state, p, state.players[target])
    p.war_declared_by_me = (name, p.idx, target)
    journal.touch(state.players[target].wars_declared_on_me).append((name, p.idx, target))
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
