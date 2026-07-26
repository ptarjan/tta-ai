"""Round-1 opening broken out BY SEAT INDEX, not averaged across seats.

Why this exists
---------------
`experiments/behaviour.py` rotates the champion through every seat
(`tasks = [(seed, g % players) ...]`) and then aggregates *all* of those games
into one `champion_behaviour` block.  Round 1 is the one round where seats are
not symmetric: §1.9 gives seat 0 one civil action and seat 3 four
(`engine/game.py`: `p.civil_actions = i + 1`).  So a single "the champion opens
with X" number silently averages a player who can take one card with a player
who can take four -- and the seat mix differs by player count (2p averages
1+2 CA, 4p averages 1+2+3+4 CA).  Any 2p-vs-4p opening comparison built that way
is comparing different seat mixes.

This script logs *every* seat of *every* game and reports round 1 per seat:

  * how many cards were taken (should equal that seat's civil actions)
  * the type of the FIRST card taken  (the actual "opening")
  * whether a wonder was taken at all in round 1
  * which row index / cost band it came from

It also supports cross-play: run the 4p weight vector at 2 players and the 2p
vector at 4 players.  If wonder-first follows the *weights* rather than the
*player count*, the opening is a property of that champion, not a strategic
response to 4p conditions.

Usage:
    python3 analysis/opening_by_seat.py --players 4 --games 120 \\
        --champion /tmp/ch4.json --json /tmp/out.json

Always copy the champion out of experiments/ first; a live hill-climb rewrites
it in place.
"""
from __future__ import annotations

import argparse
import collections
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from experiments.arena import load_spec, make_bot  # noqa: E402


def card_type(name):
    """Cards in the DB are plain dicts -- getattr() on them silently fails."""
    from engine import cards as C
    c = C.db().get(name)
    if not isinstance(c, dict):
        return "?"
    return c.get("type", "?")


class SeatLogger:
    """Callable bot wrapper. play_game calls bots as bot(state) -> move."""

    def __init__(self, inner, idx, max_round=2):
        self.inner = inner
        self.idx = idx
        self.max_round = max_round
        self.log = []          # [round, kind, name, type, row_idx]
        self.ca_round1 = None

    def __call__(self, state):
        if state.round == 1 and self.ca_round1 is None:
            try:
                self.ca_round1 = state.players[self.idx].civil_actions
            except Exception:
                pass
        mv = self.inner(state) if callable(self.inner) else \
            self.inner.choose(state, None, None)
        try:
            self._note(state, mv)
        except Exception as e:  # never let logging break a game
            self.log.append([-1, "ERR", repr(e), "?", -1])
        return mv

    def _note(self, state, mv):
        if state.round > self.max_round:
            return
        kind = mv[0]
        name, typ, row = "", "", -1
        if kind == "take":
            row = mv[1]
            try:
                name = state.card_row[row]
            except Exception:
                name = "?"
            typ = card_type(name)
        elif kind in ("build", "upgrade", "develop"):
            name = mv[-1]
            typ = card_type(name)
        elif kind in ("play_leader", "play_action", "revolution"):
            name = mv[1] if len(mv) > 1 else ""
            typ = card_type(name) if name else ""
        elif kind == "wonder_step":
            p = state.players[self.idx]
            name = p.wonder.name if p.wonder is not None else "?"
            typ = "wonder"
        self.log.append([state.round, kind, name, typ, row])


def run(players, games, champ_path, seed0=51000, opp_path=None):
    """Every seat plays `champ_path` unless --opponent is given (mirror by
    default, which is exactly how the hill climb evaluates)."""
    from engine import game
    champ = load_spec(champ_path)
    opp = load_spec(opp_path) if opp_path else champ
    rows = []
    for g in range(games):
        seed = seed0 + g
        champ_seat = g % players
        logs = []
        bots = []
        for i in range(players):
            spec = champ if (opp_path is None or i == champ_seat) else opp
            lg = SeatLogger(make_bot(spec, seed * 97 + i * 13 + 1), i)
            logs.append(lg)
            bots.append(lg)
        try:
            game.play_game(bots, players, seed=seed, move_cap=100000)
        except Exception as e:
            print("game error", seed, repr(e), file=sys.stderr)
            continue
        for i, lg in enumerate(logs):
            if opp_path is not None and i != champ_seat:
                continue
            takes = [e for e in lg.log if e[0] == 1 and e[1] == "take"]
            rows.append({
                "seed": seed, "seat": i, "players": players,
                "ca": lg.ca_round1,
                "n_takes": len(takes),
                "first_type": takes[0][3] if takes else None,
                "first_name": takes[0][2] if takes else None,
                "first_row": takes[0][4] if takes else -1,
                "types": [t[3] for t in takes],
                "names": [t[2] for t in takes],
            })
    return rows


def by_seat_table(rows, players):
    """Return {seat: {...}} plus an 'all' row (the misleading aggregate)."""
    out = {}
    groups = collections.defaultdict(list)
    for r in rows:
        groups[r["seat"]].append(r)
        groups["all"].append(r)
    for seat in sorted(groups, key=lambda s: (s == "all", s)):
        g = groups[seat]
        n = len(g)
        firsts = collections.Counter(r["first_type"] for r in g)
        anyw = sum(1 for r in g if "wonder" in r["types"])
        out[str(seat)] = {
            "games": n,
            "civil_actions": (g[0]["ca"] if seat != "all" else None),
            "mean_cards_taken": round(sum(r["n_takes"] for r in g) / max(1, n), 2),
            "first_take_type": {k: round(v / n, 3) for k, v in firsts.most_common()},
            "any_wonder_round1": round(anyw / max(1, n), 3),
            "top_first_cards": collections.Counter(
                r["first_name"] for r in g).most_common(4),
        }
    return out


def print_table(tbl, label):
    print(f"\n===== {label} =====")
    hdr = f"{'seat':>5} {'CA':>3} {'games':>6} {'cards':>6} {'wonder1st':>10} " \
          f"{'action1st':>10} {'leader1st':>10} {'anyWonderR1':>12}"
    print(hdr)
    for seat, d in tbl.items():
        f = d["first_take_type"]
        print(f"{seat:>5} {str(d['civil_actions'] or '-'):>3} {d['games']:>6} "
              f"{d['mean_cards_taken']:>6} {f.get('wonder', 0):>10.0%} "
              f"{f.get('action', 0):>10.0%} {f.get('leader', 0):>10.0%} "
              f"{d['any_wonder_round1']:>12.0%}")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--players", type=int, default=2)
    ap.add_argument("--games", type=int, default=120)
    ap.add_argument("--champion", required=True)
    ap.add_argument("--opponent", default=None,
                    help="if set, only the rotating champion seat is logged "
                         "and everyone else plays this spec")
    ap.add_argument("--seed0", type=int, default=51000)
    ap.add_argument("--label", default=None)
    ap.add_argument("--json", default=None)
    a = ap.parse_args()
    rows = run(a.players, a.games, a.champion, a.seed0, a.opponent)
    tbl = by_seat_table(rows, a.players)
    label = a.label or f"{a.players}p  champion={os.path.basename(a.champion)}"
    print_table(tbl, label)
    if a.json:
        with open(a.json, "w") as fh:
            json.dump({"label": label, "players": a.players,
                       "champion": a.champion, "opponent": a.opponent,
                       "games": a.games, "by_seat": tbl, "rows": rows}, fh)


if __name__ == "__main__":
    main()
