"""Where does an aggression die: never drawn, never offered, or never chosen?

`docs/MILITARY_DISCARD.md` §6.3 reported **34 aggressions in 600 games (0.057
per game) and 0 ever successfully defended**, and concluded the defence
channel the discard rule would be paid through is absent.  That measurement
ran `tools/discard_ab.py --spec <bare path>`, and `arena.make_bot`'s
fallthrough turns a bare path into a **1-ply `WeightedBot`** -- so it is a
1-ply number, and `docs/CARD_CENSUS.md` already showed that war and aggression
are dead at 1 ply and partly repaired by search.  This tool asks the question
the census left open: what happens **per game**, under the search, and at every
player count -- and it splits the death into stages so a rate can be
attributed rather than just reported.

Four stages, each with the previous one as its denominator, counted only at
*real* decisions (the wrapper sits outside the bot, so trial states inside the
bot's own search are never counted):

1. **held**   -- a politics decision where the player holds an aggression card.
2. **offered** -- ... and `actions._politics_moves` actually listed an
   `("aggression", ...)` move.  The gap between 1 and 2 is the rules gate:
   §5.4 step 2 makes an aggression illegal unless the attacker *already* beats
   the defender, so a held-but-not-offered card is the RULES declining, not
   the policy.
3. **chosen** -- ... and the policy picked it over everything else.
4. **won**    -- ... and the defender failed to hold it off.

The defence side is split the same way, because "0 successfully defended" has
three very different explanations:

* **impossible** -- even spending every legal card, the defender cannot reach
  the attacker's strength.  `best_defense` computes that exactly: the top
  `budget - spent` `defense_points` in hand, added to the standing strength.
* **unattempted** -- it was reachable and the policy played `defend_done`.
* **rare** -- it was attempted and still fell short.

Only the second is a defect.  The first is §5.4 step 2 doing its job: the
attacker is only *allowed* to declare when already ahead, so the defender
starts every defence behind by construction and needs bonus cards to climb
back.

Usage:

    CENSUS=tools.aggression_census
    CHAMP=analysis/frozen/champion_2p.json
    python -m $CENSUS --spec $CHAMP --players 2 --games 200 \\
        --workers 10 --out agg_1ply_2p.json
    python -m $CENSUS --spec plan:$CHAMP,width=2 --players 2 --games 200 \\
        --workers 10 --out agg_plan_2p.json
"""
import argparse
import json
import os
import re
import sys
from multiprocessing import Pool

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from engine import actions, cards, effects, game   # noqa: E402
from engine.interact import defense_points         # noqa: E402
from experiments.arena import load_spec, make_bot  # noqa: E402

_AGG_RESOLVED = re.compile(r"aggression (.*) vs P(\d+) (failed|succeeded)")

KEYS = (
    # attack funnel
    "pol_decisions", "held", "offered", "chosen",
    "held_no_ma",          # held an aggression but had 0 military actions
    "held_ma_unoffered",   # held it, had the MA, and the rules still declined
    "war_held", "war_offered", "war_chosen",
    # defence funnel
    "def_first", "def_impossible", "def_reachable",
    "def_attempted", "def_attempted_reachable", "def_gave_up_reachable",
    "def_cards_played",
    # Reachable HOW: with one card (so `_defense_move` resolves the aggression
    # immediately and a 1-ply evaluator SEES the outcome) or only with two or
    # more (so the first `defend` leaves the decision on `state.pending` and
    # the outcome is invisible without a drain).  This is the discriminator
    # between a pricing problem and a plumbing one.
    "def_reach_1card", "def_reach_multi",
    "def_gave_up_1card", "def_gave_up_multi",
    # what the policy took INSTEAD, at decisions where an aggression was legal
    "instead_pass", "instead_event", "instead_war", "instead_pact",
    "instead_other",
    # why the defender never got a decision: interact.start_defense
    # short-circuits on `budget <= 0 or not defender.hand_military`
    "target_hand_empty", "target_no_ma", "target_had_a_say",
    # outcomes off the log
    "agg_resolved", "agg_succeeded", "agg_failed",
)


#: what the policy took instead, at a decision where an aggression was legal
_INSTEAD = {"pol_pass": "instead_pass", "prepare_event": "instead_event",
            "war": "instead_war", "offer_pact": "instead_pact"}


def blank():
    return dict.fromkeys(KEYS, 0)


class Watch:
    """Count the funnel at this seat's REAL decisions, then delegate."""

    def __init__(self, inner, idx, counts, db):
        self.inner = inner
        self.idx = idx
        self.c = counts
        self.db = db

    def _types(self, hand):
        return [self.db.type_by_name.get(n) for n in hand]

    def __call__(self, state):
        c = self.c
        p = state.players[self.idx]
        pend = state.pending[-1] if state.pending else None

        if pend is not None and pend.get("kind") == "defense" \
                and pend.get("player") == self.idx:
            room = pend["budget"] - pend["spent"]
            pts = sorted((defense_points(n) for n in p.hand_military),
                         reverse=True)
            best = pend["dfn"] + sum(pts[:max(0, room)])
            reachable = best >= pend["atk"]
            # one card is enough iff the single best card clears the gap AND
            # playing it ends the decision (`interact._defense_move` keeps the
            # pending only while `spent < budget` and the hand is non-empty).
            one_card = bool(pts) and pend["dfn"] + pts[0] >= pend["atk"] \
                and (pend["budget"] <= 1 or len(p.hand_military) <= 1)
            if pend["spent"] == 0:
                c["def_first"] += 1
                c["def_reachable" if reachable else "def_impossible"] += 1
                if reachable:
                    c["def_reach_1card" if one_card
                      else "def_reach_multi"] += 1
            mv = self.inner(state)
            if mv and mv[0] == "defend":
                c["def_cards_played"] += 1
                if pend["spent"] == 0:
                    c["def_attempted"] += 1
                    if reachable:
                        c["def_attempted_reachable"] += 1
            elif pend["spent"] == 0 and reachable:
                c["def_gave_up_reachable"] += 1
                c["def_gave_up_1card" if one_card
                  else "def_gave_up_multi"] += 1
            return mv

        moves = actions.legal_moves(state)
        offered_now = any(m[0] == "aggression" for m in moves)
        if any(m[0] == "pol_pass" for m in moves):
            c["pol_decisions"] += 1
            types = self._types(p.hand_military)
            has_agg = "aggression" in types
            has_war = "war" in types
            if has_agg:
                c["held"] += 1
                if p.military_actions <= 0:
                    c["held_no_ma"] += 1
            if has_war:
                c["war_held"] += 1
            if offered_now:
                c["offered"] += 1
            elif has_agg and p.military_actions > 0:
                c["held_ma_unoffered"] += 1
            if any(m[0] == "war" for m in moves):
                c["war_offered"] += 1

        mv = self.inner(state)
        if mv and mv[0] == "aggression":
            c["chosen"] += 1
            # Snapshot the target BEFORE the engine applies the move, so this
            # is the hand `interact.start_defense` is about to test.
            d = state.players[mv[2]]
            budget = effects.state_stats(state, d).military_actions
            if not d.hand_military:
                c["target_hand_empty"] += 1
            elif budget <= 0:
                c["target_no_ma"] += 1
            else:
                c["target_had_a_say"] += 1
        elif mv and mv[0] == "war":
            c["war_chosen"] += 1
        if offered_now and mv and mv[0] != "aggression":
            c[_INSTEAD.get(mv[0], "instead_other")] += 1
        return mv


_W = {}


def _init(spec, players):
    _W["spec"], _W["n"] = spec, players
    _W["db"] = cards.db()


def _play(seed):
    n, db = _W["n"], _W["db"]
    c = blank()
    bots = [Watch(make_bot(_W["spec"], 1000 + i), i, c, db) for i in range(n)]
    st = game.new_game(n, seed)
    game.play_game(bots, num_players=n, seed=seed, move_cap=20000, state=st)
    for line in st.log:
        m = _AGG_RESOLVED.search(line)
        if m:
            c["agg_resolved"] += 1
            c["agg_succeeded" if m.group(3) == "succeeded"
              else "agg_failed"] += 1
    return c


def run(spec, players, games, seed0, workers):
    seeds = [(seed0 + g) * 7919 + 17 for g in range(games)]
    total = blank()
    with Pool(workers, initializer=_init, initargs=(spec, players)) as pool:
        for c in pool.imap_unordered(_play, seeds, chunksize=1):
            for k in KEYS:
                total[k] += c[k]
    return total


def report(total, games, label):
    t = total
    per = (lambda k: t[k] / games)
    lines = [
        f"--- {label}: {games} games ---",
        f"aggressions resolved   {t['agg_resolved']:6d}   "
        f"{per('agg_resolved'):.3f} / game",
        f"  succeeded            {t['agg_succeeded']:6d}",
        f"  DEFENDED (failed)    {t['agg_failed']:6d}",
        f"politics decisions     {t['pol_decisions']:6d}",
        f"  held an aggression   {t['held']:6d}   "
        f"{_frac(t['held'], t['pol_decisions'])}",
        f"    ..no military act  {t['held_no_ma']:6d}",
        f"    ..had MA, rules declined (5.4.2)  {t['held_ma_unoffered']:6d}",
        f"  OFFERED one          {t['offered']:6d}   "
        f"{_frac(t['offered'], t['held'])} of held",
        f"  CHOSE one            {t['chosen']:6d}   "
        f"{_frac(t['chosen'], t['offered'])} of offered",
        f"  declined for: pass {t['instead_pass']} / event "
        f"{t['instead_event']} / war {t['instead_war']} / pact "
        f"{t['instead_pact']} / other {t['instead_other']}",
        f"war held/offered/chosen {t['war_held']:5d} / {t['war_offered']} "
        f"/ {t['war_chosen']}",
        f"target could not defend at all: hand empty "
        f"{t['target_hand_empty']} / no military action {t['target_no_ma']} "
        f"/ HAD a say {t['target_had_a_say']}",
        f"defences faced         {t['def_first']:6d}",
        f"  impossible           {t['def_impossible']:6d}   "
        f"{_frac(t['def_impossible'], t['def_first'])}",
        f"  reachable            {t['def_reachable']:6d}",
        f"    attempted          {t['def_attempted']:6d}   "
        f"(of which reachable {t['def_attempted_reachable']})",
        f"    GAVE UP reachable  {t['def_gave_up_reachable']:6d}",
        f"      winnable on ONE card (outcome visible at 1 ply)  "
        f"{t['def_reach_1card']:5d}, gave up {t['def_gave_up_1card']}",
        f"      needs 2+ cards (first defend stays pending)      "
        f"{t['def_reach_multi']:5d}, gave up {t['def_gave_up_multi']}",
        f"  defence cards played {t['def_cards_played']:6d}",
    ]
    return "\n".join(lines)


def _frac(a, b):
    return f"{a / b:.4f}" if b else "n/a"


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--spec", required=True)
    ap.add_argument("--players", type=int, default=2)
    ap.add_argument("--games", type=int, default=200)
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--workers", type=int, default=10)
    ap.add_argument("--label", default=None)
    ap.add_argument("--out", default=None)
    a = ap.parse_args(argv)
    spec = load_spec(a.spec)
    total = run(spec, a.players, a.games, a.seed, a.workers)
    label = a.label or f"{a.spec} {a.players}p"
    print(report(total, a.games, label), flush=True)
    if a.out:
        with open(a.out, "w") as fh:
            json.dump({"spec": a.spec, "players": a.players,
                       "games": a.games, "seed": a.seed,
                       "counts": total}, fh, indent=1)
    return total


if __name__ == "__main__":
    main()
