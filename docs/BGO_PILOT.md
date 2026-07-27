# BGO reachability pilot (no-login recon), 2026-07-26

Scope: a feasibility check, not a scrape. Goal was to determine what
Boardgaming-Online (BGO) exposes to a completely anonymous client — **no login was
attempted, no credentials were read, used, or created** — and whether
`tools/scrape_bgo.py` still matches the live site. Total network requests made: **8**
(1 `robots.txt` + 7 page fetches), single-threaded, ≥2 s apart, well under the 30-page
budget. No POST submitted anything except an anonymous, credential-free form
resubmission of the finished-games filter (`idJeu=10`, no login fields).

**Headline finding, and it corrects a premise in `docs/EXTERNAL_AIS.md` §5a: the
finished-games index is NOT actually readable without login.** Only aggregate counters
and page-shell chrome are public. Every place actual row data would appear — the
finished-games table, an individual game's board, an individual game's journal — is
gated. `docs/EXTERNAL_AIS.md`'s "login WORKS, journals are readable" conclusion is still
correct as far as it goes, but it was reached with an authenticated session throughout;
it does not establish (and this pilot shows it is false) that the same data is visible
**without** logging in.

## 1. robots.txt

```
User-agent: *
Disallow: /classes/
Disallow: /conf/
Disallow: /images/
Disallow: /modules/
Disallow: /scripts/
Disallow: /themes/
Disallow: /deconnexion.php
Disallow: /entete.php
Disallow: /favicon.ico
Disallow: /footer.php
Disallow: /head.php
Disallow: /hors_ligne.php
Disallow: /pied.php
```

`index.php` (the only endpoint anything in this pilot or in `tools/scrape_bgo.py`
touches) is **not** disallowed. Everything fetched below was permitted by robots.txt.
No crawl-delay directive is present; this pilot used a 2 s minimum gap by its own rule,
not because the site asked for one.

## 2. Finished-games index without login: shell yes, rows no

Three anonymous fetches against `index.php?cnt=14` (the "Finished games" page):

- Plain `GET index.php?cnt=14` (no form submission at all): 200, 4055 bytes. Shows the
  login form, the nav menu, and `# finished games: 601537` — but **no table, no
  pagination, no edition radio buttons at all**. This is just the landing shell.
- Anonymous `POST idJeu=10&filtre=` to the same URL (i.e., submitting the visible filter
  form with no session cookie, no credentials): 200, 4562 bytes. This time pagination
  chrome appears — `Pages: 1 - 2 - 3 ... 12029 - 12030 - 12031` (12,031 × 50 ≈ 601,550,
  matching the *combined* both-editions total, so the `idJeu` filter itself doesn't even
  visibly take effect for an anonymous poster) — but the actual `<table class="tableau2">`
  is **empty**. Zero `<tr>` rows of game data.
- `index.php?cnt=11` ("All games in progress"): same pattern — `# games in progress: 636`,
  `# active players: 837` are shown, but again no row data, same empty-shell structure.

**Conclusion: the aggregate counters are public; the per-game rows are not.** Whatever
produces the actual `<tr>` game rows on these pages checks for a logged-in session
server-side and simply renders nothing when there isn't one. This directly falsifies the
"the game index is public and needs no login" premise this task started from — that
premise must have been inferred either from `docs/EXTERNAL_AIS.md`'s (always
authenticated) testing, or from the front-page counters alone.

## 3. Individual game journal / board without login: fully gated, distinct failure mode

Anonymous fetches, all against the well-known sample game id `7523809`
(`sources/bgo_journal_7523809.tsv`), which is known to exist and to have finished:

- `index.php?cnt=52&pl=7523809&nat=-1&pg=1&flt=` (journal, page 1): 200, 3394 bytes.
- `index.php?cnt=52&pl=7523809&nat=-1&pg=2&flt=` (journal, page 2): 200, 3394 bytes
  (byte-identical structure to pg=1).
- `index.php?cnt=202&pl=7523809&nat=-1` (final board): 200, 3394 bytes, same structure.

All three return the *same* page body: **`<p class="important">The game does not
exist</p>`** in place of any content. This is a harder gate than an empty table — the
server does not even acknowledge the game id exists for an anonymous requester. It is
indistinguishable, from outside, whether that's "no session → no game lookup at all" or
"anonymous users see nothing, full stop"; either way the practical answer is the same.

**So: index-public-but-journals-gated is *not* quite what happens — it is stricter than
that.** Nothing at the per-game level (list row, board, journal) is visible anonymously.
Only the site-wide aggregate counts and navigation chrome are public. This is exactly
the "index-public-but-journals-gated" shape the task asked me to watch for, except the
gate sits one level higher than expected: even the *list rows* of the index are behind
it, not just the journals.

This is a hard stop per the task's rules: **journals (and, it turns out, index rows)
require authentication, and I did not attempt to log in or use any credential to get
past that.** That is the finding, not a problem to route around.

## 4. Does `tools/scrape_bgo.py` still work?

Partially verifiable without logging in, and nothing found is bitrotted:

- **HTML template structure is unchanged.** `_cells()` and the `<tr>...</tr>` row
  regexes in the script were run offline (no network calls) against the anonymous HTML
  fetched above, and they parse cleanly — they just correctly report zero data rows,
  because there are none to find in an anonymous response. The parser is not the reason
  nothing came back.
- **Login form field names are still correct.** The live page still has
  `name="identifiant"`, `name="mot_de_passe"`, `name="souvenir"` exactly as
  `Session.login()` posts them.
- **The one thing this pilot explicitly could not test: whether login itself still
  succeeds.** That requires a real credential, which the task forbids and which does not
  exist on disk (deliberately deleted). So "does the script still work end-to-end" is
  **unproven either way** — the parts I could check without a password are intact; the
  authentication step is untested by design, not because it looked broken.

## 5. Cost of a full polite scrape (if it were reachable)

Using the counts already on record in `docs/EXTERNAL_AIS.md` §5a (≈178,270 finished
2015-edition games, ≈3,566 index pages at 50 games/page, ≈5 journal-page GETs per game,
sample game = 392 rows / ~207 KB raw HTML / 41.8 KB parsed TSV for a 20-round 2-player
game):

| Target | Requests | Time at ≥2 s/request (single-threaded) | Disk |
|---|---|---|---|
| Finished-games **metadata only** (all pages) | ~3,566 | ~2 h minimum (2 s × 3,566); realistically 3–4 h with page-render/parse overhead | tens of MB of structured rows |
| Full **move-by-move journals**, all ~178k 2015 games | ~178,000 games × 5 GETs ≈ 890,000 | ~2 s/request → ~495 h ≈ **~21 days** of continuous single-threaded fetching; more realistically 3 s/request accounting for latency → ~30 days | raw HTML ≈ 178,000 × 207 KB ≈ **~37 GB**; parsed TSV only ≈ 178,000 × 42 KB ≈ **~7.4 GB** |

Both of these are moot until the login/auth question above is resolved with the user's
own action (not this pilot's) — nothing here can proceed further without a credential
this task explicitly forbids touching.

## 6. Data quality for value-function training, if the gate were passed

Judged from the one sample journal (`sources/bgo_journal_7523809.tsv`, 392 rows) and
`docs/EXTERNAL_AIS.md` §5a's census of it, on the specific question: **can a full state
trajectory with a final outcome be reconstructed from journal text alone?**

What **is** recorded, in clean template-parseable form:
- Every civil-row take, by exact card name, plus which action-point cost tier it was
  taken at (`uses 1/2/3 civil action` — a real signal about row position even without
  seeing the row itself).
- Builds, upgrades (`upgrades X to Y using ...`), wonder stage construction, tactics
  adoption, leader election, action-card plays, population increases with food spent,
  resource production, event resolution and its effects, war/aggression declarations and
  outcomes with attacker/defender strength, territory bids and winners, pact-adjacent
  political-phase passes, and the full per-turn score breakdown (culture / science /
  food / consumption / resources) at every `End turn`.
- Final scores per player (from the header/index row, separately from the journal body).

What is **not** recoverable:
- **The civil card row itself.** No "cards enter the row" / refill event exists anywhere
  in the journal. You see which card a player took, never what else was on offer. This
  is exactly the caveat the task flagged: it kills imitation learning (you can't see the
  choice set) but does **not** touch value learning, since a value function only needs
  states-and-outcomes, not chosen-vs-rejected alternatives.
- Military card **identities** — draws and discards are counts only (`draws 2 military
  cards`, `discards 2 cards`), never which cards. This matters more for value learning
  than the civil-row gap does: a player's military-card hand is part of their true state,
  and it is permanently hidden here. Any reconstructed trajectory has to treat "N unknown
  military cards" as a latent/opaque quantity rather than a concrete hand — a state
  encoding gap, not just a "can't imitate" gap.

**Net judgment:** a trajectory of *public* state (culture, science, food, resources,
tech tree, wonders, army strength, territory/war outcomes) plus final score **is**
reconstructable and does parse cleanly with regexes against the templated text — the
single sample supports that. It would train a value function over the *observable*
state only, with military hand composition marked unknown/latent rather than omitted
silently. That is a real but partial state representation, not a full one, and it is
moot regardless until the login gate found in §2–3 is resolved by the user, since none
of this data is reachable by the no-login constraint this task operates under.

## Bottom line

1. `robots.txt` permits `index.php`; nothing fetched here was disallowed.
2. The finished-games **index** is not readable without login beyond an aggregate
   counter and empty pagination shell — no game rows render anonymously. This corrects
   the task's starting premise.
3. Individual game **journals and boards** are fully gated — anonymous requests get
   "The game does not exist" regardless of a known-good game id. This is a harder gate
   than "index public, journal gated"; the index rows are gated too.
4. `tools/scrape_bgo.py`'s HTML parsing logic and login form field names are unchanged
   from the live site; whether the login step itself still succeeds is **untested by
   design** (no credential was read, created, or used, per the hard limit).
5. A full journal scrape, if ever unblocked, is a multi-week (~3–4 weeks
   single-threaded, polite-rate) project at ~37 GB raw / ~7.4 GB parsed; the metadata-only
   index scrape is ~3–4 hours and tens of MB — cheap by comparison, but per §2 it is
   **also** behind the same login gate, contrary to what was assumed going in.
6. Data quality, if unblocked: journals record enough public state (culture, science,
   food, resources, tech, wonders, war outcomes, final score) to reconstruct a usable
   value-learning trajectory in parseable form; the civil row is unrecoverable (kills
   imitation learning, not value learning, as expected) and military hands are
   permanently opaque counts (a real gap in state completeness, worth flagging alongside
   the civil-row one). All of this remains gated behind a login this pilot correctly did
   not attempt.
