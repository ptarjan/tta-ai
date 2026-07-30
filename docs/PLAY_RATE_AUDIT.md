# Does the bot actually PLAY the cards? A per-card play-rate audit (2026-07-30)

Four audits have now measured card **pricing** — `docs/CARD_BLINDNESS.md`,
`docs/CARD_BLINDNESS_MILITARY.md`, `docs/CARD_CENSUS.md`,
`docs/UNCOVERED_TYPES.md` — and `CARD_CENSUS.md` states the gap all four share
in its own words: *"the suite checks that a card is priced, never that its
price is read."*

`unit_strength_credit` is what that gap cost. The ten military unit cards were
found blind, a feature was added, four tests in `tests/test_card_pricing.py`
were written and pass, and the weight shipped at **0.0** — so
`card_potential` multiplied the entire new channel by zero and the ten cards
priced *exactly* as they had before the fix. Nothing failed for days while
`docs/SYSTEM_COVERAGE.md` measured the bot taking military unit technology
0.06–0.45 times per seat-game against a human 2.79–3.84.

This document asks the other question, for all 236 cards: **at what rate does
the bot take or play each one, and what rate does a human play it at?**

Failure-mode labels, used on every outlier below and not interchangeable:

* **(a) ENGINE** — the rule is not implemented, or is implemented in a way
  that removes the decision.
* **(b) INERT** — priced, but the weight that scales the price is 0.0 or
  wrong-signed on every vector, so the pricing never reaches a decision.
  **This is the `unit_strength_credit` pattern and what this audit hunts.**
* **(c) DECLINED** — priced, weight live, and the bot chooses otherwise. May
  be correct play; reported, not condemned.
* **(d) UNPRICED** — the value never reaches any feature at all.

## 1. Method

| | 2p | 3p | 4p |
|---|---|---|---|
| bot games | 80 | 36 | 8 |
| bot seat-games | 160 | 108 | 32 |
| rounds/game | 20.1 | 20.1 | 21.9 |
| culture/seat | 189.4 | 128.8 | 109.2 |
| bot | `plan:width=2,det=1` mirror — every seat the same policy | | |
| vector | `experiments/league_state/champion_2p.json`, **gen 72, live** | `archive_prequiescent_20260730/ladder_3p/gen01314.json` | `archive_prequiescent_20260730/ladder_4p/gen00361.json` |
| human games | 692 | 133 | 186 |
| human seat-games | 1,384 | 399 | 744 |
| engine errors | 0 | 0 | 0 |

**Which vector produced which column matters and is stated once here.** Only
the 2p column describes a currently-live, currently-training vector. The live
`champion_3p.json` and `champion_4p.json` are gen 0 — both arms were restarted
clean today — so censusing them would measure `DEFAULT_WEIGHTS`, not a policy.
The 3p and 4p columns are the **archived pre-restart champions**, the last
vectors that played those table sizes at strength (gen 1,314 and gen 361).

**n = 8 games at 4p.** A factor of five in that column is a finding; a
difference of 30% is not. 2p and 3p carry the weight of this document.

Instruments, both committed with it:

* `tools/play_rate.py` — one command per side. The bot half **reuses
  `tools/system_census.py` unchanged**: it subclasses that module's `Rec` to
  add per-card buckets and substitutes the subclass before calling
  `system_census.run`, so the seat wrapper, the five engine taps and the
  `state is real` guard that makes them honest are the same code, not a copy.
* `tests/test_play_rate.py` — the standing check, section 6.

```
python3 tools/play_rate.py human --out /tmp/human_cards.json
nice -n 15 python3 tools/play_rate.py bot --players 2 --games 20 --seed 0 \
    --spec plan:experiments/league_state/champion_2p.json,width=2,det=1 \
    --out /tmp/cards_2p_a.json
python3 tools/play_rate.py report --human /tmp/human_cards.json --exact \
    /tmp/cards_2p_*.json /tmp/cards_3p_*.json /tmp/cards_4p_*.json
```

### Two measurement contracts, and they are not interchangeable

* **TAKE** (civil deck, 127 cards): the journal prints `X takes <card> in
  hand` and the bot emits a `take` move. Both sides are a free choice from a
  visible row, so the rates compare directly.
* **PLAY** (military deck, 109 cards): nobody chooses to *take* these, they
  are drawn blind. Only the decision to *use* one compares, so those rows
  count plays, declarations, tactic set-ups, colonizations and defence spends
  on both sides. Territories are counted as **colonies held**, because a
  territory is won at auction and never "played"; events are counted as
  **revealed**, because nobody chooses to play one.

### The name join is at base name, and that is a real limit

BGO prints `Orange takes Engineering Genius in hand` — no age suffix — while
the database calls those three cards `Engineering Genius (A)`, `(I)` and
`(III)`. Every rate below is therefore joined on `baseName`, and a base name
covering k printings is one row. The bot side is *also* reported per exact
card (`--exact`), because "which precise card is never played" needs the full
name and only the bot side can answer it. Six BGO spellings differ from the
database (`Stockpile`, `Charles Chaplin`, `Maximillien Robespierre`,
`Johannes Sebastian Bach`, `Ocean Liner`, `Bread & Circuses`) and are aliased;
**every other journal token resolves, and the run reports 0 unmatched.** Take-
backs (`X puts <card> back in the row`) are matched against the most recent
unmatched take and both are dropped, as `tools/bgo_parse.py` does it.

Cross-check that the parser is measuring the right thing: it independently
reproduces `docs/SYSTEM_COVERAGE.md`'s human unit-technology rate to the
digit — **3.84 / 2.79 / 3.43** per seat-game at 2p/3p/4p.

## 2. Headline: the bot builds one civilization and it is blue

Per seat-game, summed over each type's cards. `h` = human.

| type | 2p h | 2p bot | 3p h | 3p bot | 4p h | 4p bot |
|---|---|---|---|---|---|---|
| **lab** | 1.62 | **0.03** | 1.27 | **0.00** | 1.41 | **0.00** |
| **mine** | 1.18 | **0.05** | 1.21 | **0.00** | 1.33 | 0.03 |
| **artillery** | 0.85 | **0.03** | 0.68 | 0.04 | 0.76 | 0.06 |
| **infantry** | 1.12 | **0.09** | 0.90 | **0.02** | 1.03 | 0.06 |
| **cavalry** | 1.22 | **0.07** | 0.89 | **0.03** | 1.09 | 0.06 |
| **air** | 0.65 | **0.03** | 0.32 | **0.01** | 0.56 | 0.03 |
| **farm** | 1.34 | **0.18** | 1.26 | 0.13 | 1.39 | 1.03 |
| bonus (military) | 2.14 | 0.19 | 1.70 | 0.00 | 2.08 | 0.00 |
| territory | 1.51 | 0.54 | 1.15 | 1.46 | 1.39 | 1.19 |
| special-tech | 3.08 | 1.72 | 2.45 | 3.06 | 2.58 | 3.84 |
| action | 12.98 | 8.16 | 10.25 | 9.62 | 9.61 | 10.53 |
| leader | 3.70 | 2.97 | 3.62 | 3.26 | 3.57 | 3.69 |
| tactic | 2.12 | 1.57 | 1.95 | 2.21 | 2.32 | 2.13 |
| wonder | 2.87 | 2.46 | 2.58 | **0.94** | 2.65 | **0.28** |
| aggression | 0.69 | 0.89 | 0.54 | 0.91 | 0.75 | 0.50 |
| government | 1.37 | 1.63 | 1.41 | 2.44 | 1.43 | 2.50 |
| **temple** | 0.51 | **1.26** | 0.46 | 0.06 | 0.54 | 0.03 |
| **library** | 0.70 | **2.19** | 0.95 | 0.66 | 0.87 | 0.06 |
| **theater** | 0.65 | **2.27** | 0.99 | 1.96 | 0.80 | 0.06 |
| **arena** | 0.32 | 0.11 | 0.30 | 0.04 | 0.53 | **1.25** |
| **war** | 0.25 | **0.60** | 0.16 | **1.28** | 0.15 | **1.25** |

Collapsed: **military unit technology is 13× to 47× under the human rate,
laboratories 65× under at 2p and absolutely zero at 3p and 4p, mines 24× under
at 2p and zero at 3p, and the yellow half of the tech tree is barely bought at
all — while urban blue buildings run 2.5–3.5× over.** The 2p bot takes 23.2
civil cards a seat-game against a human 34.2, and spends that smaller budget
almost entirely on one colour.

The 3p and 4p columns add a second shape: **wonders collapse** (0.94 and 0.28
against a human 2.58 and 2.65) and **wars run 8× over** (1.28 and 1.25 against
0.16 and 0.15), both of which `docs/SYSTEM_COVERAGE.md` already reported at the
whole-subsystem level. This document adds the card identities under them.

## 3. The (b) findings — priced, but the price is never read

### 3.1 `unit_strength_credit` — the ten military unit cards

| vector | value |
|---|---|
| `DEFAULT_WEIGHTS` | 0.0 |
| 2p champion, gen 72 (live) | **0.0** |
| 3p ladder tip, gen 1,314 | **0.0** |
| 4p ladder tip, gen 361 | **−0.01713** — wrong-signed |

`_card_yields` reads a unit's top-level `strength` and emits it as
`(strength, n, _Y_UNIT)`; `_CREDIT_OF[_Y_UNIT]` scales it by
`unit_strength_credit`, default 0.0. At 0.0 the term vanishes and every unit
card prices as **pure cost** — `card_potential` is strictly negative for all
nine dealt unit cards under all three vectors (Swordsmen −6.51 / −8.92 /
−1.38; Air Forces −16.07 / −21.31 / −3.24 at 2p/3p/4p). `row_pressure` skips
any card whose `card_potential` is ≤ 0, so a unit in the row is invisible to
`row_urgency` and `row_bargain_forgone`, and one in hand *lowers*
`hand_potential`.

**Could anything lift it off zero?** Barely. `experiments/hillclimb.mutate`
perturbs a weight by `gauss(0, s) · (|w| + 0.15)`, so the 0.15 floor means a
weight at zero *can* move — by about 0.04 per touched mutation at
σ = 0.25 — but it is not a multiplicative escape, and `rescale`, which is 12%
of all operators, multiplies: 0 × anything is 0. Measured over the three arms'
full generation logs — **1,757 generations (72 + 1,315 + 370), 216 of them
accepted** — `unit_strength_credit` appears in an accepted mutation **exactly
once**, on the 4p arm, and that move took it from 0.0 to **−0.0171**.
`territory_credit` moved once, `bonus_card_credit` and `defense_bonus` never.

Behavioural cost, per seat-game, red technology taken:

| | 2p | 3p | 4p |
|---|---|---|---|
| human | 3.84 | 2.79 | 3.43 |
| bot | 0.218 | 0.093 | 0.217 |
| factor | **17.6×** | **30.0×** | **15.8×** |

Verdict **(b)**. A concurrent lane is fixing this; nothing here touches
`engine/bots/weighted.py`.

### 3.2 `defense_bonus` — the three Military Bonus cards

`_BONUS_TO_FEATURE` maps `defenseBonus` → `defense_bonus` and
`colonizationBonus` → `colonize_bonus`. `bonus_card_credit` is 1.0 on every
vector — but it *multiplies those two weights*, and:

| weight | 2p | 3p | 4p |
|---|---|---|---|
| `defense_bonus` | 0.0 | 0.0 | absent → 0.0 |
| `colonize_bonus` | 0.0 | 0.042 | −0.074 |

So at 2p the whole bonus class prices at exactly 0.0 no matter what
`bonus_card_credit` says, and `defense_bonus` is 0.0 on **every** vector and
was never once moved by an accepted mutation in 1,757 generations. Verdict
**(b)** — but read the behavioural half before deciding what it is worth:

| 2p, per seat-game | human | bot |
|---|---|---|
| bonus card spent **as defence** | 0.397 | **0.375** |
| bonus card spent **as colonization** | 1.741 | not separable (see below) |

The defence half is **not** a behavioural blind spot: the bot spends a bonus
card in 0.375 of its 0.512 defence-card spends, essentially the human rate,
because `("defend", card)` is a real move and the 1-ply evaluator sees the
*resolved* defence rather than the card's price. `defense_bonus` at 0.0 only
costs the bot the ability to **value holding** one — which matters for hand
valuation and military discards, not for the spend. The colonization half is
consumed automatically by `interact.force_value` when the engine assembles a
sacrifice, so it is not a bot decision at all and cannot be counted on the bot
side; the visible consequence is that the bot holds 0.54 colonies a
seat-game at 2p against a human 1.51.

This is the honest correction to the first reading of this row: the census's
`bonus 0.000` line at 3p/4p is **partly an instrument gap** — the `defend`
move was not captured until the 2p re-run — and only the 2p number should be
read as measured.

### 3.3 `free_civil_action` — the 18 action cards that grant one

| vector | value |
|---|---|
| `DEFAULT_WEIGHTS` | 0.0 |
| 2p champion | 0.0 |
| 3p ladder tip | −0.16007 |
| 4p ladder tip | −0.08449 |

Non-positive on all three: the bot is priced to *dislike* a card for granting a
free civil action. This is a third instance of the pattern and is reported
here for the first time. It has **no isolated behavioural signature** in this
census — action cards as a class are the bot's least-broken type (8.16 vs
12.98 at 2p, 9.62 vs 10.25 at 3p, 10.53 vs 9.61 at 4p) — so it is recorded and
ratcheted, not acted on. Verdict **(b)**, unmeasured cost.

## 4. Ranked discrepancy table

Worst 24 by 2p delta (per seat-game). Full table:
`python3 tools/play_rate.py report ...`.

| card (base) | type | 2p h | 2p bot | Δ | 3p h | 3p bot | 4p h | 4p bot | class |
|---|---|---|---|---|---|---|---|---|---|
| Urban Growth | action | 1.814 | 0.894 | −0.920 | 1.504 | 0.972 | 1.417 | 1.406 | (c) |
| Breakthrough | action | 1.357 | 0.463 | −0.894 | 1.023 | 0.972 | 0.903 | 0.844 | (c) |
| Military Bonus (def 4) | bonus | 0.920 | 0.106* | −0.814 | 0.679 | 0.000 | 0.831 | 0.000 | **(b)** §3.2 |
| Iron | mine | 0.786 | 0.013 | −0.774 | 0.539 | 0.000 | 0.719 | 0.000 | (c) §5.1 |
| Irrigation | farm | 0.816 | 0.081 | −0.735 | 0.459 | 0.056 | 0.452 | 0.125 | (c) §5.1 |
| Engineering Genius | action | 1.482 | 0.762 | −0.719 | 1.063 | 0.463 | 0.926 | 0.844 | (c) |
| Cannon | artillery | 0.705 | 0.013 | −0.693 | 0.434 | 0.000 | 0.516 | 0.062 | **(b)** §3.1 |
| Revolutionary Idea | action | 1.118 | 0.419 | −0.699 | 0.820 | 0.889 | 0.702 | 0.719 | (c) |
| Military Bonus (def 6) | bonus | 0.630 | 0.006* | −0.624 | 0.439 | 0.000 | 0.512 | 0.000 | **(b)** §3.2 |
| Air Forces | air | 0.653 | 0.031 | −0.622 | 0.321 | 0.009 | 0.555 | 0.031 | **(b)** §3.1 |
| Knights | cavalry | 0.640 | 0.025 | −0.615 | 0.409 | 0.000 | 0.566 | 0.031 | **(b)** §3.1 |
| Alchemy | lab | 0.601 | **0.000** | −0.601 | 0.479 | **0.000** | 0.602 | **0.000** | (c) §5.1 |
| Computers | lab | 0.619 | 0.025 | −0.594 | 0.436 | 0.000 | 0.462 | 0.000 | (c) §5.1 |
| Reserves | action | 2.048 | 1.456 | −0.592 | 1.717 | 1.574 | 1.519 | 1.500 | (c) |
| Military Bonus (def 2) | bonus | 0.588 | 0.075* | −0.513 | 0.581 | 0.000 | 0.735 | 0.000 | **(b)** §3.2 |
| Swordsmen | infantry | 0.609 | 0.044 | −0.565 | 0.253 | 0.009 | 0.352 | 0.000 | **(b)** §3.1 |
| Frugality | action | 0.848 | 0.350 | −0.498 | 0.732 | 0.806 | 0.757 | 1.031 | (c) |
| Scientific Method | lab | 0.397 | **0.000** | −0.397 | 0.351 | **0.000** | 0.344 | **0.000** | (c) §5.1 |
| Efficient Upgrade | action | 1.092 | 0.713 | −0.379 | 1.025 | 0.778 | 0.905 | 1.000 | (c) |
| Cavalrymen | cavalry | 0.389 | 0.019 | −0.371 | 0.283 | **0.000** | 0.253 | **0.000** | **(b)** §3.1 |
| Pyramids | wonder | 0.361 | 0.013 | −0.349 | 0.256 | **0.000** | 0.242 | **0.000** | (c) |
| Medieval Army | tactic | 0.375 | 0.031 | −0.344 | 0.216 | 0.019 | 0.403 | 0.062 | (c) §5.3 |
| Rich Land | action | 1.137 | 0.819 | −0.318 | 0.962 | 0.583 | 0.968 | 0.906 | (c) |
| Engineering | special-tech | 0.353 | 0.056 | −0.296 | 0.228 | 0.056 | 0.206 | 0.219 | (c) |

\* The three Military Bonus rows are diluted twice over: the `defend` move was
only captured on 80 of the 160 2p seat-games (§3.2), and the human number folds
in a colonization use the bot side cannot count at all.  On the 80 seat-games
that did carry it the bot plays a bonus card 0.375 times a seat-game against a
human **defence-only** rate of 0.397.  Read §3.2 before reading these three
rows as a behavioural gap.

### The inverse: cards the bot plays far MORE than humans

| card (base) | type | 2p h | 2p bot | Δ | 3p h | 3p bot | 4p h | 4p bot | class |
|---|---|---|---|---|---|---|---|---|---|
| Printing Press | library | 0.175 | 0.863 | +0.688 | 0.291 | 0.509 | 0.200 | 0.031 | (c) §5.1 |
| Opera | theater | 0.214 | 0.887 | +0.674 | 0.258 | 0.648 | 0.253 | 0.000 | (c) §5.1 |
| Movies | theater | 0.317 | 0.881 | +0.564 | 0.386 | 0.657 | 0.344 | 0.000 | (c) §5.1 |
| Multimedia | library | 0.332 | 0.856 | +0.524 | 0.338 | 0.074 | 0.345 | 0.031 | (c) §5.1 |
| Patriotism | action | 0.746 | 1.200 | +0.454 | 0.439 | 1.157 | 0.566 | 0.969 | (c) |
| Organized Religion | temple | 0.390 | 0.819 | +0.429 | 0.271 | 0.009 | 0.315 | 0.000 | (c) §5.1 |
| War over Culture | war | 0.160 | 0.569 | +0.408 | 0.105 | 1.009 | 0.112 | 1.062 | (c) §5.2 |
| Drama | theater | 0.122 | 0.500 | +0.378 | 0.341 | 0.657 | 0.207 | 0.062 | (c) §5.1 |
| Aggression: Raid | aggression | 0.168 | 0.537 | +0.370 | 0.113 | 0.231 | 0.157 | 0.000 | (c) |
| Theocracy | government | 0.085 | 0.450 | +0.365 | 0.098 | 0.333 | 0.055 | 0.250 | (c) |
| Theology | temple | 0.115 | 0.438 | +0.323 | 0.188 | 0.046 | 0.226 | 0.031 | (c) §5.1 |
| Taj Mahal | wonder | 0.104 | 0.406 | +0.302 | 0.118 | 0.056 | 0.138 | 0.031 | (c) |
| Journalism | library | 0.195 | 0.469 | +0.274 | 0.316 | 0.074 | 0.327 | 0.000 | (c) §5.1 |
| Mahatma Gandhi | leader | 0.043 | 0.312 | +0.269 | 0.103 | 0.287 | 0.112 | 0.250 | (c) |
| Fighting Band | tactic | 0.313 | 0.475 | +0.162 | 0.486 | 0.870 | 0.421 | 0.844 | (c) §5.3 |
| Warfare | special-tech | 0.086 | 0.263 | +0.177 | 0.193 | 0.648 | 0.210 | 0.500 | (c) |

The over-plays cluster the same way the under-plays do: **blue urban buildings
and the Age III culture wonders at 2p, wars and the cheapest Age I tactic
everywhere.** `War over Culture` at 3.6× / 9.6× / 9.5× the human rate is the
single largest over-play in the game and is the card-level restatement of
`docs/SYSTEM_COVERAGE.md`'s "wars declared 2.2× / 6.6× / 7.9× over".

## 5. Cards the bot never plays

**Never touched at ANY table size** (out of 230 dealt cards):

| card | type | 2p h | 3p h | 4p h |
|---|---|---|---|---|
| **Alchemy** | lab (II) | 0.601 | 0.479 | 0.602 |
| **Scientific Method** | lab (III) | 0.397 | 0.351 | 0.344 |
| **Coal** | mine (II) | 0.189 | 0.434 | 0.315 |

Three cards, all yellow production, all bought by a human roughly once every
two or three seat-games. Distinct cards touched: **214 / 230 at 2p, 205 / 230
at 3p, 186 / 230 at 4p.**

Never touched at one table size but not another (dealt cards only; pacts are
excluded at 2p because RULES_SPEC §13 removes them from the 2p deck):

* **2p (16):** Alchemy, Coal, Scientific Method, Sid Meier, Developed
  Territory (I), Inhabited Territory (I), and the ten pact cards.
* **3p (25):** the three above, plus **seven more yellow/red technologies**
  (Cannon, Cavalrymen, Computers, Iron, Knights, Oil, Riflemen), the **three
  Military Bonus cards**, **five tactics** (Entrenchments, Fortifications,
  Napoleonic Army, Phalanx, Shock Troops), **four wonders** (Colossus, Hanging
  Gardens, Library of Alexandria, Pyramids) and three pacts.
* **4p (44):** the 3p list plus six of the eleven aggressions, eight of the
  sixteen wonders (Eiffel Tower, Fast Food Chains, First Space Flight, Great
  Wall, Hollywood, St. Peter's Basilica, Universitas Carolina and the two
  above), War over Technology, and every library/theater card. At n = 8 games
  the 4p list is as much a statement about sample size as about the policy.

### 5.1 Why yellow is dead and blue is doubled — class (c), and probably wrong

Every yellow production technology in the game prices **strictly negative**
under all three trained vectors:

| card | 2p | 3p | 4p |
|---|---|---|---|
| Irrigation (farm II) | −4.02 | −5.54 | −0.43 |
| Iron (mine II) | −6.72 | −14.42 | −1.69 |
| Alchemy (lab II) | −11.19 | −17.00 | −2.20 |
| Scientific Method (lab III) | −15.06 | −22.78 | −2.88 |
| Computers (lab III) | −20.41 | −31.25 | −3.82 |

This is **not** the `unit_strength_credit` shape: `food_rate` (1.94 / 2.95 /
0.63), `resource_rate` (1.79 / 0.06 / 0.21) and `science_rate` (0.25 / 0.03 /
0.17) are all live and non-zero. The cards price below zero because the *cost*
side out-weighs them — `_PROD_TO_FEATURE` sends a lab's output to
`science_rate` (0.25 at 2p) while its `techCost` is charged through `science`
(0.33 at 2p) — and because `culture_rate` on the 2p champion is **31.68**, i.e.
127× the science rate, so any card that produces culture beats any card that
produces science by construction. `row_pressure` then skips every card whose
`card_potential` is ≤ 0, so the yellow half of the row is invisible to
`row_urgency` for exactly the same *mechanical* reason units were — by a
different *cause*.

Labelled **(c)**: the weights are live and the hill climb chose this. It is
recorded here as the largest behavioural discrepancy in the game that is *not*
an inert weight, and `docs/EXPERT_STRATEGY.md`'s framing says a civilization
that buys no science and no resources is not a strategy the corpus supports.

### 5.2 Wars — class (c), already open

`War over Culture` at 0.57 / 1.01 / 1.06 against a human 0.16 / 0.11 / 0.11,
and `War over Technology` / `War over Territory` at zero on the bot side at
2p. The bot declares the one war it can evaluate and never the two it cannot.
`docs/WAR_OVER_TECHNOLOGY.md` and `docs/SYSTEM_COVERAGE.md` §4 own this;
nothing new is claimed here beyond the card identities.

### 5.3 Tactics — class (c)

The bot plays 1.57 / 2.21 / 2.13 tactic cards a seat-game against a human 2.12
/ 1.95 / 2.32, so the *class* is healthy, but the mix is not: it over-plays
`Fighting Band` (the cheapest Age I tactic) 1.5× / 1.8× / 2.0× and under-plays
`Medieval Army` 12× / 11× / 6×, and never plays Phalanx, Fortifications,
Entrenchments, Napoleonic Army or Shock Troops at 3p. `tactic_level` is live
(0.033 / 0.070 / 0.148) and `tactic_gain` is 0.111 / 0.0 / 0.052, so this is a
weighting question, not a blind spot.

### 5.4 What has no human baseline at all

* **Events (55 cards).** Nobody plays an event; it is prepared face down and
  revealed. The bot reveals 7.5 / 6.4 / 8.3 a seat-game. The journal names the
  revealed card but the choice being measured — which card to *prepare* — is
  face down in the corpus, so no rate exists to compare against.
* **Pacts (10 cards).** BGO prints `accepts pact offer` and never the card
  name. The bot's aggregate is 0.00 / 1.03 / 1.50 offers a seat-game, and the
  2p zero is the rulebook, not a gap. `docs/PACTS_DIAGNOSIS.md` owns this.
* **Colonization use of a Military Bonus card.** `interact.force_value`
  assembles the sacrifice; it is not a bot decision, so there is nothing to
  count on the bot side (§3.2).

## 6. What is now permanent

The standing rule in this project is that feedback gets encoded so the failure
cannot recur. This failure has now been missed by four audits, so:

**`tools/play_rate.py`** — every card's play rate against its human rate, in
one command, all three table sizes, in one table.

**`tests/test_play_rate.py`** — the check, in two halves:

*The cheap half, always in the suite (0.8 s, no games played).* A **class
gate** is derived mechanically, not declared: perturb each weight by +1.0 and
record which cards' `card_potential` moves; a weight whose influence set is
confined to a single card type is the ONLY per-card channel that type has.
Three assertions follow:

1. `test_every_derived_gate_is_written_down` — a newly-added class gate that
   defaults to 0.0 fails until it is listed with a reason. Every fresh league
   arm starts from `DEFAULT_WEIGHTS` (`champion_3p.json` is gen 0 and
   byte-for-byte the defaults today), so a gate at 0.0 there is a card class
   every new arm begins blind to.
2. `test_no_stale_entries` — a listed gate that is no longer one fails, so a
   write-off cannot outlive its reason.
3. `test_the_dead_set_has_not_grown` / `..._gone_stale` — over every trained
   vector on disk, a gate that is zero-or-wrong-signed on **all** of them must
   be in `DEAD_ON_EVERY_TRAINED_VECTOR`, and one that stops being so must come
   out. Today that set is exactly `{unit_strength_credit, defense_bonus,
   free_civil_action}` (§3). "Wrong-signed" is measured against the default's
   sign, so `yellow_bank` — a class gate for the twelve territories that
   defaults to −0.1 because a drained bank is a cost — is not miscounted as
   dead at −0.747.

*The expensive half, behind `PLAY_RATE_CENSUS=1`.* Runs a real 12-game 2p
census and fails if any card **type** falls below **one eighth** of its human
take rate. The factor is set from the measured data: the failure this file
exists for is a factor of 6–47 (units), 65 (labs) and 24 (mines), while no
class the bot plays acceptably is within a factor of 3 of the bar. It is a
deliberately loose test — the bot is not required to play like a human, only
to be within an order of magnitude of one on a whole card class. It is off by
default because 12 games at `plan:width=2` is ~8 minutes, which does not
belong in a suite that runs on every commit.

The cheap half is the one that would have caught `unit_strength_credit` on the
day it shipped, without playing a single game.

## 7. Open

* The 4p column is 8 games. It agrees with 2p and 3p on every sign, but no 4p
  number here should be quoted as a magnitude.
* `free_civil_action` (§3.3) is a measured inert weight with no isolated
  behavioural signature. Whether it costs anything is unmeasured.
* The yellow-technology collapse (§5.1) is the largest non-inert discrepancy
  in this document and has no owner. The mechanism is identified
  (`culture_rate` 31.68 against `science_rate` 0.25 on the 2p champion, and a
  cost model that charges `techCost` at a higher weight than the output it
  buys); whether that is a pricing bug or a real 2p optimum is not settled
  here.
* The bot's colonization use of Military Bonus cards is unmeasurable through
  the bot's own move stream (§3.2). Instrumenting `interact.force_value` would
  close it.
