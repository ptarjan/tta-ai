#!/usr/bin/env python3
"""Two cheap base-rate probes that gate two items in docs/ARCHAEOLOGY.md.

Both questions are "does this thing ever vary?", which is much cheaper to
answer than the A/B each item asks for, and answering it first can make the
A/B unnecessary.

1. `has_unit` (ARCHAEOLOGY item 13, rank 1).  The unmerged `has-unit` branch
   adds a STEP feature ``1.0 if unit_workers else 0.0`` because §11.3 requires
   sacrificing a military unit, so `interact.start_auction` drops zero-unit
   players from a colony auction before they get a decision.  Its whole
   justification is `docs/AGGRESSION_FIX.md:56-60`, which measured the then
   2p and 4p champions at 0.00 and 0.07 units per player -- i.e. sitting on the
   wrong side of that cliff.  A binary feature that is 1.0 on ~every position
   carries almost no information and cannot be worth an A/B, so measure the
   base rate first.  Reported as the share of the bot's own decisions at which
   ``unit_workers == 0``.

2. The pact accept branch (ARCHAEOLOGY item 12e; `docs/PACTS_DIAGNOSIS.md`
   fix #3, "verify the accept branch isn't being systematically refused").
   `engine/interact._c_pact_offer` is the only place an offer is resolved, so
   counting its `opt` gives the accept/refuse split directly.  A systematic
   refusal shows up as accepts ~= 0 with offers > 0.

Neither probe changes engine behaviour: probe 1 wraps the bot callable and
probe 2 wraps `interact._c_pact_offer`, both read-only pass-throughs.

Usage (this box is CPU-constrained -- nice it and keep the counts small):

    nice -n 19 python3 tools/pact_unit_probe.py --players 4 --games 24 \
        --weights /tmp/probe/champion_4p.json
"""
import argparse
import collections
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from engine import cards as C          # noqa: E402
from engine import effects, game, interact  # noqa: E402
from engine.bots.book import BookBot    # noqa: E402
from engine.bots.quiescent import QuiescentBot  # noqa: E402
from engine.bots.weighted import DEFAULT_WEIGHTS, WeightedBot  # noqa: E402


class UnitProbe:
    """Pass-through bot wrapper: notes `unit_workers` at every decision."""

    def __init__(self, inner, counts):
        self.inner = inner
        self.counts = counts
        self.name = getattr(inner, "name", "probe")

    def _note(self, state):
        try:
            p = state.players[state.decider()]
        except Exception:
            p = state.me()
        u = effects.workers_on_types(p, C.UNIT_TYPES)
        self.counts["decisions"] += 1
        if u == 0:
            self.counts["zero_unit"] += 1

    def __call__(self, state):
        self._note(state)
        return self.inner(state)

    def choose(self, state, moves, rng=None):
        self._note(state)
        return self.inner.choose(state, moves, rng)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--players", type=int, default=4)
    ap.add_argument("--games", type=int, default=24)
    ap.add_argument("--seed0", type=int, default=9001)
    ap.add_argument("--weights", default=None,
                    help="champion JSON; default = DEFAULT_WEIGHTS")
    ap.add_argument("--levels", type=int, default=1,
                    help="quiescence levels; 0 = plain WeightedBot (1 ply)")
    args = ap.parse_args()

    if args.weights:
        blob = json.load(open(args.weights))
        w = blob.get("weights", blob)
        label = args.weights
    else:
        w = dict(DEFAULT_WEIGHTS)
        label = "DEFAULT_WEIGHTS"

    pact = collections.Counter()
    # NB: `interact._CHOICE` is built at import time and holds the function
    # OBJECT, so rebinding `interact._c_pact_offer` alone is a silent no-op.
    # Patch the dispatch table.
    real_offer = interact._CHOICE["pact_offer"]
    seat_box = [None]

    def probed_offer(state, p, opt, ctx, rng):
        # `p` is the player being OFFERED the pact, i.e. the one that just
        # made the accept/refuse decision.  Split by whether that was the
        # bot under test or one of the BookBot defenders.
        who = "probe" if p.idx == seat_box[0] else "book"
        pact[f"{who}:{opt}"] += 1
        return real_offer(state, p, opt, ctx, rng)

    interact._CHOICE["pact_offer"] = probed_offer

    counts = collections.Counter()
    errors = 0
    for g in range(args.games):
        seed = args.seed0 + g
        seat = g % args.players
        seat_box[0] = seat
        def mk(i):
            if i != seat:
                return BookBot(seed=seed * 97 + i)
            inner = (QuiescentBot(weights=w, levels=args.levels,
                                  seed=seed * 97 + i)
                     if args.levels else
                     WeightedBot(weights=w, seed=seed * 97 + i))
            return UnitProbe(inner, counts)
        bots = [mk(i) for i in range(args.players)]
        try:
            game.play_game(bots, args.players, seed=seed)
        except Exception as e:            # engine bug: report, keep going
            errors += 1
            print(f"  game {g} seed {seed} FAILED: {e!r}", file=sys.stderr)

    interact._CHOICE["pact_offer"] = real_offer

    d = counts["decisions"] or 1
    z = counts["zero_unit"]
    # Wald SE on the share; n is decisions, which is large.
    share = z / d
    se = (share * (1 - share) / d) ** 0.5
    print(f"vector      : {label}  (levels={args.levels})")
    print(f"players     : {args.players}   games: {args.games}"
          f"   engine errors: {errors}")
    print(f"has_unit==0 : {z}/{d} decisions = {100*share:.2f}% "
          f"+- {100*se:.2f}%   (has_unit==1 on {100*(1-share):.2f}%)")
    for who in ("probe", "book"):
        acc = pact.get(f"{who}:accept", 0)
        ref = sum(v for k, v in pact.items()
                  if k.startswith(f"{who}:") and not k.endswith(":accept"))
        n = acc + ref
        if not n:
            print(f"pact  {who:<6}: 0 offers resolved -- the accept branch was "
                  f"never reached, so it cannot be scored here")
            continue
        p_acc = acc / n
        se_a = (p_acc * (1 - p_acc) / n) ** 0.5
        print(f"pact  {who:<6}: {n} offers resolved; accept {acc} "
              f"({100*p_acc:.1f}% +- {100*se_a:.1f}%), refuse {ref}")
    print(f"            raw={dict(pact)}")


if __name__ == "__main__":
    main()
