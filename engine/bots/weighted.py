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
                 docs/CARD_BLINDNESS.md)
    cards        civil/military hand size and summed card levels
    rivals       the best rival's culture, culture rate, science rate and
                 strength (leading is what wins, not absolute output)

`evaluate(state, idx, weights)` is the whole strategy; `WeightedBot.pick`
applies every legal move to a fast copy of the state and keeps the best.
"""
from __future__ import annotations

import os
import random
from functools import lru_cache as _lru_cache

from .. import actions, cards as C, economy, effects, journal
from . import board_yields as _BY
from . import counting
from .fastcopy import copy_state
from .trial import USE_JOURNAL, fresh_trial_rng

__all__ = ["DEFAULT_WEIGHTS", "WeightedBot", "features", "evaluate",
           "card_potential", "hand_potential", "wonder_potential",
           "rival_hand_potential", "rival_board",
           "my_seeds", "my_seeded_pending", "my_event_threat",
           "row_pressure", "row_last_copy", "load_weights",
           "save_weights"]

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

# NUMERICAL GUARD, not a model claim.  `wonder_turns_to_finish` is a ratio, so
# it blows up when resource production is near zero.  20 turns is already past
# "never" for a 20-round game, so nothing inside the cap is being shaped by it;
# it only stops an infinity reaching the linear evaluator.
_TURNS_CAP = 20.0                   # NUMERICAL GUARD (divide-by-near-zero)

# features that additionally get an early-game and a late-game copy
#
# SIX CAME OFF THIS TUPLE ON 2026-08-04, and the reason is that the thing they
# were a proxy FOR now exists.  The phase pair is an affine shape in `lateness`
# -- `w[k] + (1-L)*w[k_early] + L*w[k_late]` -- and the only honest reading of
# it was ever "a rate is worth rate x turns you will still collect it".  That
# is now modelled exactly by `rate_horizon` / `rate_multiplier` below, off the
# state's own `rounds_left`, with no fitted constant in it.  The league agrees:
# it climbed `rate_horizon` to 1.0 / 1.0 / 0.862 on the three live champions.
# The replacement shipped; the thing it replaced never got deleted, so both
# were running at once.  Two separate time models multiplying the same value.
#
#   * `culture_rate`, `science_rate`, `food_rate`, `resource_rate` -- the four
#     RATE_KEYS.  `evaluate` already multiplies these by `hz`; the pair was a
#     SECOND time shape stacked on top of the exact one.
#   * `culture` -- not a rate at all.  It is the SCORE, and `culture` is the
#     FROZEN numeraire every other weight is denominated in (see `hillclimb.
#     FROZEN`).  The pair was not frozen, so it was a live rescale of the
#     objective itself: a gauge freedom that lets the search shrink what a
#     culture point is worth instead of finding a better move.  It used it --
#     `culture_early` reached -1.3113 on the 2p champion, a NET -0.31, which is
#     an instruction to shed the score (docs/THEFT_IS_PRICED_BACKWARDS.md).
#   * `wonder_progress` -- `sum(stages[:built])`, a STOCK of resources already
#     paid in.  Time enters it only through "can I still finish", which
#     `wonder_turns_to_finish` and `wonder_overrun` own.  champion_4p had run
#     it to a net -0.17 / -0.21: a standing instruction not to build wonders.
#
# THE FOUR THAT STAY are not oversights.  `workers` (each one eats food every
# turn), `strength_rel`, `tech_levels` and `hand_value` all have a genuine
# phase dependence that is NOT "a rate times the turns left", and a
# net-negative on any of them is a strategic claim the league is entitled to
# make.  `tests/test_theft_never_helps.py::PhaseKeysAreNotDoubleModelled` is
# the ratchet: nothing in RATE_KEYS and nothing frozen may come back here.
#
# Stale `*_early` / `*_late` entries in an already-trained champion JSON are
# harmless -- `evaluate` only iterates this tuple, so a retired key is simply
# never read again.
PHASE_KEYS = (
    "workers", "strength_rel", "tech_levels", "hand_value",
)

#: Weights that USED to exist and were deliberately removed.  Named, not just
#: deleted, for two reasons that are both bugs otherwise:
#:
#: 1. Every champion JSON on disk still carries them.  `load_weights` drops
#:    them, so `evaluate` is unaffected -- but `hillclimb.mutate` iterates the
#:    CHAMPION's keys, so without this the trainer would go on perturbing
#:    twelve weights nothing reads, forever, and every generation would spend
#:    part of its mutation budget on them.
#: 2. `tests/test_coordinate_registry.py` flags any key in a vector file that
#:    DEFAULT_WEIGHTS lacks.  That ratchet is for typos and orphans; a
#:    deliberate retirement is neither, and listing 12 keys x 40 files
#:    individually would bury the signal it exists to give.
#:
#: A name here is a promise that the key is GONE, not renamed.  Re-adding one
#: means taking it out of this set in the same commit.
RETIRED_KEYS = frozenset(
    k + s for k in ("culture", "culture_rate", "science_rate", "food_rate",
                    "resource_rate", "wonder_progress")
    for s in ("_early", "_late"))

# --------------------------------------------------------------- features


# A pact you have OFFERED is not a pact: the partner may refuse.  Credit it
# at this fraction of a live pact (docs/COMBAT_AUDIT.md fix #2) -- without
# it a 1-ply search sees only the card leaving your hand, so `offer_pact` is
# strictly dominated by `pol_pass` in every position and can never be picked.
# FITTED PRIOR, and it is NOT converted to a weight the way `rival_take_share`
# is, because it is spent inside `_pending_terms`, which is called from
# `features()` -- and `features()` has no weight vector.  Routing one there
# means either a second signature on the hot path or a module-level global,
# which is what this is already.  Left as a labelled prior and written down in
# docs/OPEN_ITEMS.md instead of half-plumbed.
PACT_OFFER_CREDIT = 0.5             # FITTED PRIOR (see docs/MODEL_CONSTANTS.md)


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
# NUMERICAL GUARD, not a model claim: the fifteen Age III scoring formulas are
# unbounded above (Impact of Wonders on a five-wonder board, say), and one
# outlier margin dominating a linear evaluator is a failure mode, not a
# reading.  Nothing that fits inside +/-60 culture is shaped by this number.
_SCORING_MARGIN_CAP = 60.0          # NUMERICAL GUARD (outlier clamp)


def _scoring_weights(state, idx, ctx=None):
    """event name -> how many copies of it I should expect to score, from what
    I am ALLOWED to know.

    Two sources, and the difference between them is the whole fix:

    * **My own seeds, at weight 1.0.**  I prepared them, so I know exactly
      which Age III events they are.  `my_seeds` explains why remembering that
      is memory rather than a peek.
    * **Everything else, at `p_in_pile`.**  I do NOT know what my opponents
      put face down.  `counting.event_pool` works out which Impact cards are
      still unaccounted for and how likely any one of them is to be sitting in
      a pile slot I did not seed, from the printed deck composition, the
      revealed events, the discard and the height of the stack -- all public.

    THE BUG THIS REPLACES.  The old body called
    `events.pending_final_events(state)` directly, which walks
    `current_events + future_events` **by name**.  Those are face-down cards
    (RULES_SPEC 5.3).  So every champion trained since the feature shipped has
    been scoring the endgame off Age III events its opponents had hidden --
    reading the back of a card, not counting them.  It went unnoticed because
    docs/INFORMATION_AUDIT.md 0b sampled Age I/II positions, where this term is
    structurally 0 and the leak is invisible; "a rate of zero has two causes"
    applies to a leak measurement exactly as it does to a feature measurement.
    """
    db = C.db()
    out = {}
    for name in my_seeds(state, idx):
        if db.age_of(name) == "III":
            out[name] = out.get(name, 0.0) + 1.0
    pool = ctx.get("event_pool") if ctx else None
    if pool is None:
        pool = counting.event_pool(state, idx)
    unknown, p = pool
    if p > 0.0:
        for name, copies in unknown.items():
            if db.age_of(name) != "III":
                continue
            if not (db.get(name).get("effects") or {}).get("allPlayers"):
                continue
            out[name] = out.get(name, 0.0) + p * copies
    return out


def event_scoring_margin(state, idx, ctx=None):
    """Final-scoring culture the pending Age III events owe me, less the best
    rival's, clamped to +/-`_SCORING_MARGIN_CAP`.

    Calls the engine's own `events.final_event_awards`, so the fifteen scoring
    formulas are never restated here and cannot drift from the rules;
    `tests/test_event_scoring.py` pins the agreement.  What HAS changed is
    which events it asks about: `_scoring_weights` supplies my own seeds at
    certainty and the rest at a counted probability, instead of the old body
    reading the whole face-down pile.  The engine's own scorer still sees
    everything -- it is allowed to -- so `evaluate_final_events` is untouched.

    The awards are obtained by temporarily REBINDING `state.current_events` to
    the candidate list and restoring it in a `finally`.  `final_event_awards`
    only reads, the two attributes are rebound rather than mutated (so the
    journal's list identities are undisturbed), and nothing can observe the
    swap because it is restored before this function returns -- which is the
    cheap way to ask "what would these events pay?" without a `copy_state` on
    every single evaluation.

    Zero once `state.game_over`: `game._finish_game` has by then already paid
    these events into `p.culture`, and the decks still hold the names, so
    counting them again would double the endgame.
    """
    if state.game_over or not state.has_military:
        return 0.0
    rivals = [q.idx for q in state.players
              if q.idx != idx and not q.resigned]
    if not rivals:
        return 0.0
    weights = _scoring_weights(state, idx, ctx)
    if not weights:
        return 0.0
    from .. import events as _events
    names = list(weights)
    saved_c, saved_f = state.current_events, state.future_events
    try:
        state.current_events, state.future_events = names, ()
        awards = _events.final_event_awards(state)
    except Exception:                                      # noqa: BLE001
        return 0.0
    finally:
        state.current_events, state.future_events = saved_c, saved_f
    owed = [0.0] * len(state.players)
    for name, steps in awards:
        wt = weights.get(name, 0.0)
        if not wt:
            continue
        for i, amount in steps:
            owed[i] += wt * amount
    margin = owed[idx] - max(owed[i] for i in rivals)
    return max(-_SCORING_MARGIN_CAP, min(_SCORING_MARGIN_CAP, float(margin)))


#: Event effect-block key -> (feature key, sign).  Deliberately SEPARATE from
#: `_YIELD_TO_FEATURE` below, and the reason is not tidiness: that map is for
#: pact and colony blocks, where every key is a production RATE and the value
#: carries its own sign.  Event blocks are one-shot STOCKS and spell the sign
#: in the KEY (`loseScience` against `gainScience`), so folding the two
#: together would silently price `loseScience` as a gain.
#:
#: The keys are exactly the branches of `events.apply_gains`, and
#: `tests/test_event_outlook.py` asserts that -- an effect key the engine
#: applies and this map has never heard of is precisely the "present in one
#: list, absent from the other, and nothing fails when they disagree" shape
#: this project keeps finding.
_EVENT_YIELD = {
    "science": ("science", 1), "gainScience": ("science", 1),
    "loseScience": ("science", -1),
    "culture": ("culture", 1), "gainCulture": ("culture", 1),
    "loseCulture": ("culture", -1),
    "food": ("food_stock", 1), "gainFood": ("food_stock", 1),
    "produceFood": ("food_stock", 1),
    "resources": ("resource_stock", 1),
    "gainResources": ("resource_stock", 1),
    "produceResources": ("resource_stock", 1),
    # one block, either stock, owner's choice -- priced as food because the
    # two share a weight sign and the choice is not knowable this far out.
    "foodAndOrResources": ("food_stock", 1),
    "population": ("free_workers", 1), "gainPopulation": ("free_workers", 1),
    "increasePopulation": ("free_workers", 1),
    "decreasePopulation": ("free_workers", -1),
    "losePopulation": ("free_workers", -1),
    "yellowTokens": ("yellow_bank", 1),
    "blueTokens": ("blue_free", 1),
    "strength": ("strength", 1),
    "happiness": ("happy", 1), "happy": ("happy", 1),
    "drawMilitaryCards": ("hand_military", 1),
    # `loseAllStoredFood` carries no amount -- the amount is whatever I am
    # holding -- so it is priced from the board in `_event_block_value`.
    "loseAllStoredFood": (None, -1),
}

#: The ranked event targets, as `(effect key, ranking stat, best-first)`.
#: Same six keys `events.resolve_event` dispatches on, in the same order.
_EVENT_RANKED = (
    ("strongestPlayer", "strength", True),
    ("weakestPlayer", "strength", False),
    ("playerWithMostCulture", "culture", True),
    ("playerWithLeastCulture", "culture", False),
    ("playersWithMostHappyFaces", "happy", True),
    ("playersWithMostDiscontentWorkers", "discontent", True),
)


def _event_block_value(block, w, p, sign=1):
    """One effect block, priced through the SAME weights as the real thing.

    A pact paying +1 culture a turn is worth one `culture_rate`; an event
    handing me 3 culture is worth three `culture`.  Nothing here invents a
    per-event constant, which is what keeps `Good Harvest` and `Barbarians`
    from being the same move (docs/INFORMATION_AUDIT.md section 4.1).
    """
    if not block:
        return 0.0
    total = 0.0
    for key, raw in block.items():
        hit = _EVENT_YIELD.get(key)
        if hit is None:
            continue
        fk, ksign = hit
        if fk is None:                       # loseAllStoredFood
            total += sign * ksign * float(p.food) * w.get("food_stock", 0.0)
            continue
        try:
            v = float(raw)
        except (TypeError, ValueError):
            continue                         # a nested/dict payload, not a scalar
        total += sign * ksign * v * w.get(fk, 0.0)
    return total


def my_seeds(state, idx):
    """The names **I** put in the politics pile that have not resolved yet.

    LEGALITY IS THE WHOLE DESIGN HERE, and it is why both terms built on this
    shipped without a determinization change.  This reads `state.seeded_by`
    and keeps only the names whose owner is `idx`.  Preparing an event places
    it face DOWN (docs/RULES_SPEC.md 5.3), so the pile's contents are hidden
    from everyone else -- but *you put it there*, so remembering your own
    seeds is memory, not a peek, and it is the skill the project owner listed
    first.  Nothing here reads a name somebody else seeded and nothing reads
    pile ORDER, so there is no way for a deeper search to turn either term
    into a leak.  That is a strictly stronger property than the row terms
    have -- those needed `root_row_budget` -- and it is deliberate.
    """
    seeded = state.seeded_by
    if not seeded:
        return ()
    return tuple(n for n in list(state.current_events)
                 + list(state.future_events) if seeded.get(n) == idx)


def my_seeded_pending(state, idx):
    """Summed age level of my unresolved seeds: how much event I am owed.

    A plain board count in printed units, so it belongs in `features()` --
    unlike :func:`my_event_threat`, which is priced through `w`.
    """
    db = C.db()
    return float(sum(C.level(db.get(n)["age"])
                     for n in my_seeds(state, idx) if n in db.by_name))


def my_event_threat(state, idx, w):
    """Signed, DIFFERENCED value to me of the events I planted myself.

    Priced against the board as it stands now, through the same weights as
    the real thing -- so a `Good Harvest` I planted and a `Barbarians` I
    planted stop being the same move, which docs/INFORMATION_AUDIT.md section
    4.1 names as the consequence of this blind spot.

    Differenced for the same reason `event_scoring_margin` is: a `Barbarians`
    that hits the strongest player is good news when the strongest player is
    somebody else.

    Two bounds, stated rather than hidden:

    * **`allPlayers` blocks are skipped.**  They hit me and every rival
      alike, so their differential value is zero, which is what this term
      measures.  It is NOT that they do not matter -- `increasePopulation` is
      worth more to a full yellow bank -- it is that pricing that asymmetry
      needs a per-rival board model this does not have.
    * **The ranking is TODAY's.**  Whether I am still the strongest player
      when the card actually turns up is unknowable; the board I can see is
      the only honest estimate, and it is the one a human at the table uses.
    """
    mine = my_seeds(state, idx)
    if not mine:
        return 0.0

    from .. import events as _events
    db = C.db()
    order = [q for q in state.players if not q.resigned]
    pkey = f"{len(state.players)}p"
    threat = 0.0

    for name in mine:
        if name not in db.by_name:
            continue
        eff = db.get(name).get("effects") or {}

        for key, stat, best in _EVENT_RANKED:
            block = eff.get(key)
            if not isinstance(block, dict):
                continue
            try:
                ranked = _events._rank(state, order, stat, best)
            except Exception:                              # noqa: BLE001
                continue
            if not ranked:
                continue
            target = ranked[0]
            v = _event_block_value(block, w, target)
            # Mine to enjoy, or mine to have handed a rival.
            threat += v if target.idx == idx else -v

        for key, best in (("strongestPlayers", True),
                          ("weakestPlayers", False)):
            if key not in eff:
                continue
            count = (eff[key] or {}).get(pkey, 1)
            block = eff.get("gain") if best else eff.get("lose")
            sign = 1 if best else -1
            if key == "weakestPlayers" and eff.get("gain"):
                block, sign = eff["gain"], 1
            try:
                ranked = _events._rank(state, order, "strength", best)[:count]
            except Exception:                              # noqa: BLE001
                continue
            for q in ranked:
                v = _event_block_value(block, w, q, sign=sign)
                threat += v if q.idx == idx else -v / max(1, len(order) - 1)

    return threat


def attack_targets(state, idx):
    """Who I am currently attacking: rival indices, deduplicated.

    Two places hold it and both are public the moment they are written -- an
    aggression is declared openly and a war is a card on the table:

    * `state.pending`, a `kind == "defense"` frame whose `attacker` is me
      (`interact.start_defense`), which is where an aggression lives between
      being declared and being resolved;
    * `p.war_declared_by_me`, the `(name, attacker, defender)` triple
      (`engine/state.py:128`).

    Nothing read either before this.  `tools/target_blindness.py` measured the
    consequence: holding the CARD fixed and varying only the TARGET, the
    one-ply evaluator returned a bit-identical score for 74.3% of `war`
    choices and 76.6% of `aggression` choices over 60 seeds x {3p, 4p}, with
    `cancel_pact` at 0.0% as the control proving the measurement separates
    targets when anything at all prices them.  Whichever opponent the bot
    "chose", it chose by tie-break.
    """
    out = []
    for pend in (state.pending or ()):
        if pend.get("kind") == "defense" and pend.get("attacker") == idx:
            d = pend.get("player")
            if d is not None and d != idx:
                out.append(d)
    war = state.players[idx].war_declared_by_me
    if war:
        try:
            d = war[2]
        except (TypeError, IndexError):
            d = None
        if d is not None and d != idx:
            out.append(d)
    return sorted(set(out))


def pact_partner_lead(state, idx):
    """Culture lead of whoever I am offering a pact to, over my other rivals.

    `deferred_credit` already prices the partner's SIDE of the deal, by
    feeding their yields into the `rival_*` maxima (`4e64780`).  That moved
    `offer_pact`'s measured target blindness off 100% but only to ~50%, and
    the reason is structural rather than a missing weight: those terms are a
    MAX over rivals, so lifting a rival who is not already the maximum moves
    nothing at all.  Two different partners then score identically again.

    This is the same shape of answer as :func:`attack_target_terms`: who they
    ARE, not just what the deal pays them.  Handing the runaway leader an
    alliance is a different move from handing it to last place even when the
    printed effects are symmetric.
    """
    lead = 0.0
    for pend in (state.pending or ()):
        if pend.get("kind") != "choice" or pend.get("tag") != "pact_offer":
            continue
        if (pend.get("ctx") or {}).get("owner") != idx:
            continue
        partner = pend.get("player")
        if partner is None or partner == idx:
            continue
        q = state.players[partner]
        others = [r for r in state.players
                  if r.idx not in (idx, partner) and not r.resigned]
        base = (sum(r.culture for r in others) / len(others)) if others else \
            float(q.culture)
        lead += float(q.culture) - base
    return lead


def attack_target_terms(state, idx):
    """`(lead, weakness)` -- WHO I picked, in the two ways it matters.

    `lead` is the target's culture less the mean culture of my *other* rivals.
    Positive means I am hitting the player who is ahead, which is the whole
    point of an attack in a three- or four-player game and is the single
    decision class 2p cannot exercise at all.

    `weakness` is my strength less the target's.  Positive means I picked
    someone I outgun, which is what makes the attack land rather than hand
    them a defensive card's worth of tempo.

    Both are plain board scalars in printed units, so they are `features()`
    keys rather than eval-only terms -- no weight is consulted here.

    **At 2p `lead` is structurally 0** (there are no "other rivals" to average
    over) and that is correct rather than a gap: with one opponent there is no
    target to choose.  `weakness` still reads at 2p, because "am I attacking
    into strength" is a question at every player count.  Do not read a 2p zero
    on `lead` as blindness; read it as the decision not existing.
    """
    targets = attack_targets(state, idx)
    if not targets:
        return 0.0, 0.0
    me = state.players[idx]
    my_strength = effects.compute(state, me).strength
    lead = weakness = 0.0
    for t in targets:
        q = state.players[t]
        others = [r for r in state.players
                  if r.idx not in (idx, t) and not r.resigned]
        base = (sum(r.culture for r in others) / len(others)) if others else \
            float(q.culture)
        lead += float(q.culture) - base
        weakness += float(my_strength - effects.compute(state, q).strength)
    return lead, weakness


def rival_board(state, idx):
    """The public rival board facts nothing scored, as a flat dict.

    Every one of these is on the table in the physical 2015 base game -- the
    stocks sit in front of their owner, the workers stand on the cards, the
    colony and wonder cards are face up -- so all of it is legal under the
    standing rule in docs/INFORMATION_AUDIT.md section 0c.  They were invisible
    only because nobody had written the four lines.

    `max` over rivals throughout, matching `rival_free_ca` and friends, so
    each term means the same thing at 2p, 3p and 4p.  The exception is
    `rival_building_wonder`, which is a COUNT: "somebody is mid-wonder" and
    "everybody is mid-wonder" are different rows to play into, and a max over
    a boolean would flatten them to the same 1.
    """
    rivals = [q for q in state.players if q.idx != idx and not q.resigned]
    if not rivals:
        return {"rival_science_stock": 0.0, "rival_food_stock": 0.0,
                "rival_resource_stock": 0.0, "rival_free_workers": 0.0,
                "rival_yellow_bank": 0.0, "rival_colonies": 0.0,
                "rival_mil_actions": 0.0, "rival_building_wonder": 0.0}
    return {
        "rival_science_stock": float(max(q.science for q in rivals)),
        "rival_food_stock": float(max(q.food for q in rivals)),
        "rival_resource_stock": float(max(q.resources for q in rivals)),
        "rival_free_workers": float(max(q.workers_free for q in rivals)),
        "rival_yellow_bank": float(max(q.yellow_bank for q in rivals)),
        "rival_colonies": float(max(len(q.colonies) for q in rivals)),
        "rival_mil_actions": float(max(q.military_actions for q in rivals)),
        # docs/EXPERT_STRATEGY.md:546 -- "they cannot take a wonder while one
        # is unfinished" is the cheapest read in the game on whether a wonder
        # in the row is safe to let slide, and the engine already snapshots
        # `q.wonder` into `_RivalView` for the legality gate without ever
        # scoring it.
        "rival_building_wonder": float(sum(1 for q in rivals
                                           if q.wonder is not None)),
    }


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
    the gain (docs/COMBAT_AUDIT.md):

    * ``offer_pact`` -- the pact object is created in the partner's response,
      so the mover sees only a card leaving its hand.  Credited at
      :data:`PACT_OFFER_CREDIT` of the side `idx` would take.
    * ``bid`` while rivals are still bidding -- mutates only the auction
      dict, so every bid scored EXACTLY equal to ``bid_pass`` and the
      strict-`>` tie-break always took the pass at index 0.  Credited at
      1/(1+rivals still in) of the territory's own effects.

    THERE IS A THIRD ONE NOW AND IT IS NOT HANDLED HERE: docs/OPEN_ITEMS.md
    §2 item 29.  Since RULES_SPEC 11.3's sacrifice became a real decision
    (`interact.colonize`), the *winning* ``bid_pass`` no longer finishes the
    colonization inside `apply` whenever the force is a genuine choice -- it
    leaves a ``colonize`` pending, so the trial state shows neither the
    sacrifice nor the colony, and the ``auction`` branch below has already
    stopped matching because the top of the stack is no longer an auction.
    The fix is a ``colonize`` sibling to that branch at share 1.0 (the winner
    is decided, so it is not a share at all).  Left out deliberately so the
    rules change that created it had one cause in `tools/gate.sh`.

    Returns ``(pact_offers, auction_committed, auction_bid, blocks_attack,
    gains, partner_gains)``, where `gains` maps feature keys to deferred
    amounts for `idx` and `partner_gains` maps a rival's index to the same
    for the side of the pact THEY would take.
    """
    db = C.db()
    gains = {}
    partner_gains = {}
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
            # THE OTHER HALF OF THE DEAL.  Pricing only my own side made
            # every partner identical: measured on 257 live decisions
            # (experiments/logs/census, 2026-08-01), 135 of the 144 exact
            # score ties on `offer_pact` were the SAME card offered to a
            # DIFFERENT opponent at a bit-identical score, so the bot chose
            # its allies by tiebreak.  A pact handed to the runaway leader
            # and the same pact handed to last place are not the same move
            # -- `book.py` has known that since it was written.
            #
            # Differenced, not own-only, for exactly the reason
            # `event_scoring_margin` is: a deal that pays me 8 and my rival
            # 14 is a bad deal, and a feature that only sees the 8 rates it
            # a good one.  No new coordinate: the partner's yields are fed
            # into the `rival_*` maxima the evaluator already prices, so the
            # weight the hill climb has already tuned for "a rival producing
            # this much" is the one that charges for the gift.
            if partner is not None and partner != idx:
                pg = partner_gains.setdefault(partner, {})
                for block in effects._pact_blocks(pact, partner):
                    _add_block(pg, block, PACT_OFFER_CREDIT, state, partner,
                               idx)
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
    return offers, committed, bid_cost, blocks_attack, gains, partner_gains


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
                 "techs", "government", "hand_slack")

    def __init__(self, q, hand_slack=0):
        # how many more civil cards this rival's hand can hold (RULES_SPEC
        # 2.5).  `hand_size`, not `len(hand_civil)`, so the app harness's
        # `hidden_civil` count is included; read by `row_pressure`.
        self.hand_slack = hand_slack
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


def rival_context(state, idx, root_row=None, root_counts=None):
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

    `root_counts` is the same argument, one zone over.  `engine.bots.counting`
    reads the card row, the discard, the past events and the height of each
    deck, and a trial `apply` refills the row and reveals events -- so counting
    on a searched state would let the search see the very card it just drew.
    Computed once here at the root, threaded down by the plan bots exactly as
    `root_row` is, and every caller that rebuilds this context on a trial state
    MUST pass both.  It is a pair `(civil_outlook, event_pool)`, built together
    because the two are always wanted together and neither is worth a second
    threading parameter.
    """
    counts = root_counts if root_counts is not None else (
        counting.civil_outlook(state, idx), counting.event_pool(state, idx))
    best_rate = best_sci = best_str = 0
    rates = {}
    views = []
    for q in state.players:
        if q.idx == idx or q.resigned:
            continue
        s = effects.compute(state, q)
        best_rate = max(best_rate, s.culture)
        best_sci = max(best_sci, s.science)
        best_str = max(best_str, s.strength)
        # PER-RIVAL, not just the max: `deferred_credit` needs to know what a
        # SPECIFIC rival already produces to work out what offering them a
        # pact would do to the maximum.  Free here -- `effects.compute` has
        # already run -- and impossible to reconstruct downstream without
        # paying for it again on every candidate.
        rates[q.idx] = (s.culture, s.science, s.strength)
        gate = (s.civil_actions,
                q.hand_size("civil") >= s.civil_actions + s.civil_hand_limit,
                0 if q.leader == "Michelangelo"
                else len(q.completed_wonders) + q.destroyed_wonders,
                1 if q.leader == "Hammurabi" else 0)
        views.append((_RivalView(q, max(0, s.civil_actions
                                        + s.civil_hand_limit
                                        - q.hand_size("civil"))), gate))
    return {"rival_culture_rate": best_rate, "rival_science_rate": best_sci,
            "rival_strength": best_str, "rival_views": tuple(views),
            "rival_rates": rates,
            "root_row": root_row_budget(state) if root_row is None
            else root_row,
            "root_counts": counts,
            "civil_outlook": counts[0], "event_pool": counts[1]}


def rival_strength(state, idx):
    """`rival_context(state, idx)["rival_strength"]`, on its own.

    `card_potential` is not handed a `ctx` -- `hand_potential` and
    `_hand_total` do not carry one -- and building a whole `rival_context`
    per card priced would recompute every opponent's full statistics several
    times per candidate move.  This is the one field of it `strength_marginal`
    needs, off the cached `state_stats` rather than a bare `compute`.

    A SECOND SPELLING OF ONE QUANTITY, so it is guarded like one:
    `tests/test_unit_pricing.py:TestRivalStrengthAgrees` walks self-play
    positions and fails if this and `rival_context` ever disagree -- the same
    device `_SWEEP` and `game.SWEEP` are held together with.
    """
    best = 0
    for q in state.players:
        if q.idx == idx or q.resigned:
            continue
        s = effects.state_stats(state, q).strength
        if s > best:
            best = s
    return best


def strength_marginal(state, idx, w, ctx=None):
    """What ONE point of strength is worth to `evaluate` on THIS board.

    d(`evaluate`)/d(`features()["strength"]`), evaluated exactly -- not a
    proxy for it and not a constant.  `card_potential` looks up `w["strength"]`
    and stops, and docs/CARD_BLINDNESS.md section 11.5.1 measured what
    that costs: the board expresses a point of strength through FOUR features
    and the card sees one of them, so a unit's gain is under-counted by
    between 2.3x and 7x, in a way that depends on the board and therefore
    cannot be folded into a credit constant.

    The four channels, each read straight off `features()`' own expression:

        strength            d/ds = 1, always
        strength_rel        = s - rival, so d/ds = 1, always -- and this is
                              the one strength feature in `PHASE_KEYS`, so its
                              early/late multipliers belong here too.  On the
                              live 2p champion that is the whole story:
                              `strength_rel` itself is 0.0 and
                              `strength_rel_early`/`_late` are 3.37 / 2.36.
        strength_deficit    = max(0, -rel), so d/ds = -1 while behind, 0 ahead
        strength_lead       = min(6, max(0, rel)), so d/ds = +1 while ahead by
                              less than six, 0 once the lead is capped

    The two conditional channels are why this is a board query.  A unit card
    is worth more when you are behind and worth nothing extra once your lead
    is capped, and no per-card table can say that.

    This is a LOCAL derivative and it is honest about being one: it does not
    see the clamp at `s.strength = max(0, ...)` (a rating cannot go negative,
    but a player at zero strength gains the full point anyway) and it treats
    the deficit/lead kink at `rel == 0` as right-differentiable.  Both are
    exact everywhere except at the kink itself.
    """
    if ctx is not None:
        rival = ctx["rival_strength"]
    else:
        rival = rival_strength(state, idx)
    p = state.players[idx]
    rel = effects.state_stats(state, p).strength - rival
    late = lateness(state)
    total = w.get("strength", 0.0) + w.get("strength_rel", 0.0)
    total += (1.0 - late) * w.get("strength_rel_early", 0.0)
    total += late * w.get("strength_rel_late", 0.0)
    if rel < 0:
        total -= w.get("strength_deficit", 0.0)
    elif rel < 6:
        total += w.get("strength_lead", 0.0)
    return total


_PHASE_SET = frozenset(PHASE_KEYS)


def feature_marginal(key, state, idx, w, late=None, ctx=None):
    """What ONE unit of `features()[key]` is worth to `evaluate` on THIS board.

    `strength_marginal`, generalised -- and generalising it is the whole of
    this lane's diagnosis.  `evaluate` prices a `PHASE_KEYS` feature at

        w[k] + (1 - L) * w[k_early] + L * w[k_late]

    and `card_potential` looked up the bare `w[k]`, so it read a card's yield
    through a number the evaluator does not use.  On the live 2p champion
    (gen 72) the two differ by more than an order of magnitude:

        feature        w[k]     marginal early   marginal late
        science_rate   0.250    5.291            -0.009
        tech_levels    5.841    9.230             6.759
        culture_rate  31.678   31.292            33.169

    -- so a laboratory's two science was priced at 0.50 where `evaluate` pays
    10.58 for it early, and its two levels of technology were priced at
    nothing at all where `evaluate` pays 18.46.  That is why every yellow
    production technology came out strictly negative and `row_pressure`'s
    `if val <= 0.0: continue` then made the whole colour invisible
    (docs/CARD_BLINDNESS.md).

    `strength` is delegated rather than special-cased twice: the board
    expresses a point of army through four features and `strength_marginal` is
    the one place that sums them.  Every other key is linear in exactly one
    feature, so its marginal IS its weight, plus the phase pair when it has
    one.  `late` is threaded in by callers that price several triples off one
    board, because `lateness` is not free.
    """
    if key == "strength":
        return strength_marginal(state, idx, w, ctx)
    m = w.get(key, 0.0)
    if key in _PHASE_SET:
        if late is None:
            late = lateness(state)
        m += (1.0 - late) * w.get(key + "_early", 0.0)
        m += late * w.get(key + "_late", 0.0)
    # THE RATE HORIZON.  `evaluate` prices a rate at `blend * horizon`, so its
    # marginal is too -- and because this function is the single definition of
    # "what one unit of this feature is worth", every card-pricing site picks
    # the horizon up for free and cannot disagree with `evaluate` about it.
    # That is the whole reason the horizon lives on the PRICE rather than on
    # the feature: there is exactly one place to put it.
    if key in RATE_KEYS:
        hz = rate_multiplier(state, w)
        if hz != 1.0:
            m *= hz
    return m


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
# THE DEAL RATE IS NOT A CONSTANT, AND THE STATE CAN MEASURE IT.  The old
# `CARDS_PER_ROUND = {2: 6.29, 3: 6.73, 4: 5.71}` was ONE EXACT NUMBER PLUS ONE
# FITTED ONE glued together:
#
#     cards dealt per round = n * SWEEP[n]        <- EXACT, RULES_SPEC 2.1
#                           + cards players took  <- policy, 0-4 per round
#
# `n * SWEEP[n]` is 6 / 6 / 4, so the fitted remainder was only 0.29 / 0.73 /
# 1.71 cards a round -- and it was fitted on WeightedBot self-play by a policy
# that could not see the card row at all.  Its own comment conceded the failure
# mode ("a much more card-hungry policy would drain the row faster and this
# would then run long"), and that failure mode ARRIVED: `unit_tech_credit`
# (d8a2172) and `tech_board_credit` (8b972ef) exist precisely to make the bot
# take more cards, so a rate fitted to the card-blind policy is now being used
# to plan against the card-hungry one.
#
# The take count is not hidden.  It is a difference of two exact public
# quantities -- how many cards the supply started with and how many are still
# undealt -- so the bot can MEASURE the rate in the game it is actually
# playing.  `take_rate` below does that, and `_TAKE_PRIOR` is used only for the
# first round or two, when there is no history yet.  See docs/MODEL_CONSTANTS.md.
AGE_IV_ROUNDS = 2.0      # RULE-DERIVED, 12.3: Age IV is this round or the next

# `game.SWEEP`, duplicated because `engine.game` imports `engine.actions` which
# this module imports -- importing game here is a cycle.  RULE-DERIVED
# (RULES_SPEC 2.1); `test_row_features.py` asserts the two are equal, so the
# copy cannot rot.
_SWEEP = {2: 3, 3: 2, 4: 1}

# DEAD-CODE FIX (2026-08-05): this module used to also define `_ROW =
# actions.ROW_SIZE` here ("row width... spelled here so the supply arithmetic
# below reads as one block").  Nothing below it -- or anywhere else in this
# file -- ever read `_ROW`; grepping the whole tree for the name turns up only
# its own definition.  Removed rather than ported, on the repo owner's ruling
# that a dead artifact found while porting gets fixed in both engines, not
# carried forward for shape fidelity.  `tests/test_rate_horizon.py::
# DeadCodeFoundWhilePortingToRust` pins this so it cannot grow back unnoticed.

# FITTED PRIOR, and the ONLY fitted number left in the horizon: cards taken off
# the row per replenish before this game has produced any evidence of its own.
# Measured over 240 self-play games (tools/deal_rate.py, deleted 2026-08-04;
# docs/MODEL_CONSTANTS.md
# section 2).  It is shrunk away within a couple of rounds -- `_TAKE_PRIOR_W` is
# its weight in pseudo-replenishes -- so it moves the estimate only in Age A and
# the first rounds of Age I, where nothing with a rate horizon is decided.
_TAKE_PRIOR = {2: 0.30, 3: 0.35, 4: 0.40}
_TAKE_PRIOR_W = 4.0

# Cards left in the decks of every age AFTER the given one, by live player
# count.  Built once; `C.db().civil_deck` is far too slow for the search loop.
_TAIL = {}
# Total civil cards in the whole game's supply (ages A/I/II/III), and the size
# of the Age A deck on its own, by live player count.  Both exact card data.
_SUPPLY = {}


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


def _supply(n):
    """(total civil cards for `n` players, size of the Age A deck)."""
    out = _SUPPLY.get(n)
    if out is None:
        db = C.db()
        per = {a: len(db.civil_deck(a, n)) for a in C.AGES if a != "IV"}
        out = (sum(per.values()), per["A"])
        _SUPPLY[n] = out
    return out


def _live(state):
    """`game.live_count` without importing game (circular).  RULES_SPEC 13."""
    n = 0
    for p in state.players:
        if not p.resigned:
            n += 1
    return 2 if n < 2 else (4 if n > 4 else n)


def cards_unseen(state, n=None):
    """Civil cards still to be dealt.  EXACT: the current deck plus every
    later age's deck, whose sizes are fixed by the card data and `n`."""
    if n is None:
        n = _live(state)
    return len(state.civil_deck) + _tail(n, state.age_civil)


def _replenishes(state, n):
    """How many times `game._replenish` has run.  EXACT.

    `start_turn` replenishes at the top of every turn from round 2 on
    (`engine/game.py`), and `state.turn` is a 1-based player-turn counter, so
    this is the turn counter less the `num_players` turns of round 1.
    """
    return state.turn - state.num_players


def take_rate(state, n=None):
    """Cards players take off the row per replenish, MEASURED in this game.

    Not a constant and not a guess after the first round: the supply started
    with `_supply(n)[0]` cards, `cards_unseen` of them are still undealt, and
    everything in between went somewhere the rules account for exactly --

        consumed = the 13 dealt at setup
                 + the Age A deck's leftovers, binned by the first replenish
                   (2.1/1.10 -- `_replenish` empties `civil_deck` in Age A)
                 + SWEEP[n] per replenish since
                 + one per card a player TOOK

    so the last term, the only policy-dependent one, is the other four
    subtracted from `consumed`.  Every input is public: deck sizes are card
    data, `civil_deck`'s length is a count and not an order (the encoder's
    hidden-information rule, `neural_encode.py`), and the number of replenishes
    is the turn counter.

    Shrunk toward `_TAKE_PRIOR` with weight `_TAKE_PRIOR_W`, which is what
    covers Age A and the opening rounds, where there is no history to measure.
    """
    if n is None:
        n = _live(state)
    r = _replenishes(state, n)
    if r <= 0 or state.age_civil == "A":
        return _TAKE_PRIOR[n]
    total, age_a = _supply(n)
    consumed = total - cards_unseen(state, n)
    taken = consumed - age_a - r * _SWEEP[n]
    if taken < 0.0:
        taken = 0.0
    return (taken + _TAKE_PRIOR_W * _TAKE_PRIOR[n]) / (r + _TAKE_PRIOR_W)


# REMOVED 2026-08-04: the three A/B hatches (`TTA_LEGACY_DEAL_RATE`,
# `TTA_LEGACY_LATENESS`, `TTA_LEGACY_ROW_TAKE`) and the `horizon_legacy` /
# `horizon_age` weight-vector forms of them.  Each restored one retired fitted
# constant so the old model and its replacement could be duelled.
#
# They went because the method they served went: paired-arm A/B batches were
# retired on 2026-07-31 in favour of landing on master and reading the real
# league runs, and nothing has set any of the three since.  Verified before
# deleting: no process in the crontab or the supervisor sets them, and no
# weight vector on disk carries a non-zero `horizon_legacy` or `horizon_age`.
#
# The deletion is not cosmetic.  `lateness` took a `w` ONLY so the hatch could
# be read off a vector, which forced two call spellings -- `lateness(state)`
# and `lateness(state)` -- that returned the same number for every vector that
# has ever existed but had to be threaded separately in case one day they did
# not.  That is the `late_tech`/`late_action` pair, now a single `late`.


def rounds_left(state, n=None):
    """Estimated rounds still to play, including the one in progress.

    Exact once Age IV has begun.  Before that it is the EXACT number of cards
    still to deal divided by the deal rate -- `n * SWEEP[n]` swept per round,
    which is exact, plus `n * take_rate(state)` taken, which is measured off
    this game rather than assumed.  Never returns less than 1.0.
    """
    fre = state.final_round_end
    if fre is not None:
        return max(1.0, float(fre - state.round + 1))
    if n is None:
        n = _live(state)
    cards = cards_unseen(state, n)
    per_round = n * (_SWEEP[n] + take_rate(state, n))
    return max(1.0, cards / per_round + AGE_IV_ROUNDS)


def lateness(state):
    """How far through the game we are: 0.0 at the deal, 1.0 when the civil
    supply is gone.  EXACT -- no rate, no fit, no player-count table.

    WHAT IT IS.  `1 - cards_unseen / supply`: the fraction of the game's civil
    cards that have already left the decks.  Both endpoints are rule-derived
    rather than chosen.  0 is the deal, when nothing has been dealt yet; 1 is
    the moment the Age III deck runs out, which is not a milestone somebody
    picked but THE thing that ends the game (RULES_SPEC 12.2/12.3 -- Age IV
    begins, `_set_last_round` fixes `final_round_end`, play stops).  Age IV
    therefore sits at exactly 1.0 by construction, because `civil_deck` is
    empty and `_tail` past Age III is zero.

    WHAT IT REPLACES.  An affine map `(z - rounds_left) / (z - 5)` with
    `z = 27.1 / 28.7 / 36.1` per player count, least-squares fitted to
    reproduce the OLD age-bucket gauge on 46 self-play games.  Three fitted
    constants, a per-player-count table, and a dependence on the ESTIMATED
    `rounds_left` -- to express a quantity the state knows exactly.  The two
    agree closely in slope; they differ in where the gauge saturates, which is
    the whole non-affine content of the change (see the paragraph below).

    BOUNDED TO [0, 1] BY CONSTRUCTION, and the clamp is kept anyway because it
    is load-bearing.  The old line reached ~1.1 with two rounds left, which
    makes `1 - L` negative and FLIPS THE SIGN of every `_early` term.
    Measured, n=400 head-to-head (docs/CULTURE_GAP.md section 8d): unclamped,
    the 4p champion drops to 19.9% against a 25% null and the 3p champion to
    13.6% against 33.3%.  The mechanism on the 4p champion is exact -- its
    `culture_early` is 8.792, so at `1 - L = -0.096` its own culture is priced
    at `1.000 - 0.096*8.792 = 0.156` instead of the frozen 1.000, i.e. it stops
    caring about the score in the last two rounds.  `1 - L` outside [0, 1] is
    an extrapolation beyond anything the search has ever been scored on, and
    linear evaluators do not extrapolate.  `tests/test_model_constants.py`
    asserts the bound on adversarial states -- a supply larger than the game
    holds, a negative round counter, a fabricated deck.

    THE ONE BEHAVIOURAL DIFFERENCE, stated plainly.  The phase blend is
    `w[k] + (1-L)*w[k_early] + L*w[k_late]`, so an AFFINE change of L is pure
    GAUGE for the weight CLASS (it is absorbed by rescaling the phase pair),
    though not for a FIXED already-trained vector.  The new gauge's slope in
    rounds is within ~10% of the old one; what genuinely differs is that the
    old one saturated at 1.0 about five rounds from the end and this one
    reaches 1.0 only when the supply does.  docs/MODEL_CONSTANTS.md section 5
    measures what that costs the trained 2p champion instead of assuming it.
    """
    n = _live(state)
    lv = 1.0 - cards_unseen(state, n) / float(_supply(n)[0])
    if lv <= 0.0:
        return 0.0
    return lv if lv < 1.0 else 1.0


# ----------------------------------------------------- the rate horizon
#
# THE HOLE THIS FILLS, stated as arithmetic rather than as a strategy opinion.
#
# `culture` is FROZEN at 1.0, so every weight in the vector is denominated in
# culture points.  A feature whose value is a PER-TURN RATE is therefore worth
# `rate x turns you will still collect it`, and that multiplier is a quantity
# the state knows: `rounds_left`.  The evaluator does not use it.  Before this
# change `rounds_left` was read by exactly ONE feature in the whole vector --
# `wonder_overrun` -- and by nothing that prices a rate.  Every rate went
# through
#
#     w[k] + (1 - L) * w[k_early] + L * w[k_late]
#
# which is an affine function of a [0, 1] SHAPE, not a multiplication by a
# horizon.  The class of vectors CAN express a horizon that way (rounds-left is
# roughly affine in `1 - L`), and the search demonstrably does not find it:
# docs/CULTURE_GAP.md section 11 ablates `culture_rate_early`/`_late` at 2p to
# an edge of exactly 0.0000 +/- 0.0000 -- deleting them changed not one game --
# while `culture_rate` next to them is worth 0.11-0.18 win share.  The live 2p
# champion (gen 84) carries `culture_rate` 31.678 with a phase pair of
# (-0.386, +1.492), i.e. it prices +1 culture/turn at ~31-33 culture points at
# EVERY moment of the game.  A 2p game is ~23 rounds, so 31.7 is above the
# theoretical ceiling everywhere and roughly 30x the truth on the last turn.
# That is the same mispricing docs/CULTURE_GAP.md section 23b declined to
# retract, and it is uniform across rates, not a late-game special case:
# `science_rate`, `food_rate` and `resource_rate` have exactly the same shape.
#
# WHAT IT MULTIPLIES BY, and why there is no new constant in it.  A rate is
# scaled by `rounds_left / mean rounds_left`, and BOTH halves come off the
# state:
#
#     total   = rounds_left + (round - 1)     <- rounds played + rounds to play
#     ref     = (total + 1) / 2               <- mean of rounds-left over a game
#     scale   = rounds_left / ref
#
# `rounds_left` is already exact once Age IV begins and is otherwise the exact
# count of undealt civil cards over a deal rate MEASURED IN THIS GAME
# (`take_rate`); see its docstring.  `ref` is the mean of `rounds_left` over
# the rounds of one game: it decrements by one per round from `total` to 1, so
# its mean is `(total + 1) / 2` -- arithmetic, not a fit.  `total` is estimated
# afresh at every decision from the same live quantities, so a game that runs
# long or short renormalises itself and there is no per-player-count table.
#
# WHY MEAN-NORMALISED RATHER THAN RAW `rounds_left`.  The choice of divisor is
# pure GAUGE for the weight class -- any constant factor is absorbed by
# rescaling `w[k]` -- so it is spent, exactly as docs/CULTURE_GAP.md section 8b
# spent it, on disturbing an ALREADY-TRAINED vector as little as possible.
# Mean-normalising leaves `w["culture_rate"]` meaning what it means today ("a
# rate point at an average moment of the game") and changes only the SHAPE,
# which is the entire content of the change.  A raw multiplier would have
# confounded "the horizon is now visible" with "every rate weight just got
# divided by twelve".
#
# THE CREDIT.  `rate_horizon` is a tunable weight, default 0.0, and 0.0
# reproduces master byte-for-byte -- the gate digests do not move until a
# vector asks for it.  1.0 is the full horizon.  Intermediate values are a
# genuine blend and not a switch, which is what makes it a slope the league can
# climb rather than a step it has to jump; `tools/rate_horizon_ab.py --ladder`
# (deleted 2026-08-04)
# measures that it is one.
#
# The one-shot side of the trade needs no term at all, and that is the point:
# a one-shot culture payoff (a completed Age III wonder, an Age III scoring
# event) already lands on `culture`, whose weight is FROZEN at 1.0 and does not
# move with the horizon.  Rates decaying toward zero IS the endgame preference
# for one-shots.  There is deliberately no late-game branch anywhere in this.
# The four rates the player COLLECTS.  `rival_culture_rate` and
# `rival_science_rate` are deliberately NOT here: they are max-over-rivals
# threat signals, the coordinate registry declares them inert across a
# candidate set (`tests/test_coverage_tools.py:TestInertFeatures`), and
# scaling them made them vary between candidates as a side effect -- a
# behavioural change nobody asked for, which is exactly what this lane is
# supposed to avoid smuggling in.  The argument that a rival's rate also
# only pays for the turns remaining is sound and is docs/OPEN_ITEMS.md
# item 2.31, not this change.
RATE_KEYS = frozenset((
    "culture_rate", "science_rate", "food_rate", "resource_rate",
))


def horizon_scale(state, n=None):
    """`rounds_left`, normalised so an average-moment decision scores 1.0.

    Mean ~1.0 over a game by construction; ~1.9 at the deal and ~0.09 on the
    last turn.  Never negative, because `rounds_left` never is.

    DEAD-PARAMETER FIX (2026-08-05): used to also take `w=None` and never
    read it -- the body never referenced `w`, so `rate_multiplier` and three
    tests built a full weight dict only to hand it to a parameter that
    discarded it.  Dropped; see `tests/test_rate_horizon.py::
    DeadCodeFoundWhilePortingToRust`.
    """
    rl = rounds_left(state, n)
    ref = 0.5 * (rl + max(0.0, state.round - 1.0) + 1.0)
    return rl / ref if ref > 0.0 else 1.0


def rate_multiplier(state, w, n=None):
    """What `RATE_KEYS` features are multiplied by.  1.0 when the credit is off.

    Blended so `rate_horizon` is a slope: 0.0 is master, 1.0 is the full
    `rounds_left / mean` horizon.  Floored at 0.0 so a credit above 1.0 -- which
    the league is free to propose -- can flatten a rate but never invert its
    sign, the failure mode docs/CULTURE_GAP.md section 8b(i) measured when an
    unclamped horizon drove `1 - L` negative.
    """
    c = w.get("rate_horizon", 0.0) if w is not None else 0.0
    if not c:
        return 1.0
    return max(0.0, 1.0 + c * (horizon_scale(state, n) - 1.0))


def features(state, idx, ctx=None, w=None, priced_only=False):
    """The raw feature vector from player `idx`'s point of view.

    `priced_only` is a SPEED switch and nothing else.  `evaluate` multiplies
    every entry here by `w[k]` and skips the term entirely when that weight is
    falsy, so any entry whose weight is 0.0 was computed and thrown away.  That
    is free for the arithmetic-only entries and it was NOT free for
    `event_scoring_margin`, which asks the engine to score fifteen final-event
    formulas: profiling the coordinate-registry corpus put it at 24s of 109s --
    22% of ALL evaluation time -- on a vector that prices it at 0.0.  This
    repo already holds one term to that invariant (`row_last_copy`, see
    tests/test_counting.py::test_the_default_weight_costs_nothing); this
    extends it to the expensive one.

    It is off by default so that every INSTRUMENT -- the coordinate registry,
    the census, the harness -- keeps getting the complete vector including the
    entries the current weights happen not to buy.  A dead-coordinate ratchet
    that were fed the priced-only vector would report every unpriced
    coordinate as dead, which is the ratchet reading its own switch.
    """
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
    # positions the trial state cannot show (docs/COMBAT_AUDIT.md)
    pact_offers = auction_committed = auction_bid = 0.0
    gains = partner_gains = _NO_GAINS
    if state.pending:
        (pact_offers, auction_committed, auction_bid, pending_blocks,
         gains, partner_gains) = deferred_credit(state, idx)
        blocks_attack += pending_blocks
    g = gains.get

    happy_req = economy.happy_required(p.yellow_bank)
    margin = s.happy - happy_req + g("happy", 0.0)
    discontent = max(0, -margin)
    blue_have = effects.blue_available(p)
    blue_free = blue_have + g("blue_free", 0.0)
    # 8 is the "cannot increase population at all" sentinel (empty yellow
    # bank).  The formula itself lives in exactly one place; see
    # `economy.pop_food_cost`.
    pop_cost = economy.pop_food_cost(s, p.yellow_bank)
    if pop_cost is None:
        pop_cost = 8

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
                overrun = max(0.0, turns_to_finish
                              - rounds_left(state))

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
    rboard = rival_board(state, idx)
    atk_lead, atk_weakness = attack_target_terms(state, idx)
    if ctx is None:
        ctx = rival_context(state, idx)
    rival_culture_rate = ctx["rival_culture_rate"]
    rival_science_rate = ctx["rival_science_rate"]
    rival_str = ctx["rival_strength"]
    if partner_gains:
        # An offered pact raises the partner's OWN rates, so it can raise the
        # max-over-rivals these three terms are -- and by how much depends on
        # which rival was offered it.  Recomputed as a real max off
        # `rival_rates` rather than added to the max: offering the trailing
        # player a pact that leaves them still behind the leader costs
        # nothing here, which is precisely the distinction that was missing.
        # `.get`, because a `ctx` handed in by an older caller may predate
        # the key; then this degrades to today's behaviour rather than
        # raising inside an evaluator.
        rates = ctx.get("rival_rates") or {}
        for _pi, _pg in partner_gains.items():
            base = rates.get(_pi)
            if base is None:
                continue
            rival_culture_rate = max(rival_culture_rate,
                                     base[0] + _pg.get("culture_rate", 0.0))
            rival_science_rate = max(rival_science_rate,
                                     base[1] + _pg.get("science_rate", 0.0))
            rival_str = max(rival_str, base[2] + _pg.get("strength", 0.0))
    strength = s.strength + g("strength", 0.0)
    rel = strength - rival_str


    f = {
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
        # `board_yields` prices both of these when the bot is CONSIDERING a
        # card and, until this line, nothing priced them once it HELD one --
        # so a government's urban slots and Gandhi's aggression ban appeared
        # in the take decision and then evaporated from the board.  That is
        # the mirror image of the blindness docs/CARD_BLINDNESS.md was
        # written about, and it is fixed the way the convention above says:
        # same key on both sides.
        #
        # `urban_limit` is real board state -- Despotism caps you at 2 urban
        # buildings, the Age III governments at 4 -- and nothing else in this
        # dict reflects it (`urban_workers` is workers, not the cap).
        "urban_limit": float(s.urban_limit),
        # Gandhi.  A flag rather than a count, and the weight stays unsigned
        # at 0.0, because it bundles a restriction (he may never play an
        # aggression or war, enforced at engine/actions.py:292) with a
        # benefit (opponents pay double to attack him) and which dominates is
        # a thing to measure, not to assert.
        "no_aggression": 1.0 if s.no_aggression else 0.0,
        # --- cards
        "hand_civil": len(p.hand_civil),
        "hand_value": hand_value,
        "hand_military": len(p.hand_military) + g("hand_military", 0.0),
        "hand_mil_value": hand_mil_value,
        # --- the Age III scoring events already in play (see the block above
        # `event_scoring_margin`).  0.0 whenever none are pending, which is
        # every position before the first Age III event is seeded.
        # Skipped when unpriced and only then -- see `priced_only` above.  The
        # engine call behind this is the single most expensive entry in the
        # dict by a wide margin.
        "event_scoring_margin": (
            0.0 if (priced_only and not (w or {}).get("event_scoring_margin"))
            else event_scoring_margin(state, idx, ctx)),
        # --- rivals
        "rival_culture": rival_culture,
        "rival_mean_culture": rival_mean,
        "rival_culture_rate": rival_culture_rate,
        "rival_science_rate": rival_science_rate,
        "rival_strength": rival_str,
        "rival_free_ca": rival_free_ca,
        "rival_hand_civil": rival_hand_civil,
        "rival_wonders": rival_wonders,
        # --- the events I planted myself (GAP 4).  Legal by construction:
        # `my_seeds` filters on `seeded_by == idx` and reads no pile order, so
        # no determinization change ships with this.  The companion term
        # `my_event_threat` is priced through `w` and so lives in `evaluate`.
        "my_seeded_pending": my_seeded_pending(state, idx),
        # --- WHICH opponent I am attacking (tools/target_blindness.py).
        "attack_target_lead": atk_lead,
        "attack_target_weakness": atk_weakness,
        "pact_partner_lead": pact_partner_lead(state, idx),
    }
    f.update(rboard)
    # NOTE: the rate horizon is deliberately NOT applied here.  `features()`
    # reports the BOARD -- a civilisation producing 5 culture a turn produces
    # 5 culture a turn however much game is left -- and the horizon is a
    # property of what that is WORTH, so it lives in `evaluate` and in
    # `feature_marginal`.  The first cut of this change scaled the feature
    # instead and `tests/test_build_fresh.py` caught it: `board_yields` emits
    # a card's PRINTED yield and asserts it against the real `features()`
    # delta, an invariant that only holds while the two are in the same units.
    return f


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

# The three Military Bonus cards (age I/II/III, `type: "bonus"`, six copies
# each at every player count).  They are the whole of the `bonus` type and
# they carry BOTH keys, defence 2/4/6 and colonization 1/2/3.
#
# Both mappings are the engine's own arithmetic rather than an opinion, which
# is the standard this file holds a table entry to:
#
#   * `colonizationBonus` -- `engine/interact.py:force_value` adds it into
#     the SAME sum as `effects.state_stats(p).colonize`, and `features()`
#     already publishes that stat as `colonize_bonus`.  One colonization
#     point from a card and one from the board are the same point, so they
#     share the weight, exactly as `civil_actions` is shared between the card
#     that grants one and the board that has one.
#
#   * `defenseBonus` -- `engine/interact.py:defense_points` is the authority
#     (`_defense_move` calls it) and it gives EVERY military card 1, these
#     three 2/4/6.  The flat 1 is already priced, by `hand_military`, a count
#     of the military hand; what is new information is the INCREMENT, so
#     `_card_yields` prices `defenseBonus - 1` = 1/3/5 and not the printed
#     number.  Pricing the printed number would count the generic
#     face-down-discard value of the card twice.
#
# A registry rather than two literals in `_card_yields`, for the same reason
# `_UNIT_TO_FEATURE` and `_TERR_TO_FEATURE` are: `tools/card_blindness.py
# --legacy` has to be able to clear it and still reproduce master's census
# exactly, or the baseline every later result is measured against quietly
# rewrites itself.
# How much of the printed number to believe is a separate question from what
# the number means, and it is a WEIGHT: `bonus_card_credit` (BASE_WEIGHTS,
# wired up through `_CREDIT_OF[_Y_BONUS]`).  A knob and not a constant for
# the same reason `territory_credit` is one -- both keys are CONDITIONAL
# payoffs, the defence half paying only if somebody aggresses against you and
# you spend the card, the colonization half only if a territory is up and you
# win the auction -- and how often that is, is for the league to measure
# rather than for this line to assert.  1.0 is the printed number; 0.0
# recovers the pre-change pricing byte for byte.
_BONUS_TO_FEATURE = {
    "defenseBonus": "defense_bonus",
    "colonizationBonus": "colonize_bonus",
}

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
#
#    "THE RULES ENGINE EXPRESSES THIS" IS A CLAIM ABOUT ANOTHER FILE, and for
#    SEVEN of these keys it was false for the life of the list: nothing in
#    `engine/` had ever heard of `oncePerGameTwoPoliticalActions`,
#    `removeAsPoliticalActionForYellowToken`,
#    `removeAsPoliticalActionFreeColonize`, `peekTopEventCardInPolitics`,
#    `militaryActionCombinedPopIncreaseAndUnitBuild`,
#    `civilActionUpgradeUrbanBuildingToTheater` or
#    `colonizeDiscardUpTo2MilitaryCardsForBonus`, so the write-off pointed at
#    an implementation that did not exist and nothing failed.  All seven are
#    expressed now -- `actions._end_politics`,
#    `actions._leader_politics_moves`, `interact.colonize_without_sacrifice`,
#    `events.peek_top_event`, `actions._barbarossa_moves`,
#    `actions._bach_moves` and `interact.cook_pool` -- and they stay on this
#    list because the reason above is now TRUE of them: a second political
#    action, a leader traded for a token, a colonization with no force, a look
#    at a face-down card, two actions merged into one, an upgrade across
#    building types and two cards burned for force are all rules, none of them
#    a number `_card_yields` could multiply a weight by.  The bots reach all
#    of them anyway, because each is a MOVE whose consequences the evaluator
#    already prices (a worker, a unit, a theater, a card leaving the hand).
#    `tests/test_leader_politics_abilities.py` and
#    `tests/test_leader_action_abilities.py` are what make the claim
#    checkable.
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
#    docs/CARD_BLINDNESS.md rather than guessed at here.
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
#
#    `defenseBonus` / `colonizationBonus` USED TO BE WRITTEN OFF HERE, with
#    the reason "military hand: never reaches _card_yields (hand_potential is
#    civil-only)".  That reason had been false since `hand_mil_potential`
#    landed and nothing noticed, because a write-off is not a test: with a
#    non-zero `hand_mil_potential` (the live 3p champion carries 0.01079)
#    `evaluate` walks `p.hand_military` and calls `card_potential` ->
#    `_card_yields` on every card in it, bonus cards included.  The proof is
#    in this same file -- a territory is priced through `_TERR_TO_FEATURE`
#    from inside `_card_yields`, reached by exactly that route.  So the two
#    keys were not unreachable, only unmapped; they are mapped now, in
#    `_BONUS_TO_FEATURE`, and see docs/COMBAT_AUDIT.md.
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
_Y_BONUS = 5    # a Military Bonus card's defence/colonization numbers,
#                 scaled by w["bonus_card_credit"] -- same trick again

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
    _Y_BONUS: ("bonus_card_credit", 1.0),
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
    # The three Military Bonus cards.  Their `effects` block holds nothing
    # else, so before `_BONUS_TO_FEATURE` they priced at exactly 0.0 -- the
    # ONLY military-deck type left doing so once territories were priced, and
    # written off in DELIBERATELY_UNPRICED with a reason (`hand_potential` is
    # civil-only) that `hand_mil_potential` had already made false.  See the
    # note on `_BONUS_TO_FEATURE` for why the defence number is priced as the
    # increment over the flat +1 every military card is worth face down.
    if typ == "bonus":
        for k, amt in eff.items():
            fk = _BONUS_TO_FEATURE.get(k)
            if not fk or amt is True or amt is False or \
                    not isinstance(amt, (int, float)):
                continue
            if k == "defenseBonus":
                amt -= 1
            if amt:
                out.append((fk, float(amt), _Y_BONUS))
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
# is byte-identical either way (docs/CARD_BLINDNESS.md section 13.5).
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


@_lru_cache(maxsize=None)
def _is_unit(name):
    return C.db().type_by_name.get(name) in C.UNIT_TYPES


@_lru_cache(maxsize=None)
def _is_levelled_tech(name):
    """True for a card whose development raises `tech_levels` (see
    `board_yields.LEVELLED_TYPES`) -- every worker type plus `special-tech`."""
    return C.db().type_by_name.get(name) in _BY.LEVELLED_TYPES


@_lru_cache(maxsize=None)
def _is_action(name):
    return C.db().type_by_name.get(name) == "action"


@_lru_cache(maxsize=None)
def _is_government(name):
    return C.db().type_by_name.get(name) == "government"


# ------------------------------------------------ the two ring-fenced yields
#
# `_EFF_TO_FEATURE` sends two effect keys to weights that are NOT features:
# `resourceDiscount` -> `resource_discount` and `resourcesForMilitaryUnits` ->
# `restricted_resources`.  `features()` emits neither, so `evaluate` never pays
# for either, so no game the league ever plays can tell the trainer what they
# are worth -- and both sit at 0.0 on every champion in the pool.  See
# `action_value` for why that is the whole bug.
#
# What they are amounts OF is not in doubt: both are RESOURCES.  One is
# resources you do not have to pay for a specific build; the other is resources
# you may only spend on military units.  So `action_value` prices both through
# `resource_stock`, the live trained coordinate `evaluate` does pay for, and the
# only free parameter left is how much of a resource a ring-fenced one is:
#
#   * a `resourceDiscount` resource is worth a whole one.  You were going to
#     make that build (the card is only playable if the ordered action is
#     legal, `actions._action_card_playable`), and the discount comes straight
#     off its cost.  Credit 1.0, and there is nothing to fit.
#   * a `resourcesForMilitaryUnits` resource is worth AT MOST a whole one and
#     is worth nothing to a player who builds no units this turn.  1.0 is the
#     upper bound, deliberately: the honest number depends on the policy's own
#     appetite for units, which `card_potential` cannot see, and
#     `restricted_resource_credit` is a weight rather than a constant precisely
#     so the league can measure the ring fence instead of a guess asserting it.
#
# Both are `max(0.0, ...)` on the resource weight for the same reason `_Y_COST`
# and `tech_value` are: a hill climb may drive a stock weight negative and a
# GAIN must never read as a loss because of it.
_RESTRICTED_TO_FEATURE = {
    "resource_discount": ("resource_stock", 1.0),
    "restricted_resources": ("resource_stock", "restricted_resource_credit"),
}


def _yield_marginal(key, state, idx, w, late):
    """`feature_marginal`, plus the two ring-fenced pseudo-features above."""
    fenced = _RESTRICTED_TO_FEATURE.get(key)
    if fenced is None:
        return feature_marginal(key, state, idx, w, late)
    feat, credit = fenced
    if not isinstance(credit, float):
        credit = w.get(credit, 1.0)
    if not credit:
        return 0.0
    return credit * max(0.0, feature_marginal(feat, state, idx, w, late))


def action_value(name, state, idx, w, late=None):
    """Eval-points playing this yellow action card would be worth ON THIS BOARD.

    `tech_value`'s sibling, for the one civil type left on the static table,
    and it is the same diagnosis one colour over.  Both documents this repeats
    -- docs/CARD_BLINDNESS.md and docs/CARD_BLINDNESS.md -- close with
    the same sentence: **a card is worth what `evaluate` pays for what it
    does**, and the static table kept pricing cards through coordinates
    `evaluate` does not use.  The action cards are the third and largest
    instance, and they are the worst of the three because the coordinates
    involved are not merely mis-scaled, they are UNREACHABLE:

        weight                 a feature?   value on every champion in the pool
        free_civil_action      no           0.0
        resource_discount      no           0.0   (0.498 on the live 2p only)
        restricted_resources   no           0.0   (0.155 on the live 2p only)

    `features()` emits none of the three, so `evaluate` never multiplies them
    by anything, so no game the league plays can produce a gradient on them.
    They are not weights the trainer chose to leave at zero; they are weights
    the trainer has never had any information about.  The measured consequence
    is that **thirteen of the thirty-three action cards price at EXACTLY
    0.000** on `DEFAULT_WEIGHTS` -- every Rich Land, Urban Growth, Engineering
    Genius and Efficient Upgrade, whose entire printed value is a free civil
    action plus a resource discount -- and three more (the Reserves) price at
    0.000 for a different multiplied-by-zero reason (see `card_potential`).
    Sixteen of thirty-three cards, worth nothing to the evaluator, in a type
    the bot takes 2.72 times a seat-game against a human 12.98.

    THE SHAPE, and every line of it is a derivation rather than a taste:

    1. **A one-shot gain is worth `feature_marginal`, not `w[key]`.**  Same
       correction `tech_value` makes, and it bites here through `culture`,
       which is a `PHASE_KEYS` feature: `evaluate` pays
       `w[k] + (1-L)w[k_early] + L*w[k_late]`, so Cultural Heritage's four
       culture is worth 2.4 eval points in Age A and 10.0 in Age IV on the
       defaults, where the static table says 4.0 in both.
    2. **A free civil action is worth a civil action TIMES
       `free_action_credit`, and that credit's default is 0.0 because the
       action economy is a WASH.**  This is the one number in this function
       that is not a pure derivation, and the first draft of it got the
       derivation backwards, so it is written out:

           RB 3.11 / `actions._h_play_action`: playing a yellow action card
           costs ONE civil action (`pay_ca(state, p, 1)`) and grants ONE
           ordered action.  Net zero.  Rich Land is "spend a civil action to
           build a farm three resources cheaper", not "spend a civil action
           and get one back".

       Crediting the grant without charging the play therefore double-counts
       an action the card never gives you.  Measured, at credit 1.0 on
       `DEFAULT_WEIGHTS`: action takes land exactly on the human rate (7.35 ->
       11.80 a seat-game against a human 12.98 at 2p) and the paired A/B is
       **32.8% against a 50% null** -- because the same civil actions stop
       buying technologies (11.9 -> 8.0 developed a game) and 5.5 action cards
       a seat-game are taken and never played.  Right rate, wrong reason, and
       the behaviour census would have called it a win on its own.

       It is a weight and not a hard 0.0 because the wash is not quite exact
       and the exceptions all point one way: Breakthrough's order may be spent
       on a REVOLUTION, which normally costs the whole civil-action pool
       (`actions._can_revolt`), and any ordered action is available on a turn
       whose actions are already spent.  0.0 is the conservative end of that
       range and the measured one; the league has a gradient from there.
    3. **The two ring-fenced yields are resources.**  See
       `_RESTRICTED_TO_FEATURE`.
    4. **A choice is a max, and it is not board pricing.**  Reserves' "gain 2
       food OR 2 resources" is resolved here rather than in the block
       `card_board_credit` gates, which is where it was and where it was
       multiplied by a zero -- see `card_potential`'s note.
    5. **The three per-table-size cards are board-scaled and stay so.**
       Endowment for the Arts, Wave of Nationalism and Military Build-Up
       multiply a printed coefficient by a count of rivals; `board_yields.
       board_extra` already computes exactly that from the live boards and is
       called here rather than reimplemented, so there is one implementation
       and it cannot drift.

    NOT charged, deliberately: the civil action playing the card costs, for
    the reason in point 2; and the card leaving the hand, which `hand_value`
    already prices on the board side.

    No information is added -- the effects are printed on the card, and
    `board_extra` reads only public culture totals and public strengths.
    """
    card = C.db().by_name.get(name)
    if card is None:
        return 0.0
    eff = card.get("effects") or {}
    # `late` is threaded in by callers pricing several cards off one board
    # (`_hand_total`, `hand_mil_potential`, `row_pressure`) so `lateness` is
    # computed once per evaluation instead of once per card -- see the
    # comment above those loops.
    if late is None:
        late = lateness(state)
    total = 0.0
    for k, amt in eff.items():
        if amt is True or amt is False or not isinstance(amt, (int, float)):
            continue
        fk = _EFF_TO_FEATURE.get(k)
        if fk:
            total += amt * _yield_marginal(fk, state, idx, w, late)
    for group in _card_choices(name):
        total += max(sum(amt * _yield_marginal(fk, state, idx, w, late)
                         for fk, amt, _kind in g) for g in group)
    for fk, amt, _kind in _BY.board_extra(name, state, idx):
        total += amt * _yield_marginal(fk, state, idx, w, late)
    if eff.get("freeCivilAction"):
        fc = w.get("free_action_credit", 0.0)
        if fc:
            total += fc * feature_marginal("civil_actions", state, idx, w,
                                           late)
    return total


def tech_value(name, state, idx, w, dev_credit=1.0, late=None):
    """Eval-points developing this technology is worth ON THIS BOARD.

    ONE function for all fifteen technology types, red included.  It began as
    `unit_tech_value` and the generalisation is the point: adding
    `tech_levels` to the red cards alone would be the same asymmetry
    docs/CARD_BLINDNESS.md section 14.7.1 refused to create, pointing the
    other way.

    THE DEFECT THIS REPLACES, in two instalments.

    RED (docs/CARD_BLINDNESS.md): the bot took 0.15 / 0.06 / 0.45 unit
    cards per seat-game against a human 3.84 / 2.79 / 3.43, i.e. it fought the
    entire game with Age A Warriors.  The static table prices every unit card
    strictly NEGATIVE -- Warriors -3.46 to Air Forces -16.07 on the live 2p
    champion -- because it charges `techCost` through `science` and
    `buildCost` through `resource_stock`, both trained, and returns the
    strength through `unit_strength_credit`, which is 0.0 on every champion in
    the league.  Cost priced, gain not: the standing hazard of
    docs/HAZARDS.md and `tests/test_half_priced_cards.py`.

    YELLOW AND THE REST (docs/CARD_BLINDNESS.md): the same static table
    prices every farm, mine and laboratory strictly negative too -- Irrigation
    -4.02, Iron -6.72, Alchemy -11.19, Computers -20.41 on the same vector --
    and the bot took laboratories 0.03 times a seat-game at 2p against a human
    1.62, mines 0.05 against 1.18, and Alchemy, Scientific Method and Coal
    exactly ZERO times at any table size.  The cause is NOT a dead credit this
    time, it is two half-priced gains:

      * `tech_levels` was mapped to nothing at all, on every technology card.
        It is worth up to 9.23 eval points per level on the live 2p champion,
        which is more than the rest of a yellow card put together.
      * a production rate was priced at the bare `w[k]` where `evaluate` pays
        `w[k] + (1-L)w[k_early] + L*w[k_late]`.  See `feature_marginal`; for
        `science_rate` on that vector the two are 0.25 and 5.29.

    `row_pressure` additionally skips any card whose `card_potential` is <= 0
    ("the sweep destroying a card I do not want is not a loss"), so on top of
    being under-valued in the hand term a unit card is INVISIBLE to the two
    row terms.  Both halves are measured in `tests/test_unit_pricing.py`.

    Turning the old credit up cannot fix it, and that is the part worth being
    precise about, because it is why this is a reshaping and not a retuning.
    At `unit_strength_credit` 1.0 Swordsmen goes from -6.51 to -6.12 on the
    live 2p champion; the sign flips only somewhere past 16, and every step in
    between changes no argmax at all, so the climb faces a flat plateau
    sixteen units long with a `mutate` step of ~0.15.  There is no gradient to
    walk.  docs/CARD_BLINDNESS.md section 11.5.2 measured exactly that:
    0 divergences in 967 decisions at 1.0, one at 3.0.

    THE SHAPE, four corrections, all of them derivations rather than tastes:

    1. **The move is an upgrade, not a fresh build.**  Every player starts
       with a Warriors worker, an Agriculture farm and a Bronze mine, so the
       resources actually at stake are `actions.upgrade_cost`, not
       `buildCost` -- 3 rather than 5 for Riflemen, 3 rather than 5 for Iron
       -- and the yield actually at stake is the difference, on every worker
       moved.  `board_yields.unit_upgrade` / `tech_upgrade` get both from the
       engine.
    2. **A unit of any feature is worth `feature_marginal`, not `w[key]`.**
       See that function.  For `strength` on the live 2p champion the two are
       0.19 and ~3.0, a factor of fifteen; for `science_rate` they are 0.25
       and 5.29, a factor of twenty-one.  Neither is a credit anybody could
       have guessed, they are derivatives of the objective.
    3. **Developing a technology buys `tech_levels`, `num_techs` and a
       `best_*` step whether or not anybody staffs it.**  That is the
       `develop` half of `tech_upgrade` and it is why a laboratory can be
       worth taking on a board with no laboratory worker to upgrade.
    4. **You develop the technology and then decide how many workers to
       move.**  Linear in that count, so the optimum is an endpoint: either
       all of them or none.  `max(0, ...)` is that argmax, not a floor put
       there to keep the number positive -- and the science is charged
       OUTSIDE it, so a technology nobody will staff still reads as its
       develop value minus the pure science cost.

    Costs are clamped with `max(0.0, w)` for the same reason `_Y_COST` is: a
    hill climb is free to drive a stock weight negative (the 4p champion
    reached `science` = -6.09) and paying a cost must never read as a gain.

    `dev_credit` scales correction 3 only, and it is `tech_board_credit` --
    the ONE constant that has to move to recover the parent commit's pricing
    byte for byte on every card in the game (see `card_potential`).  A red
    card is additionally scaled by `unit_tech_credit`, unchanged, so
    docs/CARD_BLINDNESS.md's escape hatch still works exactly as written.
    """
    staff, dev, sci, res = _BY.tech_upgrade(name, state, idx)
    if not (staff or dev or sci or res):
        return 0.0
    # `late` is threaded in by callers pricing several cards off one board
    # (`_hand_total`, `hand_mil_potential`, `row_pressure`) so `lateness` is
    # computed once per evaluation instead of once per card -- see the
    # comment above those loops.
    if late is None:
        late = lateness(state)
    net = -res * max(0.0, w.get("resource_stock", 0.0))
    for k, amt, _kind in staff:
        net += amt * feature_marginal(k, state, idx, w, late)
    if net < 0.0:
        net = 0.0
    # ...and the OTHER staffing plan, which is the same argmax one branch
    # wider.  `board_yields.build_fresh` answers "develop it and build ONE
    # fresh worker on it" -- the only route into play for a technology of a
    # type this player has never staffed, and the only route that will ever
    # exist for Knights, Cannon and Air Forces, which are the lowest card of
    # their own type in the deck (docs/OPEN_ITEMS.md section 2 items 21/28).
    # A `max`, not a sum: the two plans compete for the same turn, and taking
    # the better of them is a lower bound on doing both.  The develop half
    # below is outside the max because it is paid on either plan.
    # The fallback is 0.0 and not 1.0, unlike its four siblings, because the
    # SHIPPED default is 0.0 (see `BASE_WEIGHTS`): "absent from the vector"
    # and "present at the default" have to mean the same thing, or a caller
    # passing a bare dict -- `analysis/`, an old on-disk vector read without
    # `load_weights` -- would silently switch a measured-worse branch ON.
    build_credit = w.get("build_fresh_credit", 0.0)
    if build_credit:
        b_triples, b_res = _BY.build_fresh(name, state, idx)
        if b_triples:
            b_net = -b_res * max(0.0, w.get("resource_stock", 0.0))
            for k, amt, _kind in b_triples:
                b_net += amt * feature_marginal(k, state, idx, w, late)
            b_net *= build_credit
            if b_net > net:
                net = b_net
    if dev_credit:
        gain = 0.0
        for k, amt, _kind in dev:
            gain += amt * feature_marginal(k, state, idx, w, late)
        net += dev_credit * gain
    return net - sci * max(0.0, w.get("science", 0.0))


def gov_value(name, state, idx, w, late=None):
    """Eval-points putting this government in play would be worth ON THIS
    BOARD, by the cheaper of the two routes the rules offer.

    `tech_value`'s and `action_value`'s sibling, for the last civil type still
    on the static table, and the diagnosis is the same one a third time: a
    card is worth what `evaluate` pays for what it does, and the static table
    kept pricing governments through coordinates `evaluate` does not use --
    or, here, through nothing at all.  `_card_yields` reads `techCost`, which
    is `null` on all eight government cards (they print `peacefulCost` and
    `revolutionCost` instead), and reads `production`/`effects`, where a
    government's civil actions, military actions and urban limit are NOT --
    they are top-level fields `effects.compute` reads.  So on the static path
    Monarchy, Republic and Constitutional Monarchy price at **exactly
    0.000**: no cost, no gain, nothing.  The same shape as the sixteen action
    cards docs/CARD_BLINDNESS.md section 16 (the former docs/ACTION_CARD_PRICING.md) found, one type over.

    THREE THINGS THIS PRICES THAT NOTHING PRICED, all of them derivations:

    1. **The level.**  `features()` reads a government's age level twice, into
       `tech_levels` and again as `gov_level` (2.0 in `DEFAULT_WEIGHTS`), and
       neither the static table nor the swap diff emitted either -- the one
       term docs/CARD_BLINDNESS.md section 15 (the former docs/YELLOW_TECH_PRICING.md)
       added to every other technology in the game and did not add here.  docs/OPEN_ITEMS.md section 2 item 22.
    2. **Everything `compute` sees**, through the swap diff: the civil-action
       total above all, which is the game's core currency and the largest
       single source of it.  A DIFFERENCE, because a government replaces a
       government (RULES_SPEC 8.1).
    3. **Both routes, gated on the board.**  RULES_SPEC 8.2 and 8.3 are two
       different moves at two different prices -- Monarchy is 8 science and
       one civil action peacefully, or 2 science and your whole turn's civil
       actions by revolution -- and `_action_moves` generates both.  Which is
       cheaper is a board question (how many actions the pool holds is the
       government you are leaving), so it is asked rather than assumed:
       `board_yields.government_plans` returns one cost route per LEGAL move
       and this function takes the cheaper, which is a `max` over negative
       triples.  Nothing here fits a rate for "how often a revolution is
       worth it"; the answer is read off the board every time.

    THE BURN LANDS ON `ca_left`, AND THAT SETTLES docs/OPEN_ITEMS.md section
    9.1.  That item left open whether a revolution destroys the per-turn
    ALLOTMENT (`civil_actions`, 2.0) or the REMAINDER (`ca_left`, 0.05), a
    40x difference.  RULES_SPEC 8.3.1 requires every civil action to be
    available for the move to be legal at all, so the two quantities are the
    same NUMBER whenever the question can be asked; what differs is which
    coordinate `evaluate` watches move, and `_h_revolution` sets
    `p.civil_actions = 0` -- `ca_left`.  `civil_actions` does not fall, it
    RISES by the new government's own total, and item 2 above already prices
    that.  The dead `gov_action_cost` coordinate is left exactly where it is,
    on the legacy `card_board_credit` path, for the same reason `5ab4943`
    left `free_civil_action` on the static one.

    Costs are clamped with `max(0.0, w)` for the same reason `_Y_COST` and
    `tech_value` are: a hill climb is free to drive a stock weight negative
    and paying a cost must never read as a gain.
    """
    gains, routes = _BY.government_plans(name, state, idx)
    if not routes:
        return 0.0
    # `late` is an optional pre-computed `lateness(state)` -- the same
    # call `tech_value` takes, hoisted out of the per-card loops by the same
    # commit, so a caller pricing a whole hand computes it once.
    if late is None:
        late = lateness(state)
    total = 0.0
    for k, amt, _kind in gains:
        total += amt * feature_marginal(k, state, idx, w, late)
    best = None
    for route in routes:
        cost = 0.0
        for k, amt, kind in route:
            m = feature_marginal(k, state, idx, w, late)
            cost += amt * (max(0.0, m) if kind == _BY._COST else m)
        if best is None or cost > best:
            best = cost           # costs are negative: the cheapest is the max
    return total + best


def card_potential(name, w, state=None, idx=None, late=None):
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

    `late` is an optional pre-computed `lateness(state)`.  Until 2026-08-04
    this was TWO parameters, because the `horizon_legacy` hatch made
    `lateness(state, w)` and `lateness(state)` formally different (see
    `action_value`), for a caller pricing several cards off the same board
    (`_hand_total`, `hand_mil_potential`, `row_pressure`) so the board's
    lateness is computed once per evaluation rather than once per card.  Both
    default to None, which reproduces the old per-call computation exactly.
    """
    # A TECHNOLOGY is board-priced on its OWN credit, not on
    # `card_board_credit`.  Deliberately: `card_board_credit` is 0.361 on the
    # live 2p champion and 0.0 on both the 3p and the 4p ones, so hanging the
    # technology fix off it would leave two of the three league arms with the
    # defect it exists to fix.  `tech_board_credit` and `unit_tech_credit`
    # default to 1.0 and are absent from every champion file, so `load_weights`
    # fills them in from `DEFAULT_WEIGHTS` and the fix is live on all three
    # arms at once (docs/CARD_BLINDNESS.md, docs/CARD_BLINDNESS.md).
    #
    # THE OPT-OUT, and it is one constant: `tech_board_credit` = 0.0 sends
    # every non-red technology back down to the static table and drops the
    # `develop` half off the red ones, which is the parent commit's pricing
    # byte for byte on all 236 cards.  `unit_tech_credit` = 0.0 continues to
    # mean exactly what docs/CARD_BLINDNESS.md says it means: the red cards
    # fall through to the static table, `tech_board_credit` or not, because a
    # vector that has switched the board query off for units must not have it
    # switched half on.
    if state is not None and idx is not None:
        tb = w.get("tech_board_credit", 1.0)
        if _is_unit(name):
            uc = w.get("unit_tech_credit", 1.0)
            if uc:
                return uc * tech_value(name, state, idx, w, tb, late)
        elif tb and _is_levelled_tech(name):
            return tb * tech_value(name, state, idx, w, late=late)
        elif _is_government(name):
            # Same device, same reason, one type over.  `gov_board_credit`
            # defaults to 1.0 and is absent from every champion file, so
            # `load_weights` fills it in and the fix is live on all three
            # league arms at once; 0.0 sends every government back down to the
            # static table (and to `card_board_credit` + `card_board_government`
            # if a vector ever turns those on), which is the parent commit's
            # pricing byte for byte.  See `gov_value`.
            gb = w.get("gov_board_credit", 1.0)
            if gb:
                return gb * gov_value(name, state, idx, w, late)
        elif _is_action(name):
            # Same device, same reason, one type over: `action_board_credit`
            # defaults to 1.0 and is absent from every champion file, so
            # `load_weights` fills it in and the fix is live on all three
            # league arms at once; 0.0 sends every action card back down to
            # the static table, which is the parent commit's pricing byte for
            # byte.  See `action_value`.
            ab = w.get("action_board_credit", 1.0)
            if ab:
                return ab * action_value(name, state, idx, w, late)
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
    #
    # AND THAT WAS A BUG, kept here only because this whole block is the
    # opt-out.  The three Reserves are the entire population of
    # `_card_choices`, they are action cards, and multiplying a choice that
    # needs no board by `card_board_credit` -- 0.0 on every champion in the
    # league -- priced "gain 4 food OR 4 resources" at exactly nothing.  The
    # early return above never even reaches this loop.  `action_value` resolves
    # the choice with no credit on it at all, which is what closes it; this
    # line is what `action_board_credit` = 0.0 goes back to.  See
    # docs/CARD_BLINDNESS.md section 16.2.2.
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
    (docs/CARD_BLINDNESS.md sections 13.5.2 and 13.8).

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
    # Computed once for the whole hand rather than once per card: every card
    # is priced on the SAME board, so `lateness(state)` /
    # `lateness(state)` (the two different calls `tech_value` and
    # `action_value` make) are each invariant across this loop.  Pure
    # functions of `(state, w)` with no side effects and `state` not mutated
    # between iterations, so this is exactly the value each call would have
    # computed itself.
    late = lateness(state) if state is not None else None
    for n in hand:
        v = card_potential(n, w, state, idx, late)
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
    units to fill it (docs/CARD_BLINDNESS.md section 11.4).

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

    THE SEAM.  This passed `card_potential(n, w)` -- no `state`, no `idx` --
    and `card_potential` gates both of its board branches on `state is not
    None and idx is not None`, so board-aware pricing could not fire for a
    military card no matter what the weights said.  There was no reason for
    it: this function is HANDED a state, and every caller of it goes through
    `evaluate`, which has one.  It is fixed by passing them, not by threading
    a None anywhere.

    Passing them changes no number today, and the reason is worth writing
    down so the next lane knows what it is inheriting: `board_yields` returns
    None for anything outside `SWAP_TYPES` (leader/government/wonder),
    `board_extra` returns () for anything outside its three civil action
    cards, and `_board_credit_key` has no entry for a military type -- so a
    military card's board credit is the bare `card_board_credit`, which is
    0.0 on all three live champions and takes `card_potential`'s early
    return.  This is the seam being OPEN, not a reprice; a board-aware
    military pricing (an aggression priced against what the defender
    actually holds, say) has somewhere to land now.
    """
    hand = state.players[idx].hand_military
    if not hand:
        return 0.0
    total = 0.0
    # See `_hand_total`: one board, so `lateness` is invariant across the
    # hand and only needs computing once.
    late = lateness(state)
    for n in hand:
        total += card_potential(n, w, state, idx, late)
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
# (`_SWEEP` now lives with the rest of the supply arithmetic, up in "the game
# horizon" -- one copy, same rule.)

# P(a rival who CAN legally take a card I also want actually takes it) before
# my next turn.  This USED to be one flat 0.25 for every rival, every card and
# every board, on the stated grounds that the alternative on offer was the
# seven per-slot survival rates in docs/INFORMATION_AUDIT.md 2.1 and that
# baking numbers fitted on row-blind opponents into the evaluator would be
# fitting the bug.  That argument stands, and it is not an argument for a flat
# constant: it is an argument against ONE PARTICULAR fitted table.
#
# What the flat prior threw away is public.  Every rival's board is open, so
# `rival_context` already knows, per rival and exactly:
#
#   * how many civil actions they will have on their next turn (`gate[0]`),
#   * whether their civil hand is already full (`gate[1]`; hand_civil is
#     public by RULES_SPEC 2.6, the open civil cards convention),
#   * what THIS card would cost THEM -- their wonder surcharge, Hammurabi's
#     leader discount, Michelangelo's waiver (`gate[2..3]`), and
#   * which OTHER row slots they could legally take, i.e. what else is
#     competing for those civil actions (`_can_take_gated` over the row).
#
# `rival_take_p` below spends all four.  What stays a prior is the one thing
# the rules genuinely hide -- what fraction of their civil actions a rival will
# spend on the row rather than on building, upgrading or a wonder, which is
# policy and intention, not state.  docs/INFORMATION_AUDIT.md section 3 is
# explicit that hand CONTENTS are public but intentions are not, and this lane
# stays inside that line.  So that one number is exposed as the WEIGHT
# `rival_take_share` (in DEFAULT_WEIGHTS, so `hillclimb.mutate` can move it)
# instead of as a bare constant, and its default is chosen to reproduce the old
# flat 0.25 in the mean over real positions (docs/MODEL_CONSTANTS.md section 4).
RIVAL_TAKE_SHARE = 0.5   # FITTED PRIOR, overridable per vector (see above)
#: The flat per-rival 0.25 this replaced, and its `TTA_LEGACY_ROW_TAKE` hatch,
#: were deleted on 2026-08-04 with the other two A/B hatches -- see the block
#: above `rounds_left`.  `rival_take_share` is on the vector, so the trainer can
#: move it and the retired flat prior needs no separate switch to be reachable.


def _rival_take_cost(name, i, gate):
    """What row slot `i` costs THAT rival, in their civil actions.

    `actions.take_cost` restated against the gate tuple rather than against a
    live player, because a `_RivalView` is a snapshot and the surcharge and
    the discount are already in the gate `rival_context` built (RULES_SPEC
    2.3/2.5: +1 per completed or destroyed wonder, waived for Michelangelo;
    -1 on a leader for Hammurabi; floored at 0).
    """
    cost = actions.ROW_COST[i] if i < 13 else actions.row_cost(i)
    typ = actions._TYPE_BY_NAME[name]
    if typ == "wonder":
        cost += gate[2]
    elif typ == "leader":
        cost -= gate[3]
    return cost if cost > 0 else 0


def rival_take_p(cost, budget, reach, slack, share):
    """P(this rival takes THIS card before my next turn), from their board.

    Every argument but `share` is exact public state:

    * `cost` -- civil actions the card costs *that rival*, their surcharges
      and discounts included;
    * `budget` -- civil actions they will have (`effects.compute().
      civil_actions`);
    * `reach` -- how many row slots they could legally take at all, i.e. how
      many ways that budget can be spent on the row;
    * `slack` -- how many more civil cards their hand can hold (0 when full,
      in which case the answer is exactly 0 and no prior is consulted).

    The model is a budget split: of `budget` civil actions, `share` go to the
    row, buying `share*budget/cost` cards at this price, spread over the
    `reach` slots that are competing for them, and capped by the hand.  It is
    monotone the way the board says it should be -- more of their actions or
    fewer competing cards raises it, a dearer slot or a fuller hand lowers it
    -- and it collapses to 0 exactly when the legality gate says they cannot
    take the card at all.
    """
    if reach <= 0 or slack <= 0:
        return 0.0
    if cost <= 0:
        # free to them (Hammurabi on a 1-CA leader): only the hand limits it
        takes = float(slack)
    else:
        takes = share * budget / cost
        if takes > slack:
            takes = float(slack)
    p = takes / reach
    if p <= 0.0:
        return 0.0                      # a clamped-negative `share`
    return 1.0 if p > 1.0 else p


def _rival_desire(state, w, visible, views, gated, late):
    """Per rival, {slot -> (what they want THIS slot, what they want in all)}.

    WHAT THIS CLOSES.  `rival_take_p` models a rival's CAPACITY exactly -- what
    the row costs them, what their civil actions buy, how full their hand is --
    and then spreads that capacity UNIFORMLY over every slot they could legally
    take.  Uniform is wrong in the obvious direction: a rival with two civil
    actions and a Philosophy in reach is not equally likely to spend them on
    the Bronze next to it.  docs/INFORMATION_AUDIT.md called this out as "no
    model of what an opponent WANTS from the row".

    Their board is public, so it can be priced.  This runs the SAME
    `card_potential` the bot prices its own row with, from the rival's seat --
    which is the honest thing to run: it is a model of them built out of my own
    judgement applied to their visible position, not a peek at their plan.
    Intentions stay hidden; only the board is read.

    Only slots that rival can legally take are counted, and only positive
    values, so the shares are a genuine distribution over the cards they would
    actually want.  Slots they cannot reach are absent, and `row_pressure`
    falls back to the uniform `reach` for those.
    """
    out = []
    for view, gate in views:
        vals = {}
        total = 0.0
        for j, nm in visible:
            if not gated(state, view, j, gate, nm):
                continue
            v = card_potential(nm, w, state, view.idx, late)
            if v > 0.0:
                vals[j] = v
                total += v
        out.append({j: (v, total) for j, v in vals.items()})
    return out


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
    # The masking pass, hoisted out of the pricing pass so the SAME set of
    # slots defines what a rival can reach.  Anything dealt after the decision
    # began must be invisible to the rival model too, or the decay factor
    # becomes a second copy of the `end_turn` leak the mask exists to close.
    visible = []
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
        visible.append((i, name))
    if not visible:
        return 0.0, 0.0
    p = state.players[idx]
    n = _live(state)
    slide = n * _SWEEP[n]
    mine = actions._take_gate(state, p, budget=actions.ca_total(state, p))
    views = ctx.get("rival_views", ()) if ctx else ()
    cost_of = actions.ROW_COST
    gated = actions._can_take_gated
    share = w.get("rival_take_share", RIVAL_TAKE_SHARE)
    desire = w.get("rival_desire", 0.0)
    if desire < 0.0:
        desire = 0.0
    elif desire > 1.0:
        desire = 1.0
    reach = want = None     # per-rival slot counts, built at most once
    urgency = bargain = 0.0
    # See `_hand_total`: one board, so `lateness` is invariant across the
    # row and only needs computing once.
    late = lateness(state)
    for i, name in visible:
        if not gated(state, p, i, mine, name):
            continue
        val = card_potential(name, w, state, idx, late)
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
        if reach is None:
            reach = [sum(1 for j, nm in visible
                         if gated(state, v, j, g, nm))
                     for v, g in views]
            want = (_rival_desire(state, w, visible, views, gated,
                                  late)
                    if desire > 0.0 else None)
        for k, (view, gate) in enumerate(views):
            if not gated(state, view, i, gate, name):
                continue
            compete = reach[k]
            if want is not None:
                mine_i, total_i = want[k].get(i, (0.0, 0.0))
                if mine_i > 0.0 and total_i > 0.0:
                    # Effective competition: how many slots' worth of that
                    # rival's appetite this one slot is up against.  Equal
                    # to `reach` exactly when they want every reachable
                    # slot the same amount, which is what the uniform model
                    # assumed; larger when this card is one they do not
                    # want, and as small as 1.0 when it is the only card on
                    # the row that interests them.
                    eff = total_i / mine_i
                    if eff < 1.0:
                        eff = 1.0
                    compete = (1.0 - desire) * reach[k] + desire * eff
            survive *= 1.0 - rival_take_p(
                _rival_take_cost(name, i, gate), gate[0], compete,
                view.hand_slack, share)
        bargain += saving * survive
    return urgency, bargain


def row_last_copy(state, idx, w, ctx=None):
    """Value sitting in the row that, if I pass it up, I will never see again.

    THE FEATURE THE AUDIT WAS ACTUALLY ASKED FOR, in the project owner's own
    words: *"know that a second selective breeding is near the end or not and
    it has to build another farm since it won't see it"*.  `row_urgency`
    already knows a card is about to slide off the LEFT; it has never known
    whether another copy is coming off the deck, so a last copy and a card
    with three more behind it price identically.

    Each takeable slot contributes `card_potential * P(no further copy)`, and
    that probability is `max(0, 1 - outlook)` where `outlook` is
    `counting.civil_outlook`'s expected number of further copies.  The clip is
    the whole model and it has no fitted constant in it: at an outlook of 0
    the deck provably holds no more (an age's deck is always dealt out in
    full), so the probability is exactly 1; at an outlook of 1 or more another
    copy is expected, so it is 0; in between it interpolates.

    Deliberately NOT folded into `row_pressure`'s return.  That function's
    2-tuple is depended on by ~40 call sites across the tests and tools, and
    widening it to carry a third number would be a wide, silent change to code
    that is currently pinning real properties.  The duplicated gating loop is
    the cheaper mistake, and it is skipped entirely when the weight is 0.0.

    The outlook comes from `ctx` -- see `rival_context`'s `root_counts`.  With
    no ctx it is counted from `state`, which is correct at the search root and
    is the same degraded path `row_pressure` documents for `root_row`.
    """
    row = state.card_row
    if not row:
        return 0.0
    outlook = (ctx or {}).get("civil_outlook")
    if outlook is None:
        outlook = counting.civil_outlook(state, idx)
    root = ctx.get("root_row") if ctx else None
    cursor, n_root = 0, 0 if root is None else len(root)
    p = state.players[idx]
    mine = actions._take_gate(state, p, budget=actions.ca_total(state, p))
    gated = actions._can_take_gated
    late = lateness(state)
    total = 0.0
    for i, name in enumerate(row):
        if name is None:
            continue
        if root is not None:
            k = cursor
            while k < n_root and root[k] != name:
                k += 1
            if k >= n_root:
                break
            cursor = k + 1
        if not gated(state, p, i, mine, name):
            continue
        val = card_potential(name, w, state, idx, late)
        if val <= 0.0:
            continue
        gone = 1.0 - outlook.get(name, 0.0)
        if gone > 0.0:
            total += val * gone
    return total


# ------------------------------------------------------------ default weights

BASE_WEIGHTS = {
    # How much of the RATE HORIZON to believe: 0.0 prices every per-turn rate
    # flat, exactly as this file did before, and 1.0 scales it by
    # `rounds_left / mean rounds_left`.  See `rate_multiplier` for the
    # derivation -- there is no fitted constant in it -- and
    # docs/RATE_HORIZON.md for what it fixes.
    #
    # SHIPPED AT 1.0, AND 1.0 IS THE ONLY DEFENSIBLE VALUE.  At 1.0 the
    # multiplier IS the derived horizon; any other value is a hedge with a
    # fitted number in it, and docs/OPEN_ITEMS.md 3.0's constant taxonomy puts
    # rule-derived above fitted.  The thing it replaces is not a smaller
    # version of itself, it is a flat exchange rate that is wrong by 25x in
    # Age IV on the live 2p champion (docs/RATE_HORIZON.md section 2).
    #
    # LIVE ON ALL THREE LEAGUE ARMS THE MOMENT THIS LANDS, deliberately and on
    # the `tech_board_credit` / `gov_board_credit` / `action_board_credit`
    # precedent: the key is absent from every champion file on disk, so
    # `load_weights` fills it from here.  A default of 0.0 would have left the
    # fix switched off in exactly the arms that need it, which is the mistake
    # `card_board_credit` made and is still making for wonders
    # (docs/OPEN_ITEMS.md item 2.29).  It is a weight, so the league may climb
    # it away if it is wrong -- and `experiments/behaviour.py` now logs
    # `rounds_left`, `rate_mult` and `culture_rate_priced` per age so the run
    # output says whether it did.
    "rate_horizon": 1.0,
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
    # `card_potential` of the row cards that will never be dealt again, from
    # `counting.civil_outlook`.  See `row_last_copy`; 0.0 is inert.
    "row_last_copy": 0.0,
    # How much of the rival row model is DESIRE rather than uniform capacity.
    # 0.0 spreads a rival's civil actions evenly over every slot they can
    # reach, which is exactly what `row_pressure` did before and is therefore
    # free and bit-identical; 1.0 spreads them in proportion to what the cards
    # are worth on that rival's own board.  See `_rival_desire`.
    "rival_desire": 0.0,
    # NOT a value term -- a MODEL PARAMETER, and the only fitted number left
    # in `row_pressure`: the share of a rival's civil actions that goes on the
    # row rather than on building.  Everything else in `rival_take_p` is read
    # off their open board.  It lives in DEFAULT_WEIGHTS so `hillclimb.mutate`
    # can fit it instead of it being a bare constant nobody can move, and it is
    # positive so `guard_weights` keeps it out of negative-probability
    # territory.  It multiplies nothing when `row_bargain_forgone` is 0.0.
    "rival_take_share": 0.5,
    # public rival board/hand facts (GAP 3)
    "rival_free_ca": 0.0,
    "rival_hand_civil": 0.0,
    "rival_wonders": 0.0,
    "rival_hand_potential": 0.0,
    # the seven public rival board facts that were invisible until now
    # (docs/INFORMATION_AUDIT.md 0b.3), plus how many rivals are mid-wonder.
    # All 0.0: a champion trained before them evaluates bit-identically.
    "rival_science_stock": 0.0,
    "rival_food_stock": 0.0,
    "rival_resource_stock": 0.0,
    "rival_free_workers": 0.0,
    "rival_yellow_bank": 0.0,
    "rival_colonies": 0.0,
    "rival_mil_actions": 0.0,
    "rival_building_wonder": 0.0,
    # the politics pile, my own seeds only (GAP 4).  `my_seeded_pending` is a
    # linear feature; `my_event_threat` is priced through `w` and is eval-only.
    "my_seeded_pending": 0.0,
    "my_event_threat": 0.0,
    # WHICH opponent I am attacking.  Measured blind at 74-77% before this
    # (tools/target_blindness.py); 0.0 so no existing champion moves.
    "attack_target_lead": 0.0,
    "attack_target_weakness": 0.0,
    "pact_partner_lead": 0.0,
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
    # Also hand-only, and for a sharper reason than those two: a Military
    # Bonus card's defence is worth something precisely BECAUSE it is still in
    # hand -- spending it is what converts it, and once spent the card is
    # gone, so there is no board state for `features()` to mirror.  (Its
    # colonization half does have a board mirror and shares that weight;
    # see `_BONUS_TO_FEATURE`.)  0.0 for the usual reason: a new channel.
    "defense_bonus": 0.0,
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
    # NO `pop_food_discount` weight, deliberately.  Moses' food discount is
    # the one key in this block whose board side was never blind: `features`
    # subtracts it inside `pop_cost`, which carries a real trained -0.4.  A
    # second 0.0 feature beside a live one is not an extra channel, it is a
    # duplicate -- so the swap diff prices Moses through `pop_cost` too.  See
    # the note beside `board_yields._STATS_FEATURES`.
    # Gandhi: `Stats.no_aggression`.  Deliberately unsigned -- it bundles a
    # cost (he may never play an aggression or war) with a benefit
    # (opponents pay double to attack him), and which dominates is exactly
    # the sort of thing a league can measure and a prior cannot.
    "no_aggression": 0.0,
    # Patriotism / Wave of Nationalism / Military Build-Up / Churchill's
    # military option: resources ring-fenced to building military units.
    # Worth less than the same number of free resources by an amount only
    # the policy's own appetite for units decides.
    #
    # SUPERSEDED on the live path by `restricted_resource_credit` below, and
    # kept rather than deleted because it is still the STATIC answer:
    # `card_potential(name, w)` with no board (`analysis/`,
    # `tools/card_blindness.py`, the pricing censuses) has no `resource_stock`
    # marginal to take a fraction of, and `action_board_credit` = 0.0 sends the
    # live path back here too -- including `board_extra`'s half of it, which
    # still emits this key.  Churchill's ring-fenced pair is NOT priced through
    # here: `board_yields._churchill` deliberately prices him at his culture
    # option, which is the conservative read.
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
    # p = 0.15: docs/CARD_BLINDNESS.md 13.5.2 and its 2026-07-30
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
    # SUPERSEDED IN LIVE PLAY, and kept rather than deleted because it is
    # still the whole of the STATELESS answer.  `card_potential(name, w)` with
    # no board -- `analysis/`, `tools/card_blindness.py`, the pricing censuses
    # -- cannot ask what an upgrade would cost or what a point of strength is
    # worth here, so it falls back to this table and this credit.  Every
    # caller that decides a move has a state, so the credit no longer gates
    # any behaviour; `tests/test_half_priced_cards.py` still lists the ten
    # units against it, which is now a statement about the static table and
    # says so.
    "unit_strength_credit": 0.0,
    # How much of the BOARD-derived unit price to believe (`unit_tech_value`).
    #
    # 1.0, and unlike `unit_strength_credit` above that is not a guess at a
    # magnitude: at 1.0 the number is "the eval points `evaluate` itself
    # assigns to the strength this upgrade buys, minus the eval points it
    # assigns to the resources and science it costs", every term of it read
    # off the objective and the engine.  There is no free constant left in it
    # to be timid about.  0.0 recovers the static table byte for byte, which
    # is what makes the change A/B-able against itself in one process.
    #
    # It is a WEIGHT and not a hard-coded 1.0 for the usual reason -- the
    # league can move it -- but also for a specific one: the price is a
    # one-ply appraisal of a three-move plan (take, develop, upgrade), and how
    # much of a plan's value survives contact with the search is exactly the
    # kind of question a hill climb answers better than an argument does.
    "unit_tech_credit": 1.0,
    # How much of the BOARD-derived price to believe for EVERY OTHER
    # technology -- farm, mine, lab, temple, library, theater, arena and the
    # special technologies -- and, on a unit card too, how much of the
    # `tech_levels` / `num_techs` / `best_*` half that developing ANY
    # technology buys (`tech_value`, docs/CARD_BLINDNESS.md).
    #
    # 1.0 for the same reason `unit_tech_credit` is 1.0 and it is not a guess
    # either: at 1.0 the number is "the eval points `evaluate` itself assigns
    # to the technology levels and the production this development buys, minus
    # the eval points it assigns to the resources and science it costs".
    # `feature_marginal` reads every one of those off the objective.
    #
    # 0.0 is the ONE constant that recovers the parent commit's pricing byte
    # for byte on all 236 cards: the non-red technologies fall back to the
    # static table and the red ones lose the develop half, which is exactly
    # what `unit_tech_value` computed before this. That is what makes this
    # change A/B-able against itself in one process on the same deal.
    "tech_board_credit": 1.0,
    # The third of the same family, for the yellow ACTION cards
    # (`action_value`, docs/CARD_BLINDNESS.md).  1.0 on the same terms and
    # for the same reason: at 1.0 the number is "the eval points `evaluate`
    # itself assigns to the gains, the free civil action and the resources
    # this card produces", every term read off the objective by
    # `feature_marginal` and off the live boards by `board_yields.board_extra`.
    #
    # 0.0 is the one constant that sends every action card back to the static
    # table, i.e. the parent commit's pricing byte for byte, which is what
    # makes the change A/B-able against itself in one process on the same deal.
    # It is absent from every champion file in the league, so `load_weights`
    # fills it from here and the fix is live on all three arms at once --
    # deliberately, because the defect it fixes is present on all three.
    "action_board_credit": 1.0,
    # The fourth of the same family, for GOVERNMENTS (`gov_value`,
    # docs/GOVERNMENT_PRICING.md).  1.0 on the same terms: at 1.0 the number is
    # "the eval points `evaluate` itself assigns to the civil actions, the
    # military actions, the urban limit and the two levels this government
    # buys, minus the eval points it assigns to the science and the actions
    # the cheaper of the two legal routes to it costs".  Every term is read
    # off the objective by `feature_marginal` and off the board by
    # `board_yields.government_plans`; there is no free constant in it, and in
    # particular no fitted rate for how often a revolution is the right route
    # -- that is asked of the board, per card, every time.
    #
    # 0.0 is the one constant that sends every government back to the static
    # table, i.e. the parent commit's pricing byte for byte, which is what
    # makes the change A/B-able against itself in one process on the same
    # deal.  It is absent from every champion file, so `load_weights` fills it
    # from here and the fix is live on all three arms at once.
    "gov_board_credit": 1.0,
    # How much of the "develop it and BUILD one fresh worker on it" plan to
    # believe (`tech_value` -> `board_yields.build_fresh`,
    # docs/OPEN_ITEMS.md section 2 items 21 and 28).
    #
    # The fifth of the same family and the one that closes the last hole in
    # it.  `tech_upgrade` prices only "develop it and upgrade what I already
    # have", so a technology of a type this player has never staffed was
    # priced at its levels minus its science and nothing else -- and for
    # Knights, Cannon and Air Forces, the lowest card of their own type in the
    # base game, NO board can ever offer an upgrade, so that was their whole
    # price forever.  Air technologies were taken 0.00 times a seat-game.
    #
    # 1.0 on the same terms as its four siblings: at 1.0 the number is "the
    # eval points `evaluate` itself assigns to the strength (or production, or
    # happiness) one fresh worker on this technology produces and to the free
    # worker it consumes, minus the eval points it assigns to the resources it
    # costs".  Every term is read off the objective by `feature_marginal` and
    # off the board by `board_yields.build_fresh`, whose legality gate is
    # `engine/actions.py:_action_moves`' own.  There is no free constant in it.
    #
    # 0.0 is the one constant that recovers the parent commit's pricing byte
    # for byte on all 236 cards -- `tech_value`'s argmax loses one branch and
    # the remaining branch is unchanged -- which is what makes the change
    # A/B-able against itself in one process on the same deal.
    #
    # AND IT SHIPS AT 0.0, WHICH ITS FOUR SIBLINGS DID NOT.  That is the
    # measurement's decision and not a hedge.  Paired A/B, `experiments.
    # evaluate`, 400 games / 200 deals at 2p, the credit against ITSELF at
    # 0.0 with nothing else touched:
    #
    #     vector                        win rate      p        culture
    #     DEFAULT_WEIGHTS   1.0         44.1 +/- 4.6  0.0125   105 vs 112
    #     live 2p champion  1.0         44.5 +/- 4.8  0.0229   145 vs 156
    #     DEFAULT_WEIGHTS   0.5         44.9 +/- 3.6  0.0055   112 vs 114
    #
    # Two independent vectors, the same ~5.5pp loss, both significant.  The
    # champion is the informative one: it carries `workers` = 0.0 and
    # `free_workers` = 0.005 where DEFAULT_WEIGHTS carries 1.4 and 0.4, so
    # "the untrained worker priors are the cause" is ruled OUT -- the loss is
    # the same with those priors trained to nothing.  `tech_board_credit`
    # itself has been climbed to 0.334 on that vector, i.e. the league is
    # already discounting board-derived technology prices, and this is one
    # more of them.
    #
    # AND IT IS A SWITCH, NOT A LEVEL: 0.5 and 1.0 measure the same, because
    # this credit multiplies ONE BRANCH OF A `max` and any epsilon > 0 already
    # wins that max on every card whose upgrade plan is worth exactly nothing
    # -- Knights, Cannon and Air Forces on every board, and any technology of
    # a type the player has never staffed.  So "the league will find the
    # level" is NOT available here; `hillclimb.mutate` perturbs by
    # `gauss(0, s) * (abs(w) + 0.15)` and that 0.15 floor steps straight over
    # the cliff.  Whoever re-opens this should expect to fix the PRICE, not to
    # find a good value for the credit.  docs/OPEN_ITEMS.md section 2 item 31
    # carries the two candidates that survived the champion control.  Every
    # other caller of `build_fresh` (the tests, `tools/take_census.py` and
    # `tools/card_census.py` with their own vectors) gets the price today by
    # passing the credit.  The precedent for landing a measured-worse credit
    # at 0.0 rather than tuning it is `free_action_credit` (item 27).
    "build_fresh_credit": 0.0,
    # How much of a real resource a resource ring-fenced to military units is
    # worth (`_RESTRICTED_TO_FEATURE`).  1.0 is the UPPER bound -- a resource
    # you may only spend one way is worth at most one you may spend any way,
    # and worth nothing at all to a player who builds no units this turn -- and
    # it is the bound rather than a fitted number because how often that is, is
    # exactly what a league measures and an argument cannot.  A credit rather
    # than the absolute `restricted_resources` price it replaces because an
    # absolute one is unreachable: `features()` emits no such feature, so
    # `evaluate` never pays for it, so no game can produce a gradient on it --
    # which is why it is 0.0 on every champion in the pool.  See `action_value`.
    "restricted_resource_credit": 1.0,
    # How much of a civil action the ordered action on an 18-card block of
    # yellow cards ("build or upgrade a farm or a mine", "develop a
    # technology") is worth.  **0.0, and it is a measurement, not caution.**
    # Playing the card costs one civil action and grants one, so the action
    # economy is a wash and the card is worth its DISCOUNT; at 1.0 the take
    # rate lands exactly on the human number and the paired A/B is 32.8%
    # against a 50% null, because the actions stop buying technologies.  Full
    # derivation and the arms in `action_value` point 2 and
    # docs/CARD_BLINDNESS.md section 16.5.
    "free_action_credit": 0.0,
    # `territory_credit` is 1.0 but costs nothing until `hand_mil_potential`
    # is non-zero, because nothing calls `card_potential` on a military card
    # otherwise.  It is a separate knob from `hand_mil_potential` so that
    # "how much of a territory's printed effect to believe" -- it is seeded
    # into an auction anyone can win, not played -- stays separable from
    # "how much the military hand matters at all".
    "territory_credit": 1.0,
    # ...and the same knob for the three Military Bonus cards, 1.0 on the
    # same terms: it costs nothing under DEFAULT_WEIGHTS, where
    # `hand_mil_potential` is 0.0 and nothing calls `card_potential` on a
    # military card, and 0.0 recovers the pre-change pricing exactly for an
    # A/B.  Note this is NOT inert for the live 3p champion, which carries
    # `hand_mil_potential = 0.01079` -- that vector is the one place in the
    # league where this change conducts at all (docs/COMBAT_AUDIT.md).
    "bonus_card_credit": 1.0,
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
# Derived from PHASE_KEYS rather than written out, so the two can never
# disagree -- a pair for a key that is no longer phase-multiplied would be a
# weight the trainer mutates, the guard checks and `evaluate` never reads,
# which is this repo's oldest bug shape.  See the note on PHASE_KEYS for the
# six pairs that were retired on 2026-08-04 and why.
_PHASE_PRIOR = {
    "workers_early": 0.8, "workers_late": -0.6,
    "strength_rel_early": -0.1, "strength_rel_late": 0.5,
    "tech_levels_early": 0.5, "tech_levels_late": -0.4,
    "hand_value_early": 0.2, "hand_value_late": -0.2,
}
PHASE_WEIGHTS = {k + s: _PHASE_PRIOR[k + s]
                 for k in PHASE_KEYS for s in ("_early", "_late")}

DEFAULT_WEIGHTS = dict(BASE_WEIGHTS)
DEFAULT_WEIGHTS.update(PHASE_WEIGHTS)


# ------------------------------------------------------------- evaluation

def evaluate(state, idx, weights=None, ctx=None, f=None):
    w = weights if weights is not None else DEFAULT_WEIGHTS
    if f is None:
        # priced_only: the loop below drops every term whose weight is falsy,
        # so computing those terms is pure waste HERE (and only here -- see
        # `features`, which still hands instruments the whole vector).
        f = features(state, idx, ctx, w, priced_only=True)
    total = 0.0
    get = w.get
    # `rate_multiplier` is 1.0 unless the vector asks for the horizon, and the
    # `hz != 1.0` guards below keep this loop byte-identical when it is.
    hz = rate_multiplier(state, w)
    for k, v in f.items():
        wk = get(k)
        if wk:
            total += wk * v * hz if (hz != 1.0 and k in RATE_KEYS) else wk * v
    late = lateness(state)
    early = 1.0 - late
    for k in PHASE_KEYS:
        v = f[k]
        if not v:
            continue
        # the phase pair is part of the same price, so it carries the same
        # horizon -- see `feature_marginal`, which sums exactly these three.
        vv = v * hz if (hz != 1.0 and k in RATE_KEYS) else v
        we = get(k + "_early")
        if we:
            total += we * early * vv
        wl = get(k + "_late")
        if wl:
            total += wl * late * vv
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
    # Card counting, priced through `w` like everything else in this block and
    # therefore eval-only.  Default 0.0, so a champion trained before the count
    # existed evaluates bit-identically and pays nothing for it.
    rlc = get("row_last_copy")
    if rlc:
        total += rlc * row_last_copy(state, idx, w, ctx)
    # The events I planted myself, priced through `w` and therefore eval-only
    # for the same reason `hand_potential` is.  Skipped entirely at scale 0.0,
    # which is the default -- so every existing champion evaluates
    # bit-identically until a trainer fits this.
    met = get("my_event_threat")
    if met:
        total += met * my_event_threat(state, idx, w)
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
        # made us maximise a RIVAL's position (docs/COMBAT_AUDIT.md).
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


# -------------------------------------------------------- dominance guard
#
# THE HOLE THIS CLOSES.  `hillclimb_league.guard_weights` catches a value term
# whose SIGN is inverted, and it deliberately exempts the `_early`/`_late`
# phase multipliers (see the EXEMPTION note there, which is right about what
# it measured).  So nothing ever checked the NET weight a phase-multiplied
# term is evaluated at, and nothing checked two terms against each other.
# Both holes were open and the league walked through both:
#
#   champion_2p   culture 1.0 + culture_early -1.3113 = -0.31 early
#                 -> losing 3 culture RAISED its own evaluation by +0.55
#   champion_2p   resource_stock 0.0 < blue_free 0.4220
#                 -> being plundered of 4 resources was worth +1.27
#
# Measured consequence, which is how this was found: at a defence it could
# win by playing four cards, the 2p champion scored `("defend_done",)` ABOVE
# the winning line and handed the aggression over.  The search was not blind
# -- it played the whole defence out and saw the win.  It declined it.
#
# Both constraints below are RULE FACTS, not strategy:
#
# * CULTURE IS THE SCORE.  A civilization is never worse off for having more
#   of it, at any point in the game.  No lateness may make its net weight
#   negative.  The other phase-multiplied terms are deliberately NOT listed:
#   more workers costs consumption, resource production really is close to
#   worthless on the last turn, and a net-negative there is a strategic claim
#   the league is entitled to make.  A net-negative on culture is a claim that
#   losing the game is good.
# * A RESOURCE IN STOCK DOMINATES THE BLUE TOKEN IT SITS ON.  Spending the
#   resource hands that token back to the bank AND buys the thing it paid
#   for, so a stocked resource is worth at least a free token whatever either
#   is worth.  A vector that ranks them the other way is one where being
#   robbed is a gain.
#
# WHICH SIDE IS REPAIRED, and why it is not symmetric: the pair is repaired by
# RAISING `resource_stock` to `blue_free`, not by lowering `blue_free` to it.
# `blue_free` was climbed (0.15 default -> 0.4220); `resource_stock` sat at
# exactly 0.0, which is the signature of a coordinate the climb never moved
# rather than one it measured.  Raising keeps what was learned and fixes only
# the ordering.  The phase pair is repaired to the BOUNDARY (net 0), not to
# the default multiplier, for the same reason: the smallest change that makes
# the vector expressible.

#: Terms whose net weight (`k` plus either phase multiplier) may not go
#: negative, because a pure gain of them cannot hurt under the rules.
#:
#: `wonder_progress` is `sum(stages[:built])` -- resources ALREADY PAID into
#: the wonder in progress.  Those resources have left `resource_stock`
#: already, so at equal everything-else a civilization with stages built is
#: strictly closer to the culture the wonder pays and never worse off.  The
#: risk of a half-built wonder is real and is priced by four OTHER features
#: (`wonder_remaining`, `wonder_stages_left`, `wonder_turns_to_finish`,
#: `wonder_overrun`), which is where a negative weight belongs; a negative net
#: on `wonder_progress` itself says the paid stages are the problem.
#: champion_4p carries base 0.0 with both multipliers negative (net -0.165
#: early, -0.213 late), i.e. a standing instruction not to build wonders.
#: NOT MEASURED HERE: what that did to its wonder play rate.  The repair
#: leaves the term at exactly neutral and lets the league price it upward.
#:
#: EMPTY SINCE 2026-08-04, and that is a STRONGER guarantee than the guard it
#: replaces, not a retreat.  Both entries were here because a phase multiplier
#: could drag a stock's net weight below zero; both stocks have since had their
#: phase pair DELETED outright (see PHASE_KEYS), so there is no multiplier left
#: to drag anything.  A clamp you cannot reach beats a clamp that fires.  The
#: loop below is kept, and empty, because the next phase-multiplied stock -- if
#: anyone adds one -- needs it, and because deleting it would delete the
#: argument with it.
NET_NONNEG_PHASE = ()

#: ``(dominant, dominated)`` -- `w[dominant] >= w[dominated]`, repaired by
#: raising the dominant side.
DOMINATES = (("resource_stock", "blue_free"),)

#: Weights that scale a PRINTED BENEFIT on one card class and nothing else.
#: None of them may be negative.
#:
#: THE HOLE.  `hillclimb_league.NONNEG` is derived as `{k: DEFAULT[k] > 0}`, so
#: a term whose default is exactly **0.0** is in neither NONNEG nor NONPOS and
#: is sign-guarded by nothing at all.  That is the same shape as
#: `resource_stock` above -- 0.0 is not a sign violation, so nothing fires --
#: and it is the shape of every class credit in the vector, because a credit is
#: added switched off so the league can price it from scratch.
#:
#: WHY THE SIGN IS A RULE AND NOT AN OPINION.  Each of these is the ONLY
#: per-card channel its class has (`tests/test_play_rate.class_gates` derives
#: exactly that set by perturbation), and raising it raises `card_potential`
#: for EVERY card in the class -- `+1.0 .. +3.0` for the three build techs,
#: `+1.0 .. +5.0` for the Military Bonus cards, and so on.  The printed text is
#: a grant in every case: more stages per action, a bigger hand, a discount, an
#: extra civil action, more strength.  You are never compelled to use a grant,
#: so under the rules a card that prints one is never worse than the same card
#: without it.  A negative weight says the class's own printed benefit is a
#: reason NOT to take the card.
#:
#: MEASURED, which is why this exists: `wonder_stages_per_action` is negative
#: on ALL THREE live champions (-0.136 / -0.036 / -0.041).  Masonry,
#: Architecture and Engineering are the only cards it touches, and they are the
#: cards that make wonders cheap in actions -- so every champion has been
#: carrying a standing markdown on the build techs, next to the negative net
#: `wonder_progress` in NET_NONNEG_PHASE above.  Same complaint, two
#: coordinates.  `unit_strength_credit` (the failure this whole test file was
#: written for), `restricted_resources`, `free_civil_action`, `hand_limit` and
#: `build_discount` are negative on at least one trained vector too.
#:
#: The repair is to 0.0, not upward: the rules say "not a cost", they do not
#: say what it is worth.  Pricing it is the league's job, and
#: `tests/test_play_rate.py::TestNoClassIsDeadOnEveryTrainedVector` is the
#: ratchet that notices when it does.
BENEFIT_GATES = ("build_discount", "card_board_credit", "defense_bonus",
                 "free_civil_action", "hand_limit", "resource_discount",
                 "restricted_resources", "unit_strength_credit",
                 "wonder_stages_per_action")


def dominance_repair(w):
    """Return ``(weights, violations)`` with the rule-level orderings restored.

    Pure and idempotent: repairing an already-legal vector returns it
    unchanged with an empty violation list, so it is safe to apply on every
    load and again in the trainer's guard.
    """
    out = dict(w)
    viol = []
    for k in NET_NONNEG_PHASE:
        base = float(out.get(k, DEFAULT_WEIGHTS.get(k, 0.0)))
        for suf in ("_early", "_late"):
            mk = k + suf
            m = float(out.get(mk, 0.0))
            if base + m < -1e-12:
                viol.append({"weight": mk, "value": round(m, 4),
                             "default": DEFAULT_WEIGHTS.get(mk, 0.0),
                             "rule": f"{k} + {mk} >= 0"})
                out[mk] = -base
    for hi, lo in DOMINATES:
        a = float(out.get(hi, DEFAULT_WEIGHTS.get(hi, 0.0)))
        b = float(out.get(lo, DEFAULT_WEIGHTS.get(lo, 0.0)))
        if b > a + 1e-12:
            viol.append({"weight": hi, "value": round(a, 4),
                         "default": DEFAULT_WEIGHTS.get(hi, 0.0),
                         "rule": f"{hi} >= {lo}"})
            out[hi] = b
    for k in BENEFIT_GATES:
        v = float(out.get(k, DEFAULT_WEIGHTS.get(k, 0.0)))
        if v < -1e-12:
            viol.append({"weight": k, "value": round(v, 4),
                         "default": DEFAULT_WEIGHTS.get(k, 0.0),
                         "rule": f"{k} >= 0 (scales a printed benefit)"})
            out[k] = 0.0
    return out, viol


# ------------------------------------------------------------------- io

def load_weights(path):
    """Load a weight vector, with the rule-level orderings enforced.

    The repair is applied HERE and not only in the trainer because a champion
    JSON is read by the arena, by every tool and by the live league alike; a
    guard that only ran inside `hillclimb_league` would leave every one of
    those playing the unrepaired vector.  `dominance_repair` is idempotent, so
    a file the trainer has already written passes through untouched.

    `RETIRED_KEYS` are dropped on the way in.  Every champion on disk predates
    their removal and still carries them; `evaluate` would ignore them either
    way, but `hillclimb.mutate` walks the loaded vector's OWN keys, so leaving
    them in means the trainer spends part of every generation's mutation
    budget perturbing weights nothing reads.
    """
    import json
    with open(path) as fh:
        d = json.load(fh)
    w = dict(DEFAULT_WEIGHTS)
    w.update(d.get("weights", d))
    for k in RETIRED_KEYS:
        w.pop(k, None)
    return dominance_repair(w)[0]


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
