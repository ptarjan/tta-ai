"""Card counting: what a human at the table could work out about the decks.

The project owner's rule for this whole file is one sentence -- *"Card counting
is legal.  All public info can be used"* -- and the test for any number here is
whether somebody sitting at the table with the physical 2015 base game could
arrive at it from what they have seen.  Three things pass that test and are
used below; a fourth does not and is refused.

**Deck COMPOSITION is public.**  The card counts per player number are printed
in the rulebook.  `cards.Db.civil_deck(age, n)` and `.military_deck(age, n)`
return exactly that list, and reading them is reading the rules.

**Deck HEIGHT is public.**  `len(state.civil_deck)` is how many cards are left
in the stack on the table.  Anyone can count a stack.

**Everything already seen is public.**  The card row is face up, tableaux are
face up, the civil discard is a face-up pile beside the board
(`state.civil_discard`, added for exactly this), and my own hand is mine.

**Deck ORDER and deck CONTENTS are not.**  Nothing in this module iterates
`state.civil_deck` or `state.military_deck`; it only ever takes their `len()`.
That is the line between counting and cheating, and it is a one-line check any
reviewer can run: `grep -nE "civil_deck|military_deck" counting.py` must show
only `db.` composition calls and `len(state...)`.

WHY EVERY DEALT AGE IS EXACTLY ACCOUNTED FOR.  `game._deal` advances the age
the moment the last card of the current deck is dealt (`if not
state.civil_deck: _advance_age(...)`, RULES_SPEC 2.2), so an age's civil deck is
*always* fully exhausted into the row before the next age begins.  No card is
ever removed from a civil deck unseen.  That is what makes the subtraction
below exact rather than an estimate, and it is a property of the engine's turn
loop, so `tests/test_counting.py` pins it rather than trusting this paragraph.

CALLERS MUST ROOT-CACHE.  Every function here reads a zone that a trial `apply`
mutates -- dealing refills the row from the deck, revealing an event moves a
face-down card into `past_events`.  Recomputing on a searched state would let a
deeper search *see the card it just drew*, which is the precise failure
`weighted.root_row_budget` exists to prevent.  So `weighted.rival_context`
computes these once at the search root and threads them down, and the plan bots
pass `root_row`/`root_counts` the same way.
"""

from collections import Counter

from .. import cards as C


def live_ages(state):
    """The ages whose cards can still be in somebody's hand or in a deck.

    `game._antiquate` discards every card of level BELOW the level of the age
    that just ended, from hands and from the common area alike (RULES_SPEC
    12.2).  So when age `a` is current, the cards still in play are those of
    level `>= level(a) - 1`: the current age, and the one before it that
    players are allowed to keep holding.

    Returned oldest first.  Age "IV" has no decks at all -- `_advance_age`
    empties both -- so it contributes nothing and is not special-cased here;
    the composition lookups simply come back empty.
    """
    lv = C.level(state.age_civil)
    return tuple(a for a in C.AGES if lv - 1 <= C.level(a) <= lv)


def _seen_civil(state, idx, ages):
    """Face-up civil cards of `ages`, plus my own hand: name -> count.

    Every zone in here is one a human can point at.  Rival hands are NOT in
    here, deliberately -- their SIZE is public and their contents are not, and
    the whole point of `civil_outlook` is to reason about that gap rather than
    to look through it.
    """
    db = C.db()
    keep = set(ages)
    seen = Counter()
    for name in state.card_row:
        if name is not None and db.age_of(name) in keep:
            seen[name] += 1
    for age in keep:
        # Both discard records: swept off the row, and left play from a hand or
        # a board.  They are separate fields for the encoder's sake (see
        # GameState.civil_removed) and the count wants the union of them.
        for name in state.civil_discard.get(age) or ():
            seen[name] += 1
        for name in state.civil_removed.get(age) or ():
            seen[name] += 1
    for q in state.players:
        # Every civil card a player has put on the table, in every zone it can
        # sit in.  Missing ONE of these zones does not fail loudly -- it just
        # makes the subtraction read the shortfall as "still in a rival's
        # hand", which is exactly how the leader and wonder zones were found.
        board = list(q.techs)                      # tableau: face up
        board.append(q.government)                 # its own field, not a tech
        board.extend(q.completed_wonders)          # built: face up
        if q.wonder is not None:
            board.append(q.wonder.name)            # under construction: face up
        if q.leader is not None:
            board.append(q.leader)
        if q.homer_wonder is not None:
            board.append("Homer")                  # tucked under a wonder
        for name in board:
            if name in db.by_name and db.age_of(name) in keep:
                seen[name] += 1
        if q.idx == idx:                           # my hand: mine to read
            for name in q.hand_civil:
                if db.age_of(name) in keep:
                    seen[name] += 1
    return seen


def civil_outlook(state, idx):
    """name -> EXPECTED further copies that will be dealt into the row.

    This is the number the project owner asked for in the audit, in his own
    example: *"know that a second selective breeding is near the end or not
    and it has to build another farm since it won't see it"*.  A card with an
    outlook of 0.0 is one that will never appear again -- if I want it, the
    copy in the row is the last one, and passing it up is permanent.

    Three cases, and only one of them is an estimate:

    * **A future age** -- the whole deck is still to come and, by the
      exhaustion property above, every card of it *will* be dealt.  Outlook is
      the printed count, exactly.  This is certainty, not a guess: before Age
      III is opened you already know precisely which Age III cards exist.
    * **An age already past** -- its deck was fully dealt, so nothing more is
      coming.  0.0, exactly.
    * **The current age** -- `unaccounted` copies are those the printed count
      says exist and that I have not seen in the row, a tableau, the discard
      or my own hand.  Each is either still in the deck or in a rival's hand,
      and I can count the deck stack, so

          outlook(X) = unaccounted(X) * len(deck) / total unaccounted

      with no fitted constant anywhere in it.  The multiplier is the fraction
      of the unknown cards that are in the deck rather than in a hand, and
      both ends of that ratio are things a human can count.

    Uniformity across names is the one modelling assumption, and it is the
    assumption-light one: a rival holding a card is *more* likely to be
    holding a good one, so this slightly over-estimates the outlook of the
    strongest cards.  Tightening that needs a model of what the rival wants,
    which is the same missing piece `weighted.rival_take_p` names, and it is
    tracked as one item rather than two.
    """
    db = C.db()
    n = _live_count(state)
    cur = state.age_civil
    ages = [a for a in C.AGES if C.level(a) >= C.level(cur)]
    out = {}
    for age in ages:
        comp = Counter(db.civil_deck(age, n))
        if age != cur:
            out.update(comp)                       # whole deck still to come
            continue
        seen = _seen_civil(state, idx, (age,))
        unaccounted = {name: comp[name] - seen.get(name, 0) for name in comp}
        total = sum(v for v in unaccounted.values() if v > 0)
        if total <= 0:
            out.update(dict.fromkeys(comp, 0.0))
            continue
        share = len(state.civil_deck) / float(total)
        for name, left in unaccounted.items():
            out[name] = max(0.0, left) * share
    return out


def _live_count(state):
    """Players who have not resigned.  RULES_SPEC 13 trims the future decks to
    this, so it is the right `n` for a composition lookup."""
    return sum(1 for q in state.players if not q.resigned)


def event_pool(state, idx):
    """What could be in the face-down politics pile that I did not put there.

    Returns ``(unknown, p_in_pile)``: a `Counter` of event name -> copies whose
    location I do not know, and the probability that any one such copy is
    sitting in one of the pile slots I did not seed.

    THIS IS THE FUNCTION THAT REPLACES A LEAK.  `events.pending_final_events`
    walks the pile by NAME, and `weighted.event_scoring_margin` called it, so
    every trained champion has been reading Age III scoring events that its
    opponents placed face down -- which is not card counting, it is looking at
    the back of a card.  What a human really knows is:

    * exactly which events *they* prepared, because they put them there
      (`state.seeded_by`, and `weighted.my_seeds` already reads it);
    * how many face-down cards are in the pile, because it is a stack;
    * which events have already been revealed (`past_events`) and which were
      discarded (`discarded_military`), because both happened in the open;
    * the printed composition of each age's event deck.

    From those, a copy whose location is unknown is in the draw deck, in an
    opponent's hand, or in the pile, and the pile holds `k` of them out of
    `total` unknown copies, so `p_in_pile = k / total`.  Uniform over names
    because the alternative -- an opponent seeds the event that suits *them* --
    needs the same desire model `civil_outlook` wants, and guessing at it with
    a fitted number would be worse than the honest uniform.

    Note `p_in_pile` can only ever be a probability over cards *I have not
    seen*.  My own seeds are returned by `weighted.my_seeds` and are certain;
    the caller adds the two.
    """
    db = C.db()
    n = _live_count(state)
    ages = live_ages(state)
    known = Counter()
    for name in state.past_events:                 # revealed in the open
        known[name] += 1
    for age in ages:
        for name in state.discarded_military.get(age) or ():
            known[name] += 1
    me = state.players[idx]
    for name in me.hand_military:                  # my hand is mine to read
        known[name] += 1
    mine_in_pile = 0
    for name in list(state.current_events) + list(state.future_events):
        if state.seeded_by.get(name) == idx:
            known[name] += 1                       # I put it there
            mine_in_pile += 1

    unknown = Counter()
    for age in ages:
        for name, cnt in Counter(db.military_deck(age, n)).items():
            if db.type_of(name) != "event":
                continue
            left = cnt - known.get(name, 0)
            if left > 0:
                unknown[name] = left
    total = sum(unknown.values())
    k = len(state.current_events) + len(state.future_events) - mine_in_pile
    if total <= 0 or k <= 0:
        return unknown, 0.0
    return unknown, min(1.0, k / float(total))
