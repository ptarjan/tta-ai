"""Per-game counts of the moves the 1-ply search is blind to.

`experiments/behaviour.py` reports wars and aggressions but has no counter for
pact offers or colony bids, which are two of the four move classes
docs/PACTS_DIAGNOSIS.md identifies as strictly dominated at 1 ply.  This
counts all four, plus what they actually produced (pacts signed, colonies
taken), by wrapping every bot at the table and tallying the moves it picks.

Mirror tables: every seat runs the same bot, so the counts are "what this
search does when everyone uses it", not "what it does against a 1-ply field".
That is the right comparison for the emergence question -- a bot that attacks
only because its opponents cannot retaliate has not learned anything.

    nice -n 15 python3 tools/behaviour_counts.py --players 4 --games 40 \
        --spec quiesce:experiments/league_state/champion_4p.json,levels=2

`--spec` is required (no default), and is refused (see
experiments.arena.refuse_if_degenerate_champion) if it points at
experiments/champion_4p.json -- the pre-horizon-fix vector
docs/TRAINING_RUN.md says never to warm-start from -- by path or by content,
so a copy of that file is refused too.
"""
from __future__ import annotations

import argparse
import json
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine import game                                    # noqa: E402
from experiments.arena import (                            # noqa: E402
    load_spec, make_bot, refuse_if_degenerate_champion)

# move kinds we care about, in report order.  `play_action` is the fourth of
# the four classes docs/DEEPER_SEARCH.md section 1 lists as strictly dominated
# at 1 ply and was missing from the original tuple; `take` is here because
# docs/WASTED_ACTIONS.md section 6 shows it is the move the evaluation is least
# able to price, so it is the control for "did the bot just get busier".
KINDS = ("offer_pact", "war", "aggression", "bid", "play_action",
         "cancel_pact", "prepare_event", "take")

#: move kinds that spend a CIVIL action -- same table as analysis/wasted_actions
_CIVIL_KINDS = {"take", "pop", "wonder_step", "play_leader", "develop",
                "revolution", "play_action"}
_MAYBE_CIVIL = {"build", "upgrade", "destroy"}

#: the action cards whose effect ORDERS a free civil action, i.e. the ones that
#: leave a `free_civil` decision pending and are therefore invisible to a 1-ply
#: search.  The other 15 action cards resolve entirely inside `apply`.
def _ordered_action_cards():
    from engine import actions as A
    return frozenset(c["name"] for c in A._DB.cards
                     if (c.get("effects") or {}).get("freeCivilAction"))


ORDERED_ACTION_CARDS = _ordered_action_cards()


def _costs_civil_action(move):
    kind = move[0]
    if kind in _CIVIL_KINDS:
        return True
    if kind in _MAYBE_CIVIL:
        from engine import actions
        return not actions.is_unit(move[1])
    return False


class _Counting:
    """Wraps a bot and tallies the move kinds it chooses.

    Also records the two numbers docs/WASTED_ACTIONS.md section 6 turns on:
    how often a turn is ended with civil actions still in hand, and how many
    die that way.  Those are read off the state at the moment ``end_turn`` is
    CHOSEN, i.e. before it is applied, so ``p.civil_actions`` is still the
    count remaining for the turn being ended.
    """

    def __init__(self, bot):
        self.bot = bot
        self.counts = {}
        self.turns = 0
        self.turns_with_ca_left = 0
        self.ca_wasted = 0
        self.civil_spent = 0

    def _note(self, state, mv):
        if mv:
            k = mv[0]
            if k in KINDS:
                self.counts[k] = self.counts.get(k, 0) + 1
            if k == "play_action":
                # only the 18 of 33 action cards that ORDER a free action
                # enqueue a pending decision; the rest gain immediately and
                # were never blind to the 1-ply search in the first place.
                sub = ("action_ordered" if mv[1] in ORDERED_ACTION_CARDS
                       else "action_immediate")
                self.counts[sub] = self.counts.get(sub, 0) + 1
            if _costs_civil_action(mv):
                self.civil_spent += 1
            elif k == "end_turn":
                self.turns += 1
                try:
                    left = state.players[state.decider()].civil_actions
                except Exception:
                    left = 0
                if left > 0:
                    self.turns_with_ca_left += 1
                    self.ca_wasted += left
        return mv

    def choose(self, state, moves, rng=None):
        return self._note(state, self.bot.choose(state, moves, rng))

    def __call__(self, state):
        return self._note(state, self.bot(state))


def run(spec, players, games, seed0):
    tot = {k: 0 for k in KINDS + ("action_ordered", "action_immediate")}
    colonies = pacts = 0
    turns = ca_left_turns = ca_wasted = civil_spent = 0
    for g in range(games):
        bots = [_Counting(make_bot(spec, 1000 + i)) for i in range(players)]
        st = game.play_game(bots, num_players=players,
                            seed=(seed0 + g) * 7919 + 17, move_cap=20000)
        for b in bots:
            for k, v in b.counts.items():
                tot[k] += v
            turns += b.turns
            ca_left_turns += b.turns_with_ca_left
            ca_wasted += b.ca_wasted
            civil_spent += b.civil_spent
        for p in st.players:
            colonies += len(getattr(p, "colonies", ()) or ())
            pacts += len(getattr(p, "pacts", ()) or ())
    n = float(games)
    out = {k: round(tot[k] / n, 3)
           for k in KINDS + ("action_ordered", "action_immediate")}
    out["colonies_held_end"] = round(colonies / n, 3)
    out["pacts_live_end"] = round(pacts / n, 3)
    t = float(max(turns, 1))
    out["turns_per_game"] = round(turns / n, 2)
    out["ca_unspent_turn_rate"] = round(ca_left_turns / t, 4)
    out["ca_wasted_per_turn"] = round(ca_wasted / t, 3)
    out["civil_spent_per_turn"] = round(civil_spent / t, 3)
    return out


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--players", type=int, default=4, choices=(2, 3, 4))
    ap.add_argument("--games", type=int, default=40)
    ap.add_argument("--seed", type=int, default=55000)
    ap.add_argument("--spec", required=True)
    ap.add_argument("--label", default="")
    a = ap.parse_args(argv)

    refuse_if_degenerate_champion(a.spec, "behaviour_counts.py")
    res = run(load_spec(a.spec), a.players, a.games, a.seed)
    print(json.dumps({"label": a.label or a.spec, "players": a.players,
                      "games": a.games, "per_game": res}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
