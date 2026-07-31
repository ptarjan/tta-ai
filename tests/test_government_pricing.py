"""A government's level, and both of its prices.

THE DEFECT, stated as the property that failed
----------------------------------------------
`weighted.features()` reads a government's age level TWICE -- into
`tech_levels` alongside every other technology in the game, and again on its
own as `gov_level` (2.0 in `DEFAULT_WEIGHTS`) -- and neither `_card_yields`
nor the swap diff in `board_yields` ever emitted either one.  A government was
therefore missing exactly the term `docs/CARD_BLINDNESS.md` §15 (the former `YELLOW_TECH_PRICING.md`) added to
every other technology (`docs/OPEN_ITEMS.md` section 2 item 22).

It was worse than that, and `TestTheDefect` below is the measurement rather
than the claim: on the static path a government prices at **exactly 0.000**
for three of the eight cards and near it for the rest, because
`_card_yields` reads `techCost` (`null` on every government -- they print
`peacefulCost` and `revolutionCost`) and `production`/`effects` (where a
government's civil actions, military actions and urban limit are not -- they
are top-level fields only `effects.compute` reads).  No cost, no gain,
nothing.

WHAT IS PINNED, AND WHAT IS DELIBERATELY NOT
--------------------------------------------
Pinned: that the level is emitted on BOTH paths and as a DIFFERENCE; that
both routes RULES_SPEC 8.2/8.3 offers are priced and gated on the board; that
the civil actions a revolution burns are charged to the coordinate
`evaluate` actually watches move, checked by APPLYING the move and diffing
`features()`; that `gov_board_credit` = 0.0 recovers the static answer byte
for byte.

Not pinned: any particular take rate, or that a government is taken in any
given position.  Whether Monarchy is worth 8 science on THIS board is a
judgement the weights make.
"""
import random
import unittest

from engine import actions as A, cards as C, effects, game as G
from engine.bots import board_yields as BY
from engine.bots import weighted as W

#: read off the card database, deliberately NOT off `weighted._is_government`
#: -- this file has to be droppable onto a reverted tree as a negative control.
GOVERNMENTS = [c["name"] for c in C.db().cards if c["type"] == "government"]


def _w(**over):
    return dict(W.DEFAULT_WEIGHTS, **over)


def _played(seed=5, plies=40):
    st = G.new_game(2, seed)
    rng = random.Random(seed)
    bot = W.WeightedBot(seed=seed)
    for _ in range(plies):
        if st.game_over:
            break
        A.apply(st, bot.pick(st, A.legal_moves(st)), rng)
    return st


class TestTheDefect(unittest.TestCase):
    """The negative control: the static table is still exactly as blind, so
    none of the tests below can pass by the defect having quietly gone away
    somewhere else."""

    def test_the_static_table_still_prices_three_governments_at_zero(self):
        w = _w()
        flat = [n for n in GOVERNMENTS if W.card_potential(n, w) == 0.0]
        self.assertEqual(sorted(flat),
                         ["Constitutional Monarchy", "Despotism", "Monarchy",
                          "Republic"])

    def test_the_static_table_still_charges_no_science_for_a_government(self):
        """`techCost` is null on all eight; `peacefulCost` / `revolutionCost`
        are what they actually cost, and `_card_yields` reads neither."""
        for name in GOVERNMENTS:
            card = C.db().get(name)
            self.assertIsNone(card.get("techCost"), name)
            keys = [k for k, _a, _kind in W._card_yields(name)]
            self.assertNotIn("science", keys, name)


class TestTheLevelIsPriced(unittest.TestCase):

    def test_both_paths_emit_it(self):
        """`features()` reads the level twice, so both pricing paths have to
        emit it twice -- the swap diff (reached through `card_board_credit`)
        and `government_plans` (the live path)."""
        st = G.new_game(2, 1)
        swap = {k for k, _a, _kind in BY.board_yields("Monarchy", st, 0)}
        self.assertIn("tech_levels", swap)
        self.assertIn("gov_level", swap)
        gains, _routes = BY.government_plans("Monarchy", st, 0)
        keys = {k for k, _a, _kind in gains}
        self.assertIn("tech_levels", keys)
        self.assertIn("gov_level", keys)

    def test_it_is_a_difference_and_not_the_printed_level(self):
        """RULES_SPEC 8.1: the new government replaces the old regardless of
        level, so `features()` stops counting the old one."""
        st = G.new_game(2, 1)
        p = st.players[0]
        self.assertEqual(p.government, "Despotism")     # level 0
        got = dict((k, a) for k, a, _kind in
                   BY.government_plans("Republic", st, 0)[0])
        self.assertEqual(got["tech_levels"], 2.0)       # Republic is level 2
        self.assertEqual(got["gov_level"], 2.0)
        p.government = "Constitutional Monarchy"        # also level 2
        effects.invalidate(st, p)
        got = dict((k, a) for k, a, _kind in
                   BY.government_plans("Republic", st, 0)[0])
        self.assertNotIn("tech_levels", got)
        self.assertNotIn("gov_level", got)

    def test_it_moves_the_feature_it_claims_to(self):
        """The claim is checked against `features()` itself: develop the
        government and require `tech_levels` and `gov_level` to move by the
        amount that was priced."""
        st = G.new_game(2, 1)
        p = st.players[0]
        before = W.features(st, 0)
        got = dict((k, a) for k, a, _kind in
                   BY.government_plans("Monarchy", st, 0)[0])
        p.government = "Monarchy"
        effects.invalidate(st, p)
        after = W.features(st, 0)
        for key in ("tech_levels", "gov_level"):
            self.assertEqual(after[key] - before[key], got[key], key)


class TestBothRoutes(unittest.TestCase):
    """RULES_SPEC 8.2 and 8.3 are two moves at two prices and
    `engine/actions.py:_action_moves` generates both."""

    def _full_pool(self, seed=5, plies=40):
        st = _played(seed, plies)
        idx = st.current
        p = st.players[idx]
        p.science = 40
        p.civil_actions = A.ca_total(st, p)
        effects.invalidate(st, p)
        return st, idx, p

    def test_the_engine_really_offers_both(self):
        st, idx, p = self._full_pool()
        p.hand_civil.append("Monarchy")
        moves = A.legal_moves(st)
        self.assertIn(("develop", "Monarchy"), moves)
        self.assertIn(("revolution", "Monarchy"), moves)

    def test_a_revolution_appears_as_a_second_route_only_when_legal(self):
        st, idx, p = self._full_pool()
        self.assertEqual(len(BY.government_plans("Monarchy", st, idx)[1]), 2)
        p.civil_actions -= 1            # RULES_SPEC 8.3.1: no longer legal
        effects.invalidate(st, p)
        self.assertEqual(len(BY.government_plans("Monarchy", st, idx)[1]), 1)
        self.assertFalse(A._can_revolt(st, p, "Monarchy"))

    def test_the_cheaper_route_is_the_one_priced(self):
        """Two science costs, 2 against 8 for Monarchy, and the revolution's
        own cost is the pool it burns -- so which is cheaper is a board
        question and the price has to move when the board does."""
        st, idx, p = self._full_pool()
        w = _w()
        both = W.card_potential("Monarchy", w, st, idx)
        p.civil_actions -= 1
        effects.invalidate(st, p)
        peaceful_only = W.card_potential("Monarchy", w, st, idx)
        self.assertGreater(both, peaceful_only)

    def test_the_burn_is_the_ca_left_the_engine_actually_destroys(self):
        """THE DRIFT GUARD, and the derivation `docs/OPEN_ITEMS.md` 9.1 asked
        for: apply the revolution and require the priced burn to equal the
        `ca_left` `features()` loses.  A comment claiming a coordinate is the
        right one is not a check."""
        st, idx, p = self._full_pool()
        p.hand_civil.append("Monarchy")
        routes = BY.government_plans("Monarchy", st, idx)[1]
        self.assertEqual(len(routes), 2)     # [0] peaceful, [1] revolution
        priced = sum(a for k, a, _kd in routes[1] if k == "ca_left")
        self.assertLess(priced, 0.0)
        before = W.features(st, idx)
        A.apply(st, ("revolution", "Monarchy"), random.Random(1))
        after = W.features(st, idx)
        self.assertEqual(after["ca_left"] - before["ca_left"], priced)
        # ...and the ALLOTMENT went UP, which is why charging the burn to
        # `civil_actions` would have been charging the gain side
        self.assertGreater(after["civil_actions"], before["civil_actions"])

    def test_a_peaceful_change_keeps_the_pool(self):
        st, idx, p = self._full_pool()
        p.hand_civil.append("Monarchy")
        route = BY.government_plans("Monarchy", st, idx)[1][0]
        priced = sum(a for k, a, _kd in route if k == "ca_left")
        before = W.features(st, idx)
        A.apply(st, ("develop", "Monarchy"), random.Random(1))
        after = W.features(st, idx)
        self.assertEqual(after["ca_left"] - before["ca_left"], priced)

    def test_newton_hands_one_civil_action_back(self):
        st, idx, p = self._full_pool()
        plain = BY.government_plans("Monarchy", st, idx)[1][1]
        p.leader = "Isaac Newton"
        effects.invalidate(st, p)
        newton = BY.government_plans("Monarchy", st, idx)[1][1]
        burn = lambda r: sum(a for k, a, _kd in r if k == "ca_left")  # noqa
        self.assertEqual(burn(newton) - burn(plain), 1.0)

    def test_robespierre_pays_with_the_military_pool(self):
        st, idx, p = self._full_pool()
        p.leader = "Maximilien Robespierre"
        p.military_actions = effects.state_stats(st, p).military_actions
        effects.invalidate(st, p)
        rev = BY.government_plans("Monarchy", st, idx)[1][1]
        got = dict((k, a) for k, a, _kd in rev)
        self.assertLess(got["ma_left"], 0.0)
        self.assertGreaterEqual(got.get("ca_left", 0.0), 0.0)
        self.assertEqual(got["culture"], 3.0)


class TestTheFix(unittest.TestCase):

    def test_zero_credit_recovers_the_static_answer(self):
        st = _played()
        w = _w(gov_board_credit=0.0)
        for name in GOVERNMENTS:
            self.assertEqual(W.card_potential(name, w, st, 0),
                             W.card_potential(name, w), name)

    def test_credit_scales_linearly(self):
        st = G.new_game(2, 1)
        one = W.card_potential("Monarchy", _w(), st, 0)
        half = W.card_potential("Monarchy", _w(gov_board_credit=0.5), st, 0)
        self.assertAlmostEqual(half * 2.0, one)

    def test_every_government_can_price_positive_on_some_board(self):
        """`row_pressure` skips any card whose `card_potential` is <= 0.0, so
        this is the invisibility guard, not a magnitude claim.  Despotism is
        the starting government and is in no deck, so nobody can take it."""
        st = G.new_game(2, 1)
        w = _w()
        priced = {n: W.card_potential(n, w, st, 0) for n in GOVERNMENTS}
        self.assertEqual(priced["Despotism"], 0.0)
        rest = [n for n in GOVERNMENTS if n != "Despotism"]
        self.assertTrue(all(priced[n] > 0.0 for n in rest), priced)

    def test_a_government_already_in_play_is_worth_exactly_nothing(self):
        st = G.new_game(2, 1)
        self.assertEqual(BY.government_plans("Despotism", st, 0), ((), ()))
        self.assertEqual(W.card_potential("Despotism", _w(), st, 0), 0.0)

    def test_the_price_is_a_board_query_not_a_table(self):
        """The same card is worth less to a player who already has a better
        government -- a replacement, not an absolute."""
        w = _w()
        poor = G.new_game(2, 1)
        rich = G.new_game(2, 1)
        p = rich.players[0]
        p.government = "Communism"
        effects.invalidate(rich, p)
        self.assertGreater(W.card_potential("Republic", w, poor, 0),
                           W.card_potential("Republic", w, rich, 0))

    def test_the_gains_are_priced_at_the_marginal_not_the_bare_weight(self):
        """`tech_levels` is in `PHASE_KEYS`, so `evaluate` pays
        `w[k] + (1-L)w[k_early] + L*w[k_late]` where the static table looked
        up the bare `w[k]` -- the error `docs/CARD_BLINDNESS.md` §15 (the former `docs/YELLOW_TECH_PRICING.md`) found."""
        st = G.new_game(2, 1)
        base = W.card_potential("Monarchy", _w(), st, 0)
        early = W.card_potential("Monarchy", _w(tech_levels_early=3.0), st, 0)
        self.assertNotAlmostEqual(base, early)
