"""State the denominator for "the bot takes zero cards in Age IV".

docs/OPEN_ITEMS.md item 2.17 reports 0.00 civil cards taken during the Age IV
phase at every player count against a human 1.59 / 1.57 / 1.78.  A rate of zero
has two causes that look identical in a census table -- NEVER CHOSEN and NEVER
OFFERED -- and that row has already been closed once on the wrong one of them
(`26b5d74`, reopened by `1b63421`): there are no Age IV *cards*, but
`engine/game.py:_advance_age` empties the DECKS and not the ROW, so leftover
Age III cards stay on the row and taking one in the Age IV phase is legal.

This measures the denominator directly.  At every Age IV decision it records
whether the row is non-empty at all, how many `("take", i)` moves
`actions.legal_moves` actually emits, and how many the bot takes.

    python3.13 tools/age_iv_row.py --players 2 --games 20
    python3.13 tools/age_iv_row.py --players 2 --games 20 \
        --bot experiments/league_state/champion_2p.json
    python3.13 tools/age_iv_row.py --players 2 --games 20 \
        --bot plan:experiments/league_state/champion_2p.json,width=2,det=1

`--bot` is an `experiments/arena.py` spec, so the three arms above separate the
three things the census row conflates: the ENGINE (is a take even offered), the
EVALUATOR (does `DEFAULT_WEIGHTS` want one), and the SEARCH (does the beam the
census actually ran ever emit one).  A zero that survives only the third is a
`PlanBot` finding and wants a completely different fix from a pricing one.
"""
from __future__ import annotations

import argparse
import collections

import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine import actions as A, game as G                       # noqa: E402
from experiments import arena                                    # noqa: E402


class _Tap:
    """Wraps one seat.  Counts only REAL states -- the beam copies the state
    and calls the same functions on the copy, and counting those would measure
    the search rather than the game (docs/SYSTEM_COVERAGE.md, method)."""

    def __init__(self, bot, tot):
        self.bot = bot
        self.tot = tot

    def __call__(self, state):
        tot = self.tot
        iv = state.age_civil == "IV" and not state.pending
        if iv:
            tot["decisions"] += 1
            row = [c for c in state.card_row if c is not None]
            tot["row_cards"] += len(row)
            tot["row_nonempty"] += 1 if row else 0
            takes = [m for m in A.legal_moves(state) if m[0] == "take"]
            tot["takes_legal"] += len(takes)
            tot["decisions_with_a_legal_take"] += 1 if takes else 0
        mv = self.bot(state)
        if iv and mv and mv[0] == "take":
            tot["takes_made"] += 1
        return mv


def run(players, games, seed0=0, spec="default"):
    loaded = arena.load_spec(spec)
    tot = collections.Counter()
    for g in range(games):
        bots = [_Tap(arena.make_bot(loaded, seed0 + g * 17 + i), tot)
                for i in range(players)]
        G.play_game(bots, players, seed=seed0 + g)
        tot["games"] += 1
    return tot


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--players", type=int, default=2, choices=(2, 3, 4))
    ap.add_argument("--games", type=int, default=20)
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--bot", default="default",
                    help="an experiments/arena.py spec")
    args = ap.parse_args(argv)

    tot = run(args.players, args.games, args.seed, args.bot)
    seat_games = tot["games"] * args.players
    print(f"{args.players}p, {tot['games']} games, {seat_games} seat-games, "
          f"bot={args.bot}")
    d = tot["decisions"] or 1
    print(f"  Age IV decisions                 {tot['decisions']}")
    print(f"  ...with a non-empty row          {tot['row_nonempty']} "
          f"({tot['row_nonempty'] / d:.1%})")
    print(f"  ...with >=1 LEGAL take           "
          f"{tot['decisions_with_a_legal_take']} "
          f"({tot['decisions_with_a_legal_take'] / d:.1%})")
    print(f"  mean cards on the row            {tot['row_cards'] / d:.2f}")
    print(f"  legal takes offered (total)      {tot['takes_legal']}")
    print(f"  takes MADE                       {tot['takes_made']}  "
          f"= {tot['takes_made'] / seat_games:.2f} per seat-game")
    return tot


if __name__ == "__main__":
    main()
