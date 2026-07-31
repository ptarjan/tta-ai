"""Rulebook conformance for the mechanics docs/COVERAGE_AUDIT.md measured.

Every test here is a CONSTRUCTED position, not a self-play sample: the
audit's whole point is that a mechanic can be broken for a hundred games
without a single self-play run noticing, because the bots never use it.

Each test names the rule it encodes (docs/RULES_SPEC.md section, and the
source line where the wording is unambiguous).
"""
from __future__ import annotations

import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from engine import actions, cards as C, effects, game, interact  # noqa: E402

actions.STRICT = True


def _mid_turn(players=2, seed=5):
    """A player on a normal action phase of round 3, actions unspent."""
    st = game.new_game(players, seed=seed)
    st.round = 3
    st.phase = "actions"
    st.current = 0
    st.has_military = True
    p = st.players[0]
    s = effects.state_stats(st, p)
    p.civil_actions = s.civil_actions
    p.military_actions = s.military_actions
    return st, p


# ------------------------------------------------------------ §8 governments

class TestRevolution(unittest.TestCase):
    """RULES_SPEC 8.3.4, RB p.13 (sources/ubg_the-second-round.txt:271):

    'Your military actions are not affected.  You may spend any of them
    before or after the revolution, and any that you gain from the new
    government will be available to spend.'
    """

    def test_revolution_grants_the_new_governments_military_actions(self):
        st, p = _mid_turn()
        p.science = 99
        p.hand_civil = ["Monarchy"]
        self.assertEqual(p.military_actions, 2)          # Despotism
        actions.apply(st, ("revolution", "Monarchy"))
        self.assertEqual(p.government, "Monarchy")
        self.assertEqual(effects.state_stats(st, p).military_actions, 3)
        # all civil actions are spent (8.3.4), the military ones are not
        self.assertEqual(p.civil_actions, 0)
        self.assertEqual(p.military_actions, 3)

    def test_revolution_keeps_military_actions_already_spent(self):
        st, p = _mid_turn()
        p.science = 99
        p.hand_civil = ["Monarchy"]
        p.military_actions = 1                            # one already spent
        actions.apply(st, ("revolution", "Monarchy"))
        # 3 total on Monarchy, 1 of the 2 Despotism ones spent -> 2 left
        self.assertEqual(p.military_actions, 2)

    def test_peaceful_change_already_grants_them(self):
        """The contrast that makes the bug above unambiguous."""
        st, p = _mid_turn()
        p.science = 99
        p.hand_civil = ["Monarchy"]
        actions.apply(st, ("develop", "Monarchy"))
        self.assertEqual(p.military_actions, 3)

    def test_robespierre_revolution_grants_the_new_civil_actions(self):
        """CoL p.12: Robespierre pays with military actions instead, so it is
        the CIVIL actions that are 'not affected' and must include the new
        government's extras."""
        st, p = _mid_turn()
        p.leader = "Maximilien Robespierre"
        effects.invalidate(st, p)
        s = effects.state_stats(st, p)
        p.civil_actions, p.military_actions = s.civil_actions, s.military_actions
        p.science = 99
        p.hand_civil = ["Monarchy"]
        actions.apply(st, ("revolution", "Monarchy"))
        self.assertEqual(p.military_actions, 0)           # all MAs spent
        self.assertEqual(p.civil_actions, 5)              # Monarchy's 5 CAs


# ------------------------------------------------ §2.5 / §7.1 one card per name

class TestOnePerName(unittest.TestCase):
    """RULES_SPEC 2.5 / 7.1, RB p.9 (sources/ubg_the-second-round.txt:83):

    'You may never take a TECHNOLOGY card with the same name as a technology
    you already have in your hand or in play.'

    A technology is a civil card with a science cost (7.1).  Action cards have
    no science cost, are not technologies, and several of them exist in two or
    three copies in the same deck -- so holding one must not block taking the
    other.
    """

    def test_a_second_copy_of_an_action_card_may_be_taken(self):
        st, p = _mid_turn()
        name = "Rich Land (I)"
        self.assertGreater(C.db().get(name)["count"]["2p"], 1,
                           "test needs a card that exists in two copies")
        st.card_row = [None] * actions.ROW_SIZE
        st.card_row[0] = name
        p.hand_civil = [name]
        self.assertTrue(actions.can_take(st, p, 0))

    def test_a_second_copy_of_a_technology_may_not(self):
        st, p = _mid_turn()
        st.card_row = [None] * actions.ROW_SIZE
        st.card_row[0] = "Irrigation"
        p.hand_civil = ["Irrigation"]
        self.assertFalse(actions.can_take(st, p, 0))

    def test_a_technology_already_in_play_still_blocks(self):
        st, p = _mid_turn()
        st.card_row = [None] * actions.ROW_SIZE
        st.card_row[0] = "Bronze"                  # on the player board
        p.hand_civil = []
        self.assertFalse(actions.can_take(st, p, 0))

    def test_the_current_government_still_blocks(self):
        st, p = _mid_turn()
        st.card_row = [None] * actions.ROW_SIZE
        st.card_row[0] = "Despotism"
        self.assertFalse(actions.can_take(st, p, 0))


# ----------------------------------------------------------------- §11 colonies

def _military_state(seed=21, players=3):
    st = game.new_game(players, seed=seed)
    st.round = 3
    st.phase = "politics"
    st.has_military = True
    return st


def _strip_units(state, keep=()):
    db = C.db()
    for q in state.players:
        if q.idx in keep:
            continue
        for n, t in q.techs.items():
            if db.type_of(n) in C.UNIT_TYPES:
                t.workers = 0
        effects.invalidate(state, q)


class TestColonyEffects(unittest.TestCase):
    """RULES_SPEC 11.5: the permanent effects are the bottom symbols --
    ratings as well as tokens -- and they are exactly what changes hands when
    the colony does."""

    def test_permanent_rating_symbols_apply_while_the_colony_is_held(self):
        st = _military_state()
        p = st.players[0]
        base = effects.compute(st, p)
        interact.gain_colony(st, p, "Strategic Territory (I)")   # +2 strength
        self.assertEqual(effects.compute(st, p).strength, base.strength + 2)
        interact.gain_colony(st, p, "Historic Territory (II)")   # +2 happy
        self.assertEqual(effects.compute(st, p).happy, base.happy + 2)
        interact.lose_colony(st, p, "Strategic Territory (I)")
        self.assertEqual(effects.compute(st, p).strength, base.strength)

    def test_annex_moves_the_permanent_effects_and_not_the_one_time_one(self):
        """RULES_SPEC 5.5 / 11.5: Annex takes the colony's permanent effects."""
        st = _military_state()
        thief, victim = st.players[0], st.players[1]
        interact.gain_colony(st, victim, "Wealthy Territory (I)")
        res_before = thief.resources
        blue_v = victim.blue_total
        interact.push_choice(st, thief.idx, "annex", ["Wealthy Territory (I)"],
                             {"victim": victim.idx})
        self.assertIn("Wealthy Territory (I)", thief.colonies)
        self.assertNotIn("Wealthy Territory (I)", victim.colonies)
        self.assertEqual(victim.blue_total, blue_v - 3)   # permanent moves
        self.assertEqual(thief.resources, res_before)     # one-time does not

    def test_the_auction_only_ends_when_the_high_bidder_is_alone(self):
        """RULES_SPEC 11.2: pass = out permanently; last bidder standing wins."""
        st = _military_state()
        for q in st.players:
            q.techs["Warriors"].workers = 3
            effects.invalidate(st, q)
        interact.start_auction(st, "Wealthy Territory (I)", 0)
        self.assertEqual(st.pending[-1]["active"], [0, 1, 2])
        actions.apply(st, ("bid", 1))                     # P0
        self.assertEqual(st.decider(), 1)
        actions.apply(st, ("bid", 2))                     # P1 outbids
        self.assertEqual(st.decider(), 2)
        actions.apply(st, ("bid_pass",))                  # P2 out
        self.assertEqual(st.decider(), 0)                 # back to P0, not P1
        actions.apply(st, ("bid_pass",))                  # P0 out -> P1 wins
        # winning the auction does not end the decision: §11.3 hands the
        # winner the choice of WHICH units make up the force it now owes
        self.assertEqual(st.pending[-1]["kind"], "colonize")
        self.assertEqual(st.decider(), 1)
        while st.pending:
            actions.apply(st, actions.legal_moves(st)[0])
        self.assertIn("Wealthy Territory (I)", st.players[1].colonies)

    def test_the_sacrificed_units_form_armies_for_the_force(self):
        """RULES_SPEC 10.7 / 11.3: only the sacrificed units form armies, and
        their tactical strength counts toward the colonization force."""
        st = _military_state()
        p = st.players[0]
        p.techs["Warriors"].workers = 3
        effects.invalidate(st, p)
        plain = interact.max_force(st, p)
        p.tactic = "Fighting Band"                # infantry/infantry
        effects.invalidate(st, p)
        self.assertGreater(interact.max_force(st, p), plain)

    def test_bonus_cards_are_discarded_before_a_strategic_territory_draws(self):
        """RULES_SPEC 11.6 / FAQ p.11: so they can be reshuffled into the deck
        the draw comes from."""
        st = _military_state()
        p = st.players[0]
        _strip_units(st, keep=(0,))
        p.techs["Warriors"].workers = 1
        bonus = next(c["name"] for c in C.db().cards
                     if c["type"] == "bonus" and c["age"] == "I")
        p.hand_military = [bonus]
        st.age_military = "I"
        st.military_deck = []                     # only the discards exist
        st.discarded_military = {}
        effects.invalidate(st, p)
        # force 2 needs the unit (1) AND the bonus card (1), so the bonus is
        # spent; the territory then draws 3 from an EMPTY deck, which can only
        # succeed if the bonus already reached the discard pile.
        self.assertEqual(interact.max_force(st, p), 2)
        interact.colonize(st, p, "Strategic Territory (I)", 2)
        self.assertEqual(p.hand_military, [bonus],
                         "the spent bonus card was not available to redraw")


class TestColonyForceRules(unittest.TestCase):
    def test_a_player_with_no_units_is_not_in_the_auction(self):
        st = _military_state()
        _strip_units(st, keep=(1,))
        st.players[1].techs["Warriors"].workers = 1
        effects.invalidate(st)
        interact.start_auction(st, "Vast Territory (I)", 0)
        self.assertEqual(st.pending[-1]["active"], [1])

    def test_at_least_one_unit_is_always_sacrificed(self):
        """RULES_SPEC 11.3: >= 1 unit even if bonuses would cover the bid."""
        st = _military_state()
        p = st.players[0]
        _strip_units(st, keep=(0,))
        p.techs["Warriors"].workers = 2
        effects.invalidate(st, p)
        before = p.techs["Warriors"].workers
        bonus = next(c["name"] for c in C.db().cards
                     if c["type"] == "bonus" and c["age"] == "I")
        p.hand_military = [bonus]                 # colonization value alone
        interact.colonize(st, p, "Vast Territory (I)", 1)
        pend = st.pending[-1]
        # the mandatory unit had only one possible identity, so it needed no
        # decision; what is left to decide is whether to spend more
        self.assertEqual(pend["kind"], "colonize")
        self.assertEqual(pend["units"], ["Warriors"])
        # and there is no route to `send_done` from a force holding no unit,
        # even though the bonus card's colonization value would cover the bid
        empty = dict(pend, units=[], bonuses=[bonus],
                     pool=["Warriors", "Warriors"], bpool=[])
        self.assertNotIn(("send_done",), interact._colonize_moves(st, empty))
        actions.apply(st, ("send_done",))
        self.assertEqual(p.techs["Warriors"].workers, before - 1)
        self.assertEqual(p.hand_military, [bonus])   # the card was not spent


if __name__ == "__main__":
    unittest.main()
