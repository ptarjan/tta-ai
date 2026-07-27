"""Legal-vs-taken census: which mechanics does a bot never use?

Every decision in a self-play game is labelled with the *mechanics* that
were legal at that decision and the one mechanic that was actually chosen.
A mechanic that is legal thousands of times and taken zero times is either
mis-implemented (the bots correctly avoid something that does not work) or
invisible to the evaluator (no feature can express its value) -- see
docs/COVERAGE_AUDIT.md, which this tool produced.

The point of measuring rather than guessing is that a 1-ply search has two
completely different failure modes and they look identical from outside.

Usage:
    python3 tools/coverage_census.py --players 4 --games 8 \\
        --champ experiments/league_state/champion_4p.json --out /tmp/c4.json
    python3 tools/coverage_census.py --players 4 --games 8 --bot default

`--bot default` uses DEFAULT_WEIGHTS: a champion's blind spot and a
structural blind spot are different findings and the tool must be able to
tell them apart.

One worker.  ~8 games of 4p takes about a minute with TTA_JOURNAL=1.
"""
from __future__ import annotations

import argparse
import json
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine import actions, cards as C, game            # noqa: E402
from engine.bots.weighted import (DEFAULT_WEIGHTS, WeightedBot,  # noqa: E402
                                  load_weights)

_DB = C.db()


def _row_type(state, idx):
    name = state.card_row[idx] if 0 <= idx < len(state.card_row) else None
    return _DB.type_of(name) if name in _DB.by_name else "?"


def _worker_class(name):
    typ = _DB.type_of(name) if name in _DB.by_name else "?"
    if typ in C.UNIT_TYPES:
        return "unit"
    if typ in C.URBAN_TYPES:
        return "urban"
    if typ in C.PRODUCTION_TYPES:
        return typ
    return typ


def label(state, mv):
    """The mechanic tag of one move, as fine-grained as is useful."""
    k = mv[0]
    if k == "take":
        return "take:" + _row_type(state, mv[1])
    if k == "develop":
        typ = _DB.type_of(mv[1]) if mv[1] in _DB.by_name else "?"
        if typ in C.URBAN_TYPES:
            typ = "urban"
        elif typ in C.UNIT_TYPES:
            typ = "unit"
        return "develop:" + typ
    if k in ("build", "destroy"):
        return k + ":" + _worker_class(mv[1])
    if k == "upgrade":
        return "upgrade:" + _worker_class(mv[1])
    if k == "prepare_event":
        typ = _DB.type_of(mv[1]) if mv[1] in _DB.by_name else "?"
        return "prepare_event:" + typ
    if k == "play_action":
        return "play_action"
    if k == "choose":
        pend = state.pending[-1] if state.pending else {}
        return "choice:" + str(pend.get("tag", "?"))
    if k == "bid":
        return "bid"
    return k


def age_bucket(state):
    return state.age_civil


class Census:
    def __init__(self):
        self.legal = {}          # tag -> decisions where it was legal
        self.taken = {}          # tag -> decisions where it was chosen
        self.legal_moves = {}    # tag -> total legal move count
        self.decisions = 0
        self.events = {}         # named one-off engine outcomes

    def bump(self, d, k, n=1):
        d[k] = d.get(k, 0) + n

    def note(self, state, moves, chosen):
        self.decisions += 1
        seen = set()
        for m in moves:
            t = label(state, m)
            self.bump(self.legal_moves, t)
            seen.add(t)
        for t in seen:
            self.bump(self.legal, t)
        self.bump(self.taken, label(state, chosen))

    def merge(self, other):
        for name in ("legal", "taken", "legal_moves", "events"):
            dst, src = getattr(self, name), getattr(other, name)
            for k, v in src.items():
                dst[k] = dst.get(k, 0) + v
        self.decisions += other.decisions


class Watch:
    """Wraps a bot; records legal-vs-taken for every decision it owns."""

    def __init__(self, bot, census):
        self.bot, self.c = bot, census

    def __call__(self, state):
        moves = actions.legal_moves(state)
        mv = self.bot(state)
        self.c.note(state, moves, mv)
        return mv


def _snapshot(state):
    """Engine-level outcomes that no single move records."""
    out = {}
    for p in state.players:
        out["colonies_held"] = out.get("colonies_held", 0) + len(p.colonies)
        out["wonders_done"] = out.get("wonders_done", 0) + \
            len(p.completed_wonders)
        out["wonders_unfinished"] = out.get("wonders_unfinished", 0) + \
            (1 if p.wonder else 0)
        out["leader_in_play"] = out.get("leader_in_play", 0) + \
            (1 if p.leader else 0)
        out["gov_not_despotism"] = out.get("gov_not_despotism", 0) + \
            (0 if p.government == "Despotism" else 1)
        out["pacts_held"] = out.get("pacts_held", 0) + len(p.pacts)
        out["tactic_set"] = out.get("tactic_set", 0) + (1 if p.tactic else 0)
    for line in state.log or ():
        for key, pat in (("log_colonized", "colonized"),
                         ("log_no_bids", ": no bids"),
                         ("log_nobody_can", "nobody can colonize"),
                         ("log_revolution", "revolution ->"),
                         ("log_war", "war ")):
            if pat in line:
                out[key] = out.get(key, 0) + 1
    return out


def run(players, games, weights, seed0, journal_note=""):
    c = Census()
    scores_seen = 0
    for gi in range(games):
        bots = [Watch(WeightedBot(weights=weights, seed=(seed0 + gi) * 97 + i),
                      c) for i in range(players)]
        st = game.play_game(bots, players, seed=seed0 + gi)
        for k, v in _snapshot(st).items():
            c.bump(c.events, k, v)
        scores_seen += 1
        print(f"  game {gi}: scores={game.scores(st)} "
              f"decisions={c.decisions}", file=sys.stderr, flush=True)
    c.events["games"] = scores_seen
    return c


def table(c):
    """Rows sorted by how legal-but-untaken each mechanic is."""
    tags = sorted(set(c.legal) | set(c.taken))
    rows = []
    for t in tags:
        lg = c.legal.get(t, 0)
        tk = c.taken.get(t, 0)
        rows.append({"mechanic": t, "legal_decisions": lg,
                     "legal_moves": c.legal_moves.get(t, 0), "taken": tk,
                     "take_rate": round(tk / lg, 5) if lg else None})
    rows.sort(key=lambda r: (r["take_rate"] is not None and r["take_rate"],
                             -r["legal_decisions"]))
    return rows


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--players", type=int, default=4)
    ap.add_argument("--games", type=int, default=8)
    ap.add_argument("--champ", default=None)
    ap.add_argument("--bot", default="champ", choices=("champ", "default"))
    ap.add_argument("--seed0", type=int, default=90000)
    ap.add_argument("--out", default=None)
    a = ap.parse_args()

    if a.bot == "default":
        w, src = dict(DEFAULT_WEIGHTS), "default"
    else:
        src = a.champ or f"experiments/league_state/champion_{a.players}p.json"
        w = load_weights(src)

    c = run(a.players, a.games, w, a.seed0)
    out = {"players": a.players, "games": a.games, "weights": src,
           "decisions": c.decisions, "events": c.events, "census": table(c)}
    print(json.dumps(out, indent=1))
    if a.out:
        with open(a.out, "w") as fh:
            json.dump(out, fh, indent=1)


if __name__ == "__main__":
    main()
