#!/usr/bin/env python3
"""Polite scraper for Boardgaming-Online (https://www.boardgaming-online.com).

BGO hosts the two Through the Ages editions as two separate "boardgames":

    idJeu=4   Through the Ages                                  (2006 original)
    idJeu=10  Through the Ages: A New Story of Civilization     (2015 - ours)

The finished-games filter form defaults to ``idJeu=4``, which is why a naive
scrape sees nothing but 2006 games.  Always pass ``--edition 2015``.

Two products, deliberately separate (see docs/EXTERNAL_AIS.md 5a and 7):

  metadata   the finished-games index: game id, name, edition, player count,
             level, start/end dates, final age, rounds, and each player's
             final score.  50 games per page, ~3566 pages for the 2015
             edition (~178k games).  This is the recommended one.

  journal    one game's full move-by-move log, 100 entries per page, ~4-6
             pages per game.  Card identities are named for every public
             action; military draws/discards are counts only; the civil card
             row is never logged, so the choice set behind a decision cannot
             be recovered from this alone.

Credentials are read from a file at runtime and never printed or stored.
The cookie jar lives outside the repo (default /tmp).  BGO is a small
donation-funded fan server: the default delay is deliberately slow and there
is a hard page cap.  robots.txt permits /index.php, but for anything above a
few hundred pages, mail boardgamingonline@gmail.com first.

Examples
--------
    python3 tools/scrape_bgo.py journal 7523809 --pw-file ~/tmp/bgo \\
        --out sources/bgo_journal_7523809.tsv

    python3 tools/scrape_bgo.py metadata --pages 1-40 --pw-file ~/tmp/bgo \\
        --out /tmp/bgo_meta.tsv
"""

from __future__ import annotations

import argparse
import csv
import html
import http.cookiejar
import os
import pathlib
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

BASE = "https://www.boardgaming-online.com/"
UA = ("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 "
      "(KHTML, like Gecko) Chrome/138.0.0.0 Safari/537.36")
EDITIONS = {"2006": "4", "2015": "10"}

# index.php?cnt=<n> content ids, discovered from the site's own nav
CNT_FINISHED = 14
CNT_JOURNAL = 52
CNT_BOARD = 202


class Session:
    def __init__(self, jar_path: str, delay: float):
        self.delay = delay
        self.jar_path = jar_path
        self.cj = http.cookiejar.MozillaCookieJar(jar_path)
        if os.path.exists(jar_path):
            try:
                self.cj.load(ignore_discard=True, ignore_expires=True)
            except Exception:
                pass
        self.op = urllib.request.build_opener(
            urllib.request.HTTPCookieProcessor(self.cj))
        self._last = 0.0

    def fetch(self, path: str, post: dict | None = None) -> str:
        wait = self.delay - (time.monotonic() - self._last)
        if wait > 0:
            time.sleep(wait)
        url = path if path.startswith("http") else BASE + path
        data = urllib.parse.urlencode(post).encode() if post else None
        headers = {"User-Agent": UA, "Referer": BASE,
                   "Accept": "text/html,*/*;q=0.8",
                   "Accept-Language": "en-US,en;q=0.9"}
        if data:
            headers["Content-Type"] = "application/x-www-form-urlencoded"
            headers["Origin"] = BASE.rstrip("/")
        req = urllib.request.Request(url, data=data, headers=headers)
        try:
            with self.op.open(req, timeout=60) as resp:
                body = resp.read().decode("utf-8", "replace")
        except urllib.error.HTTPError as exc:
            body = exc.read().decode("utf-8", "replace")
        finally:
            self._last = time.monotonic()
        return body

    def login(self, user: str, pw_file: str) -> None:
        pw = pathlib.Path(os.path.expanduser(pw_file)).read_text().strip()
        if not pw:
            sys.exit(f"{pw_file} is empty")
        self.fetch("index.php")  # prime PHPSESSID
        body = self.fetch("index.php", {"identifiant": user,
                                        "mot_de_passe": pw,
                                        "souvenir": "on"})
        del pw
        if "deconnexion" not in body.lower():
            sys.exit("BGO login failed (no logout link in the response). "
                     "Check the username and the password file.")
        self.cj.save(ignore_discard=True, ignore_expires=True)
        os.chmod(self.jar_path, 0o600)


def _cells(row: str) -> list[str]:
    out = []
    for cell in re.findall(r"<t[dh][^>]*>(.*?)</t[dh]>", row, re.S | re.I):
        text = re.sub(r"<br\s*/?>", "; ", cell)
        text = re.sub(r"<[^>]+>", " ", text)
        out.append(re.sub(r"\s+", " ", html.unescape(text)).strip(" ;"))
    return out


def scrape_journal(sess: Session, game_id: int, max_pages: int = 40):
    """Every journal entry of one game, oldest first."""
    rows: list[list[str]] = []
    for page in range(1, max_pages + 1):
        body = sess.fetch(
            f"index.php?cnt={CNT_JOURNAL}&pl={game_id}&nat=-1&pg={page}&flt=")
        found = 0
        for row in re.findall(r"<tr[^>]*>(.*?)</tr>", body, re.S | re.I):
            cells = _cells(row)
            if len(cells) < 5 or not re.match(r"\d{4}-\d{2}-\d{2}", cells[0]):
                continue
            rows.append(cells[:5])
            found += 1
        if not found:
            break
    rows.sort(key=lambda r: r[0])
    return rows


def scrape_metadata(sess: Session, edition: str, pages, cap: int):
    """One row per finished game of one edition.

    The index table puts the first player on the game's own ``<tr>`` and every
    further player on a two-cell continuation row (score, name), so those are
    folded back into the game they belong to.
    """
    id_jeu = EDITIONS[edition]
    out: list[list[str]] = []
    for n, page in enumerate(pages):
        if n >= cap:
            print(f"page cap {cap} reached; stopping", file=sys.stderr)
            break
        body = sess.fetch(f"index.php?cnt={CNT_FINISHED}&pg={page}&flt=",
                          {"idJeu": id_jeu, "filtre": ""})
        found = 0
        current: list[str] | None = None
        for row in re.findall(r"<tr[^>]*>(.*?)</tr>", body, re.S | re.I):
            cells = [c for c in _cells(row) if c and c != ";"]
            if len(cells) >= 11 and cells[0].isdigit():
                current = [str(page)] + cells[:10] + [
                    f"{cells[10]}:{cells[9]}"]  # player:score
                out.append(current)
                found += 1
            elif current is not None and len(cells) == 2 and cells[0].lstrip(
                    "-").isdigit():
                current[-1] += f"|{cells[1]}:{cells[0]}"
        print(f"pg={page} games={found}", file=sys.stderr)
        if not found:
            break
    return out


def _pages(spec: str):
    if "-" in spec:
        lo, hi = spec.split("-", 1)
        return range(int(lo), int(hi) + 1)
    return [int(spec)]


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("mode", choices=("metadata", "journal"))
    ap.add_argument("game_id", nargs="?", type=int,
                    help="BGO game id (journal mode)")
    ap.add_argument("--user", default="ptarjan")
    ap.add_argument("--pw-file", default="~/tmp/bgo",
                    help="file whose first line is the password; never echoed")
    ap.add_argument("--edition", choices=sorted(EDITIONS), default="2015")
    ap.add_argument("--pages", default="1", help="e.g. 1-40")
    ap.add_argument("--page-cap", type=int, default=200,
                    help="hard stop; raise deliberately, not by habit")
    ap.add_argument("--delay", type=float, default=2.0,
                    help="minimum seconds between requests")
    ap.add_argument("--jar", default="/tmp/bgo_cookies.txt",
                    help="cookie jar path; keep it OUT of the repo")
    ap.add_argument("--out", required=True)
    args = ap.parse_args()

    if pathlib.Path(args.jar).resolve().is_relative_to(
            pathlib.Path(__file__).resolve().parents[1]):
        sys.exit("refusing to write the cookie jar inside the repo")

    sess = Session(args.jar, args.delay)
    sess.login(args.user, args.pw_file)

    if args.mode == "journal":
        if args.game_id is None:
            sys.exit("journal mode needs a game id")
        rows = scrape_journal(sess, args.game_id)
        header = ["date", "player_colour", "age", "round", "text"]
    else:
        rows = scrape_metadata(sess, args.edition, _pages(args.pages),
                               args.page_cap)
        header = ["page", "game_id", "game_name", "edition", "players",
                  "level", "start_date", "end_date", "final_age", "rounds",
                  "top_score", "results"]

    with open(args.out, "w", newline="") as fh:
        writer = csv.writer(fh, delimiter="\t")
        writer.writerow(header)
        writer.writerows(rows)
    print(f"wrote {len(rows)} rows to {args.out}", file=sys.stderr)


if __name__ == "__main__":
    main()
