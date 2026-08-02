"""One cache of played-out games, shared by every test that needs positions.

WHY THIS EXISTS.  The suite took fifteen minutes, and almost none of that was
assertions -- it was the same handful of self-play games being replayed from
scratch by every test that wanted a realistic position to look at.
`test_event_scoring` alone played `_play(2, 7)` nine times, `test_counting` ran
three seeds through ten tests, and none of them wanted a *different* game.  A
played game is expensive; a copy of one is cheap (`fastcopy.copy_state` is the
function the search itself uses, hundreds of times per decision).

So: play each game ONCE per process, keep the states, and hand every caller its
own deep copy.

THE COPY IS NOT OPTIONAL.  Tests here mutate what they are given -- swapping a
card in a rival's hand, reversing the deck, crediting an event to a player -- so
handing out the cached object itself would let one test's mutation reach
another's assertion, which is a far worse failure than a slow suite: it would
be order-dependent and it would look like a real bug in the engine.  Every
accessor below returns fresh copies and the cache holds states no caller ever
touches.

WHAT IT DOES NOT DO.  It does not reduce what any test checks.  The games are
the same games, played by the same bots at the same seeds; only the number of
times they are played changes.  A test that wants a game nobody else wants
simply passes its own seed and gets it cached for next time.
"""
import functools
import os
import random
import sys

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

from engine import actions as A, game                          # noqa: E402
from engine.bots import weighted as W                          # noqa: E402
from engine.bots.fastcopy import copy_state                    # noqa: E402
from engine.bots.weighted import WeightedBot                   # noqa: E402


@functools.lru_cache(maxsize=None)
def _played(players, seed, bot):
    """The FINAL state of a game.  Cached, never handed out directly.

    Built through `engine.bots.make_bots`, which is how the callers built it
    before this module existed -- the cache must not quietly play a *different*
    game than the tests were written against."""
    from engine.bots import make_bots
    return game.play_game(make_bots(bot, players, seed=seed), players,
                          seed=seed)


def played(players=2, seed=7, bot="weighted"):
    """A finished game.  Your own copy: mutate it freely."""
    return copy_state(_played(players, seed, bot))


@functools.lru_cache(maxsize=None)
def _walk(players, seed, every, limit):
    """Positions sampled every `every` plies out of one self-play game."""
    st = game.new_game(players, seed=seed)
    bots = [WeightedBot(W.DEFAULT_WEIGHTS, seed=i) for i in range(players)]
    rng = random.Random(seed)
    out = []
    for step in range(limit):
        if st.game_over:
            break
        i = st.decider()
        if i is None:
            break
        mv = bots[i].pick(st, A.legal_moves(st))
        if mv is None:
            break
        game.apply(st, mv, rng)
        if step % every == 0:
            out.append(copy_state(st))
    return tuple(out)


def positions(players=3, seed=7, every=40, limit=2000):
    """Mid-game positions from one self-play game.  Your own copies."""
    return [copy_state(s) for s in _walk(players, seed, every, limit)]


def copy_of(state):
    """A copy of a state a caller already holds.

    Here rather than importing `fastcopy` at each call site: a test that reuses
    a position for a second run and forgets to copy it compares two different
    games and passes for the wrong reason."""
    return copy_state(state)
