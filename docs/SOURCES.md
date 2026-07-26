# Sources and Provenance (TTA base 2015 data)

Status: DRAFT — being refined incrementally.

## Source ranking
1. `/Users/pt/tta-ai/sources/1j1ju_rulebook.pdf` (= docs/rulebook.pdf) — official 2015 rulebook (24 pp). AUTHORITATIVE for rules, setup counts, turn structure.
2. `/Users/pt/tta-ai/sources/faq_v15.pdf` — official FAQ v1.5 (16 pp). Authoritative clarifications.
3. `namu_*.txt` — NamuWiki card lists (machine-translated from Korean). Primary card-stat source. These pages cover BOTH the 2006 "old version", the 2015 "new edition", and the expansion; only "new edition" (or unchanged "old version" when no new-edition entry exists) values were used. Expansion-marked and Korean-edition-exclusive cards were EXCLUDED (see below).
4. `ubg_*.txt` — UltraBoardGames paraphrase (secondary, rules cross-check).
5. `hypercheat.txt` — 2006 ORIGINAL edition cheat sheet. Used only as tiebreaker; 2006-vs-2015 differences noted below.
6. `vassal_tta.html`, `cge_tta.html`, `fandom_allpages.html` — misc; used for card counts where needed.

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
- Action cards (yellow): rulebook + ubg + cge/vassal (NamuWiki has no action-card list in cache) — see OPEN_QUESTIONS.md for any gaps.
- Card counts by player count: NamuWiki "number of cards" fields (2p/3p/4p); military deck counts from rulebook appendix/FAQ where available.
