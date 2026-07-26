"""Population/happiness/corruption tables and the End-of-Turn Sequence.

All numbers follow docs/RULES_SPEC.md §6 exactly.
"""
from __future__ import annotations

from . import cards as C
from . import effects


# --------------------------------------------------------------- tables

def pop_cost_base(yellow_bank):
    """Food to increase population (§6.1). None when the bank is empty."""
    if yellow_bank <= 0:
        return None
    if yellow_bank >= 17:
        return 2
    if yellow_bank >= 13:
        return 3
    if yellow_bank >= 9:
        return 4
    if yellow_bank >= 5:
        return 5
    return 7


def consumption(yellow_bank):
    """Food eaten in the production phase (§6.1)."""
    if yellow_bank >= 17:
        return 0
    if yellow_bank >= 13:
        return 1
    if yellow_bank >= 9:
        return 2
    if yellow_bank >= 5:
        return 3
    if yellow_bank >= 1:
        return 4
    return 6


def happy_required(yellow_bank):
    """Happy faces needed to keep everyone content (§6.1/§6.3)."""
    if yellow_bank >= 17:
        return 0
    if yellow_bank >= 13:
        return 1
    if yellow_bank >= 11:
        return 2
    if yellow_bank >= 9:
        return 3
    if yellow_bank >= 7:
        return 4
    if yellow_bank >= 5:
        return 5
    if yellow_bank >= 3:
        return 6
    if yellow_bank >= 1:
        return 7
    return 8


def corruption(blue_available):
    """Resources lost each production phase (§6.2)."""
    if blue_available >= 11:
        return 0
    if blue_available >= 6:
        return 2
    if blue_available >= 1:
        return 4
    return 6


# ------------------------------------------------------- derived checks

def pop_cost(state, p):
    base = pop_cost_base(p.yellow_bank)
    if base is None:
        return None
    s = effects.state_stats(state, p)
    base -= (p.one_time_discount.get("increasePopulation") or {}).get("food", 0)
    return max(0, base - s.pop_food_discount)


def discontent(state, p):
    s = effects.state_stats(state, p)
    return max(0, happy_required(p.yellow_bank) - s.happy)


def uprising(state, p):
    return discontent(state, p) > p.workers_free


# ------------------------------------------------- end-of-turn sequence

def end_of_turn(state, p, rng):
    """§6.6, exact order. Mutates the player in place."""
    effects.invalidate(state, p)
    s = effects.state_stats(state, p)

    # 1. discard excess military cards (down to the military action total)
    limit = s.military_actions + s.military_hand_limit
    while len(p.hand_military) > limit:
        name = p.hand_military.pop(0)
        discard_military(state, name)

    # 2. uprising check
    rebels = uprising(state, p)
    if rebels:
        state.emit(f"uprising! (discontent {discontent(state, p)} > "
                   f"{p.workers_free} unused workers): production skipped")
    else:
        # 3. production phase
        # a. score science and culture
        p.science += s.science
        p.culture += s.culture
        _end_of_turn_leader_bonus(state, p)
        # b. corruption
        corr = corruption(effects.blue_available(p))
        paid = effects.pay_resources(p, corr)
        short = corr - paid
        if short:
            p.food = max(0, p.food - short)
        # c. food production
        effects.gain_food(p, s.food)
        # d. food consumption
        need = consumption(p.yellow_bank)
        if p.food >= need:
            p.food -= need
        else:
            missing = need - p.food
            p.food = 0
            p.culture = max(0, p.culture - 4 * missing)
        # e. resource production
        effects.gain_resources(p, s.resources)

    # 4. draw military cards (never in age IV, never on round 1)
    if state.has_military and state.age_military != "IV" and state.round > 1:
        for _ in range(min(3, max(0, p.military_actions))):
            card = draw_military(state)
            if card is None:
                break
            p.hand_military.append(card)

    # 5. reset actions
    effects.invalidate(state, p)
    s = effects.state_stats(state, p)
    p.civil_actions = s.civil_actions
    p.military_actions = s.military_actions
    p.tactic_action_used = False
    p.hammurabi_used = False
    p.churchill_used = False
    p.bach_upgrade_used = False
    p.ocean_liners_used = False
    p.politics_done = False
    p.taken_this_turn = []


def _end_of_turn_leader_bonus(state, p):
    if p.leader == "Genghis Khan":
        strengths = sorted(
            (effects.state_stats(state, q).strength
             for q in state.players if not q.resigned), reverse=True)
        mine = effects.state_stats(state, p).strength
        if len(strengths) < 2 or mine >= strengths[1]:
            p.culture += 3


# ---------------------------------------------------- military deck I/O

def discard_military(state, name):
    age = C.db().age_of(name) if name in C.db().by_name else state.age_military
    state.discarded_military.setdefault(age, []).append(name)


def draw_military(state):
    if not state.military_deck:
        pile = state.discarded_military.get(state.age_military) or []
        if not pile:
            return None
        state.military_deck = list(pile)
        state.discarded_military[state.age_military] = []
        _rng(state).shuffle(state.military_deck)
    return state.military_deck.pop()


def _rng(state):
    import random
    return random.Random(state.seed * 7919 + state.turn)


# ---------------------------------------------------- population helpers

def increase_population(state, p, free=False):
    """Move a token from the yellow bank into the worker pool (§3.3)."""
    if p.yellow_bank <= 0:
        return False
    cost = 0 if free else (pop_cost(state, p) or 0)
    if p.food < cost:
        return False
    p.food -= cost
    p.yellow_bank -= 1
    p.workers_free += 1
    effects.invalidate(state, p)
    return True


def lose_population(state, p):
    """'Lose 1 population': unused worker first, else one off a card."""
    if p.workers_free > 0:
        p.workers_free -= 1
        p.yellow_bank += 1
        effects.invalidate(state, p)
        return True
    db = C.db()
    for name, t in p.techs.items():
        if t.workers and db.type_of(name) in C.WORKER_TYPES:
            t.workers -= 1
            p.yellow_bank += 1
            effects.invalidate(state, p)
            return True
    return False
