"""Play N self-play games and emit the SAME per-player TSV schema as
`tools/bgo_parse.py`, so bot and human rows go through one stats tool.

    nice -n 19 python3 tools/bgo_botmatch.py --players 2 --games 40 \
        --spec quiesce:/tmp/champ_2p.json,levels=1 --out /tmp/bot_2p.tsv

Every seat runs the same spec (mirror), which is the right comparison for
"what does this policy do when nobody is exploitable", and it is also what the
human corpus is -- humans against humans, not humans against a fixed foil.

The point of sharing a schema with the human parser is that a field can only be
compared if it is *derived the same way on both sides*.  Two places where that
took work:

* **Row-slot tier.**  The human side reconstructs the tier from the logged CA
  cost minus the wonder surcharge; here the tier is read straight off the move
  (`("take", idx)` -> `actions.row_cost(idx)`), which is the ground truth the
  human side is approximating.  Any bias therefore sits on the human side and
  is bounded by its `tier_unknown` count.
* **Wars.**  A human war is `declares` + a later `wins` line.  Here a war move
  is counted when chosen and its outcome read from the state diff (culture /
  science / yellow tokens moving), so both sides count *declarations*, not
  *resolutions*; wars still pending at game end are declared-not-resolved on
  both sides.

`sci_final` is the raw science total at game end.  On the human side it is the
last `now N science` printed on an end-turn line, which is the same quantity.
`score` is final culture after end-of-game scoring on both sides.
"""
from __future__ import annotations

import argparse
import os
import sys
from collections import Counter, defaultdict

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine import actions, cards, game                       # noqa: E402
from experiments.arena import (                               # noqa: E402
    load_spec, make_bot, refuse_if_degenerate_champion)
from tools.bgo_parse import FIELDS                            # noqa: E402

_DB = cards.db()
GOVERNMENTS = ("Despotism", "Monarchy", "Theocracy", "Republic",
               "Constitutional Monarchy", "Democracy", "Fundamentalism",
               "Communism")


class Recorder:
    """Wraps a bot for one seat and tallies exactly the human-corpus fields."""

    def __init__(self, bot, idx):
        self.bot = bot
        self.idx = idx
        self.c = Counter()
        self.gov_path = []
        self.wonders_started = set()
        self.war_targets = set()
        self.aggr_targets = set()
        self.wars = []          # (round, defender)

    def _note(self, state, mv):
        if not mv:
            return mv
        k = mv[0]
        p = state.players[self.idx]
        if k == "take":
            idx = mv[1]
            name = state.card_row[idx]
            self.c["takes"] += 1
            tier = actions.row_cost(idx)
            self.c["tier%d" % tier] += 1
            card = _DB.get(name) or {}
            if card.get("type") != "wonder":
                self.c["takes_nonwonder"] += 1
            ca = actions.take_cost(state, p, idx)
            if 1 <= ca <= 3:
                self.c["take_ca%d" % ca] += 1
            # per-age counts are keyed on the age the GAME is in, matching
            # tools/bgo_parse.py -- the human journal's card names are
            # age-ambiguous (see load_cards there), the game age is not.
            age = state.age_civil          # already "A"/"I"/"II"/"III"/"IV"
            if age in ("A", "I", "II", "III", "IV"):
                self.c["take_age" + age] += 1
            else:
                self.c["take_unknown_card"] += 1
        elif k == "war":
            self.c["wars_declared"] += 1
            self.wars.append((state.round, mv[2]))
            self.war_targets.add(mv[2])
        elif k == "aggression":
            self.c["aggressions"] += 1
            self.aggr_targets.add(mv[2])
        elif k == "wonder_step":
            w = p.wonder.name if p.wonder else None
            if w:
                self.wonders_started.add(w)
            self.c["wonder_stages"] += 1
        elif k == "revolution":
            self.gov_path.append((state.round, mv[1]))
        elif k == "develop" and mv[1] in GOVERNMENTS:
            self.gov_path.append((state.round, mv[1]))
        elif k == "bid":
            self.c["bids"] += 1
        elif k == "play_leader":
            self.c["leaders_elected"] += 1
        return mv

    def choose(self, state, moves, rng=None):
        return self._note(state, self.bot.choose(state, moves, rng))

    def __call__(self, state):
        return self._note(state, self.bot(state))


class WarTap:
    """Record every war resolution of the REAL game.

    `engine.events.resolve_war` is where attacker/defender strength is
    compared, and nothing else in the engine stores the result, so it is
    wrapped.  The search copies the state, and a deep enough search can call
    `resolve_war` on a copy; the tap therefore only records calls whose state
    object *is* the game state, which is why `arm()` takes it.
    """

    def __init__(self):
        self.real = None
        self.rows = []          # (attacker_idx, defender_idx, a_str, d_str)
        self._orig = None

    def __enter__(self):
        from engine import events
        self._orig = events.resolve_war
        tap = self

        def patched(state, attacker, rng):
            war = attacker.war_declared_by_me
            if war is not None and state is tap.real:
                from engine import effects
                _n, _a, target = war
                defender = state.players[target]
                a = (effects.state_stats(state, attacker).strength
                     + effects.pact_attack_bonus(state, attacker, defender))
                d = effects.state_stats(state, defender).strength
                tap.rows.append((attacker.idx, target, a, d))
            return tap._orig(state, attacker, rng)

        events.resolve_war = patched
        return self

    def __exit__(self, *exc):
        from engine import events
        events.resolve_war = self._orig
        return False


def run(spec, players, ngames, seed0, out):
    rows = []
    tap = WarTap()
    for g in range(ngames):
      with tap:
        recs = [Recorder(make_bot(spec, 1000 + i), i) for i in range(players)]
        tap.rows = []
        st = game.new_game(players, (seed0 + g) * 7919 + 17)
        tap.real = st
        game.play_game(recs, num_players=players,
                       seed=(seed0 + g) * 7919 + 17, move_cap=20000,
                       state=st)
        war_won = Counter()
        war_def_won = Counter()
        war_str_att = defaultdict(list)
        war_str_def = defaultdict(list)
        for (ai, di, a, d) in tap.rows:
            war_str_att[ai].append(a)
            war_str_def[di].append(d)
            if a > d:
                war_won[ai] += 1
            elif d > a:
                war_def_won[di] += 1
        scores = list(st.final_scores or [p.culture for p in st.players])
        ranked = sorted(range(players), key=lambda i: -scores[i])
        rank_of = {i: n + 1 for n, i in enumerate(ranked)}
        wars_def = Counter()
        for r in recs:
            for _rnd, d in r.wars:
                wars_def[d] += 1
        for r in recs:
            p = st.players[r.idx]
            i = r.idx
            nondesp = [(rd, gv) for rd, gv in r.gov_path if gv != "Despotism"]
            margin = (scores[ranked[0]] - scores[ranked[1]]) if rank_of[i] == 1 \
                else (scores[i] - scores[ranked[0]])
            row = {
                "game_id": "bot%04d" % g,
                "players": players,
                "level": "bot",
                "rounds": st.round,
                "final_age": st.age_civil,
                "colour": "P%d" % i,
                "score": scores[i],
                "rank": rank_of[i],
                "margin_vs_next": margin,
                "won": 1 if rank_of[i] == 1 else 0,
                "sci_final": p.science,
                "cul_journal_final": "",
                "wars_declared": r.c["wars_declared"],
                "wars_declared_won": war_won[i],
                "wars_declared_lost": r.c["wars_declared"] - war_won[i],
                "wars_defended": wars_def[i],
                "wars_defended_won": war_def_won[i],
                "war_str_att_mean": round(
                    sum(war_str_att[i]) / len(war_str_att[i]), 2)
                    if war_str_att[i] else "",
                "war_str_def_mean": round(
                    sum(war_str_def[i]) / len(war_str_def[i]), 2)
                    if war_str_def[i] else "",
                "aggressions": r.c["aggressions"],
                "aggr_targets_distinct": len(r.aggr_targets),
                "colonies": len(getattr(p, "colonies", ()) or ()),
                "bids": r.c["bids"],
                "wonders_started": len(r.wonders_started),
                "wonders_completed": len(getattr(p, "completed_wonders", ()) or ()),
                "wonder_stages": r.c["wonder_stages"],
                "gov_path": ">".join(gv for _rd, gv in r.gov_path),
                "gov_changes": len(nondesp),
                "first_gov": nondesp[0][1] if nondesp else "",
                "first_gov_round": nondesp[0][0] if nondesp else "",
                "takebacks": 0,
                "leaders_elected": r.c["leaders_elected"],
            }
            for k in ("takes", "takes_nonwonder", "take_ca1", "take_ca2",
                      "take_ca3", "tier1", "tier2", "tier3", "tier_unknown",
                      "takes_free", "take_ageA", "take_ageI", "take_ageII",
                      "take_ageIII", "take_ageIV", "take_unknown_card"):
                row[k] = r.c[k]
            rows.append(row)
        sys.stderr.write("game %d/%d rounds=%d scores=%s\n"
                         % (g + 1, ngames, st.round, scores))
        sys.stderr.flush()

    fh = sys.stdout if out == "-" else open(out, "w")
    fh.write("\t".join(FIELDS) + "\n")
    for r in rows:
        fh.write("\t".join(str(r.get(k, "")) for k in FIELDS) + "\n")
    if fh is not sys.stdout:
        fh.close()


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--spec", required=True)
    ap.add_argument("--players", type=int, default=2)
    ap.add_argument("--games", type=int, default=30)
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--out", default="-")
    a = ap.parse_args(argv)
    refuse_if_degenerate_champion(a.spec, "bgo_botmatch")
    run(load_spec(a.spec), a.players, a.games, a.seed, a.out)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
