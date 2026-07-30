"""How the bot treats event seeding, and how large the mispricing is.

`engine/bots/weighted.py` contains no reference to `future_events`,
`current_events`, `past_events` or scoring events of any kind.  So when the
evaluator scores a `prepare_event` candidate it sees exactly two things:

* ``+level_of(name)`` culture, which `actions._h_prepare_event` grants on the
  spot (1, 2 or 3 points), and
* the card leaving the military hand -- `hand_military` down one and
  `hand_mil_value` down `age+1`.

It does **not** see what the event does when it fires.  For the fifteen Age III
"Impact of ..." events that is the whole card: `events.evaluate_final_events`
awards `events.scoring_culture` to every player at game end, routinely 10-40
culture, and none of it is attributable to the plant inside the search.

This tool measures three things over real self-play games:

``plants``      how many events each seat seeds, by age -- the "does the bot
                take the card at all" gate that docs/CARD_BLINDNESS.md 5.1
                says to check before pricing anything.
``final``       the culture `evaluate_final_events` actually awards at game
                end, per seat, and the margin it swings.
``gap``         for every Age III scoring event, `scoring_culture` evaluated
                on the final board for the seat that seeded it minus the mean
                over the others: the margin the planter bought and the search
                could not see.

Usage:
    python3 tools/event_plants.py --players 2 --games 20 --bot plan
"""
from __future__ import annotations

import argparse
import collections
import json
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine import cards as C, events, game          # noqa: E402
from engine.bots import make_bots                    # noqa: E402
from engine.bots.weighted import WeightedBot, load_weights   # noqa: E402


def _bots(kind, n, seed, weights):
    if weights is None:
        return make_bots(kind, n, seed=seed)
    import random
    if kind == "plan":
        from engine.bots.plan import PlanBot
        return [PlanBot(rng=random.Random(seed * 131 + i), width=2,
                        weights=weights) for i in range(n)]
    if kind == "quiescent":
        from engine.bots.quiescent import QuiescentBot
        return [QuiescentBot(rng=random.Random(seed * 131 + i),
                             weights=weights) for i in range(n)]
    return [WeightedBot(rng=random.Random(seed * 131 + i), weights=weights)
            for i in range(n)]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--players", type=int, default=2)
    ap.add_argument("--games", type=int, default=20)
    ap.add_argument("--bot", default="weighted")
    ap.add_argument("--weights", default=None)
    ap.add_argument("--seed0", type=int, default=4100)
    ap.add_argument("--out", default=None)
    a = ap.parse_args()

    w = load_weights(a.weights) if a.weights else None
    db = C.db()

    plants = collections.Counter()          # age -> plants
    plant_names = collections.Counter()
    seeded_gap = []                         # margin bought per seeded Age III
    unseeded_gap = []                       # same for events nobody seeded
    final_swing = []
    n_games = 0

    for gi in range(a.games):
        seed = a.seed0 + gi
        bots = _bots(a.bot, a.players, seed, w)
        st = game.play_game(bots, a.players, seed=seed)
        n_games += 1

        for name, owner in st.seeded_by.items():
            plants[db.age_of(name)] += 1
            plant_names[name] += 1

        # what the Age III scoring events were worth on the FINAL board
        order = list(st.players)
        live = [n for n in list(st.current_events) + list(st.future_events)
                + list(st.past_events)
                if n in db.by_name and db.age_of(n) == "III"]
        per_seat = [0.0] * a.players
        for name in live:
            block = (db.get(name).get("effects") or {}).get("allPlayers")
            if not block:
                continue
            vals = [events.scoring_culture(st, q, block, order)
                    for q in st.players]
            for i, v in enumerate(vals):
                per_seat[i] += v
            owner = st.seeded_by.get(name, -1)
            if owner >= 0:
                rivals = [v for i, v in enumerate(vals) if i != owner]
                seeded_gap.append(vals[owner] - sum(rivals) / len(rivals))
            else:
                best = max(vals)
                rest = [v for v in vals if v is not best] or vals
                unseeded_gap.append(best - sum(rest) / len(rest))
        if a.players == 2:
            final_swing.append(abs(per_seat[0] - per_seat[1]))

    def stat(xs):
        if not xs:
            return {"n": 0}
        m = sum(xs) / len(xs)
        var = sum((x - m) ** 2 for x in xs) / max(1, len(xs) - 1)
        return {"n": len(xs), "mean": round(m, 3),
                "sd": round(var ** 0.5, 3)}

    out = {
        "players": a.players, "games": n_games, "bot": a.bot,
        "weights": a.weights or "default",
        "plants_per_game": {k: round(v / n_games, 3)
                            for k, v in sorted(plants.items())},
        "plants_total": sum(plants.values()),
        "top_planted": plant_names.most_common(12),
        "seeded_ageIII_margin": stat(seeded_gap),
        "unseeded_ageIII_best_margin": stat(unseeded_gap),
        "final_scoring_abs_swing_2p": stat(final_swing),
    }
    print(json.dumps(out, indent=1))
    if a.out:
        with open(a.out, "w") as fh:
            json.dump(out, fh, indent=1)


if __name__ == "__main__":
    main()
