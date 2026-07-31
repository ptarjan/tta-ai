"""Age IV is a real phase with no cards, and three places must agree on that.

`engine/cards.py` declares five ages; the card corpus supplies four of them.
That disagreement is legitimate -- the base game genuinely deals no cards in
Age IV, which is simply the age the game ends in -- but nothing failed when the
two lists diverged, so "Age IV" could quietly mean a phase in one place and a
(nonexistent) card cohort in another.

READ THIS BEFORE CITING THESE TESTS AGAINST OPEN_ITEMS 2.17.  On 2026-07-31
that item -- "zero Age IV card takes, 260/260 seat-games" -- was briefly closed
as a false defect on the strength of these assertions.  That was wrong.  The
census counts takes made *during the Age IV phase*, and `_advance_age` empties
the *deck*, not the *row*: leftover Age III cards stay on the row and taking
one is legal.  Humans take 1.6-1.8 of them and the bot takes zero, so the item
is open.  An empty deck does not imply an empty denominator.

What these tests do pin, and all they pin:

  * Age IV is declared as an age and supplies no cards, from both sides; and
  * adding an Age IV card without teaching `_advance_age` to deal it fails
    here rather than silently producing a deck the engine throws away.
"""

import random

import engine.cards as C
import engine.game as G


def test_age_iv_is_declared_but_carries_no_cards():
    """AGES lists IV; the corpus supplies none.  Both halves are asserted."""
    assert C.AGES == ["A", "I", "II", "III", "IV"]

    db = C.db()
    by_age = {a: [c for c in db.cards if c["age"] == a] for a in C.AGES}

    for age in ("A", "I", "II", "III"):
        assert by_age[age], f"age {age} must carry cards"

    assert by_age["IV"] == [], (
        "Age IV must carry no cards.  If this fails you have added one -- "
        "then engine/game.py:_advance_age must stop blanking the Age IV "
        "decks, and OPEN_ITEMS 2.17 must be reopened."
    )


def test_age_iv_decks_are_empty_at_every_player_count():
    """The deck builders agree with the corpus, not just the raw card list."""
    db = C.db()
    for n in (2, 3, 4):
        assert db.civil_deck("IV", n) == []
        assert db.military_deck("IV", n) == []


def test_entering_age_iv_empties_both_decks():
    """_advance_age blanks the decks, so a zero take rate is forced by rules."""
    state = G.new_game(2, seed=0)
    rng = random.Random(0)
    assert state.age_civil == "A"
    for _ in range(4):
        G._advance_age(state, rng)
    assert state.age_civil == "IV", "four advances from A must land on IV"
    assert state.civil_deck == []
    assert state.military_deck == []
