"""Measure how often the evaluator cannot tell two ATTACK TARGETS apart.

WHAT THIS ANSWERS

Four move kinds name an opponent: `war`, `aggression`, `offer_pact` and
`cancel_pact` (`engine/actions.legal_moves`).  At TWO players there is exactly
one opponent, so "which target" is not a decision at all and blindness to it
costs nothing.  At three and four it is one of the most consequential choices
in the game -- hitting the runaway leader and hitting the player in last place
are not the same move -- and it is a decision class the 2p arm literally
cannot exercise.  That asymmetry is why this tool exists and why it only runs
at 3p/4p.

THE METHOD, and its one real bound.

At every decision, the legal moves are grouped so that the ONLY thing varying
inside a group is the target: `("war", "War over Territory", q)` for each legal
`q`, and likewise for the others.  A group of fewer than two is skipped -- it
is not a choice.  Each member is applied to a copy of the state and the
resulting position is scored with `weighted.evaluate`.  If the whole group
comes back bit-identical, the evaluator cannot distinguish those targets, and
whichever one the bot "chose" it chose by tiebreak.

BOUND, stated up front: this measures the ONE-PLY evaluator, which is exactly
what `QuiescentBot` decides on and exactly the leaf `PlanBot`'s beam scores.
It is NOT PlanBot's whole decision -- a deeper beam can in principle separate
two targets by what happens after them.  So read a blind group as "the leaf is
blind here", which means the beam is separating those targets on downstream
noise rather than on the attack being priced.  It does not by itself prove
PlanBot picks at random, and this tool makes no such claim.

Second bound: the driver is `WeightedBot`, not the ship policy.  The states
only need to be REALISTIC, not well-played, and a 1-ply driver buys ~4x the
positions per cpu-second.  A stronger driver would visit somewhat different
positions; it would not change what `evaluate` can see once it is in one.

WHAT IT MEASURED, 2026-08-02, 60 seeds x {3p, 4p}, DEFAULT_WEIGHTS:

    kind          groups  identical   blind%
    aggression      1804       1382     76.6%
    war             2743       2039     74.3%
    offer_pact      2859       1434     50.2%
    cancel_pact      322          0      0.0%

Stable to within a point from seed 39 onward, so this is a rate and not a
small-sample artefact.

WHY, confirmed by reading rather than inferred from the rate: no coordinate in
`weighted.features` reads WHO was attacked.  Section 5a of that file writes the
aggression and war payoffs off as unpriced on purpose, and a declared war does
not resolve on the turn it is declared, so a one-ply look at the resulting
position sees "a war exists" and nothing about the victim.

`cancel_pact` at 0.0% is the control, and it is the reason this table can be
trusted: that move has an immediate, target-specific effect on the board, the
same measurement sees it every single time, so a 74% reading elsewhere is a
real blindness and not an artefact of how the groups are built.

`offer_pact` at 50.2% is POST-FIX.  4e64780 priced the partner's side of an
offered pact after the census found 135 of 144 pact ties were the same card
offered to a different opponent.  That fix moved this number off 100% and did
not finish the job.

Run:  python3 -m tools.target_blindness [--seeds 60] [--players 3 4]
"""

import argparse
import collections
import random
import sys

from engine import actions, game
from engine.bots.fastcopy import copy_state
from engine.bots.weighted import (DEFAULT_WEIGHTS, WeightedBot, evaluate,
                                  rival_context)

#: move kind -> index of the opponent argument in the move tuple.  Everything
#: BEFORE that index is what must be held equal for a group to isolate the
#: target: the card being played, and nothing else.
TARGETED = {"aggression": 2, "war": 2, "offer_pact": 2, "cancel_pact": 1}


def measure(seeds=60, players=(3, 4), weights=None, move_cap=3000,
            progress_every=10, out=sys.stdout):
    """Return `(seen, blind, examples)`, three dicts keyed by move kind."""
    w = weights or DEFAULT_WEIGHTS
    seen = collections.Counter()
    blind = collections.Counter()
    examples = {}

    for seed in range(seeds):
        for n in players:
            st = game.new_game(n, seed=seed)
            bots = [WeightedBot(w, seed=seed * 7 + i) for i in range(n)]
            rng = random.Random(seed)
            steps = 0
            while not st.game_over and steps < move_cap:
                steps += 1
                idx = st.decider()
                moves = actions.legal_moves(st)
                if not moves:
                    break
                groups = collections.defaultdict(list)
                for mv in moves:
                    pos = TARGETED.get(mv[0])
                    if pos is not None and len(mv) > pos:
                        groups[(mv[0],) + tuple(mv[1:pos])].append(mv)
                for key, group in groups.items():
                    if len(group) < 2:
                        continue          # not a choice; not evidence
                    vals = []
                    for mv in group:
                        trial = copy_state(st)
                        try:
                            game.apply(trial, mv, random.Random(1))
                            vals.append(evaluate(trial, idx, w,
                                                 rival_context(trial, idx, w)))
                        except Exception:
                            pass          # an illegal-in-context target
                    if len(vals) < 2:
                        continue
                    seen[key[0]] += 1
                    if max(vals) - min(vals) == 0.0:
                        blind[key[0]] += 1
                        examples.setdefault(key[0], (key, group[:3], vals[:3]))
                try:
                    game.apply(st, bots[idx](st), rng)
                except Exception:
                    break
        if progress_every and seed % progress_every == progress_every - 1:
            report(seen, blind, out=out, tag=f"after seed {seed}")
    return seen, blind, examples


def report(seen, blind, examples=None, out=sys.stdout, tag="FINAL"):
    print(f"--- {tag}", file=out)
    print("kind          groups  identical   blind%", file=out)
    for k in sorted(seen):
        pct = 100.0 * blind[k] / seen[k]
        print(f"{k:<14}{seen[k]:>6}{blind[k]:>11}{pct:>9.1f}%", file=out)
    for k, (key, group, vals) in sorted((examples or {}).items()):
        print(f"\nexample {k}: {key} -> {group} scored {vals}", file=out)
    out.flush()


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--seeds", type=int, default=60)
    ap.add_argument("--players", type=int, nargs="+", default=[3, 4],
                    help="2 is accepted but measures nothing: with one "
                         "opponent every group has size 1 and is skipped.")
    args = ap.parse_args(argv)
    seen, blind, examples = measure(seeds=args.seeds,
                                    players=tuple(args.players))
    report(seen, blind, examples)
    return 0


if __name__ == "__main__":
    sys.exit(main())
