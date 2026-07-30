"""How hot is §6.6 step 1, and how often did FIFO pitch the best defender?

The size claim in docs/MILITARY_DISCARD.md §2, measured rather than inherited.
Plays self-play games, answers every `discard_military` decision the way the
pre-fix engine did (oldest card first), and records at each firing:

* whether the FIFO card was a strictly WORSE defender than some other option
  (FIFO was fine / the choice is free), or
* whether the FIFO card was the SOLE best defender in hand while a strictly
  worse card was available -- the case where the old engine provably threw the
  player's best defensive card away.

`defense_points` is the engine's own combat arithmetic (§5.4.4), not a
heuristic: bonus cards 2/4/6, every other military card the flat +1 of a
face-down card.

    nice -n 19 python3 -m tools.discard_census --games 20 --players 2
"""
from __future__ import annotations

import argparse
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine import game                                       # noqa: E402
from engine.interact import defense_points                    # noqa: E402
from experiments.arena import load_spec, make_bot             # noqa: E402


class FifoCensus:
    """Answer the discard the way the old engine did; count what it cost."""

    def __init__(self, inner, idx, tally):
        self.inner = inner
        self.idx = idx
        self.t = tally

    def __call__(self, state):
        pend = state.pending[-1] if state.pending else None
        if not (pend and pend.get("kind") == "choice"
                and pend.get("tag") == "discard_military"
                and pend.get("player") == self.idx):
            return self.inner(state)
        opts = pend["options"]
        hand = state.players[self.idx].hand_military
        oldest = hand[0] if hand else opts[0]
        i = opts.index(oldest) if oldest in opts else 0
        d = [defense_points(o) for o in opts]
        self.t["fires"] += 1
        self.t["options"] += len(opts)
        best = max(d)
        if d[i] == best and sum(1 for x in d if x == best) == 1 and min(d) < best:
            # the FIFO card is the SOLE best defender and something strictly
            # worse was available instead
            self.t["pitched_sole_best"] += 1
            self.t["defence_lost"] += best - min(d)
        if d[i] > min(d):
            self.t["pitched_above_worst"] += 1
        return ("choose", i)


def count_cards(tally):
    """Count cards discarded by §6.6 step 1 WITHOUT a decision.

    A hand whose remaining cards all share one name is discarded by
    `push_choice`'s auto-resolution with no decision at all, so the decision
    count is a lower bound on how often the step fires.
    """
    from engine import interact
    orig = interact.discard_excess_military

    def wrapped(state, p):
        before = len(p.hand_military)
        out = orig(state, p)
        # a card removed by THIS call is one push_choice auto-resolved with a
        # single distinct option; a card removed by a decision leaves the hand
        # later, in `_c_discard_military`, and is counted as a firing instead.
        tally["auto"] += before - len(p.hand_military)
        return out
    interact.discard_excess_military = wrapped
    return orig


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--spec", default=None)
    ap.add_argument("--games", type=int, default=20)
    ap.add_argument("--players", type=int, default=2)
    ap.add_argument("--seed", type=int, default=4000)
    a = ap.parse_args(argv)
    spec = load_spec(a.spec) if a.spec else None
    t = {"fires": 0, "options": 0, "pitched_sole_best": 0,
         "pitched_above_worst": 0, "defence_lost": 0, "auto": 0}
    count_cards(t)
    turns = 0
    for g in range(a.games):
        seed = a.seed + g
        bots = []
        for i in range(a.players):
            inner = (make_bot(spec, 1000 + i) if spec
                     else _default_bot(1000 + i))
            bots.append(FifoCensus(inner, i, t))
        st = game.play_game(bots, num_players=a.players, seed=seed,
                            move_cap=20000)
        turns += st.turn
    f = max(1, t["fires"])
    print(f"{a.games} games at {a.players}p, {turns} player-turns")
    fired = t["fires"] + t["auto"]
    print(f"  cards discarded by step 1   : {fired} "
          f"({fired/a.games:.1f} per game, {fired/max(1,turns):.2f} "
          f"per player-turn)")
    print(f"  of which real DECISIONS     : {t['fires']}; auto-resolved "
          f"(one distinct option, no decision): {t['auto']}")
    print(f"  mean distinct options offered: {t['options']/f:.2f}")
    print(f"  FIFO pitched a card better than the worst available: "
          f"{t['pitched_above_worst']} ({t['pitched_above_worst']/f:.1%})")
    print(f"  FIFO pitched the SOLE best defender in hand        : "
          f"{t['pitched_sole_best']} ({t['pitched_sole_best']/f:.1%})")
    print(f"  defence points thrown away that way               : "
          f"{t['defence_lost']} ({t['defence_lost']/a.games:.1f} per game)")
    return 0


def _default_bot(seed):
    from engine.bots import WeightedBot
    return WeightedBot(seed=seed)


if __name__ == "__main__":
    raise SystemExit(main())
