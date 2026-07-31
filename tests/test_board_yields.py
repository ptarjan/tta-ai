"""Board-aware card pricing: engine/bots/board_yields.py.

Three separate things are being defended here and they fail in different
ways, so they are separate classes:

* `TestTheComputeVsStateStatsTrap` -- the swap diff must call
  `effects.compute`, never `effects.state_stats`.  Getting this wrong makes
  every leader price at exactly zero, silently.
* `TestStatsKeyIsACompleteMemoKey` -- the memo key is
  `(name, effects.stats_key(state, p))`.  If `stats_key` omitted a field
  `compute` reads, the cache would serve stale valuations, which is worse
  than the blindness this module fixes.  Checked empirically against real
  self-play rather than by reading the docstring.
* `TestEveryLeaderIsPriced` -- the point of the exercise: no leader may be
  worth nothing but its leader-ness on a board that contains the things it
  pays for.
"""
import random
import unittest

from engine import actions as A, cards as C, effects, game as G
from engine.state import TechCard
from engine.bots import WeightedBot, board_yields as BY, weighted as W


def _played(players=2, seed=7, plies=60):
    """A mid-game state, reached by actually playing."""
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


class TestTheComputeVsStateStatsTrap(unittest.TestCase):
    """`state_stats` is a cache keyed on `p.idx` and validated only when the
    entry is marked dirty.  Assigning `p.leader` does not mark it dirty, so
    `state_stats` after the assignment returns the OLD leader's stats and
    every diff comes out zero.  `compute` bypasses the cache.

    This is the single easiest way to break this module and it breaks it
    silently, so it gets a test that reproduces the trap directly rather
    than a comment."""

    def test_state_stats_would_return_the_stale_answer(self):
        st = _played()
        p = st.players[0]
        effects.state_stats(st, p)           # prime the cache
        old = p.leader
        p.leader = "Winston Churchill"
        try:
            stale = effects.state_stats(st, p)
            fresh = effects.compute(st, p)
        finally:
            p.leader = old
        self.assertNotEqual(
            stale.__dict__, fresh.__dict__,
            "if these are ever equal this test has stopped testing "
            "anything -- pick a leader whose effects show up in Stats")

    def test_the_swap_restores_the_leader_and_leaves_the_cache_valid(self):
        st = _played()
        p = st.players[0]
        before = dict(effects.state_stats(st, p).__dict__)
        lead = p.leader
        BY.board_yields("Winston Churchill", st, 0)
        self.assertEqual(p.leader, lead)
        self.assertEqual(effects.state_stats(st, p).__dict__, before)
        self.assertEqual(effects.compute(st, p).__dict__, before)

    def test_the_swap_is_exception_safe(self):
        """`try/finally`, not a bare restore: a raise mid-swap would leave
        the player holding a leader they never took."""
        st = _played()
        p = st.players[0]
        lead = p.leader
        with self.assertRaises(KeyError):
            BY._swapped(st, p, "leader", "No Such Leader")
        self.assertEqual(p.leader, lead)


class TestStatsKeyIsACompleteMemoKey(unittest.TestCase):
    """The memo is keyed on `effects.stats_key`, whose docstring asserts it
    names every field `compute` reads.  Do not trust the docstring: play
    games, collect every `stats_key -> Stats` pair, and fail if one key ever
    maps to two different `Stats`."""

    def test_one_key_never_maps_to_two_different_stats(self):
        seen = {}
        for seed in (1, 2, 3, 4, 5):
            for players in (2, 3):
                st = G.new_game(players, seed)
                rng = random.Random(seed)
                bots = [WeightedBot(seed=seed + i) for i in range(players)]
                for _ in range(120):
                    if st.game_over:
                        break
                    A.apply(st, bots[st.decider()].pick(st, A.legal_moves(st)),
                            rng)
                    for p in st.players:
                        # the hypothetical side of the diff too, not just the
                        # real board: that is what the memo actually keys
                        for lead, gov in (
                                (None, None),
                                ("Winston Churchill", None),
                                ("Sid Meier", None),
                                ("Napoleon Bonaparte", None),
                                (None, "Republic"),
                                (None, "Fundamentalism")):
                            old_l, old_g = p.leader, p.government
                            if lead is not None:
                                p.leader = lead
                            if gov is not None:
                                p.government = gov
                            try:
                                k = (lead, gov, effects.stats_key(st, p))
                                v = tuple(
                                    sorted(effects.compute(st, p).__dict__
                                           .items(), key=lambda kv: kv[0]))
                            finally:
                                p.leader, p.government = old_l, old_g
                            v = repr(v)
                            prev = seen.setdefault(k, v)
                            self.assertEqual(
                                prev, v,
                                "effects.stats_key is NOT a complete key for "
                                "everything effects.compute reads, so the "
                                "board_yields memo would serve stale card "
                                f"valuations.  key={k!r}")
        self.assertGreater(len(seen), 900, "not enough coverage to mean much")


class TestTheTripleShapeAgrees(unittest.TestCase):
    """`board_yields` spells the gain/cost kind constants itself rather than
    importing them from `weighted` (which imports it).  Two spellings of one
    constant is a drift hazard, so it gets a test rather than a comment."""

    def test_the_kind_constants_match(self):
        self.assertEqual(BY._GAIN, W._Y_GAIN)
        self.assertEqual(BY._COST, W._Y_COST)

    def test_every_feature_emitted_has_a_weight(self):
        """A triple naming a feature `DEFAULT_WEIGHTS` does not contain is
        silently skipped by `card_potential`, i.e. the same drop this whole
        module exists to stop, one level down."""
        st = _played()
        for name in C.db().by_name:
            for triples in (BY.board_yields(name, st, 0) or (),
                            BY.board_extra(name, st, 0)):
                for k, _a, _c in triples:
                    self.assertIn(k, W.DEFAULT_WEIGHTS, f"{name}: {k}")


class TestTheTypeKnobIsAWeightNow(unittest.TestCase):
    """`TTA_BOARD_TYPES` used to decide which card types were board-priced,
    at import, from the environment -- so the decomposition the A/B needed
    was available to a human running a command and to nobody else.  It is
    four weights now (`card_board_leader` / `_government` / `_action` /
    `_wonder`), offsets on the shared `card_board_credit`, which is what lets
    `hillclimb_league` fit the government half rather than be told it.

    The arms the old variable expressed must still be expressible, exactly,
    or the numbers in docs/CARD_PRICING_LEADERS.md stop being comparable to
    anything measurable today.
    """

    def _priced_as_diff(self, name, w, st):
        """Is `name` being priced by the swap diff, or off the static table?

        Compared against `_card_yields` rather than against a constant: the
        two answers differ for every leader and government in the deck, which
        is the whole finding, so this cannot silently become vacuous."""
        static = W._sum_yields(W._card_yields(name), w,
                               w.get("card_rate_credit", 1.0))
        return abs(W.card_potential(name, w, st, 0) - static) > 1e-9

    def test_the_shipped_default_prices_nothing_on_the_board(self):
        st = _played()
        w = dict(W.DEFAULT_WEIGHTS)
        for name in ("Michelangelo", "Republic"):
            self.assertFalse(self._priced_as_diff(name, w, st), name)

    def test_the_credit_alone_still_means_every_type(self):
        """`card_board_credit` = 1.0 with no offsets is the aggregate arm."""
        st = _played()
        w = _w()
        for name in ("Michelangelo", "Republic", "St. Peter's Basilica"):
            self.assertTrue(self._priced_as_diff(name, w, st), name)

    def test_a_negative_offset_reproduces_the_old_leader_only_arm(self):
        st = _played()
        w = _w(card_board_government=-1.0, card_board_action=-1.0,
               card_board_wonder=-1.0)
        self.assertTrue(self._priced_as_diff("Michelangelo", w, st))
        self.assertFalse(self._priced_as_diff("Republic", w, st))
        self.assertFalse(self._priced_as_diff("St. Peter's Basilica", w, st))

    def test_a_positive_offset_turns_one_type_on_by_itself(self):
        """The point of the conversion: the league can move the government
        half on its own, from a zero credit, with no environment at all."""
        st = _played()
        w = dict(W.DEFAULT_WEIGHTS)
        w["card_board_government"] = 1.0
        self.assertTrue(self._priced_as_diff("Republic", w, st))
        self.assertFalse(self._priced_as_diff("Michelangelo", w, st))

    def test_the_environment_variable_is_gone(self):
        """A leftover reader would silently re-gate a measurement arm: an
        arm run with a stale `TTA_BOARD_TYPES` exported would quietly
        measure a different configuration than its weights say."""
        self.assertFalse(hasattr(BY, "_ENABLED"))
        with open(BY.__file__) as fh:
            src = fh.read()
        self.assertNotIn("os.environ", src)
        self.assertNotIn("getenv", src)


class TestGovernmentsWereInvisible(unittest.TestCase):
    """The find that is a result on its own: a government's whole value is
    its TOP-LEVEL `civilActions` / `militaryActions` / `urbanBuildingLimit`,
    which are in no `production` or `effects` block, so `_card_yields` --
    which only walks those two blocks -- never read them.  All eight
    governments priced at nothing, and their cost at nothing too, because
    `techCost` is `null` on every one of them and the real price is
    `revolutionCost` / `peacefulCost`."""

    def test_the_static_table_sees_no_actions_on_any_government(self):
        for card in C.db().of_type("government"):
            got = {k for k, _a, _c in W._card_yields(card["name"])}
            self.assertNotIn("civil_actions", got, card["name"])
            self.assertNotIn("military_actions", got, card["name"])
            self.assertNotIn("urban_limit", got, card["name"])

    def test_the_static_table_prices_every_government_as_free(self):
        for card in C.db().of_type("government"):
            costs = [a for k, a, c in W._card_yields(card["name"])
                     if c == W._Y_COST]
            self.assertEqual(costs, [], card["name"])

    def test_the_board_evaluator_sees_republic_beat_despotism(self):
        st = _played()
        st.players[0].government = "Despotism"
        effects.invalidate(st, st.players[0])
        got = dict((k, a) for k, a, _c in BY.board_yields("Republic", st, 0))
        # Despotism 4/2, Republic 7/2: +3 civil actions, no military change
        self.assertEqual(got["civil_actions"], 3.0)
        self.assertNotIn("military_actions", got)
        self.assertEqual(got["urban_limit"], 1.0)

    def test_a_government_now_costs_something(self):
        st = _played()
        got = dict((k, a) for k, a, _c in BY.board_yields("Democracy", st, 0))
        self.assertEqual(got["science"], -9.0)       # revolutionCost
        self.assertLess(got["gov_action_cost"], 0.0)

    def test_the_revolution_burns_the_pool_the_engine_says_it_burns(self):
        """`gov_action_cost` is the civil action total, board-aware, because
        `actions._h_revolution` sets `p.civil_actions = 0`."""
        st = _played()
        p = st.players[0]
        total = effects.state_stats(st, p).civil_actions
        got = dict((k, a) for k, a, _c in BY.board_yields("Monarchy", st, 0))
        self.assertEqual(got["gov_action_cost"], -float(total))


class TestEveryLeaderIsPriced(unittest.TestCase):
    """24 leaders, 16 of which were worth nothing beyond being a leader.

    A leader whose value is genuinely conditional prices at zero on an empty
    board and that is CORRECT -- Bach with no theaters really is worth
    nothing.  So the board this runs on is stocked with one of everything a
    leader can pay for, and then the question "is this leader worth more than
    a generic leader" has a right answer for all but the named exceptions."""

    #: leaders a fully stocked board still cannot price, and why.  Every one
    #: is a trigger or a rule change (weighted.DELIBERATELY_UNPRICED buckets
    #: 3 and 4), never an omission.  Shrinking this list is the follow-up
    #: work; GROWING it without a reason written here is a regression.
    STILL_FLAT = {
        "Aristotle":
            "trigger: 1 science per technology card TAKEN from the row",
        "Hammurabi":
            "rule: one military action usable as a civil action, plus a "
            "discount on taking the next leader",
        "Christopher Columbus":
            "rule: remove him as a political action to colonize free",
        "Frederick Barbarossa":
            "rule: pop-increase and unit-build combined into one military "
            "action, each discounted",
    }

    def _stocked(self, players=2):
        """A board carrying one staffed example of everything a leader pays
        for, so that "this leader prices at zero" means the evaluator cannot
        see it rather than the board having nothing to see."""
        st = _played(players=players)
        p = st.players[0]
        p.leader = None
        db = C.db()
        want = ("lab", "library", "theater", "temple", "mine", "farm",
                "infantry", "cavalry", "artillery")
        for typ in want:
            # the HIGHEST level of each type, not the first: Philosophy and
            # Religion are level 0, and a level-0 lab is worth exactly zero
            # culture to Sid Meier, so stocking the board with those would
            # make "this leader prices at zero" true for the wrong reason.
            pick = max(db.of_type(typ), key=lambda c: C.level(c["age"]))["name"]
            if pick not in p.techs:
                p.techs[pick] = TechCard(pick)
            p.techs[pick].workers = max(1, p.techs[pick].workers)
        if not p.colonies:
            terr = [c["name"] for c in db.of_type("territory")]
            p.colonies = list(terr[:2])
        effects.invalidate(st, p)
        return st, p

    def test_the_only_leaders_left_flat_are_triggers_and_rule_changes(self):
        """The headline: 16 of 24 leaders were worth nothing beyond being a
        leader.  On a board carrying one of everything, how many still are?"""
        st, _p = self._stocked()
        flat = []
        for card in C.db().of_type("leader"):
            got = [(k, a) for k, a, _c in BY.board_yields(card["name"], st, 0)
                   if k != "leader"]
            if not got:
                flat.append(card["name"])
        self.assertEqual(
            sorted(flat), sorted(self.STILL_FLAT),
            "the set of leaders the board evaluator cannot price has "
            "changed.  If you priced one, delete it from STILL_FLAT.  If a "
            "new one appeared, that is the bug this test exists for.")

    def test_the_static_table_leaves_sixteen_of_them_blind(self):
        """The before picture, pinned so the improvement is a number and not
        a claim: `_card_yields` alone (docs/CARD_BLINDNESS.md census)."""
        blind = [c["name"] for c in C.db().of_type("leader")
                 if not [1 for k, _a, kind in W._card_yields(c["name"])
                         if kind != W._Y_COST]]
        self.assertEqual(len(blind), 16)

    def test_each_board_scaled_leader_grows_with_the_thing_it_pays_for(self):
        """Not just non-zero -- monotone in the board count, which is what
        distinguishes real pricing from a constant."""
        cases = [("J. S. Bach", "theater", "culture_rate"),
                 ("Sid Meier", "lab", "culture_rate"),
                 ("Alexander the Great", "infantry", "strength"),
                 ("Michelangelo", "temple", "culture_rate")]
        db = C.db()
        for leader, typ, feat in cases:
            st, p = self._stocked()
            n = max((x for x in p.techs if db.type_of(x) == typ),
                    key=lambda x: db.level_of(x))
            p.techs[n].workers = 1
            effects.invalidate(st, p)
            one = dict((k, a) for k, a, _c
                       in BY.board_yields(leader, st, 0)).get(feat, 0.0)
            p.techs[n].workers = 3
            effects.invalidate(st, p)
            three = dict((k, a) for k, a, _c
                         in BY.board_yields(leader, st, 0)).get(feat, 0.0)
            self.assertGreater(one, 0.0, f"{leader} on one {typ}")
            self.assertGreater(three, one, f"{leader} on three {typ}s")

    def test_churchill_is_three_culture_a_turn_unconditionally(self):
        """His culture option needs no board and is available every turn, so
        it is a floor, not a guess -- and it is more culture than any wonder
        in the game prints."""
        st = _played()
        st.players[0].leader = None
        effects.invalidate(st, st.players[0])
        got = dict((k, a) for k, a, _c
                   in BY.board_yields("Winston Churchill", st, 0))
        self.assertEqual(got["culture_rate"], 3.0)

    def test_genghis_khan_is_unconditional_at_two_players(self):
        """"One of the two strongest civilizations, ties in your favour" is
        vacuously true at a two-player table.  A static table cannot say
        that; this is the clearest single case for board-aware pricing."""
        st = _played(players=2)
        st.players[0].leader = None
        effects.invalidate(st, st.players[0])
        got = dict((k, a) for k, a, _c
                   in BY.board_yields("Genghis Khan", st, 0))
        self.assertEqual(got["culture_rate"], 3.0)

    def test_a_leader_replacing_a_better_leader_can_be_negative(self):
        """The thing no static table can express.  Gandhi prints +2 culture;
        taking him while you hold Churchill's +3 is a LOSS of 1 culture a
        turn, and the diff says so."""
        st = _played()
        p = st.players[0]
        p.leader = "Winston Churchill"
        effects.invalidate(st, p)
        got = dict((k, a) for k, a, _c
                   in BY.board_yields("Mahatma Gandhi", st, 0))
        self.assertEqual(got.get("culture_rate", 0.0), 2.0 - 3.0)

    def test_the_static_table_cannot_and_says_so(self):
        """Same pair, through `_card_yields`: +2, whatever you already hold.
        This is the bug, stated as a test, so the contrast is on the record."""
        got = dict((k, a) for k, a, _c in W._card_yields("Mahatma Gandhi"))
        self.assertEqual(got["culture_rate"], 2.0)

    def test_a_leader_is_never_double_counted(self):
        """A swap card is priced by the diff ALONE.  Gandhi's printed +2 is
        already inside the delta; adding `_card_yields` on top would count it
        twice."""
        st = _played()
        p = st.players[0]
        p.leader = None
        effects.invalidate(st, p)
        w = _w(culture_rate=1.0, leader=0.0)
        for k in list(w):
            if k not in ("culture_rate", "card_board_credit", "leader",
                         "card_rate_credit"):
                w[k] = 0.0
        self.assertEqual(W.card_potential("Mahatma Gandhi", w, st, 0), 2.0)


class TestTheCreditGateIsExact(unittest.TestCase):
    """`card_board_credit` = 0.0 has to recover the pre-change pricing
    byte-for-byte, or the A/B in docs/CARD_BLINDNESS.md is not paired."""

    def test_zero_credit_is_the_static_answer_for_every_card(self):
        """...for every card `card_board_credit` gates.

        A TECHNOLOGY is board-priced on `unit_tech_credit` /
        `tech_board_credit` instead and is deliberately NOT gated here --
        `card_board_credit` is 0.0 on the 3p and 4p champions, so hanging the
        technology fix off it would leave two of the three league arms with
        the defect (weighted.card_potential).  The next two tests assert the
        same byte-for-byte property against the gates technologies actually
        use.
        """
        st = _played()
        w = dict(W.DEFAULT_WEIGHTS)
        self.assertEqual(w["card_board_credit"], 0.0)
        for name in C.db().by_name:
            if W._is_unit(name) or W._is_levelled_tech(name):
                continue
            self.assertEqual(W.card_potential(name, w, st, 0),
                             W.card_potential(name, w),
                             name)

    def test_zero_unit_credit_is_the_static_answer_for_every_unit(self):
        st = _played()
        w = dict(W.DEFAULT_WEIGHTS, unit_tech_credit=0.0)
        units = [n for n in C.db().by_name if W._is_unit(n)]
        self.assertEqual(len(units), 10)
        for name in units:
            self.assertEqual(W.card_potential(name, w, st, 0),
                             W.card_potential(name, w),
                             name)

    def test_zero_tech_credit_is_the_static_answer_for_every_non_unit_tech(
            self):
        """`tech_board_credit` = 0.0, the yellow lane's escape hatch.

        The red half is not asserted here because `tech_board_credit` = 0.0
        does NOT send a unit back to the static table -- it drops the develop
        half and leaves docs/UNIT_TECH_PRICING.md's board price standing,
        which is the parent commit's answer and is what the test above pins.
        """
        st = _played()
        w = dict(W.DEFAULT_WEIGHTS, tech_board_credit=0.0)
        techs = [n for n in C.db().by_name
                 if W._is_levelled_tech(n) and not W._is_unit(n)]
        self.assertEqual(len(techs), 36)
        for name in techs:
            self.assertEqual(W.card_potential(name, w, st, 0),
                             W.card_potential(name, w),
                             name)

    def test_the_default_vector_is_the_gate_shut(self):
        self.assertEqual(W.DEFAULT_WEIGHTS["card_board_credit"], 0.0)

    def test_credit_scales_linearly(self):
        st = _played()
        st.players[0].leader = None
        effects.invalidate(st, st.players[0])
        one = W.card_potential("Napoleon Bonaparte", _w(), st, 0)
        half = W.card_potential("Napoleon Bonaparte",
                                _w(card_board_credit=0.5), st, 0)
        self.assertAlmostEqual(half * 2.0, one)


class TestChoiceCards(unittest.TestCase):
    """Reserves: "gain N food OR N resources".  Summing both would be a lie
    in the other direction from dropping the key."""

    def _w(self, **over):
        w = dict.fromkeys(W.DEFAULT_WEIGHTS, 0.0)
        w["card_board_credit"] = 1.0
        w["card_rate_credit"] = 1.0
        w.update(over)
        return w

    def test_reserves_is_the_better_of_the_two_not_the_sum(self):
        w = self._w(food_stock=1.0, resource_stock=3.0)
        self.assertEqual(W.card_potential("Reserves (III)", w), 12.0)
        w["food_stock"] = 5.0
        self.assertEqual(W.card_potential("Reserves (III)", w), 20.0)

    def test_all_three_reserves_scale_with_the_printed_number(self):
        w = self._w(resource_stock=1.0)
        self.assertEqual(
            [W.card_potential(f"Reserves ({a})", w) for a in ("I", "II", "III")],
            [2.0, 3.0, 4.0])

    def test_reserves_is_still_worth_nothing_with_the_gate_shut(self):
        """All three Reserves priced at exactly zero before this change, and
        `card_board_credit` = 0.0 has to reproduce that exactly."""
        w = self._w(resource_stock=1.0, card_board_credit=0.0)
        for a in ("I", "II", "III"):
            self.assertEqual(W.card_potential(f"Reserves ({a})", w), 0.0)


class TestBoardScaledActionCards(unittest.TestCase):

    def test_endowment_pays_only_when_somebody_is_ahead_of_you(self):
        st = _played()
        p, q = st.players[0], st.players[1]
        p.culture, q.culture = 10, 50
        got = dict((k, a) for k, a, _c
                   in BY.board_extra("Endowment for the Arts", st, 0))
        self.assertEqual(got["culture"], 6.0)        # 2p coefficient
        p.culture, q.culture = 50, 10
        self.assertEqual(BY.board_extra("Endowment for the Arts", st, 0), ())

    def test_endowment_is_additive_not_a_replacement(self):
        """It is an action card, not a swap, so its static yields stay."""
        st = _played()
        st.players[0].culture, st.players[1].culture = 0, 99
        self.assertIsNone(BY.board_yields("Endowment for the Arts", st, 0))


if __name__ == "__main__":
    unittest.main()


class TestAcquisitionAndOwnershipAgree(unittest.TestCase):
    """Anything `board_yields` prices while the bot is CONSIDERING a card,
    `features` must price once the bot HOLDS it.

    The failure this exists to stop is the mirror image of the blindness the
    module was written for.  `_stats_delta` priced `urban_limit`,
    `pop_food_discount` and `no_aggression`; `features` emitted none of the
    three.  So a government that raises the urban building limit was worth
    something in the take decision and worth nothing the moment it was
    played -- value that appears on acquisition and evaporates on ownership.
    Both directions produce a bot that misjudges what it owns.

    Stated as a general invariant rather than three assertions, because the
    specific three are already fixed and what matters is that a fourth
    cannot appear quietly."""

    def _emitted_features(self):
        """Every feature name CARD PRICING can produce -- both the board path
        and the static table -- gathered by actually running them over every
        card rather than by reading the tables, because a rider that invents
        a key is exactly the case a table-reading version would miss."""
        st = _played()
        p = st.players[0]
        p.leader = None
        effects.invalidate(st, p)
        seen = set()
        for name in C.db().by_name:
            for triples in (BY.board_yields(name, st, 0) or (),
                            BY.board_extra(name, st, 0),
                            W._card_yields(name)):
                seen.update(k for k, _a, _c in triples)
            for group in W._card_choices(name):
                for g in group:
                    seen.update(k for k, _a, _c in g)
        return seen, st

    #: keys that are legitimately card-side only: they price something about
    #: PLAYING the card that does not persist as board state afterwards.
    #: Each needs a reason, exactly like DELIBERATELY_UNPRICED -- and the
    #: list is short on purpose, because "it does not persist" is a much
    #: rarer thing to be true than it first looks.
    CARD_ONLY = {
        "gov_action_cost":
            "the civil-action pool a revolution empties on the turn it is "
            "declared; gone by the next turn, so there is nothing standing "
            "for features() to report",
        "free_civil_action":
            "a rider on a one-shot action card: the free action is spent "
            "the moment the card resolves",
        "resource_discount":
            "same, the discount applies to the one build the action card "
            "pays for and then it is over",
        "restricted_resources":
            "resources ring-fenced to military units, granted for a single "
            "turn by Patriotism and friends",
        "defense_bonus":
            "a Military Bonus card defends by being SPENT (interact."
            "_defense_move discards it), so the quantity exists only while "
            "the card is in hand and there is no board state left afterwards "
            "-- unlike its colonization half, which shares colonize_bonus "
            "with the board stat effects.state_stats().colonize",
    }

    def test_every_board_priced_feature_is_also_a_board_feature(self):
        seen, st = self._emitted_features()
        board = set(W.features(st, 0))
        missing = sorted(seen - board - set(self.CARD_ONLY))
        self.assertEqual(
            missing, [],
            "board_yields prices these while the card is being CONSIDERED "
            "and weighted.features does not price them once it is OWNED, so "
            "their value evaporates on play.  Either emit them in features() "
            "or, if the quantity genuinely does not persist, add them to "
            "CARD_ONLY with a reason: " + repr(missing))

    def test_the_card_only_list_has_no_stale_entries(self):
        """A key written off here that the evaluator no longer emits is rot,
        and it would mask a real regression in the test above."""
        seen, _st = self._emitted_features()
        self.assertEqual(sorted(set(self.CARD_ONLY) - seen), [])

    def test_card_only_entries_all_carry_a_reason(self):
        for k, why in self.CARD_ONLY.items():
            self.assertTrue(isinstance(why, str) and len(why) > 20, k)

    def test_urban_limit_survives_being_played(self):
        """The concrete case: Republic's urban cap is worth something in the
        row and the same something on the board."""
        st = _played()
        p = st.players[0]
        p.government = "Despotism"
        effects.invalidate(st, p)
        before = W.features(st, 0)["urban_limit"]
        gain = dict((k, a) for k, a, _c in BY.board_yields("Republic", st, 0))
        p.government = "Republic"
        effects.invalidate(st, p)
        after = W.features(st, 0)["urban_limit"]
        self.assertEqual(after - before, gain["urban_limit"])

    def test_gandhis_ban_survives_being_played(self):
        st = _played()
        p = st.players[0]
        p.leader = None
        effects.invalidate(st, p)
        before = W.features(st, 0)["no_aggression"]
        gain = dict((k, a) for k, a, _c
                    in BY.board_yields("Mahatma Gandhi", st, 0))
        p.leader = "Mahatma Gandhi"
        effects.invalidate(st, p)
        after = W.features(st, 0)["no_aggression"]
        self.assertEqual(before, 0.0)
        self.assertEqual(after - before, gain["no_aggression"])


class TestMosesIsPricedThroughPopCost(unittest.TestCase):
    """Moses is the one key here whose board side was NEVER blind, and the
    fix is therefore the opposite of the other two: remove the duplicate
    rather than add the missing half.

    `features` already subtracts `Stats.pop_food_discount` inside `pop_cost`,
    which carries a real trained weight of -0.4.  A separate
    `pop_food_discount` feature at 0.0 was a SECOND representation of one
    quantity sitting next to a live one -- the same shape as `buildDiscount`
    summed instead of maxed."""

    def test_there_is_no_pop_food_discount_feature_or_weight(self):
        st = _played()
        self.assertNotIn("pop_food_discount", W.features(st, 0))
        self.assertNotIn("pop_food_discount", W.DEFAULT_WEIGHTS)
        self.assertNotIn("pop_food_discount",
                         [f for _a, f in BY._STATS_FEATURES])

    def test_moses_is_priced_on_pop_cost_and_matches_the_board(self):
        st = _played()
        p = st.players[0]
        p.leader = None
        p.yellow_bank = max(p.yellow_bank, 5)      # so a pop cost exists
        effects.invalidate(st, p)
        before = W.features(st, 0)["pop_cost"]
        gain = dict((k, a) for k, a, _c in BY.board_yields("Moses", st, 0))
        p.leader = "Moses"
        effects.invalidate(st, p)
        after = W.features(st, 0)["pop_cost"]
        self.assertEqual(after - before, gain["pop_cost"])
        self.assertLess(gain["pop_cost"], 0.0)     # cheaper, and priced

    def test_moses_now_prices_at_a_live_weight(self):
        """The point of routing him through `pop_cost`: he is worth
        something under a trained vector instead of nothing under a 0.0."""
        st = _played()
        p = st.players[0]
        p.leader = None
        p.yellow_bank = max(p.yellow_bank, 5)
        effects.invalidate(st, p)
        w = _w(card_board_leader=1.0)
        self.assertNotEqual(W.card_potential("Moses", w, st, 0), 0.0)


class TestThePopCostFormulaHasOneImplementation(unittest.TestCase):
    """`max(0, pop_cost_base(bank) - stats.pop_food_discount)` existed in
    four places: economy, weighted.features, neural_encode and Ocean Liners'
    rider.  That is the shape of bug this repo has already paid for twice."""

    def test_nobody_recomputes_it_by_hand(self):
        import glob
        import os
        root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
        offenders = []
        for path in glob.glob(os.path.join(root, "engine", "**", "*.py"),
                              recursive=True):
            if os.path.basename(path) == "economy.py":
                continue
            with open(path) as fh:
                for n, line in enumerate(fh, 1):
                    if "pop_food_discount" in line and "max(" in line:
                        offenders.append("%s:%d" % (os.path.relpath(path,
                                                                    root), n))
        self.assertEqual(
            offenders, [],
            "the population-cost formula belongs in "
            "economy.pop_food_cost and nowhere else: " + repr(offenders))

    def test_the_shared_helper_agrees_with_the_state_taking_wrapper(self):
        from engine import economy
        st = _played()
        p = st.players[0]
        s = effects.state_stats(st, p)
        self.assertEqual(economy.pop_food_cost(s, p.yellow_bank,
                                               p.one_time_discount),
                         economy.pop_cost(st, p))
