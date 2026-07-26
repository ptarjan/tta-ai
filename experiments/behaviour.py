"""What does the champion actually *do*?

The hill climb produces 78 numbers.  Numbers are not advice.  This module
plays the champion against a table (by default a mirror of itself) and
records the *behaviour* those numbers produce, in units a human player can
copy: the round you take your first wonder, how many civil actions you leave
on the table, which cost band you buy from, how your workers are split
between production and cities as the ages turn over.

    python3 -m experiments.behaviour --players 3 --games 60 \
        --champion experiments/champion_3p.json --out experiments/behaviour_3p.json

Every bot in the game is wrapped in a `Recorder`, so opponent behaviour is
captured on the same games and every "relative to opponents" number is a
within-game comparison rather than two separate runs.

Nothing here touches the engine: a recorder is a callable `bot(state) -> move`
that asks the wrapped bot for a move, notes what was chosen and hands it back
unchanged.  Snapshots are taken at `end_turn`, i.e. on the finished board at
the end of every one of that player's turns.
"""
from __future__ import annotations

import argparse
import json
import math
import multiprocessing as mp
import os
import statistics
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from experiments.arena import load_spec, make_bot  # noqa: E402

AGE_NAMES = ["A", "I", "II", "III", "IV"]


# --------------------------------------------------------------- recorder

def _card_type(db, name):
    c = db.by_name.get(name)
    return c["type"] if c else "?"


class Recorder:
    """Wraps a bot; records what it chose and the board it left behind."""

    def __init__(self, inner, idx, num_players):
        self.inner = inner
        self.idx = idx
        self.n = num_players
        self.rec = {
            "first": {},           # event -> round first seen
            "takes": {},           # "band1"/"band2"/"band3" -> count
            "take_types": {},      # card type -> count
            "moves": {},           # move kind -> count
            "builds": [],          # [round, kind] for build/upgrade/develop
            "snaps": [],           # one dict per completed turn
            "attacks_made": [],    # [round, kind, target]
            "attacked_by": [],     # filled in by the driver (from others)
            "rounds": 0,
            "culture": 0,
        }

    # -- helpers ----------------------------------------------------
    def _first(self, key, state):
        self.rec["first"].setdefault(key, state.round)

    def __call__(self, state):
        mv = self.inner(state)
        try:
            self._note(state, mv)
        except Exception as e:            # never let bookkeeping kill a game
            self.rec.setdefault("errors", []).append(repr(e))
        return mv

    def _note(self, state, mv):
        from engine import cards as C
        db = C.db()
        p = state.players[self.idx]
        kind = mv[0]
        self.rec["moves"][kind] = self.rec["moves"].get(kind, 0) + 1

        if kind == "take":
            idx = mv[1]
            name = state.card_row[idx]
            band = 1 if idx < 5 else (2 if idx < 9 else 3)
            self.rec["takes"][f"band{band}"] = \
                self.rec["takes"].get(f"band{band}", 0) + 1
            t = _card_type(db, name)
            self.rec["take_types"][t] = self.rec["take_types"].get(t, 0) + 1
            self._first(f"take_{t}", state)
        elif kind == "play_leader":
            self._first("leader", state)
        elif kind == "wonder_step":
            if p.wonder is not None and p.wonder.steps_built == 0:
                self._first("wonder_start", state)
            self._first("wonder_step", state)
        elif kind == "revolution":
            self._first("government", state)
        elif kind in ("build", "upgrade", "develop"):
            name = mv[-1]
            t = _card_type(db, name)
            self.rec["builds"].append([state.round, kind, t])
            self._first(f"{kind}_{t}", state)
            if t in ("farm", "mine"):
                self._first("upgrade_production", state)
            if t in ("lab", "temple", "library", "theater", "arena"):
                self._first("upgrade_urban", state)
            if t == "government":
                self._first("government", state)
        elif kind in ("war", "aggression"):
            self.rec["attacks_made"].append([state.round, kind, mv[2]])
            self._first(kind, state)
        elif kind == "offer_pact":
            self._first("pact", state)
        elif kind == "end_turn":
            self._snapshot(state)

    # -- end-of-turn board snapshot ---------------------------------
    def _snapshot(self, state):
        from engine import cards as C
        from engine import effects
        p = state.players[self.idx]
        s = effects.state_stats(state, p)
        others = [q for q in state.players if q.idx != self.idx]
        o_str, o_units = [], []
        for q in others:
            sq = effects.state_stats(state, q)
            o_str.append(sq.strength)
            o_units.append(effects.workers_on_types(q, C.UNIT_TYPES))
        units = effects.workers_on_types(p, C.UNIT_TYPES)
        self.rec["snaps"].append({
            "round": state.round,
            "age": C.AGE_LEVEL[state.age_civil],
            "culture": p.culture,
            "culture_rate": s.culture,
            "science_rate": s.science,
            "food_rate": s.food,
            "resource_rate": s.resources,
            "strength": s.strength,
            "happy": s.happy,
            "ca_left": p.civil_actions,
            "ma_left": p.military_actions,
            "ca_total": s.civil_actions,
            "ma_total": s.military_actions,
            "w_prod": effects.workers_on_types(p, C.PRODUCTION_TYPES),
            "w_urban": effects.workers_on_types(p, C.URBAN_TYPES),
            "w_units": units,
            "w_free": p.workers_free,
            "bank": p.yellow_bank,
            "wonders": len(p.completed_wonders),
            "wonder_in_progress": 1 if p.wonder is not None else 0,
            "leader": 1 if p.leader else 0,
            "techs": len(p.techs),
            "hand": len(p.hand_civil),
            "opp_strength": (sum(o_str) / len(o_str)) if o_str else 0,
            "opp_units": (sum(o_units) / len(o_units)) if o_units else 0,
            "wars_on_me": len(p.wars_declared_on_me),
        })


# ----------------------------------------------------------------- worker

_W = {}


def _init(champ, opp, num_players, move_cap):
    _W["champ"], _W["opp"] = champ, opp
    _W["n"] = num_players
    _W["cap"] = move_cap


def _play(task):
    """task = (seed, seat) -> per-game records for every seat."""
    from engine import game
    seed, seat = task
    n = _W["n"]
    specs = [_W["opp"]] * n
    specs[seat] = _W["champ"]
    recs = []
    bots = []
    for i, sp in enumerate(specs):
        r = Recorder(make_bot(sp, seed * 97 + i * 13 + 1), i, n)
        recs.append(r)
        bots.append(r)
    try:
        st = game.play_game(bots, n, seed=seed, move_cap=_W["cap"])
    except Exception as e:
        return {"error": repr(e), "seed": seed}
    sc = game.scores(st)
    best = max(sc)
    tied = [i for i, v in enumerate(sc) if v == best]
    for i, r in enumerate(recs):
        r.rec["culture"] = sc[i]
        r.rec["rounds"] = st.round
        r.rec["win"] = (1.0 / len(tied)) if i in tied else 0.0
        r.rec["is_champ"] = (i == seat)
        # who hit me
        for j, other in enumerate(recs):
            if j == i:
                continue
            for rnd, kind, target in other.rec["attacks_made"]:
                if target == i:
                    r.rec["attacked_by"].append([rnd, kind, j])
    return {"seed": seed, "seat": seat, "recs": [r.rec for r in recs]}


# ------------------------------------------------------------- aggregation

def _mean(xs):
    return (sum(xs) / len(xs)) if xs else None


def _med(xs):
    return statistics.median(xs) if xs else None


def _q(xs, f):
    if not xs:
        return None
    ys = sorted(xs)
    return ys[min(len(ys) - 1, int(f * len(ys)))]


def _summarize_group(recs, label):
    """Turn a list of per-game records into human-readable behaviour."""
    n = len(recs)
    if n == 0:
        return {}
    out = {"label": label, "games": n}
    out["win_share"] = _mean([r["win"] for r in recs])
    out["culture_mean"] = _mean([r["culture"] for r in recs])
    out["rounds_mean"] = _mean([r["rounds"] for r in recs])

    # --- first-time milestones (median round; "never" fraction)
    keys = ["wonder_start", "leader", "government", "pact", "war",
            "aggression", "upgrade_production", "upgrade_urban",
            "take_wonder", "take_leader", "take_government", "take_farm",
            "take_mine", "take_lab", "take_temple", "take_library",
            "take_arena", "take_theater", "take_special-tech", "take_action",
            "take_infantry", "take_cavalry", "take_artillery"]
    firsts = {}
    for k in keys:
        got = [r["first"][k] for r in recs if k in r["first"]]
        if not got:
            firsts[k] = {"median_round": None, "share_of_games": 0.0}
            continue
        firsts[k] = {
            "median_round": _med(got),
            "mean_round": round(_mean(got), 2),
            "p25_round": _q(got, 0.25),
            "p75_round": _q(got, 0.75),
            "share_of_games": round(len(got) / n, 3),
        }
    out["first_time"] = firsts

    # --- card row cost bands
    bands = {}
    tot = 0
    for b in ("band1", "band2", "band3"):
        c = sum(r["takes"].get(b, 0) for r in recs)
        bands[b] = c
        tot += c
    out["cost_bands"] = {
        "counts": bands,
        "share": {b: (round(bands[b] / tot, 3) if tot else None) for b in bands},
        "cards_taken_per_game": round(tot / n, 2),
        "civil_actions_spent_taking_per_game":
            round((bands["band1"] + 2 * bands["band2"] + 3 * bands["band3"]) / n, 2),
    }

    # --- what it buys
    types = {}
    for r in recs:
        for t, c in r["take_types"].items():
            types[t] = types.get(t, 0) + c
    ttot = sum(types.values()) or 1
    out["cards_taken_by_type"] = {
        t: {"per_game": round(c / n, 2), "share": round(c / ttot, 3)}
        for t, c in sorted(types.items(), key=lambda kv: -kv[1])
    }

    # --- move mix
    mv = {}
    for r in recs:
        for k, c in r["moves"].items():
            mv[k] = mv.get(k, 0) + c
    out["moves_per_game"] = {k: round(c / n, 2)
                             for k, c in sorted(mv.items(), key=lambda kv: -kv[1])}

    # --- builds/upgrades by target type
    b_by_type = {}
    for r in recs:
        for rnd, kind, t in r["builds"]:
            d = b_by_type.setdefault(t, {"count": 0, "rounds": []})
            d["count"] += 1
            d["rounds"].append(rnd)
    out["builds_by_type"] = {
        t: {"per_game": round(d["count"] / n, 2),
            "median_round": _med(d["rounds"])}
        for t, d in sorted(b_by_type.items(), key=lambda kv: -kv[1]["count"])
    }

    # --- aggression
    wars = sum(1 for r in recs for _, k, _ in r["attacks_made"] if k == "war")
    aggs = sum(1 for r in recs for _, k, _ in r["attacks_made"] if k == "aggression")
    war_hit = sum(1 for r in recs for _, k, _ in r["attacked_by"] if k == "war")
    agg_hit = sum(1 for r in recs for _, k, _ in r["attacked_by"] if k == "aggression")
    out["conflict"] = {
        "wars_started_per_game": round(wars / n, 3),
        "aggressions_started_per_game": round(aggs / n, 3),
        "wars_suffered_per_game": round(war_hit / n, 3),
        "aggressions_suffered_per_game": round(agg_hit / n, 3),
        "games_starting_any_attack":
            round(sum(1 for r in recs if r["attacks_made"]) / n, 3),
        "games_attacked_at_all":
            round(sum(1 for r in recs if r["attacked_by"]) / n, 3),
    }

    # --- per-age board profile, from the end-of-turn snapshots
    by_age = {}
    for r in recs:
        for s in r["snaps"]:
            by_age.setdefault(s["age"], []).append(s)
    ages = {}
    for a in sorted(by_age):
        ss = by_age[a]
        def m(k):
            v = _mean([x[k] for x in ss])
            return round(v, 2) if v is not None else None
        strength = _mean([x["strength"] for x in ss]) or 0.0
        opp_str = _mean([x["opp_strength"] for x in ss]) or 0.0
        units = _mean([x["w_units"] for x in ss]) or 0.0
        opp_units = _mean([x["opp_units"] for x in ss]) or 0.0
        w_tot = (_mean([x["w_prod"] + x["w_urban"] + x["w_units"] for x in ss])
                 or 1.0)
        ages[AGE_NAMES[a]] = {
            "turns_sampled": len(ss),
            "median_round": _med([x["round"] for x in ss]),
            "culture_rate": m("culture_rate"),
            "science_rate": m("science_rate"),
            "sci_per_culture": (round(m("science_rate") / m("culture_rate"), 2)
                                if m("culture_rate") else None),
            "food_rate": m("food_rate"),
            "resource_rate": m("resource_rate"),
            "strength": round(strength, 2),
            "strength_vs_opponents": (round(strength / opp_str, 2)
                                      if opp_str else None),
            "units": round(units, 2),
            "units_vs_opponents": (round(units / opp_units, 2)
                                   if opp_units else None),
            "workers_production": m("w_prod"),
            "workers_urban": m("w_urban"),
            "workers_military": m("w_units"),
            "workers_unused": m("w_free"),
            "prod_share_of_workers": round((_mean([x["w_prod"] for x in ss])
                                            or 0) / w_tot, 3),
            "urban_share_of_workers": round((_mean([x["w_urban"] for x in ss])
                                             or 0) / w_tot, 3),
            "military_share_of_workers": round((_mean([x["w_units"] for x in ss])
                                                or 0) / w_tot, 3),
            "ca_left_unspent": m("ca_left"),
            "ca_total": m("ca_total"),
            "ca_wasted_share": (round(m("ca_left") / m("ca_total"), 3)
                                if m("ca_total") else None),
            "ma_left_unspent": m("ma_left"),
            "happy_margin": m("happy"),
            "wonders_done": m("wonders"),
            "has_leader": m("leader"),
            "hand_size": m("hand"),
        }
    out["by_age"] = ages

    all_snaps = [s for r in recs for s in r["snaps"]]
    out["overall"] = {
        "ca_left_per_turn": round(_mean([s["ca_left"] for s in all_snaps]), 3)
        if all_snaps else None,
        "ma_left_per_turn": round(_mean([s["ma_left"] for s in all_snaps]), 3)
        if all_snaps else None,
        "turns_with_unspent_ca": round(
            sum(1 for s in all_snaps if s["ca_left"] > 0) / len(all_snaps), 3)
        if all_snaps else None,
        "final_wonders": round(_mean([r["snaps"][-1]["wonders"]
                                      for r in recs if r["snaps"]]), 2),
        "final_techs": round(_mean([r["snaps"][-1]["techs"]
                                    for r in recs if r["snaps"]]), 2),
    }
    return out


# ------------------------------------------------------------------- main

def collect(champ_spec, opp_spec, players, games, workers=None, seed0=0,
            move_cap=20000):
    tasks = [(seed0 + g // players * 7919 + 17, g % players) for g in range(games)]
    workers = workers or 1
    champ_recs, opp_recs, errors = [], [], []
    if workers <= 1:
        _init(champ_spec, opp_spec, players, move_cap)
        results = [_play(t) for t in tasks]
    else:
        with mp.Pool(workers, initializer=_init,
                     initargs=(champ_spec, opp_spec, players, move_cap)) as pool:
            results = pool.map(_play, tasks, chunksize=1)
    for res in results:
        if "error" in res:
            errors.append(res["error"])
            continue
        for r in res["recs"]:
            (champ_recs if r["is_champ"] else opp_recs).append(r)
    return champ_recs, opp_recs, errors


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--players", type=int, default=3)
    ap.add_argument("--games", type=int, default=60)
    ap.add_argument("--workers", type=int, default=1)
    ap.add_argument("--champion", default=None)
    ap.add_argument("--opponent", default="self",
                    help="'self' (mirror), 'default', 'greedy', 'random' or a "
                         "path to a weight file")
    ap.add_argument("--out", default=None)
    ap.add_argument("--seed0", type=int, default=90000)
    a = ap.parse_args(argv)

    champ_path = a.champion or f"experiments/champion_{a.players}p.json"
    champ = load_spec(champ_path)
    opp = champ if a.opponent == "self" else load_spec(a.opponent)

    champ_recs, opp_recs, errors = collect(
        champ, opp, a.players, a.games, a.workers, a.seed0)

    doc = {
        "players": a.players,
        "games": a.games,
        "champion": champ_path,
        "opponent": a.opponent,
        "errors": errors[:5],
        "error_count": len(errors),
        "champion_behaviour": _summarize_group(champ_recs, "champion"),
        "opponent_behaviour": _summarize_group(opp_recs, a.opponent),
    }
    out = a.out or f"experiments/behaviour_{a.players}p.json"
    tmp = out + ".tmp"
    with open(tmp, "w") as fh:
        json.dump(doc, fh, indent=1, sort_keys=True)
    os.replace(tmp, out)
    cb = doc["champion_behaviour"]
    print(f"{a.players}p: {len(champ_recs)} champion games, "
          f"{len(errors)} engine errors, win share "
          f"{cb.get('win_share')}, wrote {out}")
    return doc


if __name__ == "__main__":
    main()
