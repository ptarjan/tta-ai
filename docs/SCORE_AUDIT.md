# Does every card type score right? (2026-07-30)

Scope: the **2015 base game**, all 23 card types, 236 cards. The question is
narrow and end-of-game shaped: *when the game ends, does each card contribute
exactly the culture, science, strength, happiness, food, resources and
civil/military actions the printed rules say it should?*

New file: `tests/test_score_audit.py` — **176 tests, one section per card
type**, every expected number derived from the printed card by hand in its
own docstring, never copied out of the engine.

This landed in two commits on purpose. The **first** added the tests with the
nine bugs asserted as `@unittest.expectedFailure` and touched no engine file,
so every fingerprint digest was provably unmoved and each bug was shown to
fail *for the right reason* before anything was changed. The **second** fixes
them.

**The fixes are live: all eight fingerprint arms moved**, derived twice
independently and agreeing byte for byte, with a clean-base control first and
a per-fix attribution after. Section 9 has the table, and 9.2 records why the
*first* derivation was thrown away rather than written down.

## One-paragraph answer

**Sixteen of the 23 types scored exactly right as found, nine real bugs came
out of the other seven, and all nine are fixed — so 22 of 23 are exact now.**
A tenth defect, in the *pricer*, fell out of re-auditing
`engine/bots/board_yields.py`. The single type still not exact is **war**, and
what is missing there is a player *choice* the engine does not offer (the
victor of `War over Technology` may take blue technologies instead of
science), not a number it gets wrong. None is large: most are worth
1-6 culture in the positions that reach them, but four are *per turn* rather
than one-off and one is *per army*. Every one of the nine is the shape this
project has already shipped twice: **a value that lives in a field, or a card
clause, that no reader touches — or two readers of one rule that quietly
disagree.**

## The finding that outlives the bugs: a corpus validates only what it varies

`docs/SCORE_VALIDATION.md` scored `Impact of Agriculture` **66/66 exact**
against BGO. That card scores the wrong quantity (bug 3.1). Both statements
are true, and the reconciliation is the most useful thing in this document:

> **At 2 players every pact is removed from the game**, and the corpus is
> **2p only**. A pact's food symbol is the *only* thing in the 2015 base game
> that puts food on your board from outside a farm. So in every game in the
> corpus, "the food your farms produce" and "your food rating" are
> **identically equal** — and no quantity of that data could ever separate the
> two hypotheses. The 66/66 is real. It is 66/66 over inputs that cannot tell
> right from wrong.

This generalises well past one card. **Five of the nine bugs sit inside the
corpus's four documented blind spots** — pacts (structurally absent at 2p),
Iconoclasm and leader replacement (gated *out* of clean rows), Ravages of Time
(no flip was ever applied until a late name-resolution fix), armies and
tactics (never modelled), and happy faces (never printed in the journal). The
table is in §6.5. That is a finding about our validation method, not about the
cards, and it does not impugn the corpus work, which found three real bugs and
states all four limits out loud.

The counter-example does as much work as the finding. **Bug 3.9 was
*decided* by the corpus** — whether an unstaffed lab pays Einstein — because
unstaffed labs are common in ordinary 2p play, so those games *do* separate
the hypotheses, decisively (7303/7600 rows against 7275/7600).

> **The corpus is decisive exactly where it has variation, and silent exactly
> where it does not.** Before quoting a percentage from it, ask what inputs
> produced it and whether they could have distinguished the alternative. A
> card that names a *source* ("the food produced by their farms") rather than
> a rating can only be validated by games that put the source and the rating
> apart.

## How a rules call gets made

Bug 3.9 is the template, because the card wording alone had already been read
two ways *inside one file*. Three independent sources agreeing, not one
argued well:

1. **The printed card** — all three leaders say the best lab or library
   *produces* extra science, and in this game a building **is** a worker
   standing on a technology card.
2. **A published ruling on the identical phrase** — FAQ v1.5 p.9 resolves the
   Transcontinental Railroad as "one worker on the best mine technology card
   **that has workers**".
3. **Measurement against BGO** — 150 human games, same replayer, only the
   reading changed: **7303/7600** per-turn science rows exact staffed against
   **7275/7600** unstaffed.

And the fix is generalised rather than applied: `UnstaffedBuildingsProduceNothing`
asserts the rule for **every** key in `effects._BUILDING_OUTPUT`, and
`test_the_table_covers_every_building_output_key` fails if a new modifier key
is added without being covered. A future key cannot reintroduce this bug
quietly.

## A caveat on the swap-diff technique, which three lanes now rely on

`engine/bots/board_yields.py` prices a card by putting it on the board, calling
the real rules engine, and diffing — so it "does not reimplement a single rule"
and "can never drift". The diff is faithful: replacement is a delta, clamps are
handled correctly (a diff of two clamped `compute` results *is* the marginal
value), and Michelangelo prices at +6 culture/turn with two temples and 0 on an
empty board.

**But the guarantee is narrower than it looks, and it failed once already.**
`card_potential` prices a swap card by the diff **alone**. Blue tokens are not
a `Stats` field — `on_enter_play` puts them on `p.blue_total` — so `compute`
cannot report them, and turning board pricing on **silently dropped Taj Mahal's
blue token** that the static `_card_yields` did price. The pricing guardrail
could not see it, because `blueTokens` is priced *somewhere*.

> **A swap diff is exact over `Stats` and blind to everything else.** Anything a
> card does that is not a per-turn rating — token grants, one-time culture,
> boolean flags, triggers — is invisible to it by construction, and if the
> static path priced that thing, replacing the static path *loses* it. Every
> key a swap type carries needs a rider or a reason.

The verifications recorded in `docs/SCORE_VALIDATION.md` §6.1 (wonder rules and
stage costs) and §3.3 (Hollywood/Internet leader modifiers) **still hold at
current master** and are now pinned by tests instead of by a corpus run.
Tonight's government pricing fix is real: all eight governments' `civilActions`
/ `militaryActions` / `urbanBuildingLimit` / `peacefulCost` / `revolutionCost`
now reach the engine, and
`EveryFieldHasAReader.test_every_government_field_is_read` fails if any of the
five stops being read.

---

## 1. The two things that were verified before, re-checked at current master

| claim | where it came from | status at current master |
|---|---|---|
| all 16 wonders, all 53 stage costs, exact | SCORE_VALIDATION §6.1, 18,307 human stage lines | **holds** — `Wonder.*`, and the costs are still read from `data/cards_wonders_leaders.json` |
| `Impact of Wonders` 5/4/3/2 by age, exact | SCORE_VALIDATION §6.1, 565/565 rows | **holds** — `test_impact_of_wonders_pays_by_age` |
| Hollywood/Internet score **effective** building output, not printed | SCORE_VALIDATION §3.3, fixed in SCORE_BUGFIX | **holds** — `test_hollywood_uses_effective_output_not_printed`, `test_internet_matches_the_FAQ_sid_meier_example` (the FAQ's own 8-science/6-culture Sid Meier example) |
| Chaplin doubles one theater, not a card | SCORE_BUGFIX | **holds** — `test_chaplin_doubles_ONE_theater_not_the_card` |
| `Impact of Industry` scores mines, not the resource rating | SCORE_BUGFIX §3.1 | **holds** — and see bug 3.1 below, which is the *same card clause on the farm side*, and was NOT fixed with it |
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

## 3. The nine bugs

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

### 3.9 "Your best lab or library" counted an **unstaffed** one

Found by another lane, ruled here. `_building_modifier` and `_apply_modifier`
each held two cards of identical shape that disagreed:

    "bestTheaterDoubleCulture"        -> best_card(..., require_workers=True)
    "sciencePerBestLabOrLibraryLevel" -> best_card(...)      # no worker needed

The second is **Leonardo da Vinci, Isaac Newton and Albert Einstein**. As it
stood, a player who developed Computers and never put a worker on it still
collected +3 science a turn.

**They cannot both be right, and the staffed reading wins on three
independent grounds:**

1. **The printed card.** All three say the best lab or library *produces*
   extra science. In Through the Ages a building **is** a worker standing on
   a technology card; a card with no worker is a technology, not a lab, and
   produces nothing. The closest thing to a printed ruling on this exact
   phrase is FAQ v1.5 p.9 on the Transcontinental Railroad, quoted in our own
   card data: it doubles "one worker on the best mine technology card **that
   has workers**".
2. **The engine's own reading everywhere else.** Every other per-building
   leader multiplies by `t.workers` — Sid Meier, Bill Gates, J. S. Bach,
   Shakespeare's pairs, Michelangelo's happy faces, Napoleon's unit types.
   This one key was the only exception, in both of its two copies.
3. **BGO, measured.** `tools/bgo_rescore.py` on 150 human games, same
   replayer, only the reading changed:

   | reading | per-turn science rows exact vs BGO | all five rates exact |
   |---|---|---|
   | unstaffed lab counts (old) | 7275 / 7600 (95.7%) | 5713 (75.2%) |
   | **staffed only (new)** | **7303 / 7600 (96.1%)** | **5734 (75.4%)** |

   28 science rows and 21 whole-turn rows move the right way and nothing else
   in the tree changed. *Limit:* this is a net, not a monotone check — the
   aggregate cannot rule out some rows moving the other way — so the corpus
   is corroboration for a reading that the card and the FAQ already decide.

Fixed at **both** call sites. Generalised rather than patched: the new
`UnstaffedBuildingsProduceNothing` class asserts the rule for **every** key in
`effects._BUILDING_OUTPUT`, and `test_the_table_covers_every_building_output_key`
fails if a new modifier key is added without being covered. The coordinator's
instinct that "two of the same shape means there are probably more" was right
to check and, this time, the other six were already correct.

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
**"As found" is the audit's verdict before the fixes; "now" is after them.**
Post-fix the answer is **22 of 23 types exact**, and the one that is not is a
missing player *choice* rather than a wrong number.

| # | type | what the rules say | verdict |
|---|---|---|---|
| 55 | **event** | 15 Age III "Impact of" cards pay the end-game culture; the other 40 move culture/ratings during play | as found **14 of the 15 scoring events exact, 1 wrong** (3.1) and **1 wrong** of the other 40 (3.5); **now exact**. All 15 checked by hand; ranking tie-breaks correct on **both** paths (current player mid-game, start player at game end, RULES_SPEC 5.3/12.5) |
| 33 | **action** | yellow cards: a free civil action plus a printed gain | **exact**. `Endowment for the Arts` pays per richer civilization at the right per-player rate; the ordered-action-then-gains sequence is already ruled and tested |
| 24 | **leader** | 24 distinct abilities, mostly name-dispatched | as found **20 exact, 4 wrong** (3.2 Bill Gates, 3.3 Michelangelo, 3.6 Churchill, 3.9 Leonardo/Newton/Einstein); **now exact**. Level arithmetic confirmed against the FAQ's Sid Meier example: Age A = level 0, so Newton on a lone Philosophy adds nothing |
| 16 | **wonder** | stage costs, flat benefits, 4 Age III one-time bombs | as found **15 exact, 1 wrong** (3.3 / 3.7, St. Peter's on ruins and on colonies); **now exact**. All four bombs re-derived by hand: First Space Flight sums technology levels **including the government**, Fast Food Chains 2/1 per worker, Hollywood 2x theater+library culture, Internet culture+science+strength of urban buildings (happy excluded) |
| 15 | **tactic** | one army per copy of the composition; `obsoleteStrength` for an army holding a unit more than one age older | **exact**, including Genghis Khan's infantry-as-cavalry. `tacticBonus` in `effects` duplicates the top-level `strength` the engine reads; now pinned equal |
| 12 | **special-tech** | 4 icons, at most one per icon in play | **exact**. Build discounts, wonder stages, actions, strength, colonization all land; Masonry correctly leaves Age A alone. The one-per-icon rule is what makes `Impact of Variety` counting them by name correct |
| 12 | **territory** | immediate bonus, then a permanent one | **exact**, and now also visible to St. Peter's (3.7): token grants applied once rather than as production, rating symbols reach `Stats`, losing a colony takes the permanent bonus back |
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
| 3 | **war** | spoils by strength advantage | **2 exact, 1 partial** (3.8) -- the ONLY type not exact after the fixes, and it is an unimplemented *choice* (the victor may take blue technologies instead of science), not a wrong number. Draws move nothing; the **defender** can win and take the spoils |
| 3 | **bonus** | played from hand during a defense/colonization | **exact** — and, importantly, they contribute **nothing** while sitting in hand |
| 2 | **artillery** | 3/5 strength per worker | **exact** |
| 1 | **air** | 5 strength, and doubles one army's tactic bonus | as found **wrong** (3.4); **now exact** |

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

## 6. `board_yields.py` and `final_event_culture`, re-checked on the real tree

Both exist on master (my first clone was pinned 19 commits behind). Both were
re-audited.

### 6.1 The swap diff is faithful — with one hole, now closed

`engine/bots/board_yields.py` prices a leader, government or wonder by putting
the card on the board, calling `engine.effects.compute`, and diffing. The
failure mode is not a wrong formula but a wrong *diff*, so that is what was
checked:

* **Replacement is a delta, not an absolute.** `_swapped` sets `p.leader` to
  the candidate, so taking Einstein while holding Michelangelo prices as
  Einstein *minus* Michelangelo, and `_rider_delta` explicitly subtracts the
  outgoing leader's rider too. Wonders append instead of replacing, which is
  right: a wonder accumulates.
* **Clamps are handled correctly, and the module's reasoning is the right
  one.** A diff of two clamped `compute` results *is* the marginal value: a
  ninth happy face is genuinely worth zero, and the diff says zero. This was
  my main suspicion going in and it is unfounded.
* **Board interaction is exactly what it claims.** Michelangelo prices at
  `culture_rate +6` with two Organized Religion temples on the board and at
  nothing on an empty one. That is the whole justification for the module and
  it holds.
* **The trap is real and is guarded.** `state_stats` is a cache that a raw
  attribute write does not dirty, so the hypothetical must use `compute`. The
  module says so and a test enforces it.

**The hole: the diff silently dropped Taj Mahal's blue token.** `card_potential`
prices a swap card by the diff **alone** — deliberately, so a wonder's printed
culture is not counted twice — but blue tokens are not a `Stats` field
(`effects.on_enter_play` puts them on `p.blue_total`), so `compute` cannot
report them. The static `_card_yields` *does* price `blueTokens`
(`_EFF_TO_FEATURE` -> `blue_free`). So turning board pricing on moved Taj Mahal
from "3 culture + 1 blue token" to "3 culture", and the pricing guardrail could
not see it because `blueTokens` is priced *somewhere*.

Fixed with a `blueTokens` wonder rider, keyed by effect key so the next card
printing one is priced the day it lands. One card today; the class is "a key
one path prices and the other cannot see", which is the same class as the
eight above.

### 6.2 The shared-source-of-truth holds, and its test genuinely bites

`events.final_event_awards` is the single implementation; `evaluate_final_events`
applies its steps and `final_event_culture` sums them. That is the right shape,
and the divergence tests are not decorative — **negative control**: perturbing
`evaluate_final_events` by +1 culture per award fails
`test_payout_is_exactly_the_awards` and `test_forecast_equals_payout`, 2 of 9.
So the arrangement is defended rather than merely documented.

**One gap, now filled.** The forecast is a raw sum; the payout clamps a
player's running culture at zero after *each* award. `test_forecast_equals_payout`
skips every row where that clamp could have fired (`if b + f >= 0`), so the
divergence itself was never asserted on a position that produces it — and a
skipped case is not a checked case. `ForecastVersusPayout` now constructs it
(1 culture banked, 7 discontent workers, `Impact of Happiness` pending): the
forecast says −14, the payout pays −1 and stops at zero. The gap is
deliberate and documented in `final_event_awards`; it is now also *pinned*, so
closing or widening it is a decision somebody makes on purpose.

My `rankingCulture` finding from the first pass stands and is unaffected: the
ranking award is spelled out in both callers with different tie-break start
indices — correctly (current player mid-game per RULES_SPEC 5.3, start player
at game end per 12.5.2), but by two copies. Both are now pinned by tests.

`effects.culture` / `effects.science` (priced in `96a5db2`) are the short
spelling of per-turn production that `FLAT_KEYS` maps to `culture`/`science`;
the ten wonders and two leaders that use it are covered by
`Wonder.test_flat_benefits` and `Leader.test_flat_rating_leaders`, so the
pricer's map and the rules engine's map are checked against the same cards.

## 6.5 What the human corpus could not have caught

`docs/SCORE_VALIDATION.md` reports `Impact of Agriculture` as **66/66 exact**
against BGO. Bug 3.1 says that card scores the wrong quantity. Both are true,
and the reason is the important part:

**At 2 players every pact is removed from the game** (RULES_SPEC 13; our own
card data gives all ten pacts `"2p": 0`), and the corpus is **2p only**
(SCORE_VALIDATION 8). A pact's food symbol is the *only* thing in the 2015 base
game that puts food on your board from outside a farm. So no quantity of 2p
human games could ever separate "the food your farms produce" from "your food
rating" — the two are identically equal in every game in the corpus. The 66/66
is real, and it is 66/66 *on inputs that cannot distinguish the hypotheses*.

That is worth stating in general, because **five of the nine bugs sit inside
the corpus's four documented blind spots**:

| bug | blind spot | SCORE_VALIDATION's own words |
|---|---|---|
| 3.1 Agriculture | pacts, structurally absent at 2p | "2p only" (8) |
| 3.2 Bill Gates on leave | Iconoclasm / leader replacement | games touching Iconoclasm are gated OUT of clean rows (1) |
| 3.3 ruined wonders | Ravages of Time | "**no Ravages of Time flip was ever applied**" until the name-resolution fix (1) |
| 3.4 air force | armies and tactics | "the replayer models no tactics and therefore no armies" (2) |
| 3.7 St. Peter's + colony | happy faces | "happy faces are the one input the journal never prints" (8) |

None of this impugns the corpus work, which found and fixed three real bugs and
says all four of these limits out loud. The lesson is narrower and worth
writing down: **a corpus validates a formula only over the inputs it can
produce, and a 100% row is a statement about those inputs, not about the
formula.** Where a card names a *source* ("the food produced by their farms")
rather than a rating, the corpus can only check it if some game in the corpus
puts that rating and that source apart. For Agriculture, none could.

The counter-example is instructive: bug 3.9 was decided *by* the corpus,
because unstaffed labs are common in ordinary 2p play, so those games do
separate the two hypotheses. The corpus is decisive exactly where it has
variation and silent exactly where it does not.

## 7. What I could not verify, and one null to expect

**If any of this is A/B'd for strength, expect an explained null, not a
gain.** These are correctness fixes and only two of them touch the military
channel at all — the air force's doubling (3.4) and `War over Technology`'s
spoil (3.8, still unimplemented). The discard lane's A/B measured that channel
directly and found it nearly absent from our games: **34 aggressions across
600 games, of which zero were ever successfully defended.** A fix to how
strength is computed cannot pay in a game where the defensive decision never
decides anything, so the honest prediction for 3.4 is a null *with that
mechanism named*, not a bare null — and 3.4 is additionally inert on all eight
fingerprint arms (§9.1), because it needs two air units and a mixed-age army
set at once. None of the nine touches defensive card selection or war spoils
arithmetic.



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
python3 -m unittest tests.test_score_audit -v      # 176 tests
bash tools/gate.sh                                 # GATE PASS, digests unmoved
```

The nine bugs are fixed, so they no longer fail.  To see them fail as they
did before the fix, check out the first of the two commits (the tests-only
one) and run:

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

## 9. Which fixes are live, and the digests they moved

Nine rules fixes and one pricer fix. **All eight fingerprint arms moved**,
which is correct: these are rule violations, and correct modelling ships
whether or not it is convenient.

Derived on base `efa37b5`, in a quiet window the coordinator cleared after an
earlier attempt was abandoned rather than trusted (see the note at the end).

| arm | old | new |
|---|---|---|
| NARROW | bd0e9a62 | **cd0971ed** |
| WIDE | cf4f0a22 | **77c81e82** |
| WNARROW | 549e4a90 | **f0b240da** |
| WWIDE | 0e03e3b7 | **9010ec80** |
| QNARROW | b15d7b18 | **ad62a4e5** |
| QWIDE | bf221746 | **caf7cdd7** |
| PNARROW | d307c480 | **85c06781** |
| PWIDE | 4d71894c | **12b1dce0** |

**Two-sided, and then some.** Derivation 1 (working worktree) and derivation
2 (an independent clone of `efa37b5` with the same patch) agreed on **all
eight arms, byte for byte**. Before either, a clean checkout of `efa37b5` was
hashed as a control and reproduced the OLD column exactly — so the base was
known-good before anything of mine was measured against it, which is the step
that was missing when master's own constants were briefly stale. The
attribution run then reproduced *both* endpoints a third time
(`cd0971ed` / `bd0e9a62`).

### 9.1 Attribution: which fix moved which arm

Each fix reverted on its own from the all-fixed tree, `narrow` and
`weighted narrow` re-hashed. Measured, not reasoned:

| fix | GreedyBot (`narrow`) | WeightedBot (`weighted narrow`) |
|---|---|---|
| 3.5 plural targets | **LIVE** | **LIVE** |
| 3.9 unstaffed best lab | **LIVE** | **LIVE** |
| 3.3 ruined wonder → Michelangelo | **LIVE** | inert |
| 3.6 Churchill ring-fenced | **LIVE** | inert |
| 3.1 Agriculture scores farms | inert | **LIVE** |
| 3.2 Bill Gates on leave | inert | inert |
| 3.3 ruined wonder → St. Peter's | inert | inert |
| 3.7 St. Peter's + colony | inert | inert |
| 3.4 air force doubling | inert | inert |
| 6.1 Taj Mahal blue token | inert | inert |

**The measurement corrected me twice, which is the whole argument for
attributing rather than reasoning.**

* I predicted **3.1 (Agriculture) could not move any arm**, because 2p has no
  pacts and farm food therefore equals the food rating. It moves
  `weighted narrow` — because the fingerprint plays **4p**, where pacts
  exist. My reasoning was right and my conclusion was wrong because I forgot
  which games the arm plays.
* I predicted **3.3 (ruined wonders) would be rare**. It is live for
  GreedyBot: Ravages of Time plus Michelangelo does occur inside 33 games.

The four inert fixes are inert for stated reasons, not by luck: Bill Gates
leaving play needs a bot that replaces an Age III leader; the air force needs
two air units *and* a mixed-age army set; and the Taj Mahal rider cannot move
anything today because `card_board_credit` defaults to 0.0 and GreedyBot does
not evaluate through `weighted.py` at all. **Inert is a statement about
coverage, not about correctness** — these 135 games cannot catch a
regression in any of the four, which is worth knowing before someone
"simplifies" one of them.

### 9.2 Why the first derivation was thrown away

Recorded because the failure mode is subtle and cost hours. The first attempt
produced a `check_fp` FAIL with a **blank** "got" field. That is a *killed
subprocess*, not a moved hash — two lanes were running an unscoped
`pkill -f "engine.perf_check"`, which kills every lane's hasher, and a
`narrow` arm that takes 11 seconds unloaded was taking over five minutes.
Worse, the base itself was mid-flight: a clean checkout of the then-current
master hashed `narrow` to `bd0e9a62` against the `NARROW=0a6ed6ad` recorded
in its own `gate.sh`, so any constant derived on top would have silently
absorbed another lane's movement.

Both runs were discarded and the work waited for a quiet window. **A digest is
never re-derived to make a gate pass, and a digest derived on a saturated box
next to an unrecorded base is not a measurement.**

