"""CultureBot -- the theater/opera culture-rate engine.

This plays Camp B of the "Age I culture" row in the disagreements table of
``docs/EXPERT_STRATEGY.md``.  Camp A (the majority) says culture is
"completely irrelevant" in Age I and that building it early is the classic
way to throw a game.  Camp B is a real, cited position:

    "I find it hard to catch up if you totally ignore culture production in
    Age I.  I will find the way to have my culture generated for about +2 to
    +4 per turn... Temple + Drama + Wonder(s) give me about 4-5 culture per
    turn."  -- old.reddit r/throughtheages "Age I strategy discussion"

THE PRIORITY LIST (each item cited)
-----------------------------------
1. **A culture rate, built early and compounded.**  Age I wonders and urban
   buildings that produce 2 culture/turn are worth "as much as 30 culture
   through the game (if completed on turn 5 in a 19-round game), and usually
   at least 20" -- BGG 2494200/2569870.  The rate is the asset; the score
   follows.
2. **Theaters are for culture, not happiness** (killswitch19).  Drama (2
   culture) -> Opera (3 culture) -> Movies (4 culture); Printing Press ->
   Journalism -> Multimedia carry culture *and* science, which is why this
   line does not starve on tech.
3. **Shakespeare and Bach.**  Shakespeare "can save you up to 14 stones, 6
   science and increase the culture production by up to 8 culture per round"
   -- but is "useless if you don't have either Opera or Journalism", which
   this is the one variant that reliably has.  Bach is the "consensus
   weakest Age II" leader in 3-4p and simultaneously "Tier 1" in the cited
   2p claim -- so his bonus is keyed by player count rather than picking a
   side by fiat.
4. **Taj Mahal**, playing Camp B of its disagreement row: tournament CA-spend
   0.00 and "utterly useless" on one side; "+20 culture over Great Wall
   across 10 turns" and "the 30k dataset says strong players undervalue it"
   on the other.  Taken here at up to 2 CA, which no other variant will do.
5. **St. Peter's Basilica**, the consensus #1 wonder, which doubles as this
   line's happiness solution: "As long as you have St. Peter's Basilica, you
   can live the entire game without worrying about your happiness."  That
   matters because urban buildings eat yellow cubes and raise the happiness
   requirement.
6. **Age III impacts are the payoff**: Impact of Buildings pays "culture =
   sum of urban building levels", Impact of Variety "2 per type of military
   unit / urban building / blue tech", Impact of Happiness "2 per happy face
   (cap 16)".  A tall urban civilisation scores all three.
7. **Military still to the floor.**  The bot is not allowed to skip it:
   "gaining culture instead of building your infrastructure and military is a
   recipe for defeat", and a culture engine at +30/turn "makes you the War
   over Culture target".

KNOWN WEAKNESS (stated honestly, this is the point of the variant)
------------------------------------------------------------------
This is the plan §10 mistake #2 warns about: "Fast way to throw a game is to
play Michelangelo, meld Drama/Theology, and start spamming these culture
generators... You'll be at a 60+ culture lead in Age II, then lose two wars
over culture for like 50 points each."  A bot trained against this pool
should learn to punish it *by declaring war*, which is exactly why it earns
its place as a sparring partner even if its win rate is mediocre.

PLAYER-COUNT PARAMETERIZATION
-----------------------------
Early culture is much more dangerous with more opponents, because more
players can point aggressions and wars at the culture leader ("I had a 60-80
point lead towards the end and then had war declared on me by 3 others").
The early-culture weighting is therefore highest at 2p and damped at 3-4p,
and the Bach bonus follows the 2p-vs-multiplayer split in the sources.
"""
from __future__ import annotations

from .base import VariantBot

__all__ = ["CultureBot"]


class CultureBot(VariantBot):
    NAME = "culture"

    PROFILE = {
        # rule 1/2: culture is the axis this bot buys, damped as the table
        # grows because more opponents can punish an early culture lead
        "prod_weights": {
            "food": 1.0, "resources": 0.9, "science": 1.0,
            "culture": {2: 2.0, 3: 1.7, 4: 1.6},
            "happy": 1.2, "strength": 1.0,
        },
        "tech_bonus": {
            # rule 2 -- Drama is on one strong player's "bottom four techs"
            # list; taking it anyway *is* this variant's thesis
            "Drama": 5.0,
            "Opera": 8.0,
            "Movies": 6.0,
            "Printing Press": 5.0,
            "Journalism": 6.0,
            "Multimedia": 5.0,
            # Theology: 0 selections in 39 tournament games, and the base
            # class multiplies its production value by 0.15 accordingly.
            # This bonus deliberately buys it back -- "Joan and the first
            # phase of theology (only 2 brains) match... four smiling faces
            # can be used for a long period" (hcy1).
            "Theology": 6.0,
            "Organized Religion": 4.0,
            # "Architecture (6 sci)" is the blue tech that discounts urban
            # buildings and is the prerequisite for an Age III wonder
            "Architecture": 5.0,
            "Code of Laws": 4.0,
            "Alchemy": 3.0,
            "Oil": -6.0,
        },
        "tech_veto": frozenset(("Oil",)),
        "card_bonus": {
            # rule 4: the only variant that will pay above 1 CA for the Taj.
            # Naming it here also lifts the base class's "Taj/Great Wall at
            # 1 CA only" guard, which is the encoded majority opinion.
            "Taj Mahal": 5.0,
            "St. Peter's Basilica": 4.0,
            "Eiffel Tower": 3.0,
            "Hollywood": 3.0,
            "Endowment for the Arts": 3.0,
        },
        "leader_bonus": {
            # rule 3
            "William Shakespeare": 5.0,
            "J. S. Bach": {2: 5.0, 3: 2.0, 4: 2.0},
            # "+1 culture per Temple/government happy face"; the Joan +
            # St. Peter's pairing is named as a best base-game culture engine
            "Joan of Arc": 3.0,
            "Homer": {2: 0.0, 3: 1.5, 4: 1.5},
            "Michelangelo": 1.0,
        },
        "wonder_bonus": {
            "Taj Mahal": 1.8,             # rule 4
            "St. Peter's Basilica": 1.3,  # rule 5
            "Eiffel Tower": 1.3,          # +4 culture, "best age II wonder"
            "Hollywood": 1.4,             # "can be a 40-point spectacle"
            "Great Wall": 0.7,            # the wonder with no culture at all
        },
        # rule 6: urban buildings are the scoring engine
        "build_bonus": {"theater": 3.0, "temple": 2.0, "library": 2.0,
                        "arena": 0.5},
        "price_scale": 1.0,
        "max_take_cost": {0: 2, 1: 2, 2: 2, 3: 3, 4: 3},
        "wonder_appetite": 1.2,
        "wonder_max": 3,
        "pop_appetite": 1.2,              # urban buildings need workers
        # rule 7: the floor is not optional
        "mil_stance": "floor",
        "mil_margin": 0,
    }

    #: Placing workers (onto theaters/temples/libraries) and upgrading urban
    #: buildings in place are promoted above the wonder step, because the
    #: rate compounds and a wonder stage does not pay until it is finished.
    RULES = ("_r_round1", "_r_revolution", "_r_play_leader", "_r_happiness",
             "_r_military_floor", "_r_population", "_r_place_worker",
             "_r_upgrade", "_r_wonder_step", "_r_develop", "_r_action_card",
             "_r_take_card")
