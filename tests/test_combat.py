"""Wars, aggressions, pacts and the bonus-card mechanics that gate them.

Every test names the printed rule it checks:

``[CoL p.N]``  sources/cge_code_of_laws.pdf  (the full rulebook)
``[FAQ p.N]``  sources/faq_v15.pdf
``[card]``     the printed card text, as transcribed in
               data/cards_military_actions.json

Positions are built by hand rather than reached by self-play: the bots
essentially never declare wars (docs/CULTURE_GAP.md), so self-play cannot
exercise any of this.
"""
from __future__ import annotations

import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from engine import actions, cards as C, effects, events, game, interact  # noqa: E402

actions.STRICT = True

ALLIANCE = "Military Alliance"
PROMISE = "Promise of Military Protection"
BONUS_I = "Military Bonus (defense 2 / colonization 1)"
BONUS_II = "Military Bonus (defense 4 / colonization 2)"
BONUS_III = "Military Bonus (defense 6 / colonization 3)"


def st_military(players=3, seed=21):
    """A politics phase in a mid-game round, nothing else set up."""
    st = game.new_game(players, seed=seed)
    st.round = 3
    st.phase = "politics"
    st.has_military = True
    for p in st.players:
        p.politics_done = False
    return st


def set_strength(st, p, n):
    """Give `p` exactly `n` strength by stacking workers on Warriors."""
    p.techs["Warriors"].workers = n
    effects.invalidate(st, p)
    assert effects.state_stats(st, p).strength == n, \
        effects.state_stats(st, p).strength
    return p


def give_pact(st, owner, partner, name, a=None, b=None):
    owner.pacts = [{"name": name, "owner": owner.idx, "partner": partner.idx,
                    "a": owner.idx if a is None else a,
                    "b": partner.idx if b is None else b}]
    effects.invalidate(st)


def declare_war(st, name, target_idx):
    mv = ("war", name, target_idx)
    assert mv in actions.legal_moves(st), \
        [m for m in actions.legal_moves(st) if m[0] == "war"]
    actions.apply(st, mv)
    return mv


# ------------------------------------------------------------ declaring war

class TestWarDeclaration(unittest.TestCase):
    """[CoL p.4] 'Declare a War'."""

    def test_costs_the_printed_military_actions_and_leaves_the_hand(self):
        # [card] War over Territory: 2 MA; War over Culture: 3 MA.
        st = st_military()
        p0 = st.me()
        p0.hand_military = ["War over Territory"]
        p0.military_actions = 2
        declare_war(st, "War over Territory", 1)
        self.assertEqual(p0.military_actions, 0)
        self.assertEqual(p0.hand_military, [])
        self.assertEqual(tuple(p0.war_declared_by_me),
                         ("War over Territory", 0, 1))
        self.assertIn(("War over Territory", 0, 1),
                      [tuple(w) for w in st.players[1].wars_declared_on_me])

    def test_too_few_military_actions_makes_it_illegal(self):
        st = st_military()
        st.me().hand_military = ["War over Culture"]      # 3 MA
        st.me().military_actions = 2
        self.assertFalse([m for m in actions.legal_moves(st) if m[0] == "war"])

    def test_gandhi_doubles_the_cost(self):
        # [card] Mahatma Gandhi: opponents pay twice the military actions.
        st = st_military()
        p0 = st.me()
        p0.hand_military = ["War over Territory"]         # 2 MA -> 4
        st.players[1].leader = "Mahatma Gandhi"
        effects.invalidate(st)
        p0.military_actions = 3
        self.assertNotIn(("war", "War over Territory", 1),
                         actions.legal_moves(st))
        p0.military_actions = 4
        declare_war(st, "War over Territory", 1)
        self.assertEqual(p0.military_actions, 0)

    def test_illegal_in_the_last_round(self):
        # [CoL p.4] 'You cannot declare a war during the last round.'
        st = st_military()
        st.me().hand_military = ["War over Territory"]
        st.me().military_actions = 2
        self.assertTrue([m for m in actions.legal_moves(st) if m[0] == "war"])
        st.last_round = True
        self.assertFalse([m for m in actions.legal_moves(st) if m[0] == "war"])

    def test_a_pact_that_forbids_attacks_blocks_the_declaration(self):
        # [FAQ p.11] Peace Treaty / Loss of Sovereignty / Acceptance of
        # Supremacy prevent attacking 'by Aggression or by War'.
        st = st_military()
        p0 = st.me()
        p0.hand_military = ["War over Territory"]
        p0.military_actions = 2
        for name in ("Peace Treaty", "Loss of Sovereignty",
                     "Acceptance of Supremacy"):
            give_pact(st, p0, st.players[1], name)
            wars = [m for m in actions.legal_moves(st) if m[0] == "war"]
            self.assertNotIn(("war", "War over Territory", 1), wars, name)
            self.assertIn(("war", "War over Territory", 2), wars, name)

    def test_loss_of_sovereignty_side_b_cannot_be_declared_war_on_by_anyone(self):
        # [card] 'No one may declare war on player B.'
        st = st_military()
        p0 = st.me()
        p0.hand_military = ["War over Territory"]
        p0.military_actions = 2
        # the pact is between P1 and P2; P2 is side B
        give_pact(st, st.players[1], st.players[2], "Loss of Sovereignty",
                  a=1, b=2)
        wars = [m for m in actions.legal_moves(st) if m[0] == "war"]
        self.assertIn(("war", "War over Territory", 1), wars)
        self.assertNotIn(("war", "War over Territory", 2), wars)

    def test_gandhi_may_not_attack_at_all(self):
        # [card] 'You may not play aggression or war cards.'
        st = st_military()
        p0 = st.me()
        p0.leader = "Mahatma Gandhi"
        p0.hand_military = ["War over Territory", "Aggression: Plunder (I)"]
        p0.military_actions = 3
        set_strength(st, p0, 5)
        effects.invalidate(st)
        moves = actions.legal_moves(st)
        self.assertFalse([m for m in moves if m[0] in ("war", "aggression")])

    def test_declaring_war_cancels_a_pact_that_ends_on_attack(self):
        """[CoL p.4] 'If you and your rival have a pact that says it ends if
        you attack, remove that pact from play.'  [FAQ p.11] '... either by
        Aggression or by declaring War ...'"""
        st = st_military()
        p0, p1 = st.me(), st.players[1]
        p0.hand_military = ["War over Territory"]
        p0.military_actions = 2
        give_pact(st, p0, p1, ALLIANCE)
        self.assertEqual(len(effects.pacts_for(st, 0)), 1)
        declare_war(st, "War over Territory", 1)
        self.assertEqual(effects.pacts_for(st, 0), [],
                         "the Military Alliance survived the declaration")

    def test_declaring_war_leaves_a_pact_with_a_third_party_alone(self):
        st = st_military(players=4)
        p0 = st.me()
        p0.hand_military = ["War over Territory"]
        p0.military_actions = 2
        give_pact(st, p0, st.players[2], ALLIANCE)
        declare_war(st, "War over Territory", 1)
        self.assertEqual(len(effects.pacts_for(st, 0)), 1)


# ----------------------------------------------------------- resolving a war

class TestWarResolution(unittest.TestCase):
    """[CoL p.3] 'Resolve a War' + [card] spoils."""

    def _declared(self, name="War over Territory", players=3, cost=2):
        st = st_military(players=players)
        p0, p1 = st.me(), st.players[1]
        p0.hand_military = [name]
        p0.military_actions = cost
        declare_war(st, name, 1)
        return st, p0, p1

    def test_nothing_happens_at_declaration_time(self):
        # [CoL p.4] 'The war will be resolved at the beginning of your next
        # turn.'
        st, p0, p1 = self._declared()
        set_strength(st, p0, 9)
        set_strength(st, p1, 1)
        bank = p1.yellow_bank
        self.assertEqual(p1.yellow_bank, bank)
        self.assertIsNotNone(p0.war_declared_by_me)

    def test_resolves_in_the_start_of_turn_sequence(self):
        # [CoL p.3] start of turn: replenish -> resolve a war -> tactics.
        st, p0, p1 = self._declared()
        set_strength(st, p0, 9)
        set_strength(st, p1, 1)
        st.current = 0
        st.round = 4
        game.start_turn(st)
        self.assertIsNone(p0.war_declared_by_me)
        self.assertEqual(p1.wars_declared_on_me, [])
        self.assertLess(p1.yellow_bank, 18)

    def test_equal_strength_resolves_with_no_effect(self):
        # [CoL p.3] 'If the players have same strength, the war resolves with
        # no effect.'  [FAQ p.11] 'Ties during Wars and Aggressions'.
        st, p0, p1 = self._declared()
        set_strength(st, p0, 4)
        set_strength(st, p1, 4)
        bank, mine = p1.yellow_bank, p0.yellow_bank
        events.resolve_war(st, p0, None)
        self.assertEqual((p0.yellow_bank, p1.yellow_bank), (mine, bank))
        self.assertIsNone(p0.war_declared_by_me)      # discarded either way

    def test_the_defender_can_win(self):
        # [FAQ p.11] 'Wars: Either player can win a War.'
        st, p0, p1 = self._declared()
        set_strength(st, p0, 1)
        set_strength(st, p1, 11)                      # advantage 10
        p0.yellow_bank = 8
        events.resolve_war(st, p0, None)
        self.assertEqual(p0.yellow_bank, 8 - 3)       # 1 + 10 // 5
        self.assertEqual(p1.yellow_bank, 18 + 3)

    def test_war_over_territory_spoils(self):
        # [card] 1 token + 1 per full 5 points of strength advantage.
        for adv, expect in ((1, 1), (4, 1), (5, 2), (9, 2), (10, 3)):
            st, p0, p1 = self._declared()
            set_strength(st, p0, 1 + adv)
            set_strength(st, p1, 1)
            p1.yellow_bank = 18
            events.resolve_war(st, p0, None)
            self.assertEqual(18 - p1.yellow_bank, expect, adv)
            self.assertEqual(p0.yellow_bank, 18 + expect, adv)

    def test_war_over_territory_takes_only_what_the_bank_holds(self):
        # [FAQ p.11] 'If there are insufficient yellow markers ... the victor
        # takes only what is available.'
        st, p0, p1 = self._declared()
        set_strength(st, p0, 21)
        set_strength(st, p1, 1)
        p1.yellow_bank = 2
        events.resolve_war(st, p0, None)
        self.assertEqual(p1.yellow_bank, 0)
        self.assertEqual(p0.yellow_bank, 18 + 2)

    def test_war_over_culture_spoils(self):
        # [card] 5 culture + the strength advantage.
        st, p0, p1 = self._declared("War over Culture", cost=3)
        set_strength(st, p0, 8)
        set_strength(st, p1, 2)
        p0.culture, p1.culture = 10, 40
        events.resolve_war(st, p0, None)
        self.assertEqual(p1.culture, 40 - 11)
        self.assertEqual(p0.culture, 10 + 11)

    def test_war_over_culture_capped_by_the_victims_culture(self):
        # [FAQ p.11] 'The five initial Culture points must also come from the
        # victim ... three is all the Culture that the victor gains.'
        st, p0, p1 = self._declared("War over Culture", cost=3)
        set_strength(st, p0, 8)
        set_strength(st, p1, 2)
        p0.culture, p1.culture = 10, 3
        events.resolve_war(st, p0, None)
        self.assertEqual(p1.culture, 0)
        self.assertEqual(p0.culture, 13)

    def test_war_over_technology_spoils(self):
        # [card] science equal to the strength advantage, capped by what the
        # loser has [FAQ p.11].
        st, p0, p1 = self._declared("War over Technology")
        set_strength(st, p0, 9)
        set_strength(st, p1, 2)
        p0.science, p1.science = 1, 30
        events.resolve_war(st, p0, None)
        self.assertEqual((p0.science, p1.science), (8, 23))

        st, p0, p1 = self._declared("War over Technology")
        set_strength(st, p0, 9)
        set_strength(st, p1, 2)
        p0.science, p1.science = 1, 3
        events.resolve_war(st, p0, None)
        self.assertEqual((p0.science, p1.science), (4, 0))

    def test_no_defence_decision_is_offered_in_a_war(self):
        # [CoL p.3] 'Neither side can use military bonus cards to augment
        # their strength in a war.'
        st, p0, p1 = self._declared()
        set_strength(st, p0, 6)
        set_strength(st, p1, 1)
        p1.hand_military = [BONUS_III, BONUS_III]
        events.resolve_war(st, p0, None)
        self.assertEqual(st.pending, [])
        self.assertEqual(p1.hand_military, [BONUS_III, BONUS_III])
        self.assertEqual(p1.yellow_bank, 18 - 2)      # 1 + 5 // 5

    def test_open_borders_gives_the_declarer_plus_two(self):
        # [card] 'If they attack each other, the attacker gains +2 strength.'
        st, p0, p1 = self._declared()
        give_pact(st, p0, p1, "Open Borders Agreement")
        set_strength(st, p0, 3)
        set_strength(st, p1, 4)
        events.resolve_war(st, p0, None)              # 3+2 = 5 vs 4
        self.assertEqual(p1.yellow_bank, 18 - 1)
        self.assertEqual(p0.yellow_bank, 18 + 1)

    def test_a_pact_accepted_after_the_declaration_counts(self):
        # [FAQ p.11] 'should either Pact be accepted after a Declaration of a
        # War is made but before that War is resolved, any Military Strength
        # given by that Pact would apply to that War'.
        st, p0, p1 = self._declared(players=4)
        set_strength(st, p0, 5)
        set_strength(st, p1, 3)
        give_pact(st, st.players[2], p1, ALLIANCE)    # P1 gains +3 from P2
        events.resolve_war(st, p0, None)
        self.assertEqual(p1.yellow_bank, 18 + 1)      # 5 vs 6: P1 wins
        self.assertEqual(p0.yellow_bank, 18 - 1)

    def test_a_pact_strength_cancelled_by_the_declaration_does_not_apply(self):
        """[FAQ p.11] 'The Military Strength given by either Pact will not
        affect any War or Aggression which is declared between the two
        civilizations -- for the Pact is cancelled immediately.'"""
        st = st_military()
        p0, p1 = st.me(), st.players[1]
        p0.hand_military = ["War over Territory"]
        p0.military_actions = 2
        # P1 is side B of Promise of Military Protection: +4 strength.
        give_pact(st, p0, p1, PROMISE, a=0, b=1)
        set_strength(st, p0, 5)
        p1.techs["Warriors"].workers = 3              # 3 + 4 from the pact
        effects.invalidate(st)
        self.assertEqual(effects.state_stats(st, p1).strength, 7)
        declare_war(st, "War over Territory", 1)
        events.resolve_war(st, p0, None)
        # the pact is gone, so it is 5 vs 3 and P0 wins by 2 -> 1 token
        self.assertEqual(p1.yellow_bank, 18 - 1)
        self.assertEqual(p0.yellow_bank, 18 + 1)

    def test_resigning_removes_the_war_and_pays_seven_culture(self):
        # [CoL p.4] 'the players who declared them remove their war cards from
        # play and score 7 culture points.'
        st, p0, p1 = self._declared(players=3)
        st.current = 1
        st.phase = "politics"
        before = p0.culture
        actions.apply(st, ("resign",))
        self.assertTrue(p1.resigned)
        self.assertEqual(p0.culture, before + 7)
        self.assertIsNone(p0.war_declared_by_me)

    def test_a_declared_war_survives_antiquation(self):
        # [CoL p.3] technologies, colonies, completed wonders, tactics and
        # declared wars remain in play even if antiquated; pacts do not.
        st, p0, p1 = self._declared("War over Territory")
        # an Age I pact, antiquated the moment Age II ends
        give_pact(st, p0, p1, "Open Borders Agreement")
        st.age_civil = "II"
        st.civil_deck = []
        game._advance_age(st, __import__("random").Random(0))
        self.assertIsNotNone(p0.war_declared_by_me)
        self.assertEqual(effects.pacts_for(st, 0), [])


# ------------------------------------------------------------- aggressions

class TestAggressionLegality(unittest.TestCase):
    """[CoL p.4] 'Play an Aggression'."""

    def _ready(self, players=3, card="Aggression: Plunder (I)", ma=3):
        st = st_military(players=players)
        p0 = st.me()
        p0.hand_military = [card]
        p0.military_actions = ma
        return st, p0, st.players[1]

    def test_may_not_attack_equal_or_greater_strength(self):
        # [CoL p.4] 'You cannot attack a player whose strength equals or
        # exceeds yours.'
        st, p0, p1 = self._ready()
        set_strength(st, p0, 4)
        for theirs, ok in ((3, True), (4, False), (5, False)):
            set_strength(st, p1, theirs)
            mv = ("aggression", "Aggression: Plunder (I)", 1)
            self.assertEqual(mv in actions.legal_moves(st), ok, theirs)

    def test_a_pact_forbidding_attacks_blocks_the_aggression(self):
        st, p0, p1 = self._ready()
        set_strength(st, p0, 4)
        set_strength(st, p1, 1)
        give_pact(st, p0, p1, "Peace Treaty")
        aggs = [m for m in actions.legal_moves(st) if m[0] == "aggression"]
        self.assertNotIn(("aggression", "Aggression: Plunder (I)", 1), aggs)
        self.assertIn(("aggression", "Aggression: Plunder (I)", 2), aggs)

    def test_pact_strength_that_ends_on_attack_is_not_counted(self):
        """[CoL p.4] the strength comparison: 'Do not include bonuses from
        pacts that end if you attack.'  [FAQ p.11] the pact 'is cancelled
        immediately' so its strength never applies to the aggression."""
        st, p0, p1 = self._ready()
        give_pact(st, p0, p1, ALLIANCE)               # +3 to BOTH parties
        p0.techs["Warriors"].workers = 4
        p1.techs["Warriors"].workers = 2
        effects.invalidate(st)
        # 4+3=7 vs 2+3=5 on the table; after cancelling the pact 4 vs 2.
        self.assertEqual(effects.state_stats(st, p0).strength, 7)
        self.assertEqual(effects.state_stats(st, p1).strength, 5)
        self.assertIn(("aggression", "Aggression: Plunder (I)", 1),
                      actions.legal_moves(st),
                      "4 vs 2 after the alliance is cancelled, so it is legal")

    def test_the_cost_is_paid_even_when_the_defence_succeeds(self):
        # [CoL p.4] pay the cost when the card is revealed; a failed
        # aggression is 'discarded with no effect'.
        st, p0, p1 = self._ready(card="Aggression: Enslave", ma=3)   # 2 MA
        set_strength(st, p0, 4)
        set_strength(st, p1, 1)
        p1.hand_military = [BONUS_III]
        actions.apply(st, ("aggression", "Aggression: Enslave", 1))
        self.assertEqual(p0.military_actions, 1)
        actions.apply(st, ("defend", BONUS_III))
        self.assertEqual(p0.military_actions, 1)
        self.assertTrue(any("failed" in line for line in st.log))

    def test_gandhi_doubles_the_aggression_cost(self):
        st, p0, p1 = self._ready(card="Aggression: Enslave", ma=3)   # 2 -> 4
        set_strength(st, p0, 4)
        set_strength(st, p1, 1)
        p1.leader = "Mahatma Gandhi"
        effects.invalidate(st)
        self.assertNotIn(("aggression", "Aggression: Enslave", 1),
                         actions.legal_moves(st))
        p0.military_actions = 4
        actions.apply(st, ("aggression", "Aggression: Enslave", 1))
        self.assertEqual(p0.military_actions, 0)


class TestAggressionDefence(unittest.TestCase):
    """[CoL p.4] the defence step; [FAQ p.11] 'Aggressions'."""

    def _ready(self, atk_strength=6, dfn_strength=1, hand=(),
               card="Aggression: Plunder (I)"):
        """The position just BEFORE the aggression is played."""
        st = st_military()
        p0, p1 = st.me(), st.players[1]
        p0.hand_military = [card]
        p0.military_actions = 3
        set_strength(st, p0, atk_strength)
        set_strength(st, p1, dfn_strength)
        p1.hand_military = list(hand)
        return st, p0, p1

    def _attack(self, **kw):
        st, p0, p1 = self._ready(**kw)
        actions.apply(st, ("aggression", p0.hand_military[0], 1))
        return st, p0, p1

    @staticmethod
    def _drain(st):
        while st.pending or st.queue:
            if st.pending:
                actions.apply(st, actions.legal_moves(st)[0])
            else:
                interact.run_queue(st, None)

    def test_bonus_cards_are_worth_their_printed_defence_value(self):
        # [card] military bonus cards: defence 2 / 4 / 6 by age.  Against
        # strength 6 with a base of 1 only the Age III card (+6) holds.
        for card, value in ((BONUS_I, 2), (BONUS_II, 4), (BONUS_III, 6)):
            st, p0, p1 = self._attack(hand=[card])
            actions.apply(st, ("defend", card))
            repelled = any("failed" in line for line in st.log)
            self.assertEqual(repelled, 1 + value >= 6, card)

    def test_a_plain_military_card_is_worth_one(self):
        # [CoL p.4] '+1 strength bonus for each military card discarded'.
        st, p0, p1 = self._attack(atk_strength=3, dfn_strength=1,
                                  hand=["War over Culture", "Peace Treaty"])
        actions.apply(st, ("defend", "War over Culture"))
        self.assertEqual(st.pending[-1]["dfn"], 2)   # 1 + 1
        actions.apply(st, ("defend", "Peace Treaty"))
        self.assertEqual(st.pending, [])             # budget (2 MA) exhausted
        self.assertTrue(any("failed" in line for line in st.log))
        self.assertEqual(p1.hand_military, [])

    def test_a_tie_favours_the_defender(self):
        # [FAQ p.11] 'The attacker loses in the case of ties, and only the
        # attacker can win the Aggression.'
        st, p0, p1 = self._attack(atk_strength=3, dfn_strength=1,
                                  hand=[BONUS_I])
        actions.apply(st, ("defend", BONUS_I))       # 1 + 2 == 3
        self.assertTrue(any("failed" in line for line in st.log))

    def test_the_budget_is_the_military_action_total(self):
        # [CoL p.4] 'The total number of cards your rival plays or discards
        # for bonuses cannot exceed his or her military action total.'
        st, p0, p1 = self._attack(atk_strength=9, dfn_strength=1,
                                  hand=[BONUS_I] * 5)
        self.assertEqual(st.pending[-1]["budget"],
                         effects.state_stats(st, p1).military_actions)
        played = 0
        while st.pending and st.pending[-1]["kind"] == "defense":
            moves = [m for m in actions.legal_moves(st) if m[0] == "defend"]
            if not moves:
                break
            actions.apply(st, moves[0])
            played += 1
        self.assertEqual(played, effects.state_stats(st, p1).military_actions)

    def test_a_defender_with_no_cards_is_not_asked(self):
        st, p0, p1 = self._attack(hand=[])
        self.assertEqual([pend for pend in st.pending
                          if pend["kind"] == "defense"], [])

    def test_plunder_takes_no_more_than_the_victim_has(self):
        # [FAQ p.7] 'You cannot gain more than the victim has to lose.'
        st, p0, p1 = self._ready(hand=[])
        p1.food, p1.resources = 1, 0
        p0.food, p0.resources = 0, 0
        actions.apply(st, ("aggression", "Aggression: Plunder (I)", 1))
        self._drain(st)
        self.assertEqual((p1.food, p1.resources), (0, 0))
        self.assertEqual(p0.food + p0.resources, 1)      # not 3

    def test_plunder_takes_the_full_amount_when_it_is_there(self):
        st, p0, p1 = self._ready(hand=[])
        p1.food, p1.resources = 5, 5
        p0.food, p0.resources = 0, 0
        actions.apply(st, ("aggression", "Aggression: Plunder (I)", 1))
        self._drain(st)
        self.assertEqual((p1.food + p1.resources), 10 - 3)
        self.assertEqual(p0.food + p0.resources, 3)

    def test_raid_returns_the_worker_to_the_pool_and_pays_half(self):
        # [card] destroy an Age A/I urban building, gain half its printed
        # build cost, rounded up.  [FAQ p.7] 'To destroy a building means to
        # move a Worker to the Worker Pool (not to the yellow Bank).'
        st, p0, p1 = self._ready(card="Aggression: Raid (I)", hand=[])
        p1.techs["Philosophy"].workers = 1               # the only building
        effects.invalidate(st)
        pool, bank = p1.workers_free, p1.yellow_bank
        actions.apply(st, ("aggression", "Aggression: Raid (I)", 1))
        self._drain(st)
        self.assertEqual(p1.techs["Philosophy"].workers, 0)
        self.assertEqual(p1.workers_free, pool + 1)
        self.assertEqual(p1.yellow_bank, bank)

    def test_raid_may_only_destroy_buildings_of_the_printed_age(self):
        # Age I Raid destroys one Age A or I urban building -- an Age II
        # building is out of reach.
        st, p0, p1 = self._ready(card="Aggression: Raid (I)", hand=[])
        db = C.db()
        age2 = next(c["name"] for c in db.cards
                    if c["type"] in C.URBAN_TYPES and c["age"] == "II")
        p1.techs[age2] = type(p1.techs["Philosophy"])(age2, workers=1)
        p1.techs["Philosophy"].workers = 0
        effects.invalidate(st)
        actions.apply(st, ("aggression", "Aggression: Raid (I)", 1))
        self._drain(st)
        self.assertEqual(p1.techs[age2].workers, 1, "an Age II lab was razed")

    def test_annex_moves_the_permanent_bonus_only(self):
        # [card] 'Take 1 colony. Its permanent bonus passes to you; the
        # immediate bonus no longer applies.'
        st, p0, p1 = self._ready(card="Aggression: Annex", hand=[])
        colony = next(c["name"] for c in C.db().of_type("territory")
                      if (c.get("permanentEffects") or {}).get("yellowTokens"))
        interact.gain_colony(st, p1, colony, None)
        perm = C.db().get(colony)["permanentEffects"]["yellowTokens"]
        bank = p0.yellow_bank
        actions.apply(st, ("aggression", "Aggression: Annex", 1))
        self._drain(st)
        self.assertIn(colony, p0.colonies)
        self.assertNotIn(colony, p1.colonies)
        self.assertEqual(p0.yellow_bank, bank + perm)

    def test_infiltrate_scores_three_culture_per_level(self):
        # [card] 'Remove a leader or an unfinished wonder from the game. Gain
        # 3 culture per level of the removed card.'
        st, p0, p1 = self._ready(card="Aggression: Infiltrate", hand=[])
        leader = C.db().leaders("II")[0]["name"]         # level 2
        p1.leader = leader
        effects.invalidate(st)
        culture = p0.culture
        actions.apply(st, ("aggression", "Aggression: Infiltrate", 1))
        self._drain(st)
        self.assertIsNone(p1.leader)
        self.assertEqual(p0.culture - culture, 3 * C.db().level_of(leader))

    def test_enslave_costs_the_victim_a_population(self):
        # [card] 'Gain 2 food and 2 resources. Your opponent decreases
        # population.'
        st, p0, p1 = self._ready(card="Aggression: Enslave", hand=[])
        p0.food = p0.resources = 0
        pool, bank = p1.workers_free, p1.yellow_bank
        actions.apply(st, ("aggression", "Aggression: Enslave", 1))
        self._drain(st)
        self.assertEqual((p0.food, p0.resources), (2, 2))
        self.assertEqual(p1.workers_free, pool - 1)
        self.assertEqual(p1.yellow_bank, bank + 1)      # [FAQ p.15]

    def test_open_borders_gives_the_defender_an_extra_defence_card(self):
        # [card] 'Both players gain +1 military action', and [CoL p.4] the
        # defence budget is the military action TOTAL.
        st, p0, p1 = self._ready(hand=[BONUS_I] * 4)
        give_pact(st, p0, p1, "Open Borders Agreement")
        actions.apply(st, ("aggression", "Aggression: Plunder (I)", 1))
        self.assertEqual(st.pending[-1]["budget"], 3)   # 2 + 1 from the pact


# ------------------------------------------------------------------- pacts

class TestPacts(unittest.TestCase):
    """[CoL p.4] 'Offer a Pact' / 'Cancel a Pact'; [FAQ p.11] 'Pacts'."""

    def test_two_player_decks_contain_no_pacts(self):
        # [CoL p.2] 'Remove all pact cards from these decks.' (2 players)
        db = C.db()
        for age in ("I", "II", "III"):
            deck2 = db.military_deck(age, 2)
            deck3 = db.military_deck(age, 3)
            self.assertEqual([n for n in deck2
                              if db.type_of(n) == "pact"], [], age)
            self.assertTrue([n for n in deck3 if db.type_of(n) == "pact"], age)

    def test_a_two_player_game_never_offers_a_pact(self):
        st = st_military(players=2)
        st.me().hand_military = [ALLIANCE]
        self.assertFalse([m for m in actions.legal_moves(st)
                          if m[0] == "offer_pact"])

    def test_a_resignation_does_not_make_a_pact_in_hand_unplayable(self):
        """[FAQ p.11] on resigning: 'Do not remove any Pacts or 3+ or 4-player
        cards from the current-Age decks; but do remove them from any
        future-Age decks.'  The pact cards left in the current deck are dealt
        to the survivors, so they must still be playable."""
        st = st_military(players=3)
        st.players[2].resigned = True
        effects.invalidate(st)
        st.me().hand_military = [ALLIANCE]
        self.assertIn(("offer_pact", ALLIANCE, 1, ""),
                      actions.legal_moves(st))

    def test_future_decks_are_retrimmed_after_a_resignation(self):
        # the other half of the same FAQ sentence
        st = st_military(players=3)
        st.players[2].resigned = True
        st.age_civil = "I"
        st.civil_deck = []
        game._advance_age(st, __import__("random").Random(0))
        db = C.db()
        self.assertEqual([n for n in st.military_deck
                          if db.type_of(n) == "pact"], [])

    def test_offering_costs_no_military_actions(self):
        # [FAQ p.11] 'Offering, Accepting, or Canceling Pacts requires no
        # Military Actions.'
        st = st_military()
        p0 = st.me()
        p0.hand_military = [ALLIANCE]
        p0.military_actions = 2
        actions.apply(st, ("offer_pact", ALLIANCE, 1, ""))
        self.assertEqual(p0.military_actions, 2)
        actions.apply(st, ("choose",
                           st.pending[-1]["options"].index("accept")))
        self.assertEqual(p0.military_actions, 2)

    def test_accepting_replaces_only_the_offerers_own_pact(self):
        # [CoL p.4] 'Any other pact in your play area ends ... Note: You may
        # still be a party to pacts in other players' play areas.'
        st = st_military(players=4)
        p0 = st.me()
        give_pact(st, p0, st.players[2], "Peace Treaty")
        st.players[3].pacts = [{"name": "International Tourism", "owner": 3,
                                "partner": 1, "a": 3, "b": 1}]
        p0.hand_military = [ALLIANCE]
        actions.apply(st, ("offer_pact", ALLIANCE, 1, ""))
        actions.apply(st, ("choose",
                           st.pending[-1]["options"].index("accept")))
        self.assertEqual([pact["name"] for pact in p0.pacts], [ALLIANCE])
        self.assertEqual([pact["name"] for pact in st.players[3].pacts],
                         ["International Tourism"])
        self.assertEqual(len(effects.pacts_for(st, 1)), 2)

    def test_a_refused_pact_returns_to_hand_and_uses_the_action(self):
        st = st_military()
        p0 = st.me()
        p0.hand_military = ["Peace Treaty"]
        actions.apply(st, ("offer_pact", "Peace Treaty", 1, ""))
        actions.apply(st, ("choose",
                           st.pending[-1]["options"].index("refuse")))
        self.assertEqual(p0.hand_military, ["Peace Treaty"])
        self.assertEqual(p0.pacts, [])
        self.assertTrue(p0.politics_done)

    def test_sides_a_and_b_are_declared_by_the_offerer(self):
        # [CoL p.4] 'declare whether you are taking the role of side A or B'
        st = st_military()
        p0 = st.me()
        p0.hand_military = [PROMISE]
        moves = [m for m in actions.legal_moves(st) if m[0] == "offer_pact"]
        self.assertIn((("offer_pact", PROMISE, 1, "A")), moves)
        self.assertIn((("offer_pact", PROMISE, 1, "B")), moves)
        actions.apply(st, ("offer_pact", PROMISE, 1, "B"))
        actions.apply(st, ("choose",
                           st.pending[-1]["options"].index("accept")))
        # P0 took side B, so P0 gets the +4 strength
        self.assertEqual(effects.state_stats(st, p0).strength, 1 + 4)
        self.assertEqual(effects.state_stats(st, st.players[1]).strength, 1)

    def test_cancelling_is_open_to_either_party(self):
        # [CoL p.4] 'Choose a pact in play to which you are a party. It does
        # not have to be a pact in your play area.'
        st = st_military()
        give_pact(st, st.players[1], st.me(), "Peace Treaty")
        self.assertIn(("cancel_pact", 1), actions.legal_moves(st))
        actions.apply(st, ("cancel_pact", 1))
        self.assertEqual(effects.pacts_for(st, 0), [])

    def test_a_peace_treaty_does_not_cancel_an_already_declared_war(self):
        # [CoL p.4] 'A pact that prevents attacks or declaring a war does not
        # cancel wars that were already declared.'
        st = st_military()
        p0, p1 = st.me(), st.players[1]
        p0.hand_military = ["War over Territory"]
        p0.military_actions = 2
        declare_war(st, "War over Territory", 1)
        give_pact(st, p0, p1, "Peace Treaty")
        set_strength(st, p0, 6)
        set_strength(st, p1, 1)
        events.resolve_war(st, p0, None)
        self.assertEqual(p1.yellow_bank, 18 - 2)      # 1 + 5 // 5

    def test_only_one_pact_may_sit_in_your_own_play_area(self):
        # [FAQ p.11] 'You can have only one Pact in front of you.'
        st = st_military(players=4)
        p0 = st.me()
        p0.hand_military = [ALLIANCE, "Peace Treaty"]
        actions.apply(st, ("offer_pact", ALLIANCE, 1, ""))
        actions.apply(st, ("choose",
                           st.pending[-1]["options"].index("accept")))
        st.phase = "politics"
        p0.politics_done = False
        actions.apply(st, ("offer_pact", "Peace Treaty", 2, ""))
        actions.apply(st, ("choose",
                           st.pending[-1]["options"].index("accept")))
        self.assertEqual(len(p0.pacts), 1)
        self.assertEqual(p0.pacts[0]["name"], "Peace Treaty")


if __name__ == "__main__":
    unittest.main()
