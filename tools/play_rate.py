"""Per-CARD play-rate census: not "is this card priced", but "is it played".

`tools/card_census.py`, `tools/card_blindness.py` and `tools/uncovered_census.py`
all answer *is this card PRICED by the evaluator* -- `docs/CARD_CENSUS.md` says
so in its own words: "the suite checks that a card is priced, never that its
price is read".  `unit_strength_credit` is what that gap costs: the ten military
unit cards were given a feature, the feature shipped, the weight sat at 0.0 on
every trained vector, and the bot went on fighting Age A Warriors for days with
every card-pricing test green.

This tool measures the other thing.  For all 236 cards it reports **how often a
seat takes or plays that card per game**, and puts the human rate from
`sources/bgo/journals.tar.gz` (1,011 BGO games) beside it.

    # human side, straight from the committed journal tarball (~30s, no engine)
    python3 tools/play_rate.py human --out /tmp/human_cards.json

    # bot side, one blob per player count
    nice -n 15 python3 tools/play_rate.py bot --players 2 --games 40 \
        --spec plan:experiments/league_state/champion_2p.json,width=2,det=1 \
        --out /tmp/cards_2p.json

    # the ranked discrepancy table
    python3 tools/play_rate.py report --human /tmp/human_cards.json \
        /tmp/cards_2p.json /tmp/cards_3p.json /tmp/cards_4p.json

The bot half **reuses `tools/system_census.py` unchanged**: it subclasses that
module's `Rec` to add per-card buckets and substitutes the subclass before
calling `system_census.run`, so the seat wrapper, the five engine taps and the
`state is real` guard that makes them honest are the same code, not a copy.

## Two measurement contracts, and they are not interchangeable

* **TAKE** (civil deck, 127 cards): the human journal prints
  `X takes <card> in hand`, and the bot emits a `take` move.  Both sides are a
  free choice from a visible row, so the rates are directly comparable.
* **PLAY** (military deck, 109 cards -- events, aggressions, wars, tactics,
  pacts, bonuses, territories): nobody chooses to *take* these, they are drawn
  blind.  Only the decision to *use* one is comparable, so the military rows
  count plays/declarations/colonizations on both sides.

## The name join is at BASE name, and that is a real limit

BGO's journal prints `Orange takes Engineering Genius in hand` -- no age suffix
-- while the card database calls the same three cards `Engineering Genius (A)`,
`(I)` and `(III)`.  The corpus therefore cannot separate age variants of the
same card, so every rate here is joined on `baseName` and a base name that
covers k printings is one row.  The bot side is *also* reported per exact card
(`--exact`), because "which precise card is never played" needs the full name
and only the bot side can answer it.  Journal tokens that match no base name
are printed under UNMATCHED rather than dropped, so a template change shows up
as a coverage complaint instead of a shifted rate.
"""
from __future__ import annotations

import argparse
import csv
import io
import json
import os
import re
import sys
import tarfile
from collections import Counter, defaultdict

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine import cards                                        # noqa: E402

_ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
JOURNALS = os.path.join(_ROOT, "sources", "bgo", "journals.tar.gz")
INDEX = os.path.join(_ROOT, "sources", "bgo", "index.tsv")

CIVIL_TYPES = ("farm", "mine", "lab", "temple", "library", "arena", "theater",
               "infantry", "cavalry", "artillery", "air", "special-tech",
               "government", "wonder", "leader", "action")
UNIT_TYPES = ("infantry", "cavalry", "artillery", "air")

#: journal spelling -> database base name, for the five cards BGO spells its
#: own way beyond what `_norm` already absorbs (case and punctuation).  Every
#: journal token that resolves to nothing lands in UNMATCHED and is printed,
#: so a sixth spelling shows up as a coverage complaint, not a silent zero.
ALIASES = {
    "stockpile": "Stock Pile",
    "charleschaplin": "Charlie Chaplin",
    "maximillienrobespierre": "Maximilien Robespierre",
    "johannessebastianbach": "J. S. Bach",
    "oceanliner": "Ocean Liners",
    "breadcircuses": "Bread and Circuses",
}


def _norm(s):
    return re.sub(r"[^a-z0-9]", "", s.lower())


def db_index():
    """base name -> {names, type, deck, ages, dealt}, over all 236 cards.

    `dealt` is False for the six starting technologies printed on the player
    board (`count` 0 at every table size: Agriculture, Bronze, Philosophy,
    Religion, Warriors, Despotism).  Nobody can take those, so they must not
    appear in any "never taken" list on either side.
    """
    db = cards.db()
    out = {}
    for c in db.cards:
        b = c.get("baseName") or c["name"]
        e = out.setdefault(b, {"names": [], "type": c["type"],
                               "deck": c.get("deck"), "ages": [],
                               "dealt": False})
        e["names"].append(c["name"])
        e["ages"].append(c["age"])
        cnt = c.get("count")
        if isinstance(cnt, dict):
            if any((cnt.get(k) or 0) > 0 for k in ("2p", "3p", "4p")):
                e["dealt"] = True
        elif cnt:
            e["dealt"] = True
    return out


# ----------------------------------------------------------------- human side

# `X takes Foo in hand`.  A take-back is `X puts Foo back in the row` and is
# matched against the most recent unmatched take of the same card by the same
# player, exactly as tools/bgo_parse.py does it -- both lines are in the
# journal and counting the take alone overstates every rate by ~7%.
RE_TAKE = re.compile(r"^(\w+) takes (.+?) in hand\b")
RE_PUTBACK = re.compile(r"^(\w+) puts (.+?) back in the row\b")
RE_ELECT = re.compile(r"^(\w+) elects (.+?)\s*$")
RE_PLAY = re.compile(r"^(\w+) plays ([A-Z].*)$")
RE_WAR = re.compile(r"^(\w+) declares (War over \w+) on\b")
RE_COLONIZE = re.compile(r"^(\w+) colonizes an? (\w+) Territory\b")
RE_GOV = re.compile(r"^(\w+) (?:discovers|revolutions Change government to) "
                    r"(.+?)\s*(?:;|$)")
# `Orange sets up new tactics I / Fighting Band` / `adopts existing tactics`.
RE_TACTIC = re.compile(r"^(\w+) (?:sets up new|adopts existing) tactics "
                       r"\w+ / (.+?)\s*(?:;|$)")
#: `1 Defense card +4 played` inside a defence, `1 Colonization card +2`
#: inside a colonization: the only place BGO names a Military Bonus card, and
#: it names it by the number rather than by the card.  defence 2/4/6 and
#: colonization 1/2/3 are the age I/II/III printings of the same three cards
#: (`engine/cards`), so both spellings resolve to one card.
RE_DEFCARD = re.compile(r"(\d+) Defense card \+(\d+)")
RE_COLCARD = re.compile(r"(\d+) Colonization card \+(\d+)")
BONUS_BY_DEFENSE = {2: "Military Bonus (defense 2 / colonization 1)",
                    4: "Military Bonus (defense 4 / colonization 2)",
                    6: "Military Bonus (defense 6 / colonization 3)"}
BONUS_BY_COLONIZE = {1: "Military Bonus (defense 2 / colonization 1)",
                     2: "Military Bonus (defense 4 / colonization 2)",
                     3: "Military Bonus (defense 6 / colonization 3)"}


def parse_journals(path=JOURNALS, index=INDEX, verbose=False):
    """(rates, meta): base-name -> players -> events per SEAT-GAME."""
    meta = {}
    with open(index) as fh:
        for r in csv.DictReader(fh, delimiter="\t"):
            try:
                meta[r["game_id"]] = int(r["players"])
            except (TypeError, ValueError):
                pass
    idx = db_index()
    known = {_norm(b): b for b in idx}
    counts = defaultdict(Counter)          # players -> base name -> n
    seats = Counter()                      # players -> seat-games
    games = Counter()
    unmatched = Counter()

    def resolve(tok):
        n = _norm(tok)
        return ALIASES.get(n) or known.get(n)

    # BGO packs an action and its consequence into ONE journal cell --
    # `Orange plays Reserves Orange produces 4 resources` -- so a play line has
    # to be resolved by LONGEST NORMALISED PREFIX, not by equality.  The
    # candidate set is restricted to the card types that can appear after
    # `plays` / `elects` / `discovers`, so `Iron` cannot swallow a line that
    # was really about something else.
    def prefix_pool(*types):
        pool = []
        for b, e in idx.items():
            if e["type"] in types:
                pool.append((_norm(b), b))
                if b.startswith("Aggression: "):
                    pool.append((_norm(b[len("Aggression: "):]), b))
        for alias, b in ALIASES.items():
            if idx.get(b, {}).get("type") in types:
                pool.append((alias, b))
        pool.sort(key=lambda kv: -len(kv[0]))
        return pool

    PLAY_POOL = prefix_pool("action", "aggression")
    LEAD_POOL = prefix_pool("leader")
    TECH_POOL = prefix_pool("farm", "mine", "lab", "temple", "library",
                            "arena", "theater", "infantry", "cavalry",
                            "artillery", "air", "special-tech", "government")

    def resolve_prefix(tok, pool):
        n = _norm(tok)
        for key, base in pool:
            if n.startswith(key):
                return base
        return None

    tf = tarfile.open(path, "r:gz")
    for m in tf:
        if not m.isfile() or not m.name.endswith(".tsv"):
            continue
        gid = os.path.basename(m.name)[:-4]
        np_ = meta.get(gid)
        if not np_:
            continue
        games[np_] += 1
        seats[np_] += np_
        c = counts[np_]
        pending = defaultdict(list)
        fh = io.TextIOWrapper(tf.extractfile(m), encoding="utf-8",
                              errors="replace")
        next(fh, None)
        for line in fh:
            parts = line.rstrip("\n").split("\t")
            if len(parts) < 5:
                continue
            txt = parts[4]
            mt = RE_TAKE.match(txt)
            if mt:
                if mt.group(2).startswith("spoils of war"):
                    continue
                nm = resolve(mt.group(2))
                if nm:
                    c["take|" + nm] += 1
                    pending[mt.group(1)].append(nm)
                else:
                    unmatched["take:" + mt.group(2)[:40]] += 1
                continue
            mp = RE_PUTBACK.match(txt)
            if mp:
                nm = resolve(mp.group(2))
                if nm and nm in pending[mp.group(1)]:
                    c["take|" + nm] -= 1
                    pending[mp.group(1)].remove(nm)
                continue
            me = RE_ELECT.match(txt)
            if me:
                nm = resolve_prefix(me.group(2), LEAD_POOL)
                if nm:
                    c["elect|" + nm] += 1
                else:
                    unmatched["elect:" + me.group(2)[:40]] += 1
                continue
            mw = RE_WAR.match(txt)
            if mw:
                c["play|" + mw.group(2)] += 1
                continue
            for rx, table in ((RE_DEFCARD, BONUS_BY_DEFENSE),
                              (RE_COLCARD, BONUS_BY_COLONIZE)):
                for n, val in rx.findall(txt):
                    nm = table.get(int(val))
                    if nm:
                        c["play|" + nm] += int(n)
                    else:
                        unmatched["bonuscard:+" + val] += 1
            mt2 = RE_TACTIC.match(txt)
            if mt2:
                nm = resolve(mt2.group(2))
                if nm:
                    c["play|" + nm] += 1
                else:
                    unmatched["tactic:" + mt2.group(2)[:40]] += 1
                continue
            mc = RE_COLONIZE.match(txt)
            if mc:
                nm = resolve(mc.group(2) + " Territory")
                if nm:
                    c["play|" + nm] += 1
                else:
                    unmatched["colonize:" + mc.group(2)] += 1
                continue
            mg = RE_GOV.match(txt)
            if mg:
                nm = resolve_prefix(mg.group(2), TECH_POOL)
                if nm:
                    c["dev|" + nm] += 1
                    if idx[nm]["type"] == "government":
                        c["play|" + nm] += 1
                else:
                    unmatched["discover:" + mg.group(2)[:40]] += 1
                continue
            mpl = RE_PLAY.match(txt)
            if mpl:
                tok = mpl.group(2).strip()
                if tok.lower().startswith("event"):
                    continue
                nm = resolve_prefix(tok, PLAY_POOL)
                if nm:
                    c["play|" + nm] += 1
                else:
                    unmatched["play:" + tok[:40]] += 1
        fh.detach()
    tf.close()
    if verbose and unmatched:
        for k, v in sorted(unmatched.items(), key=lambda kv: -kv[1])[:40]:
            sys.stderr.write("UNMATCHED %6d  %s\n" % (v, k))
    return {"counts": {str(p): dict(c) for p, c in counts.items()},
            "seats": {str(p): n for p, n in seats.items()},
            "games": {str(p): n for p, n in games.items()},
            "unmatched": dict(unmatched)}


# ------------------------------------------------------------------- bot side

def _make_rec(base):
    """Subclass system_census.Rec with per-card take/play buckets."""

    class CardRec(base):
        def _note(self, state, mv):
            if mv:
                k = mv[0]
                if k == "take":
                    self.names["card_take"][state.card_row[mv[1]]] += 1
                elif k in ("play_action", "play_leader", "play_tactic",
                           "aggression", "war", "offer_pact", "copy_tactic",
                           "defend"):
                    if len(mv) > 1 and isinstance(mv[1], str):
                        self.names["card_play"][mv[1]] += 1
                elif k == "develop":
                    if len(mv) > 1 and isinstance(mv[1], str):
                        self.names["card_develop"][mv[1]] += 1
                elif k == "build":
                    if len(mv) > 1 and isinstance(mv[1], str):
                        self.names["card_build"][mv[1]] += 1
                elif k == "upgrade":
                    if len(mv) > 2 and isinstance(mv[2], str):
                        self.names["card_build"][mv[2]] += 1
            return base._note(self, state, mv)

    return CardRec


def run_bot(spec, players, games, seed, out):
    from experiments.arena import load_spec
    from tools import system_census as sc
    sc.Rec = _make_rec(sc.Rec)
    sc.run(load_spec(spec), players, games, seed, out)


# --------------------------------------------------------------------- report

def load_bot(paths):
    """Load blobs and MERGE the shards that share a player count.

    A census of N games is run as several `--seed`-disjoint shards so five
    cores can work at once; they are the same measurement and summing them is
    the whole point.  Merging here rather than in a separate script means the
    report cannot be handed a half-merged set.
    """
    by_players = {}
    for p in paths:
        b = json.load(open(p))
        b["names"] = {k: Counter(v) for k, v in b["names"].items()}
        prev = by_players.get(b["players"])
        if prev is None:
            b["totals"] = Counter(b["totals"])
            b["shards"] = [os.path.basename(p)]
            by_players[b["players"]] = b
        else:
            prev["totals"].update(b["totals"])
            prev["games"] += b["games"]
            prev["shards"].append(os.path.basename(p))
            for k, c in b["names"].items():
                prev["names"].setdefault(k, Counter()).update(c)
    return [by_players[k] for k in sorted(by_players)]


def fold_to_base(counter, idx_by_name):
    out = Counter()
    for nm, v in counter.items():
        out[idx_by_name.get(nm, nm)] += v
    return out


def build_table(human, blobs):
    """[(base, type, deck, mode, human_rate_by_np, bot_rate_by_np)]"""
    idx = db_index()
    by_name = {}
    for b, e in idx.items():
        for n in e["names"]:
            by_name[n] = b
    rows = []
    hc = {p: Counter(c) for p, c in human["counts"].items()}
    hs = {p: n for p, n in human["seats"].items()}
    folded = {}
    for b in blobs:
        p = str(b["players"])
        folded[p] = {k: fold_to_base(b["names"].get(k, Counter()), by_name)
                     for k in ("card_take", "card_play", "colony_held",
                               "revealed")}
    for base in sorted(idx):
        e = idx[base]
        if not e["dealt"]:
            continue                       # printed on the board, never dealt
        mode = "take" if e["deck"] == "civil" else "play"
        # A territory is never `play`ed: it is won at auction and then held.
        # `colony_held` is the bot-side equivalent of the human `colonizes a
        # X Territory` line, and using `card_play` here would report a
        # structural 0 for all twelve as if it were a finding.
        # An event is never played by anybody: it is prepared face down and
        # then REVEALED, so `revealed` is the only rate that exists for it.
        bucket = ("colony_held" if e["type"] == "territory" else
                  "revealed" if e["type"] == "event" else
                  "card_take" if mode == "take" else "card_play")
        hr, br = {}, {}
        for p in ("2", "3", "4"):
            n = hs.get(p, 0)
            # An event is revealed, not chosen, and a pact is never named in
            # the journal, so neither carries a human per-card rate at all.
            if e["type"] in ("event", "pact"):
                hr[p] = None
                continue
            hr[p] = (hc.get(p, Counter())["%s|%s" % (mode, base)] / n) if n else None
        for b in blobs:
            p = str(b["players"])
            seats = b["totals"]["seats"]
            br[p] = folded[p][bucket][base] / seats if seats else 0.0
        rows.append({"base": base, "type": e["type"], "deck": e["deck"],
                     "mode": mode, "printings": len(e["names"]),
                     "ages": sorted(set(e["ages"]), key=str),
                     "human": hr, "bot": br})
    return rows


def report(human_path, bot_paths, exact=False, top=40):
    human = json.load(open(human_path))
    blobs = load_bot(bot_paths)
    rows = build_table(human, blobs)
    counts = sorted({str(b["players"]) for b in blobs})

    def disc(r, p):
        h, b = r["human"].get(p), r["bot"].get(p)
        if h is None or b is None:
            return None
        return b - h

    print("== per-card play rate, per seat-game.  h = human BGO corpus "
          "(%s games), b = bot self-play" % human["games"])
    for b in blobs:
        print("   bot %sp: %s games, %s seat-games, shards=%s, spec=%s"
              % (b["players"], b["games"], b["totals"]["seats"],
                 len(b.get("shards", [1])), b["spec"]))
    hdr = "%-34s %-13s %4s" % ("card (base name)", "type", "mode")
    for p in counts:
        hdr += "  %6s %6s %7s" % (p + "p h", p + "p b", "delta")
    print("\n" + hdr)

    def worst(r):
        ds = [d for d in (disc(r, p) for p in counts) if d is not None]
        return min(ds) if ds else 0.0

    for r in sorted(rows, key=worst):
        line = "%-34s %-13s %4s" % (r["base"][:34], r["type"], r["mode"])
        for p in counts:
            h, b = r["human"].get(p), r["bot"].get(p)
            d = disc(r, p)
            line += "  %6s %6.3f %7s" % (
                "-" if h is None else "%.3f" % h, b or 0.0,
                "-" if d is None else "%+.3f" % d)
        print(line)

    print("\n-- CARDS THE BOT NEVER TOUCHES (0.000 at every measured count), "
          "with the human rate it is measured against")
    for r in sorted(rows, key=lambda r: -max(
            [v for v in r["human"].values() if v is not None] or [0.0])):
        if all((r["bot"].get(p) or 0.0) == 0.0 for p in counts):
            hh = "  ".join("%sp h=%.3f" % (p, r["human"][p])
                           for p in counts if r["human"].get(p) is not None)
            print("   %-34s %-13s %s" % (r["base"][:34], r["type"], hh))

    print("\n-- by card type, summed over the type's cards, per seat-game")
    print("%-14s %s" % ("type", "  ".join("%6s %6s" % (p + "p h", p + "p b")
                                          for p in counts)))
    types = sorted({r["type"] for r in rows})
    for t in types:
        sub = [r for r in rows if r["type"] == t]
        cells = []
        for p in counts:
            hv = [r["human"][p] for r in sub if r["human"].get(p) is not None]
            bv = [(r["bot"].get(p) or 0.0) for r in sub]
            cells.append("%6s %6.3f" % ("%.3f" % sum(hv) if hv else "-",
                                        sum(bv)))
        print("%-14s %s" % (t, "  ".join(cells)))

    if exact:
        print("\n-- exact-card bot counts (the human corpus prints no age "
              "suffix, so only the bot side can be exact)")
        dealt = {c["name"] for c in cards.db().cards
                 if isinstance(c.get("count"), dict)
                 and any((c["count"].get(k) or 0) > 0
                         for k in ("2p", "3p", "4p"))}
        for b in blobs:
            got = Counter()
            # every bucket that means "this seat used this card": taken from
            # the row, played from hand, won at auction, or revealed as the
            # current event.  Leaving `colony_held` and `revealed` out would
            # list all 12 territories and all 55 events as never-touched,
            # which is a bug in the reader, not a finding about the bot.
            for bucket in ("card_take", "card_play", "colony_held",
                           "revealed"):
                got.update(b["names"].get(bucket, Counter()))
            never = sorted(dealt - set(got))
            print("  %sp: %d/%d dealt cards touched.  NEVER: %s"
                  % (b["players"], len(dealt) - len(never), len(dealt),
                     ", ".join(never)))
    return 0


def main(argv=None):
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    h = sub.add_parser("human")
    h.add_argument("--out", default="-")
    h.add_argument("--verbose", action="store_true")
    b = sub.add_parser("bot")
    b.add_argument("--spec", required=True)
    b.add_argument("--players", type=int, default=2)
    b.add_argument("--games", type=int, default=40)
    b.add_argument("--seed", type=int, default=0)
    b.add_argument("--out", default="-")
    r = sub.add_parser("report")
    r.add_argument("--human", required=True)
    r.add_argument("--exact", action="store_true")
    r.add_argument("blobs", nargs="+")
    a = ap.parse_args(argv)
    if a.cmd == "human":
        blob = parse_journals(verbose=a.verbose)
        fh = sys.stdout if a.out == "-" else open(a.out, "w")
        json.dump(blob, fh, indent=1, sort_keys=True)
        if fh is not sys.stdout:
            fh.close()
        return 0
    if a.cmd == "bot":
        run_bot(a.spec, a.players, a.games, a.seed, a.out)
        return 0
    return report(a.human, a.blobs, a.exact)


if __name__ == "__main__":
    raise SystemExit(main())
