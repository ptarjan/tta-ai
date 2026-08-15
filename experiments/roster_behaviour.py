"""What each roster bot *does*, and what it costs to run.

WHY THIS EXISTS
---------------
``experiments/roster_match.py`` answers "who beats whom".  That is not enough
to choose a training pool.  Two bots with the same win rate are different
training partners if one declares wars and the other never fights: an opponent
pool that never attacks cannot teach a learner to defend.  So this script
records, for every entrant, the *rate* at which it plays each of the move
kinds a 1-ply search is known to undervalue (see ``docs/COMBAT_AUDIT.md``):
pact offers, wars, aggressions and colony bids.

It also times the bots.  The pool is meant for a long training run, where a
bot that is 10x slower than the rest sets the cost of every generation.  A
strong-but-ruinous bot is a real trade-off and has to be visible in the table
rather than discovered halfway through a week-long run.

MIRROR TABLES
-------------
Every seat runs the same bot, exactly as ``tools/behaviour_counts.py`` does.
The counts are therefore "what this bot does when everyone plays it", not
"what it does against some particular field" -- the latter would report the
field's aggression as much as the bot's.  Counts are per game and summed over
all seats, so at 4p a value of 4.0 means one such move per player per game.

COST IS COMPARATIVE, NOT ABSOLUTE
---------------------------------
Jobs run under a fixed worker pool, so every bot is timed under the same
contention.  The absolute seconds are therefore not a single-core benchmark;
the *ratios* between bots are the number to trust, which is what the pool
choice actually turns on.  ``ms_per_move`` normalises out the fact that
different bots end games in different numbers of moves.

    python3 -m experiments.roster_behaviour --games 40 --workers 4
"""
from __future__ import annotations

import argparse
import json
import multiprocessing as mp
import os
import sys
import time

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)),
                                ".."))

from experiments import roster_match  # noqa: E402  (installs the roster patch)
from experiments.arena import make_bot  # noqa: E402  (already patched)

KINDS = ("offer_pact", "war", "aggression", "bid", "cancel_pact")


class _Counting:
    """Wraps a bot and tallies the move kinds it chooses."""

    def __init__(self, bot):
        self.bot = bot
        self.counts = {}
        self.moves = 0

    def _note(self, mv):
        self.moves += 1
        if mv:
            k = mv[0]
            if k in KINDS:
                self.counts[k] = self.counts.get(k, 0) + 1
        return mv

    def choose(self, state, moves, rng=None):
        return self._note(self.bot.choose(state, moves, rng))

    def __call__(self, state):
        return self._note(self.bot(state))


def run_one(job):
    label, players, games, seed0 = job
    from engine import game

    spec = roster_match.load_spec(label, players)
    tot = {k: 0 for k in KINDS}
    colonies = pacts = moves = 0
    t0 = time.time()
    for g in range(games):
        bots = [_Counting(make_bot(spec, 1000 + i)) for i in range(players)]
        st = game.play_game(bots, num_players=players,
                            seed=(seed0 + g) * 7919 + 17, move_cap=20000)
        for b in bots:
            moves += b.moves
            for k, v in b.counts.items():
                tot[k] += v
        for p in st.players:
            colonies += len(getattr(p, "colonies", ()) or ())
            pacts += len(getattr(p, "pacts", ()) or ())
    secs = time.time() - t0
    n = float(games)
    out = {"label": label, "players": players, "games": games,
           "secs": round(secs, 1),
           "secs_per_game": round(secs / n, 3),
           "moves_per_game": round(moves / n, 1),
           "ms_per_move": round(1000.0 * secs / max(1, moves), 3)}
    out.update({k: round(tot[k] / n, 3) for k in KINDS})
    out["colonies_held_end"] = round(colonies / n, 3)
    out["pacts_live_end"] = round(pacts / n, 3)
    print(json.dumps(out), flush=True)
    return out


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--games", type=int, default=40)
    ap.add_argument("--seed", type=int, default=55000)
    ap.add_argument("--workers", type=int, default=4)
    ap.add_argument("--players", type=int, default=0, choices=(0, 2, 3, 4))
    ap.add_argument("--entrants", default=",".join(roster_match.DEFAULT_ENTRANTS))
    ap.add_argument("--out", default="experiments/roster_behaviour.jsonl")
    ap.add_argument("--resume", action="store_true")
    a = ap.parse_args(argv)

    entrants = [e.strip() for e in a.entrants.split(",") if e.strip()]
    counts = (2, 3, 4) if a.players == 0 else (a.players,)

    done = set()
    if a.resume and a.out and os.path.exists(a.out):
        for line in open(a.out):
            r = json.loads(line)
            done.add((r["players"], r["label"]))

    jobs = [(e, n, a.games, a.seed) for n in counts for e in entrants
            if (n, e) not in done]
    if not jobs:
        print("nothing to do")
        return 0

    ctx = mp.get_context("fork")
    with ctx.Pool(a.workers) as pool:
        rows = pool.map(run_one, jobs, chunksize=1)

    if a.out:
        with open(a.out, "a") as fh:
            for r in rows:
                fh.write(json.dumps(r) + "\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
