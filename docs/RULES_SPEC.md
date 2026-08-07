# Through the Ages: A New Story of Civilization (2015) — Rules Specification

Status: COMPLETE — all 13 sections filled 2026-07-26 from the Handbook, Code of Laws, FAQ v15, and board-image verification. A section is complete only when it ends with `<!-- SECTION COMPLETE -->`.

Source citations: `[RB p.N]` = sources/1j1ju_rulebook.pdf (Handbook) page N. `[CoL p.N]` = sources/cge_code_of_laws.pdf page N (the authoritative full-game rulebook). `[FAQ p.N]` = sources/faq_v15.pdf. `[namu_military]` = sources/namu_military.txt (card-effect summaries; verify wording against card data).

## 1. Components & Setup

Source citations also use `[CoL p.N]` = sources/cge_code_of_laws.pdf page N (the complete rules; the Handbook `[RB]` defers full-game combat rules to it).

1.1 **Player board initial technologies** (Age A civil cards, "in play" for all purposes) [RB p.2-3, CoL p.2]:

| Card | Type | Workers at start | Per-worker output | Notes |
|---|---|---|---|---|
| Warriors | military unit (infantry) | 1 | 1 strength | |
| Agriculture | farm | 2 | 1 food | |
| Bronze | mine | 2 | 1 resource | |
| Philosophy | urban: lab | 1 | 1 science | |
| Religion | urban: temple | 0 | 1 culture + 1 happy face | |
| Despotism | government | — (no workers on governments) | — | 4 CA, 2 MA, urban building limit 2 |

1.2 **Tokens per player** [RB p.2, CoL p.2]: 16 blue in blue bank (all squares); 25 yellow: 18 in yellow bank, 6 on the technologies above, 1 in worker pool (unused worker). 4 white + 2 red tokens beside the player board (they represent Despotism's 4 CA / 2 MA). Pieces: tactics standard, science/culture point counters (octagonal, start at 0 on point tracks), 4 rating markers: science 1, culture 0, strength 1, happiness 0 [RB p.4-5, CoL p.2].

1.3 **Deck trimming by player count** [CoL p.2; FAQ p.15]:
- 4p: no removals.
- 3p: from Civil decks I, II, III remove the 3 cards marked "4" in each.
- 2p: from Civil decks I, II, III remove the 9 cards marked "3+" or "4" in each (six "3+", three "4"); remove ALL pact cards from Military decks (Mil I has 2 pacts, Mil II and III have 4 each).
- Age A decks are never trimmed. (RB p.4 mentions only Civil I & II because the first-game variant omits Age III.)

1.4 **Card row**: always 13 spaces, all player counts. Deal 13 Age A civil cards face up; rest of Age A civil deck goes on current age board [RB p.4, CoL p.2]. Current age military deck space is empty in Age A (players never draw Age A military cards) [RB p.5].

1.5 **Sweep rate** (cards discarded from leftmost spaces at the start of each player's turn, from round 2 on): 2p = 3, 3p = 2, 4p = 1 [RB p.8, CoL p.3]. If a player resigned, count remaining players [CoL p.4].

1.6 **Current events seeding**: shuffle Age A military deck; place top (players + 2) cards face down as the current events deck (2p:4, 3p:5, 4p:6); return the rest to the box unseen [RB p.5, CoL p.2]. Future events space starts empty. Age A events are all positive.

1.7 **Initial military hand**: none. No military cards are drawn on the first turn (no MAs available) [RB p.5,7].

1.8 **Turn order**: starting player chosen randomly/arbitrarily; fixed clockwise order all game [RB p.5, CoL p.2].

1.9 **First round special rules** [RB p.5-6, CoL p.2-3]:
- Available CAs: starting player 1, then 2, 3, 4 by seating order. 0 MAs for everyone.
- Only legal Action Phase actions: take a card from the card row (a taken wonder goes directly into play as unfinished).
- Skip Start-of-Turn Sequence (no card-row replenish) and Politics Phase.
- End-of-Turn Sequence runs, but: no military cards in hand, no uprising/corruption risk, no military draw. Each player ends turn 1 with +1 science, 0 culture, food/resource production as normal.
- Reset Actions: full 4 CA + 2 MA available from turn 2.

1.10 **Second turn / end of Age A**: the starting player's second turn begins with the first card-row replenish; when the row is replenished for the first time, Age A ends and Age I begins immediately (remaining Age A civil deck to the box; Age I civil deck and Age I military deck become the current decks). If Age A civil cards run out while filling, continue filling from the Age I deck [RB p.8, CoL p.3; FAQ p.11]. No other end-of-age effects occur at this transition (no antiquation loss, no yellow-token loss) [RB p.21].
<!-- SECTION COMPLETE -->

## 2. Card Row Mechanics

2.1 **Replenish (start of every turn except round 1)** [RB p.8, CoL p.3]:
1. Discard any cards in the leftmost N spaces (N = 3/2/1 for 2p/3p/4p). Never discard cards to the right of those spaces, even if some leftmost spaces are already empty [FAQ p.11]. Removed civil cards leave the game permanently.
2. Slide all remaining cards left (order preserved).
3. Deal cards from the current age civil deck to the empty spaces, left to right. (Age IV: sweep and slide still happen, but no cards are dealt [FAQ p.11].)

2.2 **End of age trigger**: an age (I, II, III) ends when the LAST card of the current civil deck is dealt to the card row (can happen mid-replenish on any player's turn); the next age begins immediately and the row continues filling from the new deck [CoL p.3]. See §12.

2.3 **Cost bands** (identical for all player counts; printed under the spaces) [RB p.6]:

| Card row spaces (1 = leftmost) | Civil actions to take |
|---|---|
| 1–5 | 1 |
| 6–9 | 2 |
| 10–13 | 3 |

2.4 **Wonder surcharge**: taking a wonder costs the depicted 1–3 CA **plus 1 CA per wonder you have already completed** (destroyed wonders, e.g. via Ravages of Time, count as completed) [RB p.6, CoL p.5,12]. The wonder goes directly into play sideways as your unfinished wonder (never to hand); you may not take a wonder while you have an unfinished one [RB p.6].

2.5 **Taking limits** (non-wonder): may not take a card if civil cards in hand > civil action total (hand limit, checked only when taking) -- corrected from `≥` (`docs/REPLAY.md`'s thirteenth pass): the printed rule may read `≥`, but BGO's own implementation, which this project reconstructs, only blocks once the hand is already OVER the limit, not merely at it -- falsified against 70 real games where a human took a card with hand size already equal to the limit; may not take a technology with the same name as one in your hand or in play; may not take a second leader of the same age (ever, even if the first left play) [RB p.6,9, CoL p.5].

2.6 Cards taken are public knowledge (open civil cards convention) [RB p.7].
<!-- SECTION COMPLETE -->

## 3. Civil Actions (complete enumeration)

Action Phase: spend civil and military actions in any order, any mix; may stop with actions unspent; an action may be repeated if payable; actions may be taken back (undo) before ending the phase [RB p.8,15; CoL p.5]. An action cannot be performed unless ALL required sub-steps/costs can be paid [CoL p.5; FAQ p.15]. Complete list of civil actions [CoL p.5-6]:

1. **Take a non-wonder card from the card row** — 1/2/3 CA by position (§2.3); limits §2.5.
2. **Take a wonder from the card row** — 1/2/3 CA + 1 per completed wonder (§2.4); enters play unfinished.
3. **Increase population** — 1 CA + food equal to the white number under the rightmost occupied section of the yellow bank (§6.1); move rightmost yellow bank token to worker pool. Illegal if yellow bank empty [CoL p.5].
4. **Build a farm, mine, or urban building** — 1 CA + resource cost on the technology card in play; move an unused worker onto the card. Urban buildings limited per type by government's urban building limit; farms/mines/military units unlimited [RB p.10, CoL p.5]. Construction special techs reduce urban building costs [RB p.14].
5. **Upgrade a farm, mine, or urban building** — 1 CA + (higher card's cost − lower card's cost); move the worker from the lower- to the higher-level card of the SAME type (icon in upper right). Apply cost modifiers to both costs before taking the difference; result floors at 0 via discounts? No: if the difference computation is modified, use modified costs; discounts can reduce a cost below 0 → treat as 0 [RB p.13-14, CoL p.5; FAQ p.7 Masonry table]. Urban building limit is checked (upgrade keeps count constant, so always OK for same type).
6. **Destroy a farm, mine, or urban building** — 1 CA; move the worker to the worker pool; no refund [RB p.14, CoL p.5].
7. **Play a leader** — 1 CA; from hand into play. If you already have a leader in play, it is replaced: old leader removed from the game AND you get 1 spent civil action back [RB p.11, CoL p.5]. Effects apply immediately.
8. **Build a stage of a wonder** — 1 CA + resource cost of the leftmost uncovered stage number; cover it with a blue token from the blue bank (from a technology card if bank empty [CoL p.5; FAQ p.13]). Multiple stages same turn = repeat the action. Construction techs allow building 2 (Masonry) / up to all remaining (per card) stages for a single CA, paying summed cost at once [RB p.14]. When the last stage is covered the wonder is completed: return blue tokens to bank, straighten card, effects begin (Age III wonders: immediate one-time scoring effect) [RB p.11,22].
9. **Develop a technology** — 1 CA + science cost (upper-left); from hand into play. Blue special techs: only one per type icon (law/warfare/exploration/construction); a newer one replaces the older, which is removed [RB p.12, CoL p.5]. Governments via this action = peaceful change, pay the HIGHER science cost (§8).
10. **Declare a revolution** — ALL civil actions (must all be available) + the LOWER science cost of a government in hand (§8) [RB p.13, CoL p.5].
11. **Play an action card** — 1 CA; not in the same Action Phase in which it was taken from the row [RB p.14, CoL p.6]. Resolve text; if it orders an action, perform it under normal rules but paying no civil/military action for it; "pay X less" discounts stack cumulatively, floor 0; if you cannot perform the specified action, the card cannot be played; discard after resolving (leaves the game) [RB p.14-15, CoL p.6]. An extra military action granted (Patriotism) is virtual: use it first, usable for drawing at end of turn, not carried over [RB p.15].

Notes: workers can only be placed by build/upgrade actions; taking a technology into hand has no effect until developed; there is no action to discard civil cards (they stay in hand) [RB p.9].
<!-- SECTION COMPLETE -->

## 4. Military Actions (complete enumeration)

Spent during the Action Phase, freely interleaved with civil actions [RB p.8]. Complete list [CoL p.6]:

1. **Build a military unit** — 1 MA + resource cost of a military unit technology in play; move an unused worker onto it. No limit on units per type [RB p.10].
2. **Upgrade a military unit** — 1 MA + cost difference; worker moves from lower to higher level card of the SAME unit type (infantry/cavalry/artillery/air force icon) [RB p.14, CoL p.6].
3. **Disband a military unit** — 1 MA; worker to worker pool; no refund [RB p.14, CoL p.6].
4. **Play a tactic** — 1 MA; tactics card from hand into your play area as your exclusive tactic; put your tactics standard on it (§10).
5. **Copy a tactic** — 2 MA; move your standard to any card in the common tactics area (§10).
   - Limit: at most ONE play-or-copy tactic action per Action Phase [CoL p.6; FAQ p.15].

Military actions are ALSO consumed (but not during the Action Phase) by:
- Politics: playing an aggression or declaring a war costs the MA total printed next to the crown symbol on the card (spent from your red tokens when played) [CoL p.4]. Robespierre: revolution is paid with MAs instead of CAs [CoL p.12].
- Defending an aggression: each bonus card played or military card discarded counts against your MA total (cards, not tokens — the count may not exceed your military action total) [CoL p.4].

Unspent MAs at end of turn each draw 1 military card (max 3) — §6.7. Red tokens also define the military hand limit (§6.8).
<!-- SECTION COMPLETE -->

## 5. Politics Phase

5.0 **Turn structure** [CoL p.3]: Start-of-Turn Sequence (replenish card row → resolve a war you declared last turn → make exclusive tactics available) → Politics Phase → Action Phase → End-of-Turn Sequence. In the Politics Phase you may perform AT MOST ONE political action (or skip) [RB p.16, CoL p.4]. Only cards with the crown symbol are played as political actions, and only then [FAQ p.15].

5.1 **Political action types** [CoL p.4]: prepare an event; play an aggression; declare a war (not during the last round); offer a pact (not in 2p); cancel a pact; resign (not in Age IV).

5.2 **Prepare an event** [RB p.16, CoL p.4]: choose a green military card with the harp symbol (event or territory) from hand; place face down on top of the future events deck; SCORE culture equal to the card's level (Age A=0, I=1, II=2, III=3); then reveal and resolve the top card of the current events deck (territory → colonization auction §11; otherwise follow text, then to past events pile — resolved events never re-enter the game and do NOT go to the military discard). If that was the last current event: shuffle the future events deck, sort it face down so earlier-age cards are above later-age cards, place as the new current events deck [RB p.16, CoL p.4]. An event you prepare is always revealed on a LATER turn.

5.3 **Evaluating events** [RB p.16, CoL p.7]: no actions are paid for effects unless the card says so; "increase your population" = pay the food cost; "gain 1 population" = free token to worker pool; free builds pay no resources. Multiple-player decisions resolve clockwise from the revealing player. Statistic comparisons ("strongest/weakest" = strength rating [FAQ p.11]): ties broken in favor of the current player, then proximity in clockwise order after the current player; at game-end evaluation, treat the starting player as current [CoL p.7]. "All civilizations" with most/least: all tied civs affected, no tie-break. 2p: "two strongest/weakest" reads as "the stronger/weaker" [RB p.16, CoL p.7].

5.4 **Play an aggression** (brown card, "Aggression:" in name) [CoL p.4]:
1. Reveal; pay its military action cost; declare the rival.
2. Illegal if: a pact forbids attacking them, or the rival's strength ≥ yours (include bonuses that trigger when you attack them; exclude pact bonuses that end if you attack). Annex and Infiltrate print their own narrower target clause and are additionally illegal against a rival who fails it: Annex requires "one opponent who owns at least one colony"; Infiltrate requires "one opponent with a leader in play or a wonder under construction" [digital-edition card text, `data/cards_military_actions.json`'s `target` field; Infiltrate's wonder-loss reading confirmed by FAQ p.11, "an incomplete Wonder is lost ... due to the Infiltrate Aggression"]. No other base-game aggression prints a narrower target than "one opponent".
3. If you and rival have a pact that ends on attack, remove it now.
4. Defense: rival may play military bonus cards (add printed DEFENSE value, top half; cards then discarded) and/or discard any other military cards face down for +1 strength each. Total cards played+discarded ≤ defender's military action total. NO unit sacrificing by either side (2015 change) [RB p.24].
5. If defender's total ≥ attacker's strength: aggression fails, discard it, no effect (ties favor the defender; only the attacker can win an aggression) [FAQ p.16].
6. Else: resolve the card text (cost-based effects use printed costs, ignoring modifiers [CoL p.4; FAQ p.7]); discard the aggression. Attacker's spent MAs are not refunded on failure.

5.5 **Base-game aggressions** (MA cost / effect) [namu_military; verify exact wording against card data]: Age I: Plunder 1 (take 3 food/resources any mix), Raid 1 (destroy a level 0–1 building, gain half its printed cost in resources), Enslave 2 (rival loses 1 population; gain 2 food + 2 resources). Age II: Plunder 1 (take 5), Raid 2 (destroy up to a level 0–1 and a level 0–2 building), Infiltrate 2 (remove rival's leader or unfinished wonder from game; gain 3 culture per level), Annex 2 (take a rival's colony: gain its permanent effects, not its one-time effect), Spy 1 (take up to 5 science). Age III: Plunder 1 (take 7), Raid 3 (destroy up to a level 0–2 and a level 0–3 building), Armed Intervention 2 (take up to 7 culture).

5.6 **Declare a war** (gray card, "War over …") [CoL p.4]: reveal; pay its MA cost; declare rival (illegal if a pact forbids it; NOT restricted by relative strength); remove attack-ending pact; place the war in your play area, top toward the rival. Not during the last round; in Age IV a declaration is legal only if you will get another turn [CoL p.4; FAQ p.11].

5.7 **Resolve a war** — at the start of the ATTACKER's next turn, after replenishing the card row, before Make Tactics Available / Politics [CoL p.3]:
1. Compare current strengths (include when-attacking bonuses of the declarer). No bonus cards, no discards, no sacrifices — only the table state counts [RB p.22, CoL p.3].
2. Higher strength = victor (EITHER side can win); difference = strength advantage; follow the war card text against the defeated civilization. Equal strength = no effect. Then discard the war card.
3. Wars in play have no effect until resolved; they are removed from play when resolved, or when antiquated at age end (declared wars survive antiquation per CoL p.3 "declared wars remain in play"— i.e., other cards note: technologies, completed wonders, exclusive tactics, DECLARED WARS remain in play even if antiquated) — but PACTS antiquate away [CoL p.3].
4. If a pact granting strength is accepted after declaration but before resolution, it counts [FAQ p.11-12]. Multiple players may attack the same civ in a round [FAQ p.11].

5.8 **War spoils (base game)** [namu_military; CoL p.11 card image; FAQ p.8]:
| War (age, MA cost) | Victor takes from defeated |
|---|---|
| War over Territory (II, 2 MA) | 1 yellow token + 1 per full 5 points of strength advantage, from yellow bank (only what is available) |
| War over Technology (II, 2 MA) | science points equal to advantage; may take known blue special techs instead at their science cost (not ones you have in hand/play; higher-level steals replace yours); capped by loser's science + eligible special-tech value |
| War over Culture (III, 3 MA, 6 copies) | 5 + advantage culture points (capped at what the victim has; victim cannot go negative) |

5.9 **Offer a pact** (blue card; 3p/4p only) [CoL p.4]: reveal, name the partner and (if sides A/B exist) who is which; no prior negotiation. Partner accepts → pact enters YOUR play area (rotated so partner's side faces them); any previous pact in your area is removed; it applies immediately; a pact preventing attacks does not cancel already-declared wars. Partner refuses → back to your hand (political action still used). You may be party to many pacts but have only one in your own area. No MAs are spent for offering/accepting/canceling [FAQ p.16].

5.10 **Cancel a pact** [CoL p.4]: remove any pact you are party to from play.

5.11 **Resign** [CoL p.4]: not in Age IV. You leave play, discard hand and all cards, remove your pacts; wars declared against you are removed and score their declarer 7 culture; remaining players continue (sweep count adjusts; future-age decks re-trimmed for the new player count; current-age decks untouched; 2 players left → 2p event reading applies). Last player standing wins immediately.
<!-- SECTION COMPLETE -->

## 6. Population & Economy; End-of-Turn Sequence

6.1 **Yellow bank layout** (18 squares; fills left→right, tokens taken from the right; verified from board images RB p.2-3, p.20). Sections right→left with food cost (white bag) and the consumption number printed on each section's leftmost square (revealed as it empties). Happiness subsections (number = happy faces required once that subsection is FULLY empty): sub1(2 squares), sub2(4), sub3(2), sub4(2), sub5(2), sub6(2), sub7(2), sub8(2).

| Tokens left in yellow bank | Cost to increase pop | Consumption | Happy faces required |
|---|---|---|---|
| 18–17 | 2 | 0 | 0 |
| 16–15 | 3 | 1 | 1 |
| 14–13 | 3 | 1 | 1 (sub2 partial at 14–13: still 1) |
| 12–11 | 4 | 2 | 2 |
| 10–9 | 4 | 2 | 3 |
| 8–7 | 5 | 3 | 4 |
| 6–5 | 5 | 3 | 5 |
| 4–3 | 7 | 4 | 6 |
| 2–1 | 7 | 4 | 7 |
| 0 | — (impossible) | 6 | 8 |

Precise rules: cost = white number under the rightmost occupied section [CoL p.5]; consumption = leftmost uncovered printed negative number (−1 on square 17, −2 on sq 13, −3 on sq 9, −4 on sq 5, −6 on sq 1); happy faces required = number above the leftmost fully-empty subsection; a partially empty subsection never counts [CoL p.6,10; FAQ p.14]. (Note: token counts 16–13 all → cost 3/consumption 1; 12–9 → 4/2; 8–5 → 5/3; 4–1 → 7/4. The table above also splits rows to show the happiness thresholds: sub1 empty at ≤16, sub2 at ≤12, sub3 at ≤10, sub4 at ≤8, sub5 at ≤6, sub6 at ≤4, sub7 at ≤2, sub8 at 0.)

6.2 **Blue bank** (16 squares; fills left→right; sections 5/5/6 with −6 printed on square 1, −4 on square 6, −2 on square 11; verified RB p.20 example + ubg + namu):

| Blue tokens in bank | Corruption |
|---|---|
| 16–11 | 0 |
| 10–6 | 2 |
| 5–1 | 4 |
| 0 | 6 |

6.3 **Happiness & uprising** [RB p.19-20, CoL p.6,10; FAQ p.14]: happiness rating = total happy faces from cards/workers minus unhappy faces, clamped to 0..8. Discontent workers = number of fully-empty yellow-bank subsections whose number exceeds your happiness rating (equivalently: happy faces required − happiness rating, min 0). Unused workers do not reduce discontent workers; they only prevent the uprising. **Uprising check (end of turn, after discarding excess military cards): if discontent workers > unused workers, skip the entire Production Phase** (score/corruption/production/consumption all skipped). Military card draw and reset actions still happen [RB p.24].

6.4 **Blue-token economics** [RB p.13,20; CoL p.11; FAQ p.15]: a blue token on a farm/mine card is worth that card's printed food/resource value (even with no workers there). Pay by returning tokens to the bank and/or moving tokens to lower-value cards (making change downward, or exchanging one high token for several lower ones); NEVER move tokens to higher-value cards. Overpaying allowed only when exact payment is impossible and only leaving the bank empty [CoL p.11]. Gaining food/resources: move tokens from bank to cards, total exactly the amount (if the bank lacks tokens, gain the nearest lower achievable value; empty bank → gain nothing) [CoL p.11].

6.5 **Gaining/losing yellow & blue tokens** (card symbols) [RB p.17]: cards with +yellow/+blue symbols add tokens from the box to the bank when played, and remove them when the card leaves play; −blue symbol works in reverse. Losing yellow tokens beyond what the bank holds: lose only what is there. Losing blue tokens beyond the bank: also strip tokens from technology cards to cover [RB p.17]. "Lose 1 population": unused worker → yellow bank; if none, a worker off a card (owner's choice) [FAQ p.15].

6.6 **EXACT End-of-Turn Sequence** (2015 order, printed on player board) [RB p.7,20, CoL p.6]:
1. **Discard excess military cards** — down to military action total (red tokens), face down. Only step requiring a decision.
2. **Uprising check** — if discontent > unused workers, skip step 3 entirely.
3. **Production Phase**, in order:
   a. **Score science and culture** — points += science rating; culture points += culture rating.
   b. **Corruption** — pay resources = corruption (6.2); shortfall paid in food (lose food to cover the difference; if you cannot cover everything you still lose all you have) [CoL p.6].
   c. **Food production** — 1 blue token from bank onto each farm card per worker on it (highest-level first if bank runs short) [CoL p.6].
   d. **Food consumption** — pay food = consumption (6.1); if short, pay what you can and **lose 4 culture points per missing food** [CoL p.6].
   e. **Resource production** — 1 blue token per worker onto mine cards (highest level first if short) [CoL p.6].
4. **Draw military cards** — 1 per unused (available) military action, **max 3**, from the current age military deck; drawn even beyond hand limit; none in Age IV; none on round 1. If the deck runs out, reshuffle that age's military discards into a new deck (the age does NOT end) [RB p.20, CoL p.6].
5. **Reset actions** — all white and red tokens back to the government card.

(2015 vs 2006: corruption moved BEFORE production; military draw no longer part of production so an uprising doesn't block it; resetting actions is officially part of the sequence [RB p.24].)

6.7 **Hand limits** [RB p.20, CoL p.8]: civil hand limit = civil action total (white tokens, spent or not); enforced only when taking cards (wonders bypass hand entirely; exceeding via other means forces no discard). Military hand limit = military action total; enforced ONLY at end-of-turn step 1; drawing may exceed it.
<!-- SECTION COMPLETE -->

## 7. Technologies

7.1 **Definition**: any civil card with a science cost in the upper-left corner + the six initial board cards [CoL p.9]. Types (upper-right icon): farm; mine; urban building (lab/temple/library/theater/arena); military unit (infantry/cavalry/artillery/air force); special (blue: law/warfare/exploration/construction); government (orange).

7.2 **Research ("develop")** — 1 CA + science points = printed cost (§3.9). Pay by moving the science counter back; insufficient points → illegal [RB p.12]. One-per-name: you may never have two cards with the same name in hand or in play (blocks taking from row, §2.5). A newly developed farm/mine/urban/military technology has NO effect until workers are on it [RB p.12].

7.3 **Multiple levels**: you may have several technologies of the same type and different levels in play (e.g. Agriculture + Irrigation); each is a separate card with its own workers; you may still build lower-level things [RB p.12-13].

7.4 **Build/upgrade delta rule**: building costs the card's printed resource cost; upgrading a worker from a lower- to a higher-level card of the same type costs the DIFFERENCE of the two printed costs (both adjusted by construction-tech modifiers first: treat modified values as if they had always been the price, per FAQ p.7 table — e.g. Alchemy 6→5 with Masonry). Upgrading may skip levels [RB p.13-14].

7.5 **Urban building limit**: government card's lower-right number = max buildings of EACH urban type (e.g. Despotism 2, so max 2 labs AND 2 temples...). Applies to builds only, not to already-built stock after a government change (no forced destruction; but no new builds of a type at/over limit). Farms, mines, military units unlimited [RB p.10, CoL p.9; FAQ p.15].

7.6 **Special (blue) technologies**: effects apply immediately on development; no workers ever; max one per type icon — developing a same-icon tech keeps only the higher level in play (the lower is removed from the game); developing a lower-level one than you have is still legal? — the higher stays, lower removed ("in rare cases the one you just played is removed; still counts as developing a technology, e.g. for Einstein") [RB p.12, CoL p.5].

7.7 **Level** = age as number (A=0, I=1, II=2, III=3); "best lab/mine/…" = the one of the highest age among cards WITH workers... precisely: level of a unit/building is the level of the card it is on; "best X" = technology card of the latest age [CoL p.8; FAQ Einstein].
<!-- SECTION COMPLETE -->

## 8. Governments

8.1 Exactly one government in play at all times; start = Despotism (4 CA, 2 MA, urban limit 2). Governments are technologies with TWO science costs in the upper-left: higher = peaceful change, lower = revolution [RB p.12-13]. Workers never sit on governments. New government always replaces the old regardless of level [RB p.13].

8.2 **Peaceful change** — a normal Develop a Technology action: 1 CA + the HIGHER science cost; put into play over the old one; update CA/MA totals and urban limit; unspent actions carry over onto the new card; if totals increased, the new tokens are available THIS turn; if decreased, return tokens (spent first) [RB p.12, CoL p.5-6].

8.3 **Revolution** — special develop action [RB p.13, CoL p.5]:
1. ALL of your civil actions must be available (none spent this turn); spend them all.
2. Pay the LOWER science cost.
3. Replace the government; update statistics.
4. Any civil actions GAINED from the new government are spent immediately without effect (you end with 0 available CAs this turn). Military actions are unaffected: MAs spent before or remaining after the revolution stay usable, including extra MAs the new government provides [RB p.13].
5. Exceptions: Breakthrough action card may pay for the revolution with its 1 CA + revolution science cost (all CAs must still be available) [RB p.15, CoL p.5]. Development of Civilization event: revolution for 1 less science, no CA paid, but all CAs still used up [CoL p.12]. Robespierre: pay with all military actions instead of civil [CoL p.12]. Newton: regain 1 CA after a revolution (revolution counts as developing a technology) [CoL p.12].

8.4 Because a revolution requires all CAs available, you cannot take a government from the card row and revolt to it in the same turn (exception: spend-and-refund tricks like leader replacement are allowed as the "only civil action", since replacing gives the action back — RB p.13 states you cannot spend any CA before the revolution unless you get it back).
<!-- SECTION COMPLETE -->

## 9. Leaders & Wonders

9.1 **Leaders** [RB p.6,10-11, CoL p.5,9]:
- Green civil cards; taken from the row into hand (cost by position). Once you take a leader of an age, you may NEVER take another leader of that same age, even after the first leaves play (§2.5).
- Play: 1 CA, from hand into play; effect applies immediately; at most one leader in play.
- Replace: playing a new leader with one in play removes the old one from the game and refunds 1 civil action (only when you actually had a leader in play [RB p.24]). You may use the old leader's benefit earlier in the same turn.
- Leaders never have workers. Effects last while in play.
- Death at age end: a leader is removed from play when it becomes ANTIQUATED — i.e., when age N ends, leaders of ages OLDER than N are removed (an Age A leader dies when Age I ends; an Age I leader when Age II ends, etc.) [CoL p.3, RB p.21]. So a leader in play survives through the age after its own. No refunded action for antiquation.

9.2 **Wonders** [RB p.6,11, CoL p.5,9]:
- Purple cards; taken from the row DIRECTLY into play sideways = unfinished (never in hand; hand limit irrelevant). Cost: printed row CA cost + 1 CA per completed wonder you have (destroyed ones count) (§2.4).
- Only one unfinished wonder at a time; cannot take another while unfinished; no way to abandon an unfinished wonder voluntarily [RB p.6].
- Stages: row of numbers at the bottom = resource cost per stage, paid left to right; each stage built by the Build a Stage action (§3.8); blue token covers the stage (also counts toward corruption avoidance — those tokens are OUT of your bank; an unfinished wonder "contributes to corruption" thematically [RB p.20]).
- Completion: last stage covered → return tokens to bank, straighten, effects begin (permanent symbols + text). Age III wonders instead/additionally have a one-time "You immediately score…" effect on completion [RB p.22, CoL p.9]. Taj Mahal's text applies while in the card row [CoL p.9].
- Wonders never have workers; you may have any number of completed wonders. Each wonder exists once in the deck (no repeats); one-per-name rule also applies.
- Antiquation: an UNFINISHED wonder of an age older than the age that just ended is removed from play (blue tokens on it return to the bank); completed wonders always remain [CoL p.3, RB p.21].
<!-- SECTION COMPLETE -->

## 10. Tactics

10.1 **Current tactic**: each player has at most one, marked by their tactics standard (starts in the common area on no card = no tactic). Chosen by Play a Tactic (1 MA, from hand, becomes your EXCLUSIVE tactic in your play area) or Copy a Tactic (2 MA, any card in the common tactics area). Max one play-or-copy per Action Phase. A new choice replaces the old; armies reorganize immediately [RB p.18, CoL p.6].

10.2 **Going public (2015 shared rule)**: at the start of your turn (after replenishing the row, before Politics), you MUST move any exclusive tactics card from your play area to the common tactics area — so a played tactic is exclusive for exactly one round. Your standard stays on it; it remains your current tactic. Any player may then copy it for 2 MA (even the original owner re-copying it later costs 2 MA). Multiple standards may share one card; cards with no standards stay available; if the same card is already in the common area, merge/remove duplicates (never to the discard pile) [RB p.18, CoL p.3; ubg].

10.3 **Armies** [RB p.18, CoL p.9]: your current tactic depicts a set of unit types; your units automatically form as many complete sets (armies) as possible, each unit in at most one army, grouping chosen to maximize total tactical strength (no choice, always maximal). Each army adds the card's tactical strength to your strength rating, on top of the units' own strength.

10.4 **Outdated armies**: Age II and III tactics print two values (normal / outdated). An army is outdated if ANY of its units is 2+ levels below the tactics card's printed age (card age, not current age). Outdated army = lesser value [RB p.18].

10.5 **Air forces (Age III units)**: no tactic requires them; one air force unit may join any one army, doubling that army's tactical strength (after outdated determination); each air force joins at most one army [RB p.22].

10.6 **Composition matching**: unit type = icon (infantry/cavalry/artillery); level irrelevant for forming the set (any infantry fills an infantry slot) but drives outdatedness. Genghis Khan: infantry may count as cavalry or infantry [CoL p.12].

10.7 **Colonization**: units sacrificed to colonize may form armies per your current tactic; their tactical strength adds to the colonization force (§11) [RB p.18, CoL p.7].
<!-- SECTION COMPLETE -->

## 11. Colonies

11.1 **Trigger**: a "…Territory" card revealed as the current event starts an auction open to all players [RB p.17, CoL p.7].

11.2 **Auction** [CoL p.7]: bidding starts with the player resolving their Politics Phase, clockwise. A bid is a whole number > 0 and > previous bid, and may not exceed the maximum colonization force the bidder can actually send. Pass = drop out permanently. No bids → territory to past events pile. Otherwise last remaining bidder wins and MUST colonize (no backing out), forming a force ≥ their final bid.

11.3 **Colonization force** = sum of [CoL p.7; FAQ p.16]:
- printed strength of the sacrificed military units (≥1 unit mandatory, even if other bonuses would cover the bid);
- tactical strength of any complete armies formed (current tactic) among the sacrificed units;
- colonization modifiers (ship icon) from cards in play (Cartography etc.; NOT discarded, reusable);
- colonization value (bottom half) of any number of military bonus cards played (discarded).
EXCLUDED: strength-rating modifiers (Alexander, Napoleon, Great Wall, Warfare, wonders, leaders, special techs — none apply, neither to the force nor to unit strength) [RB p.17; FAQ p.16]. Arenas are urban buildings, not units — cannot be sent [RB p.17].

11.4 **Sacrifice**: sent units' yellow tokens go to the YELLOW BANK (not worker pool); bonus cards used are discarded. Update strength rating [RB p.17, CoL p.7]. (No red-token cost: colonization costs no actions.)

11.5 **Gaining the colony** [RB p.17, CoL p.7]: territory card into your play area = colony. Apply its PERMANENT effects (bottom symbols: ratings, yellow/blue tokens, colonization modifiers…) FIRST, then the IMMEDIATE effect (center: food/resources/science/culture/free population/military card draws — draws ignore hand limits; nothing in Age IV; if the military deck lacks cards, reshuffle the current age discards [FAQ p.11]).
- Losing/stealing the colony (e.g. Annex aggression, events): owner loses the permanent effects (new owner gains them); the immediate effect is never re-applied nor undone [RB p.17, CoL p.9].

11.6 **Note**: colonization happens within the revealing player's Politics Phase; players other than the current player can win. Bonus military cards spent bidding are discarded before any Strategic Territory card draw (so they can be reshuffled into the new deck) [FAQ p.11].
<!-- SECTION COMPLETE -->

## 12. Age Progression, Game End & Scoring

12.1 **Age sequence**: A → I → II → III → IV. Age A ends at the first card-row replenish (§1.10). Ages I/II/III end when the last card of the current CIVIL deck is dealt to the row (mid-replenish, any player's turn) [CoL p.3]. The MILITARY deck running out never ends an age — reshuffle its discards (§6.6.4).

12.2 **On an age ending (Ages I, II, III end)** — cards of ages OLDER than the age that just ended become antiquated [CoL p.3, RB p.21]:
1. All players discard antiquated cards from HANDS (civil and military).
2. Antiquated leaders removed from play; antiquated UNFINISHED wonders removed (blue tokens back to bank); antiquated pacts removed.
3. Everything else stays: technologies (incl. initial board cards), completed wonders, colonies, exclusive tactics, declared wars, workers on antiquated technology cards, events already in the event decks (never purged [FAQ p.11]).
4. **Each player loses 2 yellow tokens** (from the yellow bank; only what is there) [CoL p.3]. (Not at the end of Age A.)
5. Swap in the next age's civil and military decks on the current age board (shuffled). Entering Age IV: no decks at all.

12.3 **Game end trigger** (all player counts): Age III civil deck runs out → Age IV begins (apply 12.2 with Age II items antiquated). If Age IV begins DURING the starting player's turn, the current round is the last; otherwise the NEXT round is the last [CoL p.3, RB p.22]. Every player gets the same number of turns; the last turn belongs to the player to the right of the starting player [RB p.21,23]. (International Agreement event can trigger this during a Politics Phase refill [CoL p.12].)

12.4 **Age IV restrictions**: card row swept but not refilled; no military card draws; cannot resign; war declarations only count if the attacker gets another turn; no new decks [CoL p.3-4; FAQ p.11].

12.5 **Final scoring** [CoL p.1, RB p.23]:
1. During play, culture accumulates continuously (production, events, wonders, wars, etc.).
2. After the last turn: evaluate ALL Age III events remaining in the current AND future events decks, in any order (Age I/II events remaining are ignored). Preparing an Age III event guarantees its evaluation. Most award culture per cards/workers in play; ranked ones ("14/7/0") use the standard tie-breaker AS IF it were the starting player's turn.
3. Any end-of-game card bonuses are scored (Bill Gates) [CoL p.1].
4. Most culture points wins. **Ties: all tied players share the victory** (no tie-break) [CoL p.1, RB p.23].

12.6 First-game (Handbook) variant scores differently (tech/rating/wonder/colony bonuses, RB p.21) — NOT part of the full game; do not implement for the engine's standard mode.
<!-- SECTION COMPLETE -->

## 13. 2-Player and 3-Player Differences (consolidated)

| Rule | 2p | 3p | 4p |
|---|---|---|---|
| Civil decks I, II, III trimming | remove 9 cards each ("3+" and "4" marks) | remove 3 cards each ("4" marks) | none |
| Military decks | remove ALL pacts (I: 2, II: 4, III: 4) | full | full |
| Card row sweep per turn | 3 | 2 | 1 |
| Age A current events seeded | 4 | 5 | 6 |
| First-round CAs | 1, 2 | 1, 2, 3 | 1, 2, 3, 4 |
| Pact political actions | none (no pacts in game) | yes | yes |
| "Two strongest/weakest civilizations" on cards | read "the stronger/weaker civilization" | normal | normal |

Everything else is identical: card row is 13 spaces with the same 5/4/4 cost bands; event seeding/future-deck mechanics unchanged (each player seeds the single shared future deck via Prepare an Event; there is no double seeding in 2p); wars/aggressions unchanged; end trigger and scoring unchanged [CoL p.2-4,7; RB p.4,8,16; FAQ p.15-16].

Resignation mid-game can convert a 4p game to 3p/2p: sweep by remaining player count; re-trim only FUTURE age decks (remove "4"/"3+" cards and, at 2 players, pacts); current-age decks untouched; 2 players left → apply the 2p event reading [CoL p.4; FAQ p.16].
<!-- SECTION COMPLETE -->

---

# Appendix: card-data provenance and the rulings that produced it

*Migrated from `docs/OPEN_QUESTIONS.md` on 2026-07-30, which was deleted in the
documentation consolidation.  Every item there was RESOLVED; what is preserved
here is the **sourcing and the reasoning**, because that is what stops the same
questions being reopened.  `data/validate_cards.py` used to be the standing
regression check for any edit to the card data; it has since been ported into
`rust/src/card_table.rs`'s test module (seven tests, e.g.
`the_military_decks_are_the_sizes_the_component_list_prints`) and the Python
file deleted — `cargo test --profile difftest` is the regression check now.*

### Card data
1. ~~Action (yellow) card full list + counts per age~~ — RESOLVED 2026-07-26. Roster (14 distinct names, 33 age-variants) confirmed by three independent sources that agree exactly: the digital edition's localization keys (`CivilCards_card_names`: `RICH_LAND_0..2`, `URBAN_GROWTH_0..3`, …), throughtheages.fandom.com "Card List: Digital Edition", and faq_v15.pdf p.12 (which groups all 14 by type). Age A: Rich Land, Engineering Genius, Patriotism, Frugality, Urban Growth, Cultural Heritage, Stock Pile. Age I: + Breakthrough, Reserves; − Cultural Heritage stays, no Stock Pile. Age II: Rich Land, Engineering Genius, Revolutionary Idea, Patriotism, Frugality, Breakthrough, Efficient Upgrade, Wave of Nationalism, Urban Growth, Reserves. Age III: Engineering Genius, Revolutionary Idea, Patriotism, Efficient Upgrade, Endowment for the Arts, Military Build-Up, Urban Growth, Reserves. Exact effects taken from the digital-edition card texts (all values cross-check against an independent 2006-era spreadsheet except Age II Breakthrough, see item 16). Per-age TOTALS are sourced (10/13/13/13, see item 8); the split of those totals between names is now sourced too (item 17, RESOLVED), and Age II Breakthrough is confirmed at 3 science (item 16, RESOLVED).
2. ~~Military deck per-card counts with 2p/3p/4p removals~~ — RESOLVED 2026-07-26 [RB p.4, CoL p.2, FAQ p.15]: military cards carry NO "3+"/"4" corner marks. The only player-count change to the military decks is that ALL pact cards are removed in a 2-player game (Mil I has 2 pacts, Mil II and III have 4 each), giving deck sizes A 10/10/10, I 43/45/45, II 46/50/50, III 41/45/45 for 2p/3p/4p. The "3+"/"4" marks exist only on CIVIL cards — 6 cards marked "3+" and 3 marked "4" in each of Civil I/II/III, i.e. 9 cards removed for 2p and 3 for 3p [RB p.4] (see items 17/18 for exactly which cards). Per-card copies inside a military deck are still derived from the published per-age totals (10/45/50/45 = 150, the box component count), not from a per-card source.
3. ~~Sid Meier (2015) exact ability text~~ — RESOLVED 2026-07-26: same mechanic as 2006 Alex Randolph, confirmed via throughtheages.fandom.com (Alex_Randolph page: "Each of your labs produces culture: 1 per level; each lab produces 1 less science"; "In A New Story of Civilization, it was changed back to Sid Meier") and faq_v15.pdf worked example ("If you have two Age-III Computers Workers, they produce a combined 8 Science and 6 Culture per turn" — implies 2015 Computers = 5 science, minus 1 each). FAQ also confirms Sid Meier affects the culture scored by the Internet wonder. Data updated in data/cards_wonders_leaders.json.
4. ~~Aristotle / Moses / da Vinci / Newton / Einstein / Gandhi 2015 texts unchanged from 2006~~ — RESOLVED 2026-07-26: all six confirmed unchanged via throughtheages.fandom.com card pages (exact texts adopted in data/cards_wonders_leaders.json). Gandhi note: the fandom page marks "3 more military actions" as the EXPANSION variant; the base 2015 card keeps the 2006 "twice the military actions" wording. No FAQ contradictions found.
5. ~~Exact 2015 defense-card values~~ — RESOLVED 2026-07-26: military bonus cards, 6 per age (base game), defense +2/+4/+6 by age (upper half), colonization +1/+2/+3 (lower half). Confirmed namu_military.txt §7 + Code of Laws p.11 card diagram (Age II card shows defense 4 / colonization 2).
6. ~~Exact resource-gain formula on Raid aggressions~~ — RESOLVED 2026-07-26: the card itself states the rounding. Digital-edition card texts (`RAID_1/2/3`): Age I "Destroy 1 Age A or I urban building. Gain half the resources needed to build it (rounded up)"; Age II "…2 urban buildings: one of Age II or older and one of Age I or older…"; Age III "…one of Age III or older and one of Age II or older…". So: URBAN buildings only, half the build cost per destroyed building, ROUNDED UP (a cost-3 building yields 2 resources). FAQ p.8 fixes the base value: the building's ORIGINAL PRINTED cost, ignoring construction special technologies on either player's board. NamuWiki's looser "half of building cost" wording was the source of the earlier ambiguity. Data in data/cards_military_actions.json matches.
7. ~~Spy (Age II aggression) MA cost~~ — RESOLVED 2026-07-26: 1 MA (namu_military.txt). CARD-DATA cross-check remains for card counts.
8. ~~Age A "current events" composition~~ — RESOLVED 2026-07-26. Seeding: shuffle the 10-card Age A military deck, place the top (players + 2) cards face down as the current events deck (2p:4, 3p:5, 4p:6), return the rest to the box unseen [RB p.5, CoL p.2]. The deck itself is always the same 10 cards, one copy each (so every Age A event has count 1/1/1 in the data; WHICH of them are seeded is random, it is not a player-count trimming): Development of Agriculture, Crafts, Civil Life, Markets, Politics, Religion, Science, Settlement, Trade Routes, Warfare — matching throughtheages.fandom.com "Card List: Digital Edition" and the 10-card Age A military deck total.

   Related and also resolved: the **Age A CIVIL deck is exactly 20 cards** = 6 Age A leaders + 4 Age A wonders + 10 yellow action cards. The six Age A technologies (Warriors, Agriculture, Bronze, Philosophy, Religion, Despotism) are printed on the player boards and are NOT deck cards (they carry count 0 in data/cards_civil.json). 13 of the 20 are dealt to the card row at setup and the rest cover the first replenish; the remainder goes back in the box when Age A ends [RB p.4, p.8].

16. ~~Age II **Breakthrough** science value~~ — RESOLVED 2026-07-26: **3 science**. The BGA Studio implementation of the 2015 edition (github.com/srussking/throughtheages, `material.inc.php`; extract in `sources/bga_card_counts.tsv`) gives Age I "Develop a technology. After you pay the science cost, score 2 science" and Age II "…score 3 science". That is a third source agreeing with the digital edition's card text against the 2006-era spreadsheet's 4, so 4 is the 2006 value. Data already used 3; unchanged.

17. ~~Split of the yellow action-card copies between names within Ages I, II and III~~ — RESOLVED 2026-07-26 by two independent 2015-edition sources that agree card-for-card:
    * the **BGA Studio** implementation (`sources/bga_throughtheages_material.inc.php` → `sources/bga_card_counts.tsv`), whose every card carries `qt2`/`qt3`/`qt4`; and
    * the **Tabletop Simulator** workshop mod 2120085710 (`sources/tts_tta_workshop_2120085710.json`), whose civil decks are physically pre-split into `Civil_N` / `Civil_N (3+)` / `Civil_N (4)` piles.

    No action card carries a "3+" or "4" corner mark, so every action-card count is the same at 2/3/4 players. Final distribution (totals 10/13/13/13 as previously sourced):
    * **Age A (10):** Rich Land 2, Frugality 2, Urban Growth 2, Engineering Genius 1, Patriotism 1, Cultural Heritage 1, Stock Pile 1.
    * **Age I (13):** Rich Land 2, Frugality 2, Urban Growth 2, Breakthrough 2, Reserves 2, Engineering Genius 1, Patriotism 1, Cultural Heritage 1.
    * **Age II (13):** Breakthrough 2, Reserves 2, Efficient Upgrade 2, Rich Land 1, Frugality 1, Urban Growth 1, Engineering Genius 1, Patriotism 1, Revolutionary Idea 1, Wave of Nationalism 1.
    * **Age III (13):** Reserves 3, Urban Growth 2, Revolutionary Idea 2, Efficient Upgrade 2, Engineering Genius 1, Patriotism 1, Endowment for the Arts 1, Military Build-Up 1.

    Corrections applied to data/cards_military_actions.json: Age I Breakthrough 1→2, Age I Reserves 3→2, Age II Breakthrough 1→2, Age II Urban Growth 2→1, Age III Endowment for the Arts 2→1, Age III Reserves 2→3. All `uncertain` fields on action cards removed and replaced with a `countSource` field. Age A's split was already right.

18. ~~Age I civil deck one card too large (54 at 4 players)~~ — RESOLVED 2026-07-26: **Swordsmen has only 2 copies at 4 players (2/2/2)**; Iron, Alchemy and Knights are the three cards marked "4" (2/2/3). NamuWiki's "2/2/3" for Swordsmen is simply wrong. Both 2015-edition sources from item 17 agree: BGA gives Swordsmen `qt2=qt3=qt4=2`, and in the TTS mod `Civil_I (4)` contains exactly Alchemy, Iron, Knights while Swordsmen sits only in the 44-card base pile. The same two sources confirm the other two decks' marks: Age II "4" = Cannon, Republic, Selective Breeding; Age III "4" = Air Forces, Military Theory, Professional Sports (so Democracy's extra copy is indeed a "3+" copy). Age I "3+" = Bread and Circuses, Code of Laws, Drama, Monarchy, Theology, Warfare.

    Two further Age III corrections fell out of the same cross-check: Computers 2/2/3 → **2/2/2** and Military Theory 1/1/1 → **1/1/2** (Military Theory, not Computers, is the third Age III "4" card). `python3 data/validate_cards.py` is now completely clean: civil A 20 / I,II,III 44-50-53, 179 civil + 150 military.

### Rules — ALL RESOLVED 2026-07-26 (see docs/RULES_SPEC.md)
9. ~~End-of-turn sequence order~~ — RESOLVED [CoL p.6, RB p.7/20/24, RULES_SPEC §6.6]: (1) discard excess military cards; (2) uprising check — skip (3) entirely if discontent > unused workers; (3) Production Phase: score science+culture → corruption → food production → food consumption → resource production; (4) draw military cards (max 3; NOT skipped during uprising — 2015 change); (5) reset actions. namu_main's description matches; 2015 moved corruption before production.
10. ~~2p politics/seeding~~ — RESOLVED [CoL p.2/4, FAQ p.15-16, RULES_SPEC §13]: no double seeding; one shared future events deck as normal; 2p differences are only: pacts removed from military decks, 4 Age A events seeded, "two strongest/weakest" read as "stronger/weaker", 9 cards removed from each of Civil I–III.
11. ~~Card row sweep/bands~~ — RESOLVED [RB p.6/8, CoL p.3, RULES_SPEC §2]: sweep 2p:3 / 3p:2 / 4p:1 leftmost spaces (never sweep beyond them even if empty, FAQ p.11); row is always 13 spaces; cost bands identical for all player counts: spaces 1–5 = 1 CA, 6–9 = 2 CA, 10–13 = 3 CA.
12. ~~War resolution when attacker weaker~~ — RESOLVED [CoL p.3, RULES_SPEC §5.7-5.8]: either side can be victor; victor = strictly higher strength at resolution; difference = strength advantage fed into the card formula; equal = no effect, card discarded. No minimum spoils beyond card base (War over Territory: 1 token + 1 per full 5 advantage); spoils capped by what the loser actually has (FAQ p.8).
13. ~~Final-scoring ties~~ — RESOLVED [CoL p.1, RB p.23]: tied players share the victory. (Within event evaluation at game end, statistic ties break as though it were the starting player's turn.)
14. ~~International Agreement final-turn edge cases~~ — RESOLVED [CoL p.12] (base-game card is "International Agreement"; "Politics of Power"/"International Convention" are not base-2015 names): taking cards follows the usual rules; if the current player takes cards, action cards taken may be used the same turn; replenish the row afterward WITHOUT discarding from the first slots; if this replenish exhausts the Age III civil deck it triggers game end (starting player's turn → that round is the last); the strongest player may use the option even in the last round.
15. ~~Military deck exhaustion vs age end~~ — RESOLVED [CoL p.6, FAQ p.14]: only the CIVIL deck ends an age. An exhausted military deck is rebuilt by reshuffling that age's military discard pile (excluding past-events pile); age continues. namu_main confirmed correct.

### Rules — RESOLVED 2026-07-26 by the user (raised while implementing the yellow action cards)
19. ~~**`resourcesForMilitaryUnits` — total pool or per-unit discount?**~~ — RESOLVED 2026-07-26 by the user: **TOTAL POOL** for the turn, which is what the engine already did. Original note follows.

    **`resourcesForMilitaryUnits` — total pool or per-unit discount?** Patriotism (A/I/II/III), Wave of Nationalism and Military Build-Up read "Build or upgrade military units; pay N fewer resources" (plural "units"). RULES_SPEC §3.11 [RB p.14-15] says an ordered action is performed once, but these cards do not order a single build — Patriotism's ordered part is the extra military action, and the discount rides along with it. The engine models the discount as a TOTAL pool spent across that turn's military unit builds/upgrades (`PlayerState.mil_discount`, reset in `economy.end_of_turn`). The alternative reading is N off EVERY military unit built that turn, which is strictly stronger. Needs the printed card or a FAQ line. ENGINE item — `engine/actions.py::_h_play_action`.
20. ~~**Order of an action card's gains vs its ordered action.**~~ — RESOLVED 2026-07-26 by the user: the ordered action resolves **FIRST**, at full price, and the gains land **after** it. "You can't use the science you gain to pay for the thing — that's the whole point of that wording." So Breakthrough only develops a technology you could already afford, and banks its science afterwards; Frugality only increases population you could already feed. ENGINE UPDATED: gains now ride a `card_gains` queue item enqueued behind the `free_civil` order, `_action_card_playable` checks the order against the player's real pools, and the throwaway gain-probe clone is gone. Tests `test_gains_land_after_the_ordered_action` and `test_frugality_food_lands_after_the_population_increase` pin it. Original note follows.

    **Order of an action card's gains vs its ordered action.** Breakthrough reads "Develop a technology at full price; gain N science" and Frugality "Increase your population at full price; gain N food" — printed order is action-then-gain, but resolving the gain FIRST makes the 2015 wording exactly equivalent to the older editions' "pay N less", which is almost certainly the intent (and is how the +science can pay for the technology being developed). The engine resolves gains first. Needs confirmation. ENGINE item — `engine/actions.py::_h_play_action`, `_action_card_playable`.
