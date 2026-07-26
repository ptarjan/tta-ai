# Sources and Provenance (TTA base 2015 data)

Status: DRAFT — being refined incrementally.

## Source ranking
1. `/Users/pt/tta-ai/sources/1j1ju_rulebook.pdf` (= docs/rulebook.pdf) — official 2015 rulebook (24 pp). AUTHORITATIVE for rules, setup counts, turn structure.
2. `/Users/pt/tta-ai/sources/faq_v15.pdf` — official FAQ v1.5 (16 pp). Authoritative clarifications.
3. `namu_*.txt` — NamuWiki card lists (machine-translated from Korean). Primary card-stat source. These pages cover BOTH the 2006 "old version", the 2015 "new edition", and the expansion; only "new edition" (or unchanged "old version" when no new-edition entry exists) values were used. Expansion-marked and Korean-edition-exclusive cards were EXCLUDED (see below).
4. `ubg_*.txt` — UltraBoardGames paraphrase (secondary, rules cross-check).
5. `hypercheat.txt` — 2006 ORIGINAL edition cheat sheet. Used only as tiebreaker; 2006-vs-2015 differences noted below.
6. `vassal_tta.html`, `cge_tta.html`, `fandom_allpages.html` — misc; used for card counts where needed.
7. `sources/bga_throughtheages_material.inc.php` + `sources/bga_card_counts.tsv` — the Board Game Arena Studio implementation of the **2015** edition (github.com/srussking/throughtheages). Every one of its 247 cards carries `qt2`/`qt3`/`qt4` (copies at 2/3/4 players) plus costs and rules text. AUTHORITATIVE for per-card copy counts: its civil deck totals come out at exactly 20 / 44-50-53 / 44-50-53 / 44-50-53 and it reproduces the rulebook's "3 cards marked 4 + 6 cards marked 3+ per civil deck" exactly.
8. `sources/tts_tta_workshop_2120085710.json` — Tabletop Simulator workshop mod for the 2015 edition. Its civil decks are pre-split into `Civil_N`, `Civil_N (3+)` and `Civil_N (4)` piles, i.e. it states the corner marks directly. Independent confirmation of #7 (they agree card-for-card). Ignore its `(NWL)` decks — those are the New Leaders and Wonders expansion.
9. `sources/vassal_NewTTA_2.49.vmod` (+ extracted `..._buildFile.xml`, `..._card_counts.tsv`) — Vassal module. NOTE: despite the "NewTTA" filename this is the **2006** edition (Ideal Building Site / Mineral Deposits / Bountiful Harvest / Work of Art); no Vassal module exists for the 2015 edition. Historical cross-check only.

## Third opinion — BGG file section (OBTAINED 2026-07-26; NOT applied to the data)

The user accepted BGG's GDPR Terms-of-Service re-affirmation, which was the only remaining
blocker, and both files are now downloaded and **format-verified by magic bytes, not by
filename** (an earlier attempt produced a 131 KB HTML page named `.xls`):

| BGG fileid | saved as | verified | bytes |
|---|---|---|---|
| 154670 | `sources/bgg_154670_card_reference_v109.pdf` *(gitignored: `sources/*.pdf`)* | `25 50 44 46` = **%PDF-1.5**, 4 pages | 800,909 (exactly BGG's advertised size) |
| 409053 | `sources/bgg_409053_player_card_counts.xls` | `d0 cf 11 e0` = **OLE2 / Excel 97-2003** | 144,896 (exactly BGG's advertised size) |

Retrieval recipe (Cloudflare + a signed one-shot S3 URL): `tools/scrape_bgg_files.mjs`,
explained in `docs/EXTERNAL_AIS.md` §5c.

**The Card Reference PDF has no text layer.** All 4 pages are single RGB images
(1141×904 etc. at 118–144 ppi) with one embedded font used for nothing extractable —
`pdftotext` yields 4 bytes (form feeds only). It is a screenshot of a spreadsheet, so it
is readable only by eye/OCR (`pdftoppm -r 200 -png`, then read the images).

An earlier revision of this file guessed that it therefore "carries no information the
`.xls` does not already carry" and skipped it. **That guess was wrong and it mattered.**
Page 1 carries its own `2p 3p 4p` copy-count columns — a *fourth* independent count
opinion, by a different author from the `.xls` — and where the two BGG files disagree,
this one sides with us. See Conflict B below.

### Independence caveat (unchanged, and it matters for the verdict)
154670's uploader states *"Card data retrieved from **BGO v 2.5**, which I believe to be
the final (printed) revision."* It is a transcription of **Boardgaming-Online's** 2015
implementation, not of physical cards. Independent of sources #7 (BGA Studio) and #8
(TTS); **not** independent of any future BGO pull. 409053 (`_PLAYER CARD COUNTS.xls`,
by "Larry Schneider") carries no provenance statement at all.

### Result: the card *rosters* agree perfectly; **9 copy-counts disagree**

Keyed by (name, age) over civil technologies + governments + wonders + leaders + yellow
action cards, **both sides have exactly 121 entries and the sets are identical** — no card
in one that is missing from the other. Only two naming variants (BGG "Ocean Liner Service"
= our "Ocean Liners"; BGG "Stockpile" = our "Stock Pile"; BGG also misspells
"Consitutional Monarchy"). Every tech cost and build cost that both sides state agrees.
**Nothing was changed in `data/`.** Both values are recorded here per the standing rule:

| Card | Age | Group | **Ours** (2p/3p/4p) | **BGG 409053** (2p/3p/4p) |
|---|---|---|---|---|
| Rich Land | I | action | 2 / 2 / 2 | 2 / **3** / **3** |
| Frugality | II | action | 1 / 1 / 1 | 1 / **2** / **2** |
| Urban Growth | II | action | 1 / 1 / 1 | 1 / **2** / **2** |
| Patriotism | III | action | 1 / 1 / 1 | 1 / **2** / **2** |
| Reserves | III | action | 3 / 3 / 3 | 3 / **4** / **4** |
| Revolutionary Idea | III | action | 2 / 2 / 2 | 2 / **3** / **3** |
| Republic | II | government | 1 / 1 / 2 | 1 / **2** / 2 |
| Professional Sports | III | urban (arena) | 1 / 1 / 2 | 1 / **2** / 2 |
| Air Forces | III | military (air) | 2 / 2 / 3 | 2 / **3** / 3 |

These are two different disagreements, and they deserve different verdicts.

**Conflict A — six action cards, +1 copy each at BOTH 3p and 4p. BGG is almost certainly
wrong; keep ours.** It changes the size of the physical deck, and it breaks the published
component count:

| civil deck totals | Age A | Age I | Age II | Age III | total |
|---|---|---|---|---|---|
| ours (4p = every card) | 20 | 53 | 53 | 53 | **179** |
| BGG 409053 (4p) | 20 | 54 | 55 | 56 | **185** |

czechgames.com's own component list says **179 civil cards** (and 150 military), which our
numbers hit exactly and BGG's miss by exactly those 6 action cards. BGG's own sheet even
prints "Total Cards: 353" beside these tables, which is not 185 + 150 either.

**Conflict B — three cards marked "3+" by BGG 409053 where we mark them "4". The OTHER
BGG file settles it in our favour.** Page 1 of the Card Reference PDF (154670) tabulates
54 civil technologies, governments and special techs with explicit `2p 3p 4p` columns.
Transcribed by eye and diffed against `data/cards_civil.json`, **all 54 rows agree with
us**, including the three cards in dispute:

| Card | ours | BGG **154670** (PDF) | BGG 409053 (xls) |
|---|---|---|---|
| Republic (II) | 1 / 1 / 2 | **1 / 1 / 2** ✔ | 1 / 2 / 2 ✘ |
| Professional Sports (III) | 1 / 1 / 2 | **1 / 1 / 2** ✔ | 1 / 2 / 2 ✘ |
| Air Forces (III) | 2 / 2 / 3 | **2 / 2 / 3** ✔ | 2 / 3 / 3 ✘ |

(The only rows that "differ" are the six Age A starting cards — Agriculture, Bronze,
Philosophy, Religion, Warriors, Despotism — where the PDF prints 2/3/4, i.e. one per
player, and we store 0 because they are dealt at setup and never sit in a deck. That is a
convention difference, not a data conflict.)

So the "third opinion" was never one opinion: **the two BGG files are two opinions and
they split, 154670 with us and 409053 against us.** 409053 is also the one with no
provenance statement at all. Independently of that, the structural argument below already
condemned 409053's 3-player column:

This does not
change the 4-player deck at all; it only changes what a 3-player game removes. Here the
decisive evidence is that the marks come in a fixed pattern. The rulebook's setup says to
remove *the 6 cards marked "3+" and the 3 cards marked "4"* from each of the Age I/II/III
civil decks. Our data reproduces that pattern **exactly and uniformly — 6 and 3 in every
one of the three ages**. BGG's does not, and cannot:

| per age | ours "3+" / "4" | BGG "3+" / "4" |
|---|---|---|
| Age I | 6 / 3 | 7 / 3 |
| Age II | 6 / 3 | 9 / 2 |
| Age III | 6 / 3 | 11 / 1 |

A 7/9/11 split contradicts the printed setup instruction, so BGG's 3-player column is
unreliable. (Both sides agree on the same three "4"-marked cards in Age I — Knights,
Alchemy, Iron — which is a good sign the underlying card set is the same and only the
mark transcription drifted.)

**Verdict: no change to `data/`.** Our BGA-Studio + TTS values survive the cross-check
on every one of the 121 cards where a physical-count anchor exists, and the 9 exceptions
are all cases where BGG contradicts either the 179-card component list or the rulebook's
own 6-and-3 removal rule. `python3 data/validate_cards.py` still passes. This is a
**confirmation**, not a correction — which is itself the useful result, since the four
earlier corrections all came from this kind of conflict.

### The cross-check was NOT finished: the `.xls` has a second sheet, and it found a real bug

**✅ RESOLVED 2026-07-26 — the bug was real and the fix is now applied to `data/`. See
"Verdict" at the end of this section for the second, independent confirmation. The text
immediately below is the original flag, kept for the record; where it said "not applied",
it now is.**

`bgg_409053_player_card_counts.xls` has **two** sheets, `Civic Cards` (129 rows) and
**`Military Cards` (131 rows)**. The write-up above compared only the civil side (121
entries: civil techs, governments, wonders, leaders, yellow action cards). The military
deck — tactics, aggressions, wars, pacts, bonus cards, events, territories — had never
been cross-checked against anything since the original import. Doing it now turns up a
discrepancy that is **not** a BGG-vs-us conflict: it is **us against all three of our own
sources at once**.

Read with `xlrd` (`python3 -m venv /tmp/xlsenv && /tmp/xlsenv/bin/pip install xlrd`;
`xlrd` refuses `.xlsx` but this is a genuine OLE2 `.xls`, so it works).

**Deck totals agree.** Ours and BGA both come out at **140 / 150 / 150** military cards
(2p / 3p / 4p) — 150 is the printed component count. BGG 409053 gets 157/168/168, i.e. it
is again the outlier on totals, for the same reason as Conflict A. So the aggregate is
not where our problem is. The problem is the **distribution inside Ages I and III**:

| Age | group | **ours** | BGA (#7) | TTS (#8) | BGG 409053 |
|---|---|---|---|---|---|
| I | tactic | **5** | 10 | 10 | 10 |
| I | aggression | **11** | 6 | 6 | 7 |
| III | tactic | **4** | 6 | 6 | 6 |
| III | aggression | **10** | 8 | 8 | 9 |
| II | tactic / aggression | 6 / 9 | 6 / 9 | 6 / 9 | 6 / 9 ✔ |

Age II agrees everywhere. Ages I and III are an exact swap — we are short exactly 5
tactic copies in Age I and 2 in Age III, and long by exactly the same number of
aggressions — which is why the totals still came out right and the error survived.

Per card, and this is unanimous 3–0 against us:

| Card | Age | **ours** | BGA | TTS | BGG |
|---|---|---|---|---|---|
| Fighting Band | I | **1** | 2 | 2 | 2 |
| Heavy Cavalry | I | **1** | 2 | 2 | 2 |
| Legion | I | **1** | 2 | 2 | 2 |
| Medieval Army | I | **1** | 2 | 2 | 2 |
| Phalanx | I | **1** | 2 | 2 | 2 |
| Mechanized Army | III | **1** | 2 | 2 | 2 |
| Modern Army | III | **1** | 2 | 2 | 2 |
| Aggression: Enslave | I | **3** | 2 | 2 | 2 |
| Aggression: Plunder | I | **4** | 2 | 2 | 2 |
| Aggression: Raid | I | **4** | 2 | 2 | 2 |
| Aggression: Plunder | III | **4** | 2 | 2 | 2 |
| Aggression: Raid | III | **3** | 2 | 2 | 2 |
| Aggression: Armed Intervention | III | **3** | 4 | 4 | 4 |
| Age II tactics (Classic Army, Conquistadors, Defensive Army, Fortifications, Mobile Artillery, Napoleonic Army) | II | 1 each | 1 each | 1 each | 1 each ✔ |
| Entrenchments, Shock Troops | III | 1 each | 1 each | 1 each | 1 each ✔ |

(TTS counts each card object twice — front/back — so its raw counts of 4/4/4/4/4, 12, 12,
8, 2, 2 are halved above. Halving is confirmed by the Age II tactics and by
Entrenchments/Shock Troops, which come out at 1 exactly as everyone agrees.)

**Why this is a correction and not a conflict.** §7 of the source ranking already names
BGA Studio as *authoritative for per-card copy counts* and TTS as its *independent
confirmation*, and our data is supposed to have come from them. They agree with each
other, TTS is genuinely independent of BGA, and BGG — an unrelated fourth author working
from BGO — agrees with both. There is no source anywhere that supports our 1-copy
tactics. This is a transcription error on our side, not a third-opinion disagreement.

**Why it matters for the bot, a lot.** Tactic cards are the single highest-leverage
military draw in the game, and we have been running the hill climb on a deck with **half
the Age I tactics it should have** (5 instead of 10 out of 43 Age I military cards: 11.6%
vs 23.3%) and a correspondingly aggression-heavy deck. Every weight the hill climb has
learned about military tempo has been fitted to the wrong draw distribution.

**Why it has NOT been applied.** The hill climb is live and two other agents are working;
silently changing deck composition mid-run would make generations before and after
incomparable and would poison their experiments. The fix is small and fully specified by
the table above (13 `count` values in `data/cards_military_actions.json`; totals stay
140/150/150 so `validate_cards.py` will still pass). **User's call on when to land it and
whether to reset the hill climb.**

Two smaller things found in the same pass, neither of them errors:
- **Naming.** Our `Military Alliance` (III, pact) is called **`Military Pact`** by both
  BGA and BGG. Same card (0/1/1, 3 military actions). Cosmetic, but it is the only name
  in the military deck where we differ from BGA, and it will bite anyone diffing the two.
  Likewise our `Development of Civil Life` (A) is `Development of Civilization` everywhere
  else. Ours also spells `Loss of Sovereignty` correctly where BGA has `Sovereignity`.
- **`Aggression Kidnap` (I) and `Aggression Occupy` (III)** appear in BGG 409053 (1 copy
  each) and are absent from both our data and BGA. In TTS they appear exactly *once* each
  where every real base-game card appears twice, i.e. they are not in the base military
  deck at all. Excluding them is correct and matches BGA. They are also 2 of the 6 cards
  by which BGG's military total overshoots — consistent with 409053 being the sloppy file,
  as Conflicts A and B already established.

### Verdict (2026-07-26): our counts were wrong; fixed. Two independent sources, one of them page-images

The flag above rested on a single new source (`bgg_409053_player_card_counts.xls`, sheet
`Military Cards`) plus our own derived `sources/bga_card_counts.tsv` and TTS. Before
touching `data/` this was re-done from scratch against two sources that cannot have
copied each other:

**Source 1 — `sources/bgg_154670_card_reference_v109.pdf`, page 3.** Walter Kolczynski's
Card Reference v1.09, created 2015-11-04. It is *four page-images* (`pdftotext` returns
zero characters; `pdfimages -list` shows one RGB image per page), so it was read visually
at 200 dpi. Page 3 carries the military deck as coloured tables — Bonus, Aggression, War,
Pact, Tactic — each with a rightmost **`#` column giving the number of copies**. Read
directly off the page:

| Tactic | Age | # | | Aggression | Age | # |
|---|---|---|---|---|---|---|
| Fighting Band | I | **2** | | Enslave | I | **2** |
| Heavy Cavalry | I | **2** | | Plunder | I | **2** |
| Legion | I | **2** | | Raid | I | **2** |
| Medieval Army | I | **2** | | Annex | II | 1 |
| Phalanx | I | **2** | | Infiltrate | II | 2 |
| Classic Army … Napoleonic Army (6) | II | 1 each | | Plunder / Raid / Spy | II | 2 each |
| Entrenchments | III | 1 | | Armed Intervention | III | **4** |
| Mechanized Army | III | **2** | | Plunder | III | **2** |
| Modern Army | III | **2** | | Raid | III | **2** |
| Shock Troops | III | 1 | | | | |

Same page: wars 2 / 2 / 6, all ten pacts 1 each, military bonus **6** per age.

**Source 2 — `sources/bga_throughtheages_material.inc.php` (the raw BGA file, not our
extract).** Each card entry carries `qt2` / `qt3` / `qt4`. Entry 56 is
`'category' => 'Tactics', 'qt2' => 2, 'qt3' => 2, 'qt4' => 2, … 'name' => 'Fighting Band'`,
and likewise 2/2/2 for Heavy Cavalry (63), Legion (72), Medieval Army (76), Phalanx (84);
Age I aggressions Enslave (54), Plunder (85), Raid (87) are all 2/2/2; Age III Mechanized
Army (229) and Modern Army (234) are 2/2/2, Armed Intervention (191) is 4/4/4. Light
Cavalry (74) is `0/0/0` — the php is edition-aware and zeroes the 2006-only card, which is
a good sign these fields are exactly what they look like.

The two agree with each other on every military card, and with the `.xls` and TTS. So the
count is 4–0 against us, and two of the four are primary.

**Where our error actually came from.** Not from BGA — from *our reading of it*. The
derived file `sources/bga_card_counts.tsv` claims in its own header to be "Extracted from
Board Game Arena implementation material.inc.php", yet lists Fighting Band as `1 1 1`
where the php it names says `2 2 2`, and Age I Plunder as `4 4 4` where the php says
`2 2 2`. The transcription error is in that extraction step, and `data/` inherited it.
`sources/bga_card_counts.tsv` is therefore **not** an independent source and must not be
cited as one again; cite the `.php` (and note that the tsv is still uncorrected).

**Why the `.xls` disagreeing on deck *membership* does not weaken it on *counts*.** The
`.xls` has five military rows we do not: `Hussars` (II tactic), `Positional Army` (III
tactic), `Aggression Kidnap` (I), `Aggression Occupy` (III), `Hybrid War` (III), plus
`Naval Trade Agreement` (I pact) and events `Call to Arms`, `Dark Ages`, `Knowledge of the
Agents`, `Arms Industry`, `Freedom of Movement`, `International Negotiations`,
`Impact of Culture`, `Impact of Harmony`. Every one of those is already on the
**"New Leaders and Wonders" expansion** list in the *Edition filtering performed* section
below (some under their NamuWiki names: Kidnapping, Occupy, Hybrid Wars, Maritime Trade
Agreement, Knowledge of the Ancestors). The `.xls` is simply an **expansion-inclusive**
file. That is the whole of its 168-vs-150 overshoot, and the PDF confirms it card for
card: page 3 lists **no** Hussars, Positional Army, Kidnap, Occupy or Hybrid War, and page
4 lists **no** Call to Arms, Dark Ages, Knowledge of the Agents. Our *membership* is right
and stays untouched; only our *copy counts* were wrong. (The `.xls` also says the military
bonus is 7 per age where the PDF says 6 and BGA says 6 — one more reason to treat it as
the loosest of the four, on this too we keep our 6.)

**Applied.** 13 `count` values in `data/cards_military_actions.json`, each across
`2p`/`3p`/`4p`:

| Card | Age | was | now |
|---|---|---|---|
| Fighting Band, Heavy Cavalry, Legion, Medieval Army, Phalanx | I | 1 | **2** |
| Aggression: Enslave | I | 3 | **2** |
| Aggression: Plunder | I | 4 | **2** |
| Aggression: Raid | I | 4 | **2** |
| Mechanized Army, Modern Army | III | 1 | **2** |
| Aggression: Armed Intervention | III | 3 | **4** |
| Aggression: Plunder | III | 4 | **2** |
| Aggression: Raid | III | 3 | **2** |

Age II was already correct everywhere and is untouched. Per-age military totals are
unchanged (Age I: tactics 5→10 and aggressions 11→6; Age III: tactics 4→6 and aggressions
10→8), so the deck is still 140 / 150 / 150 and `python3 data/validate_cards.py` still
passes. The composition change is real, though: Age I tactics go from 5/45 to 10/45 of the
Age I military deck. **This invalidates comparability of hill-climb generations run before
this commit** — see `engine/PROGRESS.md`.

**Two naming notes, deliberately NOT changed** (cosmetic, and renaming would churn the
determinism digests for nothing):
- Our `Military Alliance` (III pact) is `Military Pact` in the BGA php (entry 4068) and in
  the `.xls`, but `Military Alliance` in the BGG PDF. The sources are split 1–1 and we
  match the PDF, so the earlier note that we are alone here was wrong.
- Our `Development of Civil Life` (A event) is `Development of Civilization` in all three
  of the PDF, the php and the `.xls`. We are alone; this one is worth renaming if anyone
  is ever diffing card names, but it is a label only.

## Edition filtering performed
NamuWiki lists include content that is NOT in the base 2015 game. Excluded:

### Korean-edition-only cards (in the Korean printing of the base game)
- Wonders: Seokguram Grotto (A), Hangul (I), Donguibogam (II), K-POP (III)
- Leaders: King Gwanggaeto the Great (A), King Sejong (I), Admiral Yi [Yi Sun-sin] (II), Kim Gu (III)

### "New Leaders and Wonders" expansion content (marked "expansion"/"extended" in NamuWiki)
- Leaders: Hippocrates, Boudica, Confucius, Sun Tzu ("grandson" is a mistranslation of Sun Tzu/손자), Cleopatra, Ashoka (A); Jan Žižka, Nostradamus, Isabella I, Johannes Gutenberg, Eleanor of Aquitaine, Saladin (I); Alfred Nobel, Antoni Gaudí, James Watt, Catherine II ("Ekaterina II"), Charles Darwin, Maria Theresa (II); Marie Curie, Pierre de Coubertin, Nelson Mandela, Marlene Dietrich, Steve Jobs, Ian Fleming (III)
- Wonders: Colosseum, Roman Roads, Stonehenge, Acropolis (A); Machu Picchu, Himeji Castle, Forbidden City, Silk Road (I); Harvard University, Suez Canal, Louvre, Statue of Liberty (II); Empire State Building, Manhattan Project, United Nations, International Red Cross (III)
- Tactics: Hussars (II), Positional Army (III)
- Military cards: Kidnapping (I aggression), Occupy (III aggression), Hybrid Wars (III war), Maritime Trade Agreement (I pact), Dominion (II territory), assorted expansion events ("Knowledge of the Ancestors", "Dark Ages", "Call to Arms", "Freedom of Movement", "International Negotiations", "Arms Industry", "Impact of Harmony", "Impact of Culture", "Development of Planning")
- Expansion stat variants (e.g. Professional Sports strength 4, Fundamentalism +6, Communism +1 resource) — base 2015 values used instead.

### 2006-vs-2015 differences observed (NamuWiki "(old)→(new)" and hypercheat comparison)
NamuWiki explicitly tags old/new values; where tagged, the "(new)" value was taken. Notable 2015 changes vs 2006 (hypercheat.txt reflects 2006 and MUST NOT be used for these):
- Swordsmen tech 3→4, Knights 4→5, Riflemen 5→6, Cannon 7→6, Modern Infantry 8→10, Rockets 10→8, Air Forces 11→12
- Printing Press build 4→3, Bread & Circuses build 4→3, Team Sports build 6→5, Drama 4/5→3/4, Opera build 9→8, Movies build 12→11, Computers build 10→11
- Monarchy 3(9)→2(8), Theocracy 2(7)→1(6) w/ new happy1/strength1, Constitutional Monarchy 5(12)→6(12), Republic 4(14)→3(13), Communism 6(17)→5(19), Fundamentalism 7(19)→7(18) w/ -2 science, Democracy 8(21)→9(17) w/ +3 culture
- Wonders: Library of Alexandria stages 1-2-2-1→1-4-1; Transcontinental RR 3-4-5 str+5→3-3-3-3 str+4; Kremlin culture3/happy-2→culture2/happy-1; Ocean Liners 3-2-2-2-3→4-2-2-4 (no food cost); First Space Flight 3-4-9→1-2-4-9; Internet & Hollywood scoring formulas changed; St. Peter's doubling→+1 per happy source; Taj Mahal +blue token/leader-swap discount; Colossus culture1,str1,colony1→str2,colony1
- Tactics bonuses: Napoleonic Army 8(4)→7(4), Classic Army 9(5)→8(4), Mobile Artillery 5→5(3) (obsolete value added)
- Leaders renamed/changed: Rock'n'Roll Icon→Charlie Chaplin, Alex Randolph→Sid Meier, (Bill Gates→Tesla in some 2006 printings→) Bill Gates in 2015; many ability texts changed (see cards.json)
- Rules: tactics become shared (common tactics area), no unit sacrifice in wars/aggressions (defender may instead discard military cards for +1 each), end-of-turn order changed (corruption FIRST, then food, then resources), production caps removed, blue tokens 18→16, action phase before military-card discard, Age IV formalized (no leaving the game, no war declarations)

## Per-card provenance notes
- Civil technology stats (farms/mines/urban/units/govs/special): namu_farms, namu_urban, namu_units, namu_gov, namu_spectech — "new edition" values.
- Wonders: namu_wonders "new edition" values; stage arrays cross-checked vs rulebook appendix where present.
- Leaders: namu_heroes "new edition" values (base-set 24 only).
- Events/colonies: namu_events (non-expansion entries).
- Aggressions/wars/pacts/defense: namu_military.
- Action cards (yellow), 14 names / 33 age-variants: names and exact effects from the digital edition's shipped localization strings (`CivilCards_card_names` + `CivilCards_card_texts`, obtained from the Russian localization repo `yashcherU/Through-the-Ages_ru`, which mirrors the English strings 1:1 and uses keys like `RICH_LAND_0..2`, `URBAN_GROWTH_0..3`). Roster cross-checked against throughtheages.fandom.com "Card List: Digital Edition" and faq_v15.pdf p.12 (all three agree exactly); numeric values cross-checked against an independent 2006-era card spreadsheet (`GMetola/learning_AI_games`) — everything matches except the Age II Breakthrough science value, which the BGA data settles at **3** ("Develop a technology. After you pay the science cost, score 3 science"), so the spreadsheet's 4 is the 2006 value. NamuWiki has no action-card list. Per-name COPY COUNTS for all 33 age-variants now come from BGA + TTS (sources #7/#8) and are no longer derived. Note: the physical card is "Urban Growth" (FAQ p.12 prints "Urban Grown", a typo); some digital builds label it "Urban Development".
- Card counts by player count: **BGA `qt2/qt3/qt4` + the TTS `(3+)`/`(4)` pile split (sources #7/#8) are now the primary source for every civil card and every action card**; NamuWiki "number of cards" fields agree with them everywhere except Age I Swordsmen (NamuWiki says 2/2/3, BGA and TTS both say 2/2/2 — NamuWiki is wrong, since 2/2/3 would make Civil I a 54-card deck). Military-card copies are still derived from published deck totals. Deck-size anchors: czechgames.com component list = 179 civil + 150 military cards; military decks 10/45/50/45; civil decks Age A 20 and Ages I/II/III 53 at 4 players (44 + 6 cards marked "3+" + 3 marked "4", BGG thread 1454794 "Card count"); RB p.4 / CoL p.2 for the "3+"/"4" removals and the 2-player pact removal. `python3 data/validate_cards.py` re-checks all of these.
