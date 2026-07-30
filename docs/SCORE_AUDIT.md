# Does every card type score right? (2026-07-30)

Scope: the **2015 base game**, all 23 card types, 236 cards. The question is
narrow and end-of-game shaped: *when the game ends, does each card contribute
exactly the culture, science, strength, happiness, food, resources and
civil/military actions the printed rules say it should?*

New file: `tests/test_score_audit.py` — **167 tests, one section per card
type**, every expected number derived from the printed card by hand in its
own docstring. `bash tools/gate.sh` is GATE PASS; **no engine file is
touched, so all eight fingerprint digests are unmoved on purpose** (same
discipline as `docs/SCORE_VALIDATION.md`, which also handed its findings over
rather than fixing them in the same breath).

## One-paragraph answer

**Sixteen of the 23 types score exactly right, and eight real bugs came out
of the other seven.** None is large: most are worth 1-6 culture in the
positions that reach them, and three of them (Michelangelo and St. Peter's on
a ruined wonder, St. Peter's on a colony, and the air force) are *per turn*
and *per army* rather than one-off. Every one of the eight is the same shape
as the two bugs this project has already shipped: **a value that lives in a field, or a card
clause, that no reader touches.** The verifications recorded in
`docs/SCORE_VALIDATION.md` §6.1 (wonder rules and stage costs) and §3.3
(Hollywood/Internet leader modifiers) **still hold at master `6968256`** and
are now pinned by tests instead of by a corpus run. Tonight's government
pricing fix is real: all eight governments' `civilActions` /
`militaryActions` / `urbanBuildingLimit` / `peacefulCost` / `revolutionCost`
now reach the engine, and `EveryFieldHasAReader.test_every_government_field_is_read`
fails if any of the five stops being read.

The eight bugs are asserted as tests **with the rules answer**, marked
`@unittest.expectedFailure`. Python reports an unexpected success as a suite
failure, so each one goes red the moment it is fixed and the decorator has to
come off deliberately. No test in this file asserts a wrong answer.

---

## 1. The two things that were verified before, re-checked at current master

| claim | where it came from | status at `6968256` |
|---|---|---|
| all 16 wonders, all 53 stage costs, exact | SCORE_VALIDATION §6.1, 18,307 human stage lines | **holds** — `Wonder.*`, and the costs are still read from `data/cards_wonders_leaders.json` |
| `Impact of Wonders` 5/4/3/2 by age, exact | SCORE_VALIDATION §6.1, 565/565 rows | **holds** — `test_impact_of_wonders_pays_by_age` |
| Hollywood/Internet score **effective** building output, not printed | SCORE_VALIDATION §3.3, fixed in SCORE_BUGFIX | **holds** — `test_hollywood_uses_effective_output_not_printed`, `test_internet_matches_the_FAQ_sid_meier_example` (the FAQ's own 8-science/6-culture Sid Meier example) |
| Chaplin doubles one theater, not a card | SCORE_BUGFIX | **holds** — `test_chaplin_doubles_ONE_theater_not_the_card` |
| `Impact of Industry` scores mines, not the resource rating | SCORE_BUGFIX §3.1 | **holds** — and see bug 3.1 below, which is the *same card clause on the farm side*, still unfixed |
| `Impact of Population` counts unused workers | SCORE_BUGFIX §3.2 | **holds** — `test_impact_of_population` |

## 2. The field-coverage sweep

The method the two shipped bugs demand: enumerate every field in the card
data and find the reader.

* **200 distinct effect keys** across 236 cards. **146 are read** by
  `engine/*.py` as a quoted string.
* **54 are not read at all.** Every one is accounted for:
  * **31 are leader abilities dispatched on `p.leader == "..."`**, which
    `engine/effects.py`'s own header declares as the design. The ability
    exists; the *number* in the data is decorative.
  * **10 are constants the engine spells out in Python** — the war spoils
    (1 token + 1 per 5 advantage; 5 + advantage culture), the ruins' 2
    culture, `tacticBonus`/`tacticBonusObsolete` (which duplicate the
    top-level `strength`/`obsoleteStrength` that `_army_value` actually
    reads), the air force's doubling.
  * **11 are prose or documentation** of behaviour implemented structurally
    (`note`, `order`, `ignore`, `statistics`, `chosenBy`, `duration`, ...).
  * **2 are genuinely unimplemented card clauses** — bugs 3.2 and 3.8 below.
* Top-level fields: `urbanLimitCategory` is read by nobody because the urban
  limit is applied per card **type**, and the two agree on all 16 urban cards
  (`test_urbanLimitCategory_is_the_card_type`). `scoringEvent` is read by
  nobody because `evaluate_final_events` selects by **age**; the two agree on
  all 55 events, and `test_every_age_III_event_is_a_scoring_event_and_vice_versa`
  now fails if they ever stop agreeing.

Two guard-rail classes make this permanent rather than a one-off sweep:

* **`HardcodedConstantsMatchTheData`** — every constant the engine hardcodes
  for a name-dispatched card is asserted equal to the card's own data. If
  someone corrects the data, the engine's silent copy is caught.
* **`EveryFieldHasAReader`** — re-runs the sweep inside the suite and fails on
  any **new** unread key. The 54 are listed by name with a one-line reason.

Two dead reads found and left alone (harmless, but they are the same shape):
`compute` calls `_add_production(s, card.get("permanent"))` for colonies —
no card has a `permanent` key, the data spells it `permanentEffects` — and
`_add_production(s, ...get("production"))` for the leader, which no leader
has.

---

## 3. The eight bugs

Ordered by how much I would trust the finding, most-certain first.

### 3.1 `Impact of Agriculture` scores the food **rating**, not the farms

`engine/events.py:399` reads `culturePerFoodProducedByFarms` as `v * s.food`.
The card says *"culture equal to the food produced by their farms"*. This is
**`Impact of Industry` (SCORE_VALIDATION §3.1) again, on the other card** —
that one was fixed to `mine_resources(p)` and this one was not.

What leaks in: a pact's food symbol (`International Trade Agreement`, side B,
`foodProduction: 1`). Nothing else in the base game adds food outside farms
today, which is why the corpus run scored this card 66/66 — **the corpus
cannot see it, because it never modelled pacts.** Worth 1 culture now, and
worth whatever any future food-producing card is worth.

The fix is one line and already written elsewhere in the file:
`building_output(p, frozenset({"farm"}), ("food",))`. Note the `+4` bonus
clause is *correct* as it stands: "if production exceeds consumption" is
about the whole rating, not about farms.

Test: `ScoringEvents.test_impact_of_agriculture_scores_FARMS_not_the_food_rating`
(7 vs the rules' 6).

### 3.2 Bill Gates pays his culture at game end but not when he **leaves play**

*"When Bill Gates is removed from the game **or the game ends**, gain culture
equal to that extra resource production."* `effects.end_of_game_bonus`
implements the second half. Nothing implements the first:
`actions._h_play_leader` calls `effects.on_leave_play`, which only moves blue
and yellow tokens, and so does the `Iconoclasm` event's
`discardLeaderUnlessCurrentAge` path — so a player who replaces Gates, or has
him discarded, loses the whole bonus. Two Computers workers is 6 culture;
four is 12.

`cultureOnLeaveEqualToLabResourceProduction` is one of the two effect keys in
the data with no reader anywhere — the exact shape from the brief.

Test: `Leader.test_bill_gates_also_pays_when_he_LEAVES_play` (0 vs 6).

### 3.3 A **ruined** wonder still feeds Michelangelo and St. Peter's

`compute` is explicit that a wonder flipped by Ravages of Time is ruins:
phase 2 skips its effects and pays 2 culture instead, and
`_output_modifiers` filters `p.flipped_wonders` out. Two places do **not**:

* `_apply_modifier`'s `culturePerHappyFromTemplesTheatersWonders`
  (Michelangelo) iterates `p.completed_wonders` unfiltered, so a ruined
  Hanging Gardens still pays 2 culture **every turn**.
* `_happy_source_count` (St. Peter's `extraHappyPerHappySource`) counts a
  ruined wonder as a happy source, so it still grants +1 happy face.

Both contradict the card ("its effects no longer apply") and the engine's own
established reading — `tests/test_scoring_bugfix.py` already has
`test_a_flipped_railroad_is_ruins_and_doubles_nothing`. Same rule, three
readers, one of them filtering and two not.

Tests: `Leader.test_michelangelo_does_not_pay_for_a_RUINED_wonders_happy_faces`
(6 vs 4), `Leader.test_st_peters_does_not_count_a_RUINED_wonder_as_a_happy_source`
(5 vs 4).

### 3.4 A second air force doubles the **wrong** army's bonus

`_army_value` ends with
`total += min(air, total_armies) * (val if fresh_armies else old_val)`.
Each air unit may join one army (data text; §10.5), and doubling an
**outdated** army's bonus is worth `obsoleteStrength`, not `strength`. With
one fresh and one outdated army and two air units the engine pays
`2 x 10 = 20` where the rules pay `10 + 5 = 15`.

Narrow — it needs two air workers *and* a mixed-age army set — but it is
strength, and strength is `Impact of Strength`'s 10/14/15 culture and every
war and aggression in the game.

Test: `AirForce.test_a_second_air_force_doubles_the_OUTDATED_armys_smaller_bonus`
(35 vs 30).

### 3.5 "The **players** with the most X" affects only one player

`docs/RULES_SPEC.md` §5.3, citing CoL p.7: *"'All civilizations' with
most/least: **all tied civs affected, no tie-break**."* `resolve_event`
handles `playersWithMostHappyFaces` (Immigration) and
`playersWithMostDiscontentWorkers` (Civil Unrest) in the same loop as the six
singular `strongestPlayer` / `weakestPlayer` keys and slices `[:1]`, so on a
tie exactly one player is affected, chosen by turn order.

Both cards are printed plural. Immigration's population reaches
`Impact of Population` (2 culture per content worker over ten); Civil
Unrest's blue token reaches corruption, and corruption reaches resources.

Tests: `PluralTargets.test_immigration_grows_EVERY_player_tied_on_happy_faces`
(`[1,0,0]` vs `[1,1,1]`), `PluralTargets.test_civil_unrest_taxes_EVERY_player_tied_on_discontent`.

### 3.6 Winston Churchill's military option is unrestricted

*"3 science points **usable only to develop military unit technologies** and
3 resources **usable only to build or upgrade military units**"*.
`_h_churchill` grants plain science and plain resources. The three data keys
that say so (`militaryOption`, `scienceForMilitaryTechs`,
`resourcesForMilitaryUnits` under `perTurnChoice`) have no reader.

This makes the military option strictly stronger than printed, so it inflates
whatever the player spends the science on — which at 3 science a turn over an
Age III game is not nothing. It is a *play* bug more than a scoring bug, so it
is listed here and left to the plumbing lane rather than tested as a score.
(The card's other numbers are right: `test_churchills_culture_option_is_3`.)

### 3.7 St. Peter's does not count a **colony** as a happy source

*"every building/**card** providing happy faces provides one additional happy
face"*. `_happy_source_count` walks the player's technologies, completed
wonders, leader and government — but not `p.colonies`, so a Historic
Territory's happy face earns no extra one.

This is not a "building vs card" judgement call, because **the engine has
already made that call**: it counts the government card and the leader card,
neither of which is a building. Under that reading a colony card is a card
and must count; under the stricter "building" reading the government and the
leader would have to stop counting, which would be a bigger change and
contradicts the printed word. Worth 1 happy face per happy-face colony, and a
happy face is 2 culture on `Impact of Happiness` plus a discontent worker
avoided.

Tests: `Leader.test_st_peters_counts_a_COLONY_as_a_happy_source` (5 vs 6),
with `test_st_peters_does_count_the_government_and_leader_cards` pinning the
reading it is judged against.

### 3.8 `War over Technology`'s alternative spoil is unimplemented

*"The victor takes science equal to the strength advantage, **or takes
special (blue) technologies of the same total cost**."* `resolve_war` always
takes science. `orTakesSpecialTechnologiesOfSameTotalScienceCost` is the
second effect key in the data with no reader. Taking the science is never
worse than nothing, so this costs the victor at most the difference between
the two options — flagged, not fixed, and it is a genuine choice the engine
does not offer.

---

## 4. The 23 types, one at a time

Every row's tests are in `tests/test_score_audit.py` under the named class.

| # | type | what the rules say | verdict |
|---|---|---|---|
| 55 | **event** | 15 Age III "Impact of" cards pay the end-game culture; the other 40 move culture/ratings during play | **14 of the 15 scoring events exact, 1 wrong** (3.1); of the other 40, **1 wrong** (3.5). All 15 checked by hand; ranking tie-breaks correct on **both** paths (current player mid-game, start player at game end, RULES_SPEC 5.3/12.5) |
| 33 | **action** | yellow cards: a free civil action plus a printed gain | **exact**. `Endowment for the Arts` pays per richer civilization at the right per-player rate; the ordered-action-then-gains sequence is already ruled and tested |
| 24 | **leader** | 24 distinct abilities, mostly name-dispatched | **21 exact, 3 wrong** (3.2 Bill Gates, 3.3 Michelangelo, 3.6 Churchill). Level arithmetic confirmed against the FAQ's Sid Meier example: Age A = level 0, so Newton on a lone Philosophy adds nothing |
| 16 | **wonder** | stage costs, flat benefits, 4 Age III one-time bombs | **15 exact, 1 wrong** (3.3 / 3.7, St. Peter's on ruins and on colonies). All four bombs re-derived by hand: First Space Flight sums technology levels **including the government**, Fast Food Chains 2/1 per worker, Hollywood 2x theater+library culture, Internet culture+science+strength of urban buildings (happy excluded) |
| 15 | **tactic** | one army per copy of the composition; `obsoleteStrength` for an army holding a unit more than one age older | **exact**, including Genghis Khan's infantry-as-cavalry. `tacticBonus` in `effects` duplicates the top-level `strength` the engine reads; now pinned equal |
| 12 | **special-tech** | 4 icons, at most one per icon in play | **exact**. Build discounts, wonder stages, actions, strength, colonization all land; Masonry correctly leaves Age A alone. The one-per-icon rule is what makes `Impact of Variety` counting them by name correct |
| 12 | **territory** | immediate bonus, then a permanent one | **exact** on its own terms (token grants applied once, not as production; rating symbols reach `Stats`; losing a colony takes the permanent bonus back) -- but see 3.7, where a colony's happy face is invisible to St. Peter's |
| 11 | **aggression** | culture/science/resource theft, capped by what the victim has | **exact**. A failed aggression (defense >= attack) moves nothing |
| 10 | **pact** | per-turn rating changes on one or both parties, some sided A/B | **exact**, all ten. Includes the negative sides (Loss of Sovereignty -2 culture, Acceptance of Supremacy -1 resource) and the floor at 0 |
| 8 | **government** | civil/military actions, urban limit, production, two prices | **exact** — this is tonight's fix, and all five fields are now pinned. Fundamentalism's -2 science floors at 0 rather than going negative |
| 4 | **farm** | 1/2/3/5 food per worker | **exact** (but see 3.1 for the card that scores them) |
| 4 | **mine** | 1/2/3/5 resources per worker | **exact**, including the Railroad's one doubled worker |
| 4 | **lab** | 1/2/3/5 science per worker | **exact** |
| 4 | **infantry** | 1/2/3/5 strength per worker | **exact** |
| 3 | **library** | 1/1, 2/2, 3/3 science and culture | **exact** |
| 3 | **temple** | 1 culture, 1/2/3 happy | **exact** |
| 3 | **theater** | 2/3/4 culture, 1 happy | **exact** |
| 3 | **arena** | 2/3/4 happy, 1/2/3 strength | **exact** — the only urban building that makes strength, which is why the Internet has to count it |
| 3 | **cavalry** | 2/3/5 strength per worker | **exact** |
| 3 | **war** | spoils by strength advantage | **2 exact, 1 partial** (3.8). Draws move nothing; the **defender** can win and take the spoils |
| 3 | **bonus** | played from hand during a defense/colonization | **exact** — and, importantly, they contribute **nothing** while sitting in hand |
| 2 | **artillery** | 3/5 strength per worker | **exact** |
| 1 | **air** | 5 strength, and doubles one army's tactic bonus | **wrong** (3.4) |

---

## 5. Composition — where single-card tests pass and the game still scores wrong

The brief's warning, tested directly:

* **leader x building**: Chaplin + Movies + Printing Press feeds Hollywood 18,
  not 10 — the leader modifies what the *building* produces, so the wonder
  that scores the building's output has to see it. Sid Meier's -1 science per
  lab and +1 culture per level compose to the FAQ's exact 8/6.
* **wonder x wonder x special tech**: Democracy 7 + Pyramids 1 + Kremlin 1 +
  Civil Service 2 = 11 civil actions, and `Impact of Government` scores
  `2 x 11 + 1 x 4 = 26` off the *totals*, not off the government alone.
* **ruins x everything**: a flipped Pyramids pays no civil action, a flipped
  Library of Alexandria keeps its hand limit only if it is not the flipped
  one, and both still score `Impact of Wonders` at their printed age (CoL
  p.12). Two readers get the "effects are gone" half wrong (3.3).
* **clamps**: no rating is ever negative (`Limits on Ratings`) — checked on
  Fundamentalism's -2 science, Sid Meier's -1-per-lab, Loss of Sovereignty's
  -2 culture. Happiness is additionally clamped to **8**, which is *not* a
  printed rule; it is invisible to scoring because `happy_required` maxes at
  8 and `Impact of Happiness` caps at 16 culture = 2 x 8, and it is now
  asserted so that a change to it has to be deliberate.
* **the whole payout**: one position, three sources at once — an unrevealed
  Age III event, a second in the future deck, and Bill Gates — run through
  `game._finish_game`, asserted to the point.

---

## 6. Two things the brief asked for that are **not in this tree**

Both were named as landed; neither exists at master `6968256`, and
`git cat-file -t 66e4344` is "not a valid object name":

* **`engine/bots/board_yields.py`** and the swap-in diff of `66e4344`. There
  is no such file and no such commit. The replacement-semantics question the
  brief asks (does the diff faithfully represent swapping leader A for leader
  B, including the engine's clamps?) is worth asking *of that code when it
  lands*, because `compute` clamps six ratings at 0 and happiness at 8 — a
  diff of two clamped `compute` results is **not** the card's contribution
  whenever either side is at a clamp. That is a live trap for a diffing
  pricer, and it is the first thing to test on that file.
* **`final_event_culture(state)`** does not exist. The two implementations it
  is meant to unify *do* both exist and **do agree today**:
  `events.scoring_culture` is the single body, called by `_apply_player_block`
  (the real payout) and by `evaluate_final_events` (the end-of-game sweep).
  The divergence risk is in the **ranking** half, which is duplicated: the
  `rankingCulture` block is spelled out twice, once in each caller, with the
  tie-break start index differing (revealer vs `start_player`) — correctly,
  per RULES_SPEC 5.3 and 12.5, but by two copies of the code rather than one.
  Both copies are now pinned by tests
  (`test_a_mid_game_ranking_reveal_breaks_ties_toward_the_CURRENT_player`,
  `test_the_END_OF_GAME_ranking_breaks_ties_toward_the_START_player`), which
  is the divergence test the brief asked for, on the code that actually
  exists.

`effects.culture` and `effects.science` (priced in `96a5db2`) are the short
spelling of per-turn production that `FLAT_KEYS` maps to `culture`/`science`;
the ten wonders and two leaders that use it are covered by
`Wonder.test_flat_benefits` and `Leader.test_flat_rating_leaders`, so the
pricer's map and the rules engine's map are now checked against the same
cards.

---

## 7. What I could not verify

* **Nothing here is corpus-checked.** These are hand-derived positions. Where
  a claim of mine disagrees with `docs/SCORE_VALIDATION.md`'s corpus numbers,
  the corpus wins — except on 3.1, where the corpus provably *cannot* see the
  bug because the replayer models no pacts.
* **`Impact of Happiness` remains open** for the reason SCORE_VALIDATION §8
  gives: the human journal never prints happy faces, so its 70.2% agreement
  is unresolved rather than exonerated. Every input I can check by hand
  (2 per face, 16 cap, -2 per discontent worker, the discontent table) is
  right, so if a bug is hiding there it is in a happy-face *source*, not in
  the card.
* **Aggression side effects** (Raid's half-cost resource refund, Annex,
  Infiltrate) resolve through `interact` queues; I checked the culture and
  the transfers, not the full raid/annex resolution.
* **3p/4p** ranking tables are asserted from the card data, and the 3p path
  is exercised; 4p is not.

## 8. Reproducing

```
python3 -m unittest tests.test_score_audit -v      # 167 tests
bash tools/gate.sh                                 # GATE PASS, digests unmoved
```

To see the eight bugs fail with the rules answer rather than being skipped:

```
python3 - <<'PY'
import re, sys, unittest
src = re.sub(r'^(\s*)@unittest\.expectedFailure\n', r'\1pass\n',
             open('tests/test_score_audit.py').read(), flags=re.M)
open('/tmp/_xf.py', 'w').write(src)
sys.path[:0] = ['/tmp', '.']
import _xf
unittest.main(module=_xf, argv=['x'], exit=False)
PY
```
