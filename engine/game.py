"""Turn loop, age progression and game end (docs/RULES_SPEC.md §1, §2, §5.0,
§6.6, §12).

Public API (docs/ARCHITECTURE.md):

    new_game(num_players, seed)   -> GameState
    legal_moves(state)            -> [move, ...]
    apply(state, move, rng=None)  -> GameState (mutated in place)
    current_player(state)         -> int
    is_over(state)                -> bool
    scores(state)                 -> [culture, ...] by player index

Turn structure (§5.0):
    Start-of-Turn : replenish the card row -> resolve a war I declared ->
                    exclusive tactic goes public
    Politics      : at most one political action (skipped in round 1)
    Actions       : civil + military actions in any order, ends with
                    ("end_turn",)
    End-of-Turn   : economy.end_of_turn() (§6.6)
"""
from __future__ import annotations

import random

from . import cards as C
from . import economy
from . import effects
from . import events
from .actions import ROW_SIZE, apply, legal_moves  # noqa: F401 (re-export)
from .state import GameState, PlayerState, TechCard

START_TECHS = {
    "Warriors": 1,
    "Agriculture": 2,
    "Bronze": 2,
    "Philosophy": 1,
    "Religion": 0,
}

SWEEP = {2: 3, 3: 2, 4: 1}


def _rng_for(state, rng=None):
    if rng is not None:
        return rng
    return random.Random(state.seed * 1000003 + state.turn * 97 + state.round)


# ------------------------------------------------------------------ setup

def new_game(num_players, seed=0):
    """Deal a fresh game (§1)."""
    if not 2 <= num_players <= 4:
        raise ValueError("Through the Ages is a 2-4 player game")
    db = C.db()
    rng = random.Random(seed)
    state = GameState(num_players=num_players, seed=seed)
    state.has_military = db.has_military

    for i in range(num_players):
        p = PlayerState(idx=i)
        p.techs = {n: TechCard(n, workers=w) for n, w in START_TECHS.items()}
        p.government = "Despotism"
        p.yellow_bank = 18
        p.workers_free = 1
        p.blue_total = 16
        # §1.9: first round civil actions are 1, 2, 3, 4 by seating order
        p.civil_actions = i + 1
        p.military_actions = 0
        state.players.append(p)

    state.start_player = 0
    state.current = 0
    state.turn = 1
    state.round = 1

    # civil card row: 13 spaces dealt from the shuffled Age A civil deck
    state.age_civil = "A"
    state.civil_deck = db.civil_deck("A", num_players)
    rng.shuffle(state.civil_deck)
    state.card_row = [None] * ROW_SIZE
    _deal(state, rng)

    # military: no deck in Age A; seed the current events (§1.6)
    state.age_military = "A"
    state.military_deck = []
    if state.has_military:
        age_a = db.military_deck("A", num_players)
        rng.shuffle(age_a)
        state.current_events = age_a[:num_players + 2]

    state.phase = "actions"          # §1.9: no politics phase in round 1
    return state


# ------------------------------------------------------------- card row

def live_count(state):
    """Player count for deck trimming / event tables (§13, resignations)."""
    return max(2, min(4, len(state.active_players())))


def _sweep_count(state):
    return SWEEP[live_count(state)]


def _replenish(state, rng):
    """§2.1 discard leftmost N -> slide left -> deal from the current deck."""
    if state.age_civil == "A":
        # §1.10: the first replenish ends Age A (no antiquation, no losses)
        state.civil_deck = []
        _advance_age(state, rng)

    n = _sweep_count(state)
    row = state.card_row
    for i in range(min(n, len(row))):
        row[i] = None
    kept = [c for c in row if c is not None]
    state.card_row = kept + [None] * (ROW_SIZE - len(kept))
    _deal(state, rng)


def deal_row(state, rng):
    """Fill empty row slots from the current civil deck (§2.1 step 3)."""
    _deal(state, rng)


def _deal(state, rng):
    for i in range(ROW_SIZE):
        if state.card_row[i] is not None:
            continue
        if not state.civil_deck:
            break
        state.card_row[i] = state.civil_deck.pop()
        if not state.civil_deck:
            # §2.2: the age ends the moment its last card is dealt
            _advance_age(state, rng)
            if not state.civil_deck:
                break


# --------------------------------------------------------- age progression

def _advance_age(state, rng):
    """End the current age and make the next age's decks current (§12.2)."""
    ended = state.age_civil
    if ended == "IV":
        return
    nxt = C.AGES[C.level(ended) + 1]
    db = C.db()

    if ended != "A":
        _antiquate(state, C.level(ended))
        for p in state.players:
            p.yellow_bank = max(0, p.yellow_bank - 2)   # §12.2.4

    state.age_civil = nxt
    state.age_military = nxt
    if nxt == "IV":
        state.civil_deck = []
        state.military_deck = []
        _set_last_round(state)
    else:
        # §13: future-age decks are trimmed for the surviving player count
        n = live_count(state)
        state.civil_deck = db.civil_deck(nxt, n)
        rng.shuffle(state.civil_deck)
        if state.has_military:
            state.military_deck = db.military_deck(nxt, n)
            rng.shuffle(state.military_deck)
    effects.invalidate(state)
    state.emit(f"age {ended} ended -> age {nxt}")


def _antiquate(state, ended_level):
    """Remove cards of ages OLDER than the age that just ended (§12.2)."""
    db = C.db()
    for p in state.players:
        p.hand_civil = [n for n in p.hand_civil
                        if db.level_of(n) >= ended_level]
        keep = []
        for n in p.hand_military:
            if db.level_of(n) >= ended_level:
                keep.append(n)
            else:
                economy.discard_military(state, n)
        p.hand_military = keep
        if p.leader and db.level_of(p.leader) < ended_level:
            effects.on_leave_play(state, p, p.leader)
            p.leader = None
        if p.wonder and db.level_of(p.wonder.name) < ended_level:
            p.wonder = None                      # blue tokens return to bank
        # §12.2.2 antiquated pacts leave play (technologies, wonders,
        # colonies, tactics and declared wars stay)
        p.pacts = [pact for pact in p.pacts
                   if db.level_of(pact["name"]) >= ended_level]
        effects.invalidate(state, p)


def _set_last_round(state):
    """§12.3: Age IV begins -> this or the next round is the last one."""
    if state.final_round_end is not None:
        return
    if state.current == state.start_player:
        state.final_round_end = state.round
    else:
        state.final_round_end = state.round + 1
    state.last_round = state.round >= state.final_round_end
    state.emit(f"age IV: last round is round {state.final_round_end}")


# --------------------------------------------------------------- turn loop

def start_turn(state, rng=None):
    """Start-of-Turn sequence + politics phase entry (§5.0)."""
    rng = _rng_for(state, rng)
    p = state.me()
    if state.round > 1:
        _replenish(state, rng)
        events.resolve_war(state, p, rng)        # §5.7
        if p.tactic_exclusive:                   # §10.2 tactic goes public
            if p.tactic and p.tactic not in state.available_tactics:
                state.available_tactics.append(p.tactic)
            p.tactic_exclusive = False
    state.last_round = (state.final_round_end is not None
                        and state.round >= state.final_round_end)
    p.politics_done = False
    p.taken_this_turn = []
    if p.skip_next_politics:            # International Agreement (CoL p.12)
        p.skip_next_politics = False
        state.phase = "actions"
    elif state.round > 1 and state.has_military and not state.game_over:
        state.phase = "politics"
        _auto_skip_politics(state, rng)
    else:
        state.phase = "actions"


def _auto_skip_politics(state, rng):
    """Pass immediately when passing is the only political option."""
    from . import actions
    if len(actions._politics_moves(state, state.me())) == 1:
        state.me().politics_done = True
        state.phase = "actions"


def end_turn(state, rng=None):
    """End-of-Turn sequence, then hand the turn to the next player (§6.6)."""
    rng = _rng_for(state, rng)
    p = state.me()
    economy.end_of_turn(state, p, rng)
    return _advance_turn(state, rng)


def after_resign(state, rng=None):
    """§5.11: a resigning player's turn ends at once; last one left wins."""
    rng = _rng_for(state, rng)
    active = state.active_players()
    if len(active) <= 1:
        if active:
            state.forced_winner = active[0].idx
        _finish_game(state)
        return state
    return _advance_turn(state, rng)


def _advance_turn(state, rng):
    state.turn += 1

    nxt = _next_player(state)
    if nxt is None:
        _finish_game(state)
        return state
    wrapped = _seat_index(state, nxt) <= _seat_index(state, state.current)
    state.current = nxt
    if wrapped:
        state.round += 1
        if (state.final_round_end is not None
                and state.round > state.final_round_end):
            _finish_game(state)
            return state
    start_turn(state, rng)
    return state


def _seat_index(state, idx):
    """Position in the current round's turn order (start player = 0)."""
    return (idx - state.start_player) % state.num_players


def _next_player(state):
    n = state.num_players
    for step in range(1, n + 1):
        cand = (state.current + step) % n
        if not state.players[cand].resigned:
            return cand
    return None


# ------------------------------------------------------------- game end

def _finish_game(state):
    """§12.5 final scoring."""
    rng = _rng_for(state)
    if state.has_military:
        events.evaluate_final_events(state)
    effects.invalidate(state)
    out = []
    for p in state.players:
        p.culture = max(0, p.culture + effects.end_of_game_bonus(state, p))
        out.append(p.culture)
    state.final_scores = out
    state.game_over = True
    state.phase = "done"
    state.emit(f"game over: {out}")
    return out


# --------------------------------------------------------------- API

def current_player(state):
    """Index of the player who must choose the next move (§ interact)."""
    return state.decider()


def is_over(state):
    return bool(state.game_over)


def scores(state):
    if state.final_scores is not None:
        return list(state.final_scores)
    return [p.culture for p in state.players]


def winners(state):
    """Indices of the players with the most culture (ties share the win)."""
    if state.forced_winner is not None:
        return [state.forced_winner]        # §5.11 last player standing
    sc = [(-1 if p.resigned else s) for p, s in zip(state.players, scores(state))]
    best = max(sc)
    return [i for i, v in enumerate(sc) if v == best]


# ------------------------------------------------------------- driver

MOVE_CAP = 20000


def play_game(bots, num_players=None, seed=0, move_cap=MOVE_CAP, state=None):
    """Run a full game; `bots` is a list of callables bot(state) -> move.

    Returns the finished state. `state.move_cap_hit` is set when the game was
    aborted because it exceeded `move_cap` decisions (should never happen).
    """
    if state is None:
        state = new_game(num_players or len(bots), seed)
    rng = random.Random(seed ^ 0x5EED)
    moves = 0
    while not state.game_over:
        if moves >= move_cap:
            state.move_cap_hit = True
            _finish_game(state)
            break
        mv = bots[state.decider()](state)
        apply(state, mv, rng)
        moves += 1
    state.moves_played = moves
    return state
