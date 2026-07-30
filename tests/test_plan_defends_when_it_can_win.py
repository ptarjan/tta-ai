"""A search bot must not answer its own defence blind to whether it wins.

WHAT THIS LOCKS
---------------
`PlanBot._child` scores every node inside the beam as
``copy -> apply -> _quiesce -> _score``: within its own search this bot always
prices an aggression by playing the defence out.  `PlanBot.pick` short-circuits
on ``state.pending`` to `_one_ply`, which is ``copy -> apply -> evaluate`` with
no drain -- so the SAME position was priced two different ways depending on
whether the bot was the one being searched or the one deciding.

That gap is invisible to every other test here, because every other test asks
whether a card is *priced*, and this is not a pricing bug: the defence is
priced perfectly well, one function call away, by machinery this class already
owns and already trusts.

WHY IT BITES EVERY DEFENCE IN THE GAME
--------------------------------------
`interact._defense_move` keeps the decision on ``state.pending`` while the
defender still has room and cards, so after ``("defend", card)`` the aggression
has NOT resolved: `evaluate` sees a position one military card poorer with the
attack still hanging.  ``("defend_done",)`` pops the stack and calls
`events.finish_aggression` at once, so its position shows the full loss.
Nothing in `weighted.features` reads ``pend["atk"]`` or ``pend["dfn"]``.

So without a drain the choice cannot be about winning, and the measurement says
it is not.  Over 200 games of ``plan:width=2`` at 4p
(`tools/aggression_census.py`), of 589 arithmetically winnable defences
**588 needed two or more cards** -- so in practice the first `defend` ALWAYS
leaves the outcome pending and invisible.  The bot spent cards in 145 of 193
hopeless defences and 15 of 589 winnable ones, and held off **0 of 782**
aggressions.  With `QUIET_PENDING` on, on the same seeds: every one of 332
attempts was in a winnable position, none in a hopeless one, and **332 of 832**
aggressions were held off.

THE TWO INVARIANTS
------------------
Both are rule-level claims about §5.4 step 5 -- a defender whose total reaches
the attacker's strength takes no effect at all -- not claims about strength:

1. a defence that can be won is completed, and
2. a defence that cannot be won costs nothing, because a card spent below the
   attacker's strength buys literally nothing (§5.4 step 5 is a threshold, not
   a scale).

The assertions are deliberately one-sided: they pin the behaviour with
`QUIET_PENDING` on and say nothing about the unfixed default, so flipping the
default to True later needs no edit here -- and flipping it back off fails.
"""
import random
import unittest

from engine import actions, game
from engine.bots.plan import PlanBot
from engine.bots.weighted import load_weights

BONUS2 = "Military Bonus (defense 2 / colonization 1)"
#: population loss, so the defender has something real to protect -- a fresh
#: board has 0 food and 0 resources, under which Plunder steals nothing and the
#: two branches score identically for the wrong reason.
AGGRESSION = "Aggression: Enslave"


def defence(atk, seed=5):
    """A `kind="defense"` pending with two 2-point bonus cards and dfn 2.

    Built by hand rather than by playing on until one occurs, so the
    arithmetic is fixed and the test cannot silently start measuring a
    different position: the defender can reach 2 + 2 + 2 = 6, and `atk`
    decides which side of that it lands on.  Two cards is the realistic case,
    not a contrived one -- see the 588-of-589 measurement above.
    """
    st = game.new_game(2, seed)
    d = st.players[1]
    d.hand_military = [BONUS2, BONUS2]
    d.food, d.resources = 10, 10
    st.pending.append({
        "kind": "defense", "player": 1, "attacker": 0,
        "card": AGGRESSION, "atk": atk, "dfn": 2, "spent": 0, "budget": 2,
    })
    st.current = 0
    return st


def play_out(st, bot):
    """Resolve the whole pending stack with `bot`; return (cards spent, log)."""
    spent = 0
    while st.pending:
        mv = bot.pick(st, actions.legal_moves(st))
        if mv[0] == "defend":
            spent += 1
        actions.apply(st, mv, random.Random(1))
    return spent, [ln for ln in st.log if "aggression" in ln]


def quiet_bot():
    return PlanBot(weights=load_weights("analysis/frozen/champion_2p.json"),
                   seed=3, width=2, quiet_pending=True)


class PlanDefendsWhenItCanWin(unittest.TestCase):

    def test_the_fixture_is_a_real_choice_on_both_sides_of_the_threshold(self):
        """Guard the fixture, so a green test can never mean an empty one."""
        for atk in (6, 20):
            st = defence(atk)
            moves = actions.legal_moves(st)
            self.assertIn(("defend", BONUS2), moves)
            self.assertIn(("defend_done",), moves)
            self.assertLess(st.pending[-1]["dfn"], atk)   # behind to start
        self.assertGreaterEqual(2 + 2 + 2, 6)             # 6 is winnable
        self.assertLess(2 + 2 + 2, 20)                    # 20 is not

    def test_a_winnable_defence_is_carried_through_to_the_end(self):
        spent, log = play_out(defence(6), quiet_bot())
        self.assertEqual(spent, 2)
        self.assertTrue(any("failed" in ln for ln in log), log)

    def test_a_hopeless_defence_spends_nothing(self):
        """The bug's other half, and the one a synthetic case shows plainly.

        Below the attacker's strength a defence card buys nothing at all, so
        spending one is pure waste.  Without the drain the bot plays a card and
        then gives up anyway -- it is not defending, it is deferring the
        resolution to a position `evaluate` scores as if the attack had not
        happened yet.
        """
        spent, log = play_out(defence(20), quiet_bot())
        self.assertEqual(spent, 0)
        self.assertTrue(any("succeeded" in ln for ln in log), log)


if __name__ == "__main__":
    unittest.main()
