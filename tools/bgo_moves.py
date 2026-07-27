"""Replay a BGO journal MOVE BY MOVE and emit (state, legal moves, human move).

`tools/bgo_rescore.py` rebuilds a human *tableau* so the end-of-game scorer can
be diffed against BGO's printed integers.  It never builds a position anybody
could move in: no card row, no hands, no action budget, no turn structure.  For
behaviour cloning that is the whole thing you need, so this is a second
replayer with a different contract:

    for every human decision, produce a real `GameState`, the engine's
    `legal_moves()` for it, and which of those moves the human played.

Three design decisions carry the fidelity, and all three are measurable:

1.  **The engine is not driven by its own turn loop.**  `engine.game` deals its
    own cards, draws its own events and resolves its own wars, none of which
    match the human game.  Here the turn loop is local: the row is replenished
    by hand, the action budget is set from `effects.state_stats`, and each
    human action is applied with `engine.actions.apply` -- which is the only
    part that has to be right, because it is the part behaviour cloning uses.

2.  **Every stock is resynced from BGO at the end of every turn.**  The journal
    prints culture, science, food and resources -- both the per-turn production
    and the resulting stock -- on every `End turn` line.  `bgo_rescore` never
    reads the stock half, which is why its agreement decays from 99.1% on turn
    1-5 to 58% by turn 16 (docs/SCORE_VALIDATION.md §1).  Resyncing bounds the
    drift to one turn: a turn is wrong only if something went wrong *in it*.

3.  **The card row is imputed, and that is reported rather than hidden.**  The
    row is the one thing the journal never prints (docs/HUMAN_BASELINE.md,
    "What this cannot tell you").  Cards are dealt from a correctly-composed,
    shuffled age deck; when the human takes a card that is not in our row it is
    swapped into a slot of the civil-action cost BGO logged, and the card it
    displaced goes back to the deck.  So the row is always *consistent with
    every take the human made* and is a determinization everywhere else.  The
    human's own move is therefore always available; some counterfactual takes
    are cards they never saw.  `--stats` prints the share of takes that hit a
    card we had already dealt to the right cost tier.

A turn is CLEAN, and its decisions are emitted, only when at the end of it

  * our five production numbers equal BGO's five printed ones, and
  * our four stocks equal BGO's four printed ones before the resync, and
  * yellow tokens are conserved, and
  * nothing in the turn needed a manual patch (war, aggression, auction,
    annex/infiltrate/raid, an unparsed line, an illegal reconstructed move).

Usage:

    python3 tools/bgo_moves.py --journals /tmp/bgo/journals --players 2 --stats
    python3 tools/bgo_moves.py --journals /tmp/bgo/journals --players 2 \
        --emit /tmp/bc.jsonl --limit 200
"""
from __future__ import annotations

import argparse
import json
import os
import random
import re
import sys
from collections import Counter, defaultdict

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine import actions, cards as C, economy, effects, game, journal  # noqa: E402
from engine.bots import weighted as W                                    # noqa: E402
from engine.state import GameState, PlayerState, TechCard                # noqa: E402
from tools.bgo_parse import (NAME_FIXES, COLOURS, WONDER_STAGES,         # noqa: E402
                             _ELECTABLE, _leader_prefix)

_DB = C.db()
_COL = "|".join(COLOURS)

#: the reference vector the four non-linear terms are priced through, and the
#: parameter vocabulary the emitted files are keyed on (integers, so a 20k-
#: example file stays a few tens of megabytes rather than a few hundred).
_REF = dict(W.DEFAULT_WEIGHTS)
PARAMS = sorted(W.DEFAULT_WEIGHTS)
PARAM_IX = {k: i for i, k in enumerate(PARAMS)}

AGE_ORDER = {"A": 0, "I": 1, "II": 2, "III": 3, "IV": 4}
GOVERNMENTS = ("Despotism", "Monarchy", "Theocracy", "Republic",
               "Constitutional Monarchy", "Democracy", "Fundamentalism",
               "Communism")
LOCAL_FIXES = {"Warrior": "Warriors"}


def fix(name):
    name = name.strip()
    name = NAME_FIXES.get(name, name)
    return LOCAL_FIXES.get(name, name)


#: base name -> the age-suffixed variants `engine.cards._disambiguate` made.
#: ~15 civil action names exist in two to four decks (Urban Growth is in A, I,
#: II and III) and the journal prints only the base name, so every name coming
#: out of a journal line has to be resolved against a context: the row for a
#: take, the hand for a play, the tableau for a build.
_BASE = defaultdict(list)
for _n, _c in _DB.by_name.items():
    _BASE[_c.get("baseName", _n)].append(_n)
for _k in _BASE:
    _BASE[_k].sort(key=lambda n: AGE_ORDER.get(_DB.by_name[n]["age"], 0))


def resolve(name, *contexts, age=None):
    """The concrete card name behind a journal's printed base name.

    `contexts` are containers (the row, a hand, a tableau) searched in order;
    the first variant found in one of them wins.  With no hit the newest
    variant no later than `age` is taken, which is the deck a card would have
    come from.
    """
    if name in _DB.by_name:
        return name
    variants = _BASE.get(name)
    if not variants:
        return name
    for ctx in contexts:
        if ctx is None:
            continue
        for v in variants:
            if v in ctx:
                return v
    if age is not None:
        lv = AGE_ORDER.get(age, 4)
        ok = [v for v in variants
              if AGE_ORDER.get(_DB.by_name[v]["age"], 0) <= lv]
        if ok:
            return ok[-1]
    return variants[0]


# ------------------------------------------------------------- line regexes

RE_ENDTURN = re.compile(
    r"End turn (%s) scores:.*?(-?\d+) culture \(now (-?\d+)\); "
    r"(-?\d+) science \(now (-?\d+)\); (-?\d+) food - consumption: (\d+) "
    r"\(now (-?\d+)\); (-?\d+) resources(?: \(now (-?\d+)\))?" % _COL)
RE_TAKE = re.compile(r"^(%s) takes (.+?) in hand(?: \1 uses (\d+) civil action)?"
                     % _COL)
RE_TAKE_MIL = re.compile(r" uses (\d+) military action")
RE_PUTBACK = re.compile(
    r"^(%s) puts (.+?) back in the row \1 gets (\d+) civil action" % _COL)
RE_STAGE = re.compile(
    r"(%s) builds (\d+) stages? of ([A-Za-z'. ]+?)(?=;|\s(?:%s)\s|$)"
    % (_COL, _COL))
RE_BUILD = re.compile(
    r"^(%s) builds ([A-Za-z'. ]+?)(?: using [A-Za-z'. ]+)?"
    r"(?= \1 (?:spends|loses|produces)|$)" % _COL)
RE_UPGRADE = re.compile(
    r"(%s) upgrades ([A-Za-z'. ]+?) to ([A-Za-z'. ]+?)"
    r"(?:\s+using [A-Za-z'. ]+?)?(?=;|\s\1\s|$)" % _COL)
RE_DISCOVER = re.compile(
    r"^(%s) discovers ([A-Za-z' ]+?)(?: using | \1 loses|$)" % _COL)
RE_DESTROY = re.compile(r"^(%s) (?:destroys|disbands) ([A-Za-z'. ]+?)$" % _COL)
RE_REVOL = re.compile(
    r"^(%s) revolutions (?:using \w[\w ]*? )?Change government to ([A-Za-z ]+?);"
    % _COL)
RE_ELECT = re.compile(r"^(%s) elects (.+)$" % _COL)
RE_POP = re.compile(r"^(%s) increases population" % _COL)
RE_PLAY = re.compile(r"^(%s) plays ([A-Za-z'. &-]+?)(?= \1 | against |;|$)" % _COL)
RE_TACTIC_NEW = re.compile(r"^(%s) sets up new tactics (?:[AI]{1,3} / )?(.+?)$"
                           % _COL)
RE_TACTIC_OLD = re.compile(r"^(%s) adopts existing tactics (?:[AI]{1,3} / )?(.+?)$"
                           % _COL)
RE_PASSPOL = re.compile(r"^(%s) passes Political Phase" % _COL)
RE_GETPOP = re.compile(r"(%s) gets (\d+) population" % _COL)
RE_LOSEPOP = re.compile(r"(%s) loses (\d+) population" % _COL)
RE_YELLOW_GET = re.compile(r"(%s) gets (\d+) yellow token" % _COL)
RE_YELLOW_LOSE = re.compile(r"(%s) loses (\d+) yellow token" % _COL)
RE_COLONIZE = re.compile(r"^(%s) colonizes a ([A-Za-z ]+Territory)" % _COL)
RE_TERR_AGE = re.compile(r"(A|I{1,3}) / ([A-Za-z ]+Territory)")
RE_CRUMBLE = re.compile(r"^(?:Ravages of Time )?(?:The )?"
                        r"([A-Za-z'. ]+?) crumbles?$")
RE_USING = re.compile(r" using ([A-Za-z'. &-]+?)(?=;| (?:%s) |$)" % _COL)

#: lines that mean this turn's reconstruction cannot be trusted
DIRTY_MARKERS = (
    " declares ", " plays Annex against ", " plays Infiltrate against ",
    "Iconoclasm", "Raid casualties", "Terrorists destroy", "Barbarossa enlists",
    " proposes ", " accepts ", " declines ",
    "UPRISING", "concedes defeat", " annexes ",
)

#: verbs that are a *decision* by the acting player
NOISE_PREFIXES = (
    "No Discard Phase", "Discard Phase", "Action Phase begins",
    "Last turn", "End of game", "Impact of", "All players have",
    "GAME DATA UPDATED", "I have nothing", "Game ", "Current event",
)


# ------------------------------------------------------------------ parsing

class Turn:
    __slots__ = ("colour", "round", "age", "lines", "end",
                 "prod", "stock", "draws", "discards", "corruption", "famine")

    def __init__(self, colour, rnd, age):
        self.colour = colour
        self.round = rnd
        self.age = age
        self.lines = []
        self.end = None          # the raw End turn text
        self.prod = None         # (culture, science, food_net, cons, resources)
        self.stock = None        # (culture, science, food, resources|None)
        self.draws = 0
        self.discards = 0
        self.corruption = 0
        self.famine = 0


def parse_turns(path):
    """Split a journal into per-seat turns, in (round, seat) order."""
    per_colour = defaultdict(list)
    order = []
    open_turn = {}
    with open(path, errors="replace") as fh:
        fh.readline()
        for line in fh:
            parts = line.rstrip("\n").split("\t")
            if len(parts) < 5:
                continue
            colour, age, rnd, text = parts[1], parts[2], parts[3], parts[4]
            if colour not in COLOURS:
                continue
            if colour not in per_colour and colour not in open_turn:
                order.append(colour)
            try:
                rnd = int(rnd)
            except ValueError:
                rnd = 1
            t = open_turn.get(colour)
            if t is None:
                t = open_turn[colour] = Turn(colour, rnd, age)
            m = RE_ENDTURN.search(text)
            if m and m.group(1) == colour:
                t.end = text
                t.prod = (int(m.group(2)), int(m.group(4)), int(m.group(6)),
                          int(m.group(7)), int(m.group(9)))
                t.stock = (int(m.group(3)), int(m.group(5)), int(m.group(8)),
                           int(m.group(10)) if m.group(10) else None)
                mcor = re.search(r"CORRUPTION! %s loses (\d+) resource"
                                 % colour, text)
                t.corruption = int(mcor.group(1)) if mcor else 0
                mhun = re.search(r"(?:FAMINE|HUNGER)! %s loses (\d+) food"
                                 % colour, text)
                t.famine = int(mhun.group(1)) if mhun else 0
                md = re.search(r"draws (\d+) military card", text)
                t.draws = int(md.group(1)) if md else 0
                t.age = age
                # the End turn row carries the authoritative round for the
                # turn: the row that OPENED it may be a cross-player patch
                # line left over from the previous round.
                t.round = rnd
                per_colour[colour].append(t)
                del open_turn[colour]
                continue
            t.lines.append(text)
            if age and AGE_ORDER.get(age, 0) > AGE_ORDER.get(t.age, 0):
                t.age = age
    for colour, t in open_turn.items():
        per_colour[colour].append(t)          # unterminated final turn
    # (round, seat) order.  Each seat's own turns keep their journal order, so
    # a seat that logged two `End turn` lines in one round (it happens: BGO
    # stamps the round off the wall clock) still replays in sequence.
    seat = {c: i for i, c in enumerate(order)}
    out = []
    for c in order:
        for k, t in enumerate(per_colour[c]):
            out.append((t.round, k, seat[c], t))
    out.sort(key=lambda x: (x[0], x[1], x[2]))
    return order, [x[3] for x in out]


# --------------------------------------------------------------- the replay

class Replay:
    """One journal, replayed through the real engine, decision by decision."""

    def __init__(self, path, seed=0, collect=False):
        self.path = path
        self.gid = os.path.basename(path).split(".")[0]
        self.order, self.turns = parse_turns(path)
        self.n = len(self.order)
        self.seat = {c: i for i, c in enumerate(self.order)}
        self.rng = random.Random(seed)
        self.collect = collect
        self.debug = False
        self.examples = []
        self.stat = Counter()
        self.by_round = defaultdict(Counter)
        self.last_terr_age = {}
        self.tokens = {}
        self.seat_clean = {}
        self.turn_dirty = False
        self.state = None

    # -- setup ---------------------------------------------------------
    def setup(self):
        if not (2 <= self.n <= 4):
            raise ValueError("player count %d" % self.n)
        gs = GameState(num_players=self.n, seed=0)
        gs.has_military = _DB.has_military
        for i in range(self.n):
            p = PlayerState(idx=i)
            p.techs = {n: TechCard(n, workers=w)
                       for n, w in game.START_TECHS.items()}
            p.yellow_bank = 18
            p.workers_free = 1
            p.blue_total = 16
            p.civil_actions = i + 1
            p.military_actions = 0
            gs.players.append(p)
        gs.age_civil = "A"
        gs.age_military = "A"
        gs.civil_deck = _DB.civil_deck("A", self.n)
        self.rng.shuffle(gs.civil_deck)
        gs.card_row = [None] * actions.ROW_SIZE
        gs.military_deck = []
        gs.current_events = []
        gs.future_events = []
        self._deal(gs)
        gs.phase = "actions"
        gs.current = 0
        gs.round = 1
        gs.turn = 1
        self.state = gs
        self.tokens = {i: 25 for i in range(self.n)}
        return gs

    # -- row model -----------------------------------------------------
    def _deal(self, gs):
        for i in range(actions.ROW_SIZE):
            if gs.card_row[i] is None and gs.civil_deck:
                gs.card_row[i] = gs.civil_deck.pop()

    def _replenish(self):
        gs = self.state
        k = game.SWEEP[max(2, min(4, self.n))]
        row = gs.card_row
        for i in range(min(k, len(row))):
            row[i] = None
        kept = [c for c in row if c is not None]
        gs.card_row = kept + [None] * (actions.ROW_SIZE - len(kept))
        self._deal(gs)

    def _advance_age_to(self, age):
        """Force the civil/military age from the journal's own age column."""
        gs = self.state
        while AGE_ORDER.get(age, 0) > AGE_ORDER.get(gs.age_civil, 0):
            ended = gs.age_civil
            nxt = C.AGES[C.level(ended) + 1]
            if ended != "A":
                game._antiquate(gs, C.level(ended))
                for p in gs.players:
                    lost = min(2, p.yellow_bank)
                    p.yellow_bank -= lost
                    self.tokens[p.idx] -= lost
            gs.age_civil = nxt
            gs.age_military = nxt
            if nxt == "IV":
                gs.civil_deck = []
                gs.military_deck = []
                gs.final_round_end = gs.round
            else:
                gs.civil_deck = _DB.civil_deck(nxt, self.n)
                self.rng.shuffle(gs.civil_deck)
                gs.military_deck = _DB.military_deck(nxt, self.n) \
                    if _DB.has_military else []
                self.rng.shuffle(gs.military_deck)
            effects.invalidate(gs)

    #: row slots whose §2.3 civil-action cost is 1 / 2 / 3
    TIER_SLOTS = {1: range(0, 5), 2: range(5, 9), 3: range(9, 13)}

    def _inject_row(self, name, tier):
        """Put `name` into a row slot whose civil-action cost is `tier`.

        The journal prints what the take COST, which pins the slot's tier but
        not the slot.  A card we happen to have dealt at the wrong tier is
        swapped with one at the right tier rather than taken where it lies:
        the cost is the observation, the position is the guess.
        """
        gs = self.state
        band = self.TIER_SLOTS.get(tier, range(0, 5))
        here = gs.card_row.index(name) if name in gs.card_row else None
        if here is not None:
            self.stat["take_in_row"] += 1
            if here in band:
                self.stat["take_tier_ok"] += 1
                return here
        # A UNIFORM slot inside the tier, not the leftmost.  Putting every
        # injected card at the first slot of its band was worth ~9 points to a
        # "take the leftmost legal card" baseline, purely as an artefact of
        # where this function chose to put it -- the journal pins the COST,
        # which is the band, and says nothing about the slot.
        free = [i for i in band if gs.card_row[i] is not None]
        if free:
            i = free[self.rng.randrange(len(free))]
            if here is not None:
                gs.card_row[here], gs.card_row[i] = gs.card_row[i], name
                self.stat["take_moved"] += 1
            else:
                gs.civil_deck.append(gs.card_row[i])
                self.rng.shuffle(gs.civil_deck)
                gs.card_row[i] = name
                self.stat["take_injected"] += 1
            return i
        if here is not None:
            return here
        for i in range(actions.ROW_SIZE):
            if gs.card_row[i] is not None:
                gs.civil_deck.append(gs.card_row[i])
                gs.card_row[i] = name
                self.stat["take_injected_anywhere"] += 1
                return i
        gs.card_row[0] = name
        self.stat["take_injected_anywhere"] += 1
        return 0

    def _inject_military(self, p, name):
        if name in p.hand_military:
            return True
        if name not in _DB.by_name:
            return False
        p.hand_military.append(name)
        if name in self.state.military_deck:
            self.state.military_deck.remove(name)
        return True

    # -- helpers -------------------------------------------------------
    def _tokens_ok(self, p):
        have = p.yellow_bank + p.workers_free + \
            sum(t.workers for t in p.techs.values())
        return have == self.tokens[p.idx]

    def _grant_tokens(self, p, k):
        p.yellow_bank += k
        self.tokens[p.idx] += k

    # -- the turn ------------------------------------------------------
    def run(self):
        gs = self.setup()
        seen_end = 0
        for t in self.turns:
            if t.colour not in self.seat:
                continue
            idx = self.seat[t.colour]
            p = gs.players[idx]
            gs.current = idx
            gs.round = t.round
            if t.round > 1:
                self._advance_age_to(t.age)
                self._replenish()
            self._start_turn(p, t)
            buf = []
            dirty = self._play_turn(p, t, buf)
            ok = self._settle(p, t, dirty, buf)
            self.stat["turns"] += 1
            self.stat["turns_clean"] += int(ok)
            b = min((t.round - 1) // 4, 5)
            self.by_round[b]["n"] += 1
            self.by_round[b]["ok"] += int(ok)
            if t.end is not None:
                seen_end += 1
        self.stat["end_turns"] = seen_end
        return self.stat

    def _start_turn(self, p, t):
        gs = self.state
        s = effects.state_stats(gs, p)
        if gs.round == 1:
            p.civil_actions = p.idx + 1
            p.military_actions = 0
        else:
            p.civil_actions = max(0, s.civil_actions - p.ca_penalty_next_turn)
            p.military_actions = s.military_actions
        p.ca_penalty_next_turn = 0
        p.politics_done = False
        p.taken_this_turn = []
        p.ca_spent_taking = 0
        p.tactic_action_used = False
        p.hammurabi_used = False
        p.churchill_used = False
        p.bach_upgrade_used = False
        p.ocean_liners_used = False
        p.mil_discount = 0
        p.one_time_discount = {}
        p.caesar_double_politics_used = False
        gs.phase = "politics" if (gs.round > 1 and gs.has_military) else "actions"
        gs.last_round = (gs.final_round_end is not None
                         and gs.round >= gs.final_round_end)

    def _play_turn(self, p, t, buf):
        """Apply every logged action of this turn.  Returns True if dirty."""
        gs = self.state
        dirty = False
        self.turn_dirty = False
        for text in t.lines:
            try:
                d = self._line(p, t, text, buf)
            except Exception as exc:                      # noqa: BLE001
                self.stat["line_crash"] += 1
                self.stat["crash:" + type(exc).__name__] += 1
                if self.debug:
                    print("      CRASH", type(exc).__name__, exc, "|", text[:90])
                d = True
            if d and self.debug:
                print("    dirty <-", text[:120])
            dirty = dirty or d
            self.turn_dirty = dirty
        # END TURN IS A DECISION, AND THE MOST IMPORTANT ONE.  The human
        # stopped acting here; every candidate still on the table is a civil
        # or military action they chose NOT to spend.  Without this row the
        # training set contains `end_turn` as a candidate in every example and
        # never as an answer, and the fitted vector learns to never stop --
        # which is the exact failure `docs/WASTED_ACTIONS.md` is about, in
        # reverse.  It is recorded and NOT applied: this replayer owns the
        # turn loop, so `game.end_turn` must not run.
        if gs.phase == "politics":
            self._try(p, ("pol_pass",), buf, t)
        self._pending_flush(p, buf, t)
        if not gs.pending and gs.phase == "actions" and t.end is not None:
            try:
                legal = actions.legal_moves(gs)
            except Exception:                             # noqa: BLE001
                legal = []
            end = ("end_turn",)
            if end in legal and len(legal) > 1:
                self.stat["legal:end_turn"] += 1
                self.stat["legal"] += 1
                snap = (self._snapshot(gs, p.idx, legal, end)
                        if self.collect and not dirty else None)
                buf.append((gs.round, end, snap))
        return dirty

    # -- one journal line ----------------------------------------------
    def _line(self, p, t, text, buf):
        gs = self.state
        col = t.colour
        # An action card's ordered action is logged either on the card's own
        # line (`plays Engineering Genius <P> builds 1 stage of X`) or on the
        # NEXT line (`discovers Riflemen using Breakthrough`).  Either way the
        # line that satisfies the order is consumed by the order, not by the
        # ordinary handler below -- it costs no civil action.
        if gs.pending and self._resolve_pending_from(p, text, buf, t,
                                                     once=True):
            return False
        # `<P> upgrades Bronze to Iron using Rich Land` -- BGO logs a yellow
        # action card that ORDERS an action only as a `using` clause on the
        # ordered action's own line; there is no separate `plays Rich Land`.
        if not gs.pending:
            m = RE_USING.search(text)
            if m:
                card = resolve(fix(m.group(1)), p.hand_civil,
                               age=gs.age_civil)
                c = _DB.by_name.get(card)
                if c is not None and c.get("type") == "action" \
                        and c.get("deck") != "military":
                    return self._play_card(p, t, text, buf, card)
        for pat in DIRTY_MARKERS:
            if pat in text:
                self._patch(text)
                return True
        if any(text.startswith(x) for x in NOISE_PREFIXES):
            return False

        # -- political pass
        if RE_PASSPOL.match(text):
            if gs.phase == "politics":
                return not self._try(p, ("pol_pass",), buf, t)
            return False

        # -- event lines: not a decision, patch the consequences
        if " plays event" in text or "Current event" in text:
            self._patch(text)
            return False

        # -- take
        m = RE_TAKE.match(text)
        if m and m.group(1) == col:
            name = resolve(fix(m.group(2)), gs.card_row, gs.civil_deck,
                           age=gs.age_civil)
            paid = int(m.group(3)) if m.group(3) else 0
            mm = RE_TAKE_MIL.search(text)
            if mm:
                paid += int(mm.group(1))
            if name not in _DB.by_name:
                return True
            wonders = 0 if p.leader == "Michelangelo" else \
                len(p.completed_wonders) + p.destroyed_wonders
            tier = paid - (wonders if _DB.type_of(name) == "wonder" else 0)
            if _DB.type_of(name) == "leader" and p.leader == "Hammurabi":
                tier = paid + 1
            tier = min(3, max(1, tier))
            slot = self._inject_row(name, tier)
            self.stat["takes"] += 1
            return not self._try(p, ("take", slot), buf, t)

        m = RE_PUTBACK.match(text)
        if m:
            name = resolve(fix(m.group(2)), p.hand_civil, age=gs.age_civil)
            if name in p.hand_civil:
                p.hand_civil.remove(name)
                p.civil_actions += int(m.group(3))
                for i in range(actions.ROW_SIZE):
                    if gs.card_row[i] is None:
                        gs.card_row[i] = name
                        break
                self.stat["takebacks"] += 1
            return False

        # -- wonder stages (may be nested in a played action card)
        st = RE_STAGE.search(text)
        played = RE_PLAY.match(text)
        if played and played.group(1) == col:
            return self._play_card(
                p, t, text, buf,
                resolve(fix(played.group(2)), p.hand_civil, p.hand_military,
                        age=gs.age_civil))
        if st and st.group(1) == col:
            w = fix(st.group(3))
            k = int(st.group(2))
            if p.wonder is None or p.wonder.name != w:
                if w in WONDER_STAGES and p.wonder is None:
                    return True          # a stage on a wonder we never took
                return True
            return not self._try(p, ("wonder_step", k), buf, t)

        # -- plain build
        m = RE_BUILD.match(text)
        if m and m.group(1) == col and " stage" not in text[:60].split(
                " builds ", 1)[-1][:20]:
            name = resolve(fix(m.group(2)), p.techs, age=gs.age_civil)
            if name not in _DB.by_name:
                return True
            return not self._try(p, ("build", name), buf, t)

        # -- upgrade
        m = RE_UPGRADE.search(text)
        if m and m.group(1) == col:
            lo = resolve(fix(m.group(2)), p.techs, age=gs.age_civil)
            hi = resolve(fix(m.group(3)), p.techs, p.hand_civil,
                         age=gs.age_civil)
            if lo not in _DB.by_name or hi not in _DB.by_name:
                return True
            return not self._try(p, ("upgrade", lo, hi), buf, t)

        # -- develop
        m = RE_DISCOVER.match(text)
        if m and m.group(1) == col:
            name = resolve(fix(m.group(2)), p.hand_civil, age=gs.age_civil)
            if name not in _DB.by_name:
                return True
            if name not in p.hand_civil:
                p.hand_civil.append(name)     # a card we never saw taken
                self.stat["develop_injected"] += 1
            return not self._try(p, ("develop", name), buf, t)

        # -- revolution
        m = RE_REVOL.match(text)
        if m and m.group(1) == col:
            gov = resolve(m.group(2).strip(), p.hand_civil,
                          age=gs.age_civil)
            if gov not in _DB.by_name:
                return True
            if gov not in p.hand_civil:
                p.hand_civil.append(gov)
            return not self._try(p, ("revolution", gov), buf, t)

        # -- population
        if RE_POP.match(text) and RE_POP.match(text).group(1) == col:
            return not self._try(p, ("pop",), buf, t)

        # -- leader
        m = RE_ELECT.match(text)
        if m and m.group(1) == col:
            name = _leader_prefix(m.group(2))
            if not name:
                return True
            if name not in p.hand_civil:
                p.hand_civil.append(name)
                self.stat["leader_injected"] += 1
            return not self._try(p, ("play_leader", name), buf, t)

        # -- destroy / disband
        m = RE_DESTROY.match(text)
        if m and m.group(1) == col:
            name = resolve(fix(m.group(2)), p.techs, age=gs.age_civil)
            if name not in p.techs:
                return True
            return not self._try(p, ("destroy", name), buf, t)

        # -- tactics
        m = RE_TACTIC_NEW.match(text)
        if m and m.group(1) == col:
            name = resolve(fix(m.group(2)), p.hand_military,
                           gs.military_deck, age=gs.age_military)
            if not self._inject_military(p, name):
                return True
            return not self._try(p, ("play_tactic", name), buf, t)
        m = RE_TACTIC_OLD.match(text)
        if m and m.group(1) == col:
            name = resolve(fix(m.group(2)), gs.available_tactics,
                           age=gs.age_military)
            if name not in self.state.available_tactics:
                self.state.available_tactics.append(name)
            return not self._try(p, ("copy_tactic", name), buf, t)

        # -- everything else: a state patch, not a decision.  A patch we
        # recognised is modelled and does NOT dirty the turn; a line that
        # looks like an action and matched nothing does.
        if self._patch(text):
            return False
        return self._unmodelled(text)

    def _unmodelled(self, text):
        """True (dirty) if this line looks like an action we failed to model."""
        m = re.match(r"^(%s) ([a-z]+)" % _COL, text)
        if not m:
            self.stat["ignored_nonplayer"] += 1
            return False
        verb = m.group(2)
        if verb in ("passes", "discards", "puts", "produces", "gets", "loses",
                    "wins", "defends", "spends", "thought", "tries", "must"):
            self.stat["ignored_verb:" + verb] += 1
            return False
        self.stat["unmodelled:" + verb] += 1
        return True

    def _play_card(self, p, t, text, buf, name):
        """`X plays <card>` -- a civil action card, or something we can't model."""
        gs = self.state
        if name not in _DB.by_name:
            return True
        card = _DB.get(name)
        typ = card.get("type")
        if typ != "action" or card.get("deck") == "military":
            self._patch(text)
            return True
        if name not in p.hand_civil:
            p.hand_civil.append(name)
            self.stat["action_injected"] += 1
        if not self._try(p, ("play_action", name), buf, t):
            return True
        # the ordered action may be on this line (`plays Engineering Genius
        # <P> builds 1 stage of X`) or on the next one; leave it pending if
        # this line does not satisfy it.
        self._resolve_pending_from(p, text, buf, t)
        return False

    def _resolve_pending_from(self, p, text, buf, t, once=False):
        """Resolve pending choices using the clauses of `text`.

        `once` returns True as soon as one choice was satisfied by this line,
        which is how a follow-up line gets consumed by the order it satisfies.
        """
        gs = self.state
        guard = 0
        did = False
        while gs.pending and guard < 8:
            guard += 1
            pend = gs.pending[-1]
            if pend["kind"] != "choice" or pend["player"] != p.idx:
                break
            want = self._match_option(p, text, pend, pend["options"])
            if want is None:
                break
            if not self._apply_choice(want):
                break
            did = True
        return did if once else (not gs.pending)

    def _apply_choice(self, i):
        try:
            actions.apply(self.state, ("choose", i))
            self.stat["pending_matched"] += 1
            return True
        except Exception:                                 # noqa: BLE001
            self.stat["pending_crash"] += 1
            return False

    def _match_option(self, p, text, pend, opts):
        tag = pend.get("tag")
        if tag == "free_civil":
            st = RE_STAGE.search(text)
            for i, o in enumerate(opts):
                o = list(o)
                if o[0] == "wonder_step" and st:
                    return i
                if o[0] == "build" and re.search(
                        r"builds %s\b" % re.escape(o[1]), text):
                    return i
                if o[0] == "upgrade" and re.search(
                        r"upgrades %s to %s\b" % (re.escape(o[1]),
                                                  re.escape(o[2])), text):
                    return i
                if o[0] == "develop" and re.search(
                        r"discovers %s\b" % re.escape(o[1]), text):
                    return i
                if o[0] == "pop" and "increases population" in text:
                    return i
                if o[0] == "revolution" and "Change government to" in text:
                    return i
            return None
        if tag == "food_or_res":
            if "food" in text and "resource" not in text:
                return opts.index("food") if "food" in opts else 0
            if "resource" in text:
                return opts.index("resources") if "resources" in opts else 0
            return 0
        if tag == "discard_military":
            return 0
        return None

    def _pending_flush(self, p, buf, t):
        gs = self.state
        guard = 0
        while gs.pending and guard < 12:
            guard += 1
            mvs = actions.legal_moves(gs)
            if not mvs:
                gs.pending.pop()
                continue
            try:
                actions.apply(gs, mvs[0])
            except Exception:                             # noqa: BLE001
                gs.pending.pop()
            self.stat["pending_forced"] += 1

    # -- applying one move ---------------------------------------------
    def _try(self, p, mv, buf, t, record=True):
        """Record the decision, then apply it.  False if the move was illegal."""
        gs = self.state
        if gs.pending:
            self._pending_flush(p, buf, t)
            self.stat["pending_unmatched"] += 1
        try:
            legal = actions.legal_moves(gs)
        except Exception:                                 # noqa: BLE001
            self.stat["legal_crash"] += 1
            return False
        if mv not in legal:
            if gs.phase == "politics" and mv[0] != "pol_pass":
                # the human acted in the action phase; close politics first
                if ("pol_pass",) in legal:
                    actions.apply(gs, ("pol_pass",))
                    return self._try(p, mv, buf, t, record)
            self.stat["illegal"] += 1
            self.stat["illegal:" + mv[0]] += 1
            if self.debug:
                print("      illegal %r phase=%s ca=%d ma=%d res=%d sci=%d "
                      "food=%d free=%d wonder=%s row=%r"
                      % (mv, gs.phase, p.civil_actions, p.military_actions,
                         p.resources, p.science, p.food, p.workers_free,
                         p.wonder, gs.card_row))
            self._force(p, mv)
            return False
        self.stat["legal"] += 1
        self.stat["legal:" + mv[0]] += 1
        if record and len(legal) > 1:
            # a turn that is already dirty will emit nothing, and the
            # snapshot is by far the most expensive thing here
            snap = (self._snapshot(gs, p.idx, legal, mv)
                    if self.collect and not self.turn_dirty else None)
            buf.append((gs.round, mv, snap))
        try:
            actions.apply(gs, mv)
        except Exception:                                 # noqa: BLE001
            self.stat["apply_crash"] += 1
            return False
        self._buf = buf
        return True

    def _force(self, p, mv):
        """Perform a move the engine refused, by hand, without paying for it.

        The point is to stop ONE bad reconstruction from poisoning the rest of
        the game.  The turn it happens in is already dirty and emits nothing;
        what this buys is that the TABLEAU -- which is what every later
        production check is testing -- goes on matching BGO.  Costs are best
        effort (the stocks are resynced from BGO at the end of the turn
        anyway); worker placement and card identity are not.
        """
        gs = self.state
        k = mv[0]
        try:
            if k == "take":
                name = gs.card_row[mv[1]]
                if name is None:
                    return
                gs.card_row[mv[1]] = None
                if _DB.type_of(name) == "wonder":
                    if p.wonder is None:
                        from engine.state import WonderInProgress
                        p.wonder = WonderInProgress(name=name)
                    else:
                        p.hand_civil.append(name)
                else:
                    p.hand_civil.append(name)
                    if _DB.type_of(name) == "leader":
                        p.taken_leader_ages.append(_DB.get(name)["age"])
                p.civil_actions = max(0, p.civil_actions - 1)
            elif k == "build":
                name = mv[1]
                if p.workers_free <= 0:
                    self._grant_tokens(p, 1)
                    p.yellow_bank -= 1
                    p.workers_free += 1
                p.workers_free -= 1
                t = p.techs.get(name)
                if t is None:
                    t = p.techs[name] = TechCard(name=name)
                t.workers += 1
            elif k == "upgrade":
                lo, hi = mv[1], mv[2]
                if p.techs.get(lo) and p.techs[lo].workers > 0:
                    p.techs[lo].workers -= 1
                else:
                    self._grant_tokens(p, 1)
                    p.yellow_bank -= 1
                t = p.techs.get(hi)
                if t is None:
                    t = p.techs[hi] = TechCard(name=hi)
                t.workers += 1
            elif k == "destroy":
                t = p.techs.get(mv[1])
                if t and t.workers > 0:
                    t.workers -= 1
                    p.workers_free += 1
            elif k == "develop":
                name = mv[1]
                if name in p.hand_civil:
                    p.hand_civil.remove(name)
                if _DB.type_of(name) == "government":
                    p.government = name
                else:
                    p.techs.setdefault(name, TechCard(name=name))
            elif k == "revolution":
                if mv[1] in p.hand_civil:
                    p.hand_civil.remove(mv[1])
                p.government = mv[1]
            elif k == "play_leader":
                if mv[1] in p.hand_civil:
                    p.hand_civil.remove(mv[1])
                p.leader = mv[1]
            elif k == "play_action":
                if mv[1] in p.hand_civil:
                    p.hand_civil.remove(mv[1])
            elif k == "pop":
                if p.yellow_bank > 0:
                    p.yellow_bank -= 1
                    p.workers_free += 1
            elif k == "wonder_step":
                if p.wonder is not None:
                    stages = _DB.get(p.wonder.name)["stages"]
                    p.wonder.steps_built = min(len(stages),
                                               p.wonder.steps_built + mv[1])
                    if p.wonder.steps_built >= len(stages):
                        effects.on_wonder_complete(gs, p, p.wonder.name)
                        p.completed_wonders.append(p.wonder.name)
                        p.wonder = None
            elif k == "play_tactic":
                p.tactic = mv[1]
                p.tactic_exclusive = False
                if mv[1] in p.hand_military:
                    p.hand_military.remove(mv[1])
                if mv[1] not in gs.available_tactics:
                    gs.available_tactics.append(mv[1])
            elif k == "copy_tactic":
                p.tactic = mv[1]
        except Exception:                                 # noqa: BLE001
            self.stat["force_crash"] += 1
        effects.invalidate(gs, p)
        self.stat["forced:" + k] += 1

    # -- feature snapshot ----------------------------------------------
    def _snapshot(self, gs, idx, legal, chosen):
        """Feature vector of every candidate's post-move state."""
        from engine.bots.fastcopy import copy_state
        from engine.bots.trial import fresh_trial_rng
        try:
            ctx = W.rival_context(gs, idx)
        except Exception:                                 # noqa: BLE001
            return None
        rows = []
        ci = -1
        for mv in legal:
            if mv[0] == "resign":
                continue
            trial = copy_state(gs)
            try:
                actions.apply(trial, mv, fresh_trial_rng())
                f = W.features(trial, idx, ctx)
                lv = W.lateness(trial)
            except Exception:                             # noqa: BLE001
                continue
            if mv == chosen:
                ci = len(rows)
            f["__L"] = lv
            f["__end"] = 1.0 if mv[0] == "end_turn" else 0.0
            # The four terms `evaluate` prices through `w` itself rather than
            # through a weight of their own.  They are not linear in the
            # weights, so they are emitted here priced through a FIXED
            # reference vector (`DEFAULT_WEIGHTS`) and fitted as ordinary
            # scales.  That linearisation is approximate -- the shipped file
            # will price them through the FITTED w -- but without it the
            # evaluator is card-identity-blind and every `take` in a tier is
            # byte-identical (docs/WASTED_ACTIONS.md section 4).
            try:
                f["__hp"] = W.hand_potential(trial, idx, _REF)
                f["__rhp"] = W.rival_hand_potential(trial, idx, _REF)
                u, bgn = W.row_pressure(trial, idx, _REF, ctx)
                f["__ru"], f["__rb"] = u, bgn
            except Exception:                             # noqa: BLE001
                f["__hp"] = f["__rhp"] = f["__ru"] = f["__rb"] = 0.0
            rows.append((mv, f))
        if ci < 0 or len(rows) < 2:
            return None
        return (rows, ci)

    # -- state patches (no decision) -----------------------------------
    #: standalone stock movements between turns (an event's per-player
    #: consequence, a wonder's one-off, a leader's gift).  Every one of these
    #: is logged on its own line; the same words also appear as the `spends`
    #: clause of an action line, which is why this only runs on the fallthrough
    #: path, after every action handler has declined the line.
    _STOCK_RE = re.compile(
        r"(%s) (produces|gains|gets|loses|scores) (\d+) "
        r"(resources?|food|science|culture)" % _COL)
    _STOCK_KEY = {"resource": "resources", "resources": "resources",
                  "food": "food", "science": "science", "culture": "culture"}

    def _patch(self, text):
        gs = self.state
        touched = False
        for colour, verb, n, what in self._STOCK_RE.findall(text):
            key = self._STOCK_KEY.get(what)
            if key is None:
                continue
            p = gs.players[self.seat[colour]]
            k = int(n) * (-1 if verb == "loses" else 1)
            if key == "resources":
                effects.gain_resources(p, k) if k > 0 else \
                    setattr(p, "resources", max(0, p.resources + k))
            elif key == "food":
                effects.gain_food(p, k) if k > 0 else \
                    setattr(p, "food", max(0, p.food + k))
            else:
                setattr(p, key, max(0, getattr(p, key) + k))
            touched = True
        for colour, n in RE_GETPOP.findall(text):
            p = gs.players[self.seat[colour]]
            k = min(int(n), p.yellow_bank)
            p.yellow_bank -= k
            p.workers_free += k
            touched = True
        for colour, n in RE_LOSEPOP.findall(text):
            p = gs.players[self.seat[colour]]
            for _ in range(int(n)):
                if p.workers_free > 0:
                    p.workers_free -= 1
                    p.yellow_bank += 1
            touched = True
        for colour, n in RE_YELLOW_GET.findall(text):
            self._grant_tokens(gs.players[self.seat[colour]], int(n))
            touched = True
        for colour, n in RE_YELLOW_LOSE.findall(text):
            p = gs.players[self.seat[colour]]
            k = min(int(n), p.yellow_bank)
            p.yellow_bank -= k
            self.tokens[p.idx] -= k
            touched = True
        m = re.search(r"Each civilization gains (\d+) population", text)
        if m:
            for p in gs.players:
                k = min(int(m.group(1)), p.yellow_bank)
                p.yellow_bank -= k
                p.workers_free += k
            touched = True
        m = re.search(r"Each civilization loses (\d+) population", text)
        if m:
            for p in gs.players:
                for _ in range(int(m.group(1))):
                    if p.workers_free > 0:
                        p.workers_free -= 1
                        p.yellow_bank += 1
            touched = True
        for age, tname in RE_TERR_AGE.findall(text):
            self.last_terr_age[tname] = age
        m = RE_COLONIZE.match(text)
        if m:
            p = gs.players[self.seat[m.group(1)]]
            terr = m.group(2)
            key = "%s (%s)" % (terr, self.last_terr_age.get(terr, "I"))
            if key in _DB.by_name:
                p.colonies.append(key)
                perm = _DB.get(key).get("permanentEffects") or {}
                self._grant_tokens(p, int(perm.get("yellowTokens", 0) or 0))
                p.blue_total += int(perm.get("blueTokens", 0) or 0)
            # §11.3: the winner sacrifices the listed units, and their yellow
            # tokens go back to the BANK, not to the unused pool.  Missing
            # this was worth 1-4 bank tokens per colonising player, which
            # moves the consumption band and therefore the food check.
            for piece in text.split(";")[1:]:
                mm = re.match(r"\s*(\d+) ([A-Za-z'. ]+?)\s*$", piece)
                if not mm:
                    continue
                unit = fix(mm.group(2))
                if unit in ("Colonization card", "Total force"):
                    continue
                unit = resolve(unit, p.techs, age=gs.age_civil)
                for _ in range(int(mm.group(1))):
                    t = p.techs.get(unit)
                    if t is not None and t.workers > 0:
                        t.workers -= 1
                        p.yellow_bank += 1
            effects.invalidate(gs, p)
            touched = True
        if text.startswith("Insufficient task force") or " bids " in text:
            touched = True                # an auction move, no state change
        if "crumble" in text:
            for piece in text.split(";"):
                mm = RE_CRUMBLE.match(piece.strip())
                if mm and fix(mm.group(1)) in WONDER_STAGES:
                    w = fix(mm.group(1))
                    for p in gs.players:
                        if w in p.completed_wonders and \
                                w not in p.flipped_wonders:
                            p.flipped_wonders.append(w)
                            break
            touched = True
        return touched

    # -- the fidelity gate ---------------------------------------------
    def _settle(self, p, t, dirty, buf):
        """Compare with BGO, resync the stocks, decide if the turn was clean."""
        gs = self.state
        if t.prod is None:
            return False
        effects.invalidate(gs, p)
        s = effects.state_stats(gs, p)
        cons = economy.consumption(p.yellow_bank)
        got_prod = (s.culture, s.science, s.food - cons, cons, s.resources)
        want_prod = t.prod
        prod_ok = got_prod == want_prod
        self.stat["prod_rows"] += 1
        self.stat["prod_ok"] += int(prod_ok)
        for k, lbl in enumerate(("culture", "science", "food", "cons", "res")):
            self.stat["prod_" + lbl] += int(got_prod[k] == want_prod[k])
        # stocks: ours BEFORE production, plus this turn's production
        gc = p.culture + want_prod[0]
        gsci = p.science + want_prod[1]
        gf = max(0, p.food + want_prod[2] - t.famine)
        gr = max(0, p.resources + want_prod[4] - t.corruption)
        want = t.stock
        stock_ok = (gc == want[0] and gsci == want[1] and gf == want[2]
                    and (want[3] is None or gr == want[3]))
        self.stat["stock_ok"] += int(stock_ok)
        self.stat["stock_culture"] += int(gc == want[0])
        self.stat["stock_science"] += int(gsci == want[1])
        self.stat["stock_food"] += int(gf == want[2])
        self.stat["stock_res"] += int(want[3] is None or gr == want[3])
        self.stat["gate_prod_only"] += int(prod_ok)
        rb = min((t.round - 1) // 4, 5)
        self.by_round[rb]["pn"] += 1
        self.by_round[rb]["pok"] += int(prod_ok)
        self.by_round[rb]["sok"] += int(stock_ok)
        self.stat["gate_nodirty"] += int(not dirty)
        tok_ok = self._tokens_ok(p)
        self.stat["tokens_ok"] += int(tok_ok)
        # resync
        p.culture, p.science = want[0], want[1]
        p.food = max(0, want[2])
        if want[3] is not None:
            p.resources = max(0, want[3])
        else:
            p.resources = max(0, gr)
        # military hand size
        for _ in range(t.draws):
            if gs.military_deck:
                p.hand_military.append(gs.military_deck.pop())
        mine = prod_ok and stock_ok and tok_ok and not dirty
        self.seat_clean[p.idx] = mine
        # `features()` reads every rival's culture, rates and strength, so a
        # decision is only as good as the WHOLE table's reconstruction.  Both
        # levels are emitted, tagged, so the fitter can choose.
        table = mine and all(self.seat_clean.get(q.idx, False)
                             for q in gs.players)
        self.stat["decisions_seen"] += len(buf)
        if mine:
            self.stat["decisions_clean_seat"] += len(buf)
            if table:
                self.stat["decisions_clean"] += len(buf)
            b = min((t.round - 1) // 4, 5)
            self.by_round[b]["dec"] += len(buf) if table else 0
            if self.collect:
                for rnd, mv, snap in buf:
                    if snap is None:
                        continue
                    ex = _serialize(*snap)
                    ex["r"] = rnd
                    ex["s"] = p.idx
                    ex["q"] = 2 if table else 1
                    self.examples.append(ex)
        return table


# ---------------------------------------------------------------- driver

def _expand(f):
    """One candidate's post-move state as the LINEAR parameter vector of
    `weighted.evaluate`.

    `evaluate` is `sum(w[k]*f[k]) + sum over PHASE_KEYS of
    (w[k_early]*(1-L) + w[k_late]*L)*f[k] + end_turn_bias*[move is end_turn]`,
    plus three terms (`hand_potential`, `row_urgency`, `row_bargain_forgone`,
    `rival_hand_potential`) that are priced through `w` itself and are
    therefore NOT linear in the weights.  Those four are excluded here and
    left at whatever the initial vector says; everything else is exactly
    linear, which makes the fit a convex conditional logit rather than a
    search.
    """
    lv = f["__L"]
    out = {}
    for k, v in f.items():
        if k.startswith("__") or not v:
            continue
        out[k] = v
    for k in W.PHASE_KEYS:
        v = f.get(k, 0.0)
        if not v:
            continue
        out[k + "_early"] = v * (1.0 - lv)
        out[k + "_late"] = v * lv
    if f["__end"]:
        out["end_turn_bias"] = 1.0
    for key, name in (("__hp", "hand_potential"),
                      ("__rhp", "rival_hand_potential"),
                      ("__ru", "row_urgency"),
                      ("__rb", "row_bargain_forgone")):
        v = f.get(key, 0.0)
        if v:
            out[name] = v
    return out


def _serialize(rows, ci):
    """Sparse per-candidate deltas against candidate 0.

    Only differences between candidates can move a softmax over candidate
    scores, so the reference candidate's own vector is dropped entirely --
    which is also what keeps the emitted file to a sane size.
    """
    vecs = [_expand(f) for _mv, f in rows]
    base = vecs[0]
    uniq = []
    index = {}
    cand = []
    for v in vecs:
        d = {}
        for k in set(v) | set(base):
            delta = round(v.get(k, 0.0) - base.get(k, 0.0), 4)
            if delta:
                d[k] = delta
        flat = []
        for k in sorted(d):
            i = PARAM_IX.get(k)
            if i is not None:
                flat.append(i)
                flat.append(d[k])
        key = tuple(flat)
        j = index.get(key)
        if j is None:
            j = index[key] = len(uniq)
            uniq.append(flat)
        cand.append(j)
    return {"y": ci, "n": len(rows), "u": uniq, "c": cand,
            "m": [str(m[0]) for m, _f in rows]}


def run_one(path, collect=False, seed=0):
    r = Replay(path, seed=seed, collect=collect)
    r.run()
    return r


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--journals", default="/tmp/bgo/journals")
    ap.add_argument("--game", action="append", default=[])
    ap.add_argument("--limit", type=int, default=0)
    ap.add_argument("--players", type=int, default=0)
    ap.add_argument("--emit", default=None)
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--shard", default=None, help="i/n: this slice of games")
    ap.add_argument("-v", "--verbose", action="store_true")
    a = ap.parse_args(argv)

    files = ([os.path.join(a.journals, g + ".tsv") for g in a.game] if a.game
             else sorted(os.path.join(a.journals, f)
                         for f in os.listdir(a.journals) if f.endswith(".tsv")))
    if a.shard:
        i, k = (int(x) for x in a.shard.split("/"))
        files = [f for j, f in enumerate(files) if j % k == i]
    tot = Counter()
    by_round = defaultdict(Counter)
    out = open(a.emit, "w") if a.emit else None
    ngames = 0
    for path in files:
        if a.limit and ngames >= a.limit:
            break
        try:
            r = Replay(path, seed=a.seed, collect=bool(a.emit))
            if a.players and r.n != a.players:
                continue
            r.run()
        except Exception as exc:                          # noqa: BLE001
            tot["crash"] += 1
            if a.verbose:
                print("CRASH", os.path.basename(path), type(exc).__name__, exc)
            continue
        ngames += 1
        tot.update(r.stat)
        for b, c in r.by_round.items():
            by_round[b].update(c)
        if out:
            for ex in r.examples:
                ex["g"] = r.gid
                out.write(json.dumps(ex) + "\n")
    if out:
        out.close()

    print("=== games %d (crashes %d) ===" % (ngames, tot["crash"]))
    print("turns %d  clean %d (%.1f%%)"
          % (tot["turns"], tot["turns_clean"],
             100.0 * tot["turns_clean"] / max(1, tot["turns"])))
    print("production exact %d/%d (%.1f%%)   stocks exact %d (%.1f%%)   "
          "tokens conserved %d (%.1f%%)"
          % (tot["prod_ok"], tot["prod_rows"],
             100.0 * tot["prod_ok"] / max(1, tot["prod_rows"]),
             tot["stock_ok"], 100.0 * tot["stock_ok"] / max(1, tot["prod_rows"]),
             tot["tokens_ok"],
             100.0 * tot["tokens_ok"] / max(1, tot["prod_rows"])))
    n = max(1, tot["prod_rows"])
    for lbl in ("culture", "science", "food", "cons", "res"):
        print("   prod  %-9s %6d (%.1f%%)" % (lbl, tot["prod_" + lbl],
              100.0 * tot["prod_" + lbl] / n))
    for lbl in ("culture", "science", "food", "res"):
        print("   stock %-9s %6d (%.1f%%)" % (lbl, tot["stock_" + lbl],
              100.0 * tot["stock_" + lbl] / n))
    print("gate components: production %.1f%%  no-dirty-line %.1f%%"
          % (100.0 * tot["gate_prod_only"] / n,
             100.0 * tot["gate_nodirty"] / n))
    print("decisions seen %d   own seat clean %d (%.1f%%)   "
          "whole table clean %d (%.1f%%)"
          % (tot["decisions_seen"], tot["decisions_clean_seat"],
             100.0 * tot["decisions_clean_seat"] / max(1, tot["decisions_seen"]),
             tot["decisions_clean"],
             100.0 * tot["decisions_clean"] / max(1, tot["decisions_seen"])))
    print("moves legal %d  illegal %d (%.1f%%)"
          % (tot["legal"], tot["illegal"],
             100.0 * tot["illegal"] / max(1, tot["legal"] + tot["illegal"])))
    for k in sorted(tot):
        if k.startswith("illegal:") or k.startswith("crash:"):
            print("   %-22s %d" % (k, tot[k]))
    print("takes %d  already in row %d  injected %d  tier match %d"
          % (tot["takes"], tot["take_in_row"], tot["take_injected"],
             tot["take_tier_ok"]))
    print("clean turns by round bucket:")
    for b in sorted(by_round):
        c = by_round[b]
        print("   rounds %2d-%2d  clean %6d/%-6d %5.1f%%   "
              "production exact %5.1f%%   stocks exact %5.1f%%"
              % (b * 4 + 1, b * 4 + 4, c["ok"], c["n"],
                 100.0 * c["ok"] / max(1, c["n"]),
                 100.0 * c["pok"] / max(1, c["pn"]),
                 100.0 * c["sok"] / max(1, c["pn"])))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
