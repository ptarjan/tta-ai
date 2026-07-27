#!/usr/bin/env python3
"""Behavioural profile of the 2p league champion against the real pool.

    python3 tools/twop_profile.py --games 200 --workers 3 --out /tmp/twop

WHY THIS EXISTS.  `experiments/league_state/champion_2p.json` beats every
member of the 2p pool by 70-90 culture points.  Two established results in
this repo forbid reading the *reason* off the weight vector:

  * champion weight marginals are indistinguishable from a random walk
    (KS p = 0.14-0.80) even while the champion beats its drift siblings, so
    an individual trained weight carries no strategy;
  * `experiments/league_state/weight_credit_2p.json` (1 cycle, n=72, 18 of
    78 weights, edges of order 0.01-0.04) is underpowered -- it can neither
    confirm nor deny that a weight matters.

So this tool measures what the champion *does*, from played games.

WHAT IS INSTRUMENTED, AND WHY IT CANNOT CHANGE THE GAME
-------------------------------------------------------
Two additive layers, neither of which touches `engine/`:

1. `Recorder` (imported unchanged from `experiments/behaviour.py`, subclassed
   here only to also note the NAME of a developed technology) wraps each bot:
   it asks the wrapped bot for a move, notes it, and returns it unchanged.

2. A culture-source ledger.  `PlayerState.__setattr__` is swapped for a hook
   that records every write to `culture` together with the engine
   file:line that wrote it -- and it is installed ONLY around the real
   `actions.apply` call in the driver loop below, i.e. never while a bot is
   searching.  A bot's trial states are mutated inside `bot(state)`, which is
   outside the window, so search is neither slowed nor observed.  The hook
   chains to whatever `__setattr__` was there before (under TTA_JOURNAL=1
   that is `journal._journalling_setattr`), so behaviour is identical.

   Self-check: the ledger is asserted to sum to the player's final score on
   every game (`ledger_ok`).  A game where it does not is reported, not used.

The driver loop is a transcription of `engine.game.play_game` with the same
rng seeding, so seeds are comparable with `experiments/arena.py`.
"""
from __future__ import annotations

import argparse
import json
import math
import multiprocessing as mp
import os
import random
import sys
import time

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

os.environ.setdefault("TTA_JOURNAL", "1")

from experiments import arena                                    # noqa: E402
from experiments import hillclimb_pool                           # noqa: E402,F401
from experiments.behaviour import Recorder                       # noqa: E402

CHAMP = "experiments/league_state/champion_2p.json"

OPPONENTS = ["book", "book2", "var:culture", "var:infra", "var:military",
             "var:science", "var:tempo", "var:wonder"]

VARIANT_SPECS = {
    "var:tempo": ("variant", "tempo", "TempoBot"),
    "var:infra": ("variant", "infrastructure", "InfraBot"),
    "var:military": ("variant", "military", "MilitaryBot"),
    "var:culture": ("variant", "culture", "CultureBot"),
    "var:science": ("variant", "science", "ScienceBot"),
    "var:wonder": ("variant", "wonder", "WonderBot"),
}


def resolve(label, champ_path=CHAMP):
    # `--matchups` is colon-separated, so the pool's own `var:culture` spelling
    # has to be writable as `var_culture` on the command line.
    if label.startswith("var_"):
        label = "var:" + label[4:]
    if label == "champion":
        return arena.load_spec(f"quiesce:{champ_path},levels=1")
    if label == "champion1ply":
        return arena.load_spec(champ_path)
    if label == "defaultq":
        # DEFAULT_WEIGHTS under the SAME architecture the champion plays.
        # `champion` vs `defaultq` vs `champion1ply` vs `default` is a 2x2 that
        # separates "the search found it" from "training found it".
        return arena.load_spec("quiesce:default,levels=1")
    if label.startswith("past:"):
        return arena.load_spec(label[len("past:"):])
    if label in VARIANT_SPECS:
        return VARIANT_SPECS[label]
    return label            # "book", "book2", "default", "greedy", "random"


# ------------------------------------------------------------------ ledger
#
# engine file:line -> human bucket.  Anything unmapped is reported verbatim
# under `unmapped:` so a new culture source cannot silently vanish into a
# residual.
SITES = {
    "economy.py:127": "rate:production",
    "economy.py:144": "penalty:food_shortage",
    "economy.py:179": "leader:genghis",
    "events.py:55":   "event:gain",
    "events.py:58":   "event:loss",
    "events.py:214":  "event:scoring",
    "events.py:222":  "event:ranking",
    "events.py:256":  "event:equal_to_culture_prod",
    "events.py:258":  "event:equal_to_science_prod",
    "events.py:317":  "penalty:war_food",
    "events.py:465":  "endgame:final_event_scoring",
    "events.py:474":  "endgame:final_event_ranking",
    "events.py:526":  "aggression:stolen_from_me",
    "events.py:527":  "aggression:stolen_by_me",
    "events.py:589":  "war:lost_to_victor",
    "events.py:590":  "war:spoils",
    "actions.py:855": "gov:robespierre",
    "actions.py:867": "leader:churchill",
    "actions.py:914": "card:per_richer_rival",
    "actions.py:950": "card:gainCulture",
    "actions.py:990": "military:prepare_event",
    "actions.py:1044": "opponent_resigned",
    "effects.py:1034": "leader:einstein",
    "effects.py:1057": "build:one_time_culture",
    "game.py:311":    "endgame:bonus",
    "interact.py:210": "aggression:took_leader",
    "interact.py:213": "aggression:took_wonder",
}


#: set by `wrap_events` while one named event card is resolving, so the
#: ledger can attribute culture to the CARD and to the player who seeded it.
CUR_EVENT = [None]


def wrap_events():
    """Make `events.resolve_event` publish which card it is resolving.

    Idempotent, and a pure pass-through: the wrapper sets a module global,
    calls the original, and clears it.  `state.seeded_by[name]` is the engine's
    own record of who prepared the card (`actions._h_prepare_event`), so the
    attribution is the engine's, not a reconstruction.
    """
    from engine import events
    if getattr(events.resolve_event, "_twop_wrapped", False):
        return
    orig = events.resolve_event

    def wrapped(state, name, rng, revealer_idx):
        prev = CUR_EVENT[0]
        CUR_EVENT[0] = (name, state.seeded_by.get(name, -1))
        try:
            return orig(state, name, rng, revealer_idx)
        finally:
            CUR_EVENT[0] = prev

    wrapped._twop_wrapped = True
    events.resolve_event = wrapped


class Ledger:
    """Attributes every write to `p.culture` in the REAL game to its source."""

    def __init__(self, state):
        from engine import state as S
        self.S = S
        self.idx_of = {id(p): p.idx for p in state.players}
        self.n = len(state.players)
        # bucket -> [per player total]; and the same split by age
        self.total = {}
        self.by_age = {}
        # (seeder_idx) -> [culture each player got from cards THAT player
        # seeded]; -1 is "seeded by nobody" (the initial deck).
        self.by_seeder = {}
        self.by_event = {}
        self.state = state
        self._prev = None
        self._active = False

    def _bucket(self, frame):
        f = frame.f_code.co_filename
        key = f"{f[f.rfind(os.sep) + 1:]}:{frame.f_lineno}"
        return SITES.get(key, "unmapped:" + key)

    def __enter__(self):
        prev = self._prev = self.S.PlayerState.__setattr__
        idx_of, total, by_age, n, st = (self.idx_of, self.total, self.by_age,
                                        self.n, self.state)
        by_seeder, by_event = self.by_seeder, self.by_event
        bucket = self._bucket

        # A plain function, NOT a bound method: a bound method stored on a
        # class is not a descriptor, so `obj.attr = v` would call it without
        # `obj`.
        def hook(obj, name, value):
            if name == "culture":
                i = idx_of.get(id(obj))
                if i is not None:
                    d = value - obj.__dict__.get("culture", 0)
                    if d:
                        b = bucket(sys._getframe(1))
                        total.setdefault(b, [0] * n)[i] += d
                        by_age.setdefault((b, st.age_civil), [0] * n)[i] += d
                        ev = CUR_EVENT[0]
                        if ev is not None:
                            by_seeder.setdefault(ev[1], [0] * n)[i] += d
                            by_event.setdefault(ev[0], [0] * n)[i] += d
            prev(obj, name, value)

        self.S.PlayerState.__setattr__ = hook
        return self

    def __exit__(self, *exc):
        self.S.PlayerState.__setattr__ = self._prev
        return False


# ------------------------------------------------- move-class ablation
#
# The score-composition ledger is ACCOUNTING: it says where the points landed,
# not that removing the source would remove the margin.  This wrapper is the
# causal check.  `WeightedBot`/`QuiescentBot` expose `pick(state, moves)` and
# their `__call__` is exactly `pick(state, actions.legal_moves(state))`, so
# handing them a filtered legal-move list removes a move CLASS from the bot
# without touching the engine, the weights or the search.  Politics-phase
# aggression/war moves always coexist with `pol_pass`, so the filtered list is
# never empty.

class MoveClassBan:
    """Play `inner`, but never offer it moves whose kind is in `banned`."""

    def __init__(self, inner, banned):
        self.inner = inner
        self.banned = frozenset(banned)

    def __call__(self, state):
        from engine import actions
        moves = actions.legal_moves(state)
        kept = [m for m in moves if m[0] not in self.banned]
        if not kept:
            kept = moves
        return self.inner.pick(state, kept)


# ------------------------------------------------------------------ record

class Rec2(Recorder):
    """`Recorder` plus the names of developed techs / built buildings."""

    def __init__(self, *a, **kw):
        super().__init__(*a, **kw)
        self.rec["dev_names"] = []
        self.rec["build_names"] = []
        self.rec["gap_by_round"] = []
        self.rec["prepared"] = []

    def _note(self, state, mv):
        super()._note(state, mv)
        if mv[0] == "develop":
            self.rec["dev_names"].append([state.round, mv[-1]])
        elif mv[0] in ("build", "upgrade"):
            self.rec["build_names"].append([state.round, mv[0], mv[-1]])
        elif mv[0] == "end_turn":
            self._augment(state)
        if mv[0] == "prepare_event":
            from engine import cards as C
            db = C.db()
            name = mv[1]
            self.rec["prepared"].append(
                [state.round, name, db.level_of(name),
                 db.age_of(name) if hasattr(db, "age_of") else "?"])

    def _augment(self, state):
        """Add `lead_over_rival` to the snapshot `Recorder` just took.

        This is EXACTLY the quantity the hand-written variants gate their
        offence on -- `engine/bots/variants/base.py::_lead_over`, i.e.
        `attack_strength(me, rival) - rival.strength`, which includes the
        attacker's tactic/bonus, not the bare strength difference.
        `MilitaryBot` needs >= 3-4 of it to launch an aggression and >= 5 to
        declare a war, so it is the number that decides whether its whole
        offensive plan ever fires.
        """
        from engine import effects
        if not self.rec["snaps"]:
            return
        p = state.players[self.idx]
        q = [x for x in state.players if x.idx != self.idx][0]
        try:
            lead = (effects.attack_strength(state, p, q)
                    - effects.state_stats(state, q).strength)
        except Exception:
            return
        self.rec["snaps"][-1]["lead_over_rival"] = lead


# ------------------------------------------------------------------ worker

_W = {}


def _init(champ, opp, cap, ban_a=(), ban_b=()):
    _W["champ"], _W["opp"], _W["cap"] = champ, opp, cap
    _W["ban_a"], _W["ban_b"] = tuple(ban_a), tuple(ban_b)


def _play(task):
    """task = (seed, seat) -> one game's records for both seats."""
    from engine import game, actions, cards as C
    wrap_events()
    seed, seat = task
    specs = [_W["opp"], _W["opp"]]
    specs[seat] = _W["champ"]
    bans = [_W["ban_a"] if i == seat else _W["ban_b"] for i in range(2)]
    inner = [arena.make_bot(sp, seed * 97 + i * 13 + 1)
             for i, sp in enumerate(specs)]
    inner = [MoveClassBan(bt, bn) if bn else bt
             for bt, bn in zip(inner, bans)]
    recs = [Rec2(bt, i, 2) for i, bt in enumerate(inner)]
    st = game.new_game(2, seed)
    ledger = Ledger(st)
    rng = random.Random(seed ^ 0x5EED)
    moves = 0
    try:
        while not st.game_over:
            if moves >= _W["cap"]:
                st.move_cap_hit = True
                game._finish_game(st)
                break
            mv = recs[st.decider()](st)
            with ledger:
                actions.apply(st, mv, rng)
            moves += 1
    except Exception as e:
        return {"error": repr(e), "seed": seed}
    sc = game.scores(st)
    tot = {b: list(v) for b, v in ledger.total.items()}
    ok = [abs(sum(tot[b][i] for b in tot) - sc[i]) < 1e-6 for i in range(2)]
    db = C.db()
    # Age III events still in a deck at game end: these are the ones
    # `evaluate_final_events` scores, so seeding one is a guaranteed payout.
    leftover3 = [n for n in list(st.current_events) + list(st.future_events)
                 if n in db.by_name and db.age_of(n) == "III"]
    for i, r in enumerate(recs):
        r.rec["culture"] = sc[i]
        r.rec["rounds"] = st.round
        r.rec["win"] = 0.5 if sc[0] == sc[1] else (1.0 if sc[i] > sc[1 - i]
                                                   else 0.0)
        r.rec["is_champ"] = (i == seat)
        r.rec["margin"] = sc[i] - sc[1 - i]
        r.rec["ledger"] = {b: v[i] for b, v in tot.items()}
        r.rec["ledger_by_age"] = {f"{b}|{a}": v[i]
                                  for (b, a), v in ledger.by_age.items()}
        r.rec["ledger_ok"] = ok[i]
        # culture I got out of event cards, split by WHO PUT THEM IN THE DECK
        r.rec["event_culture_by_seeder"] = {
            ("mine" if s == i else ("rival" if s >= 0 else "initial_deck")):
                v[i] for s, v in ledger.by_seeder.items()}
        r.rec["events_resolved_seeded_by_me"] = sum(
            1 for n in st.past_events if st.seeded_by.get(n, -1) == i)
        r.rec["events_resolved_total"] = len(st.past_events)
        r.rec["leftover_age3_events"] = len(leftover3)
        r.rec["leftover_age3_seeded_by_me"] = sum(
            1 for n in leftover3 if st.seeded_by.get(n, -1) == i)
        # who hit me
        other = recs[1 - i]
        for rnd, kind, target in other.rec["attacks_made"]:
            if target == i:
                r.rec["attacked_by"].append([rnd, kind, 1 - i])
        # score gap at the end of each of my turns
        r.rec["gap_by_round"] = [[s["round"], s["culture"] - s["opp_culture_max"]]
                                 for s in r.rec["snaps"]]
    return {"seed": seed, "seat": seat, "moves": moves,
            "recs": [r.rec for r in recs]}


def split_ban(label):
    """``champion!aggression!war`` -> ("champion", ("aggression", "war"))."""
    parts = label.split("!")
    return parts[0], tuple(parts[1:])


def run_matchup(champ_label, opp_label, games, workers, seed0, cap,
                champ_path=CHAMP):
    champ_label, ban_a = split_ban(champ_label)
    opp_label, ban_b = split_ban(opp_label)
    champ, opp = resolve(champ_label, champ_path), resolve(opp_label, champ_path)
    tasks = [(seed0 + g // 2 * 7919 + 17, g % 2) for g in range(games)]
    args = (champ, opp, cap, ban_a, ban_b)
    if workers <= 1:
        _init(*args)
        res = [_play(t) for t in tasks]
    else:
        ctx = mp.get_context("fork")
        with ctx.Pool(workers, initializer=_init, initargs=args) as pool:
            res = pool.map(_play, tasks, chunksize=2)
    a, b, errs = [], [], []
    for r in res:
        if "error" in r:
            errs.append(r["error"])
            continue
        for rec in r["recs"]:
            (a if rec["is_champ"] else b).append(rec)
    return a, b, errs


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--games", type=int, default=200)
    ap.add_argument("--workers", type=int, default=3)
    ap.add_argument("--seed0", type=int, default=310000)
    ap.add_argument("--cap", type=int, default=20000)
    ap.add_argument("--champion", default=CHAMP)
    ap.add_argument("--out", default="/tmp/twop")
    ap.add_argument("--matchups", default=None,
                    help="comma list of A:B labels; default champion vs pool")
    a = ap.parse_args(argv)

    if a.matchups:
        pairs = [tuple(m.split(":", 1)) for m in a.matchups.split(",")]
    else:
        pairs = [("champion", o) for o in OPPONENTS]

    os.makedirs(a.out, exist_ok=True)
    for A, B in pairs:
        t0 = time.time()
        recs_a, recs_b, errs = run_matchup(A, B, a.games, a.workers, a.seed0,
                                           a.cap, a.champion)
        path = os.path.join(a.out, f"{A.replace(':', '_')}__vs__"
                                   f"{B.replace(':', '_')}.json")
        with open(path, "w") as fh:
            json.dump({"a": A, "b": B, "games": a.games, "seed0": a.seed0,
                       "errors": errs[:3], "error_count": len(errs),
                       "champion": a.champion,
                       "recs_a": recs_a, "recs_b": recs_b}, fh)
        wins = [r["win"] for r in recs_a]
        mar = [r["margin"] for r in recs_a]
        bad = sum(1 for r in recs_a if not r["ledger_ok"])

        def se(xs):
            if len(xs) < 2:
                return 0.0
            m = sum(xs) / len(xs)
            return math.sqrt(sum((x - m) ** 2 for x in xs) / (len(xs) - 1)
                             / len(xs))
        print(f"{A} vs {B}: n={len(wins)} win={sum(wins)/max(1,len(wins)):.3f}"
              f"+/-{se(wins):.3f} margin={sum(mar)/max(1,len(mar)):.1f}"
              f"+/-{se(mar):.1f} ledger_mismatch={bad} errs={len(errs)}"
              f" [{time.time()-t0:.0f}s] -> {path}", flush=True)


if __name__ == "__main__":
    main()
