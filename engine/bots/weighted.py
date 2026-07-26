"""WeightedBot: a 1-ply bot whose entire behaviour is a JSON weight dict.

The evaluation is linear over ~57 features covering the real strategic
levers of Through the Ages, plus 10 of those features duplicated with an
"early" and a "late" copy scaled by how far the game has progressed
(78 weights total).  Everything is JSON-serializable, so hill climbing
(experiments/hillclimb.py) can mutate, checkpoint and reload a bot.

Feature groups
    economy      culture/science stock and rate, food/resource rate and
                 stock, blue bank, corruption and consumption losses,
                 population cost, workers by category
    happiness    happy margin, discontent, uprising flag
    actions      civil/military action totals and unspent actions
    military     absolute strength, strength relative to the strongest
                 rival, split into a capped lead and an uncapped deficit,
                 tactic level, colonies, pacts
    technology   summed tech levels, government level, best card level per
                 type (the "tech curve"), number of techs, special techs
    wonders      completed wonders, blue steps invested, cost remaining
    cards        civil/military hand size and summed card levels
    rivals       the best rival's culture, culture rate, science rate and
                 strength (leading is what wins, not absolute output)

`evaluate(state, idx, weights)` is the whole strategy; `WeightedBot.pick`
applies every legal move to a fast copy of the state and keeps the best.
"""
from __future__ import annotations

import random

from .. import actions, cards as C, economy, effects
from .fastcopy import copy_state

__all__ = ["DEFAULT_WEIGHTS", "WeightedBot", "features", "evaluate",
           "load_weights", "save_weights"]

# ---------------------------------------------------------------- metadata

_META = None


def _meta():
    """name -> (type, level) for every card, built once."""
    global _META
    if _META is None:
        db = C.db()
        _META = {n: (c["type"], C.level(c["age"]))
                 for n, c in db.by_name.items()}
    return _META


_BEST_TYPES = ("farm", "mine", "lab", "temple", "theater", "library", "arena")

# features that additionally get an early-game and a late-game copy
PHASE_KEYS = (
    "culture", "culture_rate", "science_rate", "food_rate", "resource_rate",
    "workers", "strength_rel", "tech_levels", "wonder_progress", "hand_value",
)

# --------------------------------------------------------------- features


def rival_context(state, idx):
    """Rival aggregates that only change when *they* move.

    Computed once per decision at the root and reused for every candidate
    move, which keeps the 1-ply search from recomputing every opponent's
    full statistics ~30 times per decision.
    """
    best_rate = best_sci = best_str = 0
    for q in state.players:
        if q.idx == idx or q.resigned:
            continue
        s = effects.compute(state, q)
        best_rate = max(best_rate, s.culture)
        best_sci = max(best_sci, s.science)
        best_str = max(best_str, s.strength)
    return {"rival_culture_rate": best_rate, "rival_science_rate": best_sci,
            "rival_strength": best_str}


def lateness(state):
    """0.0 at the start of Age A, 1.0 from Age III on."""
    lv = C.level(state.age_civil)
    return min(1.0, lv / 3.0)


def features(state, idx, ctx=None):
    """The raw feature vector from player `idx`'s point of view."""
    meta = _meta()
    db = C.db()
    p = state.players[idx]
    s = effects.compute(state, p)

    workers = prod_workers = urban_workers = unit_workers = 0
    tech_levels = 0
    special_techs = 0
    best = dict.fromkeys(_BEST_TYPES, 0)
    best_unit = 0
    for name, t in p.techs.items():
        typ, lv = meta.get(name, ("?", 0))
        if typ in _BEST_TYPES:
            if lv > best[typ]:
                best[typ] = lv
        if typ in C.UNIT_TYPES:
            if lv > best_unit:
                best_unit = lv
            unit_workers += t.workers
            tech_levels += lv
        elif typ in C.URBAN_TYPES:
            urban_workers += t.workers
            tech_levels += lv
        elif typ in C.PRODUCTION_TYPES:
            prod_workers += t.workers
            tech_levels += lv
        elif typ == "special-tech":
            special_techs += 1
            tech_levels += lv
        workers += t.workers
    tech_levels += meta.get(p.government, ("?", 0))[1]

    happy_req = economy.happy_required(p.yellow_bank)
    margin = s.happy - happy_req
    discontent = max(0, -margin)
    blue_free = effects.blue_available(p)
    pop_base = economy.pop_cost_base(p.yellow_bank)
    pop_cost = 8 if pop_base is None else max(0, pop_base - s.pop_food_discount)

    progress = remaining = 0
    if p.wonder is not None:
        stages = db.get(p.wonder.name)["stages"]
        progress = sum(stages[:p.wonder.steps_built])
        remaining = sum(stages[p.wonder.steps_built:])

    hand_value = sum(meta.get(n, ("?", 0))[1] + 1 for n in p.hand_civil)
    hand_mil_value = sum(meta.get(n, ("?", 0))[1] + 1 for n in p.hand_military)

    rivals = [q for q in state.players if q.idx != idx and not q.resigned]
    rival_culture = max((q.culture for q in rivals), default=0)
    rival_mean = (sum(q.culture for q in rivals) / len(rivals)) if rivals else 0
    if ctx is None:
        ctx = rival_context(state, idx)
    rel = s.strength - ctx["rival_strength"]

    return {
        # --- economy
        "culture": p.culture,
        "culture_rate": s.culture,
        "science": p.science,
        "science_rate": s.science,
        "food_rate": s.food,
        "resource_rate": s.resources,
        "food_stock": p.food,
        "resource_stock": p.resources,
        "blue_free": blue_free,
        "corruption_loss": economy.corruption(blue_free),
        "consumption": economy.consumption(p.yellow_bank),
        "pop_cost": pop_cost,
        "yellow_bank": p.yellow_bank,
        "free_workers": p.workers_free,
        "workers": workers,
        "prod_workers": prod_workers,
        "urban_workers": urban_workers,
        "unit_workers": unit_workers,
        # --- happiness
        "happy_margin": min(3, margin),
        "discontent": discontent,
        "uprising": 1.0 if discontent > p.workers_free else 0.0,
        # --- actions
        "civil_actions": s.civil_actions,
        "military_actions": s.military_actions,
        "ca_left": p.civil_actions,
        "ma_left": p.military_actions,
        # --- military
        "strength": s.strength,
        "strength_rel": rel,
        "strength_deficit": max(0, -rel),
        "strength_lead": min(6, max(0, rel)),
        "tactic_level": meta.get(p.tactic, ("?", 0))[1] if p.tactic else 0,
        "colonies": len(getattr(p, "colonies", ()) or ()),
        "pacts": len(getattr(p, "pacts", ()) or ()),
        # --- technology
        "tech_levels": tech_levels,
        "gov_level": meta.get(p.government, ("?", 0))[1],
        "best_farm": best["farm"],
        "best_mine": best["mine"],
        "best_lab": best["lab"],
        "best_temple": best["temple"],
        "best_theater": best["theater"],
        "best_library": best["library"],
        "best_arena": best["arena"],
        "best_unit": best_unit,
        "num_techs": len(p.techs),
        "special_techs": special_techs,
        # --- wonders / leader
        "wonders": len(getattr(p, "completed_wonders", ()) or ()),
        "wonder_progress": progress,
        "wonder_remaining": remaining,
        "leader": 1.0 if p.leader else 0.0,
        # --- cards
        "hand_civil": len(p.hand_civil),
        "hand_value": hand_value,
        "hand_military": len(p.hand_military),
        "hand_mil_value": hand_mil_value,
        # --- rivals
        "rival_culture": rival_culture,
        "rival_mean_culture": rival_mean,
        "rival_culture_rate": ctx["rival_culture_rate"],
        "rival_science_rate": ctx["rival_science_rate"],
        "rival_strength": ctx["rival_strength"],
    }


# ------------------------------------------------------------ default weights

BASE_WEIGHTS = {
    # economy
    "culture": 1.0,
    "culture_rate": 5.0,
    "science": 0.5,
    "science_rate": 4.0,
    "food_rate": 1.2,
    "resource_rate": 1.6,
    "food_stock": 0.2,
    "resource_stock": 0.3,
    "blue_free": 0.15,
    "corruption_loss": -0.9,
    "consumption": -0.5,
    "pop_cost": -0.4,
    "yellow_bank": -0.1,
    "free_workers": 0.4,
    "workers": 1.4,
    "prod_workers": 0.3,
    "urban_workers": 0.5,
    "unit_workers": 0.1,
    # happiness
    "happy_margin": 1.2,
    "discontent": -3.0,
    "uprising": -12.0,
    # actions
    "civil_actions": 2.0,
    "military_actions": 0.7,
    "ca_left": 0.05,
    "ma_left": 0.05,
    # military
    "strength": 0.35,
    "strength_rel": 0.35,
    "strength_deficit": -0.6,
    "strength_lead": 0.3,
    "tactic_level": 0.5,
    "colonies": 2.0,
    "pacts": 0.5,
    # technology
    "tech_levels": 1.0,
    "gov_level": 2.0,
    "best_farm": 0.5,
    "best_mine": 0.5,
    "best_lab": 0.8,
    "best_temple": 0.6,
    "best_theater": 0.6,
    "best_library": 0.5,
    "best_arena": 0.3,
    "best_unit": 0.5,
    "num_techs": 0.3,
    "special_techs": 0.8,
    # wonders / leader
    "wonders": 3.0,
    "wonder_progress": 1.0,
    "wonder_remaining": -0.3,
    "leader": 1.5,
    # cards
    "hand_civil": 0.3,
    "hand_value": 0.25,
    "hand_military": 0.3,
    "hand_mil_value": 0.15,
    # rivals
    "rival_culture": -0.35,
    "rival_mean_culture": -0.1,
    "rival_culture_rate": -1.0,
    "rival_science_rate": -0.6,
    "rival_strength": -0.15,
    # search bias: value of the "end turn" move itself (its child state has
    # already collected a production phase, which flatters it)
    "end_turn_bias": -3.0,
}

# early/late multipliers: the contribution of PHASE_KEYS features is
# w[k] + (1-L)*w[k_early] + L*w[k_late] with L = lateness(state).
PHASE_WEIGHTS = {
    "culture_early": -0.4, "culture_late": 1.5,
    "culture_rate_early": 2.0, "culture_rate_late": -2.0,
    "science_rate_early": 2.5, "science_rate_late": -2.5,
    "food_rate_early": 0.6, "food_rate_late": -0.6,
    "resource_rate_early": 0.5, "resource_rate_late": -0.4,
    "workers_early": 0.8, "workers_late": -0.6,
    "strength_rel_early": -0.1, "strength_rel_late": 0.5,
    "tech_levels_early": 0.5, "tech_levels_late": -0.4,
    "wonder_progress_early": 0.3, "wonder_progress_late": -0.3,
    "hand_value_early": 0.2, "hand_value_late": -0.2,
}

DEFAULT_WEIGHTS = dict(BASE_WEIGHTS)
DEFAULT_WEIGHTS.update(PHASE_WEIGHTS)


# ------------------------------------------------------------- evaluation

def evaluate(state, idx, weights=None, ctx=None, f=None):
    w = weights if weights is not None else DEFAULT_WEIGHTS
    if f is None:
        f = features(state, idx, ctx)
    total = 0.0
    get = w.get
    for k, v in f.items():
        wk = get(k)
        if wk:
            total += wk * v
    late = lateness(state)
    early = 1.0 - late
    for k in PHASE_KEYS:
        v = f[k]
        if not v:
            continue
        we = get(k + "_early")
        if we:
            total += we * early * v
        wl = get(k + "_late")
        if wl:
            total += wl * late * v
    return total


# ------------------------------------------------------------------- bot

class WeightedBot:
    """1-ply search under a fully parameterized linear evaluation."""

    name = "weighted"

    def __init__(self, weights=None, rng=None, seed=None, name=None):
        self.weights = dict(weights) if weights else dict(DEFAULT_WEIGHTS)
        self.rng = rng or random.Random(seed)
        if name:
            self.name = name

    # -- harness adapters
    def choose(self, state, moves, rng=None):
        return self.pick(state, moves)

    def __call__(self, state):
        return self.pick(state, actions.legal_moves(state))

    def pick(self, state, moves):
        if len(moves) == 1:
            return moves[0]
        # Score for whoever actually owns the move. On a pending decision that
        # is NOT the turn player -- pact accept/refuse is always one of these,
        # and 10/16 auction decisions were measured to be -- `state.current`
        # made us maximise a RIVAL's position (docs/PACTS_DIAGNOSIS.md).
        idx = state.decider()
        try:
            ctx = rival_context(state, idx)
        except Exception:
            ctx = {"rival_culture_rate": 0, "rival_science_rate": 0,
                   "rival_strength": 0}
        w = self.weights
        end_bias = w.get("end_turn_bias", 0.0)
        best, best_val = None, None
        for mv in moves:
            trial = copy_state(state)
            try:
                actions.apply(trial, mv, random.Random(0))
                val = evaluate(trial, idx, w, ctx)
            except Exception:
                # The engine grows new move types and new state fields under
                # us; an unscorable candidate is skipped, never fatal.  If
                # every candidate is unscorable we still return a legal move.
                continue
            if mv[0] == "end_turn":
                val += end_bias
            if best_val is None or val > best_val:
                best, best_val = mv, val
        if best is None:
            return self.rng.choice(moves)
        return best


# ------------------------------------------------------------------- io

def load_weights(path):
    import json
    with open(path) as fh:
        d = json.load(fh)
    w = dict(DEFAULT_WEIGHTS)
    w.update(d.get("weights", d))
    return w


def save_weights(path, weights, **extra):
    import json
    import os
    d = {"weights": weights}
    d.update(extra)
    tmp = path + ".tmp"
    os.makedirs(os.path.dirname(os.path.abspath(path)), exist_ok=True)
    with open(tmp, "w") as fh:
        json.dump(d, fh, indent=1, sort_keys=True)
    os.replace(tmp, path)
