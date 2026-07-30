"""WHICH of its own pending decisions does the drain change, and how often?

`docs/AGGRESSION_RATE.md` diagnosed the missing drain on `kind="defense"`,
because that is where the consequence was measurable (1,104 winnable defences,
0 held off).  But `PlanBot.pick`'s short-circuit did not test the kind: it fired
on ``state.pending``, and the engine pushes *three* kinds onto that stack
(`engine/interact.py`) --

    "defense"   the defender's card-by-card answer to an aggression
    "auction"   a colony/pact bid, resolved round-robin
    "choice"    everything else, carrying a `tag`: which card to discard, which
                sacrifice to take, which branch of an event to resolve, ...

-- so EVERY one of the bot's own nested decisions was priced on a position with
the rest of its own resolution still hanging, while the identical position
inside its own beam was priced after `_quiesce`.  The defence zero was the
symptom that happened to be countable, not the extent of the defect.

This tool reports the extent.  It plays real games with the drain ON, and at
every real decision of its own where the stack is non-empty it prices the
candidates BOTH ways -- `_one_ply` (no drain, master's behaviour) and
`_one_ply_quiet` (drained) -- and records whether the two disagree, split by
kind and by `choice` tag.  Both scorers are non-mutating (copy or
journal-rollback), so recording costs decisions nothing but time and the game
that gets played is the drained one.

A disagreement rate is not a strength claim; it is the size of the surface the
A/B is measuring, and it is what tells you whether a win-rate move came from
defence alone or from the whole stack.

    python3 -m tools.pending_divergence --players 3 --games 30 \
        --weights analysis/frozen/champion_3p_gen1255_99key.json --workers 2
"""
from __future__ import annotations

import argparse
import collections
import json
import os
import random
import sys
from multiprocessing import Pool

_ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
sys.path.insert(0, _ROOT)

from engine import game                                    # noqa: E402
from engine.bots.fastcopy import copy_state                 # noqa: E402
from engine.bots.plan import PlanBot, determinize           # noqa: E402
from engine.bots.weighted import (                          # noqa: E402
    DEFAULT_WEIGHTS, load_weights, rival_context)

_NO_CTX = {"rival_culture_rate": 0, "rival_science_rate": 0,
           "rival_strength": 0}


class Probe(PlanBot):
    """PlanBot recording where a lever changes its pick at its own pendings.

    ``lever="drain"`` compares `_one_ply` against `_one_ply_quiet` (the drain).
    ``lever="det"`` compares the drained pick on the raw state against the
    drained pick on a DETERMINIZED root -- i.e. it asks whether removing the
    deck peek (§9) changes anything, which is the conduction question for
    `PENDING_DETERMINIZE`.  A lever that moves 0% of picks cannot be the cause
    of a win rate, however real the thing it removes is.

    ``lever="ev"`` is the same conduction question one path over, on the BEAM
    rather than on the pending short-circuit (§9a).  `determinize` used to
    shuffle the two draw decks and leave `current_events` in its true order, so
    every `end_turn` the beam expanded revealed the real next event.  This
    lever runs the beam twice at each of the bot's own ordinary turns: once on
    a root determinized the old way (decks only) and once on a root
    determinized the new way (decks + events).

    The two roots are built from **separately seeded but identical** RNGs, and
    the deck shuffles run first in both, so the two roots have byte-identical
    decks and differ in the event pile alone.  Without that the deck order
    would change too and the measurement would be of "determinization is
    stochastic", which is not a question.  Keyed by "beam" rather than by
    pending kind, since there is no pending stack here.
    """

    def __init__(self, *a, counts=None, lever="drain", **kw):
        super().__init__(*a, **kw)
        self.QUIET_PENDING = True
        self.lever = lever
        self.counts = counts if counts is not None else collections.Counter()

    @staticmethod
    def _decks_only(state, rng):
        """`plan.determinize` as it stood before the event pile was added.

        Kept here rather than imported because the point is to compare against
        a version of the code that no longer exists; a future edit to
        `determinize` must NOT silently change this arm's control.
        """
        if state.civil_deck:
            rng.shuffle(state.civil_deck)
        if state.military_deck:
            rng.shuffle(state.military_deck)
        return state

    def _beam_pick(self, state, moves, me, w, ctx, det_fn, key):
        root = copy_state(state)
        det_fn(root, random.Random(key))
        best = self._beam(root, moves, me, w, ctx)
        # argmax spelled exactly as `PlanBot.pick` spells it, including the
        # first-wins tie break: a different tie break would show up here as a
        # divergence the lever did not cause.
        scored = [(best[mv], mv) for mv in moves if mv in best]
        if not scored:
            return moves[0]
        return max(scored, key=lambda t: t[0])[1]

    def pick(self, state, moves):
        if len(moves) == 1:
            return moves[0]
        me = state.decider()
        if self.lever == "ev":
            if state.pending or state.current != me:
                return super().pick(state, moves)
            w = self.weights
            try:
                ctx = rival_context(state, me)
            except Exception:
                ctx = dict(_NO_CTX)
            # same key both sides: the deck shuffles are drawn first and are
            # therefore identical, so the ONLY difference is the event pile
            key = (state.seed or 0) * 7919 + state.turn * 31 + me
            old = self._beam_pick(state, moves, me, w, ctx,
                                  self._decks_only, key)
            new = self._beam_pick(state, moves, me, w, ctx, determinize, key)
            self.counts[("seen", "beam")] += 1
            if old != new:
                self.counts[("moved", "beam")] += 1
            return new
        if not state.pending:
            return super().pick(state, moves)
        # A real decision of somebody's, with a non-empty stack.  Only count
        # the ones that are MINE -- the drain only ever changes my own pick.
        pend = state.pending[-1]
        if pend.get("player") != me:
            return super().pick(state, moves)
        w = self.weights
        try:
            ctx = rival_context(state, me)
        except Exception:
            ctx = dict(_NO_CTX)
        if self.lever == "det":
            root = copy_state(state)
            determinize(root, self.rng)
            plain = self._one_ply_quiet(state, moves, me, w, ctx)
            quiet = self._one_ply_quiet(root, moves, me, w, ctx)
        else:
            plain = self._one_ply(state, moves, me, w, ctx)
            quiet = self._one_ply_quiet(state, moves, me, w, ctx)
        kind = pend.get("kind", "?")
        key = kind if kind != "choice" else f"choice:{pend.get('tag', '?')}"
        self.counts[("seen", key)] += 1
        if plain != quiet:
            self.counts[("moved", key)] += 1
        return quiet


_W = {}


def _init(weights, players, lever="drain"):
    _W["w"], _W["n"], _W["lever"] = weights, players, lever


def _play(seed):
    n = _W["n"]
    counts = collections.Counter()
    bots = [Probe(weights=_W["w"], seed=1000 + i, width=2, counts=counts,
                  lever=_W.get("lever", "drain"))
            for i in range(n)]
    st = game.new_game(n, seed)
    try:
        game.play_game(bots, num_players=n, seed=seed, move_cap=20000,
                       state=st)
    except Exception:
        pass
    return counts


def report(total, players, games, lever="drain"):
    seen = {k[1]: v for k, v in total.items() if k[0] == "seen"}
    moved = {k[1]: v for k, v in total.items() if k[0] == "moved"}
    ns, nm = sum(seen.values()), sum(moved.values())
    print(f"--- pending divergence [{lever}]: {players}p, {games} games ---")
    # the `ev` lever fires at ORDINARY turns, where there is no pending stack
    # at all, so calling its denominator "pending decisions" would be a lie in
    # the one place a reader looks first
    label = ("own beam decisions   " if lever == "ev"
             else "own pending decisions")
    print(f"{label} {ns:8d}   {ns / max(games, 1):8.2f} / game")
    print(f"  LEVER CHANGED pick  {nm:8d}   {nm / max(ns, 1):8.1%} of them")
    print(f"{'kind / choice tag':38s} {'seen':>8s} {'moved':>8s} {'rate':>7s}")
    for k in sorted(seen, key=lambda k: -moved.get(k, 0)):
        print(f"  {k:36s} {seen[k]:8d} {moved.get(k, 0):8d} "
              f"{moved.get(k, 0) / seen[k]:7.1%}")


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--players", type=int, default=3)
    ap.add_argument("--games", type=int, default=30)
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--weights")
    ap.add_argument("--workers", type=int, default=2)
    ap.add_argument("--out")
    ap.add_argument("--lever", choices=("drain", "det", "ev"),
                    default="drain",
                    help="drain = does quiescing change the pick; "
                         "det = does removing the deck peek change the pick")
    a = ap.parse_args(argv)
    w = load_weights(a.weights) if a.weights else dict(DEFAULT_WEIGHTS)
    seeds = [(a.seed + g) * 7919 + 17 for g in range(a.games)]
    total = collections.Counter()
    if a.workers <= 1:
        _init(w, a.players, a.lever)
        for s in seeds:
            total.update(_play(s))
    else:
        with Pool(a.workers, initializer=_init,
                  initargs=(w, a.players, a.lever)) as pool:
            for c in pool.imap_unordered(_play, seeds, chunksize=1):
                total.update(c)
    report(total, a.players, a.games, a.lever)
    if a.out:
        with open(a.out, "w") as fh:
            json.dump({"players": a.players, "games": a.games,
                       "counts": {f"{k[0]}|{k[1]}": v
                                  for k, v in total.items()}}, fh, indent=1)
    return 0


if __name__ == "__main__":
    sys.exit(main())
