"""The variant roster: a pool of distinct, strong, rule-based opponents.

WHY A ROSTER AND NOT A BOT
--------------------------
``docs/STRENGTH_CHECK.md`` showed self-play had converged on weak play: the
champion only ever had to beat a mirror of itself, and a hand-written rule
list beat it 62.9% at 2p.  Training against a single opponent teaches a bot
to beat that opponent.  **Diversity is the point.**  A bot that beats one
opponent type has learned an exploit; a bot that beats six structurally
different ones has learned something about Through the Ages.

WHERE THE ROSTER COMES FROM
---------------------------
Each variant is a side of a row in the "Biggest open disagreements" table at
the end of ``docs/EXPERT_STRATEGY.md`` -- the places where strong human
players genuinely split.  Rather than resolve those rows by fiat, each side
becomes a playable strategy:

======================  ==========================================
variant                 the position it plays
======================  ==========================================
``tempo``               3-Bronze, buy the 5th civil action, take
                        many 1-CA yellows.  The top-2p-player line.
``infra``               Iron + Irrigation, the orthodox 3-4p line.
``military``            Top-2 military position, and actually cash
                        the strength into aggressions and wars.
``culture``             Build a culture rate in Age I, play for the
                        Age III urban-building impacts.
``science``             Alchemy first, hit the published science
                        scale, play for the rank-based Age III
                        science impacts.
``wonder``              Michelangelo / wonder spam.  Included
                        deliberately as the line experts call a
                        noob trap, so training learns to punish it.
======================  ==========================================

All six share :class:`engine.bots.variants.base.VariantBot`, which is
``engine/bots/book.py``'s priority list in its v2 (empirical tournament tier
list) configuration with a profile of knobs bolted on.  They are plain
callables like every other bot in this package, deterministic given their
rng, and they never draw from it.

USAGE
-----
::

    from engine.bots.variants import make_variant, VARIANTS
    bot = make_variant("tempo", seed=7)
    list(VARIANTS)          # -> ['tempo', 'infra', 'military', ...]

The round-robin that measures them is ``experiments/roster_match.py`` and
its results table is ``docs/BOT_ROSTER.md``.
"""
from __future__ import annotations

from .base import DEFAULT_PROFILE, VariantBot
from .culture import CultureBot
from .infrastructure import InfraBot
from .military import MilitaryBot
from .science import ScienceBot
from .tempo import TempoBot
from .wonder import WonderBot

__all__ = ["VARIANTS", "make_variant", "VariantBot", "DEFAULT_PROFILE",
           "TempoBot", "InfraBot", "MilitaryBot", "CultureBot", "ScienceBot",
           "WonderBot"]

#: name -> class.  Order is the roster order used in the docs table.
VARIANTS = {
    TempoBot.NAME: TempoBot,
    InfraBot.NAME: InfraBot,
    MilitaryBot.NAME: MilitaryBot,
    CultureBot.NAME: CultureBot,
    ScienceBot.NAME: ScienceBot,
    WonderBot.NAME: WonderBot,
}


def make_variant(name, seed=None, rng=None, profile=None):
    """Build a variant by name.

    Raises ``KeyError`` with the full roster listed, because a silent
    fallback to some default bot would quietly corrupt a benchmark.
    """
    try:
        cls = VARIANTS[name]
    except KeyError:
        raise KeyError(f"unknown variant {name!r}; "
                       f"known: {', '.join(VARIANTS)}") from None
    return cls(rng=rng, seed=seed, profile=profile)
