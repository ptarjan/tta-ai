"""Where does culture actually leak away?

The behaviour harvest records production *rates* but not the penalties charged
during the production phase.  This replays champion mirror games with
`economy.end_of_turn` wrapped, so each turn we can compare the culture the
player's rating said it should score against the culture it actually scored.
The gap is starvation (4 culture per missing food, RULES_SPEC §6.6) or an
uprising (whole production phase skipped, §6.6 step 2).

Owned by the heuristics-doc agent.  Read-only over engine/ and experiments/;
it only wraps functions at runtime, it does not edit them.

Usage:  python3 analysis/leak_check.py [--games 24] [--players 2 3 4]
"""
from __future__ import annotations

import argparse
import os
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, ROOT)

from engine import economy, effects  # noqa: E402
from experiments.arena import load_spec, make_bot  # noqa: E402

_ORIG_END = economy.end_of_turn


def install(tally):
    """The bot's 1-ply search also calls end_of_turn on *cloned* states, so we
    key every tally on id(state) and afterwards keep only the id of the state
    `play_game` actually returned.  The real state is alive for the whole game,
    so no clone can reuse its id."""
    def end_of_turn(state, p, rng):
        try:
            idx = state.players.index(p)
        except ValueError:
            return _ORIG_END(state, p, rng)
        rec = tally.setdefault((id(state), idx), blank())
        s = effects.state_stats(state, p)
        rebels = economy.uprising(state, p)
        cul_before = p.culture
        sci_before = p.science
        # §6.6 step 1 (the military discard) is a decision, so end_of_turn
        # SUSPENDS and is re-entered once per discard; only the entry that
        # returns True ran the production phase.  Tally on that one alone, or
        # every counter here double-counts a turn with discards in it.
        done = _ORIG_END(state, p, rng)
        if not done:
            return done
        rec["turns"] += 1
        rec["age_turns"][state.age_civil] = rec["age_turns"].get(state.age_civil, 0) + 1
        if rebels:
            rec["uprisings"] += 1
            rec["uprising_culture"] += s.culture
            rec["uprising_science"] += s.science
        if not rebels:
            gained = p.culture - cul_before
            gap = s.culture - gained          # >0 means culture was burned
            if gap > 0:
                rec["starve_culture"] += gap
                rec["starve_turns"] += 1
                rec["starve_by_age"][state.age_civil] = (
                    rec["starve_by_age"].get(state.age_civil, 0) + gap)
            elif gap < 0:
                rec["bonus_culture"] += -gap  # leaders etc. paying out
            rec["science_gained"] += p.science - sci_before
        return done
    economy.end_of_turn = end_of_turn


def blank():
    return {"turns": 0, "uprisings": 0, "uprising_culture": 0,
            "uprising_science": 0, "starve_culture": 0, "starve_turns": 0,
            "bonus_culture": 0, "science_gained": 0, "starve_by_age": {},
            "age_turns": {}}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--games", type=int, default=24)
    ap.add_argument("--players", type=int, nargs="*", default=[2, 3, 4])
    a = ap.parse_args()
    from engine import game

    for n in a.players:
        champ = f"/tmp/beh_champ_{n}p.json"
        if not os.path.exists(champ):
            champ = os.path.join(ROOT, "experiments", f"champion_{n}p.json")
        spec = load_spec(champ)
        agg = blank()
        finals = []
        for seed in range(a.games):
            tally = {}
            install(tally)
            bots = [make_bot(spec, seed * 97 + i * 13 + 1) for i in range(n)]
            st = game.play_game(bots, n, seed=seed)
            finals.extend(game.scores(st))
            real = [tally.get((id(st), i), blank()) for i in range(n)]
            for r in real:
                for k, v in r.items():
                    if isinstance(v, dict):
                        for kk, vv in v.items():
                            agg[k][kk] = agg[k].get(kk, 0) + vv
                    else:
                        agg[k] += v
        economy.end_of_turn = _ORIG_END
        seats = a.games * n
        print(f"== {n}p, {a.games} games, {seats} player-games, "
              f"{agg['turns']} player-turns, mean final culture "
              f"{sum(finals)/len(finals):.1f}")
        print(f"   culture burned to starvation : {agg['starve_culture']/seats:7.2f} "
              f"per player-game  ({agg['starve_turns']/max(1,agg['turns']):.1%} of turns)")
        print(f"   culture lost to uprisings    : {agg['uprising_culture']/seats:7.2f} "
              f"per player-game  ({agg['uprisings']/max(1,agg['turns']):.2%} of turns)")
        print(f"   science lost to uprisings    : {agg['uprising_science']/seats:7.2f}")
        print(f"   extra culture from card effects: {agg['bonus_culture']/seats:7.2f}")
        by = agg["starve_by_age"]
        tot = agg["age_turns"]
        print("   starvation culture by age    : " + ", ".join(
            f"{k}={by.get(k,0)/seats:.1f} ({by.get(k,0)/max(1,tot.get(k,1)):.2f}/turn)"
            for k in ("A", "I", "II", "III", "IV")))


if __name__ == "__main__":
    main()
