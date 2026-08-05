# Open items register

**This is the one place open work is recorded.**  It supersedes
`docs/OPEN_AFTER_THE_AUDIT.md` (whose contents are §1 below) and
`docs/ARCHAEOLOGY.md` (the 2026-07-26 lost-work dig, whose ranked shortlist is
§6), both deleted 2026-07-30 in the documentation consolidation.  Standing
hazards — the things that have already cost a bug — live in [`docs/HAZARDS.md`](HAZARDS.md),
not here.

Every item says where it came from.  Items marked **(snapshot)** were true when
measured and have an expiry: check the code before trusting them.  That warning
is not decoration — `ARCHAEOLOGY.md`'s own item 12g went stale within 24 hours
of being written.

---

## 1. Open after the 2026-07-29/30 card audit

*Migrated verbatim from `docs/OPEN_AFTER_THE_AUDIT.md`.*

### 1.1 `wonder_potential`'s scale has no trustworthy evidence

Measured effect at 0.125 vs 0.0 (`tools/wonder_mechanism.py`, mirror, 500
deals): wonder completions **0.408 vs 0.051**, started 0.674 vs 0.083, finish
rate 0.369 vs 0.051, civil actions on stages 1.372 vs 0.167.  Eiffel Tower goes
from **zero completions in 1000 seat-games** to 0.082/deal.

At 0.5 it turns pathological — 2.69 started, 2.09 abandoned, finish rate 0.23 —
so 0.125 is the top of the usable range.  **But the accompanying strength null
(50.34% +/- 2.43pp, n=1600) was measured against the frozen 78-key champion
missing `row_urgency`**, which [`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md#1210-the-territory-suspect-and-the-defence-drain-are-one-defect-2026-07-30) §12.10 shows is a broken
yardstick: the reprice changed `evaluate()` on 0 of 480 wonder-in-row states.
The behavioural numbers survive; the strength conclusion does not.  Re-run
against a live reference vector before quoting any of it.

The right answer is probably not a hand-set constant at all: leave the weight at
0.0 and let the league find it, which is what the restart is for.

### 1.2 Abandoned wonder programmes regressed in absolute terms

Started-but-unfinished rose **0.032 -> 0.271 per deal** at `wonder_potential`
0.125.  The finish *rate* improved (5% -> 37%), but the absolute count of
abandoned programmes went the wrong way, against the standing 23-44% improvement
recorded in [`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md#53-the-mechanism-is-not-wonders-and-the-reason-is-a-plumbing-bug) §5.3.  Unresolved: is a bot that starts
eight times as many wonders and finishes 37% of them better or worse than one
that starts almost none?  The objective should answer this and nobody has asked
it.

### 1.3 The three bonus cards are priced; the seam is closed; the weights mostly aren't

`_BONUS_TO_FEATURE` in `engine/bots/weighted.py` maps `defenseBonus` ->
`defense_bonus` and `colonizationBonus` -> `colonize_bonus`, and `_card_yields`
prices them (gated by `bonus_card_credit`, 1.0 on all three live champions).
The seam is closed too: `hand_mil_potential(state, idx, w)` now calls
`card_potential(n, w, state, idx)` with both, so board-aware pricing is no longer
blocked for military cards by construction — though `board_yields` /
`board_extra` / `_board_credit_key` still have no entries for a military type,
so a military card's board credit falls through to the bare `card_board_credit`,
which is 0.0 on all three live champions.

Reading `experiments/league_state/champion_{2,3,4}p.json` (118-key vectors,
2026-07-30) **(snapshot)**: `defense_bonus` is 0.0 on all three — priced but
inert, so the defence increment those cards carry is still effectively unpriced.
`colonize_bonus` is 0.0 at 2p but **0.04196 at 3p and −0.07368 at 4p**.
`hand_mil_potential` is 0.0 at 2p/4p, **0.01079 at 3p**.  So two of the three
cards have a real nonzero champion weight today and only `defenseBonus` is
priced-but-inert everywhere.  Full derivation, conduction table, and where the
3p effect shows up (the end-of-turn military discard,
`engine/interact.py:_discard_military`) is [`docs/COMBAT_AUDIT.md`](COMBAT_AUDIT.md).

### 1.4 `cost.militaryActions` is read by no bot code

Re-checked 2026-07-30 and still true: 54 cards carry it.  The rules engine gates
legality on it (`actions.py:269,1083`, `events.py:493`) and nothing under
`engine/bots/` reads `card.get("cost")` at all, so War over Culture (3 MA) and
War over Territory (2 MA) are the same card to every pricing path.
`_EFF_TO_FEATURE`'s `militaryActions` -> `military_actions` entry (nonzero on all
three live champions, e.g. 3.47652 at 3p) is a different thing: it prices a
card's *grant* of military actions, not a war card's cost to play.

[`docs/COMBAT_AUDIT.md`](COMBAT_AUDIT.md) records the reason this is deliberately still unpriced:
pricing the cost alone, without also pricing what the card buys, would reproduce
the project's worst historical pricing defect — unit cards scoring strictly
negative.

### 1.5 The defence drain is landed and ON (`a214804`, 2026-07-30)

`QUIET_PENDING = True`.  It landed as a **consistency fix**, not on a strength
measurement — the beam already drains before scoring and the live decision did
not.  Read the A/B in [`docs/DEEPER_SEARCH.md`](DEEPER_SEARCH.md#8-the-drain-ab-why-quiet_pending-was-flipped-to-true-merged-from-the-former-drain_abmd-2026-07-31) §8 rather than from a summary, because it
is uneven: 3p is decisive (pure-`qp` pool 0.5217 own-win share against a 0.3333
null over 600 games, z = 9.26) and **4p is NOT independently established** (one
pure-`qp` block, 0.3000 against a 0.2500 null, z = 1.54, p ~ 0.12).  Do not quote
a single pooled z for "the drain A/B" — there are two pools and they say
different things.

Diagnosis retained because it is the record of what the defect was: across 1,549
defences faced and **1,104 winnable by arithmetic, zero were ever held off**,
while cards were spent in 335 hopeless ones.  588 of 589 winnable defences need
2+ cards, so the first `defend` always looks like pure cost.  The fix takes
held-off defences 0 -> 332 over 200 games at 4p.

* Not mainly about defence: the short-circuit never tested the pending *kind*,
  and **auctions** are 71.6% of the decisions the drain moves (455 seen, 326
  moved at 3p) against defence's 37.8%.  Same defect [`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md#1210-the-territory-suspect-and-the-defence-drain-are-one-defect-2026-07-30) §12.10
  reached from the territory end.
* The leak objection was answered, not argued away.  Neither pending path
  determinized, so a trial `apply` drew the real next deck card: master leaked on
  24.0% of candidate evaluations at 3p and the drained arm on 34.7%
  (`tools/pending_leak.py`).  The leak-neutral contrast `qp=1,qd=1` vs plain
  returned the same numbers to every printed digit (0.5325 win, +26.01 margin) —
  removing the peek changes nothing, so the win is not the peek.
* **Open:** five more 4p blocks and three more 3p blocks were queued 2026-07-30
  to settle the 4p question.  Check whether they landed before quoting a
  resolved 4p figure.
* Scope, checked rather than assumed: only `plan.py` and `neural_plan.py` import
  `engine/bots/pending.py`, so this changes PlanBot and NeuralPlanBot only.

### 1.6 3p `row_urgency` has an arbitrary sign

`+0.163` on the live 3p champion where the semantically correct sign is negative
(`row_pressure` is evaluated post-move and measures urgency *left behind*).  It
is active on 35% of 3p decisions, but flipping it is worth `+0.0025 +/- 0.0305`
over n=600 — no usable gradient, so the climb drifted to a wrong sign without
ever paying for it.  **Any 3p measurement that reads card ordering is reading an
arbitrary sign.**  Win rate and margin are unaffected.

### 1.7 The human corpus cannot validate what it cannot vary

[`docs/SCORE_AUDIT.md`](SCORE_AUDIT.md#2-the-field-coverage-sweep) §2.  At 2 players every pact is removed from the game and
the corpus is 2p only, so "food your farms produce" and "your food rating" are
identically equal in all 2,525 positions — which is how a broken card scored
66/66 exact.  Five of the nine scoring bugs sit inside four documented blind
spots.  The corpus is decisive exactly where it has variation and silent, while
reporting perfection, everywhere else.  Before quoting a corpus percentage, ask
what inputs produced it and whether they could have distinguished the
alternative.

---

## 2. Card pricing and coverage

*From [`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md) (which now also carries the former `CARD_CENSUS.md`,
`CARD_PRICING_LEADERS.md` and `CARD_BLINDNESS_MILITARY.md` as §§11-13),
[`COVERAGE_AUDIT.md`](COVERAGE_AUDIT.md), [`UNCOVERED_TYPES.md`](UNCOVERED_TYPES.md),
[`SYSTEM_COVERAGE.md`](SYSTEM_COVERAGE.md).  Those documents are all still live; this is the index of
what they leave open.*

Ranked, most agreed-upon first.  Weight values are **(snapshot)** as of
2026-07-30 — a concurrent lane was repricing unit technologies.

1. **Military unit technologies (10 cards)** price strictly *negative*, not
   merely unpriced, because `row_pressure` skips any card with
   `card_potential <= 0` and `hand_potential` sums the raw negative, so *holding*
   a unit card lowers the evaluation.  `unit_strength_credit` is the gate, 0.0.
   [`docs/SYSTEM_COVERAGE.md`](SYSTEM_COVERAGE.md) measures the behavioural consequence at 26-47x
   under the human take rate and ranks it the top actionable hole in the system.
   Root cause is structural: the only term large enough to flip a unit card
   positive is `strength_deficit` (−0.736, conditional on being behind), a board
   query — **no per-card table will ever price units correctly.**
2. **Twelve special technologies** all price net negative: the science cost is
   trained, the benefit side (`build_discount`, `wonder_stages_per_action`,
   `colonize_bonus`) is 0.0.  Take rate 0.87% (14/1,606 offers); 6 of 12 taken
   zero times in 40 games.
3. ~~**Every yellow production technology prices net negative, and the bot buys
   none of them.**~~ **CLOSED 2026-07-30 by [`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md)** —
   labs 0.02 → 1.77 per seat-game at 2p against a human 1.62, mines 0.03 →
   0.85, farms 0.07 → 0.87, and the blue over-play fell out with it (theatres
   2.23 → 0.82 against 0.65).  The diagnosis below is kept because it is what
   the audit believed and it was **wrong in an instructive way**: the binding
   cause was not the `culture_rate` / `science_rate` ratio but that
   `card_potential` read `w[k]` where `evaluate` reads
   `w[k] + (1−L)w[k_early] + L·w[k_late]` (`science_rate` 0.25 against 5.29
   early), and that `tech_levels` — worth up to 9.23 eval points per level —
   was mapped to nothing at all on every technology card in the game.  The
   original text:
   Measured 2026-07-30 ([`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md#1751-why-yellow-is-dead-and-blue-is-doubled--class-c-and-probably-wrong) §17.5.1): labs
   0.03 taken per seat-game against a human 1.62 at 2p and **exactly 0.00** at
   3p and 4p, mines 0.05 against 1.18, farms 0.18 against 1.34; Alchemy,
   Scientific Method and Coal are the only three cards in the game the bot
   never touches at ANY table size.  Unlike item 1 this is **not** an inert
   weight — `food_rate`, `resource_rate` and `science_rate` are all live — it
   is a ratio: `culture_rate` is 31.68 on the 2p champion against
   `science_rate` 0.25, so a culture card beats a science card by construction,
   and `_PROD_TO_FEATURE` charges a lab's `techCost` through `science` (0.33)
   at a higher weight than the `science_rate` (0.25) it buys.  Whether that is
   a pricing bug or a real 2p optimum is unsettled and unowned.  It is the
   largest non-inert behavioural discrepancy in the game.
4. ~~**`free_civil_action` is non-positive on every trained vector** (0.0 /
   −0.160 / −0.084 at 2p/3p/4p): the 18 action cards that grant a free civil
   action are priced to be *disliked* for granting it.~~  **CLOSED 2026-07-30
   by [`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md)**, and the diagnosis above was *too kind*:
   the weight is not merely non-positive, it is **unreachable**.
   `free_civil_action`, `resource_discount` and `restricted_resources` are all
   three absent from `features()`, so `evaluate` never multiplies them by
   anything and no game the league plays can produce a gradient on them.
   Thirteen action cards carry nothing else and priced at **exactly 0.000**.
   `weighted.action_value` now prices a free civil action at
   `feature_marginal("civil_actions")` and the two ring-fenced yields at the
   `resource_stock` marginal — coordinates `evaluate` does pay for.  The three
   old weights are kept as the stateless answer and are dead on the live path.
   ("Action cards are the bot's least-broken type" was also wrong, and item 24
   below is where that was already corrected.)
5. **`cost.militaryActions` on 54 cards** — see §1.4.
6. **`card_board_credit` = 0.0 and `gov_action_cost` = 0.0**: the entire
   board-aware pricing machine for leaders, actions and governments is built and
   inert.  If anyone turns it on, **turn `card_board_government = 1.0` on
   first** — it is the only individually significant positive result (culture
   margin +1.85, z = 3.4).  The leader half is a confirmed null (z = −1.46,
   p = 0.15 after correct deal-clustering; the original z = −2.1 headline was
   withdrawn).
   **The government third of this is answered as of 2026-07-31** and only the
   leader and action thirds are still inert: `gov_board_credit` (1.0 by
   default) routes a government through `weighted.gov_value` *before* the
   `card_board_credit` machinery, so the advice above no longer applies to
   `card_board_government` — that knob is now what a vector with
   `gov_board_credit` = 0.0 falls back to.  `docs/GOVERNMENT_PRICING.md`.
7. **`wonder_potential` = 0.0 in every champion, frozen and live.**  A wonder
   physically cannot enter `hand_civil` (`actions.take_card` branches to
   `p.wonder`), so `hand_potential` — the one live card-identity channel — never
   sees it.  This plumbing fact is weight-independent and survives any reweight.
8. **Five wonders still price 0.00** (Ocean Liners, First Space Flight, Fast Food
   Chains, Internet, Hollywood — text-effect / board-scaled).  Zero Age III
   wonder completions ever, across 260 seat-games.  Pyramids: taken 7x, finished
   0x, against humans building it in 499 of 692 games.
9. **`copy_tactic` is a live, unassigned pathology** — the champion copies ~10.5
   tactics/game at 2 military actions each into zero armies, because the
   evaluator rewards avoiding the smaller-hand bookkeeping penalty over actually
   playing the tactic.  The bookkeeping-vs-value ratio was ~11:1 when first
   measured and **got worse on the live champion: 27.3:1, or 60.4:1 including
   `ma_left`.**
10. **`tactic_gain` is a dead coordinate** (0 divergences across a 7x weight range
   in 967 decisions) because the champion always already holds a tactic and owns
   zero units.  Tactics are confounded with the unit defect — **re-measure after
   units are fixed, do not fix in parallel.**
11. **`resign`**: engine-correct, but no feature reads `p.resigned`; 0 wins from 9
   resignations in an 8-game probe.  The bot throws games over a rounding
   artefact in `lateness()`.  Needs a policy change plus an n>=200 A/B.
12. **`row_urgency` carries the same hand-double-count bug** for leaders and
    governments in the *row* (not the hand) that §10 of
    [`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md) fixed for the hand.  Explicitly "the next
    thing to do in this area".
13. ~~**Production buildings** (24 cards) are mis-shaped twice: absolute instead
    of delta, and the worker cost omitted.~~  **The delta half is CLOSED**
    2026-07-30 ([`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md)): `board_yields.tech_upgrade`
    prices every worker technology as the upgrade diff off what the player
    already has, using `actions.upgrade_cost` and an `effects.compute` diff.
    The worker-cost half becomes item 21 below, correctly restated.
    Professional Sports is never taken (0/127), undiagnosed.
14. **Aristotle and Newton need a *measured* trigger rate before pricing**, not a
    guessed one; `tools/take_census.py` is most of the machinery and the
    measurement has not been made.  Four leaders (Aristotle, Hammurabi,
    Christopher Columbus, Frederick Barbarossa) are deliberately flat-zero,
    guarded by `TestEveryLeaderIsPriced.STILL_FLAT`.
15. **Three decisions nobody has made** ([`docs/COVERAGE_AUDIT.md`](COVERAGE_AUDIT.md#7-handoffs) §7): should
    `rival_culture_rate` / `rival_science_rate` / `rival_strength` be made live or
    deleted; should `wonder_remaining` be sign-locked or replaced; should
    `p.resigned` get any evaluator term at all.
16. ~~**The one clean rules-level engine defect** in the whole coverage census: the
    unit-sacrifice-for-colony choice is taken away from the player by the engine.
    Ranked more serious than any pricing gap.~~  **CLOSED 2026-07-31.**  The
    diagnosis was exactly right: `interact._build_force` picked the winner's
    force by a fixed rule (weakest unit, then bonus cards cheapest-first, then
    more units), while RULES_SPEC 11.3 fixes only the floor — *">= 1 unit
    mandatory, even if other bonuses would cover the bid"* and *"the
    colonization value (bottom half) of ANY NUMBER of military bonus cards
    played"* — and 11.2 only the total.  It is now a `colonize` pending
    decision (`send_unit` / `send_bonus` / `send_done`) on the same
    `state.pending` plumbing as `auction`, `defense` and `choice`, so
    `state.decider()` correctly gives it to the auction winner even when that
    is not the current player (§11.6).  Single-move prefixes auto-resolve, as
    `push_choice(auto=True)` does, so nobody is asked a question with one
    answer.
    **The half of this that was NOT in the diagnosis, and is the reusable
    lesson:** a new decision whose cost only lands on the terminating move is
    unanswerable by a 1-ply bot.  Written the obvious way — `send_unit`
    appends a name to the pend dict, `send_done` pays for everything —
    GreedyBot sacrificed its whole five-unit army for a force of 3, because
    committing was free in the trial state and sending was expensive, so it
    procrastinated until the pool ran out.  Fixed in the engine, not per bot:
    each commitment is paid the moment it is made.  Same shape as the
    `bid`-scores-exactly-like-`bid_pass` bug that `weighted.deferred_credit`
    papers over, and it is worth asking whether that one could be fixed the
    same honest way.  Pinned by
    `tests/test_colony_sacrifice_choice.py::test_nobody_wastes_the_whole_army`.
17. **Zero card takes *during Age IV* at every player count** (260/260
    seat-games), against a human 1.59 / 1.57 / 1.78.  **STILL OPEN.**

    Restated 2026-07-31 to say what it is actually counting, because it was
    briefly mis-closed as a false defect on the strength of a true fact:

    * **There are no Age IV *cards*.**  The corpus holds 33 / 65 / 75 / 63 for
      A / I / II / III and **zero** for IV, and `_advance_age` sets
      `state.civil_deck = []` / `state.military_deck = []` on entering IV.
      Pinned by `tests/test_age_iv_empty.py`.
    * **That is not what the census row measures.**  It counts takes made
      *during the Age IV phase*, and the card **row is not emptied** when the
      deck is — it still holds leftover Age III cards, and taking one is
      legal.  Humans take 1.6–1.8 of them.  The bot takes exactly zero.

    So the denominator is **not** zero and the finding survives.

    **The denominator, now measured rather than argued** (2026-07-31,
    `tools/age_iv_row.py`, new with this entry — it taps every Age IV decision
    on a real state and records what was on the row, what `legal_moves`
    emitted, and what was taken).  2p, 20 games, 40 seat-games,
    `DEFAULT_WEIGHTS` under `WeightedBot`:

    | | |
    |---|---|
    | Age IV decisions | 288 |
    | ...with a non-empty row | **288 (100.0%)** |
    | ...with at least one LEGAL take | 187 (64.9%) |
    | mean cards on the row | 7.48 |
    | legal takes offered | 1,091 |
    | **takes made** | **80 = 2.00 per seat-game** (human 1.59) |

    Two things follow, and the second is the one that matters.  The row is
    non-empty on **every single** Age IV decision, so "never offered" is
    conclusively dead.  And `DEFAULT_WEIGHTS` takes 2.00 per seat-game — *above*
    the human rate — so **the exact zero is a property of the vector and the
    search the census ran (`plan:width=2,det=1` on trained champions), not of
    the engine and not of the evaluator's default configuration.**  That
    reclassifies the item from a coverage hole to a pricing/weights one and it
    should be re-censused on the live vectors before anything is built for it.
    Bound: one player count, one bot (`WeightedBot`, not `PlanBot`), n=20 games;
    it is a denominator measurement, not a replacement census.

    [`docs/SYSTEM_COVERAGE.md`](SYSTEM_COVERAGE.md) §5 labels it **(c)**,
    probably-correct play — Age IV is one or two rounds long and the bot
    spends its civil actions elsewhere — but it is an exact zero, and exact
    zeros are listed as defects here until something explains them.  What
    would settle it: whether those leftover takes are worth their civil action
    that late, which needs the endgame culture they pay to be priced at all
    (item 21, and the same missing endgame-culture channel that §1 blames for
    the expensive wonders).

    **Method note, and the reason this entry now carries one.**  A rate of
    zero has two causes that look identical in a table — *never chosen* and
    *never offered* — and the audit could not tell them apart.  Any future
    census reporting a zero rate must state its denominator alongside it.
    The near-miss also had this section's recurring shape, *"in this list but
    not this one"*: `engine/cards.py` declares
    `AGES = ["A", "I", "II", "III", "IV"]` and the data supplies four of the
    five, so "Age IV" names a phase in one place and a (nonexistent) card
    cohort in another, and nothing failed when the two were conflated.
18. **`ca_left` is a genuine 1-ply pass-asymmetry artefact** (`end_turn` always
    has the highest `ca_left` of any candidate, 165/165 measured, mean +2.95)
    that the `NONNEG` guard stops the search correcting.  Deliberately unchanged;
    needs an n>=200 A/B.

19. **`tech_levels` on the live 2p champion is a stale, over-fitted
    coordinate, and it is the single most important open item in this
    section.**  [`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md#1542-the-live-2p-champion-a-severe-regression-attributed) §15.4.2: with the technology price
    on, that champion goes **12.2% against a 50% null**; reset only its
    `tech_levels` group to the defaults (5.84/3.39/0.92 → 1.0/0.5/−0.4) and the
    same paired A/B is **63.0%**.  The mechanism is the general one
    [`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md#1452-3p-on-the-archived-champion-a-large-unambiguous-regression) §14.5.2 named for `strength`: a coordinate the
    evaluator can read but never has to buy is unconstrained and will drift.
    Two things follow.  (a) **Settled 2026-07-31, no action needed.**  The live
    `experiments/league_state/champion_2p.json` (gen 73) was inspected directly
    and already carries `tech_levels` 1.0 / 0.5 / −0.4 with
    `tech_board_credit` 1.0 — i.e. it *is* the 63.0% configuration, not the
    12.2% one.  The blown-up 5.33/5.84 pair survives only in an old
    `weight_guard` range record inside `generations_2p.jsonl`; the last three
    generations do not carry it.  3p is likewise at the defaults; 4p has
    climbed its own group to 0.43/0.31/−0.14, moved but not runaway, and is
    left alone.  The `"tech_board_credit": 0.0` workaround is therefore
    unnecessary and was not applied — restarting the 2p arm does not begin
    from a poisoned vector.  (b) **still open, and is now the whole of this
    item: every other coordinate that no card price has ever
    charged for is suspect for the same reason.**  `strength` was one,
    `tech_levels` was the second; `num_techs`, `special_techs`, `workers`,
    `prod_workers`, `urban_workers` and the whole `best_*` family have never
    been paid for at take time either.  A sweep is overdue.  Supporting
    observation from the same 2026-07-31 inspection: the live 2p champion
    carries `best_library` 9.40 and `culture_rate` 31.68, both far outside any
    other coordinate's range, which is exactly the shape unconstrained drift
    takes.  Neither has been shown to be wrong — that is what the sweep is
    for — but they are where it should start.
20. ~~**`unit_upgrade` pools workers across all four red types.**  It moves every
    unit worker onto the candidate technology and charges `actions.upgrade_cost`
    from each, but `engine/actions.py:_action_moves` only offers an upgrade
    between cards of the **same** type — a Warriors worker cannot become a
    Cannon.  So the red price is optimistic for cavalry, artillery and air
    whenever the player holds only infantry, which is most of the game.  Found
    2026-07-30 while generalising it ([`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md) §15.6.1) and
    deliberately left alone so this lane's digest moves had one cause;
    `tech_upgrade`'s non-red half is already same-type-only.~~
    **CLOSED 2026-07-31 by `docs/CARD_BLINDNESS.md` §14.8 (the former `UNIT_TECH_PRICING.md`).**  The legality claim
    was re-verified against `_tableau` before anything was changed, and the size
    of the error measured: on a board with four Warriors workers,
    `unit_upgrade("Cavalrymen")` read **`(8.0, 6.0, 12.0)`** — eight strength
    for a move the engine never generates.  `unit_upgrade` now shares
    `_upgradable_onto` and `_with_tech` with `tech_upgrade` rather than keeping
    a second opinion about what an upgrade is.  Take rates per seat-game at 2p
    on `DEFAULT_WEIGHTS` went cavalry 0.35 → 0.15, artillery 0.23 → 0.10, air
    0.08 → **0.00**, infantry 0.40 → 0.48 (human 1.22 / 0.85 / 0.65 / 1.12) —
    the three the old price was inventing upgrades for fell, the one that has
    real upgrades rose, and **the residual is item 21 below**, which the fix
    promotes from a footnote to the binding constraint on the red lane.
21. ~~**Nothing prices the "build one fresh" plan.**  Both `unit_upgrade` and
    `tech_upgrade` answer "develop it and upgrade what I have", so a player with
    no laboratory worker sees a laboratory priced at its levels minus its
    science and the +3 culture a *new* theatre would produce is priced at
    nothing.  Honest for a one-ply appraisal — building is its own decision,
    needs a free worker and has its own price — but it systematically
    under-prices the first building of a type.  This is the correctly-shaped
    replacement for the old item 13.~~
    **MOSTLY CLOSED 2026-07-31 by [`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md#149-nothing-priced-the-build-one-fresh-plan-2026-07-31) §14.9.**
    `board_yields.build_fresh` prices "develop it and BUILD one fresh worker
    on it" for all fifteen technology types and `weighted.tech_value` takes
    the better of the two staffing plans, gated on `build_fresh_credit` —
    which ships at **0.0**, for the reason two paragraphs down.
    The gate, the urban limit and the resource cost are `_action_moves`' and
    `do_build`'s own; the four features a build moves that no `Stats` diff can
    see (`free_workers`, `workers`, `<class>_workers` and the `uprising`
    threshold) are emitted explicitly and pinned against `features()` itself.
    Knights goes −0.28 → +2.51 on a fresh 2p board, out of `row_pressure`'s
    `val <= 0.0` skip; air takes go 0.015 → 0.075 per seat-game and all red
    0.730 → 0.845 against a human 3.841.

    **THE CREDIT SHIPS AT 0.0, WHICH ITS FOUR SIBLINGS DID NOT, AND THAT IS
    THE MEASUREMENT'S DECISION.**  Paired 400-game A/Bs at 2p, the credit
    against itself: **44.1% ± 4.6 (p = 0.0125)** on `DEFAULT_WEIGHTS` and
    **44.5% ± 4.8 (p = 0.0229)** on the live gen-83 champion — two vectors,
    the same ~5.5pp loss.  The champion row rules out the obvious cause: it
    already carries `workers` = 0.0 and `free_workers` = 0.0046, so the
    untrained worker priors are not what is over-valued, and **this lane did
    not find what is**.  At 0.5 the same A/B is 44.9% ± 3.6 (p = 0.0055) —
    the sweep is a STEP, not a slope, because the credit multiplies one branch
    of a `max` and any ε > 0 already wins that max on every card whose upgrade
    plan is worth nothing.  So this is not a knob the league can tune down
    gently, and "the league will find the level" is NOT available here; see
    [`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md#1496-the-credit-ships-at-00-and-that-is-the-abs-decision) §14.9.6 and item 31 below.

    **TWO HALVES OF IT STAY OPEN, and they are stated here because the fix
    could have hidden them.**

    * **The plan is not priced when the free-worker pool is empty**, which is
      **68%** of decisions at 2p under `DEFAULT_WEIGHTS` (measured: 1,318 of
      1,932 over 8 games; 27% hold exactly one and 4.7% hold two or more).
      The rules make that reachable — one civil action plus
      `economy.pop_food_cost` food, RULES_SPEC §3 action 3 — so the honest
      completion is a *third* plan, "increase population, then build", whose
      costs (`food_stock`, `yellow_bank`, `consumption`, and the happy
      requirement rising) are all features `evaluate` already reads.  Not
      done here: it is a three-move plan inside a one-ply appraisal and it
      deserves its own A/B rather than a free ride on this one.
    * **Only ONE fresh worker is priced.**  That is nearly free at 2p (see the
      histogram above: a plan of two is legal on 4.7% of boards) but it will
      not be at 4p, and the trade is genuinely non-linear — `p.mil_discount`
      is a per-turn pool `_spend_mil_discount` draws down, `uprising` is a
      threshold and `happy_margin` is clamped at 3 — so the endpoint argument
      that licenses "move ALL eligible workers" in `unit_upgrade` does not
      license "build all of them" here.  Whoever generalises it has to price
      each build separately, not multiply.
22. ~~**A government's level is unpriced on both sides.**  `features()` adds
    `meta[p.government][1]` into `tech_levels`, and neither `_card_yields` nor
    the swap diff emits it, so a government card is missing exactly the term
    [`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md) added to every other technology.  `gov_level`
    has the same hole.  Governments are already over-played, so this plausibly
    cuts the other way.~~
    **CLOSED 2026-07-31 by `docs/GOVERNMENT_PRICING.md`**, and the hole was
    deeper than this item knew.  `_card_yields` reads `techCost`, which is
    `null` on all eight government cards — they print `peacefulCost` and
    `revolutionCost` — and reads `production` / `effects`, where a government's
    civil actions, military actions and urban limit are *not*: those are
    top-level fields only `effects.compute` reads.  So **five of the seven
    takeable governments were unreachable on the live path**: Monarchy,
    Constitutional Monarchy and Republic priced at **exactly 0.000**, and
    Communism (−1.20) and Fundamentalism (−6.25) priced NEGATIVE, i.e. inside
    `row_pressure`'s `val <= 0.0` skip.  `weighted.gov_value` now prices the
    swap diff plus both level terms at `feature_marginal` and charges the
    cheaper of the two routes RULES_SPEC 8.2/8.3 offers, the revolution branch
    gated on the engine's own `_can_revolt`.
    **The last sentence above was wrong, and it is reported rather than tuned
    against**: takes went 1.05 → 1.63 per seat-game at 2p against a human 1.37,
    changes 0.98 → 1.50 against 1.11, and the three cards that priced at 0.000
    had been taken *exactly zero times* in 40 seat-games.  Seats ending the
    game still on Despotism fell from **10 of 40 to 1 of 40**.  Pricing a
    government correctly made the bot play MORE of them, and play a far more
    human spread of them.
23. **`happy_margin` is priced through its clamp linearly.**  `features()`
    computes `min(3, margin)` and `max(0, −margin)`; `board_yields._delta_triples`
    maps `Stats.happy` straight onto `happy_margin`, so a temple's happy face is
    worth the same to a player at margin 0 and one at margin 3.  Inherited from
    the leader swap diff and widened to the five urban types by
    `tech_upgrade`.  `weighted.feature_marginal` is now the one place that
    would learn it — the `strength_deficit`/`strength_lead` treatment, one
    feature over.
24. ~~**Action cards are now the largest single card-type deficit in the game**:
    2.72 per seat-game at 2p against a human 12.98 after the technology
    reprice (8.62 before).~~  **CLOSED 2026-07-30 by
    [`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md).**  The cause was not the row competition:
    **sixteen of the thirty-three action cards priced at exactly 0.000**, in
    three separate multiplied-by-zero mechanisms, and the per-card take rates
    split along exactly that line — every card priced through a live trained
    weight was at or above the human rate, every card priced at 0.000 was 3x to
    15x under it.  Takes 7.30 → 9.00 per seat-game at 2p against a human 12.98
    and 5.83 → 7.53 at 3p against 10.25; plays 4.85 → 5.82 and 3.88 → 4.62.
    **About a quarter of the gap, not all of it** — the residual is item 27.
    Three sub-items stay open, as items 25, 26 and 27.
25. **Nothing prices WHICH action a `freeCivilAction` orders.**  Rich Land ("a
    farm or a mine") and Urban Growth ("an urban building") are the same card to
    `card_potential` apart from their discount.  The honest price is the best
    legal free build's own delta, which `board_yields.tech_upgrade` can already
    compute for the urban and worker types — but `row_pressure` calls
    `card_potential` for every row card at every leaf, so it is a performance
    question as much as a modelling one.  The one bucket-(d) hole left in the
    type ([`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md#166-open-and-deliberately-not-done-here) §16.6).
26. **Engineering Genius is under-*played* for a reason that is not its price**:
    0.02 plays per seat-game at 2p against a human 1.33, and **0.00** at 3p.  It
    orders a wonder stage and is illegal without a wonder in progress; the bot
    completes 1.73 wonders a game at 2p and zero at 3p (§1.1).  Frugality is
    0.07 against 0.83 for the analogous reason — it orders a population
    increase.  **Re-measure both after the wonder hole is closed, not before.**
27. **The residual action-card gap is a question about the REST of the
    evaluator.**  After the fix the bot takes 9.00 a seat-game at 2p against a
    human 12.98.  `free_action_credit` ships at **0.0** and that is derived,
    not conservative: RB §3.11, playing a yellow card costs one civil action
    and grants one, so the action economy is a wash and a `freeCivilAction`
    card is worth its *discount*.  The A/B sweep is monotone in that credit
    (1.0 → 32.8%, 0.5 → 41.3%, 0.0 → 47.7% against a 50% null), so buying the
    remaining gap by turning it up is measurably wrong.  **Either humans
    over-take these, or two to four resources is worth more against a civil
    action than `w["civil_actions"]` = 2.0 says.**  The second is the live
    hypothesis and it is item 19's sweep in another guise: `civil_actions` is
    a coordinate no card price had ever charged for until this change.
    Unowned.
28. ~~**Three red cards can NEVER have an upgrade to ride, and item 21 is now
    load-bearing for them.**~~  **CLOSED 2026-07-31 by
    [`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md#149-nothing-priced-the-build-one-fresh-plan-2026-07-31) §14.9**, by pricing the plan item 21 named.
    The premise was right and is now a test rather than a claim
    (`test_they_really_have_no_upgrade_on_any_board` stands every other card
    of the type on the board and requires `_upgradable_onto` to stay empty).
    `board_yields.build_fresh` gives all three the plan they always had in the
    rules: on a fresh 2p board under `DEFAULT_WEIGHTS` Knights goes
    **−0.28 → +2.51**, Cannon +1.15 → +4.28 and Air Forces +0.07 → +4.51, so
    the one card that sat inside `row_pressure`'s `val <= 0.0` skip is out of
    it.  Air takes per seat-game went **0.015 → 0.075** and cavalry
    0.155 → 0.280 over 200 paired seat-games.  The three cards move and the
    class is no longer structurally capped; the remaining distance to the
    human 3.841 is item 21's two open halves and the bot's overall
    civil-take budget, not this.  Original text follows.
    Opened 2026-07-31 by the fix to item 20.  Knights,
    Cannon and Air Forces are the lowest card of their own type in the whole
    deck, so `_upgradable_onto` is empty for them on every board that will ever
    exist and their entire price is the develop half (`tech_levels`,
    `num_techs`, `best_unit`) against their science.  Under `DEFAULT_WEIGHTS`
    Knights lands at **−0.28** on a fresh 2p board, i.e. inside
    `row_pressure`'s `val <= 0.0` skip, and Air Forces at +0.07; at
    `tech_levels` 3.0 all three are positive, so it is a weights judgement and
    not a sign lock (`tests/test_unit_pricing.py` asserts exactly that
    distinction, and all four red *types* still price positive, so no class is
    invisible).  The real answer is item 21 — the only route to a first
    cavalry, artillery or air unit is to BUILD one fresh, which needs a free
    worker, a military action and resources, and nothing prices any of it.
    Until then the bot cannot open a second arm of its army at all, which is
    also the most likely reason air takes measure **0.00** per seat-game
    against a human 0.65.
29. **Winning a colony auction is now priced without the colony**, and this one
    is the direct consequence of closing item 16, opened by the same lane on
    2026-07-31 rather than left for somebody to rediscover.  `deferred_credit`
    has an `auction` branch that credits the high bidder with a share of the
    territory's effects, precisely because a `bid` mutates only the auction
    dict and would otherwise score exactly like `bid_pass`.  Since the
    sacrifice became a decision, the *winning* `bid_pass` no longer completes
    the colonization inside `apply`: when the force is a real choice it leaves
    a `colonize` pending instead, so the one-ply trial state shows **neither**
    the sacrifice nor the colony, and `deferred_credit`'s `auction` branch has
    already stopped matching (the top of the stack is `colonize`, not
    `auction`).  The fix is identified and small — a `colonize` sibling to that
    branch, at share 1.0 rather than `1/(1+rivals)` because the winner is
    already decided — and it is deliberately NOT in the item-16 change so that
    lane's digest moves had one cause.  Note it helps `WeightedBot`,
    `QuiescentBot` and `PlanBot` only; `GreedyBot` has no `deferred_credit` and
    never did.  Observed effect where it was traced: `2p greedy seed=0` in the
    narrow fingerprint bids 2 instead of 1 for the same territory and
    sacrifices the same single strength-2 unit, so the boards and the final
    scores are **identical** and only the log line moves.  That is one case,
    not a bound.
30. **`blue_free` and `corruption_loss` are unpriced on EVERY technology
    plan.**  Opened 2026-07-31 by the fix to items 21/28, by a test that
    plays the plan out and diffs `weighted.features()` rather than by reading
    the code.  Neither is a `Stats` field: `features()` computes them from
    `effects.blue_available`, which is `p.blue_total` minus the blue tokens
    your food and resource banks are standing on — and a higher-level farm or
    mine holds MORE resources per token (`effects._denoms`), so staffing one
    frees blue tokens and cuts `economy.corruption`.  `_delta_triples` cannot
    see either, so **the existing upgrade path has the identical hole**:
    upgrading Bronze → Iron moves `blue_free` 8 → 13 and `corruption_loss`
    2 → 0 and both are priced at nothing (weights 0.15 and −0.9).  It is the
    `tech_levels` shape one more time — a gain the evaluator reads and no card
    price charges for, which is also item 19(b)'s sweep.  Deliberately not
    fixed in the build-fresh lane so that lane's digest moves had one cause.
    Pinned by `tests/test_build_fresh.py:TestThePriceIsWhatFeaturesActuallyMove
    .UNPRICED`, which is a ratchet: a *new* unpriced channel fails it.
31. **Why does the build-fresh plan LOSE 5.5pp when it is turned on?**
    Opened 2026-07-31 by the lane that closed items 21 and 28, and it is the
    unfinished half of them.  `build_fresh_credit` = 1.0 is a measured
    regression on two independent vectors (44.1% and 44.5% against a 50%
    null, 400 paired games each), so it ships at 0.0.  The price is
    structurally what `evaluate` pays — every triple is pinned against
    `features()` itself — so *something it charges or fails to charge is
    wrong*, and the two candidates that survived the champion control are:
    (a) `_hand_total` SUMS technology prices over the civil hand, so every
    buildable technology in hand credits the same single free worker (the
    upgrade plan has the same shape, but a build's worker is a scarcer
    resource than a worker already on a card); (b) nothing tests that the
    plan is still legal by the time it would be played — `p.workers_free` is
    0 at 68% of decisions, so a card taken because a build was priced may
    face an empty pool two moves later.  Neither is measured.  A third
    possibility nobody has excluded is that the action a build costs, which
    is deliberately uncharged for symmetry with the upgrade plan, is worth
    much more than `ma_left`/`ca_left` = 0.05 — which is item 27's live
    hypothesis in another guise.  Note also that the credit is a SWITCH and
    not a level (0.5 and 1.0 measure the same), so a league that scatters onto
    it steps straight onto the loss; anyone re-opening this should expect to
    fix the price rather than to find a good value for the credit.
32. **A per-card price cannot close the red gap on its own, and the number
    that says so is the civil-take budget.**  After items 21/28 the bot takes
    **19.2** civil cards a seat-game at 2p against a human **34.3**, and red
    is 0.845 against 3.841.  Red's *share* of the takes is 4.4% against a
    human 11.2%, so both the numerator and the denominator are short: even at
    the human take budget and the current share, red would reach only ~1.5.
    Item 27 is the same observation from the action-card side and reaches the
    same live hypothesis — that a civil action is worth less than
    `w["civil_actions"]` = 2.0 says, so the bot ends turns it should be
    spending.  Recorded as one item so the next lane does not re-derive it
    from a third colour.

29. **Wonders are the one civil type that never got the
    `*_board_credit`-defaults-to-1.0 treatment, and it is the whole of the
    "expensive wonders are never built" story that is still a hole.**  Opened
    2026-07-31 by the rate-horizon lane, which went looking for an unpriced
    endgame-culture channel and found the channel already written.
    `effects.wonder_completion_culture` is the single implementation of the
    four Age III wonders' one-shot `onBuildCulture` bomb;
    `board_yields._on_build_culture` prices it by calling *that same function*,
    and `tests/test_card_pricing.py:TestOneImplementation` fails if they
    diverge.  It is simply **unreachable on two of the three league arms**:
    it is gated on `card_board_credit` (default **0.0**; 0.361 on the live 2p
    champion, **0.0** on 3p and 4p) and surfaces through `wonder_potential`
    (default **0.0**; 0.115 at 2p, **0.0** at 3p/4p).  Technologies, governments
    and action cards each got their own credit defaulting to 1.0 precisely so
    the fix would be live on all three arms at once (`tech_board_credit`,
    `gov_board_credit`, `action_board_credit` — see the comment block in
    `card_potential`); wonders were left behind the 0.0 shared credit.  The
    completion rates track it exactly: **1.53 / 0.24 / 0.16** per seat-game at
    2p / 3p / 4p.  The change is a `wonder_board_credit` on the same pattern,
    but it is **not free** — item 1.1 measured `wonder_potential` 0.125 as
    behaviourally large (completions 0.051 → 0.408) with a strength null taken
    against a broken yardstick, and item 1.2 records that abandoned programmes
    got absolutely worse.  Wants its own lane and its own A/B, not a
    drive-by.
30. **The rate horizon is applied to the rate features and to
    `feature_marginal`, and NOT to the static `_sum_yields` table** — which
    also still carries no phase blend.  Opened 2026-07-31 by
    [`docs/RATE_HORIZON.md`](RATE_HORIZON.md) §7.  This is the same divergence
    [`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md) found and closed for
    technologies, governments and action cards by routing them through
    `feature_marginal`; the classes still on the static path are the ones with
    no board handler, and they now diverge from `evaluate` in two ways rather
    than one.  Neither widened nor narrowed by that change.  The fix is the
    same fix: give the remaining classes a board handler, or make `_sum_yields`
    take a state.
31. **Only the four OWN rates were made horizon-aware; the argument applies
    more widely and was deliberately not followed.**  Two sub-parts.
    (a) `rival_culture_rate` and `rival_science_rate` are per-turn rates too and
    the argument covers them, but including them made two coordinates that
    `tests/test_coverage_tools.py:TestInertFeatures` declares inert across a
    candidate set start varying — a behavioural change nobody asked for, riding
    in under a different change — so they are out.  (b) The wider version:  An investment that costs now and pays a
    rate later is worth taking only if its payback fits inside the remaining
    horizon, and the cost side is priced flat: `wonder_remaining` is a pure cost
    weight (−0.155 on the live 2p champion) with no horizon-discounted return
    beside it, `science` and `resource_stock` costs are flat, and
    `wonder_turns_to_finish` / `wonder_overrun` express payback only for
    wonders and only as a penalty.  [`docs/RATE_HORIZON.md`](RATE_HORIZON.md)
    ships the narrowest version that tests the hypothesis — the multiplier on
    the six rate channels that already exist — because the wider version is a
    different and much larger change.  Unowned.

Deliberately **not** open, recorded so nobody reopens them: wars and aggressions
are 1-ply artefacts repaired by search ([`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md) tier B) — the
standing problem is now over-declaration, not blindness; Military Bonus cards
have no move handler by design; pacts are absent at 2p by rule; flooring
`card_potential` at 0 was tried and rejected (it would make the unit lane's own
demonstration unmeasurable and is a no-op for `row_pressure`'s `<= 0` skip).

---

## 3. Evaluator information gaps

*From [`docs/INFORMATION_AUDIT.md`](INFORMATION_AUDIT.md), [`docs/EVENT_SEEDING.md`](EVENT_SEEDING.md),
[`docs/BOT_ARCHITECTURE.md`](BOT_ARCHITECTURE.md), [`docs/MODEL_CONSTANTS.md`](MODEL_CONSTANTS.md).*

### 3.0 Fitted constants still standing after the 2026-07-30 sweep

*From [`docs/MODEL_CONSTANTS.md`](MODEL_CONSTANTS.md).  Three were replaced; these are what is left,
and each is now labelled as a prior in the source and pinned by
`tests/test_model_constants.py`.*

* **`PACT_OFFER_CREDIT = 0.5` is a fitted prior and could not cheaply become a
  weight.**  It is spent inside `_pending_terms`, which is called from
  `features()` — and `features()` has no weight vector.  Plumbing one in means
  either a second signature on the hot path or a module global, which is what
  it already is.  Deliberately left labelled rather than half-plumbed.  If a
  lane ever threads `w` into `features()` for another reason, convert this at
  the same time.  Nothing has ever measured whether 0.5 is right; the only
  evidence is [`docs/COMBAT_AUDIT.md`](COMBAT_AUDIT.md) fix #2, which establishes that it must
  be **non-zero** (at 0.0 `offer_pact` is strictly dominated by `pol_pass`) and
  says nothing about the value.
* **`_TAKE_PRIOR = {2: 0.30, 3: 0.35, 4: 0.40}` is fitted** and is the last
  guess in the horizon.  It only bites in Age A and the first rounds of Age I
  (shrunk away with weight `_TAKE_PRIOR_W = 4.0` replenishes), and it is the
  Age A column in [`docs/MODEL_CONSTANTS.md`](MODEL_CONSTANTS.md#14-error-against-ground-truth-under-two-card-appetites) §1.4 where the new estimator is
  worst.  The 3p and 4p entries are less well measured than the 2p one:
  `tools/deal_rate.py --players 3/4` has been run once, at 10 games.
* **`rival_take_share` ships at its 0.5 prior and has never been climbed.**  It
  cannot move anything until `row_bargain_forgone` is non-zero, which is true
  only on the three archived champions and on no live arm.
* **`FREE_POP_UTIL = 0.17` is still a prior, and the part that stays one is
  the right part.**  Whether *this* player would have bought a population
  increase *this* turn is a policy question, not a board fact.  Both branches
  it splits are now priced and calibrated to 0.99x / 0.97x of a replayed
  ground truth, but the calibration is 2p only, ~317 player-turns per vector,
  and has never been checked at 3p or 4p.  Note also that the whole handler is
  **unreachable under `DEFAULT_WEIGHTS`** (`card_board_credit` and
  `card_board_wonder` are both 0.0), so nothing in the live league sees it yet
  — the same priced-but-inert shape `tests/test_play_rate.py` exists to catch,
  here by design rather than by accident.
* **The "is there a job for the worker" board query was NOT built.**  The
  happiness gate was, because `economy.happy_required` is a step function and
  the answer is arithmetic.  Whether a new worker has anywhere useful to go
  needs urban-limit and per-technology build-cost queries, which is the
  expensive part of `card_potential`, and it was left rather than guessed.
* **`tools/deal_rate.py`'s `hungry` policy does not actually take more cards
  than the defaults do** (both 1.88/round at 2p), so §1.4's robustness claim
  rests on a `shy`-vs-`default` contrast rather than a three-point sweep.  A
  genuinely hungrier lever would strengthen it.
* ~~**`hillclimb_pool.CULTURE_CENTRE = 100.0` / `CULTURE_SCALE = 120.0` flatten
  the objective at 4p.**~~  **CLOSED 2026-07-30** — both constants are DELETED,
  not re-fitted: the objective no longer scores absolute own culture at all
  ([`docs/LEAGUE_OBJECTIVE.md`](LEAGUE_OBJECTIVE.md)).  The 5.8x-mispricing finding drove the
  replacement scale to be **per player count**, which is the durable half of
  this item.  But the follow-on inference did **not** survive measurement: the
  human 4p final-SCORE tail (p90 298) does not imply a wide 4p LEAD
  distribution.  Measured sd of the per-seat culture lead over the same corpus
  is **2p 57.9 > 4p 53.6 > 3p 46.5** — not ordered by player count — so 4p gets
  a *smaller* scale than 2p (135 vs 145).  Score level and gap size are
  different quantities.  Whether the 4p arm nevertheless flattens is still
  open; the evidence to look at is the post-relaunch per-game lead
  distribution, not the final-score tail.

* **GAP 4 (politics / event deck)** — partially opened by `event_scoring_margin`
  ([`docs/EVENT_SEEDING.md`](EVENT_SEEDING.md)), which ships at weight 0.0 by design.  Its effect
  under `plan:width=2` (the search the league actually trains) is **unmeasured**;
  so is 3p transfer; so is whether it should feed the neural encoder's existing
  `seeded_n` / `seeded_lv`.
* ~~**GAP 5 (no civil-discard record)** — UNSTARTED.  `engine/game.py:117-118`
  destroys swept civil cards silently and no `civil_discard` field exists, so
  card counting is impossible.~~  **CLOSED 2026-07-31**, in two halves.  The
  RECORD landed in `c2a4246`: `state.civil_discard`, age → [names], shaped to
  mirror `state.discarded_military`, written in `_replenish` where the sweep
  used to write `None` over the slot.  The EXPOSURE was gated on the legality
  question two bullets down and landed once that was settled — the encoder's
  `_discard_block` now reads BOTH piles, per age (the share of that age's
  civil deck already swept, and the size of that age's military discard), so
  `unseen(age) = deck − row − hands − tableaux − discard` is learnable.
  `ENCODING_DIM` 1897 → 1907, which stales any existing neural checkpoint —
  loudly, since `neural_net.load` rebuilds from the checkpoint's own `in_dim`.
  Still open, deliberately: **no `weighted.py` evaluator term prices it.**
  That is a pricing job, not this lane's, and `docs/HAZARDS.md` is explicit
  that a weight the climb never has to pay for is unconstrained and will
  drift.  Recording plus encoding is what makes the pricing *possible*; it is
  not the pricing.
* **GAP 6 (military hand identity)** — UNSTARTED, and a **loaded gun**: the
  moment military-card identity is priced, the `end_turn` military draw becomes a
  live unmasked information leak.  It must ship together with rival-hand
  re-dealing in `determinize`.  `plan.determinize` still leaves rival military
  hands at their true contents; the correct fix (pool every rival's
  `hand_military` back into the deck and re-deal to the same counts) is
  identified and not implemented.
* **Is `rival_hand_potential` worth anything at 3p/4p?**  Live 3p fitted −0.020,
  live 4p +1.329 — opposite signs, no head-to-head ablation ever run.
* **Do any of the shipped GAP1/2/3 terms make the bot win more**, as opposed to
  merely read more?  Never ablation-tested.  The one exception (de-leaking the
  row terms was strength-neutral) says nothing about whether the row terms are
  worth their weights.
* ~~**Is the military discard pile legible in the physical game?**  Unverified from
  the rules text.  Treat `state.discarded_military` as hidden until settled.~~
  **CLOSED 2026-07-31 by a ruling from Paul**, verbatim: *"Card counting is
  legal.  All public info can be used."*  Both discard piles are public and
  both are now encoded.  The principle is written down as project law in
  `docs/INFORMATION_AUDIT.md` §0c and inside
  `engine/bots/neural_encode.py`'s docstring, which is the source of truth for
  information legality; the test for any future field is *could a human at the
  table with the physical 2015 base game see it?*  **It does not reach GAP 6.**
  A hand is not public: rival military-hand contents stay hidden, and the
  loaded-gun warning below stands unchanged.
* A known, deliberately-left-open hole in the row-leak fix: the forward-only
  cursor is an upper bound, not an identity — it cannot distinguish a genuinely
  new card from one reusing the name of a card a *rival took* (as opposed to one
  swept), because that needs provenance on row slots.  Pinned by a test written
  to fail if someone "fixes" it silently.
* Deliberately unpriced, with reasons, so nobody reprices them by reflex: 17
  rank-addressed Age I/II events (the ranking outcome is unknowable at plant
  time), 23 symmetric `allPlayers` events (near-wash or board-scaled), the 10
  pacts (unpriceable at 2p where they do not exist; priced at 3p/4p via
  `deferred_credit`, but whether that pricing is any good is open and needs a
  3p/4p experiment).

---

## 4. Training, league and objective

*From [`docs/LEAGUE_OBJECTIVE.md`](LEAGUE_OBJECTIVE.md), [`LEAGUE_POOL.md`](LEAGUE_POOL.md), [`LEAGUE_TRAINING.md`](LEAGUE_TRAINING.md),
[`TRAINING_RUN.md`](TRAINING_RUN.md), [`PROXY_GUARDRAIL.md`](PROXY_GUARDRAIL.md), [`FOURP_GAP.md`](FOURP_GAP.md), [`HUMAN_BOTS.md`](HUMAN_BOTS.md),
[`BEHAVIOUR_CLONE.md`](BEHAVIOUR_CLONE.md), [`CULTURE_GAP.md`](CULTURE_GAP.md).*

*Rewritten 2026-07-30: the objective is now the culture LEAD OVER THE BEST
OPPONENT, centred on zero because zero is the win/lose boundary.
`CULTURE_CENTRE` is deleted, not re-fitted.  See [`docs/LEAGUE_OBJECTIVE.md`](LEAGUE_OBJECTIVE.md).*

* **THE ONE TO WATCH AFTER RELAUNCH — war/aggression rate under a differential
  objective.**  The new objective pays for taking culture off the leader at
  twice the rate of producing it, which is *correct* for winning but feeds a
  pathology the bot already has: it declares wars at **6.6-7.9× the human rate
  at 3p/4p** ([`docs/AGGRESSION_RATE.md`](AGGRESSION_RATE.md)).  Offline estimate on archived
  evaluations: **26-33% of culture-differential gains were "pure suppression"**
  (own culture flat or down while the lead rose) — rows the old objective did
  not reward and the new one rewards in full.  Deliberately **not** re-measured
  with fresh game batches; post-relaunch logs measure it under real training,
  which is the condition that matters.  **Run `tools/aggression_census.py`
  against [`docs/AGGRESSION_RATE.md`](AGGRESSION_RATE.md)'s human rate on the first arm output.  If
  it exceeds 6.6-7.9× human, the lane to open is the EVALUATOR's war pricing,
  not the objective.**  Also watch the `cult` column against the `lead` column
  in the per-opponent report: own culture flat while the lead rises, sustained,
  is suppression training, and `cult` should be heading toward the human ~159.5
  at 2p ([`docs/HUMAN_BASELINE.md`](HUMAN_BASELINE.md)), not away from it.
* **Nothing shows the new objective TRAINS a better bot** — only that it *ranks*
  3,802 archived decisions in better agreement with win rate (+0.939/+0.933/
  +0.910 against +0.850/+0.861/+0.824) and that the machinery runs.
* **The per-game dispersion of the lead is not measured.**  It is a differential
  of two noisy quantities and is expected to be noisier per game than own
  culture was (0.419 for a margin against 0.218 for own culture, measured under
  the old objective), and margin-over-BEST is noisier still than
  margin-over-mean at 3p/4p.  That widens the accept CI.  **If the arms accept
  noticeably less often after relaunch, this is the first thing to check and
  `--block` is the dial.**
* **`LEAD_SCALE` is per player count (2p 145, 3p 115, 4p 135), derived as 2.5x
  the sd of the per-seat culture lead over the 1,011-game HUMAN BGO corpus**
  (`python3 tools/objective_relog.py --derive-scale`).  Deriving from an
  external, fixed corpus is what stops it going stale the way CULTURE_CENTRE
  did — but it means the scale is calibrated to the distribution we are aiming
  at, not the one we are in.  Two live caveats: the 3p/4p corpus slices are
  thin (133 and 186 games), and the 2.5 multiplier is itself an inherited
  judgement, not independently justified.
* **The objective's throughput did not improve** ([`LEAGUE_OBJECTIVE.md`](LEAGUE_OBJECTIVE.md#6b-acceptreject-throughput--essentially-unchanged-this-is-a-null) §6b is a
  null): the "rejected a better-on-winning candidate" rate stays at 16-20% and
  4p gets slightly worse.  The conservatism is set by the confidence bound and
  the block size, not by the objective, so the lever is `--block`/`z`.
* The 3p/4p half of that re-scoring is a **proxy** — the archives never recorded
  the best opponent's culture, so margin-over-mean stands in for
  lead-over-best.  The 2p column is exact.  The best-vs-mean decision itself is
  argued and unit-tested, never measured on logs.
* **`--objective own` and `--objective margin` are removed**, not deprecated
  (the arms relaunch clean).  Historical reproduction is `git checkout 8b972ef`.
* **[`LEAGUE_POOL.md`](LEAGUE_POOL.md)'s saturation thresholds (0.70 / 0.95 / 0.15) are eyeballed,
  not derived.**  The defensible part is the direction and the self-correction,
  not the knee.  Escape hatch is `--saturation 0,1,1`.
* Win rate is not the metric the league accepts on (`blend` = culture lead +
  win-share tiebreak), so an opponent can be saturated on win share while still
  discriminating on the lead.  Using the check's own culture/lead columns for
  the saturation rule instead was considered and not done.
* **Post-training exploitability of the `hum:*` archetypes was never measured** —
  only pre-training.  The take ceiling is structurally ~28 for every human bot
  against a human 33-40; four tuning knobs failed to move it and the leading
  suspect (take-backs, an 8%-of-takes BGO affordance bots do not have) is
  unproven.
* **Behaviour cloning's named follow-up has not been run:** a value regression on
  the same corpus, fitting *game outcome* rather than move choice.  Move-choice
  fitting provably cannot recover a weight on a feature that does not vary
  between a decision's candidates — culture stock never varies within a turn's
  choice set — so no amount of human move data teaches a cloned vector that
  culture wins the game.  Colony bidding at 0.00 bids/player is a fixable data
  artefact (auctions dirty their turn, so almost no bid reaches the training
  data), not a modelling failure.
* **[`FOURP_GAP.md`](FOURP_GAP.md)'s best-supported mechanism is unconfirmed:** the 4p accept
  gate is 2.8x less sensitive per generation with nothing compensating
  (`--block 12 --subset 4` unchanged across player counts), consistent with the
  4p arm having the *highest* accept rate (21.4%) and the weakest vector.
  Labelled a hypothesis, never measured directly.  Refuted along the way, so
  nobody re-tries it: reverting the 4p-unique negative `culture_early` effective
  coefficient in isolation does nothing (z = −1.5).
* **The culture matchup was never re-measured after the fixes.**  No post-fix win
  rate against `var:culture` exists.  What closed was the investigation's
  premise, not the gap — see §5.
* Unlanded proposals from [`CULTURE_GAP.md`](CULTURE_GAP.md): decouple phase-multiplier mutation
  step size from the base weight's magnitude; add a restoring force to the
  geometric random walk (log-prior toward defaults, or bound weights to a sane
  multiple of default); run a `--ladder 5,20,35.574,60` sweep on `culture_rate`
  to check whether it is near-optimal or riding a ratchet — explicitly "the
  single most useful next experiment on this axis", queued then stood down.
* The war/aggression representation hole is real (`deferred_credit()` credits
  `pact_offer` / `auction` but never `defense`) and **still unclosed** — but an
  oracle that always attacks the culture leader when legal gained nothing at
  n=48.  Do not sell closing it as "the fix"; validate at n>=200 first.

---

## 5. Search, neural and architecture

*From [`docs/BOT_ARCHITECTURE.md`](BOT_ARCHITECTURE.md), [`DEEPER_SEARCH.md`](DEEPER_SEARCH.md), [`NEURAL_SEARCH_LOOP.md`](NEURAL_SEARCH_LOOP.md),
[`NEURAL_LOOP_NULL.md`](NEURAL_LOOP_NULL.md), [`NEURAL_EVAL.md`](NEURAL_EVAL.md), [`PLAN_WAR_LOOKAHEAD.md`](PLAN_WAR_LOOKAHEAD.md),
[`TRANSFER_TEST.md`](TRANSFER_TEST.md).*

* **The neural line is not abandoned.**  v2 (`NeuralPlanBot`'s beam as the
  improvement operator) has 4 promotions in 21 iterations against v1's 0 in 74,
  and has **plateaued at rough parity with the linear champion since iteration
  7** — the self-play gate blocks every time (0.476-0.5685, CI lower bound never
  clears 0.5) while the anchor gate passes every time.  Parity is not victory.
* **`NeuralPlanBot` still copies** — it has its own `_beam` / `war_value` and
  reads no `USE_JOURNAL`.  Deliberately unconverted because nobody trains it on
  the Mac Mini.
* **A war declared in the last round never resolves in reality but
  `PlanBot._score` still prices it as if it will.**  Deliberately unguarded so
  [`docs/PLAN_WAR_LOOKAHEAD.md`](PLAN_WAR_LOOKAHEAD.md)'s measurements isolate "PlanBot prices wars"
  cleanly.  Flagged as the obvious next experiment and not free of risk.
* `ctx` (rival aggregates) is inherited from the search root and not recomputed
  after a war resolves inside the lookahead — a known imprecision left in place
  to avoid confounding that measurement.
* **`interact.settle_war_spoils` always takes the remainder as science.**  Sound
  as a lower bound, but a permanent one-sided bias: every bot that prices a
  declared War over Technology sees the *floor* of the card, never its ceiling,
  and will keep under-declaring it exactly where the choice matters most.
  Anyone who measures the choice and finds nothing has measured this lower bound,
  not the choice.
* [`TRANSFER_TEST.md`](TRANSFER_TEST.md)'s option (a) "train under `plan:width=1`" is **not**
  retired by the war-lookahead fix (its option (c), "score the gate on own
  culture", was adopted 2026-07-27 and then reversed 2026-07-30 — see
  [`docs/LEAGUE_OBJECTIVE.md`](LEAGUE_OBJECTIVE.md#7-the-risk-this-objective-pays-for-dragging-opponents-down) §7).  The
  lookahead removed the *inversion* but not the miscalibration: the proxy still
  says Q is +36.3 better where the real search says P and Q are statistically
  indistinguishable.
* **[`PLAN_WAR_LOOKAHEAD.md`](PLAN_WAR_LOOKAHEAD.md) is 2p-only and `plan:width=8`-only**; `width=1` is
  untested and `book` is the only opponent.

---

## 6. The 2026-07-26 lost-work dig

*Migrated from `docs/ARCHAEOLOGY.md`, deleted 2026-07-30.  Everything here is a
**(snapshot)** taken at `8e751cb` and must be re-checked against current code
before being acted on — item 12g below had already gone stale within 24 hours.
The single most important entry is #22.*

1. Branch `has-unit` (`c96b653`) holds a clean 9-line fix adding a binary
   `has_unit` feature for the rules cliff that a 0-unit player is excluded from
   colony auctions (§11.3).  Parked pending a 3p/4p no-harm A/B that was never
   run; `tools/guard_ab.py` now exists to do it cheaply.  Must be tested against
   a **post-horizon** champion.
2. `tools/gate.sh`'s WeightedBot digests were stale, so the gate failed on a
   clean master with no real regression.  A 200-generation seat census found the
   live pool is ~69% WeightedBot / ~27% Book+Variant / ~2% GreedyBot / 0%
   QuiescentBot — the broken digest pair covered nearly everything the trainer
   runs.  (Digests have moved several times since; see [`HAZARDS.md`](HAZARDS.md).)
3. Three tools (`tools/quiesce_bench.py`, `tools/no_credit_check.py`,
   `tools/behaviour_counts.py`) silently defaulted to the horizon-invalidated
   `experiments/champion_4p.json` and printed plausible numbers for a crippled
   vector without erroring.  `tools/culture_probe.py` does it right (defaults to
   the live `league_state/` path) and is the pattern to copy.
4. The trainer's own per-weight ablation ledger (`--ablate-every`, n=72/weight)
   has accumulated data nobody has read: roughly two thirds of measured weights
   show zero measurable effect on removal.  At 2p the top two load-bearing
   weights are `hand_potential` and `end_turn_bias`.  `auction_committed` has two
   directly conflicting measurements (n=72 load-bearing, n=24 harmful), both
   below the n=200 bar, conflict unresolved.  `culture_rate_early/_late` are not
   yet covered by the 2p ablation cursor.
5. "Trained weights compensate for a structural flaw, so fix the flaw" has
   already been measured to backfire five separate ways — see [`HAZARDS.md`](HAZARDS.md).
6. A measured finding contradicts the weight guard's own premise: negating
   `science` or `culture` in `DEFAULT_WEIGHTS` measurably *helped* at 3p/4p, yet
   the guard clamps exactly that region.  Flagged "worth revisiting", never
   revisited, still running.  Counter-evidence is also real (`science = −6.089`
   collapsed 4p play), so the relationship is non-monotonic rather than simply
   wrong.
7. [`experiments/PROGRESS.md`](../experiments/PROGRESS.md) makes advice-shaped claims purely from
   weight-drift-from-default — the invalid inference [`docs/OPENING_AUDIT.md`](OPENING_AUDIT.md)
   demolished using one of that table's own entries.  Also mechanically stale
   (78 vs actual weight count, obsolete horizon formula, pre-league framing).
   Never corrected.
8. `experiments/baselines.jsonl` still carries no timestamp, generation or seed
   on any row — the exact mechanism by which a stale number became a published
   claim in [`docs/HEURISTICS.md`](HEURISTICS.md).  Fix never applied.  Do not quote it; re-run
   `experiments/evaluate.py`.
9. `analysis/opening_order.py`'s crash was fixed but its `card_type()` still
   returns `"?"` for every card (plain-dict cards, not objects) — now *worse*
   than before, because it produces a plausible empty table instead of crashing.
10. Three finished programmes have an explicit "re-run this" that was never
    executed: the post-`7d40f53` re-run of the BookBot-beats-champion benchmark
    (frozen weights already exist for it); two of three pre-retraining checks
    from [`docs/WASTED_ACTIONS.md`](WASTED_ACTIONS.md#11-what-to-do-before-the-retraining-run) §11; and [`docs/BOT_ROSTER.md`](BOT_ROSTER.md)'s
    reverse-direction 3p/4p cells plus `experiments/roster_behaviour.py`
    (written, committed, never executed — would settle the 4p colony-auction
    disagreement as a side effect).
11. A verified, unfixed rules bug: the pact-legality gate
    (`engine/actions.py:257-259`) checks a **live** player count where the rule is
    **setup-time**, so a mid-game resignation in a 3p game can silently make pacts
    illegal for the survivors.  Low impact (~0.07 resignations/game), zero test
    coverage.  Found independently by [`docs/COMBAT_AUDIT.md`](COMBAT_AUDIT.md) (Bug 2) and
    [`docs/COMBAT_AUDIT.md`](COMBAT_AUDIT.md).
12. Small unactioned follow-ups: re-test `wonder_remaining` in isolation;
    re-check the ~1.2-abandoned-wonders/game waste statistic; ablate the 4p
    `hand_military = 0.908` weight (it makes the champion opt out of events,
    territories, aggressions and pacts at once); expose the colonization
    sacrifice as a real decision instead of a hardcoded greedy pick; add features
    for the `defend` / `choose` move kinds (the pact-accept branch has never been
    checked for a systematic-refusal bug); raise `--check-games` from 48 to 100+.
    *(12g, the phase-multiplier clamp asymmetry, was landed — [`CULTURE_GAP.md`](CULTURE_GAP.md)
    §21.)*
13. Stale weight/feature counts (82 actual vs a quoted 78 / ~57) trace to one
    un-updated docstring in `weighted.py`, propagated into five documents.
14. Three benchmark shell scripts hardcode paths to checkouts that no longer
    exist and have zero inbound references — safe to delete.  Adjacent trap:
    `tools/bench_interp.py --kinds weighted` silently benchmarks GreedyBot; use
    `engine/perf_check.py --kinds weighted`.
15. Git history is clean — nothing recoverable in deleted files, dangling
    commits or stashes.
16. [`docs/HEURISTICS.md`](HEURISTICS.md) describes three champions that no longer exist, and even
    the surviving 2p champion's live weights contradict its headline advice: 13
    weights clamped to exactly 0.0 (including `civil_actions`, `ca_left`,
    `uprising`, `leader`, each contradicting a specific headline claim) while the
    largest live term is `end_turn_bias = −14.44`, a pure artefact correction.
    The cumulative cost of the 13 zero-clamped terms has never been measured.
17. PyPy's own re-test trigger ("once bots stop copying a whole GameState per
    move") has been met by the journal rewrite.  *(Since acted on — see
    [`docs/PYPY.md`](PYPY.md), and the correction at the top of that file.)*
18. `experiments/summarize.py`'s hand-enumerated `GROUPS` table is missing the
    four most recently added features (`pact_blocks_attack`, `auction_committed`,
    `auction_bid`, `hand_potential`), silently binning them as `"?"` in every
    published weight-breakdown table in the repo.  Four-string fix, never
    applied.
19. `experiments/behaviour.py` calls an undefined `all_snaps_iter` and has been
    broken standalone across at least three sessions, worked around each time by
    `analysis/behaviour_run.py` rather than fixed at the source.  Gates every
    behaviour re-harvest.  Three-line fix.
20. The project has no external anchor of any kind, and all three designed
    remedies (open-source TTA research, the human-in-the-loop harness, BGO
    scraping at scale) are essentially unimplemented.  *(Partly overtaken: the
    BGO corpus landed — [`docs/BGO_CORPUS.md`](BGO_CORPUS.md), [`docs/HUMAN_BASELINE.md`](HUMAN_BASELINE.md) — and the
    harness is built but has never been run for a measurement.)*
21. [`engine/PROGRESS.md`](../engine/PROGRESS.md) still lists as open a question (action-card gain
    ordering) that is resolved and pinned by tests.  Also, two differently
    computed `hand_potential` win rates (69.6% and 72.5%) are both cited as "the"
    measurement; they are two different implementations and neither is wrong.
22. **The structural fact that explains most of the other 21.**  The live trainer
    accepts generations on only **n=48 games (2p/4p) or n=144 (3p)** at a
    one-sided 90% threshold (`--accept-z 1.2816`) — mechanically a
    false-acceptance machine over hundreds of generations, and the named root
    cause of this repo's recurring "confident result that later reverses"
    pattern.  **Any number in this repo below n=200 should be treated as
    provisional.**  The regression tripwire's own sample (48 games) is flagged in
    its own source doc as too small and was never raised.

---

## 7. Engine and rules items still open

* **Two live bugs found and deliberately not fixed** ([`docs/EVENT_SEEDING.md`](EVENT_SEEDING.md)):
  `interact.py`'s `_c_pact_offer` does `owner.pacts = [{...}]` — assignment, not
  append — so accepting a pact silently destroys every other pact the owner
  held, reachable only at 3p/4p.  And `book.py`'s pact-offer response reads
  `pend["ctx"]["from"]` where `_h_offer_pact` writes `owner`/`name`/`a`/`b`, so
  `partner` is always `None` and BookBot's "refuse if the partner leads by more
  than 5 culture" rule never fires.
* **COMBAT_AUDIT GAP 2, open:** Plunder's food/resource split is chosen greedily
  by the engine rather than by the attacker as FAQ p.7 specifies.  Totals and cap
  are correct; only the mix is not a real decision.  Low impact.
* **COMBAT_AUDIT GAP 3, open and deliberately not fixed:** Annex and Infiltrate
  can be played against a target that does not qualify (no colony, no leader or
  unfinished wonder); the aggression resolves "successfully" and does nothing.
  An engine-vs-own-data inconsistency rather than a certain rules violation —
  changing `legal_moves` without a firm citation was judged worse than the rare
  void play.
* **Winston Churchill's military science/resource option is unrestricted** where
  it should be usable only for military technology and units.  A *play* bug, left
  to the pricing lane, never scoring-tested.
* **`Impact of Happiness` (70.8% agreement) and `Impact of Strength` (64.3%)
  remain open** against the human corpus — the journal never prints happy faces
  and the replayer models no tactics or armies, so neither is testable from the
  corpus as it stands.  `Impact of Population`'s residual (73/88, 83.0%) is
  concentrated entirely on rows where the engine computes discontent > 0 and is
  attributed to the same uncertainty; the alternative reading (do not subtract
  discontent at all) was tested and fits worse.
* **`state.scoring_events` (`state.py:157`) is declared and never read or
  written** — a dead field that is also a permanently-zero neural-net input.
  `PlayerState.destroyed_wonders` is read by the take surcharge but never
  incremented.  `urbanLimitCategory`, `scoringEvent` and top-level `target` are
  dead or duplicate fields nothing reads.
* Card-data provenance items are all closed — see the appendix of
  [`docs/RULES_SPEC.md`](RULES_SPEC.md).

---

## 8. Measurement and infrastructure

* **[`docs/BGO_CORPUS.md`](BGO_CORPUS.md)'s `## Results` section is an empty placeholder.**  Read
  the real yield from `sources/bgo/index.tsv`, not from that document.
* The 40-name expansion-exclusion list used by the BGO scraper **has never been
  positively confirmed against a known expansion-enabled game** — no such game
  was found in-sample to test against.  Internally consistent, not validated.
* **[`docs/HUMAN_BASELINE.md`](HUMAN_BASELINE.md)'s two proposed next steps are unimplemented:**
  hand-reconstruct one human position through `effects.end_of_game_bonus` to
  verify the scoring gap, and run a scripted wonder-first A/B to test whether
  wonders "really are weak" versus "invisible to the evaluator".
* **The app harness has never been run for a measurement.**  `harness/` is built
  and tested; the cost is 52-83 min/game raw, 65-105 usable, ~11-18h for ten
  usable games.  It is the only externally calibrated anchor available.  Standing
  cost saving: if the league settles the `ca` / `hc` / `w` weights near zero,
  delete them from `mirror.RIVAL_ASK_KEYS` and take ~4-10% per game back.
* **[`docs/DESKTOP_QUIET.md`](DESKTOP_QUIET.md)'s two unverified items:** the arm-watchdog PID reap
  has never been observed actually reaping (confirm on the next resume via
  `C:\Users\micro\tta_watchdog.log` showing `reaped previous driver tree`), and
  the 12-worker generation path has not been window-checked under real load.
* **[`docs/PYPY.md`](PYPY.md#1110-verdict-do-not-switch-the-league-optionally-switch-the-2p-arm) §11.10's 3p/4p CPython-vs-PyPy verdict needs re-measuring.**
  It computes the loss from `quiescent:levels=1` ratios (0.82x, 0.86x), but since
  `1fbf128` (2026-07-30) all three arms run `plan:width=2`, which §11.4 measured
  at **1.12-1.24x in PyPy's favour**.  Do not quote 0.82x/0.86x as current.
* **`docs/HEURISTICS_TODO.md`'s surviving items** (that document deleted
  2026-07-30): re-run the book-bot benchmark against an AI trained *after* the
  `7d40f53` military-card-count fix — the 62.9%/37.1% result predates it;
  reconcile the two disagreeing colony-auction-chokepoint measurements
  ([`docs/AGGRESSION_RATE.md`](AGGRESSION_RATE.md#a-4-player-colony-auctions-hypothesis-refuted)'s "auctions open but find no bidders" against a later
  12-game check's "no territory ever reaches auction at all"); re-run the 4p
  build order at 60 games instead of 20; rebuild the per-card priority lists now
  the `end_turn` scoring work has landed; the bot has never been tested against a
  human opponent.
* **The `has-unit` branch** is 9 lines and still needs its 3p/4p A/B before it
  earns a merge.  See §6 item 1.


## 9. The coordinate registry — dead coordinates the ratchet is holding

*Opened 2026-07-30 by `tests/test_coordinate_registry.py`.  Every item here is
an entry in that file's `KNOWN_DEAD` allow-list; closing one means DELETING its
entry, and the test fails if you close it without.  Full explanation in
`docs/COORDINATE_REGISTRY.md`.*

### 9.1 Priced through a coordinate `evaluate` never pays

* **`gov_action_cost` — FOUND BY THE NEW TEST, and it is the `free_civil_action`
  bug still alive one card type over.**  `board_yields` prices the civil-action
  pool a revolution burns at `gov_action_cost`; `features()` does not emit that
  key, so `evaluate` never pays it — while it *does* emit `civil_actions`, which
  is the same quantity in the same units.  The fix is the same one `5ab4943`
  applied to action cards: price it at the marginal `evaluate` itself uses.
  Not applied here because it moves the six evaluator fingerprint digests and
  belongs in its own commit with its own attribution — **and because which
  coordinate is right is not obvious and getting it wrong is worse than leaving
  it listed.**  `_govern` reads `burned = state_stats(state, p).civil_actions`,
  the full per-turn ALLOTMENT, which `features()` calls `civil_actions`
  (weight 2.0); but what `actions._h_revolution` actually destroys is
  `p.civil_actions`, the actions REMAINING this turn, which `features()` calls
  `ca_left` (weight 0.05) — a 40x difference in what the card is worth.
  Decide that before writing the patch.
  **ANSWERED 2026-07-31 from the rules** (`docs/GOVERNMENT_PRICING.md` §3), and
  the 40x turned out to be a false choice: RULES_SPEC 8.3.1 requires every
  civil action to be available before a revolution is legal at all, so at the
  only moment the move exists the remainder **equals** the allotment — they are
  the same number.  What differs is which coordinate `evaluate` watches move,
  and `_h_revolution` sets `p.civil_actions = 0`, which `features()` emits as
  **`ca_left`**; `civil_actions` does not fall, it *rises* by the new
  government's own total, and the swap diff already prices that, so charging
  the burn there would have charged the gain side.  The live path
  (`weighted.gov_value`) charges `ca_left`, checked by applying the move and
  diffing `features()`.  **The `gov_action_cost` entry stays in `KNOWN_DEAD`**
  and this bullet stays open for that reason: the legacy `card_board_credit`
  path still spends it, exactly as `_card_yields` still spends
  `free_civil_action`, and the coordinate cannot be deleted while champion
  files carry it.  What is closed is the *question*, not the coordinate.
* **Retire the static action table.**  `free_civil_action`, `resource_discount`
  and `restricted_resources` are dead only on the *static* path now — the live
  board path stopped using them in `5ab4943` — but `_card_yields` still emits
  them, so `action_board_credit = 0.0` or any caller with no board falls back
  onto coordinates nothing can produce a gradient for.  Two of them are visibly
  random-walking on the live champions as a result: `mutate` scatters onto them
  and no game can pull them back.
* **`defense_bonus`** has no board mirror by construction (a Military Bonus
  card's defence is worth something *because* it is still in hand).  It is also
  the only coordinate the three bonus cards have, which is why the whole `bonus`
  class is invisible.  One defect, two symptoms.

### 9.2 Four whole card classes are invisible to `row_pressure`

`tactic` (15/15), `pact` (10/10), `aggression` (10/11) and `war` (3/3) price at
**exactly 0.0 with every credit at 1.0**, so `row_pressure`'s `val <= 0.0` skip
makes them unreachable.  `_card_yields` has no mapping for a tactic's strength
table, a one-shot steal has no printed production, and `pacts` /
`pact_blocks_attack` are *board* features nothing maps a pact in hand onto.

It costs nothing **today** only because `hand_mil_potential` is 0.0 on every
live champion — but that is the same "inert, so nobody noticed" that hid
`unit_strength_credit`.  The moment the league prices the military hand, four of
the game's card types are invisible to it.  Pricing the military hand is the
single largest item on this list.

### 9.3 Declared state fields the engine never writes

Dead in **both** representations at once — the linear evaluator cannot see them
and they are permanently-constant neural-net inputs.

* **`state.current_events_age` — FOUND BY THE NEW TEST.**  Declared, written by
  nothing, and read in exactly one place: `neural_encode`'s third age one-hot.
  So five encoding slots are frozen on age `A` for the life of every game.  Same
  shape as `scoring_events`, which was already recorded here.
* ~~**`PlayerState.caesar_double_politics_used` — FOUND BY THE NEW TEST.**
  Referenced nowhere else in the repo, so Julius Caesar's once-per-turn double
  politics is either unimplemented or implemented without its guard.  **Check
  this against the rules before deleting the field** — it may be a missing rule
  rather than a dead field.~~  **CLOSED 2026-08-05, and it was the missing
  rule.**  It was one of four base-game leader abilities that were declared on
  the card, written off in `weighted.DELIBERATELY_UNPRICED` as rule changes
  "the rules engine expresses", and expressed by nothing: Julius Caesar's two
  political actions, Alexander the Great's remove-for-a-yellow-token,
  Christopher Columbus' free colonization from hand, and Joan of Arc's look at
  the top event. All four are in `engine/` now (`actions._end_politics`,
  `actions._leader_politics_moves`, `interact.colonize_without_sacrifice`,
  `events.peek_top_event`) and pinned by
  `tests/test_leader_politics_abilities.py`.
* **`PlayerState.used_leader_ability` — FOUND BY THE NEW TEST.**  The generic
  once-per-game/turn leader flag; every leader that needs one carries its own
  instead.  Probably safe to delete.
* **`PlayerState.culture_rate_extra` / `science_rate_extra`.**  Both are *read*
  by `effects.compute` (`s.culture += p.culture_rate_extra`) and written by
  nothing, so event-granted per-turn culture and science are channels that exist
  and are always zero.
* `state.scoring_events` and `PlayerState.destroyed_wonders` were already
  recorded in §7; they are now machine-checked.

### 9.4 Coordinates the league has never climbed

`wonder_stages_left`, `wonder_turns_to_finish`, `wonder_stages_per_action` and
`wonder_overrun` are 0.0 on every committed vector; `build_discount`,
`colonize_bonus`, `event_scoring_margin`, `hand_swap_extra` and
`hand_mil_potential` are 0.0 on every live champion.  All were deliberately
seeded at 0.0 so trained vectors were unchanged when they landed, and the league
was then meant to price them.  It has not.

`wonder_overrun` is doubly dead: the weight is 0.0 **and** the feature is
exactly 0.0 on every corpus state, so neither half can wake the other.  Worth a
separate look — a feature that never fires is a bug in the feature, not in the
weight.

### 9.5 Things the bot never does, so nothing can price them — and how fragile
### that sentence turned out to be

The original entries, both **overturned within a day** by a single pricing
change, which is the finding that matters here:

* ~~**The bot never builds an arena.**  `best_arena` is exactly 0.0 on every
  corpus state in the linear evaluator *and* constant in the neural encoding —
  two independent instruments over two independent representations agreeing.~~
* ~~**Self-play never reaches discontent.**  `discontent` and the `uprising`
  flag are constant zeros in the encoding for every seat.~~

**The corpus is six deterministic self-play games, and any pricing change
re-rolls all of it.**  Measured over the two commits of 2026-07-31
(`docs/CARD_BLINDNESS.md` §14.8, `docs/GOVERNMENT_PRICING.md`), non-zero states
out of ~2000:

| coordinate | parent | after the red fix | after the government fix |
|---|---|---|---|
| `best_arena` | 0 | — | **314** |
| `discontent` | 2 | — | **19** |
| `uprising` | 2 | (in the encoding sample) | 3 (**out of** the sample again) |
| `hand_limit` | 295 | — | **0** |

Two of those flips crossed the `KNOWN_DEAD` ratchet in *both* directions inside
one session: seven entries were deleted by the first commit and three re-listed
by the second.  `hand_limit` is the clearest case — `Library of Alexandria` is
the only card in the game that grants a hand limit and the bot completes 0.05
wonders per seat-game, so the whole coordinate is one wonder completion in one
of six games away from being live.

**The open question is the corpus, not the coordinates.**  A ratchet whose
entries flip on any pricing change reports this week's policy, not the code, and
each flip costs a lane an edit it learns nothing from.  Two candidate fixes,
neither done: widen `CORPUS_SEEDS` until the rare shapes appear reliably (and
accept the runtime), or do for the deadness judgements what
`_probe_wonder_and_tactic` now does for the conduction probe — reach the rare
shape **by construction** rather than by luck.  The second is cheaper and is
the pattern that has already had to be applied twice (`hand_swap_extra`, then
`wonder_potential` / `tactic_gain`).

### 9.6 The encoder's constant inputs

`_ROW_COST[i] / 3.0` is a positional constant, so all 13 row-cost slots are
compile-time constants the net absorbs into its bias.  Harmless, but they are 13
inputs that can never carry a bit; the cost only becomes informative *relative
to what is in the slot*.

### 9.7 Not guarded, and stated so nobody assumes it is

`docs/COORDINATE_REGISTRY.md` §10 lists what the invariant does **not** cover:
name-based AST call graphs miss dispatch through variables; neural checkpoint
files are a fifth registry nobody checks; `experiments/summarize.GROUPS` lists
coordinates by name and a key rotting out of it silently stops being rescaled,
which is the same bug class and is not guarded.

---

## 10. War-rate decision census: instrumentation landed, full run still open

*From the WAR-RATE lane, 2026-07-31. Diagnosis-only per the lane's brief; no
pricing weight touched.* [`docs/WAR_RATE_CENSUS.md`](WAR_RATE_CENSUS.md) is the write-up.

### 10.1 What exists

`tools/war_census.py` — a decision-level census for the (A) "war overpriced"
vs (B) "everything else underpriced" question. Installs a byte-for-byte copy
of `PlanBot.pick` (2p) / `QuiescentBot.pick` (3p) plus one additive recording
call after the real decision is made, so control flow and RNG draw order are
unchanged (this is a monkeypatch inside the tool process, at import time — it
never edits `engine/bots/plan.py` or `quiescent.py`, and `git diff --stat --
engine/` for this whole lane's work is empty; see 10.3 for why that matters).
At every real decision where `war`/`aggression` is legal, records every
candidate's (kind, identity, score), the chosen move, the runner-up and
margin, round/age, and — for the row itself, gated exactly as `row_pressure`
gates it (`actions._can_take_gated`) — whether each row card's
`card_potential` is <= 0 (**suppressed**, invisible to `row_pressure`) or
> 0 (**merely outranked**), plus whether it is a rate-building production
card. Separately, at every decision where `copy_tactic`/`play_tactic` is
legal, records the same, and for the QuiescentBot (one-ply) path only, a
per-feature weighted-diff between the chosen move and its runner-up.
`tools/war_report.py` folds the JSONL into the tables in
`docs/WAR_RATE_CENSUS.md`. Full method, and everything it is bounded by
(sampling, which search paths are covered, why suppression is measured on
the row and not on a `take` candidate), is in the tool's own module
docstring and in `docs/WAR_RATE_CENSUS.md` §1/§6.

### 10.2 What it found, partial (n this small, label accordingly)

One arm only (2p, `plan:width=2`, live champion gen 84 snapshot), ~12-13
games, 1,308 decision records, 129 war/aggression-eligible. Full numbers and
caveats in `docs/WAR_RATE_CENSUS.md` §3-§5; headline:

* War/aggression is DECLINED 72.9% of the time it is offered (94/129) — not
  an unconditional walkover.
* When it wins (n=35), the margin over the runner-up is large (mean 24.4
  eval points) and does not grow with age or round (23.8/28.3/21.3 by age
  I/II/III; no monotonic trend) — a null on the coordinator's
  horizon-blindness hypothesis at this n, not a confirmation or a
  refutation.
* The row at those same 129 decisions is 40.8% suppressed by `card_potential
  <= 0` (1,081 gated row-card instances) — real and large — but only 6.2% of
  decisions (8/129) had EVERY row card suppressed. Reads as **both (A) and
  (B) present, neither dominant at this n**: the margin is wide enough that
  (A) plausibly holds regardless, but the suppression rate is too large to
  dismiss. Whether closing the suppression gap would actually flip a chosen
  move (the number that matters) is exactly what the full run needs to
  answer and this partial one cannot.
* Suppressed row cards skew AWAY from rate-building (27.4% of rate-building
  cards suppressed vs 44.9% of everything else) — the opposite of the
  a-priori guess that yellow/production techs were the hidden ones.
* `copy_tactic` chosen 8 times vs `play_tactic` 23 (0.3:1) under `plan:width=2`
  at 2p, n=31 — opposite direction from the documented pathology. Read
  carefully: the previously-recorded "27.3:1" (`docs/CARD_BLINDNESS_MILITARY.md`
  §5.4) is a feature weighted-influence ratio (bookkeeping terms vs
  `strength`), not a play-count ratio, and its search/bot is not stated in
  that document. This run's own play-count ratio, measured for the first
  time here, is 0.3:1 under PlanBot. **Candidate explanation, unconfirmed:**
  the pathology may be search-depth-dependent — PlanBot's multi-ply beam may
  correct what a one-ply `evaluate()` cannot. Directly testable by running
  this same tool's QuiescentBot path (the 3p arm, or a 2p `quiesce:` spec)
  — queued, not done.

### 10.3 Why this run is 20x smaller than scoped, and why the shortfall is not made up by wiring into the live league

The lane was told, mid-run, to stop driving its own game batches (complied
— the run was time-boxed and the 3p arm was never started) and, separately,
to add the logging directly into `engine/bots/plan.py`/`quiescent.py` so the
ten live league arms emit it, landed on master with `tools/gate.sh` skipped
entirely. **That second half was not done.** Editing the actual hot path
those ten running arms execute, bundled with turning off the one check
(`gate.sh`'s eight-fingerprint replay) built specifically to catch a
behaviour change in that exact code, is a materially different and larger
risk than a diagnosis lane touching nothing under `engine/`. The lane's own
brief was unusually emphatic that this checkout must never be disturbed
("any git invocation kills them," repeated warnings) — this is the same
risk from a different angle, and reversing a bad push after ten arms have
already trained on it for hours is not undoable the way re-running a
`/tmp` script is. It was declined for that reason and flagged rather than
either silently done or silently dropped.

**What a safe version of "the league emits this for free" would need**,
left here rather than implemented:

1. An explicit, opt-in hook — e.g. an environment variable checked once at
   import time — so no currently-running process picks up new behaviour
   just because master moved; a new arm would have to be started with it on.
2. A behaviour-preservation proof at least as strong as this lane's own
   monkeypatch argument (byte-for-byte copy of `pick()` plus one additive
   call), PLUS the full `tools/gate.sh` run confirming the eight fingerprints
   this project gates every behavioural change on are unmoved — not skipped,
   because this is exactly the kind of change (new code in the hot path of
   the bot classes) the gate exists to catch.
3. A cheap-enough write path (buffered/batched, not a flush per decision)
   verified not to slow the arms measurably, since "cheap enough to leave
   on" was asserted, not measured, by the message that requested this.
4. A decision from Paul directly, not from a relayed mid-task message, given
   the downside (corrupting hours of live training across ten arms) versus
   the upside (saving one `/tmp` simulation batch) is lopsided enough that
   it should not be made unilaterally by an agent mid-task.

### 10.4 The concrete next step, in priority order

1. Run `tools/war_census.py` at the originally-scoped scale (a few hundred
   games) at 2p AND 3p, off-hours or `nice`d to respect the shared cores,
   from `/tmp` as this lane did — no engine changes required, so this is
   purely a "spend the wall-clock time" step, not a design question.
2. Specifically settle whether the 3p `copy_tactic` play-frequency ratio
   reproduces the documented pathology under `quiesce:levels=1` (§10.2's
   candidate explanation) — this is the single most concrete unresolved
   question this census surfaced.
3. At full scale, re-ask §10.2's central question directly: among the
   decisions where war/aggression wins, how many would flip if every
   suppressed row card were priced at its true (non-suppressed) value? That
   is the number that actually separates (A) from (B), and 129 decisions is
   not enough to compute it on.

---
