"""The "build one fresh" plan, and the three cards that have no other one.

THE DEFECT, stated as the property that failed
----------------------------------------------
`docs/OPEN_ITEMS.md` §2 item 21: `board_yields.tech_upgrade` answers exactly
one question -- *"develop it and upgrade the workers I already have"* -- so a
technology of a type the player has never staffed was priced at its levels
minus its science and nothing else.  §2 item 28 is the sharpest instance:
**Knights, Cannon and Air Forces are the lowest card of their own type in the
base game**, so `_upgradable_onto` is empty for them on *every* board that
will ever exist and no board can ever offer them an upgrade to ride.  Under
`DEFAULT_WEIGHTS` on a fresh 2p board Knights therefore priced at **-0.28**,
i.e. inside `weighted.row_pressure`'s `if val <= 0.0: continue`, and air
technologies were taken **0.00** times a seat-game against a human 0.65.

`test_the_gateway_cards_are_the_negative_control` is the NEGATIVE CONTROL for
the whole file, in the sense `tests/test_unit_pricing.py` uses the term: with
`build_fresh_credit` at 0.0 it asserts the defect is still there, so nothing
below can pass by the hole having quietly closed for an unrelated reason.

WHAT IS PINNED, AND WHAT IS DELIBERATELY NOT
--------------------------------------------
Pinned: that the plan is the ENGINE's move (`("build", name)` is generated
exactly when `build_fresh` reports a plan, and costs exactly what it says);
that every triple it emits is a feature `weighted.features()` really moves by
that amount when the plan is played out for real; that the free worker and
the `uprising` cliff are charged; that `build_fresh_credit` = 0.0 makes the
branch unreachable.

Not pinned: any take rate, and no claim that a build is the right move on any
particular board.  That is a judgement the weights make -- and the measured
one is that it is a BAD judgement today: `build_fresh_credit` ships at 0.0
because turning it up is a ~5.5pp paired loss on two independent vectors
(docs/CARD_BLINDNESS.md 14.9.6).  Every test below therefore passes its own
credit explicitly rather than relying on `DEFAULT_WEIGHTS`, and none of them
would start failing if the league later climbed that constant.
"""
import random
import unittest

from engine import actions as A, cards as C, economy, effects, game as G
from engine.bots import board_yields as BY
from engine.bots import weighted as W
from engine.state import TechCard


#: the three cards with no possible upgrade, read off the card database rather
#: than off the code under test, for the same reason `tests/test_unit_pricing`
#: builds its own list: this file has to be droppable onto a reverted tree.
def _lowest_of_its_type():
    db = C.db()
    lo = {}
    for c in db.cards:
        typ = c["type"]
        if typ not in C.UNIT_TYPES:
            continue
        cur = lo.get(typ)
        if cur is None or db.level_of(c["name"]) < db.level_of(cur):
            lo[typ] = c["name"]
    return lo


def _gateways(state, idx=0):
    """The unit types whose lowest card is still TAKEABLE -- item 28's three.

    Infantry drops out and that is the point: its lowest card is Warriors,
    which every player starts holding, so an infantry technology always has a
    worker able to upgrade onto it.  Cavalry, artillery and air do not, and
    excluding infantry by "the player already has the bottom of that ladder"
    rather than by name keeps this reading off the board and the card
    database instead of off a hand-written list of three.
    """
    have = state.players[idx].techs
    return {t: n for t, n in _lowest_of_its_type().items() if n not in have}


#: every technology that prints a `buildCost`, i.e. every card this plan can
#: possibly apply to.
BUILDABLE = [c["name"] for c in C.db().cards
             if c.get("buildCost") is not None]


def _w(**over):
    return dict(W.DEFAULT_WEIGHTS, **over)


def _fresh(seed=0, players=2):
    return G.new_game(players, seed)


def _played(seed=5, plies=60):
    """A mid-game board, so nothing here is only true on turn one."""
    st = G.new_game(2, seed)
    rng = random.Random(seed)
    bot = W.WeightedBot(seed=seed)
    for _ in range(plies):
        if st.game_over:
            break
        A.apply(st, bot.pick(st, A.legal_moves(st)), rng)
    return st


class TestTheEngineOffersExactlyThisMove(unittest.TestCase):
    """The plan is `engine/actions.py`'s, not a description of it."""

    def test_a_plan_means_the_engine_generates_the_build(self):
        """Develop the card, pay it what it asks for, and `legal_moves` has
        `("build", name)` in it.

        `build_fresh` deliberately does NOT gate on affordability (see its
        docstring: `unit_upgrade` charges `upgrade_cost` whether or not the
        treasury covers it, and a price that flickered with the resource pool
        would be unlearnable), so the resources and the action are supplied
        here before the legality claim is checked.
        """
        st = _fresh()
        p = st.players[0]
        st.round = 3                    # RULES_SPEC 1.9: round 1 is take-only
        checked = 0
        for name in BUILDABLE:
            if name in p.techs:
                continue
            triples, res = BY.build_fresh(name, st, 0)
            if not triples:
                continue
            probe = G.new_game(2, 0)
            probe.round = 3
            q = probe.players[0]
            q.techs[name] = TechCard(name, workers=0)
            q.resources = 30
            q.military_actions = 3
            q.civil_actions = 4
            effects.invalidate(probe, q)
            self.assertIn(("build", name), A.legal_moves(probe),
                          f"{name}: priced a plan the engine never offers")
            checked += 1
        self.assertGreater(checked, 8, "nothing was actually checked")

    def test_no_free_worker_means_no_plan(self):
        """`_action_moves` guards every build with `if p.workers_free > 0`."""
        st = _fresh()
        p = st.players[0]
        self.assertGreater(p.workers_free, 0)
        with_worker = [n for n in BUILDABLE
                       if n not in p.techs and BY.build_fresh(n, st, 0)[0]]
        self.assertTrue(with_worker)
        p.workers_free = 0
        for n in with_worker:
            self.assertEqual(BY.build_fresh(n, st, 0), ((), 0.0),
                             f"{n}: priced a build with no free worker")

    def test_the_urban_limit_kills_the_plan(self):
        """`_action_moves` skips an urban build once that urban TYPE already
        stands at `Stats.urban_limit`."""
        st = _fresh()
        p = st.players[0]
        p.workers_free = 4
        lab = next(n for n in BUILDABLE
                   if C.db().type_of(n) == "lab" and n not in p.techs)
        self.assertTrue(BY.build_fresh(lab, st, 0)[0])
        limit = effects.state_stats(st, p).urban_limit
        # stand `limit` workers on some other lab-type technology
        other = next(n for n in BUILDABLE
                     if C.db().type_of(n) == "lab" and n != lab)
        p.techs[other] = TechCard(other, workers=limit)
        p.workers_free = 4
        effects.invalidate(st, p)
        self.assertEqual(BY.build_fresh(lab, st, 0), ((), 0.0))

    def test_an_already_developed_card_has_no_plan(self):
        st = _fresh()
        p = st.players[0]
        for n in p.techs:
            self.assertEqual(BY.build_fresh(n, st, 0), ((), 0.0))

    def test_a_non_technology_has_no_plan(self):
        st = _fresh()
        for n in ("Michelangelo", "Monarchy", "Pyramids", "Rich Land"):
            self.assertEqual(BY.build_fresh(n, st, 0), ((), 0.0))


class TestThePriceIsWhatFeaturesActuallyMove(unittest.TestCase):
    """The strong form: play the plan out and diff `weighted.features()`.

    This is the standard `docs/GOVERNMENT_PRICING.md` set for the revolution
    burn -- apply the move and require the priced amount to equal the feature
    the evaluator really loses -- applied to all four features a build moves
    that no `Stats` diff can see (`free_workers`, `workers`,
    `<class>_workers`, `uprising`) plus everything the diff can.
    """

    def _one(self, name, seed=0):
        """features(develop+build) - features(develop), against the price."""
        base = G.new_game(2, seed)
        b = base.players[0]
        b.workers_free = max(2, b.workers_free)
        b.resources = 30
        b.military_actions = 3
        b.civil_actions = 4
        b.techs[name] = TechCard(name, workers=0)
        effects.invalidate(base, b)
        before = W.features(base, 0, W.DEFAULT_WEIGHTS)

        # the price, asked of a board that has NOT yet developed the card
        pre = G.new_game(2, seed)
        q = pre.players[0]
        q.workers_free = max(2, q.workers_free)
        q.resources = 30
        q.military_actions = 3
        q.civil_actions = 4
        effects.invalidate(pre, q)
        triples, res = BY.build_fresh(name, pre, 0)

        A.do_build(base, b, name)
        effects.invalidate(base, b)
        after = W.features(base, 0, W.DEFAULT_WEIGHTS)
        return triples, res, before, after

    def test_every_triple_is_the_real_feature_delta(self):
        """Every amount priced is the amount `features()` really moves.

        `happy_margin` is the one declared exception and it is not this
        plan's: `features()` computes `min(3, margin)` while
        `board_yields._delta_triples` maps `Stats.happy` onto it linearly, so
        a happy face bought at margin 3 is priced and not delivered.  That is
        `docs/OPEN_ITEMS.md` §2 item 23, it is inherited from the leader swap
        diff and the upgrade path, and it is measured by
        `test_the_happy_clamp_is_item_23_and_nothing_new` below rather than
        excused here.
        """
        checked = 0
        for name in BUILDABLE:
            triples, res, before, after = self._one(name)
            if not triples:
                continue
            for key, amt, _kind in triples:
                got = after[key] - before[key]
                if key == "happy_margin" and got < amt:
                    continue                      # item 23, pinned below
                self.assertAlmostEqual(
                    amt, got, 6,
                    f"{name}: priced {key} at {amt}, features() moved {got}")
            checked += 1
        self.assertGreater(checked, 20, "nothing was actually checked")

    #: Features a real build moves that these triples do NOT carry.  A
    #: RATCHET, not an excuse list: it is asserted to be exact, so a new hole
    #: fails the test and so does closing one without deleting its entry.
    #:
    #:   strength_rel / strength_deficit / strength_lead
    #:       priced, through `weighted.strength_marginal`, which is what
    #:       `feature_marginal("strength", ...)` delegates to -- the board
    #:       expresses one point of army through four features and that
    #:       function is the one place they are summed.  Not a hole.
    #:   blue_free / corruption_loss
    #:       A REAL HOLE, and an INHERITED one: `effects.blue_available`
    #:       counts the blue tokens your food and resource banks are standing
    #:       on, and a higher-level farm or mine holds more per token, so
    #:       staffing one frees blue tokens and cuts `economy.corruption`.
    #:       Neither is a `Stats` field, so `_delta_triples` cannot see it and
    #:       the UPGRADE path has the identical hole today (upgrading
    #:       Bronze -> Iron moves `blue_free` 8 -> 13 and `corruption_loss`
    #:       2 -> 0, both priced at nothing).  Deliberately not fixed here so
    #:       this lane's digest moves have one cause; docs/OPEN_ITEMS.md §2
    #:       item 30.
    UNPRICED = frozenset({"strength_rel", "strength_deficit", "strength_lead",
                          "blue_free", "corruption_loss"})

    #: Not this plan's to price, and said so in `build_fresh`'s docstring:
    #: the resources come back separately as a cost, and the action is
    #: deliberately uncharged because the upgrade plan it competes with pays
    #: exactly the same one action per worker and does not charge it either.
    NOT_MINE = frozenset({"resource_stock", "ma_left", "ca_left",
                          "take_cost_paid"})

    def test_nothing_the_build_moves_is_left_unpriced(self):
        """The other direction, which is the one that catches a MISSING term.

        Deleting `_build_triples`' `free_workers` / `workers` /
        `<class>_workers` / `uprising` entries fails here and nowhere else.
        """
        seen = set()
        checked = 0
        for name in BUILDABLE:
            triples, res, before, after = self._one(name)
            if not triples:
                continue
            priced = {k for k, _a, _kd in triples}
            for key, val in after.items():
                if key in self.NOT_MINE or key in priced:
                    continue
                if abs(before.get(key, 0.0) - val) > 1e-9:
                    seen.add(key)
            checked += 1
        self.assertGreater(checked, 20, "nothing was actually checked")
        self.assertEqual(seen - set(self.UNPRICED), set(),
                         "a channel this plan moves is priced at nothing")

    def test_the_happy_clamp_is_item_23_and_nothing_new(self):
        """The clamp over-prices, never under-prices, and only at the clamp.

        Pinned so item 23 is a measured quantity rather than a remark:
        Professional Sports buys a happy face this board cannot use, priced
        at 4.0 where `features()` moves 3.0.
        """
        triples, _res, before, after = self._one("Professional Sports")
        amt = next(a for k, a, _kd in triples if k == "happy_margin")
        self.assertEqual(amt, 4.0)
        self.assertEqual(after["happy_margin"] - before["happy_margin"], 3.0)
        self.assertEqual(after["happy_margin"], 3.0)      # the clamp itself

    def test_the_resource_cost_is_what_do_build_charges(self):
        checked = 0
        for name in BUILDABLE:
            triples, res, before, after = self._one(name)
            if not triples:
                continue
            spent = before["resource_stock"] - after["resource_stock"]
            self.assertAlmostEqual(res, spent, 6, name)
            checked += 1
        self.assertGreater(checked, 20)


class TestTheUprisingCliff(unittest.TestCase):
    """Staffing your LAST free worker while in discontent is a catastrophe the
    rules already describe (production is skipped entirely), and `features()`
    prices it at -12.0.  A build that crosses that threshold must read as a
    loss, and this is why the plan cannot be a constant."""

    def _discontented(self):
        """A board sitting exactly ON the threshold, not merely near it.

        `features()`: `uprising = discontent > p.workers_free`.  With
        `yellow_bank` 13 the requirement is 1 happy face (RULES_SPEC §6.1) and
        a fresh board produces 0, so discontent is 1: not an uprising while
        one free worker is standing spare, and an uprising the moment that
        worker is spent.  Constructed rather than searched for, and the two
        numbers are asserted below so the probe cannot rot into a board where
        the threshold is never crossed.
        """
        st = _fresh()
        p = st.players[0]
        p.yellow_bank = 13
        p.workers_free = 1
        effects.invalidate(st, p)
        s = effects.state_stats(st, p)
        req = economy.happy_required(p.yellow_bank)
        self.assertEqual(req - s.happy, 1)
        self.assertEqual(p.workers_free, 1)
        return st, p

    def test_the_cliff_is_priced_and_it_is_a_loss(self):
        st, p = self._discontented()
        unit = next(n for n in BUILDABLE
                    if C.db().type_of(n) in C.UNIT_TYPES and n not in p.techs)
        triples, _res = BY.build_fresh(unit, st, 0)
        self.assertIn("uprising", {k for k, _a, _kd in triples},
                      "the last free worker went onto a unit for free")
        amt = next(a for k, a, _kd in triples if k == "uprising")
        self.assertEqual(amt, 1.0)
        self.assertLess(W.DEFAULT_WEIGHTS["uprising"], 0.0)

    def test_the_cliff_is_the_feature_features_really_reads(self):
        st, p = self._discontented()
        unit = next(n for n in BUILDABLE
                    if C.db().type_of(n) in C.UNIT_TYPES and n not in p.techs)
        before = W.features(st, 0, W.DEFAULT_WEIGHTS)
        p.techs[unit] = TechCard(unit, workers=0)
        p.resources = 30
        p.military_actions = 3
        effects.invalidate(st, p)
        A.do_build(st, p, unit)
        effects.invalidate(st, p)
        after = W.features(st, 0, W.DEFAULT_WEIGHTS)
        self.assertEqual(after["uprising"] - before["uprising"], 1.0)


class TestTheCredit(unittest.TestCase):

    def test_credit_zero_makes_the_branch_unreachable(self):
        """0.0 is the one constant that recovers the parent commit's pricing,
        and this is the structural half of that claim: with the credit at 0.0
        `card_potential` never calls `build_fresh` at all, on any of the 236
        cards, on a fresh board or a played one."""
        real = BY.build_fresh
        calls = []

        def boom(name, state, idx):
            calls.append(name)
            raise AssertionError("build_fresh reached at credit 0.0")

        off = _w(build_fresh_credit=0.0)
        for st in (_fresh(), _played()):
            BY.build_fresh = boom
            try:
                for c in C.db().cards:
                    W.card_potential(c["name"], off, st, 0)
            finally:
                BY.build_fresh = real
            self.assertEqual(calls, [])

    def test_the_credit_is_linear_where_the_plan_wins(self):
        st = _fresh()
        for name in _gateways(st).values():
            base = W.card_potential(name, _w(build_fresh_credit=0.0), st, 0)
            one = W.card_potential(name, _w(build_fresh_credit=1.0), st, 0)
            half = W.card_potential(name, _w(build_fresh_credit=0.5), st, 0)
            self.assertGreater(one, base, name)
            self.assertAlmostEqual(half - base, (one - base) / 2.0, 6, name)

    def test_the_plan_is_a_max_and_never_a_penalty(self):
        """`tech_value` takes the better of the two staffing plans, so turning
        the credit up can never LOWER a price."""
        for st in (_fresh(), _played()):
            for c in C.db().cards:
                n = c["name"]
                self.assertGreaterEqual(
                    W.card_potential(n, _w(build_fresh_credit=1.0), st, 0)
                    - W.card_potential(n, _w(build_fresh_credit=0.0), st, 0),
                    -1e-9, n)


class TestTheGatewayCards(unittest.TestCase):
    """Knights, Cannon and Air Forces: `docs/OPEN_ITEMS.md` §2 item 28."""

    def test_they_really_have_no_upgrade_on_any_board(self):
        """The premise, checked against the card database rather than
        asserted: each is the lowest level of its own type, so
        `_upgradable_onto` is empty for it whatever the player holds."""
        db = C.db()
        for typ, name in _gateways(_fresh()).items():
            st = _fresh()
            p = st.players[0]
            for c in db.cards:
                if c["type"] == typ and c["name"] != name:
                    p.techs[c["name"]] = TechCard(c["name"], workers=2)
            effects.invalidate(st, p)
            self.assertEqual(BY._upgradable_onto(p, name), [],
                             f"{name} is not the lowest {typ} after all")

    def test_the_gateway_cards_are_the_negative_control(self):
        """With the credit off, the hole item 28 describes is still there:
        the whole price of a gateway card is the develop half against its
        science, and Knights lands below zero -- inside `row_pressure`'s
        `val <= 0.0` skip."""
        st = _fresh()
        off = _w(build_fresh_credit=0.0)
        self.assertLess(W.card_potential("Knights", off, st, 0), 0.0)

    def test_all_three_are_positive_once_the_plan_is_priced(self):
        st = _fresh()
        on = _w(build_fresh_credit=1.0)
        for name in _gateways(st).values():
            self.assertGreater(W.card_potential(name, on, st, 0), 0.0, name)

    def test_a_gateway_card_is_visible_to_row_pressure(self):
        """The second half of the defect: `row_pressure` skips any card whose
        `card_potential` is <= 0.0, so a negative price does not merely
        under-value a card, it hides it."""
        st = _fresh()
        st.card_row[0] = "Knights"
        on = _w(build_fresh_credit=1.0, row_urgency=1.0)
        off = _w(build_fresh_credit=0.0, row_urgency=1.0)
        self.assertGreater(W.row_pressure(st, 0, on)[0],
                           W.row_pressure(st, 0, off)[0])


if __name__ == "__main__":
    unittest.main()
