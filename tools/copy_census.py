"""Census of `copy_state` calls by CALL SITE, and what each copy is used for.

docs/PYPY.md section 6 argues that a copy whose only use is
``copy -> apply -> evaluate -> discard`` should be an undo-stack rollback
instead.  The undo stack landed (section 9) but only `GreedyBot` and
`WeightedBot` were converted; `PlanBot` and `QuiescentBot`, which are what the
league actually trains, are still 100% on the copy path.

Before designing anything for them, this measures the shape of their copies:

  * **by site** -- which `copy_state(...)` line makes them, and how many;
  * **discard-shaped vs survivor** -- a copy is a SURVIVOR if it is later
    passed to `copy_state` again as the source, i.e. it outlived the score that
    was computed on it.  In `PlanBot._beam` that is exactly "survived the beam
    prune and got expanded at the next ply".  Everything else was made, scored
    and thrown away, which is the shape an undo stack replaces.

Survivorship is detected without touching `engine/bots/plan.py`: every copy
made inside `_beam` is kept alive for the duration of one root decision (so
`id()` cannot be recycled underneath the measurement) and its id is checked
against the id of the source of every later `_beam` copy.

    python3 tools/copy_census.py --spec plan:width=2 --players 2 --games 4
"""
from __future__ import annotations

import argparse
import collections
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine import game                       # noqa: E402
from engine.bots import plan as _plan         # noqa: E402
from engine.bots import quiescent as _quies   # noqa: E402


class Census:
    def __init__(self):
        self.by_site = collections.Counter()
        # ids of states produced by a `_beam` copy, during one root decision
        self._made = set()
        self._survived = set()
        self._alive = []
        self.beam_copies = 0
        self.beam_survivors = 0
        self.beam_decisions = 0

    # -- the wrapper installed over each module's `copy_state` -------------
    def wrap(self, fn, module):
        def copy_state(state, *a, **k):
            f = sys._getframe(1)
            site = f"{module}:{f.f_code.co_name}:{f.f_lineno}"
            self.by_site[site] += 1
            out = fn(state, *a, **k)
            if f.f_code.co_name == "_beam":
                self.beam_copies += 1
                # `state` is being expanded, so it survived a prune.  Count
                # each surviving STATE once, not once per child it spawns.
                if id(state) in self._made:
                    self._survived.add(id(state))
                self._made.add(id(out))
                self._alive.append(out)
            return out
        return copy_state

    def wrap_beam(self, fn):
        def _beam(bot, root, *a, **k):
            self.beam_decisions += 1
            try:
                return fn(bot, root, *a, **k)
            finally:
                self.beam_survivors += len(self._survived)
                self._made.clear()
                self._survived.clear()
                self._alive.clear()
        return _beam


def run(spec_text, players, games, weights=None):
    from experiments import arena
    from tools.profile_bot import _spec
    import tools.profile_bot as pb
    pb.WEIGHTS = weights
    spec = _spec(spec_text)

    c = Census()
    _plan.copy_state = c.wrap(_plan.copy_state, "plan")
    _quies.copy_state = c.wrap(_quies.copy_state, "quiescent")
    _plan.PlanBot._beam = c.wrap_beam(_plan.PlanBot._beam)

    for seed in range(games):
        bots = [arena.make_bot(spec, seed * 131 + i) for i in range(players)]
        game.play_game(bots, players, seed=seed)
    return c


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--spec", default="plan:width=2")
    ap.add_argument("--players", type=int, default=2)
    ap.add_argument("--games", type=int, default=4)
    ap.add_argument("--weights", default=None)
    a = ap.parse_args()
    w = None
    if a.weights:
        from engine.bots.weighted import load_weights
        w = load_weights(a.weights)
    c = run(a.spec, a.players, a.games, w)
    total = sum(c.by_site.values())
    print(f"{a.spec} {a.players}p, {a.games} games, {total} copy_state calls\n")
    print(f"{'copies':>9} {'share':>7}  site")
    for site, n in c.by_site.most_common():
        print(f"{n:9d} {100 * n / total:6.1f}%  {site}")
    if c.beam_copies:
        s, n = c.beam_survivors, c.beam_copies
        print(f"\n_beam: {n} copies over {c.beam_decisions} root decisions, "
              f"{s} survived the prune and were expanded again "
              f"({100.0 * s / n:.1f}%); {100.0 * (n - s) / n:.1f}% were "
              f"made, scored and discarded.")
    print(f"\ndiscard-shaped overall: "
          f"{100.0 * (total - c.beam_survivors) / total:.1f}% "
          f"({total - c.beam_survivors} of {total})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
