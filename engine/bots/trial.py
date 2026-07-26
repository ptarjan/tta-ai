"""Machinery shared by every bot that searches by applying trial moves.

Two things live here rather than in ``bots/__init__.py``, for one reason:
``bots/__init__.py`` imports ``bots.weighted`` at module scope, so anything
``weighted`` needs from the package would be a half-initialised import.  A leaf
module both can import is the only arrangement that does not depend on the
order of the lines in ``__init__.py``.

Keeping ONE ``_TRIAL_RNG`` shared between ``GreedyBot`` and ``WeightedBot`` is
deliberate and safe -- see the note on the object below.
"""
from __future__ import annotations

import os
import random

__all__ = ["USE_JOURNAL", "TrialRandom", "TRIAL_RNG", "TRIAL_RNG_STATE",
           "fresh_trial_rng"]

#: Search with the undo stack (docs/PYPY.md section 6) instead of `copy_state`.
#: OPT-IN and off by default.  `QuiescentBot` must NEVER honour this: it holds
#: several live trial states at once (`_war_value` copies a state that is
#: itself already a trial) and `journal.begin` raises on nesting by design --
#: docs/PYPY.md 9.13 and 9.15.
USE_JOURNAL = os.environ.get("TTA_JOURNAL") == "1"


# A searching bot hands every candidate move a *freshly seeded* ``Random(0)`` so
# each candidate sees the identical random stream from the identical starting
# point.  Constructing one per candidate cost ~6% of GreedyBot's runtime and
# ~10.8% of a profile overall (docs/PYPY.md 5a/8): seeding a Mersenne Twister
# runs ``init_by_array`` over a 624-word state array, ~9 us a time, thousands of
# times per game.
#
# Measured fact that makes the cheap fix possible: a trial ``apply`` consumes
# the rng in only ~0.4% of candidates (69 of 18003 in a 4-game 4p sample) -- the
# engine's only rng use anywhere is ``rng.shuffle``.  So the rng object is
# reused, and re-seeded ONLY when the previous candidate actually drew from it;
# an untouched Mersenne Twister is already byte-identical to a fresh
# ``Random(0)``, so every candidate still sees exactly the ``Random(0)`` stream
# from its start.  ``used`` is set by the two C-level entry points every other
# method is built on (``random`` and ``getrandbits``), so no draw can escape it.
class TrialRandom(random.Random):
    """``Random`` that records whether it has been drawn from."""

    used = False

    def random(self):
        self.used = True
        return super().random()

    def getrandbits(self, k):
        self.used = True
        return super().getrandbits(k)


#: ONE instance, shared by GreedyBot and WeightedBot.  Sharing is safe because
#: the invariant is not "this object belongs to one bot" but "the state is
#: pristine at the start of every candidate": `fresh_trial_rng` re-seeds
#: whenever the *previous* consumer drew, whoever that was.  Two bots never
#: search concurrently anyway -- a game asks exactly one bot for a move at a
#: time -- but the invariant does not rely on that.
TRIAL_RNG = TrialRandom(0)
TRIAL_RNG_STATE = TRIAL_RNG.getstate()   # a pristine Random(0), frozen


def fresh_trial_rng():
    """The shared trial rng, reset iff the last candidate drew from it."""
    r = TRIAL_RNG
    if r.used:
        r.setstate(TRIAL_RNG_STATE)
        r.used = False
    return r
