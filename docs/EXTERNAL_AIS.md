# External AIs and External Data: can we get stronger by not only playing ourselves?

Status: **IN PROGRESS** (written incrementally, 2026-07-26). Sections are committed as
they are finished; a section marked TODO has not been investigated yet.

## Why this document exists

Self-play hill climbing over `WeightedBot` weight vectors has one structural weakness:
it can only discover strategies that some mutation of the current population happens to
stumble on, and it optimizes against *itself*, so a whole population can share a blind
spot forever (e.g. everybody under-values military, so nobody is punished for it).
Classic fixes are (a) an external opponent that plays differently, and (b) an external
corpus of strong play to imitate or to score against. This document asks, honestly, which
of those are actually **reachable** for Through the Ages, and what each would cost.

Verdict up front (details below): there is no drop-in strong external TTA bot we can
plug into a socket. The realistic wins are, in order, a **diverse-opponent league inside
our own engine** (cheap, no external dependency), a **human-in-the-loop evaluation
harness against the official app's Hard AI** (cheap-ish, low volume, high signal),
and **rules/strategy corpora we already have** as a source of hand-written heuristic
priors. Everything involving mining third-party game databases is a dead end or a
scraping project with a bad effort/value ratio.

---

## 1. The official CGE digital app (Steam / iOS / Android)

**What it is.** Czech Games Edition's official digital Through the Ages (Steam app id
`758370`, Google Play `com.czechgames.tta`, App Store id `966245474`; mobile release
Sep 2017, Steam Mar 2018, still actively patched). It is the 2015 edition — the same
edition our engine implements — including the New Leaders & Wonders expansion as DLC
(which we do **not** implement; games would need to be started without it).

**AI offering.** Four AI strengths: a training level plus easy / medium / hard, and on
top of that "world leader" AI personalities with flavoured play styles. There are also
scripted single-player "challenges". In multiplayer/tournament contexts CGE has
special-cased AI behaviour (AI players never offer pacts and refuse all pacts offered).

**How strong is it, really?** Community consensus, not measured:
- The Hard AI is "way beyond average" — it does not blunder or overlook things the way a
  casual human does — but it is "not brilliant"
  ([Steam: Humans vs AI?](https://steamcommunity.com/app/758370/discussions/0/1696043263487678139/)).
- Players routinely accuse it of cheating (seeing hidden info / extra resources); the
  usual explanation is that it just plays a tight tempo game.
- Descriptions of the implementation are consistently that it is a **weighting /
  scoring heuristic** — "the AI has some sort of weighting algorithm, which tells it in
  every situation which one choice among many is the best" — not a search-based or
  learned agent. CGE has tweaked it repeatedly in patches based on player feedback.

That matters a lot for us: if true, the app's Hard AI is architecturally *the same class
of agent as our `WeightedBot`*, just with hand-tuned weights and (probably) a lot of
special-case logic. It is a good **calibration target** — "are we at strong-app-AI
level yet?" is a meaningful question — but it is not an oracle whose play we should try
to clone at scale. Realistic ceiling: strong club human. Not superhuman.

**Programmatic surface: essentially none.** Investigated:

| Surface | Reachable? | Notes |
|---|---|---|
| Game log / replay export | **No** | The single most-requested version of this: a Steam thread explicitly asking for a text dump of the play log for statistical analysis. CGE dev "Elwen" replied it was added to the *features wishlist*, no promise. Players kept bumping it through Oct 2024 with no implementation. [thread](https://steamcommunity.com/app/758370/discussions/0/1735468693689629960/) |
| Documented API / SDK | **No** | None exists. Online play goes through CGE's own account service (`account.czechgames.com`); no public API, no docs, and no third-party client or reverse-engineering write-up exists that I could find. |
| Modding hooks / scripting | **No** | The app has no mod support. The only user-modifiable surface anyone has exploited is **localization strings** — see `yashcherU/Through-the-Ages_ru` (a Russian translation shipped as a drop-in string archive), which is how we got the exact English action-card texts in `docs/SOURCES.md`. Strings only; no game logic, no state. |
| Local save files | Only as opaque blobs | Local/pass-and-play games persist so you can resume, so *some* serialized state exists on disk (Steam userdata / app sandbox), but it is undocumented and there is zero public work on the format. Decoding it would be a from-scratch reverse-engineering project against a shipping binary, and it would only give you *saves*, not per-move logs. |
| Network protocol sniffing | Technically possible, practically bad | TLS to CGE's servers; would need mitmproxy + cert pinning bypass on a rooted Android/emulator, then a protocol reverse-engineer, then a bot account. This is (a) a multi-week project, (b) a ToS violation, (c) it gets you *human* games, not AI games, since AI games are local. Not recommended. |
| Screen scraping | Possible, expensive | The app *does* replay every opponent move visually before your turn (that is a shipped feature — you watch the AI's turn animate). So every AI decision is observable on screen. Turning that into data means OCR/CV against an animated Unity UI. Weeks of work for a brittle pipeline. |

**Conclusion for the app:** there is **no** path to running the app's AI as an automated
sparring partner, and **no** path to bulk-harvesting its games. It is reachable only
through a **human at the keyboard**. See §6 for the design of that.

---

## 2. Board Game Arena

TODO — under investigation.

## 3. Open-source TTA AI projects

TODO — under investigation.

## 4. Published research

TODO — under investigation.

## 5. Human game corpora and strategy corpora

### 5a. Boardgaming-Online (BGO) — the only large TTA game database that exists

`https://www.boardgaming-online.com` — a fan-run play-by-web TTA server, live since 2010
and **still up and still busy** (probed 2026-07-26: HTTP 200, "# games in progress: 636,
# active players: 839"). It is semi-official: the Jan 2016 news post says BGO shipped
*A New Story of Civilization* "after months of teamwork with Vlaada Chvátil and CGE
Team", so it implements **both** the 2006 edition and our 2015 edition. It has an
in-game "journal"/log of every action (referenced repeatedly in its own news posts).

**Public front page counter: 601,532 finished games since Aug 2010.** This is by far the
largest TTA corpus anywhere, and the BGG thread
["Data-Driven strategy tips"](https://boardgamegeek.com/thread/1933554/data-driven-strategy-tips)
is somebody who already mined ~10k BGO games for win-rate statistics — proof the data is
minable in principle.

What I verified myself, unauthenticated:
- `index.php?cnt=14` (Finished games) is **public**. It needs a POST `filtre=<string>`
  (game id / game name / player name) to render results; with a filter it returns a
  paginated table (12,031 pages) with, per game: **game id, game name, edition string,
  player count, level (Prince/King/…), start date, end date, final age, round count, and
  every player's name and final score.** No login. That metadata alone is a real dataset.
- `index.php?cnt=11` (Games in progress) is the same, public.
- The per-game link is `index.php?cnt=202&pl=<gameid>`. Fetched unauthenticated it
  returns **"The game does not exist"** → the actual board/journal view is behind a
  session. Registration is free (`index.php?cnt=9`), so this is *probably* one free
  account away, but I did not create one and therefore **have not verified that the log
  is readable, machine-parseable, or complete**. Treat "we can get move-level BGO logs"
  as UNPROVEN.
- **Edition caveat, and it is a serious one:** every result I could surface through the
  public filter showed edition `Through the Ages 2.4` (the 2006 edition) with dates
  clustered in 2015 and game ids ~7.27M. I could not surface a single post-2016
  new-edition game through the public list. Either the public finished-games index is
  stale/capped, or new-edition games are listed under a different name I did not hit.
  Unresolved. If the mineable corpus is 2006-edition-only it is worth much less to us —
  the 2015 edition changed governments, wonders, tactics, unit techs and end-of-turn
  order (see `docs/SOURCES.md` for the diff list), so 2006 game outcomes do not transfer
  cleanly.

**Is it reachable?** Metadata: yes, today, with `curl` + a POST filter. Move logs:
unknown, likely yes with a free account, definitely a scraping project (rate limiting,
ISO-8859-1 HTML, no API, ToS unexamined).

**What it would actually buy us.** Be realistic about the two very different products:
1. *Outcome metadata only* (cheap, ~a day of scraping): lets you compute nothing about
   moves. You get score distributions by player count and by level, typical game length
   in rounds, and score-vs-rank curves. Genuinely useful for **calibrating our engine's
   score distribution** — if our self-play 3p games end at a mean 140 and BGO humans end
   at 190, something in our engine or our bots is badly off. That's a cheap, high-value
   sanity check and it needs maybe 5–20k games' metadata.
2. *Move-level logs* (expensive, uncertain, wrong edition): the thing you'd want for
   imitation learning. Requires an account, a parser for a hand-rolled PHP journal
   format, and the 2006/2015 edition question resolved. Only worth starting if (a) an
   account confirms new-edition logs exist and are parseable and (b) we actually want
   supervised bootstrapping, which we may not.

### 5b. Written human strategy corpus

We already have a chunk of this in `sources/` (`hypercheat.txt`, `ubg_*`, the GameFAQs
in-depth guide — though `sources/gamefaqs_75690.txt` is currently just a Cloudflare
challenge page, i.e. **that scrape failed and needs redoing**). Additional identified
material:
- BGG [thread 1933554 "Data-Driven strategy tips"](https://boardgamegeek.com/thread/1933554/data-driven-strategy-tips)
  (win rates from ~10k BGO games), [thread 2801950](https://boardgamegeek.com/thread/2801950/a-strategy-guide-for-the-game-with-the-expansion)
  (guide based on ~100 games, 3–4p), [thread 934016](https://boardgamegeek.com/thread/934016/general-strategy-tips-for-a-newbie).
  **Note: BGG blocks plain `curl` and `WebFetch` with 403, and the XML API returns 401
  for `thread?id=`** — scraping BGG forums needs a browser-like session or an API key,
  budget for that.
- Steam guide ["TTA strategy game and some basic knowledge"](https://steamcommunity.com/sharedfiles/filedetails/?id=1367549747)
  (translated Chinese guide) — Steam pages fetch fine.
- [Stately Play "Strategy 101: Through the Ages, Resource Edition"](https://statelyplay.com/2017/09/25/strategy-101-through-the-ages-resource-edition/).
- **TTA World Championship** exists and is active (2023 winner interviewed on BGG;
  a [2025 World Championship YouTube playlist](https://www.youtube.com/playlist?list=PLN735uyn0raXB1jnNK8YksQ6Koxz35_0s)
  with participants replaying and analysing every game). Expert games with commentary,
  but the medium is **video** — extracting positions is manual transcription. Useful as
  a handful of gold-standard annotated games, not as a corpus.

**Value:** this is the cheapest external input we have and it is *already partly in the
repo*. It cannot train a bot, but it is exactly the right raw material for two things:
(a) seeding sane initial weight vectors and feature definitions for `WeightedBot` so hill
climbing starts in a good basin instead of at random, and (b) writing **falsifiable
assertions** to test the trained bot against ("a strong player almost never stays in
Despotism past Age I"; "2 irrigated farms carry you through Age II"). Turn each into a
statistic over self-play logs; where our bot disagrees with expert consensus, that is a
lead on an eval bug or a genuine discovery. Effort: hours, not weeks.

## 6. The human-in-the-loop option (play the app, log the AI)

TODO — under investigation.

## 7. Ranking and recommendation

TODO — under investigation.
