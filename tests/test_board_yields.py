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

    def test_the_type_knob_defaults_to_everything(self):
        """`TTA_BOARD_TYPES` exists to decompose the A/B.  Unset must mean
        every board-priced type, or a measurement arm silently becomes a
        production one.  `wonder` joined the list when Lane A made wonders a
        swap type; anything added here must be added there in the same
        commit, which is what this assertion is for."""
        self.assertEqual(sorted(BY._ENABLED),
                         ["action", "government", "leader", "wonder"])


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
        st = _played()
        w = dict(W.DEFAULT_WEIGHTS)
        self.assertEqual(w["card_board_credit"], 0.0)
        for name in C.db().by_name:
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
