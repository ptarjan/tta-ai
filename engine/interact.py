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
``colonize`` moves are ``("send_unit", tech)`` / ``("send_bonus", card)`` /
             ``("send_done",)`` -- the auction winner builds the force it
             sacrifices (RULES_SPEC 11.3)
"""
from __future__ import annotations

from . import cards as C
from . import economy
from . import effects
from . import journal

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
    if kind == "colonize":
        return _colonize_moves(state, pend)
    raise ValueError(f"unknown pending decision {pend!r}")


def apply_pending(state, move, rng=None):
    pend = state.pending[-1]
    kind = move[0]
    if kind == "choose":
        journal.touch(state.pending).pop()
        _resolve_choice(state, pend, move[1], rng)
    elif kind in ("bid", "bid_pass"):
        _auction_move(state, pend, move, rng)
    elif kind in ("defend", "defend_done"):
        _defense_move(state, pend, move, rng)
    elif kind in ("send_unit", "send_bonus", "send_done"):
        _colonize_move(state, pend, move, rng)
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
    journal.touch(state.pending).append(pend)
    return True


def enqueue(state, item):
    """Defer a sub-effect (resolved in FIFO order once the stack empties)."""
    journal.touch(state.queue).append(item)


# ------------------------------------------------- military hand discards

def defense_points(name):
    """What a military card adds to defense when spent defending (§5.4.4).

    The three Military Bonus cards print 2/4/6; every other military card is
    worth the flat +1 of a face-down card.  `_defense_move` is the authority
    and calls this, so the two can never disagree.
    """
    eff = (_BY_NAME[name].get("effects") or {}) if name in _BY_NAME else {}
    bonus = eff.get("defenseBonus")
    return bonus if isinstance(bonus, int) else 1


def discard_options(hand):
    """Distinct military cards, LEAST defensively useful first.

    Presentation order only -- every bot is free to score the options with its
    own evaluator, and the search bots do.  It is load-bearing anyway, because
    the weighted-family evaluator is documented-blind to military card identity
    beyond age (`hand_mil_value` is a sum of age+1; docs/CARD_BLINDNESS.md §3
    item 5, "military hand"),
    so same-age options tie and every argmax in the project falls back to
    option 0.  Alphabetical order made that tie-break arbitrary -- it would
    pitch a Military Bonus (defense 6) ahead of a spent event on nothing but
    the letter M.  Ordering by the engine's own defense arithmetic makes the
    fallback "discard the card that defends least", which is the rule's intent
    and is derived, not invented.  Name is the secondary key, so the order is
    total and deterministic.
    """
    return sorted(set(hand), key=lambda n: (defense_points(n), n))


def discard_excess_military(state, p):
    """§6.6 step 1: discard down to the military hand limit (§6.7).

    The player CHOOSES which card goes -- the rulebook's only end-of-turn
    decision.  Returns True when a choice is now pending and the caller must
    suspend; False when the hand is legal (nothing left to decide).

    Idempotent: it re-reads the limit every call, so the resume path is just
    "call it again".  The limit cannot move while the loop runs -- it is read
    off cards in play, not off the hand.
    """
    while True:
        s = effects.state_stats(state, p)
        limit = s.military_actions + s.military_hand_limit
        if len(p.hand_military) <= limit:
            return False
        opts = discard_options(p.hand_military)
        if not opts:
            return False
        if push_choice(state, p.idx, "discard_military", opts):
            return True
        # one distinct name left: push_choice resolved it without a decision
        # (§6.6 is still satisfied -- there was nothing to choose between)


def run_queue(state, rng=None):
    """Resolve deferred sub-effects until one of them needs a decision."""
    while not state.pending and state.queue:
        item = journal.touch(state.queue).pop(0)
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
        journal.touch(p.flipped_wonders).append(opt)


def _c_discard_military(state, p, opt, ctx, rng):
    if opt in p.hand_military:
        journal.touch(p.hand_military).remove(opt)
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
    journal.touch(p.colonies).append(opt)
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


# ------------------------------------------- War over Technology spoils

#: AN OPTION INDEX, NOT AN AMOUNT OF SCIENCE.  `WAR_TECH_SCIENCE_IDX = 0` reads
#: like a rules value ("a war over technology pays zero science"), which is
#: the opposite of what it says; the name carries `_IDX` for that reason.
#:
#: Index of the "take the rest as science" option inside a `war_tech`
#: choice.  It is FIRST on purpose: every argmax in this project breaks a tie
#: to the lowest index, and index 0 being "science" makes the blind fallback
#: exactly the behaviour the engine had before the choice existed.
#: `settle_war_spoils` depends on this too.
WAR_TECH_SCIENCE_IDX = 0


def war_tech_options(state, victor, loser, budget):
    """Blue technologies the victor of a War over Technology may steal now.

    The card (2015 digital edition text, `data/cards_military_actions.json`):
    *"The victor takes science equal to the strength advantage, or takes
    special (blue) technologies of the same total cost."*  The rules that
    decide WHICH ones are on offer are all in the Code of Laws p.3
    ("Resolve a War") and the official FAQ p.8:

    * *"the victor takes the card from the defeated civilization's PLAY
      AREA"* -- so only cards the loser has in play, not in hand and not
      from the card row.
    * *"A player cannot steal a special technology that is the same as one
      he or she already has in play or in hand."*  FAQ p.8 restates it:
      *"you are not allowed to choose a Technology card which you currently
      have in play or in your hand."*
    * FAQ p.8, *"As long as you win enough Science points you can always
      choose to take some or all of them in blue Special Technologies"* --
      the steal is paid for out of the strength advantage at the card's
      cost, so a technology the remaining advantage cannot cover is not on
      offer.
    * PRINTED cost, not `effects.tech_cost`: Code of Laws p.4, *"If the
      effect refers to the cost of a card, use the cost printed on the card,
      ignoring any modifiers."*

    Most expensive first, then by name: a total order, and the option that
    spends the most of the advantage leads.
    """
    out = []
    for name in loser.techs:
        if _TYPE_BY_NAME.get(name) != "special-tech":
            continue
        if name in victor.techs or name in victor.hand_civil:
            continue
        cost = _BY_NAME[name].get("techCost") or 0
        if cost > budget:
            continue
        out.append((cost, name))
    return [n for _cost, n in sorted(out, key=lambda cn: (-cn[0], cn[1]))]


def war_tech_spoils(state, victor, loser, budget, rng=None):
    """§5.7: hand the victor of a War over Technology its spoils decision.

    Called by `events.resolve_war`.  Mixing is explicitly legal -- FAQ p.8's
    "some or all", and the digital edition's own logs do it constantly (a
    26-vs-14 win taking Code of Laws 6 + Cartography 4 + 2 science = 12) --
    so this offers ONE steal at a time and re-offers with the advantage
    reduced by what the card cost, until the victor takes the remainder as
    science or there is nothing left it may steal.
    """
    _offer_war_tech(state, victor, loser, int(budget), rng)


def _offer_war_tech(state, victor, loser, budget, rng):
    opts = war_tech_options(state, victor, loser, budget)
    if not opts:
        # Nothing stealable: there is no decision, so do not manufacture one.
        take_war_science(state, victor, loser, budget)
        return
    push_choice(state, victor.idx, "war_tech", ["science"] + opts,
                {"victim": loser.idx, "budget": budget}, auto=False)


def _c_war_tech(state, p, opt, ctx, rng):
    loser = state.players[ctx["victim"]]
    budget = int(ctx["budget"])
    if opt == "science":
        take_war_science(state, p, loser, budget)
        return
    cost = _BY_NAME[opt].get("techCost") or 0
    _steal_special_tech(state, p, loser, opt)
    state.emit(f"war spoils: P{p.idx} steals {opt} from P{loser.idx}")
    _offer_war_tech(state, p, loser, budget - cost, rng)


def take_war_science(state, victor, loser, budget):
    """The other half of the card, and the cap FAQ p.8 puts on it.

    *"The attacker cannot take more Science points than the loser has to
    lose"* -- so `min(budget, loser.science)`, exactly as the engine has
    always done.  The FAQ's parenthesis, that the loser's losable total also
    counts the cost of stealable blue cards, is not an extra term here: it is
    already true, because those cards were spendable out of the same budget
    a moment ago.
    """
    take = min(budget, loser.science)
    if take <= 0:
        return
    loser.science -= take
    victor.science += take
    state.emit(f"war spoils: P{victor.idx} takes {take} science "
               f"from P{loser.idx}")


def _steal_special_tech(state, victor, loser, name):
    """Move one blue technology from the loser's play area into the victor's.

    Code of Laws p.3: *"the victor takes the card from the defeated
    civilization's play area and puts it into his or her own play area"*,
    and *"If you steal a special technology of the same type as one that you
    have in play, you keep the higher level card in play and discard the
    other."*  That second sentence is precisely §7.6's one-per-icon rule,
    which `actions.put_special_in_play` already implements for the develop
    path -- so the steal routes through it instead of restating it, and a
    stolen card that loses the comparison is discarded while the loser has
    still lost it.

    It is NOT a develop: no science is paid and `effects.on_develop`
    (Leonardo / Newton / Einstein) does not fire, because nobody played the
    card.  `effects.on_enter_play` / `on_leave_play` DO fire on both sides,
    because the card really does leave one play area and enter the other.
    """
    from . import actions
    effects.on_leave_play(state, loser, name)
    del journal.touch(loser.techs)[name]
    actions.put_special_in_play(state, victor, name)
    effects.invalidate(state, loser)


def settle_war_spoils(state, rng=None):
    """Take any outstanding War over Technology spoils as SCIENCE.

    For LOOKAHEAD callers only (`bots/quiescent.war_value`, and PlanBot
    through it): they resolve a declared war on a scratch state and score the
    position immediately, so a decision left hanging would price the war at
    nothing at all.  Science is the option the card names first and the only
    one the engine had before this choice existed, so a war is priced at its
    science value -- a LOWER bound on the spoils, since a victor only ever
    steals when its own evaluator prefers the cards to the science.

    Deliberately not a search: `war_value` runs on every candidate move of
    every beam node.

    KNOWN, PERMANENT, ONE-SIDED BIAS -- write it down rather than rediscover
    it.  Every search that prices a declared `War over Technology` now sees
    the SCIENCE branch and never the steal, so it sees the floor of the card
    and never its ceiling.  The bots will therefore keep under-declaring that
    war in exactly the positions where the choice was worth implementing:
    the ones where the loser has a fat blue technology to take.  A lower
    bound beats the zero it replaces, so this ships -- but anyone measuring
    the choice and finding nothing has measured THIS, not the choice.  The
    fix, when someone wants it, is to price the best affordable option from
    `war_tech_options` instead of the remainder, at the cost of a card
    valuation inside a function that runs on every beam node.

    Called from `bots/quiescent.war_value` (and PlanBot through it) and from
    `bots/neural_plan._leaf_enc`.  Those are all three of the sites that
    resolve a war on a scratch state; a fourth would need this too.
    """
    while state.pending and state.pending[-1].get("tag") == "war_tech":
        apply_pending(state, ("choose", WAR_TECH_SCIENCE_IDX), rng)


def _c_pact_offer(state, p, opt, ctx, rng):
    """§5.9: the partner accepts or refuses; refusal returns it to hand."""
    owner = state.players[ctx["owner"]]
    name = ctx["name"]
    if opt == "accept":
        # ASSIGNMENT, and it is the printed rule, not a bug -- it has been
        # reported as one.  sources/ubg_full-game.txt:70 (2015 rulebook):
        # "You can be a party to more than one pact, but you can have only one
        # pact in YOUR PLAY AREA.  If you offer a new pact and the player
        # accepts, any pact in your play area is automatically cancelled."
        # So agreeing a new pact as OWNER replaces the one you own, and that is
        # all this line does.  It does not cost you the pacts you are party to
        # but do not own: `effects.pacts_for` scans every player's list, so
        # those live in the other owner's play area and are untouched.
        # tests/test_combat.py pins both halves of that sentence.
        owner.pacts = [{"name": name, "owner": owner.idx, "partner": p.idx,
                        "a": ctx.get("a", owner.idx), "b": ctx.get("b", p.idx)}]
        state.emit(f"pact {name} between P{owner.idx} and P{p.idx}")
        effects.invalidate(state)
    else:
        journal.touch(owner.hand_military).append(name)
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
    "war_tech": _c_war_tech,
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
                journal.touch(state.queue).insert(
                    0, {"player": p.idx, "tag": "lose_pop", "n": left})
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


def _q_end_of_turn(state, p, item, rng):
    """Resume §6.6 after the player's discard decision (economy.end_of_turn).

    The end-of-turn sequence is the one place a decision interrupts a phase
    transition rather than an action, so the continuation rides the deferred
    queue like any other sub-effect: `apply_pending` drains the queue after
    the choice resolves, this runs, and the turn advances from there.
    """
    from . import game
    game._resume_end_turn(state, p, rng)


def _q_auto_skip_politics(state, p, item, rng):
    """Resume the start-of-turn "is passing the only political option?" test
    once a War over Technology's spoils decision has been answered.

    See `game.start_turn` for why it has to wait: the spoils can change the
    current player's military actions.
    """
    from . import game
    if state.phase == "politics" and not p.politics_done:
        game._auto_skip_politics(state, rng)


def _q_discard_military(state, p, item, rng):
    for _ in range(int(item.get("n", 1))):
        opts = discard_options(p.hand_military)
        if not opts:
            return
        if push_choice(state, p.idx, "discard_military", opts):
            left = int(item.get("n", 1)) - 1
            if left > 0:
                journal.touch(state.queue).insert(
                    0, {"player": p.idx, "tag": "discard_military",
                        "n": left})
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
    "end_of_turn": _q_end_of_turn,
    "auto_skip_politics": _q_auto_skip_politics,
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
        journal.touch(state.past_events).append(name)
        state.emit(f"territory {name}: nobody can colonize")
        return
    journal.touch(state.pending).append({"kind": "auction", "card": name, "active": active,
                          "pos": 0, "bid": 0, "high": None,
                          "player": active[0]})


def _auction_move(state, pend, move, rng):
    if move[0] == "bid":
        journal.touch(pend)["bid"] = move[1]
        journal.touch(pend)["high"] = pend["player"]
        journal.touch(pend)["pos"] = (pend["pos"] + 1) % len(pend["active"])
    else:
        del journal.touch(pend["active"])[pend["pos"]]
        if pend["pos"] >= len(pend["active"]):
            journal.touch(pend)["pos"] = 0
    active = pend["active"]
    if not active:
        journal.touch(state.pending).pop()
        journal.touch(state.past_events).append(pend["card"])
        state.emit(f"territory {pend['card']}: no bids")
        return
    if len(active) == 1 and pend["high"] == active[0]:
        journal.touch(state.pending).pop()
        winner = state.players[active[0]]
        colonize(state, winner, pend["card"], pend["bid"], rng)
        return
    journal.touch(pend)["player"] = active[pend["pos"]]


def colonize(state, p, name, bid, rng=None):
    """The auction winner sacrifices a force of at least `bid` (§11.3-11.5).

    RULES_SPEC 11.3 fixes only the FLOOR on the sacrifice -- "printed strength
    of the sacrificed military units (>= 1 unit mandatory, even if other
    bonuses would cover the bid)" plus armies, plus reusable ship icons, plus
    "the colonization value (bottom half) of ANY NUMBER of military bonus cards
    played" -- and §11.2 fixes only that the winner "MUST colonize (no backing
    out), forming a force >= their final bid".  WHICH units and WHICH bonus
    cards make up that force is the player's decision, and it is a real one:
    a unit is permanent strength and a worker, a bonus card is a consumable
    that is also worth +2/+4/+6 defence if kept, and the two are traded off
    against each other every time.

    This used to be `_build_force`, a fixed greedy rule inside the engine
    (weakest unit, then bonus cards cheapest-first, then more units), so the
    engine took the choice away from the player -- the one clean rules-level
    defect in the coverage census.  It is now a pending decision like every
    other (module docstring), resolved by `_colonize_move`.

    Single-move prefixes are auto-resolved, exactly as `push_choice(auto=True)`
    does for a one-option `choice`: while the force so far admits only ONE
    legal continuation there is nothing to decide, so no decision is offered.
    That is not a shortcut, it is the definition of "no choice" -- and it is
    why the common shape (one unit type, no bonus cards) never reaches a bot.

    EACH COMMITMENT IS PAID WHEN IT IS MADE, not in a lump at `send_done`.
    That is a deliberate choice and it is what makes the decision answerable:
    a bot prices a candidate move by applying it to a trial state one ply
    deep, so a `send_unit` that only appended a name to a dict would cost
    *nothing* an evaluator can see, while `send_done` carried the entire bill.
    Every 1-ply bot then reads "commit another unit" as free and "send the
    force" as expensive, and procrastinates until the pool is empty.  That is
    not hypothetical: GreedyBot sacrificed its whole five-unit army for a
    force of 3 the first time this was written the other way
    (`tests/test_colony_sacrifice_choice.py::test_nobody_wastes_the_whole_army`
    is that regression, pinned).  It is the same shape as the `bid`-scores-
    exactly-like-`bid_pass` bug that `weighted.deferred_credit` exists to
    paper over; here the honest fix was available, so the cost is real at the
    moment it is incurred and no bot needs to know anything special.  The end
    state is identical either way -- §11.4's tokens still land in the yellow
    bank and §11.6's cards still reach the discard before any territory draw.
    """
    units, bonuses = unit_pool(p), bonus_pool(p)
    if not units:
        # Unreachable from an auction: `start_auction` only admits players
        # with `max_force > 0` and §11.2 caps a bid at the bidder's own
        # `max_force`.  Loud, because the alternative is pushing a decision
        # with no legal moves, which hangs a bot instead of naming the bug.
        raise ValueError(f"P{p.idx} must colonize {name} with no units")
    pend = {"kind": "colonize", "player": p.idx, "card": name, "bid": bid,
            "units": [], "bonuses": [], "pool": units, "bpool": bonuses}
    journal.touch(state.pending).append(pend)
    _colonize_auto(state, pend, rng)


def _colonize_auto(state, pend, rng):
    """Play out every step of the force that has only one legal answer."""
    while state.pending and state.pending[-1] is pend:
        moves = _colonize_moves(state, pend)
        if len(moves) != 1:
            return
        _colonize_step(state, pend, moves[0], rng)


def _colonize_moves(state, pend):
    """§11.3 choices, ordered so a `moves[0]` bot reproduces the old greedy.

    `unit_pool` is sorted weakest-first and `bonus_pool` cheapest-first, so
    "the mandatory unit, then bonus cards, then more units, and stop as soon
    as the bid is met" is exactly what the deleted `_build_force` did.
    """
    p = state.players[pend["player"]]
    out = []
    if pend["units"] and force_value(
            state, p, pend["units"], pend["bonuses"]) >= pend["bid"]:
        out.append(("send_done",))
    units = [("send_unit", n) for n in _distinct(pend["pool"])]
    if not pend["units"]:                 # §11.3: at least one unit, always
        return out + units
    return out + [("send_bonus", n) for n in _distinct(pend["bpool"])] + units


def _distinct(names):
    """Order-preserving de-duplication: two copies of one card are one move."""
    seen, out = set(), []
    for n in names:
        if n not in seen:
            seen.add(n)
            out.append(n)
    return out


def _colonize_move(state, pend, move, rng):
    _colonize_step(state, pend, move, rng)
    _colonize_auto(state, pend, rng)


def _colonize_step(state, pend, move, rng):
    """One commitment, paid immediately (see `colonize`), or the send."""
    p = state.players[pend["player"]]
    name = move[1] if len(move) > 1 else None
    if move[0] == "send_unit":             # §11.4 token to the YELLOW BANK
        journal.touch(pend["pool"]).remove(name)
        journal.touch(pend["units"]).append(name)
        p.techs[name].workers -= 1
        p.yellow_bank += 1
        effects.invalidate(state, p)
        return
    if move[0] == "send_bonus":            # §11.6 discarded before any draw
        journal.touch(pend["bpool"]).remove(name)
        journal.touch(pend["bonuses"]).append(name)
        journal.touch(p.hand_military).remove(name)
        economy.discard_military(state, name)
        effects.invalidate(state, p)
        return
    journal.touch(state.pending).pop()
    state.emit(f"P{p.idx} colonized {pend['card']} with force {pend['bid']}")
    gain_colony(state, p, pend["card"], rng)


def gain_colony(state, p, name, rng=None):
    """Permanent effects first, then the one-time effect (§11.5)."""
    from . import events
    db = _DB
    journal.touch(p.colonies).append(name)
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
    journal.touch(p.colonies).remove(name)
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
    journal.touch(state.pending).append(ctx)


def _defense_move(state, pend, move, rng):
    d = state.players[pend["player"]]
    if move[0] == "defend":
        name = move[1]
        journal.touch(d.hand_military).remove(name)
        journal.touch(pend)["dfn"] += defense_points(name)
        journal.touch(pend)["spent"] += 1
        economy.discard_military(state, name)
        if pend["spent"] < pend["budget"] and d.hand_military:
            return
    journal.touch(state.pending).pop()
    _finish_defense(state, pend, rng)


def _finish_defense(state, ctx, rng):
    from . import events
    events.finish_aggression(state, ctx, rng)
