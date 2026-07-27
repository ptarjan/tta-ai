"""Torch-free tensor encoding of a `GameState` from one player's viewpoint.

This module has NO numpy/torch dependency on purpose: it produces a flat
``list[float]`` so the engine's own stdlib test suite and `tools/gate.sh`
can exercise it on the Mac (where torch is not installed), while the neural
net that consumes it lives in `neural_net.py` behind a guarded torch import.

What is encoded, and why it is legal
------------------------------------
The encoding is faithful to what a player can actually SEE at the table
(docs/INFORMATION_AUDIT.md).  Concretely:

* **My board** (player ``idx``): fully encoded -- techs/tech-curve, workers by
  category, resources/food/science/culture (stock and rate), civil/military
  actions, government, leader, wonder-in-progress, happiness, tactic, colonies,
  pacts, and BOTH my civil and my military hands (I know my own hand).
* **The card row** (13 slots): every slot's card identity (via `card_vec`) and
  its civil-action cost.  Public -- INFORMATION_AUDIT GAP 1/2.
* **Each rival** (public only): board, tech-curve, resources, actions,
  government, leader, wonders, tactic, colonies, pacts, and their **civil hand**
  (public, "open civil cards convention", RULES_SPEC 2.6).  Their **military
  hand is encoded as a COUNT only** -- contents are hidden (RULES_SPEC 196).
* **My own seeded events** (`seeded_by[name] == idx`): count + summed level.  I
  know what I put in the politics deck; I do NOT encode other players' seeds or
  the order of the current-events deck.
* **Global**: ages (civil/military/current-events), round, turn, an exact/est
  `rounds_left`, lateness L, player count, phase, last-round / scoring flags.

Deliberately NOT encoded (would be cheating or is unknowable):
  * civil_deck / military_deck ORDER (hidden; only counts feed `rounds_left`).
  * rival military-hand CONTENTS (count only).
  * other players' event seeds and the current-events order.
  * happy faces of rivals beyond what their public board implies.

Player perspective
------------------
`encode(state, idx)` always lays the encoding out as [me, rival0, rival1,
rival2] in turn order starting just after ``idx``, zero-padding rival slots up
to `MAX_PLAYERS-1` so the vector length is fixed across 2p/3p/4p games.
"""
from __future__ import annotations

from functools import lru_cache

from .. import cards as C, economy, effects

__all__ = ["card_vec", "encode", "encoding_dim", "CARD_VEC_DIM",
           "MAX_PLAYERS", "describe"]

MAX_PLAYERS = 4

# ---- card identity vector -------------------------------------------------

CARD_TYPES = (
    "farm", "mine", "lab", "temple", "library", "arena", "theater",
    "infantry", "cavalry", "artillery", "air", "government", "special-tech",
    "wonder", "leader", "tactic", "aggression", "war", "pact", "bonus",
    "territory", "event", "action",
)
_TYPE_IX = {t: i for i, t in enumerate(CARD_TYPES)}

# per-turn production printed on the card
PROD_KEYS = ("culture", "science", "food", "resources", "happy", "strength")
# numeric effect-block keys worth exposing (one-time gains and permanents)
EFF_KEYS = (
    "civilActions", "militaryActions", "culture", "science", "food",
    "resources", "happy", "strength", "cultureProduction", "scienceProduction",
    "foodProduction", "resourceProduction", "yellowTokens", "blueTokens",
    "drawMilitaryCards",
)

# type one-hot + level + prod + eff + [techCost, buildCost, stages_sum, strength]
CARD_VEC_DIM = len(CARD_TYPES) + 1 + len(PROD_KEYS) + len(EFF_KEYS) + 4
_ZERO_CARD = tuple(0.0 for _ in range(CARD_VEC_DIM))


@lru_cache(maxsize=None)
def card_vec(name):
    """A fixed-length identity/effect vector for a card name (0-vector if None
    or unknown).  Cached: the card DB is immutable."""
    if not name:
        return _ZERO_CARD
    db = C.db()
    card = db.by_name.get(name)
    if card is None:
        return _ZERO_CARD
    v = [0.0] * CARD_VEC_DIM
    ix = _TYPE_IX.get(card.get("type"))
    if ix is not None:
        v[ix] = 1.0
    o = len(CARD_TYPES)
    v[o] = C.level(card.get("age")) / 4.0
    o += 1
    prod = card.get("production") or {}
    for i, k in enumerate(PROD_KEYS):
        val = prod.get(k)
        if isinstance(val, (int, float)) and val is not True:
            v[o + i] = float(val) / 4.0
    o += len(PROD_KEYS)
    eff = card.get("effects") or {}
    for i, k in enumerate(EFF_KEYS):
        val = eff.get(k)
        if isinstance(val, (int, float)) and val is not True:
            v[o + i] = float(val) / 4.0
    o += len(EFF_KEYS)
    tc = card.get("techCost") or 0
    bc = card.get("buildCost") or 0
    stages = card.get("stages") or []
    v[o] = float(tc) / 8.0
    v[o + 1] = float(bc) / 8.0
    v[o + 2] = float(sum(stages)) / 8.0
    st = card.get("strength")
    v[o + 3] = (float(st) / 6.0) if isinstance(st, (int, float)) else 0.0
    return tuple(v)


def _sum_card_vec(names):
    """Element-wise sum of card_vec over a list of names (0-vector if empty)."""
    acc = [0.0] * CARD_VEC_DIM
    for n in names:
        cv = card_vec(n)
        for i in range(CARD_VEC_DIM):
            acc[i] += cv[i]
    return acc


_BEST_TYPES = ("farm", "mine", "lab", "temple", "theater", "library", "arena")


# ---- one player's block ---------------------------------------------------

def _player_block(state, idx, me_idx, best_rival_strength, out):
    """Append player ``idx``'s block to ``out``.

    ``full`` (idx == me_idx) controls the fields that are private to a player:
    the military-hand CONTENTS and the seeded-event summary are emitted only
    for me; for a rival those positions are zeroed so the block length is
    identical for every seat.
    """
    full = (idx == me_idx)
    p = state.players[idx]
    s = effects.compute(state, p)

    meta = C.db()
    workers = prod_w = urban_w = unit_w = tech_levels = special = 0
    best = dict.fromkeys(_BEST_TYPES, 0)
    best_unit = 0
    for name, t in p.techs.items():
        typ = meta.type_of(name)
        lv = C.level(meta.by_name[name]["age"]) if name in meta.by_name else 0
        if typ in _BEST_TYPES and lv > best[typ]:
            best[typ] = lv
        if typ in C.UNIT_TYPES:
            best_unit = max(best_unit, lv)
            unit_w += t.workers
            tech_levels += lv
        elif typ in C.URBAN_TYPES:
            urban_w += t.workers
            tech_levels += lv
        elif typ in C.PRODUCTION_TYPES:
            prod_w += t.workers
            tech_levels += lv
        elif typ == "special-tech":
            special += 1
            tech_levels += lv
        workers += t.workers

    happy_req = economy.happy_required(p.yellow_bank)
    margin = s.happy - happy_req
    discontent = max(0, -margin)
    blue = effects.blue_available(p)
    pop_base = economy.pop_cost_base(p.yellow_bank)
    pop_cost = 8 if pop_base is None else max(0, pop_base - s.pop_food_discount)

    progress = remaining = 0
    if p.wonder is not None:
        stages = meta.by_name[p.wonder.name]["stages"]
        progress = sum(stages[:p.wonder.steps_built])
        remaining = sum(stages[p.wonder.steps_built:])

    rel = s.strength - best_rival_strength

    seeded_n = seeded_lv = 0
    if full:
        for name, owner in state.seeded_by.items():
            if owner == idx:
                seeded_n += 1
                if name in meta.by_name:
                    seeded_lv += C.level(meta.by_name[name]["age"])

    scal = [
        1.0,                                    # present
        p.culture / 100.0, p.science / 50.0,
        p.food / 50.0, p.resources / 50.0,
        s.culture / 30.0, s.science / 30.0, s.food / 20.0, s.resources / 20.0,
        s.strength / 30.0, s.happy / 10.0,
        s.civil_actions / 8.0, s.military_actions / 6.0,
        p.yellow_bank / 18.0, p.workers_free / 10.0, blue / 16.0,
        economy.corruption(blue) / 5.0, economy.consumption(p.yellow_bank) / 10.0,
        pop_cost / 8.0,
        min(3, margin) / 3.0, discontent / 10.0,
        1.0 if discontent > p.workers_free else 0.0,
        p.civil_actions / 8.0, p.military_actions / 6.0, p.ca_spent_taking / 6.0,
        best["farm"] / 4.0, best["mine"] / 4.0, best["lab"] / 4.0,
        best["temple"] / 4.0, best["theater"] / 4.0, best["library"] / 4.0,
        best["arena"] / 4.0, best_unit / 4.0,
        prod_w / 15.0, urban_w / 15.0, unit_w / 15.0, workers / 15.0,
        len(p.techs) / 25.0, special / 12.0, tech_levels / 60.0,
        len(p.completed_wonders) / 8.0, progress / 12.0, remaining / 12.0,
        1.0 if p.wonder is not None else 0.0,
        (C.level(meta.by_name[p.tactic]["age"]) / 4.0
         if p.tactic and p.tactic in meta.by_name else 0.0),
        len(p.colonies) / 3.0,
        sum(1 for q in state.players for pc in q.pacts
            if idx in (pc["owner"], pc["partner"])) / 5.0,
        rel / 30.0, max(0, -rel) / 30.0, min(6, max(0, rel)) / 6.0,
        p.hand_size("civil") / 12.0, p.hand_size("military") / 12.0,
        seeded_n / 10.0, seeded_lv / 20.0,
        C.level(meta.by_name[p.government]["age"]) / 4.0
        if p.government in meta.by_name else 0.0,
        1.0 if p.leader else 0.0,
    ]
    out.extend(scal)
    # card-identity blocks (each CARD_VEC_DIM long)
    out.extend(card_vec(p.government))
    out.extend(card_vec(p.leader))
    out.extend(card_vec(p.wonder.name if p.wonder else None))
    out.extend(_sum_card_vec(p.hand_civil))          # public for everyone
    # military hand contents: mine only; rival gets a zero-vector (count above)
    out.extend(_sum_card_vec(p.hand_military) if full else list(_ZERO_CARD))


_PLAYER_SCALARS = 56   # length of `scal` above; asserted by the encoder test
PLAYER_BLOCK_DIM = _PLAYER_SCALARS + 5 * CARD_VEC_DIM


def _empty_player_block(out):
    out.extend([0.0] * PLAYER_BLOCK_DIM)


# ---- global block ---------------------------------------------------------

def _onehot_age(age):
    v = [0.0] * len(C.AGES)
    if age in C.AGES:
        v[C.AGES.index(age)] = 1.0
    return v


def _global_block(state, idx):
    from .weighted import rounds_left, lateness
    n = state.num_players
    npc = [0.0, 0.0, 0.0]
    if 2 <= n <= 4:
        npc[n - 2] = 1.0
    phase = [1.0 if state.phase == ph else 0.0
             for ph in ("politics", "actions", "done")]
    try:
        rl = rounds_left(state)
        lat = lateness(state)
    except Exception:
        rl, lat = 20.0, 0.0
    row_occ = sum(1 for c in state.card_row if c is not None)
    g = []
    g += _onehot_age(state.age_civil)
    g += _onehot_age(state.age_military)
    g += _onehot_age(state.current_events_age)
    g += npc
    g += [
        state.round / 30.0, state.turn / 60.0, rl / 30.0, lat,
        1.0 if state.last_round else 0.0,
        1.0 if state.scoring_events else 0.0,
        len(state.current_events) / 10.0, len(state.future_events) / 10.0,
        row_occ / 13.0,
    ]
    g += phase
    return g


_GLOBAL_DIM = 3 * len(C.AGES) + 3 + 9 + 3


# ---- the card row ---------------------------------------------------------

# civil-action cost by slot (engine/actions.py ROW_COST), duplicated so this
# module stays import-cycle-free.  The encoder test asserts it matches.
_ROW_COST = (1, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3)
_ROW_SIZE = 13


def _row_block(state):
    out = []
    row = state.card_row
    for i in range(_ROW_SIZE):
        name = row[i] if i < len(row) else None
        out.append(1.0 if name else 0.0)
        out.append(_ROW_COST[i] / 3.0)
        out.extend(card_vec(name))
    return out


_ROW_DIM = _ROW_SIZE * (2 + CARD_VEC_DIM)


# ---- top level ------------------------------------------------------------

ENCODING_DIM = _GLOBAL_DIM + _ROW_DIM + MAX_PLAYERS * PLAYER_BLOCK_DIM


def encode(state, idx):
    """Flat ``list[float]`` of length :data:`ENCODING_DIM` for player ``idx``."""
    out = _global_block(state, idx)
    out.extend(_row_block(state))
    # best rival strength (for my relative-strength features), computed once
    best_rs = 0
    for q in state.players:
        if q.idx != idx and not q.resigned:
            best_rs = max(best_rs, effects.compute(state, q).strength)
    _player_block(state, idx, idx, best_rs, out)
    # rivals in seating order after me, up to MAX_PLAYERS-1, zero-padded
    n = state.num_players
    rivals = [(idx + k) % n for k in range(1, n)]
    for r in rivals[:MAX_PLAYERS - 1]:
        # a rival's relative strength is measured against *its* best rival
        best = 0
        for q in state.players:
            if q.idx != r and not q.resigned:
                best = max(best, effects.compute(state, q).strength)
        _player_block(state, r, idx, best, out)
    for _ in range(MAX_PLAYERS - 1 - len(rivals)):
        _empty_player_block(out)
    return out


def encoding_dim():
    return ENCODING_DIM


def describe():
    """Human-readable size breakdown, for the doc and for sanity."""
    return {
        "card_vec_dim": CARD_VEC_DIM,
        "global_dim": _GLOBAL_DIM,
        "row_dim": _ROW_DIM,
        "player_block_dim": PLAYER_BLOCK_DIM,
        "player_scalars": _PLAYER_SCALARS,
        "max_players": MAX_PLAYERS,
        "encoding_dim": ENCODING_DIM,
    }
