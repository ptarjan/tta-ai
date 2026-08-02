"""Card counting must count only what a human at the table could count.

`engine/bots/counting.py` claims three things in its docstring, and a docstring
is not a guarantee.  These tests turn each claim into a failure:

* the counting is EXACT, because an age's civil deck is always dealt out in
  full before the age turns over -- so the subtraction has no leak in it;
* it is HONEST, because nothing in it reads a hidden zone -- swapping a card
  inside the deck, or inside a rival's hand, must not move any number;
* it is USEFUL, because the numbers actually separate a last copy from a card
  with more behind it, on real positions rather than constructed ones.

The third is the one that matters most in this repo's experience.  "A rate of
zero has two causes" and an instrument that returns the same number everywhere
is indistinguishable from one that is switched off, so the useful-ness tests
assert SPREAD, not just absence of crashes.
"""
import unittest
from collections import Counter

from engine import cards as C
from engine.bots import counting, weighted as W

import corpus


def _positions(players=3, seed=7, every=40, limit=2000):
    """Real self-play positions, sampled every `every` plies.

    Cached per (players, seed) in tests/corpus.py and handed out as copies --
    these tests deliberately mutate what they are given (reversing the deck,
    swapping a hand), so the copy is what keeps them independent."""
    return corpus.positions(players, seed, every, limit)


class TheDeckIsFullyDealt(unittest.TestCase):
    """The property the whole subtraction rests on.

    `counting.civil_outlook` says a past age's outlook is exactly 0 and a
    future age's is exactly its printed count.  Both statements are false the
    moment a card can leave a deck without being dealt, so this checks the
    engine rather than the counter.
    """

    def test_every_card_of_a_finished_age_is_somewhere_visible(self):
        db = C.db()
        checked = 0
        for st in _positions(3, seed=5):
            n = sum(1 for q in st.players if not q.resigned)
            for age in C.AGES:
                if C.level(age) >= C.level(st.age_civil):
                    continue                  # not finished yet
                if age == "A":
                    continue                  # dealt at setup, not from a deck
                printed = Counter(db.civil_deck(age, n))
                if not printed:
                    continue
                seen = counting._seen_civil(st, 0, (age,))
                # Whatever is not visible is in a rival's hand -- it cannot be
                # in a deck, because the deck is gone.  So the shortfall can
                # never exceed the cards the rivals are actually holding.
                short = sum(max(0, printed[k] - seen.get(k, 0))
                            for k in printed)
                held = sum(q.hand_size("civil") for q in st.players
                           if q.idx != 0)
                self.assertLessEqual(
                    short, held,
                    f"age {age}: {short} cards of a FINISHED age are "
                    f"unaccounted for but the rivals hold only {held} cards. "
                    "A civil deck lost cards without dealing them, which "
                    "breaks every count in engine/bots/counting.py")
                checked += 1
        self.assertGreater(checked, 0, "no position had a finished age")


class ItReadsNoHiddenZone(unittest.TestCase):
    """The honesty tests.  Change a hidden zone; nothing may move."""

    def test_reordering_the_draw_deck_changes_nothing(self):
        moved = 0
        for st in _positions(3, seed=9):
            if len(st.civil_deck) < 2:
                continue
            before = counting.civil_outlook(st, 0)
            st.civil_deck.reverse()
            if counting.civil_outlook(st, 0) != before:
                moved += 1
        self.assertEqual(moved, 0, "the outlook moved when the DECK ORDER "
                                   "changed -- something is reading the deck")

    def test_replacing_a_rival_hand_changes_nothing(self):
        """A rival's hand SIZE is public and its contents are not.

        Cards are swapped for others of the same age so the size, and every
        other public quantity, is untouched.
        """
        db = C.db()
        moved = checked = 0
        for st in _positions(3, seed=13):
            q = next((x for x in st.players
                      if x.idx != 0 and x.hand_civil), None)
            if q is None:
                continue
            before = counting.civil_outlook(st, 0)
            age = db.age_of(q.hand_civil[0])
            alt = [n for n in db.by_name
                   if db.age_of(n) == age and db.type_of(n) != "wonder"
                   and n not in q.hand_civil]
            if not alt:
                continue
            q.hand_civil[0] = alt[0]
            checked += 1
            if counting.civil_outlook(st, 0) != before:
                moved += 1
        self.assertGreater(checked, 0, "no rival ever held a civil card")
        self.assertEqual(moved, 0, f"{moved}/{checked} positions changed the "
                         "outlook when a rival's HIDDEN hand was swapped")

    def test_my_own_hand_is_visible_to_me(self):
        """The other side of it: my hand is mine, so it must count.

        Without this, a mask that ignored every hand would pass the test above
        and quietly throw away information the bot is entitled to.
        """
        db = C.db()
        moved = checked = 0
        for st in _positions(3, seed=13):
            me = st.players[0]
            if not me.hand_civil:
                continue
            before = counting.civil_outlook(st, 0)
            held = me.hand_civil[0]
            age = db.age_of(held)
            alt = [n for n in db.civil_deck(age, 3)
                   if n != held and n not in me.hand_civil]
            if not alt:
                continue
            me.hand_civil[0] = alt[0]
            checked += 1
            if counting.civil_outlook(st, 0) != before:
                moved += 1
        self.assertGreater(checked, 0, "I never held a civil card")
        self.assertGreater(moved, 0, "swapping a card in MY OWN hand never "
                           "changed the outlook -- the counter is ignoring "
                           "information I am entitled to use")


class ItSaysSomethingUseful(unittest.TestCase):
    """An instrument that returns one number everywhere is switched off."""

    def test_the_outlook_separates_last_copies_from_plentiful_ones(self):
        spread = zeros = 0
        for st in _positions(3, seed=21):
            o = counting.civil_outlook(st, 0)
            cur = [v for k, v in o.items()
                   if C.db().age_of(k) == st.age_civil]
            if not cur:
                continue
            if max(cur) - min(cur) > 1e-9:
                spread += 1
            zeros += sum(1 for v in cur if v == 0.0)
        self.assertGreater(spread, 0, "the outlook was flat in every sampled "
                                      "position -- it is not counting")
        self.assertGreater(zeros, 0, "no card was ever counted out entirely, "
                           "so the 'last copy' signal never fires")

    def test_row_last_copy_fires_and_is_free_when_unweighted(self):
        fired = 0
        for st in _positions(3, seed=21):
            ctx = W.rival_context(st, 0)
            if W.row_last_copy(st, 0, W.DEFAULT_WEIGHTS, ctx) > 0.0:
                fired += 1
        self.assertGreater(fired, 0, "row_last_copy was 0.0 in every position")

    def test_the_default_weight_costs_nothing(self):
        """Zero default must mean zero COST, the invariant every eval-only
        term in this file is held to (see tests/test_row_features.py)."""
        calls = []
        real = W.row_last_copy
        W.row_last_copy = lambda *a, **k: (calls.append(1), real(*a, **k))[1]
        try:
            for st in _positions(3, seed=3, every=200, limit=400):
                W.evaluate(st, 0, W.DEFAULT_WEIGHTS, W.rival_context(st, 0))
        finally:
            W.row_last_copy = real
        self.assertEqual(calls, [], "row_last_copy ran at its 0.0 default")


class TheEventPoolIsCounted(unittest.TestCase):

    def test_my_seeds_are_never_in_the_unknown_pool(self):
        for st in _positions(3, seed=17):
            unknown, _ = counting.event_pool(st, 0)
            for name in W.my_seeds(st, 0):
                self.assertEqual(
                    unknown.get(name, 0), 0,
                    f"{name} is an event I prepared MYSELF and it is also in "
                    "the unknown pool, so it is being counted twice")

    def test_a_revealed_event_leaves_the_pool(self):
        checked = 0
        for st in _positions(3, seed=17):
            if not st.past_events:
                continue
            unknown, _ = counting.event_pool(st, 0)
            for name in st.past_events:
                self.assertEqual(unknown.get(name, 0), 0,
                                 f"{name} was revealed in the open and is "
                                 "still counted as unknown")
            checked += 1
        self.assertGreater(checked, 0, "no event was ever revealed")

    def test_the_probability_is_a_probability(self):
        for st in _positions(3, seed=17):
            _unknown, p = counting.event_pool(st, 0)
            self.assertGreaterEqual(p, 0.0)
            self.assertLessEqual(p, 1.0)


if __name__ == "__main__":
    unittest.main()
