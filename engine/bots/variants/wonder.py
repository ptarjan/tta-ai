"""WonderBot -- the Michelangelo / wonder-spam line, included ON PURPOSE.

Every other variant in this roster is a line some strong player defends.
This one is the line strong players call a **noob trap**, and it is here
because a training pool with no bad-but-coherent opponent teaches a bot
nothing about how to punish one.  A bot that cannot beat this has not
learned that tempo and military beat monuments.

WHAT THE SOURCES ACTUALLY SAY
-----------------------------
- Michelangelo is "last in every list found"; tournament CA-spend **0.00**;
  "Michelangelo is bad in BGA meta.  I'd be surprised if it was picked in
  even 5% of games" (#1 BGA player).  Explicitly a "noob trap": it "invites
  novice players to over-invest... encourages wasting resources on bad
  wonders and inflating the price in actions of Age 3 wonders."
  -- BGG 2393942, 2494200
- "Fast way to throw a game is to play Michelangelo, meld Drama/Theology,
  and start spamming these culture generators... You'll be at a 60+ culture
  lead in Age II, then lose two wars over culture for like 50 points each."
  -- BGG 2258467
- Masonry, the tech that makes wonder spam mechanically possible ("two
  wonder stages per action"), is "separately singled out as terrible":
  "3 Science + 2 CA early game... the time until investment payoff is
  extremely high."
- Michelangelo's own mechanical statement, which is what makes the trap
  *coherent* rather than merely bad: "CA provided is geometric with respect
  to the number of wonders picked; amazing value if many (4+) wonders are
  picked, otherwise unimpressive."  And the conditional rule the corpus
  gives: "only take Michelangelo if (Hanging Gardens or St. Peter's is
  yours) AND (Iron, ideally + Masonry)."  In four player "you are either
  going to win with him or finish fourth."

THE PRIORITY LIST THIS BOT PLAYS
--------------------------------
1. Start a wonder as early as possible and always continue it -- the wonder
   step is promoted **above the military floor**, which is precisely §10
   mistake #1 ("Neglecting military, then getting culture-warred") made
   structural rather than accidental.
2. Take Michelangelo, and take Masonry to double up stages.
3. Play Camp A of the Hanging Gardens disagreement: statelyplay -- the
   oldest and the only base-game-only source -- ranks it the **best Age A
   wonder** ("allowing players to skip religion buildings").  Everyone else
   in this roster plays Camp B.
4. Chase Age III wonders for the one-shot culture bomb and Impact of Wonders
   ("5 per Age A, 4 per Age I, 3 per Age II, 2 per Age III wonder").
5. Keep the resource production a wonder habit needs: "Without a wonder it's
   possible that you get a lot of resources through events and won't be able
   to spend them, which leads to corruption" is the *pro*-wonder argument,
   and it is real.

WHAT IT IS NOT ALLOWED TO DO
----------------------------
It is a trap, not a clown.  The shared roster discipline still applies: it
respects the convex CA price ladder (76% of tournament Age I picks are at
1 CA, 2.5% at 3 CA), the "no leader for 3 CA before Age III" rule, and the
"don't spend 3 CA until you have 5-6 actions" threshold.  Its failure mode
is **over-investment in monuments**, not indiscriminate overpaying -- because
the thing training should learn to punish is the strategy, not a bug.

The one place the price discipline is loosened is wonders themselves
(``price_scale`` 0.85), since each already-completed wonder adds +1 CA to
the cost of taking the next one, which is the self-limiting mechanism the
trap ignores.

PLAYER-COUNT PARAMETERIZATION
-----------------------------
"in four player, you are either going to win with him or finish fourth" --
the variance of this line rises with the table size, and the Michelangelo
bonus is keyed accordingly.  Note also that ``docs/AGGRESSION_RATE.md``
(appendix A) records that no territory is ever auctioned at 4p in our engine's games, so
nothing in this plan is allowed to depend on colonies; it does not.
"""
from __future__ import annotations

from .base import VariantBot

__all__ = ["WonderBot"]


class WonderBot(VariantBot):
    NAME = "wonder"

    PROFILE = {
        # rule 1/4: monuments are the plan
        "wonder_appetite": 1.8,
        "wonder_max": 99,
        "wonder_bonus": {
            # rule 3: Camp A of the Hanging Gardens row
            "Hanging Gardens": 1.8,
            "St. Peter's Basilica": 1.4,
            "Pyramids": 1.2,
            "Library of Alexandria": 1.1,
            # the Age III culture bombs
            "Hollywood": 1.3,
            "Fast Food Chains": 1.3,
            "Internet": 1.2,
            "First Space Flight": 1.2,
        },
        "leader_bonus": {
            # rule 2 -- the trap itself.  Tournament CA-spend 0.00.
            "Michelangelo": {2: 6.0, 3: 7.0, 4: 8.0},
            # "free wonder taking" pairing; Homer needs a completed wonder,
            # which this is the one variant guaranteed to have
            "Homer": 2.0,
        },
        "tech_bonus": {
            # rule 2: "Masonry: build discount AND two wonder stages per
            # action" -- the enabler the sources call terrible
            "Masonry": 7.0,
            "Architecture": 5.0,
            "Engineering": 4.0,
            # rule 5: stages are paid in rocks
            "Iron": 4.0,
            "Coal": 3.0,
            "Oil": -4.0,
        },
        "tech_veto": frozenset(("Oil",)),
        "card_bonus": {
            "Hanging Gardens": 3.0,
            "St. Peter's Basilica": 3.0,
            "Engineering Genius (A)": 4.0,   # discount on stages
            "Engineering Genius (I)": 4.0,
            "Engineering Genius (II)": 3.0,
        },
        "prod_weights": {"food": 1.0, "resources": 1.2, "science": 0.9,
                         "culture": 1.2, "happy": 1.0, "strength": 0.8},
        "build_bonus": {"mine": 1.0},
        # loosened only for the row, and only slightly; the caps and the
        # 3-CA threshold from the base class still apply
        "price_scale": 0.85,
        "max_take_cost": {0: 2, 1: 2, 2: 2, 3: 3, 4: 3},
        "pop_appetite": 1.0,
        # the floor, and only the floor -- rule 1 puts even that second
        "mil_stance": "floor",
        "mil_margin": 0,
    }

    #: The trap, made explicit: ``_r_wonder_step`` sits ABOVE
    #: ``_r_military_floor``, so a stage of a monument outranks not being the
    #: weakest civilisation at the table.  This is the single ordering that
    #: the corpus says loses games, and the reason this bot is in the pool.
    RULES = ("_r_round1", "_r_revolution", "_r_play_leader", "_r_happiness",
             "_r_wonder_step", "_r_military_floor", "_r_population",
             "_r_place_worker", "_r_upgrade", "_r_develop", "_r_action_card",
             "_r_take_card")
