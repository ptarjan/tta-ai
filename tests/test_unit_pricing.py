"""A unit technology may not price as pure cost, and the pin for the fix.

THE DEFECT, stated as the property that failed
----------------------------------------------
`docs/SYSTEM_COVERAGE.md` measured the bot taking **0.15 / 0.06 / 0.45** unit
cards per seat-game against a human **3.84 / 2.79 / 3.43** -- it fights the
whole game with Age A Warriors.  The mechanism is arithmetic and is reproduced
below rather than asserted: `weighted._card_yields` charges a unit's `techCost`
through `science` and its `buildCost` through `resource_stock`, both trained
weights, and returns the strength through `unit_strength_credit`, which is
0.0 on every champion the league has.  Cost priced, gain not -- the standing
hazard of docs/HAZARDS.md.

`test_the_old_static_pricing_is_still_strictly_negative` is the NEGATIVE
CONTROL for the whole file, in the sense `tests/test_search_root_is_
determinized.py` uses the term: it asserts the defect is still there on the
stateless path, so the tests below cannot pass by the defect having quietly
gone away for some unrelated reason.  Revert `card_potential`'s unit branch
and `test_every_unit_can_price_positive_on_some_board` and
`test_a_unit_is_visible_to_row_pressure` both fail, while the control keeps
passing.

WHAT IS PINNED, AND WHAT IS DELIBERATELY NOT
--------------------------------------------
Pinned: that the price is a board query, that it is not structurally
sign-locked, that `unit_tech_credit` = 0.0 recovers the static answer exactly,
and that `row_pressure` can see a unit at all.

Not pinned: any particular take rate, or that a unit is taken in any given
position.  Whether Riflemen is worth 6 science on THIS board is a judgement
the weights make, and pinning it here would be pinning the champion of the
week into the test suite.
"""
import random
import unittest

from engine import actions as A, cards as C, effects, game as G
from engine.bots import weighted as W
from engine.bots import board_yields as BY


#: read off the card database, deliberately NOT off `weighted._is_unit` --
#: this file has to be droppable onto a reverted tree as a negative control,
#: and a list built from the code under test cannot fail there, it can only
#: fail to import.
UNITS = [c["name"] for c in C.db().cards if c["type"] in C.UNIT_TYPES]


def _w(**over):
    return dict(W.DEFAULT_WEIGHTS, **over)


def _played(seed=5, plies=40):
    """A mid-game board, so the tests are not all standing on turn one."""
    st = G.new_game(2, seed)
    rng = random.Random(seed)
    bot = W.WeightedBot(seed=seed)
    for _ in range(plies):
        if st.game_over:
            break
        A.apply(st, bot.pick(st, A.legal_moves(st)), rng)
    return st


class TestTheDefect(unittest.TestCase):
    """The negative control: the static table is still exactly as biased."""

    def test_the_old_static_pricing_is_still_strictly_negative(self):
        w = _w()
        self.assertEqual(w["unit_strength_credit"], 0.0)
        for name in UNITS:
            self.assertLess(W.card_potential(name, w), 0.0, name)

    def test_the_credit_that_cannot_flip_a_sign(self):
        """Why this needed reshaping and not retuning.

        `unit_strength_credit` multiplies the printed strength only, so it
        moves the price by (strength x w["strength"]) per unit of credit
        against a cost fixed by `techCost`/`buildCost`.  On DEFAULT_WEIGHTS
        that is a plateau tens of units long, and `hillclimb.mutate` walks it
        in steps of ~0.15 x sigma with no fitness gradient anywhere on it.
        """
        base = W.card_potential("Modern Infantry", _w())
        one = W.card_potential("Modern Infantry", _w(unit_strength_credit=1.0))
        self.assertLess(base, 0.0)
        self.assertLess(one, 0.0)
        # and the credit it would take to flip it is far outside anything a
        # 0.0-initialised climb reaches
        step = one - base
        self.assertGreater(-base / step, 3.0)


class TestTheFix(unittest.TestCase):

    def test_zero_credit_recovers_the_static_answer(self):
        st = _played()
        w = _w(unit_tech_credit=0.0)
        for name in UNITS:
            self.assertEqual(W.card_potential(name, w, st, 0),
                             W.card_potential(name, w), name)

    def test_credit_scales_linearly(self):
        st = _played()
        one = W.card_potential("Swordsmen", _w(), st, 0)
        half = W.card_potential("Swordsmen", _w(unit_tech_credit=0.5), st, 0)
        self.assertAlmostEqual(half * 2.0, one)

    def test_every_unit_can_price_positive_on_some_board(self):
        """FAILS ON THE OLD CODE for all ten, on EVERY board and at every
        weight vector in the repo -- the static table has no board in it, and
        docs/CARD_BLINDNESS_MILITARY.md section 5.1 is the measurement that no
        setting of `unit_strength_credit` flips the sign either.

        The board chosen is a player with four unit workers, i.e. four
        upgrades riding on one `develop`.  That is not a contrived position --
        it is the position a player who has been buying army all game is in,
        and it is exactly the one the static table cannot distinguish from a
        player with none.  What is asserted is that the sign is REACHABLE, not
        that any particular board takes the card.
        """
        st = G.new_game(2, 1)
        p = st.players[0]
        p.techs["Warriors"].workers = 4
        effects.invalidate(st, p)
        w = _w()
        priced = {n: W.card_potential(n, w, st, 0) for n in UNITS}
        # Warriors is in `game.START_TECHS`, so it is already developed and
        # the card is dead in hand: exactly 0.0, neither cost nor gain.
        self.assertEqual(priced["Warriors"], 0.0)
        rest = [n for n in UNITS if n != "Warriors"]
        self.assertTrue(all(priced[n] > 0.0 for n in rest),
                        "still sign-locked: %r" % priced)
        # ...and the same nine cards on the same board under the static table
        # are every one of them negative.  This is the paired half: it is the
        # BOARD that was missing, not the weights.
        off = dict(w, unit_tech_credit=0.0)
        self.assertTrue(all(W.card_potential(n, off, st, 0) < 0.0
                            for n in rest))

    def test_the_price_is_a_board_query_not_a_table(self):
        """The same card, two boards, two prices -- and the direction is the
        one the rules imply: more workers to move, more strength bought."""
        w = _w()
        one = G.new_game(2, 1)
        many = G.new_game(2, 1)
        p = many.players[0]
        p.techs["Warriors"].workers = 3
        effects.invalidate(many, p)
        self.assertGreater(W.card_potential("Swordsmen", w, many, 0),
                           W.card_potential("Swordsmen", w, one, 0))

    def test_a_unit_already_developed_is_worth_exactly_nothing(self):
        st = G.new_game(2, 1)
        self.assertIn("Warriors", st.players[0].techs)
        self.assertEqual(BY.unit_upgrade("Warriors", st, 0), (0.0, 0.0, 0.0))

    def test_the_costs_come_from_the_engine(self):
        """`unit_upgrade` may not restate the rules.  Riflemen off Warriors
        is `actions.upgrade_cost`, not `buildCost`; 3, not 5."""
        st = G.new_game(2, 1)
        gained, sci, res = BY.unit_upgrade("Riflemen", st, 0)
        p = st.players[0]
        self.assertEqual(res, float(A.upgrade_cost(st, p, "Warriors",
                                                   "Riflemen")))
        self.assertEqual(sci, float(effects.tech_cost(st, p, "Riflemen")))
        self.assertNotEqual(res, float(C.db().get("Riflemen")["buildCost"]))
        # and the strength is the DIFFERENCE, on the one worker held
        self.assertEqual(gained, 2.0)


class TestRowPressureCanSeeAUnit(unittest.TestCase):
    """The second half of the mechanism, and the one that is not about
    magnitude at all: `row_pressure` skips any card whose `card_potential` is
    <= 0, so a strictly-negative unit was invisible to `row_urgency` and
    `row_bargain_forgone` at ANY weight."""

    def test_a_unit_in_the_row_reaches_row_urgency_only_once_positive(self):
        st = G.new_game(2, 1)
        p = st.players[0]
        p.techs["Warriors"].workers = 4
        effects.invalidate(st, p)
        st.card_row[0] = "Modern Infantry"
        ctx = W.rival_context(st, 0, root_row=tuple(st.card_row))
        off = _w(row_urgency=1.0, row_bargain_forgone=1.0,
                 unit_tech_credit=0.0)
        on = dict(off, unit_tech_credit=1.0)
        # slot 0 at 2p is swept before this player acts again, so a card
        # priced above zero there lands in `row_urgency` and one priced at or
        # below zero is skipped entirely.
        self.assertLess(W.card_potential("Modern Infantry", off, st, 0), 0.0)
        self.assertGreater(W.card_potential("Modern Infantry", on, st, 0), 0.0)
        u_off, _ = W.row_pressure(st, 0, off, ctx)
        u_on, _ = W.row_pressure(st, 0, on, ctx)
        self.assertAlmostEqual(
            u_on - u_off, W.card_potential("Modern Infantry", on, st, 0))


class TestStrengthMarginal(unittest.TestCase):
    """`strength_marginal` claims to be d(evaluate)/d(strength).  That is a
    checkable claim, so it is checked numerically rather than by reading the
    code -- the drift guard `docs/HAZARDS.md` asks for
    ("two implementations of one rule always drift")."""

    def _check(self, st, idx, w):
        ctx = W.rival_context(st, idx)
        before = W.evaluate(st, idx, w, ctx)
        p = st.players[idx]
        p.strength_extra += 1
        effects.invalidate(st, p)
        try:
            after = W.evaluate(st, idx, w, ctx)
        finally:
            p.strength_extra -= 1
            effects.invalidate(st, p)
        # `evaluate` also carries `hand_potential`/`row_pressure`, which price
        # cards through `w` and now read the board's strength through
        # `strength_marginal` themselves.  Switch them off so the numerical
        # derivative measures the linear features and nothing else.
        return after - before

    def test_it_equals_the_numerical_derivative(self):
        w = _w(hand_potential=0.0, row_urgency=0.0, row_bargain_forgone=0.0,
               rival_hand_potential=0.0, wonder_potential=0.0,
               hand_mil_potential=0.0, strength=0.35, strength_rel=0.2,
               strength_deficit=-0.6, strength_lead=0.3,
               strength_rel_early=0.4, strength_rel_late=-0.2)
        seen = 0
        for seed in (1, 3, 5, 7, 11, 13, 17, 19):
            for plies in (20, 30, 45):
                st = _played(seed, plies=plies)
                for idx in (0, 1):
                    p = st.players[idx]
                    s = effects.state_stats(st, p).strength
                    rel = s - W.rival_strength(st, idx)
                    if rel in (0, 6) or s == 0:
                        continue      # the two kinks and the rating clamp
                    seen += 1
                    self.assertAlmostEqual(
                        self._check(st, idx, w),
                        W.strength_marginal(st, idx, w), places=9,
                        msg="seed=%d plies=%d idx=%d rel=%s"
                            % (seed, plies, idx, rel))
        self.assertGreater(seen, 10)


class TestRivalStrengthAgrees(unittest.TestCase):
    """`rival_strength` is a second spelling of one field of `rival_context`,
    written for cost.  Same device as `_SWEEP` vs `game.SWEEP`: hold the two
    together with a test rather than hoping."""

    def test_it_matches_rival_context_over_self_play(self):
        seen = 0
        for seed in (2, 4, 6):
            st = G.new_game(3, seed)
            rng = random.Random(seed)
            bot = W.WeightedBot(seed=seed)
            for _ in range(60):
                if st.game_over:
                    break
                for idx in range(3):
                    seen += 1
                    self.assertEqual(
                        W.rival_strength(st, idx),
                        W.rival_context(st, idx)["rival_strength"])
                A.apply(st, bot.pick(st, A.legal_moves(st)), rng)
        self.assertGreater(seen, 100)


class TestTheCacheIsKeyedCompletely(unittest.TestCase):
    """`unit_upgrade` memoises on `(name, effects.stats_key(state, p))`.  A key
    that missed a field would hand out silently stale prices -- the failure
    mode `tests/test_board_yields.py:TestStatsKeyIsACompleteMemoKey` guards
    for the swap diff, restated for this cache."""

    def test_moving_a_worker_changes_the_key_and_the_price(self):
        st = G.new_game(2, 1)
        p = st.players[0]
        first = BY.unit_upgrade("Swordsmen", st, 0)
        p.techs["Warriors"].workers = 3
        effects.invalidate(st, p)
        second = BY.unit_upgrade("Swordsmen", st, 0)
        self.assertNotEqual(first, second)
        self.assertEqual(second[0], 3.0)


if __name__ == "__main__":
    unittest.main()
