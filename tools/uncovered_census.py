"""Take/play rates for the card types nobody had measured: the 12 special
technologies, the 24 production buildings and the 3 military bonus cards.

Every other census in this repo answers "is this card's data mapped".  That is
not the same question as "does the bot ever take it", and the difference is the
whole point of docs/UNCOVERED_TYPES.md: a card can be priced perfectly and
still never be reached, and a card can be reached constantly and be priced
wrong by a little, which costs more.

Three numbers per card:

* ``offers``  -- decision points at which ``("take", name)`` was a LEGAL move,
  i.e. the card was in the row and affordable.  This is the denominator that
  makes a take rate mean anything: Engineering being taken twice per game is
  damning only if it was on offer forty times.
* ``takes``   -- times it was actually taken.
* ``plays``   -- times it was developed (special techs and production
  technologies) -- the move that turns a card in hand into a card in play.
* ``builds``  -- times a worker was placed on it (``build``), plus
  ``upgrades`` in/out for the production buildings, whose real value is a
  DELTA over the level below and not the absolute number on the card.

Plus the counters that sized the end-of-turn discard defect (D1): how much
hand-limit pressure there is, and -- as a COUNTERFACTUAL now that `1c08790` has
made the discard a real choice -- how often the old `pop(0)` would have
destroyed the best defence card in the hand.  Kept so the fix stays
attributable instead of becoming folklore.

    nice -n 19 python3 tools/uncovered_census.py --players 2 --games 40 \
        --spec analysis/frozen/champion_2p.json
"""
from __future__ import annotations

import argparse
import collections
import json
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine import cards as C                              # noqa: E402
from engine import game                                    # noqa: E402
from experiments.arena import (                            # noqa: E402
    load_spec, make_bot, refuse_if_degenerate_champion)

MY_TYPES = ("special-tech", "farm", "mine", "lab", "temple", "library",
            "arena", "theater", "bonus")


def _defence_of(name):
    """A military card's DEFENCE contribution (RULES_SPEC 125): the printed
    `defenseBonus` for a bonus card, +1 for any other card discarded face
    down."""
    card = C.db().by_name.get(name)
    eff = (card.get("effects") or {}) if card else {}
    b = eff.get("defenseBonus")
    return b if isinstance(b, int) else 1


class _Watch:
    """Wraps a bot; counts offers and chosen moves per card name.

    EVERYTHING here is counted at the moment a move is CHOSEN by the real bot,
    never inside the engine.  That is not a style preference, it is the
    correctness condition: the searching bots apply and roll back thousands of
    speculative moves per decision, so an instrument that hooks an engine
    function counts the search as well as the game.  An earlier version of the
    discard probe below wrapped `economy.end_of_turn` and reported 129 discards
    per 2p game; the true figure is ~30.7 (independently measured by the lane
    that owns the discard bug), and the whole of the difference was rollouts.
    """

    def __init__(self, bot, offers, takes, plays, builds, upgr_from, upgr_to,
                 stats=None, db=None):
        self.bot = bot
        self.offers, self.takes, self.plays = offers, takes, plays
        self.builds, self.upgr_from, self.upgr_to = builds, upgr_from, upgr_to
        self.stats, self.db = stats, db

    def _note_discard(self, state):
        """Hand-limit pressure at a REAL end_turn, and what FIFO would cost.

        Read off the state at the moment ``end_turn`` is chosen -- before it is
        applied -- so the hand and the limit are the ones §6.6 step 1 will see.

        `fifo_would_lose_best` is now a COUNTERFACTUAL, not a measurement of
        what happens.  `engine/economy.py` used to `pop(0)` and make no
        decision at all, which is the defect this counter was built to size
        (docs/UNCOVERED_TYPES.md D1, docs/MILITARY_DISCARD.md).  The player now
        chooses, so this number no longer describes play -- it describes the
        size of the hole that was there, and it is kept so the fix stays
        attributable rather than becoming folklore.
        """
        from engine import effects
        if self.stats is None:
            return
        p = state.players[state.decider()]
        s = effects.state_stats(state, p)
        limit = s.military_actions + s.military_hand_limit
        hand = list(p.hand_military)
        excess = len(hand) - limit
        if excess <= 0:
            return
        self.stats["turns_over_limit"] += 1
        self.stats["over_limit_by"] += excess
        doomed = hand[:excess]              # what the OLD pop(0) would have hit
        for n in doomed:
            if self.db.type_of(n) == "bonus":
                self.stats["fifo_would_lose_bonus"] += 1
        best = max(hand, key=_defence_of)
        if best in doomed and _defence_of(best) > 1:
            self.stats["fifo_would_lose_best"] += 1

    def _note(self, state, moves, mv):
        # `("take", idx)` is a ROW SLOT, not a card name (engine/actions.py:376)
        row = state.card_row
        seen = set()
        for m in moves:
            if m[0] != "take":
                continue
            n = row[m[1]] if 0 <= m[1] < len(row) else None
            if n and n not in seen:
                seen.add(n)
                self.offers[n] += 1
        if mv:
            k = mv[0]
            if k == "take":
                n = row[mv[1]] if 0 <= mv[1] < len(row) else None
                if n:
                    self.takes[n] += 1
            elif k == "develop":
                self.plays[mv[1]] += 1
            elif k == "build":
                self.builds[mv[1]] += 1
            elif k == "upgrade":
                self.upgr_from[mv[1]] += 1
                self.upgr_to[mv[2]] += 1
            elif k == "end_turn":
                self._note_discard(state)
        return mv

    def __call__(self, state):
        from engine import actions
        moves = actions.legal_moves(state)
        return self._note(state, moves, self.bot(state))


def run(spec, players, games, seed0):
    db = C.db()
    mine = {c["name"]: c["type"] for c in db.cards if c.get("type") in MY_TYPES}
    offers = collections.Counter()
    takes = collections.Counter()
    plays = collections.Counter()
    builds = collections.Counter()
    upgr_from = collections.Counter()
    upgr_to = collections.Counter()
    workers_end = collections.Counter()
    in_play_end = collections.Counter()

    # --- the discard probe (docs/UNCOVERED_TYPES.md D1), counted at the real
    # `end_turn` decision rather than inside the engine.  See `_Watch`.
    stats = {"turns_over_limit": 0, "over_limit_by": 0,
             "fifo_would_lose_bonus": 0, "fifo_would_lose_best": 0}

    for g in range(games):
        bots = [_Watch(make_bot(spec, 1000 + i), offers, takes, plays,
                       builds, upgr_from, upgr_to, stats, db)
                for i in range(players)]
        st = game.play_game(bots, num_players=players,
                            seed=(seed0 + g) * 7919 + 17, move_cap=20000)
        for p in st.players:
            for n, t in p.techs.items():
                if n in mine:
                    in_play_end[n] += 1
                    workers_end[n] += t.workers

    rows = []
    for name, typ in sorted(mine.items(), key=lambda kv: (kv[1], kv[0])):
        rows.append({
            "name": name, "type": typ,
            "offers": offers[name], "takes": takes[name],
            "take_rate": (takes[name] / offers[name]) if offers[name] else None,
            "plays": plays[name], "builds": builds[name],
            "upgraded_away": upgr_from[name], "upgraded_into": upgr_to[name],
            "in_play_end": in_play_end[name],
            "workers_end": workers_end[name],
        })
    return {"spec": str(spec), "players": players, "games": games,
            "discard": stats, "cards": rows}


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--players", type=int, default=2)
    ap.add_argument("--games", type=int, default=20)
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--spec", required=True)
    ap.add_argument("--json", default=None)
    a = ap.parse_args(argv)
    refuse_if_degenerate_champion(a.spec, "uncovered_census")
    spec = load_spec(a.spec)
    out = run(spec, a.players, a.games, a.seed)
    if a.json:
        with open(a.json, "w") as fh:
            json.dump(out, fh, indent=1)
    g = float(a.games)
    d = out["discard"]
    print(f"# {a.players}p x{a.games} games  spec={a.spec}")
    over = d["turns_over_limit"] or 1
    print(f"# hand-limit pressure: {d['turns_over_limit']} player-turns over "
          f"the limit ({d['turns_over_limit']/g:.1f}/game), "
          f"{d['over_limit_by']} cards above it ({d['over_limit_by']/g:.1f}"
          f"/game).  COUNTERFACTUAL, the old pop(0) would have destroyed the "
          f"best defence card on {d['fifo_would_lose_best']} of those turns "
          f"({100.0*d['fifo_would_lose_best']/over:.1f}%) and "
          f"{d['fifo_would_lose_bonus']} bonus cards.")
    print(f"{'card':26s} {'type':13s} {'offers':>7s} {'takes':>6s} "
          f"{'rate':>6s} {'dev':>5s} {'built':>6s} {'upIn':>5s} "
          f"{'upOut':>6s} {'endW':>5s}")
    for r in out["cards"]:
        rate = "-" if r["take_rate"] is None else f"{r['take_rate']:.3f}"
        print(f"{r['name'][:26]:26s} {r['type']:13s} {r['offers']:7d} "
              f"{r['takes']:6d} {rate:>6s} {r['plays']:5d} {r['builds']:6d} "
              f"{r['upgraded_into']:5d} {r['upgraded_away']:6d} "
              f"{r['workers_end']:5d}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
