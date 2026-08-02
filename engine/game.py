"""Turn loop, age progression and game end (docs/RULES_SPEC.md §1, §2, §5.0,
§6.6, §12).

Public API (docs/README.md):

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
from . import journal

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
        journal.touch(state.players).append(p)

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
    db = C.db()
    for i in range(min(n, len(row))):
        name = row[i]
        if name is not None:
            # keep the public record of what was destroyed (§2.1) -- see
            # GameState.civil_discard and docs/INFORMATION_AUDIT.md GAP 5
            age = db.age_of(name) if name in db.by_name else state.age_civil
            journal.touch(journal.touch(state.civil_discard)
                          .setdefault(age, [])).append(name)
        journal.touch(row)[i] = None
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
        journal.touch(state.card_row)[i] = journal.touch(state.civil_deck).pop()
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
    """Remove cards of ages OLDER than the age that just ended (§12.2).

    The civil cards culled out of hands are RECORDED, by
    `economy.discard_civil`, exactly as the military ones already go through
    `economy.discard_military` below.  They used to vanish: the list
    comprehension dropped them and nothing wrote them down, so an age's printed
    card count stopped adding up the moment antiquation touched it, and
    `engine.bots.counting` -- which subtracts what it has seen from what the
    rulebook prints -- got a silent shortfall it could not distinguish from
    cards still in a rival's hand.  That is the same "in this list but not this
    one" shape GAP 5 was, one zone over, and it was found by the counting tests
    rather than by reading.

    A human at the table sees this happen: the cull is public and the cards go
    to the discard face up.  Nothing in the rules or the turn loop reads
    `civil_removed` -- it is a record, not state -- so this cannot change play.
    """
    db = C.db()
    for p in state.players:
        for n in p.hand_civil:
            if db.level_of(n) < ended_level:
                economy.discard_civil(state, n)
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
            economy.discard_civil(state, p.leader)
            p.leader = None
        if p.wonder and db.level_of(p.wonder.name) < ended_level:
            economy.discard_civil(state, p.wonder.name)
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
                journal.touch(state.available_tactics).append(p.tactic)
            p.tactic_exclusive = False
    state.last_round = (state.final_round_end is not None
                        and state.round >= state.final_round_end)
    p.politics_done = False
    p.taken_this_turn = []
    p.ca_spent_taking = 0
    if p.skip_next_politics:            # International Agreement (CoL p.12)
        p.skip_next_politics = False
        state.phase = "actions"
    elif state.round > 1 and state.has_military and not state.game_over:
        state.phase = "politics"
        if state.pending:
            # `resolve_war` above can leave a War over Technology's spoils
            # decision outstanding (§5.7), and stealing a blue technology can
            # hand the CURRENT player military actions -- Warfare +1,
            # Strategy +2, Military Theory +3 -- which is exactly what
            # `_auto_skip_politics` reads to decide whether passing is the
            # only political option.  Answering that question before the
            # spoils are taken would deny the victor a politics phase it is
            # owed, so the test is deferred behind the decision.  Nothing
            # else can be pending here: measured across the fingerprint's 33
            # games, `state.pending` was empty on all 3737 arrivals.
            from . import interact
            interact.enqueue(state, {"player": state.current,
                                     "tag": "auto_skip_politics"})
        else:
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
    return _resume_end_turn(state, state.me(), rng)


def _resume_end_turn(state, p, rng):
    """Run §6.6 from step 1; suspend if the discard step needs a decision.

    §6.6 step 1 is the only end-of-turn step that asks the player anything,
    and it is a real choice (RB p.20: "Once you have decided which military
    cards to discard, the rest of your turn is automatic").  When it pushes
    that choice, the turn does NOT advance: the continuation is queued as an
    `end_of_turn` item, `apply_pending` drains the queue once the player has
    chosen, and `interact._q_end_of_turn` lands back here.  Steps 2-5 and the
    hand-off therefore stay strictly after the discard, as the sequence
    requires -- the next player may not start until the discarding is done.
    """
    from . import interact
    # Derive the rng HERE and not only in `end_turn`: the resume path arrives
    # from `interact._q_end_of_turn` with whatever rng `apply_pending` was
    # given, which is None for callers that use the `actions.apply(state, mv)`
    # default.  `_rng_for` reads seed/turn/round, none of which a discard can
    # change, so deriving it at resume time gives the same stream the
    # unsuspended sequence would have used.
    rng = _rng_for(state, rng)
    if not economy.end_of_turn(state, p, rng):
        interact.enqueue(state, {"player": p.idx, "tag": "end_of_turn"})
        return state
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
