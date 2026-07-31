"""An action card may not be priced through a coordinate `evaluate` never pays.

THE DEFECT, stated as the property that failed
----------------------------------------------
`docs/YELLOW_TECH_PRICING.md` section 3 measured the bot taking **2.72** action
cards per seat-game at 2p against a human **12.98**, and **5.90** against
**10.25** at 3p -- a 4.8x gap and the largest single card-type deficit left in
the game (`docs/OPEN_ITEMS.md` item 24).

The cause is one mechanism with three faces, and all three are the same
sentence `docs/UNIT_TECH_PRICING.md` and `docs/YELLOW_TECH_PRICING.md` already
closed with: *a card is worth what `evaluate` pays for what it does.*

1. **Three of the coordinates an action card's value is spelled in are not
   features at all.**  `free_civil_action`, `resource_discount` and
   `restricted_resources` appear in `DEFAULT_WEIGHTS` and in `card_potential`
   and NOWHERE in `features()`, so `evaluate` never multiplies them by
   anything and no game the league plays can produce a gradient on them.  They
   are 0.0 on every champion in the pool.  Thirteen of the thirty-three action
   cards -- every Rich Land, Urban Growth, Engineering Genius and Efficient
   Upgrade -- carry NOTHING ELSE, so they price at exactly **0.000**.
2. **The Reserves' "gain N food OR N resources" is multiplied by
   `card_board_credit`**, which is 0.0 on every champion in the pool, and the
   early return above it skips `_card_choices` outright.  Three more cards at
   exactly 0.000, from pricing code that is present and correct.
3. **A one-shot `gainCulture` is priced at the bare `w["culture"]`** where
   `evaluate` pays `w[k] + (1-L)w[k_early] + L*w[k_late]`.  Same phase-blend
   mismatch `feature_marginal` was written for one lane ago.

`TestTheDefect` is the NEGATIVE CONTROL for the whole file: it asserts the
STATELESS table is still exactly as blind as it was, so nothing below can pass
because the defect quietly went away for an unrelated reason.

WHAT IS PINNED, AND WHAT IS DELIBERATELY NOT
--------------------------------------------
Pinned: that no action card is worth exactly nothing on a board where it is
worth something; that the free civil action is priced at the civil-action
marginal `evaluate` itself uses; that the two ring-fenced yields are priced as
resources; that a choice is a max and needs no board credit; that
`action_board_credit` = 0.0 recovers the static answer for all 236 cards
exactly; and that an action card in the row can reach `row_urgency` at all.

Not pinned: any take rate, or that any particular card is taken in any given
position.  Whether Engineering Genius beats Alchemy on THIS board is a
judgement the weights make, and pinning it here would pin the champion of the
week into the test suite.
"""
import random
import unittest

from engine import actions as A, cards as C, game as G
from engine.state import WonderInProgress
from engine.bots import weighted as W


#: read off the card database, deliberately NOT off `weighted._is_action` --
#: this file has to be droppable onto the parent tree as a negative control,
#: and a list built from the code under test cannot fail there, it can only
#: fail to import.
ACTIONS = [c["name"] for c in C.db().cards if c["type"] == "action"]

#: the thirteen whose ENTIRE printed value is a free civil action plus a
#: resource discount, i.e. the ones that price at exactly 0.000 statically.
FLAT_ZERO = [c["name"] for c in C.db().cards
             if c["type"] == "action"
             and set(c.get("effects") or {}) <= {"freeCivilAction",
                                                 "resourceDiscount"}]

RESERVES = [n for n in ACTIONS if n.startswith("Reserves")]

ALL_CARDS = [c["name"] for c in C.db().cards]


def _w(**over):
    return dict(W.DEFAULT_WEIGHTS, **over)


def _played(seed=5, plies=60, players=2):
    st = G.new_game(players, seed)
    rng = random.Random(seed)
    bot = W.WeightedBot(seed=seed)
    for _ in range(plies):
        if st.game_over:
            break
        A.apply(st, bot.pick(st, A.legal_moves(st)), rng)
    return st


class TestTheDefect(unittest.TestCase):
    """The negative control: the stateless table is still exactly as blind.

    Every assertion here passes on the parent tree AND after the fix -- the
    fix is entirely in the BOARD path, and this is the thing that says so."""

    def test_thirteen_action_cards_still_price_at_exactly_zero_statelessly(self):
        self.assertEqual(len(FLAT_ZERO), 13)
        for n in FLAT_ZERO:
            self.assertEqual(W.card_potential(n, _w()), 0.0, n)

    def test_the_three_dead_coordinates_are_still_not_features(self):
        """The whole diagnosis in one assertion.  If any of these ever becomes
        a real feature this test should be deleted, not relaxed."""
        f = W.features(G.new_game(2, 1), 0, None)
        for k in ("free_civil_action", "resource_discount",
                  "restricted_resources"):
            self.assertIn(k, W.DEFAULT_WEIGHTS)
            self.assertNotIn(k, f)

    def test_the_static_reserves_are_still_gated_on_the_board_credit(self):
        for n in RESERVES:
            self.assertEqual(W.card_potential(n, _w()), 0.0, n)
            self.assertGreater(
                W.card_potential(n, _w(card_board_credit=1.0)), 0.0, n)


class TestNoActionCardIsWorthNothing(unittest.TestCase):
    """The behavioural property the defect violated: a card that does
    something has to price as doing something."""

    def test_every_flat_zero_card_is_positive_on_a_real_board(self):
        st = _played()
        for n in FLAT_ZERO:
            self.assertEqual(W.card_potential(n, _w()), 0.0, n)
            self.assertGreater(W.card_potential(n, _w(), st, 0), 0.0, n)

    def test_every_reserves_is_positive_on_a_real_board(self):
        st = _played()
        for n in RESERVES:
            self.assertGreater(W.card_potential(n, _w(), st, 0), 0.0, n)

    def test_a_bigger_discount_is_worth_more(self):
        """Rich Land A/I/II pay 1/2/3 fewer resources for the same ordered
        action, so their prices must be strictly increasing.  Statically all
        three are 0.000 and this cannot hold."""
        st = _played()
        vals = [W.card_potential("Rich Land (%s)" % a, _w(), st, 0)
                for a in ("A", "I", "II")]
        self.assertEqual(vals, sorted(vals))
        self.assertLess(vals[0], vals[-1])


class TestItIsPricedInCoordinatesEvaluatePays(unittest.TestCase):
    """Each half of `action_value` moves with the LIVE weight it claims to be
    derived from, and with nothing else."""

    def test_the_free_civil_action_is_worth_a_civil_action_times_its_credit(
            self):
        """At `free_action_credit` 1.0 the ordered action is priced at exactly
        one civil action's marginal, and the credit scales it linearly."""
        st = _played()
        base = _w(free_action_credit=1.0)
        bump = _w(free_action_credit=1.0,
                  civil_actions=base["civil_actions"] + 1.0)
        for n in FLAT_ZERO:
            self.assertAlmostEqual(
                W.card_potential(n, bump, st, 0)
                - W.card_potential(n, base, st, 0), 1.0, places=9, msg=n)

    def test_the_shipped_default_charges_nothing_for_the_ordered_action(self):
        """RB 3.11: playing the card costs one civil action and grants one, so
        the action economy is a wash and the shipped credit is 0.0.  At 1.0 the
        take rate lands on the human number and the A/B is 32.8% against a 50%
        null -- see `weighted.action_value` point 2."""
        self.assertEqual(W.DEFAULT_WEIGHTS["free_action_credit"], 0.0)
        st = _played()
        base = _w()
        bump = _w(civil_actions=base["civil_actions"] + 5.0)
        for n in FLAT_ZERO:
            self.assertAlmostEqual(W.card_potential(n, bump, st, 0),
                                   W.card_potential(n, base, st, 0),
                                   places=9, msg=n)

    def test_the_resource_discount_is_worth_resources(self):
        st = _played()
        base = _w()
        bump = _w(resource_stock=base["resource_stock"] + 1.0)
        # Rich Land (II) is "pay 3 fewer resources"
        self.assertAlmostEqual(
            W.card_potential("Rich Land (II)", bump, st, 0)
            - W.card_potential("Rich Land (II)", base, st, 0), 3.0, places=9)

    def test_the_dead_coordinates_no_longer_move_the_board_price(self):
        st = _played()
        base = _w()
        ref = {n: W.card_potential(n, base, st, 0) for n in ACTIONS}
        for key in ("free_civil_action", "resource_discount",
                    "restricted_resources"):
            w = _w(**{key: 5.0})
            for n in ACTIONS:
                self.assertAlmostEqual(W.card_potential(n, w, st, 0), ref[n],
                                       places=9, msg="%s / %s" % (n, key))

    def test_a_ring_fenced_resource_is_a_resource_times_its_credit(self):
        st = _played()
        base = _w()
        # Patriotism (III): +1 military action, 4 resources off military units
        full = W.card_potential("Patriotism (III)", base, st, 0)
        none = W.card_potential("Patriotism (III)",
                                _w(restricted_resource_credit=0.0), st, 0)
        half = W.card_potential("Patriotism (III)",
                                _w(restricted_resource_credit=0.5), st, 0)
        self.assertAlmostEqual(full - none,
                               4.0 * base["resource_stock"], places=9)
        self.assertAlmostEqual(half, (full + none) / 2.0, places=9)

    def test_a_one_shot_culture_gain_takes_the_phase_blend(self):
        """`culture` is a `PHASE_KEYS` feature.  The static table reads the
        bare weight; `evaluate` pays the blend, and so must the card."""
        st = _played()
        w = _w(culture_early=-0.4, culture_late=1.5)
        late = W.lateness(st)
        want = (w["science"] * 1.0
                + 4.0 * W.feature_marginal("culture", st, 0, w, late))
        self.assertAlmostEqual(
            W.card_potential("Cultural Heritage (A)", w, st, 0), want,
            places=9)
        self.assertNotAlmostEqual(
            W.card_potential("Cultural Heritage (A)", w, st, 0),
            W.card_potential("Cultural Heritage (A)", w), places=6)

    def test_a_choice_is_a_max_and_needs_no_board_credit(self):
        """Reserves (III) is "gain 4 food OR 4 resources": the better of the
        two, at `card_board_credit` 0.0, which is where every champion is."""
        st = _played()
        w = _w(food_stock=0.2, resource_stock=0.9, card_board_credit=0.0)
        self.assertAlmostEqual(W.card_potential("Reserves (III)", w, st, 0),
                               4.0 * 0.9, places=9)
        w2 = _w(food_stock=0.9, resource_stock=0.2, card_board_credit=0.0)
        self.assertAlmostEqual(W.card_potential("Reserves (III)", w2, st, 0),
                               4.0 * 0.9, places=9)


class TestTheBoardScaledThree(unittest.TestCase):
    """Endowment for the Arts, Wave of Nationalism and Military Build-Up print
    a coefficient per table size and multiply it by a count of rivals.
    `board_yields.board_extra` already computed that and nothing reached it
    without `card_board_credit`, which is 0.0 everywhere."""

    def test_endowment_is_worth_nothing_when_ahead_and_something_when_behind(self):
        st = G.new_game(2, 1)
        w = _w()
        st.players[0].culture, st.players[1].culture = 50, 10
        self.assertEqual(W.card_potential("Endowment for the Arts", w, st, 0),
                         0.0)
        st.players[0].culture, st.players[1].culture = 10, 50
        behind = W.card_potential("Endowment for the Arts", w, st, 0)
        # 6 culture per richer rival at 2p, one rival, at the culture marginal
        self.assertAlmostEqual(
            behind, 6.0 * W.feature_marginal("culture", st, 0, w), places=9)

    def test_military_build_up_is_priced_when_a_rival_is_stronger(self):
        st = _played()
        w = _w()
        v = W.card_potential("Military Build-Up", w, st, 0)
        self.assertGreaterEqual(v, 0.0)
        # whatever the board says, it must be ring-fenced resources and
        # nothing else, so the credit switches it entirely off
        self.assertEqual(
            W.card_potential("Military Build-Up",
                             _w(restricted_resource_credit=0.0), st, 0), 0.0)


class TestTheOptOut(unittest.TestCase):
    """`action_board_credit` = 0.0 is the one constant that recovers the parent
    commit's pricing, and it has to do so for ALL 236 cards, not just the 33 --
    that is what makes the change duellable against itself in one process."""

    def test_zero_credit_is_the_static_answer_for_every_card(self):
        st = _played()
        w = _w(action_board_credit=0.0, tech_board_credit=0.0,
               unit_tech_credit=0.0, gov_board_credit=0.0,
               card_board_credit=0.0)
        for n in ALL_CARDS:
            self.assertAlmostEqual(W.card_potential(n, w, st, 0),
                                   W.card_potential(n, w), places=9, msg=n)

    def test_the_credit_scales_linearly(self):
        st = _played()
        for n in ACTIONS:
            one = W.card_potential(n, _w(), st, 0)
            self.assertAlmostEqual(
                W.card_potential(n, _w(action_board_credit=2.5), st, 0),
                2.5 * one, places=9, msg=n)

    def test_it_does_not_touch_any_other_type(self):
        st = _played()
        base = _w()
        off = _w(action_board_credit=0.0)
        for n in ALL_CARDS:
            if C.db().type_by_name[n] == "action":
                continue
            self.assertAlmostEqual(W.card_potential(n, base, st, 0),
                                   W.card_potential(n, off, st, 0),
                                   places=9, msg=n)


class TestItReachesRowPressure(unittest.TestCase):
    """The second half of the mechanism, and the one that is not about
    magnitude: `row_pressure` skips any card whose `card_potential` is <= 0, so
    a flat-zero action card was invisible to `row_urgency` and
    `row_bargain_forgone` at ANY weight."""

    def test_an_action_card_in_the_row_reaches_row_urgency_only_once_priced(self):
        st = G.new_game(2, 1)
        # a row of ONE card, so the two sums differ by that card and nothing
        # else -- the other slots hold technologies whose own price moves for
        # reasons this test is not about.
        for i in range(len(st.card_row)):
            st.card_row[i] = None
        st.card_row[0] = "Engineering Genius (II)"
        ctx = W.rival_context(st, 0, root_row=tuple(st.card_row))
        off = _w(row_urgency=1.0, row_bargain_forgone=1.0,
                 action_board_credit=0.0)
        on = dict(off, action_board_credit=1.0)
        self.assertEqual(W.card_potential("Engineering Genius (II)", off,
                                          st, 0), 0.0)
        self.assertGreater(W.card_potential("Engineering Genius (II)", on,
                                            st, 0), 0.0)
        u_off, _ = W.row_pressure(st, 0, off, ctx)
        u_on, _ = W.row_pressure(st, 0, on, ctx)
        self.assertAlmostEqual(
            u_on - u_off,
            W.card_potential("Engineering Genius (II)", on, st, 0))


class TestTheEngineCanActuallyDoIt(unittest.TestCase):
    """Bucket (a) of the audit, closed by assertion rather than by reading:
    every action card in the game has a legal `play_action` move in some
    position the engine can reach, so none of these prices is pricing a move
    that cannot be made."""

    def test_every_action_card_is_playable_from_a_stocked_hand(self):
        for n in ACTIONS:
            st = G.new_game(2, 1)
            st.round = 2                  # 1.9: round 1 is takes only
            p = st.players[0]
            p.hand_civil.append(n)
            p.food, p.resources, p.science = 30, 30, 30
            # The ordered action has to be legal, which is the point of this
            # test rather than an exception to it: an action card's price is
            # the price of a move the engine can actually make.  Engineering
            # Genius orders a wonder stage, Breakthrough a development.
            # Efficient Upgrade orders an upgrade, so something upgradeable
            # has to exist: develop Irrigation for real (the engine's own
            # handler) and leave the starting Agriculture workers on the board.
            p.wonder = WonderInProgress("Pyramids")
            p.hand_civil.append("Irrigation")
            A._h_develop(st, p, ("develop", "Irrigation"), random.Random(1),
                         free=True)
            p.hand_civil.append("Philosophy")
            self.assertTrue(A._action_card_playable(st, p, n),
                            "%s never playable" % n)
            self.assertIn(("play_action", n), A.legal_moves(st))


if __name__ == "__main__":
    unittest.main()
