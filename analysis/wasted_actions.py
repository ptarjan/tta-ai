"""Why does the champion end its turn with civil actions unspent?

For every decision where the bot chooses ``("end_turn",)`` while it still has
civil actions left, this records:

  * how many civil actions die unspent,
  * every legal move that WOULD have spent one (i.e. an affordable card /
    build / develop / wonder step -- ``legal_moves`` already filters on civil
    actions, resources, science, hand limit and urban limit, so "legal" and
    "affordable" are the same set),
  * what the bot's own evaluation said about each of them, relative to the
    do-nothing baseline, and
  * the evaluation gap between ``end_turn``'s child state and that baseline
    -- the *production flattery*, since ending the turn runs a production
    phase that no other candidate move gets.

Usage:
    python3 analysis/wasted_actions.py --players 2 --games 150 \
        --champion experiments/champion_2p.json --out analysis/wasted_2p.json

Nothing here mutates the engine or the weight files.
"""
from __future__ import annotations

import argparse
import json
import multiprocessing as mp
import os
import random
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from engine import actions, cards as C, game            # noqa: E402
from engine.bots.fastcopy import copy_state             # noqa: E402
from engine.bots.weighted import (                      # noqa: E402
    DEFAULT_WEIGHTS, WeightedBot, evaluate, load_weights, rival_context)

# Move kinds that consume a CIVIL action (engine/actions.py handlers).  build /
# upgrade / destroy are civil only when the card is not a military unit.
_CIVIL_KINDS = {"take", "pop", "wonder_step", "play_leader", "develop",
                "revolution", "play_action"}
_MAYBE_CIVIL = {"build", "upgrade", "destroy"}


def costs_civil_action(move):
    kind = move[0]
    if kind in _CIVIL_KINDS:
        return True
    if kind in _MAYBE_CIVIL:
        return not actions.is_unit(move[1])
    return False


def _card_type(db, name):
    c = db.by_name.get(name)
    return c["type"] if c else "?"


def move_label(db, move):
    """A coarse category for grouping declined moves."""
    kind = move[0]
    if kind == "take":
        return "take"
    if kind in ("build", "upgrade", "destroy"):
        return kind
    if kind == "develop":
        return "develop"
    return kind


class Probe:
    """Wraps the champion; scores every candidate itself to see the reasons."""

    def __init__(self, weights, idx, seed=0):
        self.inner = WeightedBot(weights=weights, seed=seed)
        self.w = self.inner.weights
        self.idx = idx
        self.events = []

    def __call__(self, state):
        moves = actions.legal_moves(state)
        mv = self.inner.pick(state, moves)
        if mv[0] == "end_turn":
            p = state.players[state.decider()]
            if p.civil_actions > 0:
                try:
                    self._record(state, moves, p)
                except Exception as e:      # diagnostics must never kill a game
                    self.events.append({"error": repr(e)})
        return mv

    def _record(self, state, moves, p):
        db = C.db()
        idx = state.decider()
        ctx = rival_context(state, idx)
        w = self.w
        bias = w.get("end_turn_bias", 0.0)

        # do-nothing baseline: the position as it stands, unmoved
        base = evaluate(state, idx, w, ctx)

        end_raw = None
        alts = []
        for m in moves:
            trial = copy_state(state)
            try:
                actions.apply(trial, m, random.Random(0))
                val = evaluate(trial, idx, w, ctx)
            except Exception:
                continue
            if m[0] == "end_turn":
                end_raw = val
                continue
            if not costs_civil_action(m):
                continue
            name = m[1] if len(m) > 1 else None
            if m[0] == "take":
                name = state.card_row[m[1]]
            alts.append({
                "move": list(m),
                "kind": move_label(db, m),
                "card": name,
                "ctype": _card_type(db, name) if isinstance(name, str) else None,
                "delta": round(val - base, 4),
            })

        alts.sort(key=lambda a: -a["delta"])
        best = alts[0] if alts else None
        # what the row actually offered, regardless of legality
        row_nonempty = sum(1 for n in state.card_row if n is not None)
        take_legal = sum(1 for a in alts if a["kind"] == "take")
        yellow_in_hand = sum(1 for n in p.hand_civil
                             if _card_type(db, n) == "action")
        yellow_playable = sum(1 for a in alts
                              if a["move"][0] == "play_action")

        self.events.append({
            "age": state.age_civil,
            "round": state.round,
            "ca_left": p.civil_actions,
            "ca_total": actions.ca_total(state, p),
            "hand_civil": len(p.hand_civil),
            "hand_limit": actions.civil_hand_limit(state, p),
            "row_nonempty": row_nonempty,
            "take_legal": take_legal,
            "yellow_in_hand": yellow_in_hand,
            "yellow_playable": yellow_playable,
            "n_alts": len(alts),
            "base": round(base, 4),
            "end_raw": round(end_raw, 4) if end_raw is not None else None,
            # how much ending the turn is flattered by its free production phase
            "flattery": (round(end_raw - base, 4)
                         if end_raw is not None else None),
            "bias": bias,
            "best_alt": best,
            "alts": alts[:6],
        })


def _play_one(task):
    seed, n, weights = task
    probes = [Probe(weights, i, seed=seed * 97 + i) for i in range(n)]
    try:
        st = game.play_game(probes, n, seed=seed)
    except Exception as e:
        return {"seed": seed, "error": repr(e), "events": []}
    return {"seed": seed, "scores": game.scores(st),
            "events": [e for pr in probes for e in pr.events]}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--players", type=int, default=2)
    ap.add_argument("--games", type=int, default=100)
    ap.add_argument("--champion", default=None)
    ap.add_argument("--out", required=True)
    ap.add_argument("--workers", type=int, default=0)
    a = ap.parse_args()

    weights = load_weights(a.champion) if a.champion else dict(DEFAULT_WEIGHTS)
    tasks = [(s * 7919 + 17, a.players, weights) for s in range(a.games)]
    workers = a.workers or max(1, (os.cpu_count() or 2) - 1)
    if workers <= 1:
        recs = [_play_one(t) for t in tasks]
    else:
        ctx = mp.get_context("fork")
        with ctx.Pool(workers) as pool:
            recs = pool.map(_play_one, tasks, chunksize=2)

    events = [e for r in recs for e in r["events"]]
    errs = [r for r in recs if r.get("error")]
    with open(a.out, "w") as fh:
        json.dump({"players": a.players, "games": a.games,
                   "champion": a.champion, "errors": len(errs),
                   "error_sample": [r["error"] for r in errs[:3]],
                   "events": events}, fh)
    print(f"{a.players}p: {len(events)} wasted-action turns from {a.games} "
          f"games ({len(errs)} failed) -> {a.out}")


if __name__ == "__main__":
    main()
