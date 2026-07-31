"""The colonization sacrifice is the PLAYER's choice (RULES_SPEC 11.3).

`docs/OPEN_ITEMS.md` 2.16 -- "the one clean rules-level engine defect in the
whole coverage census" -- was that `interact._build_force` decided the force
for the winner: weakest unit, then bonus cards cheapest-first, then more
units.  RULES_SPEC 11.3 fixes only the floor ("printed strength of the
sacrificed military units (>= 1 unit mandatory, even if other bonuses would
cover the bid)" ... "the colonization value (bottom half) of ANY NUMBER of
military bonus cards played") and 11.2 fixes only the total ("forming a force
>= their final bid").  Everything in between belongs to the player.

Every test in the first two classes fails against the pre-change engine: there
was no `colonize` pending, no `send_unit`/`send_bonus`/`send_done` move and no
way to reach any sacrifice other than the one the engine picked.
"""
from __future__ import annotations

import copy
import os
import random
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from engine import actions, cards as C, effects, game, interact, journal  # noqa: E402
from engine.bots import GreedyBot, RandomBot, WeightedBot  # noqa: E402
from engine.bots.book import BookBot  # noqa: E402
from engine.bots.fastcopy import copy_state  # noqa: E402
from engine.bots.plan import PlanBot  # noqa: E402
from engine.bots.quiescent import QuiescentBot  # noqa: E402
from engine.state import TechCard  # noqa: E402

actions.STRICT = True

BONUS_I = next(c["name"] for c in C.db().cards
               if c["type"] == "bonus" and c["age"] == "I")


def _solo(seed=21, players=3):
    """Round-3 state where only P0 has any military unit at all."""
    st = game.new_game(players, seed=seed)
    st.round = 3
    st.phase = "politics"
    st.has_military = True
    db = C.db()
    for q in st.players:
        for n, t in q.techs.items():
            if db.type_of(n) in C.UNIT_TYPES:
                t.workers = 0
        effects.invalidate(st, q)
    return st


def _two_unit_types(st):
    """P0 with two DISTINCT units: Warriors (strength 1), Swordsmen (2)."""
    p = st.players[0]
    p.techs["Warriors"].workers = 1
    p.techs["Swordsmen"] = TechCard("Swordsmen", workers=1)
    effects.invalidate(st, p)
    return p


class TheChoiceExists(unittest.TestCase):

    def test_two_unit_types_is_a_decision_and_not_the_engines(self):
        """The defect, stated as a test: a bid either unit alone can pay."""
        st = _solo()
        p = _two_unit_types(st)
        interact.colonize(st, p, "Vast Territory (I)", 1)
        self.assertTrue(st.pending, "the engine chose the force by itself")
        pend = st.pending[-1]
        self.assertEqual(pend["kind"], "colonize")
        self.assertEqual(pend["player"], p.idx)
        self.assertEqual(pend["units"], [])          # nothing decided yet
        self.assertEqual(
            sorted(m for m in actions.legal_moves(st)),
            [("send_unit", "Swordsmen"), ("send_unit", "Warriors")])

    def test_both_answers_are_reachable_and_they_differ(self):
        """Not merely offered: the two branches really do end differently.

        A bid of 1 that EITHER unit pays on its own, so the choice is purely
        "which of my two units do I want to keep" and nothing else varies.
        """
        st = _solo()
        _two_unit_types(st)
        left = {}
        for send in ("Warriors", "Swordsmen"):
            s = copy_state(st)
            interact.colonize(s, s.players[0], "Vast Territory (I)", 1)
            actions.apply(s, ("send_unit", send))
            # the bid is already met; throwing the other unit in as well is
            # legal and pointless, so `send_done` is a real move to make
            self.assertIn(("send_done",), actions.legal_moves(s))
            actions.apply(s, ("send_done",))
            self.assertFalse(s.pending)
            left[send] = {n: s.players[0].techs[n].workers
                          for n in ("Warriors", "Swordsmen")}
            self.assertIn("Vast Territory (I)", s.players[0].colonies)
        self.assertEqual(left["Warriors"], {"Warriors": 0, "Swordsmen": 1})
        self.assertEqual(left["Swordsmen"], {"Warriors": 1, "Swordsmen": 0})

    def test_a_bonus_card_may_be_spent_instead_of_a_second_unit(self):
        """RULES_SPEC 11.3: ANY NUMBER of bonus cards, at the player's option.

        The old engine always reached for the bonus card first.  Both are now
        legal and both are offered.
        """
        st = _solo()
        p = st.players[0]
        p.techs["Warriors"].workers = 2
        p.hand_military = [BONUS_I]
        effects.invalidate(st, p)
        interact.colonize(st, p, "Vast Territory (I)", 2)
        # the mandatory unit has one identity, so it is taken without asking;
        # closing the last point of the bid is the decision
        self.assertEqual(st.pending[-1]["units"], ["Warriors"])
        moves = actions.legal_moves(st)
        self.assertIn(("send_bonus", BONUS_I), moves)
        self.assertIn(("send_unit", "Warriors"), moves)
        self.assertNotIn(("send_done",), moves)       # force 1 < bid 2

        keep_card = copy_state(st)
        actions.apply(keep_card, ("send_unit", "Warriors"))
        actions.apply(keep_card, ("send_done",))
        self.assertEqual(keep_card.players[0].hand_military, [BONUS_I])
        self.assertEqual(keep_card.players[0].techs["Warriors"].workers, 0)

        keep_unit = copy_state(st)
        actions.apply(keep_unit, ("send_bonus", BONUS_I))
        actions.apply(keep_unit, ("send_done",))
        self.assertEqual(keep_unit.players[0].hand_military, [])
        self.assertEqual(keep_unit.players[0].techs["Warriors"].workers, 1)

    def test_the_winner_owns_the_decision_not_the_current_player(self):
        """RULES_SPEC 11.6: a player other than the current one can win."""
        st = _solo()
        for q in st.players:
            q.techs["Warriors"].workers = 2
            q.techs["Swordsmen"] = TechCard("Swordsmen", workers=1)
            effects.invalidate(st, q)
        interact.start_auction(st, "Wealthy Territory (I)", 0)
        actions.apply(st, ("bid", 2))                 # P0
        actions.apply(st, ("bid", 3))                 # P1 outbids
        actions.apply(st, ("bid_pass",))              # P2 out
        actions.apply(st, ("bid_pass",))              # P0 out -> P1 wins
        self.assertEqual(st.pending[-1]["kind"], "colonize")
        self.assertEqual(st.decider(), 1)
        self.assertEqual(st.current, 0)


class TheFloorIsStillEnforced(unittest.TestCase):

    def test_send_done_is_illegal_below_the_bid(self):
        st = _solo()
        p = st.players[0]
        p.techs["Warriors"].workers = 3
        p.techs["Swordsmen"] = TechCard("Swordsmen", workers=1)
        effects.invalidate(st, p)
        interact.colonize(st, p, "Vast Territory (I)", 4)
        steps = 0
        while st.pending and ("send_done",) not in actions.legal_moves(st):
            pend = st.pending[-1]
            self.assertLess(
                interact.force_value(st, p, pend["units"], pend["bonuses"]), 4)
            actions.apply(st, actions.legal_moves(st)[0])
            steps += 1
        self.assertGreater(steps, 0, "the force was never short of the bid")
        # `send_done` appeared only once the force actually reached the bid
        pend = st.pending[-1] if st.pending else None
        force = 4 if pend is None else interact.force_value(
            st, p, pend["units"], pend["bonuses"])
        self.assertGreaterEqual(force, 4)

    def test_send_done_is_illegal_with_no_unit_in_the_force(self):
        """RULES_SPEC 11.3: >= 1 unit even if bonuses would cover the bid."""
        st = _solo()
        p = st.players[0]
        p.techs["Warriors"].workers = 1
        p.hand_military = [BONUS_I, BONUS_I]
        effects.invalidate(st, p)
        pend = {"kind": "colonize", "player": 0, "card": "Vast Territory (I)",
                "bid": 1, "units": [], "bonuses": [BONUS_I, BONUS_I],
                "pool": ["Warriors"], "bpool": []}
        moves = interact._colonize_moves(st, pend)
        self.assertNotIn(("send_done",), moves)
        self.assertNotIn(("send_bonus", BONUS_I), moves)   # unit comes first
        self.assertEqual(moves, [("send_unit", "Warriors")])

    def test_no_decision_is_offered_when_there_is_no_choice(self):
        """One unit, no cards: `push_choice(auto=True)`'s convention."""
        st = _solo()
        p = st.players[0]
        p.techs["Warriors"].workers = 1
        effects.invalidate(st, p)
        interact.colonize(st, p, "Vast Territory (I)", 1)
        self.assertFalse(st.pending)
        self.assertIn("Vast Territory (I)", p.colonies)
        self.assertEqual(p.techs["Warriors"].workers, 0)

    def test_identical_units_are_one_move_not_n_moves(self):
        st = _solo()
        p = st.players[0]
        p.techs["Warriors"].workers = 4
        effects.invalidate(st, p)
        interact.colonize(st, p, "Vast Territory (I)", 2)
        sends = [m for m in actions.legal_moves(st) if m[0] == "send_unit"]
        self.assertEqual(sends, [("send_unit", "Warriors")])


class EveryBotAnswersIt(unittest.TestCase):
    """A decision point bots answer badly is a regression even when the rules
    are right.  Every shipped bot must terminate, stay legal, and pay a force
    that really does meet the bid without over-spending wildly."""

    def _drive(self, bot):
        st = _solo()
        p = st.players[0]
        p.techs["Warriors"].workers = 3
        p.techs["Swordsmen"] = TechCard("Swordsmen", workers=2)
        p.hand_military = [BONUS_I]
        effects.invalidate(st, p)
        before_units = sum(p.techs[n].workers for n in ("Warriors",
                                                        "Swordsmen"))
        interact.colonize(st, p, "Vast Territory (I)", 3)
        for _ in range(20):
            if not st.pending:
                break
            moves = actions.legal_moves(st)
            self.assertTrue(moves, "a colonize decision with no legal move")
            mv = bot.choose(st, moves)
            self.assertIn(mv, moves, f"{bot} played an illegal move {mv}")
            actions.apply(st, mv)
        self.assertFalse(st.pending, f"{bot} never finished the force")
        self.assertIn("Vast Territory (I)", p.colonies)
        spent = before_units - sum(p.techs[n].workers
                                   for n in ("Warriors", "Swordsmen"))
        self.assertGreaterEqual(spent, 1)             # §11.3 floor
        return spent

    def test_book(self):
        # the book policy is the old engine rule, made explicit: the cheapest
        # unit, then the bonus CARD before any further unit
        st = _solo()
        p = st.players[0]
        p.techs["Warriors"].workers = 2
        p.hand_military = [BONUS_I]
        effects.invalidate(st, p)
        interact.colonize(st, p, "Vast Territory (I)", 2)
        bot = BookBot(seed=1)
        while st.pending:
            actions.apply(st, bot.choose(st, actions.legal_moves(st)))
        self.assertEqual(p.hand_military, [])            # card spent...
        self.assertEqual(p.techs["Warriors"].workers, 1)  # ...unit kept
        self.assertEqual(self._drive(BookBot(seed=1)), 2)

    def test_greedy(self):
        self._drive(GreedyBot(seed=1))

    def test_weighted(self):
        self._drive(WeightedBot(seed=1))

    def test_quiescent(self):
        self._drive(QuiescentBot(seed=1))

    def test_plan(self):
        self._drive(PlanBot(seed=1))

    def test_random(self):
        for seed in range(8):
            self._drive(RandomBot(seed=seed))

    def test_nobody_wastes_the_whole_army(self):
        """The bid is 3 and the pool is worth far more: a bot that answers
        `send_unit` all the way down would pass every legality check above."""
        for bot in (BookBot(seed=1), GreedyBot(seed=1), WeightedBot(seed=1),
                    QuiescentBot(seed=1), PlanBot(seed=1)):
            self.assertLessEqual(self._drive(bot), 2, f"{bot} over-spent")


class ItCopiesAndRollsBack(unittest.TestCase):

    def _pending_state(self):
        st = _solo()
        p = _two_unit_types(st)
        interact.colonize(st, p, "Vast Territory (I)", 2)
        return st

    def test_fastcopy_does_not_alias_the_force_lists(self):
        st = self._pending_state()
        cp = copy_state(st)
        self.assertEqual(cp.pending, st.pending)
        cp.pending[-1]["units"].append("Warriors")
        cp.pending[-1]["pool"].clear()
        self.assertEqual(st.pending[-1]["units"], [])
        self.assertTrue(st.pending[-1]["pool"])

    def test_the_journal_rolls_the_whole_decision_back(self):
        st = self._pending_state()
        journal.install()
        before = copy.deepcopy(st.pending)
        workers = st.players[0].techs["Swordsmen"].workers
        j = journal.begin(st)
        try:
            actions.apply(st, ("send_unit", "Swordsmen"))
            self.assertEqual(st.pending[-1]["units"], ["Swordsmen"])
            actions.apply(st, ("send_done",))
            self.assertFalse(st.pending)
        finally:
            journal.rollback(j)
        self.assertEqual(st.pending, before)
        self.assertEqual(st.players[0].techs["Swordsmen"].workers, workers)
        self.assertEqual(st.players[0].colonies, [])


class ItSurvivesRealGames(unittest.TestCase):

    def test_random_play_never_deadlocks_on_a_colonize_decision(self):
        """The legality fuzzer: a pending with an empty move list would hang
        the arena, and `_colonize_moves` is the only new generator."""
        seen = 0
        for seed in range(12):
            st = game.new_game(3, seed=seed)
            rng = random.Random(seed)
            bot = RandomBot(seed=seed)
            for _ in range(1500):
                if st.game_over:
                    break
                moves = actions.legal_moves(st)
                if not moves:
                    self.fail("no legal move")
                if st.pending and st.pending[-1]["kind"] == "colonize":
                    seen += 1
                actions.apply(st, bot.choose(st, moves), rng)
        # not asserted as a rate -- only that the path is reachable at all,
        # so this test is not silently vacuous
        self.assertGreater(seen, 0, "no colonize decision was ever reached")


if __name__ == "__main__":
    unittest.main()
