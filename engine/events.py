"""Events, aggressions and wars (§5, §11, §12.5).

Only reachable when the military card data is complete
(`CardDB.has_military`).  Card effects are read from a small tag
vocabulary; unknown tags are logged and ignored so that partial data can
never crash a game.
"""
from __future__ import annotations

from . import cards as C
from . import economy
from . import effects

# effect tags understood on events / aggressions / territories
GAIN_TAGS = {
    "gainFood": lambda st, p, v: effects.gain_food(p, v),
    "gainResources": lambda st, p, v: effects.gain_resources(p, v),
    "gainScience": lambda st, p, v: setattr(p, "science", p.science + v),
    "gainCulture": lambda st, p, v: setattr(p, "culture", p.culture + v),
    "loseScience": lambda st, p, v: setattr(p, "science", max(0, p.science - v)),
    "loseCulture": lambda st, p, v: setattr(p, "culture", max(0, p.culture - v)),
}


def apply_gains(state, p, eff):
    for k, v in (eff or {}).items():
        fn = GAIN_TAGS.get(k)
        if fn:
            fn(state, p, v)
        elif k == "gainPopulation":
            for _ in range(v):
                if p.yellow_bank > 0:
                    p.yellow_bank -= 1
                    p.workers_free += 1
        elif k == "losePopulation":
            for _ in range(v):
                economy.lose_population(state, p)
    effects.invalidate(state, p)


def reveal_current_event(state, rng):
    """Reveal and resolve the top card of the current events deck (§5.2)."""
    if not state.current_events:
        _recycle_future_events(state, rng)
        if not state.current_events:
            return None
    name = state.current_events.pop()
    card = C.db().get(name) if name in C.db().by_name else None
    state.past_events.append(name)
    if card is None:
        return name
    if card["type"] == "territory":
        # colonization auctions are not implemented yet: the card is simply
        # discarded to the past events pile (see engine/PROGRESS.md)
        state.emit(f"territory {name} revealed (auction not implemented)")
        return name
    scope = (card.get("effects") or {}).get("scope", "revealer")
    targets = ([state.me()] if scope == "revealer"
               else [q for q in state.players if not q.resigned])
    for q in targets:
        apply_gains(state, q, card.get("effects"))
    state.emit(f"event {name} resolved")
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
    # earlier ages resolve first => later ages at the bottom of the pop() list
    deck.sort(key=lambda n: -C.db().level_of(n))
    state.current_events = deck


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
    # defender discards military cards for +1 each, up to their MA total
    budget = effects.state_stats(state, defender).military_actions
    spent = 0
    while dfn < atk and spent < budget and defender.hand_military:
        card_name = defender.hand_military.pop()
        bonus = (db.get(card_name).get("effects") or {}).get("defense", 1) \
            if card_name in db.by_name else 1
        dfn += bonus
        economy.discard_military(state, card_name)
        spent += 1
    if dfn >= atk:
        state.emit(f"aggression {name} vs P{defender.idx} failed")
        return False
    apply_gains(state, attacker, card.get("effects"))
    apply_gains(state, defender, (card.get("effects") or {}).get("victim"))
    state.emit(f"aggression {name} vs P{defender.idx} succeeded")
    return True


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
        w for w in defender.wars_declared_on_me if w != tuple(war)]
    a = effects.state_stats(state, attacker).strength
    d = effects.state_stats(state, defender).strength
    economy.discard_military(state, name)
    if a == d:
        return
    victor, loser, adv = ((attacker, defender, a - d) if a > d
                          else (defender, attacker, d - a))
    kind = WAR_SPOILS.get(name)
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


def evaluate_final_events(state):
    """Age III events left in the current/future decks score at game end."""
    db = C.db()
    for name in list(state.current_events) + list(state.future_events):
        if name not in db.by_name or db.age_of(name) != "III":
            continue
        card = db.get(name)
        for q in state.players:
            if not q.resigned:
                apply_gains(state, q, card.get("effects"))
