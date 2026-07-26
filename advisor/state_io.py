"""Terse, human-typed text format for a Through the Ages board.

Design goal: the human is sitting at a physical table with a phone or laptop
next to the board.  Typing must be FAST, so

* card names are fuzzy-matched -- ``pyr`` / ``loa`` / ``hang gard`` all work
  (prefix, initials or subsequence, resolved against the real card database);
* nothing has to be repeated between turns -- the advisor keeps a mirror of
  the board and the human only reports what CHANGED (``deal`` for the new
  cards on the right of the row, ``p1 c=34`` for a rival's culture, ...);
* every value may be ``?`` meaning "I don't know", which leaves the mirror
  untouched and records the field as unknown instead of crashing.

Two representations:

``dumps(board)`` / ``loads(text)``
    the full snapshot, one section per line, round-trips exactly.

``patch(board, line)``
    a single-line update, the thing the human actually types between turns.

A :class:`Board` is the engine ``GameState`` plus the advisor's own
book-keeping: which seat the human occupies, how many cards a rival holds
that we cannot see, and which fields the human declared unknown.
"""
from __future__ import annotations

import re
from dataclasses import dataclass, field

from engine import cards as C
from engine import effects
from engine import game as G
from engine.actions import ROW_SIZE
from engine.state import GameState, TechCard, WonderInProgress

__all__ = [
    "Board", "new_board", "dumps", "loads", "render", "patch",
    "resolve_card", "CardError", "AmbiguousCard", "UnknownCard", "PatchError",
    "advance_row", "sync_row",
]

VERSION = 1
EMPTY = "."
UNKNOWN = "?"


# --------------------------------------------------------------- errors

class PatchError(ValueError):
    """A line the human typed that we could not use.  Always recoverable:
    the caller prints ``str(exc)`` and asks again."""


class CardError(PatchError):
    pass


class UnknownCard(CardError):
    def __init__(self, text, pool_name="card"):
        super().__init__(f"no {pool_name} matches {text!r}")
        self.text = text


class AmbiguousCard(CardError):
    def __init__(self, text, options):
        opts = ", ".join(sorted(options)[:8])
        more = "" if len(options) <= 8 else f" (+{len(options) - 8} more)"
        super().__init__(f"{text!r} is ambiguous: {opts}{more}")
        self.text = text
        self.options = sorted(options)


# ------------------------------------------------------- card resolution

def _norm(s):
    return re.sub(r"[^a-z0-9]", "", s.lower())


def _initials(name):
    words = [w for w in re.split(r"[^A-Za-z0-9]+", name) if w]
    return "".join(w[0].lower() for w in words)


def _is_subseq(needle, hay):
    it = iter(hay)
    return all(ch in it for ch in needle)


_POOLS = {}


def pool(kind="any"):
    """Candidate card names for a slot, e.g. ``pool("tech")``."""
    if kind in _POOLS:
        return _POOLS[kind]
    db = C.db()
    names = list(db.by_name)
    if kind == "tech":
        names = [n for n in names
                 if db.type_of(n) in (C.WORKER_TYPES | {"special-tech"})]
    elif kind == "gov":
        names = [n for n in names if db.type_of(n) == "government"]
    elif kind == "wonder":
        names = [n for n in names if db.type_of(n) == "wonder"]
    elif kind == "leader":
        names = [n for n in names if db.type_of(n) == "leader"]
    elif kind == "tactic":
        names = [n for n in names if db.type_of(n) == "tactic"]
    elif kind == "row":
        names = [n for n in names if db.by_name[n]["deck"] == "civil"]
    elif kind == "military":
        names = [n for n in names if db.by_name[n]["deck"] == "military"]
    elif kind == "event":
        names = [n for n in names if db.type_of(n) in ("event", "territory")]
    _POOLS[kind] = names
    return names


def resolve_card(text, kind="any", extra=()):
    """Fuzzy-match ``text`` to one card name.

    Tiers, first non-empty wins: exact, case/punctuation-insensitive exact,
    prefix, initials, subsequence.  Raises :class:`AmbiguousCard` or
    :class:`UnknownCard`, both of which are :class:`PatchError`s so the
    interactive loop can just re-prompt.
    """
    text = text.strip()
    if not text:
        raise UnknownCard(text, kind)
    names = list(extra) + list(pool(kind))
    if text in names:
        return text
    q = _norm(text)
    tiers = [
        [n for n in names if _norm(n) == q],
        [n for n in names if _norm(n).startswith(q)],
        [n for n in names if _initials(n) == q],
        [n for n in names if _initials(n).startswith(q)],
        [n for n in names if _is_subseq(q, _norm(n))],
    ]
    for tier in tiers:
        uniq = sorted(set(tier))
        if len(uniq) == 1:
            return uniq[0]
        if len(uniq) > 1:
            raise AmbiguousCard(text, uniq)
    raise UnknownCard(text, kind)


# ---------------------------------------------------------------- board

@dataclass
class Board:
    """Engine state + the advisor's book-keeping about hidden information."""
    state: GameState
    me: int = 0
    # cards a player holds whose identity we do not know: {(idx, "civil"): n}
    hidden: dict = field(default_factory=dict)
    # fields the human explicitly declared unknown, e.g. {"p1.culture"}
    unknown: set = field(default_factory=set)

    # ---- convenience
    @property
    def players(self):
        return self.state.players

    def player(self, idx):
        if not 0 <= idx < len(self.state.players):
            raise PatchError(f"no player p{idx} in a "
                             f"{self.state.num_players}-player game")
        return self.state.players[idx]

    def hidden_count(self, idx, which):
        return self.hidden.get((idx, which), 0)

    def hand_size(self, idx, which="civil"):
        p = self.player(idx)
        known = p.hand_civil if which == "civil" else p.hand_military
        return len(known) + self.hidden_count(idx, which)

    def copy(self):
        return Board(self.state.copy(), self.me, dict(self.hidden),
                     set(self.unknown))


def new_board(num_players, me=0, seed=0):
    """A fresh game mirroring a freshly set-up physical board."""
    st = G.new_game(num_players, seed)
    return Board(st, me=me)


# ------------------------------------------------------------- helpers

def _stats(board, p):
    effects.invalidate(board.state, p)
    return effects.compute(board.state, p)


def _int(tok, key):
    try:
        return int(tok)
    except (TypeError, ValueError):
        raise PatchError(f"{key}: {tok!r} is not a number")


def _split_cards(text, kind=None):
    """Split a card list.

    Commas are the real separator.  Without a comma we allow whitespace --
    ``deal bro irr alc`` -- but only after checking that the whole string is
    not itself one card name, so ``deal Hanging Gardens`` still works.
    """
    text = text.strip()
    if not text:
        return []
    if "," in text:
        return [p.strip() for p in text.split(",") if p.strip()]
    if kind and " " in text:
        try:
            resolve_card(text, kind)
            return [text]
        except PatchError:
            pass
    return text.split()


def _split_strict(text):
    """Comma-separated card list, as written by :func:`dumps`."""
    return [p.strip() for p in text.split(",") if p.strip()]


# scalar keys the human may set on a player line: key -> state field
SCALARS = {
    "ca": "civil_actions", "ma": "military_actions",
    "f": "food", "food": "food",
    "r": "resources", "res": "resources",
    "s": "science", "sci": "science",
    "c": "culture", "cult": "culture",
    "blue": "blue_total", "yel": "yellow_bank", "y": "yellow_bank",
    "fw": "workers_free",
    "strx": "strength_extra", "hapx": "happy_extra",
    "crx": "culture_rate_extra", "srx": "science_rate_extra",
}
# keys given as a DERIVED total; we back out the "extra" so the mirror agrees
FORCED = {"str": ("strength", "strength_extra"),
          "hap": ("happy", "happy_extra"),
          "cr": ("culture", "culture_rate_extra"),
          "sr": ("science", "science_rate_extra")}
FLAGS = {"pol": "politics_done", "resigned": "resigned"}


def _force(board, p, key, value):
    stat_attr, extra_attr = FORCED[key]
    setattr(p, extra_attr, 0)
    base = getattr(_stats(board, p), stat_attr)
    setattr(p, extra_attr, value - base)
    effects.invalidate(board.state, p)


def _set_player_key(board, p, key, tok):
    """One ``key=value`` token on a player line."""
    if tok == UNKNOWN:
        board.unknown.add(f"p{p.idx}.{key}")
        return f"p{p.idx}.{key} unknown"
    board.unknown.discard(f"p{p.idx}.{key}")
    if key == "gov":
        p.government = resolve_card(tok, "gov")
        effects.invalidate(board.state, p)
        return f"p{p.idx} government = {p.government}"
    if key in ("hc", "hm"):
        which = "civil" if key == "hc" else "military"
        known = len(p.hand_civil if which == "civil" else p.hand_military)
        board.hidden[(p.idx, which)] = max(0, _int(tok, key) - known)
        return f"p{p.idx} {which} hand = {board.hand_size(p.idx, which)}"
    if key in FORCED:
        _force(board, p, key, _int(tok, key))
        return f"p{p.idx} {key} = {tok}"
    if key in SCALARS:
        tok = tok.split("/")[0]          # accept "ca=3/4"
        setattr(p, SCALARS[key], _int(tok, key))
        effects.invalidate(board.state, p)
        return f"p{p.idx} {SCALARS[key]} = {tok}"
    raise PatchError(f"unknown player key {key!r} "
                     f"(try: {', '.join(sorted(set(SCALARS) | set(FORCED)))})")


# ----------------------------------------------------------- the card row

def sync_row(board, names):
    """Force the row to ``names`` (list of card names / None), keeping the
    internal deck's *composition* honest: a card we dealt but that is not
    really on the table goes back into the deck, and one that really is on
    the table is taken out of it."""
    st = board.state
    names = list(names)[:ROW_SIZE]
    names += [None] * (ROW_SIZE - len(names))
    old = [n for n in st.card_row if n]
    new = [n for n in names if n]
    for n in new:
        if n in st.civil_deck and n not in old:
            st.civil_deck.remove(n)
    for n in old:
        if n not in new and n not in st.civil_deck:
            st.civil_deck.append(n)
    st.card_row = names


def advance_row(board, new_cards, sweep=None):
    """The start-of-turn row shuffle, driven by what the human reports.

    Discards the leftmost ``sweep`` cards (3/2/1 by player count), slides the
    rest left and puts ``new_cards`` into the empty slots left-to-right.
    """
    st = board.state
    if sweep is None:
        sweep = G.SWEEP[G.live_count(st)]
    row = list(st.card_row)
    for i in range(min(sweep, len(row))):
        row[i] = None
    kept = [c for c in row if c is not None]
    row = kept + [None] * (ROW_SIZE - len(kept))
    for name in new_cards:
        for i in range(ROW_SIZE):
            if row[i] is None:
                row[i] = name
                break
        else:
            raise PatchError("more new cards than empty row slots")
    sync_row(board, row)
    return row


# ------------------------------------------------------------ dump / load

def _fmt_hand(names):
    return ", ".join(names) if names else EMPTY


def dumps(board):
    """The full snapshot.  ``loads(dumps(b))`` reproduces ``b``."""
    st = board.state
    out = [f"tta {VERSION}"]
    out.append(
        f"game {st.num_players}p seed={st.seed} turn={st.turn} "
        f"round={st.round} age={st.age_civil}/{st.age_military} "
        f"cur={st.current} start={st.start_player} phase={st.phase} "
        f"me={board.me}")
    out.append(f"deck civil={len(st.civil_deck)} mil={len(st.military_deck)}")
    row = [n if n else EMPTY for n in st.card_row]
    out.append("row " + ", ".join(row))
    out.append(f"events age={st.current_events_age} "
               f"fut={len(st.future_events)} past={len(st.past_events)}")
    if st.current_events:
        out.append("curev " + ", ".join(st.current_events))
    if st.available_tactics:
        out.append("tactics " + ", ".join(st.available_tactics))
    if st.last_round:
        out.append("last_round")
    for p in st.players:
        tag = " me" if p.idx == board.me else ""
        flags = "".join(f" {k}" for k, attr in FLAGS.items()
                        if getattr(p, attr))
        out.append(
            f"p{p.idx}{tag} ca={p.civil_actions} "
            f"ma={p.military_actions} f={p.food} r={p.resources} "
            f"s={p.science} c={p.culture} blue={p.blue_total} "
            f"yel={p.yellow_bank} fw={p.workers_free} "
            f"strx={p.strength_extra} hapx={p.happy_extra} "
            f"crx={p.culture_rate_extra} srx={p.science_rate_extra}{flags}")
        out.append(f"p{p.idx} gov {p.government}")
        if p.techs:
            techs = ", ".join(f"{n}:{t.workers}"
                              for n, t in sorted(p.techs.items()))
            out.append(f"p{p.idx} tech {techs}")
        if p.hand_civil or p.hand_military:
            out.append(f"p{p.idx} hand {_fmt_hand(sorted(p.hand_civil))}"
                       f" | {_fmt_hand(sorted(p.hand_military))}")
        hc = board.hidden_count(p.idx, "civil")
        hm = board.hidden_count(p.idx, "military")
        if hc or hm:
            out.append(f"p{p.idx} hidden hc={hc} hm={hm}")
        if p.wonder:
            out.append(f"p{p.idx} wonder {p.wonder.name} "
                       f"{p.wonder.steps_built}")
        if p.completed_wonders:
            out.append(f"p{p.idx} built " + ", ".join(p.completed_wonders))
        if p.leader:
            out.append(f"p{p.idx} leader {p.leader}")
        if p.tactic:
            star = "*" if p.tactic_exclusive else ""
            out.append(f"p{p.idx} tactic {p.tactic}{star}")
    for key in sorted(board.unknown):
        out.append(f"unknown {key}")
    return "\n".join(out) + "\n"


def loads(text):
    """Parse a snapshot produced by :func:`dumps` (or hand-written)."""
    lines = [ln.strip() for ln in text.splitlines()]
    lines = [ln for ln in lines if ln and not ln.startswith("#")]
    if not lines or not lines[0].startswith("tta"):
        raise PatchError("snapshot must start with a 'tta <version>' line")
    head = None
    for ln in lines:
        if ln.startswith("game "):
            head = _kv(ln.split(None, 1)[1])
            break
    if head is None:
        raise PatchError("snapshot has no 'game' line")
    n = _int(head.get("np") or head.get("players")
             or _players_token(lines), "players")
    board = Board(GameState(num_players=n, seed=_int(head.get("seed", 0),
                                                     "seed")), me=0)
    st = board.state
    st.has_military = C.db().has_military
    from engine.state import PlayerState
    st.players = [PlayerState(idx=i) for i in range(n)]
    st.card_row = [None] * ROW_SIZE
    # the deck line is applied LAST: it only carries counts, and loading the
    # row adjusts deck composition, which would otherwise change the count
    deck_line = None
    for ln in lines[1:]:
        if ln.split(None, 1)[0] == "deck":
            deck_line = ln
            continue
        _load_line(board, ln)
    if deck_line:
        _load_line(board, deck_line)
    for p in st.players:
        effects.invalidate(st, p)
    return board


def _players_token(lines):
    for ln in lines:
        if ln.startswith("game "):
            m = re.search(r"\b(\d)p\b", ln)
            if m:
                return m.group(1)
    raise PatchError("game line must say how many players, e.g. '3p'")


def _kv(text):
    out = {}
    for tok in text.split():
        if "=" in tok:
            k, v = tok.split("=", 1)
            out[k] = v
    return out


def _load_line(board, ln):
    st = board.state
    word = ln.split(None, 1)[0]
    rest = ln.split(None, 1)[1] if " " in ln else ""
    if word == "game":
        kv = _kv(rest)
        for k, attr in (("turn", "turn"), ("round", "round"),
                        ("cur", "current"), ("start", "start_player"),
                        ("seed", "seed")):
            if k in kv:
                setattr(st, attr, _int(kv[k], k))
        if "age" in kv:
            civ, _, mil = kv["age"].partition("/")
            st.age_civil = civ
            st.age_military = mil or civ
        if "phase" in kv:
            st.phase = kv["phase"]
        if "me" in kv:
            board.me = _int(kv["me"], "me")
        return
    if word == "deck":
        kv = _kv(rest)
        st.civil_deck = _fake_deck(st.age_civil, st.num_players,
                                   _int(kv.get("civil", 0), "civil"))
        st.military_deck = _fake_deck(st.age_military, st.num_players,
                                      _int(kv.get("mil", 0), "mil"),
                                      military=True)
        return
    if word == "row":
        names = [None if t == EMPTY else resolve_card(t, "row")
                 for t in _split_strict(rest)]
        sync_row(board, names)
        return
    if word == "events":
        kv = _kv(rest)
        st.current_events_age = kv.get("age", st.current_events_age)
        st.future_events = ["?"] * _int(kv.get("fut", 0), "fut")
        st.past_events = ["?"] * _int(kv.get("past", 0), "past")
        return
    if word == "curev":
        st.current_events = [resolve_card(t, "military")
                             for t in _split_strict(rest)]
        return
    if word == "tactics":
        st.available_tactics = [resolve_card(t, "tactic")
                                for t in _split_strict(rest)]
        return
    if word == "last_round":
        st.last_round = True
        return
    if word == "unknown":
        board.unknown.add(rest.strip())
        return
    if re.fullmatch(r"p\d+", word):
        _load_player_line(board, int(word[1:]), rest)
        return
    raise PatchError(f"don't understand line {ln!r}")


def _fake_deck(age, num_players, size, military=False):
    """A deck of the right SIZE (identities are hidden from the human).

    Only the count matters -- it is what decides when the age ends -- so we
    take the real age deck and trim it.
    """
    db = C.db()
    n = max(2, min(4, num_players))
    names = (db.military_deck(age, n) if military else db.civil_deck(age, n))
    if len(names) >= size:
        return names[:size]
    return names + [names[0] if names else "Bronze"] * (size - len(names))


def _load_player_line(board, idx, rest):
    p = board.player(idx)
    st = board.state
    words = rest.split(None, 1)
    head = words[0] if words else ""
    tail = words[1] if len(words) > 1 else ""
    if head == "gov":
        p.government = resolve_card(tail, "gov")
        effects.invalidate(st, p)
        return
    if head == "tech":
        p.techs = {}
        for tok in _split_strict(tail):
            name, _, w = tok.rpartition(":")
            if not name:
                name, w = tok, "0"
            p.techs[resolve_card(name, "tech")] = TechCard(
                resolve_card(name, "tech"), workers=_int(w, "workers"))
        effects.invalidate(st, p)
        return
    if head == "hand":
        civil, _, mil = tail.partition("|")
        p.hand_civil = [resolve_card(t, "row")
                        for t in _split_strict(civil) if t != EMPTY]
        p.hand_military = [resolve_card(t, "military")
                           for t in _split_strict(mil) if t != EMPTY]
        return
    if head == "hidden":
        kv = _kv(tail)
        board.hidden[(idx, "civil")] = _int(kv.get("hc", 0), "hc")
        board.hidden[(idx, "military")] = _int(kv.get("hm", 0), "hm")
        return
    if head == "wonder":
        bits = tail.rsplit(None, 1)
        if len(bits) == 2 and bits[1].isdigit():
            p.wonder = WonderInProgress(resolve_card(bits[0], "wonder"),
                                        int(bits[1]))
        else:
            p.wonder = WonderInProgress(resolve_card(tail, "wonder"), 0)
        return
    if head == "built":
        p.completed_wonders = [resolve_card(t, "wonder")
                               for t in _split_strict(tail)]
        return
    if head == "leader":
        p.leader = None if tail.strip() in ("", EMPTY, "-") \
            else resolve_card(tail, "leader")
        effects.invalidate(st, p)
        return
    if head == "tactic":
        t = tail.strip()
        p.tactic_exclusive = t.endswith("*")
        t = t.rstrip("*")
        p.tactic = None if t in ("", EMPTY, "-") else resolve_card(t, "tactic")
        effects.invalidate(st, p)
        return
    # otherwise: a line of key=value tokens (+ bare flags)
    _apply_player_tokens(board, p, rest)


def _apply_player_tokens(board, p, rest):
    msgs = []
    for tok in rest.split():
        if tok == "me":
            board.me = p.idx
            continue
        if tok in FLAGS:
            setattr(p, FLAGS[tok], True)
            continue
        if "=" not in tok:
            raise PatchError(f"expected key=value, got {tok!r}")
        k, v = tok.split("=", 1)
        msgs.append(_set_player_key(board, p, k, v))
    return msgs


# ---------------------------------------------------------------- patches

PATCH_HELP = """\
between-turn updates (one per line, blank line when done):

  deal <cards>          new cards dealt on the right after the sweep
  row <13 cards>        retype the whole row ('.' = empty slot)
  take p1 7             a rival took the card in row slot 7 (0-based)
  p1 c=34 s=9 str=6     a rival's scalars (any of: %s)
  p1 tech+ Bronze:2     add/replace a technology (name:workers)
  p1 tech- Warriors     remove a technology
  p1 wonder Pyramids 2  wonder in progress + steps built
  p1 built+ Colossus    completed a wonder
  p1 leader Caesar      played a leader ('-' clears)
  p1 tactic Legion      played a tactic
  p1 gov=Monarchy       changed government
  p1 hc=3 hm=2          rival hand sizes
  event <card>          a new current event was revealed
  age II                the age advanced
  ?                     anything you don't know -- e.g. p1 c=?
""" % ", ".join(sorted(set(SCALARS) | set(FORCED)))


def patch(board, line):
    """Apply ONE human-typed update line.  Returns a short confirmation.

    Raises :class:`PatchError` (never anything else) for input we cannot use,
    so the caller can print the message and ask again.
    """
    try:
        return _patch(board, line)
    except PatchError:
        raise
    except Exception as exc:                       # never crash on input
        raise PatchError(f"{type(exc).__name__}: {exc}")


def _patch(board, line):
    st = board.state
    line = line.strip()
    if not line or line.startswith("#"):
        return ""
    word, _, rest = line.partition(" ")
    word = word.lower()
    rest = rest.strip()

    if word == "deal":
        names = [resolve_card(t, "row") for t in _split_cards(rest, "row")]
        advance_row(board, names)
        return f"row advanced, dealt {len(names)} card(s)"
    if word == "row":
        toks = _split_cards(rest, "row")
        if not toks:
            raise PatchError("usage: row <up to 13 cards, '.' for empty>")
        names = [None if t == EMPTY else resolve_card(t, "row") for t in toks]
        sync_row(board, names)
        return "row set"
    if word == "take":
        bits = rest.split()
        if len(bits) != 2:
            raise PatchError("usage: take p<N> <slot 0-12>")
        idx = _player_idx(bits[0])
        slot = _int(bits[1], "slot")
        if not 0 <= slot < ROW_SIZE:
            raise PatchError(f"row slot must be 0..{ROW_SIZE - 1}")
        name = st.card_row[slot]
        if name is None:
            raise PatchError(f"row slot {slot} is already empty")
        st.card_row[slot] = None
        board.hidden[(idx, "civil")] = board.hidden_count(idx, "civil") + 1
        return f"p{idx} took {name} from slot {slot}"
    if word == "event":
        for t in _split_cards(rest, "military"):
            st.current_events.append(resolve_card(t, "military"))
        return "current event(s) noted"
    if word == "age":
        age = rest.strip().upper()
        if age not in C.AGES:
            raise PatchError(f"age must be one of {', '.join(C.AGES)}")
        st.age_civil = st.age_military = age
        return f"age -> {age}"
    if word in ("last", "last_round"):
        st.last_round = True
        return "final round"
    if word in ("turn", "cur", "current"):
        st.current = _player_idx(rest)
        return f"current player = p{st.current}"
    if re.fullmatch(r"p\d+", word):
        return _patch_player(board, int(word[1:]), rest)
    raise PatchError(f"don't understand {word!r}.\n{PATCH_HELP}")


def _player_idx(tok):
    tok = tok.strip().lower().lstrip("p")
    if not tok.isdigit():
        raise PatchError(f"expected a player like p1, got {tok!r}")
    return int(tok)


def _patch_player(board, idx, rest):
    p = board.player(idx)
    st = board.state
    words = rest.split(None, 1)
    head = words[0].lower() if words else ""
    tail = words[1].strip() if len(words) > 1 else ""

    if head in ("tech+", "+tech"):
        out = []
        for tok in _split_cards(tail, "tech"):
            name, _, w = tok.rpartition(":")
            if not name:
                name, w = tok, "1"
            name = resolve_card(name, "tech")
            p.techs[name] = TechCard(name, workers=_int(w, "workers"))
            out.append(f"{name}:{p.techs[name].workers}")
        effects.invalidate(st, p)
        return f"p{idx} techs " + ", ".join(out)
    if head in ("tech-", "-tech"):
        for tok in _split_cards(tail, "tech"):
            name = resolve_card(tok, "tech", extra=list(p.techs))
            p.techs.pop(name, None)
        effects.invalidate(st, p)
        return f"p{idx} removed {tail}"
    if head in ("built+", "+built", "built"):
        for tok in _split_cards(tail, "wonder"):
            name = resolve_card(tok, "wonder")
            if name not in p.completed_wonders:
                p.completed_wonders.append(name)
            if p.wonder and p.wonder.name == name:
                p.wonder = None
        effects.invalidate(st, p)
        return f"p{idx} completed {tail}"
    if head in ("wonder", "wonder+"):
        if tail in ("-", EMPTY, ""):
            p.wonder = None
            return f"p{idx} has no wonder in progress"
        _load_player_line(board, idx, f"wonder {tail}")
        return f"p{idx} wonder {p.wonder.name} {p.wonder.steps_built} step(s)"
    if head in ("leader", "tactic", "hand", "hidden", "gov", "tech"):
        _load_player_line(board, idx, rest)
        return f"p{idx} {head} updated"
    msgs = _apply_player_tokens(board, p, rest)
    return "; ".join(m for m in msgs if m) or f"p{idx} updated"


def patch_all(board, text):
    """Apply a whole block of update lines; returns (messages, errors)."""
    msgs, errs = [], []
    for ln in text.splitlines():
        try:
            m = patch(board, ln)
            if m:
                msgs.append(m)
        except PatchError as exc:
            errs.append(f"{ln.strip()!r}: {exc}")
    return msgs, errs


# ------------------------------------------------------------ pretty print

def render(board, width=78):
    """A board summary a human can check against the table in one glance."""
    st = board.state
    L = []
    L.append(f"== TTA  round {st.round}  age {st.age_civil}"
             f"  turn: p{st.current}"
             f"{'  (you)' if st.current == board.me else ''}"
             f"{'  [FINAL ROUND]' if st.last_round else ''}")
    L.append("-" * width)
    L.append("card row (cost 1 / 1 1 1 1 | 2 2 2 2 | 3 3 3 3):")
    for i, name in enumerate(st.card_row):
        cost = 1 if i < 5 else (2 if i < 9 else 3)
        label = name if name else "--"
        extra = ""
        if name:
            card = C.db().get(name)
            extra = f"  [{card['type']} {card['age']}]"
        L.append(f"  {i:>2} ({cost}) {label}{extra}")
    if st.current_events:
        L.append(f"current events: {', '.join(st.current_events)}"
                 f"   (future {len(st.future_events)})")
    if st.available_tactics:
        L.append(f"tactics available: {', '.join(st.available_tactics)}")
    L.append("-" * width)
    for p in st.players:
        L.extend(_render_player(board, p))
    if board.unknown:
        L.append(f"unknown: {', '.join(sorted(board.unknown))}")
    return "\n".join(L)


def _render_player(board, p):
    from engine import economy
    st = board.state
    s = _stats(board, p)
    who = f"p{p.idx}" + (" (you)" if p.idx == board.me else "")
    need = economy.happy_required(p.yellow_bank)
    L = [f"{who}  {p.government}"
         f"{'  leader: ' + p.leader if p.leader else ''}"
         f"{'  tactic: ' + p.tactic if p.tactic else ''}"]
    L.append(f"   culture {p.culture} (+{s.culture}/t)   "
             f"science {p.science} (+{s.science}/t)   strength {s.strength}   "
             f"happy {s.happy}/{need}")
    L.append(f"   food {p.food} (+{s.food})   res {p.resources} (+{s.resources})"
             f"   CA {p.civil_actions}/{s.civil_actions}"
             f"   MA {p.military_actions}/{s.military_actions}"
             f"   free workers {p.workers_free}   yellow bank {p.yellow_bank}")
    techs = ", ".join(f"{n}:{t.workers}" for n, t in sorted(p.techs.items())
                      if t.workers or C.db().type_of(n) != "special-tech")
    L.append(f"   techs: {techs}")
    if p.wonder:
        stages = C.db().get(p.wonder.name)["stages"]
        done = p.wonder.steps_built
        left = ",".join(str(x) for x in stages[done:])
        L.append(f"   building: {p.wonder.name} {done}/{len(stages)}"
                 f"  (remaining stages {left})")
    if p.completed_wonders:
        L.append(f"   wonders: {', '.join(p.completed_wonders)}")
    hc = board.hand_size(p.idx, "civil")
    hm = board.hand_size(p.idx, "military")
    if p.idx == board.me and (p.hand_civil or p.hand_military):
        L.append(f"   hand civil: {_fmt_hand(p.hand_civil)}")
        L.append(f"   hand mil:   {_fmt_hand(p.hand_military)}")
    else:
        L.append(f"   hand: {hc} civil, {hm} military")
    _ = st
    return L
