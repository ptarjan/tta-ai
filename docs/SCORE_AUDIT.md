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
out of the other seven, and all nine are fixed — and with `a7a5ef1` the last
gap closed too, so all 23 of 23 are exact now.**
A tenth defect, in the *pricer*, fell out of re-auditing
`engine/bots/board_yields.py`. The type that was still not exact was **war**,
and what was missing was a player *choice* the engine did not offer (the
victor of `War over Technology` may take blue technologies instead of
science), not a number it gets wrong. None is large: most are worth
1-6 culture in the positions that reach them, but four are *per turn* rather
than one-off and one is *per army*. Every one of the nine is the shape this
project has already shipped twice: **a value that lives in a field, or a card
clause, that no reader touches — or two readers of one rule that quietly
disagree.**

## The finding that outlives the bugs: a corpus validates only what it varies

§10 below (the former `docs/SCORE_VALIDATION.md`) scored `Impact of Agriculture` **66/66 exact**
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

The verifications recorded in §10.6.1 (wonder rules and
stage costs) and §10.3.3 (Hollywood/Internet leader modifiers) of the former `docs/SCORE_VALIDATION.md` **still hold at
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
| all 16 wonders, all 53 stage costs, exact | §10.6.1 (former SCORE_VALIDATION), 18,307 human stage lines | **holds** — `Wonder.*`, and the costs are still read from `data/cards_wonders_leaders.json` |
| `Impact of Wonders` 5/4/3/2 by age, exact | §10.6.1 (former SCORE_VALIDATION), 565/565 rows | **holds** — `test_impact_of_wonders_pays_by_age` |
| Hollywood/Internet score **effective** building output, not printed | §10.3.3 (former SCORE_VALIDATION), fixed in §11 (former SCORE_BUGFIX) | **holds** — `test_hollywood_uses_effective_output_not_printed`, `test_internet_matches_the_FAQ_sid_meier_example` (the FAQ's own 8-science/6-culture Sid Meier example) |
| Chaplin doubles one theater, not a card | §11 (former SCORE_BUGFIX) | **holds** — `test_chaplin_doubles_ONE_theater_not_the_card` |
| `Impact of Industry` scores mines, not the resource rating | §11.1.1 (former SCORE_BUGFIX) | **holds** — and see bug 3.1 below, which is the *same card clause on the farm side*, and was NOT fixed with it |
| `Impact of Population` counts unused workers | §11.1.2 (former SCORE_BUGFIX) | **holds** — `test_impact_of_population` |

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
**`Impact of Industry` (§10.3.1, former SCORE_VALIDATION) again, on the other card** —
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

[`docs/RULES_SPEC.md`](RULES_SPEC.md) §5.3, citing CoL p.7: *"'All civilizations' with
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
Post-fix the answer is **23 of 23 types exact**. The last holdout was war,
where what was missing was a missing player *choice* rather than a wrong
number; `a7a5ef1` implements it ([`docs/COMBAT_AUDIT.md`](COMBAT_AUDIT.md)).

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

§10 below (the former `docs/SCORE_VALIDATION.md`) reports `Impact of Agriculture` as **66/66 exact**
against BGO. Bug 3.1 says that card scores the wrong quantity. Both are true,
and the reason is the important part:

**At 2 players every pact is removed from the game** (RULES_SPEC 13; our own
card data gives all ten pacts `"2p": 0`), and the corpus is **2p only**
(§10.8). A pact's food symbol is the *only* thing in the 2015 base
game that puts food on your board from outside a farm. So no quantity of 2p
human games could ever separate "the food your farms produce" from "your food
rating" — the two are identically equal in every game in the corpus. The 66/66
is real, and it is 66/66 *on inputs that cannot distinguish the hypotheses*.

That is worth stating in general, because **five of the nine bugs sit inside
the corpus's four documented blind spots**:

| bug | blind spot | the former SCORE_VALIDATION's own words |
|---|---|---|
| 3.1 Agriculture | pacts, structurally absent at 2p | "2p only" (§10.8) |
| 3.2 Bill Gates on leave | Iconoclasm / leader replacement | games touching Iconoclasm are gated OUT of clean rows (§10.1) |
| 3.3 ruined wonders | Ravages of Time | "**no Ravages of Time flip was ever applied**" until the name-resolution fix (§10.1) |
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
  a claim of mine disagrees with the former `docs/SCORE_VALIDATION.md`'s (now §10) corpus numbers,
  the corpus wins — except on 3.1, where the corpus provably *cannot* see the
  bug because the replayer models no pacts.
* **`Impact of Happiness` remains open** for the reason §10.8 (former SCORE_VALIDATION)
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


## 10. Does our engine score the same game BGO scored? (2026-07-27) (merged from the former `SCORE_VALIDATION.md`, 2026-07-31)

Branch: `score-validation`. New tools: `tools/bgo_rescore.py`, `tools/wonder_ab.py`.
Nothing in `engine/` is touched by this branch; `bash tools/gate.sh` is green
(GATE PASS, all six digests unmoved) and `python3 -m unittest discover -s
tests -q` is 393 tests OK (381 + the 12 new ones in
`tests/test_bgo_rescore.py`).

This answers proposals 1 and 2 of [`docs/HUMAN_BASELINE.md`](HUMAN_BASELINE.md). That document's
own "What this cannot tell you" says the 84-vs-160 score comparison "is not a
clean skill measurement… nothing here independently verifies that our
end-of-game scoring matches BGO's." It does now.

### One-paragraph answer

**Our engine scores the same game.** On 43,847 turn snapshots reconstructed
from the 1,011 human journals it reproduces BGO's own printed culture,
science, food, consumption and resource numbers exactly on 34,733 of them
(99.1% on turns 1-5, falling only as the *replayer* drifts, not the engine),
every one of the 16 wonders' stage costs, the `+1 CA per completed wonder`
take surcharge including the Michelangelo exemption, and, of the fifteen Age
III scoring events, six at 100% and eleven at 86% or better on verified
reconstructions. **Our games are not short**: 20.0 rounds
against a human 19.4. Three real engine bugs fell out, all worth single-digit
culture — they do not explain a 76-point gap. **The score gap is a policy
fact, not a scoring fact**: the same engine, run with the 1-ply-lineage vector
instead of the quiescent champion, scores 139.8 [131.6, 148.3] against a human
159.5 [156.0, 163.0], and does it with 0.76 wonders per player. **Wonders are
neither broken nor a free lunch**: costs and surcharge are exactly right,
benefits are if anything *under*-implemented, and forcing a strong bot to
build human numbers of them costs it 34.3 ± 7.0 margin.

---

### 10.1. Method: replay the journal, ask our engine, diff against BGO

[`docs/HUMAN_BASELINE.md`](HUMAN_BASELINE.md) proposal 1 says "reconstruct one finished human
position by hand". Hand-reconstructing one position tests one position, and a
19-round game has ~40 actions per player, so the hand is at least as likely to
be wrong as the engine. `tools/bgo_rescore.py` does it mechanically instead,
for every seat of every game, and gets three independent oracles out of the
journal that a hand reconstruction would not have:

1. **Every `End turn` line prints that player's production**
   (`N culture (now C); N science (now S); N food - consumption: K; N
   resources`). That is five engine outputs per player per turn, 43,847 of
   them, on positions we can rebuild.
2. **Every `Impact of ...` line at game end prints each player's award** —
   i.e. `engine/events.py::scoring_culture` as computed by somebody else.
3. **The `End of game` line prints the final totals**, and BGO's own
   arithmetic (last culture + end-of-game impacts = printed score) checks out
   on 71.9% of rows without any modelling at all, which is the parse sanity
   check.

The replayer rebuilds each seat's tableau (workers per card, government,
leader, completed/flipped wonders, colonies, yellow bank) from the action
lines, builds a real `GameState`, and calls `effects.state_stats`,
`events.scoring_culture`, `effects.on_wonder_complete` and
`effects.end_of_game_bonus` on it.

#### The cleanliness gate, and why it is the whole design

A disagreement between the replayer and BGO is ambiguous: it can be the
replayer losing a worker as easily as an engine bug. So a row only counts as
evidence about the *scorer* when the replay of that row is independently
verified:

* all five production numbers match BGO's own print-out that turn, **and**
* the seat's yellow tokens are conserved
  (`bank + unused + on-cards == 25 − 2 per age end + grants`), **and**
* no line the replayer cannot model (Annex, Infiltrate, Iconoclasm, Raid
  casualties, Terrorists, Barbarossa) touched the game.

That leaves 405 of 2,525 final positions (16.0%). Small, but they are *known
good* rather than assumed good, and the ranking events additionally require
every seat in the game to be clean.

**The gate is also a measurement, not just a filter.** Running our engine
against BGO's numbers on every turn of every game, with no filtering at all:

| quantity | our engine == BGO |
|---|---|
| culture production | 40,280 / 43,847 (91.9%) |
| science production | 42,322 (96.5%) |
| food production | 39,703 (90.5%) |
| food consumption | 40,183 (91.6%) |
| resource production | 42,259 (96.4%) |
| **all five at once** | **34,733 (79.2%)** |

and by turn index:

| | all five exact |
|---|---|
| turns 1-5 | 12,514 / 12,625 (**99.1%**) |
| turns 6-10 | 10,354 / 12,571 (82.4%) |
| turns 11-15 | 7,758 / 11,582 (67.0%) |
| turns 16+ | 4,107 / 7,069 (58.1%) |

**That decay is the signature of a drifting replayer, not of an engine bug.**
An engine that computed culture wrongly would be wrong on turn 3 too. An
engine that agrees with BGO on 99.1% of early positions and degrades smoothly
as reconstruction error accumulates is an engine that agrees.

#### Two things this method found about the replayer, recorded so the next
#### person does not rediscover them

* **The `-2 yellow tokens at each age end` rule is real and BGO applies it.**
  On one hand-traced game it looked like BGO did *not*, which would have been
  a large economy bug in `engine/game.py:164`. Run as a whole-corpus A/B over
  43,847 end-turn lines it is not close: consumption predicted correctly on
  **91.6% at `age_loss=2`, 68.7% at 1, 52.2% at 0**, and the residuals at 0
  are systematically one band low while at 2 they are symmetric (+1 × 1,351,
  −1 × 2,146). The rule stays. This is written down because the single-game
  version of this check produced a confident wrong answer.
* Four replay bugs each looked like an engine bug first, and each is pinned by
  a case in `tests/test_bgo_rescore.py`: leader names truncated at the first
  word (`elects William Shakespeare Leonardo Da Vinci dies` → `William`, the
  same failure [`docs/HUMAN_BASELINE.md`](HUMAN_BASELINE.md) records costing 39% of elections); an
  upgrade routing the worker through the unused pool and minting a yellow
  token per upgrade; `Warrior` (BGO's singular) not resolving to `Warriors`,
  923 lines in a 150-game sample, and invisible to a production check because
  units produce only strength; and `The Pyramids crumble` not resolving to
  `Pyramids`, so **no Ravages of Time flip was ever applied** — fixing that one
  alone moved culture agreement from 89.4% to 91.9% and turn-16+ agreement
  from 50.8% to 58.1%, which is a fair measure of how much of the residual in
  this document is still replayer and not engine.

---

### 10.2. Result: the end-of-game scorer

Clean rows only (n is the number of clean player-awards; "all n" is every row
including unverified reconstructions, for scale).

| Age III event | clean n | exact | % | all n | exact |
|---|---|---|---|---|---|
| Impact of Wonders | 78 | 78 | **100.0** | 565 | 565 |
| Impact of Government | 92 | 92 | **100.0** | 647 | 643 |
| Impact of Progress | 103 | 103 | **100.0** | 688 | 673 |
| Impact of Balance | 86 | 86 | **100.0** | 580 | 561 |
| Impact of Agriculture | 66 | 66 | **100.0** | 528 | 505 |
| Impact of Science (ranking) | 49 | 49 | **100.0** | 759 | 737 |
| Impact of Technology | 83 | 82 | 98.8 | 606 | 602 |
| Impact of Architecture | 73 | 68 | 93.2 | 554 | 464 |
| Impact of Variety | 87 | 81 | 93.1 | 585 | 508 |
| Impact of Competition | 66 | 61 | 92.4 | 455 | 397 |
| Impact of Colonies | 67 | 58 | 86.6 | 513 | 429 |
| **Impact of Industry** | 81 | 61 | **75.3** | 542 | 452 |
| Impact of Happiness | 94 | 66 | 70.2 | 640 | 442 |
| **Impact of Population** | 81 | 43 | **53.1** | 584 | 322 |
| Impact of Strength (ranking) | 40 | 26 | 65.0 | 742 | 510 |

Plus `effects.end_of_game_bonus` (Bill Gates): **411 / 420 exact**, with no
cleanliness filter at all.

> **Read the 100% rows with §10.8 in hand (added 2026-07-30, see
> [`docs/SCORE_AUDIT.md`](SCORE_AUDIT.md)).** A percentage here is over *the inputs this corpus
> can produce*, and for one row that turns out to matter: **`Impact of
> Agriculture` is 66/66 exact against an implementation that scores the wrong
> quantity.** The card scores "the food produced by their farms"; the engine
> scored the food *rating*. The two differ only when something puts food on
> your board from outside a farm, the only such thing in the base game is a
> pact's food symbol, **every pact is removed from the game at 2 players**,
> and this corpus is 2p only — so the two quantities are identically equal in
> all 2,525 positions here, and no amount of this data could have separated
> them. Five of the nine bugs [`SCORE_AUDIT.md`](SCORE_AUDIT.md) found sit inside the four
> blind spots §10.8 already names. The corpus is decisive exactly where it has
> variation: it *settled* the "does an unstaffed lab pay Newton?" question
> (7303/7600 against 7275/7600) because unstaffed labs are common in 2p play.

Reading the rows that are not 100%:

* **Colonies (±3, symmetric)** and **Architecture / Variety / Competition
  (small, mixed sign)** are the replayer: a stolen colony (Annex) and missing
  military workers, which the five-rate gate cannot see because units produce
  *strength*, and strength is never printed outside a war.
* **Impact of Strength** residuals are ±10 × 7, exactly the 2p ranking table.
  The replayer models no tactics and therefore no armies, so it cannot rank
  strength. Not evidence about the engine either way. Contrast **Impact of
  Science**, the other ranking card, which is 43/43 once every seat is clean —
  the ranking machinery itself is right.
* **Happiness** is 70.2% with mixed-sign residuals (+2 × 10, −2 × 6). Happy
  faces are the one input the journal never prints, so the gate cannot verify
  them and this row is **unresolved**, not exonerated. Restricted to rows
  where our engine says discontent is 0 (which removes the other unverifiable
  input) it is **61/75 (81.3%)**, residuals +2 × 9, −2 × 4, +4 × 1 — still
  mixed sign, still open.
* **Industry and Population are real engine bugs.** They are the only two rows
  whose residuals are large and all one sign, and for both the corrected
  formula matches BGO nearly perfectly. See §10.3.

---

### 10.3. Three engine bugs, all small, none of them the score gap

Nothing was fixed on this branch — `tools/gate.sh` digests are unmoved on
purpose. These are handed over as findings.

> **All three are FIXED on master; see [`docs/SCORE_AUDIT.md`](SCORE_AUDIT.md).** A fourth fell
> out of fixing §10.3.3 (Charlie Chaplin was doubling every worker on the best
> theater card instead of one building), and it is the only one of the four
> that moves a rating rather than a score: fixing it took our agreement with
> BGO's printed per-turn culture from 91.9% to 92.9% over 43,847 lines.
> §10.6.2's wonder A/B was re-run on both vectors afterwards and **did not
> move** — every P row shifted by less than a fifth of its own SE and every Q
> row is identical to one decimal — because the unforced production bot
> completes exactly *one* Age III wonder in 80 seat-games, so the two cards
> §10.3.3 is about were essentially never being scored. §10.6.3's conclusions
> stand.

#### 10.3.1 `Impact of Industry` scores the resource *rating*, not mine production

`engine/events.py:393` reads `culturePerResourceProducedByMines` as
`v * s.resources`, the whole resource rating. The card says "the resources
produced by their mines (ignoring other bonuses)". Two things add resources
outside mines: **Bill Gates** (`resourcesPerLabEqualToLevel`) and
`Transcontinental Railroad`'s doubled mine worker (which per the FAQ *does*
count, being a mine).

Scoring mines-only + the Railroad's double against BGO: **81 / 81 exact**,
against our engine's 61 / 81. Residuals of the current code are +6 × 9,
+9 × 6, +4 × 2, +12 × 2, +7 × 1 — all positive, all Bill Gates lab levels.
**We over-score this card, by 4-12 culture, only for Bill Gates players.**

#### 10.3.2 `Impact of Population` does not count unused workers

`engine/events.py:412` computes content workers as
`sum(t.workers for t in p.techs.values()) - discontent`, i.e. workers standing
on cards. Yellow tokens in the worker pool are workers too.

Adding `p.workers_free`: **68 / 81 exact** against our engine's 43 / 81 on
clean rows, and restricted further to the rows where our engine says
discontent is 0 (removing the one input the replayer cannot check),
**63 / 66 against 43 / 66**, with the alternative's only residuals being
+2 × 2 and −2 × 1 — one worker, mixed sign, i.e. replay noise. Current-code
residuals are −2 × 21, −4 × 8, −6 × 5, −8 × 2, −10 × 1 — every one negative and
every one an exact multiple of 2, i.e. a whole number of uncounted workers.
**We under-score this card by 2 culture per unused worker.**

#### 10.3.3 Age III wonder completion bonuses ignore leader modifiers

`effects._one_time_culture` builds Hollywood and Internet from *printed*
`production` values. BGO uses the buildings' **effective** output. Against the
corpus (clean seats only, at the moment of completion):

| wonder | exact | residuals (ours − BGO) |
|---|---|---|
| Fast Food Chains | 131 / 139 | −2 × 6, −1 × 2 (replay drift) |
| First Space Flight | 167 / 178 | −1 × 10, −2 × 1 (replay drift) |
| **Hollywood** | **30 / 72** | −8 × 22, −6 × 11, −12 × 6 |
| **Internet** | **69 / 105** | −3 × 17, −4 × 6, −6 × 4 |

and the mismatches are perfectly explained by *which leader was in play*:

* Hollywood: **every** Charlie Chaplin (32/32) and William Shakespeare (7/7)
  completion is wrong; with any other leader it is exact.
* Internet: every Charlie Chaplin (13), William Shakespeare (4) and **Albert
  Einstein** (14/14) completion is wrong; **Sid Meier is 38/38 exact** —
  because `_one_time_culture` already special-cases Sid Meier and nobody else.

Chaplin doubles the best theater's culture, Shakespeare pays 2 per
library/theater pair, Einstein adds science to the best lab/library — all of
them modify exactly the per-building output these two wonders sum.
**We under-score the two biggest wonder payoffs in the game, by ~4.4 culture
per Hollywood and ~1.4 per Internet on average.** Note the sign: this bug
makes wonders *worse* in our engine than in the real game, which is the
direction that matters for §10.5.

#### Size check

All three together are worth single-digit culture per game to a typical
position. They cannot make 84 into 160, and they do not change any conclusion
in [`docs/HUMAN_BASELINE.md`](HUMAN_BASELINE.md) about behaviour.

---

### 10.4. Game length: our games are not short

[`docs/HUMAN_BASELINE.md`](HUMAN_BASELINE.md) already reported this as "overlap"; it is confirmed
on a bigger sample and it is *not* the direction a scoring bug would want.

| | rounds |
|---|---|
| human corpus (1,011 games, from the journal's own round column) | median 19, mean **19.27** |
| human corpus (`tools/bgo_stats.py`, 2p only) | **19.43** [19.38, 19.49] |
| bot, 1-ply lineage vector, 1-ply search, 2p mirror (n=60) | **20.02** [19.87, 20.17] |
| bot, 1-ply lineage vector, quiescence (n=60) | 20.10 [19.95, 20.25] |
| bot, quiescent champion, 1-ply search (n=60) | 20.05 [19.92, 20.18] |
| bot, quiescent champion, quiescence `levels=1` (n=60) | **17.32** [16.40, 18.15] |

Three of the four configurations run *longer* than humans. The one short row
is the quiescent champion under its own training search, and the same vector
under 1-ply search runs 20.05 — so it is that policy's play, not the engine's
age/end-trigger timing, that shortens the game. The mechanism is not
established here. **Game length is not the gap**, and in the one place it does
move it moves for a bot whose score is *lowest*, i.e. it cannot be doing the
explanatory work either way for the other three.

---

### 10.5. The score gap is a property of the vector, not of the engine

This is the finding that reframes [`docs/HUMAN_BASELINE.md`](HUMAN_BASELINE.md) §"Bot vs human".
That document measured **one** policy: the quiescent champion at
`quiesce:...,levels=1`. [`docs/TRANSFER_TEST.md`](TRANSFER_TEST.md) had already established that
this vector is a *suppression* engine that scores 111-125 while holding its
rival to 43-84, and that the 1-ply lineage vector is a *production* engine
scoring 160-212. Nobody had run the human-corpus comparison on the second one.

`tools/bgo_botmatch.py`, 2p mirror, n=60 games each, same seeds:

| | human | Q champion, quiescence (the HUMAN_BASELINE config) | Q champion, 1 ply | P 1-ply lineage, 1 ply | P, quiescence |
|---|---|---|---|---|---|
| **final culture** | **159.5** [156.0,163.0] | **64.7** [56.2,72.6] | 110.5 [104.8,116.5] | **139.8** [131.6,148.3] | 130.3 [121.9,138.6] |
| rounds | 19.43 | 17.32 | 20.05 | 20.02 | 20.10 |
| wonders completed | 2.74 | 0.41 | 0.28 | 0.76 | 0.53 |
| wonder stages | 8.77 | 1.86 | 1.49 | 3.12 | 2.45 |
| civil cards taken | 34.3 | 22.2 | 25.4 | 22.9 | 23.1 |
| % of takes at 3 CA | 4.5 | 22.3 | 22.0 | 23.2 | 24.1 |
| wars declared /player | 0.25 | 0.49 | 0.00 | 0.00 | 0.72 |
| colony bids | 3.22 | 1.83 | 11.41 | 0.07 | 0.07 |

Three things follow.

1. **"Our bot scores half what humans score" is a statement about one
   vector.** Swap the vector and the same engine, same search family, same
   scoring code produces 139.8 against a human 159.5 — a 20-point gap, not a
   76-point one. (The CIs still do not overlap; our best bot is genuinely
   below the human mean. But it is not half.)
2. **The wonder gap is in every configuration; the score gap is not.** All
   four build 0.28-0.76 wonders — 10% to 28% of the human 2.74 — while score
   ranges over 65-140 across exactly those four. The two do move together a
   little (the 139.8 bot has the most wonders), but nothing like enough:
   whatever is suppressing wonders in our ecosystem is mostly not what is
   suppressing score.
3. **So is the card-take profile.** 22-25 takes and 22-24% at 3 CA in all four
   configurations, across a 75-point score range. [`docs/HUMAN_BASELINE.md`](HUMAN_BASELINE.md)
   finding 2 ("a smaller civil-action budget spent impatiently") is real and
   universal in our bots — but this run gives no evidence that it is what
   costs the points, because it does not vary while score does.

Two smaller notes: our reproduction of the quiescent champion's score is
**64.7 [56.2, 72.6]** where [`docs/HUMAN_BASELINE.md`](HUMAN_BASELINE.md) reported **84.1
[73.7, 95.2]** on n=40. Those CIs do not overlap. Different generation
(231 vs 224) and different seeds; either the champion drifted downward on this
axis in seven generations or one of the two samples is unlucky. It is flagged,
not explained. And `P` at 1 ply essentially **stops colonising** (0.07 bids
against a human 3.22) while `Q` at 1 ply over-colonises (11.41) — the two
vectors are off-distribution in opposite directions on that axis.

---

### 10.6. Part 2: are wonders weak, or invisible?

#### 10.6.1 The wonder rules and data are right

* **Stage costs.** Extracted from 18,307 human stage lines: for each wonder
  and each stage index, the maximum resource cost anybody paid (discounts only
  reduce, so the max is the printed price). **All 16 wonders, all 53 stages,
  match `data/cards_wonders_leaders.json` exactly. Zero mismatches.**
* **The `+1 CA per completed wonder` take surcharge.** For all 7,395 wonder
  takes in the corpus, `logged CA − (completed wonders at that moment)` lands
  in the legal row-slot range 1-3 for **6,980 (94.4%)**, and the 415 that do
  not are dominated by **Michelangelo** (380 of them) — the leader
  `actions.take_cost` already exempts. So the surcharge is implemented exactly
  as BGO implements it, exemption included, and it is **not** over-charging.
  (`engine/actions.py:79-89` also charges `p.destroyed_wonders`, which §2.4
  requires and which the corpus cannot test — Ravages of Time flips a wonder
  rather than destroying it.)
* **Benefits.** `Impact of Wonders` is 61/61 exact on clean rows and 565/565
  on *all* rows. The Age III one-time bombs are 94% exact for Fast Food Chains
  and First Space Flight and **under**-scored for Hollywood and Internet
  (§10.3.3).

So there is no rules or cost bug making wonders bad. If anything our engine
pays slightly less for them than the real game does.

#### 10.6.2 The scripted A/B: forcing wonders

`tools/wonder_ab.py` wraps a policy and overrides it with probability
`--force` whenever a `wonder_step` is legal (largest available) or a wonder
sits in the row and none is in progress (cheapest slot). `--force 0` is the
unmodified bot. Seats are mirrored on the same deal, so the margin is paired
and the deal is the unit of error; ± is one SE.

**P, the 1-ply-lineage production vector, 1-ply search, 40 deals = 80 games per row:**

| force | own culture | rival | margin | win share | wonders | overrides/game |
|---|---|---|---|---|---|---|
| 0.00 | 155.2 ± 5.2 | 155.2 ± 5.2 | 0.0 ± 6.1 | 0.512 ± 0.056 | 0.71 | 0 |
| 0.10 | 145.2 ± 4.4 | 156.0 ± 5.0 | **−10.8 ± 5.6** | 0.412 ± 0.055 | 1.45 | 3.0 |
| 0.20 | 147.4 ± 4.5 | 155.8 ± 4.7 | −8.4 ± 6.0 | 0.388 ± 0.055 | 1.8 | 5.7 |
| 0.40 | 148.0 ± 5.0 | 154.1 ± 4.8 | −6.1 ± 6.4 | 0.463 ± 0.056 | 2.4 | 9.9 |
| 0.70 | 139.5 ± 5.2 | 162.9 ± 4.9 | −23.4 ± 6.1 | 0.312 ± 0.052 | 3.1 | 14.6 |
| 1.00 | 125.2 ± 5.6 | 159.4 ± 4.7 | **−34.3 ± 7.0** | 0.300 ± 0.052 | 3.8 | 17.7 |

**Q, the quiescent champion, `quiesce:levels=1`, 25 deals = 50 games per row:**

| force | own culture | rival | margin | win share | wonders |
|---|---|---|---|---|---|
| 0.00 | 64.9 ± 6.4 | 64.9 ± 6.4 | 0.0 ± 5.5 | 0.540 ± 0.071 | 0.40 |
| 0.20 | 71.6 ± 6.9 | 72.3 ± 5.8 | −0.7 ± 6.1 | 0.480 ± 0.071 | 0.88 |
| 0.50 | 80.8 ± 5.9 | 81.1 ± 5.4 | −0.3 ± 5.8 | 0.520 ± 0.071 | 1.46 |
| 1.00 | **85.7 ± 6.7** | 81.4 ± 5.8 | **+4.3 ± 7.0** | 0.560 ± 0.071 | 1.9 |

**The two vectors answer the question in opposite directions, and that is the
finding.**

* On **P** every dose is negative and the sign is consistent across six
  points; at full force it reaches human-scale wonder counts (3.8) and pays
  **34.3 ± 7.0 margin and 30 points of its own culture** for them. There is no
  hidden payoff here for a `levels=1` evaluator to be blind to. For this
  vector the answer is closer to **(a)**: at the economy this bot actually
  runs, wonders are worse than what it does instead.
* On **Q** forcing wonders **raises its own culture by 20.8** (64.9 → 85.7)
  and its margin never leaves zero: −0.7 ± 6.1, −0.3 ± 5.8, +4.3 ± 7.0 across
  the three doses, every one inside one SE of the null.
  Q's evaluator genuinely cannot see the wonder (**(b)** for this vector), but
  what it gains in culture it gives back in suppression: the *rival's* score
  rises by the same 16 points, because the actions went into wonders instead
  of into wars and aggressions. The league gates on `margin_share`
  ([`docs/TRANSFER_TEST.md`](TRANSFER_TEST.md#5-why-the-two-vectors-are-different-animals) §5), which pays twice for a stolen point and once
  for a produced one, so a change that is +21 own culture and 0 margin is
  invisible to the trainer by construction.

#### 10.6.3 What this does and does not license

* Wonders are **not broken**: costs exact, surcharge exact, `Impact of
  Wonders` exact, one-time bombs correct for two of four and *understated* for
  the other two.
* Wonders are **not a free lunch our search is missing**, at least not for the
  strongest vector we have.
* **Wonders are not the score gap.** At full forcing Q reaches 1.9 wonders and
  85.7 culture; P at zero forcing has 0.71 wonders and 155. The correlation
  between wonders and score across these ten rows is not the story.
* **The override is crude and this bounds the claim.** It builds a stage
  whenever one is legal, including on turns where the wonder cannot finish,
  and it takes the leftmost wonder in the row rather than the best one. A
  competent wonder policy could plausibly do better than this one. What the
  A/B rules out is "there is a large payoff sitting there that a 1-ply search
  cannot reach"; it does not rule out "a *good* wonder plan is worth points".
  Pricing a *hand-written competent* wonder script, rather than a random
  override, is the obvious follow-up.
* **A plausible mechanism nobody has tested:** a wonder costs 1 civil action
  per stage, so a human's 8.8 stages is ~9 civil actions plus the take. Our
  bots take 22-25 cards against a human 34.3 on the same 19-20 rounds, i.e.
  they are ~10 civil actions poorer over the game — which is roughly the
  entire cost of a human's wonder programme. Under that story the wonder
  deficit is *downstream* of the civil-action deficit and forcing wonders
  without fixing the budget is exactly the wrong order of operations, which is
  what the P table looks like. This document does not test it.

---

### 10.7. Reproducing

```
tar xzf sources/bgo/journals.tar.gz -C /tmp/bgo

# §1-§3: replay the corpus through our engine
python3 tools/bgo_rescore.py --journals /tmp/bgo/journals
python3 tools/bgo_rescore.py --game 7520718 --trace Orange   # per-turn diff
for al in 0 1 2; do python3 tools/bgo_rescore.py --age-loss $al; done

# §4-§5: the four bot configurations (champions copied out of the live
# training dir first, because the trainer rewrites them mid-run)
cp experiments/league_state/champion_2p.json /tmp/Q2p.json
cp experiments/archive_preplan/league_state_1ply_20260726/champion_2p.json /tmp/P2p.json
for s in quiesce:/tmp/Q2p.json,levels=1 /tmp/Q2p.json /tmp/P2p.json quiesce:/tmp/P2p.json,levels=1; do
  nice -n 19 python3 tools/bgo_botmatch.py --players 2 --games 60 --seed 7000 \
      --spec "$s" --out /tmp/bm.tsv
  python3 tools/bgo_stats.py --tsv /tmp/human.tsv --vs /tmp/bm.tsv --players 2
done

# §6.2: the wonder A/B
nice -n 19 python3 tools/wonder_ab.py --spec /tmp/P2p.json --deals 40 \
    --force 0 --force 0.1 --force 0.2 --force 0.4 --force 0.7 --force 1.0
nice -n 19 python3 tools/wonder_ab.py --spec quiesce:/tmp/Q2p.json,levels=1 \
    --deals 25 --force 0 --force 0.2 --force 0.5 --force 1.0
```

Everything above ran `nice -n 19` alongside five live training workers and
another agent's PlanBot experiments on a 6-core box.

### 10.8. Limits

* **The replayer is the weak side of every comparison, deliberately.** 16.0%
  of final positions survive the cleanliness gate. Everything reported as an
  engine result is gated; everything gated out is gated out because the
  *replay* could not be verified, not because the engine disagreed.
* **Happy faces are unverifiable.** The journal never prints them, so
  `Impact of Happiness` (70.2%) is genuinely open and the strength ranking
  (65.0%) is untestable without modelling tactics, which the replayer does
  not do. If a third engine bug is hiding anywhere in the scorer, it is behind
  one of those two.
* **n = 60 games per bot configuration, 40/25 deals per A/B row.** The score
  differences quoted between vectors are 3-10 SE and safe; the *within*-vector
  dose response in §10.6.2 is not clean (P's −10.8, −8.4, −6.1 are within noise
  of each other) and only the sign and the endpoints should be leaned on.
* **2p only.** Nothing here was run at 3p or 4p.
* **[`docs/HUMAN_BASELINE.md`](HUMAN_BASELINE.md)'s behavioural findings are untouched.** This
  document validates the *arithmetic* and reframes the *score* comparison. It
  does not dispute that our bots build 3-7x fewer wonders, take 10 fewer
  cards, pay 3 CA five times as often, or revolt four rounds early — every one
  of those reproduced here on new samples and on a second vector.

## 11. Fixing the scoring bugs `docs/SCORE_AUDIT.md` §10 found (2026-07-27) (merged from the former `SCORE_BUGFIX.md`, 2026-07-31)

Branch: `score-bugfix`, merged to master. Acts on §10.3 of
[`docs/SCORE_AUDIT.md`](SCORE_AUDIT.md) (the former `docs/SCORE_VALIDATION.md`), which located three scoring bugs against the
1,011-game BGO human corpus and deliberately left them unfixed so the gate
digests would stay put for that measurement.

All three are confirmed and fixed. **A fourth fell out of fixing the third**,
and it is the only one of the four that was visible outside the scoring code.
All four gate digests moved and were re-derived deliberately (§11.4). The suite
adds 25 tests (23 in `tests/test_scoring_bugfix.py`, 2 in
`tests/test_bgo_rescore.py` for the new oracle) — 401 -> 426 on the branch,
**461 green** after rebasing onto master `9c8b6f5`, whose own 35 tests and all
four digests are unaffected by these changes.

### One-paragraph answer

The three reported bugs were real, the rules and BGO agree on all three, and
fixing them moves each one's own oracle: `Impact of Industry` **452 → 542 of
542**, `Impact of Population` **322 → 341 of 584**, `Hollywood` **85 → 168 of
186**, `Internet` **174 → 247 of 293** (all-rows counts, whose denominator
does not move). Fixing Hollywood exposed a fourth: **Charlie Chaplin was
doubling every worker on the best theater card instead of one building**, and
because that is a culture *rating* bug rather than a scoring bug it has an
independent oracle — BGO's printed per-turn culture, on 43,847 lines, which
goes **91.9% → 92.9%** and drags all-five-rates agreement from 79.2% to 80.0%
and turn-16+ agreement from 58.1% to **62.1%**. **The wonder A/B did not
move.** [`docs/SCORE_AUDIT.md`](SCORE_AUDIT.md#1062-the-scripted-ab-forcing-wonders) §10.6.2 hoped that Bug 3 was suppressing
wonder payoffs; re-run on both vectors after the fix, forcing wonders still
costs the production vector **−33.4 ± 6.9** margin (was −34.3 ± 7.0) and is
still worth **+20.8 own culture and zero margin** to the quiescent champion
(+4.3 ± 7.0, unchanged to the decimal). The reason is measured in §11.3: the
unforced production bot completes **one** Age III wonder in 80 seat-games,
so the two cards Bug 3 touched were essentially never scored at all.

---

### 11.1. The four bugs

#### 11.1.1 `Impact of Industry` scored the resource rating (over-scored)

Card, digital edition (`sources/bga_throughtheages_material.inc.php:3835`,
and the same wording in `data/cards_military_actions.json`): *"Each
civilization scores culture equal to the amount of resources its mines
produce. **(Ignore any production from other sources.)**"* Rules and BGO
agree; nothing to adjudicate.

`engine/events.py` read `v * s.resources`. Two things put resources on the
rating without being mine production, and the card data already says which
way each goes:

* **Bill Gates** — his own card text: *"stored as on a mine; **not affected by
  Transcontinental Railroad or Event: Industry**"*. Excluded.
* **Transcontinental Railroad** — *"one of your best mines produces twice as
  many resources"*; the card note cites FAQ v1.5 p.9, *"benefit counts toward
  Impact of Industry"*. Included.

Now `effects.mine_resources(p)`. Residuals before the fix were `+6×9, +9×6,
+4×2, +12×2, +7×1` — every one positive and every one a Bill Gates lab level,
which is what a rating-vs-mines confusion looks like.

#### 11.1.2 `Impact of Population` ignored unused workers (under-scored)

Card: *"2 culture per content worker above 10."* A yellow token in the worker
pool is a worker. The rulebook makes this concrete
(`sources/ubg_subsequent-rounds.txt`, "A Discontent Worker"): a discontent
worker is physically **an unused worker moved onto the happiness track**, and
"this worker still counts as an unused worker". So the population this card
counts is on-card workers **plus** the pool, minus discontent.

`engine/events.py` summed `t.workers` only. Residuals before the fix were
`−2×21, −4×8, −6×5, −8×2, −10×1` — all negative, all exact multiples of 2,
i.e. a whole number of uncounted workers.

**This one is not fully closed, and the remainder is the known-open happy-face
question, not the population formula.** Split by whether our engine says the
seat has discontent workers (clean rows only, after the fix):

| | n | exact | residuals |
|---|---|---|---|
| our discontent == 0 | 72 | **68** | +2×2, −2×1, −4×1 (mixed sign = replay noise) |
| our discontent > 0 | 16 | 5 | −2×5, −4×5, −12×1 (all negative) |

The obvious alternative — BGO does not subtract discontent at all — was
tested and **does not fit either**: 75/88 overall against the fix's 73/88,
7/16 on the discontent rows against 5/16, and its residuals go positive
(+2×9, +4×2). So neither reading is right on those 16 rows, our discontent
estimate is the suspect, and happy faces are exactly the input
[`docs/SCORE_AUDIT.md`](SCORE_AUDIT.md#108-limits) §10.8 says the journal never prints. Same open
question as `Impact of Happiness` (70.8%). The card says "content worker", so
discontent stays subtracted.

#### 11.1.3 Hollywood and Internet used printed production (under-scored)

Both cards score what buildings *give*, not what is printed on them:

* Hollywood: *"culture equal to twice the total culture production of your
  theaters and libraries"*
* Internet: *"culture equal to the combined culture, science and strength your
  urban buildings give"* — and `data/cards_wonders_leaders.json` already
  recorded the answer, *"CONFIRMED via fandom wiki + FAQ v1.5: leader effects
  on urban-building output count — Sid Meier, Shakespeare, Bach, Chaplin,
  Newton, Einstein"*. The engine implemented one of those six.

Those are exactly the six leaders who can still be alive when an Age III
wonder completes (§9.1: an Age I leader is dead before Age III), which is a
useful cross-check that the list is the whole list.

Rather than add five more special cases, `engine/effects.py` now has
`_BUILDING_OUTPUT`: a table mapping each modifier key to *(the building types
it modifies, the rating it modifies)*, and `building_output(p, types, attrs)`
which sums printed per-worker production over those buildings plus every
modifier whose types are a subset of the ones asked about. Hollywood, the
Internet and `Impact of Industry` are all three lines against it, so they can
no longer disagree with each other, and `_building_modifier` is deliberately
the same arithmetic as the matching branch of `_apply_modifier` so a card that
scores a building's output and the rating that building feeds cannot diverge.

The subset rule is what makes Shakespeare correct without a special case: his
`culturePerLibraryTheaterPair` reads a library *and* a theater, so it counts
for Hollywood (which asks about both) and would not for a theaters-only
question. Michelangelo is deliberately **not** in the table — he pays for
happy faces, not for output — which the corpus agrees with.

#### 11.1.4 (new) Charlie Chaplin doubled a whole card, not one building

Fixing 1.3 flipped Hollywood's residual sign for Chaplin: from `−8×9` to
`+8×6, +6×2, +16×1`. Those are twice `4×{1,2}` and `3×1` — a whole number of
*extra workers* on a 4-culture (Movies) or 3-culture (Opera) theater.

Card: *"Your best theater produces twice as much culture."* One theater = one
building = one worker, the same reading the engine already gave the
Transcontinental Railroad's *"one of your best mines produces twice as many
resources"* (which it implemented as one worker). `_apply_modifier` was
multiplying by `p.worker_count(b)`.

**This is the one bug of the four with an oracle outside the scoring code**,
because it changes the culture *rating* that BGO prints on every `End turn`
line, so it is testable on 43,847 rows rather than on ~100 scoring events:

| | before | after |
|---|---|---|
| culture production == BGO | 40,280 / 43,847 (91.9%) | **40,718 (92.9%)** |
| all five rates at once | 34,733 (79.2%) | **35,077 (80.0%)** |
| turns 16+, all five | 4,107 / 7,069 (58.1%) | **4,388 (62.1%)** |
| final positions passing the cleanliness gate | 405 / 2,525 (16.0%) | **454 (18.0%)** |

For scale, the largest replayer fix in [`docs/SCORE_AUDIT.md`](SCORE_AUDIT.md) (Ravages of
Time) was worth 89.4% → 91.9% on the same row. This one is a third of that,
and it is in the engine.

---

### 11.2. Before / after, whole corpus

`python3 tools/bgo_rescore.py --journals /tmp/bgo/journals`, 1,011 games,
0 crashes. Three columns because fixing 1.4 also lets *more replays pass the
cleanliness gate*, so the clean denominators move and are not comparable
across columns; the **all-rows** counts are, because their denominator is
fixed.

Clean rows (denominator moves in the last column — read the % not the count):

| oracle | before | after 1.1-1.3 | after 1.4 too |
|---|---|---|---|
| Impact of Industry | 61 / 81 (75.3%) | 81 / 81 | **95 / 95 (100%)** |
| Impact of Population | 43 / 81 (53.1%) | 68 / 81 (84.0%) | **73 / 88 (83.0%)** |
| Hollywood (at completion) | 20 / 35 (57.1%) | 26 / 35 (74.3%) | **44 / 44 (100%)** |
| Internet (at completion) | 46 / 65 (70.8%) | 60 / 65 (92.3%) | **63 / 68 (92.6%)** |

All rows, no cleanliness filter at all (fixed denominator):

| oracle | before | after |
|---|---|---|
| Impact of Industry | 452 / 542 | **542 / 542** |
| Impact of Population | 322 / 584 | **341 / 584** |
| Hollywood | 85 / 186 | **168 / 186** |
| Internet | 174 / 293 | **247 / 293** |
| Fast Food Chains (control, untouched) | 376 / 435 | 376 / 435 |
| First Space Flight (control, untouched) | 456 / 505 | 456 / 505 |

Nothing else in the fifteen-row `Impact of ...` table moved except by gaining
rows: Agriculture, Balance, Government, Progress, Science and Wonders are
still 100%, Technology 98.9%, and Happiness (70.8%) and Strength (64.3%)
are still the two [`docs/SCORE_AUDIT.md`](SCORE_AUDIT.md#108-limits) §10.8 lists as untestable.

#### 11.2.1 A fourth oracle, added to `tools/bgo_rescore.py`

[`docs/SCORE_AUDIT.md`](SCORE_AUDIT.md#1033-age-iii-wonder-completion-bonuses-ignore-leader-modifiers) §10.3.3's Hollywood/Internet table was computed ad
hoc and not committed, so there was nothing to re-run. It is now part of the
tool: every `"...; Wonder completed; <Colour> scores N culture"` line is BGO's
own Age III one-time bonus on a tableau we can rebuild, so the seat is frozen
at that instant and `effects.on_wonder_complete` is asked the same question.
The line is only used when **exactly one** wonder finished and **exactly one**
culture figure is attributed to its owner, so the number cannot be a sum of
two effects. A row is clean when the seat has no unmodelled events, its
tokens are conserved at that instant, and the last `End turn` before it had
all five production numbers exact.

This gate is stricter than §3.3's (35 clean Hollywoods against its 72), which
is why the counts differ from that document. The *signature* is identical:
before the fix, Hollywood was wrong on Chaplin 10/10 and Shakespeare 5/5 and
right on everything else; Internet on Einstein 11/11, Shakespeare 3/3, Newton
1/1, Chaplin 2/2, and right on Sid Meier 28/30.

---

### 11.3. The wonder A/B: re-run, and it did not move

This was the point of the exercise. [`docs/SCORE_AUDIT.md`](SCORE_AUDIT.md#1033-age-iii-wonder-completion-bonuses-ignore-leader-modifiers) §10.3.3 noted
that Bug 3's sign made wonders *worse* in our engine than in the real game,
and §6.2 measured forcing wonders at −34.3 ± 7.0 margin for the production
vector P and +20.8 own culture for the quiescent champion Q. Same command,
same frozen champion files, same seeds, after the fix:

**P, 1-ply-lineage production vector, 1-ply search, 40 deals = 80 games/row:**

| force | own culture | rival | margin | was (§6.2) | wonders |
|---|---|---|---|---|---|
| 0.00 | 155.1 ± 5.1 | 155.1 ± 5.1 | 0.0 ± 6.0 | 0.0 ± 6.1 | 0.73 |
| 0.10 | 145.1 ± 4.4 | 155.0 ± 5.1 | −9.9 ± 5.6 | −10.8 ± 5.6 | 1.45 |
| 0.20 | 147.3 ± 4.4 | 154.9 ± 4.8 | −7.6 ± 6.0 | −8.4 ± 6.0 | 1.8 |
| 0.40 | 148.3 ± 5.0 | 153.6 ± 4.8 | −5.3 ± 6.4 | −6.1 ± 6.4 | 2.4 |
| 0.70 | 139.8 ± 5.1 | 162.6 ± 4.9 | −22.8 ± 6.0 | −23.4 ± 6.1 | 3.1 |
| 1.00 | 125.5 ± 5.6 | 158.9 ± 4.6 | **−33.4 ± 6.9** | −34.3 ± 7.0 | 3.8 |

**Q, quiescent champion `levels=1`, 25 deals = 50 games/row:**

| force | own culture | rival | margin | was (§6.2) | wonders |
|---|---|---|---|---|---|
| 0.00 | 64.9 ± 6.4 | 64.9 ± 6.4 | 0.0 ± 5.5 | 0.0 ± 5.5 | 0.40 |
| 0.20 | 71.6 ± 6.9 | 72.3 ± 5.8 | −0.7 ± 6.1 | −0.7 ± 6.1 | 0.88 |
| 0.50 | 80.8 ± 5.9 | 81.1 ± 5.4 | −0.3 ± 5.8 | −0.3 ± 5.8 | 1.46 |
| 1.00 | 85.7 ± 6.7 | 81.4 ± 5.8 | **+4.3 ± 7.0** | +4.3 ± 7.0 | 1.9 |

**Every P row moved by less than a fifth of its own standard error and every Q
row is identical to one decimal place.** [`docs/SCORE_AUDIT.md`](SCORE_AUDIT.md#1063-what-this-does-and-does-not-license) §10.6.3's
conclusions stand unaltered: wonders are still bad value for the strongest
vector we have, and still invisible-but-margin-neutral for the champion.

#### 11.3.1 Why it did not move — measured, not assumed

Because the two cards Bug 3 touched are almost never scored. Counting which
wonders actually complete, P at 2p, 40 deals × 2 seats = 80 seat-games:

| | force 0.0 (the real bot) | force 1.0 |
|---|---|---|
| Fast Food Chains / game | 0.000 | 0.188 |
| First Space Flight | 0.000 | 0.025 |
| **Hollywood** | **0.013** (1 in 80) | **0.100** |
| **Internet** | **0.000** | **0.100** |
| all wonders | 0.725 | 3.8 |

**The unforced bot completes one Age III wonder in 80 seat-games**, total,
across all four of them. Even at full forcing Hollywood and the Internet land
0.1 times each per game, and their average under-score was ~3.5 and ~1.0
culture — so Bug 3 was worth about **0.45 culture per game at maximum
forcing** and ~0.05 in normal play, against a −33 margin. It was never a
candidate explanation, and this is the number that says so.

The mechanism is mundane: Age III wonders cost 14-18 resources across 3-5
stages and become available in the last few rounds of a 20-round game. Any
future wonder work should price *when* a wonder is reachable, not whether its
payoff is implemented — §6.3's untested civil-action-budget story is still the
live hypothesis and is still untested.

#### 11.3.2 What Bug 4 does to bot play (it is not free either)

Chaplin is the final leader in **22-33%** of P's games, so 1.4 is the one fix
that changes ordinary play — and it changes it *downward*: we were
over-crediting Chaplin's culture rating. It is small (P's unforced culture
155.2 → 155.1, inside noise) but it is not nothing, and it is why all six gate
digests moved.

---

### 11.4. Gate digests

`bash tools/gate.sh` on master before any change: **GATE PASS, 401 tests**,
`NARROW 2fd656b3`, `WNARROW 7fc72fca`, `WIDE 1169007d`, `WWIDE 9dc0a5a6`.

All four moved. Re-derived per [`docs/PYPY.md`](PYPY.md) 9.0's rule — computed from
scratch in the working worktree and independently in a second detached one,
with the two required to agree — and **attributed rather than assumed**: each
of the four fixes was reverted on its own and all four arms re-hashed.

| | old | new |
|---|---|---|
| NARROW | `2fd656b3` | `0a6ed6ad` |
| WIDE | `1169007d` | `4a8c6ca6` |
| WNARROW | `7fc72fca` | `302c546c` |
| WWIDE | `9dc0a5a6` | `4e40a58c` |

The attribution (`SAME` = that fix alone does not move that arm):

| revert | NARROW | WIDE | WNARROW | WWIDE |
|---|---|---|---|---|
| 1.1 Industry | SAME | SAME | `142b3371` | `d7328f3a` |
| 1.2 Population | **`2fd656b3`** | **`1169007d`** | `4ce2cf6e` | `ecbfc9dd` |
| 1.3 Hollywood/Internet | SAME | SAME | SAME | SAME |
| 1.4 Chaplin | SAME | SAME | SAME | SAME |
| 1.1 **and** 1.2 together | — | — | **`7fc72fca`** | **`9dc0a5a6`** |

Read the bold cells: reverting only 1.2 puts both GreedyBot arms back on
their old master digests exactly, and reverting both `engine/events.py` hunks
puts both WeightedBot arms back on theirs. So the whole movement of all four
digests is the two `Impact of ...` fixes and nothing else.

**Two of the four fixes move no digest at all, and that is a coverage
finding.** The fingerprint's 135 games essentially never complete an Age III
wonder (§11.3.1: one Hollywood in 80 seat-games for the *trained* production
vector; zero for GreedyBot and DEFAULT_WEIGHTS), and never reach Chaplin with
two workers on his best theater. `tools/gate.sh` cannot catch a regression in
either 1.3 or 1.4 — only `tests/test_scoring_bugfix.py` and
`tools/bgo_rescore.py` can. That is written into `tools/gate.sh` next to the
constants, in the same place as every other cause note.

---

### 11.5. Negatives, nulls and what is still open

* **The wonder A/B is a null.** That is the headline result of §11.3 and it is
  reported as a null, not buried: the fix that was hoped to change it changed
  it by less than a fifth of a standard error, and the reason is that the
  affected cards are never played.
* **`Impact of Population` is 83%, not 100%**, and the residual is entirely on
  rows where our engine computes discontent > 0. Two readings were tested and
  neither fits. Unresolved; same root as `Impact of Happiness`.
* **`Impact of Happiness` (70.8%) and `Impact of Strength` (64.3%) are
  untouched and still open.** Nothing here looked at either. If a fifth
  scoring bug exists it is behind happy faces or behind tactics, exactly where
  [`docs/SCORE_AUDIT.md`](SCORE_AUDIT.md#108-limits) §10.8 said it would be.
* **`Impact of Colonies` is 86.2%** with symmetric ±3 residuals and
  Architecture / Variety / Competition sit at 92-93% with small mixed-sign
  ones. §10.2 of the former `docs/SCORE_VALIDATION.md` attributes these to the replayer (stolen
  colonies, unseen military workers). Not re-examined here; nothing in this
  branch moved them.
* **The three controls did not move**, which is the check that the refactor
  did not smear: Fast Food Chains and First Space Flight are 376/435 and
  456/505 before and after, and Sid Meier's Internet rows stayed at 28/30.
* **2p only**, same as everything before it. Nothing was run at 3p or 4p.
* **`n` is 80 games per A/B row for P and 50 for Q**, unchanged from §6.2, so
  the "did not move" claim is a claim about a shift of ≲ 1 point, not about
  exact equality.

### 11.6. Reproducing

```
tar xzf sources/bgo/journals.tar.gz -C /tmp/bgo
python3 tools/bgo_rescore.py --journals /tmp/bgo/journals   # all four oracles

cp experiments/league_state/champion_2p.json /tmp/Q2p.json
cp experiments/archive_preplan/league_state_1ply_20260726/champion_2p.json /tmp/P2p.json
nice -n 19 python3 tools/wonder_ab.py --spec /tmp/P2p.json --deals 40 \
    --force 0 --force 0.1 --force 0.2 --force 0.4 --force 0.7 --force 1.0
nice -n 19 python3 tools/wonder_ab.py --spec quiesce:/tmp/Q2p.json,levels=1 \
    --deals 25 --force 0 --force 0.2 --force 0.5 --force 1.0
```

Everything ran `nice -n 19` alongside three live league arms.
