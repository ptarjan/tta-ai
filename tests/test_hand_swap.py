"""The hand term and the single-slot card classes: leaders and governments.

`card_potential` prices a leader or a government as a DIFF -- what swapping
it in for the one you already have would change (engine/bots/board_yields.py,
"Replacement").  `hand_potential` used to sum that over the whole civil hand,
which prices three leaders in hand as three replacements of the current
leader.  Only one of them can ever be that replacement.

The three classes of fact this file defends, and they fail differently:

* `TestTheDoubleCount` -- the defect itself, stated as the arithmetic that
  used to hold: a hand of N leaders was worth N times one leader.
* `TestTheSlotCollapse` -- the fix.  Each single-slot class collapses to its
  best member independently, and `hand_swap_extra` at 1.0 recovers the old
  summing exactly, which is what makes the two duellable in one process.
* `TestStillInertAtTheShippedDefaults` -- with `card_board_credit` at 0.0
  (what is shipped) nothing here may change any number at all.
"""
import random
import unittest

from engine import actions as A, cards as C, game as G
from engine.bots import WeightedBot, board_yields as BY, weighted as W

# Three leaders and two governments that are all in the deck, priced by the
# swap diff.  Names, not indices, because `hand_civil` holds names.
LEADERS = ("Michelangelo", "Julius Caesar", "Homer")
GOVERNMENTS = ("Republic", "Monarchy")


def _played(players=2, seed=7, plies=60):
    """A mid-game state, reached by actually playing (as test_board_yields)."""
    st = G.new_game(players, seed)
    rng = random.Random(seed)
    bots = [WeightedBot(seed=seed + i) for i in range(players)]
    for _ in range(plies):
        if st.game_over:
            break
        A.apply(st, bots[st.decider()].pick(st, A.legal_moves(st)), rng)
    return st


def _w(**over):
    w = dict(W.DEFAULT_WEIGHTS)
    w["card_board_credit"] = 1.0
    w.update(over)
    return w


def _hand(st, idx, names):
    st.players[idx].hand_civil = list(names)


def _each(st, idx, w, names):
    return [W.card_potential(n, w, st, idx) for n in names]


def _with_the_best_leader_already_on_the_board(st, w):
    """Put the STRONGEST of `LEADERS` on the board, so the rest are worse.

    `max(vals) > sum(vals)` needs at least two leaders in hand that are worse
    than the incumbent, and that is a fact about the position, not about the
    pricing.  Searching a self-play game for such a position was tried and is
    the wrong instrument: it depends on the policy (it broke when the rate
    horizon landed, docs/RATE_HORIZON.md) and on nothing else in the suite
    having warmed a cache, so it passed alone and failed in the full run.
    Constructing the precondition is deterministic and states the claim
    exactly.
    """
    vals = _each(st, 0, w, LEADERS)
    st.players[0].leader = LEADERS[vals.index(max(vals))]
    return st


class TestTheDoubleCount(unittest.TestCase):
    """The bug, pinned as arithmetic rather than as a story.

    Every one of these values is a diff against the SAME current leader, so
    summing them asserts that the bot gets to make that replacement three
    times over.  `hand_swap_extra = 1.0` is exactly the old behaviour, so
    these assertions also document what was fixed and keep the corner alive
    as a control arm for the A/B.
    """

    def test_n_leaders_were_priced_as_n_replacements(self):
        st = _played()
        w = _w(hand_swap_extra=1.0)       # the pre-fix pricing
        _hand(st, 0, LEADERS)
        vals = _each(st, 0, w, LEADERS)
        self.assertEqual(len(set(round(v, 9) for v in vals)), len(LEADERS),
                         "pick leaders that price differently or this test "
                         "cannot tell a sum from a max")
        self.assertAlmostEqual(W.hand_potential(st, 0, w), sum(vals), places=9)
        # ...and that really is N times over, not a coincidence of one card:
        # the same leader three times is worth three of it.
        _hand(st, 0, (LEADERS[0],) * 3)
        one = W.card_potential(LEADERS[0], w, st, 0)
        self.assertAlmostEqual(W.hand_potential(st, 0, w), 3.0 * one,
                               places=9)
        self.assertNotAlmostEqual(one, 0.0, places=6)

    def test_the_same_was_true_of_governments(self):
        st = _played()
        w = _w(hand_swap_extra=1.0, urban_limit=0.5, gov_action_cost=0.25)
        _hand(st, 0, GOVERNMENTS)
        vals = _each(st, 0, w, GOVERNMENTS)
        self.assertAlmostEqual(W.hand_potential(st, 0, w), sum(vals), places=9)


class TestTheSlotCollapse(unittest.TestCase):
    """The fix: one leader slot, one government slot, priced once each."""

    def test_a_hand_of_leaders_is_worth_the_best_one(self):
        w = _w()                            # hand_swap_extra defaults to 0.0
        # The collapse (`hand_potential == max`, not `sum`) is true on every
        # board.  The DIRECTION of `max > sum` is not: it needs at least one
        # leader in hand that is WORSE than the one already on the board, which
        # is a property of the position and stopped holding at ply 60 when the
        # rate horizon changed what the bot builds (docs/RATE_HORIZON.md).  So
        # the position is sought rather than assumed, and the assertion is
        # unchanged.
        st = _with_the_best_leader_already_on_the_board(_played(), w)
        _hand(st, 0, LEADERS)
        vals = _each(st, 0, w, LEADERS)
        self.assertAlmostEqual(W.hand_potential(st, 0, w), max(vals),
                               places=9)
        # the two answers must actually differ, or this asserts nothing: at
        # least one of the leaders is worse than the one on the board, so the
        # old pricing charged the bot for replacing it with that one as well.
        self.assertNotAlmostEqual(max(vals), sum(vals), places=6)
        self.assertGreater(max(vals), sum(vals))

    def test_one_leader_is_unchanged(self):
        """The collapse may not move the N = 1 case: that is the case the
        A/B measured and the case the pricing was written for."""
        st = _played()
        w = _w()
        for name in LEADERS:
            _hand(st, 0, (name,))
            self.assertAlmostEqual(W.hand_potential(st, 0, w),
                                   W.card_potential(name, w, st, 0),
                                   places=9)

    def test_the_two_slots_collapse_independently(self):
        st = _played()
        w = _w(urban_limit=0.5, gov_action_cost=0.25)
        names = LEADERS + GOVERNMENTS
        _hand(st, 0, names)
        lead = max(_each(st, 0, w, LEADERS))
        gov = max(_each(st, 0, w, GOVERNMENTS))
        self.assertAlmostEqual(W.hand_potential(st, 0, w), lead + gov,
                               places=9)

    def test_ordinary_cards_still_sum(self):
        """A hand of ordinary cards is unaffected: they are not single-slot,
        you play all of them."""
        st = _played()
        w = _w()
        db = C.db()
        plain = [n for n, c in db.by_name.items()
                 if c["type"] not in ("leader", "government")
                 and W.card_potential(n, w, st, 0) > 0.1][:4]
        self.assertGreaterEqual(len(plain), 3)
        _hand(st, 0, plain)
        self.assertAlmostEqual(W.hand_potential(st, 0, w),
                               sum(_each(st, 0, w, plain)), places=9)

    def test_a_spare_leader_is_credited_by_its_own_weight(self):
        """`hand_swap_extra` is the free parameter: 0.0 says a second leader
        is worth nothing extra, 1.0 is the old sum, and the league fits what
        is in between.  Linear in the weight, so the climber has a
        gradient."""
        st = _played()
        _hand(st, 0, LEADERS)
        base = _w()
        vals = _each(st, 0, base, LEADERS)
        rest = sum(vals) - max(vals)
        for x in (0.0, 0.25, 0.5, 1.0):
            w = _w(hand_swap_extra=x)
            self.assertAlmostEqual(W.hand_potential(st, 0, w),
                                   max(vals) + x * rest, places=9)

    def test_wonders_are_swap_priced_and_still_SUM(self):
        """The distinction the two sets in `board_yields` exist for.

        A wonder is priced by a swap diff (Lane A added it to `SWAP_TYPES`),
        but it is not single-slot: two wonders in hand really can both be
        built, one after the other, so summing them is optimism about time
        and not the arithmetic impossibility that summing two leaders is.
        Keying the hand collapse on `SWAP_TYPES` instead of `SINGLE_SLOT`
        would silently start collapsing them, which is exactly the kind of
        change that lands with no test failing."""
        st = _played()
        w = _w()
        # both priced differently by the board than by the static table in this
        # state, so this is not vacuous
        pair = ("St. Peter's Basilica", "Transcontinental Railroad")
        for n in pair:
            self.assertEqual(C.db().by_name[n]["type"], "wonder")
        self.assertIn("wonder", BY.SWAP_TYPES)
        self.assertNotIn("wonder", BY.SINGLE_SLOT)
        _hand(st, 0, pair)
        vals = _each(st, 0, w, pair)
        self.assertAlmostEqual(W.hand_potential(st, 0, w), sum(vals),
                               places=9)

    def test_the_rival_hand_collapses_the_same_way(self):
        """`rival_hand_potential` prices the rival's hand through the same
        function on the rival's own board; the rival has one leader slot
        too."""
        st = _played()
        w = _w()
        _hand(st, 0, ())
        _hand(st, 1, LEADERS)
        vals = _each(st, 1, w, LEADERS)
        self.assertAlmostEqual(W.rival_hand_potential(st, 0, w), max(vals),
                               places=9)


class TestStillInertAtTheShippedDefaults(unittest.TestCase):
    """`card_board_credit` is 0.0 in `DEFAULT_WEIGHTS` and in every champion.

    At 0.0 a leader is not priced as a replacement -- it is priced off the
    static table like anything else -- so there is no diff to collapse and
    the hand must still be a plain sum, byte for byte.  This is the property
    that lets the league arms restart on this commit.
    """

    def test_the_hand_is_still_a_plain_sum(self):
        st = _played()
        w = dict(W.DEFAULT_WEIGHTS)
        self.assertEqual(w["card_board_credit"], 0.0)
        self.assertEqual(w["hand_swap_extra"], 0.0)
        for names in (LEADERS, GOVERNMENTS, LEADERS + GOVERNMENTS):
            _hand(st, 0, names)
            self.assertAlmostEqual(W.hand_potential(st, 0, w),
                                   sum(_each(st, 0, w, names)), places=9)

    def test_the_new_weights_are_all_zero(self):
        for k in ("hand_swap_extra", "card_board_leader",
                  "card_board_government", "card_board_action"):
            self.assertEqual(W.DEFAULT_WEIGHTS[k], 0.0, k)
