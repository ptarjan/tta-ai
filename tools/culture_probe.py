"""Instrument champion-vs-CultureBot games: who attacks whom, and why not.

Diagnostic for docs/CULTURE_GAP.md.  Not part of the training loop.

Per decision of every seat we record:
  * whether an ``aggression`` / ``war`` move was legal at all, and against whom;
  * what the seat actually chose;
  * for the champion seat only, the 1-ply eval of the chosen move, of
    ``pol_pass``, and of the best attack -- the same probe
    docs/AGGRESSION_FIX.md B used, but against a culture opponent and with
    the CURRENT champion weights.

Usage:
    python3 tools/culture_probe.py --players 4 --games 12 \
        --champ experiments/league_state/champion_4p.json --out /tmp/probe_4p.json
"""
from __future__ import annotations

import argparse
import json
import os
import random
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine import actions, effects, game, journal          # noqa: E402
from engine.bots.weighted import WeightedBot, evaluate, load_weights, \
    rival_context                                            # noqa: E402
from engine.bots.trial import USE_JOURNAL, fresh_trial_rng   # noqa: E402
from engine.bots.fastcopy import copy_state                  # noqa: E402
from engine.bots.variants.culture import CultureBot          # noqa: E402


def score_move(state, mv, idx, w, ctx):
    """1-ply eval of one candidate, exactly as WeightedBot scores it."""
    if USE_JOURNAL:
        j = journal.begin(state)
        try:
            actions.apply(state, mv, fresh_trial_rng())
            return evaluate(state, idx, w, ctx)
        except Exception:
            return None
        finally:
            journal.rollback(j)
    trial = copy_state(state)
    try:
        actions.apply(trial, mv, fresh_trial_rng())
        return evaluate(trial, idx, w, ctx)
    except Exception:
        return None


class Watch:
    """Wraps a bot, records what was legal and what was picked."""

    def __init__(self, bot, seat, rec, weights=None, force=False):
        self.bot, self.seat, self.rec, self.w = bot, seat, rec, weights
        self.force = force

    def __call__(self, state):
        moves = actions.legal_moves(state)
        r = self.rec
        aggs = [m for m in moves if m[0] == "aggression"]
        wars = [m for m in moves if m[0] == "war"]
        is_politics = any(m[0] in ("pol_pass", "aggression", "war",
                                   "prepare_event", "offer_pact") for m in moves)
        if is_politics:
            r["politics"] += 1
            if aggs:
                r["agg_legal"] += 1
            if wars:
                r["war_legal"] += 1

        # `--force war`: an ORACLE overlay, not a bot.  Whenever a war (else
        # an aggression) against the current culture leader is legal, take
        # it.  This is the counterfactual "what if the champion did the thing
        # culture.py's docstring says should punish it", and it is also the
        # end-to-end proof that the engine's war path works in real play.
        if self.force and (wars or aggs):
            rivals = [(q.culture, q.idx) for q in state.players
                      if q.idx != self.seat and not q.resigned]
            if rivals:
                leader = max(rivals)[1]
                pick = ([m for m in wars if m[2] == leader]
                        or [m for m in aggs if m[2] == leader])
                if pick:
                    mv = pick[0]
                    r["forced"] += 1
                    self._note(state, mv, r)
                    return mv

        mv = self.bot(state)

        self._note(state, mv, r)

        # eval probe: only for the weighted seat, only where an attack existed
        if self.w is not None and (aggs or wars) and len(moves) > 1:
            idx = state.decider()
            try:
                ctx = rival_context(state, idx)
            except Exception:
                ctx = {"rival_culture_rate": 0, "rival_science_rate": 0,
                       "rival_strength": 0}
            leader = max((q.culture, q.idx) for q in state.players
                         if q.idx != idx)[1]
            best_att, best_att_v, best_att_leader_v = None, None, None
            for m in aggs + wars:
                v = score_move(state, m, idx, self.w, ctx)
                if v is None:
                    continue
                if best_att_v is None or v > best_att_v:
                    best_att, best_att_v = m, v
                if m[2] == leader and (best_att_leader_v is None
                                       or v > best_att_leader_v):
                    best_att_leader_v = v
            chosen_v = score_move(state, mv, idx, self.w, ctx)
            if mv[0] == "end_turn":
                chosen_v = (chosen_v or 0) + self.w.get("end_turn_bias", 0.0)
            pp = [m for m in moves if m[0] == "pol_pass"]
            pass_v = score_move(state, pp[0], idx, self.w, ctx) if pp else None
            if best_att_v is not None and chosen_v is not None:
                r["probe_n"] += 1
                r["probe_gap"] += chosen_v - best_att_v
                if best_att_v >= chosen_v:
                    r["probe_attack_wins"] += 1
                if pass_v is not None and best_att_v > pass_v:
                    r["probe_attack_beats_pass"] += 1
                if best_att_leader_v is not None:
                    r["probe_leader_n"] += 1
                    r["probe_leader_gap"] += chosen_v - best_att_leader_v
                r.setdefault("probe_samples", [])
                if len(r["probe_samples"]) < 8:
                    r["probe_samples"].append({
                        "round": state.round, "chosen": list(map(str, mv)),
                        "chosen_v": round(chosen_v, 3),
                        "best_attack": list(map(str, best_att)),
                        "best_attack_v": round(best_att_v, 3),
                        "pol_pass_v": (None if pass_v is None
                                       else round(pass_v, 3)),
                    })
        return mv

    def _note(self, state, mv, r):
        if mv[0] not in ("aggression", "war"):
            return
        tgt = mv[2]
        r[mv[0] + "_played"] += 1
        r.setdefault(mv[0] + "_targets", {})
        r[mv[0] + "_targets"][str(tgt)] = \
            r[mv[0] + "_targets"].get(str(tgt), 0) + 1
        cults = [(q.culture, q.idx) for q in state.players
                 if q.idx != self.seat]
        if cults and tgt == max(cults)[1]:
            r[mv[0] + "_on_leader"] += 1
        r.setdefault("attacks", []).append([state.round, mv[0], mv[1], tgt])


def blank():
    return {"forced": 0, "politics": 0, "agg_legal": 0, "war_legal": 0,
            "aggression_played": 0, "war_played": 0,
            "aggression_on_leader": 0, "war_on_leader": 0,
            "probe_n": 0, "probe_gap": 0.0, "probe_attack_wins": 0,
            "probe_attack_beats_pass": 0,
            "probe_leader_n": 0, "probe_leader_gap": 0.0}


def merge(dst, src):
    for k, v in src.items():
        if isinstance(v, (int, float)):
            dst[k] = dst.get(k, 0) + v
        elif isinstance(v, dict):
            d = dst.setdefault(k, {})
            for kk, vv in v.items():
                d[kk] = d.get(kk, 0) + vv
        elif isinstance(v, list):
            dst.setdefault(k, []).extend(v[:8])


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--players", type=int, default=4)
    ap.add_argument("--games", type=int, default=12)
    ap.add_argument("--champ", default=None)
    ap.add_argument("--opp", default="culture",
                    help="culture|self  (self = champion mirror)")
    ap.add_argument("--seed0", type=int, default=70000)
    ap.add_argument("--patch-rival-culture", type=float, default=None,
                    help="override the champion's rival_culture weight")
    ap.add_argument("--force", default="none", choices=("none", "war"),
                    help="oracle overlay: always attack the culture leader")
    ap.add_argument("--out", default=None)
    a = ap.parse_args()

    k = a.players
    champ = a.champ or f"experiments/league_state/champion_{k}p.json"
    w = load_weights(champ)
    if a.patch_rival_culture is not None:
        w["rival_culture"] = a.patch_rival_culture

    tot_c, tot_o = blank(), blank()
    wins = 0.0
    cul_c, cul_o, str_c, str_o = [], [], [], []
    games = 0
    warlog = []
    for gi in range(a.games):
        seat = gi % k
        seed = a.seed0 + gi
        rc, ro = blank(), blank()
        bots = []
        for i in range(k):
            if i == seat:
                bots.append(Watch(WeightedBot(weights=w, seed=seed * 97 + i),
                                  i, rc, weights=w,
                                  force=(a.force == "war")))
            else:
                inner = (WeightedBot(weights=w, seed=seed * 97 + i)
                         if a.opp == "self" else CultureBot())
                bots.append(Watch(inner, i, ro))
        try:
            st = game.play_game(bots, k, seed=seed)
        except Exception as e:            # noqa: BLE001
            print(f"game {gi} failed: {e!r}", file=sys.stderr)
            continue
        games += 1
        sc = game.scores(st)
        best = max(sc)
        tied = [i for i, v in enumerate(sc) if v == best]
        wins += (1.0 / len(tied)) if seat in tied else 0.0
        cul_c.append(sc[seat])
        cul_o.extend(sc[i] for i in range(k) if i != seat)
        for i in range(k):
            s = effects.compute(st, st.players[i])
            (str_c if i == seat else str_o).append(s.strength)
        # engine-level proof: did declared wars actually RESOLVE, and who won?
        for line in getattr(st, "log", ()) or ():
            if line.startswith("war ") or line.startswith("aggression "):
                warlog.append(line)
        merge(tot_c, rc)
        merge(tot_o, ro)
        print(f"  game {gi} seat {seat}: scores={sc} "
              f"champ_att={rc['aggression_played']}a/{rc['war_played']}w "
              f"opp_att={ro['aggression_played']}a/{ro['war_played']}w",
              flush=True)

    def per(d, n):
        return {kk: (round(vv / n, 3) if isinstance(vv, (int, float)) else vv)
                for kk, vv in d.items()
                if kk not in ("probe_samples", "attacks")}

    out = {
        "players": k, "games": games, "champ": champ, "opp": a.opp,
        "patch_rival_culture": a.patch_rival_culture,
        "win_rate": round(wins / max(1, games), 4),
        "null": round(1.0 / k, 4),
        "culture_champ": round(sum(cul_c) / max(1, len(cul_c)), 2),
        "culture_opp": round(sum(cul_o) / max(1, len(cul_o)), 2),
        "strength_champ": round(sum(str_c) / max(1, len(str_c)), 2),
        "strength_opp": round(sum(str_o) / max(1, len(str_o)), 2),
        "champ_per_game": per(tot_c, max(1, games)),
        "opp_per_game_per_seat": per(tot_o, max(1, games * (k - 1))),
        "champ_probe_mean_gap": (round(tot_c["probe_gap"]
                                       / tot_c["probe_n"], 3)
                                 if tot_c["probe_n"] else None),
        "champ_probe_leader_gap": (round(tot_c["probe_leader_gap"]
                                         / tot_c["probe_leader_n"], 3)
                                   if tot_c["probe_leader_n"] else None),
        "probe_samples": tot_c.get("probe_samples", [])[:8],
        "warlog": warlog[:40],
        "warlog_n": len(warlog),
        "champ_attacks": tot_c.get("attacks", [])[:20],
        "opp_attacks": tot_o.get("attacks", [])[:20],
    }
    print(json.dumps(out, indent=1))
    if a.out:
        with open(a.out, "w") as fh:
            json.dump(out, fh, indent=1)


if __name__ == "__main__":
    main()
