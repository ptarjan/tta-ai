"""Turn-by-turn opening: what does the champion actually do on rounds 1-6?

`experiments/behaviour.py` reports *median rounds per milestone*, which is
enough to say "production goes up on round 2" but not enough to say
**farm or mine**, nor to say what the second and third actions of a turn are.
This script logs every move the champion seat makes in rounds 1-6, with the
card name and card type attached, and aggregates:

  * per round: the ordered list of move kinds, most common first/second/third
  * the farm-vs-mine split of the first production build
  * which card is taken on round 1
  * how often each thing happens at all

Usage:
    python3 analysis/opening_order.py --players 2 --games 60 \
        --champion /tmp/ch2.json

Always copy the champion file out of experiments/ first: a live hill-climb
rewrites it.
"""
from __future__ import annotations

import argparse
import collections
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from experiments.arena import load_spec, make_bot  # noqa: E402


def card_type(db, name):
    """Cards in the db are plain dicts: {'name':..., 'age':..., 'type':...}."""
    c = db.get(name)
    if not c:
        return "?"
    if isinstance(c, dict):
        return c.get("type") or "?"
    return getattr(c, "type", None) or getattr(c, "kind", "?")


def card_age(db, name):
    c = db.get(name)
    if not c:
        return "?"
    if isinstance(c, dict):
        return c.get("age") or "?"
    return getattr(c, "age", "?")


class Logger:
    """Bot wrapper that records (round, kind, name, type) for early rounds."""

    def __init__(self, inner, idx, max_round=8):
        self.inner = inner
        self.idx = idx
        self.max_round = max_round
        self.log = []          # [round, kind, name, type]
        self.first_prod = None  # ('farm'|'mine', round, name)
        # whole-game milestones, NOT limited to max_round:
        self.first_develop = {}  # type -> round of first research (science paid)
        self.first_build = {}    # type -> round the first worker goes on one
        self.first_take = {}     # type -> round the card is first taken
        self.took = {}           # card name -> round first taken (whole game)
        self.played = {}         # card name -> round first built/developed

    def __call__(self, state):
        """engine.game.play_game calls bots as ``bot(state) -> move``."""
        mv = self.inner(state)
        try:
            if state.decider() == self.idx:
                self._note(state, mv)
        except Exception as e:
            self.log.append([-1, "ERR", repr(e), "?"])
        return mv

    def _note(self, state, mv):
        from engine import cards as C
        db = C.db()
        kind = mv[0]
        name, typ = "", ""
        if kind == "take":
            try:
                name = state.card_row[mv[1]]
            except Exception:
                name = "?"
            typ = card_type(db, name)
            self.first_take.setdefault(typ, state.round)
            if name and name != "?":
                self.took.setdefault(name, state.round)
        elif kind in ("build", "upgrade", "develop"):
            name = mv[-1]
            typ = card_type(db, name)
            if name:
                self.played.setdefault(name, state.round)
            if kind == "develop":
                self.first_develop.setdefault(typ, state.round)
            elif kind == "build":
                self.first_build.setdefault(typ, state.round)
            if typ in ("farm", "mine") and self.first_prod is None:
                self.first_prod = (typ, state.round, name, kind)
        if state.round > self.max_round:
            return
        elif kind == "destroy":
            name = mv[1] if len(mv) > 1 else ""
            typ = card_type(db, name)
            # a unit disband is a military action; a building razing is civil
            kind = "disband" if db.is_unit_name.get(name) else "raze"
        elif kind in ("play_leader", "play_action", "revolution"):
            name = mv[1] if len(mv) > 1 else ""
            typ = card_type(db, name) if name else ""
        elif kind == "wonder_step":
            p = state.players[self.idx]
            name = p.wonder.name if p.wonder is not None else "?"
            typ = "wonder"
        self.log.append([state.round, kind, name, typ])


def run(players, games, champ_path, seed0=51000, max_round=8):
    from engine import game
    spec = load_spec(champ_path)
    out = []
    for g in range(games):
        seed = seed0 + g
        seat = g % players
        loggers = []
        bots = []
        for i in range(players):
            lg = Logger(make_bot(spec, seed * 97 + i * 13 + 1), i, max_round)
            loggers.append(lg)
            bots.append(lg)
        try:
            game.play_game(bots, players, seed=seed, move_cap=100000)
        except Exception as e:
            print("game error", seed, repr(e), file=sys.stderr)
            continue
        out.append(loggers[seat])
    return out


def summarize(loggers, players):
    n = len(loggers)
    print(f"\n===== {players}p, {n} games =====")

    # farm vs mine on the first production build
    fp = [lg.first_prod for lg in loggers if lg.first_prod]
    c = collections.Counter(t for t, r, nm, k in fp)
    print(f"\nFIRST PRODUCTION BUILD ({len(fp)}/{n} games have one by round "
          f"{loggers[0].max_round}):")
    for t, k in c.most_common():
        rounds = [r for tt, r, nm, kk in fp if tt == t]
        names = collections.Counter(nm for tt, r, nm, kk in fp if tt == t)
        kinds = collections.Counter(kk for tt, r, nm, kk in fp if tt == t)
        print(f"  {t:6s} {k:4d} ({k/max(1,len(fp)):.0%})  "
              f"median round {sorted(rounds)[len(rounds)//2]}  "
              f"{dict(kinds)}  top cards {names.most_common(3)}")

    # whole-game milestones: research it / build the first one
    print("\nFIRST RESEARCH vs FIRST BUILD (median round; share of games):")
    print(f"  {'type':14s} {'develop':>18s} {'build':>18s} {'take':>18s}")
    kinds = sorted({k for lg in loggers
                    for d in (lg.first_develop, lg.first_build, lg.first_take)
                    for k in d})
    for t in kinds:
        cells = []
        for attr in ("first_develop", "first_build", "first_take"):
            rs = sorted(getattr(lg, attr).get(t) for lg in loggers
                        if getattr(lg, attr).get(t) is not None)
            cells.append(f"{rs[len(rs)//2]:>3d} ({len(rs)/n:.0%})" if rs
                         else "    --   ")
        print(f"  {t:14s} {cells[0]:>18s} {cells[1]:>18s} {cells[2]:>18s}")

    # per-card pickup rate, grouped by age and type: the raw material for a
    # priority list. "took" = the card entered the player's hand/board at all.
    from engine import cards as C
    db = C.db()
    by_group = collections.defaultdict(list)
    for lg in loggers:
        for nm in lg.took:
            by_group[(card_age(db, nm), card_type(db, nm))].append(nm)
    print("\nPICKUP RATE BY AGE AND TYPE "
          "(share of games the card was taken at all; median round taken):")
    for age in ("A", "I", "II", "III"):
        for typ in sorted({t for (a, t) in by_group if a == age}):
            names = collections.Counter(by_group[(age, typ)])
            row = []
            for nm, k in names.most_common():
                rs = sorted(lg.took[nm] for lg in loggers if nm in lg.took)
                row.append(f"{nm} {k/n:.0%}@r{rs[len(rs)//2]}")
            print(f"  [{age}] {typ:14s} " + ", ".join(row))

    # what happens each round
    for rnd in range(1, 7):
        kinds = collections.Counter()
        seqs = collections.Counter()
        names = collections.Counter()
        for lg in loggers:
            ms = [e for e in lg.log if e[0] == rnd]
            for r, k, nm, t in ms:
                if k in ("end_turn", "pol_pass", "choose"):
                    continue
                kinds[k if k not in ("build", "upgrade", "develop", "take",
                                     "disband", "raze")
                      else f"{k}:{t}"] += 1
                if nm:
                    names[f"{k}:{nm}"] += 1
            sig = tuple(k for r, k, nm, t in ms
                        if k not in ("end_turn", "pol_pass", "choose"))
            seqs[sig] += 1
        print(f"\n-- round {rnd} --")
        print("  actions/game:", ", ".join(
            f"{k} {v/n:.2f}" for k, v in kinds.most_common(12)))
        print("  top cards   :", ", ".join(
            f"{k} {v/n:.2f}" for k, v in names.most_common(8)))
        print("  top sequence:", " | ".join(
            f"{'>'.join(s) or '(nothing)'} {v/n:.0%}"
            for s, v in seqs.most_common(3)))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--players", type=int, default=2)
    ap.add_argument("--games", type=int, default=60)
    ap.add_argument("--champion", required=True)
    ap.add_argument("--seed0", type=int, default=51000)
    ap.add_argument("--max-round", type=int, default=8)
    a = ap.parse_args()
    lgs = run(a.players, a.games, a.champion, a.seed0, a.max_round)
    summarize(lgs, a.players)


if __name__ == "__main__":
    main()
