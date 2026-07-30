"""WeightedBot: a 1-ply bot whose entire behaviour is a JSON weight dict.

The evaluation is linear over 77 features covering the real strategic
levers of Through the Ages, plus 10 of those features duplicated with an
"early" and a "late" copy scaled by how far the game has progressed, plus a
handful of scales on non-linear terms priced through the weights themselves
(`hand_potential`, `row_urgency`, `card_rate_credit`, `card_board_credit`,
...) -- 105 weights total.  Everything is JSON-serializable, so hill climbing
(experiments/hillclimb.py) can mutate, checkpoint and reload a bot.

A weight absent from a loaded vector is filled in from `DEFAULT_WEIGHTS`, and
almost every weight added since the champions were frozen defaults to 0.0 for
exactly that reason: an old vector keeps playing the policy it was trained on.

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
    wonders      completed wonders, blue steps invested, cost remaining,
                 and whether the one in progress can actually be finished
                 in the resources and rounds left (docs/CARD_BLINDNESS.md)
    board        what a leader or a government is worth on THIS board, by
                 swapping it in and diffing `effects.compute` rather than
                 reading numbers off the card (engine/bots/board_yields.py,
                 docs/CARD_PRICING_LEADERS.md)
    cards        civil/military hand size and summed card levels
    rivals       the best rival's culture, culture rate, science rate and
                 strength (leading is what wins, not absolute output)

`evaluate(state, idx, weights)` is the whole strategy; `WeightedBot.pick`
applies every legal move to a fast copy of the state and keeps the best.
"""
from __future__ import annotations

import random
from functools import lru_cache as _lru_cache

from .. import actions, cards as C, economy, effects, journal
from . import board_yields as _BY
from .fastcopy import copy_state
from .trial import USE_JOURNAL, fresh_trial_rng

__all__ = ["DEFAULT_WEIGHTS", "WeightedBot", "features", "evaluate",
           "card_potential", "hand_potential", "wonder_potential",
           "rival_hand_potential",
           "row_pressure", "load_weights", "save_weights"]

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

# `wonder_turns_to_finish` is a ratio, so it blows up when resource production
# is near zero.  20 turns is already past "never" for a 20-round game.
_TURNS_CAP = 20.0

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


# ------------------------------------------------- Age III scoring events
#
# `_h_prepare_event` grants `+level_of(name)` culture on the spot and puts the
# card in `state.future_events`.  Those three points, plus one fewer
# `hand_mil_value`, are the ENTIRE visible consequence of a plant: this file
# contains no other reference to `future_events`, `current_events` or
# `seeded_by`, and `docs/INFORMATION_AUDIT.md` confirms that deleting all three
# moves no feature.
#
# For the fifteen Age III "Impact of ..." events that omits the whole card.
# Each awards `events.scoring_culture` to EVERY player -- 5/4/3/2 culture per
# completed wonder, 2 per content worker above ten, a 10/0 ranking on strength
# -- either when it is revealed or, if it never is, at game end through
# `events.evaluate_final_events`.  Measured at 2p under the frozen champion
# (`tools/event_plants.py`, 20 games): the bot seeds 8.75 of them per game and
# they swing 12.9 culture of final margin, but the margin its own choices buy
# it averages +0.62 (sd 3.84, n=175).  It plants constantly and picks which
# card to plant at random, because nothing it can see distinguishes them.
#
# This is deliberately a MARGIN and not a pair of own/rival terms.  An event
# that pays me 8 and my rival 14 is a bad plant, and a feature that only knew
# the 8 would rate it a good one -- which is exactly today's failure.  One
# coordinate is also far likelier to be found by the hill climb than two
# (docs/CARD_BLINDNESS.md 5.1 on dead coordinates).
#
# It is NOT double counting against the `margin_share` objective's double pay
# for a stolen point: nothing here is stolen.  `evaluate_final_events` awards
# culture to everyone out of the bank, so the differenced quantity below is
# the only part of it that can move a margin at all.
#
# The forecast is the current board, and the payout is the board at reveal or
# at game end.  That is an approximation and the honest one available: it is
# the same estimate a human makes when deciding what to seed, and the
# alternative is a model of one's own future development that the bot has not
# got.  It also means the feature is not only about planting -- with "Impact
# of Wonders" in play, finishing a wonder raises it too, which is a second and
# correct source of gradient.
_SCORING_MARGIN_CAP = 60.0


def event_scoring_margin(state, idx):
    """Final-scoring culture the pending Age III events owe me, less the best
    rival's, clamped to +/-`_SCORING_MARGIN_CAP`.

    Calls the engine's own `events.final_event_culture`, so the fifteen
    scoring formulas are never restated here and cannot drift from the rules;
    `tests/test_event_scoring.py` pins the agreement.

    Zero once `state.game_over`: `game._finish_game` has by then already paid
    these events into `p.culture`, and the decks still hold the names, so
    counting them again would double the endgame.
    """
    if state.game_over or not state.has_military:
        return 0.0
    from .. import events as _events
    try:
        if not _events.pending_final_events(state):
            return 0.0
        owed = _events.final_event_culture(state)
    except Exception:                                      # noqa: BLE001
        return 0.0
    rivals = [q.idx for q in state.players
              if q.idx != idx and not q.resigned]
    if not rivals:
        return 0.0
    margin = owed[idx] - max(owed[i] for i in rivals)
    return max(-_SCORING_MARGIN_CAP, min(_SCORING_MARGIN_CAP, float(margin)))


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


class _RivalView:
    """An immutable snapshot of the parts of a rival's board `can_take` reads.

    `rival_context` is computed ONCE per decision, at the root, and is handed
    unchanged to every candidate -- including on the journalled path, where
    the "root state" is the very object the candidates mutate and un-mutate.
    That is why `rival_context` has always returned plain numbers and never
    references into the state (see its docstring).  A view keeps that
    invariant: every field is copied out into an immutable container at
    build time, so nothing here can be aliased to a rollback.

    The attribute names are exactly the ones `actions._can_take_gated` reads
    off a player, so the real legality rules are reused rather than restated.
    """
    __slots__ = ("idx", "wonder", "taken_leader_ages", "hand_civil",
                 "techs", "government")

    def __init__(self, q):
        self.idx = q.idx
        # `_can_take_gated` only ever asks `p.wonder is None`
        self.wonder = None if q.wonder is None else True
        self.taken_leader_ages = tuple(q.taken_leader_ages)
        self.hand_civil = frozenset(q.hand_civil)
        self.techs = frozenset(q.techs)          # `name in p.techs`
        self.government = q.government


def root_row_budget(state):
    """The card-row names VISIBLE AT THE ROOT of a search, IN ROW ORDER.

    The one piece of hidden information the evaluator can actually read.
    `_replenish` runs at the start of every player's turn, so any trial that
    crosses a turn boundary -- every `end_turn` candidate, and every deeper
    ply of a search -- deals the REAL next civil cards into `state.card_row`,
    and `row_pressure` below would then price cards the mover cannot know.
    Measured at 94.2% of `end_turn` candidates at 2p and 92.0% at 3p, and
    once the 3p arm fitted real row weights it began changing the chosen move
    (docs/INFORMATION_AUDIT.md 6.1).

    A SEQUENCE, not a set and not a bare multiset.  Both weaker forms leak,
    for the same reason and by different amounts:

    * A set would price a freshly dealt SECOND copy of a card the root row
      held once -- the civil decks contain duplicate names.
    * A multiset with a per-name count closes that, but still leaks through
      the cards that LEAVE the row.  `_replenish` discards the leftmost
      `_sweep_count` slots, so a root-row card that gets SWEPT never spends
      its own budget entry, and a freshly dealt card with the same name then
      spends it instead.  That donor hole is the ENTIRE residual left open by
      docs/INFORMATION_AUDIT.md 6.2: 11 of 1583 `end_turn` candidates at 3p,
      and it is `row_bargain_forgone` in every one of them -- the military
      hand named as the suspect there varies in 0 of 1583 (see 6.4).

    Order closes the swept case because the row is a QUEUE and every write to
    it is order-preserving: `_replenish` discards from the left, compacts the
    survivors keeping their relative order, and `_deal` fills the empty slots
    left to right -- as does `interact._finish_take_row`, which compacts first
    for exactly that reason.  So in any reachable state **every dealt card
    sits strictly to the right of every surviving root card**, and a
    forward-only cursor over this sequence (`row_pressure` below) never
    rewinds onto the name of a card that was swept off the left.

    It is a bound, not an identity, and the gap is documented rather than
    papered over.  A root card that left the row from a slot to the RIGHT of
    the survivors -- one a rival TOOK -- has not been passed by the cursor
    yet, so a later dealt card sharing its name is still priced
    (`test_known_hole_a_taken_card_can_still_lend_its_name`).  That needs a
    take AND a turn boundary in the same trial, so it is unreachable at 1 ply
    and invisible to `tools/leak_impact.py`; closing it exactly needs
    provenance on the row slots, not a cleverer reading of the names, because
    names alone cannot distinguish a survivor from its own duplicate
    (INFORMATION_AUDIT 6.4).

    Cards that merely SLID LEFT are unaffected -- the slide is public
    arithmetic every player can do, and the card keeps its name -- so they are
    still priced, at their new (cheaper) slot.  That is the whole point of
    evaluating the row on the post-move state, and it survives; so does a
    survivor sitting to the right of a hole a take left behind.
    """
    names = []
    for c in state.card_row:
        if c is None:
            continue
        names.append(c["name"] if isinstance(c, dict)
                     else getattr(c, "name", c))
    return tuple(names)


def rival_context(state, idx, root_row=None):
    """Rival aggregates that only change when *they* move.

    Computed once per decision at the root and reused for every candidate
    move, which keeps the 1-ply search from recomputing every opponent's
    full statistics ~30 times per decision.

    `rival_views` carries, per live rival, an immutable `_RivalView` plus the
    `actions._take_gate` tuple for that rival budgeted at their FULL civil
    action allotment -- i.e. what they will be able to reach on their next
    turn, not what is left of this one.  It is built here for the same reason
    everything else here is: `effects.compute` is the expensive part and the
    trial states thrown at `evaluate` have no stats cache.

    `root_row` must be passed by any caller REBUILDING this context on a trial
    state mid-search (the rivals moved, so the aggregates went stale).  Without
    it the row budget would be recomputed from the trial's own row, which is
    precisely the leaked information the budget exists to hide, and the rebuild
    would silently re-open the leak for exactly the deep nodes that have one.
    """
    best_rate = best_sci = best_str = 0
    views = []
    for q in state.players:
        if q.idx == idx or q.resigned:
            continue
        s = effects.compute(state, q)
        best_rate = max(best_rate, s.culture)
        best_sci = max(best_sci, s.science)
        best_str = max(best_str, s.strength)
        gate = (s.civil_actions,
                q.hand_size("civil") >= s.civil_actions + s.civil_hand_limit,
                0 if q.leader == "Michelangelo"
                else len(q.completed_wonders) + q.destroyed_wonders,
                1 if q.leader == "Hammurabi" else 0)
        views.append((_RivalView(q), gate))
    return {"rival_culture_rate": best_rate, "rival_science_rate": best_sci,
            "rival_strength": best_str, "rival_views": tuple(views),
            "root_row": root_row_budget(state) if root_row is None
            else root_row}


# ------------------------------------------------------- the game horizon
#
# `lateness()` scales every early/late phase weight, and what those weights are
# mostly pricing is RATES: a +1 culture rate is worth one culture point for each
# of your remaining turns and nothing at all on the last one.  It used to be
#
#     min(1.0, C.level(state.age_civil) / 3.0)
#
# -- a four-step function of the civil deck's age, SATURATED AT 1.0 FROM AGE III
# ON.  Age III and Age IV therefore priced a production rate identically, which
# is exactly the stretch where the true value collapses: measured over 46
# WeightedBot self-play games, a decision in Age III has 6.2 rounds left on
# average and a decision in Age IV has 2.0.  docs/CULTURE_GAP.md section 4
# measured the consequence -- the 4p champion pays 35.6 culture points for a +1
# culture rate on the last turn of the game, and buys its culture engine several
# rounds after CultureBot does.
#
# WHAT THE ENGINE ACTUALLY KNOWS.  There is no fixed turn count.  The game ends
# when the Age III civil deck runs out (RULES_SPEC 12.2/12.3): Age IV begins,
# `_set_last_round` fixes `final_round_end` at this round or the next, and play
# stops.  So:
#
#   * once `state.final_round_end` is set the remaining rounds are EXACT;
#   * before that, the number of civil cards still to be dealt is exact -- the
#     current deck plus every later age's deck, whose sizes are fixed by the
#     card data and the live player count -- and the only estimated quantity is
#     the RATE at which the row eats them.  `game.SWEEP[n]` cards are swept and
#     redealt per player-turn, which is exact; on top of that players take cards
#     off the row, which is policy-dependent and is the part that is a guess.
#
# CARDS_PER_ROUND below is the measured total (game.SWEEP[n]*n is 6 / 6 / 4 of
# it; the remainder is cards taken off the row).  Against ground truth over
# those 46 games (2p/3p/4p, 1078 pre-Age-IV decisions) the resulting estimate
# has a residual sd of 0.68 / 1.00 / 1.13 rounds and a bias under a quarter of
# a round; the age bucket it replaces cannot do better than 2.7 rounds even if
# you hand it the per-age mean.  It is at its best where it matters -- sd 0.86
# rounds in Age III at 4p, and exact in Age IV -- and at its worst in Age A,
# which is fine: Age A is two rounds long and no rate horizon is decided there.
# It is calibrated on WeightedBot self-play; a much more card-hungry policy
# would drain the row faster and this would then run long.
CARDS_PER_ROUND = {2: 6.29, 3: 6.73, 4: 5.71}
AGE_IV_ROUNDS = 2.0      # 12.3: Age IV is this round or the next, then it ends

# Cards left in the decks of every age AFTER the given one, by live player
# count.  Built once; `C.db().civil_deck` is far too slow for the search loop.
_TAIL = {}


def _tail(n, age):
    key = (n, age)
    out = _TAIL.get(key)
    if out is None:
        db = C.db()
        lv = C.level(age)
        out = sum(len(db.civil_deck(a, n)) for a in C.AGES[lv + 1:]
                  if a != "IV")
        _TAIL[key] = out
    return out


def _live(state):
    """`game.live_count` without importing game (circular).  RULES_SPEC 13."""
    n = 0
    for p in state.players:
        if not p.resigned:
            n += 1
    return 2 if n < 2 else (4 if n > 4 else n)


def rounds_left(state, n=None):
    """Estimated rounds still to play, including the one in progress.

    Exact once Age IV has begun; before that it is cards-still-to-deal divided
    by the measured deal rate.  Never returns less than 1.0.
    """
    fre = state.final_round_end
    if fre is not None:
        return max(1.0, float(fre - state.round + 1))
    if n is None:
        n = _live(state)
    cards = len(state.civil_deck) + _tail(n, state.age_civil)
    return cards / CARDS_PER_ROUND[n] + AGE_IV_ROUNDS


# The affine map from `rounds_left` to L.  The phase blend is
# `w[k] + (1-L)*w[k_early] + L*w[k_late]`, so an AFFINE change of L is pure
# GAUGE: adding c to both phase weights and subtracting c from the base is the
# identical policy, and any (scale, offset) applied to L can be absorbed the
# same way.  Only the NON-affine part of this change moves a decision -- and
# that part is the whole fix, because no affine function of rounds_left can be
# flat inside an age.
#
# The gauge is therefore free, and it is spent on not breaking the three
# already-trained champions: these constants make the new L the least-squares
# best linear-in-rounds_left approximation of the OLD age-bucket L, fitted per
# player count on the same measured decisions (residual sd 0.10 in L; per-age
# means land within 0.05 of the old 0 / 1/3 / 2/3 / 1 / 1).  Per player count
# because a 4p game runs ~29 rounds against ~23 at 2p/3p, and each arm's
# champion was trained against its own arm's schedule.  `_L_ONE` came out at
# 5.0 / 5.2 / 5.1 independently and is rounded to a single 5.0: the old
# schedule's "late" was, in effect, "about five rounds from the end".
_L_ZERO = {2: 27.1, 3: 28.7, 4: 36.1}   # rounds left at which L = 0
_L_ONE = 5.0                            # rounds left at which L = 1


def lateness(state):
    """0.0 with a whole game ahead, 1.0 with 5 or fewer rounds left, monotone
    in estimated rounds remaining in between.

    CLAMPED TO [0, 1], and that clamp is load-bearing.  The unclamped line
    reaches ~1.1 with two rounds left, which makes `1 - L` negative and FLIPS
    THE SIGN of every `_early` term.  Measured, n=400 head-to-head (see
    docs/CULTURE_GAP.md section 8d): unclamped, the 4p champion drops to 19.9%
    against a 25% null and the 3p champion to 13.6% against 33.3%.  The
    mechanism on the 4p champion is exact -- its `culture_early` is 8.792, so
    at `1 - L = -0.096` its own culture is priced at
    `1.000 - 0.096*8.792 = 0.156` instead of the frozen 1.000, i.e. it stops
    caring about the score in the last two rounds.  `1 - L` outside [0, 1] is
    an extrapolation beyond anything the search has ever been scored on, and
    linear evaluators do not extrapolate.
    """
    n = _live(state)
    z = _L_ZERO[n]
    lv = (z - rounds_left(state, n)) / (z - _L_ONE)
    if lv <= 0.0:
        return 0.0
    return lv if lv < 1.0 else 1.0


def lateness_by_age(state):
    """The pre-fix schedule: 0.0 in Age A, 1.0 from Age III on.

    Kept only so `horizon_age` in a weight file can select it for an A/B.
    """
    return min(1.0, C.level(state.age_civil) / 3.0)


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

    # --- wonders: how far in, and whether it can possibly be finished.
    #
    # `wonder_remaining` (resources still owed) already existed and is flat in
    # the economy that has to pay them.  The three terms added here are the
    # ones that separate "6 resources at 5/turn on round 4" from "12 resources
    # at 4/turn on round 15", which the old feature set scored identically per
    # resource.  Across 120 logged games the bot started Pyramids 13 times and
    # finished it 0 times, and went 0-for-58 on the three 12-resource Age II
    # wonders (docs/HEURISTICS.md, "Wonders, by age"); nothing in the
    # evaluation could see that, because starting a wonder it will never
    # complete looked exactly like starting one it will.
    #
    # All three are 0.0 with no wonder in progress and drop back to 0.0 the
    # instant it completes, so a negative weight on any of them prices
    # STARTING (and stalling), and completion is what removes the penalty.
    # That is the shape the sources ask for -- "start a wonder by round 12 or
    # do not start it" -- expressed as something the league can tune rather
    # than a hard rule.
    progress = remaining = 0
    stages_left = turns_to_finish = overrun = 0.0
    if p.wonder is not None:
        stages = db.get(p.wonder.name)["stages"]
        built = p.wonder.steps_built
        progress = sum(stages[:built])
        remaining = sum(stages[built:])
        if remaining > 0:
            stages_left = float(len(stages) - built)
            # turns of the player's WHOLE resource output the wonder still
            # owes, net of what is already banked.  Scale-free, so it means
            # the same thing to an Age A economy and an Age III one.
            owed = remaining - p.resources
            if owed > 0:
                turns_to_finish = min(
                    _TURNS_CAP, owed / max(1.0, s.resources))
                # ...and the part of that the game will not last long enough
                # to pay.  This is the 0-for-58 detector.
                overrun = max(0.0, turns_to_finish - rounds_left(state))

    hand_value = sum(meta.get(n, ("?", 0))[1] + 1 for n in p.hand_civil)
    hand_mil_value = sum(meta.get(n, ("?", 0))[1] + 1 for n in p.hand_military)

    rivals = [q for q in state.players if q.idx != idx and not q.resigned]
    rival_culture = max((q.culture for q in rivals), default=0)
    rival_mean = (sum(q.culture for q in rivals) / len(rivals)) if rivals else 0
    # public rival board facts the evaluator was blind to (GAP 3).  `max`
    # everywhere, so each term means the same thing at 2p, 3p and 4p: the
    # most action-rich rival is the one who can reach deepest into the row,
    # the fullest civil hand is the one closest to being unable to take
    # anything at all (§2.5), and completed wonders are both score and the
    # +1/wonder take surcharge they will pay for the next one.
    # `hand_size`, not `len(hand_civil)`: a hand of three cards we cannot name
    # is a hand of three cards.  Identical in self-play (`hidden_civil` is
    # always 0 there); the difference is the app harness, where the count is
    # public and the identities are not (docs/APP_HARNESS.md section 2).
    rival_free_ca = max((q.civil_actions for q in rivals), default=0)
    rival_hand_civil = max((q.hand_size("civil") for q in rivals), default=0)
    rival_wonders = max((len(q.completed_wonders) for q in rivals), default=0)
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
        # civil actions spent THIS turn reaching into the row (GAP 1).  A
        # separate channel from `ca_left` on purpose: row depth used to reach
        # the evaluation only through `ca_left`, whose 3p champion weight is
        # -0.0974, so paying 3 CA instead of 1 for the identical card scored
        # as a GAIN of 0.195 (docs/INFORMATION_AUDIT.md section 0).
        "take_cost_paid": p.ca_spent_taking,
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
        # finish discipline (all 0.0 with nothing in progress)
        "wonder_stages_left": stages_left,
        "wonder_turns_to_finish": turns_to_finish,
        "wonder_overrun": overrun,
        "wonder_stages_per_action": float(s.wonder_stages - 1),
        "leader": 1.0 if p.leader else 0.0,
        # --- board side of the card-pricing keys added with
        # docs/CARD_BLINDNESS.md.  Same key on both sides, the way
        # `civil_actions` already is: `_card_yields` prices the card in hand
        # and this prices the effect once it is on the board.
        "hand_limit": float(s.civil_hand_limit + s.military_hand_limit),
        "colonize_bonus": float(s.colonize),
        "build_discount": float(sum(s.build_discount.values())
                                if s.build_discount else 0),
        # --- cards
        "hand_civil": len(p.hand_civil),
        "hand_value": hand_value,
        "hand_military": len(p.hand_military) + g("hand_military", 0.0),
        "hand_mil_value": hand_mil_value,
        # --- the Age III scoring events already in play (see the block above
        # `event_scoring_margin`).  0.0 whenever none are pending, which is
        # every position before the first Age III event is seeded.
        "event_scoring_margin": event_scoring_margin(state, idx),
        # --- rivals
        "rival_culture": rival_culture,
        "rival_mean_culture": rival_mean,
        "rival_culture_rate": ctx["rival_culture_rate"],
        "rival_science_rate": ctx["rival_science_rate"],
        "rival_strength": ctx["rival_strength"],
        "rival_free_ca": rival_free_ca,
        "rival_hand_civil": rival_hand_civil,
        "rival_wonders": rival_wonders,
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
#
# THE OMISSION THIS MAP EXISTED WITH FOR MOST OF THE PROJECT: `culture` and
# `science` were absent.  `engine/effects.py:FLAT_KEYS` treats an effect-block
# `culture` exactly like `cultureProduction` -- it lands in `Stats.culture`,
# i.e. it is culture PER TURN -- and ten cards use the short spelling,
# including Eiffel Tower (4), Taj Mahal (3), St. Peter's Basilica (2),
# Kremlin (2), Library of Alexandria, Universitas Carolina, Great Wall,
# Hanging Gardens, Joan of Arc and Mahatma Gandhi.  Two more carry a dropped
# `science` (Library of Alexandria 1, Universitas Carolina 2).  With those two
# keys missing, seven of the sixteen wonders priced out at nothing beyond
# "it is a wonder" -- including the two the tournament data likes best.  See
# docs/CARD_BLINDNESS.md.
#
# They map to the RATE features, not the stock ones, because that is what the
# engine does with them.  `_YIELD_TO_FEATURE` above spells the same two words
# as stock gains and is CORRECT to: there they come from a territory's
# `immediateEffects`, which really are one-shot.
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
    "culture": "culture_rate",
    "science": "science_rate",
    "cultureProduction": "culture_rate",
    "scienceProduction": "science_rate",
    "foodProduction": "food_rate",
    "resourceProduction": "resource_rate",
    "strength": "strength",
    "happy": "happy_margin",
    "yellowTokens": "yellow_bank",
    "blueTokens": "blue_free",
    # --- keys whose amount is numeric but which no pre-existing feature
    # matched.  Each gets its own weight, defaulting to 0.0 (see
    # BASE_WEIGHTS), so adding them is inert for every trained vector and the
    # league decides what they are worth.
    "civilHandLimit": "hand_limit",
    "militaryHandLimit": "hand_limit",
    "colonizeBonus": "colonize_bonus",
    "resourceDiscount": "resource_discount",
    # The four Patriotisms: "build or upgrade military units; pay N fewer
    # resources", one shot, this turn.  Ring-fenced, so it is not worth N
    # resources -- `restricted_resources` is a separate 0.0 weight rather
    # than a discount on `resource_stock` precisely so the league can price
    # the ring fence instead of a constant guessing at it.
    "resourcesForMilitaryUnits": "restricted_resources",
}

# Keys offering a CHOICE of yields rather than a sum, handled by
# `_card_choices`; listed here so the coverage test can see them as priced.
_EFF_CHOICE = {
    # Reserves I/II/III: "gain N food OR N resources"
    "gainFoodOrResources": "food_stock|resource_stock",
}

# A territory's `immediateEffects` / `permanentEffects`, keyed the same way
# `deferred_credit` keys them.  DERIVED from `_YIELD_TO_FEATURE` rather than
# retyped, so the auction path and the hand path cannot drift apart -- that
# drift is the whole failure mode this file keeps hitting.
#
# One substitution, and it is required, not cosmetic.  `_YIELD_TO_FEATURE`
# sends `happy`/`happiness` to the string `"happy"`, which is NOT a weight:
# `features()` resolves it by hand into `happy_margin` (`margin = s.happy -
# happy_req + g("happy", 0.0)`).  `card_potential` has no such step -- it does
# a bare `w.get(k, 0.0)` -- so leaving it as `"happy"` would silently drop
# Historic Territory's whole permanent effect, which is exactly the bug class
# above one level down.  `tests/test_card_pricing.py` asserts both halves:
# that this map agrees with `_YIELD_TO_FEATURE` everywhere else, and that
# every value it names is a real key of `DEFAULT_WEIGHTS`.
_TERR_TO_FEATURE = dict(_YIELD_TO_FEATURE,
                        happy="happy_margin", happiness="happy_margin")

# A military unit's TOP-LEVEL `strength`, the per-worker yield `production`
# never held.  A one-entry registry rather than a literal so `--legacy` can
# clear it (see `_card_yields`), and so the top-level-field walk in
# `tests/test_card_pricing.py` has something to point at.
_UNIT_TO_FEATURE = {"strength": "strength"}

# Effect keys `_card_yields` prices but that need more than a table lookup:
# the value is a dict, an offset, or a bare presence flag.  Handled in
# `_card_yields`; listed here so the coverage test can see them as priced.
_EFF_SPECIAL = {
    # {age: resources off}, e.g. Masonry {"I": 1, "II": 1, "III": 1}.
    # Reduced by MAX, not by sum -- the per-age entries are ALTERNATIVES, one
    # of which applies to any given build, not a stack.  See `_card_yields`.
    "buildDiscount": "build_discount",
    # 2 means "two stages per action", i.e. a bonus of one
    "wonderStagesPerAction": "wonder_stages_per_action",
    # a string naming the action you get free ("build_or_upgrade_farm_or_mine").
    # 18 cards.  Priced as a presence flag: WHICH free action it is varies, the
    # fact that there is one does not.
    "freeCivilAction": "free_civil_action",
}

# ---------------------------------------------------- the documented gap
#
# Everything else printed on a card that `_card_yields` does NOT price, with
# the reason.  `tests/test_card_pricing.py` walks all 236 cards and fails if a
# key in any `production`/`effects` block is neither mapped above nor listed
# here, and also fails if a key listed here no longer appears on any card.
#
# The point is not that this set is empty -- it cannot be, most of it is
# genuinely unpriceable by a board-independent per-card table.  The point is
# that its SIZE IS VISIBLE.  `culture` sat in this gap silently for the whole
# project and nobody could see it, which is the failure this test prevents.
DELIBERATELY_UNPRICED = {}


def _unpriced(reason, *keys):
    for k in keys:
        DELIBERATELY_UNPRICED[k] = reason


# 1. The coefficient is numeric but the YIELD is coefficient x a board count.
#    `_card_yields` is `lru_cache`d on the card name alone and has no state,
#    so it cannot evaluate any of these.
#
#    THIS BUCKET IS NEARLY EMPTY NOW.  `engine/bots/board_yields.py` prices
#    the board-scaled keys by swapping the card in and asking
#    `engine.effects.compute` what changed, so everything on a leader or a
#    government moved to `board_yields.BOARD_PRICED`.  What is left is
#    carried by wonders and by an event, where a swap is the wrong question:
#    a wonder accumulates rather than replaces.
_unpriced(
    "board-scaled: numeric coefficient times a board count _card_yields "
    "cannot see, on a card no swap diff reaches",
    "gainCulturePerLevelOfRemovedCard",
)

# 2. The value is a formula in text, or a bare True meaning "read the card".
#    There is no number to multiply a weight by at all.
_unpriced(
    "text effect: the value is a formula or a bare flag, not a scalar",
    "doublesTacticBonusOfOneArmy", "infantryCountsAsCavalryForTactics",
)

# 2a. Bill Gates' second clause, which is neither production nor a trigger:
#     "when Bill Gates leaves play or the game ends, gain culture equal to
#     that extra resource production".  The resource production itself IS
#     priced (`resourcesPerLabEqualToLevel`, through the swap diff); this is
#     a one-off score at an unknown future time, worth the same number again
#     but only once, and only if he is still there at the end.  Pricing it
#     needs a model of when a leader gets replaced, which nothing here has.
_unpriced(
    "end-of-life payout: needs a model of when the leader leaves play",
    "cultureOnLeaveEqualToLabResourceProduction",
)

# 3. A rule change with no scalar value: it makes something legal, illegal or
#    cheaper in a way only the rules engine can express.
_unpriced(
    "rule change: alters what is legal, not what is produced",
    "noAttacksBetweenParties", "cancelledIfPartiesAttackEachOther",
    "opponentsPayDoubleMilitaryActionsToAttackYou",
    "wonderTakeNoExtraCivilActions", "revolutionUsesMilitaryActionsInstead",
    "oncePerGameTwoPoliticalActions", "removeAsPoliticalActionForYellowToken",
    "removeAsPoliticalActionFreeColonize", "peekTopEventCardInPolitics",
    "militaryActionCombinedPopIncreaseAndUnitBuild",
    "civilActionUpgradeUrbanBuildingToTheater",
    "militaryActionAsCivilPerTurn", "civilActionBackOnTechDevelop",
    "colonizeDiscardUpTo2MilitaryCardsForBonus",
    "colonyPermanentBonusTransfers", "colonyImmediateBonusApplies",
    "orTakesSpecialTechnologiesOfSameTotalScienceCost", "removeFromGame",
    "onReplacePutUnderCompletedWonderHappy",
    "libraryDiscountsIfTheater",
)

# 4. A trigger: it pays out when some later action happens, so the amount on
#    the card is a rate per event, not a yield.  Several of these are large
#    (Einstein's 3 culture per technology, Newton's action refund); pricing
#    them needs a model of how often the trigger fires.
#
#    These are the honest remainder of the leader work.  A swap diff cannot
#    reach them, because `effects.compute` builds the production phase and a
#    trigger is not in the production phase.  The measurement that would
#    close this bucket -- how often each trigger actually fires per round in
#    real games -- is a separate piece of work and is named as such in
#    docs/CARD_PRICING_LEADERS.md rather than guessed at here.
_unpriced(
    "trigger: pays per future event, not on play; needs a measured "
    "firing rate, not a guessed one",
    "scienceOnTechCardTake", "resourceOnTechDevelop", "cultureOnTechDevelop",
    "resourceOnMilitaryUnitBuildOrUpgrade", "cultureOnRevolution",
    "leaderTakeCivilActionDiscount",
    "comboFoodDiscount", "comboResourceDiscount",
    "theaterTechScienceDiscount", "theaterResourceDiscountIfLibrary",
    "theaterScienceDiscountIfLibrary",
)

# 5. Military-hand cards.  `hand_potential` walks `hand_civil` ONLY, so
#    `_card_yields` is never called for a tactic, war, aggression, territory
#    or bonus card and mapping these keys would change nothing today.  The
#    military hand reaches the evaluator through `hand_mil_value` (a sum of
#    age+1), i.e. every military card of an age is interchangeable.  That is
#    a real, separate blind spot -- see docs/CARD_BLINDNESS.md.
#
#    Since lane B this is no longer true of every military card:
#    `hand_mil_potential` gives the military hand the same treatment, and
#    territories are priced from their `immediateEffects`/`permanentEffects`
#    through `_TERR_TO_FEATURE`.  The keys below are the ones it still does
#    not reach.
_unpriced(
    # A tactic's whole value is `tacticBonus x armies you can form`, which is
    # a board query, not a card constant -- and the engine never reads these
    # two keys at all: `_army_value` uses the card's TOP-LEVEL `strength` /
    # `obsoleteStrength`, of which these are a duplicate spelling.  Tactics
    # are priced by `engine/effects.py:tactic_outlook` off the same top-level
    # fields the engine uses, surfaced as `tactic_gain` / `tactic_short`.
    # `tests/test_card_pricing.py` asserts the two spellings agree on every
    # tactic, so the duplicate cannot rot into a disagreement.
    "tactic bonus: board-scaled, and a duplicate spelling of the top-level "
    "strength the engine actually reads (see tactic_outlook)",
    "tacticBonus", "tacticBonusObsolete",
)
_unpriced(
    "military hand: never reaches _card_yields (hand_potential is civil-only)",
    "defenseBonus", "colonizationBonus",
)

# 5a. The aggression and war payoffs.  These are written off here for a
#     STRONGER reason than the rest of category 5, and the distinction matters
#     because the census in docs/CARD_BLINDNESS.md counts these eleven
#     aggressions and three wars as "zero visible gain" and they are not.
#
#     `_card_yields` is a board-independent table, and an aggression's value is
#     board-dependent by construction: the loot is capped by what the defender
#     actually holds, and `actions._politics_moves` only OFFERS an aggression
#     whose target it can already beat.  The search does not price these from a
#     table, it plays them out -- `QuiescentBot` drains the defender's
#     `kind="defense"` pending with real 1-ply picks and evaluates the quiet
#     position, and `PlanBot` inherits that.  Wars go through
#     `quiescent.war_value`, which calls the engine's own `events.resolve_war`
#     on a scratch copy and substitutes the result at the leaf.
#
#     So the numbers below are already priced, by the rules engine, more
#     accurately than any weight on `victorTakesCulture` could manage.  Adding
#     a table entry would not improve them; it would double count against a
#     resolution that has already happened.  See docs/EVENT_SEEDING.md section 2.
_unpriced(
    "priced by resolution, not by table: quiescence plays the defense out and "
    "quiescent.war_value calls the engine's own resolve_war",
    "destroyUrbanBuildings", "opponentDecreasesPopulation", "stealColony",
    "takeFromOpponent", "victorTakesYellowTokens", "victorTakesScienceUpTo",
    "victorTakesCulture", "decreasePopulation",
)

# 6. Event / pact structure: nested "who it happens to" blocks.  Events are
#    resolved by the rules engine and pacts are already priced through
#    `deferred_credit` / `_YIELD_TO_FEATURE`, which reads INSIDE these blocks.
#    The outer keys are addressing, not value.
#
#    Three things are true about the 55 events and are worth writing here
#    rather than only in docs/EVENT_SEEDING.md, because this is the block the
#    next person reads:
#
#    * The fifteen Age III "Impact of ..." events ARE priced now, but not from
#      this table and not per card.  `features()` carries
#      `event_scoring_margin`, which asks the engine what the scoring events
#      already in play will pay out.  A per-card table could not do it: the
#      same card is worth +15 to the player with three wonders and -15 to the
#      player facing him.
#    * The 10 pacts have `count 2p: 0` -- they are not in a two-player deck at
#      all, so nothing measured at 2p can say anything about them.
#    * The 16 rank-addressed Age I/II events (`strongestPlayer` and friends)
#      are left unpriced deliberately.  They fire at an unpredictable time --
#      `events._recycle_future_events` shuffles the pile and pops it lowest-age
#      first -- and they resolve against whoever is strongest or weakest AT
#      THAT MOMENT, not at plant time.  Pricing them on the current board would
#      assert a rank ordering several rounds out that the bot has no model for,
#      and their printed swings are small (+/-3 to 4 culture).  A wrong price
#      is worse than a known zero.
_unpriced(
    "addressing: names who an event or pact side applies to, not a yield",
    "allPlayers", "bothPlayers", "weakestPlayer", "weakestPlayers",
    "strongestPlayer", "strongestPlayers", "playerWithMostCulture",
    "playerWithLeastCulture", "playersWithMostHappyFaces",
    "playersWithMostDiscontentWorkers", "target", "condition", "duration",
    "lastRoundSubstitute", "onAttackBetweenParties", "gain", "lose", "A", "B",
)

# ------------------------------------------- the gap one level down: VALUES
#
# `DELIBERATELY_UNPRICED` is keyed by effect KEY, and that is not fine enough
# for a key that is a number on most cards and a sentence on a few.  Those are
# the invisible ones: the key is mapped, the coverage test is green, and
# `_card_yields` drops the card anyway because `isinstance(amt, (int, float))`
# is False.  `gainResources` is exactly that shape and nothing could see it
# until `tests/test_card_pricing.py` grew the value-level check that found it.
#
# Keyed by (card name, key), so writing one off does not write off the
# numeric spellings of the same key on other cards.
UNPRICED_VALUES = {}


def _unpriced_value(reason, *pairs):
    for pair in pairs:
        UNPRICED_VALUES[pair] = reason


_unpriced_value(
    "prose amount: 'half of each destroyed building's printed build cost, "
    "rounded up' depends on what the aggression destroys, which is chosen "
    "when it resolves and is not a property of the card",
    ("Aggression: Raid (I)", "gainResources"),
    ("Aggression: Raid (II)", "gainResources"),
    ("Aggression: Raid (III)", "gainResources"),
)


# 7. Prose.
_unpriced("prose: a rules clarification for human readers", "note")


# the third slot of a `_card_yields` triple
_Y_GAIN = 0     # priced straight through w[k]
_Y_COST = 1     # priced through max(0, w[k]) -- see the docstring below
_Y_RATE = 2     # the newly-visible short-spelling culture/science, scaled by
#                 w["card_rate_credit"] so the fix can be A/B'd against itself
_Y_UNIT = 3     # a military unit's per-worker strength, scaled by
#                 w["unit_strength_credit"] -- same A/B-against-itself trick
_Y_TERR = 4     # a territory's immediate/permanent effects, scaled by
#                 w["territory_credit"]

# What kind is scaled by which weight, and what that weight defaults to when
# the vector does not carry it.  One table so `card_potential` stays a single
# `dict.get` per kind and adding the next credit cannot forget the default.
# The fallbacks MUST equal the `DEFAULT_WEIGHTS` entries of the same name --
# `tests/test_card_pricing.py` asserts it.  `card_potential` is called both
# with a vector `load_weights` has filled in from `DEFAULT_WEIGHTS` and, in
# tools and tests, with a raw weight dict straight out of a champion file; if
# the two disagree the SAME vector prices the same card differently depending
# on how it was loaded, which is a silent divergence of exactly the kind this
# file keeps producing.
#
# `_Y_RATE` is deliberately absent: `card_potential` resolves
# `card_rate_credit` once and threads it through `_sum_yields` as an argument,
# because the board-pricing paths call `_sum_yields` several times per card.
_CREDIT_OF = {
    _Y_UNIT: ("unit_strength_credit", 0.0),
    _Y_TERR: ("territory_credit", 1.0),
}


@_lru_cache(maxsize=None)
def _card_yields(name):
    """(feature, amount, kind) triples for a card, independent of weights.

    `_Y_COST` marks a COST: it is priced through max(0, w) because `science`
    and `resource_stock` are stock weights a hill climb is free to drive
    negative (the 4p champion reached science = -6.09).  Unclamped, a
    negative stock weight turns "this card is expensive" into "this card is a
    bargain" -- Alchemy scored +67.04 under the 4p vector against +5.86 under
    the 2p one.  Paying a cost must never read as a gain.

    `_Y_RATE` marks the two effect keys this function used to drop silently,
    `culture` and `science` (docs/CARD_BLINDNESS.md).  They are separated only
    so `card_rate_credit` can switch them off and recover the exact pre-fix
    pricing for the A/B; they are ordinary gains otherwise.  `_Y_UNIT` and
    `_Y_TERR` are the same device for the two blind spots below.
    """
    db = C.db()
    card = db.by_name.get(name)
    if card is None:
        return ()
    typ = card["type"]
    out = []
    for k, amt in (card.get("production") or {}).items():
        fk = _PROD_TO_FEATURE.get(k)
        if fk and isinstance(amt, (int, float)) and amt is not True:
            out.append((fk, float(amt), _Y_GAIN))
    # A military unit spells its per-worker yield as a TOP-LEVEL `strength`
    # rather than `production: {"strength": n}`, so the loop above never saw
    # it and the loop below never will either.  That is not a judgement call
    # about what a unit is worth: `engine/effects.py:_tech_prog` puts a unit
    # card's top-level `strength` into exactly the slot it puts a farm's
    # `production.food` into, once per worker standing on the technology.
    #
    # The consequence was worse than the culture/science omission, because
    # `_card_yields` DOES read a unit's `techCost` and `buildCost` below: all
    # ten unit cards priced out as PURE COST, strictly negative under every
    # trained vector (Swordsmen -1.66, Air Forces -4.40 under the 2p
    # champion).  `row_pressure` skips any card whose `card_potential` is
    # <= 0, so no unit card in the civil row was ever visible to
    # `row_urgency`/`row_bargain_forgone` at all, and holding one in hand
    # LOWERED `hand_potential`.  See docs/CARD_BLINDNESS.md.
    #
    # Gated on `_UNIT_TO_FEATURE` rather than written straight in, for the
    # same reason `_EFF_SPECIAL` is: `tools/card_blindness.py --legacy` has to
    # be able to switch every later pass back off and still reproduce
    # master's census exactly, or the baseline every result is measured
    # against quietly rewrites itself.
    if typ in C.UNIT_TYPES:
        fk = _UNIT_TO_FEATURE.get("strength")
        st = card.get("strength") or 0
        if fk and isinstance(st, (int, float)) and st is not True and st:
            out.append((fk, float(st), _Y_UNIT))
    eff = card.get("effects") or {}
    for k, amt in eff.items():
        if amt is True or amt is False or not isinstance(amt, (int, float)):
            continue
        fk = _EFF_TO_FEATURE.get(k)
        if fk:
            kind = _Y_RATE if k in ("culture", "science") else _Y_GAIN
            out.append((fk, float(amt), kind))
    # the three `_EFF_SPECIAL` keys: a dict, an offset and a presence flag.
    # Each is gated on its own `_EFF_SPECIAL` membership rather than written
    # straight in, so `tools/card_blindness.py --legacy` can switch the whole
    # set off and reproduce master's census exactly.
    if "buildDiscount" in _EFF_SPECIAL:
        bd = eff.get("buildDiscount")
        if isinstance(bd, dict):
            # MAX, not sum.  `buildDiscount` is {age: resources off} and the
            # ages are MUTUALLY EXCLUSIVE: a build is of exactly one age, so
            # Engineering's {"I": 1, "II": 2, "III": 3} takes at most 3 off any
            # one urban building, never 6.  `effects.build_cost` (engine, l.980)
            # already does the right thing -- `cost -= bd.get(card["age"], 0)`,
            # one lookup -- so summing here priced the card at something the
            # rules engine will never pay out.  The error was not uniform
            # either: it scaled the three Construction techs 3 : 5 : 6 when the
            # rules scale them 1 : 2 : 3, so it got their ORDER wrong too.
            # Max is the ceiling of what one build can realise; see
            # docs/UNCOVERED_TYPES.md section 2.
            vals = [v for v in bd.values() if isinstance(v, (int, float))]
            got = max(vals) if vals else 0
            if got:
                out.append(("build_discount", float(got), _Y_GAIN))
    if "wonderStagesPerAction" in _EFF_SPECIAL:
        wsp = eff.get("wonderStagesPerAction")
        # printed as the TOTAL stages per action (2); one is the base rate
        if isinstance(wsp, (int, float)) and wsp is not True and wsp > 1:
            out.append(("wonder_stages_per_action", float(wsp) - 1.0, _Y_GAIN))
    if "freeCivilAction" in _EFF_SPECIAL and eff.get("freeCivilAction") is not None:
        out.append(("free_civil_action", 1.0, _Y_GAIN))
    # A territory keeps its entire value in `immediateEffects` (one-shot) and
    # `permanentEffects` (ongoing) -- two blocks this function never opened,
    # which is why the census reported all 12 territories as "zero visible
    # gain" with ZERO dropped keys: there was nothing in `production` or
    # `effects` to drop.  Their `effects` block is literally `{}`.
    #
    # This is not a new opinion about what a territory is worth.  It is the
    # SAME pricing the auction path already uses: `deferred_credit` prices the
    # high bid by pushing these two blocks through `_YIELD_TO_FEATURE`, so
    # once you hold the high bid the evaluator can see the card and before
    # that it cannot.  `_TERR_TO_FEATURE` closes that asymmetry.
    if typ == "territory":
        for block in ("immediateEffects", "permanentEffects"):
            for k, amt in (card.get(block) or {}).items():
                if amt is True or amt is False or \
                        not isinstance(amt, (int, float)):
                    continue
                fk = _TERR_TO_FEATURE.get(k)
                if fk:
                    out.append((fk, float(amt), _Y_TERR))
    tc = card.get("techCost") or 0
    if tc:
        out.append(("science", -float(tc), _Y_COST))
    bc = card.get("buildCost") or 0
    if bc:
        out.append(("resource_stock", -float(bc), _Y_COST))
    if typ == "wonder":
        out.append(("wonders", 1.0, _Y_GAIN))
        stages = card.get("stages") or []
        if stages:
            out.append(("resource_stock", -float(sum(stages)), _Y_COST))
    return tuple(out)


@_lru_cache(maxsize=None)
def _card_choices(name):
    """Mutually exclusive alternatives on a card, as a tuple of triple-groups.

    `_card_yields` sums everything it finds, which is right for a card that
    gives you two things and wrong for a card that makes you pick one.  The
    three Reserves are the whole of this today: "gain 2 food OR 2 resources".
    Summing them would price Reserves (III) at four food AND four resources;
    dropping the key, which is what happened until now, priced all three
    Reserves at exactly nothing.  You get the better of the two, so the value
    is a max over the group and only `card_potential` (which holds the
    weights) can take it.
    """
    card = C.db().by_name.get(name)
    if card is None:
        return ()
    eff = card.get("effects") or {}
    # gated on `_EFF_CHOICE` membership rather than written straight in, for
    # the same reason the `_EFF_SPECIAL` block in `_card_yields` is: it lets
    # `tools/card_blindness.py --legacy` switch the whole set off and
    # reproduce master's census exactly.
    amt = (eff.get("gainFoodOrResources")
           if "gainFoodOrResources" in _EFF_CHOICE else None)
    if isinstance(amt, (int, float)) and amt is not True:
        return ((
            (("food_stock", float(amt), _Y_GAIN),),
            (("resource_stock", float(amt), _Y_GAIN),),
        ),)
    return ()


def _sum_yields(triples, w, credit):
    total = 0.0
    # `_Y_RATE`'s credit is the `credit` argument, resolved once by the
    # caller.  The other credited kinds resolve from `_CREDIT_OF` and are
    # memoized here so a card carrying several of them still costs one
    # `dict.get` per kind.
    credits = {}
    for k, amt, kind in triples:
        wk = w.get(k, 0.0)
        if kind == _Y_COST:
            if wk < 0.0:
                wk = 0.0
        elif kind == _Y_RATE:
            amt *= credit
        elif kind != _Y_GAIN:
            c = credits.get(kind)
            if c is None:
                ck, cdef = _CREDIT_OF[kind]
                c = credits[kind] = w.get(ck, cdef)
            amt *= c
        if wk and amt:
            total += wk * amt
    return total


# ------------------------------------------------- how much board to believe
#
# `card_board_credit` is the shared credit on board-aware pricing.  These
# four are per-type OFFSETS added to it, so:
#
#   * `card_board_credit` alone moves every type together, which is what
#     the aggregate A/B measured;
#   * a single offset moves one type on its own, which is what the
#     decomposition arms measured -- and, unlike the `TTA_BOARD_TYPES`
#     environment variable they used to be, these are weights, so
#     `hillclimb_league` can fit them instead of a human setting them.
#     `card_board_credit` 1.0 with `card_board_government` -1.0 is exactly
#     the old `TTA_BOARD_TYPES=leader`, offsets being additive.
#
# All four default to 0.0 on top of a 0.0 credit, so the shipped evaluator
# is byte-identical either way (docs/CARD_PRICING_LEADERS.md section 5).
_BOARD_CREDIT_KEYS = {
    "leader": "card_board_leader",
    "government": "card_board_government",
    "action": "card_board_action",
    "wonder": "card_board_wonder",
}


@_lru_cache(maxsize=None)
def _board_credit_key(name):
    """The per-type offset key for `name`, or None if its type has none."""
    return _BOARD_CREDIT_KEYS.get(C.db().type_by_name.get(name))


@_lru_cache(maxsize=None)
def _swap_type(name):
    """`"leader"` / `"government"` for a single-slot card, else None.

    `SINGLE_SLOT`, deliberately, and NOT `SWAP_TYPES`: a wonder is priced by
    a swap diff too but two wonders in hand can both be built, so collapsing
    them would be wrong.  See the note on both sets in `board_yields`.
    """
    typ = C.db().type_by_name.get(name)
    return typ if typ in _BY.SINGLE_SLOT else None


def card_potential(name, w, state=None, idx=None):
    """Eval-points a single card in hand would be worth if it were played.

    `state`/`idx` are optional and turn on board-aware pricing
    (`engine/bots/board_yields.py`), which is what lets a leader be priced by
    what it would actually do on THIS board instead of by the numbers printed
    on it.  Board pricing is gated on `w["card_board_credit"]` plus the card
    type's own offset, exactly as the `effects.culture` fix is gated on
    `card_rate_credit`, so that 0.0 recovers the old pricing byte-for-byte and
    the two can be duelled paired, in one process, on the same deal.  Callers
    without a state (and the whole of `analysis/`) keep the old signature and
    the old answer.
    """
    credit = w.get("card_rate_credit", 1.0)
    base = w.get("card_board_credit", 0.0)
    key = _board_credit_key(name)
    board = (base + w.get(key, 0.0)) if key is not None else base
    if not base and not board:
        # the exact pre-change answer, and this early return is why: every
        # branch below has to be behind the gate for the A/B to be paired.
        # `_card_choices` is in here too even though it needs no board --
        # it needs more than a table lookup, which is the same thing as far
        # as "does this move a digest" is concerned.
        return _sum_yields(_card_yields(name), w, credit)
    on_board = state is not None and idx is not None
    if on_board and board:
        # A swap card is priced ONLY by the diff: `_card_yields` would count
        # Gandhi's printed +2 culture a second time on top of the delta that
        # already contains it.  A swap card whose type is credited 0.0 falls
        # through to the static table instead, which is what the type knob
        # meant when it was an environment variable.
        swap = _BY.board_yields(name, state, idx)
        if swap is not None:
            return board * _sum_yields(swap, w, credit)
    total = _sum_yields(_card_yields(name), w, credit)
    # `_card_choices` rides the shared credit and not the `action` offset:
    # it is not board-aware pricing at all (it needs no board), it is here
    # because it is more than a table lookup.  That is also what the type
    # knob did when it was an environment variable.
    for group in _card_choices(name):
        total += base * max(_sum_yields(g, w, credit) for g in group)
    if on_board and board:
        total += board * _sum_yields(_BY.board_extra(name, state, idx), w,
                                     credit)
    return total


def _hand_total(hand, state, idx, w):
    """`card_potential` over a civil hand, with the single-slot classes
    collapsed to one card each.

    THE THING THIS FIXES.  A leader and a government are priced as a swap
    DIFF -- what replacing the one you have with this one would change
    (board_yields, "Replacement").  Summing that over the hand asserts you
    get to make the same replacement once per card you hold, and you do not:
    holding Joan of Arc, a hand of {Michelangelo, Julius Caesar, Homer} was
    priced at -6.95 (three replacements of Joan, two of them ruinous) when
    the truthful answer is +3.60, because Michelangelo is the one you would
    play and the other two you simply would not.  Harmless while the bot held
    ~0 leaders; not harmless once board pricing made it take 55% more of them
    (docs/CARD_PRICING_LEADERS.md sections 5.2 and 8).

    So each slot contributes the BEST card in the hand for it, plus
    `hand_swap_extra` times the rest.  The spares are not worthless -- you
    may play the best one now and a better one two ages later -- but their
    true incremental value is measured against the leader you will have by
    then, not against the one you have now, which is not a quantity this
    function can see.  `hand_swap_extra` is therefore a free parameter and it
    is a 0.0-default WEIGHT rather than a constant somebody picked: 0.0 says a
    spare leader is worth nothing extra, 1.0 is exactly the old summing (which
    keeps the defect available as a paired control arm in one process), and
    the league can find what is in between.  Linear in the weight, so
    `hillclimb.mutate` has a gradient on it from 0.0.

    Only cards that are ACTUALLY being priced as a diff are collapsed: with
    the board credit at 0.0 a leader is priced off the static table like
    anything else, there is no replacement being double-counted, and the hand
    stays a plain sum -- which is what keeps the shipped default byte-
    identical.  N = 1 is untouched either way; the sign of a lone unplayable
    leader is a separate question and deliberately not changed here.
    """
    total = 0.0
    slots = None
    for n in hand:
        v = card_potential(n, w, state, idx)
        slot = _swap_slot(n, w)
        if slot is None:
            total += v
            continue
        if slots is None:
            slots = {}
        cur = slots.get(slot)
        if cur is None:
            slots[slot] = [v, v]          # best so far, sum so far
        else:
            if v > cur[0]:
                cur[0] = v
            cur[1] += v
    if slots:
        extra = w.get("hand_swap_extra", 0.0)
        for best, tot in slots.values():
            total += best + extra * (tot - best)
    return total


def _swap_slot(name, w):
    """The single-slot class `name` is being priced as a diff for, or None.

    None for an ordinary card, and also for a leader when the leader credit
    is 0.0: then `card_potential` returned the static-table value, which is
    not a replacement and must not be collapsed.
    """
    typ = _swap_type(name)
    if typ is None:
        return None
    if not (w.get("card_board_credit", 0.0)
            + w.get(_BOARD_CREDIT_KEYS[typ], 0.0)):
        return None
    return typ


def hand_potential(state, idx, w):
    """`card_potential` over the civil hand, single-slot classes collapsed
    (0.0 for an empty hand).  See `_hand_total`."""
    hand = state.players[idx].hand_civil
    if not hand:
        return 0.0
    return _hand_total(hand, state, idx, w)


def wonder_potential(state, idx, w):
    """WHICH wonder am I building?  `hand_potential`'s missing sibling.

    THE PLUMBING BUG THIS EXISTS TO FIX.  `hand_potential` was added because
    two different cards in hand produced a byte-identical feature vector, so
    the search had no basis to prefer a good card to a bad one.  That argument
    applies verbatim to the wonder in progress, and nothing covered it:

    * `engine/actions.py:take_card` puts a wonder **straight into
      `p.wonder`** -- a wonder NEVER enters `hand_civil`.  So
      `hand_potential`, the live term the search optimises at every decision,
      never sees one.
    * `features()` reads `p.wonder` only for `stages` and `steps_built`
      (`wonder_progress`, `wonder_remaining`, `wonder_stages_left`,
      `wonder_turns_to_finish`, `wonder_overrun`).  Every one of those is
      arithmetic on RESOURCES.  Eiffel Tower and Ocean Liners at the same
      stage of the same cost are the same card to all five.
    * The only path left is `row_pressure`, gated on `row_urgency` /
      `row_bargain_forgone`, which default to 0.0 and are unset in all three
      frozen champions -- so in practice a wonder's identity reached the
      policy through nothing at all.

    That is why repricing a leader moves play hard and repricing a wonder
    barely moves it: leaders go to hand and are priced by a live term,
    wonders are not.  Pricing the wonders better cannot fix that; only a term
    that reads them can.

    This is an omission of the same kind as the missing `culture` mapping,
    not a design decision.  It was worth checking for a rules reason and
    there is none: a wonder in progress is public (`rival_context` already
    exposes `q.wonder`, reduced to a bool only because `_can_take_gated` asks
    nothing else of it), so this reads no hidden state and does not touch the
    determinization leak that keeps rival military hands out of the
    evaluation.

    It bites at BOTH decisions that matter, and the second one is the point:

    1. keep paying stages into this wonder, or stop;
    2. **take this wonder from the row at all** -- because `take_card` sets
       `p.wonder` immediately, the 1-ply search's post-move state already has
       the wonder in place, so taking Eiffel Tower and taking Ocean Liners
       stop looking identical.

    GAINS ONLY.  The stage cost is deliberately dropped: `wonder_remaining`
    already prices the outstanding resources, and it prices them correctly
    for a PART-BUILT wonder, which `_card_yields`' flat `-sum(stages)` does
    not.  Adding the cost here would double-count it and would charge for
    stages already paid.

    Scaled by its own weight, `wonder_potential`, default 0.0 -- so this is
    inert until the league is told to look, exactly like `hand_potential`
    before it was measured.
    """
    p = state.players[idx]
    if p.wonder is None:
        return 0.0
    credit = w.get("card_rate_credit", 1.0)
    board = w.get("card_board_credit", 0.0)
    name = p.wonder.name
    if board:
        swap = _BY.board_yields(name, state, idx)
        if swap is not None:
            return board * _sum_yields(_gains_only(swap), w, credit)
    return _sum_yields(_gains_only(_card_yields(name)), w, credit)


def _gains_only(triples):
    """Drop `_Y_COST` triples -- see `wonder_potential`'s last paragraph."""
    return tuple(t for t in triples if t[2] != _Y_COST)


def tactic_terms(state, idx):
    """(tactic_gain, tactic_short) -- the two halves of the tactic deadlock.

    See `engine/effects.py:tactic_outlook` for what the deadlock is.  Briefly:
    playing a tactic you have no army for is +0 strength for a military action
    and a card, and building the unit that would complete an army is +printed
    strength only because the tactic is not in play yet, so a 1-ply search
    does neither and the champion ends every game holding a tactic with zero
    units to fill it (docs/CARD_BLINDNESS_MILITARY.md section 4).

    * `tactic_gain` -- army strength the best REACHABLE tactic (in hand, or
      copyable from `state.available_tactics`) would add over the one in play.
      Exactly 0 once that tactic is in play, so a positive weight prices
      getting there rather than pricing tactics.
    * `tactic_short` -- unit workers still owed before it forms one more army.
      `tactic_gain` alone is a step function, flat at zero for the first two
      of Heavy Cavalry's three cavalry; this is the gradient.

    NOT in `features()`, and the reason is cost, not taste: it is 81us against
    that function's 433us, i.e. +19% on the hottest path in the project, for a
    pair of terms that default to 0.0.  `evaluate` gates it on the two weights
    the way it already gates `hand_potential` and `row_pressure`, so a vector
    that does not use it pays nothing at all.
    """
    p = state.players[idx]
    if not state.has_military:
        return 0.0, 0.0
    type_of = C.db().type_by_name
    cands = [n for n in p.hand_military if type_of.get(n) == "tactic"]
    cands.extend(state.available_tactics or ())
    if p.tactic:
        cands.append(p.tactic)
    if not cands:
        return 0.0, 0.0
    best_army, short = effects.tactic_outlook(state, p, cands)
    gain = float(max(0, best_army - effects.army_strength(state, p)))
    return gain, float(short)


def hand_mil_potential(state, idx, w):
    """Summed `card_potential` over the MILITARY hand.

    The sibling `hand_potential` never had, and the reason the census could
    report 12 territories as invisible: `hand_potential` walks `hand_civil`
    only, so `_card_yields` was never called for a territory, tactic, war,
    aggression, pact or bonus card.  The military hand reached the evaluator
    through `hand_mil_value` alone -- `sum(age_level + 1)` -- under which a
    Vast Territory, a Fighting Band and an Aggression of the same age are the
    same card.

    Scaled by its own weight, defaulting to 0.0, so adding it is inert: with
    no weight on it nothing calls `card_potential` on a military card and the
    evaluation is bit-identical.  It is the hook the other military card types
    need, not just territories.
    """
    hand = state.players[idx].hand_military
    if not hand:
        return 0.0
    total = 0.0
    for n in hand:
        total += card_potential(n, w)
    return total


def rival_hand_potential(state, idx, w, rivals=None):
    """The most dangerous rival civil hand, priced through the same weights.

    LEGAL, and this is the point of it: cards taken from the row are public
    knowledge (docs/RULES_SPEC.md:71, "open civil cards convention"), so
    `q.hand_civil` is information a human at the table has and the evaluator
    simply never looked at (docs/INFORMATION_AUDIT.md section 3).  It reads no
    hidden field, so unlike the military hand it does not interact with the
    determinization leak in section 6.

    `max` over rivals rather than `sum`, so the term means the same thing at
    2p, 3p and 4p.  Within one rival's hand the single-slot classes collapse
    exactly as they do in mine (`_hand_total`): a rival holding three leaders
    is not three replacements dangerous either, and pricing my hand and
    theirs through two different functions is how the two drift apart.
    """
    if rivals is None:
        rivals = [q for q in state.players
                  if q.idx != idx and not q.resigned]
    best = 0.0
    for q in rivals:
        if not q.hand_civil:
            continue
        # priced on the RIVAL's board -- a leader that pays per theater is
        # worth what it is worth to the player who would play it
        total = _hand_total(q.hand_civil, state, q.idx, w)
        if total > best:
            best = total
    return best


# ------------------------------------------------- take now vs let it slide
#
# `game.SWEEP`, duplicated because `engine.game` imports `engine.actions`
# which this module imports -- importing game here is a cycle.  `test_row_
# features.py` asserts the two are equal, so the copy cannot rot.
_SWEEP = {2: 3, 3: 2, 4: 1}

# P(a rival who CAN legally take a card I also want actually takes it) before
# my next turn.  One constant, deliberately: the alternative on offer was the
# seven measured per-slot survival rates in docs/INFORMATION_AUDIT.md 2.1,
# and that audit flags them itself as directional (n~210 per slot, and the
# opponents generating them were themselves row-blind bots, so a field that
# actually competed for cards would take more).  Baking seven numbers fitted
# on blind play into the evaluator would be fitting the bug.  The legality
# gate underneath -- can that rival reach that slot at all, is their hand
# full, do they already hold the card, are they mid-wonder -- is EXACT, and
# is where the signal the expert sources single out actually lives
# (docs/EXPERT_STRATEGY.md:546: wonders and second leaders are safe to let
# slide).  This constant only shapes how fast the bargain decays in the
# number of rivals who could take it.
RIVAL_TAKE_P = 0.25


def row_pressure(state, idx, w, ctx=None):
    """(row_urgency, row_bargain_forgone) for the civil row.

    The slide is arithmetic, not a guess.  `_replenish` runs at the START of
    every player's turn (`game.start_turn`), so between one of my turns and
    the next there are exactly `live` replenishes of `SWEEP[live]` cards --
    6 slots at 2p, 6 at 3p, 4 at 4p -- plus one more for every card to the
    left of it that somebody takes.  Ignoring those takes makes `next_slot`
    an UPPER bound on what the card will cost me next turn, i.e. the bargain
    below is understated, never overstated.

    * **row_urgency** -- summed `card_potential` of the cards I could legally
      take that the sweep destroys before I act again.  Take it now or never.
    * **row_bargain_forgone** -- summed civil actions I overpay by taking a
      card now instead of next turn, discounted by the chance a rival takes
      it first.

    Both are evaluated on the POST-move state like every other feature, so
    taking a doomed card lowers `row_urgency` and taking a card that was
    about to get cheaper leaves `row_bargain_forgone` behind for the
    candidate that did not.  Cards whose `card_potential` is <= 0 are skipped
    in both sums: the sweep destroying a card I do not want is not a loss,
    and waiting for one is not a bargain.

    Reads `state.card_row`, my own board, and the public rival boards/civil
    hands snapshotted into `ctx`.  The row is public AT THE ROOT only: a trial
    that crossed a turn boundary has had the real next cards dealt into it, so
    every slot is matched against `ctx["root_row"]` -- the root row's names in
    ROW ORDER -- with a forward-only cursor, and cards that were not visible
    when the decision started are SKIPPED.  Without that check this function is
    the mechanism of the `end_turn` information leak, and it was measurably
    changing the chosen move at 3p (INFORMATION_AUDIT 6.1).

    The cursor only ever moves right, and that is what tightened the mask from
    the per-name count it replaced.  Skipping over root names to find a match
    is how swept cards, taken cards and the holes they leave are tolerated
    (their slots are public arithmetic); consuming each root name at most once,
    IN ORDER, is what stops a dealt card reusing the name of a card that was
    swept off the left, which a per-name count could not (INFORMATION_AUDIT
    6.4).  It is an upper bound on the survivors, not an identity -- see
    `root_row_budget` for the one departure direction it still cannot see.

    `ctx` without a `root_row` key (a caller that built its own dict, or the
    degraded no-ctx path) masks nothing and is leaky, as it was before; the
    bots all go through `rival_context`, which always supplies it.
    """
    row = state.card_row
    if not row:
        return 0.0, 0.0
    # Forward-only cursor into the root row's name SEQUENCE, advanced as slots
    # are accepted.  Never rewound, so N dealt copies of a card the root row
    # held once are priced once, and a root card that was swept off the left
    # cannot lend its name to a dealt card (see `root_row_budget`).
    # `None` (a caller-built ctx or the degraded no-ctx path) masks nothing and
    # is leaky, as documented below; an EMPTY tuple is a genuinely empty root
    # row and masks everything, because then every card present was dealt.
    root = ctx.get("root_row") if ctx else None
    cursor, n_root = 0, 0 if root is None else len(root)
    p = state.players[idx]
    n = _live(state)
    slide = n * _SWEEP[n]
    mine = actions._take_gate(state, p, budget=actions.ca_total(state, p))
    views = ctx.get("rival_views", ()) if ctx else ()
    cost_of = actions.ROW_COST
    gated = actions._can_take_gated
    urgency = bargain = 0.0
    for i, name in enumerate(row):
        if name is None:
            continue
        if root is not None:
            k = cursor
            while k < n_root and root[k] != name:
                k += 1               # swept, or taken out from under the row
            if k >= n_root:
                # Dealt after the decision began, and -- because dealt cards
                # are always a suffix -- so is every slot to its right.
                break
            cursor = k + 1
        if not gated(state, p, i, mine, name):
            continue
        val = card_potential(name, w, state, idx)
        if val <= 0.0:
            continue
        nxt = i - slide
        if nxt < 0:
            urgency += val
            continue
        saving = cost_of[i] - cost_of[nxt]
        if saving <= 0:
            continue
        survive = 1.0
        for view, gate in views:
            if gated(state, view, i, gate, name):
                survive *= 1.0 - RIVAL_TAKE_P
        bargain += saving * survive
    return urgency, bargain


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
    # --- INFORMATION_AUDIT gaps 1/2/3, all defaulted to 0.0 ON PURPOSE.
    #
    # Every one of these is a NEW channel, not a change to an existing one, so
    # at 0.0 the evaluation is bit-identical to the one the three frozen
    # champions were trained under and `analysis/frozen/champion_*.json` stay
    # valid (`load_weights` fills them in from here).  That is the whole point:
    # the cutover costs nothing and the trainer decides what they are worth.
    #
    # 0.0 is not a dead weight either -- `hillclimb.mutate` perturbs by
    # `gauss(0, s) * (abs(w) + 0.15)`, and that 0.15 floor is exactly what
    # lets a term that starts at zero move on the first generation that
    # scatters onto it.
    #
    # `take_cost_paid` is deliberately NOT sign-constrained: the sources say a
    # CA spent grabbing from the row gets MORE valuable late while a CA spent
    # upgrading gets less (docs/EXPERT_STRATEGY.md:550), so a fitted sign is
    # the answer here, not a prior.  (A 0.0 default also keeps it out of
    # hillclimb_league's NONNEG/NONPOS, which are derived from the sign of the
    # default.)
    "take_cost_paid": 0.0,
    # take now vs let it slide (GAP 2): value the sweep is about to destroy,
    # and civil actions forgone by not waiting.  Both are priced through `w`
    # itself (like `hand_potential`), so they are scales on a non-linear term
    # and must not pick up the early/late phase multipliers.
    "row_urgency": 0.0,
    "row_bargain_forgone": 0.0,
    # public rival board/hand facts (GAP 3)
    "rival_free_ca": 0.0,
    "rival_hand_civil": 0.0,
    "rival_wonders": 0.0,
    "rival_hand_potential": 0.0,
    # military
    "strength": 0.35,
    "strength_rel": 0.35,
    "strength_deficit": -0.6,
    "strength_lead": 0.3,
    "tactic_level": 0.5,
    # --- the tactic deadlock (docs/CARD_BLINDNESS.md, lane B).  Both 0.0, so
    # adding them is inert for every trained vector; see
    # `engine/effects.py:tactic_outlook` for what they mean and why one is
    # not enough.  `tactic_short` is NOT given a negative prior even though
    # "fewer units owed is better" reads obvious: a negative default puts it
    # in hillclimb_league's NONPOS set, and the sign is genuinely arguable
    # (owing units to a big Age III tactic is a position, not only a debt).
    "tactic_gain": 0.0,
    "tactic_short": 0.0,
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
    # --- finish discipline (docs/CARD_BLINDNESS.md).  All 0.0, so a trained
    # vector is unchanged; the league decides the price.  Deliberately NOT
    # given a negative prior even though the evidence points that way (0 for
    # 58 on the three 12-resource Age II wonders): a negative default would
    # also put them in hillclimb_league's NONPOS set and forbid the climber
    # from ever discovering that a wonder programme is worth having.
    "wonder_stages_left": 0.0,
    "wonder_turns_to_finish": 0.0,
    "wonder_overrun": 0.0,
    # Masonry and friends: stages per build action, above the base of one.
    "wonder_stages_per_action": 0.0,
    # Identity of the wonder in progress, which nothing else in the vector
    # can see: every other `wonder_*` term is arithmetic on resources.  0.0
    # for the usual reason -- a new channel, not a change to an existing one
    # -- but note that unlike the three finish-discipline terms this one is
    # NOT a near-dead coordinate: it differs across candidates at every
    # take-a-wonder decision, because `take_card` sets `p.wonder` in the
    # post-move state the search scores.
    "wonder_potential": 0.0,
    "leader": 1.5,
    # --- effect keys that were dropped on the floor and now have somewhere to
    # land.  0.0 for the same reason as the INFORMATION_AUDIT block above: a
    # new channel, not a change to an existing one.
    "hand_limit": 0.0,
    "colonize_bonus": 0.0,
    "build_discount": 0.0,
    # hand-only: both live on one-shot action cards, so there is no board
    # state for them and `features()` never emits them.
    "free_civil_action": 0.0,
    "resource_discount": 0.0,
    # --- board-aware card pricing (engine/bots/board_yields.py).  All 0.0,
    # so the whole of it is inert until `card_board_credit` is turned up.
    #
    # `urban_limit` and `gov_action_cost` are the two the governments needed:
    # a government's whole value is its top-level `civilActions` /
    # `militaryActions` / `urbanBuildingLimit`, which live in no
    # `production` or `effects` block and which `_card_yields` therefore
    # never read -- all eight governments priced out as free and worthless
    # at once.  `gov_action_cost` is the civil-action pool a revolution
    # burns, which is board-aware (a 7-action Republic turn is a dearer
    # thing to spend than a 4-action Despotism turn).
    "urban_limit": 0.0,
    "gov_action_cost": 0.0,
    # Moses: `Stats.pop_food_discount`, food off every population increase.
    "pop_food_discount": 0.0,
    # Gandhi: `Stats.no_aggression`.  Deliberately unsigned -- it bundles a
    # cost (he may never play an aggression or war) with a benefit
    # (opponents pay double to attack him), and which dominates is exactly
    # the sort of thing a league can measure and a prior cannot.
    "no_aggression": 0.0,
    # Patriotism / Wave of Nationalism / Military Build-Up / Churchill's
    # military option: resources ring-fenced to building military units.
    # Worth less than the same number of free resources by an amount only
    # the policy's own appetite for units decides.
    "restricted_resources": 0.0,
    # How much of the board-aware pricing to believe, the exact analogue of
    # `card_rate_credit` below and for the same reason: at 0.0 `card_
    # potential` is byte-identical to the static-table answer, so the fix
    # can be duelled against itself paired, in one process, on one deal.
    "card_board_credit": 0.0,
    # Final-scoring culture the pending Age III "Impact of ..." events owe me
    # less the best rival's (see `event_scoring_margin`).  0.0 = inert: every
    # trained vector plays exactly as it did and the digests do not move.  The
    # A/B that says what it is worth is docs/EVENT_SEEDING.md.
    "event_scoring_margin": 0.0,
    # Per-type offsets on that credit, replacing the `TTA_BOARD_TYPES`
    # environment variable so the league can fit what only a human could set
    # before.  The credit for a card of type T is `card_board_credit +
    # card_board_T`, so one knob still moves all four together and each type
    # can also move on its own.  The government half is the one with the
    # measured positive (culture margin +1.85, z = 3.4, where the leader half
    # is a null once its blocks are clustered honestly -- 48.20%, z = -1.46,
    # p = 0.15: docs/CARD_PRICING_LEADERS.md 5.2 and its 2026-07-30
    # correction), and this is what lets the climber find that without being
    # told.
    "card_board_leader": 0.0,
    "card_board_government": 0.0,
    "card_board_action": 0.0,
    "card_board_wonder": 0.0,
    # What a SPARE single-slot card is worth: the hand's best leader is priced
    # in full and every other leader in the hand at this fraction of its own
    # replacement diff (`weighted._hand_total`).  0.0 = a second leader adds
    # nothing, because only one of them can be the replacement; 1.0 is exactly
    # the summing this replaced, which is what makes the defect a paired
    # control arm rather than something only a rebuild can reproduce.  A
    # weight and not a constant because the honest value depends on how often
    # a later leader beats the one you played, which nothing here can see.
    "hand_swap_extra": 0.0,
    # How much of the short-spelling `effects.culture` / `effects.science`
    # (the two keys `_card_yields` used to drop on the floor) to believe when
    # pricing a card in hand.  1.0 = price them like any other per-turn
    # production, which is what the engine does with them on the board.
    #
    # This is the ONE weight in this change whose default is not 0.0, and it
    # is deliberate: at 0.0 the fix would be off and the frozen champions
    # would keep playing blind.  1.0 is therefore a real behaviour change for
    # every vector, and the four WeightedBot/QuiescentBot/PlanBot fingerprint
    # digests move for it (docs/CARD_BLINDNESS.md section 5).  It is a weight
    # rather than a hard-coded mapping precisely so that 0.0 recovers the
    # pre-fix pricing exactly and the two can be duelled in one process.
    "card_rate_credit": 1.0,
    # --- the two credits added with the military-card lane, same device and
    # the same reason: 0.0 recovers the pre-fix pricing exactly, so each fix
    # can be duelled against itself in one process on the same deal.
    #
    # `unit_strength_credit` is 0.0, unlike `card_rate_credit`, and the reason
    # is measured rather than cautious.  TWO facts, both in
    # docs/CARD_BLINDNESS.md:
    #
    # 1. At 1.0 it is a NO-OP for every trained vector.  `champion_2p` vs
    #    itself with the credit flipped is 60 games byte-identical -- same win
    #    rate, same cultures, mirrored seat by seat -- because over 2264 plies
    #    of its own self-play it held a unit card in its civil hand ONCE.  A
    #    unit was legally takeable at 30% of plies and it took one.  So 1.0
    #    would move three of the eight fingerprint digests -- weighted narrow
    #    5eff41eb -> beba1c96, weighted wide d03e0964 -> da252e5d, plan narrow
    #    c534ac3d -> b896b53a, with both greedy arms, both quiescent arms and
    #    plan wide unchanged -- to buy behaviour nobody can measure.
    #
    # 2. 1.0 is not privileged the way it is for `card_rate_credit`.  There,
    #    1.0 is exactly what the engine does with the key.  Here the board
    #    expresses a point of strength through FOUR features -- `strength`,
    #    `strength_rel`, and `strength_lead` or `strength_deficit` -- and
    #    `card_potential` looks up only the first, so 1.0 is between a 2.3x
    #    (strength + strength_rel, both unconditional) and a 7x (when behind)
    #    under-count of the truth.  There is no defensible constant here, only
    #    a weight, and the league has a gradient for it: `hillclimb.mutate`
    #    perturbs by `gauss(0, s) * (abs(w) + 0.15)`, so a 0.0 weight moves on
    #    the first generation that scatters onto it.
    #
    # What is NOT deferred is the mapping itself: `_card_yields` now reports a
    # unit's strength, so the information exists and `tools/card_blindness.py`
    # no longer counts the ten unit cards as blind.  Only how much of it to
    # believe is left to the trainer.
    "unit_strength_credit": 0.0,
    # `territory_credit` is 1.0 but costs nothing until `hand_mil_potential`
    # is non-zero, because nothing calls `card_potential` on a military card
    # otherwise.  It is a separate knob from `hand_mil_potential` so that
    # "how much of a territory's printed effect to believe" -- it is seeded
    # into an auction anyone can win, not played -- stays separable from
    # "how much the military hand matters at all".
    "territory_credit": 1.0,
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
    # scale on the identity-aware MILITARY hand term (see
    # `hand_mil_potential`).  0.0, and that is what makes the territory
    # pricing above inert: at 0.0 `card_potential` is never called on a
    # military card at all.
    "hand_mil_potential": 0.0,
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
    # `horizon_age` is an A/B escape hatch, not a strategy weight: it restores
    # the pre-fix four-step age bucket for THIS weight vector only, so the old
    # and new horizons can be seated at the same table and duelled directly
    # (docs/CULTURE_GAP.md section 7).  Deliberately absent from
    # DEFAULT_WEIGHTS, so the trainer never emits it, `mutate` never perturbs it
    # and `guard_weights` never sees it; it exists only in hand-written A/B
    # weight files.  The extra `get` costs one dict lookup per evaluation
    # against the ~90 the loop above already does.
    late = lateness_by_age(state) if get("horizon_age") else lateness(state)
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
    # which wonder am I building (and, through the post-move state, which
    # one am I taking) -- see `wonder_potential`.  0.0 by default.
    wp = get("wonder_potential")
    if wp:
        total += wp * wonder_potential(state, idx, w)
    # The row / rival-hand terms, same shape and same reason as
    # `hand_potential` above: they are priced through `w`, so they are not
    # linear features and cannot live in `features()`.  Each is skipped
    # entirely when its scale is 0, which is the default -- so a champion
    # trained before these existed evaluates exactly as it did, and pays
    # nothing for them either.
    hmp = get("hand_mil_potential")
    if hmp:
        total += hmp * hand_mil_potential(state, idx, w)
    # The tactic deadlock terms.  Linear features, unlike the three above, but
    # gated here for the same reason those are: `tactic_terms` is +19% on
    # `features()` and both weights default to 0.0.
    tg = get("tactic_gain")
    ts = get("tactic_short")
    if tg or ts:
        gain, short = tactic_terms(state, idx)
        if tg:
            total += tg * gain
        if ts:
            total += ts * short
    rhp = get("rival_hand_potential")
    if rhp:
        total += rhp * rival_hand_potential(state, idx, w)
    ru = get("row_urgency")
    rb = get("row_bargain_forgone")
    if ru or rb:
        urgency, bargain = row_pressure(state, idx, w, ctx)
        if ru:
            total += ru * urgency
        if rb:
            total += rb * bargain
    return total


# ------------------------------------------------------------------- bot

class WeightedBot:
    """1-ply search under a fully parameterized linear evaluation."""

    name = "weighted"

    def __init__(self, weights=None, rng=None, seed=None, name=None,
                 allow_resign=False):
        self.weights = dict(weights) if weights else dict(DEFAULT_WEIGHTS)
        self.rng = rng or random.Random(seed)
        self.allow_resign = allow_resign
        if name:
            self.name = name

    # -- harness adapters
    def choose(self, state, moves, rng=None):
        return self.pick(state, moves)

    def __call__(self, state):
        return self.pick(state, actions.legal_moves(state))

    def pick(self, state, moves):
        # `("resign",)` (§5.11) is legal on almost every turn, and in a 2p game
        # it is never right -- it hands the win to the opponent immediately.
        # `RandomBot` has guarded against it since it was written, because a
        # uniform bot would otherwise end most games in round 2; `WeightedBot`
        # never did, and that is a live trap rather than a theoretical one: a
        # value vector fitted by regression resigned on turn 3 of 3 games in 12
        # (docs/BOT_ARCHITECTURE.md §3b), which silently contaminated an n=400
        # duel with games that ended at round 2 with scores [0, 0].  The trained
        # champions happen never to resign, so this filter is a no-op for them
        # and a correctness fix for every new vector.
        if not self.allow_resign and len(moves) > 1:
            live = [m for m in moves if m[0] != "resign"]
            if live:
                moves = live
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
                   "rival_strength": 0, "rival_views": ()}
        w = self.weights
        end_bias = w.get("end_turn_bias", 0.0)
        if USE_JOURNAL:
            return self._pick_journalled(state, moves, idx, ctx, w, end_bias)
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

    def _pick_journalled(self, state, moves, idx, ctx, w, end_bias):
        """`pick` with the undo stack instead of `copy_state` (docs/PYPY.md 6).

        Line-for-line the same search as above; the only difference is that the
        candidate is applied to the REAL state and undone, rather than to a copy
        that is thrown away.  Kept as a separate method rather than a branch
        inside the loop so the copy path stays exactly as it was and can go on
        being the paranoid oracle.

        Three points of WeightedBot's own semantics that are preserved here and
        are NOT the same as GreedyBot's:

        * `ctx` is computed once at the root, on the *unmutated* state, and is
          passed to every candidate.  Under the journal the root state is the
          same object the candidates mutate, so `ctx` has to be captured before
          the loop -- which `pick` already does -- and must not be recomputed
          inside it.  `rival_context` returns a plain dict of numbers, so it is
          not aliased to anything a rollback restores.
        * `end_bias` is ADDED (GreedyBot subtracts a fixed 0.01).
        * `evaluate` sits INSIDE the `except Exception: continue`, because an
          unscorable candidate must be skipped, never fatal.  GreedyBot's
          journalled loop evaluates outside its try; copying that here would
          turn a skip into a crash mid-game.

        Not reachable from `QuiescentBot`, which holds several live trial states
        at once and must stay on the copy path -- see docs/PYPY.md 9.15.  It has
        its own `_pick` and never enters this class; the only code it shares
        with this module is `evaluate`/`rival_context`, which do not mutate.
        """
        begin, rollback = journal.begin, journal.rollback
        best, best_val = None, None
        for mv in moves:
            j = begin(state)
            try:
                try:
                    actions.apply(state, mv, fresh_trial_rng())
                    val = evaluate(state, idx, w, ctx)
                except Exception:
                    continue            # the `finally` still rolls back
            finally:
                rollback(j)
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
