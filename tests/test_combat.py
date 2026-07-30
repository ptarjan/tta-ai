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
from engine.state import TechCard  # noqa: E402

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

    # -- War over Technology's alternative spoil (the victor's choice) ------
    #
    # [card] 'The victor takes science equal to the strength advantage, or
    # takes special (blue) technologies of the same total cost.'
    # [CoL p.3] 'If the victor steals a special technology, the victor takes
    # the card from the defeated civilization's play area and puts it into
    # his or her own play area.  A player cannot steal a special technology
    # that is the same as one he or she already has in play or in hand.  If
    # you steal a special technology of the same type as one that you have in
    # play, you keep the higher level card in play and discard the other.'
    # [FAQ p.8] 'As long as you win enough Science points you can always
    # choose to take some or all of them in blue Special Technologies.'

    def _tech_war(self, adv=10, blues=(), mine=(), hand=()):
        st, p0, p1 = self._declared("War over Technology")
        for n in blues:
            p1.techs[n] = TechCard(n)
        for n in mine:
            p0.techs[n] = TechCard(n)
        p0.hand_civil = list(hand)
        p0.science, p1.science = 0, 30
        effects.invalidate(st)
        # Some blue technologies print strength of their own (Cartography +1,
        # Warfare +1, Strategy +3), so the advantage is set AFTER they are in
        # play and read off the engine rather than assumed.
        p1.techs["Warriors"].workers = 1
        effects.invalidate(st, p1)
        d = effects.state_stats(st, p1).strength
        p0.techs["Warriors"].workers = 0
        effects.invalidate(st, p0)
        p0.techs["Warriors"].workers = (
            d + adv - effects.state_stats(st, p0).strength)
        effects.invalidate(st, p0)
        assert (effects.state_stats(st, p0).strength
                - effects.state_stats(st, p1).strength) == adv
        return st, p0, p1

    def _opts(self, st):
        return list(st.pending[-1]["options"])

    def _choose(self, st, opt):
        i = self._opts(st).index(opt)
        interact.apply_pending(st, ("choose", i))

    def test_no_decision_when_the_loser_has_no_blue_technology(self):
        """A war the victor cannot spend on cards must not manufacture a
        decision -- `push_choice` is never reached."""
        st, p0, p1 = self._tech_war(adv=7)
        events.resolve_war(st, p0, None)
        self.assertEqual(st.pending, [])
        self.assertEqual((p0.science, p1.science), (7, 23))

    def test_the_victor_is_offered_science_or_the_loser_s_blue_cards(self):
        st, p0, p1 = self._tech_war(adv=10, blues=("Code of Laws",
                                                   "Cartography"))
        events.resolve_war(st, p0, None)
        self.assertEqual(st.decider(), p0.idx)
        # science first (the pre-choice behaviour is the index-0 fallback),
        # then the technologies most expensive first
        self.assertEqual(self._opts(st),
                         ["science", "Code of Laws", "Cartography"])

    def test_taking_the_science_is_exactly_the_old_behaviour(self):
        st, p0, p1 = self._tech_war(adv=10, blues=("Code of Laws",))
        events.resolve_war(st, p0, None)
        self._choose(st, "science")
        self.assertEqual((p0.science, p1.science), (10, 20))
        self.assertIn("Code of Laws", p1.techs)
        self.assertEqual(st.pending, [])

    def test_stealing_moves_the_card_between_the_play_areas(self):
        # [CoL p.3] out of the defeated civilization's play area, into the
        # victor's -- and its effect comes with it (+1 civil action).
        st, p0, p1 = self._tech_war(adv=10, blues=("Code of Laws",))
        ca_before = effects.state_stats(st, p0).civil_actions
        events.resolve_war(st, p0, None)
        self._choose(st, "Code of Laws")
        self.assertIn("Code of Laws", p0.techs)
        self.assertNotIn("Code of Laws", p1.techs)
        self.assertEqual(effects.state_stats(st, p0).civil_actions,
                         ca_before + 1)

    def test_the_victor_may_mix_cards_and_science(self):
        # [FAQ p.8] 'some or all of them'.  The digital edition's own log for
        # a 26-vs-14 win: Code of Laws (6) + Cartography (4) + 2 science.
        st, p0, p1 = self._tech_war(adv=12, blues=("Code of Laws",
                                                   "Cartography"))
        events.resolve_war(st, p0, None)
        self._choose(st, "Code of Laws")               # 6 of 12
        self._choose(st, "Cartography")                # 4 of the last 6
        self.assertEqual(st.pending, [])               # nothing left to steal
        self.assertEqual((p0.science, p1.science), (2, 28))
        self.assertEqual(set(p0.techs) & {"Code of Laws", "Cartography"},
                         {"Code of Laws", "Cartography"})

    def test_a_card_the_advantage_cannot_pay_for_is_not_offered(self):
        # [FAQ p.8] 'as long as you win enough Science points'.  Code of Laws
        # costs 6; an advantage of 5 cannot reach it.
        st, p0, p1 = self._tech_war(adv=5, blues=("Code of Laws",))
        events.resolve_war(st, p0, None)
        self.assertEqual(st.pending, [])
        self.assertEqual(p0.science, 5)
        self.assertIn("Code of Laws", p1.techs)

    def test_the_budget_shrinks_by_the_printed_cost(self):
        # Cartography is 4, so a 9 advantage leaves 5 -- not enough for the
        # 6-cost Code of Laws, which drops out of the second offer.
        st, p0, p1 = self._tech_war(adv=9, blues=("Code of Laws",
                                                  "Cartography"))
        events.resolve_war(st, p0, None)
        self._choose(st, "Cartography")
        self.assertEqual(st.pending, [])
        self.assertEqual(p0.science, 5)
        self.assertIn("Code of Laws", p1.techs)

    def test_cannot_steal_a_card_already_in_play_or_in_hand(self):
        # [CoL p.3] / [FAQ p.8] the two-part exclusion.
        st, p0, p1 = self._tech_war(adv=12, blues=("Code of Laws",
                                                   "Cartography"),
                                    mine=("Code of Laws",),
                                    hand=("Cartography",))
        events.resolve_war(st, p0, None)
        self.assertEqual(st.pending, [])               # nothing on offer
        self.assertEqual(p0.science, 12)

    def test_stealing_the_same_icon_keeps_the_higher_level_card(self):
        # [CoL p.3] 'you keep the higher level card in play and discard the
        # other'.  Navigation (II) replaces Cartography (I).
        st, p0, p1 = self._tech_war(adv=12, blues=("Navigation",),
                                    mine=("Cartography",))
        events.resolve_war(st, p0, None)
        self._choose(st, "Navigation")
        self.assertIn("Navigation", p0.techs)
        self.assertNotIn("Cartography", p0.techs)      # discarded
        self.assertNotIn("Navigation", p1.techs)       # the loser lost it

    def test_stealing_a_lower_level_card_is_pure_denial(self):
        # The other half of the same sentence: the stolen card loses the
        # comparison and is discarded, but the loser has still lost it.
        st, p0, p1 = self._tech_war(adv=12, blues=("Cartography",),
                                    mine=("Navigation",))
        events.resolve_war(st, p0, None)
        self.assertIn("Cartography", self._opts(st))
        self._choose(st, "Cartography")
        self.assertNotIn("Cartography", p0.techs)      # discarded
        self.assertNotIn("Cartography", p1.techs)      # and gone from theirs
        self.assertIn("Navigation", p0.techs)

    def test_the_science_half_is_still_capped_by_what_the_loser_holds(self):
        # [FAQ p.8] 'cannot take more Science points than the loser has'.
        st, p0, p1 = self._tech_war(adv=12, blues=("Code of Laws",))
        p1.science = 3
        events.resolve_war(st, p0, None)
        self._choose(st, "Code of Laws")
        self.assertEqual((p0.science, p1.science), (3, 0))

    def test_the_defender_can_win_and_choose(self):
        # [FAQ p.16] 'Either player can win a War' -- and then it is the
        # DEFENDER holding the decision, not the player to move.
        st, p0, p1 = self._declared("War over Technology")
        set_strength(st, p0, 1)
        set_strength(st, p1, 11)
        p0.techs["Code of Laws"] = TechCard("Code of Laws")
        p0.science, p1.science = 30, 0
        effects.invalidate(st)
        events.resolve_war(st, p0, None)
        self.assertEqual(st.decider(), p1.idx)
        self._choose(st, "Code of Laws")
        self.assertIn("Code of Laws", p1.techs)

    def test_only_blue_special_technologies_are_stealable(self):
        # Brown/grey/red technologies stay put -- the card says "special
        # (blue) technologies".
        st, p0, p1 = self._tech_war(adv=12, blues=("Bronze", "Philosophy"))
        events.resolve_war(st, p0, None)
        self.assertEqual(st.pending, [])
        self.assertEqual(p0.science, 12)

    def test_the_turn_does_not_advance_while_the_choice_is_outstanding(self):
        # The decision arrives inside the start-of-turn sequence, so the
        # politics phase must wait for it (and the auto-skip test with it,
        # because a stolen Warfare/Strategy changes the military actions the
        # test reads).
        st, p0, p1 = self._tech_war(adv=12, blues=("Strategy",))
        st.current = 0
        st.round = 4
        p0.hand_military = []
        game.start_turn(st)
        self.assertTrue(st.pending)
        self.assertEqual(actions.legal_moves(st),
                         interact.pending_moves(st))
        self._choose(st, "Strategy")
        self.assertEqual(st.pending, [])
        self.assertIn("Strategy", p0.techs)

    def test_the_effect_key_gates_the_offer(self):
        # The alternative spoil is a property of the CARD DATA
        # (`orTakesSpecialTechnologiesOfSameTotalScienceCost`), not of the
        # spoils kind, so a war paying science without that clause takes it
        # with no decision at all.
        eff = C.db().get("War over Technology")["effects"]
        self.assertTrue(eff["orTakesSpecialTechnologiesOfSameTotalScienceCost"])
        for other in ("War over Territory", "War over Culture"):
            self.assertNotIn(
                "orTakesSpecialTechnologiesOfSameTotalScienceCost",
                C.db().get(other).get("effects") or {})

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

    def test_several_players_may_declare_war_on_the_same_civilization(self):
        # [FAQ p.11] 'Multiple Attacks: More than one player may attack ...
        # the same Civilization in a given round.'
        st = st_military(players=4)
        for attacker in (0, 2):
            st.current = attacker
            st.phase = "politics"
            p = st.players[attacker]
            p.politics_done = False
            p.hand_military = ["War over Territory"]
            p.military_actions = 2
            declare_war(st, "War over Territory", 1)
        self.assertEqual(len(st.players[1].wars_declared_on_me), 2)
        set_strength(st, st.players[0], 6)
        set_strength(st, st.players[2], 6)
        set_strength(st, st.players[1], 1)
        events.resolve_war(st, st.players[0], None)
        events.resolve_war(st, st.players[2], None)
        self.assertEqual(st.players[1].wars_declared_on_me, [])
        self.assertEqual(st.players[1].yellow_bank, 18 - 4)   # 2 each

    def test_you_may_resolve_a_war_and_then_aggress_the_same_rival(self):
        # [FAQ p.11] '... a single player to both resolve a War and conduct
        # an Aggression against the same player during a single turn.'
        st, p0, p1 = self._declared()
        set_strength(st, p0, 6)
        set_strength(st, p1, 1)
        st.current = 0
        st.round = 4
        p0.hand_military = ["Aggression: Plunder (I)"]
        p0.military_actions = 2          # a fresh turn's red tokens
        game.start_turn(st)
        self.assertIsNone(p0.war_declared_by_me)
        self.assertIn(("aggression", "Aggression: Plunder (I)", 1),
                      actions.legal_moves(st))

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


# ------------------------- who answers the new decision, and with what -----

class TestWarOverTechnologyPolicies(unittest.TestCase):
    """Every bot needs a policy for the spoils choice.

    Four of the five need no new code and this class is the proof: they score
    ``("choose", i)`` by cloning, applying and asking the evaluator they
    already use, so their policy is DERIVED from their own valuation and
    cannot drift from it.  BookBot -- which does no lookahead at all -- gets a
    preference, and it is built out of the tables the book already has.
    """

    def _position(self, players=3, blues=("Code of Laws",), adv=12):
        st = st_military(players=players)
        p0, p1 = st.me(), st.players[1]
        p0.hand_military = ["War over Technology"]
        p0.military_actions = 2
        declare_war(st, "War over Technology", 1)
        for n in blues:
            p1.techs[n] = TechCard(n)
        p0.science, p1.science = 0, 30
        effects.invalidate(st)
        p1.techs["Warriors"].workers = 1
        effects.invalidate(st, p1)
        d = effects.state_stats(st, p1).strength
        p0.techs["Warriors"].workers = 0
        effects.invalidate(st, p0)
        p0.techs["Warriors"].workers = (
            d + adv - effects.state_stats(st, p0).strength)
        effects.invalidate(st, p0)
        events.resolve_war(st, p0, None)
        assert st.pending, "the choice is not live in this position"
        return st, p0, p1

    def test_the_choice_reaches_the_move_generator(self):
        st, p0, _p1 = self._position()
        moves = actions.legal_moves(st)
        self.assertEqual(moves, [("choose", 0), ("choose", 1)])
        self.assertEqual(st.decider(), p0.idx)

    def test_the_evaluator_bots_answer_it_without_new_code(self):
        from engine.bots.weighted import DEFAULT_WEIGHTS, WeightedBot
        from engine.bots.quiescent import QuiescentBot
        for bot in (WeightedBot(DEFAULT_WEIGHTS, seed=3),
                    QuiescentBot(DEFAULT_WEIGHTS, seed=3, levels=1)):
            with self.subTest(bot=type(bot).__name__):
                st, _p0, _p1 = self._position()
                mv = bot.pick(st, actions.legal_moves(st))
                self.assertIn(mv, actions.legal_moves(st))

    def test_the_evaluator_can_see_the_difference_between_the_options(self):
        """The conduction check for the new lever.

        A decision the evaluator scores identically either way is not a
        decision, it is noise in the move stream.  Stealing `Code of Laws`
        buys a civil action and 30 science does not, so the two branches must
        NOT evaluate equal.
        """
        from engine.bots.fastcopy import copy_state
        from engine.bots.weighted import (DEFAULT_WEIGHTS, evaluate,
                                          rival_context)
        st, p0, _p1 = self._position()
        ctx = rival_context(st, p0.idx)
        vals = []
        for i in range(len(st.pending[-1]["options"])):
            trial = copy_state(st)
            interact.apply_pending(trial, ("choose", i))
            while trial.pending and trial.pending[-1]["tag"] == "war_tech":
                interact.apply_pending(trial, ("choose", 0))
            vals.append(evaluate(trial, p0.idx, DEFAULT_WEIGHTS, ctx))
        self.assertNotAlmostEqual(vals[0], vals[1], places=6)

    def test_bookbot_prefers_a_technology_that_upgrades_an_icon(self):
        # Code of Laws costs 6 and is rank 9 in the book's own table, so it
        # beats 6 science comfortably.
        from engine.bots.book import BookBot
        st, _p0, _p1 = self._position(blues=("Code of Laws",))
        mv = BookBot(seed=1).choose(st, actions.legal_moves(st))
        self.assertEqual(st.pending[-1]["options"][mv[1]], "Code of Laws")

    def test_bookbot_takes_the_science_over_a_card_it_cannot_use(self):
        # [CoL p.3] a stolen card of the same icon and no higher level is
        # discarded, so it buys nothing but denial -- and the book prices
        # denial below a point of science.
        from engine.bots.book import BookBot
        from engine.bots.weighted import DEFAULT_WEIGHTS   # noqa: F401
        st, p0, _p1 = self._position(blues=("Cartography",))
        p0.techs["Navigation"] = TechCard("Navigation")    # out-levels it
        effects.invalidate(st, p0)
        mv = BookBot(seed=1).choose(st, actions.legal_moves(st))
        self.assertEqual(st.pending[-1]["options"][mv[1]], "science")


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

    def _pact_offer_to(self, st, offerer, target, name=ALLIANCE):
        """Put a live `pact_offer` decision in front of `target`."""
        st.players[offerer].hand_military = [name]
        st.current = offerer
        st.phase = "politics"
        st.players[offerer].politics_done = False
        actions.apply(st, ("offer_pact", name, target, ""))
        self.assertEqual(st.pending[-1]["tag"], "pact_offer")
        return st.pending[-1]

    def test_bookbot_refuses_a_pact_that_props_up_the_culture_leader(self):
        """BookBot's refusal branch had never once executed.

        `book.py:_choice` read `pend["ctx"]["from"]` to find the counterparty,
        but `actions._h_offer_pact` builds the ctx as
        {"owner", "name", "a", "b"} and has never written a "from".  `.get`
        returned None, `leading` was therefore always False, and the bot
        ACCEPTED EVERY PACT IT WAS EVER OFFERED -- a whole branch of its policy
        was dead code.  The counterparty is the offerer, `ctx["owner"]`.

        Both directions are checked, because a fix that always REFUSES would
        pass a one-sided version of this test just as happily.
        """
        from engine.bots.book import BookBot
        bot = BookBot(seed=1)

        # (a) the offerer is far ahead on culture -> refuse
        st = st_military(players=3)
        st.players[0].culture = 60
        st.players[1].culture = 0
        pend = self._pact_offer_to(st, 0, 1)
        mv = bot(st)
        self.assertEqual(pend["options"][mv[1]], "refuse")

        # (b) the offerer is not ahead -> accept
        st = st_military(players=3)
        st.players[0].culture = 0
        st.players[1].culture = 60
        pend = self._pact_offer_to(st, 0, 1)
        mv = bot(st)
        self.assertEqual(pend["options"][mv[1]], "accept")

    def test_offer_pact_ctx_carries_the_offerer(self):
        """The key any bot must read to know who it is dealing with.

        This is the guardrail for the bug above: it fails if `_h_offer_pact`
        ever renames or drops the key, instead of the failure showing up as a
        bot silently agreeing to everything for another season.
        """
        st = st_military(players=3)
        pend = self._pact_offer_to(st, 0, 1)
        self.assertEqual(pend["ctx"]["owner"], 0)
        self.assertEqual(pend["player"], 1)
        self.assertNotIn("from", pend["ctx"])

    def test_you_stay_party_to_pacts_you_do_not_own(self):
        """The FIRST half of the printed sentence, and the half that was
        missing: accepting a new pact as OWNER must not cost you the pacts you
        are party to but do not own.

        sources/ubg_full-game.txt:70 (2015 rulebook): "You can be a party to
        MORE THAN ONE PACT, but you can have only one pact in your play area.
        If you offer a new pact and the player accepts, any pact in your play
        area is automatically cancelled."

        `_c_pact_offer`'s `owner.pacts = [...]` has been reported as a bug --
        "assignment, not append, so accepting a pact destroys every other pact
        that player holds".  It does not, and this test is the difference:
        `effects.pacts_for` scans every player's list, so a pact someone else
        owns sits in THEIR play area and survives.  Only the owner's own is
        replaced, which is the rule.  Pinning it here means a future
        "correction" to `append` fails against the printed text rather than
        against taste.
        """
        st = st_military(players=4)
        p0, p2 = st.me(), st.players[2]
        # P1 owns a pact with P0: P0 is a party to it but does not own it.
        give_pact(st, st.players[1], p0, PROMISE)
        self.assertEqual(len(effects.pacts_for(st, 0)), 1)
        # now P0 offers one of its own and P2 accepts
        p0.hand_military = [ALLIANCE]
        actions.apply(st, ("offer_pact", ALLIANCE, 2, ""))
        actions.apply(st, ("choose",
                           st.pending[-1]["options"].index("accept")))
        self.assertEqual([pact["name"] for pact in p0.pacts], [ALLIANCE])
        names = sorted(pact["name"] for pact in effects.pacts_for(st, 0))
        self.assertEqual(names, sorted([ALLIANCE, PROMISE]),
                         "accepting a pact as owner ate a pact P0 was party "
                         "to but did not own")
        self.assertEqual(len(effects.pacts_for(st, 1)), 1,
                         "P1's own pact was destroyed by P0's agreement")
        self.assertEqual(len(effects.pacts_for(st, 2)), 1)

    def test_only_one_pact_may_sit_in_your_own_play_area(self):
        # [FAQ p.11] 'You can have only one Pact in front of you.'  Same rule
        # in the rulebook transcription, with the mechanism spelled out:
        # sources/ubg_full-game.txt:70 "If you offer a new pact and the player
        # accepts, any pact in your play area is automatically cancelled."
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
