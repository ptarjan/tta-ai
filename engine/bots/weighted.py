"""WeightedBot: a 1-ply bot whose entire behaviour is a JSON weight dict.

The evaluation is linear over ~57 features covering the real strategic
levers of Through the Ages, plus 10 of those features duplicated with an
"early" and a "late" copy scaled by how far the game has progressed
(78 weights total).  Everything is JSON-serializable, so hill climbing
(experiments/hillclimb.py) can mutate, checkpoint and reload a bot.

Feature groups
    economy      culture/science stock and rate, food/resource rate and
                 stock, blue bank, corruption and consumption losses,
                 population cost, workers by category
    happiness    happy margin, discontent, uprising flag
    actions      civil/military action totals and unspent actions
    military     absolute strength, strength relative to the strongest
                 rival, split into a capped lead and an uncapped deficit,
                 tactic level, colonies, pacts
    technology   summed tech levels, government level, best card level per
                 type (the "tech curve"), number of techs, special techs
    wonders      completed wonders, blue steps invested, cost remaining
    cards        civil/military hand size and summed card levels
    rivals       the best rival's culture, culture rate, science rate and
                 strength (leading is what wins, not absolute output)

`evaluate(state, idx, weights)` is the whole strategy; `WeightedBot.pick`
applies every legal move to a fast copy of the state and keeps the best.
"""
from __future__ import annotations

import random
from functools import lru_cache as _lru_cache

from .. import actions, cards as C, economy, effects
from .fastcopy import copy_state
from .trial import fresh_trial_rng

__all__ = ["DEFAULT_WEIGHTS", "WeightedBot", "features", "evaluate",
           "card_potential", "hand_potential",
           "load_weights", "save_weights"]

# ---------------------------------------------------------------- metadata

_META = None


def _meta():
    """name -> (type, level) for every card, built once."""
    global _META
    if _META is None:
        db = C.db()
        _META = {n: (c["type"], C.level(c["age"]))
                 for n, c in db.by_name.items()}
    return _META


_BEST_TYPES = ("farm", "mine", "lab", "temple", "theater", "library", "arena")

# features that additionally get an early-game and a late-game copy
PHASE_KEYS = (
    "culture", "culture_rate", "science_rate", "food_rate", "resource_rate",
    "workers", "strength_rel", "tech_levels", "wonder_progress", "hand_value",
)

# --------------------------------------------------------------- features


# A pact you have OFFERED is not a pact: the partner may refuse.  Credit it
# at this fraction of a live pact (docs/PACTS_DIAGNOSIS.md fix #2) -- without
# it a 1-ply search sees only the card leaving your hand, so `offer_pact` is
# strictly dominated by `pol_pass` in every position and can never be picked.
PACT_OFFER_CREDIT = 0.5

# Effect-block key -> feature key.  A *deferred* gain is priced with exactly
# the same weights as the real thing: a pact that pays +1 culture a turn is
# worth one `culture_rate`, not one generic "pact".  That is what lets the
# evaluator tell `Peace Treaty` (+1 culture to both) from the B side of
# `Loss of Sovereignty` (-2 culture) instead of counting cards.
_YIELD_TO_FEATURE = {
    # pact effect blocks / colony permanentEffects
    "cultureProduction": "culture_rate",
    "scienceProduction": "science_rate",
    "foodProduction": "food_rate",
    "resourceProduction": "resource_rate",
    "strength": "strength",
    "happy": "happy",
    "happiness": "happy",
    "civilActions": "civil_actions",
    "militaryActions": "military_actions",
    "yellowTokens": "yellow_bank",
    "blueTokens": "blue_free",
    # territory immediateEffects
    "culture": "culture",
    "science": "science",
    "food": "food_stock",
    "resources": "resource_stock",
    "population": "free_workers",
    "drawMilitaryCards": "hand_military",
}


_NO_GAINS = {}          # shared empty mapping: no pending decision, no gains


def _add_block(gains, block, scale, state, idx, other):
    """Accumulate one effect block's yields, scaled by how likely it is."""
    if not block:
        return
    for k, v in block.items():
        if v is True or v is False or not isinstance(v, (int, float)):
            continue
        fk = _YIELD_TO_FEATURE.get(k)
        if fk:
            gains[fk] = gains.get(fk, 0.0) + scale * v
    per = block.get("cultureProductionPerCompletedWonderOfTheOtherParty")
    if per and other is not None:
        wonders = len(state.players[other].completed_wonders)
        gains["culture_rate"] = gains.get("culture_rate", 0.0) + \
            scale * per * wonders


def deferred_credit(state, idx):
    """Payoffs the 1-ply trial state cannot show yet, priced by their yield.

    Two moves spend something now and pay off only inside *another* player's
    decision, so applying them to a trial state shows the cost and none of
    the gain (docs/PACTS_DIAGNOSIS.md):

    * ``offer_pact`` -- the pact object is created in the partner's response,
      so the mover sees only a card leaving its hand.  Credited at
      :data:`PACT_OFFER_CREDIT` of the side `idx` would take.
    * ``bid`` while rivals are still bidding -- mutates only the auction
      dict, so every bid scored EXACTLY equal to ``bid_pass`` and the
      strict-`>` tie-break always took the pass at index 0.  Credited at
      1/(1+rivals still in) of the territory's own effects.

    Returns ``(pact_offers, auction_committed, auction_bid, blocks_attack,
    gains)``, where `gains` maps feature keys to deferred amounts.
    """
    db = C.db()
    gains = {}
    offers = committed = bid_cost = blocks_attack = 0.0
    for pend in state.pending:
        kind = pend.get("kind")
        if kind == "choice":
            ctx = pend.get("ctx") or {}
            if pend.get("tag") != "pact_offer" or ctx.get("owner") != idx:
                continue
            name = ctx.get("name")
            if name not in db.by_name:
                continue
            offers += PACT_OFFER_CREDIT
            eff = db.get(name).get("effects") or {}
            if eff.get("noAttacksBetweenParties"):
                blocks_attack += PACT_OFFER_CREDIT
            pact = {"name": name, "a": ctx.get("a"), "b": ctx.get("b")}
            partner = pend.get("player")
            for block in effects._pact_blocks(pact, idx):
                _add_block(gains, block, PACT_OFFER_CREDIT, state, idx,
                           partner)
        elif kind == "auction" and pend.get("high") == idx:
            rivals = sum(1 for i in pend.get("active", ()) if i != idx)
            share = 1.0 / (1.0 + rivals)
            committed += share
            # the winner sacrifices units worth at least the bid (§11.3);
            # none of that is in the trial state either
            bid_cost += share * pend.get("bid", 0)
            card = db.by_name.get(pend.get("card"))
            if card:
                _add_block(gains, card.get("permanentEffects"), share,
                           state, idx, None)
                _add_block(gains, card.get("immediateEffects"), share,
                           state, idx, None)
    return offers, committed, bid_cost, blocks_attack, gains


def rival_context(state, idx):
    """Rival aggregates that only change when *they* move.

    Computed once per decision at the root and reused for every candidate
    move, which keeps the 1-ply search from recomputing every opponent's
    full statistics ~30 times per decision.
    """
    best_rate = best_sci = best_str = 0
    for q in state.players:
        if q.idx == idx or q.resigned:
            continue
        s = effects.compute(state, q)
        best_rate = max(best_rate, s.culture)
        best_sci = max(best_sci, s.science)
        best_str = max(best_str, s.strength)
    return {"rival_culture_rate": best_rate, "rival_science_rate": best_sci,
            "rival_strength": best_str}


def lateness(state):
    """0.0 at the start of Age A, 1.0 from Age III on."""
    lv = C.level(state.age_civil)
    return min(1.0, lv / 3.0)


def features(state, idx, ctx=None):
    """The raw feature vector from player `idx`'s point of view."""
    meta = _meta()
    db = C.db()
    p = state.players[idx]
    s = effects.compute(state, p)

    workers = prod_workers = urban_workers = unit_workers = 0
    tech_levels = 0
    special_techs = 0
    best = dict.fromkeys(_BEST_TYPES, 0)
    best_unit = 0
    for name, t in p.techs.items():
        typ, lv = meta.get(name, ("?", 0))
        if typ in _BEST_TYPES:
            if lv > best[typ]:
                best[typ] = lv
        if typ in C.UNIT_TYPES:
            if lv > best_unit:
                best_unit = lv
            unit_workers += t.workers
            tech_levels += lv
        elif typ in C.URBAN_TYPES:
            urban_workers += t.workers
            tech_levels += lv
        elif typ in C.PRODUCTION_TYPES:
            prod_workers += t.workers
            tech_levels += lv
        elif typ == "special-tech":
            special_techs += 1
            tech_levels += lv
        workers += t.workers
    tech_levels += meta.get(p.government, ("?", 0))[1]

    # pacts live in the OWNER's area but bind both parties (§5.9), so count
    # every pact idx is a party to, not just the ones sitting in front of it
    pacts = 0
    blocks_attack = 0.0
    for q in state.players:
        for pact in q.pacts:
            if idx in (pact["owner"], pact["partner"]):
                pacts += 1
                if (db.get(pact["name"]).get("effects") or {}).get(
                        "noAttacksBetweenParties"):
                    blocks_attack += 1.0
    # deferred payoffs: an offered pact and a live high bid are both real
    # positions the trial state cannot show (docs/PACTS_DIAGNOSIS.md)
    pact_offers = auction_committed = auction_bid = 0.0
    gains = _NO_GAINS
    if state.pending:
        pact_offers, auction_committed, auction_bid, pending_blocks, gains = \
            deferred_credit(state, idx)
        blocks_attack += pending_blocks
    g = gains.get

    happy_req = economy.happy_required(p.yellow_bank)
    margin = s.happy - happy_req + g("happy", 0.0)
    discontent = max(0, -margin)
    blue_have = effects.blue_available(p)
    blue_free = blue_have + g("blue_free", 0.0)
    pop_base = economy.pop_cost_base(p.yellow_bank)
    pop_cost = 8 if pop_base is None else max(0, pop_base - s.pop_food_discount)

    progress = remaining = 0
    if p.wonder is not None:
        stages = db.get(p.wonder.name)["stages"]
        progress = sum(stages[:p.wonder.steps_built])
        remaining = sum(stages[p.wonder.steps_built:])

    hand_value = sum(meta.get(n, ("?", 0))[1] + 1 for n in p.hand_civil)
    hand_mil_value = sum(meta.get(n, ("?", 0))[1] + 1 for n in p.hand_military)

    rivals = [q for q in state.players if q.idx != idx and not q.resigned]
    rival_culture = max((q.culture for q in rivals), default=0)
    rival_mean = (sum(q.culture for q in rivals) / len(rivals)) if rivals else 0
    if ctx is None:
        ctx = rival_context(state, idx)
    strength = s.strength + g("strength", 0.0)
    rel = strength - ctx["rival_strength"]

    return {
        # --- economy
        "culture": p.culture + g("culture", 0.0),
        "culture_rate": s.culture + g("culture_rate", 0.0),
        "science": p.science + g("science", 0.0),
        "science_rate": s.science + g("science_rate", 0.0),
        "food_rate": s.food + g("food_rate", 0.0),
        "resource_rate": s.resources + g("resource_rate", 0.0),
        "food_stock": p.food + g("food_stock", 0.0),
        "resource_stock": p.resources + g("resource_stock", 0.0),
        "blue_free": blue_free,
        "corruption_loss": economy.corruption(blue_have),
        "consumption": economy.consumption(p.yellow_bank),
        "pop_cost": pop_cost,
        "yellow_bank": p.yellow_bank + g("yellow_bank", 0.0),
        "free_workers": p.workers_free + g("free_workers", 0.0),
        "workers": workers,
        "prod_workers": prod_workers,
        "urban_workers": urban_workers,
        "unit_workers": unit_workers,
        # --- happiness
        "happy_margin": min(3, margin),
        "discontent": discontent,
        "uprising": 1.0 if discontent > p.workers_free else 0.0,
        # --- actions
        "civil_actions": s.civil_actions + g("civil_actions", 0.0),
        "military_actions": s.military_actions + g("military_actions", 0.0),
        "ca_left": p.civil_actions,
        "ma_left": p.military_actions,
        # --- military
        "strength": strength,
        "strength_rel": rel,
        "strength_deficit": max(0, -rel),
        "strength_lead": min(6, max(0, rel)),
        "tactic_level": meta.get(p.tactic, ("?", 0))[1] if p.tactic else 0,
        "colonies": len(getattr(p, "colonies", ()) or ()),
        "pacts": pacts + pact_offers,
        "pact_blocks_attack": blocks_attack,
        "auction_committed": auction_committed,
        "auction_bid": auction_bid,
        # --- technology
        "tech_levels": tech_levels,
        "gov_level": meta.get(p.government, ("?", 0))[1],
        "best_farm": best["farm"],
        "best_mine": best["mine"],
        "best_lab": best["lab"],
        "best_temple": best["temple"],
        "best_theater": best["theater"],
        "best_library": best["library"],
        "best_arena": best["arena"],
        "best_unit": best_unit,
        "num_techs": len(p.techs),
        "special_techs": special_techs,
        # --- wonders / leader
        "wonders": len(getattr(p, "completed_wonders", ()) or ()),
        "wonder_progress": progress,
        "wonder_remaining": remaining,
        "leader": 1.0 if p.leader else 0.0,
        # --- cards
        "hand_civil": len(p.hand_civil),
        "hand_value": hand_value,
        "hand_military": len(p.hand_military) + g("hand_military", 0.0),
        "hand_mil_value": hand_mil_value,
        # --- rivals
        "rival_culture": rival_culture,
        "rival_mean_culture": rival_mean,
        "rival_culture_rate": ctx["rival_culture_rate"],
        "rival_science_rate": ctx["rival_science_rate"],
        "rival_strength": ctx["rival_strength"],
    }


# --------------------------------------------------- card identity in hand
#
# `features()` above reduces the whole civil hand to `hand_civil` (a count)
# and `hand_value` (sum of age level + 1).  Two DIFFERENT cards therefore
# produce a byte-identical feature vector, so the 1-ply search has no basis
# to prefer a good card to a bad one and `("take", i)` scores ~0 for every
# slot in the row -- measured at -0.155 mean, and a flat -0.67 for every Age
# III card whatever it is (docs/WASTED_ACTIONS.md §4).  That blindness, not
# the `end_turn` search artifact, is why the bot leaves civil actions unspent.
#
# The fix below prices a card in hand by what it would DO if it were played,
# through the SAME weight vector, so it introduces no new hand-tuned
# constants beyond the single `hand_potential` scale.  Measured at 2p against
# the frozen champion, mirror match, seat-rotated, n=400 per row:
#
#     hand_potential   win rate           mean culture
#     0.0 (control)    50.0% +/- 6.9%     132.1 vs 132.1   <- byte-identical
#     0.125            69.6% +/- 4.5%     137.8 vs 117.0
#     0.25             67.2% +/- 4.6%     133.2 vs 110.8
#     0.5              63.2% +/- 4.7%     123.8 vs 110.4
#     1.0              63.2% +/- 4.7%     120.5 vs 107.7
#
# The term only has to BREAK THE TIE between cards, which is why the small
# scales win and the curve falls off above ~0.25.

# a card's per-turn yield once developed and staffed
_PROD_TO_FEATURE = {
    "culture": "culture_rate",
    "science": "science_rate",
    "food": "food_rate",
    "resources": "resource_rate",
    "happy": "happy_margin",
    "strength": "strength",
}

# one-shot gains and permanent modifiers printed on the card
_EFF_TO_FEATURE = {
    "gainScience": "science",
    "gainCulture": "culture",
    "gainFood": "food_stock",
    "gainResources": "resource_stock",
    "gainPopulation": "free_workers",
    "civilActions": "civil_actions",
    "militaryActions": "military_actions",
    "extraCivilActions": "civil_actions",
    "extraMilitaryActions": "military_actions",
    "cultureProduction": "culture_rate",
    "scienceProduction": "science_rate",
    "foodProduction": "food_rate",
    "resourceProduction": "resource_rate",
    "strength": "strength",
    "happy": "happy_margin",
    "yellowTokens": "yellow_bank",
    "blueTokens": "blue_free",
}


@_lru_cache(maxsize=None)
def _card_yields(name):
    """(feature, amount, clamp) triples for a card, independent of weights.

    `clamp` marks a COST: it is priced through max(0, w) because `science`
    and `resource_stock` are stock weights a hill climb is free to drive
    negative (the 4p champion reached science = -6.09).  Unclamped, a
    negative stock weight turns "this card is expensive" into "this card is a
    bargain" -- Alchemy scored +67.04 under the 4p vector against +5.86 under
    the 2p one.  Paying a cost must never read as a gain.
    """
    db = C.db()
    card = db.by_name.get(name)
    if card is None:
        return ()
    out = []
    for k, amt in (card.get("production") or {}).items():
        fk = _PROD_TO_FEATURE.get(k)
        if fk and isinstance(amt, (int, float)) and amt is not True:
            out.append((fk, float(amt), False))
    for k, amt in (card.get("effects") or {}).items():
        if amt is True or amt is False or not isinstance(amt, (int, float)):
            continue
        fk = _EFF_TO_FEATURE.get(k)
        if fk:
            out.append((fk, float(amt), False))
    tc = card.get("techCost") or 0
    if tc:
        out.append(("science", -float(tc), True))
    bc = card.get("buildCost") or 0
    if bc:
        out.append(("resource_stock", -float(bc), True))
    if card["type"] == "wonder":
        out.append(("wonders", 1.0, False))
        stages = card.get("stages") or []
        if stages:
            out.append(("resource_stock", -float(sum(stages)), True))
    return tuple(out)


def card_potential(name, w):
    """Eval-points a single card in hand would be worth if it were played."""
    total = 0.0
    for k, amt, clamp in _card_yields(name):
        wk = w.get(k, 0.0)
        if clamp and wk < 0.0:
            wk = 0.0
        if wk:
            total += wk * amt
    return total


def hand_potential(state, idx, w):
    """Summed `card_potential` over the civil hand (0.0 for an empty hand)."""
    hand = state.players[idx].hand_civil
    if not hand:
        return 0.0
    total = 0.0
    for n in hand:
        total += card_potential(n, w)
    return total


# ------------------------------------------------------------ default weights

BASE_WEIGHTS = {
    # economy
    "culture": 1.0,
    "culture_rate": 5.0,
    "science": 0.5,
    "science_rate": 4.0,
    "food_rate": 1.2,
    "resource_rate": 1.6,
    "food_stock": 0.2,
    "resource_stock": 0.3,
    "blue_free": 0.15,
    "corruption_loss": -0.9,
    "consumption": -0.5,
    "pop_cost": -0.4,
    "yellow_bank": -0.1,
    "free_workers": 0.4,
    "workers": 1.4,
    "prod_workers": 0.3,
    "urban_workers": 0.5,
    "unit_workers": 0.1,
    # happiness
    "happy_margin": 1.2,
    "discontent": -3.0,
    "uprising": -12.0,
    # actions
    "civil_actions": 2.0,
    "military_actions": 0.7,
    "ca_left": 0.05,
    "ma_left": 0.05,
    # military
    "strength": 0.35,
    "strength_rel": 0.35,
    "strength_deficit": -0.6,
    "strength_lead": 0.3,
    "tactic_level": 0.5,
    "colonies": 2.0,
    "pacts": 0.5,
    # a pact that forbids attacks between the parties (§5.4.2) buys safety no
    # other feature can see
    "pact_blocks_attack": 0.5,
    # holding the high bid of a live auction: the expected colony, discounted
    # by the rivals who can still outbid you.  Its yields are priced through
    # the economy features; these two are the colony itself and its price in
    # sacrificed units (§11.3-11.4), which the trial state cannot show.
    "auction_committed": 2.0,
    "auction_bid": -0.4,
    # technology
    "tech_levels": 1.0,
    "gov_level": 2.0,
    "best_farm": 0.5,
    "best_mine": 0.5,
    "best_lab": 0.8,
    "best_temple": 0.6,
    "best_theater": 0.6,
    "best_library": 0.5,
    "best_arena": 0.3,
    "best_unit": 0.5,
    "num_techs": 0.3,
    "special_techs": 0.8,
    # wonders / leader
    "wonders": 3.0,
    "wonder_progress": 1.0,
    "wonder_remaining": -0.3,
    "leader": 1.5,
    # cards
    "hand_civil": 0.3,
    "hand_value": 0.25,
    # scale on the identity-aware hand term (see `hand_potential` above).
    # 0.125 measured best of {0, 0.125, 0.25, 0.5, 1.0} at 2p: 69.6% +/- 4.5%
    # against the frozen champion.  NOT yet validated at 3p/4p -- at 3p the
    # term was not significant and the 4p champion's weight vector is
    # degenerate in its own right (docs/WASTED_ACTIONS.md §5, §7).
    "hand_potential": 0.125,
    "hand_military": 0.3,
    "hand_mil_value": 0.15,
    # rivals
    "rival_culture": -0.35,
    "rival_mean_culture": -0.1,
    "rival_culture_rate": -1.0,
    "rival_science_rate": -0.6,
    "rival_strength": -0.15,
    # Search bias: value of the "end turn" move itself.  Its child state has
    # already collected a production phase, which flatters it by +12.6 eval
    # points on average at 2p (+26.3 in Age IV) against alternatives worth
    # fractions of a point.
    #
    # DO NOT "FIX" THIS.  It looks like an obvious bug and it is a real
    # asymmetry, but removing it was measured, twice, two different ways, and
    # it makes the bot MUCH weaker (docs/WASTED_ACTIONS.md §6, n=400 each):
    #
    #     score end_turn on the unmoved board, eps 0.0    38.4% +/- 4.8%
    #     ... eps -0.05                                   39.8% +/- 4.8%
    #     roll every candidate to the same horizon        29.8% +/- 4.4%
    #     ... and pass MORE instead (eps +4.0)            11.0% +/- 4.3%
    #     the same fix on top of `hand_potential`         39.8% +/- 6.7%
    #
    # against a 50% null.  The flattery incidentally acts as a move-quality
    # filter: it admits only moves the evaluation can confidently price and
    # screens out the ones it cannot.  The wasted civil actions it causes are
    # a symptom of card-identity blindness, which `hand_potential` addresses
    # directly; fix that first and re-measure before touching this.
    "end_turn_bias": -3.0,
}

# early/late multipliers: the contribution of PHASE_KEYS features is
# w[k] + (1-L)*w[k_early] + L*w[k_late] with L = lateness(state).
PHASE_WEIGHTS = {
    "culture_early": -0.4, "culture_late": 1.5,
    "culture_rate_early": 2.0, "culture_rate_late": -2.0,
    "science_rate_early": 2.5, "science_rate_late": -2.5,
    "food_rate_early": 0.6, "food_rate_late": -0.6,
    "resource_rate_early": 0.5, "resource_rate_late": -0.4,
    "workers_early": 0.8, "workers_late": -0.6,
    "strength_rel_early": -0.1, "strength_rel_late": 0.5,
    "tech_levels_early": 0.5, "tech_levels_late": -0.4,
    "wonder_progress_early": 0.3, "wonder_progress_late": -0.3,
    "hand_value_early": 0.2, "hand_value_late": -0.2,
}

DEFAULT_WEIGHTS = dict(BASE_WEIGHTS)
DEFAULT_WEIGHTS.update(PHASE_WEIGHTS)


# ------------------------------------------------------------- evaluation

def evaluate(state, idx, weights=None, ctx=None, f=None):
    w = weights if weights is not None else DEFAULT_WEIGHTS
    if f is None:
        f = features(state, idx, ctx)
    total = 0.0
    get = w.get
    for k, v in f.items():
        wk = get(k)
        if wk:
            total += wk * v
    late = lateness(state)
    early = 1.0 - late
    for k in PHASE_KEYS:
        v = f[k]
        if not v:
            continue
        we = get(k + "_early")
        if we:
            total += we * early * v
        wl = get(k + "_late")
        if wl:
            total += wl * late * v
    # identity-aware hand term: what the cards actually in hand would be worth
    # if played.  Deliberately NOT folded into `features()` -- it is priced
    # through `w` itself, so it is not a linear feature and must not pick up
    # the early/late phase multipliers above (that is the form that was
    # measured; see the block above `_card_yields`).
    hp = get("hand_potential")
    if hp:
        total += hp * hand_potential(state, idx, w)
    return total


# ------------------------------------------------------------------- bot

class WeightedBot:
    """1-ply search under a fully parameterized linear evaluation."""

    name = "weighted"

    def __init__(self, weights=None, rng=None, seed=None, name=None):
        self.weights = dict(weights) if weights else dict(DEFAULT_WEIGHTS)
        self.rng = rng or random.Random(seed)
        if name:
            self.name = name

    # -- harness adapters
    def choose(self, state, moves, rng=None):
        return self.pick(state, moves)

    def __call__(self, state):
        return self.pick(state, actions.legal_moves(state))

    def pick(self, state, moves):
        if len(moves) == 1:
            return moves[0]
        # Score for whoever actually owns the move. On a pending decision that
        # is NOT the turn player -- pact accept/refuse is always one of these,
        # and 10/16 auction decisions were measured to be -- `state.current`
        # made us maximise a RIVAL's position (docs/PACTS_DIAGNOSIS.md).
        idx = state.decider()
        try:
            ctx = rival_context(state, idx)
        except Exception:
            ctx = {"rival_culture_rate": 0, "rival_science_rate": 0,
                   "rival_strength": 0}
        w = self.weights
        end_bias = w.get("end_turn_bias", 0.0)
        best, best_val = None, None
        for mv in moves:
            trial = copy_state(state)
            try:
                # `fresh_trial_rng()` is a reused Random(0) rather than a new
                # one per candidate; an undrawn Mersenne Twister is byte-
                # identical to a fresh Random(0), so every candidate still sees
                # exactly the Random(0) stream from its start.  docs/PYPY.md
                # 5a measured the per-candidate construction at ~10.8% of a
                # profile -- and 8.1 measured the A/B and found the profiler
                # had overstated it, so this one is measured in 9.16, not
                # assumed.
                actions.apply(trial, mv, fresh_trial_rng())
                val = evaluate(trial, idx, w, ctx)
            except Exception:
                # The engine grows new move types and new state fields under
                # us; an unscorable candidate is skipped, never fatal.  If
                # every candidate is unscorable we still return a legal move.
                continue
            if mv[0] == "end_turn":
                val += end_bias
            if best_val is None or val > best_val:
                best, best_val = mv, val
        if best is None:
            return self.rng.choice(moves)
        return best


# ------------------------------------------------------------------- io

def load_weights(path):
    import json
    with open(path) as fh:
        d = json.load(fh)
    w = dict(DEFAULT_WEIGHTS)
    w.update(d.get("weights", d))
    return w


def save_weights(path, weights, **extra):
    import json
    import os
    d = {"weights": weights}
    d.update(extra)
    tmp = path + ".tmp"
    os.makedirs(os.path.dirname(os.path.abspath(path)), exist_ok=True)
    with open(tmp, "w") as fh:
        json.dump(d, fh, indent=1, sort_keys=True)
    os.replace(tmp, path)
