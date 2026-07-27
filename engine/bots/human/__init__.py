"""Human-derived pool opponents, fitted to the 1,011-game BGO corpus.

Read `archetypes.py`'s docstring first: the headline evidence is that the
corpus does NOT contain clean archetypes, and that determined the shape of
everything here.

    from engine.bots.human import HUMANS, make_human
    bot = make_human("builder", seed=7)
    list(HUMANS)            # -> ['builder', 'wonder', 'tempo', 'warlord']

These are separate from `engine/bots/variants/` on purpose.  The variants are
positions in an expert-strategy argument, hand-tuned; these are fits to
measured human behaviour, they are stochastic, and they carry a `TARGET` the
fit can be re-run and re-checked against.  Mixing them would also have let the
`variant` pool tier's weight be split ten ways instead of six, quietly
demoting the existing roster (see `experiments/hillclimb_pool.py`'s weighting
note) -- they get their own tier instead.
"""
from __future__ import annotations

import random

from .base import HUMAN_DEFAULTS, HumanBot, logistic
from .archetypes import (HumanBuilderBot, HumanTempoBot, HumanWarlordBot,
                         HumanWonderBot)

__all__ = ["HUMANS", "make_human", "HumanBot", "HUMAN_DEFAULTS", "logistic",
           "HumanBuilderBot", "HumanWonderBot", "HumanTempoBot",
           "HumanWarlordBot"]

#: short name -> class.  The short name is what `human:builder` and the pool
#: label `hum:builder` are built from.
HUMANS = {
    "builder": HumanBuilderBot,
    "wonder": HumanWonderBot,
    "tempo": HumanTempoBot,
    "warlord": HumanWarlordBot,
}


def make_human(name, seed=None, rng=None, profile=None):
    """Build an archetype by short name."""
    cls = HUMANS[name]
    return cls(rng=rng or (random.Random(seed) if seed is not None else None),
               profile=profile)
