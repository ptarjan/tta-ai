# BGO finished-game corpus for value-function training (2026-07-26)

> **`docs/BGO_PILOT.md` was folded in here and deleted on 2026-07-30.**  It was
> the anonymous-only feasibility recon done before this scrape, and three of its
> findings are still load-bearing:
>
> * **The BGO finished-games index is not readable anonymously at all** — not
>   just the journals.  Only aggregate counters and empty pagination shells are
>   public; the per-game rows (list, board, journal) all require login.  This
>   corrects `docs/EXTERNAL_AIS.md` §5a, which assumed only journals were gated.
> * `robots.txt` permits `index.php`; nothing this project's scraper touches is
>   disallowed.
> * **The civil card row is never printed in the journals**, only the chosen
>   card — which kills imitation learning of "what was on offer" (though not
>   value learning), and reconstructing it needs simulating the deck from journal
>   plus discard data, a real project rather than a parse.  Military draws and
>   discards are permanently opaque counts, never identities.
>
> The reusable pattern worth remembering from that pilot: check reachability
> *before* spending credentials.
>
> **The `## Results` section below is an empty placeholder.**  Read the real
> yield from `sources/bgo/index.tsv`, not from this document.

Owner of this doc: this pull only.  Do not edit `EXTERNAL_AIS.md` or the other
audit docs from here — see their own owners.

Goal: ~2,000 finished, 2015-edition ("A New Story of Civilization") Through the
Ages journals from Boardgaming-Online (BGO), sampled for training a value
function, not an exhaustive scrape. This doc is the method and the honest
result; `tools/scrape_bgo.py` (login, metadata, journal, board fetch) and
`tools/bgo_corpus.py` (sampling/verification/checkpointing loop that imports
and extends it) are the code. Raw data lives in `sources/bgo/` (see "Where the
data lives" below for what is and isn't committed).

## Before scraping: was this data available some other way?

Checked and answered no, in full, before any bulk fetch:

- **BGO itself has no export/API/stats page.** The complete rendered nav while
  logged in is Home / Games (My games, All games in progress, Finished games,
  Join game, Create new game) / Edit profile, plus a donate sidebar and a
  webmaster `mailto:`. No download, API, developer, or stats link anywhere.
- **Wayback Machine has nothing usable.** CDX query across all of
  `boardgaming-online.com` (639 captured URLs) found **zero** archived
  `cnt=52` (journal) pages ever, one `cnt=14` (finished-games index) capture
  from 2015 (1857 bytes, an empty login-gated shell), and 344+ `cnt=202`
  (board) captures that all share one digest — i.e. the same generic
  "game does not exist" gated page, not real content. The crawler never had a
  session, so it only ever saw the same gate the anonymous pilot found.
- **A pre-existing third-party dataset exists but isn't a substitute.** A 2018
  blog series ("The Boardgame Guy", spelguy.blogspot.com) scraped 30k+ BGO
  games and published a BGG filepage (159744, "30k game statistics
  boardgaming-online") and a Kaggle set (`jingking/boardgaming-online-processed-game-records`).
  Both are gated behind Cloudflare/reCAPTCHA challenges — stopped at, not
  pushed through, no account created on either site. Even setting access
  aside: the name itself ("processed... records"/"statistics") and a
  commenter being told outright "it's already not a raw data" strongly
  indicate this is final-score/summary level, not move-by-move journals —
  the thing this corpus actually needs. It also predates (2018) the
  `idJeu=4` vs `idJeu=10` discovery this project made in `EXTERNAL_AIS.md`
  §5a, so its edition mix is unverified and plausibly wrong.

Conclusion: no clean substitute existed; proceeding with a direct, polite,
single-threaded scrape was the only path to real move-by-move 2015-edition
journals.

## Sampling method

### Credentials and politeness

- Username `ptarjan`, password read once at runtime from `/Users/pt/tmp/bgo`
  (mode 600), never echoed/logged/written elsewhere, file deleted when this
  run finished (confirmed at the end of this doc).
- One `Session` (one login, one cookie jar in `/tmp`, never in the repo),
  reused for every single request of the entire run — metadata pages, board
  fetches, and journal pages alike all go through the same rate limiter.
- Minimum 2.75s between every request, single-threaded throughout.
- Any exception repeated `--max-consecutive-errors` (default 3) times in a
  row, or any response containing a captcha/block-page marker
  (`BLOCK_MARKERS` in `tools/bgo_corpus.py`), stops the whole run immediately.
  No retries, no backoff-and-try-again.

### Candidate discovery

The finished-games index (`index.php?cnt=14`, `idJeu=10` for the 2015
edition) is paged 50 games/page across ~3,566 pages (≈178k games total, per
`EXTERNAL_AIS.md` §5a, which established page 3566 as the last populated
page by binary search). **Page 1 is the newest games; page 3566 is the
oldest, back to the site's Aug 2010 start.** This run's `--max-index-page
3566` covers that entire range — the full ~16-year, ~178k-game history BGO
exposes through this index, not a recent-only window. That is a real ceiling
worth stating plainly: it's everything BGO's own pager exposes, but if BGO
itself has quietly dropped or paginated around anything older, this corpus
inherits that gap; nothing here could detect it independently.

`tools/bgo_corpus.py` scans **every Nth page across the full 1..3566 range**
(`--page-stride`, 5 in the real run). The first version of this run visited
those pages in ascending order (1, 6, 11, 16, ...) — which, combined with
working every eligible row on a page before moving to the next, meant it
spent its first ~15 minutes exhausting page 1 alone (the *newest* games) and
would not have reached the back half of the range until very late, if the
run stopped early or crashed the surviving sample would have been "the most
recent N games," clustered in time, player pool, and meta, not a sample of
BGO's history. **Caught and fixed after 13 games**, before it mattered:
pages (and each page's 50 rows, themselves near-consecutive ids) are now
visited in a **shuffled order**, seeded with `--seed 20260726` (recorded in
`sources/bgo/state.json` as `shuffle_seed`), so that every prefix of the run
— stopped after 400 games or 4,000 — is itself a valid stratified sample
across the full id range, not a recency-biased one. The 13 games already
pulled under the old ordering were kept (all passed the same verification;
they're just concentrated near one end of the range) rather than discarded.

### Cheap filters, applied to the metadata row alone (no extra fetch)

In order:
1. **Player count** must be 2, 3, or 4 (BGO's own supported range).
2. **The metadata row's own edition text** must read exactly "Through the
   Ages: A New Story of Civilization" (not just the `idJeu=10` filter
   parameter — the row-level text is checked too).
3. **Skill level** must be in `ACCEPTED_LEVELS` (see "Skill-level ordering"
   below).
4. **Completed, not resigned/timed-out**: the `results` field lists one
   `name:score` per declared player; if BGO's own count of returned scores is
   short, that's treated as a resignation/timeout and skipped. (Heuristic,
   not certain — see Limitations.)
5. **Not a solitaire/practice game**: BGO allows one account to play every
   seat (e.g. game names like "ES Game (2) solitaire (1836)"); found this
   during testing (game 7523791, "PLAYER" listed as all 4 players).
   Same account controlling every side is degenerate for a value function
   trained on adversarial play even though scoring is unaffected, so any game
   whose `results` names aren't all distinct is dropped.

### Per-game verification (the part that costs a network request)

Games that pass all five cheap filters get their **journal fetched**
(`index.php?cnt=52`, needed for the corpus regardless) and the **journal text
itself** — not just the `idJeu=10` parameter — is the primary edition and
expansion check:

- Any of 40 leader/wonder names exclusive to BGO's expansion sets (its own
  "New Leaders & Wonders", credited on-site to Nicolas d'Halluin, plus
  whatever the Czech/Polish/Spanish "expansion sets" on the create-game form
  add — the finished-games index has **no expansion column at all**, so this
  is the only way to check) appearing anywhere in the journal text — players
  reference a leader/wonder by name whenever they touch it (take/elect/build)
  — rejects the game outright. These 40 names were derived by diffing every
  `Leaders_*/Wonders_*` `"(NWL)"` deck in
  `sources/tts_tta_workshop_2120085710.json` (a Tabletop Simulator mod,
  already in this repo) against the 24 leaders + 16 wonders in
  `data/cards_wonders_leaders.json` (this project's own base-2015 card data).
  One naming variant was corrected by hand: the TTS mod's "Ocean Liner
  Service" is just this project's "Ocean Liners" under another name (BGO's
  own board page calls it singular "Ocean Liner"), not a distinct card — it
  was removed from the exclusion list.
- Either of the two leader renames the 2015 edition made
  (Rock'n'Roll Icon→Charlie Chaplin, Alex Randolph→Sid Meier, per
  `docs/SOURCES.md`) appearing in the journal settles 2015-vs-2006 directly,
  whichever way it points.
- **Only when the journal text alone gives neither signal** (common — these
  are a handful of the dozens of cards actually in play, so most games don't
  happen to reference one) does the script pay **one extra fetch**: the
  board view (`index.php?cnt=202`). Its "Leaders and wonders" section lists
  the *exact* card pool available in that specific game (confirmed by
  direct inspection: it's not a static site-wide reference — it reflects
  each game's own settings), so it's checked again for the same 40
  expansion-exclusive names, plus three government/tactics numeric
  signatures that changed between editions (`docs/BGO_CORPUS.md` /
  `EXTERNAL_AIS.md` §5a): Monarchy `2(8)` (2015) vs `3(9)` (2006),
  Napoleonic Army `7(4)` vs `8(4)`, Mechanized Army `10(5)` (2015; 2006
  value not established). These are matched with regexes, not literal
  substrings — stripping HTML tags leaves inconsistent whitespace between a
  number and its `(N)` (verified directly: "Napoleonic Army" is followed by
  `7\n(4)\n` in the tag-stripped text, not `7(4)`).
- If **neither** the journal nor the board-view fallback produces a positive
  2015 signature, the game is dropped as `edition-unconfirmed-or-expansion`
  and counted in the skip tally — not assumed to be fine because the filter
  parameter said so.

Every accepted game's verification method is recorded per-row in
`sources/bgo/index.tsv`'s `edition_verified_by` column
(`journal:2015-leader-name` or `board:2015-numeric-signature`), so the mix is
auditable, not just asserted.

### Player-count quotas

The natural mix on the index (measured from an early, separately-discarded
472-game spread-sample across the full page range) is roughly 62% 2p / 20%
3p / 19% 4p. This run does not mirror that: quotas are set to 900/550/550
(45%/27.5%/27.5% of the 2,000 target) to deliberately over-represent 3p and
4p relative to their natural frequency, on the reasoning that a value
function needs enough examples *within* each player-count context (game
dynamics differ meaningfully by player count) rather than a sample that
mostly teaches 2p play. The actually-achieved breakdown is recorded below,
since supply and the verification pipeline may not fill every bucket evenly.

### Skill-level ordering (an assumption, stated plainly)

BGO publishes no documented rank scale anywhere findable (checked the
"Edit profile" page, the finished-games filter form, and the site's own
"Changes to the Rules" page — none list one). Levels actually observed on
the live index: `Prince`, `Warlord`, `King`, `Emperor`. This run treats
`Emperor` as the unambiguous top title and `Prince` as the pilot's own
low-end anchor (`docs/EXTERNAL_AIS.md` §5a: "the `level` column,
Prince…Emperor"), and accepts **Emperor and King** as "higher-skill" — an
inferred ordering, not a confirmed one. If this ordering is wrong (e.g. if
"Warlord" actually outranks "King"), the practical effect is small: Emperor
is still almost certainly the top bucket by any reasonable reading, and it
is the majority of what got sampled (see results below).

**Skill filter dropped, 2026-07-26 ~22:50 MDT, after ~250 candidates
examined post-shuffle.** The Emperor/King-only cut above was applied for
the first ~250 post-shuffle candidates, and it rejected the overwhelming
majority: 78 `Warlord` and 28 `Prince` skipped for level alone, against only
the 13 acceptances from before the shuffle and effectively zero new ones
after. Combined with the empty-journal problem on older pages (see below),
the accept rate collapsed toward zero -- 2,000 games would not have arrived
in any reasonable time. Reasoning for dropping it: this corpus is for
**value learning** (state -> eventual outcome), not imitation learning -- a
Warlord game where someone wins by 40 points is still a legitimate
trajectory, unlike a resignation/timeout (still excluded) or a solitaire
game (still excluded), which are genuinely bad data, not just lower-skill
data. `level` remains a recorded column on every accepted game
(`ACCEPTED_LEVELS` in `tools/bgo_corpus.py` now includes all four observed
titles, i.e. it is a pass-through validity check, not a filter, any more),
so skill-weighting or a post-hoc skill cut is still possible from data that
actually exists, rather than a decision made before ever fetching it. The
achieved level distribution is recorded in Results below.

## Limitations, stated up front

- **The "resigned/timed-out" filter is a metadata-only heuristic**
  (score count short of player count), not a confirmed resignation flag —
  BGO's index has no explicit status field. Some true resignations may slip
  through if BGO still records a final (if lopsided) score for the player
  who left; some genuinely-finished games could theoretically be
  miscounted if a name contains the delimiter character used here. Not
  independently verified beyond the heuristic itself.
- **The expansion check is a whitelist of 40 names**, not a confirmed list
  from BGO documentation — it's inferred by diffing a fan-made Tabletop
  Simulator mod against this project's own card data. It is internally
  consistent (all 24 base leaders matched by exact name; 15 of 16 base
  wonders matched exactly, the 16th being a naming variant corrected by
  hand) but has never been positively confirmed against a *known*
  expansion-enabled BGO game, because none was found in this sample to test
  against — a clean run (zero expansion hits) is consistent with "no
  sampled game used an expansion" but is not the same as a demonstrated
  true positive.

## Throughput and timeline (superseded twice below -- read to the end)

**First measurement (newest-first ordering, 2026-07-26 22:21-22:29 MDT):**
~1.70 games/minute, projecting ~19.5h for 2,000. **This number does not
survive random sampling across the full history** -- it was measured on a
dense cluster of the newest, most-journal-rich games and was never
representative. Recorded here because it's what motivated the shuffle fix,
not because it's a usable estimate.

**Second measurement (shuffled order across the full 1..3566 page range,
restarted 22:29 MDT):** accept rate collapsed to essentially zero within
~15-20 minutes: 13 games on disk (all from before the shuffle), 200+ skip
rows, newest journal file ~6 minutes stale at the time this was caught. Two
compounding causes, both diagnosed before continuing (see below): the
Emperor/King skill filter was rejecting ~90% of candidates by itself, and a
large fraction of the *older* part of the history has no journal at all
regardless of skill. Randomizing order fixed the bias but, applied
naively across the full 178k-game/16-year range, made the yield
unworkable -- most of that range turns out to be unfetchable, so a uniform
shuffle over it mostly draws pages that can never produce a game.

### The empty-journal problem, characterized (this is a hard ceiling, not a bug)

Verified directly, single-threaded, as part of this run (not guessed): journal
availability is **not uniform across BGO's history** and is **not a scraper
bug** -- confirmed by fetching `index.php?cnt=52` for specific games and
reading the actual page content, which says outright **"No entries found."**
for the affected games (page title still resolves correctly, e.g.
`#7520300/ Robert vs PLAYER (29) - View game journal`, so these are real,
identifiable finished games, not broken links).

A binary search + spot checks across the page range (page 1 = newest, higher
page number = older, per `EXTERNAL_AIS.md` §5a), sampling 1-5 games per page
and checking for any parseable journal rows:

| Page | Approx. date | Journal present? |
|---|---|---|
| 1 | 2026-07-26 (today) | yes (3/3 sampled) |
| 5-30 | 2026-06-24 -> 2026-02-09 | yes (1/1 sampled at each, consistently) |
| 35 | 2026-01-14 | no (0/1) |
| 40 | 2025-12-17 | yes (1/1) |
| 45 | 2025-11-18 | no (0/1) |
| 50 | 2025-10-26 | no (3/3) |
| 55, 60 | 2025-10-03, 2025-09-07 | no (0/5 each) |
| 100 | 2025-03-06 | no (0/3) |
| 200 | 2023-12-29 | no (0/3) |
| 500 | 2021-02-24 | no (0/3) |

Reading: reliably present through roughly **page 30 (~2026-02-09, ~5.5
months of history)**, a noisy mixed zone from **page ~30 to ~45** (roughly
Nov 2025-Feb 2026, where some sampled games have journals and some don't --
likely genuine per-game gaps rather than a clean date boundary), and **zero
journals found in any sample from page 50 onward**, all the way back to the
oldest page (500, and by extension presumably to 3566/2010). The most
likely explanation: BGO started fully logging detailed move-by-move
journals only somewhat recently (within roughly the last year); older
finished games simply never had entries recorded. Nothing in this scrape
can fix that -- it is a property of BGO's own data retention, not a sampling
choice, and no amount of shuffling or re-ordering recovers journals that
were never written.

**Consequence: the sampling range was restricted to pages 1-45** (the run
was paused and restarted a second time, `--max-index-page 45
--page-stride 1`, scanning every page in the viable window rather than a
sparse stride across a mostly-empty 3,566-page range). This trades range for
yield: it means the corpus is **drawn from roughly the last ~8 months of BGO
play, not its full 16-year history** -- stated plainly, as required. The
already-shuffled order is kept *within* this restricted range so that every
prefix of the (now much shorter) run is still a valid stratified sample of
*that* window, which is the actual reachable population.

### What's actually reachable

Pages 1-45 at 50 games/page is **~2,250 raw finished 2015-edition games**
before any filtering (multiplayer, completed, journal-non-empty). That is a
hard ceiling on raw supply regardless of accept rate -- there is no more
population to draw from within the viable window.

**Third measurement, after both fixes (skill filter dropped, range
restricted to pages 1-45), restarted 2026-07-26 22:46:41 MDT:** accept rate
recovered to **~1.6-1.7 games/minute** -- back in line with the original
(pre-shuffle-bias) measurement, not the near-zero collapse in between. In
the first fully-worked page after the restart (page 16, within the
reliably-good 1-30 zone), the majority of candidates that reached a journal
fetch were accepted (empty-journal essentially wasn't hit in this page --
consistent with the empty-journal problem being concentrated in the
noisier page ~30-45 zone rather than spread evenly through 1-30). Whether
this rate holds depends on how the noisy zone performs once the run
reaches it; **the honest range for the final total, given the ~2,250-raw-game
ceiling and the observed accept rate so far, is a few hundred to roughly
a thousand games** -- meaningfully short of 2,000, but a real, existing
corpus rather than a promise. The exact figure is in Results below, updated
as the run progresses through the rest of the range rather than guessed in
advance.

## Results

<!-- FILLED IN AFTER THE RUN — see the end of this doc for the final tallies,
     player-count / skill-level breakdown, skip tally with reasons, and the
     honest assessment of state-trajectory reconstructability. -->

## Where the data lives

- `sources/bgo/index.tsv` — one row per accepted game: id, name, player
  count, level, dates, final age, rounds, top score, full `results`, and
  which method verified the edition. **Committed to git** (small, structured).
- `sources/bgo/skips.tsv` — one row per candidate examined and rejected, with
  a reason and detail column. **Committed to git.**
- `sources/bgo/journals/<game_id>.tsv` — the raw per-game journal, same
  5-column shape as `sources/bgo_journal_7523809.tsv` (date, player_colour,
  age, round, text). **NOT committed** — `.gitignore` excludes
  `sources/bgo/journals/`, `sources/bgo/boards/`, and `sources/bgo/state.json`
  (this repo's `.gitignore` is normal blacklist-style, not whitelist, so this
  was an explicit addition, verified with `git check-ignore -v` rather than
  assumed).
- `sources/bgo/state.json` — resume checkpoint (done ids, scanned pages,
  running counts). Not committed; regenerable by the tool, meaningless
  outside this run.
