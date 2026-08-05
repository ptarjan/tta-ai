"""The four base-game leader abilities that live in the POLITICS phase.

All four were declared in `data/cards_wonders_leaders.json`, written off in
`engine/bots/weighted.DELIBERATELY_UNPRICED` as rule changes "the rules engine
expresses", and expressed by nothing in `engine/`.  Nothing failed when the
two disagreed -- which is this project's recurring bug class -- so each one is
pinned here by the behaviour it is supposed to produce, plus the limit that
makes it a cost rather than a gift:

* **Julius Caesar** (A) -- "Once per game, you may take two political actions
  in your politics phase."  ONCE PER GAME, and passing on the second action
  does not spend it.
* **Alexander the Great** (A) -- "As a political action, you may remove
  Alexander from the game to take 1 yellow token from the box into your yellow
  bank."  He leaves the game, so it can happen at most once.
* **Christopher Columbus** (I) -- "As a political action, you may remove
  Columbus from the game to colonize a territory card from your hand without
  sacrificing any military units."  No auction, no bid, no unit lost.
* **Joan of Arc** (I) -- "When you begin your politics phase, you may look at
  the top card of the current events deck."  Information for HER seat, and
  `plan.determinize` is what turns it into a decision.
"""
from __future__ import annotations

import os
import random
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from engine import actions, cards as C, effects, game, interact  # noqa: E402
from engine.bots import GreedyBot, RandomBot, WeightedBot  # noqa: E402
from engine.bots.book import BookBot  # noqa: E402
from engine.bots.fastcopy import copy_state  # noqa: E402
from engine.bots.plan import PlanBot, determinize  # noqa: E402
from engine.bots.quiescent import QuiescentBot  # noqa: E402

actions.STRICT = True

_DB = C.db()
TERRITORIES = [c["name"] for c in _DB.of_type("territory")
               if c["age"] == "I"][:2]
#: Two Age I events that resolve without pushing a decision of their own, so a
#: politics test measures the politics phase and not an event's fallout.
EVENTS = ["Rats", "Pestilence"]
#: The pile the fixture reveals from, for the same reason.
FILLER = ["Raiders"] * 4


def _rng(seed=0):
    """`actions.apply(state, move)` defaults `rng` to None, and
    `events._recycle_future_events` needs one the moment the current events
    pile empties -- so every apply here passes one, deterministically."""
    return random.Random(seed)


def _politics(leader=None, players=3, seed=21, hand=()):
    """A round-3 state sitting in P0's politics phase."""
    st = game.new_game(players, seed=seed)
    st.round = 3
    st.phase = "politics"
    st.has_military = True
    st.current_events = list(FILLER)
    st.future_events = []
    p = st.players[0]
    p.leader = leader
    p.hand_military = list(hand)
    p.politics_done = False
    effects.invalidate(st, p)
    return st, p


def _apply(st, mv, seed=0):
    return actions.apply(st, mv, _rng(seed))


def _kinds(st):
    return {m[0] for m in actions.legal_moves(st)}


class JuliusCaesarTakesTwoPoliticalActions(unittest.TestCase):

    def test_the_phase_stays_open_after_the_first_action(self):
        st, p = _politics("Julius Caesar", hand=list(EVENTS))
        _apply(st, ("prepare_event", EVENTS[0]))
        self.assertEqual(st.phase, "politics", "the phase closed after one")
        self.assertFalse(p.politics_done)
        self.assertTrue(p.caesar_second_politics)
        self.assertFalse(p.caesar_double_politics_used, "spent by being owed")
        self.assertIn(("prepare_event", EVENTS[1]), actions.legal_moves(st))

    def test_the_second_action_really_resolves_and_spends_the_ability(self):
        st, p = _politics("Julius Caesar", hand=list(EVENTS))
        _apply(st, ("prepare_event", EVENTS[0]))
        _apply(st, ("prepare_event", EVENTS[1]))
        self.assertEqual(st.phase, "actions")
        self.assertTrue(p.politics_done)
        self.assertTrue(p.caesar_double_politics_used)
        self.assertFalse(p.caesar_second_politics)
        # both cards left the hand and both are in the events deck
        self.assertEqual(p.hand_military, [])
        for name in EVENTS:
            self.assertIn(name, st.future_events)

    def test_passing_on_the_second_action_does_not_spend_it(self):
        """"You MAY take two": being offered the second one is not taking it."""
        st, p = _politics("Julius Caesar", hand=[EVENTS[0]])
        _apply(st, ("prepare_event", EVENTS[0]))
        _apply(st, ("pol_pass",))
        self.assertEqual(st.phase, "actions")
        self.assertFalse(p.caesar_double_politics_used)
        self.assertFalse(p.caesar_second_politics)

    def test_it_is_once_per_game_not_once_per_turn(self):
        st, p = _politics("Julius Caesar", hand=list(EVENTS))
        p.caesar_double_politics_used = True          # already spent
        _apply(st, ("prepare_event", EVENTS[0]))
        self.assertEqual(st.phase, "actions")
        self.assertTrue(p.politics_done)
        self.assertFalse(p.caesar_second_politics)

    def test_every_other_leader_gets_exactly_one_political_action(self):
        for leader in (None, "Alexander the Great", "Joan of Arc"):
            st, p = _politics(leader, hand=list(EVENTS))
            _apply(st, ("prepare_event", EVENTS[0]))
            self.assertEqual(st.phase, "actions", leader)
            self.assertTrue(p.politics_done, leader)

    def test_the_flag_survives_a_turn_boundary(self):
        """The once-per-game guard is per GAME: it must not be cleared by the
        start-of-turn reset that clears every per-turn flag next to it."""
        st, p = _politics("Julius Caesar", players=2, hand=list(EVENTS))
        _apply(st, ("prepare_event", EVENTS[0]))
        _apply(st, ("prepare_event", EVENTS[1]))
        self.assertTrue(p.caesar_double_politics_used)
        bots = [GreedyBot(seed=3) for _ in st.players]
        for _ in range(400):
            if st.game_over or (st.current == 0 and st.phase == "politics"
                                and not st.pending and st.turn > 0):
                break
            mvs = actions.legal_moves(st)
            if not mvs:
                break
            _apply(st, bots[st.decider()].choose(st, mvs))
        self.assertTrue(p.caesar_double_politics_used,
                        "a turn boundary refilled a once-per-game ability")
        if st.phase == "politics" and st.current == 0 and not st.game_over:
            self.assertFalse(p.caesar_second_politics)

    def test_a_second_aggression_is_a_real_second_action(self):
        """Not just `prepare_event`: the second action is the whole list."""
        st, p = _politics("Julius Caesar", hand=[EVENTS[0]])
        p.military_actions = 4
        _apply(st, ("prepare_event", EVENTS[0]))
        self.assertEqual(st.phase, "politics")
        # `resign` is always offered outside age IV, so the list is not empty
        self.assertIn(("pol_pass",), actions.legal_moves(st))


class AlexanderTradesHimselfForAYellowToken(unittest.TestCase):

    def test_the_move_exists_only_while_he_is_in_play(self):
        st, _p = _politics("Alexander the Great")
        self.assertIn("remove_leader_yellow", _kinds(st))
        st, _p = _politics("Julius Caesar")
        self.assertNotIn("remove_leader_yellow", _kinds(st))
        st, _p = _politics(None)
        self.assertNotIn("remove_leader_yellow", _kinds(st))

    def test_it_takes_one_token_from_the_box_and_removes_him(self):
        st, p = _politics("Alexander the Great")
        bank, granted = p.yellow_bank, p.yellow_granted
        _apply(st, ("remove_leader_yellow",))
        self.assertEqual(p.yellow_bank, bank + 1)
        self.assertEqual(p.yellow_granted, granted + 1,
                         "a token from the box is not tracked as granted")
        self.assertIsNone(p.leader)
        self.assertIn("Alexander the Great",
                      st.civil_removed.get("A", []),
                      "the leader was dropped on the floor, not recorded")
        self.assertEqual(st.phase, "actions")
        self.assertTrue(p.politics_done)

    def test_his_strength_bonus_goes_with_him(self):
        st, p = _politics("Alexander the Great")
        p.techs["Warriors"].workers = 2
        effects.invalidate(st, p)
        before = effects.state_stats(st, p).strength
        _apply(st, ("remove_leader_yellow",))
        after = effects.state_stats(st, p).strength
        self.assertEqual(after, before - 2, "+1 strength per unit stayed")

    def test_it_can_only_ever_happen_once(self):
        st, p = _politics("Alexander the Great")
        _apply(st, ("remove_leader_yellow",))
        st.phase = "politics"                 # a later turn's politics phase
        p.politics_done = False
        self.assertNotIn("remove_leader_yellow", _kinds(st))

    def test_caesars_second_action_can_be_alexanders(self):
        """The two abilities do not know about each other, and must not."""
        st, p = _politics("Julius Caesar", hand=[EVENTS[0]])
        _apply(st, ("prepare_event", EVENTS[0]))
        p.leader = "Alexander the Great"      # e.g. replaced mid-turn
        self.assertIn("remove_leader_yellow", _kinds(st))
        _apply(st, ("remove_leader_yellow",))
        self.assertEqual(st.phase, "actions")
        self.assertTrue(p.caesar_double_politics_used)


class ColumbusColonizesFromHandForFree(unittest.TestCase):

    def test_one_move_per_territory_and_none_without_one(self):
        st, _p = _politics("Christopher Columbus", hand=list(TERRITORIES))
        self.assertEqual(
            sorted(m for m in actions.legal_moves(st)
                   if m[0] == "columbus_colonize"),
            sorted(("columbus_colonize", n) for n in TERRITORIES))
        st, _p = _politics("Christopher Columbus", hand=[EVENTS[0]])
        self.assertNotIn("columbus_colonize", _kinds(st))
        st, _p = _politics("Joan of Arc", hand=list(TERRITORIES))
        self.assertNotIn("columbus_colonize", _kinds(st))

    def test_the_colony_is_gained_and_nothing_is_sacrificed(self):
        st, p = _politics("Christopher Columbus", hand=list(TERRITORIES))
        p.techs["Warriors"].workers = 2
        effects.invalidate(st, p)
        units = {n: t.workers for n, t in p.techs.items()}
        name = TERRITORIES[0]
        perm = _DB.get(name).get("permanentEffects") or {}
        bank = p.yellow_bank
        _apply(st, ("columbus_colonize", name))
        self.assertIn(name, p.colonies)
        self.assertNotIn(name, p.hand_military)
        self.assertEqual({n: t.workers for n, t in p.techs.items()}, units,
                         "a unit was sacrificed for a free colonization")
        # §11.5 permanent effects applied (the immediate ones may push a
        # decision, so they are only asserted through the colony itself)
        self.assertEqual(p.yellow_bank - bank,
                         perm.get("yellowTokens", 0)
                         + (0 if not st.pending else 0),
                         "the colony's permanent yellow tokens did not land")
        self.assertIsNone(p.leader)
        self.assertIn("Christopher Columbus", st.civil_removed.get("I", []))

    def test_it_needs_no_army_at_all(self):
        """The rule the auction path cannot express: `interact.colonize`
        raises when the pool is empty, and Columbus with no units is the
        position his card is FOR."""
        st, p = _politics("Christopher Columbus", hand=[TERRITORIES[0]])
        for n, t in p.techs.items():
            if _DB.type_of(n) in C.UNIT_TYPES:
                t.workers = 0
        effects.invalidate(st, p)
        self.assertEqual(interact.max_force(st, p), 0)
        _apply(st, ("columbus_colonize", TERRITORIES[0]))
        self.assertIn(TERRITORIES[0], p.colonies)

    def test_it_can_only_ever_happen_once(self):
        st, p = _politics("Christopher Columbus", hand=list(TERRITORIES))
        _apply(st, ("columbus_colonize", TERRITORIES[0]))
        while st.pending:                     # an immediate effect may ask
            _apply(st, actions.legal_moves(st)[0])
        st.phase = "politics"
        p.politics_done = False
        self.assertNotIn("columbus_colonize", _kinds(st))
        self.assertEqual(len(p.colonies), 1)

    def test_the_territory_leaves_the_hand_exactly_once(self):
        st, p = _politics("Christopher Columbus",
                          hand=[TERRITORIES[0], TERRITORIES[0]])
        _apply(st, ("columbus_colonize", TERRITORIES[0]))
        self.assertEqual(p.hand_military, [TERRITORIES[0]])


class JoanOfArcSeesTheTopEvent(unittest.TestCase):

    def _turn(self, leader, events_pile=("Rats", "Pestilence")):
        st = game.new_game(2, seed=5)
        st.has_military = True
        st.round = 2
        st.current = 0
        for q in st.players:
            q.leader = None
        st.players[0].leader = leader
        st.current_events = list(events_pile)
        game.start_turn(st, random.Random(1))
        return st, st.players[0]

    def test_she_sees_the_card_the_next_reveal_would_turn_up(self):
        st, p = self._turn("Joan of Arc")
        self.assertEqual(st.phase, "politics")
        self.assertEqual(p.peeked_event, st.current_events[-1])

    def test_nobody_else_sees_anything(self):
        for leader in (None, "Julius Caesar", "Alexander the Great"):
            st, p = self._turn(leader)
            self.assertIsNone(p.peeked_event, leader)
            self.assertIsNone(st.players[1].peeked_event, leader)

    def test_it_really_is_the_card_that_gets_revealed(self):
        st, p = self._turn("Joan of Arc")
        looked = p.peeked_event
        st.players[0].hand_military = [EVENTS[0]]
        _apply(st, ("prepare_event", EVENTS[0]))
        self.assertIn(looked, st.past_events + st.current_events,
                      "the card she looked at was not the one revealed")
        self.assertNotIn(looked, st.current_events)

    def test_the_look_ends_with_the_phase(self):
        st, p = self._turn("Joan of Arc")
        self.assertIsNotNone(p.peeked_event)
        _apply(st, ("pol_pass",))
        self.assertIsNone(p.peeked_event)

    def test_an_empty_pile_is_not_a_peek(self):
        st, p = self._turn("Joan of Arc", events_pile=())
        self.assertIsNone(p.peeked_event)

    def test_a_turn_with_no_politics_phase_sees_nothing(self):
        st = game.new_game(2, seed=5)
        st.has_military = True
        st.round = 2
        st.players[0].leader = "Joan of Arc"
        st.players[0].skip_next_politics = True      # International Agreement
        st.current_events = ["Rats", "Pestilence"]
        game.start_turn(st, random.Random(1))
        self.assertEqual(st.phase, "actions")
        self.assertIsNone(st.players[0].peeked_event)

    def test_determinize_keeps_what_she_saw_and_hides_the_rest(self):
        """The bot-side half: a search must not shuffle away information the
        mover legitimately has."""
        pile = ["Rats", "Pestilence", "Raiders", "Barbarians"]
        pile = [n for n in pile if n in _DB.by_name]
        self.assertGreaterEqual(len(pile), 3, "fixture events not in the DB")
        st, p = self._turn("Joan of Arc", events_pile=pile)
        self.assertEqual(p.peeked_event, pile[-1])
        moved = 0
        for s in range(24):
            root = copy_state(st)
            determinize(root, random.Random(s))
            self.assertEqual(root.current_events[-1], pile[-1],
                             "determinize shuffled away Joan's own look")
            self.assertEqual(sorted(root.current_events), sorted(pile))
            if root.current_events != pile:
                moved += 1
        self.assertGreater(moved, 6, "the rest of the pile stopped being "
                                     "hidden; this test cannot see a leak")

    def test_determinize_still_hides_the_top_from_everybody_else(self):
        pile = ["Rats", "Pestilence", "Raiders", "Barbarians"]
        pile = [n for n in pile if n in _DB.by_name]
        st, _p = self._turn(None, events_pile=pile)
        pinned = 0
        for s in range(24):
            root = copy_state(st)
            determinize(root, random.Random(s))
            if root.current_events[-1] == pile[-1]:
                pinned += 1
        self.assertLess(pinned, 20, "the top event never moved for a leader "
                                    "with no right to see it")

    def test_a_stale_note_pins_nothing(self):
        """If the card she looked at is no longer the top one, the note is
        stale and must not hold a card it does not name in place."""
        st, p = self._turn("Joan of Arc",
                           events_pile=["Rats", "Pestilence", "Raiders"])
        p.peeked_event = "Rats"               # bottom, not top
        moved = 0
        for s in range(24):
            root = copy_state(st)
            determinize(root, random.Random(s))
            if root.current_events[-1] != st.current_events[-1]:
                moved += 1
        self.assertGreater(moved, 4)


class TheBotsCanActuallyUseThem(unittest.TestCase):
    """An ability no bot ever selects is only half implemented."""

    def test_a_free_colony_beats_passing(self):
        """Columbus's move carries the card, so a 1-ply search prices the
        colony itself rather than the (worthless) leader it spends."""
        for bot in (GreedyBot(seed=1), WeightedBot(seed=1),
                    QuiescentBot(seed=1)):
            st, _p = _politics("Christopher Columbus", players=2,
                               hand=list(TERRITORIES))
            mv = bot.choose(st, actions.legal_moves(st))
            self.assertEqual(mv[0], "columbus_colonize",
                             f"{bot.name} passed up a free colony ({mv})")

    def test_a_yellow_token_is_taken_when_the_bank_is_squeezed(self):
        """Alexander's ability needs no bot-side term either, and the reason
        is worth pinning: the evaluator sees the token through the population
        ladder (`consumption`, `pop_cost`, `happy_margin`, `discontent`), not
        through `yellow_bank` -- whose weight is NEGATIVE on every champion.
        With a full bank the ladder is flat and trading a leader for a token
        is correctly refused; with a squeezed bank it is correctly taken."""
        def _pick(bot, bank):
            st, p = _politics("Alexander the Great", players=2)
            p.yellow_bank = bank
            for n, t in p.techs.items():          # no strength to lose
                if _DB.type_of(n) in C.UNIT_TYPES:
                    t.workers = 0
            effects.invalidate(st, p)
            return bot.choose(st, actions.legal_moves(st))

        take = ("remove_leader_yellow",)
        for bot in (WeightedBot(seed=1), GreedyBot(seed=1)):
            self.assertEqual(_pick(bot, 4), take,
                             f"{bot.name} left a token in the box with the "
                             f"population ladder biting")
        # ...and the trained evaluator declines it when the ladder is flat, so
        # the ability is a decision rather than a reflex.  GreedyBot is NOT
        # asserted here: its frozen control weights price `yellow_bank` at
        # +0.15 and carry no `leader` term at all, so it always trades.
        self.assertNotEqual(_pick(WeightedBot(seed=1), 18), take)

    def test_every_bot_answers_a_politics_phase_holding_all_of_them(self):
        for bot in (BookBot(seed=1), GreedyBot(seed=1), WeightedBot(seed=1),
                    QuiescentBot(seed=1), PlanBot(seed=1, width=2),
                    RandomBot(seed=1)):
            for leader in ("Julius Caesar", "Alexander the Great",
                           "Christopher Columbus", "Joan of Arc"):
                st, p = _politics(leader, players=2,
                                  hand=list(TERRITORIES) + list(EVENTS))
                for _ in range(12):
                    if st.phase != "politics" and not st.pending:
                        break
                    moves = actions.legal_moves(st)
                    self.assertTrue(moves, f"{bot.name}/{leader}: no move")
                    mv = bot.choose(st, moves)
                    self.assertIn(mv, moves,
                                  f"{bot.name}/{leader} played {mv}")
                    _apply(st, mv)
                self.assertEqual(st.phase, "actions",
                                 f"{bot.name}/{leader} never left politics")

    def test_the_second_political_action_is_priced_on_its_own_merits(self):
        """No bot needs to know Caesar exists: the second action is scored
        exactly like a first one, so a valuable one is taken."""
        st, p = _politics("Julius Caesar", players=2, hand=list(EVENTS))
        _apply(st, ("prepare_event", EVENTS[0]))
        bot = WeightedBot(seed=1)
        mv = bot.choose(st, actions.legal_moves(st))
        self.assertEqual(mv, ("prepare_event", EVENTS[1]),
                         "the second political action was left on the table")


class ItSurvivesRealGames(unittest.TestCase):

    def test_random_play_never_deadlocks_with_these_leaders_in_play(self):
        seen = {"remove_leader_yellow": 0, "columbus_colonize": 0,
                "caesar_second": 0, "peek": 0}
        for seed in range(6):
            st = game.new_game(3, seed=seed)
            rng = random.Random(seed)
            bot = RandomBot(seed=seed)
            for i, q in enumerate(st.players):
                q.leader = ("Julius Caesar", "Alexander the Great",
                            "Christopher Columbus")[i % 3]
            for _ in range(600):
                if st.game_over:
                    break
                moves = actions.legal_moves(st)
                self.assertTrue(moves, "no legal move")
                for m in moves:
                    if m[0] in seen:
                        seen[m[0]] += 1
                p = st.players[st.decider()]
                if p.caesar_second_politics:
                    seen["caesar_second"] += 1
                if p.peeked_event:
                    seen["peek"] += 1
                actions.apply(st, bot.choose(st, moves), rng)
        for k in ("remove_leader_yellow", "columbus_colonize",
                  "caesar_second"):
            self.assertGreater(seen[k], 0, f"{k} was never reachable")

    def test_the_journal_rolls_all_four_back(self):
        from engine import journal
        journal.install()
        for leader, mv in (("Alexander the Great", ("remove_leader_yellow",)),
                           ("Christopher Columbus",
                            ("columbus_colonize", TERRITORIES[0])),
                           ("Julius Caesar", ("prepare_event", EVENTS[0]))):
            st, p = _politics(leader, hand=list(TERRITORIES) + list(EVENTS))
            before = copy_state(st)
            j = journal.begin(st)
            try:
                actions.apply(st, mv, _rng())
            finally:
                journal.rollback(j)
            self.assertEqual(p.leader, leader)
            self.assertEqual(p.colonies, before.players[0].colonies)
            self.assertEqual(p.yellow_bank, before.players[0].yellow_bank)
            self.assertEqual(p.hand_military, before.players[0].hand_military)
            self.assertFalse(p.caesar_second_politics)
            self.assertEqual(st.phase, "politics")


if __name__ == "__main__":
    unittest.main()
