"""How much of the copied state does a 1-ply bot actually MUTATE?

`GreedyBot.pick` copies the whole `GameState` once per candidate move
(`engine/bots/fastcopy.copy_state`), applies the move to the copy and throws
the copy away.  If the applied move only touches a tiny slice of the state,
then any constant-factor speed-up of `copy_state` is the wrong fix and a
copy-on-write state (or an undo stack) is the right one.

This tool measures that fraction directly.  For every candidate move at every
GreedyBot decision point it:

  1. copies the state,
  2. applies the move to the copy,
  3. structurally diffs copy vs original,

and reports two ratios:

  * **slots**  -- scalar leaves (dict values, list items, dataclass fields)
    that differ, over all scalar leaves copied.  This is the "how much data
    changed" number.
  * **nodes**  -- container objects (dataclass / dict / list / set) that lie on
    a path to *some* change, over all containers copied.  This is the number a
    copy-on-write state would have to clone: COW clones exactly the spine from
    the root down to every mutation.

`log` and `_`-prefixed attributes are excluded, exactly as `copy_state`
excludes them.

    nice -n 10 python3 tools/measure_mutation.py --games 3
"""
from __future__ import annotations

import argparse
import collections
import random
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from engine import actions, game                  # noqa: E402
from engine.bots import GreedyBot                 # noqa: E402
from engine.bots.fastcopy import copy_state       # noqa: E402

_ATOMIC = (str, int, float, bool, bytes, type(None))
_SKIP = ("log",)


class Counter:
    __slots__ = ("slots", "slots_changed", "nodes", "nodes_changed")

    def __init__(self):
        self.slots = self.slots_changed = self.nodes = self.nodes_changed = 0


def _children(v):
    """(key, value) pairs of a container, or None if v is a leaf."""
    t = type(v)
    if t in (list, tuple):
        return list(enumerate(v))
    if t is dict:
        return list(v.items())
    if t is set:
        return [(x, x) for x in v]
    if hasattr(v, "__dataclass_fields__"):
        return [(k, x) for k, x in v.__dict__.items() if k[0] != "_"]
    return None


def diff(a, b, c: Counter, top=False):
    """Walk a (original) and b (mutated copy) in parallel.  Returns changed?"""
    ca = _children(a)
    cb = _children(b) if ca is not None else None
    if ca is None or cb is None:
        # leaf (or type changed): one slot
        c.slots += 1
        if a is not b and a != b:
            c.slots_changed += 1
            return True
        return False

    c.nodes += 1
    changed = False
    da, db = dict(ca), dict(cb)
    if da.keys() != db.keys():
        changed = True
        # keys added/removed count as changed slots too
        c.slots += len(da.keys() ^ db.keys())
        c.slots_changed += len(da.keys() ^ db.keys())
    for k in da.keys() & db.keys():
        if top and k in _SKIP:
            continue
        if diff(da[k], db[k], c):
            changed = True
    if changed:
        c.nodes_changed += 1
    return changed


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--games", type=int, default=2)
    ap.add_argument("--players", type=int, default=4)
    ap.add_argument("--seed", type=int, default=11)
    ap.add_argument("--max-moves", type=int, default=100000)
    a = ap.parse_args()

    tot = Counter()
    per_kind = collections.defaultdict(Counter)
    n_cand = 0
    decisions = 0

    for g in range(a.games):
        seed = a.seed + g
        bots = [GreedyBot(random.Random(seed * 977 + i))
                for i in range(a.players)]
        st = game.new_game(a.players, seed=seed)
        moves_done = 0
        while not game.is_over(st) and moves_done < a.max_moves:
            legal = actions.legal_moves(st)
            if not legal:
                break
            if len(legal) > 1:
                decisions += 1
                for mv in legal:
                    trial = copy_state(st)
                    try:
                        actions.apply(trial, mv, random.Random(0))
                    except Exception:
                        continue
                    c = Counter()
                    diff(st, trial, c, top=True)
                    n_cand += 1
                    for obj in (tot, per_kind[mv[0]]):
                        obj.slots += c.slots
                        obj.slots_changed += c.slots_changed
                        obj.nodes += c.nodes
                        obj.nodes_changed += c.nodes_changed
            mv = bots[st.decider() % len(bots)].choose(st, legal)
            actions.apply(st, mv, random.Random(moves_done))
            moves_done += 1
        print(f"game {g}: {moves_done} moves, {decisions} multi-move decisions",
              file=sys.stderr)

    if not n_cand:
        print("no candidates", file=sys.stderr)
        return 1

    print(f"\n{a.players}p GreedyBot, {a.games} games, "
          f"{decisions} branching decisions, {n_cand} candidate moves\n")
    print(f"mean scalar slots copied per candidate : {tot.slots/n_cand:9.1f}")
    print(f"mean scalar slots MUTATED per candidate: "
          f"{tot.slots_changed/n_cand:9.2f}   "
          f"({100*tot.slots_changed/tot.slots:.3f}%)")
    print(f"mean container nodes copied            : {tot.nodes/n_cand:9.1f}")
    print(f"mean container nodes on a mutated path : "
          f"{tot.nodes_changed/n_cand:9.2f}   "
          f"({100*tot.nodes_changed/tot.nodes:.3f}%)")

    print("\nby move kind (candidates / slots changed / nodes on mutated path):")
    rows = sorted(per_kind.items(), key=lambda kv: -kv[1].slots)
    for kind, c in rows[:14]:
        n = max(1, c.slots // max(1, int(tot.slots / n_cand)))
        print(f"  {kind:22s} slots {100*c.slots_changed/max(1,c.slots):6.3f}%"
              f"   nodes {100*c.nodes_changed/max(1,c.nodes):6.3f}%")
    return 0


if __name__ == "__main__":
    sys.exit(main())
