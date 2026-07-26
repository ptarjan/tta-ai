# Open Questions / Unresolved Ambiguities (TTA base 2015)

Status: WORKING LIST — items removed as resolved, added as discovered.

## Card data
1. ~~Action (yellow) card full list + counts per age~~ — RESOLVED if rulebook appendix covers it; otherwise reconstruct from ubg/vassal/cge. PENDING VERIFICATION.
2. Military deck per-card counts (events, aggressions, wars, pacts, tactics, defense) with 2p/3p/4p removals — NamuWiki gives none for these; need rulebook appendix ("3+", "4" corner marks). PENDING.
3. ~~Sid Meier (2015) exact ability text~~ — RESOLVED 2026-07-26: same mechanic as 2006 Alex Randolph, confirmed via throughtheages.fandom.com (Alex_Randolph page: "Each of your labs produces culture: 1 per level; each lab produces 1 less science"; "In A New Story of Civilization, it was changed back to Sid Meier") and faq_v15.pdf worked example ("If you have two Age-III Computers Workers, they produce a combined 8 Science and 6 Culture per turn" — implies 2015 Computers = 5 science, minus 1 each). FAQ also confirms Sid Meier affects the culture scored by the Internet wonder. Data updated in data/cards_wonders_leaders.json.
4. ~~Aristotle / Moses / da Vinci / Newton / Einstein / Gandhi 2015 texts unchanged from 2006~~ — RESOLVED 2026-07-26: all six confirmed unchanged via throughtheages.fandom.com card pages (exact texts adopted in data/cards_wonders_leaders.json). Gandhi note: the fandom page marks "3 more military actions" as the EXPANSION variant; the base 2015 card keeps the 2006 "twice the military actions" wording. No FAQ contradictions found.
5. Exact 2015 defense-card values (assumed Age I +2 def/+1 colonize, II +4/+2, III +6/+3; 6 per age) — cross-check.
6. Exact resource-gain formula on Raid aggressions ("half of building cost" per NamuWiki) — verify wording/rounding.
7. Spy (Age II aggression) military-action cost: NamuWiki says 1; verify.
8. Age A "current events" composition: number seeded = players+2; verify list of Age A event names & counts against rulebook.

## Rules
9. Exact end-of-turn sequence order (2015): production phase order per rulebook (corruption → food production → food consumption → resource production? vs namu_main which says science/culture first, corruption first among material steps, military draw last) — VERIFY against rulebook pp. (production chapter).
10. Politics phase in 2-player games: pacts unused; event seeding — does each player seed both future decks? 2p uses "double seeding"? VERIFY rulebook 2p variant section.
11. Card row: positions swept per player count at turn start (2p:3, 3p:2, 4p:1 per namu_main) and cost bands — verify exact band boundaries per player count from board layout in rulebook.
12. War declared but attacker's strength lower at resolution — spoils formula per war card (difference-based); confirm no minimum.
13. Tie-breaking in final scoring: namu_main claims ties share victory — verify rulebook.
14. "International Convention"/"Politics of Power" event edge cases on final turn — verify.
15. Whether the last Age III civil card triggering end applies to military deck exhaustion (namu_main: military deck reshuffles, age does not end) — verify.
