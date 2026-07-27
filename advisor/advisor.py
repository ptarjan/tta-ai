"""The advisor: tell a human at a real table what to play, then take their
report of what everyone else did.

Run it:

    python3 -m advisor.advisor --players 3 --seat 0

Every turn it prints the top few moves with a score and a one-line reason,
you press Enter to accept the top one (or type your own move), and between
turns you type short update lines describing what your opponents did and
which cards were dealt.  Nothing you can type crashes it: bad input is
explained and re-prompted, and ``?`` always means "I don't know that".
"""
from __future__ import annotations

import argparse
import os
import random
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from engine import actions as A                      # noqa: E402
from engine import cards as C                        # noqa: E402
from engine import economy                           # noqa: E402
from engine import effects                           # noqa: E402
from engine import game as G                         # noqa: E402
from engine.bots import weighted as W                # noqa: E402

from advisor import state_io as S                    # noqa: E402
from advisor.state_io import Board, PatchError       # noqa: E402

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CHAMPION = os.path.join(ROOT, "experiments", "champion_{n}p.json")


# --------------------------------------------------------------- the bot

def load_bot(num_players, path=None, seed=0):
    """The strongest bot we have: hill-climbed champion weights if trained."""
    path = path or CHAMPION.format(n=num_players)
    weights, source = None, "built-in default weights"
    if os.path.exists(path):
        try:
            weights = W.load_weights(path)
            source = os.path.relpath(path, ROOT)
        except Exception as exc:                       # corrupt / half-written
            source = f"built-in defaults ({os.path.basename(path)}: {exc})"
    bot = W.WeightedBot(weights, rng=random.Random(seed))
    bot.source = source
    return bot


# ------------------------------------------------------ describing moves

def _cost_note(state, p, move):
    """The price tag of a move, in the units a player pays at the table."""
    try:
        kind = move[0]
        if kind == "take":
            return f"{A.take_cost(state, p, move[1])} civil action(s)"
        if kind == "pop":
            return f"{economy.pop_cost(state, p)} food, 1 civil action"
        if kind == "build":
            c = A.build_cost_net(state, p, move[1])
            unit = A.is_unit(move[1])
            return (f"{c} resources, 1 "
                    f"{'military' if unit else 'civil'} action")
        if kind == "upgrade":
            c = A.upgrade_cost_net(state, p, move[1], move[2])
            unit = A.is_unit(move[2])
            return (f"{c} resources, 1 "
                    f"{'military' if unit else 'civil'} action")
        if kind == "wonder_step":
            c = sum(A.wonder_stage_cost(state, p, k)
                    for k in range(1, move[1] + 1)) if move[1] > 1 else \
                A.wonder_stage_cost(state, p, 1)
            return f"{c} resources, 1 civil action"
        if kind == "develop":
            return f"{effects.tech_cost(state, p, move[1])} science, 1 civil action"
        if kind == "revolution":
            return f"{C.db().get(move[1]).get('revolutionCost')} science, all civil actions"
        if kind == "play_leader":
            return "1 civil action"
        if kind == "play_action":
            return "1 civil action"
        if kind in ("play_tactic",):
            return "1 military action"
        if kind == "copy_tactic":
            return "2 military actions"
        if kind in ("aggression", "war"):
            cost = (C.db().get(move[1]).get("cost") or {}).get(
                "militaryActions", 0)
            return f"{cost} military action(s)"
    except Exception:
        pass
    return ""


def describe_move(state, move, board=None):
    """Plain English for one move.  Unknown move types degrade gracefully."""
    try:
        return _describe(state, move, board)
    except Exception:
        return " ".join(str(x) for x in move)


def _who(board, idx):
    if board is not None and idx == board.me:
        return f"p{idx} (you)"
    return f"p{idx}"


def _describe(state, move, board=None):
    p = state.actor()
    kind = move[0]
    note = _cost_note(state, p, move)
    tail = f"  [{note}]" if note else ""

    if kind == "take":
        idx = move[1]
        name = state.card_row[idx]
        card = C.db().get(name)
        return (f"TAKE '{name}' ({card['type']}, age {card['age']}) "
                f"from row slot {idx}{tail}")
    if kind == "pop":
        return f"INCREASE POPULATION: move a yellow token to your unused pile{tail}"
    if kind == "pop_free":
        return "INCREASE POPULATION for free (Ocean Liners / leader ability)"
    if kind == "build":
        return f"BUILD '{move[1]}': put an unused worker on it{tail}"
    if kind == "upgrade":
        return f"UPGRADE '{move[1]}' -> '{move[2]}'{tail}"
    if kind == "destroy":
        verb = "DISBAND" if A.is_unit(move[1]) else "DESTROY"
        return f"{verb} '{move[1]}': the worker goes back to your unused pile"
    if kind == "wonder_step":
        w = p.wonder
        stages = C.db().get(w.name)["stages"] if w else []
        return (f"BUILD WONDER '{w.name if w else '?'}' step "
                f"{(w.steps_built + 1) if w else 1}/{len(stages)}"
                f"{' x%d' % move[1] if move[1] > 1 else ''}{tail}")
    if kind == "play_leader":
        return f"PLAY LEADER '{move[1]}'{tail}"
    if kind == "develop":
        card = C.db().get(move[1])
        return f"DEVELOP '{move[1]}' ({card['type']}, age {card['age']}){tail}"
    if kind == "revolution":
        return f"REVOLUTION to '{move[1]}'{tail}"
    if kind == "churchill":
        return f"CHURCHILL: take the {move[1]} bonus"
    if kind == "play_tactic":
        return f"PLAY TACTIC '{move[1]}'{tail}"
    if kind == "copy_tactic":
        return f"COPY TACTIC '{move[1]}' from the common area{tail}"
    if kind == "play_action":
        return f"PLAY ACTION CARD '{move[1]}'{tail}"
    if kind == "end_turn":
        return "END YOUR TURN (production, then pass the board on)"
    if kind == "pol_pass":
        return "PASS on politics (play no military card this turn)"
    if kind == "prepare_event":
        return f"PREPARE EVENT '{move[1]}' (into the future events deck)"
    if kind == "aggression":
        return f"AGGRESSION '{move[1]}' against {_who(board, move[2])}{tail}"
    if kind == "war":
        return f"DECLARE WAR '{move[1]}' on {_who(board, move[2])}{tail}"
    if kind == "offer_pact":
        side = f" (side {move[3]})" if len(move) > 3 and move[3] else ""
        return f"OFFER PACT '{move[1]}' to {_who(board, move[2])}{side}"
    if kind == "cancel_pact":
        return f"CANCEL the pact owned by {_who(board, move[1])}"
    if kind == "resign":
        return "RESIGN from the game"
    # interactive/pending decisions
    if kind == "choose":
        pend = state.pending[-1] if state.pending else None
        opts = (pend or {}).get("options") or []
        if move[1] < len(opts):
            return f"CHOOSE: {_option_text(opts[move[1]])}"
        return f"CHOOSE option {move[1]}"
    if kind == "bid":
        return f"BID {move[1]} military strength"
    if kind == "bid_pass":
        return "PASS on the bid"
    if kind == "defend":
        return f"ADD '{move[1]}' to your defence"
    if kind == "defend_done":
        return "DEFENCE DONE (play no more military cards)"
    return " ".join(str(x) for x in move)


def _option_text(opt):
    if isinstance(opt, dict):
        return opt.get("label") or opt.get("text") or str(opt)
    if isinstance(opt, (list, tuple)):
        return " ".join(str(x) for x in opt)
    return str(opt)


# ------------------------------------------------------------- reasoning

# feature -> how a human would say it (positive direction)
FEATURE_WORDS = {
    "culture": "culture", "culture_rate": "culture/turn",
    "science": "science", "science_rate": "science/turn",
    "strength": "military strength", "strength_rel": "strength vs the leader",
    "strength_lead": "military lead", "strength_deficit": "military deficit",
    "food_rate": "food/turn", "resource_rate": "resources/turn",
    "food_stock": "food", "resource_stock": "resources",
    "workers": "workers on cards", "prod_workers": "farm/mine workers",
    "urban_workers": "urban workers", "unit_workers": "military units",
    "free_workers": "unused workers", "yellow_bank": "population left",
    "civil_actions": "civil actions", "military_actions": "military actions",
    "ca_left": "unspent civil actions", "ma_left": "unspent military actions",
    "happy_margin": "happiness margin", "discontent": "discontent",
    "uprising": "uprising risk", "corruption_loss": "corruption",
    "consumption": "food consumption", "pop_cost": "cost of new population",
    "wonders": "completed wonders", "wonder_progress": "wonder progress",
    "wonder_remaining": "wonder cost left", "tech_levels": "technology level",
    "num_techs": "technologies", "special_techs": "special technologies",
    "gov_level": "government level", "leader": "leader in play",
    "hand_civil": "civil cards in hand", "hand_value": "value of your hand",
    "hand_military": "military cards in hand",
    "hand_mil_value": "value of your military hand",
    "tactic_level": "tactic level", "colonies": "colonies", "pacts": "pacts",
    "blue_free": "blue tokens in your bank",
    "best_farm": "farm level", "best_mine": "mine level",
    "best_lab": "lab level", "best_temple": "temple level",
    "best_library": "library level", "best_theater": "theater level",
    "best_arena": "arena level", "best_unit": "best unit level",
    "rival_culture": "rival culture", "rival_culture_rate": "rival culture/turn",
    "rival_science_rate": "rival science/turn", "rival_strength": "rival strength",
    "rival_mean_culture": "average rival culture",
}


def explain(before, after, weights, top=3):
    """Turn a feature delta into a short reason, best contributions first."""
    parts = []
    for k, v1 in after.items():
        d = v1 - before.get(k, 0)
        if not d:
            continue
        w = weights.get(k, 0.0)
        if not w:
            continue
        parts.append((abs(w * d), k, d))
    parts.sort(reverse=True)
    words = []
    for _, k, d in parts[:top]:
        label = FEATURE_WORDS.get(k, k.replace("_", " "))
        sign = "+" if d > 0 else ""
        val = int(d) if float(d).is_integer() else round(d, 1)
        words.append(f"{sign}{val} {label}")
    return ", ".join(words) if words else "keeps your options open"


# ------------------------------------------------------------- candidates

class Candidate:
    def __init__(self, move, score, text, reason):
        self.move = move
        self.score = score
        self.text = text
        self.reason = reason

    def __repr__(self):
        return f"<Candidate {self.move} {self.score:.2f}>"


def rank_moves(board, bot, top=3, include_end_turn=True):
    """Score every legal move with the bot's own evaluation.

    Returns the best ``top`` candidates, each with the score the bot gives
    the resulting position and a plain-English reason.
    """
    from engine.bots.fastcopy import copy_state
    st = board.state
    moves = G.legal_moves(st)
    if not moves:
        return []
    idx = st.decider()
    moves = [m for m in moves if m[0] != "resign"] or moves
    try:
        ctx = W.rival_context(st, idx)
    except Exception:
        ctx = None
    try:
        before = W.features(st, idx, ctx)
    except Exception:
        before = {}
    w = bot.weights
    end_bias = w.get("end_turn_bias", 0.0)
    scored = []
    for mv in moves:
        trial = copy_state(st)
        try:
            A.apply(trial, mv, random.Random(0))
            after = W.features(trial, idx, ctx)
            val = W.evaluate(trial, idx, w, ctx, after)
        except Exception:
            continue                       # unscorable candidate, never fatal
        if mv[0] == "end_turn":
            val += end_bias
            if not include_end_turn and len(moves) > 1:
                continue
        scored.append((val, mv, after))
    if not scored:
        mv = moves[0]
        return [Candidate(mv, 0.0, describe_move(st, mv, board),
                          "only move the engine could score")]
    base = max(v for v, _, _ in scored)
    scored.sort(key=lambda t: -t[0])
    out = []
    for val, mv, after in scored[:top]:
        out.append(Candidate(mv, val - base, describe_move(st, mv, board),
                             explain(before, after, w)))
    return out


# ---------------------------------------------------- parsing human moves

MOVE_ALIASES = {
    "t": "take", "take": "take",
    "b": "build", "build": "build",
    "u": "upgrade", "up": "upgrade", "upgrade": "upgrade",
    "d": "develop", "dev": "develop", "develop": "develop",
    "pop": "pop", "population": "pop",
    "w": "wonder_step", "wonder": "wonder_step", "step": "wonder_step",
    "leader": "play_leader", "l": "play_leader",
    "action": "play_action", "card": "play_action",
    "tactic": "play_tactic",
    "copy": "copy_tactic",
    "gov": "revolution", "revolution": "revolution", "rev": "revolution",
    "destroy": "destroy", "disband": "destroy",
    "end": "end_turn", "e": "end_turn", "done": "end_turn",
    "pass": "pol_pass", "p": "pol_pass",
    "agg": "aggression", "attack": "aggression",
    "war": "war", "pact": "offer_pact", "event": "prepare_event",
    "choose": "choose", "bid": "bid", "defend": "defend",
}


def _arg_matches(elem, arg):
    if isinstance(elem, int):
        return arg.isdigit() and int(arg) == elem
    if isinstance(elem, str):
        a, e = S._norm(arg), S._norm(elem)
        return e.startswith(a) or S._is_subseq(a, e) or \
            S._initials(elem).startswith(a)
    return False


def _move_tokens(state, move):
    """Everything a human might name when picking this move: the move's own
    arguments plus, for row moves, the name of the card in that slot."""
    toks = list(move[1:])
    if move[0] == "take" and isinstance(move[1], int):
        name = state.card_row[move[1]] if move[1] < len(state.card_row) else None
        if name:
            toks.append(name)
    if move[0] == "wonder_step" and state.actor().wonder:
        toks.append(state.actor().wonder.name)
    return toks


def _why_not(state, kind, arg):
    """A useful message when a move the human named is not legal."""
    if kind == "take" and arg.isdigit():
        slot = int(arg)
        if not 0 <= slot < len(state.card_row):
            return f"row slot {slot} does not exist (0..{len(state.card_row) - 1})"
        name = state.card_row[slot]
        if name is None:
            return f"row slot {slot} is empty"
        try:
            cost = A.take_cost(state, state.actor(), slot)
            return (f"you cannot take '{name}' from slot {slot}: it costs "
                    f"{cost} civil actions and you have "
                    f"{state.actor().civil_actions}")
        except Exception:
            return f"you cannot take '{name}' from slot {slot} right now"
    return f"no legal {kind} matches {arg!r}"


def parse_move(state, text, board=None):
    """Turn what the human typed into one of the legal moves.

    Verb-first and fuzzy: ``t 4``, ``build bronze``, ``dev philo``, ``end``.
    Raises :class:`PatchError` listing the possibilities when unsure.
    """
    moves = G.legal_moves(state)
    toks = text.split()
    if not toks:
        raise PatchError("type a move, or just press Enter for the top pick")
    kinds = sorted({m[0] for m in moves})
    verb = toks[0].lower().rstrip(":")
    kind = MOVE_ALIASES.get(verb)
    if kind not in kinds:
        hits = [k for k in kinds if k.startswith(verb)] or \
               [k for k in kinds if verb in k]
        if len(hits) == 1:
            kind = hits[0]
        elif len(hits) > 1:
            raise PatchError(f"{verb!r} could be: {', '.join(hits)}")
        elif kind is None:
            raise PatchError(f"no legal move called {verb!r}. "
                             f"Legal now: {', '.join(kinds)}")
        else:
            raise PatchError(f"{kind!r} is not legal right now. "
                             f"Legal now: {', '.join(kinds)}")
    cands = [m for m in moves if m[0] == kind]
    for arg in toks[1:]:
        cands = [m for m in cands
                 if any(_arg_matches(e, arg) for e in _move_tokens(state, m))]
        if not cands:
            raise PatchError(_why_not(state, kind, arg))
    if len(cands) == 1:
        return cands[0]
    if len(cands) > 1:
        opts = "\n   ".join(describe_move(state, m, board) for m in cands[:8])
        raise PatchError(f"which one?\n   {opts}")
    raise PatchError(f"no legal {kind} move")


# ------------------------------------------------------------- the session

PATCH_VERBS = ("deal", "row", "event", "age", "last", "last_round", "set")


def _looks_like_patch(line):
    """Is this a board update rather than a move or a list of card names?

    Lets the human type 'take p1 3' / 'p1 c=34' at ANY prompt instead of
    having to track which prompt they are at.  ``take 4`` (a move) and
    ``take p1 4`` (a rival took a card) are told apart by the p<N>.
    """
    import re as _re
    toks = line.split()
    if not toks:
        return False
    first = toks[0].lower()
    if _re.fullmatch(r"p\d+", first) or "=" in first:
        return True
    if first in PATCH_VERBS:
        return True
    if first == "take":
        return len(toks) >= 2 and bool(_re.fullmatch(r"p\d+", toks[1].lower()))
    return False


def _new_slots(before, after):
    """Row slots holding a card that was NOT in the row before.

    The row slides left when it is replenished, so a positional diff would
    flag almost every slot; what we want is the multiset difference, matched
    to the rightmost occurrences (new cards are always dealt on the right).
    """
    from collections import Counter
    added = Counter(n for n in after if n) - Counter(n for n in before if n)
    slots = []
    for i in range(len(after) - 1, -1, -1):
        name = after[i]
        if name and added.get(name, 0) > 0:
            added[name] -= 1
            slots.append(i)
    return sorted(slots)


class Advisor:
    """Board mirror + bot + the operations the interactive loop performs.

    Kept free of I/O so it can be driven by tests as well as by a terminal.
    """

    def __init__(self, board, bot=None, seed=0):
        self.board = board
        self.bot = bot or load_bot(board.state.num_players, seed=seed)
        self.rng = random.Random(seed ^ 0xA5D)
        self.log = []

    # -- queries
    @property
    def state(self):
        return self.board.state

    def my_turn(self):
        st = self.state
        return not st.game_over and st.decider() == self.board.me

    def recommend(self, top=3):
        return rank_moves(self.board, self.bot, top=top)

    # -- mutation
    def play(self, move):
        """Apply a move to the mirror; returns (ok, message)."""
        st = self.state
        legal = G.legal_moves(st)
        if move not in legal and list(move) not in [list(m) for m in legal]:
            return False, f"{move!r} is not legal right now"
        text = describe_move(st, move, self.board)
        row_before = list(st.card_row)
        try:
            G.apply(st, move, self.rng)
        except Exception as exc:
            return False, f"the engine refused that move: {exc}"
        self.log.append(text)
        self.dealt_slots = _new_slots(row_before, st.card_row)
        return True, text

    def skip_opponent_turn(self):
        """Hand the turn on without simulating the opponent's decisions.

        The human reports the *result* of their turn as patches; the engine
        still does the book-keeping (turn order, round and age progression,
        the end-of-turn production of the player whose turn it was).
        """
        st = self.state
        row_before = list(st.card_row)
        guard = 0
        while not st.game_over and guard < 40:
            guard += 1
            if st.pending:
                moves = G.legal_moves(st)
                G.apply(st, moves[0], self.rng)
                continue
            if st.phase == "politics":
                G.apply(st, ("pol_pass",), self.rng)
                continue
            break
        if not st.game_over:
            who = st.current
            G.apply(st, ("end_turn",), self.rng)
            self.log.append(f"p{who} turn ended")
        self.dealt_slots = _new_slots(row_before, st.card_row)
        return self.dealt_slots

    def set_dealt(self, names):
        """Replace the cards the engine guessed in the freshly dealt slots."""
        slots = getattr(self, "dealt_slots", [])
        if not slots:
            raise PatchError("no cards were dealt since the last update")
        if len(names) > len(slots):
            raise PatchError(f"only {len(slots)} card(s) were dealt "
                             f"(slots {', '.join(str(s) for s in slots)})")
        row = list(self.state.card_row)
        for slot, raw in zip(slots, names):
            row[slot] = S.resolve_card(raw, "row")
        S.sync_row(self.board, row)
        self.dealt_slots = []
        return [row[s] for s in slots[:len(names)]]

    def patch(self, line):
        return S.patch(self.board, line)


# ------------------------------------------------------------------- REPL

class _Quit(Exception):
    """The human asked to leave."""


BANNER = """\
Through the Ages advisor.  Commands at the 'your move' prompt:

  <Enter>      play the top recommendation
  1 / 2 / 3    play that numbered recommendation
  take 4       play your own move (verb + fuzzy args), e.g.
               build bronze | dev philosophy | wonder | end | pass
  more         show more candidate moves
  board        print the full board
  state        print the raw snapshot (paste-able)
  p1 c=34      correct the board at any prompt (see the update syntax
               below); 'set <line>' works too
  undo         undo back to the start of your turn
  help         this text
  quit         leave

At the 'what happened' prompt type update lines (blank line = done):
""" + S.PATCH_HELP


class Console:
    def __init__(self, adv, inp=input, out=print):
        self.adv = adv
        self.inp = inp
        self.out = out
        self._snapshot = None

    # ---- io helpers
    def ask(self, prompt):
        try:
            return self.inp(prompt)
        except EOFError:
            return "quit"

    def say(self, *args):
        self.out(*args)

    # ---- main loop
    def run(self):
        self.say(BANNER)
        self.say(f"bot: {self.adv.bot.source}")
        self.say(S.render(self.adv.board))
        try:
            while not self.adv.state.game_over:
                if self.adv.my_turn():
                    if not self.my_turn():
                        return
                else:
                    if not self.opponent_turn():
                        return
        except _Quit:
            self.say("bye -- the snapshot below restores this game:")
            self.say(S.dumps(self.adv.board))
            return
        self.say("\ngame over.  final culture: "
                 + ", ".join(f"p{i}={s}" for i, s in
                             enumerate(G.scores(self.adv.state))))
        return True

    # ---- your turn
    def my_turn(self):
        adv = self.adv
        self._snapshot = S.dumps(adv.board)
        self.check_dealt()
        while adv.my_turn() and not adv.state.game_over:
            cands = adv.recommend(3)
            if not cands:
                return True
            self.show_candidates(cands)
            line = self.ask("your move> ").strip()
            if not self.handle_move_input(line, cands):
                return False
        return True

    def show_candidates(self, cands):
        st = self.adv.state
        p = st.actor()
        self.say(f"\n-- your turn (round {st.round}, age {st.age_civil}): "
                 f"CA {p.civil_actions}, MA {p.military_actions}, "
                 f"food {p.food}, res {p.resources}, sci {p.science}")
        for i, c in enumerate(cands, 1):
            mark = "*" if i == 1 else " "
            gap = "" if i == 1 else f"  ({c.score:+.1f})"
            self.say(f" {mark}{i}. {c.text}{gap}")
            self.say(f"       why: {c.reason}")

    def handle_move_input(self, line, cands):
        adv = self.adv
        low = line.lower()
        if low in ("quit", "q", "exit"):
            return False
        if low in ("help", "?", "h"):
            self.say(BANNER)
            return True
        if low == "board":
            self.say(S.render(adv.board))
            return True
        if low == "state":
            self.say(S.dumps(adv.board))
            return True
        if low == "more":
            for c in adv.recommend(10)[3:]:
                self.say(f"    - {c.text}  ({c.score:+.1f})  why: {c.reason}")
            return True
        if low == "undo":
            if self._snapshot:
                board = S.loads(self._snapshot)
                adv.board.state = board.state
                # hidden-card counts ride on the state itself now
                adv.board.unknown = board.unknown
                self.say("rolled back to the start of your turn")
            return True
        if _looks_like_patch(line):
            # a board correction typed at the move prompt
            self.report(line)
            return True
        move = None
        if line == "":
            move = cands[0].move
        elif line.isdigit() and 1 <= int(line) <= len(cands):
            move = cands[int(line) - 1].move
        else:
            try:
                move = parse_move(adv.state, line, adv.board)
            except PatchError as exc:
                self.say(f"  ! {exc}")
                return True
        ok, msg = adv.play(move)
        self.say(("  -> " if ok else "  ! ") + msg)
        if ok and move[0] == "end_turn":
            self.after_my_turn()
        return True

    def after_my_turn(self):
        self.say("\nyour turn is over.  Anything to correct on YOUR board "
                 "(military cards drawn, event effects)?")
        self.collect_updates()

    # ---- opponents
    def opponent_turn(self):
        adv = self.adv
        who = adv.state.decider()
        self.check_dealt()
        self.say(f"\n-- p{who}'s turn.  Tell me what they did "
                 f"(blank line when done, 'help' for the syntax):")
        if not self.collect_updates():
            return False
        adv.skip_opponent_turn()
        return True

    def collect_updates(self):
        while True:
            line = self.ask("  > ").strip()
            if line == "":
                return True
            if line.lower() in ("quit", "q", "exit"):
                return False
            if line.lower() in ("help", "?"):
                self.say(S.PATCH_HELP)
                continue
            if line.lower() == "board":
                self.say(S.render(self.adv.board))
                continue
            self.report(line)

    def report(self, line):
        line = line.strip()
        if line.lower().startswith("set "):
            line = line[4:].strip()
        try:
            msg = self.adv.patch(line)
            if msg:
                self.say(f"    ok: {msg}")
        except PatchError as exc:
            self.say(f"    ! {exc}  (type '?' for the syntax, or use '?' as a "
                     f"value if you don't know it)")

    def check_dealt(self):
        """Ask which cards were actually dealt into the row."""
        slots = getattr(self.adv, "dealt_slots", [])
        if not slots:
            return
        where = ", ".join(str(s) for s in slots)
        self.say(f"\n{len(slots)} new card(s) in row "
                 f"{'slot' if len(slots) == 1 else 'slots'} {where}.")
        while True:
            line = self.ask("  new cards (left to right, '?' if unseen)> ")
            line = line.strip()
            if line.lower() in ("quit", "q", "exit"):
                raise _Quit()
            if line.lower() in ("help", "h"):
                self.say(S.PATCH_HELP)
                continue
            if line.lower() == "board":
                self.say(S.render(self.adv.board))
                continue
            if line in ("", "?"):
                self.adv.board.unknown.add("row.new")
                self.adv.dealt_slots = []
                return
            if _looks_like_patch(line):
                # the human is already reporting the rest of the turn
                self.report(line)
                continue
            try:
                got = self.adv.set_dealt(S._split_cards(line, "row"))
                self.say(f"    ok: {', '.join(got)}")
                return
            except PatchError as exc:
                self.say(f"    ! {exc}")


# ------------------------------------------------------------------- main

def main(argv=None):
    ap = argparse.ArgumentParser(description="Through the Ages advisor")
    ap.add_argument("--players", type=int, default=3, help="2, 3 or 4")
    ap.add_argument("--seat", type=int, default=0,
                    help="your seat, 0 = start player")
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--weights", default=None, help="bot weight JSON")
    ap.add_argument("--load", default=None, help="resume from a snapshot file")
    args = ap.parse_args(argv)

    if args.load:
        with open(args.load) as fh:
            board = S.loads(fh.read())
    else:
        board = S.new_board(args.players, me=args.seat, seed=args.seed)
    bot = load_bot(board.state.num_players, args.weights, seed=args.seed)
    adv = Advisor(board, bot, seed=args.seed)
    if not args.load:
        # the physical row was dealt by the real deck; take it from the human
        adv.dealt_slots = list(range(A.ROW_SIZE))
    Console(adv).run()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
