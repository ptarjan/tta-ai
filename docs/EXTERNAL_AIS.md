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

### 4a. On Through the Ages: there is none. We would be first.

Checked and came up empty: arXiv (`all:"Through the Ages"` ∩ cs.AI), Semantic Scholar,
Google Scholar, Czech university repositories (Charles/dspace.cuni.cz, CTU/dspace.cvut.cz,
Masaryk/is.muni.cz — searched in Czech for *diplomová/bakalářská práce* + *umělá
inteligence* + *Vlaada*), and
[captn3m0/boardgame-research](https://github.com/captn3m0/boardgame-research), the
community index of essentially all modern-boardgame AI research — it has sections for
Carcassonne, Dominion, Puerto Rico, Race for the Galaxy, Catan, Terra Mystica, Hanabi and
more, and **no Through the Ages section at all**. Czech CS theses on game AI do exist
(curling, Carcassonne, Quoridor, Scotland Yard) but none on TTA.

One coverage gap to note honestly: dspace.cuni.cz returned HTTP 429 under automated
querying, so its internal search was not exhaustively swept. A manual check is cheap if
anyone cares.

**So there is no published algorithm-and-strength result to copy or benchmark against.**
That cuts both ways: no free head start, but also a genuinely open problem.

### 4b. TAG (Tabletop Games framework, QMUL) — the most useful adjacent asset

[github.com/GAIGResearch/TabletopGames](https://github.com/GAIGResearch/TabletopGames),
Java, Apache-licensed, actively maintained. Its `games/` directory was enumerated
directly: **42 games**, and **Through the Ages is not one of them**. The closest
structural analogues it *does* implement are **Terraforming Mars, Puerto Rico, 7 Wonders
(`wonders7`), Dominion, Power Grid, Root, Catan**.

Why it matters even though it lacks TTA: TAG ships MCTS / RHEA / OSLA agents, a PyTAG
Gym wrapper for RL, and — importantly — an evaluation methodology for exactly our
problem (high-variance multiplayer games where you cannot tell skill from luck).

Key papers, with the ones worth actually reading marked:
- **★ Goodman PhD thesis, "Dice, Cards, Action! The Analysis, Play and Design of
  Multiplayer Tabletop Board Games with MCTS"**, QMUL 2025 —
  [qmro.qmul.ac.uk/xmlui/handle/123456789/108265](https://qmro.qmul.ac.uk/xmlui/handle/123456789/108265).
  The single most concentrated body of knowledge on our exact problem class.
- **★ MultiTree MCTS in Tabletop Games**, Goodman/Perez-Liebana/Lucas, CoG 2022 —
  [pdf](https://ieee-cog.org/2022/assets/papers/paper_91.pdf). One search tree per
  player; tested on 11 TAG games; helps at low simulation budgets. Directly relevant to
  3–4p TTA.
- **★ Following the Leader in Multiplayer Tabletop Games**, FDG 2023 —
  [pdf](http://www.diego-perez.net/papers/FollowingLeader-FDG23.pdf). Opponent modelling,
  max-n vs paranoid in >2 players. TTA has real kingmaking/leader-bashing via aggression,
  so this is not academic.
- **★ Skill Depth in Tabletop Board Games** (CoG 2024) and **Seeding for Success: Skill
  and Stochasticity in Tabletop Games** (ToG 2025), Goodman et al. — how to tell whether
  a bot is actually stronger or just luckier. Our hill climb needs this; TTA variance is
  high enough to fool a naive round-robin.
- Design and Implementation of TAG — [arXiv:2009.12065](https://arxiv.org/abs/2009.12065);
  PyTAG (multi-agent RL over TAG) — [arXiv:2405.18123](https://arxiv.org/abs/2405.18123),
  whose honest finding is that **RL struggles on the complex games while MCTS stays
  competitive**; TAG: Terraforming Mars, AIIDE 2021 —
  [pdf](https://tabletopgames.ai/assets/pdf/gaina2021terraforming.pdf) (the best reference
  implementation if we ever port TTA into TAG).
- Evaluation of Perfect-Information MCTS in Imperfect-Information Games, CoG 2026 —
  argues cheap determinized/PIMC search is often enough vs full ISMCTS. Relevant because
  **TTA's hidden information is thin**: the card row is public, only hands and the future
  events deck are hidden.

Other frameworks were checked and are not a fit: **OpenSpiel** (~70 envs) and **Ludii**
have nothing in the heavy-euro class; **RLCard** is trick-taking/poker only.

### 4c. Comparable games — what algorithms actually worked, and how strong

| Work | Game | Algorithm | Strength reached | Code |
|---|---|---|---|---|
| **Keldon Jones' RFTG AI** ([bnordli/rftg](https://github.com/bnordli/rftg), [writeup](https://medium.com/@tduringer/race-for-the-galaxy-ai-4cc933249814)) — not a paper, but the strongest result in this genre | Race for the Galaxy | **TD-learning MLP** predicting win probability at turn granularity, ~30k self-play games, hand-designed features | Widely regarded near-world-class; shipped in the commercial Temple Gates app | Yes, C, runnable |
| [Mastering Terra Mystica](https://arxiv.org/abs/2102.10540), Perez 2021 | Terra Mystica | AlphaZero-style self-play w/ hand-designed state repr | Beats baselines; compared to typical human scores; **not** shown to beat strong humans. Unrefereed preprint | Yes (`terrazero`) |
| [Playing Various Strategies in Dominion with Deep RL](https://ojs.aaai.org/index.php/AIIDE/article/view/27518), AIIDE 2023 | Dominion | Geometric DL over a multiset state repr; Soft Actor-Critic adapted to **variable-size action sets** | Best learning-based Dominion agent; still loses to search-based agents in some kingdoms | Partial |
| [AIs for Dominion Using MCTS](https://link.springer.com/content/pdf/10.1007/978-3-319-19066-2_5.pdf), Winder 2014 | Dominion | UCB / UCT | 67% vs a good finite-state agent | — |
| MCTS for the Game of 7 Wonders, Robilliard et al. 2014 | 7 Wonders | plain UCT | Beat their heuristic bots; key result is that **MCTS tuning lore transfers from abstract games to modern euros** | — |
| [AI Techniques for Puerto Rico](https://link.springer.com/10.1007/978-3-319-59394-4_8), 2018 | Puerto Rico | RL that **switches between high-level scripted strategies** | Modest, but the portfolio/script idea is the takeaway | — |
| [SCOUT](https://doi.org/10.1007/978-3-319-61030-6_27), ICCBR 2017 | Race for the Galaxy | case-based reasoning | Below Keldon's net | — |
| [Splendor-Zero](https://github.com/inhabae/Splendor-Zero) (hobby, unrefereed) | Splendor | C++ engine + PyTorch policy/value net + **IS-MCTS** | Claims 2068 Elo / top of the Spendee server | Yes |
| Catan line: Szita/Chaslot/Spronck 2010 (MCTS), POMCP+human preferences AIIDE 2018, [cross-dimensional NN](https://arxiv.org/abs/2008.07079) | Catan | MCTS / POMCP / CNN | Beat the JSettlers heuristic bot; no strong-human claims | Some |

No published AI research exists for Twilight Struggle, Scythe, Agricola, Root (beyond
TAG), or the Civilization board game either.

### 4d. What the literature says we should do

- **Action-space size is the central problem, not hidden information.** TTA's hidden info
  is thin; its per-turn combinatorics are not. Two proven levers: a **portfolio of
  scripted high-level strategies** with the learner choosing among them (Puerto Rico
  paper), and a **variable-size action-set policy head** (the Dominion SAC paper is the
  best template — it solves literally "the legal action set changes every turn and is
  huge", which is our `legal_moves()` situation).
- **Decompose the turn into per-action-point decision nodes** rather than enumerating
  whole-turn sequences. This is what makes branching tractable and is how TAG's
  `extendedSequence` machinery works. Worth checking our `engine/actions.py` already does
  this (it appears to — moves are single tagged tuples).
- **Try determinized/PIMC search before ISMCTS.** Cheaper to build, and the CoG 2026
  result says it is often adequate for thin hidden info.
- **Multiplayer: use max-n or MultiTree MCTS, not paranoid/minimax.**
- **TD-learning a value function over self-play** (Keldon's RFTG recipe: hand-designed
  features → small net → predict win probability, ~30k games) is the highest
  strength-per-effort result anyone has achieved in this genre, and it is a *strict
  upgrade path from our current linear `WeightedBot`* — same features, nonlinear head,
  trained by TD instead of by hill climbing. This is arguably the single most actionable
  finding in this whole document, and it needs **no external AI at all**.
- Background: [ISMCTS](https://ieeexplore.ieee.org/document/6203567) (Cowling et al.
  2012), [MCTS survey](https://arxiv.org/abs/2103.04931),
  [AlphaZe∗∗](https://www.frontiersin.org/journals/artificial-intelligence/articles/10.3389/frai.2023.1014561/full)
  (AlphaZero-style baselines are surprisingly strong on imperfect-info games — supports
  skipping CFR machinery), [RHEA against Pandemic](https://arxiv.org/abs/2103.15090).

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

§1 concluded that the app's AI is reachable **only** through a human at the keyboard.
The good news is that we already built 90% of the harness for a different reason: the
advisor (`advisor/advisor.py`, `advisor/state_io.py`) exists to sit next to a *physical*
table, mirror the board, recommend the human's move, and absorb "here is what the
opponents did" as terse patch lines. Pointing it at a screen instead of a table is a
configuration change, not a new program. What is missing is (a) structured logging and (b) an
honest accounting of what a human hour buys.

### 6a. The design

**Setup.** One game of the official app, base game only — the New Leaders & Wonders DLC
must be off, since our engine does not implement it (§1). Human takes seat 0, opponents
are app AIs at a **recorded, fixed difficulty** (Hard for the headline number; the
"world leader" personalities are a *different* experiment and must be labelled as such).
The advisor runs in a terminal beside the app: `python3 -m advisor.advisor --players 3
--seat 0 --log games/2026-07-26-a.jsonl`.

**Two modes, and the distinction is the whole point of the exercise:**

- **Strict mode** — the human presses Enter every single turn and plays whatever the
  advisor starred, with no judgement of their own, ever. The human is an I/O device: a
  pair of eyes for the app and a pair of hands for the mouse. This is the only mode in
  which the final score is a *measurement of our bot*. Any turn where the human "fixes"
  a recommendation silently destroys that property.
- **Free mode** — the human plays their own game and the advisor just records what it
  *would* have done. The score measures the human, not the bot, but the **override rate**
  (how often a competent human rejects the bot's top pick, and at which decision types)
  is a genuinely useful, nearly-free quality signal that needs no opponent transcription
  at all.

Run strict mode for evaluation. Run free mode when you want a cheap bug-hunt.

**Fidelity tiers.** These differ by an order of magnitude in human cost, so pick
deliberately:

| Tier | What is logged | Overhead vs just playing | What it buys |
|---|---|---|---|
| **0 — outcome only** | player count, AI difficulty, final scores, round count. No advisor. | ~1 min/game | Score-distribution calibration (same product as §5a metadata, but on the *right edition*, which BGO may not be). Nothing about moves. |
| **1 — advised seat, coarse opponents** | full state snapshot + ranked candidate list + move played at every one of *our* decisions; opponents reported only as the cheap visible fields (card taken, culture, science, strength, new techs/wonders) | ~1.5–2× | Win rate and score margin of *our bot* vs the app AI; our bot's decisions in real (non-self-play) positions; override rate. **This is the tier to actually run.** |
| **2 — full transcription** | every opponent action replayed through the engine as a real move, so the app AI's *policy* is captured | ~3×, and needs new code | Move-level agreement/disagreement with the app AI: the disagreement catalogue. Worth doing for a handful of games only. |

Tier 2 needs a change the advisor deliberately does not have today: `advisor/README.md`
is explicit that "**Opponents' turns are *not* replayed as moves; you report the
result**". Someone would have to add an opponent-move entry path that pushes rival
actions through `engine.actions` — which also means resolving the hidden information the
mirror does not have (their hand). Non-trivial. Do not assume Tier 2 is one flag away.

### 6b. Logging format

Append-only **JSONL**, one record per decision point, plus a header and a footer record
per game. The key design decision: **embed the existing `state_io.dumps()` snapshot
verbatim** rather than inventing a serializer. `loads(dumps(b))` round-trips exactly
(that is `state_io`'s stated contract, and the round trip is covered in
`advisor/tests/`), so every logged position can be reconstructed into a real
`GameState` offline. That is what makes the human's time *reusable*: one logged game can
be re-scored by every future bot we ever train, not just the one that was in the room.

```jsonl
{"v":1,"type":"game","id":"2026-07-26-a","src":"cge-app","app_version":"...","edition":"2015-base","dlc":false,"players":3,"seat":0,"opponents":[{"kind":"ai","level":"hard"},{"kind":"ai","level":"hard"}],"mode":"strict","weights":"experiments/champion_3p.json","started":"2026-07-26T19:04:00Z"}
{"v":1,"type":"decision","game":"2026-07-26-a","ply":37,"round":6,"age":"I","actor":"p0",
 "state":"tta 1\ngame 3p seed=0 turn=6 round=6 age=I/I cur=0 start=0 phase=actions me=0\n...",
 "ranked":[{"move":["take",4],"score":12.31},{"move":["play_action","Rich Land (A)"],"score":11.29}],
 "played":["take",4],"source":"bot","latency_s":4}
{"v":1,"type":"observed","game":"2026-07-26-a","ply":38,"actor":"p1",
 "patches":["take p1 4","p1 c=41 s=12 str=9","p1 tech+ irrigation:2","p1 hc=3"],
 "state_after":"tta 1\n..."}
{"v":1,"type":"result","game":"2026-07-26-a","scores":{"p0":183,"p1":201,"p2":166},"winner":"p1","rounds":18,"human_minutes":95,"notes":"mirror desynced at round 14, resynced with 'row'"}
```

Notes on the fields, and why each is there:

* `state` is the snapshot **string**, not a nested object. It is ~1 KB at setup and a
  few KB late game; at ~200 decisions/game that is well under 1 MB per game. There is
  no reason to store deltas and every reason not to — a delta stream only replays
  correctly against the engine version that produced it, and the engine is under active
  development.
* `ranked` holds the advisor's candidate list with its scores. Store the **full** list,
  not the top 3 shown in the UI — top-k agreement, rank-of-chosen-move and regret all
  need the tail. `rank_moves()` in `advisor/advisor.py` already produces exactly this
  structure (move, score, text, reason).
* `played` is the engine's own tagged move tuple, so it replays directly.
* `source` ∈ `bot` (strict-mode Enter) / `human` (an override, and then `note` should say
  why) / `observed` (a rival's action). Without this field a free-mode log is
  uninterpretable later.
* `patches` on `observed` records preserve the literal lines the human typed. When the
  mirror later turns out to have drifted, these are the only forensic trail.
* The `result` record's `human_minutes` and `notes` are not bureaucracy — they are how
  we find out, after five games, whether this whole idea is affordable.

Implementation is small: `Advisor` already accumulates a narrative `self.log` list
(`advisor/advisor.py:481`); this is a structured sibling written through a `--log` flag,
appended and `flush()`ed at every decision so a crashed or abandoned game still leaves
usable data.

**One extra command worth adding: `verify`.** The mirror can drift silently (a misread
opponent culture, a missed event). The app displays every player's score and civil-card
count; a command that prints our mirror's version of the same handful of public numbers
side by side, prompted once per round, converts a silent corruption into a caught one.
Without it, a 90-minute game can be entirely worthless and nobody notices.

### 6c. How many games do we actually need?

**(a) For evaluation — the answer is "tens, and only for a coarse verdict".**

The naive statistic is win rate in 3-player games (baseline 1/3). Two-sided at 5%, 80%
power: distinguishing "our bot wins 33%" from "our bot wins 50%" — a *huge* effect —
needs ≈ 65 games. Distinguishing 33% from 42% needs ≈ 220. At Tier 1 cost (below) that
is 100 and 350 human-hours respectively. **Win rate is unaffordable at human speed.**

Score margin (our final score minus the best opponent's) is continuous and much cheaper.
Taking a between-game SD of roughly 40–50 points for that margin, detecting a 20-point
shift needs ≈ 40 games; detecting the difference between "competitive" and "hopeless"
(a 40-point shift) needs ≈ 10–12. So:

- **5 games**: tells you whether the mirror + advisor + app loop *works at all*, and
  catches gross engine/eval bugs. Do these first and expect the first two to be thrown
  away.
- **10–15 games**: supports a coarse, honest verdict — "our bot is clearly behind the
  Hard AI" / "roughly level" / "clearly ahead". That is genuinely the resolution we need
  right now, and it is the recommended stopping point.
- **40+ games**: needed for anything finer, e.g. "champion_3p is 15 points better than
  the previous champion". Do not use humans for that. Use self-play arenas
  (`experiments/arena.py`), which give thousands of games for free; the app AI's role is
  to be an *anchor* that self-play cannot provide, and an anchor only has to be located
  once, roughly.

A much better return per hour comes from statistics over **decisions** rather than
games. One game is ~150–250 of our own decisions, so **5–10 games is already thousands
of scored positions** — enough to say "our bot's top pick was one of the top 3 human
picks 71% of the time", to find decision *types* where it is systematically weird (never
starts a wonder before round 5; always keeps 2 military actions unspent), and to
re-score every one of those positions against a future bot. The disagreement catalogue,
not the p-value, is the product.

**(b) For training — the honest answer is "this will never be a training corpus".**

Imitation learning of a policy of TTA's complexity wants order 10⁴–10⁵ labelled
decisions at minimum; the strongest comparable result in the genre (Keldon's Race for
the Galaxy net, §4c) used ~30,000 games of *self-play*, not human games. 10⁴ decisions
of app-AI play is ~50 fully-transcribed Tier-2 games ≈ 150+ human hours, to clone an
agent that §1 argues is *the same architectural class as our own `WeightedBot`*. The
effort/reward is indefensible. If we want a policy target, the literature's answer
(§4d) is TD-learning over self-play, which needs no humans at all.

There is one narrow training-shaped use that *is* affordable: using ~10 games of logged
positions as a **fixed evaluation set for weight vectors** — a held-out board of real,
non-self-play positions on which any candidate weight vector can be scored offline in
seconds. That is a regularizer against hill climbing overfitting to its own population,
and it costs 10 games once.

### 6d. Effort per game — the honest number

Playing a 3-player app game against Hard AIs: **30–45 min** on its own. The advisor adds:

- Our own turns: mostly a single Enter in strict mode, but reading the recommendation
  and mirroring it in the app UI is real time. ~5–10 min/game.
- Opponent turns: this is where the cost lives. The app *does* animate every AI move
  before your turn, so the information is on screen — but transcribing it is roughly
  4–8 patch fields × 2 opponents × ~18 rounds. At a fluent 20–30 s per opponent turn
  that is **12–18 min/game**, and it is dull, error-prone work at exactly the moment the
  human wants to be thinking about their own move.
- Setup, verification passes, resyncs when the mirror drifts, writing the result record:
  ~10 min.

**Tier 1 realistic total: 75–110 minutes per game, i.e. 2–2.5× the cost of just
playing.** Tier 2 is ~3×+ and adds cognitive load that will itself cause errors. Ten
Tier-1 games is therefore a **12–18 hour** commitment for one person, spread over
whatever calendar time they can stand. That is affordable exactly once, for one coarse
verdict — which is why the recommendation is 10–15 games and then stop, not an ongoing
programme.

Failure modes to price in honestly: the first games will desync and be discarded; the
app's DLC/difficulty settings must be checked every single game or the log is
mislabelled; and any human who starts "helping" the bot in strict mode has silently
invalidated the measurement without producing an error message.

## 7. Ranking and recommendation

TODO — under investigation.

---

## Next steps for whoever picks this up

State as of the last commit: §1, §4, §5, §6 written; §2 and §3 under active
investigation; §7 waits on them.

1. **§2 / §3** — if these are still marked TODO, the open questions are listed inside
   each section stub. Both are pure research, no code.
2. **Blocked on a human login** (do not create accounts): the pages that defeated
   unauthenticated fetching are listed inline where they occur — BGO's per-game journal
   view `boardgaming-online.com/index.php?cnt=202&pl=<gameid>` (§5a), BGG forum threads
   and the BGG file section (403 to `curl`/WebFetch), and any Board Game Arena
   replay/archive page noted in §2. If the user logs in to any of these in their normal
   Chrome, a Playwright-over-CDP scrape becomes possible; until then those claims stay
   marked UNPROVEN.
3. **Cheapest unblocked follow-ups**, in order: re-scrape `sources/gamefaqs_75690.txt`
   (currently a Cloudflare challenge page, §5b); pull 5–20k rows of BGO *metadata* (§5a,
   public, no login) purely to calibrate our score distribution; resolve the BGO
   2006-vs-2015 edition question, which decides whether that corpus is worth anything.
4. **Do not** start a network-sniffing or save-file reverse-engineering project against
   the CGE app (§1) without re-reading why it was ruled out.
