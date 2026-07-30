#!/usr/bin/env python3
"""Sample ~2,000 finished, 2015-edition Through the Ages journals from BGO.

Extends tools/scrape_bgo.py (imported, not duplicated) with the sampling,
per-game edition/expansion verification, and checkpointing this corpus pull
needs. See docs/BGO_CORPUS.md for the full method and results.

Pipeline per candidate game, cheapest checks first (all free, from the
already-fetched metadata row, before any extra network request):
  1. player count in {2,3,4}
  2. metadata row's own edition text says "...New Story of Civilization"
  3. level is in the accepted (high-skill) set
  4. `results` lists one score per declared player (a resignation/timeout
     commonly leaves a player with no final score in this column)
Only games that pass all four get the journal fetched (index.php?cnt=52,
~4-6 pages) -- and the journal TEXT ITSELF is then the primary edition and
expansion check, not just the idJeu=10 filter parameter:
  5a. any of the 40 leader/wonder names exclusive to BGO's expansion sets
      (its own "New Leaders & Wonders", or the Czech/Polish/Spanish
      community sets on the create-game form) appearing anywhere in the
      journal text -- players interact with a leader/wonder by name
      (take/elect/build) whenever they touch it, so this is a direct,
      no-extra-request check straight from the move log -- rejects the
      game outright, no fallback needed.
  5b. one of the two leader renames the 2015 edition made (Rock'n'Roll
      Icon->Charlie Chaplin, Alex Randolph->Sid Meier) appearing in the
      journal text settles 2015-vs-2006 directly, whichever way it goes.
  5c. Only when the journal alone gives NEITHER signal (common -- these are
      a handful of the dozens of cards in play, so most games don't happen
      to reference one) do we fall back to ONE extra fetch of the board
      view (index.php?cnt=202), whose "Leaders and wonders" section lists
      the exact card pool for that specific game, and check it for the
      same expansion names plus the Monarchy/Napoleonic Army/Mechanized
      Army numeric values that changed between editions (e.g. Monarchy
      2(8) in 2015 vs 3(9) in 2006). If even that is inconclusive, the
      game is dropped as edition-unconfirmed rather than assumed.

Checkpointing: every accepted game is written to sources/bgo/journals/<id>.tsv
and appended to sources/bgo/index.tsv immediately; sources/bgo/state.json
records done ids, scanned metadata pages, and running counts, flushed after
every game. A crash or Ctrl-C loses at most the one in-flight fetch.

Politeness: single Session (one login, one cookie jar, reused for every
request), a hard minimum delay between every single fetch, and an immediate
abort (no retries) on repeated errors or any sign of being blocked.
"""
from __future__ import annotations

import argparse
import csv
import html
import json
import os
import pathlib
import random
import re
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import scrape_bgo as B  # noqa: E402

ROOT = pathlib.Path(__file__).resolve().parents[1]
OUT_DIR = ROOT / "sources" / "bgo"
JOURNAL_DIR = OUT_DIR / "journals"
INDEX_PATH = OUT_DIR / "index.tsv"
SKIP_PATH = OUT_DIR / "skips.tsv"
STATE_PATH = OUT_DIR / "state.json"

INDEX_HEADER = [
    "game_id", "game_name", "players", "level", "start_date", "end_date",
    "final_age", "rounds", "top_score", "results", "edition_verified_by",
]

# Every title actually observed on the live index, in the order this run
# treats as low->high skill. BGO publishes no documented rank scale; this
# ordering is inferred (Emperor = unambiguous top title; Prince = the
# pilot's own low-end anchor) and recorded here, not assumed silently.
# See docs/BGO_CORPUS.md for the reasoning and the caveat.
LEVEL_ORDER = ["Prince", "Warlord", "King", "Emperor"]
# NOT used as a filter (see docs/BGO_CORPUS.md "skill filter dropped"):
# an initial Emperor/King-only cut rejected ~90% of candidates (Warlord and
# Prince together dominate the pool) and made the 2,000-game target
# unreachable in any reasonable time. `level` is recorded on every accepted
# game instead, so skill can be weighted or filtered post-hoc from data that
# actually exists on disk, rather than deciding the cut before fetching.
ACCEPTED_LEVELS = {"Emperor", "King", "Warlord", "Prince"}

EXPECTED_EDITION_TEXT = "Through the Ages: A New Story of Civilization"

# The 40 leader/wonder names exclusive to BGO's expansion sets (its own
# "New Leaders & Wonders" -- credited on-site to Nicolas d'Halluin -- plus
# whatever the Czech/Polish/Spanish "expansion sets" on the create-game form
# add; the index has no way to tell these apart, so all are treated as
# "not base 2015" alike). Derived as:
#   (every Leaders_*/Wonders_* "(NWL)" deck in
#    sources/tts_tta_workshop_2120085710.json)
#   MINUS (the 24 leaders + 16 wonders in data/cards_wonders_leaders.json)
# "Ocean Liner Service" was removed by hand from the wonders diff: it is
# just the TTS mod's name for the base "Ocean Liners" (BGO's own board page
# renders it as singular "Ocean Liner"), not a distinct card.
NLW_LEADERS = [
    "Alfred Nobel", "Antoni Gaudi", "Ashoka", "Boudica", "Catherine the Great",
    "Charles Darwin", "Cleopatra", "Confucius", "Eleanor of Aquitaine",
    "Hippocrates", "Ian Fleming", "Isabella of Castile", "James Watt",
    "Jan Zizka", "Johannes Gutenberg", "Maria Theresa",
    "Marie Curie Sklodowska", "Marlene Dietrich", "Nelson Mandela",
    "Nostradamus", "Pierre de Coubertin", "Saladin", "Steve Jobs", "Sun Tzu",
]
NLW_WONDERS = [
    "Acropolis", "Colosseum", "Empire State Building", "Forbidden City",
    "Harvard College", "Himeji Castle", "International Red Cross",
    "Louvre Museum", "Machu Picchu", "Manhattan Project", "Roman Roads",
    "Silk Road", "Statue of Liberty", "Stonehenge", "Suez Canal",
    "United Nations",
]
NLW_NAMES = NLW_LEADERS + NLW_WONDERS

# The two leaders the 2015 edition renamed (docs/SOURCES.md "Leaders
# renamed/changed"). Either name settles 2015-vs-2006 directly, from plain
# journal text, whichever way it points.
RENAME_2015 = ["Charlie Chaplin", "Sid Meier"]
RENAME_2006 = ["Rock'n'Roll Icon", "Alex Randolph"]

# Numeric board-view fallback signatures (docs/BGO_CORPUS.md / EXTERNAL_AIS.md
# 5a): government/tactics cards whose stat values changed between editions.
# Only consulted when the journal text alone is inconclusive. Values are
# regexes, not literal strings: stripping HTML tags leaves inconsistent
# whitespace/newlines between a card's number and its "(N)" part (verified
# against a live board fetch -- e.g. "Napoleonic Army" is followed by
# "7\n(4)\n", not "7(4)"), so every join is `\s*`.
BOARD_SIGNATURES_2015 = [
    ("Monarchy", r"2\s*\(\s*8\s*\)"),
    ("Napoleonic Army", r"7\s*\(\s*4\s*\)"),
    ("Mechanized Army", r"10\s*\(\s*5\s*\)"),
]
BOARD_SIGNATURES_2006 = [
    ("Monarchy", r"3\s*\(\s*9\s*\)"),
    ("Napoleonic Army", r"8\s*\(\s*4\s*\)"),
]

BLOCK_MARKERS = [
    "captcha", "too many requests", "rate limit", "temporarily blocked",
    "access denied", "please verify you are human",
]

DEFAULT_QUOTAS = {"2": 900, "3": 550, "4": 550}


def load_state() -> dict:
    if STATE_PATH.exists():
        return json.loads(STATE_PATH.read_text())
    return {"done_ids": [], "scanned_pages": [], "counts": {"2": 0, "3": 0, "4": 0},
            "skip_counts": {}}


def save_state(state: dict) -> None:
    tmp = STATE_PATH.with_suffix(".json.tmp")
    tmp.write_text(json.dumps(state))
    tmp.replace(STATE_PATH)


def append_tsv(path: pathlib.Path, header: list[str], row: list) -> None:
    is_new = not path.exists()
    with open(path, "a", newline="") as fh:
        w = csv.writer(fh, delimiter="\t")
        if is_new:
            w.writerow(header)
        w.writerow(row)


def log_skip(state: dict, game_id, reason: str, detail: str = "") -> None:
    append_tsv(SKIP_PATH, ["game_id", "reason", "detail"], [game_id, reason, detail])
    state["skip_counts"][reason] = state["skip_counts"].get(reason, 0) + 1


def is_completed(players: str, results: str) -> bool:
    """Heuristic: a resignation/timeout typically leaves the row missing a
    score for whichever player left, so len(results) != declared player
    count. Not perfect (see docs/BGO_CORPUS.md) but cheap and index-only."""
    if not results:
        return False
    try:
        return len(results.split("|")) == int(players)
    except ValueError:
        return False


def is_multi_player(players: str, results: str) -> bool:
    """False for "solitaire" entries where one account plays every seat
    (BGO allows this, e.g. game names like "ES Game (2) solitaire (1836)").
    Same player controlling every side is degenerate for a value function
    trained on adversarial play, even though the rules and scoring are
    unaffected -- so these are dropped, not just the resigned ones."""
    if not results:
        return False
    names = [seg.rsplit(":", 1)[0] for seg in results.split("|") if ":" in seg]
    try:
        return len(set(names)) == int(players) == len(names)
    except ValueError:
        return False


def check_not_blocked(body: str) -> None:
    low = body.lower()
    for marker in BLOCK_MARKERS:
        if marker in low:
            raise RuntimeError(f"possible block/captcha detected (marker={marker!r})")


def _near(text: str, name: str, value_re: str, window: int = 200) -> bool:
    """True if `value_re` (a regex) matches within `window` chars after some
    occurrence of the literal `name`."""
    start = 0
    pat = re.compile(value_re)
    while True:
        i = text.find(name, start)
        if i == -1:
            return False
        if pat.search(text[i + len(name):i + len(name) + window]):
            return True
        start = i + 1


def _plain_text(html_body: str) -> str:
    return html.unescape(re.sub(r"<[^>]+>", "\n", html_body))


def verify_from_journal(jrows: list) -> tuple[bool | None, str]:
    """First-choice check: scan the journal text itself, no extra fetch.

    Returns (True, method) if confirmed 2015, (False, reason) if positively
    rejected (expansion cards seen, or a 2006-only rename seen), or
    (None, "") if the journal alone is inconclusive and the board-view
    fallback should be tried.
    """
    text = "\n".join(row[4] for row in jrows if len(row) > 4)
    hits = [n for n in NLW_NAMES if n in text]
    if hits:
        return False, "expansion-cards-in-journal:" + ",".join(hits[:4])
    if any(n in text for n in RENAME_2006):
        return False, "2006-leader-name-in-journal"
    if any(n in text for n in RENAME_2015):
        return True, "journal:2015-leader-name"
    return None, ""


def verify_from_board(board_html: str) -> tuple[bool, str]:
    """Fallback check: one extra fetch, only used when the journal text
    alone didn't settle it."""
    check_not_blocked(board_html)
    hits = [n for n in NLW_NAMES if n in board_html]
    if hits:
        return False, "expansion-cards-on-board:" + ",".join(hits[:4])
    text = _plain_text(board_html)
    if any(_near(text, name, value_re) for name, value_re in BOARD_SIGNATURES_2006):
        return False, "2006-numeric-signature-on-board"
    if any(_near(text, name, value_re) for name, value_re in BOARD_SIGNATURES_2015):
        return True, "board:2015-numeric-signature"
    return False, "inconclusive-no-signature-found"


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__,
                                  formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--user", default="ptarjan")
    ap.add_argument("--pw-file", default="/Users/pt/tmp/bgo")
    ap.add_argument("--jar", default="/tmp/bgo_corpus_cookies.txt")
    ap.add_argument("--delay", type=float, default=2.75)
    ap.add_argument("--target", type=int, default=2000)
    ap.add_argument("--page-stride", type=int, default=8,
                     help="scan every Nth metadata page across the full range")
    ap.add_argument("--max-index-page", type=int, default=3566)
    ap.add_argument("--quota-2p", type=int, default=DEFAULT_QUOTAS["2"])
    ap.add_argument("--quota-3p", type=int, default=DEFAULT_QUOTAS["3"])
    ap.add_argument("--quota-4p", type=int, default=DEFAULT_QUOTAS["4"])
    ap.add_argument("--max-consecutive-errors", type=int, default=3)
    ap.add_argument("--seed", type=int, default=20260726,
                     help="shuffle seed for page (and in-page row) order, so "
                          "every prefix of the run is a valid stratified "
                          "sample instead of newest-first/id-clustered")
    args = ap.parse_args()

    JOURNAL_DIR.mkdir(parents=True, exist_ok=True)

    quotas = {"2": args.quota_2p, "3": args.quota_3p, "4": args.quota_4p}

    state = load_state()
    done_ids = set(state["done_ids"])
    counts = state["counts"]
    scanned = set(state["scanned_pages"])

    state["shuffle_seed"] = args.seed
    save_state(state)
    print(f"resuming: {len(done_ids)} games already done, counts={counts}, "
          f"{len(scanned)} pages already scanned, shuffle_seed={args.seed}",
          file=sys.stderr)

    if pathlib.Path(args.jar).resolve().is_relative_to(ROOT):
        sys.exit("refusing to write the cookie jar inside the repo")

    sess = B.Session(args.jar, args.delay)
    sess.login(args.user, args.pw_file)
    print("login OK", file=sys.stderr)

    # Shuffled, not sequential: scanning pages 1,6,11,... in ascending order
    # means page 1 (the newest games) gets fully worked before page 6 is
    # even looked at, so any prefix of the run -- and any run that stops
    # early -- would be "the most recent N games", densely clustered in
    # time and id space (and likely in player pool). Shuffling the page
    # visit order with a fixed, recorded seed makes every prefix of the run
    # a valid stratified sample across the full id range instead.
    rng = random.Random(args.seed)
    pages = [p for p in range(1, args.max_index_page + 1, args.page_stride)
             if p not in scanned]
    rng.shuffle(pages)
    consecutive_errors = 0

    def total_done():
        return sum(counts.values())

    for pg in pages:
        if total_done() >= args.target:
            break
        if all(counts[p] >= quotas[p] for p in quotas):
            break
        try:
            rows = B.scrape_metadata(sess, "2015", [pg], cap=5)
            consecutive_errors = 0
        except Exception as exc:  # noqa: BLE001
            consecutive_errors += 1
            print(f"metadata page {pg} fetch failed: {exc}", file=sys.stderr)
            if consecutive_errors >= args.max_consecutive_errors:
                print("STOPPING: repeated fetch errors, looks like the "
                      "server is pushing back.", file=sys.stderr)
                break
            continue
        scanned.add(pg)
        state["scanned_pages"] = sorted(scanned)
        save_state(state)
        rng.shuffle(rows)  # a page's 50 games are near-consecutive ids

        for r in rows:
            if total_done() >= args.target:
                break
            (_page, gid, name, edition, players, level, start, end,
             final_age, rounds, top_score, results) = r
            gid = str(gid)
            if gid in done_ids:
                continue
            if players not in quotas:
                log_skip(state, gid, "player-count-out-of-range", players)
                continue
            if counts[players] >= quotas[players]:
                continue  # bucket already full; not a "skip", just not needed
            if edition != EXPECTED_EDITION_TEXT:
                log_skip(state, gid, "edition-index-mismatch", edition)
                continue
            if level not in ACCEPTED_LEVELS:
                log_skip(state, gid, "skill-level-below-cutoff", level)
                continue
            if not is_completed(players, results):
                log_skip(state, gid, "incomplete-or-resigned",
                          f"players={players} results={results!r}")
                continue
            if not is_multi_player(players, results):
                log_skip(state, gid, "solitaire-single-account",
                          f"players={players} results={results!r}")
                continue

            try:
                jrows = B.scrape_journal(sess, int(gid))
                consecutive_errors = 0
            except Exception as exc:  # noqa: BLE001
                consecutive_errors += 1
                print(f"game {gid} journal fetch failed: {exc}", file=sys.stderr)
                if consecutive_errors >= args.max_consecutive_errors:
                    print("STOPPING: repeated fetch errors on journal fetch.",
                          file=sys.stderr)
                    save_state(state)
                    return
                log_skip(state, gid, "fetch-error-journal", str(exc))
                continue
            if not jrows:
                log_skip(state, gid, "empty-journal", "")
                continue

            verified, reason = verify_from_journal(jrows)
            if verified is False:
                log_skip(state, gid, "edition-or-expansion-rejected-journal", reason)
                continue
            if verified is None:
                # Journal text alone didn't settle it -- one extra fetch.
                try:
                    board_html = B.fetch_board(sess, gid)
                    verified, reason = verify_from_board(board_html)
                    consecutive_errors = 0
                except Exception as exc:  # noqa: BLE001
                    consecutive_errors += 1
                    print(f"game {gid} board fetch failed: {exc}", file=sys.stderr)
                    if consecutive_errors >= args.max_consecutive_errors:
                        print("STOPPING: repeated fetch errors on board fetch.",
                              file=sys.stderr)
                        save_state(state)
                        return
                    log_skip(state, gid, "fetch-error-board", str(exc))
                    continue
                if not verified:
                    log_skip(state, gid, "edition-unconfirmed-or-expansion", reason)
                    continue

            jpath = JOURNAL_DIR / f"{gid}.tsv"
            with open(jpath, "w", newline="") as fh:
                w = csv.writer(fh, delimiter="\t")
                w.writerow(["date", "player_colour", "age", "round", "text"])
                w.writerows(jrows)

            append_tsv(INDEX_PATH, INDEX_HEADER,
                       [gid, name, players, level, start, end, final_age,
                        rounds, top_score, results, reason])
            done_ids.add(gid)
            counts[players] = counts.get(players, 0) + 1
            state["done_ids"] = sorted(done_ids)
            state["counts"] = counts
            save_state(state)
            print(f"[{total_done()}/{args.target}] game {gid} OK "
                  f"({players}p {level}, {len(jrows)} journal rows)",
                  flush=True)

    print(f"FINISHED. counts={counts} skip_counts={state['skip_counts']}",
          file=sys.stderr)


if __name__ == "__main__":
    main()
