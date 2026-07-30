"""Does draining a pending decision PEEK at the real deck?

The honest-instrument question about `PlanBot._one_ply_quiet`.  `pick`'s beam
path re-shuffles the unseen decks first (`determinize`), because
`fastcopy.copy_state` copies `civil_deck`/`military_deck` verbatim and a trial
`apply` that draws therefore draws the *real* next card -- `tools/infoleak.py`
measures 94.9% of `end_turn` candidates doing exactly that at 2p.  The pending
short-circuit does NOT determinize, on either the drained or the undrained
path.  So before a +20pp win-rate move is attributed to better defence, it has
to be shown that the drain is not simply seeing the future: an extra
`_quiesce` per candidate is extra `apply` calls, and extra `apply` calls are
extra chances to draw.

This tool counts, at every real pending decision the bot owns, how many
candidate evaluations consume real deck cards or change the visible card row --
separately for the apply (which master already does) and for the drain (which
the flip adds).  If the drain's column is zero, the drain adds no peek and the
A/B is measuring play; if it is not, the number belongs next to the win rate.

    python3 -m tools.pending_leak --players 3 --games 24 --workers 2 \
        --weights analysis/frozen/champion_3p_gen1255_99key.json
"""
from __future__ import annotations

import argparse
import collections
import json
import os
import sys
from multiprocessing import Pool

_ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
sys.path.insert(0, _ROOT)

from engine import actions, game                            # noqa: E402
from engine.bots.fastcopy import copy_state                 # noqa: E402
from engine.bots.plan import PlanBot, _rng                  # noqa: E402
from engine.bots.weighted import (                          # noqa: E402
    DEFAULT_WEIGHTS, load_weights, rival_context)

_NO_CTX = {"rival_culture_rate": 0, "rival_science_rate": 0,
           "rival_strength": 0}


def _fingerprint(st):
    """(civil deck depth, military deck depth, the visible row as names)."""
    row = tuple(getattr(c, "name", c) if not isinstance(c, dict)
                else c.get("name") for c in (st.card_row or []))
    return (len(st.civil_deck), len(st.military_deck), row)


class Probe(PlanBot):
    """Drained bot that also records what each candidate evaluation consumed."""

    def __init__(self, *a, counts=None, **kw):
        super().__init__(*a, **kw)
        self.QUIET_PENDING = True
        self.counts = counts if counts is not None else collections.Counter()

    def pick(self, state, moves):
        if len(moves) == 1:
            return moves[0]
        me = state.decider()
        if not state.pending or state.pending[-1].get("player") != me:
            return super().pick(state, moves)
        w = self.weights
        try:
            ctx = rival_context(state, me)
        except Exception:
            ctx = dict(_NO_CTX)
        c = self.counts
        c["decisions"] += 1
        before = _fingerprint(state)
        for mv in moves:
            t = copy_state(state)
            try:
                actions.apply(t, mv, _rng())
            except Exception:
                continue
            c["cand"] += 1
            mid = _fingerprint(t)
            if mid[:2] != before[:2]:
                c["apply_drew"] += 1
            if mid[2] != before[2]:
                c["apply_moved_row"] += 1
            try:
                self._quiesce(t, w, root_row=ctx.get("root_row"))
            except Exception:
                continue
            after = _fingerprint(t)
            if after[:2] != mid[:2]:
                c["drain_drew"] += 1
            if after[2] != mid[2]:
                c["drain_moved_row"] += 1
        return super().pick(state, moves)


_W = {}


def _init(weights, players):
    _W["w"], _W["n"] = weights, players


def _play(seed):
    n = _W["n"]
    counts = collections.Counter()
    bots = [Probe(weights=_W["w"], seed=1000 + i, width=2, counts=counts)
            for i in range(n)]
    st = game.new_game(n, seed)
    try:
        game.play_game(bots, num_players=n, seed=seed, move_cap=20000,
                       state=st)
    except Exception:
        pass
    return counts


def report(c, players, games):
    cand = max(c["cand"], 1)
    print(f"--- pending leak: {players}p, {games} games ---")
    print(f"own pending decisions      {c['decisions']:8d}")
    print(f"candidate evaluations      {c['cand']:8d}")
    print(f"  apply consumed deck      {c['apply_drew']:8d}  "
          f"{c['apply_drew'] / cand:6.1%}   (master already does this)")
    print(f"  apply changed the row    {c['apply_moved_row']:8d}  "
          f"{c['apply_moved_row'] / cand:6.1%}")
    print(f"  DRAIN consumed deck      {c['drain_drew']:8d}  "
          f"{c['drain_drew'] / cand:6.1%}   (added by the flip)")
    print(f"  DRAIN changed the row    {c['drain_moved_row']:8d}  "
          f"{c['drain_moved_row'] / cand:6.1%}")


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--players", type=int, default=3)
    ap.add_argument("--games", type=int, default=24)
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--weights")
    ap.add_argument("--workers", type=int, default=2)
    ap.add_argument("--out")
    a = ap.parse_args(argv)
    w = load_weights(a.weights) if a.weights else dict(DEFAULT_WEIGHTS)
    seeds = [(a.seed + g) * 7919 + 17 for g in range(a.games)]
    total = collections.Counter()
    if a.workers <= 1:
        _init(w, a.players)
        for s in seeds:
            total.update(_play(s))
    else:
        with Pool(a.workers, initializer=_init,
                  initargs=(w, a.players)) as pool:
            for x in pool.imap_unordered(_play, seeds, chunksize=1):
                total.update(x)
    report(total, a.players, a.games)
    if a.out:
        with open(a.out, "w") as fh:
            json.dump({"players": a.players, "games": a.games,
                       "counts": dict(total)}, fh, indent=1)
    return 0


if __name__ == "__main__":
    sys.exit(main())
