"""Population/happiness/corruption tables and the End-of-Turn Sequence.

All numbers follow docs/RULES_SPEC.md §6 exactly.
"""
from __future__ import annotations

import random as _random

from . import cards as C
from . import effects
from . import journal

# Module-level bindings for the singleton card DB: `C.db()` was ~734k calls
# per 60 4p games.  cards.py has no engine imports, so this is safe at import.
_DB = C.db()
_TYPE_BY_NAME = _DB.type_by_name
_BY_NAME = _DB.by_name
_LEVEL_BY_NAME = _DB.level_by_name


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

def pop_food_cost(stats, yellow_bank, one_time=None):
    """Food to increase population, given already-computed `stats` (§6.1).

    THE single implementation of the formula.  It existed in three copies --
    here, `weighted.features` and `neural_encode` -- which is the shape of
    bug this repo has now paid for twice (`buildDiscount` summed instead of
    maxed, and the hand double-count).  The evaluators hold the `Stats`
    already and must not pay for a second `state_stats`, which is why this
    takes stats rather than state; `pop_cost` below is the state-taking
    wrapper for callers that do not.

    `one_time` is deliberately optional and the two evaluator callers pass
    nothing, which preserves their exact current behaviour: neither has ever
    applied `one_time_discount`.  That is a real (small) blind spot -- an
    evaluator overprices a population increase while a one-shot discount is
    pending -- but fixing it changes what the bot plays, so it belongs in its
    own measured change rather than smuggled into a de-duplication.
    """
    base = pop_cost_base(yellow_bank)
    if base is None:
        return None
    if one_time:
        base -= (one_time.get("increasePopulation") or {}).get("food", 0)
    return max(0, base - stats.pop_food_discount)


def pop_cost(state, p):
    return pop_food_cost(effects.state_stats(state, p), p.yellow_bank,
                         p.one_time_discount)


def discontent(state, p):
    s = effects.state_stats(state, p)
    return max(0, happy_required(p.yellow_bank) - s.happy)


def uprising(state, p):
    return discontent(state, p) > p.workers_free


# ------------------------------------------------- end-of-turn sequence

def end_of_turn(state, p, rng):
    """§6.6, exact order. Mutates the player in place.

    Returns False when step 1 pushed the player's discard decision and the
    sequence is SUSPENDED: steps 2-5 have not run.  The caller resumes by
    calling this again once the choice resolves (`game._resume_end_turn`,
    driven by the `end_of_turn` queue item).  Step 1 is idempotent -- it
    re-reads the hand limit -- so re-entry is the whole resume mechanism.

    Returns True when the sequence ran to the end.  §6.6 step 1 is the only
    step that can suspend: "Once you have decided which military cards to
    discard, the rest of your turn is automatic. That is, it requires no more
    decisions." [RB p.20, quoted in RULES_SPEC §6.6].
    """
    from . import interact
    effects.invalidate(state, p)

    # 1. discard excess military cards (down to the military action total).
    #    The player CHOOSES which -- this is the one decision in §6.6.
    if interact.discard_excess_military(state, p):
        return False

    s = effects.state_stats(state, p)

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
            journal.touch(p.hand_military).append(card)

    # 5. reset actions
    effects.invalidate(state, p)
    s = effects.state_stats(state, p)
    p.civil_actions = max(0, s.civil_actions - p.ca_penalty_next_turn)
    p.ca_penalty_next_turn = 0
    p.military_actions = s.military_actions
    p.tactic_action_used = False
    p.hammurabi_used = False
    p.churchill_used = False
    p.bach_upgrade_used = False
    p.ocean_liners_used = False
    p.politics_done = False
    p.caesar_second_politics = False
    # Backstop for Joan of Arc's look: `actions._end_politics` clears it when
    # the phase closes, and a turn that never had a politics phase never set
    # it, but a stale name here would be a lie about what this seat knows.
    p.peeked_event = None
    p.taken_this_turn = []
    p.mil_discount = 0                   # §3.11 action-card discounts expire
    p.mil_sci_discount = 0               # Churchill's ring-fenced science
    return True


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
    age = _DB.age_of(name) if name in _DB.by_name else state.age_military
    journal.touch(journal.touch(state.discarded_military)
                  .setdefault(age, [])).append(name)


def discard_civil(state, name):
    """Record a civil card leaving play into the face-up discard.

    The military side has had `discard_military` since GAP 5; the civil side
    grew the same need the moment `engine.bots.counting` started subtracting
    what it has seen from what the rulebook prints.  A leader that is replaced,
    an antiquated leader or half-built wonder, a wonder an opponent destroys, a
    one-shot action card spent, a government superseded -- every one of those
    happens in the open at the table, and every one of them used to be dropped
    on the floor by the engine (`p.leader = None` and nothing else), so an
    age's printed card count stopped adding up and the counter read the
    shortfall as "still in a rival's hand".

    It writes `state.civil_removed`, NOT `state.civil_discard`: that field
    already means "swept off the row" to `neural_encode` and to
    `tools/card_census`, and widening it would have changed a trained encoder's
    input without anything failing.  Both are RECORDS, not state -- nothing in
    the rules or the turn loop reads either -- so this cannot change play.
    """
    age = _DB.age_of(name) if name in _DB.by_name else state.age_civil
    journal.touch(journal.touch(state.civil_removed)
                  .setdefault(age, [])).append(name)


def draw_military(state):
    if not state.military_deck:
        pile = state.discarded_military.get(state.age_military) or []
        if not pile:
            return None
        state.military_deck = list(pile)
        journal.touch(state.discarded_military)[state.age_military] = []
        _rng(state).shuffle(state.military_deck)
    return journal.touch(state.military_deck).pop()


def _rng(state):
    return _random.Random(state.seed * 7919 + state.turn)


# ---------------------------------------------------- population helpers

def increase_population(state, p, free=False, discount=0):
    """Move a token from the yellow bank into the worker pool (§3.3).

    `discount` is food off THIS increase only, floored at 0 -- Frederick
    Barbarossa's `comboFoodDiscount`, which applies to the increase bought by
    his combined military action and to no other.  It is deliberately NOT
    `Stats.pop_food_discount`: that field is a STANDING discount on every
    population increase (`popIncreaseFoodDiscount`, e.g. Irrigation), it is
    read by both evaluators through `pop_food_cost`, and putting a
    once-per-action discount there would make every plain `pop` cheaper than
    the rulebook says and make the evaluators believe it.
    """
    if p.yellow_bank <= 0:
        return False
    cost = 0 if free else max(0, (pop_cost(state, p) or 0) - discount)
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
    db = _DB
    for name, t in p.techs.items():
        if t.workers and db.type_of(name) in C.WORKER_TYPES:
            t.workers -= 1
            p.yellow_bank += 1
            effects.invalidate(state, p)
            return True
    return False
