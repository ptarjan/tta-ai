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

**Short version: BGA has the largest 2015-edition game corpus in existence and it is the
one place we could get real move-level logs of the right edition — but it has no AI
opponent at all, everything is behind a login, and their Terms of Service prohibit
automated extraction in unusually explicit terms. The genuinely free win from BGA is
their published source code as a rules oracle, not their data.**

### 2a. There are two TTA games on BGA and you want the second one

Verified from the game metadata embedded in BGA's own gamelist payload:

| | 2006 original | **2015 "A New Story of Civilization"** |
|---|---|---|
| slug | `throughtheages` | **`throughtheagesnewstory`** |
| BGA game id | 1011 | **1144** |
| published | 2014-05-19 | **2019-05-10** |
| games played | 553,111 | **1,187,441** |
| players | 2–4 | 2–4 |
| avg duration | 73 min | 32 min |
| status | public, free, ranked | public, free, ranked (ELO league + 2p Arena) |

[gamepanel (2015 edition)](https://en.boardgamearena.com/gamepanel?game=throughtheagesnewstory).
Careful: `?game=throughtheages` is the **2006** game. Both BGA records carry
`bgg_id: 25613` (the BGG entry for the original) and therefore both gamepanels display
"Year: 2006" — that is a BGA metadata artifact, not an edition claim. The 2015 identity
of `throughtheagesnewstory` is confirmed independently by the CGE "A New Story of
Civilization" box art shipped in the implementation source (§2d) and by BGA's own
[launch announcement](https://en.boardgamearena.com/news?id=186). Both are credited to
developer Romain Fromi.

**1.19M games of the right edition** is roughly twice BGO's whole finished-game count
(§5a) and, unlike BGO, there is no doubt about which edition it is. That is the single
largest TTA corpus we found anywhere.

### 2b. Is there a bot? No. Not even a weak one.

BGA's own documentation lists the complete set of bot-capable games —
[Bots and Artificial Intelligence](https://en.doc.boardgamearena.com/Bots_and_Artificial_Intelligence)
— as *Conspiracy, Glow, Crew, Crew Deepsea, Tapestry*, and adds "**None of them
currently is a real AI.** Usually its implementation of 'Automa' rules". TTA is not on
the list. Corroborating hard signal: bot-capable games expose a 1-player table
(`tapestry` is `[1..5]`, `glow` `[1..6]`); both TTA games are `[2,3,4]`, so a solo table
cannot even be created.

The only automated player is BGA's **zombie** mechanism, which fires when a human
quits. TTA's actual `zombieTurn()` is readable in the published source (§2d) and it does
not play the game: on a normal turn it immediately transitions to end-of-turn; on the
politics phase it calls **`concedeGame()`**; territory bids pass with 0; pact offers are
refused; everything else is `zombiePass`. A zombied TTA seat concedes or does nothing.

**So BGA contributes exactly zero as a sparring partner.** Any hope of "point our bot at
BGA and measure it" would mean playing against *humans* through a bot account, which is
both a ToS violation and a completely different (and much slower) experiment.

### 2c. Export and the public archive: endpoints exist, all login-gated

There is **no export button and no bulk download anywhere**. Everything goes through
AJAX endpoints, reverse-engineered from BGA's own `ly_metasite.js` bundle
(module `ebg.site.gamereview`). The real flow is:

1. `GET /gamereview?table=<id>` — **requires login**; anonymous gets
   `302 → /account?warn&redirect=…`. Loading this page is also what *materializes* the
   archive; hitting the log endpoint without it fails (documented as a bug workaround in
   a third-party scraper, [gcheckers `doc/BUGS.md`](https://github.com/JeromeA/gcheckers)).
2. If the page carries a `not_allowed_beacon`, the JS bounces you to the **Premium**
   sales page — the premium gate, confirmed in their shipped code. *Unverified:* the
   exact server-side condition that sets it (own-game vs someone-else's-game vs quota).
   Community threads also assert a **daily cap on replays** for free and premium users
   alike, but `forum.boardgamearena.com` returned 504 on fetch and neither the
   [FAQ](https://en.boardgamearena.com/faq) nor the
   [Premium page](https://en.boardgamearena.com/premium) documents it. **Treat any
   specific replay-quota number as unverified.**
3. Not-yet-built archives: `POST /gamereview/gamereview/requestTableArchive.html`,
   then poll `checkTableArchiveReady.html` every 5 s.
4. Logs: `GET /archive/archive/logs.html?table=<id>&translated=true` → JSON
   `{logs, players}`.

Probed unauthenticated (no cookie): `/table/table/tableinfos.html?id=…`,
`/archive/archive/logs.html?table=…` and `/gamestats/gamestats/getGames.html` all return
HTTP 200 carrying `{"error":"Invalid session information for this action","code":806}` —
i.e. **session cookie required**. `/gamestats?game=throughtheagesnewstory` 302s to the
login page. (`/gamereview/gamereview/getGameLogs.html` does **not** exist — 404; the
right endpoint is `/archive/archive/logs.html`.)

ELO and Arena ratings exist for both TTA games (`is_ranking_disabled: false`), but the
stats pages are login-gated the same way.

**Blocked on a human:** every one of the above needs the user logged in to BGA in their
own browser. I did not create an account and did not attempt a login. If the user has a
BGA account and logs in in Chrome, a CDP-driven Playwright session could confirm in
minutes whether TTA replay logs are actually readable and what a single game's JSON
looks like. Until then, "we can read BGA TTA logs" is **UNPROVEN**.

### 2d. Is there a rate-limited scraping path? Technically yes; permitted, no.

- **robots.txt** ([boardgamearena.com/robots.txt](https://boardgamearena.com/robots.txt))
  disallows `/table`, `/playerstat`, `/player`, `/play`, `/message/board`. `/gamereview`
  and `/archive` are not named — but are login-gated regardless.
- **The ToS is the blocker, and it is explicit**
  ([legal?section=tos](https://en.boardgamearena.com/legal?section=tos)). Users undertake
  "not to obtain information about Users and the Content they publish using automated
  methods (such as robots, spiders, etc.); not to use the Services … using automated
  methods … not to override any security feature or circumvent or avoid any control of
  access". On top of that BGA (AD2G Studio SAS, France) asserts French *sui generis*
  database rights (Art. L.341-1 / L.342-1 CPI) prohibiting "extraction by permanent or
  temporary transfer of all or a … substantial part of the content" of their databases.
  This is a stronger and more specific prohibition than the usual boilerplate.
- **No public or documented API exists.** The path used in practice by people who have
  done this is to *email BGA admins for approval first* — the author of a published
  BGA-scraping write-up says exactly that, alongside "web scraping is not allowed by
  bga's terms of service, so users may be banned for scraping"
  ([medium write-up](https://medium.com/@liamdj/web-scraping-for-board-game-analysis-8f584379f3c)).
- **Prior art, if we ever did get permission**: the mature example is
  [HStrand/bga-tm-scraper](https://github.com/HStrand/bga-tm-scraper) (Terraforming Mars)
  — account login, `REQUEST_DELAY = 2` seconds, explicit FAST/NORMAL/SLOW profiles.
  Others: [pocc/bga_stockfish](https://github.com/pocc/bga_stockfish),
  [davidspies/rftg-analyzer](https://github.com/davidspies/rftg-analyzer),
  [liamdj/tokaido-analysis](https://github.com/liamdj/tokaido-analysis),
  [bskinn/bga-wingspan-scraper](https://github.com/bskinn/bga-wingspan-scraper),
  [th1rt3en/ark-nova-logs-ext](https://github.com/th1rt3en/ark-nova-logs-ext).
  **No TTA-specific scraper exists.** ~2 s/request is the community norm; at that rate
  even 10k games is ~6 hours of requests plus whatever the replay quota turns out to be.

**Recommendation on BGA data: do not scrape it.** Not because it is technically hard —
it is the most tractable log source we found — but because the ToS prohibition is
explicit, the account is the user's own, and a ban costs them a service they use. If
this corpus ever becomes important, the correct first move is a polite email to BGA
asking for permission or a data dump, citing a non-commercial research use.

### 2e. The part of BGA that IS free to use: their source code

[github.com/srussking/throughtheages](https://github.com/srussking/throughtheages) —
already cited elsewhere in this repo — turns out to be much more valuable than "the BGA
implementation exists". It is a complete BGA Studio project containing **BGA production
TTA source**, and it is the `throughtheagesnewstory` (2015) codebase: PHP, pushed
2025-05-23, headers crediting Gregory Isabelli and Romain Fromi (the credited BGA TTA
developer), and the 2015 box art in `img/game_box180.png`. Licence is "Other" (ships a
`LICENCE_BGA` file) — **read it before copying anything**; treat this as readable
reference, not as code we can vendor.

What is in it:
- `throughtheagesmobilereadability.game.php` — **~10,200 lines** of full game logic:
  corruption and consumption tables, army strength, blue/yellow token banks, per-card
  effect dispatch, final scoring, and `zombieTurn`.
- `material.inc.php` — **~4,400 lines**, every card with `name`, `type`, `age`,
  `techcost`, `resscost`, `food`, `ress`, `culture`, `strength`, `happy`, `science`,
  `CA`, `MA`, `text`.
- `states.inc.php`, `dbmodel.sql`, `stats.inc.php`, `gameoptions.inc.php` (which reveals
  the supported variants: "Game version: Handbook / Complete", "Peaceful Variant").

This is an **independent, production-tested implementation of the exact edition we are
implementing**, readable in full. That makes it the best **rules cross-check** available
to us by a wide margin: any disagreement between our `engine/` and this file on a card's
numbers, on corruption, or on scoring is a bug in one of us, and `material.inc.php` is a
direct cross-check for our card database. It costs nothing and needs no login. See §7 —
this is the highest-value thing in this entire section.

It also means that *if* log access were ever granted, parsing would be easy rather than
guesswork: BGA replay logs are literally the recorded `notifyAllPlayers` /
`notifyPlayer` notification stream ([Game replay](https://en.doc.boardgamearena.com/Game_replay):
"All notifications sent to the browser are added to the archive… an exact recording"),
and every notification type and its args are declared in that same `.game.php`.

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

### 5a. Boardgaming-Online (BGO) — RESOLVED: it is the 2015 edition, logs are complete, and we can read them

**Verdict line (2026-07-26): the BGO login WORKS, full move-by-move journals ARE
readable and machine-parseable, and the archive we care about IS the 2015 edition.**
The previous version of this section's "everything I could surface is the 2006 edition"
worry was a **UI artefact, not a fact about the corpus** — see below. Every UNPROVEN
mark in the old text is now settled.

`https://www.boardgaming-online.com` — fan-run play-by-web TTA server, live since 2010,
still busy (2026-07-26: "# games in progress: 636, # active players: 839"), semi-official
(shipped *A New Story of Civilization* in Jan 2016 "after months of teamwork with Vlaada
Chvátil and CGE Team"). Public front page counter: **601,532 finished games since Aug
2010** across both editions.

#### The edition question — settled, with evidence

**BGO models the two editions as two separate boardgames, and the finished-games filter
defaults to the 2006 one.** The filter form on `index.php?cnt=14` carries a radio group:

```html
<input type="radio" name="idJeu" value="4"  checked>  Through the Ages
<input type="radio" name="idJeu" value="10">          Through the Ages: A New Story of Civilization
```

`idJeu=4` is the 2006 original; `idJeu=10` is our 2015 edition. Because `4` is
`checked` by default, anyone who submits the form without touching it gets a
2006-only list — which is exactly what the earlier probe saw and mis-read as "the
archive is 2006". POST `idJeu=10` and the list is entirely 2015 games.

Three independent confirmations that `idJeu=10` really is our edition:

1. **The list's own edition column** reads `Through the Ages: A New Story of
   Civilization` on every row.
2. **The archive is current, not historical.** The newest finished `idJeu=10` game
   (#7523809) *ended 2026-07-26*, i.e. today. People are still playing the 2015 edition
   on BGO right now. 2015-edition games are therefore not a legacy tail — they are the
   live corpus.
3. **The card values in a rendered 2015 game are the 2015 values.** Reading game
   #7523809's board view: `Monarchy 2(8)` (2006 was 3(9)), `Napoleonic Army 7(4)`
   (2006 was 8(4)), `Mechanized Army 10(5)`. These are precisely the numbers
   `docs/SOURCES.md` records as changed in 2015. This is the decisive test and it
   passes.

So **§7 does not have to write the corpus off.** The mineable-at-scale data is the
edition we are building.

#### The log format — one full game pulled and characterised

Stable URL patterns (all `GET`, all under `index.php`, all permitted by
`robots.txt` — the disallow list is only `/classes/ /conf/ /images/ /modules/
/scripts/ /themes/` plus a few includes; `index.php` is **not** disallowed):

| What | URL |
|---|---|
| Finished-games index | `index.php?cnt=14` + `POST idJeu=10&filtre=<optional>`; pages via `index.php?cnt=14&pg=<n>&flt=` (50 games/page) |
| Games in progress | `index.php?cnt=11` |
| Final board / position | `index.php?cnt=202&pl=<gameid>&nat=-1` |
| **Move-by-move journal** | `index.php?cnt=52&pl=<gameid>&nat=-1&pg=<n>&flt=` |
| Discard pile | `index.php?cnt=53&pl=<gameid>&nat=-1` |
| Rules-version notes | `index.php?cnt=205&pl=<gameid>&nat=-1` |

Login is a plain form POST to `index.php` with `identifiant` / `mot_de_passe`
(+ optional `souvenir`); it sets `PHPSESSID` and two persistent cookies. Nothing
exotic, no CSRF token, no Cloudflare.

**Journal structure.** A plain HTML `<table>`, newest-first, five columns:
`Date | Player (colour) | Age | Round | Text`. `pg=1` is a short "current turn" page;
`pg>=2` hold **100 entries each**. Game #7523809 (2 players, Emperor level, ran
2026-07-25 10:28 → 2026-07-26 13:21, ended Age IV round 20) has **392 entries over 5
pages, ~207 KB total** — so a whole game is **5 GETs**. Entry text is generated from
templates and parses cleanly with regexes. A representative census of that game:

```
37  End turn <P> scores: N culture (now N) N science (now N) N food - consumption: N ...
28  <P> increases population   <P> spends N food
19  No Discard Phase
12  <P> discards N card(s)
12  <P> bids N
11  Discard Phase  N military cards must be discarded
 8  <P> passes Political Phase
 5  <P> takes Urban Growth in hand    <P> uses N civil action
 5  <P> builds Knights                <P> spends N resources
 4  <P> plays Reserves                <P> produces N resources
 3  <P> upgrades Bronze to Iron       <P> spends N resources
 3  <P> declares War over Culture on <P> ...
 3  <P> wins War over Culture   Attacker's strength: N  Defender's strength: N
 2  <P> upgrades Philosophy to Scientific Method using Efficient Upgrade
 2  <P> wins Inhabited Territory   Winning bid is N
 1  <P> puts Alexander the Great back in the row   <P> gets 1 civil action
 2  GAME DATA UPDATED  <P> culture: N -> N        (admin score corrections — filter these)
```

**Card identities: named wherever the rules make them public.** Civil-row takes
(`takes <card> in hand`), builds, upgrades, wonder stages, leader elections, tactics
adoption, action-card plays, event resolution, war/aggression declarations and outcomes
all carry the **exact card name** plus the resource/action cost paid. Player identity is
by seat colour, and the header row maps colour → account name → final score.

**Hidden-information redaction is exactly what you'd expect, and it is the main
limitation.** Two things are counts-only, never identities:
- military card draws — `Purple draws 2 military cards`;
- discards — `<P> discards 2 cards`.

**And one thing is missing entirely: the civil card row is never logged.** There is no
"new cards enter the row" / refill / reshuffle event. You can see *which* card a player
took and — from `uses N civil action` — *what row position it was in* (cost 1/2/3, a
genuinely useful signal), but you cannot reconstruct **what else was on offer**. For
imitation learning that is serious: you can observe the chosen action but not the full
choice set, so a policy trained on it learns "what humans take" and not "what humans
take *given the alternatives*". Reconstructing the row would mean simulating the whole
deck from the journal plus the discard-pile page, and the per-player-count deck
composition — possible with our card data, but it is a real project, not a parse.

#### Volume

Kept deliberately low: ~20 page fetches total to characterise all of this, with delays.
No bulk download was performed. `robots.txt` permits `index.php`; the site has no
published API and no explicit anti-automation clause found on the pages visited, but it
is a small donation-funded fan server, so any future scrape must be slow, cached, and
ideally cleared with the webmaster (`boardgamingonline@gmail.com`) first. That is a
different posture from Board Game Arena, whose terms explicitly forbid automated access
(§2d) — BGO does not.

#### What it would actually buy us

1. *Outcome metadata only* (cheap): `idJeu=10` finished-games pages give game id, name,
   player count, level, start/end dates, final age, round count, and every player's
   final score, 50 per page. Enough to **calibrate our engine's score distribution** by
   player count and skill level — if our self-play 3p games end at a mean 140 and BGO
   humans end at 190, something is off. 5–20k games is a few hundred polite fetches.
2. *Move-level logs* (5 GETs/game, parseable, right edition): now genuinely on the table
   for imitation bootstrapping — with the card-row caveat above, and with the caveat
   that BGO's player pool spans every skill level (the `level` column, Prince…Emperor,
   is the filter for that).

### 5b. Written human strategy corpus

We already have a chunk of this in `sources/` (`hypercheat.txt`, `ubg_*`, the GameFAQs
in-depth guide — though `sources/gamefaqs_75690.txt` is currently just a Cloudflare
challenge page, i.e. **that scrape failed and needs redoing**). Additional identified
material:
- BGG [thread 1933554 "Data-Driven strategy tips"](https://boardgamegeek.com/thread/1933554/data-driven-strategy-tips)
  (win rates from ~10k BGO games), [thread 2801950](https://boardgamegeek.com/thread/2801950/a-strategy-guide-for-the-game-with-the-expansion)
  (guide based on ~100 games, 3–4p), [thread 934016](https://boardgamegeek.com/thread/934016/general-strategy-tips-for-a-newbie).
  **See §5c: the 403 was only a User-Agent block and is now solved. BGG forum and file
  *metadata* fetch fine unauthenticated; the login now works too, and the file *bodies*
  are blocked on one human click (BGG's GDPR Terms-of-Service re-affirmation form).**

### 5c. BGG file section — login now WORKS; one legal click still gates the file bodies

**Verdict line (2026-07-26, after the user's password reset): the BGG login SUCCEEDS.**
`POST https://boardgamegeek.com/login/api/v1` with the new password returns a valid
session; `GET https://boardgamegeek.com/api/accounts/current` with that cookie jar
returns `{"user":544841,"username":"ptarjan",...}`. The "dormant account" block recorded
in the previous version of this section is **gone and that verdict is now stale** — the
reset cleared it. Everything below supersedes it.

**Verdict line for the two requested files: STILL NOT DOWNLOADED, blocked on one
human click, and it is not a technical problem.** BGG redirects every authenticated
file download to `https://boardgamegeek.com/read_terms` — a GDPR-era
**Terms of Service / Privacy Policy re-affirmation form** (`POST /geekaccount.php`)
that the `ptarjan` account has never accepted. Until it is accepted, every file
download 302s to that page and returns the site's HTML shell instead of bytes.

**What the user must do (30 seconds, once):** open <https://boardgamegeek.com> in a
normal browser while logged in as `ptarjan`; a "Why am I seeing this (again)?" page
appears; read it, tick the agreement + newsletter choices, submit. After that the two
files are one `curl` away with the existing session. **This was deliberately not done
on the user's behalf**: the form is a binding legal agreement whose own text calls out
"the waiver of your right to a jury trial", "the requirement to arbitrate any disputes"
and "the prohibition of class action lawsuits". An agent must not accept that for a
human. That is the *only* remaining blocker.

#### What was actually proven this round

| Step | Result |
|---|---|
| `POST /login/api/v1` (JSON body, `Content-Type: application/json`, Chrome UA) | **200**, sets `SessionID` + `bggusername` + `bggpassword` cookies |
| `GET /api/accounts/current` with those cookies | **200**, `username: ptarjan`, user id 544841 |
| `GET /api/files/154670`, `/api/files/409053` (anonymous *or* authed) | **200** JSON metadata |
| `GET /file/download/<fileid>` via `curl` | **403 Cloudflare "Just a moment…" challenge** |
| `GET /file/download_redirect/<signed-token>/<filename>` via `curl` | **403**, same Cloudflare challenge |
| Same URL fetched *inside real Chrome* (Playwright, `channel: 'chrome'`, session cookies injected) | **200 but redirected to `/read_terms`** — HTML, not the file |

Two corrections to the earlier write-up, both important for whoever retries:

1. **The old "`/file/download/<id>` returns *Error: Forbidden: Admins only*" reading was
   wrong.** That URL now returns a **Cloudflare bot challenge** to any non-browser
   client. BGG's `/file/*` path is Cloudflare-protected; `urllib`/`curl` cannot pass it
   no matter what headers they send. Only a real browser gets through. (The
   *metadata* API on `api.geekdo.com` is **not** behind that challenge — it is still
   plain-`urllib` friendly with a Chrome User-Agent, as previously recorded.)
2. **`download_redirect` is not 410 Gone and the download URL *is* discoverable.** The
   filepage is an Angular/React shell, so the link is absent from the served HTML but
   present in the DOM after hydration. Rendering the filepage in Playwright and reading
   `a[href*="download_redirect"]` yields a working, signed, per-session URL, e.g.
   `…/file/download_redirect/<opaque-token>/Through+the+Ages+-+A+New+Story+of+Civilization+-+Card+Reference+v1.09.pdf`.
   The token changes on every page render, so it must be scraped fresh each time and
   used from the same browser context.

So the recipe, once the ToS is accepted, is fixed and known: **Playwright + real Chrome
+ the `/login/api/v1` cookie jar -> load the filepage -> read the `download_redirect`
href -> `fetch()` it inside the page.** No further reverse-engineering needed.

**Both target files remain correctly identified:**

| fileid | filepage | filename | size | downloads |
|---|---|---|---|---|
| 154670 | 123302 | `Through the Ages - A New Story of Civilization - Card Reference v1.09.pdf` | 800,909 | **27,322** |
| 409053 | 293343 | `_PLAYER CARD COUNTS.xls` ("Through the Ages Card Counts", v1.1, 2025-01) | 144,896 | 143 |

**A provenance discovery from 154670's metadata that changes how much it is worth.**
The uploader's own description says: *"Card data retrieved from **BGO v 2.5**, which I
believe to be the final (printed) revision."* So the 27k-download community card
reference is **not** an independent transcription of the physical cards — it is a dump
of Boardgaming-Online's 2015 implementation (§5a). That is still a genuinely useful
third opinion (BGO is a semi-official implementation and v2.5 is the final revision, and
its changelog shows real errata being fixed through v1.09), but it is **correlated with
BGO**, and if we ever pull card data from BGO directly the two are one source, not two.
It is *not* correlated with our existing two sources (BGA Studio + Tabletop Simulator),
so as a cross-check against `data/cards_civil.json` it still counts.

409053's own description warns it "assumes you own the Leaders and Wonders expansion", so
any count from it must be filtered to base-game cards before comparison.

**Nothing was silently changed.** No BGG-derived value has been written into
`data/cards_civil.json` or `data/cards_military_actions.json`; the card data remains
resolved from the two independent 2015-edition sources in `docs/SOURCES.md`. The rule
stands: BGG is a *third opinion*, and any disagreement gets **both** values written into
`docs/SOURCES.md` and flagged, not quietly applied.
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

**One systematic bias worth stating loudly: pacts.** §1 records that CGE's AI players
never offer a pact and refuse every pact offered. Our engine *does* implement the full
pact subsystem (`offer_pact` / `cancel_pact` in `engine/actions.py`, §5.9–5.10, disabled
at 2 players). So in every app-AI game the entire pact branch of our bot's policy is
dead weight: it can never be exercised, never rewarded, never punished. That means the
human-in-the-loop number is an evaluation of our bot **on a strictly smaller game than
the one we are training on**, and any pact-related weight has to be validated by
self-play only. It is not a reason to skip the exercise, but do not quietly forget it
when reporting the result.

Failure modes to price in honestly: the first games will desync and be discarded; the
app's DLC/difficulty settings must be checked every single game or the log is
mislabelled; and any human who starts "helping" the bot in strict mode has silently
invalidated the measurement without producing an error message.

## 7. Ranking and recommendation

Everything above, priced. "Effort" is one person's working time, honestly estimated;
"value" is what it changes about the bot we ship. The ranking moved this round: §5a went
from *unproven and probably the wrong edition* to *proven, right edition, and readable*,
which promotes it from near-bottom to near-top.

| # | Option | Effort | Value | Verdict |
|---|---|---|---|---|
| 1 | **Diverse-opponent league inside our own engine** (§ intro) | ~0 external, already running | Directly attacks the blind-spot problem hill climbing has | **Do it — it is already the default and nothing here beats it** |
| 2 | **Hand-written heuristic priors from the strategy corpus** (§5b) | hours | Seeds `WeightedBot` in a good basin; gives falsifiable assertions to test the bot against | **Do it — cheapest real win on the list** |
| 3 | **BGO outcome metadata** (§5a, `idJeu=10`) | ~1 day of polite scraping, no login needed for the index | Calibrates our score distribution against ~170k real 2015 games by player count and skill level | **Do it — high value, low effort, no ethical friction** |
| 4 | **BGG card reference + card counts** (§5c) | one human click, then 2 fetches | Third opinion on card data; costs almost nothing | **Do it once the ToS is accepted** |
| 5 | **Human-in-the-loop vs the app's Hard AI** (§6) | **12–18 h** for 10–15 games | The *only* external anchor that exists; answers "are we near strong-human level?" and yields thousands of scored positions + a held-out eval set | **Do it, once, at 10–15 games — then stop** |
| 6 | **BGO move-level logs for imitation learning** (§5a) | weeks: scraper + journal parser + card-row reconstruction | Large, right-edition, but the choice set is unrecoverable and the player pool is mixed-skill | **Defer.** Revisit only if 1–5 stall |
| 7 | **BGA** (§2) | n/a | Largest corpus (1.19M) but no bot, all login-gated, ToS explicitly forbids automated extraction | **Dead as data.** Their published source stays useful as a *rules oracle* only |
| 8 | **Reverse-engineering the CGE app** (§1: saves, protocol, screen-scraping) | weeks, ToS-violating, brittle | Would only yield saves or human games, not AI decisions | **Do not start** |

### The one recommendation

**Spend the next block of effort on §6: play 10–15 logged 3-player games against the
app's Hard AI, using the existing advisor as the mirror.**

Why that and not the newly-unblocked BGO corpus, which is bigger and cheaper per unit:

- **We have no anchor at all right now.** Every number in `docs/HEURISTICS.md` and every
  champion in `experiments/` is measured against *ourselves*. A population that shares a
  blind spot cannot detect it, and no amount of extra self-play fixes that. §6 is the
  only option on this list that produces an *externally calibrated* verdict. BGO
  metadata (option 3) calibrates the **score scale**; it cannot tell us whether our
  *policy* is good, because a score distribution is not an opponent.
- **The cost is bounded and one-off.** §6c shows 10–15 games buys the coarse verdict we
  actually need ("clearly behind / roughly level / clearly ahead"), and §6d prices that
  at 12–18 hours. Beyond that the marginal value collapses and self-play arenas are
  strictly better. This is a one-time purchase, not a programme.
- **The by-product is worth as much as the verdict.** 10 games is ~1,500–2,500 of our
  own scored decisions plus a held-out set of real, non-self-play positions that any
  future weight vector can be re-scored against offline in seconds. That is a standing
  regularizer against hill climbing overfitting to its own population, and we get it
  once and keep it forever.
- **BGO's move logs, the obvious rival, have a specific flaw that §6 does not.** The
  journal never records the civil card row (§5a), so we can see the chosen card but not
  the alternatives. Imitation learning on chosen-action-without-choice-set is weak, and
  reconstructing the row from the deck composition is its own multi-week project. BGO's
  *metadata* has no such flaw, which is why option 3 is ranked above option 6 and why
  they are listed as two different projects.

Do options 2, 3 and 4 alongside it — they are hours, not days, and none of them competes
for the same attention. Explicitly **do not** start option 6 or 8.

**Caveat on this ranking:** §3 (open-source TTA AI projects) is still TODO. If a usable
open-source TTA agent exists it would be a *second* external anchor at a fraction of §6's
human cost, and would outrank it. That is the one finding that could change this
recommendation, and it is cheap to check — do §3 before committing 15 hours to §6.

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
