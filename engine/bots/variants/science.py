"""ScienceBot -- Alchemy first, a high science rate, Age III science impacts.

The science line is the one place where the corpus gives a *calibrated
numeric scale* rather than opinion, so this variant is built around hitting
those numbers rather than around maximising a stat forever.

MasN's science-per-turn scale at the end of Age I (BGG 2258467):

    1 = "a way to throw the game"      4 = "a good amount"
    2 = struggle but survivable        5 = a lot -- buy Code of Laws
    3 = not ideal, no panic            6 = diminishing returns
                                       7 = over-invested

and for Age III, the opposite pressure: "Science is king in age 3.  You
should be producing 10+ as a goal during that age so that you can develop a
tech each turn if possible."

THE PRIORITY LIST (each item cited)
-----------------------------------
1. **Alchemy is the Age I priority.**  On the must-buy list; "I definitely
   prioritize Alchemy over Iron" (#1 BGA player).  Age I science budget:
   "you usually have enough science for 3 things: (1) Swords or Horses,
   (2) Alchemy or Printing Press, (3) Code of Laws or Iron" -- BGG 2233796.
2. **Then the lab track**: Alchemy -> Scientific Method -> Computers.
   Computers is on the "top four techs" list and is the prerequisite for
   Bill Gates and First Space Flight.
3. **Universitas Carolina** is the wonder of this line: "an XOR with Alchemy
   and a fantastic first wonder, generally preferable to Pyramids or
   Library, but not always"; "Along with the initial two philosophies,
   provides enough science income for the rest of the game"; "will produce
   20 science (three more techs)" over ten turns.  Library of Alexandria is
   the Age A version of the same idea, and is the side of the
   Pyramids-vs-LoA disagreement this variant plays ("Library performs
   significantly better for winning the game").
4. **Aristotle, Leonardo, Newton.**  Aristotle "wins if 2 or more are
   available at the same CA cost", expected value +4 science.  Leonardo is
   "one of the most powerful age 1 economy leaders" but has a hard
   prerequisite -- "Don't take him without either Alchemy or Printing Press"
   -- which the base class already enforces and which this variant is the
   one most likely to satisfy.  Newton is Tier 1 and "can be used to go
   through a military revolution and still leave yourself with one civil
   action".
5. **Stop at the published ceiling.**  Labs are refused once production has
   reached the scale's diminishing-returns point for the current age, because
   "unless your 6 science is from LoA + 2 Alchemy + Da Vinci, you probably
   made a mistake", and because science in the bank scores nothing -- the
   exact defect ``docs/STRENGTH_CHECK.md`` measured in our champion, which
   ended games "sitting on ~15 more unspent science than BookBot".
6. **Convert science into technologies every turn** -- the develop rule is
   promoted above the card row, since "the only thing [science] is for is
   being converted into technologies."
7. **The Age III payoff is rank-based.**  Impact of Science pays
   **10/0** (2p), **14/7/0** (3p), **15/10/5/0** (4p) by *rank*, and Impact
   of Technology pays "4 culture per Age III tech".  Being merely second in
   science at 2p pays zero, which is why the rate target is player-count
   keyed.
8. **Balance rule**: "try to produce at least so many science points as your
   lowest production of food and stone" -- Impact of Balance pays "2x the
   lowest of culture / science / food / resource production."

PLAYER-COUNT PARAMETERIZATION
-----------------------------
Impact of Science is a *rank* payout, and the gap between first and second is
worth the whole prize at 2p (10 vs 0) but only half of it at 3p (14 vs 7) and
a third at 4p (15 vs 10).  The Age III science ceiling is therefore highest
at 2p, where being outproduced by one point costs the entire impact.
"""
from __future__ import annotations

from .base import VariantBot, pc

__all__ = ["ScienceBot"]


class ScienceBot(VariantBot):
    NAME = "science"

    #: Rule 5, straight off MasN's scale, by age index (0=A, 1=I, 2=II, 3=III).
    #: Above this per-turn rate the bot stops adding *new* lab workers; it
    #: keeps upgrading existing ones, which costs no worker.
    SCIENCE_CEILING = {
        0: 4, 1: {2: 6, 3: 5, 4: 5}, 2: {2: 9, 3: 8, 4: 8}, 3: 99, 4: 99,
    }

    PROFILE = {
        "prod_weights": {"food": 1.0, "resources": 1.0, "science": 1.6,
                         "culture": 1.0, "happy": 1.0, "strength": 1.0},
        "tech_bonus": {
            "Alchemy": 10.0,             # rule 1
            "Scientific Method": 8.0,    # rule 2
            "Computers": 8.0,
            "Printing Press": 5.0,       # science AND culture
            "Journalism": 5.0,
            "Multimedia": 4.0,
            "Code of Laws": 5.0,         # "5 science = a lot, buy Code of Laws"
            "Architecture": 3.0,
            # "Satellites" is on the never-buy list alongside Oil
            "Satellites": -6.0,
            "Oil": -6.0,
            "Masonry": -3.0,
        },
        "tech_veto": frozenset(("Oil", "Satellites")),
        "card_bonus": {
            "Universitas Carolina": 6.0,    # rule 3
            "Library of Alexandria": 4.0,
            "First Space Flight": 3.0,      # "around 20 to 30" with science
            "Breakthrough (I)": 3.0,
            "Breakthrough (II)": 3.0,
        },
        "leader_bonus": {                   # rule 4
            "Aristotle": 4.0,
            "Leonardo da Vinci": 3.0,       # conditional check is in book.py
            "Isaac Newton": 4.0,
            "Moses": -2.0,
        },
        "wonder_bonus": {
            "Universitas Carolina": 1.5,
            # rule 3: this variant plays the LoA side of the LoA-vs-Pyramids
            # disagreement.  TempoBot plays the Pyramids side.
            "Library of Alexandria": 1.3,
            "Pyramids": 0.95,
            "First Space Flight": 1.4,
            "Great Wall": 0.7,
        },
        "build_bonus": {"lab": 3.0, "library": 2.0},
        "price_scale": 1.0,
        "max_take_cost": {0: 2, 1: 2, 2: 2, 3: 3, 4: 3},
        "wonder_appetite": 1.0,
        "wonder_max": 3,
        "pop_appetite": 1.2,               # labs need workers
    }

    #: Develop is promoted above every discretionary spend: unspent science
    #: is the single largest defect measured in docs/STRENGTH_CHECK.md.
    RULES = ("_r_round1", "_r_revolution", "_r_play_leader", "_r_happiness",
             "_r_military_floor", "_r_develop", "_r_wonder_step",
             "_r_population", "_r_place_worker", "_r_upgrade",
             "_r_action_card", "_r_take_card")

    # rule 5 --------------------------------------------------------------
    def science_ceiling(self, ctx):
        return pc(self.SCIENCE_CEILING.get(ctx.age, 99), ctx.nplayers)

    def _best_build(self, state, p, ctx, by_kind, happy_only=False):
        """Refuse to staff another lab past the published ceiling.

        Without this the science weighting runs away and the civilisation
        ends up over-invested -- MasN's "7 = over-invested", and the general
        rule that "all types of stored resources are worth nothing at the
        end".
        """
        if ctx.s.science >= self.science_ceiling(ctx):
            builds = [m for m in by_kind.get("build", ())
                      if ctx.db.type_of(m[1]) != "lab"]
            by_kind = dict(by_kind, build=builds)
        return super()._best_build(state, p, ctx, by_kind,
                                   happy_only=happy_only)
