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

## 5. Human strategy corpora

TODO — under investigation.

## 6. The human-in-the-loop option (play the app, log the AI)

TODO — under investigation.

## 7. Ranking and recommendation

TODO — under investigation.
