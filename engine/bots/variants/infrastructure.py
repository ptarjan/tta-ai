"""InfraBot -- the orthodox Iron + Irrigation line (the 3-4 player book).

This is Camp A of the "Iron vs 3-Bronze" row in the disagreements table of
``docs/EXPERT_STRATEGY.md``: the line the ~100-game 3-4p strategy guide
teaches, and the direct opponent of :mod:`engine.bots.variants.tempo`.

THE PRIORITY LIST (each item cited)
-----------------------------------
1. **Reach 2 farm / 3 mine / 2 lab by turn 3.**  "On turn 3 you then send a
   worker to philosophy or bronze to end up with 2 workers at agriculture,
   3 workers at bronze and 2 workers at philosophy... Bronze needs more
   workers because stone is much more valuable than food in the first age."
   -- BGG 2801950; corroborated verbatim by the Steam guide.
2. **Upgrade the three Bronze mines to Iron.**  Goal #2 of Age I is
   "Upgrading 3 bronze mines to iron mines"; "Iron is often worth 3 civil
   actions"; "You can't win by producing 3 stones per round, so get Coal as
   soon as you can if you haven't got Iron" -- BGG 2801950.  "Iron is one of
   the best techs in the game" -- BGG 2233796.
3. **Irrigation at the end of Age I.**  "the farms are practically
   unskippable.  95% of the time you'll want either Irrigation or the Age II
   counterpart"; "Irrigation is a tech predestined to be developed at the end
   of Age I, not significantly earlier or later" (rushing it causes
   corruption and happiness trouble) -- BGG 2258467, 2724657.
4. **Hit the published rock benchmarks**: end of Age I = 3/turn acceptable
   *with Iron*; end of Age II = "+5 or better"; Age III = "+6 or better is
   optimal".  +3 and +5 per turn are the breakpoints that buy "an Urban
   Building/Knight/Swordsman every turn" and "an Age II Military every
   turn." -- BGG 2097526
5. **Alchemy for the science engine**, targeting ~4 science/turn at the end
   of Age I ("a good amount" on MasN's scale; 3 is "no panic").
6. **Constitutional Monarchy** as the government target -- "generally the
   best peaceful development in the game."
7. Military to the floor only; economy first.

WHY IT IS NOT JUST BOOKBOT
--------------------------
BookBot v2 already leans orthodox, so this variant is deliberately the
*closest* member of the roster to it.  What differs: the mine and farm
tracks carry explicit priority bonuses rather than falling out of generic
production value, upgrading in place is promoted above taking cards (the
opposite of TempoBot's ordering), and the "don't upgrade a track at the very
end of its age" discipline is applied to the Coal/Oil decision.  Its value to
the roster is as the *orthodox pole*: a training opponent that punishes a
bot which cannot keep up on raw production.

PLAYER-COUNT PARAMETERIZATION
-----------------------------
The Iron priority is strongest at 3-4p, where "not taking Iron creates a big
risk where Coal is hate drafted or comes out very late", and weakest at 2p,
where the same corpus says "Iron is not worth 3 civil actions in most
situations."  The Iron bonus is therefore keyed by player count rather than
being one number pretending to be universal.
"""
from __future__ import annotations

from .base import VariantBot

__all__ = ["InfraBot"]


class InfraBot(VariantBot):
    NAME = "infra"

    PROFILE = {
        "tech_bonus": {
            # rule 2, keyed by player count (see docstring)
            "Iron": {2: 4.0, 3: 7.0, 4: 8.0},
            "Coal": {2: 2.0, 3: 4.0, 4: 5.0},
            # rule 3 -- and Selective Breeding as the Age II fallback,
            # "always worth 3 civil actions" but only if you skipped Irrigation
            "Irrigation": 6.0,
            "Selective Breeding": 4.0,
            # rule 5
            "Alchemy": 6.0,
            "Scientific Method": 4.0,
            # rule 6 and the 5th action
            "Code of Laws": 5.0,
            "Constitutional Monarchy": 6.0,
            # blue techs: "Architecture (6 sci) > Justice System / Navigation.
            # Skip Masonry."
            "Architecture": 3.0,
            "Masonry": -3.0,
            # "Oil: the worst CP in the game"; "you want to convert your rocks
            # to culture, not more rock production"
            "Oil": -6.0,
            # "don't take both Iron and Coal, or both Age I and Age II of the
            # same tech" is handled by the engine's level check, which already
            # values a same-track upgrade at 0 once the better one is in play.
        },
        "tech_veto": frozenset(("Oil",)),
        "card_bonus": {
            "Universitas Carolina": 2.0,   # "enough science income for the rest"
            "Engineering Genius (A)": 3.0,
            "Rich Land (A)": 2.0,
        },
        "leader_bonus": {
            # "Aristotle wins if 2 or more are available at the same CA cost"
            "Aristotle": 2.0,
            # Barbarossa "needs (1) Irrigation, (2) a 3rd MA, (3) a happiness
            # solution" -- this is the only variant that reliably has (1).
            "Frederick Barbarossa": 2.0,
            "Moses": -1.0,                 # "producing too much food can be problematic"
        },
        # rule 1/4: production tiles are the plan
        "prod_weights": {"food": 1.1, "resources": 1.3, "science": 1.0,
                         "culture": 0.9, "happy": 1.0, "strength": 1.0},
        "build_bonus": {"mine": 1.5, "farm": 0.5, "lab": 0.5},
        "pop_appetite": 1.2,
        "price_scale": 1.0,
        "max_take_cost": {0: 2, 1: 2, 2: 2, 3: 3, 4: 3},
        "wonder_appetite": 1.0,
        "wonder_max": 3,
    }

    #: Upgrading in place is promoted above the card row -- one civil action,
    #: no new worker, and it frees the old card's production immediately.
    #: This is the exact inverse of TempoBot's ordering, which is what makes
    #: the pair a real disagreement rather than two flavours of one plan.
    RULES = ("_r_round1", "_r_revolution", "_r_play_leader", "_r_happiness",
             "_r_military_floor", "_r_wonder_step", "_r_population",
             "_r_place_worker", "_r_upgrade", "_r_develop", "_r_action_card",
             "_r_take_card")
