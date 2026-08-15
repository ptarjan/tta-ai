"""Parse the BGO human-game journal corpus into per-player summary rows.

Input:  a directory of `<game_id>.tsv` journals (5 columns: date, player_colour,
        age, round, text) -- i.e. what `sources/bgo/journals.tar.gz` unpacks to --
        plus `sources/bgo/index.tsv` for the metadata row (player count, level).
Output: one TSV row per (game, player) on stdout or `--out`.

The journal text is templated server-side, so everything here is a regex against
a fixed sentence.  Where a template is ambiguous the parser records nothing
rather than guessing; `tools/bgo_stats.py` reports per-field coverage so a field
that silently stopped matching shows up as a coverage drop, not as a shifted
median.

Three journal facts that are not obvious and that the regexes depend on:

* **Take-backs exist.**  BGO lets a player undo a take inside their own turn:
  `X takes Foo in hand / X uses 1 civil action` followed by `X puts Foo back in
  the row / X gets 1 civil action`.  Both lines are in the journal.  A put-back
  is matched against the most recent unmatched take of the same card by the same
  player and *both* are dropped, so the CA-cost histogram counts decisions the
  player actually kept.

* **A wonder take costs more than its row slot.**  `engine/actions.py:take_cost`
  charges `row_cost(idx) + len(completed_wonders) + destroyed_wonders` for a
  wonder (unless the player's leader is Michelangelo).  That is why the corpus
  contains takes for 4-7 civil actions, all of them wonders.  To recover the row
  *slot tier* (the thing the "did they pay 3 to grab it early" question is about)
  the parser tracks completed wonders per player from the `builds N stage(s) of
  W` lines and subtracts.  Non-wonder takes need no correction and are reported
  separately as the clean subset.

* **Age/round columns are per-turn, not global.**  Row `age` is the age at the
  time of *that player's* turn, so two players can legitimately show different
  ages on the same wall-clock second.  That is fine for "cards taken per age"
  (which is per player anyway) but means you cannot read a single global age
  clock off the file.

Military strength is NOT recoverable: the journal never prints a player's army
strength except inside a war resolution.  `war_str_*` fields carry those
snapshots and nothing else does.
"""
from __future__ import annotations

import argparse
import json
import os
import re
import sys
from collections import Counter, defaultdict

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

DATA = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "data")

GOVERNMENTS = ("Despotism", "Monarchy", "Theocracy", "Republic",
               "Constitutional Monarchy", "Democracy", "Fundamentalism",
               "Communism")

COLOURS = ("Orange", "Purple", "Green", "Grey", "Blue", "Red", "Yellow",
           "White", "Black")

# BGO spells a few cards differently from data/*.json.  Left side is BGO.
NAME_FIXES = {
    "Leonardo Da Vinci": "Leonardo da Vinci",
    "Stockpile": "Stock Pile",
    "Bread & Circuses": "Bread and Circuses",
    "Ocean Liner": "Ocean Liners",
    "St. Peter's Basilica": "St. Peter's Basilica",
    # BGO renders three leaders under their full/native names
    "Charles Chaplin": "Charlie Chaplin",
    "Maximillien Robespierre": "Maximilien Robespierre",
    "Johannes Sebastian Bach": "J. S. Bach",
}


def load_cards():
    """name -> (age, kind) for every base-2015 card a player can take.

    CAUTION: ~15 action-card names exist at more than one age (Urban Growth is
    in the A, I, II and III decks; Reserves in I, II, III; and so on), and the
    journal prints only the name.  `age` here is therefore the *earliest* age
    a card of that name exists and is NOT usable as "which deck did this come
    from" -- which is why the per-age take counts below are keyed off the
    journal's own age column (the age the GAME was in when the card was taken),
    not off this table.  `kind` is unambiguous and is what the tier
    reconstruction uses.
    """
    out = {}
    for fn, kinds in (("cards_civil.json", None),
                      ("cards_wonders_leaders.json", None),
                      ("cards_military_actions.json", None)):
        with open(os.path.join(DATA, fn)) as fh:
            db = json.load(fh)
        for c in db["cards"]:
            kind = c.get("type") or c.get("deck") or "?"
            if fn == "cards_military_actions.json" and c.get("deck") != "civil":
                # military deck cards and events are never taken from the row
                continue
            out.setdefault(c["name"], (c.get("age"), kind))
    return out


def load_wonder_stages():
    with open(os.path.join(DATA, "cards_wonders_leaders.json")) as fh:
        db = json.load(fh)
    return {c["name"]: len(c["stages"]) for c in db["cards"]
            if c.get("type") == "wonder"}


CARDS = load_cards()
WONDER_STAGES = load_wonder_stages()
LEADERS = {n for n, (_a, k) in CARDS.items() if k == "leader"}
#: every spelling BGO may print for a leader, ours plus its own variants
_ELECTABLE = sorted(LEADERS | {k for k, v in NAME_FIXES.items() if v in LEADERS})
WONDERS = set(WONDER_STAGES)

_COL = "|".join(COLOURS)
RE_TAKE = re.compile(r"^(%s) takes (.+?) in hand(?: \1 uses (\d+) civil action)?"
                     % _COL)
# Hammurabi may pay part or all of a take with a military action, which BGO logs
# as a separate `uses N military action` clause on the same line.  The take's
# real row-slot cost is civil + military, so both are read.
RE_TAKE_MIL = re.compile(r" uses (\d+) military action")
RE_PUTBACK = re.compile(
    r"^(%s) puts (.+?) back in the row \1 gets (\d+) civil action" % _COL)
RE_DECLARE = re.compile(r"^(%s) declares (War over \w+) on (%s) " % (_COL, _COL))
RE_WINWAR = re.compile(
    r"^(%s) wins (War over \w+) Attacker's strength: (\d+); "
    r"Defender's strength: (\d+)" % _COL)
RE_AGGR = re.compile(r"^(%s) plays ([A-Za-z' -]+?) against (%s) " % (_COL, _COL))
# NOT anchored: 2809 of the corpus's 18307 stage lines are nested inside
# `<P> plays Engineering Genius <P> builds 1 stage of X; ...`, and anchoring
# this at the start of the line loses every free stage -- a 40% undercount of
# completed wonders, which is one of the headline comparisons.  The wonder
# name runs to the next `;` or to the next colour word (the `<P> spends ...`
# clause), whichever comes first.
RE_STAGE = re.compile(
    r"(%s) builds (\d+) stages? of ([A-Za-z'. ]+?)(?=;|\s(?:%s)\s|$)"
    % (_COL, _COL))
#: BGO prints this marker in the same entry as the stage that finished the
#: wonder.  It is authoritative and is used instead of counting stages against
#: `data/cards_wonders_leaders.json`, which cannot see stages this parser
#: failed to match and cannot see a wonder finished by an event.
WONDER_DONE = "Wonder completed"
RE_REVOLUTION = re.compile(
    r"^(%s) revolutions (?:using \w[\w ]*? )?Change government to ([A-Za-z ]+?);" % _COL)
# `discovers X`, `discovers X using Breakthrough`, either followed by the
# science cost or (rarely) by nothing.  The tech name is taken up to the first
# ` using ` / ` <colour> loses` / end, so "Constitutional Monarchy using
# Breakthrough" resolves to the government, not to a name that matches nothing.
RE_DISCOVER = re.compile(
    r"^(%s) discovers ([A-Za-z' ]+?)(?: using | \1 loses|$)" % _COL)
# `elects X`, optionally followed by `<previous leader> dies; ...`.  Both
# names are unpunctuated word sequences, so no regex can split them -- the
# match takes the whole remainder and `_leader_prefix` finds the longest known
# leader name it starts with.
RE_ELECT = re.compile(r"^(%s) elects (.+)$" % _COL)
RE_ENDTURN = re.compile(r"^End turn (%s) scores:" % _COL)
RE_SCI = re.compile(r"(-?\d+) science \(now (-?\d+)\)")
RE_CUL = re.compile(r"(-?\d+) culture \(now (-?\d+)\)")
RE_WINNER = re.compile(r"WINNER IS .+? AS (%s) \((\d+) PTS\)" % _COL.upper(), re.I)
RE_PLACE = re.compile(r"(\d+)(?:nd|rd|th) is .+? as (%s) \((\d+) pts\)" % _COL, re.I)
RE_COLONIZE = re.compile(r"^(%s) colonizes a " % _COL)
RE_BID = re.compile(r"^(%s) bids (\d+)" % _COL)
RE_PACT = re.compile(r"^(%s) (?:offers|signs) " % _COL)


def norm(name):
    name = name.strip()
    return NAME_FIXES.get(name, name)


def _leader_prefix(rest):
    """Longest known leader name that `rest` starts with, or None.

    `Orange elects Leonardo Da Vinci Hammurabi dies; ...` contains two leader
    names with nothing between them; the elected one is the prefix.  Longest
    wins so "Leonardo Da Vinci" is not truncated to a shorter match.
    """
    best = None
    for raw in _ELECTABLE:
        if rest.startswith(raw) and (best is None or len(raw) > len(best)):
            best = raw
    return norm(best) if best else None


FIELDS = [
    "game_id", "players", "level", "rounds", "final_age",
    "colour", "score", "rank", "margin_vs_next", "won",
    "sci_final", "cul_journal_final",
    "wars_declared", "wars_declared_won", "wars_declared_lost",
    "wars_defended", "wars_defended_won",
    "war_str_att_mean", "war_str_def_mean",
    "aggressions", "aggr_targets_distinct",
    "colonies", "bids",
    "wonders_started", "wonders_completed", "wonder_stages",
    "gov_path", "gov_changes", "first_gov", "first_gov_round",
    "takes", "takes_nonwonder", "take_ca1", "take_ca2", "take_ca3",
    "tier1", "tier2", "tier3", "tier_unknown",
    "takes_free", "takebacks",
    "take_ageA", "take_ageI", "take_ageII", "take_ageIII", "take_ageIV",
    "take_unknown_card",
    "leaders_elected",
]


def parse_game(gid, rows, meta):
    """rows: list of (date, colour, age, round, text)."""
    seats = set()
    take_events = []            # (colour, card, cost, round, age)
    pending = defaultdict(list)  # (colour, card) -> indices into take_events
    takebacks = Counter()
    wars_dec = Counter()
    wars_dec_won = Counter()
    wars_def = Counter()
    wars_def_won = Counter()
    war_str_att = defaultdict(list)
    war_str_def = defaultdict(list)
    pending_wars = []           # [(attacker, defender, kind)], oldest first
    aggr = Counter()
    aggr_targets = defaultdict(set)
    colonies = Counter()
    bids = Counter()
    stages = defaultdict(Counter)   # colour -> wonder -> stages built
    unknown_wonder = Counter()      # names RE_STAGE matched but data doesn't know
    completed = defaultdict(set)    # colour -> completed wonders
    completed_at_take = {}
    gov_path = defaultdict(list)    # colour -> [(round, gov)]
    leaders = Counter()
    cur_leader = {}
    sci_final = {}
    cul_final = {}
    last_round = 0
    unresolved_wins = 0
    final_age = ""
    scores = {}
    order = []

    for (_date, colour, age, rnd, text) in rows:
        try:
            rnd = int(rnd)
        except (TypeError, ValueError):
            rnd = last_round
        last_round = max(last_round, rnd)
        if age:
            final_age = age
        if colour in COLOURS:
            seats.add(colour)

        m = RE_ENDTURN.match(text)
        if m:
            c = m.group(1)
            seats.add(c)
            ms = RE_SCI.search(text)
            if ms:
                sci_final[c] = int(ms.group(2))
            mc = RE_CUL.search(text)
            if mc:
                cul_final[c] = int(mc.group(2))
            continue

        m = RE_PUTBACK.match(text)
        if m:
            c, card = m.group(1), norm(m.group(2))
            key = (c, card)
            if pending[key]:
                idx = pending[key].pop()
                take_events[idx] = None
                takebacks[c] += 1
            continue

        m = RE_TAKE.match(text)
        if m:
            c, card, cost = m.group(1), norm(m.group(2)), m.group(3)
            seats.add(c)
            paid = int(cost) if cost else 0
            mm = RE_TAKE_MIL.search(text)
            if mm:
                paid += int(mm.group(1))
            take_events.append(
                (c, card, paid or None, rnd, age, len(completed[c])))
            pending[(c, card)].append(len(take_events) - 1)
            continue

        m = RE_STAGE.search(text)
        if m:
            c, n, w = m.group(1), int(m.group(2)), norm(m.group(3))
            if w in WONDER_STAGES:
                stages[c][w] += n
                if text.count(WONDER_DONE):
                    completed[c].add(w)
            else:
                unknown_wonder[w] += 1
            continue

        m = RE_DECLARE.match(text)
        if m:
            a, kind, d = m.group(1), m.group(2), m.group(3)
            seats.add(a)
            seats.add(d)
            wars_dec[a] += 1
            wars_def[d] += 1
            pending_wars.append((a, d, kind))
            continue

        m = RE_WINWAR.match(text)
        if m:
            w, kind, sa, sd = m.group(1), m.group(2), int(m.group(3)), int(m.group(4))
            # the resolution line names only the winner and the war type, so
            # match it to the oldest outstanding war of that type that the
            # winner is a party to (wars resolve FIFO, one per declarer turn).
            hit = None
            for i, (a, d, k) in enumerate(pending_wars):
                if k == kind and w in (a, d):
                    hit = i
                    break
            if hit is None:
                unresolved_wins += 1
                continue
            a, d, _ = pending_wars.pop(hit)
            war_str_att[a].append(sa)
            war_str_def[d].append(sd)
            if w == a:
                wars_dec_won[a] += 1
            else:
                wars_def_won[d] += 1
            continue

        m = RE_AGGR.match(text)
        if m:
            c, card, tgt = m.group(1), norm(m.group(2)), m.group(3)
            seats.add(c)
            seats.add(tgt)
            aggr[c] += 1
            aggr_targets[c].add(tgt)
            continue

        m = RE_COLONIZE.match(text)
        if m:
            colonies[m.group(1)] += 1
            continue

        m = RE_BID.match(text)
        if m:
            bids[m.group(1)] += 1
            continue

        m = RE_REVOLUTION.match(text)
        if m:
            gov_path[m.group(1)].append((rnd, m.group(2).strip()))
            continue

        m = RE_DISCOVER.match(text)
        if m:
            c, tech = m.group(1), norm(m.group(2))
            if tech in GOVERNMENTS:
                gov_path[c].append((rnd, tech))
            continue

        m = RE_ELECT.match(text)
        if m:
            c = m.group(1)
            who = _leader_prefix(m.group(2))
            if who:
                leaders[c] += 1
                cur_leader[c] = who
            continue

        if text.startswith("End of game") or "WINNER IS" in text:
            mw = RE_WINNER.search(text)
            if mw:
                scores[mw.group(1).title()] = int(mw.group(2))
                order.append(mw.group(1).title())
            for mp in RE_PLACE.finditer(text):
                scores[mp.group(2).title()] = int(mp.group(3))
                order.append(mp.group(2).title())

    # ---- resolve takes into row-slot tiers -------------------------------
    per = defaultdict(lambda: Counter())
    for ev in take_events:
        if ev is None:
            continue
        c, card, cost, rnd, age, wonders_then = ev
        per[c]["takes"] += 1
        cage, ckind = CARDS.get(card, (None, None))
        if cage is None:
            per[c]["take_unknown_card"] += 1
        if age in ("A", "I", "II", "III", "IV"):
            per[c]["take_age" + age] += 1
        if cost is None:
            per[c]["takes_free"] += 1
            continue
        if cost <= 3:
            per[c]["take_ca%d" % cost] += 1
        if ckind == "wonder":
            # undo the +1-per-completed-wonder surcharge (engine/actions.py)
            tier = cost if cur_leader.get(c) == "Michelangelo" \
                else cost - wonders_then
        else:
            per[c]["takes_nonwonder"] += 1
            tier = cost + (1 if (ckind == "leader"
                                 and cur_leader.get(c) == "Hammurabi") else 0)
        if 1 <= tier <= 3:
            per[c]["tier%d" % tier] += 1
        else:
            per[c]["tier_unknown"] += 1

    # ---- assemble rows ---------------------------------------------------
    if scores:
        seats |= set(scores)
    ranked = sorted(scores.items(), key=lambda kv: -kv[1])
    rank_of = {c: i + 1 for i, (c, _s) in enumerate(ranked)}

    out = []
    for c in sorted(seats):
        s = scores.get(c)
        rank = rank_of.get(c)
        margin = ""
        if rank == 1 and len(ranked) > 1:
            margin = ranked[0][1] - ranked[1][1]
        elif rank and rank > 1:
            margin = ranked[rank - 1][1] - ranked[0][1]
        path = gov_path.get(c, [])
        nondesp = [(r, g) for (r, g) in path if g != "Despotism"]
        row = {
            "game_id": gid,
            "players": meta.get("players", len(seats)),
            "level": meta.get("level", ""),
            "rounds": last_round,
            "final_age": final_age,
            "colour": c,
            "score": s if s is not None else "",
            "rank": rank or "",
            "margin_vs_next": margin,
            "won": 1 if rank == 1 else 0,
            "sci_final": sci_final.get(c, ""),
            "cul_journal_final": cul_final.get(c, ""),
            "wars_declared": wars_dec[c],
            "wars_declared_won": wars_dec_won[c],
            "wars_declared_lost": wars_dec[c] - wars_dec_won[c],
            "wars_defended": wars_def[c],
            "wars_defended_won": wars_def_won[c],
            "war_str_att_mean": round(sum(war_str_att[c]) / len(war_str_att[c]), 2)
                                if war_str_att[c] else "",
            "war_str_def_mean": round(sum(war_str_def[c]) / len(war_str_def[c]), 2)
                                if war_str_def[c] else "",
            "aggressions": aggr[c],
            "aggr_targets_distinct": len(aggr_targets[c]),
            "colonies": colonies[c],
            "bids": bids[c],
            "wonders_started": len(stages[c]),
            "wonders_completed": len(completed[c]),
            "wonder_stages": sum(stages[c].values()),
            "gov_path": ">".join(g for _r, g in path),
            "gov_changes": len(nondesp),
            "first_gov": nondesp[0][1] if nondesp else "",
            "first_gov_round": nondesp[0][0] if nondesp else "",
            "takebacks": takebacks[c],
            "leaders_elected": leaders[c],
        }
        for k in ("takes", "takes_nonwonder", "take_ca1", "take_ca2", "take_ca3",
                  "tier1", "tier2", "tier3", "tier_unknown", "takes_free",
                  "take_ageA", "take_ageI", "take_ageII", "take_ageIII",
                  "take_ageIV", "take_unknown_card"):
            row[k] = per[c][k]
        out.append(row)
    return out


def read_journal(path):
    rows = []
    with open(path, encoding="utf-8", errors="replace") as fh:
        header = fh.readline()
        if not header.startswith("date"):
            fh.seek(0)
        for line in fh:
            parts = line.rstrip("\n").split("\t")
            if len(parts) < 5:
                continue
            rows.append((parts[0], parts[1], parts[2], parts[3],
                         "\t".join(parts[4:])))
    return rows


def read_index(path):
    meta = {}
    with open(path, encoding="utf-8", errors="replace") as fh:
        head = fh.readline().rstrip("\n").split("\t")
        for line in fh:
            parts = line.rstrip("\n").split("\t")
            if len(parts) < len(head):
                continue
            d = dict(zip(head, parts))
            try:
                d["players"] = int(d["players"])
            except (KeyError, ValueError):
                pass
            meta[d["game_id"]] = d
    return meta


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--journals", required=True)
    ap.add_argument("--index", default="")
    ap.add_argument("--out", default="-")
    a = ap.parse_args(argv)

    meta = read_index(a.index) if a.index else {}
    files = sorted(f for f in os.listdir(a.journals) if f.endswith(".tsv"))
    out_rows = []
    bad = []
    for f in files:
        gid = f[:-4]
        try:
            rows = read_journal(os.path.join(a.journals, f))
            out_rows.extend(parse_game(gid, rows, meta.get(gid, {})))
        except Exception as exc:                          # noqa: BLE001
            bad.append((gid, repr(exc)))

    fh = sys.stdout if a.out == "-" else open(a.out, "w")
    fh.write("\t".join(FIELDS) + "\n")
    for r in out_rows:
        fh.write("\t".join(str(r.get(k, "")) for k in FIELDS) + "\n")
    if fh is not sys.stdout:
        fh.close()
    sys.stderr.write("games=%d rows=%d failed=%d\n"
                     % (len(files) - len(bad), len(out_rows), len(bad)))
    for gid, exc in bad[:10]:
        sys.stderr.write("  FAIL %s %s\n" % (gid, exc))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
