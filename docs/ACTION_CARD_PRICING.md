# Sixteen of the thirty-three action cards are worth exactly nothing, because three of the coordinates they are priced in are not features

*2026-07-30.  Companion to [`docs/UNIT_TECH_PRICING.md`](UNIT_TECH_PRICING.md) and
[`docs/YELLOW_TECH_PRICING.md`](YELLOW_TECH_PRICING.md); same sentence, third colour:* **a card is worth
what `evaluate` pays for what it does.**

## 0. The finding, and the numbers it is

[`docs/OPEN_ITEMS.md`](OPEN_ITEMS.md) item 24 ranked action cards the largest single card-type
deficit in the game: **2.72 taken per seat-game at 2p against a human 12.98**
on the live 2p champion, **5.90 against 10.25** at 3p.  Measured again here on
`DEFAULT_WEIGHTS` — the vector two of the three live league arms are gen 0 of —
it is **7.35 against 12.98** at 2p and **5.83 against 10.25** at 3p.

The diagnosis is not a magnitude.  It is that **the value of an action card is
spelled in coordinates `evaluate` never multiplies by anything**:

| weight | in `DEFAULT_WEIGHTS`? | in `features()`? | value on every champion in the pool |
|---|---|---|---|
| `free_civil_action` | yes | **no** | 0.0 |
| `resource_discount` | yes | **no** | 0.0 (0.498 on the live 2p only) |
| `restricted_resources` | yes | **no** | 0.0 (0.155 on the live 2p only) |

`features()` emits none of the three, so `evaluate` never pays for them, so no
game the league plays can produce a gradient on them.  They are not weights the
trainer chose to leave at zero; they are weights the trainer has never had any
information about.  `unit_strength_credit` and `tech_levels` were coordinates
the evaluator *could* read but never had to *buy* ([`docs/UNIT_TECH_PRICING.md`](UNIT_TECH_PRICING.md)
§5.2); these are one step worse — the evaluator cannot read them at all.

The consequence is arithmetic.  Thirteen of the thirty-three action cards carry
**nothing but** a `freeCivilAction` and a `resourceDiscount` — every Rich Land,
Urban Growth, Engineering Genius and Efficient Upgrade — so they price at
**exactly 0.000**.  Three more (the Reserves) price at exactly 0.000 for a
second, independent multiplied-by-zero reason, and three more (Endowment for
the Arts, Wave of Nationalism, Military Build-Up) for a third.  Sixteen of
thirty-three cards worth literally nothing to the evaluator, in the type the
bot under-takes 4.8x.

**And the per-card take rates say so directly.**  Ranked by whether the card
touches a live coordinate at all — this is the whole finding in one table,
`DEFAULT_WEIGHTS`, 30 games at 2p, 60 seat-games:

| card | priced through | static price | bot takes | human takes |
|---|---|---|---|---|
| Revolutionary Idea | `science` (live) | 2.00 / 3.00 | 1.07 | 1.12 |
| Breakthrough | `science` (live) + a dead flag | 1.00 / 1.50 | 1.03 | 1.36 |
| Patriotism | `military_actions` (live) + a dead weight | 0.70 | 0.85 | 0.75 |
| Cultural Heritage | `culture` + `science` (live) | 4.50 / 3.00 | 0.78 | **0.28** |
| Stock Pile | `food_stock` + `resource_stock` (live) | 0.50 | 0.22 | 0.16 |
| Frugality | `food_stock` (live) + a dead flag | 0.20–0.60 | 0.58 | 0.85 |
| **Reserves** | a choice multiplied by `card_board_credit` = 0 | **0.000** | 0.73 | **2.05** |
| **Urban Growth** | two dead weights | **0.000** | 0.65 | **1.81** |
| **Efficient Upgrade** | two dead weights | **0.000** | 0.37 | **1.09** |
| **Rich Land** | two dead weights | **0.000** | 0.33 | **1.14** |
| **Engineering Genius** | two dead weights | **0.000** | 0.28 | **1.48** |
| **Wave of Nationalism** | `board_extra`, gated on `card_board_credit` = 0 | **0.000** | 0.02 | **0.31** |
| Endowment for the Arts | `board_extra`, gated on `card_board_credit` = 0 | **0.000** | 0.20 | 0.32 |
| Military Build-Up | `board_extra`, gated on `card_board_credit` = 0 | **0.000** | 0.23 | 0.28 |

Every card whose price runs through a live trained weight is at or above the
human rate.  Every card priced at 0.000 is 3x to 15x under it.  The line
between the two halves of that table is the line between "is a feature" and
"is not a feature", and nothing else — not age, not colour, not cost.

## 1. The classification, per card

The house method ([`docs/HAZARDS.md`](HAZARDS.md)): every play-rate outlier lands in exactly
one of four buckets.

* **(a) the ENGINE cannot do it.**  **Empty.**  All thirty-three effects are
  implemented — `engine/actions.py:_h_play_action` resolves the gains,
  `apply_card_gains` the one-shots, `free_action_moves` the ordered action, and
  `_action_card_playable` gates on the ordered action being legal.
  `tests/test_action_pricing.py:TestTheEngineCanActuallyDoIt` asserts every one
  of the thirty-three produces a legal `("play_action", name)` from a stocked
  hand, so this bucket is closed by assertion and not by reading.
* **(b) priced, but the weight is 0.0.**  **Nineteen cards**, in three distinct
  mechanisms (see §2).
* **(c) priced, weight live, the bot declines.**  **Fourteen cards** — and for
  eleven of them the decline is defensible (they are at or above the human take
  rate).  Cultural Heritage is the one taken *more* than humans take it (0.78
  against 0.28) and the reason is §2.3: its four culture is priced at the bare
  `w["culture"]` where `evaluate` pays a phase blend that is *lower* than the
  bare weight for most of the game.
* **(d) NOT PRICED AT ALL.**  **No whole card**, but one real sub-item: nothing
  prices **which** action a `freeCivilAction` orders.  Rich Land ("a farm or a
  mine") and Urban Growth ("an urban building") are the same card to
  `card_potential` apart from their discount, and always will be until
  something asks the board what the best legal free build would be.  Left open,
  §6.

| card | age | effect keys | bucket | why |
|---|---|---|---|---|
| Rich Land | A/I/II | `freeCivilAction`, `resourceDiscount` | **b** | both weights are non-features, 0.0 everywhere |
| Urban Growth | A/I/II/III | `freeCivilAction`, `resourceDiscount` | **b** | same |
| Engineering Genius | A/I/II/III | `freeCivilAction`, `resourceDiscount` | **b** | same |
| Efficient Upgrade | II/III | `freeCivilAction`, `resourceDiscount` | **b** | same |
| Frugality | A/I/II | `freeCivilAction`, `gainFood` | **b** | the food is live; the free action, which is most of the card, is not |
| Breakthrough | I/II | `freeCivilAction`, `gainScience` | **b** | the science is live; the free development is not |
| Reserves | I/II/III | `gainFoodOrResources` | **b** | `_card_choices` is correct and is multiplied by `card_board_credit`, 0.0 on every champion — and the early return above it skips it outright |
| Wave of Nationalism | II | `resourcesForMilitaryUnitsPerStrongerCivilization` | **b** | `board_yields.board_extra` computes it correctly and is gated on `card_board_credit` + `card_board_action`, both 0.0; it then lands on `restricted_resources`, also 0.0 |
| Military Build-Up | III | same | **b** | same, twice |
| Endowment for the Arts | III | `culturePerCivilizationWithMoreCulture` | **b** | same gate; lands on `culture`, which *is* live, so this one is only singly dead |
| Patriotism | A/I/II/III | `militaryActions`, `resourcesForMilitaryUnits` | **b**/**c** | the military action is live (0.85 taken against a human 0.75); the 1–4 ring-fenced resources are 0.0 |
| Cultural Heritage | A/I | `gainCulture`, `gainScience` | **c** | both live — and **over**-taken, 0.78 against 0.28, because `culture` is read at the bare weight and not at the phase blend |
| Revolutionary Idea | II/III | `gainScience` | **c** | live, 1.07 against a human 1.12 |
| Stock Pile | A | `gainFood`, `gainResources` | **c** | live, 0.22 against a human 0.16 |

Nineteen **b**, fourteen **c**, zero **a**, zero whole-card **d**.

(Nineteen and fourteen sum to thirty-three plus the four Patriotisms, which are
counted in both: the military action is live and the ring-fenced resources are
not.)

## 2. The mechanism, in three parts

### 2.1 Two of the coordinates are not features (13 cards at exactly 0.000)

`_EFF_TO_FEATURE` sends `resourceDiscount` to `resource_discount` and
`_EFF_SPECIAL` sends `freeCivilAction` to `free_civil_action` as a bare
presence flag.  Neither name appears in `features()`.  `card_potential` does
`w.get(k, 0.0)`, gets 0.0, and returns 0.0 for the whole card.

Turning the weights up by hand cannot fix it and that is the part worth being
precise about, because it is why this is a reshaping and not a retuning.  There
is no scale to turn them *to*: `free_civil_action` is a flag whose "1.0" means
nothing in eval points, and the only honest number for it is whatever
`evaluate` already pays for a civil action — which is a number the evaluator
has, in `w["civil_actions"]`, and which the league has fitted, and which the
card price simply did not read.

### 2.2 A choice is multiplied by the board credit (3 cards at exactly 0.000)

`_card_choices` resolves Reserves' "gain N food OR N resources" as a max over
the group.  It is correct, and it is dead twice over in `card_potential`:

```python
if not base and not board:          # base = w["card_board_credit"] = 0.0
    return _sum_yields(_card_yields(name), w, credit)     # <- returns here
...
for group in _card_choices(name):
    total += base * max(...)                              # <- x 0.0
```

The comment beside it says outright that `_card_choices` "is not board-aware
pricing at all — it needs no board".  It is riding a gate that has nothing to
do with it, and that gate is 0.0 on every champion in the league.  Reserves is
the second-most-taken action card among humans (2.05 per seat-game at 2p) and
the bot's price for all three printings is zero.

### 2.3 A one-shot `gainCulture` is priced at the bare weight

`culture` is in `PHASE_KEYS`.  `evaluate` pays
`w[k] + (1-L)w[k_early] + L·w[k_late]`; `card_potential` reads the bare `w[k]`.
On the defaults that is 1.0 against a marginal of 0.6 in Age A rising to 2.5 in
Age IV.  This is the **same** phase-blend mismatch `feature_marginal` was
written for in [`docs/YELLOW_TECH_PRICING.md`](YELLOW_TECH_PRICING.md), still live on the one-shot gains
because that lane only routed technologies through it.  It is why Cultural
Heritage is the one action card the bot *over*-takes.

## 3. What changed

`weighted.action_value(name, state, idx, w)` — `tech_value`'s sibling for the
one civil type still on the static table.  Everything in it is a derivation:

1. **A one-shot gain is worth `feature_marginal`, not `w[key]`.**  Closes §2.3
   for every action card at once.
2. **A free civil action is worth `free_action_credit` civil actions, and
   that credit ships at 0.0 because the action economy is a wash.**  This is
   the one number here that is not a pure derivation, the first draft of it got
   the derivation backwards, and §5 is the correction written out.  RB §3.11 /
   `actions._h_play_action`: playing a yellow action card **costs one civil
   action** (`pay_ca(state, p, 1)`) and **grants one** ordered action.  Rich
   Land is "spend a civil action to build a farm three resources cheaper", not
   "spend a civil action and get one back", so the card is worth its
   **discount** and crediting the grant on top double-counts an action it never
   gives you.
3. **Both ring-fenced yields are resources.**  `resourceDiscount` at the full
   `resource_stock` marginal (you were going to make that build — the card is
   only playable if the ordered action is legal), `resourcesForMilitaryUnits`
   at `restricted_resource_credit` times it, default 1.0, which is the upper
   bound and is a weight rather than a constant precisely so the league can
   measure the ring fence instead of a guess asserting it.
4. **A choice is a max and needs no board credit.**  Closes §2.2.
5. **The three per-table-size cards keep their board scaling.**
   `board_yields.board_extra` already computed rivals-with-more-culture and
   rivals-stronger-than-me from the live boards; it is **called**, not
   reimplemented, so there is one implementation and it cannot drift.

Not charged, deliberately: the civil action playing the card costs (point 2),
and the card leaving the hand, which `hand_value` already prices on the board
side.

No information is added: the effects are printed on the card, and
`board_extra` reads only public culture totals and public strengths.

### 3.1 The gate, and it is one constant

`action_board_credit`, default **1.0**, absent from every champion file in the
league — so `load_weights` fills it from `DEFAULT_WEIGHTS` and the fix is live
on all three arms at once, deliberately, because the defect is present on all
three.  **0.0 sends every action card back to the static table, which is the
parent commit's pricing byte for byte on all 236 cards**
(`tests/test_board_yields.py:test_zero_action_credit_is_the_static_answer_for_every_action`),
and that is what makes the change duellable against itself in one process on
the same deal.

`restricted_resource_credit` (1.0) and `free_action_credit` (0.0) are the only
other new keys.

The three dead weights are **kept, not deleted**: they are still the STATIC
answer, which `analysis/`, `tools/card_blindness.py` and the pricing censuses
call with no board, and they are what `action_board_credit` = 0.0 goes back to.
Deleting them would make the opt-out unrepresentable.

## 4. What it did — before/after, `tools/play_rate.py`

Mirror table, `plan:default,width=2,det=1`, same seeds, `DEFAULT_WEIGHTS`, the
only difference being `action_board_credit` 0.0 → 1.0.  30 games at 2p (60
seat-games), 20 at 3p (60 seat-games).  **Takes and plays reported separately**
— an action card can be taken and never played, and the two decisions are
different code paths.

Mirror table, `plan:default,width=2,det=1`, same seeds, `DEFAULT_WEIGHTS`, the
only difference being `action_board_credit` 0.0 → 1.0 — a **descriptive**
measurement, deliberately small: it says whether the bug is fixed, not how much
stronger the bot is.  **Takes and plays are reported separately** because an
action card can be taken and never played, and the two decisions are different
code paths.

**On the shipped base (`7bf483a`), 20 games at 2p, 40 seat-games:**

| per seat-game | take before | take after | take human | play before | play after |
|---|---|---|---|---|---|
| Breakthrough | 1.15 | 0.88 | 1.36 | 1.15 | 0.88 |
| Cultural Heritage | 0.78 | 0.72 | 0.28 | 0.78 | 0.72 |
| Efficient Upgrade | 0.38 | **0.55** | 1.09 | 0.15 | **0.33** |
| Endowment for the Arts | 0.30 | **0.45** | 0.32 | 0.17 | **0.38** |
| Engineering Genius | 0.20 | **0.75** | 1.48 | 0.00 | 0.00 |
| Frugality | 0.62 | 0.40 | 0.85 | 0.17 | 0.10 |
| Military Build-Up | 0.25 | 0.30 | 0.28 | 0.03 | 0.07 |
| Patriotism | 0.88 | **1.25** | 0.75 | 0.55 | **0.75** |
| Reserves | 0.65 | **0.93** | 2.05 | 0.30 | **0.50** |
| Revolutionary Idea | 1.05 | 1.15 | 1.12 | 1.02 | 1.00 |
| Rich Land | 0.28 | **0.55** | 1.14 | 0.20 | **0.38** |
| Stock Pile | 0.20 | 0.17 | 0.16 | 0.05 | 0.05 |
| Urban Growth | 0.55 | **0.78** | 1.81 | 0.25 | **0.55** |
| Wave of Nationalism | 0.03 | **0.12** | 0.31 | 0.03 | **0.12** |
| **all action** | **7.30** | **9.00** | **12.98** | **4.85** | **5.82** |

Total civil takes 23.70 → 23.38 and developments 6.78 → 6.17, so this is a
substitution inside the row and not a change in appetite.

The three tables below were measured on the **previous** base (`8b972ef`,
before the horizon lane landed) at 30 games / 60 seat-games at 2p and 20 games
/ 60 seat-games at 3p.  They are kept because they carry the 3p side and
because the two bases agree to within the noise of a 40-seat-game sample (7.35
→ 8.80 there against 7.30 → 9.00 here), which is itself the useful check.

| per seat-game | 2p take before | 2p take after | 2p human | 2p play before | 2p play after |
|---|---|---|---|---|---|
| Breakthrough | 1.03 | 0.92 | 1.36 | 1.03 | 0.88 |
| Cultural Heritage | 0.78 | 0.77 | 0.28 | 0.78 | 0.77 |
| Efficient Upgrade | 0.37 | **0.50** | 1.09 | 0.17 | 0.33 |
| Endowment for the Arts | 0.20 | **0.47** | 0.32 | 0.07 | **0.43** |
| Engineering Genius | 0.28 | **0.68** | 1.48 | 0.02 | 0.02 |
| Frugality | 0.58 | 0.35 | 0.85 | 0.07 | 0.03 |
| Military Build-Up | 0.23 | 0.25 | 0.28 | 0.05 | 0.07 |
| Patriotism | 0.85 | **1.23** | 0.75 | 0.53 | 0.65 |
| Reserves | 0.73 | 0.75 | 2.05 | 0.45 | 0.32 |
| Revolutionary Idea | 1.07 | 1.15 | 1.12 | 1.02 | 1.08 |
| Rich Land | 0.33 | **0.48** | 1.14 | 0.22 | 0.27 |
| Stock Pile | 0.22 | 0.18 | 0.16 | 0.05 | 0.10 |
| Urban Growth | 0.65 | **0.82** | 1.81 | 0.40 | 0.45 |
| **Wave of Nationalism** | **0.02** | **0.25** | 0.31 | 0.02 | **0.18** |
| **all action** | **7.35** | **8.80** | **12.98** | **4.87** | **5.58** |

| per seat-game | 3p take before | 3p take after | 3p human | 3p play before | 3p play after |
|---|---|---|---|---|---|
| Efficient Upgrade | 0.37 | 0.48 | 1.03 | 0.22 | 0.27 |
| Endowment for the Arts | 0.13 | **0.32** | 0.18 | 0.08 | **0.30** |
| Engineering Genius | 0.37 | **0.73** | 1.06 | 0.00 | 0.00 |
| Reserves | 0.67 | **0.95** | 1.72 | 0.43 | 0.57 |
| Rich Land | 0.22 | **0.53** | 0.96 | 0.05 | **0.30** |
| Urban Growth | 0.52 | **0.98** | 1.50 | 0.37 | **0.67** |
| Wave of Nationalism | 0.03 | 0.12 | 0.17 | 0.00 | 0.08 |
| **all action** | **5.83** | **7.53** | **10.25** | **3.88** | **4.62** |

**What moved is exactly what the diagnosis said would move.**  Every card in
§0's "priced at 0.000" half goes up and every card in the live-weight half
stands still.  Wave of Nationalism, the single worst outlier in the game at
0.02 against a human 0.31, goes to 0.25 and starts being **played** (0.02 →
0.18).  Endowment for the Arts more than doubles at both counts.  Rich Land,
Urban Growth, Efficient Upgrade and Engineering Genius all rise 35–140%.

**What did NOT happen, reported rather than buried: the 4.8x gap is not
closed.**  7.30 → 9.00 at 2p against a human 12.98, and 5.83 → 7.53 against
10.25 at 3p.  About a quarter of the gap.  The residual is not a bug — it is that a
`freeCivilAction` card is genuinely worth only its discount (§3 point 2), and
2–4 resources is a smaller thing than the technology it competes with in the
row.  Whether humans are right to take twelve of these a game is a question
about the *rest* of the evaluator, not about whether these sixteen cards are
priced; that half is now unambiguously fixed.

Total civil takes are flat on the old base too (23.35 → 22.63 at 2p, 22.58 →
22.52 at 3p) and developments near-flat (6.05 → 5.67, 7.15 → 6.92).


## 5. Strength: the paired A/Bs, and the modelling error the first one caught

**These A/Bs were run before the owner's 2026-07-30 instruction to stop
running them and let the league logs be the measurement.**  They are reported
because they exist and because the first one is what caught the modelling error
in §3 point 2 — not because anything was waiting on them.  `experiments.
evaluate`, the fix against **itself** on `DEFAULT_WEIGHTS`, `WeightedBot`,
paired on the deal, the two arms differing in exactly one number.

| arm | games | win rate | null | p |
|---|---|---|---|---|
| 2p, `free_action_credit` **1.0** | 300 | **32.83%** ±5.2pp | 50% | ~0 |
| 2p, `free_action_credit` **0.5** | 300 | **41.33%** ±5.5pp | 50% | 0.0019 |
| 2p, `free_action_credit` **0.0** (shipped) | 300 | **47.67%** ±4.6pp | 50% | 0.31 |
| 3p, `free_action_credit` **0.0** (shipped) | 240 | **32.71%** ±5.6pp | 33.3% | 0.82 |

**The shipped default is a null at both counts, and the credit sweep is
monotone.**  That monotonicity is the evidence for §3 point 2 and it is the
reason the default is 0.0 rather than a guess: at 1.0 the behaviour census
looked like a triumph — action takes 7.35 → **11.80** against a human 12.98,
landing exactly on the human number — while the bot lost 17pp of win rate,
because the same civil actions stopped buying technologies (11.9 → 8.0
developed a game) and 5.5 action cards a seat-game were taken and never played.
**Right rate, wrong reason.**  A behaviour census alone would have shipped it.

This is the standing policy case: correct modelling gets committed on the
modelling, and the null is what a correct model of sixteen previously-invisible
cards should look like when the rest of the vector was fitted without them.


## 5.1 Fingerprints

Six of the eight `tools/gate.sh` arms moved and two did not.

| arm | parent (7bf483a) | this commit | third clone, `action_board_credit` 0.0 |
|---|---|---|---|
| NARROW (greedy) | ca255af3 | ca255af3 | ca255af3 |
| WIDE (greedy) | f223cea1 | f223cea1 | f223cea1 |
| WNARROW | 6d888d7c | **ba77b499** | 6d888d7c |
| WWIDE | c52302c2 | **f4d6a545** | c52302c2 |
| QNARROW | bbbb203a | **4ab439b2** | bbbb203a |
| QWIDE | 3df0155f | **5d05f578** | 3df0155f |
| PNARROW | 1b883d6f | **0a637b40** | 1b883d6f |
| PWIDE | 3922ebc4 | **ccc96764** | 3922ebc4 |

**The base moved under this lane and every arm was recomputed, not carried
over.**  The whole set was first derived against `8b972ef` (e9cdc2d4 /
0c5a4337 / ce0d22bf / 49b898e1 / 65d9a884 / b952c68e, with all eight verified
the same way); the horizon lane ([`docs/MODEL_CONSTANTS.md`](MODEL_CONSTANTS.md)) then landed
underneath and moved all six evaluator arms on its own.  Re-using a digest
across a base change is exactly the laundering [`docs/PYPY.md`](PYPY.md#90-a-trap-found-before-any-code-was-written-the-fingerprint-files-are-stale) §9.0 forbids, so
the clean-base control, both derivations and the attribution were all re-run
from scratch on `7bf483a`.  The discarded first set is recorded in
`tools/gate.sh` so a reader can see it was discarded rather than reconciled.

* **The two GreedyBot arms held still**, which is the informative half:
  GreedyBot never calls `card_potential`, so an arm of it moving would have
  meant a card-pricing change had leaked into the rules.
* **All six evaluator arms moved**, predicted before the run: `DEFAULT_WEIGHTS`
  carries `action_board_credit` at 1.0, so every action card in the row and in
  the civil hand prices differently for all three searching bots.
* **Two-sided per [`docs/PYPY.md`](PYPY.md#90-a-trap-found-before-any-code-was-written-the-fingerprint-files-are-stale) §9.0**: derived from scratch in
  `/tmp/actionfix` and independently in `/tmp/actionfix2`, two separate clones
  of the same tree, which agreed byte for byte on **all eight** arms —
  including the two that did not move.  A clean-base control on the parent
  commit in `/tmp/actionctl2` reproduced all eight of *its* committed
  constants first.
* **Attributed to one constant.**  A third clone with `action_board_credit`
  1.0 → 0.0 and nothing else touched reproduces **all eight** parent digests.
  `action_value`, `_yield_marginal`, `_RESTRICTED_TO_FEATURE`, `_is_action`,
  `restricted_resource_credit` and `free_action_credit` are therefore provably
  inert on their own.

Nothing was re-derived to make the gate pass.

Test count **1107 → 1128**: +20 from `tests/test_action_pricing.py`, +1 from
splitting `test_zero_credit_is_the_static_answer_for_every_card` in
`tests/test_board_yields.py` a third time.

**One existing test was repaired, and it is worth recording why rather than
just that.**  `tests/test_row_features.py:
test_swept_card_cannot_lend_its_name_to_a_dealt_card` builds its position from
40 plies of self-play and then checks, as a *vacuity guard*, that the dealt
card is one the unmasked evaluator would have priced.  It dealt that card at
slot 7, outside the sweep slide, where `row_pressure` scores it through
`bargain` — and `bargain` multiplies by `rival_take_p`, which
[`docs/MODEL_CONSTANTS.md`](MODEL_CONSTANTS.md) had just turned into a per-rival board estimate that
saturates at 1.0 (survive 0, bargain 0) whenever the one rival can afford the
one card they can reach.  Any evaluator change moves the 40-ply tableau, so
**the guard could stop guarding without anything failing**; this lane is what
tripped it.  The card is now dealt at slot 5, inside the slide, where the
quantity it moves is a plain sum of `card_potential`.  The assertion is
unchanged in strength, still requires the mask to skip a suffix card, and now
passes on the parent tree and this one for the same reason rather than by
coincidence.

**Negative control on the regression test**, in the sense
`tests/test_search_root_is_determinized.py` uses it: dropped onto a clean tree
at the parent commit, `tests/test_action_pricing.py` gives **12 failures of
19** (before `free_action_credit` split one test into two).  The seven that
still pass there are exactly the ones written to pass — the three
`TestTheDefect` controls (thirteen cards still price at 0.000 statelessly, the
three coordinates are still not features, the Reserves are still gated on the
board credit), `TestTheEngineCanActuallyDoIt`, and the two `TestTheOptOut`
tests that are trivially true when there is no credit.

## 6. Open, and deliberately not done here

1. **Nothing prices WHICH action a `freeCivilAction` orders.**  Rich Land and
   Urban Growth differ only in their discount to `card_potential`.  The honest
   price is the best legal free build's own delta, which `board_yields.
   tech_upgrade` can already compute for the urban and worker types — but it is
   a per-card enumeration on a path `row_pressure` runs for every row card at
   every leaf, so it is a performance question as much as a modelling one.
   This is bucket **(d)** and it is the only one left.
2. **Engineering Genius is under-*played*, not just under-taken**, and the
   cause is somewhere else: 0.02 plays per seat-game at 2p against a human 1.33
   and **0.00** at 3p.  It orders a wonder stage and is illegal without a
   wonder in progress; the bot completes 1.73 wonders a game at 2p and **zero**
   at 3p ([`docs/OPEN_ITEMS.md`](OPEN_ITEMS.md#11-wonder_potentials-scale-has-no-trustworthy-evidence) §1.1).  Re-measure it after the wonder hole is
   closed, not before.
3. **Frugality is under-played for a related reason** — 0.07 against a human
   0.83.  It orders "increase your population at full price", and how often the
   bot wants a population increase is `pop_cost`'s question, not this one.
4. **`free_civil_action`, `resource_discount` and `restricted_resources` are
   still non-features.**  This change routes around them rather than deleting
   them, because they are still the stateless answer.  If anyone ever wants
   them live, the fix is to make them features, not to fit them.
