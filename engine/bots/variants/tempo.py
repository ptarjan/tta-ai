"""TempoBot -- the wide, 3-Bronze, buy-the-5th-action line.

This is Camp B of the "Iron vs 3-Bronze" row in the disagreements table of
``docs/EXPERT_STRATEGY.md``, and it is the line the strongest *2-player*
players describe.

THE PRIORITY LIST (each item cited)
-----------------------------------
1. **Do not upgrade the mine track.**  "there is no need to get a mine tech.
   In most cases if you miss Iron you complete the game on bronze.  I
   definitely prioritize Alchemy over Iron" (#1 BGA player); "~2/3 of the
   games I played, I did not upgrade from 3 Bronze"; a top-5 player: "very
   often in top 10 players matches they don't improve either mines or
   farms."  "I have won games against high-level players with only 3 bronze
   all game.  Lots of CAs to snag lots of yellow cards is the key."
   -- BGG 2393942, 2097526
2. **Buy the 5th civil action.**  "the 5th CA is critical to allow you to
   grab yellows, and as such Code of Laws is an MVP tech" (BGG 2393942);
   "1 extra civil action is as valuable as 2 upgrades of Iron!  So I usually
   think Code of Laws' priority is higher than Iron" (BGG 2732988); 30k-game
   data: "Code of Laws performs much better than Warfare."  The three
   sources of that action are **Code of Laws, Pyramids and Hammurabi**.
3. **A 4th Bronze beats an Iron upgrade.**  Iron for a single mine is
   "5 science, 3 actions, 3 ore for +1 ore/turn... no better than just
   building a 4th Bronze (0 science, 1 action, 1 worker, 2 ore)"
   -- hcy1.blogspot.com/2017/03/tta_31.html
4. **Take lots of cheap yellow cards.**  "Most yellow cards are great picks
   at 1CA from Age 1"; "Engineering Genius for 1 CA is a no-brainer"
   (killswitch19).  Never Stockpile / Patriotism A / Cultural Heritage /
   Frugality -- those cost a *second* civil action to realise.
5. **Stay at the cheap end of the row.**  76% of tournament Age I picks
   were made at 1 CA; this variant is the most 1-CA-biased of the roster
   (``price_scale`` 1.2, and Age A capped at 1 CA outright: "I often see
   players take Hammurabi for 2 CA when they could also get him next turn
   for 1, don't do that").
6. Military only to the floor -- "you don't have to be in the lead, but you
   don't want to be in last place."  Tempo pays for actions, not armies.

PLAYER-COUNT PARAMETERIZATION (required: this is a 2p-derived line)
-------------------------------------------------------------------
The lobby that produced rule 1 is almost exclusively 2p, and the corpus has
an explicit 4p counter: "in four player at least not taking Iron creates a
big risk where Coal is hate drafted or comes out very late" (BGG 2393942),
and 3-4p advice says "You can't win by producing 3 stones per round, so get
Coal as soon as you can if you haven't got Iron" (BGG 2801950).  So the mine
veto is keyed by player count: pure 3-Bronze at 2p, Iron still skipped at
3p/4p but **Coal is allowed** at 4p rather than riding Bronze into Age III.

KNOWN WEAKNESS (stated honestly)
--------------------------------
Rock production of 3/turn is below the published Age II benchmark of "+5 or
better".  If the extra civil actions do not convert into yellow-card
resources, this civilisation simply cannot afford Age II units or wonders.
That is the actual disagreement, and this bot is the side that says the
actions are worth more than the rocks.
"""
from __future__ import annotations

from .base import VariantBot

__all__ = ["TempoBot"]


class TempoBot(VariantBot):
    NAME = "tempo"

    PROFILE = {
        # rule 1 + the player-count split.  Oil is universally rejected
        # ("the worst CP in the game"), so it is vetoed at every count.
        "tech_veto": {
            2: frozenset(("Iron", "Coal", "Oil")),
            3: frozenset(("Iron", "Coal", "Oil")),
            4: frozenset(("Iron", "Oil")),
        },
        # never spend a civil action turning a Bronze into anything
        "upgrade_veto": {
            2: frozenset(("mine",)),
            3: frozenset(("mine",)),
            4: frozenset(),
        },
        "tech_bonus": {
            "Code of Laws": 12.0,   # rule 2, the MVP tech of this line
            "Alchemy": 5.0,         # "I definitely prioritize Alchemy over Iron"
            "Knights": 4.0,         # "always worth 2 civil actions"
            "Swordsmen": 3.0,       # the acceptable substitute
            "Irrigation": 1.0,      # tolerated, not chased
            "Masonry": -3.0,        # "3 Science + 2 CA early game" for nothing
        },
        "card_bonus": {
            # rule 2: the other two sources of the 5th action
            "Pyramids": 6.0,
            # rule 4: the cheap yellows this line is built to spend on
            "Engineering Genius (A)": 4.0,
            "Engineering Genius (I)": 3.0,
            "Rich Land (A)": 2.5,   # "almost all the things you want early
            "Rich Land (I)": 2.5,   #  come in increments of 3 rocks"
            "Urban Growth (A)": 2.0,
            "Urban Growth (I)": 2.0,
            "Breakthrough (I)": 2.0,
            "Patriotism (I)": 1.5,  # "Patriotism I, on the other hand... pretty good"
        },
        "leader_bonus": {
            # "for 2p I would pick Hammurabi, an extra CA worths so much than
            # any others"; the fast leader you may draft freely behind.
            "Hammurabi": 3.0,
            # slow leaders: "must spend, so draft less" -- wrong for this plan
            "Moses": -2.0,
        },
        # rule 5: the most 1-CA-disciplined variant in the roster
        "price_scale": 1.2,
        "max_take_cost": {0: 1, 1: 2, 2: 2, 3: 3, 4: 3},
        # it takes many cards, so hand rot is the real risk it must respect
        "hand_penalty": 1.1,
        # rule 3: a 4th (and 5th) Bronze instead of an upgrade
        "build_bonus": {"mine": 2.0},
        # wide = more workers on more cheap tiles
        "pop_appetite": 1.5,
        # actions, not armies
        "wonder_appetite": 0.9,
        "wonder_max": 2,
    }

    #: Reordered: action cards (free tempo) and card-row picks come before
    #: upgrading anything in place, because the whole plan is that a civil
    #: action spent on the row beats a civil action spent on a mine.
    #: "CAs which you spend for grabbing from the card row increase in value
    #: as the game progresses, while it is the other way around for CAs you
    #: spend to upgrade workers." -- BGG 2724657
    RULES = ("_r_round1", "_r_revolution", "_r_play_leader", "_r_happiness",
             "_r_military_floor", "_r_action_card", "_r_wonder_step",
             "_r_population", "_r_place_worker", "_r_develop",
             "_r_take_card", "_r_upgrade")
