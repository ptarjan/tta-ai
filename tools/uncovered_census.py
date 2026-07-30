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

Plus the two counters that size the end-of-turn discard defect: how many
military cards the hand-limit rule throws away per game, and how many of those
were the single best DEFENCE card in the hand at the time.

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
from engine import economy, game                           # noqa: E402
from experiments.arena import (                            # noqa: E402
    load_spec, make_bot, refuse_if_degenerate_champion)

MY_TYPES = ("special-tech", "farm", "mine", "lab", "temple", "library",
            "arena", "theater", "bonus")


class _Watch:
    """Wraps a bot; counts offers and chosen moves per card name."""

    def __init__(self, bot, offers, takes, plays, builds, upgr_from, upgr_to):
        self.bot = bot
        self.offers, self.takes, self.plays = offers, takes, plays
        self.builds, self.upgr_from, self.upgr_to = builds, upgr_from, upgr_to

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

    # --- the discard probe (docs/UNCOVERED_TYPES.md D1).  Read-only: it
    # inspects the hand and the limit exactly as `economy.end_of_turn` is about
    # to, records what FIFO is going to destroy, and then calls the real thing
    # unchanged.  It does not alter play, so the counts below belong to the
    # same games the take rates do.
    stats = {"discarded": 0, "best_defence_discarded": 0, "bonus_discarded": 0}

    def _defence_of(name):
        card = db.by_name.get(name)
        eff = (card.get("effects") or {}) if card else {}
        b = eff.get("defenseBonus")
        return b if isinstance(b, int) else 1

    orig_eot = economy.end_of_turn

    def patched_eot(state, p, rng):
        s = economy.effects.state_stats(state, p)
        limit = s.military_actions + s.military_hand_limit
        before = list(p.hand_military)
        excess = max(0, len(before) - limit)
        if excess:
            best = max(before, key=_defence_of)
            doomed = before[:excess]
            stats["discarded"] += excess
            for n in doomed:
                if db.type_of(n) == "bonus":
                    stats["bonus_discarded"] += 1
            if best in doomed and _defence_of(best) > 1:
                stats["best_defence_discarded"] += 1
        return orig_eot(state, p, rng)

    economy.end_of_turn = patched_eot
    try:
        for g in range(games):
            bots = [_Watch(make_bot(spec, 1000 + i), offers, takes, plays,
                           builds, upgr_from, upgr_to) for i in range(players)]
            st = game.play_game(bots, num_players=players,
                                seed=(seed0 + g) * 7919 + 17, move_cap=20000)
            for p in st.players:
                for n, t in p.techs.items():
                    if n in mine:
                        in_play_end[n] += 1
                        workers_end[n] += t.workers
    finally:
        economy.end_of_turn = orig_eot

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
    print(f"# hand-limit discards: {d['discarded']} "
          f"({d['discarded']/g:.2f}/game), of which bonus cards "
          f"{d['bonus_discarded']}; games' best defence card pitched "
          f"{d['best_defence_discarded']} times")
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
