"""Per-system behavioural census: does the bot ever touch each subsystem?

`tools/behaviour_counts.py` counts four move classes and `tools/bgo_botmatch.py`
emits the human-comparable TSV.  Neither answers "which wonders are NEVER
built", "does it reach an Age III government", "does an Age III event ever get
revealed", or "which colony cards are unreachable" -- questions about card
IDENTITY and about systems (defence, military discard, pacts) that have no
column in either tool.  This adds them, on top of the same seat-wrapper
technique bgo_botmatch uses, so the two are directly comparable.

    nice -n 15 python3 tools/system_census.py --players 2 --games 60 \
        --spec plan:experiments/league_state/champion_2p.json,width=2,det=1 \
        --out /tmp/sys_2p.json

Every seat runs the same spec (mirror).  All taps check `state is real` before
recording, because the search copies the state and calls the same engine
functions on the copy -- counting those would measure the search, not the game
(the mistake `docs/CARD_CENSUS.md` had to fix in its discard probe).
"""
from __future__ import annotations

import argparse
import json
import os
import sys
from collections import Counter, defaultdict

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine import cards, game                                # noqa: E402
from experiments.arena import load_spec, make_bot             # noqa: E402

_DB = cards.db()

GOVERNMENTS = ("Despotism", "Monarchy", "Theocracy", "Republic",
               "Constitutional Monarchy", "Democracy", "Fundamentalism",
               "Communism")

#: colour buckets for "techs taken by colour".  The base game does not print
#: these words on the cards; this is the standard grouping and is stated in
#: docs/SYSTEM_COVERAGE.md so the number means one thing.
COLOUR = {}
for _t in ("farm", "mine"):
    COLOUR[_t] = "yellow"
for _t in ("lab", "temple", "library", "arena", "theater"):
    COLOUR[_t] = "blue"
for _t in ("infantry", "cavalry", "artillery", "air"):
    COLOUR[_t] = "red"
COLOUR["special-tech"] = "green"


class Rec:
    """Wraps one seat's bot; sees only REAL decisions."""

    def __init__(self, bot, idx):
        self.bot = bot
        self.idx = idx
        self.c = Counter()
        self.names = defaultdict(Counter)   # bucket -> card name -> n
        self.gov_path = []
        self.wonders_started = []

    def _note(self, state, mv):
        if not mv:
            return mv
        k = mv[0]
        p = state.players[self.idx]
        pend = state.pending[-1] if state.pending else None
        self.c["mv:" + k] += 1
        if pend:
            tag = pend.get("tag") if pend["kind"] == "choice" else pend["kind"]
            self.c["pend:%s" % tag] += 1
            if pend["kind"] == "defense":
                if k == "defend":
                    self.c["defend_card_spent"] += 1
                else:
                    self.c["defend_done"] += 1
        if k == "take":
            name = state.card_row[mv[1]]
            card = _DB.get(name) or {}
            typ = card.get("type")
            self.c["take:%s" % typ] += 1
            self.c["take_age:%s" % card.get("age")] += 1
            col = COLOUR.get(typ)
            if col:
                self.c["tech:%s" % col] += 1
                self.c["tech:%s:%s" % (col, card.get("age"))] += 1
            if typ == "wonder":
                self.names["wonder_taken"][name] += 1
            elif typ == "leader":
                self.names["leader_taken"][name] += 1
            elif typ == "government":
                self.names["gov_taken"][name] += 1
            elif typ in COLOUR:
                self.names["tech_taken"][name] += 1
        elif k == "wonder_step":
            w = p.wonder.name if p.wonder else None
            if w:
                if not self.wonders_started or self.wonders_started[-1] != w:
                    self.wonders_started.append(w)
                self.names["wonder_stage"][w] += mv[1] if len(mv) > 1 else 1
            self.c["wonder_steps"] += 1
        elif k == "revolution":
            self.gov_path.append(("rev", state.round, mv[1]))
            self.names["gov_change"][mv[1]] += 1
        elif k == "develop" and mv[1] in GOVERNMENTS:
            self.gov_path.append(("dev", state.round, mv[1]))
            self.names["gov_change"][mv[1]] += 1
        elif k == "play_leader":
            self.names["leader_played"][mv[1]] += 1
        elif k == "war":
            self.names["war_declared"][mv[1]] += 1
        elif k == "aggression":
            self.names["aggression_played"][mv[1]] += 1
        elif k == "prepare_event":
            card = _DB.get(mv[1]) or {}
            self.c["prep:%s" % card.get("type")] += 1
            self.c["prep_age:%s" % card.get("age")] += 1
        elif k == "offer_pact":
            self.names["pact_offered"][mv[1]] += 1
        elif k == "play_action":
            self.names["action_played"][mv[1]] += 1
        elif k == "play_tactic":
            self.names["tactic_played"][mv[1]] += 1
        elif k == "destroy":
            card = _DB.get(mv[1]) or {}
            if card.get("type") in ("infantry", "cavalry", "artillery", "air"):
                self.c["disband_unit"] += 1
        return mv

    def choose(self, state, moves, rng=None):
        return self._note(state, self.bot.choose(state, moves, rng))

    def __call__(self, state):
        return self._note(state, self.bot(state))


class Taps:
    """Patch the four engine entry points that carry an OUTCOME."""

    def __init__(self):
        self.real = None
        self.reset()

    def reset(self):
        self.c = Counter()
        self.names = defaultdict(Counter)
        self.war_rows = []
        self.aggr_rows = []

    def __enter__(self):
        from engine import events, interact, effects
        self._orig = (events.resolve_war, events.finish_aggression,
                      events.resolve_event, interact.start_auction,
                      interact.start_defense)
        tap = self

        def war(state, attacker, rng):
            war_t = attacker.war_declared_by_me
            live = war_t is not None and state is tap.real
            if live:
                name, _a, target = war_t
                defender = state.players[target]
                a = (effects.state_stats(state, attacker).strength
                     + effects.pact_attack_bonus(state, attacker, defender))
                d = effects.state_stats(state, defender).strength
                tap.war_rows.append((attacker.idx, target, name, a, d))
            return tap._orig[0](state, attacker, rng)

        def aggr(state, ctx, rng):
            live = state is tap.real
            out = tap._orig[1](state, ctx, rng)
            if live:
                tap.aggr_rows.append((ctx["attacker"], ctx["player"],
                                      ctx["card"], bool(out)))
            return out

        def ev(state, name, rng, revealer_idx):
            if state is tap.real:
                card = _DB.get(name) or {}
                tap.c["revealed:%s" % card.get("type")] += 1
                tap.c["revealed_age:%s" % card.get("age")] += 1
                tap.names["revealed"][name] += 1
            return tap._orig[2](state, name, rng, revealer_idx)

        def auc(state, name, revealer_idx, rng=None):
            if state is tap.real:
                tap.c["auction_started"] += 1
                tap.names["auction"][name] += 1
            return tap._orig[3](state, name, revealer_idx, rng)

        def dfn(state, attacker, defender, name, atk_strength, rng=None):
            if state is tap.real:
                tap.c["defense_started"] += 1
            return tap._orig[4](state, attacker, defender, name,
                                atk_strength, rng)

        events.resolve_war = war
        events.finish_aggression = aggr
        events.resolve_event = ev
        interact.start_auction = auc
        interact.start_defense = dfn
        return self

    def __exit__(self, *exc):
        from engine import events, interact
        (events.resolve_war, events.finish_aggression, events.resolve_event,
         interact.start_auction, interact.start_defense) = self._orig
        return False


def run(spec, players, ngames, seed0, out):
    tot = Counter()
    names = defaultdict(Counter)
    per_game = []
    taps = Taps()
    for g in range(ngames):
        with taps:
            taps.reset()
            recs = [Rec(make_bot(spec, 1000 + i), i) for i in range(players)]
            seed = (seed0 + g) * 7919 + 17
            st = game.new_game(players, seed)
            taps.real = st
            game.play_game(recs, num_players=players, seed=seed,
                           move_cap=20000, state=st)
            tot["games"] += 1
            tot["seats"] += players
            tot["rounds"] += st.round
            tot["final_age:%s" % st.age_civil] += 1
            for k, v in taps.c.items():
                tot[k] += v
            for b, cc in taps.names.items():
                names[b].update(cc)
            for (ai, di, nm, a, d) in taps.war_rows:
                tot["war_resolved"] += 1
                tot["war_att_won" if a > d else
                    ("war_draw" if a == d else "war_att_lost")] += 1
                names["war_resolved"][nm] += 1
            for (ai, di, nm, ok) in taps.aggr_rows:
                tot["aggr_resolved"] += 1
                tot["aggr_succeeded" if ok else "aggr_held_off"] += 1
            for r in recs:
                p = st.players[r.idx]
                for k, v in r.c.items():
                    tot[k] += v
                # WHICH route to a new government (RULES_SPEC 8.2 vs 8.3).
                # `gov_path` recorded both from the day it was written and
                # nothing ever aggregated it, so "government changes" could be
                # counted but the peaceful/revolution split could not.
                for kind, _rnd, _nm in r.gov_path:
                    tot["gov_revolution" if kind == "rev"
                        else "gov_peaceful"] += 1
                for b, cc in r.names.items():
                    names[b].update(cc)
                comp = list(getattr(p, "completed_wonders", ()) or ())
                tot["wonders_completed"] += len(comp)
                for w in comp:
                    names["wonder_completed"][w] += 1
                started = list(dict.fromkeys(r.wonders_started))
                tot["wonders_started"] += len(started)
                if p.wonder is not None:
                    tot["wonder_unfinished_at_end"] += 1
                tot["wonders_abandoned"] += max(
                    0, len(started) - len(comp) - (1 if p.wonder else 0))
                cols = list(getattr(p, "colonies", ()) or ())
                tot["colonies_held"] += len(cols)
                for cname in cols:
                    names["colony_held"][
                        cname if isinstance(cname, str)
                        else getattr(cname, "name", str(cname))] += 1
                gv = getattr(p, "government", None)
                gname = gv if isinstance(gv, str) else getattr(gv, "name", None)
                if gname:
                    names["gov_final"][gname] += 1
                lead = getattr(p, "leader", None)
                if lead:
                    names["leader_final"][
                        lead if isinstance(lead, str)
                        else getattr(lead, "name", str(lead))] += 1
                pacts = list(getattr(p, "pacts", ()) or ())
                tot["pacts_held"] += len(pacts)
                tot["score"] += (st.final_scores or
                                 [q.culture for q in st.players])[r.idx]
            per_game.append(st.round)
        sys.stderr.write("game %d/%d rounds=%d age=%s\n"
                         % (g + 1, ngames, st.round, st.age_civil))
        sys.stderr.flush()
    blob = {"spec": str(spec)[:80], "players": players, "games": ngames,
            "totals": dict(tot),
            "names": {k: dict(v) for k, v in names.items()}}
    fh = sys.stdout if out == "-" else open(out, "w")
    json.dump(blob, fh, indent=1, sort_keys=True)
    if fh is not sys.stdout:
        fh.close()


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--spec", required=True)
    ap.add_argument("--players", type=int, default=2)
    ap.add_argument("--games", type=int, default=40)
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--out", default="-")
    a = ap.parse_args(argv)
    run(load_spec(a.spec), a.players, a.games, a.seed, a.out)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
