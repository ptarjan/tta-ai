"""A technology may not be priced through a weight `evaluate` does not use.

THE DEFECT, stated as the property that failed
----------------------------------------------
`docs/CARD_BLINDNESS.md` measured the bot taking **0.03 / 0.00** laboratories
and **0.05 / 0.00** mines per seat-game at 2p/3p against a human **1.62 / 1.27**
and **1.18 / 1.21**, and Alchemy, Scientific Method and Coal literally zero
times at any table size.  Every yellow production technology prices strictly
negative on the live 2p champion (Irrigation -4.02, Iron -6.72, Alchemy
-11.19, Computers -20.41) and `row_pressure` skips any card whose
`card_potential` is <= 0, so the whole colour was invisible to `row_urgency`
and `row_bargain_forgone` -- the same MECHANICAL suppression that hid the
military units, from a different CAUSE.

The cause is two half-priced gains, both of them `card_potential` reading a
number `evaluate` does not use:

1. **`tech_levels` was mapped to nothing at all**, on every technology card in
   the game.  It is a `PHASE_KEYS` feature worth up to 9.23 eval points per
   level on the live 2p champion -- more than the rest of a yellow card put
   together.
2. **A production rate was priced at the bare `w[k]`** where `evaluate` pays
   `w[k] + (1-L)w[k_early] + L*w[k_late]`.  For `science_rate` on that vector
   the two are 0.25 and 5.29, a factor of twenty-one.

`TestTheDefect` is the NEGATIVE CONTROL for the whole file, in the sense
`tests/test_search_root_is_determinized.py` uses the term: it asserts the
stateless path is still exactly as blind, so nothing below can pass by the
defect having quietly gone away for an unrelated reason.

WHAT IS PINNED, AND WHAT IS DELIBERATELY NOT
--------------------------------------------
Pinned: that `feature_marginal` really is the derivative of `evaluate` it
claims to be, that every technology's `tech_levels` reaches the price, that the
price is a board query, that `tech_board_credit` = 0.0 recovers the static
answer exactly, and that `row_pressure` can see a laboratory at all.

Not pinned: any particular take rate, or that a laboratory is taken in any
given position.  Whether Alchemy is worth 4 science on THIS board is a
judgement the weights make, and pinning it here would be pinning the champion
of the week into the test suite.
"""
import random
import unittest

from engine import actions as A, cards as C, effects, game as G
from engine.bots import weighted as W
from engine.bots import board_yields as BY


#: read off the card database, deliberately NOT off `weighted._is_levelled_tech`
#: -- this file has to be droppable onto a reverted tree as a negative control,
#: and a list built from the code under test cannot fail there, it can only
#: fail to import.
YELLOW = [c["name"] for c in C.db().cards
          if c["type"] in ("farm", "mine", "lab")]
TECHS = [c["name"] for c in C.db().cards
         if c["type"] in (C.URBAN_TYPES | C.UNIT_TYPES | C.PRODUCTION_TYPES
                          | {"special-tech"})]


def _w(**over):
    return dict(W.DEFAULT_WEIGHTS, **over)


def _played(seed=5, plies=40, players=2):
    st = G.new_game(players, seed)
    rng = random.Random(seed)
    bot = W.WeightedBot(seed=seed)
    for _ in range(plies):
        if st.game_over:
            break
        A.apply(st, bot.pick(st, A.legal_moves(st)), rng)
    return st


class TestTheDefect(unittest.TestCase):
    """The negative control: the stateless table is still exactly as blind."""

    def test_the_static_table_still_cannot_see_tech_levels(self):
        """No technology card's stateless price moves when `tech_levels` or
        either of its phase weights does.  True before the fix and after it --
        the fix is in the BOARD path, and this is what says so."""
        base = _w()
        ref = {n: W.card_potential(n, base) for n in TECHS}
        for key in ("tech_levels", "tech_levels_early", "tech_levels_late"):
            w = _w(**{key: base[key] + 7.0})
            for n in TECHS:
                self.assertEqual(W.card_potential(n, w), ref[n],
                                 "%s moved on %s" % (n, key))

    def test_the_static_table_still_prices_a_rate_at_the_bare_weight(self):
        """`science_rate_early` is 2.5 in the defaults and 5.04 on the live 2p
        champion, and the stateless price of a laboratory ignores both."""
        base = _w()
        ref = W.card_potential("Alchemy", base)
        self.assertEqual(W.card_potential("Alchemy",
                                          _w(science_rate_early=9.0)), ref)
        self.assertNotEqual(W.card_potential("Alchemy",
                                             _w(science_rate=9.0)), ref)


class TestFeatureMarginal(unittest.TestCase):
    """`feature_marginal` claims to be d(evaluate)/d(features()[key]).  That is
    a checkable claim, so it is checked numerically against `evaluate` itself
    rather than by reading the code -- the drift guard `docs/HAZARDS.md` asks
    for ("two implementations of one rule always drift").

    `evaluate` takes a precomputed `f`, so the derivative can be taken exactly:
    bump one entry of the feature dict by one and re-evaluate.  The terms that
    are not linear in `f` (`hand_potential`, `row_pressure`, ...) are computed
    from the state both times and cancel.
    """

    def test_it_equals_the_numerical_derivative_of_evaluate(self):
        w = _w(science_rate=1.1, science_rate_early=2.3, science_rate_late=-0.7,
               culture_rate=3.1, culture_rate_early=-0.5, culture_rate_late=2.2,
               tech_levels=4.4, tech_levels_early=1.7, tech_levels_late=-0.9,
               food_rate=0.9, resource_rate=1.3, num_techs=0.6, best_lab=0.8)
        keys = ("science_rate", "culture_rate", "tech_levels", "food_rate",
                "resource_rate", "num_techs", "best_lab", "happy_margin")
        seen = 0
        for seed in (1, 3, 5, 7, 11):
            for plies in (20, 40):
                st = _played(seed, plies=plies)
                for idx in (0, 1):
                    ctx = W.rival_context(st, idx)
                    f = W.features(st, idx, ctx)
                    base = W.evaluate(st, idx, w, ctx, dict(f))
                    for k in keys:
                        f2 = dict(f)
                        f2[k] = f[k] + 1.0
                        seen += 1
                        self.assertAlmostEqual(
                            W.evaluate(st, idx, w, ctx, f2) - base,
                            W.feature_marginal(k, st, idx, w), places=9,
                            msg="seed=%d plies=%d idx=%d key=%s"
                                % (seed, plies, idx, k))
        self.assertGreater(seen, 100)

    def test_strength_is_delegated_rather_than_respelled(self):
        st = _played()
        for idx in (0, 1):
            w = _w()
            self.assertEqual(W.feature_marginal("strength", st, idx, w),
                             W.strength_marginal(st, idx, w))


class TestEveryTechnologyPricesItsLevels(unittest.TestCase):
    """The headline omission: `_card_yields` maps nothing to `tech_levels`, for
    farms, labs, units, temples and special technologies alike.

    FAILS ON THE OLD CODE for all 60-odd of them, on every board and at every
    weight vector in the repo."""

    def test_tech_levels_reaches_the_price_of_every_technology(self):
        st = G.new_game(2, 1)
        p = st.players[0]
        base = _w()
        missed = []
        for n in TECHS:
            if n in p.techs:
                continue            # already developed: dead in hand, and 0.0
            ref = W.card_potential(n, base, st, 0)
            up = W.card_potential(n, _w(tech_levels=base["tech_levels"] + 1.0),
                                  st, 0)
            if abs(up - ref) <= 1e-12:
                missed.append(n)
        self.assertEqual(missed, [], "tech_levels never reaches: %r" % missed)

    def test_an_age_a_technology_is_worth_no_levels(self):
        """`tech_levels` is the AGE LEVEL, so an Age A card contributes 0 and
        the test above must not be passing on a constant."""
        st = G.new_game(2, 1)
        st.players[0].techs.pop("Warriors", None)
        effects.invalidate(st, st.players[0])
        _staff, dev, _sci, _res = BY.tech_upgrade("Warriors", st, 0)
        self.assertEqual(dict((k, a) for k, a, _ in dev)["tech_levels"], 0.0)


class TestTheFix(unittest.TestCase):

    def test_zero_credit_recovers_the_static_answer(self):
        st = _played()
        w = _w(tech_board_credit=0.0)
        checked = 0
        for name in TECHS:
            if C.db().type_by_name[name] in C.UNIT_TYPES:
                continue            # its gate is `unit_tech_credit`
            checked += 1
            self.assertEqual(W.card_potential(name, w, st, 0),
                             W.card_potential(name, w), name)
        self.assertGreater(checked, 30)

    def test_credit_scales_linearly(self):
        st = _played()
        one = W.card_potential("Alchemy", _w(), st, 0)
        half = W.card_potential("Alchemy", _w(tech_board_credit=0.5), st, 0)
        self.assertNotEqual(one, 0.0)
        self.assertAlmostEqual(half * 2.0, one)

    def test_every_yellow_technology_can_price_positive_on_some_board(self):
        """FAILS ON THE OLD CODE for the seven of them that carry a `techCost`,
        on a board where the static table has them all strictly negative.

        The board is a fresh game with extra workers on the starting farm and
        mine, i.e. a player who has been buying an economy -- exactly the
        position the static table cannot tell apart from a player with none.
        What is asserted is that the sign is REACHABLE, not that any particular
        board takes the card.
        """
        st = G.new_game(2, 1)
        p = st.players[0]
        for start in ("Agriculture", "Bronze", "Philosophy"):
            p.techs[start].workers = 2
        effects.invalidate(st, p)
        w = _w()
        rest = [n for n in YELLOW if n not in p.techs]
        priced = {n: W.card_potential(n, w, st, 0) for n in rest}
        self.assertTrue(all(v > 0.0 for v in priced.values()),
                        "still sign-locked: %r" % priced)
        # the paired half: on the SAME board the static table says otherwise
        # for the majority of them.  It is the BOARD that was missing.
        off = dict(w, tech_board_credit=0.0)
        neg = [n for n in rest if W.card_potential(n, off, st, 0) < 0.0]
        self.assertGreaterEqual(len(neg), 4, neg)

    def test_the_price_is_a_board_query_not_a_table(self):
        """The same card, two boards, two prices -- and the direction is the
        one the rules imply: more workers to upgrade, more production bought."""
        w = _w()
        one = G.new_game(2, 1)
        many = G.new_game(2, 1)
        p = many.players[0]
        p.techs["Bronze"].workers += 3
        effects.invalidate(many, p)
        self.assertGreater(W.card_potential("Iron", w, many, 0),
                           W.card_potential("Iron", w, one, 0))

    def test_a_technology_already_developed_is_worth_exactly_nothing(self):
        st = G.new_game(2, 1)
        self.assertIn("Bronze", st.players[0].techs)
        self.assertEqual(BY.tech_upgrade("Bronze", st, 0), ((), (), 0.0, 0.0))
        self.assertEqual(W.card_potential("Bronze", _w(), st, 0), 0.0)

    def test_the_costs_come_from_the_engine(self):
        """`tech_upgrade` may not restate the rules.  Iron off Bronze is
        `actions.upgrade_cost`, not `buildCost`; 3, not 5."""
        st = G.new_game(2, 1)
        p = st.players[0]
        p.techs["Bronze"].workers = 1
        effects.invalidate(st, p)
        _staff, _dev, sci, res = BY.tech_upgrade("Iron", st, 0)
        self.assertEqual(res, float(A.upgrade_cost(st, p, "Bronze", "Iron")))
        self.assertEqual(sci, float(effects.tech_cost(st, p, "Iron")))
        self.assertNotEqual(res, float(C.db().get("Iron")["buildCost"]))

    def test_only_same_type_lower_level_workers_can_move(self):
        """`engine/actions.py:_tableau` only offers an upgrade between cards of
        the SAME type and strictly increasing level, so a farm worker is not
        eligible to become a mine and Iron cannot be upgraded onto Bronze."""
        st = G.new_game(2, 1)
        p = st.players[0]
        for n in ("Agriculture", "Bronze", "Philosophy"):
            p.techs[n].workers = 1
        effects.invalidate(st, p)
        got = dict(BY._upgradable_onto(p, "Iron"))
        self.assertEqual(got, {"Bronze": 1})
        # ... and nothing can move DOWN onto an Age A technology
        p.techs["Iron"] = st.players[0].techs["Bronze"].__class__("Iron",
                                                                  workers=1)
        effects.invalidate(st, p)
        self.assertEqual(BY._upgradable_onto(p, "Bronze"), [])

    def test_the_develop_half_is_what_tech_board_credit_gates_on_a_unit(self):
        """A unit card keeps docs/CARD_BLINDNESS.md's board price at
        `tech_board_credit` = 0.0 and gains exactly the develop half at 1.0 --
        which is what makes ONE constant recover the parent commit's pricing
        on all 236 cards."""
        st = G.new_game(2, 1)
        off = W.card_potential("Swordsmen", _w(tech_board_credit=0.0), st, 0)
        on = W.card_potential("Swordsmen", _w(), st, 0)
        _staff, dev, _sci, _res = BY.tech_upgrade("Swordsmen", st, 0)
        want = sum(a * W.feature_marginal(k, st, 0, _w()) for k, a, _ in dev)
        self.assertGreater(want, 0.0)
        self.assertAlmostEqual(on - off, want)


class TestRowPressureCanSeeAYellowTechnology(unittest.TestCase):
    """The second half of the mechanism, and the one that is not about
    magnitude at all: `row_pressure` skips any card whose `card_potential` is
    <= 0, so a strictly-negative laboratory was invisible to `row_urgency` and
    `row_bargain_forgone` at ANY weight."""

    def test_a_lab_in_the_row_reaches_row_urgency_only_once_positive(self):
        st = G.new_game(2, 1)
        st.card_row[0] = "Alchemy"
        ctx = W.rival_context(st, 0, root_row=tuple(st.card_row))
        off = _w(row_urgency=1.0, row_bargain_forgone=1.0,
                 tech_board_credit=0.0,
                 # the live 2p champion's shape: culture is king and science
                 # is priced below the science it costs
                 culture_rate=31.7, science_rate=0.25, science=0.33,
                 resource_stock=1.73)
        on = dict(off, tech_board_credit=1.0)
        self.assertLess(W.card_potential("Alchemy", off, st, 0), 0.0)
        self.assertGreater(W.card_potential("Alchemy", on, st, 0), 0.0)
        u_off, _ = W.row_pressure(st, 0, off, ctx)
        u_on, _ = W.row_pressure(st, 0, on, ctx)
        self.assertAlmostEqual(
            u_on - u_off, W.card_potential("Alchemy", on, st, 0))


class TestTheCacheIsKeyedCompletely(unittest.TestCase):
    """`tech_upgrade` memoises on `(name, effects.stats_key(state, p))`.  A key
    that missed a field would hand out silently stale prices -- the failure
    mode `tests/test_board_yields.py:TestStatsKeyIsACompleteMemoKey` guards for
    the swap diff, restated for this cache."""

    def test_moving_a_worker_changes_the_key_and_the_price(self):
        st = G.new_game(2, 1)
        p = st.players[0]
        first = BY.tech_upgrade("Iron", st, 0)
        p.techs["Bronze"].workers += 3
        effects.invalidate(st, p)
        second = BY.tech_upgrade("Iron", st, 0)
        self.assertNotEqual(first, second)
        self.assertGreater(second[3], first[3])       # more resources owed


if __name__ == "__main__":
    unittest.main()
