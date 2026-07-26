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
`pdftotext` yields **zero bytes**. It is a screenshot of a spreadsheet, so it is only
readable by eye/OCR, and it carries no information the `.xls` does not already carry in
machine-readable form. It was therefore **not** used for the numeric cross-check below.

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

**Conflict B — three cards marked "3+" by BGG where we mark them "4".** This does not
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
