"""What a card is worth ON THIS BOARD, computed by asking the engine.

`weighted._card_yields` prices a card by looking its `production`/`effects`
keys up in a static table.  That works for a card whose whole content is a
number and fails completely for a card whose value is written in prose --
docs/CARD_BLINDNESS.md counts 149 of 236 cards with no visible gain at all,
and **all 24 leaders** are in that set.

Leaders are the extreme case.  Sixteen of the twenty-four are worth *nothing*
to the evaluator beyond "it is a leader", because what they print is
conditional or relative to your board:

    Michelangelo    culture per happy face from temples/theaters/wonders
    Napoleon        +2 strength per type of military unit you field
    Sid Meier       each lab produces culture equal to its level
    Shakespeare     2 culture per library/theater pair
    ...

The temptation is a table of hand-written handlers, one per key.  Do not do
that.  `engine/effects.py:1197-1202` is the note left by the last person who
did: Hollywood and Internet score off `_BUILDING_OUTPUT`, and before that fix
the code "summed printed values with an ad-hoc Sid Meier special case, which
under-scored every Chaplin, Shakespeare, Newton and Einstein completion."  Two
implementations of one rule drift, and the evaluator's copy drifts silently.

So this module does not reimplement a single rule.  **It swaps the card in and
asks `engine.effects.compute` what changed.**

    old = p.leader
    p.leader = "Michelangelo"
    after = effects.compute(state, p)      # the real rules engine
    p.leader = old
    delta = after - effects.state_stats(state, p)

All thirteen of the `effects.MODIFIER_KEYS` that any leader carries are then
priced exactly, for free, and can never drift, because it *is* the rules.

------------------------------------------------- what the diff CANNOT see

**The guarantee is narrower than "it is the rules", and it has already failed
once.**  A swap diff is exact over `Stats` and **blind to everything else**.
`compute` builds the per-turn ratings; anything a card does that is not a
rating -- a token grant, one-time culture, a boolean flag, a turn trigger --
is invisible to it by construction.

That would be merely incomplete, except that `weighted.card_potential` prices
a swap card by the diff **ALONE** (it has to: otherwise the printed culture
already inside the delta would be counted twice).  So for a swap type, the
diff does not *supplement* the static `_card_yields` table, it **replaces**
it -- and any key the static table priced that the diff cannot see is
silently DROPPED the moment `card_board_credit` goes non-zero.

That is exactly what happened to Taj Mahal's `blueTokens`: not a `Stats`
field (`on_enter_play` puts it on `p.blue_total`), priced by
`_EFF_TO_FEATURE -> blue_free` on the static path, and worth nothing here
until `_blue_tokens` was added below.  `tests/test_card_pricing.py` could not
catch it, because `blueTokens` *is* priced -- somewhere.  See
docs/SCORE_AUDIT.md, "A caveat on the swap-diff technique".

**So: every effect key a SWAP TYPE carries needs either a `Stats` field, a
rider, or a written reason.**  `WONDER_RIDERS` and `RIDERS` are where the
first two live; `BOARD_PRICED` at the bottom is where the reason goes.
Three further things fall out that no per-key handler gets right by
construction:

1. **Replacement.**  A leader replaces the leader you have.  Taking Einstein
   while you hold Michelangelo is worth Einstein *minus Michelangelo*, which
   is a diff, not an absolute, and is sometimes negative.
2. **Clamps.**  `compute` ends with `happy = max(0, min(8, happy))` and
   `rating = max(0, rating)`.  A leader's ninth happy face is worth zero and
   the diff says so.
3. **Governments, for free.**  `compute` reads `p.government`'s *top-level*
   `civilActions` / `militaryActions` / `urbanBuildingLimit`, which are not in
   a `production` or `effects` block at all -- so `_card_yields` has never
   read them, and **Republic's 7 civil actions against Despotism's 4 has been
   invisible to the evaluator**.  Civil actions are the game's core currency
   and this is the largest single source of them.

------------------------------------------------------------------ the trap

**Use `compute`, never `state_stats`, for the hypothetical.**

`state_stats` is a per-mutation cache keyed on `p.idx`, validated against
`stats_key(state, p)` and *only rebuilt when the entry is marked dirty*.
Mutating `p.leader` and calling `state_stats` does not mark it dirty, so it
returns the stats of the OLD leader and the diff comes out as exactly zero --
a silent, total failure that no test of the mapping would catch.  `compute`
bypasses the cache entirely and never writes to it, so the swap leaves the
cache correct and valid for the real leader after we restore it.

`tests/test_board_yields.py:TestTheComputeVsStateStatsTrap` fails if anyone
switches these two calls around.

------------------------------------------------------------ memoisation

`compute` is hot -- roughly ten calls per generated move already -- and this
adds one per swap-type card in hand or in the row per leaf.  The result is
memoised on `(name, effects.stats_key(state, p))`.

`stats_key` carries the documented invariant that it names every field
`compute` reads, which is exactly the completeness this key needs, and it
already includes `p.leader` and `p.government` so the *current* side of the
diff is pinned too.  The docstring is not taken on trust:
`tests/test_board_yields.py:TestStatsKeyIsACompleteMemoKey` plays self-play
games, records every `(stats_key -> compute)` pair it sees, and fails if one
key ever maps to two different `Stats`.  A key that missed a field would give
silently stale valuations, which is a worse bug than the blindness this
module exists to fix.
"""
from __future__ import annotations

from .. import actions as A, cards as C, economy, effects
from ..state import TechCard

__all__ = ["board_yields", "board_choices", "unit_upgrade", "tech_upgrade",
           "SWAP_TYPES", "SINGLE_SLOT", "BOARD_PRICED", "LEVELLED_TYPES"]

_DB = C.db()

# Card types priced by swapping the card in and diffing `effects.compute`.
# Leader and government are single-slot: you have exactly one of each, and
# playing the card replaces what is there, which is what makes a diff the
# right question.
#
# WONDERS were excluded when this module landed, on the reasoning that "a
# wonder accumulates rather than replaces".  That is true and it is an
# argument for the diff being SIMPLER here, not for it being wrong: append
# the wonder to `p.completed_wonders`, compute, restore, and the delta is a
# pure gain with nothing to net off.  Doing it buys the same exactness for
# Great Wall (`strengthPerInfantry`, `strengthPerArtillery`), St. Peter's
# Basilica (`extraHappyPerHappySource`) and Transcontinental Railroad
# (`doubleBestMine`) that the leader swap bought for Michelangelo -- clamps
# included, so St. Peter's ninth happy face is correctly worth nothing.
#
# Two consequences worth knowing.  A wonder is not free, so `_wonder_cost`
# adds back the stage resources and the generic `wonders` term that
# `_card_yields` supplies for the static path (exactly as `_government_cost`
# adds back a government's science).  And a wonder's printed `culture` moves
# from `_Y_RATE` to `_GAIN`, so `card_rate_credit` no longer gates it when
# board pricing is on -- the same trade this module already accepted for
# Gandhi's printed +2.
SWAP_TYPES = frozenset(("leader", "government", "wonder"))

# The types that are single-slot, which is a STRICTLY SMALLER claim than
# "priced by a swap diff" and is the one `weighted._hand_total` needs.
#
# You hold one leader and one government, so two leaders in hand are two
# candidates for ONE replacement and summing them is arithmetic nonsense.
# A wonder is different in exactly the way the block above says: its diff is
# a pure gain with nothing netted off, and two wonders in hand really can
# both be built, one after the other.  Holding both is over-optimistic about
# TIME, not impossible -- and the time is what `wonder_turns_to_finish` /
# `wonder_overrun` exist to price.  So a wonder is a swap card and is NOT
# collapsed; keying the hand collapse on `SWAP_TYPES` would silently start
# collapsing wonders the moment they joined that set, which is why these are
# two sets and not one.
SINGLE_SLOT = frozenset(("leader", "government"))

# Which card types are board-priced is NOT decided here any more.  It used to
# be `TTA_BOARD_TYPES`, an environment variable read once at import, which
# meant only a human running a command could turn the government half on and
# the league could never learn it.  It is now four weights -- `card_board_
# leader`, `card_board_government`, `card_board_action`, `card_board_wonder`
# -- resolved by `weighted._board_credit_key`, which is where the credit they
# offset lives too.  This module just prices; the caller decides how much of
# the price to believe.
#
# `weighted.card_potential` calls neither `board_yields` nor `board_extra`
# when the relevant credit is 0.0, so the cost of a disabled type is still
# nothing.

# --------------------------------------------------------------- features
#
# `Stats` field -> evaluator feature.  Only fields whose delta means
# something to the evaluator; the pact-derived ones (`tech_discount`,
# `war_immune`, `food_as_resource`, `resource_as_food`) are Lane D's and no
# leader or government carries them.
_STATS_FEATURES = (
    ("culture", "culture_rate"),
    ("science", "science_rate"),
    ("food", "food_rate"),
    ("resources", "resource_rate"),
    ("strength", "strength"),
    ("happy", "happy_margin"),
    ("civil_actions", "civil_actions"),
    ("military_actions", "military_actions"),
    ("colonize", "colonize_bonus"),
    # new channels, 0.0 by default (see weighted.BASE_WEIGHTS)
    ("urban_limit", "urban_limit"),
    ("wonder_stages", "wonder_stages_per_action"),
)

# NOT in the table above, deliberately: `pop_food_discount`.
#
# Moses' "increasing your population costs 1 food less" is the one key here
# whose board side was NEVER blind.  `weighted.features` subtracts
# `Stats.pop_food_discount` from `pop_cost`, which carries a real trained
# weight of -0.4, so a player holding Moses has always been valued correctly.
# Giving the delta its own `pop_food_discount` feature therefore did not fix
# an asymmetry, it created a SECOND representation of one quantity sitting at
# 0.0 next to a live one -- the same shape as `buildDiscount` summed instead
# of maxed, and as the hand double-count.
#
# So the diff prices Moses through `pop_cost`, the feature the board
# evaluation actually reads, and there is exactly one representation again.
# `economy.pop_food_cost` is the single implementation of the formula, shared
# with `features` and `neural_encode`.
_POP_SENTINEL = 8.0     # `features`' "cannot increase population at all"

# the third slot of a yield triple, mirroring weighted._Y_GAIN / _Y_COST.
# Imported by value rather than from weighted to keep this module free of a
# circular import; `tests/test_board_yields.py` asserts they agree.
_GAIN = 0
_COST = 1


def _swapped(state, p, field, name):
    """`effects.compute` with `p.<field>` temporarily holding `name`.

    See "the trap" in the module docstring: this MUST be `compute` and not
    `state_stats`.  `try/finally` because a raise here would leave the player
    holding a card they do not own.

    `completed_wonders` is a LIST and is appended to rather than replaced --
    a wonder adds to what you have.  The list is rebound to a new object
    rather than mutated in place, so a journal that is watching the original
    list never sees a write and `finally` restores by identity.
    """
    old = getattr(p, field)
    setattr(p, field, list(old) + [name] if field == "completed_wonders"
            else name)
    try:
        return effects.compute(state, p)
    finally:
        setattr(p, field, old)


# (name, stats_key) -> yield triples.  `stats_key` names every field
# `compute` reads, so it is a complete key for the diff; the test named in
# the module docstring checks that empirically rather than trusting it.
_DELTA_CACHE = {}
_DELTA_CACHE_MAX = 200_000


def _pop_cost(stats, p):
    """`weighted.features`' `pop_cost`, to the letter, including its
    sentinel -- so the diff and the board cannot disagree about Moses."""
    got = economy.pop_food_cost(stats, p.yellow_bank)
    return _POP_SENTINEL if got is None else float(got)


def _delta_triples(before, after, p):
    """Yield triples for the difference between two `Stats` of one player.

    Factored out of `_stats_delta` so the swap diff (a leader / government /
    wonder replacing what you have) and the technology diff (`tech_upgrade`,
    below) read the SAME fields through the SAME feature names.  Two copies of
    this list is precisely how `_PROD_TO_FEATURE` and `_YIELD_TO_FEATURE`
    drifted apart, which is the bug class this module keeps paying for.
    """
    out = []
    for attr, feat in _STATS_FEATURES:
        d = getattr(after, attr) - getattr(before, attr)
        if d:
            out.append((feat, float(d), _GAIN))
    d = ((after.civil_hand_limit + after.military_hand_limit)
         - (before.civil_hand_limit + before.military_hand_limit))
    if d:
        out.append(("hand_limit", float(d), _GAIN))
    d = (sum(after.build_discount.values())
         - sum(before.build_discount.values()))
    if d:
        out.append(("build_discount", float(d), _GAIN))
    # Moses, priced through the feature the board evaluation actually reads.
    # See the note beside `_STATS_FEATURES`.
    d = _pop_cost(after, p) - _pop_cost(before, p)
    if d:
        out.append(("pop_cost", float(d), _GAIN))
    # Gandhi: `cannotPlayAggressionOrWar`.  A real cost (his owner may never
    # play an aggression or a war again) bundled with a real benefit
    # (`opponentsPayDoubleMilitaryActionsToAttackYou`, which is not in Stats).
    # The net sign is genuinely unknown, which is why the weight defaults to
    # 0.0 and the league decides rather than a prior deciding.
    # Symmetric on purpose: replacing Gandhi LIFTS the restriction, and that
    # is a real change in the other direction.  An asymmetric flag would make
    # Gandhi a one-way ratchet the evaluator could never price its way out of.
    d = int(after.no_aggression) - int(before.no_aggression)
    if d:
        out.append(("no_aggression", float(d), _GAIN))
    return tuple(out)


def _stats_delta(state, p, field, name):
    key = (name, effects.stats_key(state, p))
    hit = _DELTA_CACHE.get(key)
    if hit is not None:
        return hit
    before = effects.state_stats(state, p)
    after = _swapped(state, p, field, name)
    out = _delta_triples(before, after, p)
    if len(_DELTA_CACHE) >= _DELTA_CACHE_MAX:
        _DELTA_CACHE.clear()
    _DELTA_CACHE[key] = out
    return out


# --------------------------------------------------------------- riders
#
# What the swap diff CANNOT see, card by card.  `compute` builds the
# per-turn ratings, so anything that pays out on an event rather than in the
# production phase is not in `Stats` and has to be priced here.  This is the
# part that is a judgement rather than a derivation, so every entry says what
# it is worth and why, and every feature it lands on defaults to 0.0.


def _live_rivals(state, p):
    return [q for q in state.players if q.idx != p.idx and not q.resigned]


def _genghis(state, p):
    """Genghis Khan: 3 culture at the end of your turn if you are one of the
    two strongest civilizations, ties in your favour.

    Not a `Stats` field -- it is a turn-end trigger -- but it is *exactly*
    computable from the board, and it fires every turn it is true, so it is
    honest per-turn culture production and belongs on `culture_rate` rather
    than on a trigger weight.

    Note what this says at 2 players: "one of the two strongest" out of two
    civilizations is unconditionally true, so Genghis is a flat +3 culture a
    turn there and a conditional elsewhere.  A static table cannot express
    that difference and this is the whole reason the pricing is board-aware.
    """
    mine = effects.state_stats(state, p).strength
    stronger = sum(1 for q in _live_rivals(state, p)
                   if effects.state_stats(state, q).strength > mine)
    return (("culture_rate", 3.0, _GAIN),) if stronger <= 1 else ()


def _churchill(state, p):
    """Winston Churchill: once each turn, 3 culture, or 3 restricted science
    plus 3 restricted resources.

    The culture option is unconditional, needs no board and no other card, and
    is available every single turn -- so the floor on Churchill is a flat +3
    culture production, which is more than any wonder in the game prints.  The
    military option is priced as the same thing at a discount: it is 6 points
    of raw material, but both halves are ring-fenced (science usable only on
    military unit techs, resources only on military units), and a bot that
    wants neither gets nothing, so it cannot be worth its face.  Taking the
    culture option as the value of the card is the conservative read and the
    one the engine's own `("churchill", "culture")` move makes available every
    turn regardless of board state.
    """
    return (("culture_rate", 3.0, _GAIN),)


# name -> rider function.  Only the leaders whose value is NOT in `Stats`
# and IS computable; the rest stay in weighted.DELIBERATELY_UNPRICED with a
# written reason, which is the honest place for them.
RIDERS = {
    "Genghis Khan": _genghis,
    "Winston Churchill": _churchill,
}


def _rider_delta(state, p, name):
    """Rider triples for taking `name`, MINUS the rider of the leader it
    replaces.

    The subtraction is the whole point and is easy to forget.  `Stats` is
    diffed by `_stats_delta`, so the replacement is handled there; a rider
    lives outside `Stats` and would otherwise be counted as a pure gain.
    Taking Gandhi (+2 printed culture) while holding Churchill (+3 culture a
    turn from his rider) is a LOSS of one culture a turn, and only a rider
    that subtracts can say so.
    """
    out = []
    new = RIDERS.get(name)
    if new is not None:
        out.extend(new(state, p))
    cur = RIDERS.get(p.leader)
    if cur is not None:
        for feat, amt, kind in cur(state, p):
            out.append((feat, -amt, kind))
    return tuple(out)


# ------------------------------------------------------- government costs


def _government_cost(state, p, name, out):
    """The science a government actually costs, and the actions it burns.

    Two routes exist and the engine implements both:

    * peaceful -- `effects.tech_cost` charges `peacefulCost` science (a
      government is developed like a technology; `techCost` is `null` on
      every government card, which is exactly why `_card_yields`, which reads
      `techCost`, has priced all eight of them as FREE).
    * revolution -- `actions._h_revolution` charges `revolutionCost` science
      and empties the civil action pool for the turn (`p.civil_actions = 0`),
      or the military pool under Robespierre.

    `revolutionCost` is cheaper in science on every card in the deck
    (Monarchy 2 vs 8, Democracy 9 vs 17), so it is the route that is priced,
    and the action pool it burns is priced separately on `gov_action_cost` --
    board-aware, because burning a 7-action Republic turn is not the same
    price as burning a 4-action Despotism turn.  Splitting them rather than
    summing them into one number is what lets the league discover the
    exchange rate instead of being told it.
    """
    card = _DB.get(name)
    sci = card.get("revolutionCost")
    if sci is None:
        sci = card.get("peacefulCost")
    if sci:
        out.append(("science", -float(sci), _COST))
    burned = effects.state_stats(state, p).civil_actions
    if burned:
        out.append(("gov_action_cost", -float(burned), _GAIN))


# ----------------------------------------------------------- wonder riders
#
# The two things a `compute` diff structurally CANNOT see on a wonder,
# because neither is a `Stats` field.


def _on_build_culture(state, p, name):
    """The four Age III wonders: one-time culture scored on completion.

    `effects.compute` builds per-turn ratings.  `onBuildCulture` and
    `onBuildCulturePerTechLevelSum` are paid by `effects.on_wonder_complete`,
    once, at the moment the last stage goes down -- so they are not in
    `Stats` and no swap diff will ever find them.  That is why Hollywood,
    Internet, Fast Food Chains and First Space Flight were still worth
    nothing after the leader work: they are the residue the diff leaves.

    This does not reimplement the formulas.  It calls
    `effects.wonder_completion_culture`, which is the function
    `on_wonder_complete` itself calls to pay them out, so there is exactly
    one implementation and `tests/test_card_pricing.py:TestOneImplementation`
    fails if that stops being true.  (The `onBuildCulture` *value* in the
    card data -- `"2*workers(farm,mine)+..."` -- is an English gloss that
    nothing parses; `_one_time_culture` dispatches on card name.  There was
    never a formula parser to reuse and there is still only one evaluator.)

    Lands on `culture`, the STOCK feature, because it is paid once.  Culture
    is the score, so this is the one card class the evaluator can be exactly
    right about rather than approximately right.

    Two errors of opposite sign are left standing, deliberately, and both are
    named here rather than hidden: the amount is measured on TODAY'S board
    and a wonder takes many turns to build, which UNDERSTATES it; and it is
    credited in full without discounting the chance the wonder is never
    finished, which OVERSTATES it.  `wonder_overrun` already exists to price
    non-completion and this is not a second opinion about that.
    """
    got = effects.wonder_completion_culture(state, p, name)
    return (("culture", float(got), _GAIN),) if got else ()


#: Fraction of player-turns on which a population increase happens at all.
#: MEASURED, not chosen -- `tools/free_pop_rate.py`, 2p self-play under
#: `analysis/frozen/champion_2p.json`, 410 player-turns:
#:
#:     U_paid  0.132   turns the bot pays a civil action + food for one anyway
#:     want    0.654   turns on which a FREE one would improve its evaluation
#:     gain    0.646   mean eval points a free one is worth, measured directly
#:
#: and the refund model this drives -- U_paid x (1 civil action + pop_cost
#: food), priced through the champion's own weights -- comes to 0.51-0.98
#: points per turn across pop_cost 2..5, which BRACKETS the 0.646 measured
#: end-to-end.  That bracket is what makes 0.13 a measurement.
FREE_POP_UTIL = 0.13


def _free_pop_increase(state, p, name):
    """Ocean Liners, whose entire card is `freePopIncreasePerTurn: True`.

    "Once per turn you may increase population without spending a civil
    action or food."  `effects._apply_special` turns it into
    `Stats.free_pop_per_turn = True` -- a BOOLEAN, not a rating -- so the
    swap diff sees a flag flip and has no number to report.  There is no
    number on the card either.  The value has to be constructed.

    The marginal value is exactly the two things the action would otherwise
    have cost: one civil action, and the current food price of a population
    increase.  Both are already features with fitted weights and both are
    already PER-TURN quantities in this evaluator, so pricing it as "+U civil
    actions per turn, +U x pop_cost food per turn" puts it in the same units
    as `culture_rate` with no new weight.  Multiplying only this one card by
    turns-remaining would make it incommensurable with every other rate in
    the vector, all of which are implicitly "per turn for the rest of the
    game"; that is why there is no turns-left factor.

    `U` is the fraction of turns you would have taken the increase anyway.
    On the other turns the effect is a free worker rather than a refund,
    which is a different and generally smaller quantity, so scaling by
    `U_paid` rather than by the 0.654 "would want it" rate is the
    conservative read -- and over-pricing a wonder is the specific bias
    docs/SCORE_VALIDATION.md 6.2 measured as costly.  This under-claims.

    The board-aware part is a hard gate rather than a scaling:
    `economy.pop_cost_base` returns None when the yellow bank is empty, and a
    player with no tokens left cannot increase population at all.  For them
    Ocean Liners is worth exactly nothing, and a static table would still be
    offering them four stages of resources for it.
    """
    food = economy.pop_food_cost(effects.state_stats(state, p), p.yellow_bank)
    if food is None:
        return ()
    return (("civil_actions", FREE_POP_UTIL, _GAIN),
            ("food_rate", FREE_POP_UTIL * food, _GAIN))


def _blue_tokens(state, p, name):
    """Taj Mahal's `blueTokens: 1`, which the swap diff structurally cannot see.

    Blue tokens are not a `Stats` field -- `effects.on_enter_play` adds them
    to `p.blue_total` -- so `compute` never reports them and the diff comes
    back without them.  That matters because `card_potential` prices a swap
    card by the diff ALONE: the static `_card_yields` DOES price
    `blueTokens` (`_EFF_TO_FEATURE` -> `blue_free`), so turning board pricing
    on silently DROPPED it.  A key that one path prices and another does not
    is the whole failure mode this module was built to end
    (docs/SCORE_AUDIT.md 6).
    """
    n = (_DB.get(name).get("effects") or {}).get("blueTokens") or 0
    return (("blue_free", float(n), _GAIN),) if n else ()


#: effects key -> rider, for wonders.  Keyed by KEY rather than by card name
#: (unlike `RIDERS`) so the next card printing the same thing is priced the
#: day it lands.  No subtraction here: wonders accumulate, so there is no
#: incumbent whose rider has to be netted off.
WONDER_RIDERS = {
    "onBuildCulture": _on_build_culture,
    "onBuildCulturePerTechLevelSum": _on_build_culture,
    "freePopIncreasePerTurn": _free_pop_increase,
    "blueTokens": _blue_tokens,
}


def _wonder_rider_delta(state, p, name):
    eff = _DB.get(name).get("effects") or {}
    out = []
    for k in eff:
        fn = WONDER_RIDERS.get(k)
        if fn is not None:
            out.extend(fn(state, p, name))
    return tuple(out)


def _wonder_cost(state, p, name, out):
    """What a wonder costs, which the swap diff does not charge for.

    Exactly what `weighted._card_yields` puts on a wonder, restated here
    because `card_potential` prices a swap card by the diff ALONE (otherwise
    the printed culture would be counted twice).  Drop this and every wonder
    becomes free.
    """
    out.append(("wonders", 1.0, _GAIN))
    stages = _DB.get(name).get("stages") or []
    if stages:
        out.append(("resource_stock", -float(sum(stages)), _COST))


# ------------------------------------------------- unit technologies
#
# A unit technology is NOT a swap card and is not priced by `board_yields`
# below; it gets its own entry point because the question it answers is a
# different one and the answer is not a `Stats` field of a card you play.
#
# WHY IT NEEDS THE BOARD AT ALL.  The static table in `weighted._card_yields`
# prices a unit as "develop it and build ONE FRESH unit": full `techCost` in
# science, full `buildCost` in resources, the printed per-worker `strength`
# back.  That is internally consistent and it is not what the engine offers.
# Every player starts the game with a Warriors worker (`game.START_TECHS`),
# so the move that is actually on the table is `("upgrade", lo, hi)` -- it
# costs the DIFFERENCE of the two build costs (`actions.upgrade_cost`) and it
# pays the DIFFERENCE of the two strengths, on every worker you move.  Pricing
# the fresh build instead over-charges the resources (5 for Riflemen where the
# upgrade from Warriors costs 3) and mis-states the gain, and neither error is
# expressible as a constant: both depend on what you already have developed
# and on how many workers are standing on it.
#
# WHAT IS DERIVED AND WHAT IS CHOSEN.  Everything here is derived:
#
#   * the strength delta is an `effects.compute` diff, so it is exact and it
#     picks up the things a per-card table cannot -- Great Wall's
#     `strengthPerInfantry` when the upgrade changes the unit TYPE, the tactic
#     army re-forming under `army_strength`, the rating clamp at zero;
#   * the resources are `actions.upgrade_cost`, the function the engine
#     charges the player with;
#   * the science is `effects.tech_cost`, likewise.
#
# The one modelling statement is "move ALL of them or none", and it is not a
# guess: the trade is linear in the number of workers moved, so its optimum is
# at an endpoint.  `weighted.unit_tech_value` takes that max; this function
# reports the all-of-them end of it.


_UNIT_CACHE = {}
_UNIT_CACHE_MAX = 200_000


def _unit_workers(p):
    """[(tech name, workers)] for every unit technology carrying a worker."""
    type_of = _DB.type_by_name
    return [(n, t.workers) for n, t in p.techs.items()
            if t.workers and type_of.get(n) in C.UNIT_TYPES]


def _with_unit(state, p, name, workers):
    """`effects.compute` with `name` developed and `workers` workers on it.

    The same `try/finally` discipline as `_swapped`, and the same reason: a
    raise here would leave the player holding a technology they never
    developed.  `p.techs` is REBOUND to a new dict rather than mutated, so a
    journal watching the original mapping never sees a write.
    """
    old = p.techs
    type_of = _DB.type_by_name
    new = {}
    for n, t in old.items():
        if t.workers and type_of.get(n) in C.UNIT_TYPES:
            new[n] = TechCard(n, workers=0, stored=t.stored)
        else:
            new[n] = t
    new[name] = TechCard(name, workers=workers)
    p.techs = new
    try:
        return effects.compute(state, p)
    finally:
        p.techs = old


def unit_upgrade(name, state, idx):
    """(strength gained, science cost, resource cost) for a unit technology.

    "Develop `name`, then move every unit worker I have onto it."  Returns
    `(0.0, 0.0, 0.0)` for a card that is not a unit technology, or one the
    player has already developed (it cannot be developed twice, so the card is
    dead in hand and worth exactly nothing -- not a cost, and not a gain).

    Memoised on `(name, effects.stats_key(state, p))` for the same reason
    `_stats_delta` is, and on the same key: `stats_key` carries the invariant
    that it names every field `compute` reads, and it already includes the
    per-technology worker counts this function moves.
    """
    if _DB.type_by_name.get(name) not in C.UNIT_TYPES:
        return 0.0, 0.0, 0.0
    p = state.players[idx]
    if name in p.techs:
        return 0.0, 0.0, 0.0
    key = (name, effects.stats_key(state, p))
    hit = _UNIT_CACHE.get(key)
    if hit is not None:
        return hit
    sci = effects.tech_cost(state, p, name) or 0
    held = _unit_workers(p)
    workers = sum(n for _, n in held)
    if workers:
        before = effects.state_stats(state, p)
        after = _with_unit(state, p, name, workers)
        gained = float(after.strength - before.strength)
        res = float(sum(k * A.upgrade_cost(state, p, lo, name)
                        for lo, k in held))
    else:
        # Nobody to move.  Developing the technology is legal and buys no
        # strength at all until a unit is built, and building one is its own
        # decision with its own price -- so this is a science cost and
        # nothing else, which is the honest answer rather than a floor.
        gained = res = 0.0
    out = (gained, float(sci), res)
    if len(_UNIT_CACHE) >= _UNIT_CACHE_MAX:
        _UNIT_CACHE.clear()
    _UNIT_CACHE[key] = out
    return out


# --------------------------------------------- EVERY technology, not just red
#
# `unit_upgrade` above answers "what does developing this unit technology buy
# me" for the four red types.  `tech_upgrade` asks the same question of the
# other eleven -- farm, mine, lab, temple, library, theater, arena and the
# special technologies -- and of the red ones too, so there is one answer and
# not two.  docs/YELLOW_TECH_PRICING.md is the measurement; the short version
# is that the static table in `weighted._card_yields` priced every yellow
# production technology strictly NEGATIVE on the live 2p champion (Iron -6.72,
# Alchemy -11.19, Computers -20.41) and `row_pressure` skips anything <= 0, so
# the yellow half of the card row was invisible for exactly the mechanical
# reason the red half was.
#
# THREE THINGS THE STATIC TABLE CANNOT SAY, all of them derivations:
#
#   1. **Developing a technology raises `tech_levels`, and nothing priced it.**
#      `weighted.features` adds a technology's age level into `tech_levels` for
#      every worker type AND for `special-tech`; the live 2p champion weights
#      that feature at 5.84 with an early/late pair of 3.39 / 0.92, i.e. up to
#      **9.23 eval points per level**, which is more than everything else on a
#      yellow card put together.  `_card_yields` maps nothing to it.  Same for
#      `num_techs` and the `best_*` family.  These are the DEVELOP half and
#      they are paid on any technology, staffed or not.
#   2. **The move is an upgrade, not a fresh build.**  Identical to the
#      argument above `unit_upgrade`, with one correction that matters here:
#      `engine/actions.py:_action_moves` only offers `("upgrade", lo, hi)`
#      between cards of the SAME type and strictly increasing level, so the
#      workers eligible to move onto a farm are the workers on your older
#      farms and nothing else.
#   3. **A rate is not worth `w[rate]`.**  `culture_rate`, `science_rate`,
#      `food_rate`, `resource_rate` and `tech_levels` are all in
#      `weighted.PHASE_KEYS`, so `evaluate` prices them at
#      `w[k] + (1-L)*w[k_early] + L*w[k_late]` while `card_potential` looked up
#      the bare `w[k]`.  On the live 2p champion that is 0.25 against 5.29
#      early for `science_rate` -- a factor of twenty-one, and the same shape
#      as the factor of fifteen `strength_marginal` was written for.
#      `weighted.feature_marginal` is the one place that arithmetic lives now.
#
# The one modelling statement is the same one `unit_upgrade` makes, for the
# same reason: the trade is linear in the number of workers moved, so its
# optimum is at an endpoint -- all of them or none.  `weighted.tech_value`
# takes that max; this function reports the all-of-them end.

_TECH_CACHE = {}
_TECH_CACHE_MAX = 200_000

# The types whose development adds to `weighted.features()`' `tech_levels`.
# Read straight off that loop: every worker type, plus `special-tech`.  A
# government also carries a level, but a government is priced by the swap diff
# above and its level delta is a different question (see
# docs/OPEN_ITEMS.md).
LEVELLED_TYPES = frozenset(C.WORKER_TYPES | {"special-tech"})

# type -> the `best_*` feature it feeds.  The four unit types share
# `best_unit`, exactly as `weighted.features` computes it.
_BEST_FEATURE = {t: "best_" + t for t in
                 ("farm", "mine", "lab", "temple", "theater", "library",
                  "arena")}
_BEST_FEATURE.update({t: "best_unit" for t in C.UNIT_TYPES})


def _upgradable_onto(p, name):
    """[(tech, workers)] this player could LEGALLY upgrade onto `name`.

    Same type, strictly lower level, at least one worker standing on it --
    which is `engine/actions.py:_tableau`'s `higher` relation read backwards.
    """
    type_of = _DB.type_by_name
    level_of = _DB.level_by_name
    typ = type_of.get(name)
    lv = level_of.get(name, 0)
    return [(n, t.workers) for n, t in p.techs.items()
            if t.workers and type_of.get(n) == typ and level_of.get(n, 0) < lv]


def _with_tech(state, p, name, moved):
    """`effects.compute` with `name` developed and `moved` workers moved onto
    it, off the technologies they are standing on.

    The same `try/finally` discipline as `_swapped` and `_with_unit`, and the
    same reason: a raise here would leave the player holding a technology they
    never developed.  `p.techs` is REBOUND to a new dict rather than mutated,
    so a journal watching the original mapping never sees a write.
    """
    old = p.techs
    new = dict(old)
    total = 0
    for n, k in moved:
        t = new[n]
        new[n] = TechCard(n, workers=t.workers - k, stored=t.stored)
        total += k
    new[name] = TechCard(name, workers=total)
    p.techs = new
    try:
        return effects.compute(state, p)
    finally:
        p.techs = old


def tech_upgrade(name, state, idx):
    """(staff triples, develop triples, science cost, resource cost).

    "Develop `name`, then move every worker that could legally upgrade onto
    it."  The two triple groups are separated because the two halves of the
    plan are decided separately and the caller has to be able to take the
    argmax over the second one:

    * **develop** -- `tech_levels`, `num_techs`, `best_*` (and `special_techs`
      for a special technology).  Paid the moment the card is developed,
      whether or not anybody staffs it.
    * **staff** -- the `effects.compute` diff of moving the workers, which is
      where the production rate, the strength and the happiness live.
      Optional, and its resource cost is `resource cost`.

    `((), (), 0.0, 0.0)` for a card that is not a technology, or one the player
    has already developed (it cannot be developed twice, so the card is dead
    in hand and worth exactly nothing -- not a cost and not a gain).

    Memoised on `(name, effects.stats_key(state, p))` for the same reason
    `_stats_delta` and `unit_upgrade` are, and on the same key.
    """
    typ = _DB.type_by_name.get(name)
    if typ not in LEVELLED_TYPES:
        return (), (), 0.0, 0.0
    p = state.players[idx]
    if name in p.techs:
        return (), (), 0.0, 0.0
    key = (name, effects.stats_key(state, p))
    hit = _TECH_CACHE.get(key)
    if hit is not None:
        return hit
    level_of = _DB.level_by_name
    type_of = _DB.type_by_name
    lv = level_of.get(name, 0)
    dev = [("tech_levels", float(lv), _GAIN), ("num_techs", 1.0, _GAIN)]
    if typ == "special-tech":
        dev.append(("special_techs", 1.0, _GAIN))
    feat = _BEST_FEATURE.get(typ)
    if feat is not None:
        # `best_unit` is the max over all four red types; every other
        # `best_*` is the max over its own type.  `weighted.features`, exactly.
        fam = C.UNIT_TYPES if typ in C.UNIT_TYPES else (typ,)
        cur = max((level_of.get(n, 0) for n in p.techs
                   if type_of.get(n) in fam), default=0)
        if lv > cur:
            dev.append((feat, float(lv - cur), _GAIN))

    if typ in C.UNIT_TYPES:
        # The red half is `unit_upgrade`, unchanged and deliberately so: it is
        # the shape docs/UNIT_TECH_PRICING.md measured and landed, and
        # re-deriving it here would silently re-open a settled A/B.  The one
        # known defect in it -- it pools workers across all four unit types,
        # where the engine only offers a same-type upgrade -- is recorded in
        # docs/OPEN_ITEMS.md rather than fixed inside this commit, so this
        # lane's digest attribution stays a single constant.
        gained, sci, res = unit_upgrade(name, state, idx)
        staff = (("strength", gained, _GAIN),) if gained else ()
    else:
        sci = float(effects.tech_cost(state, p, name) or 0)
        held = _upgradable_onto(p, name)
        if held:
            before = effects.state_stats(state, p)
            after = _with_tech(state, p, name, held)
            staff = _delta_triples(before, after, p)
            res = float(sum(k * A.upgrade_cost(state, p, lo, name)
                            for lo, k in held))
        else:
            # Nobody to move.  Developing the technology is legal and buys no
            # production at all until a building is built on it, and building
            # one is its own decision with its own price -- so the staffing
            # half is empty, which is the honest answer rather than a floor.
            # `unit_upgrade` takes the identical position.
            staff, res = (), 0.0
    out = (staff, tuple(dev), float(sci), float(res))
    if len(_TECH_CACHE) >= _TECH_CACHE_MAX:
        _TECH_CACHE.clear()
    _TECH_CACHE[key] = out
    return out


# ------------------------------------------------------------- entry point


def board_yields(name, state, idx):
    """(feature, amount, kind) triples for `name` on this board, or None.

    None means "this card is not board-priced, use the static table".
    """
    card = _DB.by_name.get(name)
    if card is None:
        return None
    typ = card["type"]
    if typ not in SWAP_TYPES:
        return None
    p = state.players[idx]
    field = {"leader": "leader", "government": "government",
             "wonder": "completed_wonders"}[typ]
    out = list(_stats_delta(state, p, field, name))
    if typ == "wonder":
        _wonder_cost(state, p, name, out)
        out.extend(_wonder_rider_delta(state, p, name))
    elif typ == "leader":
        if not p.leader:
            # the generic "it is a leader" term, which `_card_yields` gets
            # from `features()`.  Not a gain when you already have one: a
            # leader replaces a leader.
            out.append(("leader", 1.0, _GAIN))
        out.extend(_rider_delta(state, p, name))
    else:
        _government_cost(state, p, name, out)
    return _merge(out)


def _merge(triples):
    """One triple per (feature, kind), summed.

    `card_potential` sums whatever it is given, so merging changes no value.
    It matters because a caller that builds a dict off this -- a test, a
    census, anything of Lane A's -- would otherwise silently keep only the
    last of two `culture_rate` entries.  The Gandhi-over-Churchill case
    produces exactly that: +2 from the Stats diff and -3 from the rider.
    """
    merged = {}
    order = []
    for feat, amt, kind in triples:
        k = (feat, kind)
        if k not in merged:
            order.append(k)
        merged[k] = merged.get(k, 0.0) + amt
    return tuple((f, merged[(f, kd)], kd) for f, kd in order
                 if merged[(f, kd)])


# ------------------------------------- board-scaled action cards (additive)
#
# Three action cards print a coefficient per player count and multiply it by
# a count of rivals.  They are not swap cards -- nothing is replaced -- so
# these triples are ADDED to the static ones rather than replacing them.


def _per_player_count(val, state):
    """`{"2p": 6, "3p": 3, "4p": 2}` -> the number for this table size."""
    if not isinstance(val, dict):
        return 0.0
    return float(val.get(f"{len(state.players)}p") or 0)


_EXTRA_KEYS = ("culturePerCivilizationWithMoreCulture",
               "resourcesForMilitaryUnitsPerStrongerCivilization")

#: the three cards `board_extra` has anything to say about, resolved once.
#: `board_extra` is called for every non-swap card in the hand and the row on
#: every leaf, so the common answer -- "nothing" -- has to be one set probe.
_EXTRA_CARDS = frozenset(
    n for n, c in _DB.by_name.items()
    if any(k in (c.get("effects") or {}) for k in _EXTRA_KEYS))


def board_extra(name, state, idx):
    """Board-scaled triples to ADD to `weighted._card_yields(name)`."""
    if name not in _EXTRA_CARDS:
        return ()
    card = _DB.by_name[name]
    eff = card["effects"]
    p = state.players[idx]
    out = []
    # Endowment for the Arts: "gain 2 culture for each civilization with more
    # culture points than you (3 at 3p, 6 at 2p)".  A one-shot culture stock
    # gain, so it lands on `culture`, not `culture_rate`.  At 2p the count is
    # 0 or 1 and the card is worth 6 or nothing -- the single widest swing of
    # any action card, and currently priced at zero either way.
    per = eff.get("culturePerCivilizationWithMoreCulture")
    if per is not None:
        n = sum(1 for q in _live_rivals(state, p) if q.culture > p.culture)
        if n:
            out.append(("culture", _per_player_count(per, state) * n, _GAIN))
    # Wave of Nationalism / Military Build-Up: resources off military units
    # for each civilization stronger than you.  Ring-fenced to military units,
    # hence `restricted_resources` rather than `resource_stock`: the bot that
    # is not buying units gets nothing, and only the league knows how often
    # that is.
    per = eff.get("resourcesForMilitaryUnitsPerStrongerCivilization")
    if per is not None:
        mine = effects.state_stats(state, p).strength
        n = sum(1 for q in _live_rivals(state, p)
                if effects.state_stats(state, q).strength > mine)
        if n:
            out.append(("restricted_resources",
                        _per_player_count(per, state) * n, _GAIN))
    return tuple(out)


def board_choices(name, state, idx):
    """Mutually exclusive alternatives, as a tuple of triple-groups.

    Nothing board-priced needs one yet; the hook exists because
    `weighted.card_potential` resolves static choices (Reserves' "2 food OR 2
    resources") the same way and the two must not diverge.
    """
    return ()


# ------------------------------------------------------ the guardrail side
#
# Keys `tests/test_card_pricing.py` should count as PRICED even though they
# are absent from the static tables, because `board_yields` prices them
# through the engine.  Every one of these is carried only by leaders (see
# `tools/card_blindness.py --cards leader`), so it is priced whenever
# `card_board_credit` is non-zero and unpriced otherwise -- which is a
# statement about a weight, not about a blind spot.
BOARD_PRICED = {
    # --- engine/effects.py:_apply_modifier, phase 3.  Priced exactly, by
    # the engine's own code, via the leader swap diff.
    "strengthPerMilitaryUnit": "leader swap diff: effects._apply_modifier",
    "strengthPerUnitType": "leader swap diff: effects._apply_modifier",
    "strengthPerTempleOrGovernmentHappy":
        "leader swap diff: effects._apply_modifier",
    "sciencePerBestLabOrLibraryLevel":
        "leader swap diff: effects._apply_modifier",
    "sciencePerLab": "leader swap diff: effects._apply_modifier",
    "culturePerTheater": "leader swap diff: effects._apply_modifier",
    "culturePerLabEqualToLevel": "leader swap diff: effects._apply_modifier",
    "culturePerLibraryTheaterPair":
        "leader swap diff: effects._apply_modifier",
    "culturePerHappyFromTemplesTheatersWonders":
        "leader swap diff: effects._apply_modifier",
    "bestTheaterDoubleCulture": "leader swap diff: effects._apply_modifier",
    "resourcesPerLabEqualToLevel":
        "leader swap diff: effects._apply_modifier",
    "cultureFirstColony": "leader swap diff: effects._apply_modifier",
    "culturePerAdditionalColony": "leader swap diff: effects._apply_modifier",
    # --- engine/effects.py:_apply_special
    "popIncreaseFoodDiscount":
        "leader swap diff: effects._apply_special -> Stats.pop_food_discount",
    "cannotPlayAggressionOrWar":
        "leader swap diff: effects._apply_special -> Stats.no_aggression",
    # --- wonders, priced by the wonder swap diff (Lane A).  Every one of
    # these is `effects._apply_modifier` arithmetic the engine already does.
    "strengthPerInfantry": "wonder swap diff: effects._apply_modifier",
    "strengthPerArtillery": "wonder swap diff: effects._apply_modifier",
    "extraHappyPerHappySource": "wonder swap diff: effects._apply_modifier",
    "doubleBestMine": "wonder swap diff: effects._apply_modifier",
    # --- wonder riders: not Stats fields, so no diff can see them
    "onBuildCulture":
        "wonder rider: effects.wonder_completion_culture, the scorer itself",
    "onBuildCulturePerTechLevelSum":
        "wonder rider: effects.wonder_completion_culture, the scorer itself",
    "freePopIncreasePerTurn":
        "wonder rider: a free pop increase per turn, priced as the civil "
        "action and food it refunds at the measured rate (FREE_POP_UTIL)",
    "blueTokens":
        "wonder rider: not a Stats field, so the swap diff cannot see it and "
        "the static table's `blue_free` would be dropped (SCORE_AUDIT 6)",
    # --- riders, priced above rather than by the engine
    "cultureIfTopTwoStrength":
        "board rider: _genghis, exact from rival strengths",
    "perTurnChoice":
        "board rider: _churchill, the unconditional 3-culture option",
    # --- board-scaled action cards, priced additively by `board_extra`
    "culturePerCivilizationWithMoreCulture":
        "board_extra: rivals with more culture x the per-table coefficient",
    "resourcesForMilitaryUnitsPerStrongerCivilization":
        "board_extra: stronger rivals x the per-table coefficient",
}
