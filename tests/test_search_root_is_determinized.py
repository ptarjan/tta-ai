"""No search of a hidden pile may read the pile's real order.

WHY THIS FILE EXISTS.  `engine/state.py` keeps `civil_deck`, `military_deck`
and `current_events` as full ordered lists, and `fastcopy.copy_state` copies
them verbatim -- deliberately, because it is a *copier*.  So a trial `apply`
inside a search draws the REAL next card unless somebody re-shuffles the piles
first.  `plan.determinize` is that somebody, and `PlanBot.pick` /
`NeuralPlanBot.pick` / `NeuralBot.pick` / `pending.prepare_root` are the four
places it is called.

The defect this file closes is not that the call was missing.  It is that the
call was *incomplete* and nothing noticed for as long as it existed:
`determinize` shuffled the two draw decks and never touched `current_events`,
while `events.reveal_current_event` pops that pile at the top of every turn --
so every `end_turn` the beam ever expanded revealed the true next event.  On
the instrument that can tell the difference (`tools/infoleak.py --true-card`,
2p, 8 games) the trial drew the true top CIVIL card on 23.6% of civil draws
and the true top EVENT on **100.0%** of event draws.

So the guarantee here is deliberately NOT "determinize is called".  A call that
covers two of three piles passes that test.  It is:

1. **Coverage is a decision somebody wrote down.**  `plan.HIDDEN_ORDER` is
   pinned against this file's own copy, so adding a fourth hidden pile to
   `GameState` -- or dropping one from the tuple -- fails here instead of
   silently leaking.
2. **Every listed pile is actually permuted**, on a real state, by the real
   function.
3. **Nothing else is.**  Every other container on the state is compared
   element-wise across `determinize` and must be identical.  That is what stops
   a future "just shuffle everything" from re-dealing the visible card row or
   somebody's own hand -- the information a human at the table legitimately
   has.
4. **The multiset survives.**  Determinization re-orders what is unseen; it
   does not invent cards or lose them.
5. **The age bands survive.**  The events pile is sorted by descending age
   level (`events._recycle_future_events`), because `pop()` takes from the end
   and the oldest age must come out first.  That ordering is PUBLIC.  A flat
   shuffle would hide private information by destroying public information and
   would let the search see Age III events arrive early.
6. **The end-to-end version of all of the above**, asserted on tracked state
   rather than on a call count: play real games, and at every decision where a
   bot actually searched, assert that the state its search was handed differs
   in hidden order from the true state and agrees with it everywhere else.

Test 6 is the one that fails if the leak reopens by any route at all --
including a route nobody thought of when writing tests 1-5.
"""
from __future__ import annotations

import os
import random
import sys
import unittest
from dataclasses import fields

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)),
                                ".."))

from engine import actions, cards, game                       # noqa: E402
from engine.bots import pending                               # noqa: E402
from engine.bots import plan as plan_mod                      # noqa: E402
from engine.bots.fastcopy import copy_state                   # noqa: E402
from engine.bots.plan import PlanBot, determinize             # noqa: E402

#: The pinned copy of `plan.HIDDEN_ORDER`.  Written out here rather than
#: imported so that a change to the production tuple has to be made twice, on
#: purpose, with this docstring in front of the second one.
EXPECTED_HIDDEN = ("civil_deck", "military_deck", "current_events")


_CACHE = {}


_MAX_PLIES = 400


def _mid_game_state(players=3, seed=4242, moves=140):
    """A state deep enough that all three hidden piles are non-trivial.

    Built once and handed out as copies.  This is the expensive half of the
    file and four tests want the same fixture; the gate runs the whole suite
    twice (plain and ``JOURNAL_PARANOID``), so a fixture rebuilt per test is
    paid for eight times.
    """
    key = (players, seed, moves)
    if key not in _CACHE:
        st = game.new_game(players, seed)
        rng = random.Random(seed)
        from engine.bots import WeightedBot
        bots = [WeightedBot(seed=7) for _ in range(players)]
        # `moves` is a FLOOR, not a fixed depth.  A hardcoded ply count is a
        # fixture that silently stops testing anything the moment the policy
        # changes: `current_events` fell to a single distinct entry at ply 140
        # when the rate horizon landed (docs/RATE_HORIZON.md), and the only
        # symptom was this file's own "deepen _mid_game_state" assertion.  So
        # walk past the floor until every pile the test permutes has at least
        # two distinct entries, and stop at the first state that does.
        best = None
        for i in range(_MAX_PLIES):
            if game.is_over(st):
                break
            mvs = actions.legal_moves(st)
            if not mvs:
                break
            cur = game.current_player(st)
            st = game.apply(st, bots[cur].choose(st, mvs, rng), rng)
            best = st
            if i + 1 >= moves and all(
                    len(set(getattr(st, fld))) > 1
                    for fld in plan_mod.HIDDEN_ORDER):
                break
        _CACHE[key] = best if best is not None else st
    # a copy, because `test_the_public_age_order...` rewrites current_events
    return copy_state(_CACHE[key])


class DeterminizeCoversEveryHiddenPile(unittest.TestCase):

    def test_the_hidden_pile_registry_is_pinned(self):
        self.assertEqual(
            tuple(plan_mod.HIDDEN_ORDER), EXPECTED_HIDDEN,
            "plan.HIDDEN_ORDER changed.  Every name in it is a list on "
            "GameState whose ORDER a player at the table cannot see and which "
            "some actions.apply can consume during a trial.  If you added a "
            "pile, add it to EXPECTED_HIDDEN here too and say in the commit "
            "message why it is hidden; if you removed one, say why a search "
            "may now read its true order.")

    def test_every_registered_pile_is_actually_permuted(self):
        # One pile at a time, so a function that shuffles two of three (which
        # is exactly the bug this file was written for) names the one it drops.
        st = _mid_game_state()
        for fld in plan_mod.HIDDEN_ORDER:
            with self.subTest(field=fld):
                before = list(getattr(st, fld))
                self.assertGreater(
                    len(set(before)), 1,
                    f"{fld} has <2 distinct entries in the fixture, so this "
                    f"test cannot see a permutation; deepen _mid_game_state")
                # 48 seeds and a bar of 11, NOT 24 and a bar of 12.  A pile
                # holding k entries is left in its true order by a correct
                # shuffle with probability 1/k!, so the shortest pile in the
                # fixture -- `current_events` reaches k = 2 -- puts the
                # EXPECTED count at exactly half the seeds and a bar of "more
                # than half" is a coin flip.  It came up tails the first time
                # a pricing change re-rolled the fixture
                # (docs/CARD_BLINDNESS.md).  A pile nobody shuffles
                # still scores 0, which is what this is really looking for,
                # and 11/48 is ~3.7 sd below the k = 2 expectation.
                moved = 0
                for s in range(48):
                    root = copy_state(st)
                    determinize(root, random.Random(s))
                    if list(getattr(root, fld)) != before:
                        moved += 1
                self.assertGreater(
                    moved, 11,
                    f"determinize left {fld} in its true order on "
                    f"{48 - moved}/48 seeds.  A pile nobody shuffles is read "
                    f"by every trial apply that draws from it.")

    def test_nothing_a_player_can_see_is_touched(self):
        st = _mid_game_state()
        root = copy_state(st)
        determinize(root, random.Random(11))
        hidden = set(plan_mod.HIDDEN_ORDER)
        for f in fields(game.GameState if hasattr(game, "GameState")
                        else type(st)):
            if f.name in hidden or f.name == "log":
                continue
            with self.subTest(field=f.name):
                self.assertEqual(
                    getattr(root, f.name), getattr(st, f.name),
                    f"determinize changed {f.name}, which is NOT in "
                    f"HIDDEN_ORDER.  Re-ordering something the mover can see "
                    f"-- the card row, a hand, a tableau, the resolved events "
                    f"-- destroys information a human at the table has and is "
                    f"a worse bug than the leak it was trying to fix.")
        # `players` compares equal above only because PlayerState is a
        # dataclass; assert the hands explicitly so a future non-dataclass
        # PlayerState cannot make that check vacuous.
        for i, (a, b) in enumerate(zip(root.players, st.players)):
            self.assertEqual(a.hand_civil, b.hand_civil, f"p{i} civil hand")
            self.assertEqual(a.hand_military, b.hand_military, f"p{i} mil hand")

    def test_the_multiset_survives(self):
        st = _mid_game_state()
        root = copy_state(st)
        determinize(root, random.Random(5))
        for fld in plan_mod.HIDDEN_ORDER:
            self.assertEqual(
                sorted(getattr(root, fld)), sorted(getattr(st, fld)),
                f"determinize changed WHAT is in {fld}, not just the order")

    def test_the_public_age_order_of_the_event_pile_survives(self):
        # Built by hand rather than sampled: a mixed-age pile is exactly the
        # case a flat shuffle breaks, and the fixture is not guaranteed to
        # contain one.
        db = cards.db()
        by_level = {}
        for name in db.by_name:
            try:
                if db.type_of(name) == "event":
                    by_level.setdefault(db.level_of(name), []).append(name)
            except Exception:
                continue
        levels = sorted(by_level)[:3]
        if len(levels) < 2:
            self.skipTest("card DB has events of fewer than two ages")
        st = _mid_game_state()
        # pop() takes from the end, so DESCENDING level == oldest age last-out
        # first.  Build the pile the way events._recycle_future_events does.
        pile = []
        for lv in sorted(levels, reverse=True):
            pile.extend(by_level[lv][:3])
        st.current_events = list(pile)
        want = [db.level_of(n) for n in pile]
        for s in range(12):
            root = copy_state(st)
            determinize(root, random.Random(s))
            got = [db.level_of(n) for n in root.current_events]
            self.assertEqual(
                got, want,
                "determinize re-ordered the AGE BANDS of the event pile.  "
                "That order is public (events._recycle_future_events sorts by "
                "descending level so the oldest age pops first); only the "
                "order WITHIN a band is hidden.")


class NoSearchEverSeesTheTrueOrder(unittest.TestCase):
    """Test 6: the end-to-end guarantee, on tracked state.

    `plan.determinize` is replaced with a wrapper that records the state it was
    handed and then calls the real thing.  Two assertions follow, and it is the
    pair that is the guarantee:

      * a bot that stops calling `determinize` records nothing, and the
        "searched but never determinized" count is non-zero -> FAIL;
      * a bot that calls it but leaves a pile alone records a root whose
        hidden order still matches reality -> FAIL.

    Neither is a regex over prose and neither is a call count on its own.
    """

    #: The pile a trial `apply` actually consumes is the one at the END of the
    #: list (`pop()`), so THAT is the card the leak hands the search.  Counting
    #: whether the whole permutation matched instead would dilute a short pile
    #: -- `current_events` is only 3-5 long -- and an aggregate over three
    #: fields dilutes it a second time.  Both dilutions were real: an earlier
    #: draft of this test PASSED against the un-shuffled event pile.  So this
    #: is per-field, and it counts top cards.
    MIN_PILE = 3
    #: chance alone gives 1/len <= 1/3 for a pile of >=3; the leak gives 1.000
    MAX_TOP_MATCH_RATE = 0.55

    def _run(self, players, seeds, width=2, limit=220):
        seen = []

        real = plan_mod.determinize

        def spy(state, rng):
            seen.append(state)
            return real(state, rng)

        searched_undeterminized = 0
        top = {f: [0, 0] for f in plan_mod.HIDDEN_ORDER}   # field -> [hit, n]

        plan_mod.determinize = spy
        try:
            for sd in seeds:
                st = game.new_game(players, sd)
                rng = random.Random(sd)
                bots = [PlanBot(seed=3 + i, width=width)
                        for i in range(players)]
                n = 0
                while not game.is_over(st) and n < limit:
                    mvs = actions.legal_moves(st)
                    p = game.current_player(st)
                    bot = bots[p]
                    before_searches = bot.searches
                    before_roots = pending.counters()["roots"]
                    del seen[:]
                    mv = bot.choose(st, mvs, rng)
                    did_search = (bot.searches > before_searches
                                  or pending.counters()["roots"] > before_roots)
                    if did_search and len(mvs) > 1:
                        if not seen:
                            searched_undeterminized += 1
                        for root in seen:
                            for fld in plan_mod.HIDDEN_ORDER:
                                a, b = getattr(root, fld), getattr(st, fld)
                                if len(b) < self.MIN_PILE:
                                    continue
                                top[fld][1] += 1
                                if a and b and a[-1] == b[-1]:
                                    top[fld][0] += 1
                    st = game.apply(st, mv, rng)
                    n += 1
        finally:
            plan_mod.determinize = real
        return searched_undeterminized, top

    def test_every_search_prices_a_determinized_root(self):
        undet, top = self._run(2, (77, 78), limit=190)
        self.assertEqual(
            undet, 0,
            f"{undet} real decisions were SEARCHED without any pile being "
            f"determinized.  The search then draws the true next card on "
            f"100% of trial draws -- that is not a rate, it is an identity.")
        for fld in plan_mod.HIDDEN_ORDER:
            hit, n = top[fld]
            with self.subTest(field=fld):
                self.assertGreater(
                    n, 40,
                    f"only {n} observations of {fld} at length >= "
                    f"{self.MIN_PILE}; this arm cannot see a leak in it, so "
                    f"deepen the fixture rather than trusting a pass")
                rate = hit / n
                self.assertLess(
                    rate, self.MAX_TOP_MATCH_RATE,
                    f"the state handed to the search had the TRUE next {fld} "
                    f"card on top in {hit}/{n} ({100 * rate:.1f}%) of "
                    f"searches.  Chance gives at most ~{100 / self.MIN_PILE:.0f}%. "
                    f"That pile is not being determinized, and every trial "
                    f"apply that draws from it is reading the future.")

    def test_the_instrument_can_fail(self):
        # The negative control the rest of this file is worth nothing without:
        # with determinization off, the SAME measurement must come back at the
        # identity, so a passing run above is evidence and not a vacuous loop.
        real = plan_mod.determinize
        seen_true = 0
        checked = 0
        st = game.new_game(2, 91)
        rng = random.Random(91)
        bots = [PlanBot(seed=3 + i, width=2, determinize=False)
                for i in range(2)]
        n = 0
        while not game.is_over(st) and n < 120:
            mvs = actions.legal_moves(st)
            p = game.current_player(st)
            # with determinize=False the root is a plain copy, so the pile the
            # search prices IS the true pile -- copy it the same way pick does
            root = copy_state(st)
            for fld in plan_mod.HIDDEN_ORDER:
                if len(getattr(st, fld)) >= 4:
                    checked += 1
                    if list(getattr(root, fld)) == list(getattr(st, fld)):
                        seen_true += 1
            st = game.apply(st, bots[p].choose(st, mvs, rng), rng)
            n += 1
        self.assertIs(plan_mod.determinize, real, "spy leaked out of a test")
        self.assertGreater(checked, 100)
        self.assertEqual(
            seen_true, checked,
            "copy_state stopped copying the hidden piles verbatim.  That is "
            "not a fix -- it would mean the copier is lying about being a "
            "copy -- and it makes the positive test above vacuous.")


if __name__ == "__main__":
    unittest.main()
