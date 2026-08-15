"""Replay a BGO journal into our `GameState` and re-score it with our engine.

`docs/HUMAN_BASELINE.md` compares our champion's final culture (84) with the
human corpus (160) and says plainly that nothing verifies our end-of-game
scoring is the same arithmetic BGO ran.  This is that check.

The journal is a complete public log of one game, and BGO prints three things
this tool leans on:

* every ``End turn`` line prints that player's **production rates** --
  ``N culture (now C); N science (now S); N food - consumption: K; N resources``
  -- so the last one of a game is a per-player snapshot of five engine outputs
  on a position we can rebuild;
* the ``Impact of ...`` lines at game end print **each player's award for each
  Age III scoring event**, which is `engine.events.scoring_culture` computed by
  somebody else;
* the ``End of game`` line prints the **final totals**.

So: replay the journal into a `PlayerState` per seat (tableau, workers,
government, leader, wonders, colonies, yellow bank), then ask our engine for
`effects.state_stats` and `events.scoring_culture` on that position and diff
against BGO's own integers.

A mismatch is ambiguous -- it can be this replayer dropping a worker as easily
as an engine bug -- so the tool reports the five rate residuals FIRST and only
counts an impact-event comparison as evidence when the rates on that row are
exact.  A replay that reproduces culture, science, food, resources and
consumption to the point is a replay whose tableau is right.

    python3 tools/bgo_rescore.py --journals /tmp/bgo/journals --limit 200
    python3 tools/bgo_rescore.py --journals /tmp/bgo/journals --game 7520718 -v
"""
from __future__ import annotations

import argparse
import os
import re
import sys
from collections import Counter, defaultdict

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine import actions, cards, economy, effects, events   # noqa: E402
from engine.state import GameState, PlayerState, TechCard     # noqa: E402
from tools.bgo_parse import (                                 # noqa: E402
    NAME_FIXES, COLOURS, WONDER_STAGES, _ELECTABLE)

_DB = cards.db()
_COL = "|".join(COLOURS)

GOVERNMENTS = ("Despotism", "Monarchy", "Theocracy", "Republic",
               "Constitutional Monarchy", "Democracy", "Fundamentalism",
               "Communism")

START_TECHS = {"Warriors": 1, "Agriculture": 2, "Bronze": 2,
               "Philosophy": 1, "Religion": 0}

AGE_ORDER = {"A": 0, "I": 1, "II": 2, "III": 3, "IV": 4}

# ---------------------------------------------------------------- regexes

RE_ENDTURN = re.compile(
    r"End turn (%s) scores:.*?(-?\d+) culture \(now (-?\d+)\); "
    r"(-?\d+) science \(now (-?\d+)\); (-?\d+) food - consumption: (\d+) "
    r"\(now (-?\d+)\); (-?\d+) resources" % _COL)
RE_BUILD_STAGE = re.compile(
    r"(%s) builds (\d+) stages? of ([A-Za-z'. ]+?)(?=;|\s(?:%s)\s|$)"
    % (_COL, _COL))
RE_BUILD = re.compile(
    r"^(%s) builds ([A-Za-z'. ]+?)(?: using [A-Za-z'. ]+)?"
    r"(?= \1 (?:spends|loses|produces)|$)" % _COL)
RE_UPGRADE = re.compile(
    r"(%s) upgrades ([A-Za-z'. ]+?) to ([A-Za-z'. ]+?)"
    r"(?:\s+using [A-Za-z'. ]+?)?(?=;|\s\1\s|$)" % _COL)
RE_DISCOVER = re.compile(
    r"(%s) discovers ([A-Za-z'. ]+?)(?: using [A-Za-z'. ]+?)?"
    r"(?= \1 (?:loses|gets)|;|$)" % _COL)
RE_DESTROY = re.compile(r"^(%s) (?:destroys|disbands) ([A-Za-z'. ]+?)$" % _COL)
RE_REVOL = re.compile(r"(%s) revolutions.*?Change government to ([A-Za-z ]+?);"
                      % _COL)
# The leader name must come from the known list: "elects William Shakespeare
# Leonardo Da Vinci dies" truncates to "William" against any generic pattern
# (`docs/HUMAN_BASELINE.md` records the same bug costing 39% of elections).
RE_ELECT = re.compile(r"^(%s) elects (%s)\b"
                      % (_COL, "|".join(re.escape(n) for n in
                                        sorted(_ELECTABLE, key=len,
                                               reverse=True))))
#: BGO prints `sets up new tactics` / `adopts existing tactics` followed by
#: the tactic card name; the tactic stays in play until replaced.  Without it
#: `effects.army_strength` is always 0 and every Impact of Strength /
#: Science ranking is computed on a tableau missing up to ~14 strength.
RE_TACTIC = re.compile(
    r"^(%s) (?:sets up new tactics|adopts existing tactics) [AIV]+ / "
    r"([A-Za-z' -]+?)(?:;|$)" % _COL)
RE_POP = re.compile(r"(%s) increases population" % _COL)
RE_GETPOP = re.compile(r"(%s) gets (\d+) population" % _COL)
RE_LOSEPOP = re.compile(r"(%s) loses (\d+) population" % _COL)
RE_YELLOW_GET = re.compile(r"(%s) gets (\d+) yellow token" % _COL)
RE_YELLOW_LOSE = re.compile(r"(%s) loses (\d+) yellow token" % _COL)
RE_COLONIZE = re.compile(r"^(%s) colonizes a ([A-Za-z ]+Territory)" % _COL)
RE_TERR_AGE = re.compile(r"(A|I{1,3}) / ([A-Za-z ]+Territory)")
RE_TERRORIST = re.compile(r"Terrorists destroy a (%s) ([A-Za-z'. ]+?)$" % _COL)
#: BGO prints "The Pyramids crumble" / "Ravages of Time The Pyramids
#: crumble"; the definite article and the event prefix are not part of
#: any card name, and leaving them on silently flipped nothing.
RE_CRUMBLE = re.compile(r"^(?:Ravages of Time )?(?:The )?"
                        r"([A-Za-z'. ]+?) crumbles?$")
RE_IMPACT = re.compile(r"^Impact of ([A-Za-z]+)\b")
RE_SCORES = re.compile(r"(%s) scores (\d+) culture" % _COL)
RE_ENDGAME = re.compile(r"^End of game")
RE_WINNER = re.compile(r"AS (%s) \((\d+) PTS\)" % _COL.upper(), re.I)
RE_PLACE = re.compile(r"is .*? as (%s) \((\d+) pts\)" % _COL)

SPECIAL_ICONS = {}   # name -> icon, filled lazily


#: BGO prints the Age A infantry card in the singular when a worker is put on
#: it ("Green builds Warrior"); `tools/bgo_parse.py` never needed the mapping
#: because it only reads *takes*, and the card is never taken.
LOCAL_FIXES = {"Warrior": "Warriors"}


def fix(name):
    name = name.strip()
    name = NAME_FIXES.get(name, name)
    return LOCAL_FIXES.get(name, name)


def card_of(name):
    return _DB.get(name) if name in _DB.by_name else None


class Seat:
    def __init__(self, colour, idx):
        self.colour = colour
        self.idx = idx
        self.techs = dict(START_TECHS)
        self.government = "Despotism"
        self.leader = None
        self.tactic = None
        self.wonder_stages = Counter()
        self.completed = []          # in completion order
        self.flipped = []
        self.colonies = []
        self.bank = 18
        self.free = 1
        self.blue_extra = 0
        self.culture = 0
        self.science = 0
        # last End-turn snapshot: (culture_rate, science_rate, food_net,
        # consumption, resources_net)
        self.last_rates = None
        self.rate_hist = []
        self.cons_checks = []        # (predicted_bank, printed_consumption)
        self.snaps = []              # tableau snapshot at every End turn
        self.bad = 0                 # replay events this seat cannot model
        #: yellow tokens this seat should own (25 at setup, -2 per age end,
        #: plus card grants and transfers).  `bank + free + workers` must
        #: equal it; when it does not, this replay lost a worker somewhere and
        #: the row is not evidence about anything.
        self.tokens = 25
        self.extra_culture = 0       # culture scored after the last End turn
        self.gates_bonus = None      # BGO's "Bill Gates scoring" line
        #: BGO's fourth oracle: every "...; Wonder completed; <Colour> scores
        #: N culture" line is an Age III wonder's one-time bonus, computed by
        #: somebody else on a tableau we can rebuild.  Entries are dicts (see
        #: `_wonder_snapshot`).
        self.wonder_scores = []

    # -- worker bookkeeping ------------------------------------------------
    def add_worker(self, tech):
        self.techs[tech] = self.techs.get(tech, 0) + 1
        self.free -= 1

    def drop_worker(self, tech, to_bank=False):
        if self.techs.get(tech, 0) <= 0:
            self.bad += 1
            return False
        self.techs[tech] -= 1
        if to_bank:
            self.bank += 1
        else:
            self.free += 1
        return True


def _wonder_snapshot(s, wonder, printed):
    """Freeze a seat's tableau at the instant a wonder completed."""
    return {"wonder": wonder, "want": printed,
            "snap": (dict(s.techs), s.government, s.leader, list(s.completed),
                     list(s.flipped), list(s.colonies), s.bank, s.blue_extra,
                     None, s.tactic),
            "bad": s.bad,
            #: the seat's own token audit at this instant, and the index of
            #: the last End-turn snapshot before it (whose five production
            #: numbers BGO printed -- that is what verifies the tableau)
            "tokens_ok": (s.bank + s.free + sum(s.techs.values()) == s.tokens),
            "prev_snap": len(s.snaps) - 1}


def _special_icon(name):
    if name not in SPECIAL_ICONS:
        c = card_of(name)
        SPECIAL_ICONS[name] = actions.special_icon(c) if c else None
    return SPECIAL_ICONS[name]


def replay(path, age_loss=2):
    """Return (seats, end_impacts, final_scores, rounds, warnings)."""
    seats = {}
    order = []
    end_impacts = []          # (impact_name, {colour: culture})
    final_scores = {}
    rounds = 0
    warn = Counter()
    in_endgame = False
    endgame_names = []
    last_terr_age = {}

    def seat(colour):
        if colour not in seats:
            seats[colour] = Seat(colour, len(order))
            order.append(colour)
        return seats[colour]

    max_age = 0
    with open(path, errors="replace") as fh:
        header = fh.readline()
        for line in fh:
            parts = line.rstrip("\n").split("\t")
            if len(parts) < 5:
                continue
            colour, age, rnd, text = parts[1], parts[2], parts[3], parts[4]
            try:
                rounds = max(rounds, int(rnd))
            except ValueError:
                pass
            # global age clock: -2 yellow at the end of ages I, II, III
            a = AGE_ORDER.get(age, 0)
            if a > max_age:
                for step in range(max_age + 1, a + 1):
                    if step >= 2:            # end of I -> II, II -> III, ...
                        for s in seats.values():
                            s.tokens -= min(age_loss, s.bank)
                            s.bank = max(0, s.bank - age_loss)
                    # a leader of an age OLDER than the age that just ended is
                    # removed (§9.1); BGO prints nothing when this happens
                    ended = step - 1
                    for s in seats.values():
                        if s.leader:
                            c = card_of(s.leader)
                            if c and AGE_ORDER.get(c.get("age"), 0) < ended:
                                s.leader = None
                max_age = a
            if colour in COLOURS:
                seat(colour)

            # ---------------- end of game ----------------
            if RE_ENDGAME.search(text):
                in_endgame = True
                body = text.split(":", 1)[-1]
                endgame_names = [x.strip() for x in body.split(";")
                                 if x.strip().startswith("Impact of")]
                m = RE_WINNER.search(text)
                if m:
                    final_scores[m.group(1).capitalize()] = int(m.group(2))
                for c, pts in RE_PLACE.findall(text):
                    final_scores[c] = int(pts)
                continue

            m = RE_IMPACT.match(text)
            if m and in_endgame:
                name = "Impact of " + m.group(1)
                awards = {c: int(v) for c, v in RE_SCORES.findall(text)}
                end_impacts.append((name, awards))
                continue

            _apply_line(seats, seat, text, warn, last_terr_age)

    return (seats, order, end_impacts, final_scores, rounds,
            endgame_names, warn)


def _apply_line(seats, seat, text, warn, last_terr_age):
    # --- production snapshot
    m = RE_ENDTURN.search(text)
    if m:
        s = seat(m.group(1))
        s.last_rates = (int(m.group(2)), int(m.group(4)), int(m.group(6)),
                        int(m.group(7)), int(m.group(9)))
        s.culture = int(m.group(3))
        s.science = int(m.group(5))
        s.extra_culture = 0
        s.rate_hist.append(s.last_rates)
        s.cons_checks.append((s.bank, int(m.group(7))))
        s.snaps.append((dict(s.techs), s.government, s.leader,
                        list(s.completed), list(s.flipped), list(s.colonies),
                        s.bank, s.blue_extra, s.last_rates, s.tactic))
        return
    # culture scored after this player's last End turn still counts (an event
    # revealed on a rival's turn, a wonder completed, Bill Gates)
    for colour, v in RE_SCORES.findall(text):
        seat(colour).extra_culture += int(v)
    for colour, v in re.findall(r"(%s) loses (\d+) culture" % _COL, text):
        seat(colour).extra_culture -= int(v)
    m = re.match(r"Bill Gates scoring (%s) scores (\d+) culture" % _COL, text)
    if m:
        seat(m.group(1)).gates_bonus = int(m.group(2))

    # --- wonder stages (may be nested inside Engineering Genius)
    finished = []
    for colour, n, wname in RE_BUILD_STAGE.findall(text):
        s = seat(colour)
        w = fix(wname)
        if w not in WONDER_STAGES:
            warn["unknown_wonder:" + w] += 1
            continue
        s.wonder_stages[w] += int(n)
        if s.wonder_stages[w] >= WONDER_STAGES[w] and w not in s.completed:
            s.completed.append(w)
            finished.append((s, w))
    # "... Wonder completed; <Colour> scores N culture" -- the Age III
    # one-time bonus.  Only used when exactly ONE wonder finished on the line
    # and exactly one culture figure is attributed to its owner, so the number
    # cannot be a sum of two effects.
    if len(finished) == 1 and "Wonder completed" in text:
        s, w = finished[0]
        paid = RE_SCORES.findall(text)
        if len(paid) == 1 and paid[0][0] == s.colour:
            s.wonder_scores.append(_wonder_snapshot(s, w, int(paid[0][1])))

    # --- plain build (worker onto a technology)
    m = RE_BUILD.match(text)
    if m and " stage" not in text.split(" builds ", 1)[1][:20]:
        s, name = seat(m.group(1)), fix(m.group(2))
        c = card_of(name)
        if c is None:
            warn["unknown_build:" + name] += 1
            s.bad += 1
        else:
            s.add_worker(name)

    # --- upgrade
    for colour, lo, hi in RE_UPGRADE.findall(text):
        s = seat(colour)
        lo, hi = fix(lo), fix(hi)
        if card_of(hi) is None:
            warn["unknown_upgrade:" + hi] += 1
            continue
        # the worker MOVES: it must not pass through the unused-worker pool
        if not s.drop_worker(lo):
            warn["upgrade_from_empty:" + lo] += 1
        else:
            s.free -= 1
        s.techs[hi] = s.techs.get(hi, 0) + 1

    # --- discover (develop a technology)
    for colour, name in RE_DISCOVER.findall(text):
        s, name = seat(colour), fix(name)
        c = card_of(name)
        if c is None:
            warn["unknown_tech:" + name] += 1
            continue
        if name in GOVERNMENTS:
            s.government = name
        elif c.get("type") == "special-tech":
            icon = _special_icon(name)
            for other in [n for n in s.techs
                          if card_of(n) and card_of(n).get("type")
                          == "special-tech" and _special_icon(n) == icon]:
                del s.techs[other]
            s.techs.setdefault(name, 0)
        else:
            s.techs.setdefault(name, 0)

    # --- revolution
    for colour, gov in RE_REVOL.findall(text):
        seat(colour).government = gov.strip()

    # --- destroy / disband
    m = RE_DESTROY.match(text)
    if m:
        s, name = seat(m.group(1)), fix(m.group(2))
        if not s.drop_worker(name):
            warn["destroy_empty:" + name] += 1

    # --- raid casualties: "Raid casualties 1 Printing Press; 1 Drama; X ..."
    if text.startswith("Raid casualties"):
        for piece in text.split(";"):
            mm = re.match(r"\s*(\d+) ([A-Za-z'. ]+?)\s*$", piece)
            if mm:
                warn["raid_casualty"] += 1

    # --- terrorists
    m = RE_TERRORIST.search(text)
    if m:
        seat(m.group(1)).drop_worker(fix(m.group(2)))

    # --- leader election
    m = RE_ELECT.match(text)
    if m:
        seat(m.group(1)).leader = fix(m.group(2))

    # --- tactic: set/replaced by name; stays until the next such line
    m = RE_TACTIC.match(text)
    if m:
        seat(m.group(1)).tactic = m.group(2).strip()

    # --- population (also nested: "X plays Frugality X increases population")
    for colour in RE_POP.findall(text):
        s = seat(colour)
        s.free += 1
        s.bank = max(0, s.bank - 1)
    for colour, n in RE_GETPOP.findall(text):
        s = seat(colour)
        s.free += int(n)
        s.bank = max(0, s.bank - int(n))
    for colour, n in RE_LOSEPOP.findall(text):
        # a per-player resolution line, not the aggression's card text (which
        # says "Your rival loses 1 population" and carries no colour)
        s = seat(colour)
        for _ in range(int(n)):
            s.bank += 1
            if s.free > 0:
                s.free -= 1
            else:
                warn["lose_pop_from_card"] += 1
    for colour, n in re.findall(r"(%s) gains \d+ culture ?and (\d+) population"
                                % _COL, text):
        s = seat(colour)
        s.free += int(n)
        s.bank = max(0, s.bank - int(n))
    for colour, n in re.findall(r"(%s) loses \d+ culture ?and (\d+) population"
                                % _COL, text):
        s = seat(colour)
        s.bank += int(n)
        s.free = max(0, s.free - int(n))
    # events whose per-player consequence BGO does NOT print
    m = re.search(r"Each civilization gains (\d+) population", text)
    if m:
        for s in seats.values():
            s.free += int(m.group(1))
            s.bank = max(0, s.bank - int(m.group(1)))
    m = re.search(r"Each civilization loses (\d+) population", text)
    if m:
        for s in seats.values():
            s.bank += int(m.group(1))
            if s.free >= int(m.group(1)):
                s.free -= int(m.group(1))
            else:
                s.bad += 1
    for colour, n in RE_YELLOW_GET.findall(text):
        seat(colour).bank += int(n)
        seat(colour).tokens += int(n)
    for colour, n in RE_YELLOW_LOSE.findall(text):
        s = seat(colour)
        s.tokens -= min(int(n), s.bank)
        s.bank = max(0, s.bank - int(n))

    # --- territory age hints and colonisation
    for age, tname in RE_TERR_AGE.findall(text):
        last_terr_age[tname] = age
    m = RE_COLONIZE.match(text)
    if m:
        s, terr = seat(m.group(1)), m.group(2)
        age = last_terr_age.get(terr, "I")
        s.colonies.append((terr, age))
        key = "%s (%s)" % (terr, age)
        c = card_of(key)
        if c:
            perm = c.get("permanentEffects") or {}
            s.bank += int(perm.get("yellowTokens", 0) or 0)
            s.tokens += int(perm.get("yellowTokens", 0) or 0)
            s.blue_extra += int(perm.get("blueTokens", 0) or 0)
        else:
            warn["unknown_territory:" + key] += 1
        for piece in text.split(";")[1:]:
            mm = re.match(r"\s*(\d+) ([A-Za-z'. ]+?)\s*$", piece)
            if not mm:
                continue
            unit = fix(mm.group(2))
            if unit in ("Colonization card", "Total force"):
                continue
            for _ in range(int(mm.group(1))):
                if not s.drop_worker(unit, to_bank=True):
                    # BGO prints singular unit names ("1 Warrior")
                    alt = unit + "s"
                    if not s.drop_worker(alt, to_bank=True):
                        warn["colonize_unit:" + unit] += 1

    # --- effects this replayer does not model; the row is marked unusable
    for pat, tag in ((" plays Annex against ", "annex"),
                     (" plays Infiltrate against ", "infiltrate"),
                     ("Iconoclasm", "iconoclasm"),
                     ("Raid casualties", "raid"),
                     ("Terrorists destroy", "terrorists"),
                     ("Barbarossa enlists", "barbarossa")):
        if pat in text:
            warn["unmodelled_" + tag] += 1
            for s in seats.values():
                s.bad += 1

    # --- Ravages of Time
    if "crumble" in text:
        for piece in text.split(";"):
            mm = RE_CRUMBLE.match(piece.strip())
            if mm:
                w = fix(mm.group(1))
                if w in WONDER_STAGES:
                    for s in seats.values():
                        if w in s.completed and w not in s.flipped:
                            s.flipped.append(w)
                            break


# ------------------------------------------------------------ engine side

def build_state(seats, order):
    gs = GameState(num_players=len(order), seed=0)
    for colour in order:
        s = seats[colour]
        p = PlayerState(idx=s.idx)
        p.techs = {n: TechCard(name=n, workers=w) for n, w in s.techs.items()}
        p.government = s.government
        p.leader = s.leader if s.leader and s.leader in _DB.by_name else None
        p.tactic = s.tactic if s.tactic and s.tactic in _DB.by_name else None
        p.completed_wonders = list(s.completed)
        p.flipped_wonders = list(s.flipped)
        p.colonies = [n for n in ("%s (%s)" % (t, a)
                      for t, a in s.colonies) if n in _DB.by_name]
        p.yellow_bank = max(0, s.bank)
        p.workers_free = max(0, s.free)
        p.culture = s.culture + s.extra_culture
        p.science = s.science
        p.blue_total = 16 + s.blue_extra
        gs.players.append(p)
    return gs


#: journal impact name -> the card name in data/cards_military_actions.json
IMPACTS = {}


def _load_impacts():
    for name in _DB.by_name:
        if name.startswith("Impact of"):
            IMPACTS[name] = name


_load_impacts()

RANKING = {"Impact of Science", "Impact of Strength"}


def alt_score(gs, p, name, got):
    """Rival readings of three cards, to localise a systematic residual."""
    db = _DB
    if name == "Impact of Population":
        # count UNUSED workers as population too
        workers = sum(t.workers for t in p.techs.values()) + p.workers_free
        content = max(0, workers - economy.discontent(gs, p))
        return 2 * max(0, content - 10)
    if name == "Impact of Industry":
        # resources produced by MINES only, not the resource rating; the
        # Transcontinental Railroad's doubled worker is a mine and counts
        # (data/cards_wonders_leaders.json cites FAQ v1.5 p.9)
        tot, best, best_lv = 0, None, -1
        for n, t in p.techs.items():
            if db.type_of(n) != "mine":
                continue
            per = (db.get(n).get("production") or {}).get("resources", 0)
            tot += per * t.workers
            if t.workers and db.level_of(n) > best_lv:
                best, best_lv = n, db.level_of(n)
        if "Transcontinental Railroad" in p.completed_wonders and best:
            tot += (db.get(best).get("production") or {}).get("resources", 0)
        return tot
    return got


def check_game(path, verbose=False, age_loss=2):
    seats, order, end_impacts, final_scores, rounds, names, warn = replay(
        path, age_loss=age_loss)
    gs = build_state(seats, order)
    out = {"game": os.path.basename(path).split(".")[0],
           "rounds": rounds, "players": len(order),
           "rate_rows": 0, "rate_ok": 0, "impacts": [], "warn": warn,
           "final_delta": None, "cons": Counter(), "gates": [],
           "turns": Counter(), "wonders": []}
    res = []
    for colour in order:
        s = seats[colour]
        p = gs.players[s.idx]
        if s.last_rates is None:
            continue
        for i, snap in enumerate(s.snaps):
            # snap is (techs, gov, leader, comp, flip, cols, bank, blue,
            # last_rates, tactic) -- the printed five rates are element 8
            got_t = _rates_from_snap(snap, len(order), s.idx)
            want_t = snap[8]
            out["turns"]["n"] += 1
            out["turns"]["all5"] += int(got_t == want_t)
            for k, lbl in enumerate(LBL):
                out["turns"][lbl] += int(got_t[k] == want_t[k])
            b = min(i // 5, 3)
            out["turns"]["b%d_n" % b] += 1
            out["turns"]["b%d_ok" % b] += int(got_t == want_t)
        for bank, printed in s.cons_checks:
            got = economy.consumption(max(0, bank))
            out["cons"]["n"] += 1
            out["cons"]["ok" if got == printed else "bad"] += 1
            out["cons"]["d%+d" % (got - printed)] += 1
        st = effects.state_stats(gs, p)
        cons = economy.consumption(p.yellow_bank)
        got = (st.culture, st.science, st.food - cons, cons, st.resources)
        want = s.last_rates
        out["rate_rows"] += 1
        ok = (got == want and s.bad == 0 and s.free >= 0
              and s.bank + s.free + sum(s.techs.values()) == s.tokens)
        if ok:
            out["rate_ok"] += 1
        res.append((colour, want, got, ok))
        if verbose:
            print(f"  {colour:7s} rates want={want} got={got} "
                  f"{'OK' if ok else 'MISMATCH'}")
    out["rate_rows_detail"] = res

    # --- end-of-game impact events
    ok_colours = {c for c, _w, _g, ok in res if ok}
    all_clean = bool(res) and len(ok_colours) == len(res)
    for name, awards in end_impacts:
        if name not in _DB.by_name:
            out["warn"]["no_such_impact:" + name] += 1
            continue
        block = (_DB.get(name).get("effects") or {}).get("allPlayers") or {}
        ranking = name in RANKING or "rankingCulture" in block
        for colour in order:
            p = gs.players[seats[colour].idx]
            got = events.scoring_culture(gs, p, block, gs.players)
            if ranking:
                table = (block["rankingCulture"].get("%dp" % len(order))
                         or [])
                stat = {"strengthRating": "strength",
                        "scienceProduction": "science"}[block["statistic"]]
                rk = events._rank(gs, gs.players, stat, True)
                if p in rk and rk.index(p) < len(table):
                    got += table[rk.index(p)]
            want = awards.get(colour, 0)
            out["impacts"].append(
                {"impact": name, "colour": colour, "want": want, "got": got,
                 "alt": alt_score(gs, p, name, got),
                 "clean": (all_clean if ranking else colour in ok_colours),
                 "ranking": ranking})
            if verbose:
                print(f"  {name:24s} {colour:7s} want={want:3d} got={got:3d}"
                      f" {'' if want == got else '  <<< DIFF'}"
                      f"{' (rank)' if ranking else ''}"
                      f"{'' if colour in ok_colours else ' (dirty)'}")

    # --- Age III wonder one-time bonuses, at the instant of completion
    for colour in order:
        s = seats[colour]
        for rec in s.wonder_scores:
            i = rec["prev_snap"]
            verified = (i >= 0
                        and _rates_from_snap(s.snaps[i], len(order), s.idx)
                        == s.snaps[i][8])
            gs2, p2 = _state_from_snap(rec["snap"], len(order), s.idx)
            out["wonders"].append(
                {"wonder": rec["wonder"], "want": rec["want"],
                 "got": effects.on_wonder_complete(gs2, p2, rec["wonder"]),
                 "leader": p2.leader or "-",
                 "clean": rec["bad"] == 0 and rec["tokens_ok"] and verified})

    for colour in order:
        s = seats[colour]
        if s.gates_bonus is not None:
            out["gates"].append((s.gates_bonus,
                                 effects.end_of_game_bonus(gs,
                                                           gs.players[s.idx])))

    # --- total: last culture + end impacts == printed final score
    if final_scores:
        deltas = {}
        for colour in order:
            base = seats[colour].culture + seats[colour].extra_culture
            add = sum(a.get(colour, 0) for _n, a in end_impacts)
            if colour in final_scores:
                deltas[colour] = base + add - final_scores[colour]
        out["final_delta"] = deltas
    return out


LBL = ("culture", "science", "food", "consumption", "resources")


def _state_from_snap(snap, nplayers, idx):
    # 10 elements when the snapshot carries a tactic (End-turn rows and the
    # wonder records written after that change); 9 for anything older.
    snap = tuple(snap)
    snap = snap + (None,) * (10 - len(snap))
    techs, gov, leader, comp, flip, cols, bank, blue, _want, tactic = snap
    gs = GameState(num_players=max(1, nplayers), seed=0)
    for i in range(max(1, nplayers)):
        gs.players.append(PlayerState(idx=i))
    p = gs.players[idx]
    p.techs = {n: TechCard(name=n, workers=w) for n, w in techs.items()}
    p.government = gov
    p.leader = leader if leader and leader in _DB.by_name else None
    p.tactic = tactic if tactic and tactic in _DB.by_name else None
    p.completed_wonders = list(comp)
    p.flipped_wonders = list(flip)
    p.colonies = [n for n in ("%s (%s)" % (t, a) for t, a in cols)
                  if n in _DB.by_name]
    p.yellow_bank = max(0, bank)
    p.blue_total = 16 + blue
    return gs, p


def _rates_from_snap(snap, nplayers, idx):
    gs, p = _state_from_snap(snap, nplayers, idx)
    st = effects.state_stats(gs, p)
    cons = economy.consumption(p.yellow_bank)
    return (st.culture, st.science, st.food - cons, cons, st.resources)


def trace_game(path, colour=None, age_loss=2):
    seats, order, _ei, _fs, _r, _n, warn = replay(path, age_loss=age_loss)
    for c in order:
        if colour and c != colour:
            continue
        s = seats[c]
        print("--", c)
        for i, snap in enumerate(s.snaps):
            got = _rates_from_snap(snap, len(order), s.idx)
            want = snap[-1]
            bad = [LBL[k] for k in range(5) if want[k] != got[k]]
            print(f"  t{i:2d} want={want} got={got} "
                  + (("BAD " + ",".join(bad)) if bad else "ok"))
    if warn:
        print("  warnings:", dict(warn))
    return 0


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--journals", default="/tmp/bgo/journals")
    ap.add_argument("--game", action="append", default=[])
    ap.add_argument("--limit", type=int, default=0)
    ap.add_argument("--players", type=int, default=0)
    ap.add_argument("-v", "--verbose", action="store_true")
    ap.add_argument("--trace", default=None,
                    help="print per-turn rate diffs for this colour")
    ap.add_argument("--age-loss", type=int, default=2,
                    help="yellow tokens lost at the end of ages I/II/III")
    a = ap.parse_args(argv)

    files = ([os.path.join(a.journals, g + ".tsv") for g in a.game]
             if a.game else
             sorted(os.path.join(a.journals, f)
                    for f in os.listdir(a.journals) if f.endswith(".tsv")))
    if a.limit:
        files = files[:a.limit]

    if a.trace is not None:
        for path in files:
            print("==", os.path.basename(path))
            trace_game(path, a.trace or None, age_loss=a.age_loss)
        return 0

    tot = Counter()
    per_impact = defaultdict(Counter)
    resid = defaultdict(Counter)
    rate_field_bad = Counter()
    final_bad = Counter()
    rounds_hist = Counter()
    cons = Counter()
    gates = Counter()
    turns = Counter()
    per_wonder = defaultdict(Counter)
    wresid = defaultdict(Counter)
    wleader = defaultdict(Counter)
    for path in files:
        if a.verbose:
            print("==", os.path.basename(path))
        try:
            r = check_game(path, a.verbose, age_loss=a.age_loss)
        except Exception as exc:                     # noqa: BLE001
            tot["crash"] += 1
            if a.verbose:
                print("  CRASH", exc)
            continue
        if a.players and r["players"] != a.players:
            continue
        tot["games"] += 1
        rounds_hist[r["rounds"]] += 1
        cons.update(r["cons"])
        turns.update(r["turns"])
        for want, got in r["gates"]:
            gates["n"] += 1
            gates["ok" if want == got else "bad"] += 1
        tot["rate_rows"] += r["rate_rows"]
        tot["rate_ok"] += r["rate_ok"]
        for _c, want, got, ok in r["rate_rows_detail"]:
            if not ok:
                for i, lbl in enumerate(("culture", "science", "food",
                                         "consumption", "resources")):
                    if want[i] != got[i]:
                        rate_field_bad[lbl] += 1
        for d in r["impacts"]:
            key = d["impact"]
            bucket = per_impact[key]
            tag = "clean" if d["clean"] else "dirty"
            bucket[tag + "_n"] += 1
            if d["want"] == d["got"]:
                bucket[tag + "_ok"] += 1
            elif d["clean"]:
                resid[key][d["got"] - d["want"]] += 1
            if d["clean"]:
                bucket["alt_ok"] += int(d["alt"] == d["want"])
        for d in r["wonders"]:
            b = per_wonder[d["wonder"]]
            tag = "clean" if d["clean"] else "dirty"
            b[tag + "_n"] += 1
            if d["want"] == d["got"]:
                b[tag + "_ok"] += 1
            elif d["clean"]:
                wresid[d["wonder"]][d["got"] - d["want"]] += 1
            if d["clean"]:
                wleader[d["wonder"]][d["leader"]] += 1
                wleader[d["wonder"]][d["leader"] + " ok"] += int(
                    d["want"] == d["got"])
        if r["final_delta"]:
            for c, dv in r["final_delta"].items():
                final_bad[dv] += 1

    print("\n=== replay coverage ===")
    print(f"games {tot['games']}   crashes {tot['crash']}")
    print(f"player-rows {tot['rate_rows']}   rates exact on all five: "
          f"{tot['rate_ok']} ({100.0*tot['rate_ok']/max(1,tot['rate_rows']):.1f}%)")
    if rate_field_bad:
        print("  mismatching field counts:", dict(rate_field_bad))
    md = sorted(rounds_hist.elements())
    if md:
        print(f"  journal rounds: median {md[len(md)//2]}  "
              f"mean {sum(md)/len(md):.2f}")

    print("\n=== per-turn production: our engine vs BGO, every End-turn line ===")
    n = max(1, turns["n"])
    print(f"  turn snapshots {turns['n']}   all five exact "
          f"{turns['all5']} ({100.0*turns['all5']/n:.1f}%)")
    for lbl in LBL:
        print(f"    {lbl:12s} {turns[lbl]:6d} ({100.0*turns[lbl]/n:5.1f}%)")
    for b, rng in enumerate(("turns 1-5", "turns 6-10", "turns 11-15",
                             "turns 16+")):
        bn = max(1, turns["b%d_n" % b])
        print(f"    {rng:12s} all five exact {turns['b%d_ok' % b]}/"
              f"{turns['b%d_n' % b]} ({100.0*turns['b%d_ok' % b]/bn:.1f}%)")

    print("\n=== yellow bank: our consumption() vs BGO's printed consumption ===")
    print(f"  end-turn lines {cons['n']}  exact {cons['ok']} "
          f"({100.0*cons['ok']/max(1,cons['n']):.1f}%)  (age-loss={a.age_loss})")
    dd = {k: v for k, v in cons.items() if k.startswith('d')}
    print("  residuals (ours - BGO):",
          ", ".join(f"{k[1:]}x{v}" for k, v in
                    sorted(dd.items(), key=lambda kv: -kv[1])[:7]))
    if gates["n"]:
        print(f"\n=== Bill Gates end-of-game bonus: {gates['ok']}/{gates['n']} exact")

    print("\n=== BGO's own arithmetic (last culture + end impacts - printed) ===")
    tot_f = sum(final_bad.values())
    for dv, n in sorted(final_bad.items(), key=lambda kv: -kv[1])[:8]:
        print(f"  delta {dv:+4d}: {n} rows ({100.0*n/max(1,tot_f):.1f}%)")

    print("\n=== end-of-game Age III events: our scorer vs BGO ===")
    print(f"{'event':28s} {'clean n':>8s} {'exact':>7s} {'%':>7s}   "
          f"{'all n':>7s} {'exact':>7s}")
    for name in sorted(per_impact):
        b = per_impact[name]
        cn, co = b["clean_n"], b["clean_ok"]
        an = cn + b["dirty_n"]
        ao = co + b["dirty_ok"]
        print(f"{name:28s} {cn:8d} {co:7d} "
              f"{100.0*co/max(1,cn):6.1f}%   {an:7d} {ao:7d}"
              + (f"   alt {b['alt_ok']} ({100.0*b['alt_ok']/max(1,cn):.1f}%)"
                 if b["alt_ok"] != co else ""))
        if resid[name] and co < cn:
            top = sorted(resid[name].items(), key=lambda kv: -kv[1])[:5]
            print("      residuals (ours - BGO):",
                  ", ".join(f"{d:+d}x{n}" for d, n in top))

    print("\n=== Age III wonder one-time bonus, at the instant of completion "
          "===")
    print(f"{'wonder':28s} {'clean n':>8s} {'exact':>7s} {'%':>7s}   "
          f"{'all n':>7s} {'exact':>7s}")
    for name in sorted(per_wonder):
        b = per_wonder[name]
        cn, co = b["clean_n"], b["clean_ok"]
        an, ao = cn + b["dirty_n"], co + b["dirty_ok"]
        print(f"{name:28s} {cn:8d} {co:7d} {100.0*co/max(1,cn):6.1f}%   "
              f"{an:7d} {ao:7d}")
        if wresid[name] and co < cn:
            top = sorted(wresid[name].items(), key=lambda kv: -kv[1])[:5]
            print("      residuals (ours - BGO):",
                  ", ".join(f"{d:+d}x{n}" for d, n in top))
            bad = [(l, wleader[name][l] - wleader[name][l + " ok"])
                   for l in sorted(wleader[name]) if not l.endswith(" ok")]
            print("      by leader (wrong/clean):",
                  ", ".join(f"{l} {w}/{wleader[name][l]}"
                            for l, w in bad if w))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
