"""How often can `War over Technology` actually offer its choice?

This is the CONDUCTION measurement for docs/WAR_OVER_TECHNOLOGY.md, and it is
meant to be run BEFORE spending games on an A/B rather than as an explanation
afterwards.  The lever is a decision that only exists in a narrow conjunction:

  1. a war card is DECLARED at all (bots essentially never declare wars --
     docs/CULTURE_GAP.md; tests/test_combat.py says so in its docstring),
  2. the card is `War over Technology` specifically, not the other two wars,
  3. it resolves with UNEQUAL strength (a draw moves nothing, CoL p.3),
  4. and the loser holds a blue special technology in play that the victor
     holds neither in play nor in hand (CoL p.3's exclusion) whose PRINTED
     cost fits inside the strength advantage (FAQ p.8's "as long as you win
     enough Science points").

Every one of those four is counted separately, so a null A/B can be attributed
to the step that actually failed instead of to "wars are rare" in general.

The choice itself is answered with SCIENCE at every firing, which is the
pre-change engine's behaviour -- so this tool measures the decision's
frequency without perturbing the games it measures.

    nice -n 19 python3 -m tools.wartech_census --games 60 --players 4
    nice -n 19 python3 -m tools.wartech_census --games 60 --players 3 \
        --spec weighted:analysis/frozen/champion_3p_gen1255_99key.json
"""
from __future__ import annotations

import argparse
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine import cards as C, events, game, interact       # noqa: E402
from experiments.arena import load_spec, make_bot           # noqa: E402

TECH_WAR = "War over Technology"


def instrument(tally):
    """Wrap the engine so every stage of the conjunction above is counted.

    Wrapping rather than replaying: `resolve_war` is the only place the four
    conditions are all in scope at once, and `war_tech_options` is the engine's
    own answer to condition 4 -- so the census cannot drift from the rule the
    way a re-implementation would.
    """
    orig_resolve = events.resolve_war
    orig_opts = interact.war_tech_options

    def resolve_war(state, attacker, rng):
        war = attacker.war_declared_by_me
        if war:
            name = war[0]
            tally["declared_resolved"] += 1
            base = C.db().get(name).get("baseName", name) \
                if name in C.db().by_name else name
            if base == TECH_WAR:
                tally["tech_wars"] += 1
        return orig_resolve(state, attacker, rng)

    def war_tech_options(state, victor, loser, budget):
        out = orig_opts(state, victor, loser, budget)
        if tally["_in_offer"] == 0:
            # the FIRST offer of a resolution: the war was not a draw, so
            # condition 3 held
            tally["undrawn"] += 1
            if out:
                tally["with_options"] += 1
                tally["first_options"] += len(out)
            else:
                # why not?  separate "loser had no blue at all" from
                # "everything they had was excluded or unaffordable"
                blues = [n for n in loser.techs
                         if C.db().type_of(n) == "special-tech"]
                if not blues:
                    tally["no_blue_in_play"] += 1
                else:
                    tally["all_excluded_or_unaffordable"] += 1
        tally["_in_offer"] += 1
        return out

    events.resolve_war = resolve_war
    interact.war_tech_options = war_tech_options
    return orig_resolve, orig_opts


class TakeScience:
    """Answer every `war_tech` choice with science: the pre-change engine."""

    def __init__(self, inner, idx):
        self.inner = inner
        self.idx = idx

    def __call__(self, state):
        pend = state.pending[-1] if state.pending else None
        if (pend and pend.get("kind") == "choice"
                and pend.get("tag") == "war_tech"
                and pend.get("player") == self.idx):
            return ("choose", interact.WAR_TECH_SCIENCE_IDX)
        return self.inner(state)


def _default_bot(seed):
    from engine.bots import WeightedBot
    return WeightedBot(seed=seed)


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--spec", default=None)
    ap.add_argument("--games", type=int, default=60)
    ap.add_argument("--players", type=int, default=4)
    ap.add_argument("--seed", type=int, default=7000)
    a = ap.parse_args(argv)
    spec = load_spec(a.spec) if a.spec else None
    t = {"declared_resolved": 0, "tech_wars": 0, "undrawn": 0,
         "with_options": 0, "first_options": 0, "no_blue_in_play": 0,
         "all_excluded_or_unaffordable": 0, "_in_offer": 0}
    instrument(t)
    games_with = 0
    for g in range(a.games):
        seed = a.seed + g
        before = t["with_options"]
        t["_in_offer"] = 0
        bots = []
        for i in range(a.players):
            inner = (make_bot(spec, 1000 + i) if spec
                     else _default_bot(1000 + i))
            bots.append(TakeScience(inner, i))
        # `_in_offer` must reset per RESOLUTION, not per game; the wrapper
        # above zeroes it whenever a fresh war resolves.
        _patch_reset(t)
        game.play_game(bots, num_players=a.players, seed=seed,
                       move_cap=20000)
        if t["with_options"] > before:
            games_with += 1
    n = a.games
    print(f"{n} games at {a.players}p"
          + (f", spec={a.spec}" if a.spec else ", WeightedBot defaults"))
    print(f"  wars resolved                      : {t['declared_resolved']}"
          f"  ({t['declared_resolved']/n:.3f}/game)")
    print(f"  of which {TECH_WAR:<25}: {t['tech_wars']}"
          f"  ({t['tech_wars']/n:.3f}/game)")
    print(f"  ... not a draw, so spoils moved    : {t['undrawn']}")
    print(f"  ... AND a steal was on offer       : {t['with_options']}"
          f"  ({t['with_options']/n:.3f}/game)")
    if t["with_options"]:
        print(f"      mean technologies offered      : "
              f"{t['first_options']/t['with_options']:.2f}")
    print(f"  no blue technology in the loser's play area: "
          f"{t['no_blue_in_play']}")
    print(f"  had one, but excluded or unaffordable      : "
          f"{t['all_excluded_or_unaffordable']}")
    print(f"  GAMES containing at least one decision     : {games_with}"
          f"/{n} ({games_with/n:.1%})")
    return 0


def _patch_reset(t):
    """Zero the per-resolution counter each time a war starts resolving."""
    orig = events.resolve_war

    def resolve_war(state, attacker, rng):
        t["_in_offer"] = 0
        return orig(state, attacker, rng)
    if getattr(events.resolve_war, "_census_reset", False):
        return
    resolve_war._census_reset = True
    events.resolve_war = resolve_war


if __name__ == "__main__":
    raise SystemExit(main())
