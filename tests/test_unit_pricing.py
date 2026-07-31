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
from engine.state import TechCard


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


def _action_board(seed=5):
    """A played 2p board stopped at a state where the acting player really is
    choosing an action -- no pending choice, and the engine offers moves other
    than a take.

    Constructed rather than assumed: `_played(seed, plies=30)` lands wherever
    self-play lands, which moves whenever any pricing changes, and round 1 is
    take-only by RULES_SPEC 1.9.
    """
    st = G.new_game(2, seed)
    rng = random.Random(seed)
    bot = W.WeightedBot(seed=seed)
    for _ in range(200):
        if st.game_over:
            break
        if not st.pending and st.round > 1 and \
                any(m[0] == "build" or m[0] == "develop"
                    for m in A.legal_moves(st)):
            return st, st.current
        A.apply(st, bot.pick(st, A.legal_moves(st)), rng)
    raise AssertionError("no action-phase board found")


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
        docs/CARD_BLINDNESS.md section 11.5.1 is the measurement that no
        setting of `unit_strength_credit` flips the sign either.

        Each card is asked on a board where the engine would actually offer
        the upgrade: four workers standing on the lower-level card of its OWN
        type, because `("upgrade", lo, hi)` is same-type-only (`_action_moves`
        via `_tableau`'s `higher`).  Until 2026-07-31 this test stacked four
        Warriors and asked all ten, which passed for the wrong reason -- the
        price pooled infantry workers onto the Cannon.  What is asserted is
        that the sign is REACHABLE, not that any particular board takes the
        card.

        THE GATEWAY CARDS.  Knights, Cannon and Air Forces are the lowest card
        of their own type in the whole deck, so no board can ever offer an
        upgrade onto them -- the only route to a cavalry unit is to BUILD one
        fresh, which is docs/OPEN_ITEMS.md section 2 items 21 and 28.  That
        plan is priced by `board_yields.build_fresh` since 2026-07-31, but
        `build_fresh_credit` ships at 0.0, so `_w()` here is still the
        upgrade-only price.  They are therefore checked one property weaker:
        the
        sign must not be structurally locked (a vector that values a
        technology level flips them), which is the thing
        `unit_strength_credit` could never do.
        """
        w = _w()
        db = C.db()
        type_of, level_of = db.type_by_name, db.level_by_name
        priced = {}
        gateway = []
        for name in UNITS:
            st = G.new_game(2, 1)
            p = st.players[0]
            lower = [n for n in UNITS
                     if type_of[n] == type_of[name]
                     and level_of[n] < level_of[name]]
            if lower:
                # the highest one below it: the position of a player who has
                # been buying that arm of the army all game
                lo = max(lower, key=lambda n: level_of[n])
                if lo not in p.techs:
                    p.techs[lo] = TechCard(lo)
                p.techs[lo].workers = 4
                effects.invalidate(st, p)
            elif name not in p.techs:
                gateway.append(name)
            priced[name] = W.card_potential(name, w, st, 0)
        # Warriors is in `game.START_TECHS`, so it is already developed and
        # the card is dead in hand: exactly 0.0, neither cost nor gain.
        self.assertEqual(priced["Warriors"], 0.0)
        self.assertEqual(sorted(gateway), ["Air Forces", "Cannon", "Knights"])
        staffable = [n for n in UNITS
                     if n != "Warriors" and n not in gateway]
        self.assertTrue(all(priced[n] > 0.0 for n in staffable),
                        "still sign-locked: %r" % priced)
        # every one of the four red TYPES reaches a positive price, which is
        # the property `row_pressure`'s `val <= 0.0` skip is about
        best = {}
        for name in UNITS:
            typ = type_of[name]
            best[typ] = max(best.get(typ, -1e9), priced[name])
        self.assertEqual(sorted(best), sorted(C.UNIT_TYPES))
        self.assertTrue(all(v > 0.0 for v in best.values()), best)
        # the gateway three are a weights judgement, not a sign lock: one
        # technology level is worth 1.5 eval points on DEFAULT_WEIGHTS and
        # 9.23 on the live 2p champion, and at the higher number they are
        # positive with nothing else changed
        rich = _w(tech_levels=3.0)
        for name in gateway:
            self.assertGreater(W.card_potential(name, rich,
                                                G.new_game(2, 1), 0), 0.0,
                               name)
        # ...and the same nine cards on the same boards under the static table
        # are every one of them negative.  This is the paired half: it is the
        # BOARD that was missing, not the weights.
        off = dict(w, unit_tech_credit=0.0)
        for name in [n for n in UNITS if n != "Warriors"]:
            st = G.new_game(2, 1)
            self.assertLess(W.card_potential(name, off, st, 0), 0.0, name)

    def test_a_warriors_worker_cannot_become_a_cannon(self):
        """THE NEGATIVE CONTROL for the same-type fix, and it fails on the
        parent tree (`d15cb5b`), where four Warriors bought a Cannon 8
        strength and were charged `upgrade_cost` four times for a move
        `engine/actions.py:_action_moves` never generates.

        The rule is checked against the ENGINE rather than restated: whatever
        `legal_moves` offers on this board is what may be priced.  (Round 1 is
        take-only by RULES_SPEC 1.9, hence a played board, and an upgrade
        target has to be DEVELOPED before the engine will offer it, hence the
        two explicit `TechCard`s.)
        """
        st, idx = _action_board(5)
        p = st.players[idx]
        for name in ("Swordsmen", "Knights"):
            if name not in p.techs:
                p.techs[name] = TechCard(name)
            p.techs[name].workers = 0
        p.techs["Warriors"].workers = 4
        p.science = 99
        p.resources = 99
        p.military_actions = max(p.military_actions, 2)
        effects.invalidate(st, p)
        pairs = {(m[1], m[2]) for m in A.legal_moves(st) if m[0] == "upgrade"}
        self.assertIn(("Warriors", "Swordsmen"), pairs)
        self.assertNotIn(("Warriors", "Knights"), pairs)
        self.assertFalse([1 for lo, hi in pairs
                          if C.db().type_by_name[lo]
                          != C.db().type_by_name[hi]])
        # so the cavalry card one level up from the Knights this player holds
        # -- with no cavalry WORKER to move -- has no staffing half at all:
        # science only, no strength bought and no resources spent.  On the
        # parent tree the four Warriors are pooled onto it and it reads
        # `(8.0, 6.0, 12.0)`.
        gained, sci, res = BY.unit_upgrade("Cavalrymen", st, idx)
        self.assertEqual((gained, res), (0.0, 0.0))
        self.assertGreater(sci, 0.0)
        # ...while the infantry card those workers CAN legally reach does
        gained, _sci, res = BY.unit_upgrade("Riflemen", st, idx)
        self.assertGreater(gained, 0.0)
        self.assertGreater(res, 0.0)

    def test_the_price_charges_exactly_the_pairs_the_engine_offers(self):
        """The same claim as an EQUIVALENCE over all 90 ordered pairs of unit
        technologies, constructed rather than waited for.

        A sweep over played boards was tried first and is not good enough: it
        only sees the pairs this week's policy happens to develop, so a
        pricing change elsewhere re-rolls it (docs/HAZARDS.md, "four sampling
        tests re-rolled").  Here every pair is built.
        """
        db = C.db()
        seen = 0
        for lo in UNITS:
            for hi in UNITS:
                if lo == hi:
                    continue
                st, idx = _action_board(5)
                p = st.players[idx]
                for n in list(p.techs):
                    if db.type_by_name[n] in C.UNIT_TYPES:
                        p.techs[n].workers = 0
                for n in (lo, hi):
                    if n not in p.techs:
                        p.techs[n] = TechCard(n)
                p.techs[lo].workers = 2
                p.techs[hi].workers = 0
                p.science = 99
                p.resources = 99
                p.military_actions = max(p.military_actions, 2)
                effects.invalidate(st, p)
                offered = ((lo, hi) in
                           {(m[1], m[2]) for m in A.legal_moves(st)
                            if m[0] == "upgrade"})
                charged = lo in dict(BY._upgradable_onto(p, hi))
                self.assertEqual(offered, charged,
                                 "%s -> %s: engine %s, price %s"
                                 % (lo, hi, offered, charged))
                seen += 1
        self.assertEqual(seen, len(UNITS) * (len(UNITS) - 1))

    def test_the_price_is_a_board_query_not_a_table(self):
        """The same card, two boards, two prices -- and the direction is the
        one the rules imply: more workers to move, more strength bought.

        SIX workers, not three, and the number is derived rather than tuned
        up until it passed.  `tech_value` takes the better of two staffing
        plans since `build_fresh_credit` landed; at the shipped default of 0.0
        three workers is enough, but at 1.0 a fresh 2p board's one spare
        worker makes "build one Swordsman" beat "upgrade my Warriors" until
        the Warriors outnumber it -- 1 to 4 workers price identically, 6 puts
        the upgrade plan back in front.  Six therefore asserts the same claim
        at either setting of the credit, which is what a test of the UPGRADE
        branch should do.
        """
        w = _w()
        one = G.new_game(2, 1)
        many = G.new_game(2, 1)
        p = many.players[0]
        p.techs["Warriors"].workers = 6
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
