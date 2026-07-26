"""Decision points that do not belong to the player whose turn it is.

The rest of the engine is a single-player-at-a-time move generator: the
player to move is ``state.current``.  Several rules break that assumption
-- colonization auctions (§11), aggression defense (§5.4.4), pact offers
(§5.9) and "each player chooses" event effects (§5.3) -- so the state
carries a stack of PENDING decisions.  The same machinery also carries
decisions that ARE the current player's but arrive mid-effect: an action
card's ordered free action (§3.11), raid/annex/infiltrate targeting.

* ``state.pending``  -- stack of decision dicts; the top one owns the move.
* ``state.decider()`` -- index of the player who must move next.
* ``state.queue``    -- FIFO of *deferred* sub-effects (clockwise order,
  §5.3); each entry is a plain JSON dict so the state stays serializable.

Decision kinds:

``choice``   options are JSON values; moves are ``("choose", i)``
``auction``  moves are ``("bid", n)`` / ``("bid_pass",)``
``defense``  moves are ``("defend", card)`` / ``("defend_done",)``
"""
from __future__ import annotations

from . import cards as C
from . import economy
from . import effects

# Module-level bindings for the singleton card DB: `C.db()` was ~734k calls
# per 60 4p games.  cards.py has no engine imports, so this is safe at import.
_DB = C.db()
_TYPE_BY_NAME = _DB.type_by_name
_BY_NAME = _DB.by_name
_LEVEL_BY_NAME = _DB.level_by_name


# ------------------------------------------------------------- plumbing

def top(state):
    return state.pending[-1] if state.pending else None


def pending_moves(state):
    pend = state.pending[-1]
    kind = pend["kind"]
    if kind == "choice":
        return [("choose", i) for i in range(len(pend["options"]))]
    if kind == "auction":
        p = state.players[pend["player"]]
        out = [("bid_pass",)]
        top_bid = pend["bid"]
        for n in range(top_bid + 1, max_force(state, p) + 1):
            out.append(("bid", n))
        return out
    if kind == "defense":
        out = [("defend_done",)]
        d = state.players[pend["player"]]
        if pend["spent"] < pend["budget"]:
            for n in sorted(set(d.hand_military)):
                out.append(("defend", n))
        return out
    raise ValueError(f"unknown pending decision {pend!r}")


def apply_pending(state, move, rng=None):
    pend = state.pending[-1]
    kind = move[0]
    if kind == "choose":
        state.pending.pop()
        _resolve_choice(state, pend, move[1], rng)
    elif kind in ("bid", "bid_pass"):
        _auction_move(state, pend, move, rng)
    elif kind in ("defend", "defend_done"):
        _defense_move(state, pend, move, rng)
    else:
        raise ValueError(f"unknown pending move {move!r}")
    run_queue(state, rng)
    effects.invalidate(state)


def push_choice(state, player_idx, tag, options, ctx=None, auto=True):
    """Ask `player_idx` to pick one of `options`.

    With `auto` a single option is taken immediately (no decision) and an
    empty option list is a no-op -- so callers never have to special-case
    "there is nothing to choose from".
    """
    if not options:
        return False
    pend = {"kind": "choice", "player": player_idx, "tag": tag,
            "options": list(options), "ctx": dict(ctx or {})}
    if auto and len(options) == 1:
        _resolve_choice(state, pend, 0, None)
        return False
    state.pending.append(pend)
    return True


def enqueue(state, item):
    """Defer a sub-effect (resolved in FIFO order once the stack empties)."""
    state.queue.append(item)


def run_queue(state, rng=None):
    """Resolve deferred sub-effects until one of them needs a decision."""
    while not state.pending and state.queue:
        item = state.queue.pop(0)
        _run_item(state, item, rng)


def _run_item(state, item, rng):
    p = state.players[item["player"]]
    if p.resigned:
        return
    tag = item["tag"]
    fn = _QUEUE_ITEMS.get(tag)
    if fn is not None:
        fn(state, p, item, rng)


# --------------------------------------------------------------- choices

def _resolve_choice(state, pend, idx, rng):
    fn = _CHOICE.get(pend["tag"])
    if fn is None:
        return
    p = state.players[pend["player"]]
    fn(state, p, pend["options"][idx], pend.get("ctx") or {}, rng)
    effects.invalidate(state, p)


def _c_gain_block(state, p, opt, ctx, rng):
    from . import events
    events.apply_gains(state, p, opt, rng)


def _c_free_build(state, p, opt, ctx, rng):
    if opt == "skip" or p.workers_free <= 0:
        return
    if opt not in p.techs:
        return
    p.techs[opt].workers += 1
    p.workers_free -= 1


def _c_destroy_own(state, p, opt, ctx, rng):
    """Destroy one of your own farms/mines/urban buildings (no refund)."""
    t = p.techs.get(opt)
    if t and t.workers > 0:
        t.workers -= 1
        p.workers_free += 1


def _c_lose_pop(state, p, opt, ctx, rng):
    """'Lose 1 population' when there is no unused worker (FAQ p.15)."""
    t = p.techs.get(opt)
    if t and t.workers > 0:
        t.workers -= 1
        p.yellow_bank += 1


def _c_lose_colony(state, p, opt, ctx, rng):
    lose_colony(state, p, opt)


def _c_flip_wonder(state, p, opt, ctx, rng):
    if opt in p.completed_wonders and opt not in p.flipped_wonders:
        p.flipped_wonders.append(opt)


def _c_discard_military(state, p, opt, ctx, rng):
    if opt in p.hand_military:
        p.hand_military.remove(opt)
        economy.discard_military(state, opt)


def _c_raid(state, p, opt, ctx, rng):
    """Attacker picks the urban building to destroy (namu_military 36)."""
    db = _DB
    victim = state.players[ctx["victim"]]
    t = victim.techs.get(opt)
    if not t or t.workers <= 0:
        return
    t.workers -= 1
    victim.workers_free += 1
    if ctx.get("loot", True):
        printed = db.get(opt).get("buildCost") or 0
        effects.gain_resources(p, (printed + 1) // 2)
    effects.invalidate(state, victim)


def _c_annex(state, p, opt, ctx, rng):
    """Aggression: Annex -- the colony's permanent bonus changes hands."""
    victim = state.players[ctx["victim"]]
    lose_colony(state, victim, opt)
    p.colonies.append(opt)
    perm = _DB.get(opt).get("permanentEffects") or {}
    effects.grant_yellow(p, perm.get("yellowTokens", 0))
    p.blue_total = max(0, p.blue_total + perm.get("blueTokens", 0))
    effects.invalidate(state, victim)


def _c_infiltrate(state, p, opt, ctx, rng):
    """Remove the rival's leader or unfinished wonder; 3 culture per level."""
    db = _DB
    victim = state.players[ctx["victim"]]
    per = ctx.get("per", 3)
    if opt == "leader" and victim.leader:
        effects.on_leave_play(state, victim, victim.leader)
        p.culture += per * db.level_of(victim.leader)
        victim.leader = None
    elif opt == "wonder" and victim.wonder:
        p.culture += per * db.level_of(victim.wonder.name)
        victim.wonder = None
    effects.invalidate(state, victim)


def _c_pact_offer(state, p, opt, ctx, rng):
    """§5.9: the partner accepts or refuses; refusal returns it to hand."""
    owner = state.players[ctx["owner"]]
    name = ctx["name"]
    if opt == "accept":
        owner.pacts = [{"name": name, "owner": owner.idx, "partner": p.idx,
                        "a": ctx.get("a", owner.idx), "b": ctx.get("b", p.idx)}]
        state.emit(f"pact {name} between P{owner.idx} and P{p.idx}")
        effects.invalidate(state)
    else:
        owner.hand_military.append(name)
        state.emit(f"pact {name} refused by P{p.idx}")


def _c_take_row(state, p, opt, ctx, rng):
    """International Agreement: spend up to N civil actions taking cards."""
    from . import actions
    if opt == "stop":
        _finish_take_row(state, rng)
        return
    idx = int(opt)
    budget = ctx["budget"]
    cost = actions.take_cost(state, p, idx)
    actions.take_card(state, p, idx)
    budget -= cost
    _offer_take_row(state, p, budget, rng)


def _offer_take_row(state, p, budget, rng):
    from . import actions
    opts = []
    for idx, name in enumerate(state.card_row):
        if name is None:
            continue
        if actions.take_cost(state, p, idx) > budget:
            continue
        if actions.can_take(state, p, idx, budget=budget):
            opts.append(idx)
    if not opts:
        _finish_take_row(state, rng)
        return
    push_choice(state, p.idx, "take_row", opts + ["stop"],
                {"budget": budget}, auto=False)


def _finish_take_row(state, rng):
    """CoL p.12: replenish afterwards WITHOUT discarding the first slots."""
    from . import game
    row = state.card_row
    kept = [c for c in row if c is not None]
    state.card_row = kept + [None] * (len(row) - len(kept))
    game.deal_row(state, rng)


def _c_free_civil(state, p, opt, ctx, rng):
    """Perform an action card's ordered action (§3.11)."""
    from . import actions
    actions.apply_free_action(state, p, tuple(opt), ctx.get("discount", 0))


def _c_food_or_res(state, p, opt, ctx, rng):
    n = int(ctx.get("n", 0))
    if opt == "food":
        effects.gain_food(p, n)
    else:
        effects.gain_resources(p, n)


_CHOICE = {
    "gain_block": _c_gain_block,
    "free_civil": _c_free_civil,
    "food_or_res": _c_food_or_res,
    "free_build": _c_free_build,
    "destroy_own": _c_destroy_own,
    "lose_pop": _c_lose_pop,
    "lose_colony": _c_lose_colony,
    "flip_wonder": _c_flip_wonder,
    "discard_military": _c_discard_military,
    "raid": _c_raid,
    "annex": _c_annex,
    "infiltrate": _c_infiltrate,
    "pact_offer": _c_pact_offer,
    "take_row": _c_take_row,
}


# ------------------------------------------------------- queued sub-effects

def _q_gains(state, p, item, rng):
    from . import events
    events.apply_gains(state, p, item["block"], rng, sign=item.get("sign", 1))


def _q_choose(state, p, item, rng):
    push_choice(state, p.idx, "gain_block", item["options"])


def _q_free_build(state, p, item, rng):
    """Event free build: pick a card of the allowed kind, or decline."""
    db = _DB
    spec = item["spec"]
    if p.workers_free <= 0:
        return
    want = spec.get("card")
    opts = []
    for name in sorted(p.techs):
        card = db.get(name)
        if card["type"] not in C.WORKER_TYPES:
            continue
        if want and card.get("baseName", name) != want and name != want:
            continue
        if spec.get("age") and card["age"] != spec["age"]:
            continue
        if spec.get("type") and card["type"] != spec["type"]:
            continue
        if card["type"] in C.URBAN_TYPES:
            from . import actions
            s = effects.state_stats(state, p)
            if actions.urban_count(p, card["type"]) >= s.urban_limit:
                continue
        cost = spec.get("cost", 0)
        if cost and p.resources < cost:
            continue
        opts.append(name)
    if not opts:
        return
    push_choice(state, p.idx, "free_build", opts + ["skip"], auto=False)


def _q_destroy_own(state, p, item, rng):
    db = _DB
    opts = sorted(n for n, t in p.techs.items()
                  if t.workers > 0 and db.type_of(n) in
                  C.URBAN_OR_PRODUCTION)
    push_choice(state, p.idx, "destroy_own", opts)


def _q_lose_pop(state, p, item, rng):
    for _ in range(int(item.get("n", 1))):
        if p.workers_free > 0:
            p.workers_free -= 1
            p.yellow_bank += 1
            continue
        db = _DB
        opts = sorted(n for n, t in p.techs.items()
                      if t.workers > 0 and db.type_of(n) in C.WORKER_TYPES)
        if push_choice(state, p.idx, "lose_pop", opts):
            # remaining losses are re-queued behind the decision
            left = int(item.get("n", 1)) - 1
            if left > 0:
                state.queue.insert(0, {"player": p.idx, "tag": "lose_pop",
                                       "n": left})
            return
    effects.invalidate(state, p)


def _q_lose_colony(state, p, item, rng):
    push_choice(state, p.idx, "lose_colony", sorted(p.colonies))


def _q_flip_wonder(state, p, item, rng):
    db = _DB
    ages = item.get("ages") or ["A", "I"]
    opts = sorted(w for w in p.completed_wonders
                  if w not in p.flipped_wonders and db.age_of(w) in ages)
    push_choice(state, p.idx, "flip_wonder", opts)


def _q_discard_military(state, p, item, rng):
    for _ in range(int(item.get("n", 1))):
        opts = sorted(set(p.hand_military))
        if not opts:
            return
        if push_choice(state, p.idx, "discard_military", opts):
            left = int(item.get("n", 1)) - 1
            if left > 0:
                state.queue.insert(0, {"player": p.idx,
                                       "tag": "discard_military", "n": left})
            return


def _q_raid(state, p, item, rng):
    db = _DB
    victim = state.players[item["victim"]]
    max_lv = C.level(item.get("max_age", "A"))
    opts = sorted(n for n, t in victim.techs.items()
                  if t.workers > 0 and db.type_of(n) in C.URBAN_TYPES
                  and db.level_of(n) <= max_lv)
    push_choice(state, p.idx, "raid", opts,
                {"victim": victim.idx, "loot": not item.get("no_loot")})


def _q_annex(state, p, item, rng):
    victim = state.players[item["victim"]]
    push_choice(state, p.idx, "annex", sorted(victim.colonies),
                {"victim": victim.idx})


def _q_infiltrate(state, p, item, rng):
    victim = state.players[item["victim"]]
    opts = []
    if victim.leader:
        opts.append("leader")
    if victim.wonder:
        opts.append("wonder")
    push_choice(state, p.idx, "infiltrate", opts,
                {"victim": victim.idx, "per": item.get("per", 3)})


def _q_take_row(state, p, item, rng):
    _offer_take_row(state, p, int(item.get("budget", 0)), rng)


def _q_free_civil(state, p, item, rng):
    """Offer the concrete moves that satisfy an action card's order."""
    from . import actions
    opts = actions.free_action_moves(state, p, item["kind"],
                                     item.get("discount", 0),
                                     item.get("revolt_ok", False))
    push_choice(state, p.idx, "free_civil", [list(m) for m in opts],
                {"discount": item.get("discount", 0)})


def _q_card_gains(state, p, item, rng):
    """The gain half of an action card, landing after its ordered action."""
    from . import actions
    actions.apply_card_gains(state, p, item.get("gains") or {})


_QUEUE_ITEMS = {
    "gains": _q_gains,
    "free_civil": _q_free_civil,
    "card_gains": _q_card_gains,
    "choose": _q_choose,
    "free_build": _q_free_build,
    "destroy_own": _q_destroy_own,
    "lose_pop": _q_lose_pop,
    "lose_colony": _q_lose_colony,
    "flip_wonder": _q_flip_wonder,
    "discard_military": _q_discard_military,
    "raid": _q_raid,
    "annex": _q_annex,
    "infiltrate": _q_infiltrate,
    "take_row": _q_take_row,
}


# ------------------------------------------------------------ colonization

def unit_pool(p):
    """Every military unit in play, one entry per worker (§11.3)."""
    db = _DB
    out = []
    for n, t in p.techs.items():
        if db.type_of(n) in C.UNIT_TYPES and t.workers > 0:
            out.extend([n] * t.workers)
    out.sort(key=lambda n: (db.get(n).get("strength") or 0, db.level_of(n)))
    return out


def bonus_pool(p):
    db = _DB
    out = [n for n in p.hand_military
           if n in db.by_name and db.type_of(n) == "bonus"]
    out.sort(key=lambda n: (db.get(n).get("effects") or {}).get(
        "colonizationBonus", 0))
    return out


def force_value(state, p, units, bonuses):
    """Colonization force of a concrete sacrifice (§11.3)."""
    db = _DB
    if not units:
        return 0
    total = sum(db.get(n).get("strength") or 0 for n in units)
    total += effects.army_strength_units(
        state, p, [(db.type_of(n), db.level_of(n)) for n in units])
    total += effects.state_stats(state, p).colonize
    total += sum((db.get(n).get("effects") or {}).get("colonizationBonus", 0)
                 for n in bonuses)
    return total


def max_force(state, p):
    """Largest force this player could send -- the bidding ceiling (§11.2)."""
    units = unit_pool(p)
    if not units:
        return 0
    return force_value(state, p, units, bonus_pool(p))


def start_auction(state, name, revealer_idx, rng=None):
    """A territory card revealed as the current event is auctioned (§11.1)."""
    from . import events
    order = [q.idx for q in events._order_from(state, revealer_idx)]
    active = [i for i in order if max_force(state, state.players[i]) > 0]
    if not active:
        state.past_events.append(name)
        state.emit(f"territory {name}: nobody can colonize")
        return
    state.pending.append({"kind": "auction", "card": name, "active": active,
                          "pos": 0, "bid": 0, "high": None,
                          "player": active[0]})


def _auction_move(state, pend, move, rng):
    if move[0] == "bid":
        pend["bid"] = move[1]
        pend["high"] = pend["player"]
        pend["pos"] = (pend["pos"] + 1) % len(pend["active"])
    else:
        del pend["active"][pend["pos"]]
        if pend["pos"] >= len(pend["active"]):
            pend["pos"] = 0
    active = pend["active"]
    if not active:
        state.pending.pop()
        state.past_events.append(pend["card"])
        state.emit(f"territory {pend['card']}: no bids")
        return
    if len(active) == 1 and pend["high"] == active[0]:
        state.pending.pop()
        winner = state.players[active[0]]
        colonize(state, winner, pend["card"], pend["bid"], rng)
        return
    pend["player"] = active[pend["pos"]]


def colonize(state, p, name, bid, rng=None):
    """Pay a force of at least `bid` and take the colony (§11.3-11.5)."""
    units, bonuses = _build_force(state, p, bid)
    for n in units:                       # §11.4 tokens go to the yellow bank
        p.techs[n].workers -= 1
        p.yellow_bank += 1
    for n in bonuses:                     # §11.6 discarded before any draw
        p.hand_military.remove(n)
        economy.discard_military(state, n)
    effects.invalidate(state, p)
    state.emit(f"P{p.idx} colonized {name} with force {bid}")
    gain_colony(state, p, name, rng)


def _build_force(state, p, bid):
    """Cheapest-ish sacrifice reaching `bid`: bonus cards before units."""
    units = unit_pool(p)
    bonuses = bonus_pool(p)
    chosen_u = [units.pop(0)]             # §11.3 at least one unit
    chosen_b = []
    while force_value(state, p, chosen_u, chosen_b) < bid:
        if bonuses:
            chosen_b.append(bonuses.pop(0))
        elif units:
            chosen_u.append(units.pop(0))
        else:
            break
    return chosen_u, chosen_b


def gain_colony(state, p, name, rng=None):
    """Permanent effects first, then the one-time effect (§11.5)."""
    from . import events
    db = _DB
    p.colonies.append(name)
    perm = db.get(name).get("permanentEffects") or {}
    effects.grant_yellow(p, perm.get("yellowTokens", 0))
    p.blue_total = max(0, p.blue_total + perm.get("blueTokens", 0))
    effects.invalidate(state, p)
    events.apply_gains(state, p, db.get(name).get("immediateEffects") or {},
                       rng)


def lose_colony(state, p, name):
    """The permanent effects go away; the one-time effect is never undone."""
    if name not in p.colonies:
        return
    p.colonies.remove(name)
    perm = _DB.get(name).get("permanentEffects") or {}
    p.yellow_bank = max(0, p.yellow_bank - perm.get("yellowTokens", 0))
    p.blue_total = max(0, p.blue_total - perm.get("blueTokens", 0))
    effects.invalidate(state, p)


# ------------------------------------------------------- aggression defense

def start_defense(state, attacker, defender, name, atk_strength, rng=None):
    """§5.4.4: the defender may play bonus cards / discard military cards."""
    budget = effects.state_stats(state, defender).military_actions
    dfn = effects.state_stats(state, defender).strength
    ctx = {"kind": "defense", "player": defender.idx, "attacker": attacker.idx,
           "card": name, "atk": atk_strength, "dfn": dfn, "spent": 0,
           "budget": budget}
    if budget <= 0 or not defender.hand_military:
        _finish_defense(state, ctx, rng)
        return
    state.pending.append(ctx)


def _defense_move(state, pend, move, rng):
    db = _DB
    d = state.players[pend["player"]]
    if move[0] == "defend":
        name = move[1]
        d.hand_military.remove(name)
        eff = (db.get(name).get("effects") or {}) if name in db.by_name else {}
        bonus = eff.get("defenseBonus")
        pend["dfn"] += bonus if isinstance(bonus, int) else 1
        pend["spent"] += 1
        economy.discard_military(state, name)
        if pend["spent"] < pend["budget"] and d.hand_military:
            return
    state.pending.pop()
    _finish_defense(state, pend, rng)


def _finish_defense(state, ctx, rng):
    from . import events
    events.finish_aggression(state, ctx, rng)
