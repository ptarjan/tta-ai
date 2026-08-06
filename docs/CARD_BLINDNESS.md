# What the evaluator cannot see on a card (2026-07-29)

This started as a narrower question — "every wonder is the same card to the
evaluator, fix that" — and the narrow version turned out to be false. The
evaluator does read each card's `production` and `effects` blocks and map them
onto features. What it does is drop, silently, every key those two tables do
not name.

Two of the dropped keys were `culture` and `science`.

## One-paragraph answer

`engine/bots/weighted.py:_card_yields` prices a card by looking its
`production` and `effects` keys up in `_PROD_TO_FEATURE` / `_EFF_TO_FEATURE`.
`_EFF_TO_FEATURE` had no entry for `culture` or `science`, and ten cards spell
their per-turn culture that way — Eiffel Tower (4), Taj Mahal (3), St. Peter's
Basilica (2), Kremlin (2), Library of Alexandria, Universitas Carolina, Great
Wall, Hanging Gardens, Joan of Arc, Mahatma Gandhi — with two more spelling
science that way. **Seven of the sixteen wonders therefore priced out at
nothing beyond "it is a wonder", including the two the tournament data likes
best.** Across all 236 cards, 168 had zero visible gain. This document adds
the missing mappings, adds a test that makes the next such omission fail
instead of costing a season of training, writes down the size of the blind
spot that remains, and measures what the fix is worth: **+9.5pp win rate and
+10 culture margin at 2p over 3200 paired games, and it transfers to 3p
(+5.5pp on a 33.3% null, n=900).** The wonder finish-discipline features that
this work started out being about are a **flat null** and one of the three is
a near-dead coordinate; §5.1 says so and explains why.

Everything here is base game (2015 "A New Story of Civilization").

## 1. The census

`tools/card_blindness.py` walks the card DB, asks `_card_yields` what it can
see, and counts what fell on the floor. "zero visible gain" means it produced
no non-cost pair at all, *excluding* the generic `("wonders", 1.0)` every
wonder gets just for being a wonder — that term cannot tell Pyramids from
Colossus, so counting it would conceal the thing being counted.

```
python3 -m tools.card_blindness --legacy    # master
python3 -m tools.card_blindness             # this branch
```

> **CORRECTION, 2026-07-29 ([`docs/EVENT_SEEDING.md`](EVENT_SEEDING.md)).** The table below
> originally pooled both decks, and that **over-reported the blind spot by
> 109 cards.** `_card_yields` is reached only through `card_potential` ←
> `hand_potential`, and `hand_potential` walks `p.hand_civil` ONLY, so it is
> **never called for a military-deck card** — every event, aggression, war,
> pact, tactic, territory and bonus. For those, a "dropped key" is not a
> finding: mapping it changes nothing, because nothing ever asks. Reading the
> old pooled rows as "these 109 cards are unpriced" sent a work stream after
> table entries that could not have helped. The tool now prints the two decks
> separately and so does this table. An over-reporting measurement is exactly
> as dangerous as an under-reporting one.

**Civil row — `_card_yields` is asked about these.** This is the real census.

| card type | n | with a dropped key (master → now) | zero visible gain (master → now) |
|---|---|---|---|
| action | 33 | 28 → 3 | 19 → 3 |
| leader | 24 | 24 → 24 | 17 → 16 |
| wonder | 16 | 15 → 8 | **7 → 5** |
| special-tech | 12 | 6 → 0 | 3 → 0 |
| government | 8 | 0 → 0 | 4 → 4 |
| units (infantry/cavalry/artillery/air) | 10 | 1 → 1 | 10 → 10 |
| farm/mine/lab/temple/library/arena/theater | 24 | 0 → 0 | 0 → 0 |
| **SUBTOTAL** | **127** | **74 → 36** | **60 → 38** |

The "now" column moves as each lane lands; regenerate rather than trust it —
`python3 -m tools.card_blindness` and `--legacy` for the master column. With
`--board` (the board-aware evaluator of [`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md)
counted too) the totals are **125 dropped / 129 zero-gain**.

**Military deck — `_card_yields` is never asked.** These rows are recorded for
completeness. They are *not* a measure of how well the bot values these cards,
because the tool cannot see where they are actually priced.

> **CORRECTION, 2026-07-30 ([`docs/COMBAT_AUDIT.md`](COMBAT_AUDIT.md)).** "Never asked" is true of
> `DEFAULT_WEIGHTS` and no longer true in general. `hand_mil_potential` walks
> `p.hand_military` and calls `card_potential` → `_card_yields` on every card
> in it, so any vector carrying a non-zero `hand_mil_potential` **does** ask —
> the live 3p league champion carries 0.01079. That is how the territory row
> below got priced, and it is why the `bonus` row's "genuinely unpriced" had
> quietly become a leftover write-off rather than a limitation. Run
> `tools/conduction_table.py` on the vector you care about before reading this
> block as a statement about it.

| card type | n | dropped key | "zero visible gain" | where it is really priced |
|---|---|---|---|---|
| event | 55 | 55 | 55 | Age III scoring events: `weighted.event_scoring_margin` → `events.final_event_culture`. The other 40 deliberately unpriced, reasons in EVENT_SEEDING §6 |
| tactic | 15 | 15 | 15 | **genuinely unpriced** — needs a military sibling to `hand_potential`, not a table entry |
| territory | 12 | 0 | 12 | `deferred_credit`'s auction branch |
| aggression | 11 | 11 | 10 | quiescence: the defender's `defense` pending is drained and the quiet position scored |
| pact | 10 | 10 | 10 | `deferred_credit`; and `count 2p: 0`, so absent from 2p entirely |
| bonus | 3 | 3 | 3 | ~~genuinely unpriced~~ → `_BONUS_TO_FEATURE`: `defenseBonus−1` (the increment over the +1 any face-down card is worth) and `colonizationBonus` → `colonize_bonus`, both derived from `engine/interact.py`. [`docs/COMBAT_AUDIT.md`](COMBAT_AUDIT.md) |
| war | 3 | 3 | 3 | `quiescent.war_value` → the engine's own `events.resolve_war` |
| **SUBTOTAL** | **109** | **97** | **108** | |

| **TOTAL (both decks)** | **236** | **133** | **146** |
|---|---|---|---|

The pattern is crisp and worth stating on its own: **the evaluator prices a
card correctly when the card is a bag of numbers, and not at all when its
value is written in prose.** Every farm, mine, lab, temple, library, arena and
theater — the 24 cards whose whole content is a production number — is priced
exactly right.

The wonders in full, before and after:

| wonder | master saw | now sees | still dropped |
|---|---|---|---|
| Pyramids | `civil_actions+1` | `civil_actions+1` | — |
| Hanging Gardens | `happy_margin+2` | `culture_rate+1, happy_margin+2` | — |
| Colossus | `strength+2` | `strength+2, colonize_bonus+1` | — |
| Library of Alexandria | **nothing** | `culture_rate+1, science_rate+1, hand_limit+2` | — |
| Great Wall | `happy_margin+1` | `culture_rate+1, happy_margin+1` | `strengthPerInfantry`, `strengthPerArtillery` |
| St. Peter's Basilica | `happy_margin+1` | `culture_rate+2, happy_margin+1` | `extraHappyPerHappySource` |
| Universitas Carolina | **nothing** | `culture_rate+1, science_rate+2` | — |
| Taj Mahal | `blue_free+1` | `culture_rate+3, blue_free+1` | — |
| Transcontinental Railroad | `strength+4` | `strength+4` | `doubleBestMine` |
| Eiffel Tower | `happy_margin+1` | `culture_rate+4, happy_margin+1` | — |
| Kremlin | `civil_actions+1, military_actions+1, happy_margin-1` | `culture_rate+2, ...` | — |
| Ocean Liners | **nothing** | **nothing** | `freePopIncreasePerTurn` |
| First Space Flight | **nothing** | **nothing** | `onBuildCulturePerTechLevelSum` |
| Fast Food Chains | **nothing** | **nothing** | `onBuildCulture` |
| Internet | **nothing** | **nothing** | `onBuildCulture` |
| Hollywood | **nothing** | **nothing** | `onBuildCulture` |

Five wonders are still worth nothing but their wonder-ness, and all five are
text-effect cards — the four Age III wonders score by a formula over your
board (`"2*workers(farm,mine)+1*workers(urban,military)"`) and Ocean Liners'
entire card is `freePopIncreasePerTurn: True`. A lookup table cannot price
those; a board-aware card evaluator could, and that is the obvious next piece
of work rather than something to fake here.

### The keys, by how many cards carry them

Top of the dropped list on master, with how many of those values are
non-numeric (i.e. `True`, a string, or a nested block):

| key | cards | non-numeric |
|---|---|---|
| `allPlayers` | 39 | 39 |
| `freeCivilAction` | 18 | 18 |
| `tacticBonus` | 15 | 0 |
| `resourceDiscount` | 13 | 0 |
| `culture` | 10 | 0 |
| `tacticBonusObsolete` | 10 | 0 |
| `note` | 10 | 10 |
| `weakestPlayer` / `strongestPlayer` | 6 each | all |
| `colonizeBonus` | 4 | 0 |
| `buildDiscount` | 3 | 3 (dict) |
| `wonderStagesPerAction` | 3 | 0 |
| `science` | 2 | 0 |

Full list: `python3 -m tools.card_blindness --legacy --keys`.

## 2. What was changed

### 2.1 The two mappings that are a straight omission

```python
"culture": "culture_rate",
"science": "science_rate",
```

They map to the **rate** features, not the stock ones, and that is not a
judgement call: `engine/effects.py:FLAT_KEYS` sends an effect-block `culture`
to the same `Stats` slot as `cultureProduction`, so the engine already treats
Eiffel Tower's 4 as four culture *per turn*. `tests/test_card_pricing.py`
asserts that agreement directly, so the two files cannot drift.

Note what this does *not* fix, because it bounds the whole result: the board
side was never blind. `features()["culture_rate"]` reads `Stats.culture`,
which includes these effects. A completed Eiffel Tower has always been
visible. The blindness was confined to `card_potential`, i.e. to **valuing a
card in hand or in the row** — deciding what to take and what to play.

### 2.2 Nine new weights, all defaulting to 0.0

Four for wonder finish discipline, five for effect keys that had nowhere to
land:

| weight | what it measures |
|---|---|
| `wonder_stages_left` | unbuilt stages, i.e. civil actions still owed |
| `wonder_turns_to_finish` | `(remaining resources − banked) / resource production`, capped at 20: turns of your *entire* output the wonder still needs |
| `wonder_overrun` | `max(0, wonder_turns_to_finish − rounds_left)` — the part the game will not last long enough to pay |
| `wonder_stages_per_action` | Masonry and friends: stages per build action above the base of one |
| `hand_limit` | `civilHandLimit` + `militaryHandLimit` (Library of Alexandria) |
| `colonize_bonus` | `colonizeBonus` (Colossus, Cartography, …) |
| `build_discount` | `buildDiscount` summed over ages (Masonry) |
| `free_civil_action` | presence of a `freeCivilAction` rider — 18 cards |
| `resource_discount` | the `resourceDiscount` on those same cards |

The first three are the finish-discipline term. They are 0.0 with no wonder in
progress and drop back to 0.0 the instant it completes, so a negative weight
on any of them prices **starting** (and stalling), and finishing is what
removes the penalty. That is the shape [`docs/HEURISTICS.md`](HEURISTICS.md) asks for — "start
a wonder by round 12 or do not start it" — expressed as something the league
can tune rather than a hard rule. They are deliberately **not** given a
negative prior despite the evidence pointing that way (0 for 58 on the three
12-resource Age II wonders), because a negative default would also put them
into `hillclimb_league`'s `NONPOS` set and forbid the climber from ever
discovering that a wonder programme is worth having.

Where a key has both a card side and a board side, both use the same weight,
the way `civil_actions` already does.

### 2.3 One weight that is not 0.0

`card_rate_credit`, default **1.0**: how much of the newly-visible
`effects.culture` / `effects.science` to believe. This is the only
behaviour-changing default in the change. It is a weight rather than a
hard-coded mapping for one reason: **0.0 recovers master's pricing exactly**,
which is what makes §5's A/B a paired, same-process duel instead of a
comparison across two builds.

### 2.4 The guardrail, which matters more than any single mapping

`tests/test_card_pricing.py` walks all 236 cards and fails if any key in a
`production` or `effects` block is neither mapped nor listed in an explicit
`DELIBERATELY_UNPRICED` set with a written reason. It also fails on a stale
entry (a written-off key no card carries any more), on a reason shorter than
20 characters, and on a mapping that points at a feature `DEFAULT_WEIGHTS`
does not contain — which would be the same silent-drop failure one level down.

The point is not that the blind spot is empty. It cannot be. The point is that
its **size is visible**. `culture` sat in it for most of this project's life
and nothing could see it.

Negative control, run before trusting it: removing the `culture`/`science`
mappings again makes the suite fail with 3 failures and 1 error, naming the
cards.

## 3. The blind spot that remains, written down

28 effect keys are now mapped and **95 are written off**, in seven buckets.
This is the honest inventory:

1. **board-scaled** (16 keys) — a numeric coefficient times a board count.
   `strengthPerInfantry`, `extraHappyPerHappySource`,
   `culturePerHappyFromTemplesTheatersWonders`, … `_card_yields` is
   `lru_cache`d on the card name alone and has no state, so it cannot evaluate
   any of them.
2. **text effect** (13 keys) — the value is a formula or a bare `True`. There
   is no number to multiply a weight by. This is what keeps the four Age III
   wonders and Ocean Liners at zero.
3. **rule change** (20 keys) — makes something legal, illegal or cheaper.
   Gandhi's aggression immunity is here, and [`docs/HEURISTICS.md`](HEURISTICS.md) already
   flags that the bot cannot value it.
4. **trigger** (14 keys) — pays out per future event, so the printed number is
   a rate, not a yield. Einstein's 3 culture per technology and Newton's civil
   action refund are here; pricing them needs a model of how often the trigger
   fires.
5. **military hand** (12 keys) — `hand_potential` walks `hand_civil` only, so
   `_card_yields` is *never called* for a tactic, war, aggression, territory
   or bonus card. Mapping `tacticBonus` today would change nothing. See §6.
   *(Superseded for two of the twelve: `hand_mil_potential` does call it, so
   `defenseBonus`/`colonizationBonus` were mapped rather than written off —
   [`docs/COMBAT_AUDIT.md`](COMBAT_AUDIT.md). `tacticBonus` stays unpriced, but for the sharper
   reason recorded in `DELIBERATELY_UNPRICED`: it is a duplicate spelling of
   the top-level `strength` the engine actually reads.)*
6. **addressing** (19 keys) — `allPlayers`, `weakestPlayer` and friends name
   *who* an event or pact side applies to. Pacts are already priced by
   `deferred_credit`, which reads inside these blocks.
7. **prose** — `note`.

## 4. Method for §5

`experiments/arena.duel` at 2 players plays each deal twice with the seats
swapped, so the comparison is paired on the deal. Both arms are the **same
weight vector** — `analysis/frozen/champion_2p.json` — differing only in
`card_rate_credit` (1.0 vs 0.0). Nothing else about the two bots differs, so
the duel isolates the mapping.

Plain `WeightedBot` 1-ply, which is how `hand_potential` was measured
(`engine/bots/weighted.py`, the block above `_card_yields`) and is the
cheapest thing that exercises `card_potential`. Note the 2p league actually
trains `plan:width=2`; this result is for the 1-ply bot.

**MDE.** n = 3200 games gives SE ≈ 0.88pp on the win rate, so the minimum
detectable effect at 80% power and α = 0.05 two-sided is **≈ 2.5pp**. Pairing
on the deal makes the true SE somewhat smaller than that, so 2.5pp is
conservative. An effect below ~2.5pp is not something this experiment can
speak to.

> That last sentence was right, and §10 measures how right: the deal-clustered
> SE here is **0.66pp**, not 0.88pp, for a true MDE of **1.86pp**. The habit of
> calling the naive figure "conservative" and moving on is exactly what let the
> same formula be wrong in the *other* direction elsewhere in the project
> without anyone noticing.

How large is the change in the quantity being fed to the search? Under the 2p
champion's own weights, `card_potential` moves:

| card | credit 0.0 | credit 1.0 |
|---|---|---|
| Eiffel Tower | 3.95 | **27.45** |
| Taj Mahal | 4.32 | 21.95 |
| Library of Alexandria | 4.74 | 14.81 |
| Pyramids | 6.11 | 6.11 |
| Colossus | 5.04 | 5.04 |
| Philosophy | 3.33 | 3.33 |

so this is not a tie-breaker-sized perturbation; it reorders the row.

## 5. Result

**Not a null.** Mapping `effects.culture` and `effects.science` is worth about
**+9.5 percentage points** of win rate and **+10 culture of margin** to the
frozen 2p champion, against the identical vector without them.

| | n | win rate (paired) | culture margin | own culture |
|---|---|---|---|---|
| `card_rate_credit` 1.0 vs 0.0 | **3200 games / 1600 deals** | **59.53% ± 1.30pp** (z = 14.4) | **+10.39 ± 1.15** (z = 17.8) | 150.8 vs 140.4 |

Both intervals above are **deal-clustered** and were re-derived from the raw
`per_game` arrays on 2026-07-30 during the audit in §10. They came through
unchanged, to four significant figures, which is the good outcome and not the
usual one — §10 lists the numbers elsewhere in this project that did move. For
reference, the independent-samples formula on the same 3200 games would have
reported **±1.69pp (z = 11.1)** on the win rate and **±1.54 (z = 13.2)** on the
margin: *wider* than the truth, because the pairing is doing real work here.

Eight independent blocks of 400 on disjoint deals, so the consistency is
checkable rather than assumed:

| block | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 |
|---|---|---|---|---|---|---|---|---|
| win rate | 59.6% | 57.4% | 61.5% | 58.5% | 60.0% | 62.3% | 57.1% | 59.9% |

Every block is on the same side and the spread (57.1–62.3) is what 400 games
of binomial noise looks like. No single block is carrying the result.

I went into this expecting a null and said so in advance, for a specific and
good reason: [`docs/SCORE_AUDIT.md`](SCORE_AUDIT.md#1062-the-scripted-ab-forcing-wonders) §10.6.2 measured that *forcing* wonders
on the strongest vector cost **34.3 ± 7.0 margin**, which is a real warning
that wonders may be correctly avoided in the suppressive equilibrium these
bots play. That warning is not contradicted here, and the reason is worth
being precise about:

* §6.2 forced the bot to **build** wonders — largest legal stage, leftmost
  wonder in the row, whether or not it could finish. It measured what happens
  when you override the policy.
* This measures what happens when you **tell the policy the truth about what a
  card produces** and let it decide. Nothing here forces a wonder, or even
  mentions wonders: `effects.culture` is also on Joan of Arc and Gandhi, and
  the change reprices the whole civil row.

Those are compatible. "A crude wonder programme is worse than what this bot
does instead" and "the bot was mispricing every card that spells its culture
production the short way" can both be true, and appear to be.

### What this does not establish

* **It is 1-ply `WeightedBot`.** The 2p league trains `plan:width=2`. A deeper
  search has more chances to discover the value of a card through rollout
  rather than through the leaf evaluation, so the effect could shrink there.
  Untested.
* **The champion was trained blind.** Its 78 weights (the Python evaluator's
  count at the time) were fitted while
  `card_potential` under-reported these cards. Feeding it better information
  helps immediately, but the *right* comparison — a champion retrained with
  the fix against a champion retrained without it — needs a league run and is
  not in this document.
* **It is not evidence that the tier-list question matters.** See §6.
* **It does not show the mechanism is wonders.** The change reprices ten
  cards, eight of them wonders and two of them leaders, and this experiment
  cannot attribute the 9.5pp among them. **§5.3 closes this and the answer is
  no: it is the two leaders, and wonders completed is a null.** §5.3 also
  corrects "six wonders" to eight, and corrects the reading of the +10 culture
  figure above — that is a duel margin, and own-culture production is +2.58.

### 5.1 Finish discipline: a null, and the reason is more interesting than the null

The three finish-discipline features default to 0.0, so they are inert until
someone gives them a weight. A coarse hand scan on top of the fixed pricing,
n = 400 per arm, each against `card_rate_credit=1.0` with everything else
identical:

| arm | win rate | own culture vs rival |
|---|---|---|
| `wonder_overrun` = −0.5 | 49.8% ± 4.9% (p=0.92) | 144 vs 145 |
| `wonder_overrun` = −1.5 | 49.8% ± 4.9% (p=0.92) | 144 vs 145 |
| `wonder_overrun` = −4.0 | 49.8% ± 4.9% (p=0.92) | 144 vs 145 |
| `wonder_turns_to_finish` = −0.3 | 50.0% ± 4.9% (p=1.00) | 144 vs 145 |
| `wonder_turns_to_finish` = −1.0 | 49.9% ± 4.9% (p=0.96) | 143 vs 145 |

**Flat null on every arm.** Note the three `wonder_overrun` rows are not
merely similar, they are *bit-identical* — same win rate, same cultures, at
weights differing by a factor of eight. Different weights cannot produce
identical games unless the term never changes a decision, so that is a
stronger statement than "no effect": at every weight tried, `wonder_overrun`
**never once won an argmax in 1200 games.**

`tools/feature_variance.py` says why. Over 984 decisions of 2p self-play under
this champion, per feature: `varying` is the fraction of decisions where the
feature differs across the candidate moves at all, and `mean_range` is the
mean spread across candidates — a feature can only change a decision through
`weight × range`.

| feature | weight | varying | mean_range |
|---|---|---|---|
| `hand_value` | 0.306 | 0.792 | 3.256 |
| `culture_rate` | 5.876 | 0.559 | 0.977 |
| `wonder_remaining` | −0.236 | 0.432 | 4.856 |
| **`wonder_turns_to_finish`** | 0.0 | **0.435** | **1.310** |
| **`wonder_stages_left`** | 0.0 | **0.432** | **1.518** |
| **`wonder_overrun`** | 0.0 | **0.037** | **0.241** |
| `wonder_stages_per_action` | 0.0 | **0.000** | 0.000 |
| `hand_limit` | 0.0 | **0.000** | 0.000 |
| `build_discount` | 0.0 | **0.000** | 0.000 |
| `colonize_bonus` | 0.0 | 0.003 | 0.006 |

Three separate findings there, and only the first is what I set out to test:

1. **`wonder_turns_to_finish` and `wonder_stages_left` are live features.** They
   differ across candidates at 43% of decisions with ranges of 1.3–1.5, which
   is the same order as `wonder_remaining`, an existing weight that does real
   work. The league genuinely can tune them. A one-dimensional hand scan
   simply did not find a value that helps, which is a much weaker statement
   than "they cannot help" — the climber moves ~90 weights jointly and this
   scan moved one at a time.
2. **`wonder_overrun` is very nearly a dead coordinate.** It fires at 3.7% of
   decisions with a range of 0.24, so even at −4.0 it contributes under 1.0
   against a `culture_rate` term contributing ~5.7. It was the feature I
   expected most from — it is the direct encoding of "start a wonder by round
   12 or do not start it" — and it is the one the evidence says is worthless
   as built. Being honest about which of my own three ideas died is the point
   of writing this down.
3. **Three of the newly-mapped keys are stone dead in 2p self-play**:
   `wonder_stages_per_action`, `hand_limit` and `build_discount` have
   `varying` = 0.000 exactly. The champion never takes Masonry or Library of
   Alexandria in these games, so those weights have no gradient at all and a
   hill climb will only drift them. **Giving a card a weight does not help
   until the bot takes the card**, and the census is the way to tell the
   difference. They cost nothing at 0.0 and are kept for the same reason the
   mapping is kept — so the information exists when the policy changes — but
   nobody should expect the league to find a value for them.

All three are left at 0.0.

### 5.2 3p: does it transfer?

[`docs/STRENGTH_CHECK.md`](STRENGTH_CHECK.md) is explicit that the tournament-derived 2p result did
not transfer to 3p, so this was worth checking rather than assuming.

**It transfers.** Same two weight files, 3 blocks of 300, challenger rotated
through all three seats, null 33.3%:

| block | 1 | 2 | 3 | **pooled** |
|---|---|---|---|---|
| win rate | 38.0% | 40.3% | 38.2% | **38.83% ± 2.53pp** (z = 4.28) |
| own culture vs rival | 170 / 162 | 177 / 167 | 172 / 165 | **172.9 / 164.8** |

> **Corrected 2026-07-30.** This row previously read **38.83% ± 3.18pp
> (z = 3.4)**, which was the independent-samples interval over 900 *games*.
> The 3p design deals each seed three times, rotating the challenger through
> all three seats, so the independent unit is the 300 *deals*. Deal-clustered:
> **±2.53pp, z = 4.28, p = 1.9e-05** (ρ = −0.18, block heterogeneity χ² = 0.68
> on 2 df — the three blocks agree). The point estimate does not move and the
> conclusion — it transfers — is unchanged and slightly better supported. See
> §10.

+5.5pp on a 33.3% null, all three blocks on the same side, and the same
culture story as 2p (+8 own culture). It is a smaller effect relative to its
null than the 2p result, which is the usual pattern, but it is not the BookBot
v2 outcome — that was *slightly negative* at 3p. This is not surprising on
reflection: a tier list is an opinion about which card is better, and opinions
formed in a 2p lobby need not hold at 3p, whereas "Eiffel Tower produces four
culture a turn" is a fact about the card that is true at every player count.

**4p is not measured.** The 4p champion is separately known to be degenerate
(`experiments/arena.py:DEGENERATE_CHAMPION_PATH`), so an A/B on it would be
hard to interpret and I did not run one.

### 5.3 The mechanism is not wonders, and the reason is a plumbing bug

§5 says explicitly that it cannot attribute the 9.5pp among the ten repriced
cards. This section closes that gap. **It is not the wonders.** Wonders
completed is a clean null; the entire take-side response is two leaders; and
the reason is structural, not a fact about wonders being weak.

Measured at commit `6968256`, 2p, frozen champion, engine read-only, three
runs, 12,800 games, zero engine errors.

> **CORRECTION (see [`analysis/frozen/README.md`](../analysis/frozen/README.md)). The wonder null below is an
> arithmetic identity, not a measurement.** This section is right that the
> mechanism is plumbing and right that it is not the wonders — but it reached
> that by the wrong route, and the quantitative null cannot be quoted.
>
> Below, this section correctly identifies `row_urgency` as *the single
> channel* by which a wonder's `card_potential` can reach the policy. **In the
> frozen champion that weight is 0.0**, and `evaluate()` gates the
> `row_pressure` call behind `if row_urgency or row_bargain_forgone:`. So the
> function is never called, and repricing the eight wonders cannot change the
> evaluation of any state. Measured directly: `evaluate()` credit1 vs credit0
> on states with a repriced wonder in the row differs on **0 of 480** under
> the frozen champion (max delta 0.0000) and on **480 of 480** under the live
> 2p league champion (max delta 21.1403).
>
> Consequences, precisely:
> * **"Wonders completed: a null … I could have detected an 8.5% relative
>   change" — withdraw the MDE.** A power calculation implies an effect that
>   could have existed. No sample size can move a coefficient multiplied by
>   zero. The 12,800 games measured nothing about wonder repricing.
> * **"take wonder −0.0444, p<1e-4" is real but is displacement, not
>   repricing.** The same table shows *every* card class falling; the bot
>   spends its freed actions on the two leaders. That is the only causal story
>   available, because the wonder arm of the treatment is inert.
> * **§5's +9.5pp / 59.53% headline is unaffected.** The ten repriced cards
>   are eight wonders and two leaders; leaders enter `hand_civil` and are
>   priced by `hand_potential` (0.125), which conducts fine. The headline was
>   always the two leaders. This section already said so.
> * **The conclusion "a correctly priced wonder has almost no path into the
>   decision" holds for this vector and is stronger than stated — it has
>   *none*. Whether it holds for the bot the league actually trains is a
>   separate question:** the live 2p champion carries `row_urgency = −0.19109`,
>   so the channel is open there. That re-run is reported in §5.4.

**The measurement stack validates against §5.** The probe reproduces the
published headline independently — **59.08% win rate and 151.0 vs 140.9
culture, against §5's 59.53% and 150.8 vs 140.4**, on the same generator but a
separately written harness. That agreement is worth a line of its own: it
means the wonder counts below come out of an apparatus that demonstrably
measures the same thing §5 measured.

#### The premise was wrong: eight wonders were repriced, not six

§1's table is right about what each wonder gained, but "six wonders newly
gained pricing" undercounts it, because four of the wonders that already had a
`happy_margin` or `blue_free` term *also* gained `culture_rate`. The checkable
version is the `card_potential` delta under the frozen champion's own weights:

| wonder | credit1 | credit0 | delta | group |
|---|---|---|---|---|
| Eiffel Tower | 27.45 | 3.95 | **+23.50** | repriced |
| Taj Mahal | 21.95 | 4.32 | **+17.63** | repriced |
| Universitas Carolina | 18.14 | 3.89 | **+14.25** | repriced (from nothing) |
| St. Peter's Basilica | 17.12 | 5.37 | **+11.75** | repriced |
| Kremlin | 15.61 | 3.86 | **+11.75** | repriced |
| Library of Alexandria | 14.81 | 4.74 | **+10.06** | repriced (from nothing) |
| Great Wall | 10.96 | 5.09 | **+5.88** | repriced |
| Hanging Gardens | 13.02 | 7.14 | **+5.88** | repriced |
| Pyramids | 6.11 | 6.11 | 0.00 | priced, unchanged |
| Colossus | 5.04 | 5.04 | 0.00 | priced, unchanged |
| Transcontinental Railroad | 3.64 | 3.64 | 0.00 | priced, unchanged |
| Ocean Liners | 3.03 | 3.03 | 0.00 | still unpriced |
| Internet | 2.47 | 2.47 | 0.00 | still unpriced |
| Hollywood | 1.90 | 1.90 | 0.00 | still unpriced |
| First Space Flight | 1.90 | 1.90 | 0.00 | still unpriced |
| Fast Food Chains | 1.90 | 1.90 | 0.00 | still unpriced |

A clean 8/8 split, and a better-balanced test than 6-vs-5. The two repriced
non-wonders are Mahatma Gandhi (+11.75) and Joan of Arc (+5.88); those ten
cards are the whole treatment.

#### Two designs, because the duel confounds contention

* **DUEL**, 3200 games: both arms at the same table, seat-rotated — the regime
  §5 measured in. The two arms *compete for the same wonder row*, so if one
  takes fewer wonders the other mechanically gets more, which inflates any
  difference.
* **MIRROR**, 3200 deals x 2 arms = 6400 games / 12,800 seat-games: every seat
  runs one arm, the two arms run on identical deals and bot seeds. No
  cross-arm contention. This is the honest behavioural number.

#### Wonders completed: a null

| design | credit1 | credit0 | diff (95% CI) | p | MDE @80% |
|---|---|---|---|---|---|
| duel | 0.1022 | 0.1156 | −0.0134 [−0.0290, +0.0021] | 0.090 | 0.0222 (19% rel) |
| mirror | 0.0997 | 0.1047 | −0.0050 [−0.0112, +0.0012] | 0.117 | **0.0089 (8.5% rel)** |

Distribution, mirror, 6400 seat-games per arm:

| wonders completed | credit1 | credit0 |
|---|---|---|
| 0 | 5765 (90.08%) | 5732 (89.56%) |
| 1 | 632 (9.88%) | 666 (10.41%) |
| 2 | 3 (0.05%) | 2 (0.03%) |
| 3+ | 0 | 0 |

**I could have detected an 8.5% relative change and did not see one.** The
point estimate is slightly *negative* in both designs.

#### Repriced vs unrepriced: both flat, and the unpriced set falls harder

Mirror, completions per seat-game:

| group | credit1 | credit0 | diff | p | rel |
|---|---|---|---|---|---|
| newly priced (6) | 0.0803 | 0.0850 | −0.0047 | 0.105 | −5.5% |
| from nothing (2) | 0.0159 | 0.0156 | +0.0003 | 0.806 | +2.0% |
| **all 8 repriced** | 0.0963 | 0.1006 | −0.0044 | 0.160 | −4.3% |
| still unpriced (5) | 0.0005 | 0.0003 | +0.0002 | 0.655 | 3 vs 2 events |
| priced, unchanged (3) | 0.0030 | 0.0037 | −0.0008 | 0.132 | −20.8% |
| **all 8 unrepriced** | 0.0034 | 0.0041 | −0.0006 | 0.317 | −15.4% |

Starts tell the same story with more counts: repriced −10.7% (p<1e-4),
unrepriced −16.4% (p=0.022). **The unpriced set — which by construction the
fix cannot have touched — falls *more*, not less.** That is the opposite of
the causal prediction. Not one of the eight repriced wonders rises in either
design; every one is flat or negative. Per-wonder tables are in the artifacts
listed in §7.

#### What actually moved: two leaders

All-takes duel, 3200 games:

| metric | credit1 | credit0 | diff | p |
|---|---|---|---|---|
| takes of the 10 repriced cards | 0.9022 | 0.2653 | **+0.6369** | <1e-4 |
| take wonder | 0.1419 | 0.1862 | **−0.0444** | <1e-4 |
| take leader | 1.8422 | 1.4103 | **+0.4319** | <1e-4 |
| leaders played | 1.8319 | 1.4059 | +0.4259 | <1e-4 |
| total card takes | 14.8159 | 15.6934 | −0.8775 | <1e-4 |

The top two movers in the entire 236-card database are the two repriced
leaders:

| card | credit1 | credit0 | diff | p | ratio |
|---|---|---|---|---|---|
| **Joan of Arc** | 0.6356 | 0.1081 | +0.5275 | <1e-4 | **5.9x** |
| **Mahatma Gandhi** | 0.1434 | 0.0069 | +0.1366 | <1e-4 | **21x** |

Both are played at the rate they are taken. Every other card class *falls* —
action −0.42, lab −0.13, farm −0.11, arena −0.11, theater −0.10, library
−0.10, temple −0.10, mine −0.08, government −0.06 — so the bot takes fewer
cards overall and spends the freed actions on two leaders.

#### Why: a wonder cannot reach the policy the way a leader can

This is the part worth keeping, and it is a fact about the **plumbing**, not
about wonders being weak.

`actions.take_card` does not put a wonder in your hand:

```python
if card["type"] == "wonder":
    p.wonder = WonderInProgress(name)
else:
    journal.touch(p.hand_civil).append(name)
```

So a wonder never enters `hand_civil`, and `hand_potential` — which sums
`card_potential` over the civil hand and is a **live evaluator term** the
1-ply search optimises at every single decision (`hand_value` = 0.306,
`hand_civil` = 1.31) — never sees a wonder at all. A wonder's
`card_potential` reaches the policy through exactly one channel:
`row_urgency`, a take-*timing* heuristic that asks whether a card is worth
grabbing before it slides. A leader gets both channels.

**And in the vector measured here, that one channel is closed.**
`analysis/frozen/champion_2p.json` has no `row_urgency` entry at all, so
`load_weights` fills it from `DEFAULT_WEIGHTS` as 0.0, and `evaluate()` gates
the `row_pressure` call behind `if ru or rb:` — the function never runs. The
channel is not narrow, it is absent. That is why the reprice moved *exactly*
nothing rather than nearly nothing, and it is why no sample size here could
have said otherwise.

**That is why repricing Eiffel Tower by +23.50 moved nothing and repricing
Joan of Arc by +5.88 moved the policy 5.9x.** The size of the reprice is
irrelevant when the term it lands in is not one the search can act on — and
doubly irrelevant when the term is switched off.

This reframes the question this document started from. "The bot does not build
wonders — are wonders modelled wrong?" The answer is no: §6.1 of
[`docs/SCORE_AUDIT.md`](SCORE_AUDIT.md) verified the rules exact, and §1 here has now given
the evaluator the printed numbers. The bot still does not build them, because
**a correctly priced wonder has almost no path into the decision.** It is a
plumbing bug, not a pricing bug, and fixing it means giving wonder-in-progress
its own evaluator term — something the search can actually optimise — not
another row in a lookup table.

*Qualified:* "almost no path" is right in general and an understatement for
this vector, where the path is closed outright. But the prescription — a new
wonder-in-progress term — assumes the existing channel is inadequate rather
than merely switched off. The live league champion has `row_urgency` non-zero,
so that assumption is testable rather than given; see §5.4.

#### Own culture: +2.58, not +10

Stated plainly because it is easy to misread §5 and I want to make it
impossible. §5's **+10.39 is a duel margin against a weakened opponent**, not
production. In the mirror, where both arms play the same field, credit1's own
culture is **146.47 vs 143.88, +2.58 [+1.53, +3.64]**, p<1e-4. Real, in the
same direction, and a quarter the size. The margin is inflated because the
credit0 arm is *also* being made worse by facing a stronger opponent — the
point the since-deleted `docs/LEAGUE_OBJECTIVE.md` (Python-era league
objective, git history) made: a stolen point moves a lead-based margin twice
and a produced point once.

#### The one real wonder change: finish discipline, from a general term

| design | metric | credit1 | credit0 | diff | p |
|---|---|---|---|---|---|
| duel | started | 0.1419 | 0.1862 | −0.0444 | <1e-4 |
| duel | started, not finished | 0.0397 | 0.0706 | **−44%** | <1e-4 |
| mirror | started | 0.1497 | 0.1697 | −0.0200 | <1e-4 |
| mirror | started, not finished | 0.0500 | 0.0650 | **−23%** | <1e-4 |

Pooled finish rate, bootstrap CI over units: duel 327/454 = 72.0% vs 370/596 =
62.1%, **+9.95pp [+3.8, +16.0]**; mirror 638/958 = 66.6% vs 670/1086 = 61.7%,
**+4.90pp [+1.6, +8.3]**. Civil actions sunk into wonder stages fall from
0.351 to 0.300 (duel) and 0.317 to 0.302 (mirror).

The bot starts fewer wonder programmes and finishes a larger share of the ones
it starts. **That is exactly what §5.1's three purpose-built
finish-discipline features were for, and §5.1 records all three as a flat null
with `wonder_overrun` a dead coordinate that never once won an argmax in 1200
games.** A general correction — pricing the row honestly — delivered what
three special-purpose terms could not. The lesson is about feature design: a
term that fires on 3.7% of decisions with a range of 0.24 cannot compete with
fixing the number every decision reads.

#### Caveats

* **The CI may be optimistic by up to √2.** Another lane found this project
  computes confidence intervals with the independent-samples formula on paired
  designs (between-shard χ² 11.76 on 5 df where independence predicts 5). The
  mirror design is paired on the deal, so the same criticism applies here. A
  corrected estimator is landing separately; rather than block on it, here is
  what changes if every SE is widened by √2. **Every headline null is
  unaffected — they are already non-significant, and widening only strengthens
  a null.** The MDE on wonders completed inflates from 0.0089 to **0.0126**
  (8.5% → 12% relative), which does not change the conclusion. Of the
  secondary results, these survive: `started` (mirror p 1e-4→0.0008),
  `unfinished` (1e-4→0.0027), `inprog_end` (0.0016→0.0254),
  `started_repriced8` (1e-4→0.0019), own culture (1e-4→0.0007). These four
  would **cross 0.05 and should be treated as unresolved**: duel
  `ca_on_stages` (0.024→0.112), duel `started_new6` (0.017→0.092), mirror
  `started_unrepriced8` (0.022→0.106), mirror `started_unpriced5`
  (0.010→0.069).
* **Wonders are civil-deck, so the `tools/card_blindness.py` military-deck gap
  another lane found does not touch these counts.** All 16 wonders are
  `deck: civil` and all 16 are present; the counts here come from live engine
  state (`p.completed_wonders`, `p.wonder`) and recorded moves, not from the
  census tool.
* **The rules were re-verified at `6968256`,** not assumed from
  [`docs/SCORE_AUDIT.md`](SCORE_AUDIT.md#1061-the-wonder-rules-and-data-are-right) §10.6.1: all 16 wonders and all 53 stages match the
  data file exactly, the take surcharge is `+1 CA per completed + destroyed`
  with the Michelangelo exemption, `Impact of Wonders` scores 5/4/3/2 by age
  as printed, and `tests.test_card_pricing` + `tests.test_scoring_bugfix` pass
  (40 tests). `96a5db2` touched no engine rules file.
* **The obvious trap in instrumenting this.** The first version of the probe
  monkeypatched `actions.take_card` and reported **57 wonder takes per game**.
  The 1-ply search applies every candidate move to a *copy* of the state, so
  an engine-level patch counts hypothetical moves. The real number is ~0.15.
  Anything that counts engine calls has to filter to the real state or, as
  here, record only the move the bot actually chose.

### 5.4 The same measurement against the LIVE champion: §5.3 does not survive

§5.3 measured the frozen 2p champion, whose `row_urgency` is 0.0 — the single
channel a wonder's `card_potential` has. This is the identical experiment
against the bot the league is actually training.

**Arms.** `analysis/frozen/champion_2p_gen54_99key.json` (a frozen copy of
`experiments/league_state/champion_2p.json`, gen 54, 99 keys,
`row_urgency = −0.19109`) with `card_rate_credit` 1.0 vs 0.0 — one key apart,
exactly as §5.3's arms were. Mirror design, 3200 deals × 2 arms = **12,800
seat-games**, matching §5.3's sample exactly. Zero engine errors. Intervals
are deal-clustered with the t correction (`experiments/paired_stats`, commit
`6d6fec1`), not the independent-samples formula §5.3's caveat warned about.

| metric | frozen (§5.3) | **live (this run)** |
|---|---|---|
| wonders COMPLETED, credit1 | 0.0997 | **1.2233** |
| wonders COMPLETED, credit0 | 0.1047 | **0.6502** |
| **diff** | **−0.0050** [−0.0112, +0.0012], p=0.117 | **+0.5731** [+0.5502, +0.5960], p<1e-4 |
| relative | −4.8% (a null, MDE 0.0089) | **+88%** |
| wonders STARTED | −0.0200 | **+1.1756** (2.2991 vs 1.1234) |
| civil actions sunk into stages | 0.317 → 0.302 (*fell*) | 2.5288 → 4.6966 (**+2.17**) |

**The effect is 64× the minimum detectable effect §5.3 quoted while calling
the result a clean null.**

And the causal specificity is now exactly right, which it was not before:

| group | frozen (§5.3) | **live** |
|---|---|---|
| all 8 **repriced**, completed | −0.0044, p=0.160 | **+0.5725 (+88.1%)** |
| all 8 **unrepriced**, completed | −0.0006, p=0.317 | **+0.0006 (+0.0%)** |

§5.3 had to report that *the unpriced set fell more than the repriced set* —
"the opposite of the causal prediction". That anomaly is gone. On the live
vector the eight cards the treatment touches move by 88% and the eight it does
not touch move by a rounding error. Distribution of wonders completed per
seat-game: the share finishing **zero** wonders drops from **43.5% to 19.3%**,
and 3+ goes from 0.36% to 6.25%.

#### What this changes

* **"Repricing wonders moves wonder behaviour by zero" is false for the bot we
  train.** It was true, and necessarily true, of the vector §5.3 measured.
* **"A correctly priced wonder has almost no path into the decision" is false
  for the bot we train.** `row_urgency = −0.19109` carries it comfortably. The
  one channel is narrow but it is not weak.
* **The prescription — "fixing it means giving wonder-in-progress its own
  evaluator term" — is not established.** Worth noting that such a term
  already exists: `evaluate()` calls `wonder_potential(state, idx, w)` behind
  `if wp:`, and `wonder_potential` is 0.0 in *every* champion in the repo,
  frozen and live alike. Before building a new term, the cheaper experiment is
  to let the league climb the one that is already there.
* **§5.3's qualitative reading survives in one respect and one only:** a
  wonder really does reach the policy through exactly one channel while a
  leader gets two, so wonder pricing is *structurally* more fragile than
  leader pricing. That was worth finding. The quantitative null attached to it
  was not a finding.

**Why the two runs disagree so violently.** They are not the same bot. The
frozen champion is gen 220 of an older 78-key climb; the live one is gen 54 of
a 99-key one, and 21 of those 99 keys — the entire card-row block — did not
exist when the snapshot was cut. The baseline behaviour differs accordingly:
the frozen bot completes 0.10 wonders a game and the live one 0.65 before any
treatment is applied. See [`analysis/frozen/README.md`](../analysis/frozen/README.md).

**Reproduce:**

```bash
nice -n 19 python3 tools/wonder_mechanism.py --mode mirror \
  --a analysis/livechamp/live2p_credit1.json --deals 3200 \
  --tag live_credit1 --out /tmp/live_mirror.jsonl --workers 8
nice -n 19 python3 tools/wonder_mechanism.py --mode mirror \
  --a analysis/livechamp/live2p_credit0.json --deals 3200 \
  --tag live_credit0 --out /tmp/live_mirror.jsonl --workers 8
nice -n 19 python3 tools/wonder_mechanism.py --report \
  --mirror /tmp/live_mirror.jsonl \
  --a analysis/livechamp/live2p_credit1.json \
  --b analysis/livechamp/live2p_credit0.json \
  --tag-a live_credit1 --tag-b live_credit0
```

The tool now refuses the §5.3 arms outright
(`experiments.arena.assert_lever_conducts`) rather than reproduce a null that
is an arithmetic identity; pass `--no-lever-check` if you want the original
numbers back.

## 6. What is still broken after this

* **The 37 cards with 0 dropped keys and 0 visible gain** — territories,
  units and tactics — are taken up in [`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md). Short
  version: none of them keeps its value in `production`/`effects`, so neither
  the census above nor the guardrail below could see them, and the ten unit
  cards were priced *negative* rather than at zero.
* **Military cards have no per-card pricing at all.** `weighted.py`'s
  `hand_mil_value` is `sum(age + 1)` over the military hand, so every tactic,
  war, aggression and territory of the same age is interchangeable. Tactics
  are the largest single lever in the military game and there are 15 of them.
  Fixing this means giving `hand_potential` a military sibling, not another
  table entry.
* **Five wonders and all 55 events are still worth zero** to
  `card_potential`, for the reasons in §3 buckets 1–2. A board-aware card
  evaluator — one that takes `(name, state, idx, w)` rather than `(name, w)` —
  is what closes buckets 1 and 4, and it is the single highest-value follow-up
  this census suggests.
* **A wonder has almost no path into the decision, however well it is priced.**
  §5.3 is the evidence: a wonder never enters `hand_civil`, so `hand_potential`
  — the live term the search optimises every decision — cannot see it, and its
  only channel is the `row_urgency` take-timing heuristic. Repricing Eiffel
  Tower by +23.50 moved nothing; repricing Joan of Arc by +5.88 moved the
  policy 5.9x. **Pricing wonders better cannot help until wonder-in-progress
  has an evaluator term the search can act on.** This is a bigger lever than
  any remaining entry in the lookup table.
* **The tier-list question is untouched.** This change gives the evaluator the
  *printed* value of a wonder. It says nothing about the tournament ordering,
  and [`docs/STRENGTH_CHECK.md`](STRENGTH_CHECK.md)'s BookBot v2 result (+2.1%, p=0.098 at 2p,
  slightly negative at 3p) remains the best evidence that hand-coded tier
  knowledge is worth little here.
* **3p and 4p are not measured.** Everything in §5 is 2p.

## 7. Reproducing

```bash
# the census, before and after
python3 -m tools.card_blindness --legacy
python3 -m tools.card_blindness
python3 -m tools.card_blindness --legacy --keys
python3 -m tools.card_blindness --cards wonder

# the guardrail
python3 -m unittest tests.test_card_pricing

# the A/B (8 blocks of 400, paired seats, ~25 min under load)
for s in 0 200 400 600 800 1000 1200 1600; do
  python3 -m experiments.evaluate \
    --a analysis/cardblind/champ2p_credit1.json \
    --b analysis/cardblind/champ2p_credit0.json \
    --players 2 --games 400 --seed $s --workers 2 --out /tmp/ab_main.jsonl
done

# 3p (3 blocks of 300)
for s in 0 200 400; do
  python3 -m experiments.evaluate \
    --a analysis/cardblind/champ2p_credit1.json \
    --b analysis/cardblind/champ2p_credit0.json \
    --players 3 --games 300 --seed $s --workers 2 --out /tmp/ab_3p.jsonl
done

# the finish-discipline scan
for k in overrun_0.5 overrun_1.5 overrun_4.0 turns_0.3 turns_1.0; do
  python3 -m experiments.evaluate --a analysis/cardblind/fd_$k.json \
    --b analysis/cardblind/champ2p_credit1.json \
    --players 2 --games 400 --seed 0 --workers 2
done

# why the scan is null: the dead-coordinate census
python3 tools/feature_variance.py --players 2 --games 6 \
  --champ analysis/cardblind/champ2p_credit1.json --out /tmp/fv2.json

# section 5.3: the wonder-mechanism probe (12,800 games, ~50 min at nice 19).
# Two designs because the duel confounds contention for the wonder row; the
# mirror is the honest behavioural number.  The probe records only the move
# the bot CHOSE -- patching engine/actions.py instead counts the 1-ply
# search's candidate moves and reports 57 wonder takes a game.
nice -n 19 python3 tools/wonder_mechanism.py --mode duel \
  --a analysis/cardblind/champ2p_credit1.json \
  --b analysis/cardblind/champ2p_credit0.json --deals 1600 --out /tmp/duel.jsonl
nice -n 19 python3 tools/wonder_mechanism.py --mode mirror \
  --a analysis/cardblind/champ2p_credit1.json --deals 3200 \
  --tag m_credit1 --out /tmp/mirror.jsonl
nice -n 19 python3 tools/wonder_mechanism.py --mode mirror \
  --a analysis/cardblind/champ2p_credit0.json --deals 3200 \
  --tag m_credit0 --out /tmp/mirror.jsonl
nice -n 19 python3 tools/wonder_mechanism.py --report \
  --duel /tmp/duel.jsonl --mirror /tmp/mirror.jsonl

# the gate.  GATE PASS on this branch; NARROW/WIDE unchanged from master,
# the six evaluator arms re-derived two-sidedly (see tools/gate.sh).
bash tools/gate.sh
```

## 8. Provenance of the numbers in this document

Everything above was run for this document; nothing is carried over.

* Census: `tools/card_blindness.py`, deterministic, no games.
* 2p A/B: 3200 games, 8 disjoint blocks, paired on the deal.
* 3p A/B: 900 games, 3 disjoint blocks.
* Finish-discipline scan: 5 × 400 games.
* Dead-coordinate census: 984 decisions over 6 games.
* Digests: derived twice, independently, in two worktrees, agreeing on all 8.

The one thing this document does **not** contain is a league run. Every
result here is the *frozen* champion evaluated under better information, not
a champion retrained with it.

## 9. Operational: changing the evaluator invalidates the cached pool weights

This is the part that is easy to miss, and it costs training time rather than
correctness, so nothing fails loudly when you get it wrong.

`experiments/hillclimb_league.py:win_rates()` prices every pool opponent from
`state_Np.json:last_full_check` -- the champion's measured win rate at the last
full pool check. `hillclimb_pool.saturation_multiplier` then cuts an opponent's
weight from 1.0 toward `sat_floor` as that rate rises past `sat_lo`, and flags
it `inert` at the top of the range, where it is dropped from the acceptance
rotation entirely. The docstring says the rule "cannot go stale: a champion
that stops beating an opponent gets that opponent's weight back at the next
check." That is true **within** an engine and false **across** one. The cached
rates were measured by a different evaluator; after a change like this one they
describe a bot that no longer exists.

`build()` runs once at process start, so a restart carries the stale weights
into every generation until the next `--full-check-every` boundary. At the halt
for this change the muting was **2p: 10 of 22 opponents at >=95%, 3p: 3 of 22,
4p: 1 of 22** -- concentrated almost entirely in the arm with the longest
generation.

**The fix is to delete the key, not to re-measure it.** `saturation_multiplier`
returns 1.0 for `win_rate is None` and `inert` is `False` unless a rate exists:

```python
if win_rate is None:
    return 1.0          # never measured -> presumed informative
...
e.inert = e.sat <= self.sat_floor + 1e-9 and e.win_rate is not None
```

so an absent `last_full_check` yields a fully un-muted pool at full weight,
which is exactly the honest post-change state: no valid evidence, therefore no
opponent muted. The next scheduled full check then re-measures all 22 at full
`--check-games` precision, because nothing is flagged inert and so nothing is
sampled at the reduced `inert_games` rate.

A forced full check at restart was considered and rejected: it costs 2p ~58
min, 3p ~10 min, 4p ~26 min of *pure checking* to buy information the next
scheduled check produces for free, while nulling the key costs nothing and
leaves the pool in the strictly safer direction (over-weighting a beaten
opponent wastes games; under-weighting a live one biases acceptance).

**Checklist when you change the evaluator:**

1. Stop the arms with `experiments/logs/stop_league_{2,3,4}p.json`, then run
   `experiments/watchdog.sh` once so it reaps the supervisors, and confirm
   `pgrep -f run_league.sh` is empty before going on.
2. `git pull` only once no climber is running.
3. Back up `state_Np.json`, then delete its `last_full_check` key.
4. Remove the sentinels and let `watchdog.sh` relaunch, so the REQUIRED-flag
   assertion re-checks the arg list instead of you hand-typing it.
5. Confirm each arm logs `0 opponents measured` on its startup pool line and
   then advances a generation.

**THE HALT CANNOT STOP A SUPERVISOR OLDER THAN THE HALT.** Step 1 used to read
"wait for the generation boundary -- climbers do not poll mid-generation", and
that was wrong twice over. The sentinel is not polled between generations at
all: `hillclimb_league.main` tests it *once, at startup*, and `run_league.sh`
tests it at the top of its restart loop, so the granularity is the `--hours 1`
invocation and not the generation. Worse, `run_league.sh` is parsed by bash
**once, when the supervisor launches** -- a `while` loop is a single compound
command, read into memory whole -- so editing the script does not change the
behaviour of an arm that is already running. On 2026-07-30 all three arms were
launched on 07-29 at 08:14/08:50/09:20 and the stop-file check landed in
`run_league.sh` at 07-29 15:33. Every one of them was executing a loop body
that had never heard of the sentinel. 3p sat in a 60-second cycle -- climber
refuses at startup, supervisor sleeps 60, restarts it, forever -- and would
have done so until the deadline, looking busy in `ps` the entire time.

So the sentinel's real and only job is to stop `watchdog.sh` from *relaunching*
an arm. Stopping a *running* arm is a kill, and it always was. `watchdog.sh`
now does that kill itself (`reap`), which is what makes step 1 above true
rather than aspirational: the file is once again sufficient, because the thing
that reads the file is the thing cron re-executes from disk every ten minutes.
`watchdog.sh` can be fixed by editing it; `run_league.sh` cannot, and any
future halt mechanism belongs in the watchdog for exactly that reason.

## 10. The unit of analysis: every interval in this project was computed on the wrong n

Audited 2026-07-30, across the whole repo. This section is the reference for
how to compute an interval on an arena result, and a record of what changed
when the published numbers were recomputed. Nothing here is a re-run: the
point estimates are untouched and every corrected figure comes from the same
raw `per_game` arrays the originals came from.

### 10.1 The defect

`experiments/arena.duel` builds its task list as

```python
for g in range(games):
    seat = g % num_players
    seed = seed0 + g // num_players
```

so a 3200-game 2p run is **1600 deals each played twice with the seats
swapped**. `experiments/neural_eval.py` deals identically. The games are not
independent, and until this audit every interval in the repo divided by the
number of *games*:

* `experiments/arena.py:mean_ci` — `1.96*sqrt(var/n_games)`, inherited by
  `hillclimb_league`, `roster_match`, `roster_report`, `proxy_check`,
  `summarize`, `evaluate`, `champ_vs_drift`, `level_sweep`, and every
  `tools/*_ab.py` that clones it.
* `experiments/pool_summary.py:56` — `1.96*sqrt(p(1-p)/n_games)`, blunter
  still, and the one feeding the neural loop's promotion gate.
* `analysis/cardvalue_duel.py`, `analysis/passfix_duel.py`,
  `experiments/human_strength.py` (whose header says "seat-rotated,
  seed-paired" directly above a per-game SE), `tools/ab_summary.py` — which
  was written *the same night as this audit* and had already inherited the
  formula from `pool_summary`. That is four independent copies.

Three places already had it right and are worth copying from:
`exp_quiesce/analyse.py`, `tools/transfer_ab.py:by_deal`, `tools/wonder_ab.py`.

### 10.2 The correction is not a factor of √2, and its sign is not obvious

At P = 2, with `Y_k` the mean of deal `k`'s two seat-swapped games:

```
Var(Y_k)     = (p(1-p) + Cov(X_k0, X_k1)) / 2
naive SE^2   = p(1-p) / (2K)
correct SE^2 = (p(1-p) + Cov) / (2K)
ratio        = sqrt(1 + rho),   rho = corr(X_k0, X_k1)
```

* **ρ > 0** — the deal favours a *strategy* whatever seat it sits in. The naive
  interval is too narrow, by at most √2.
* **ρ < 0** — the deal favours a *seat*, so the challenger tends to win one
  game of the pair and lose the other. Swapping the seats cancels that
  nuisance variance and the correct interval is **narrower**, → 0 at ρ = −1.

**Measured, ρ is negative almost everywhere in this project** — −0.04 to −0.72
across the eight datasets below — because the deal×seat interaction in Through
the Ages is large. So the naive interval was usually *conservative*, and most
of these results get **stronger**, not weaker. Anyone "fixing" this by
multiplying by √2 would make every number in the project wrong in a new way.

The demonstration that settles it needs no model. `exp_quiesce/ab.jsonl`'s
`ctrl_2p` row is the same deterministic bot on both sides, n = 800, published
at **±3.46pp** — when every deal splits 1-1 by construction and the true width
is **exactly zero**. All 3.46pp of that interval was seat-assignment noise.
`tests/test_paired_stats.py` asserts that case at zero, so the formula cannot
come back.

### 10.3 The second defect: shards that do not agree

Runs are fanned out over disjoint `--seed0` blocks. Block means are
independent by construction, so they are a check on the deal-level interval
that assumes nothing about what happens inside a block. When the blocks
scatter further than deal-level noise allows, the deals are demonstrably not
exchangeable and the honest interval is the coarser, block-clustered one.
`paired_stats.pooled` runs that χ² and escalates automatically.

Cluster intervals use **t₍ₖ₋₁₎, not 1.96**. With six shards the variance
estimate is itself noisy and t₅ = 2.571; treating it as 1.96 understates the
interval by 31% before any of the rest of this is counted.

### 10.4 What changed, number by number

Point estimates are unchanged throughout. "Final" is deal-clustered unless the
blocks failed the heterogeneity test, in which case it is block-clustered.

| result | n | published | corrected | verdict |
|---|---|---|---|---|
| culture/science 2p (§5) | 3200 / 1600 deals | 59.53% ± 1.30pp, z = 14.4 | **59.53% ± 1.30pp, z = 14.4** | **unchanged** — was already deal-clustered; naive would have said ±1.69pp |
| ... its culture margin | 3200 | +10.39 ± 1.15, z = 17.8 | **+10.39 ± 1.15, z = 17.8** | **unchanged** |
| culture/science 3p (§5.2) | 900 / 300 deals | 38.83% ± 3.18pp, z = 3.4 | **38.83% ± 2.53pp, z = 4.28** | **stronger**, conclusion unchanged |
| board-aware wonder pricing | 2000 / 1000 deals | 53.52% ± 2.17pp, z = 3.18 | **53.52% ± 1.77pp, z = 3.90** | **stronger**, conclusion unchanged |
| Lane C, partial | 200 / 100 deals | 46.00% ± 7.30pp | **46.00% ± 7.39pp** | **wider** (ρ = +0.12, the one positive) — still a null |
| Lane C, one block | 400 / 200 deals | 48.0% ± 4.95pp | **48.00% ± 4.98pp** | ~unchanged — still a null |
| Lane C, governments | 3200 / 1600 deals | 51.02% ± 1.40pp, z = +1.4 | **51.02% ± 1.40pp, z = +1.42** | **unchanged** — already deal-clustered |
| **Lane C, leaders** | 3200 / 1600 deals | 48.20% ± 1.69pp, **z = −2.1** | **48.20% ± 2.92pp, z = −1.46, p = 0.15** | **CONCLUSION CHANGES** — see §10.5 |
| Lane D event seeding | 3200 / 1600 deals | 57.38% ± 1.70pp, z = 8.49 | **57.38% ± 0.91pp, z = 15.90** | **much stronger** |
| ... its culture margin | 3200 | +6.52 ± 1.49, z = 8.59 | **+6.52 ± 0.34, z = 38.1** | **much stronger** |
| **the anchor, post-fix** | 240 / 6 shards | 0.4313 ± 0.0627 | **0.4313 ± 0.1260** | **CONCLUSION CHANGES** — 2.01× optimistic, see §10.6 |
| the anchor, pre-fix | 240 / 6 shards | 0.4396 ± 0.0628 | *not recoverable* | shard logs overwritten by the re-seed; no corrected interval is claimed |

Three of these moved materially, and two of the three moved **in favour of the
result**. The two that got worse are the two that were being used to make
decisions — the leaders arm and the promotion gate — which is not a
coincidence: a marginal number is exactly the kind that gets acted on before
anyone checks the denominator.

### 10.5 Lane C leaders: the one substantive retraction

[`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md#1352-win-rate-a-flat-aggregate-that-decomposes-into-two-opposite-signs) §13.5.2 reports the leaders-only arm at
**48.20% ± 1.69pp, z = −2.1**, and reads that as "leaders hurt slightly". The
interval is correctly deal-clustered. The problem is one level up: the eight
blocks are **over-dispersed**, χ² = 14.41 on 7 df against a critical value of
14.07 (p ≈ 0.044). Per-block win rates are 43.8, 47.8, 52.4, 46.3, 53.8, 46.3,
45.6, 49.9 — a spread of 3.49pp where deal-level noise predicts 2.44pp.

Clustering on the block instead gives **48.20% ± 2.92pp, z = −1.46, p = 0.15**.
The effect is **not significant** and should not be described as one.

This is a borderline call and worth stating as such: the escalation trigger
itself is only just tripped, and with eight blocks the heterogeneity test is
not powerful. But that cuts the same way — a result whose significance depends
on which of two defensible clusterings you pick is not a result. The culture
margin on the same arm was already a null (−0.48 ± 1.33) and stays one
(−0.48 ± 2.56). The governments half of that document is unaffected: it is
already deal-clustered, the blocks agree (χ² = 2.59 on 7 df), and its margin
result (+1.85 ± 1.07, z = 3.4) stands.

### 10.6 The anchor, and why the neural loop's gate floor is too narrow

`loop2/anchor_seed_{0..5}.log`, six shards of 40:

```
0.3250  0.3000  0.3875  0.5625  0.4250  0.5875     mean 0.4313
```

`pool_summary` reported **±6.27pp** from `1.96*sqrt(p(1-p)/240)`. Those shards
have χ² = **11.76 on 5 df** (p ≈ 0.038) against a critical 11.07 — they do not
agree, and a formula that pools over games cannot tell. Shard-clustered with
t₅ = 2.571: **±12.60pp**. The published interval was optimistic by **2.01×**.

Note this is *not* the seat-pairing defect. Seat pairing would have made the
anchor tighter. Two independent bugs happened to sit on the same number.

The substantive comparison survives: post-fix 0.4313 against pre-fix 0.4396 is
a difference of −0.83pp, against a 95% CI of the difference of ±8.86pp naive or
**±13.59pp** corrected. It was never a detectable difference and still is not.
What does not survive is the claim about *resolving power*.

**The gate floor.** `neural_search_loop.sh` sets

```
floor = incumbent - sqrt((cand_ci/1.96)^2 + (inc_ci/1.96)^2)
```

i.e. one standard error of the difference, and the comment reasons "at n = 240
a side is se ≈ 0.032, so the band is ~0.045". With the shard-clustered SE of
**0.0490** per side the band is **6.93pp, not 4.52pp**. The gate is currently
about **1.5× tighter than the data supports**, which is the promotion-on-noise
failure [`docs/NEURAL_LOOP_NULL.md`](NEURAL_LOOP_NULL.md) documents at length.

**This was deliberately not changed by the audit.** `pool_summary` kept
emitting `ci=` with the legacy value so a loop in flight saw byte-identical
behaviour, and published `ci_cluster=`, `chi2=` and `overdispersed=` alongside.
Moving a promotion threshold under a running experiment is a human's decision.

### 10.6.1 The gate floor, as applied

Applied 2026-07-30 at the iteration-11 boundary, on the box owner's
instruction. `pool_summary` now also publishes **`se_cluster=`** — the cluster
standard error itself — and arm B reads that field directly. Nothing divides a
half-width by a critical value anywhere in the decision:

| per-side SE | source | band | verdict |
|---|---|---|---|
| 0.0320 | `ci/1.96`, per-game binomial | **4.52pp** | what shipped to 2026-07-30; blind to the shards, ~1.5× too tight |
| **0.0490** | **`se_cluster`, shard-clustered** | **6.93pp** | **the gate** |
| 0.0643 | `ci_cluster/1.96` | 9.09pp | the trap: `ci_cluster` is t₅·se, so this leaves 2.571/1.96 behind |

Why shard clustering and not the deal clustering of §10.2: the anchor's defect
is between-shard over-dispersion (χ² = 11.76 on 5 df), not seat pairing, and
seat pairing here would make the interval *tighter* — the wrong direction. The
shards are disjoint `--seed0` ranges, so their independence is a fact of the
design and needs no assumption about what happens inside one. It is also the
only estimator computable in the pooling path, which sees shard means and not
per-game vectors.

**Only arm B moved.** Arm A (`win - ci > 0.5`) still reads the legacy `ci`:
it is the arm carrying the type-I control, correcting it would *tighten*
promotion, and moving two thresholds in one commit would make the
discontinuity recorded in `loop2/curve.tsv` uninterpretable. Arm A's
`ci_cluster`/`chi2`/`overdispersed` are now logged every iteration so that
decision can be made on its own measured evidence.

**What the band costs.** Arm B is a regression veto, not a significance test —
the net is ~14pp behind the champion and a 5%-level test against it would
freeze the loop. Under the null, arm B passed 74.3% of candidates at the old
floor (−0.65 true SE) and passes 84.1% at the new one (−1.00 SE). Joint
false-promotion, both arms, moves **1.9% → 2.1%**. Arm B was never the arm
doing the rejecting, which is why loosening it is safe and why arm A must not
be loosened alongside it.

`ANCHORF` (`loop2/anchor_best.txt`) grew a third field, `win ci se`. A
two-field file makes arm B **fail closed** with a message naming the fix: a
cluster SE cannot be recovered from a per-game CI, and the one reconstruction
available is the defect itself.

Tests: `tests/test_gate_floor.py` extracts `anchor_floor` from the driver and
executes it, asserting 6.93pp and asserting 4.52pp and 9.09pp as the two wrong
answers; `tests/test_pool_summary.py::TestClusterStandardError` pins
`se_cluster` at 0.0490 and asserts it is *not* `ci_cluster/1.96`.

### 10.7 How to compute an interval from now on

```python
from experiments import paired_stats as PS

est = PS.paired(res["per_game"], res["players"])      # one duel
est = PS.pooled([b["per_game"] for b in blocks], 2)   # several seed blocks
print(est.fmt(), est.z_against(0.5), est.p_against(0.5))
```

`est.naive_half` carries the legacy number for reconciling against older
reports, `est.rho` and `est.deff` say how much the pairing bought or cost, and
`est.het_chi2` / `est.escalated` say whether the blocks agreed. There is also
`PS.block_bootstrap` if you want a distribution-free cross-check; it agreed
with the closed form to within 0.05pp on every dataset in §10.4.

Or, for a whole `--out` jsonl: `python3 tools/ab_summary.py /tmp/ab_main.jsonl`.

The one rule that would have prevented all of this: **the denominator is the
thing the experiment randomises, and this arena randomises deals.**

## 11. Territories, units and tactics: the 37 cards with no dropped keys (2026-07-29) (merged from the former `CARD_BLINDNESS_MILITARY.md`, 2026-07-31)

A follow-up to [`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md), which fixed the cards whose printed
value was being **dropped** by `_card_yields`. This one is about the 37 cards
the census reported as **zero visible gain with zero dropped keys** — 12
territories, 10 military units, 15 tactics. Those two numbers look like a
contradiction and the contradiction is the finding.

### One-paragraph answer

`_card_yields` reads `production` and `effects`, and `tools/card_blindness.py`
counts a key as "dropped" only if it appears in one of those two blocks and
does not map. **None of these 37 cards keeps its value there.** A unit's yield
is a top-level `strength` field; a territory's is in `immediateEffects` /
`permanentEffects`; a tactic's is not a card constant at all. So the census
could not see them and neither could the guardrail — `tests/test_card_pricing.py`
walked exactly the same two blocks, which means **the test written to make the
blind spot visible had the same blind spot.** For the ten unit cards the result
was worse than invisible: `_card_yields` *does* read a unit's `techCost` and
`buildCost`, so every unit priced out as **pure cost, strictly negative**
(Swordsmen −1.66, Air Forces −4.40 under the frozen 2p champion). This document
maps all three, widens the guardrail to the other blocks *and* to top-level
fields, and reports what each is worth. The headline behavioural finding is
measured, not inferred: **the 2p champion had a unit card legally takeable at
30% of its plies and took one in twelve games, and across 30 further games had
five unit workers standing on the whole table — while spending ~29 military
actions a game on tactics that could form zero armies.**

Everything here is base game (2015), 2 players unless stated.

### 11.1. Why the census said "0 dropped, all blind"

| type | n | where the value actually is | seen by `_card_yields` before |
|---|---|---|---|
| territory | 12 | `immediateEffects` (one-shot) + `permanentEffects` (ongoing); `effects` is literally `{}` | nothing |
| infantry/cavalry/artillery/air | 10 | top-level `strength`, per worker | **only the costs** |
| tactic | 15 | `tacticBonus` × armies formable from the board | nothing |

#### 11.1.1 Units were negative, not zero

This is the part worth being precise about, because "zero visible gain" understates it.

```
Warriors           -0.57   (('resource_stock', -2.0, COST),)
Swordsmen          -1.66   (('science', -4.0, COST), ('resource_stock', -3.0, COST))
Riflemen           -2.63   (('science', -6.0, COST), ('resource_stock', -5.0, COST))
Cavalrymen         -2.63   ... byte-identical to Riflemen and to Cannon
Cannon             -2.63
Modern Infantry    -4.00
Air Forces         -4.40
```

Two consequences, both live:

* `row_pressure` skips any card whose `card_potential` is `<= 0`
  (`weighted.py`, "the sweep destroying a card I do not want is not a loss").
  So **no unit card in the civil row was ever visible to `row_urgency` or
  `row_bargain_forgone`**, at any weight.

  > **Sharpened (2026-07-30).** On the frozen champion this bullet was true
  > but vacuous: `row_urgency` and `row_bargain_forgone` are both 0.0 there,
  > so `row_pressure` is never called on *any* card. On the **live** 2p league
  > champion (`row_urgency = −0.19109`) the function does run, and the bullet
  > becomes a real, operative claim — units still price strictly negative
  > (Warriors −3.46, Swordsmen −6.51, Air Forces −16.07, all 7/7 negative and
  > 2–4× more negative than on the frozen vector), so `row_pressure` genuinely
  > skips every one of them. **The finding holds on the bot the league trains
  > and its mechanism is now demonstrable rather than untestable.**
* `hand_potential` sums the raw value, so **holding a unit card lowered the
  evaluation**. Taking one was priced as taking a liability.

And the three Age II units are the same number to the byte, which is the
`meta.get(n)[1] + 1` age-only bug the brief names — except it turns out that
for units the age-only value is *nearly right*, because within an age
infantry, cavalry and artillery genuinely share strength and cost. What
differs between them is **which tactic they can fill**, which is §11.4.

#### 11.1.2 The guardrail had the same blind spot

`tests/test_card_pricing.py` walked `("production", "effects")`. A test that
only looks where the values already are cannot report a value in the wrong
place. It now walks four blocks, and separately accounts for **every top-level
field on every card** — `TOP_LEVEL_PRICED` / `TOP_LEVEL_UNPRICED`, on the same
terms as `DELIBERATELY_UNPRICED`: named, or written off with a reason.

Note what did *not* happen: `DELIBERATELY_UNPRICED` barely shrank. Only
`tacticBonus` / `tacticBonusObsolete` moved, and only to a corrected reason —
they are still not read. Nothing else could shrink, because **none of these 37
cards' values were ever on that list**; they were outside the surface the list
describes. The guardrail got stronger by growing the walked surface, not by
emptying the write-off set, and that is the more useful direction: a write-off
is a known unknown, and what cost the ten unit cards was an unknown unknown.
Two new checks close the adjacent holes — a key may not be both mapped and
written off, and `_CREDIT_OF`'s fallbacks must equal `DEFAULT_WEIGHTS`, so the
same vector cannot price the same card differently depending on how it was
loaded (that one was a live bug in this change, caught by writing the test).

That second walk immediately found a case outside this lane and it is left
**written off rather than fixed**, because it is another lane's card type:
a government's `civilActions` / `militaryActions` are top-level too, so
`_card_yields` prices a government from its `production`/`effects` and never
sees its action counts. That is the unit bug exactly. The written-off entry is
the visible record that it is still open.

### 11.2. What was changed — and which parts are facts

Paul's rule: **a fact from the rules lands unconditionally; a modelling choice
with a free parameter needs evidence.** Sorting this lane that way:

| change | fact or choice |
|---|---|
| a unit's top-level `strength` is its per-worker yield | **fact** — `effects._tech_prog` already treats it as one |
| a territory yields its `immediateEffects` / `permanentEffects` | **fact** — `interact.gain_colony` applies exactly those |
| `_TERR_TO_FEATURE` agrees with the auction path's map | **fact** — same card, same blocks, same engine |
| a tactic's bonus is `tacticBonus` × armies formable | **fact** — `effects._army_value` |
| **how much** of a unit's strength to believe (`unit_strength_credit`) | **choice** — free parameter, defaulted 0.0, §11.3 |
| **how much** the military hand is worth (`hand_mil_potential`) | **choice** — free parameter, defaulted 0.0 |
| the shape of `tactic_short` and its sign | **choice** — free parameter, defaulted 0.0 |

Every fact is landed. Every choice is a weight at 0.0 for the league to
learn, and the A/B numbers below are reported as findings, not as a condition
of landing.



#### 11.2.1 Units — the mapping is not a judgement call

`engine/effects.py:_tech_prog` puts a unit card's top-level `strength` into
the same per-worker programme slot it puts a farm's `production.food` into.
The engine already treats it as that unit's production. So `_card_yields`
reports it as the `strength` feature, and `tests/test_card_pricing.py` asserts
the agreement directly — the same argument, and the same drift guard, as
`culture` → `culture_rate`.

#### 11.2.2 Territories — priced from the applied effect, not the card text

`interact.gain_colony(state, p, name)` is what actually applies a territory:
`permanentEffects.yellowTokens` through `grant_yellow`, `blueTokens` onto
`blue_total`, `immediateEffects` through `events.apply_gains`, and the rating
symbols folded into `Stats` by `effects._colony_permanents`.

`_TERR_TO_FEATURE` is **derived from `_YIELD_TO_FEATURE`**, not retyped,
because `deferred_credit` already prices exactly these two blocks with exactly
that map — for a territory you *already hold the high bid on*. The asymmetry
was that the evaluator could see the card once it was winning the auction and
not before, and not in hand. One substitution is required: `_YIELD_TO_FEATURE`
sends `happy`/`happiness` to the string `"happy"`, which `features()` resolves
by hand and which is **not a weight**; `card_potential` does a bare
`w.get(k, 0.0)`, so leaving it would silently drop Historic Territory's whole
permanent effect — the same bug one level down.

Territories now price 0.46 (Inhabited I) to 13.40 (Historic II) under the 2p
champion, against 0.00 for all twelve before.

#### 11.2.3 The hook: `hand_mil_potential`

None of that reaches the evaluator on its own, because `hand_potential` walks
`hand_civil` **only** — which is the real reason all 12 territories were
invisible and why mapping `tacticBonus` would have changed nothing.
`hand_mil_potential` is its military sibling. It is the piece the other
military card types need too: of the 94 military-deck cards it prices, 12
territories and 1 aggression are non-zero and the rest are 0, so it is
currently an almost pure territory probe and a hook for lanes C/D.

**It does not open an information leak**, and that is worth stating rather
than assuming, because reading a hand across a turn boundary is exactly the
`end_turn` leak of [`docs/INFORMATION_AUDIT.md`](INFORMATION_AUDIT.md#6-the-information-set-question-question-5) §6. Two reasons, both already
established there: it reads `state.players[idx].hand_military`, my own hand,
never a rival's (§11.6 checks that every `hand_military` read in `weighted.py` is
`p = state.players[idx]`); and §6.2 measured that `hand_mil_value` "varies in
**0 of 1583** `end_turn` candidates and structurally cannot vary" — a 1-ply
trial does not reach my next military draw. `hand_mil_potential` is a
different function of *the same hand*, so it inherits that result exactly.

#### 11.2.4 Tactics — the deadlock, and why one feature is not enough

A tactic is worthless without units and units are worth much less without a
tactic, and **a 1-ply search cannot see either half from the other side**:

* playing a tactic you have no army for is +0 strength for a military action
  and a card, so the bot does not play it;
* building the unit that would complete an army is +printed-strength only,
  because the tactic is not in play yet, so the bot does not build it.

`engine/effects.py:tactic_outlook(state, p, names)` returns both halves over
the tactics reachable right now (in hand, or in `state.available_tactics`):

* **`tactic_gain`** — army strength the best reachable tactic would add over
  the one in play. It goes to exactly 0 once that tactic *is* in play, so a
  positive weight prices **getting there**, not holding the card.
* **`tactic_short`** — unit workers still owed before that tactic forms one
  more army. `tactic_gain` alone is a step function that is flat at zero for
  the first two of Heavy Cavalry's three cavalry; this is the gradient.

`tacticBonus` / `tacticBonusObsolete` stay in `DELIBERATELY_UNPRICED`, with a
corrected reason: they are board-scaled, **and** they are a duplicate spelling
of the top-level `strength` / `obsoleteStrength` the engine actually reads
(`_army_value` never touches the effect keys). A test asserts the two
spellings agree on all 15 tactics, so the duplicate cannot rot.

### 11.3. Inert, and why that is the measured choice rather than the timid one

**Every fingerprint digest is byte-identical to master's.** All four new
weights that could change behaviour default to 0.0: `unit_strength_credit`,
`hand_mil_potential`, `tactic_gain`, `tactic_short`. `territory_credit` is
1.0 but is gated behind `hand_mil_potential`, so it costs nothing either.

`unit_strength_credit` is the interesting one, because the precedent in
[`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md#23-one-weight-that-is-not-00) §2.3 argued the opposite way for `card_rate_credit`
(1.0, live, "0.0 would leave the champions playing blind"). Two measurements
say units are not that case:

1. **At 1.0 it is a no-op for every trained vector.** `champion_2p` against
   itself with the credit flipped is 60 games **byte-identical** — same win
   rate, same cultures, mirrored seat by seat. Not "not significant":
   identical. The mechanism is §11.5.1 below.
2. **1.0 is not privileged the way it is for culture.** For `culture` → 
   `culture_rate`, 1.0 is exactly what the engine does with the key. For
   strength it is not: the board expresses one point of strength through
   *four* features — `strength` (0.150), `strength_rel` (0.193), and
   `strength_lead` (0.267) or `strength_deficit` (−0.736) — while
   `card_potential` looks up only the first. So 1.0 is somewhere between a
   2.3× and a 7× under-count of the truth, and there is no defensible
   constant, only a weight.

Choosing 1.0 would therefore have moved digests to buy behaviour that no
experiment on a trained vector can detect. Both arms were derived in full so
the number exists without anyone re-deriving it:

| arm | master / this branch | `unit_strength_credit` = 1.0 |
|---|---|---|
| narrow (greedy) | `0a6ed6ad` | `0a6ed6ad` |
| wide (greedy) | `4a8c6ca6` | `4a8c6ca6` |
| weighted narrow | `5eff41eb` | **`beba1c96`** |
| weighted wide | `d03e0964` | **`da252e5d`** |
| quiescent narrow | `eff1bef5` | `eff1bef5` |
| quiescent wide | `9e9695d4` | `9e9695d4` |
| plan narrow | `c534ac3d` | **`b896b53a`** |
| plan wide | `ee627d64` | `ee627d64` |

Three of eight, and the pattern is itself informative: QuiescentBot is
untouched on both arms and PlanBot only on the narrow one, i.e. even among
bots that search under this evaluator the term almost never wins an argmax.
`TestLaneBWeightsAreInert` fails if a default is flipped without noticing.

### 11.4. What the bot actually does with military cards

`tools/behaviour_counts.py` gained `play_tactic`, `copy_tactic`, `build_unit`,
`with_tactic_end` and `unit_workers_end` for this, because "did it play more
tactics" and "did it build more units" are meaningless apart.

Frozen 2p champion, mirror table, 30 games, per game:

| counter | value |
|---|---|
| `play_tactic` | 3.8 |
| `copy_tactic` | 10.5 (2 military actions each) |
| `build_unit` | 1.6 |
| `with_tactic_end` | **2.0 of 2 players** |
| **`unit_workers_end`** | **0.167** |
| `colonies_held_end` | 0.167 |
| `bid` | 0.167 |

Both players end every game holding a tactic, and across 30 games there were
**five unit workers standing on the whole table** — one every six games. About
**29 military actions a game** go into playing and copying tactics that can
form no army at all. That is not a pricing subtlety, it is a strict waste, and
it is the deadlock of §11.2.4 seen from outside. (The last two rows are §11.5.3's
subject: colonization happens about as often as a unit survives.)

Why the bot never buys a unit, over 12 games / 2264 plies of self-play:

| | |
|---|---|
| plies with a unit card in the row | 1681 (74%) |
| plies where a unit was **legally takeable** | 684 (30%) |
| `take` moves chosen | 377 |
| ...of which a unit card | **1** |
| `develop` moves chosen | 194 |
| ...of which a unit tech | **1** |
| unit cards ever in the civil hand | **1 ply in 2264** |

The bottleneck is `take`, not `develop`: `best_unit` carries a weight of 1.03,
so developing a unit tech is well rewarded — the bot just never has the card.

### 11.5. Results

#### 11.5.1 Units: a null, and the reason it cannot be otherwise

`unit_strength_credit` 1.0 vs 0.0, paired on the deal, was **60 games
byte-identical** (§11.3), so the A/B was stopped rather than extended: an
experiment cannot resolve an effect on games that are the same games.

The reason is the table in §11.4 and it generalizes past this fix. At credit 1.0
the mapping raises Swordsmen from −1.66 to −1.36 and Modern Infantry from
−4.00 to −3.25 (at the shipped 0.0 they stay at the §11.1.1 values, since the
credit multiplies the amount).
**It never flips a unit's sign**, at any weight vector in this repo: under
`DEFAULT_WEIGHTS` (`strength` 0.35) Swordsmen is still −2.2. Since the sign
does not flip, `row_pressure` still skips every unit card, and the only
surviving channel is `hand_potential` at 0.125 × ≈0.3 ≈ 0.04 eval points —
against a card the bot holds once in 2264 plies.

Adding `strength_rel` to the emission (defensible: `rel = strength −
rival_strength`, so d(rel)/d(strength) = 1 unconditionally) was considered and
does not help either: it takes a point of strength from 0.150 to 0.343, and
Modern Infantry from −3.25 to −2.28. **The only term large enough to flip a
unit positive is `strength_deficit` at −0.736, and that one is conditional on
being behind — i.e. it is a board query, not a card constant.** So this is a
clean, specific statement of what a board-aware card evaluator would buy that
a table cannot: *units are the card type whose value is dominated by a
board-conditional term*, and no per-card table will ever price them.

This is the §5.1 finding of [`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md) — "giving a card a weight
does not help until the bot takes the card" — with the causal chain filled in
rather than inferred from a variance census.

#### 11.5.2 Which knobs can change a game at all — run this first

[`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md#51-finish-discipline-a-null-and-the-reason-is-more-interesting-than-the-null) §5.1 spent 1200 games finding out that
`wonder_overrun` never once won an argmax, and only noticed because three
weights differing by a factor of eight produced *bit-identical* games. That is
a deterministic fact about the evaluator and it costs seconds to measure
directly. `tools/argmax_divergence.py` plays reference games under the base
vector and, at every decision with more than one legal move, asks whether the
arm would have picked differently. Both arms see the same states, so the
output is an exact paired divergence rate — and a rate of zero is *proof*, not
an underpowered null.

Six games of 2p champion self-play, **967 decisions**:

| arm | weight | decisions changed | rate |
|---|---|---|---|
| `unit_strength_credit` | 1.0 | **0** | 0.00% |
| `unit_strength_credit` | 3.0 | 1 | 0.10% |
| `tactic_gain` | 0.15 | **0** | 0.00% |
| `tactic_gain` | 0.4 | **0** | 0.00% |
| `tactic_gain` | 1.0 | **0** | 0.00% |
| `tactic_short` | −0.15 | 1 | 0.10% |
| `tactic_short` | −0.4 | 1 | 0.10% |
| `tactic_short` | −1.0 | 3 | 0.31% |
| `hand_mil_potential` | 0.05 | 7 | 0.72% |
| `hand_mil_potential` | 0.125 | 9 | 0.93% |
| `hand_mil_potential` | 0.25 | 14 | 1.45% |
| `hand_mil_potential` | 0.5 | **19** | **1.96%** |

Four of the five knobs were retired in six games.

**`tactic_gain` is a dead coordinate**, and this is the `wonder_overrun`
outcome exactly: zero divergences at three weights spanning a factor of
seven. It is worth being blunt about why, because it was the feature I
expected most from. `tactic_gain` is the strength you would gain by playing
the best tactic you can reach — and §11.4 says the champion **already holds a
tactic in every game** and has **zero units**, so the best reachable tactic
adds 0 and the one in play also adds 0. The term is 0 on every candidate
move. A feature designed to break a deadlock cannot break it from the side
the bot is already stuck on.

`tactic_short` fires, barely, and only at −1.0. It is the gradient toward
building the unit that completes an army, and the bot cannot build a unit it
has not developed, and has not developed one (§11.4). The deadlock is three
moves deep — take, develop, build — and `tactic_short` only rewards the last.

`hand_mil_potential` is the only arm that earns a full A/B, and its
monotonicity in the weight is the sanity check that the term is wired up.

#### 11.5.3 Territories

**Behaviour first, because it frames the win rate.** Mirror table, 30 games,
`hand_mil_potential` 0.5 (the strongest arm in §11.5.2) against the base
champion:

| counter | base | `hand_mil_potential` 0.5 |
|---|---|---|
| `bid` | 0.167 | 0.0 |
| `colonies_held_end` | 0.167 | 0.0 |
| `prepare_event` | 23.9 | 22.5 |
| `take` | 29.5 | 31.2 |
| `unit_workers_end` | 0.167 | 0.1 |

The first two rows are the point, and they are not really a difference
between arms — they are a statement about the game these bots play.
**Colonization essentially never happens: one bid and one colony every six
games.** The 12 territories were repriced from 0.00 to a well-ordered
0.46–13.40, and the decision that pricing informs is taken about six times per
hundred games. n = 30 cannot distinguish 0.167 from 0.0 and I am not claiming
it does.

The one directional signal is `prepare_event`, 23.9 → 22.5, and it is in the
**unhelpful** direction for a reason worth writing down. A territory in hand
cannot be played; the only thing you can do with it is `prepare_event`, which
seeds it into the event deck for an auction *anyone* can win. So putting a
positive value on a territory **in hand** prices holding it, and holding it is
worth nothing. `hand_mil_potential` as built makes the bot marginally more
reluctant to do the one useful thing available. The right shape is almost
certainly a term on the *timing* of `prepare_event` — seed the big territory
when your `max_force` beats the table's — and that is a board query, not a
card constant. Same conclusion as units, arrived at from the other end.

**The A/B**, `hand_mil_potential` 0.125 against the identical vector at 0.0 —
same file, one key — paired within each call by seat rotation, disjoint blocks
of 100 games:

| | n | win rate | culture margin | own culture |
|---|---|---|---|---|
| `hand_mil_potential` 0.125 vs 0.0 | **800 games / 400 deals** | **50.56% ± 1.89pp** (z = 0.58, p = 0.56) | **−0.15 ± 1.07** (z = −0.28) | 146.7 |

Eight disjoint blocks of 100: 51.0, 55.0, 46.5, 49.0, 47.0, 52.5, 48.0, 55.5
(chi2 = 11.62 on 7 df). Scattered either side of 50 with no block carrying
anything. **A flat null**, on both the win rate and the dense margin signal.

**MDE = 2.70pp**, clustered on the deal via `tools/ab_summary.py`
(`experiments/paired_stats.py`), not the independent-samples formula.

That correction is a *tightening* here, not the sqrt(2) widening I had
assumed while writing this up. The within-deal correlation is
rho = **−0.699**, strongly negative — these deals favour a *seat* rather than
a strategy — so the naive interval was conservative. Naive: ±3.46pp on the
win rate, ±2.88 on the margin. Corrected: **±1.89pp** and **±1.07**.

So this is a **well-powered null, not an underpowered one**: effects above
2.7pp are excluded, on both the win rate and the dense margin. Which was
predictable — §11.5.2 measured the term changing 0.93% of decisions, so an
effect that size was never on the table. It is a bound, not a discovery, and
the top of this section says why the bound is uninteresting: the decision it
informs happens 0.167 times a game.

**And it is the wrong test, which Lane C established after I had started it.**
[`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md#52-3p-does-it-transfer) §5.2: a fresh 0.0-default feature does nothing until
the league learns its weight, so switching a credit on hands a *frozen* vector
each card's upside at full price and its downside at zero. Every card in this
lane is cost-bearing — a unit costs resources and a worker, a territory costs
units sacrificed into a bid, a tactic costs a military action and is dead
weight unfilled. A tighter CI on this comparison would be a better-measured
wrong test. The number is here because a **large negative** would mean the
implementation is broken and worth looking at; −0.15 culture on 800 games says
it is not. The real validation is the league.

A note on method, because I got it wrong first. `arena.duel` maps
`seed = seed0 + g // num_players`, so a block of 100 games at 2p consumes 50
deals starting at `seed0`. My first attempt used `seed0` 11/22/33/44, which
overlap by 78%, and it showed up immediately as two "independent" blocks
returning identical win rates (53.5/53.5, 49.5/49.5). Those were not
replications, they were near-duplicates. Blocks below are spaced by
`games / players` = 50 and are genuinely disjoint.

#### 11.5.4 Tactics are a plumbing problem, not a pricing problem

Checked after the wonder lane showed that repricing 8 wonders moved wonder
completions by a measured zero, because a wonder never enters `hand_civil` and
so reaches the policy only through a take-timing heuristic. The same question
for tactics: **which term actually carries a tactic's value into the policy?**

> **Note on the premise (2026-07-30).** That "measured zero" was an arithmetic
> identity, not a result: the take-timing heuristic is gated on `row_urgency`,
> which is 0.0 in the frozen champion, so the wonder reprice could not have
> moved anything. See [`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md#53-the-mechanism-is-not-wonders-and-the-reason-is-a-plumbing-bug) §5.3 and
> [`analysis/frozen/README.md`](../analysis/frozen/README.md). **This section's own finding does not depend on
> that premise and is unaffected** — every feature in the table below is a
> plain `features()` term consumed by the ungated weight loop in `evaluate()`,
> so nothing here was switched off. It was re-run against the live 2p league
> champion and got *stronger*; see the second table.

Two measurements. First, there is no card-pricing path at all — with
`hand_mil_potential`, `territory_credit` and `card_board_credit` all switched
ON, `card_potential` is **0.0 for all fifteen tactics**, and
`_card_yields("Modern Army")` is the empty tuple. `tacticBonus` is a board
query, so no table reaches it.

Second, and this is the sharp one. Over **374 decisions where a `play_tactic`
or `copy_tactic` move was legal**, how often each feature differs across the
candidate moves, and its mean `|weight| × range` — a feature can only change a
decision through that product:

| feature | varies | mean \|w\|×range |
|---|---|---|
| `hand_military` (card **count**) | 94.9% | **0.464** |
| `hand_mil_value` (sum of age+1) | 96.8% | **0.283** |
| `tactic_level` (the tactic's **age**) | 44.1% | 0.267 |
| `ma_left` | 100.0% | 0.080 |
| `strength_rel` | 32.9% | 0.085 |
| `strength_lead` | 20.9% | 0.078 |
| **`strength`** (the armies it actually forms) | **32.9%** | **0.066** |

The three identity-blind bookkeeping terms — how many cards are in the
military hand, the sum of their age levels, and the tactic's own age —
outweigh what the tactic is actually worth by about **11 to 1**. Pricing a
tactic correctly cannot matter while that is true, and that is a statement
about plumbing, not about the price.

**Re-run against the live 2p league champion (gen 54, 99 keys), 451
decisions.** The ratio is a property of a weight vector, so it has to be
re-measured on the bot the league actually trains rather than on the frozen
snapshot:

| | frozen 2p (n=374) | live 2p (n=374 → 451) |
|---|---|---|
| `hand_military` | 0.464 | **0.0** |
| `hand_mil_value` | 0.283 | **1.900** |
| `tactic_level` | 0.267 | **0.0** |
| bookkeeping subtotal | 1.014 | **1.900** |
| `strength` | 0.066 | 0.070 |
| **ratio** | **15.4 : 1** | **27.3 : 1** |
| incl. `ma_left` + `military_actions` | — | **60.4 : 1** |

**The finding survives and gets worse.** The live champion routes its
bookkeeping through a different term — `hand_mil_value` instead of
`hand_military` — but the shape is identical and the imbalance nearly doubles.
It is also *more* blind than the frozen one: `strength_rel` and
`strength_lead` are both 0.0 in the live vector, so `strength` at 0.070 is the
only remaining term that reads what the tactic is actually worth.

It also explains §11.4's strangest number mechanically. `copy_tactic` costs 2
military actions but takes **no card out of hand**, so `hand_military` and
`hand_mil_value` are unchanged while `tactic_level` rises: a clean +0.27
against `ma_left` at 0.080. Playing a tactic *from hand* removes the card and
gives up 0.464 + 0.283. **The evaluator therefore prefers copying to playing,
and the champion copies 10.5 tactics a game.** It is paying two military
actions a time to avoid the bookkeeping penalty for having a smaller hand.

So the §11.5.2 null on `tactic_gain` should be read narrowly. `tactic_gain` and
`tactic_short` are state features evaluated on every candidate, so unlike card
pricing they are not blocked by the plumbing — their 0-of-967 is a genuine
measurement of the feature. But it is a measurement taken on a board where the
champion already holds a tactic and owns no units, so the best reachable
tactic and the one in play are both worth zero. The feature is not wrong; the
position it was built to detect never arises, because the plumbing above stops
the bot ever getting units.

### 11.6. What is still broken, and what would actually fix it

This lane closes the *pricing* gap for 22 of its 37 cards and leaves a precise
statement of why the other 15 are not a pricing problem at all.

* **Units need a board-conditional term, not a table entry.** §11.5.1: the only
  weight large enough to make a unit card worth taking is `strength_deficit`,
  which is conditional on being behind. A per-card table is structurally
  incapable of it. This is the single most concrete thing a board-aware card
  evaluator — `(name, state, idx, w)` rather than `(name, w)` — would buy, and
  it is a sharper case for that interface than the four Age III wonders,
  because units are 10 cards the bot needs and the wonders are 4 it can skip.
* **The tactic deadlock is three moves deep, and `tactic_short` only rewards
  the last.** take → develop → build → play. `tactic_gain` and `tactic_short`
  both read unit *workers*, so neither rewards taking or developing the tech
  that would let you build the unit. Breaking it needs either a deeper search
  or a term that credits a developed-but-unbuilt unit technology.
* **`copy_tactic` is a live pathology and it is not a card-pricing bug.** §11.4:
  10-12 copies a game at 2 military actions each, into zero armies. It is
  trained-weight behaviour — `tactic_level` is +0.44 and `ma_left` is −0.04,
  so *spending* two military actions to raise the tactic level scores as a
  gain twice over. None of the features here touch it. Somebody should.
* **Governments have the unit bug.** Top-level `civilActions` /
  `militaryActions` that `_card_yields` never reads (§11.1.2). Written off, not
  fixed; another lane's card type.

### 11.7. Reproducing

The census delta this lane is responsible for, measured on top of Lane C's
tree: **territories 12 -> 0 and units 10 -> 0 cards with zero visible gain**,
taking the repo total from Lane C's 146 to **124**. `--legacy` still
reproduces master's 171 dropped / 168 blind to the card, which is the check
that the baseline every result is measured against has not quietly moved --
`use_legacy_maps` clears `_UNIT_TO_FEATURE` and `_TERR_TO_FEATURE` along with
the rest.

```bash
python3 -m tools.card_blindness                    # 168 -> 124 blind
python3 -m tools.card_blindness --legacy           # still 171 / 168
python3 -m unittest tests.test_card_pricing        # 38 cases
python3 tools/behaviour_counts.py --players 2 --games 40 \
    --spec analysis/frozen/champion_2p.json --label champ2p
bash tools/gate.sh                                 # GATE PASS, no digest moved
```

## 12. Every card type, measured: what the bot plays and what can reach the policy (2026-07-30) (merged from the former `CARD_CENSUS.md`, 2026-07-31)

[`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md) asked what the evaluator can *see* on a card and
fixed a real omission. Then eight wonders were repriced and wonder
completions moved by a measured **zero** (0.0997 → 0.1047, p=0.12, n=12,800
seat-games). The pricing was right and it bought nothing, because a wonder's
price has no wire to the policy. Nothing in the test suite noticed, and
nothing could have: the suite checks that a card is priced, never that its
price is *read*.

This document generalises that failure into an instrument and runs it over
all 236 cards.

### One-paragraph answer

Two questions, asked separately and then crossed. **Does the bot play this
card?** — `tools/card_census.py run/report`, a per-card lifecycle census over
12,087 real games, conditional on availability. **Can this card's value reach
the policy at all?** — `tools/card_census.py probe`, which reproduces
`WeightedBot.pick` exactly and asks whether, at a real decision, the score of
a candidate depends on **which card it is**. Then a third question decides
the ranking: **does the search the league actually trains repair it?** — the
whole census re-run under `plan:width=2`.

The answer, over the 23 types and 236 cards: **4 broken kinds spanning 7
types and 93 cards; 2 types (14 cards) that only looked broken at 1 ply; 3
types (26 cards) that are real problems but not mispricings; and 11 types
(103 cards) healthy.**

Underneath all of it is one structural fact that took the whole audit to see
plainly: **each frozen champion is a 78-key file and the evaluator [at the
time this was written] had 112 weights, so 34 of the shipped policy's
weights were never trained, and 28 of those default to `0.0`.** The entire
card-identity channel is a single untrained weight — `hand_potential` =
0.125 — and everything that does not flow through it flows through a zero.
That count is *growing*: it was 110 weights when this census was measured
and 112 by the time it landed, because sibling lanes are correctly adding
pricing behind 0.0 defaults faster than anything turns one on. See §12.9. **By
2026-07-30 it reached 118**, and the live `experiments/league_state/
champion_{2,3,4}p.json` grew to 118 keys along with it — see the box above
and [`docs/OPEN_ITEMS.md`](OPEN_ITEMS.md#3-evaluator-information-gaps) §3. This paragraph's "78 vs 112" gap is
about the *frozen* snapshots this census actually ran against, which are
still 78-key and still accurate as described; it is not a current statement
about the live evaluator.

The headline is therefore that the wonder pipe is not weak, it is **severed**:
`row_urgency` is `0.0` in all three frozen champions — they do not contain
the key and `load_weights` fills it from a `0.0` default — so a wonder's
`card_potential` is multiplied by zero before it reaches anything. The probe
measures the consequence directly: the policy ranks two wonders against each
other at **concordance 0.525 / 0.534 / 0.383** at 2p/3p/4p against their own
priced value — a coin flip at 2p and 3p and *worse than chance* at 4p, on the
largest value spread in the deck. What survives the severing is
`wonder_remaining`, a weight on the wonder's **cost**, and the census catches
that pipe red-handed: the wonder take rate varies **76×** across the three
champions (0.006 → 0.031 → 0.454) tracking the sign of that cost weight, and
**1.7×** across a 14.4× value range within any one of them.

> **The severing is real and is a property of the FROZEN champions only
> (2026-07-30).** This section is scrupulous about saying "in all three frozen
> champions", and §12.8 re-derived it from `load_weights` rather than
> `DEFAULT_WEIGHTS` for exactly the right reason. The gap it could not see is
> that **the frozen champions are not the bot the league is training.** They
> are a 2026-07-26 snapshot of a 78-key climb; the live league champions carry
> **99** keys, and `experiments/league_state/champion_2p.json` has
> **`row_urgency = −0.19109`**. The wire is connected on the live bot.
>
> Re-running [`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md#53-the-mechanism-is-not-wonders-and-the-reason-is-a-plumbing-bug) §5.3's wonder A/B against the live 2p
> champion on the same 12,800 seat-games moves wonder completions
> **+0.5731 (+88%, p<1e-4)**, against the −0.0050 null (MDE 0.0089) the frozen
> vector produced. See §5.4 there (in `docs/CARD_BLINDNESS.md`) and [`analysis/frozen/README.md`](../analysis/frozen/README.md).
>
> **UPDATED 2026-07-30: the 99-key figure above is itself superseded.** The
> live `experiments/league_state/champion_{2,3,4}p.json` now carry **118**
> keys, matching `engine/bots/weighted.py`'s `DEFAULT_WEIGHTS` exactly (no
> evaluator key is missing from any of the three live champions any more —
> the untrained-weight gap this document is about is closed for the live
> bot, though the *frozen* snapshots under `analysis/frozen/` remain the
> 78-key files described throughout, unchanged). The count keeps growing, so
> treat any hardcoded key count in this document as dated to when it was
> written, not as current. `row_urgency = −0.19109` for the live 2p champion
> is unchanged from when this box was first written.
>
> What this does and does not cost the census:
> * **The plumbing map is untouched and is the durable contribution.** A
>   wonder really does reach the policy through `row_pressure` alone. That is
>   a fact about `engine/`, not about a vector.
> * **"CONFIRMED BROKEN (Tier A)" for wonders should read "confirmed broken in
>   the frozen champions".** On the live vector the pipe carries an effect
>   large enough to shift the zero-wonder share from 43.5% to 19.3%.
> * **The concordance numbers (0.525 / 0.534 / 0.383) are frozen-vector
>   numbers.** 2p has been recomputed; 3p and 4p were blocked on there being
>   no live reference, which is now fixed —
>   `analysis/frozen/champion_3p_gen1255_99key.json` and
>   `champion_4p_gen350_99key.json` are cut and carry `row_pressure` open, so
>   both can be redone. **Read the 3p caveat in [`analysis/frozen/README.md`](../analysis/frozen/README.md)
>   first:** the 3p champion's `row_urgency` is `+0.16269`, the wrong sign for
>   a post-move residual, and a seed-paired A/B (n=600) shows flipping it is
>   worth `+0.0025 ± 0.0305` — a tight null. The weight is active on 35% of
>   decisions but has no gradient at the strength level, so **3p card-ordering
>   concordance is measured against an arbitrary sign.** That is a real
>   caveat on any recomputed 3p concordance figure, not a blocker.
> * **The 4p column is separately unreliable** — `analysis/frozen/champion_4p`
>   is the known-degenerate vector; see [`analysis/frozen/README.md`](../analysis/frozen/README.md).
> * **The ranking in §12.4 may reorder.** Wonders were ranked suspect #1 on the
>   strength of a severed pipe. Territories (`hand_mil_potential = 0.0`) were
>   still 0.0 in the live champions when this was written. **UPDATED
>   2026-07-30:** `hand_mil_potential` is now nonzero on the live 3p champion
>   (`0.01079`, confirmed by reading
>   `experiments/league_state/champion_3p.json`), though still `0.0` at 2p
>   and 4p — see [`docs/OPEN_ITEMS.md`](OPEN_ITEMS.md#3-evaluator-information-gaps) §3. The pipe is no longer
>   uniformly severed across player counts, which weakens "the better
>   candidate for a pure severed pipe" at 3p specifically; 2p and 4p are
>   unaffected.
>
> The generalisable lesson survives intact and is arguably the real finding:
> **a weight at 0.0 makes an A/B return a null that is an arithmetic identity,
> indistinguishable from a measured negative.** That is now enforced rather
> than documented — `experiments.arena.assert_lever_conducts()`.

Three findings I did not expect and would have got wrong without the
controls. **`war` is declared 0 times in 71,229 draws at 1 ply — and 357
times in 2,220 under `plan:width=2`.** Search repairs a missing rollout; it
cannot repair a severed wire, and that single split is what separates the
real bugs from the 1-ply artefacts. **Military units are the opposite defect
from wonders**: their pipe is live and carries an actively *negative* number
(`card_potential` −4.40…−0.57, because `unit_strength_credit` is 0.0), so
holding a unit card lowers `hand_potential` — take rate 0.0091 over 742,091
offers, essentially unmoved by search. And **`cost.militaryActions` on 54
cards is the government bug still open**: a top-level field the rules engine
gates legality on and no card-pricing path reads.

### 12.1. The instrument

Two subcommands, both landed in `tools/card_census.py`, both re-runnable
after any evaluator change. Neither touches the engine: the census replays
`game.play_game`'s own loop and diffs a snapshot across each *real* `apply`,
so the trial states inside the bot's search are never counted, and the probe
reuses `evaluate` / `rival_context` / `copy_state` unmodified.

#### 12.1.1 What "conditional on availability" means, and why the denominator is per type

A raw count is worthless here: a card that appears rarely and is always taken
is healthy, and a card that is offered constantly and never taken is the
signal. So every rate has a denominator, and the denominator is chosen from
`card["deck"]` — the engine's own field — rather than from whichever counter
happens to be non-zero:

| | civil deck (127 cards) | military deck (109 cards) |
|---|---|---|
| how it becomes available | dealt into the open row | dealt straight into `hand_military` |
| `offered` | **player-turns on which the mover could LEGALLY take it** — `actions._can_take_gated`, the real rule: reach, hand limit, duplicate leader age, mid-wonder | n/a, there is no row |
| the take question | `taken / offered` | n/a |
| the play question | `played / taken` | `played / drawn` |

`offered` is sampled once per player-turn, not once per decision, so it
counts opportunities rather than evaluations.

**`played` is not one thing**, and getting it wrong in either direction ruins
the census. It is read off the **move tuple** wherever a move exists, because
that is exact, and from a container diff only for the transitions that have
no move of the holder's own. The traps, each of which the first draft of the
tool got wrong and the smoke test caught:

* a tactic can enter play with **no card** via `("copy_tactic", n)`, so
  counting `p.tactic` transitions double-counts against `drawn` (it reported
  a play rate of 1.395);
* a territory is *prepared* like an event and only later *colonized* — two
  different rates, and the second is not the holder's decision;
* a **refused pact returns to the hand** (`interact.py:228`), so a hand
  departure is not a play and a hand arrival is not a draw;
* a bonus card has **no move handler at all** — it is only ever spent inside
  the defense / colonization machinery;
* a wonder's play is its **completion**, not its take.

That table is `PLAYED_BY` in the tool, and it is **coverage-checked at
runtime** against the card DB rather than left as a comment: a card type with
no entry, or an entry for a type that no longer exists, is a hard error. A
play rate whose definition nobody wrote down is worse than no play rate.

#### 12.1.2 The probe: does card identity move the score?

The census says what the bot does. The probe says why, and it is the part
that turns "seldom played" into "wonder-class defect" or "just a bad card".

At every real decision it groups the legal moves by `(move kind, card type)`
and, for each candidate of the same kind and type, records the evaluation the
policy actually saw alongside that card's `card_potential`. Then, per group:

* **`flat`** — fraction of decisions where every candidate scored
  *identically*. If two different events always score the same, no amount of
  event pricing can change the choice.
* **`SEVERED`** — of the decisions where `card_potential` **did** differ
  across candidates, the fraction where the score still did not. `1.000`
  means the priced value cannot reach the policy at all.
* **`concordance`** — over candidate pairs where both the score and
  `card_potential` differ, how often they agree on which card is better.
  `0.5` is a coin flip: the score is moving for some reason *other than* the
  card's value. This is the number that survives the case the other two miss
  — a wonder's score moves with its **cost** whatever its value, so `flat`
  and `SEVERED` both look mild and `concordance` reads 0.5.

Sanity check on the metric's sign: `destroy | library` reads concordance
`0.000` at every player count, which is correct — destroying your most
valuable card *should* be your worst option, and a metric that could not
produce a 0 would not be measuring direction.

#### 12.1.3 It fails loudly, and only when it should

`tools/card_census.py check --baseline analysis/census/baseline.json` is the
thing that is supposed to notice next time. Getting it to be worth reading
took three corrections, each of which I shipped wrong first, and each of
which is a instance of the same failure this document is about — a check that
cannot see the thing it exists to see.

1. **A pure ratio test cannot fail a type whose baseline is already zero.**
   `rate < 0 × (1 − tol)` is never true, so the obvious implementation would
   have permanently *blessed* every type found broken here; `war` at 0
   declarations in 71,229 draws would have passed forever. The baseline
   therefore records zero types **by name** per arm (`known_zero`, today
   `["war"]` at all three counts), prints them as a standing `ZERO` defect,
   and **FAILs any type that reaches zero and is not on that list**. Fixing
   war means deleting it from `known_zero`, after which a regression fails.
2. **Rates are not comparable across player counts,** so the baseline is
   stored **per arm** and compared like-for-like. Territory plays at 0.708 at
   2p and 0.146 at 4p; pooling them meant a 3p-only run reported a change in
   the *mix* as a change in the bot.
3. **A gate that cries wolf gets turned off.** Both tests are gated on
   *expected count* rather than sample size (`held × baseline_rate ≥ 5` to
   call a zero, `≥ 10` to trust a ratio), and a ratio drop must additionally
   be **significant**, `z ≤ −3`, not merely larger than `--tol`. Without that
   last guard, 23 types per arm produce a scary-looking failure from ordinary
   binomial noise about every other run — territory at 3p came in at 0.317
   against a 0.497 baseline, which is `z = −2.3`, a 2% event that 23 tests
   give you for free. Under-powered types are reported as such, with the
   sample size they would need.

Negative controls, run before trusting it:

| control | result |
|---|---|
| unmodified 6-game sample | **PASS**, `1 known-zero: 3p:war`, territory dip explained as `z=-2.3` noise |
| claim library was 0.990 at 3p | **FAIL**, `z=-23.2`, exit 1 |
| blank `known_zero`, claim war was 0.400 | **FAIL**, `expected 14.0`, exit 1 |
| aggression at 0/79 (true rate 0.013) | **note, not a failure** — "under-powered, need ~420 acquisitions" |

### 12.2. The plumbing map

Traced in code, not inferred. The two columns that matter are the last two:
which term carries this card's **identity** into the policy, and **what
weight does that term actually have** in the shipped vectors. The verdicts
are the ones §12.4.1 arrives at, after the search control in §12.4.0 — a type that
is dead at 1 ply and alive under `plan:width=2` is Tier B, not a bug.

**The critical fact, and reading `DEFAULT_WEIGHTS` alone will not give it to
you.** Each frozen champion is a **78-key** file. The evaluator had **112**
weights at the time this was written (110 when the census was measured at
`50ba471`; **118 as of 2026-07-30**, and still growing — see the note in the
"One-paragraph answer" section above). `load_weights`
fills the gap from `DEFAULT_WEIGHTS`, so **34 of the 112 weights in the
shipped policy were never trained** — they were added after the champions
were frozen, and every one of them sits at whatever default it was born
with. **28 of those 34 defaults are `0.0`.**

Among the 32 untrained: `row_urgency`, `row_bargain_forgone`,
`rival_hand_potential`, `hand_mil_potential`, `tactic_gain`, `tactic_short`,
`card_board_credit` and `unit_strength_credit` — all `0.0`. The six with a
non-zero default are `hand_potential` (0.125), `card_rate_credit` (1.0),
`territory_credit` (1.0), `auction_committed` (2.0), `auction_bid` (−0.4) and
`pact_blocks_attack` (0.5).

So the *entire* card-identity channel of the shipped policy is one untrained
weight, `hand_potential` = 0.125, and everything that does not flow through
it flows through a zero. That is the whole of §12.4 in one sentence, and it is
why "the weight exists, defaulted to 0.0, so the trainer can decide what it
is worth" — the reasoning [`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md#22-nine-new-weights-all-defaulting-to-00) §2.2 uses, correctly, for
a *new* channel — quietly stops being true once the champions that would do
the deciding are frozen and the leagues warm-start from them.

| type | n | after acquisition (file:line) | identity term | weight | verdict |
|---|---|---|---|---|---|
| farm | 4 | `hand_civil` `actions.py:699` | `hand_potential` | **0.125** | healthy |
| mine | 4 | `hand_civil` | `hand_potential` | **0.125** | healthy |
| lab | 4 | `hand_civil` | `hand_potential` | **0.125** | healthy |
| temple | 3 | `hand_civil` | `hand_potential` | **0.125** | healthy |
| library | 3 | `hand_civil` | `hand_potential` | **0.125** | healthy |
| arena | 3 | `hand_civil` | `hand_potential` | **0.125** | healthy |
| theater | 3 | `hand_civil` | `hand_potential` | **0.125** | healthy |
| special-tech | 12 | `hand_civil` | `hand_potential` | **0.125** | healthy |
| action | 33 | `hand_civil` + `taken_this_turn` `actions.py:702` | `hand_potential`; play resolves on the board | **0.125** | healthy |
| leader | 24 | `hand_civil` `actions.py:699` | `hand_potential` (printed) + `board_yields` | **0.125** / 0.0 | healthy, board half inert |
| government | 8 | `hand_civil`, never `p.techs` | `hand_potential` (printed) + `board_yields` | **0.125** / 0.0 | healthy at take; top-level fields inert, Tier C |
| pact | 10 | `hand_military` | `deferred_credit` prices the pending offer into `features()` | **live** | healthy |
| infantry | 4 | `hand_civil` | `hand_potential`, but `strength` gated on `unit_strength_credit` | **0.125** × **0.0** | **CONFIRMED BROKEN (wrong sign)** (Tier A) |
| cavalry | 3 | `hand_civil` | same | same | **CONFIRMED BROKEN (wrong sign)** (Tier A) |
| artillery | 2 | `hand_civil` | same | same | **CONFIRMED BROKEN (wrong sign)** (Tier A) |
| air | 1 | `hand_civil` | same | same | **CONFIRMED BROKEN (wrong sign)** (Tier A) |
| **wonder** | 16 | **`p.wonder`** `actions.py:696` — never a hand | **`row_urgency` only** | **0.0** | **CONFIRMED BROKEN** (Tier A) |
| **event** | 55 | `hand_military` | `hand_mil_potential` (and `_card_yields` returns `()` for all 55) | **0.0** | **CONFIRMED BROKEN** (Tier A) |
| **territory** | 12 | `hand_military` | `hand_mil_potential` | **0.0** | **CONFIRMED BROKEN** (Tier A) |
| war | 3 | `hand_military` | `hand_mil_potential`; resolution is a round later, `_h_war` pushes nothing onto `pending` | **0.0** | **dead at 1 ply, repaired by search** (Tier B) |
| aggression | 11 | `hand_military` | `hand_mil_potential`; resolution deferred via `state.pending`, not covered by `deferred_credit` | **0.0** | **dead at 1 ply, repaired by search** (Tier B) |
| tactic | 15 | `hand_military` | `hand_mil_potential` + `tactic_gain`/`tactic_short` | **0.0** | consequence-priced only; Tier C |
| bonus | 3 | `hand_military` | none — **no move handler exists** | n/a | no agency, not a policy bug |

#### 12.2.1 Wonders: the archetype, and the mechanism is more specific than "blind"

`take_card` (`engine/actions.py:696`) branches on the type and puts a wonder
straight into `p.wonder`, so it never enters `hand_civil` and `hand_potential`
— the one live card-identity term — never walks it. The only other consumer
of `card_potential` on a row card is `row_pressure`, gated on `row_urgency`
and `row_bargain_forgone`, both **0.0**.

So what *does* change when the bot considers `("take", i)` on a wonder?
Exactly one identity-bearing feature: `wonder_remaining`, which becomes
`sum(stages)` — the wonder's **cost** — at a weight of −0.2355 (2p), −0.2118
(3p), +0.3391 (4p). **The pipe that survives carries the price tag and not
the goods.** That predicts concordance at or below 0.5, and more expensive
wonders (which are the better ones) scoring *worse*. Measured:

| | 2p | 3p | 4p |
|---|---|---|---|
| `take \| wonder` concordance | **0.525** | **0.534** | **0.383** |
| candidate pairs | 1542 | 2363 | 439 |
| mean `card_potential` spread | 8.10 | 10.74 | 22.93 |
| `take \| leader` concordance (control) | 0.940 | 0.843 | 0.968 |
| `take \| government` concordance (control) | 1.000 | 0.990 | 0.973 |

The 4p number below 0.5 is the prediction landing: at 4p `wonder_remaining`
is *positive*, and the ordering inverts.

This is a complete explanation of the null. Repricing wonders moved
`card_potential` from 3.95 to 27.45 on Eiffel Tower and the policy never saw
a single point of it.

#### 12.2.2 The military hand: same defect, four more types

`hand_mil_potential` was added to master tonight as the sibling
`hand_potential` never had. It defaults to **0.0**, so today the military
hand still reaches the evaluator only through `hand_mil_value` —
`sum(age_level + 1)` — under which a Vast Territory, a Fighting Band and an
Aggression of the same age are the same card.

Two flavours underneath that one gate, and they need different fixes:

* **Priced but not plumbed.** `_card_yields` returns real numbers for all 12
  territories (`card_potential` ranges 0.46 → 13.40, via `immediateEffects` /
  `permanentEffects`). The probe measures `prepare_event | territory` at
  **`SEVERED` 0.903** — the value differs and the score does not. Turning up
  `hand_mil_potential` alone fixes this type.
* **Neither priced nor plumbed.** `_card_yields` returns the **empty tuple**
  for all 55 events, all 15 tactics, all 10 pacts, all 3 wars, all 3 bonuses
  and 10 of 11 aggressions. `prepare_event | event` is **`flat` 0.897 /
  0.823 / 0.775** at 2p/3p/4p — most of the time *every event in hand is
  interchangeable*. Turning up `hand_mil_potential` changes nothing for
  these; they need a mapping first.

Note the pre-existing census got this backwards for half these cards.
[`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md#3-the-blind-spot-that-remains-written-down) §3 bucket 5 wrote off `tacticBonus` and friends as
"military hand: never reaches `_card_yields`", which is true, and the summary
table then reported all 55 events, 11 aggressions, 10 pacts and 3 wars as
"zero visible gain" — a claim about a function it never called on them.
Aggressions and wars are in fact priced by *resolution*, not by
`_card_yields`, which is a different mechanism with a different failure mode
(§12.2.3). **Verifying which path a type takes before claiming anything about it
is the discipline this document is trying to install.**

#### 12.2.3 War and aggression: priced by resolution, and the resolution is not in the trial

These two are not blind in the `_card_yields` sense. They are supposed to be
priced by *consequence*: play the move, look at the resulting board. That
fails at 1 ply for two different reasons, both verified:

* **Aggression** — `_h_aggression` → `events.start_aggression` →
  `interact.start_defense`, which pushes a **`defense` pending owned by the
  defender** (`interact.py:603-613`). The trial state therefore shows the
  card gone and the military actions spent, and none of the loot.
  `weighted.deferred_credit` hand-prices exactly two pending kinds —
  `pact_offer` and `auction` — and a `defense` pending is neither, so nothing
  credits it back.
* **War** — `_h_war` pushes **nothing** onto `pending` at all. It sets
  `p.war_declared_by_me` and the war resolves a full round later in
  `game.start_turn` (`game.py:229`). A 1-ply trial sees 2–3 military actions
  and a card, spent for a state change worth nothing to any feature.

`QuiescentBot` fixes the first by draining `pending` before scoring, and both
`QuiescentBot` and `PlanBot` special-case the second with
`quiescent.war_value`, which runs the real `resolve_war` on a scratch copy.
So this pair is **search-dependent** in a way the other defects are not, and
the census below is 1-ply. That caveat is stated again in §12.6.

#### 12.2.4 Units: the rarer defect — a live pipe carrying the wrong sign

Infantry, cavalry, artillery and air are civil-deck cards. They land in
`hand_civil` and `hand_potential` walks them at a live 0.125. The pipe is
fine. What comes down it is not:

`_card_yields` prices a unit's top-level `strength` through `_Y_UNIT`, which
`_CREDIT_OF` scales by `w["unit_strength_credit"]`, **default 0.0**. The
`techCost` and `buildCost` are *not* gated. So every unit card's
`card_potential` under the 2p champion is a bare cost:

| | infantry | cavalry | artillery | air |
|---|---|---|---|---|
| `card_potential` range | −4.00 … −0.57 | −3.80 … −1.86 | −3.60 … −2.63 | −4.40 |

Every one negative. **Holding a unit card in hand actively lowers
`hand_potential`**, and `row_pressure` skips any row card whose
`card_potential` is `<= 0` outright, so a unit card is invisible to the row
terms even if somebody turns them on. The census measures the result:
**0.0091** across **742,091 offers** pooled (6,779 taken), and **0.0017** at
2p specifically — 429 unit cards taken from 257,848 offers.

This is worth separating from the wonder class because the diagnosis is
opposite. A wonder's value cannot reach the policy. A unit's *anti*-value
reaches it perfectly.

### 12.3. The census

`tools/card_census.py run`, the frozen champion of each player count under
the 1-ply `WeightedBot`, on Paul's desktop at `nice`/idle priority.

**How many games this needs, and the answer.** The binding constraint is the
rarest card, not the average one. In a 2p game ~110 civil-card instances
enter the row and ~25 military cards are drawn per player, so a card's
availability accrues at roughly one observation per game per copy in the
deck; Age III cards, which only appear in the last third of a game, accrue at
maybe a fifth of that. Targeting ≥1,000 availabilities for the rarest card
puts the requirement at a few thousand games per player count. **Run:
12,087 games (2p 6,000, 3p 4,335, 4p 1,752), zero engine errors.** Median
availability is **~10,400 per card**, and the minimum over the 220 acquirable
cards is comfortably above 1,000 — this measurement is not sample-limited,
and the residual uncertainty in every rate below is in the third decimal.

**16 of the 236 cards have zero availability, and that is structural, not a
gap**: the six starting-tableau cards (Agriculture, Bronze, Despotism,
Warriors, Philosophy, Religion — `game.py:35,64`) and the ten Age A events,
which `game.py:91` seeds straight into `current_events` so they never enter
anyone's hand. They are not in a deck to be drawn. Every rate below is over
the **220 acquirable cards**.

| type | deck | n | offered | taken/drawn | take/offer | played | play/held | never played |
|---|---|---|---|---|---|---|---|---|
| event | military | 55 | — | 300,106 | — | 181,541 | 0.605 | 0/55 |
| action | civil | 33 | 1,959,231 | 105,077 | 0.054 | 47,513 | 0.452 | 1/33 |
| leader | civil | 24 | 737,260 | 59,297 | 0.080 | 55,603 | **0.938** | 0/24 |
| **wonder** | civil | 16 | 644,318 | 29,288 | **0.045** ‡ | 5,590 | **0.191** ‡ | 0/16 |
| tactic | military | 15 | — | 140,260 | — | 52,733 | 0.376 | 0/15 |
| special-tech | civil | 12 | 549,349 | 39,315 | 0.072 | 26,783 | 0.681 | 0/12 |
| territory | military | 12 | — | 62,791 | — | 33,407 | 0.532 | 0/12 |
| **aggression** | military | 11 | — | 151,335 | — | 2,005 | **0.013** | 2/11 |
| pact | military | 10 | — | 34,590 | — | 44,476 | 1.286 † | 3/10 |
| government | civil | 8 | 323,074 | 33,600 | 0.104 | 26,591 | 0.791 | 0/8 |
| farm | civil | 4 | 170,541 | 32,077 | 0.188 | 28,264 | 0.881 | 0/4 |
| mine | civil | 4 | 161,970 | 33,069 | 0.204 | 21,366 | 0.646 | 0/4 |
| lab | civil | 4 | 122,272 | 55,636 | 0.455 | 44,462 | 0.799 | 0/4 |
| **infantry** | civil | 4 | 226,216 | 1,812 | **0.008** | 1,117 | 0.616 | 0/4 |
| temple | civil | 3 | 70,585 | 33,802 | 0.479 | 32,945 | 0.975 | 0/3 |
| library | civil | 3 | 87,397 | 58,449 | 0.669 | 45,834 | 0.784 | 0/3 |
| arena | civil | 3 | 119,715 | 23,206 | 0.194 | 21,284 | 0.917 | 0/3 |
| theater | civil | 3 | 95,921 | 54,612 | 0.569 | 38,667 | 0.708 | 0/3 |
| **cavalry** | civil | 3 | 254,069 | 1,916 | **0.008** | 1,165 | 0.608 | 0/3 |
| **war** | military | 3 | — | 71,229 | — | **0** | **0.000** | **3/3** |
| **bonus** | military | 3 | — | 108,775 | — | 418 | **0.004** | 0/3 |
| **artillery** | civil | 2 | 156,450 | 1,549 | **0.010** | 886 | 0.572 | 0/2 |
| **air** | civil | 1 | 105,356 | 1,502 | **0.014** | 608 | 0.405 | 0/1 |

† `played` for a pact is an *offer*, and a refused pact returns to the hand
and can be offered again (`interact.py:228`), so the ratio legitimately
exceeds 1. Draws are netted of returns; offers are not.

‡ **Do not read the pooled wonder row.** It is dominated by 4p, which takes
wonders 75× more often than 2p for a reason §12.3.2 makes exact. Wonders are the
one type where pooling across player counts destroys the finding, and it is
worth noticing that a less careful census would have reported "wonders:
take 0.045, finish 0.19" and buried the actual result.

#### 12.3.1 The four numbers that carry the finding

* **War is never declared.** **0 in 71,229 draws**, at every player count
  separately: 0/35,336 at 2p, 0/22,707 at 3p, 0/12,669 at 4p. War over
  Culture alone is drawn 31,606 times and declared zero times. This is the
  only type the baseline records in `known_zero`.
* **Aggressions are drawn and rot.** 2,005 thrown in 151,335 draws. Two of
  eleven — Aggression: Raid (II) and Raid (III) — are thrown **zero** times
  in 18,783 draws between them.
* **A wonder taken is a wonder abandoned — at 3p, 8,720 started and 189
  finished (0.022), with 8 of the 16 wonders never completed once in 4,335
  games.** Per card the completion rate spans 0.001 (Ocean Liners: 1,498
  taken, **1** completed) to 0.907 (Hanging Gardens), while the *take* rate
  that produced that spread is flat at 0.032–0.053. The bot takes the best
  and the worst wonder at the same rate and then finishes whichever happened
  to be cheap.
* **Unit cards are refused on sight.** 6,779 taken from 742,091 offers across
  the four unit types — a take rate of **0.0091**, and **0.0017** at 2p.

#### 12.3.2 Wonders by player count: the plumbing map predicting its own exception

This is the sharpest evidence in the document, and it only exists because the
census was run at all three player counts.

| | 2p | 3p | 4p |
|---|---|---|---|
| `wonder_remaining` weight | −0.2355 | −0.2118 | **+0.3391** |
| offers | 324,211 | 278,906 | 41,201 |
| taken | 1,850 | 8,720 | 18,718 |
| **take / offer** | **0.006** | **0.031** | **0.454** |
| completed | 1,150 | 189 | 4,251 |
| play / held | 0.622 | **0.022** | 0.227 |
| wonders never completed | 3/16 | **8/16** | 3/16 |
| `take \| wonder` concordance | 0.525 | 0.534 | **0.383** |

The take rate varies by **76×** across the three vectors. The thing it tracks
is the sign and size of `wonder_remaining` — a weight on the wonder's
**cost**. The thing it does not track, at any player count, is
`card_potential` — the wonder's **value** — because that is multiplied by
`row_urgency = 0.0` in all three. At 2p and 3p the cost term is negative and
the bot essentially refuses wonders; at 4p it is positive and the bot takes
one at every opportunity and abandons three quarters of them. Same severed
pipe, sign flipped, and the concordance row inverts with it.

Other player-count differences worth naming:

| | 2p | 3p | 4p |
|---|---|---|---|
| war play/held | 0.000 | 0.000 | 0.000 |
| aggression play/held | 0.005 | 0.012 | 0.037 |
| event play/held | 0.766 | 0.596 | **0.178** |
| territory play/held | 0.708 | 0.497 | **0.146** |
| pact play/held | n/a § | 0.995 | 1.809 |

§ **Pacts do not exist at 2p.** Every pact card's deck count is `{"2p": 0}`,
and `actions.py:280` skips pact move generation below 3 players. A 2p-only
census would have reported all 10 as dead cards — which is exactly the error
this document is about, made one level up.

### 12.4. The cross: ranked suspects

The two axes are "seldom played" (§12.3) and "can its value reach the policy"
(§12.2). Only the corner where both are true is a bug.

#### 12.4.0 The control that decides the ranking

Before ranking anything: a type that is dead at 1 ply and alive under the
search the league actually trains is a **1-ply artefact**, not a shipped
defect. So the whole census was re-run at 2p under `plan:width=2`, the
`experiments/watchdog.sh:154` candidate bot, on the same frozen weights.

| type | 1-ply `WeightedBot` | `plan:width=2` | verdict |
|---|---|---|---|
| **war** | **0.000** (0 / 35,336) | **0.161** (357 / 2,220), 0/3 never played | **search REPAIRS it** |
| **aggression** | 0.005 (412 / 75,443), 2/11 never | **0.038** (187 / 4,940), **0/11 never** | **search repairs it (7×)** |
| **wonder** | 0.006 take, 3/16 never taken | **0.003 take, 11/16 never taken** | **search makes it WORSE** |
| units (4 types) | **0.0017** (429 of 257,848 offers) | **0.0048** (64 of 13,248) | no change — still ~nothing |
| event | 0.766 played | 0.688 | no change |
| territory | 0.708 played | 0.564 | no change |
| bonus | 0.003 | **0.000** (0 / 3,518) | no change |

Every cell is 2p against 2p on the same weights, so the only difference is
the search. That is a clean split, and it is exactly the split the plumbing map predicts.
War and aggression fail at 1 ply because their **payoff is not in the trial
state** — and `PlanBot`/`QuiescentBot` fix precisely that, by running the real
`resolve_war` on a scratch copy (`quiescent.war_value`) and by draining
`pending` before scoring. Wonders, units, events and territories fail in the
**leaf evaluation**, which every search shares, so no amount of search can
help and a deeper one merely spends its extra accuracy avoiding them harder.

**Search repairs a missing rollout. It cannot repair a severed wire.**

#### 12.4.1 The ranking

**Tier A — evaluator-structural. Survives every search. These are the bugs.**

| # | type | n | the number | the pipe |
|---|---|---|---|---|
| 1 | **wonder** | 16 | take/offer varies **76×** across player counts tracking a **cost** weight, and 1.7× across a 14.4× **value** range within one; 8/16 never completed at 3p; concordance 0.525 / 0.534 / **0.383**; `plan:width=2` makes it worse | `row_urgency` = 0.0 → **severed**. The one surviving identity feature is `wonder_remaining`, the cost. **The archetype, and the thing this audit was asked to generalise.** |
| 2 | **units** — infantry, cavalry, artillery, air | 10 | take/offer **0.0091** over **742,091 offers** (0.0017 at 2p); barely moves under the search | pipe is *live* (`hand_potential` 0.125) and carries an actively **negative** number: `unit_strength_credit` = 0.0 leaves `card_potential` at −4.40…−0.57, pure cost. `row_pressure` additionally skips any card with `card_potential <= 0`, so units are invisible to the row terms too. **Wrong sign, not a missing wire** |
| 3 | **event** | 55 | prepared often (0.18–0.77) but **`flat` 0.775–0.897**: at most decisions every event in hand scores identically | `hand_mil_potential` = 0.0 *and* `_card_yields` returns the empty tuple for all 55. The only thing separating two events is `p.culture += level` in `_h_prepare_event`, and the prepare rate duly tracks age (0.574 → 0.643) and nothing else about the card. **The *choice* is broken, not the rate** |
| 4 | **territory** | 12 | **`SEVERED` 0.903** — value spans 0.46–13.40 and the score does not move | priced correctly by `_card_yields`, carried by `hand_mil_potential` = 0.0. **A pure severed pipe, and the one type a single non-zero weight fixes** |

**Tier B — 1-ply artefacts. Already repaired in the shipped search; do not
spend evaluator work on them.**

| # | type | n | the number | why |
|---|---|---|---|---|
| 5 | war | 3 | 0 / 71,229 at 1 ply → **0.161** under `plan:width=2` | `_h_war` pushes nothing onto `pending`; the payoff lands a round later in `game.start_turn`. `quiescent.war_value` already runs the real resolution |
| 6 | aggression | 11 | 0.013 at 1 ply → **0.038** under `plan:width=2`, and 2/11-never becomes 0/11-never | resolution goes through a `defense` pending that `deferred_credit` does not cover (it handles `pact_offer` and `auction` only); `QuiescentBot` drains `pending` first |

**Tier C — real, but not a mispricing.**

| # | type | n | the number | why |
|---|---|---|---|---|
| 7 | tactic | 15 | play/held falls **0.729 → 0.121** from Age I to Age III; `play_tactic` is `flat` 0.805 | unpriced (`_card_yields` = `()` for all 15 — top-level `strength`, `obsoleteStrength`, `composition` are read only for `UNIT_TYPES`), and consequence-priced through `tactic_level`/`strength`, which is **zero with no units to fill the army**. **Confounded with #2**: fixing units may fix this for free, and it should be re-measured after, not fixed in parallel |
| 8 | government | 8 | healthy in play: take-rate spans **318×**, concordance 0.973–1.000 | but top-level `civilActions`/`militaryActions`/`urbanBuildingLimit`/`revolutionCost`/`peacefulCost` reach pricing only via `board_yields`, behind `card_board_credit` = 0.0. **Fixed tonight, shipped off** |
| 9 | bonus | 3 | 418 spends in 108,775 draws (0.004), and **0 in 3,518** under the search | **no move handler exists.** Only spendable by the defense / colonization machinery. **Not a policy bug — there is no decision to make.** If bonus cards should be playable, that is a rules-coverage question, not an evaluator one |

**Healthy — priced, plumbed through a live term, and the score follows the
value:** farm, mine, lab, temple, library, arena, theater (24 cards, the
"bag of numbers" cards, exactly as [`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md) predicted),
special-tech (12), leader (24), action (33), pact (10). That is **11 types
and 103 of 236 cards**, all reached by `hand_potential` at 0.125 or, for
pacts, by `deferred_credit` into `features()`. Their probe controls are the reference
the broken types are measured against: `take | leader` 0.843–0.968,
`take | special-tech` 0.848–0.987, `take | action` 0.792–0.972.

#### 12.4.2 How a bug was distinguished from a bad card

This is the part that matters, because "seldom played" on its own says
nothing. Three tests, applied in order:

1. **Does `card_potential` vary across cards of this type?** If it is
   identically zero for all of them (events, tactics, pacts, wars, bonuses,
   10 of 11 aggressions) the type is *unpriced*, and its play rate is
   uninterpretable — the bot is not choosing badly, it is not choosing.
2. **If it varies, does the score follow it?** `SEVERED` and `concordance`.
   A type where `card_potential` varies and the score does not (territory,
   `SEVERED` 0.903) is a **severed pipe**: a bug, and one that a fix to the
   *pricing* cannot touch.
3. **If the score follows it, is the level right?** Units pass test 2 with
   concordance `1.000` — the policy orders them correctly — and fail here,
   because every value in the ordering is negative. Ordering and level are
   different failures and the probe separates them.

A type that passes all three and is still seldom played is a **bad card, or a
correct avoidance**, and is reported as such. That is the honest answer for
several of the specific low-take-rate *cards* inside otherwise healthy types,
and [`docs/SCORE_AUDIT.md`](SCORE_AUDIT.md#1062-the-scripted-ab-forcing-wonders) §10.6.2 — forcing wonders cost 34.3 ± 7.0 margin
— is the standing reminder that low use is not automatically wrong. It is
also why §12.4.0's search control comes before any of this: "seldom played" and
"cannot see it" are both necessary, and neither is sufficient.

#### 12.4.3 If you fix one thing

In cost order, cheapest first, because three of the four Tier A defects are
one-line changes that are already built and switched off:

1. **territory** — set `hand_mil_potential` above 0.0. The pricing already
   exists and is correct (0.46 → 13.40); the wire is the only missing part.
   This is the single cleanest A/B in the list and it also un-blinds the
   *denominator* for the other military types.
2. **units** — set `unit_strength_credit` above 0.0. This flips ten cards
   from a strictly negative `card_potential` to something with a sign that
   matches reality, and it is the prerequisite for **tactic** (#7), which
   cannot be judged until the bot owns units to fill an army.
3. **wonder** — this one is *not* a weight. `row_urgency` at 0.0 is the
   symptom; turning it up prices a wonder only at take *timing*, through a
   heuristic the search does not optimise. The fix is structural: give
   `p.wonder` a term the search sees at every decision, the way
   `hand_potential` covers `hand_civil`. **A wonder lane is already on this;
   this document is the measurement it should be judged against, and §12.3.2 is
   the specific table that should move.**
4. **event** — the largest type (55 cards) and the most work: `_card_yields`
   returns nothing for any of them, so plumbing alone changes nothing. Needs
   a mapping first, and the `allPlayers`/rank-block tree is exactly the
   board-scaled and trigger-shaped pricing [`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md#3-the-blind-spot-that-remains-written-down) §3
   already wrote off as hard. Lowest ratio of value to effort of the four.

And one that is not on the list but should be somebody's: **`cost.militaryActions`
on 54 cards** (§12.5.1) is a genuine top-level-field blind spot of exactly the
kind that just cost a season on governments, and it is cheap.

### 12.5. Incidental findings, for routing

None of these are mine to fix; they are named here so they are not lost.

#### 12.5.1 `cost.militaryActions` is the government bug, still open, on 54 cards

The government defect was a **top-level** card field the rules engine honours
and no card-pricing path reads. Sweeping the whole DB for that class turns up
exactly one more, and it is bigger:

`cost: {"militaryActions": N}` sits on **54 cards** — every tactic (15),
aggression (11), territory (12), pact (10), war (3) and bonus (3). The rules
engine gates move legality on it in three places (`actions.py:269`,
`actions.py:1083`, `events.py:493`). **No code under `engine/bots/` reads it
at all.** War over Culture costs 3 military actions and War over Territory
costs 2, and to every card-pricing path in the project they are the same
card.

Two smaller members of the same class, both on tactics: top-level `strength`
is read by `_card_yields` **only** `if typ in C.UNIT_TYPES`
(`weighted.py:1040`), so a tactic's printed strength is dropped; same for
`obsoleteStrength` and `composition`.

#### 12.5.2 `hand_mil_potential` cannot ever use board pricing

`weighted.py:1269` calls `card_potential(n, w)` with **no `state`/`idx`**,
while its civil sibling `hand_potential` passes both. So even with
`card_board_credit` turned up, board-aware pricing can never fire for a
military card. Whoever turns on `hand_mil_potential` will get the printed
numbers only, and will not be told.

#### 12.5.3 The degenerate-champion guard has a hole, and 4p walks through it

`arena.refuse_if_degenerate_champion` compares weight files by **exact
content**. `analysis/frozen/champion_4p.json` is **76 of 78 weights identical**
(78 was the frozen snapshot's weight count at that vocabulary generation —
see [`analysis/frozen/README.md`](../analysis/frozen/README.md) for the
78→99→112 growth history that predates the Rust port's current 133-key table)
to `experiments/champion_4p.json` — the vector [`docs/TRAINING_RUN.md`](TRAINING_RUN.md) says
never to warm-start from — differing only in `colonies` and `pacts`, and
keeping the thing that makes it degenerate: **`science = -6.0888`**. It
passes the guard silently. `tools/card_census.py` now warns on ≥95%
similarity as well as exact identity, and **every 4p number in this document
carries that caveat.**

#### 12.5.4 Dead data and dead code

* `state.scoring_events` is declared (`state.py:157`), copied by
  `fastcopy.py:87` and encoded as a neural feature
  (`neural_encode.py:272`) — and **never written by anything**. The neural
  net has a permanently-zero input.
* `PlayerState.destroyed_wonders` is read by the take surcharge
  (`actions.py:90`, `actions.py:124`) and **never incremented**, so that
  surcharge can never fire.
* `urbanLimitCategory` (16 cards) duplicates `type` and nothing reads it;
  `scoringEvent` (15 cards) duplicates "age III event" and nothing reads it;
  top-level `target` (69 cards) is prose.
* `tests/test_card_pricing.py:100-106` writes off a government's
  `civilActions` / `militaryActions` as "still open". At the shipped default
  that is **correct and not stale** — `card_board_credit` is 0.0, so
  `_card_yields` genuinely still cannot see them — but it becomes wrong the
  moment that credit is turned up, and nothing will flag it. The
  `DELIBERATELY_UNPRICED` mechanism has no way to say "written off *while*
  this weight is zero", which is a gap in the guardrail rather than in the
  entry. (I initially recorded this as a contradiction with
  `board_yields.py:44-51`; re-reading both, it is not one.)

### 12.6. What this does not establish

* **The `plan:width=2` control is 2p only, and n=350.** It is decisive for
  the types it moved — war goes from 0/71,229 to 357/2,220, which is not a
  sample-size question — but the Tier A "no change" rows are the weaker
  claim, and 3p/4p under the search are not measured at all. Re-running the
  control at 3p is the cheapest way to strengthen this document.
* **`plan:width=2` is what the league TRAINS, not necessarily what ships.**
  `experiments/watchdog.sh:121-154` says width=2 was chosen on cost and that
  a gap between training and shipping configuration is expected. A
  `QuiescentBot` census would likely repair aggression further still.
* **It is the frozen champions, not retrained ones.** Turning on a 0.0 weight
  changes what the league would converge to; nothing here predicts that.
* **4p is measured on a known-degenerate vector.** See §12.5.3.
* **It does not measure what any fix is worth.** Every claim here is about
  what the policy *can see*, not about win rate. The wonder lane's A/B is the
  template for turning one of these into a number.
* **`offered` is an opportunity count, not a preference.** A card offered in
  slot 12 at 3 civil actions is not the same offer as one in slot 1, and the
  census does not weight by slot cost.

### 12.7. Reproducing

```bash
# the census (raw JSONL, one line per game, written as games finish)
python3 -m tools.card_census run --players 2 --games 6000 --seed 100000 \
    --workers 3 --champion analysis/frozen/champion_2p.json --out raw_2p.jsonl
python3 -m tools.card_census report raw_2p.jsonl raw_3p.jsonl raw_4p.jsonl
python3 -m tools.card_census report raw_2p.jsonl --cards wonder

# the identity probe -- the plumbing claim, measured rather than argued
python3 -m tools.card_census probe --players 2 --games 40 --workers 4 \
    --champion analysis/frozen/champion_2p.json

# the control that decides the ranking: does the trained search repair it?
python3 -m tools.card_census run --players 2 --games 350 --seed 900000 \
    --workers 3 --champion plan:analysis/frozen/champion_2p.json,width=2 \
    --out raw_2p_plan.jsonl

# freeze the baseline, then gate on it after any evaluator change
python3 -m tools.card_census baseline raw_*.jsonl --out analysis/census/baseline.json
python3 -m tools.card_census check raw_*.jsonl \
    --baseline analysis/census/baseline.json --tol 0.35

# the gate
bash tools/gate.sh
```

### 12.8. Provenance

Everything above was run for this document; nothing is carried over.

* **Census:** 12,087 games — 2p 6,000, 3p 4,335, 4p 1,752 — under
  `analysis/frozen/champion_{2,3,4}p.json` at 1 ply, on the desktop at idle
  priority. **Zero engine errors.** Raw per-game JSONL, frozen as
  `final_{2,3,4}p.jsonl` before any analysis was run against it.
* **Control:** 350 games at 2p under `plan:width=2` on the same weights.
* **Probe:** 40 games at each of 2p/3p/4p, 116 `(move, type)` groups, saved
  to `analysis/census/identity_probe.json`.
* **Baseline:** `analysis/census/baseline.json`, derived from the frozen
  snapshot, `known_zero = ["war"]`.
* **Plumbing map:** read out of `engine/` at master `50ba471`, with every
  claim in §12.2 carrying a file:line. The three claims the conclusions rest on
  — `row_urgency` = 0.0 in all three champions, `hand_mil_potential` = 0.0,
  and `card_potential` strictly negative for all 10 unit cards — were each
  re-derived directly from `load_weights` output rather than read off
  `DEFAULT_WEIGHTS`, because the champion files and the defaults are
  different objects and only one of them is what plays.
* **Gate:** `bash tools/gate.sh` → GATE PASS. Both fingerprints unchanged,
  plain and under `FASTCOPY_PARANOID`, which is the expected result: this
  change adds two files under `tools/` and `docs/` and touches nothing on the
  hash path.

The one thing this document does **not** contain is a fix, or a win-rate
number for one. It is an inventory of what the policy can see.

### 12.9. Rebase note: what landed while this was measuring, and why the numbers stand

This census was measured against master `50ba471`. By the time it landed,
eleven commits from sibling lanes had gone in, four of them squarely in the
territory of §12.4's Tier A:

| commit | lane |
|---|---|
| `ec0d2a5` | `wonder_potential`: let the wonder in progress reach the policy at all (**inert**) |
| `237cd34` | wonders join the board-aware swap diff (**inert**) |
| `660b5c8` | price Age III event seeding; correct the census that over-reported blindness |
| `7084a04` | tactics are a plumbing problem: card COUNT outweighs army strength 11:1 |
| `12d6b8a` | audit every card type's end-of-game scoring: 8 bugs, 167 tests |
| `f6ff7db` | re-analyse the territory A/B on the deal: a well-powered null |

**Every number in this document still describes the shipped policy**, and
that is not luck — it is the finding restating itself. Checked directly after
the rebase:

```
wonder_potential      default=0.0   effective=0.0
hand_mil_potential    default=0.0   effective=0.0
unit_strength_credit  default=0.0   effective=0.0
card_board_credit     default=0.0   effective=0.0
tactic_gain/short     default=0.0   effective=0.0
```

So the wonder lane has now built the term §12.4.3 asks for — `wonder_potential`,
a wonder-in-progress sibling to `hand_potential` — and at the shipped default
it contributes exactly zero, which is precisely the state this document
exists to make visible. The count of untrained weights went **110 → 112**
while I was measuring. Nothing here argues those defaults are wrong: 0.0 is
the correct way to land a new channel without invalidating three frozen
champions. The argument is narrower and it is the point of §12.2's weight
column: **"the trainer will decide what it is worth" stops being true the
moment the champions that would do the deciding are frozen and the leagues
warm-start from them**, and at that point a 0.0 default is not a neutral
prior, it is a decision to ship the blind version.

The concrete ask that follows: `tools/card_census.py check` should be run
against `analysis/census/baseline.json` **after** any of these weights is
turned up, and §12.3.2's wonder table is the specific thing that should move.
If it does not, the fix is inert for the same reason the last one was.

### 12.10. The territory suspect and the defence drain are ONE defect (2026-07-30)

§12.4.1 ranked **territory** its number-one confirmed-broken suspect and §12.2.3
described war and aggression as "priced by resolution, and the resolution is not
in the trial". Both are the same defect, and the defence lane found it from the
other end — see [`docs/AGGRESSION_RATE.md`](AGGRESSION_RATE.md#8-the-bigger-half-this-was-never-mainly-about-defence-it-was-about-auctions) §8.

The census looked for a missing *feature* (`hand_mil_potential = 0.0`, a severed
pipe). The missing thing is not only a feature: it is the **position** the
feature is read on. `PlanBot.pick` short-circuits on `state.pending` to a 1-ply
pick with no `_quiesce`, while `_child` drains every node inside the beam — so
at a real decision the bot prices a *half-resolved* position. A territory is
acquired through an `auction` pend resolved round-robin, so an undrained
position after `("bid", n)` shows the money committed and **not whether the
territory was won**. The bot chose what to pay without ever scoring a position
that said whether it got the colony.

`tools/pending_divergence.py` at 3p, 24 games, `champion_3p_gen1255_99key`:
auctions are **71.6%** of the decisions the drain moves (455 seen, 326 moved) —
against defence's 37.8% and `discard_military`'s 6.0%. Territory pricing
therefore cannot be evaluated on this engine until the drain question is
settled: **a territory-credit A/B run today is measuring a weight applied to a
position that does not yet contain the outcome.** §12.4.3 ("if you fix one thing")
should be read with that in front of it.

Consequence for this document's numbers: the census counts and the plumbing map
stand — they are about which features exist and fire. The *ranking* of territory
as a pricing defect is confounded, because part of what looked like a mispriced
card is a mispriced position.

## 13. Pricing the cards whose value is a sentence (2026-07-29) (merged from the former `CARD_PRICING_LEADERS.md`, 2026-07-31)

Lane C of the follow-up to [`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md): **leaders (24), actions
(33) and governments (8)**.

That document ends by naming its own biggest gap — "a board-aware card
evaluator — one that takes `(name, state, idx, w)` rather than `(name, w)` —
is what closes buckets 1 and 4, and it is the single highest-value follow-up
this census suggests." This is that evaluator, for the three card types where
the gap was worst.

**Result in one line:** the pricing lands and is inert (all eight fingerprint
digests unmoved); the census and the behavioural counter both move a long way
(16 → 4 blind leaders; leaders taken 3.6 → 5.6 per game); and the win-rate A/B
is **flat in aggregate, with one half significant and one half not** —
governments help (culture margin +1.85, z = 3.4), leaders are a **null**
(−1.8pp, z = −1.46, p = 0.15 once the over-dispersed blocks are accounted for)
— so the governments half is the reverse of what I predicted (§13.5.2).

> This line originally read "decomposes into two opposite signs … leaders hurt
> slightly (−1.8pp, z = −2.1)". The leaders half did not survive the
> unit-of-analysis audit of 2026-07-30; see §13.5.2's correction box and
> [`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md#105-lane-c-leaders-the-one-substantive-retraction) §10.5. I was wrong about governments, and about
> leaders I was right for the wrong reason — I predicted "neutral-to-positive"
> and the answer is "neutral", but the −2.1 I then reported as a real negative
> was an artefact of clustering on the wrong unit.

### 13.0. One-paragraph answer

All 24 leaders had a dropped key and **16 of the 24 were worth nothing to the
evaluator beyond "it is a leader"**. The fix is not a table of handlers. The
engine already implements every one of these rules in
`engine/effects.py:_apply_modifier`, so `engine/bots/board_yields.py` prices a
leader by **swapping it onto the player's board and asking
`effects.compute` what changed**. That reuses the rules instead of copying
them, and it gets three things right that no per-key handler can: leader
*replacement* is a diff and can be negative, the engine's clamps apply, and
**governments fall out for free** — which turned out to matter, because a
government's whole value is its top-level `civilActions` /
`militaryActions` / `urbanBuildingLimit`, which live in no `production` or
`effects` block and which `_card_yields` has therefore never read at all.

### 13.1. The governments finding, which is a result on its own

`_card_yields` walks exactly two blocks of a card, `production` and
`effects`. A government keeps its most important numbers outside both:

```json
{"name": "Republic", "civilActions": 7, "militaryActions": 2,
 "urbanBuildingLimit": 3, "peacefulCost": 13, "revolutionCost": 3,
 "techCost": null, "effects": {}}
```

Despotism grants 4 civil actions. Republic grants 7. **Civil actions are the
core currency of Through the Ages and the evaluator could not see the largest
single source of them.** Four of the eight governments (Despotism, Monarchy,
Constitutional Monarchy, Republic) have an empty `production` and an empty
`effects`, so they were *literally* the empty card to `card_potential`.

The cost side was blind in the same way and for a related reason.
`_card_yields` reads `card["techCost"]` — and `techCost` is `null` on every
government, because a government is paid for either peacefully
(`peacefulCost` science, charged by `effects.tech_cost`) or by revolution
(`revolutionCost` science plus the whole civil action pool, charged by
`actions._h_revolution`). So all eight governments were priced as **free and
worthless simultaneously**, which is the only reason the blindness was not
already obvious in play.

`board_yields` prices the revolution route, because `revolutionCost` is
cheaper in science on every card in the deck (Monarchy 2 vs 8, Democracy 9 vs
17) and is the route the engine's own `_can_revolt` makes available. The
science goes on `science` as a clamped cost; the burned action pool goes on
its own `gov_action_cost` weight, board-aware, because emptying a 7-action
Republic turn is not the same price as emptying a 4-action Despotism turn.
Splitting them rather than summing them is what lets the league discover the
exchange rate instead of being told it.

This is confined to governments: no card of any other type carries a
top-level action count, so there is nothing here to route to another lane.

### 13.2. Why a swap diff and not a handler table

The obvious implementation is a dispatch table, one handler per effect key:
`culturePerTheater` → `val * workers_on_types(p, {"theater"})`. It is also
the implementation `engine/effects.py:1197-1202` exists to warn about:

> Hollywood and Internet score off `_BUILDING_OUTPUT`, not their printed
> production [...] before that fix the code summed printed values with an
> ad-hoc Sid Meier special case, which under-scored every Chaplin,
> Shakespeare, Newton and Einstein completion.

Two implementations of one rule drift, and the evaluator's copy drifts
silently — nothing fails, the bot just misprices. So:

```python
old = p.leader
p.leader = "Michelangelo"
after = effects.compute(state, p)     # the real rules engine
p.leader = old
delta = after - effects.state_stats(state, p)
```

All thirteen of the `effects.MODIFIER_KEYS` that any leader carries are then
priced exactly, for free, by the code that actually runs them (19 keys in
total once `_apply_special`'s two and the two riders are counted). Three
things fall out that a per-key handler gets wrong by construction:

1. **Replacement.** A leader replaces the leader you have, so the value of
   taking one is a *difference*. Taking Gandhi (+2 printed culture) while you
   hold Churchill (+3 culture a turn) is a **loss of 1 culture a turn**, and
   the diff says so. `_card_yields` says `+2` regardless of what you hold, and
   always did.
2. **Clamps.** `compute` ends with `happy = max(0, min(8, happy))` and every
   rating floored at 0. A leader's ninth happy face is worth nothing and the
   diff knows.
3. **Governments**, per §13.1, with no extra code at all — `compute` reads
   `p.government` the same way it reads `p.leader`.

#### 13.2.1 The trap, written down because it fails silently

**Use `compute` for the hypothetical, never `state_stats`.**

`state_stats` is a per-mutation cache keyed on `p.idx`, validated against
`stats_key(state, p)` and *only rebuilt when the entry is marked dirty*.
Assigning `p.leader` does not mark it dirty. So the natural-looking version of
the code above returns the stats of the **old** leader and every diff comes
out as exactly zero — no exception, no warning, every leader priced at nothing,
which is indistinguishable from the bug being fixed.
`tests/test_board_yields.py:TestTheComputeVsStateStatsTrap` reproduces the
trap directly and fails if the two calls are ever swapped.

#### 13.2.2 The memo key, verified rather than trusted

`compute` is hot, so the diff is memoised on
`(name, effects.stats_key(state, p))`. `stats_key` carries a documented
invariant that it names every field `compute` reads, which is exactly the
completeness this key needs.

A docstring is not evidence. `TestStatsKeyIsACompleteMemoKey` plays 2p and 3p
self-play games, and for every player at every ply — under six different
hypothetical leader/government swaps, so the *hypothetical* side of the diff
is covered too — records `stats_key -> compute`. It fails if one key ever maps
to two different `Stats`. Over ~1300 distinct keys there are **no
collisions**. A key that missed a field would serve silently stale card
valuations, which is a worse bug than the blindness this module fixes.

#### 13.2.3 Which parts are rules and which are judgement calls

Worth separating explicitly, because the two need different kinds of
justification and only the second kind needs evidence.

**Rule-faithful — no free parameter, no discount, nothing to tune.** These are
not models of the rules, they are the rules, obtained by running them:

* every `Stats` delta from the leader/government swap — all thirteen
  `MODIFIER_KEYS` on leaders, both `_apply_special` keys, and the top-level
  government action counts;
* the replacement semantics (a leader replaces a leader), the engine's clamps,
  and the government science cost, which is read off the card;
* **Genghis Khan.** "One of the two strongest, ties in your favour" is
  computed exactly from rival strengths. It looks like a judgement call and is
  not one.

**Judgement calls — a choice was made and could be made differently.** Each is
flagged here so nobody later mistakes it for a derivation:

| choice | what was chosen | the alternative |
|---|---|---|
| **Churchill's `perTurnChoice`** | value him at the culture option, 3/turn | model the military option's 6 ring-fenced points as worth more |
| **Which government route to price** | revolution (`revolutionCost`), always the cheaper science | price the peaceful route, or the min of the two under current stats |
| **Revolution's action cost** | its own `gov_action_cost` feature at 0.0 | fold it into `civil_actions`, i.e. assert an exchange rate |
| **`resourcesForMilitaryUnits`** | own `restricted_resources` feature at 0.0 | treat ring-fenced resources as plain `resource_stock` |
| **Reserves' "food OR resources"** | max under the current weights | a fixed 50/50, or always the resource side |
| **A SPARE leader in hand** (§13.10) | `hand_swap_extra` at 0.0: the hand's best single-slot card is priced in full, the others at that fraction of their own diff | a fixed discount (½ for the second, ¼ for the third…), or keep summing them |

Note the shape of that table: every judgement call except Churchill's was
resolved by **creating a 0.0-weight feature rather than by picking a number**.
That is deliberate — it converts a choice-with-a-free-parameter into something
the league fits, so the only genuinely hand-set constant in the whole change is
Churchill's 3.

#### 13.2.4 Value that appears on acquisition and evaporates on ownership

A follow-up found by the uncovered-types lane: `_stats_delta` priced
`urban_limit`, `pop_food_discount` and `no_aggression`, and `weighted.features`
emitted none of the three. So a government that raises the urban building
limit was worth something while the bot was *considering* it and worth nothing
the moment it was *played*.

That is the mirror image of the blindness this document was written about.
Both directions produce a bot that misjudges what it owns, and the same
principle settles them: **a card whose gain is priced on one side and not the
other is biased, not inert.**

The three are *not* the same call, which is why they got three answers.

| key | direction | why | kind |
|---|---|---|---|
| `urban_limit` | **emit it in `features`** | real persistent board state (Despotism caps you at 2 urban buildings, the Age III governments at 4) and nothing else in the feature dict reflects it — `urban_workers` is workers, not the cap | **rule-fact**: the file's own convention, written above these lines, is "same key on both sides, the way `civil_actions` already is" |
| `no_aggression` | **emit it in `features`** | permanent board state, enforced at `engine/actions.py:292` | direction is rule-fact; the **judgement** is encoding it as a 0/1 flag rather than a count, and the weight stays unsigned at 0.0 so the league decides which of Gandhi's two halves dominates |
| `pop_food_discount` | **stop pricing it in the delta** | see below — the opposite call | **rule-fact**: there must be exactly one representation, and `pop_cost` is the one the board already uses |

**Moses is the interesting one, because the premise was wrong.** His board
side was never blind. `features` computes

```python
pop_cost = max(0, pop_cost_base(bank) - s.pop_food_discount)
```

and `pop_cost` carries a real trained weight of **−0.4**. A player holding
Moses has always been valued correctly. Giving the swap diff its own
`pop_food_discount` feature therefore did not fix an asymmetry — it created a
**second representation of one quantity, at 0.0, sitting next to a live one**.
That is the same shape as `buildDiscount` summed instead of maxed, and as the
hand double-count: this repo has now paid for it three times.

So the diff prices Moses through `pop_cost`, the feature the board evaluation
actually reads, and the `pop_food_discount` weight is deleted. Two consequences
worth stating: Moses is now priced by a **live** weight instead of a dead one
(he is worth something under a trained vector for the first time), and there is
one representation again rather than two.

**While confirming that, the formula turned out to exist in four places** —
`economy.pop_cost`, `weighted.features`, `neural_encode`, and Ocean Liners'
`freePopIncreasePerTurn` rider — and the three copies outside `economy` all
omitted the `one_time_discount` term the canonical one applies. It is now one
implementation, `economy.pop_food_cost`, with
`TestThePopCostFormulaHasOneImplementation` walking `engine/` to forbid a
fifth. The two evaluator callers still pass no `one_time_discount`, which
preserves their exact behaviour: that omission is a real (small) blind spot,
but fixing it changes what the bot plays and belongs in its own measured
change rather than smuggled into a de-duplication.

**The guardrail matters more than the three fixes.**
`TestAcquisitionAndOwnershipAgree` gathers every feature name card pricing can
emit — board path *and* static table, by running them over all 236 cards
rather than by reading the tables, since a rider that invents a key is exactly
what a table-reading version would miss — and fails unless each one is either
emitted by `features` or listed in `CARD_ONLY` with a written reason. After
this change `CARD_ONLY` has **four** entries, every one a genuinely one-shot
quantity: `gov_action_cost` (the pool a revolution empties that turn),
`free_civil_action`, `resource_discount` and `restricted_resources` (riders on
one-shot action cards). Nothing else in the evaluator prices something on
acquisition that it cannot price on ownership.

This change is **inert**: both new features default to 0.0 and contribute
exactly 0.0 to `evaluate`, the Moses reroute only fires when
`card_board_leader` is non-zero (it is 0.0), and the `pop_food_cost`
extraction is verified byte-identical to the inline formula over four games of
self-play.

### 13.3. The census

`tools/card_blindness.py` grew a `--board` mode that counts the board-aware
evaluator as well as the static table, on a board stocked with one staffed
example of everything a card can be paid for. Without stocking the board the
question is meaningless: Bach with no theaters really is worth nothing, and
counting that as blindness would be counting the wrong thing.

| card type | n | zero visible gain: master → static now → **board** |
|---|---|---|
| **leader** | 24 | 17 → 16 → **4** |
| **government** | 8 | 4 → 4 → **0** |
| **action** | 33 | 19 → 6 → **2** |
| TOTAL (all types) | 236 | 168 → 146 → **129** |

`--legacy` still reproduces the published master column exactly (171 dropped /
168 zero-gain), and now has a test pinning it — see §13.7.

The four leaders that remain flat are **Aristotle** (1 science per technology
card taken), **Hammurabi** (a military action usable as a civil action),
**Christopher Columbus** (remove him to colonize free) and **Frederick
Barbarossa** (a combined pop-increase and unit build). Every one is a trigger
or a rule change, not an omission, each has a written reason, and
`TestEveryLeaderIsPriced.STILL_FLAT` fails if the list grows.

### 13.4. The cards, individually

Two leaders are priced by a **rider** rather than by `compute`, because their
payout is a turn-end trigger and `compute` builds only the production phase.
Both are exactly computable, so neither is a guess:

* **Winston Churchill** — "once each turn, choose: 3 culture; or 3 restricted
  science and 3 restricted resources." The culture option needs no board, no
  other card and no condition, and is available every turn, so his floor is a
  flat **+3 culture production** — more than any wonder in the game prints.
  The military option is taken as worth no more, because both its halves are
  ring-fenced.
* **Genghis Khan** — "3 culture at end of turn if you are one of the two
  strongest civilizations, ties in your favour." Computed exactly from rival
  strengths. Note what it says **at two players**: "one of the two strongest"
  out of two civilizations is vacuously true, so Genghis is an unconditional
  +3 culture a turn at 2p and a real condition at 3p and 4p. No static table
  can express that, and it is the cleanest single argument for board-aware
  pricing in the set.

Both riders **subtract the outgoing leader's rider**, for the same reason the
Stats side is a diff. Forgetting that subtraction is how Gandhi-over-Churchill
comes out as +2 instead of −1.

Three action cards were board-scaled and are now priced additively (they are
not swaps, so nothing is replaced): **Endowment for the Arts** (culture per
civilization ahead of you on culture — 6 per rival at 2p, so worth 6 or
nothing and never anything in between), **Wave of Nationalism** and **Military
Build-Up** (resources per stronger civilization, ring-fenced to military
units, hence `restricted_resources` rather than `resource_stock`).

The three **Reserves** needed something different again: "gain N food **or** N
resources". Summing both would be a lie in the opposite direction from
dropping the key, so `_card_choices` returns mutually exclusive groups and
`card_potential` takes the better one under the current weights.

### 13.5. Inert, then live

Everything is gated on one new weight, `card_board_credit`, defaulting to
**0.0** — the exact analogue of `card_rate_credit` and for the same reason:
at 0.0 `card_potential` returns the byte-identical pre-change answer, so the
A/B is paired, same-process, on the same deal, and the eight fingerprint
digests do not move. Six new features (`urban_limit`, `gov_action_cost`,
`pop_food_discount`, `no_aggression`, `restricted_resources`, plus the credit
itself) all default to 0.0.

`tests/test_board_yields.py:TestTheCreditGateIsExact` asserts that at 0.0 the
board-aware and static answers agree for **all 236 cards**.

A 0.0 default has a second consequence worth naming: `hillclimb_league`
derives `NONNEG` / `NONPOS` from the *sign* of the default, so a weight
defaulting to 0.0 is in neither set and the climber may move it in either
direction. `card_board_credit` is therefore not merely inert — **the league
can switch this on by itself** if it is worth switching on, without anybody
editing a constant. That is the same reasoning
[`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md#22-nine-new-weights-all-defaulting-to-00) §2.2 gives for refusing to put a negative prior on
the finish-discipline terms.

All eight fingerprint digests were checked and **none moved**:

```
narrow 0a6ed6ad  wide 4a8c6ca6   (GreedyBot, plain and FASTCOPY_PARANOID)
weighted narrow 5eff41eb   weighted wide d03e0964
quiescent narrow eff1bef5  quiescent wide 9e9695d4
plan narrow c534ac3d
```

One note on process, because the run looked alarming for a while. The
`weighted wide` arm first reported `FAIL`, and the recorded digest was
**empty** rather than wrong — the `perf_check` subprocess had been killed
under load rather than producing a different answer. A control run on a
pristine `6968256` checkout died in exactly the same way, which is what
identified it as environmental. Re-derived on its own, the arm produces
`d03e096414d7adb4af7b6d22cd534195a45f27beb91678cde547a7b05e47597c`, which is
the constant already in `tools/gate.sh`. The constant was not touched. Worth
recording as a gate-reading habit: `check_fp` renders a crashed arm and a
genuinely moved digest almost identically, and the tell is that the "got"
field is blank.

#### 13.5.1 How large is the perturbation?

Under the frozen 2p champion's own weights, on a round-11 board where it holds
Joan of Arc:

| card | credit 0.0 | credit 1.0 |
|---|---|---|
| Michelangelo | 0.00 | **+10.64** |
| Winston Churchill | 0.00 | **+10.64** |
| Genghis Khan | 0.00 | **+10.64** |
| Endowment for the Arts | 0.00 | +6.00 |
| Republic | 0.00 | +3.49 |
| Fundamentalism | −7.62 | −0.12 |
| Reserves (III) | 0.00 | +1.87 |
| **Sid Meier** | 0.00 | **−11.18** |
| Eiffel Tower (control) | 27.45 | 27.45 |

Sid Meier is the one to look at. He prices *negative* because that board's
only lab is level-0 Philosophy, so his "each lab makes culture equal to its
level" pays nothing while his "−1 science per lab" still bites, and on top of
that he would replace Joan of Arc. He is genuinely a bad card on that board,
and this is the first time the evaluator has been able to say so about any
card.

The negatives are not a bug and are worth being explicit about: once you hold
a leader, a *worse* leader correctly prices below zero. That suppresses
downgrades while leaving upgrades (the +10.64 rows) firmly attractive.

#### 13.5.2 Win rate: a flat aggregate that decomposes into two opposite signs

Method as in [`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md#4-method-for-5) §4: `experiments.evaluate` at 2 players
plays each deal twice with the seats swapped, so the comparison is paired on
the deal; both arms are `analysis/frozen/champion_2p.json` differing in
`card_board_credit` alone (verified: exactly 1 of 105 weights differs). Each
arm is 8 disjoint blocks of 400, n = 3200 games / 1600 deals, SE ≈ 0.7pp on
the paired win rate, **MDE ≈ 2.0pp**. Run on the desktop pinned at `664cdfc`,
12 workers.

`TTA_BOARD_TYPES` restricts board pricing to a subset of card types, which is
what makes the decomposition possible at all:

| arm | win rate (paired) | culture margin | own culture |
|---|---|---|---|
| everything on | 49.95% ± 1.68pp (z = −0.1) | +0.95 ± 1.31 (z = +1.4) | 150.8 vs 149.8 |
| **governments only** | 51.02% ± 1.40pp (z = +1.4) | **+1.85 ± 1.07 (z = +3.4)** | 149.4 vs 147.5 |
| **leaders only** | 48.20% ± **2.92pp** (z = **−1.46**, p = 0.15) | −0.48 ± 2.56 (z = −0.4) | 149.0 vs 149.5 |

> **Corrected 2026-07-30** ([`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md#10-the-unit-of-analysis-every-interval-in-this-project-was-computed-on-the-wrong-n) §10). The leaders row
> previously read **48.20% ± 1.69pp (z = −2.1)** and was read as "leaders hurt
> slightly". That interval is correctly clustered on the deal; the problem is
> one level up. **The eight blocks are over-dispersed**: per-block win rates
> 43.8, 47.8, 52.4, 46.3, 53.8, 46.3, 45.6, 49.9, a spread of 3.49pp where
> deal-level noise predicts 2.44pp, χ² = 14.41 on 7 df against a critical
> 14.07. Clustering on the block instead gives **z = −1.46, p = 0.15**.
>
> **The leaders effect is not statistically significant and this document
> should not be read as showing that leaders hurt.** The honest summary of
> §13.5.2 is now "governments help on the culture margin; leaders are a null with
> an unstable point estimate", not "two opposite signs".
>
> This is a borderline call, stated as one: the escalation trigger is only just
> tripped and a heterogeneity test on eight blocks is not powerful. But a
> result whose significance depends on which of two defensible clusterings you
> choose is not a result. If the leaders arm matters, it needs more blocks, not
> a different formula.
>
> **The governments half is unaffected.** It was already deal-clustered, its
> blocks agree (χ² = 2.59 on 7 df), and **+1.85 ± 1.07 (z = 3.4)** stands. The
> "everything on" aggregate is likewise unchanged at 49.95% ± 1.68pp — but note
> that its flatness can no longer be explained as two significant opposite
> signs cancelling, since only one of the two is significant.

The aggregate is a textbook flat null — 49.95% against a 50% null, z = −0.1,
which is as close to nothing as 3200 paired games can report.

**The aggregate is flat because the two halves point in opposite directions
and roughly cancel.** That is the whole reason to decompose, and an aggregate
null would have hidden it completely.

##### I predicted this backwards, in both directions

Before running, and on the record: *"governments negative, leaders
neutral-to-positive"*. The reasoning was that `gov_action_cost` defaults to
0.0, so the on-arm prices a revolution's science but not the civil-action pool
it burns, and the behavioural counter showed governments taken **doubling**
(1.1 → 2.1 per game) — a bot revolting twice as often while blind to the cost
of revolting looked like an obvious way to lose.

**That mechanism is refuted.** Governments are the half that *helps*: the
culture margin is +1.85 with z = 3.4, which is the only individually
significant effect in the whole experiment. So the doubled revolution rate is
apparently closer to correct play than the frozen champion's, even with the
action cost unpriced — which is a much more interesting statement about the
game than my prediction was, and it makes the §13.1 finding stand on its own two
feet: **the evaluator not being able to see Republic's 7 civil actions was
costing something real and measurable.**

Leaders are the half that hurts, by −1.8pp, marginally (z = −2.1, p ≈ 0.04,
and one arm out of two at that threshold is roughly what you expect by
chance). It is small, it is not the "markedly worse" that would indicate a
broken implementation, and the culture margin does not corroborate it
(z = −0.7). But it is the direction that warrants a look rather than a shrug,
and the two candidates worth checking first are both in §13.8 already:

1. **`hand_potential` double-counts leaders.** Every leader in hand is priced
   as replacing the *current* leader, but only one of them can be. That
   over-count was harmless when the bot held ~0 leaders in hand and is not
   harmless now that it takes 55% more of them. This is the strongest
   candidate and it is a defect in the *hand term*, not in the pricing.
   **Established and fixed in §13.10** — and note what the correction box above
   does to the question: with the leaders arm a null rather than a −1.8pp
   negative, the double-count is no longer a defect that needs to explain a
   number. It is a defect because the arithmetic is wrong.
2. **A leader's upside lands on well-fitted weights and its restrictions land
   on 0.0 ones**, per the asymmetry table below.

Neither is a reason to unship rule-faithful pricing, and neither is settled by
this experiment.

##### What this means for shipping

`card_board_credit` stays at **0.0**, which is what is committed. Concretely:

* Nothing needs to change before a league restart — the shipped engine is
  byte-identical to master in behaviour, so the arms can be restarted on it
  safely.
* If anyone turns this on, **turn the government half on first**. It is the
  half with a positive, individually significant signal, and
  `card_board_government` = 1.0 expresses exactly that configuration — a
  weight, so the league can find it without being told (§13.10.2; it was
  `TTA_BOARD_TYPES=government`, an environment variable, when this was
  written).
* The leader half should wait on the `hand_potential` double-count — which
  §13.10 fixes and re-measures.

### 13.6. Does the bot actually take these cards?

[`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md#51-finish-discipline-a-null-and-the-reason-is-more-interesting-than-the-null) §5.1 names the trap that makes this question
mandatory rather than optional: **giving a card a weight does not help until
the bot takes the card.** Three of the keys added there sit at exactly 0.000
variance because the champion never takes Masonry or Library of Alexandria,
so those weights have no gradient at all and a hill climb only drifts them.

`tools/take_census.py` counts every card taken from the civil row under a
given vector. Under the frozen 2p champion, 8 games, credit 0.0 vs 1.0:

| | credit 0.0 | credit 1.0 |
|---|---|---|
| leaders taken per game | 3.62 | **5.62** |
| governments taken per game | 1.12 | **2.12** |
| leaders NEVER taken (of 24) | **14** | **7** |

So this is not the §13.5.1 situation: the bot takes these cards constantly, both
before and after. The interesting part is *which* ones, and it lines up with
the census exactly. Before the change the leaders it took were precisely the
ones the static table could already see — Joan of Arc, Homer, Gandhi,
Shakespeare, all of which print a plain `happy` or `culture` — and the 14 it
had never once taken included Michelangelo, Napoleon, Sid Meier, Churchill,
Bach and Genghis Khan, precisely the ones that priced at zero.

That was written down as a prediction before the arm was run, and it holds:

| leader | taken, credit 0.0 | taken, credit 1.0 |
|---|---|---|
| Genghis Khan | 0 | 7 |
| Michelangelo | 0 | 5 |
| Winston Churchill | 0 | 4 |
| Charlie Chaplin | 0 | 3 |
| Isaac Newton | 0 | 3 |
| J. S. Bach / Sid Meier / Moses | 0 | 1 each |
| Joan of Arc | 8 | 3 |

Joan of Arc going *down* is the other half of the same fact: she is no longer
the only leader the evaluator can see.

### 13.7. Guardrails added

* `tests/test_board_yields.py` — 28 cases: the `compute`/`state_stats` trap,
  the memo-key completeness sweep, the government blindness stated as tests
  against the *old* behaviour, per-leader pricing, monotonicity in the board
  count (a leader must grow with the thing it pays for, not merely be
  non-zero), the credit gate's exactness, and the choice cards.
* `tests/test_card_pricing.py` — the existing key-coverage guardrail now also
  reads `_EFF_CHOICE` and `board_yields.BOARD_PRICED`; new cases reject a
  board-priced key with no written reason, a stale one, and a key claimed
  both priced and unpriced.
* **`--legacy` is now pinned.** This one caught a real bug rather than being
  written for tidiness. `_card_choices` read the card data directly instead of
  being gated on its registry the way `_EFF_SPECIAL` is, so
  `use_legacy_maps()` could not switch it off and `--legacy` silently stopped
  reproducing the published 171/168 "before" numbers — quietly rewriting the
  baseline that every later result is measured against. Now a test.
* `DELIBERATELY_UNPRICED` lost 17 keys to `BOARD_PRICED` and 2 to the static
  tables. Bucket 1 ("board-scaled") is down from 16 keys to 4, and the four
  that remain are on wonders and an event — where a swap is the wrong
  question, because a wonder accumulates rather than replaces.

### 13.8. Known limitations, stated rather than discovered later

* ~~**Two leaders in one hand are both priced as replacing the current one.**~~
  **FIXED in §13.10**, as max-plus-remainder, and the wonder aside in the
  original wording was wrong: see §13.10.1 for why a wonder is not the same
  structure. The rest of this bullet stood — it was the pre-existing shape of
  `hand_potential`, not something the pricing introduced.
* **`gov_action_cost` sits at 0.0**, so in the on-arm a revolution's science
  cost is priced but the civil-action pool it burns is not. That makes
  governments somewhat too attractive until the league prices that weight. It
  is deliberate — a non-zero default would not be inert — but if the
  government decomposition arm comes out negative, this is the first thing to
  suspect.
* **Cost, measured.** Board pricing calls `effects.compute` once per
  swap-type card in the hand and the row per leaf. 2p self-play, 6 games,
  same seeds, cache cleared between arms:

  | arm | ms/ply | |
  |---|---|---|
  | `card_board_credit` 0.0 | 4.07 | |
  | `card_board_credit` 1.0 | 5.14 | **1.26×** |

  **At the shipped default the cost is exactly zero**, not merely small:
  `card_potential` returns on `if not board` before touching
  `board_yields` at all, which is the same early return that makes the
  pricing byte-identical to master. The 1.26× is what turning it on costs,
  and it is the number to beat if it is ever turned on for a league run.
  The memo helps less than it looks like it should — 1894 distinct entries
  over ~1100 plies — because `stats_key` includes the worker counts and so
  changes on nearly every move; it collapses the several cards priced within
  one decision, not across decisions.
* **The four remaining flat leaders need a measured trigger rate**, not a
  guessed one. Aristotle pays 1 science per technology card taken, Newton
  refunds a civil action per technology played; pricing either honestly needs
  a count of how often those events actually happen per round, which
  `tools/take_census.py` is now most of the machinery for.

### 13.9. Reproducing

```bash
python3 -m tools.card_blindness --legacy          # 171 / 168, the baseline
python3 -m tools.card_blindness                   # static table only
python3 -m tools.card_blindness --board           # with board_yields
python3 -m tools.card_blindness --board --cards leader

python3 -m unittest tests.test_board_yields tests.test_card_pricing

# what the vector actually TAKES -- the docs/CARD_BLINDNESS.md 5.1 check
python3 tools/take_census.py --w analysis/laneC/off.json --games 40 \
    --type leader

# the A/B, and the two decomposition arms.  The arm is a WEIGHT
# configuration now, not an environment variable (section 10.2).
bash analysis/laneC/run_ab.sh main
bash analysis/laneC/run_ab.sh government
bash analysis/laneC/run_ab.sh leader
bash analysis/laneC/run_ab.sh leader 1.0    # ...with the double-count back

bash tools/gate.sh
```

### 13.10. The hand double-count, fixed

A follow-up lane, on the strongest of the two candidates §13.5.2 left open. The
order matters and is deliberate: the defect is established as arithmetic
first, fixed because it is wrong, and only then re-measured. It would still
have shipped if the measurement came back flat.

That ordering turned out to be load-bearing rather than merely tidy. This
lane was commissioned to explain a −1.8pp negative, and while it was running,
the unit-of-analysis audit **withdrew that negative** (the correction box in
§13.5.2: the leaders arm is a null, z = −1.46, p = 0.15). A lane that had set
out to explain the number would have had nothing left to do. The defect is
independent of it: it is wrong arithmetic, it was demonstrated without
playing a single game, and it would be worth fixing if the leaders arm had
come back at +5pp.

#### 13.10.1 The defect, in the only terms that settle it

`card_potential` prices a leader or a government as a **diff** — what
replacing the one you have with this one would change. `hand_potential`
summed that over the civil hand. Summing diffs against a single slot asserts
you get to make the same replacement once per card you hold.

Concretely, from `tests/test_hand_swap.py` (2p, seed 7, 60 plies, the player
holding **Joan of Arc**):

| leader in hand | swap diff |
|---|---|
| Michelangelo | **+3.60** |
| Julius Caesar | −5.35 |
| Homer | −5.20 |
| `hand_potential`, before | **−6.95** |
| `hand_potential`, after | **+3.60** |

The old number is not merely too large or too small — it has **the wrong
sign**. The hand contains a leader worth +3.60 and the evaluator priced the
hand at −6.95, because it charged the bot for replacing Joan of Arc with two
leaders it would obviously never play. The same file pins the pure form: three
copies of one leader priced at exactly 3× that leader.

**The fix.** Each single-slot class contributes the best card in the hand for
it, plus `hand_swap_extra` × the rest. The spares are not worthless — you may
play the best one now and a better one two ages later — but their true
incremental value is a diff against *the leader you will have by then*, which
this function cannot see. So it is a free parameter, and per §13.2.3's
convention it is a **0.0-default weight and not a constant somebody picked**.
Two properties fall out that are worth stating:

* `hand_swap_extra = 1.0` **is** the old behaviour, exactly. The defect stays
  reproducible as a control arm in the same binary rather than only by
  checking out an old commit, which is what makes §13.10.3's before/after a
  same-process comparison.
* At `card_board_credit = 0.0` nothing is priced as a diff, so there is
  nothing to collapse and the hand stays a plain sum. The shipped evaluator is
  byte-identical: all eight `tools/gate.sh` fingerprint arms are unmoved.

**What else has this shape**, since the whole point of a structural fix is
that it is not one card class:

* **Governments — yes, exactly.** One government slot, priced by the same
  swap diff, and it was summed the same way. Fixed by the same code; the two
  slots collapse independently, so a hand of two leaders and two governments
  is one leader replacement plus one revolution, not four.
* **`rival_hand_potential` — yes.** It prices a rival's hand through
  `card_potential` on the rival's own board, and a rival holding three leaders
  is not three replacements dangerous. Fixed in the same helper, deliberately:
  pricing my hand and theirs through two different functions is how the two
  drift apart.
* **`row_pressure`'s `row_urgency` — yes, and NOT fixed here.** It sums
  `card_potential` over the row cards the sweep is about to destroy, so two
  leaders in the row are two replacements there too. Demonstrated: with an
  empty leader slot and three leaders in the row, two of them contributed
  2.70 + 2.55 to one `row_urgency`. It is a different question ("take now or
  never") and a separate behavioural change, so it is written down here rather
  than folded into a measured fix. **This is the next thing to do in this
  area.**
* **Wonders — no, and §13.8's original aside was wrong about this.** Lane A has
  since made a wonder a swap card too, but its diff is a *pure gain with
  nothing netted off* — you do not have a wonder slot that a second wonder
  displaces, and `actions._take_gate` will not even let you take another
  while one is in progress. Two wonders in hand is optimism about **time**,
  not an arithmetic impossibility, and the time is what
  `wonder_turns_to_finish` / `wonder_overrun` exist to price.

  This is why the collapse is keyed on a new `board_yields.SINGLE_SLOT` and
  **not** on `SWAP_TYPES`: had it been keyed on `SWAP_TYPES`, wonders would
  have started collapsing silently the moment Lane A added them to that set,
  with nothing failing. `tests/test_hand_swap.py` asserts a hand of two
  wonders still sums, for exactly that reason.
* **Tactics — same shape, currently inert.** One tactic in play, and
  `hand_mil_potential` would sum two. It is 0.0 by default, so nothing calls
  `card_potential` on a military card at all; `tactic_terms` already takes a
  max over reachable tactics. Fix it when that weight is turned on, not
  before.

#### 13.10.2 The type knob is a weight now, not an environment variable

`TTA_BOARD_TYPES` gated board-aware pricing by card type. It was read from
`os.environ` once at import, which means the configuration with the only
individually significant result in §13.5.2 — governments alone, culture margin
+1.85, z = 3.4 — was reachable by a human typing a command and by nothing
else. The league could never find it.

It is now four weights, `card_board_leader` / `card_board_government` /
`card_board_action` / `card_board_wonder` (Lane A's), **additive offsets** on
the shared `card_board_credit`, so that:

* `card_board_credit` alone still moves all four together — the aggregate arm
  is unchanged and §13.5.2's top row is still the same experiment;
* `card_board_government = 1.0` from a zero credit turns that half on by
  itself, which is the recommendation in §13.5.2 expressed as something
  `hillclimb_league` can fit;
* `card_board_credit = 1.0` with `-1.0` on every other type is **exactly**
  the old `TTA_BOARD_TYPES=leader`,
  which is what keeps §13.10.3 comparable to the table in §13.5.2 rather than
  nearly-comparable;
* all three default to 0.0 on top of a 0.0 credit, so nothing ships turned on.

`tests/test_board_yields.py:TestTheTypeKnobIsAWeightNow` asserts each of those
four configurations by checking whether a leader and a government are actually
priced by the diff or by the static table, and fails if any reader of the
environment comes back — a stale exported variable would silently make a
measurement arm measure something other than its weight file says.

#### 13.10.3 Re-measuring the leaders arm: a null, and a control that lands exactly

Same design as §13.5.2 and deliberately the same *seeds*: 8 blocks of 400 at 2p,
paired on the deal, seeds 0/200/400/600/800/1000/1200/1600, 12 workers on the
28-core desktop. Raw blocks in `analysis/handfix/`.

Three arms, and the second one is the reason the first can be believed:

| arm | win rate | culture margin |
|---|---|---|
| leaders only, **fixed** (`hand_swap_extra` 0.0) | 47.67% ± 1.67pp (z = −2.74, deal-clustered, χ² = 8.90/7 — blocks agree) | −1.07 ± 1.29 |
| leaders only, **pre-fix control** (`hand_swap_extra` 1.0) | 48.20% ± 2.92pp (z = −1.46, escalated to block-clustered, χ² = 14.41/7) | −0.48 ± 2.56 |
| **fixed vs pre-fix, head to head**, both board-on | **50.33% ± 1.09pp (z = 0.59, p = 0.55)** | +0.05 ± 0.77 |

Intervals from `tools/ab_summary.py` (the corrected estimator that landed
2026-07-30); `analysis/laneC/agg.py`'s legacy numbers are printed alongside
them in `analysis/handfix/results.txt` so the arms reconcile against the older
table.

**The control arm reproduces §13.5.2's leaders row exactly** — 48.20% ± 1.69pp
legacy, margin −0.48, own culture 149.0 vs 149.5, and *block for block*
(43.8, 47.8, 52.4, 46.2, 53.8, 46.2, 45.6, 49.9). That is worth more than it
looks:

* it confirms `hand_swap_extra = 1.0` is the pre-fix pricing to the game, not
  approximately — so the before/after is one binary, one engine, one seed set,
  and the difference is the fix and nothing else;
* it confirms none of the engine work that landed between `664cdfc` and here
  (tactics, event seeding, the score audit, wonders becoming a swap type)
  perturbs this measurement at all.

**The answer to "does −1.8pp move": no.** The head-to-head duel is the direct
test — the same vector with and only with the defect — and it is a **null with
real power**: 50.33% ± 1.09pp against an MDE of 1.55pp. The fix is worth
somewhere between −1.2pp and +1.5pp, and the tight interval comes from
ρ = −0.60, i.e. the seat-swapped pairing doing exactly what
`experiments/paired_stats.py` says it does.

Two honest caveats on the first row:

* The fixed arm reads z = −2.74 where the control reads z = −1.46, and it
  would be easy to write that up as "the fix made it worse". **It is not a
  difference in the point estimate** (47.67 vs 48.20, well inside either
  interval) — it is a difference in *which clustering the escalation rule
  picks*. The control's blocks are over-dispersed and get the coarser
  interval; the fixed arm's blocks agree (χ² = 8.90 against a critical 14.07)
  and keep the deal-level one. The head-to-head, which needs no such choice,
  says the two vectors are the same strength.
* The leaders arm being **below 50% at all** is untouched by this work and is
  still unexplained. §13.5.2's second candidate — a leader's upside landing on
  well-fitted weights while its restrictions land on 0.0 ones — is now the
  live one, and `row_urgency`'s copy of the double-count (§13.10.1) is the other.

**Cost at the shipped default: not measurable.** The collapse adds one type
lookup and one weight lookup per card in the hand, and the per-type credit
adds one more inside `card_potential`. Timed on the idle desktop, the 33-game
WeightedBot fingerprint workload (`engine.perf_check hash --weighted`, which
runs at `DEFAULT_WEIGHTS`, i.e. board pricing off) took 12.86s / 12.27s on
master and 12.41s / 12.37s here — a difference smaller than the spread of the
repeats. §13.8's "at the shipped default the cost is exactly zero" is no longer
quite literal, but it is still below what can be measured.

**The behaviour does move, so the null is not a trivial one.** 40 games of 2p
self-play under each vector (`tools/take_census.py --type leader`):

| | leaders taken/game | governments taken/game |
|---|---|---|
| pre-fix | 5.50 | 1.75 |
| fixed | **5.00** | **1.60** |

A bot that no longer believes a second leader in hand is worth a second
replacement takes ~9% fewer of them, which is the predicted direction. It just
does not win more or fewer games for it.

**The government half is unharmed, which matters because it is the half §13.5.2
recommends turning on.** Governments collapse in the hand too (they are the
other single-slot class), so the arm was re-run: **51.05% ± 1.39pp
(z = 1.47), culture margin +1.95 ± 1.07 (z = 3.59)**, against the published
51.02% / +1.85 (z = 3.4). Unchanged within noise on the win rate and, on the
margin, a shade stronger. The recommendation stands as written.

**Shipped anyway, and the ordering in §13.10's preamble is why.** The pricing was
wrong arithmetic; a hand holding a +3.60 leader was priced at −6.95. Correct
modelling ships whether or not the win rate notices, and `hand_swap_extra`
leaves the league a gradient to disagree on.

## 14. The bot never upgrades its army, and the reason is a table that cannot say "it depends" (merged from the former `UNIT_TECH_PRICING.md`, 2026-07-31)

2026-07-30.  Closes the top-ranked hole in [`docs/SYSTEM_COVERAGE.md`](SYSTEM_COVERAGE.md) ("What the
bot never does", #1).  Base game (2015), all three player counts.

### 14.0. The finding, and the number it is

[`docs/SYSTEM_COVERAGE.md`](SYSTEM_COVERAGE.md#5-technology-by-colour--the-biggest-structural-hole-in-the-whole-census) §5 measured unit-technology takes per seat-game at

| | 2p | 3p | 4p |
|---|---|---|---|
| bot | **0.15** | **0.06** | **0.45** |
| human (BGO corpus) | 3.84 | 2.79 | 3.43 |

— 8× to 47× under.  The bot fights the whole game with its Age A Warriors, and
[`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md#114-what-the-bot-actually-does-with-military-cards) §11.4 measured the downstream consequence:
across 30 games there were **five unit workers standing on the whole table**,
while ~29 military actions a game went into playing and copying tactics that
could form no army at all.

### 14.1. The mechanism, verified before anything was changed

Three claims, each re-derived here rather than taken from the census.

**(a) Every unit card prices strictly negative, on every vector in the
league.**  `card_potential`, no changes, five vectors:

| card | live 2p (g72) | archived 3p (g1314) | archived 4p (g361) | DEFAULT |
|---|---|---|---|---|
| Warriors | −3.46 | −5.44 | −0.90 | −0.60 |
| Swordsmen | −6.51 | −8.92 | −1.38 | −2.90 |
| Riflemen | −10.63 | −14.73 | −2.28 | −4.50 |
| Modern Infantry | −15.41 | −20.93 | −3.24 | −7.10 |
| Air Forces | −16.07 | −21.31 | −3.24 | −8.10 |

10 of 10 negative on 6 of 6 vectors tried.  The gain half is **exactly 0.0** on
the live 2p and archived 3p vectors (`unit_strength_credit` = 0.0) and is
*negative* on the archived 4p one, which carries `unit_strength_credit` =
−0.017 — so on that vector believing a unit's strength makes it worse.

**(b) `row_pressure` really does skip them.**  `weighted.py`: `val =
card_potential(...)`, `if val <= 0.0: continue`.  So a unit in the civil row is
invisible to `row_urgency` and `row_bargain_forgone` at any weight, on top of
being under-valued in `hand_potential`.  Pinned in
`tests/test_unit_pricing.py:TestRowPressureCanSeeAUnit`.

**(c) It is what suppresses the take — but it is not the whole gap, and the
census's phrasing needs one correction.**  Reference play, live 2p champion
under `plan:width=2,det=1`, 6 games / 1,597 decisions, every legal move scored
with the bot's own evaluator:

* a unit card was **legally takeable at 446 of 1,597 decisions (28%)**;
* the best unit take was the **best move 0 times in 446**;
* at the 437 decisions where a unit take *and* a non-unit take were both
  legal, the best unit take was the best take **1 time in 437**, a median
  **1.43 eval points** behind the best other take.

Now the counterfactual that isolates the bias: floor a unit's `card_potential`
at zero — remove the negative, add nothing — and the same 437 decisions give
**20 in 437** and a median gap of 0.76.  A twentyfold move, so the negative
pricing is genuinely load-bearing.  But 20/437 is still 4.6%: **a card worth
exactly zero is still not a card worth taking.**  Removing the bias is
necessary and is not sufficient, and any fix that only clamps the sign would
have produced a null and looked like a refutation.

### 14.2. Why turning the existing credit up cannot work

`unit_strength_credit` multiplies the printed strength.  On the live 2p
champion that buys 0.39 eval points per unit of credit against a cost of 6.51,
so **the sign flips somewhere past 16** — and every step between 0 and 16
changes no argmax at all.  `hillclimb.mutate` perturbs by `gauss(0, s) *
(abs(w) + 0.15)`; from 0.0 that is a flat plateau sixteen units long walked in
steps of ~0.15.  [`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md#1152-which-knobs-can-change-a-game-at-all--run-this-first) §11.5.2 measured the plateau
directly: **0 argmax divergences in 967 decisions at credit 1.0, one at 3.0.**

So this needed reshaping, not retuning.  `tests/test_unit_pricing.py:
TestTheDefect` pins both halves of that argument.

### 14.3. What changed

A unit technology is now priced by a **board query**, on its own credit —
`engine/bots/board_yields.py:unit_upgrade` and
`engine/bots/weighted.py:unit_tech_value`.  Three corrections, all derived:

**3.1 The move on the table is an upgrade, not a fresh build.**  The static
table priced "develop it and build ONE FRESH unit": full `techCost` in science,
full `buildCost` in resources, printed per-worker `strength` back.  Every
player starts with a Warriors worker (`game.START_TECHS`), so what the engine
actually offers is `("upgrade", lo, hi)` — it costs the *difference* of the two
build costs and pays the *difference* of the two strengths, on every worker
moved.  Riflemen off Warriors is 3 resources, not 5.  The numbers come from
`actions.upgrade_cost` and `effects.tech_cost`, the functions that charge the
player, and the strength comes from an `effects.compute` diff, so Great Wall's
`strengthPerInfantry`, the tactic army re-forming and the rating clamp are all
picked up for free.  Nothing is restated.

**3.2 A point of strength is not worth `w["strength"]`.**
`weighted.strength_marginal` is d(`evaluate`)/d(`features()["strength"]`),
computed exactly:

    strength          d/ds = 1 always
    strength_rel      d/ds = 1 always -- and it is the one strength feature in
                      PHASE_KEYS, so its early/late multipliers belong here
    strength_deficit  d/ds = -1 while behind, 0 ahead
    strength_lead     d/ds = +1 while ahead by < 6, 0 once capped

[`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md#1151-units-a-null-and-the-reason-it-cannot-be-otherwise) §11.5.1 named this — "the board expresses one
point of strength through four features and `card_potential` looks up only the
first" — and reckoned the under-count at 2.3× to 7× on the *frozen* champion.
On the **live 2p champion it is a factor of fifteen**, and the whole of the
difference is the phase multipliers that section did not consider:
`strength_rel` itself is 0.0 there while `strength_rel_early` is 3.37 and
`strength_rel_late` is 2.36.  0.19 versus ~3.0.  That is not a credit anybody
could have guessed at; it is a derivative of the objective, and it is why the
useful region is now near 1.0 instead of past 16.

The two conditional channels are the reason this has to be a board query: a
unit is worth more when you are behind and worth nothing extra once your lead
is capped, and no per-card table can express either.

**3.3 You develop first and decide how many workers to move second.**  The
trade is linear in that count, so the optimum is an endpoint — all of them or
none.  `max(0, ...)` in `unit_tech_value` is that argmax, not a floor put there
to keep the number positive, and the science is charged *outside* it, so a
technology nobody will staff still reads as the pure science cost it is.

**The new weight is `unit_tech_credit`, default 1.0.**  Not gated on
`card_board_credit`, deliberately: that weight is 0.361 on the live 2p champion
and **0.0 on both the 3p and the 4p ones**, so hanging the fix off it would
leave two of the three league arms with the defect it exists to fix.  It is a
new key, so `load_weights` fills it in from `DEFAULT_WEIGHTS` on every champion
file in the league and the change is live on all three at once.  0.0 recovers
the static table byte for byte, which is what makes every measurement below a
paired A/B in one process on the same deal.

1.0 is not a guess at a magnitude, and that is the difference from
`unit_strength_credit`'s argued-for 0.0: at 1.0 the number *is* "the eval
points `evaluate` itself assigns to the strength this buys, minus the eval
points it assigns to the resources and science it costs".  There is no free
constant left in it.  It stays a weight rather than a hard-coded 1.0 because
the price is a one-ply appraisal of a three-move plan (take → develop →
upgrade), and how much of a plan survives contact with the search is exactly
what a hill climb answers better than an argument.

#### 14.3.4 The seam the brief warned about does not block this

`hand_mil_potential` calling `card_potential` without a state was real and is
**already fixed** ([`docs/COMBAT_AUDIT.md`](COMBAT_AUDIT.md#11-hand_mil_potential-never-passed-the-board) §1.1 — the arguments are forwarded).
It would not have blocked this fix in any case: **unit technologies are CIVIL
cards.**  They arrive in the civil row and go to `hand_civil`, so they reach
`card_potential` through `row_pressure`, `hand_potential` and
`rival_hand_potential`, every one of which has always passed the state.
`hand_mil_potential` never sees a unit card at all.

#### 14.3.5 One implementation, not two

* the strength delta is an `effects.compute` diff — the engine's own
  arithmetic, not a re-derivation;
* the resources are `actions.upgrade_cost`, the science `effects.tech_cost`;
* `strength_marginal` is checked *numerically* against `evaluate` — bump
  `p.strength_extra` by one, re-evaluate, require the difference to equal the
  claimed derivative to nine places over self-play positions
  (`TestStrengthMarginal`).  A comment claiming to be a derivative is not a
  derivative; this one is measured against the thing it differentiates.
* `weighted.rival_strength` is a second spelling of one field of
  `rival_context`, written because `card_potential` is handed no `ctx` and
  building one per card priced would recompute every opponent's statistics
  several times per candidate.  It is held to the original by
  `TestRivalStrengthAgrees`, the same device `_SWEEP` and `game.SWEEP` use.

No information is added: `unit_upgrade` reads the tableau, which is public, and
`strength_marginal` reads rival strength, which `features()` already reads.

### 14.4. What it did — before/after, `tools/system_census.py`

Mirror table, `plan:width=2,det=1`, same seeds, same vectors, the only
difference being `unit_tech_credit` 0.0 → 1.0.  2p is the **live** champion
(gen 72); 3p is the **archived pre-restart** champion (gen 1314), because
`champion_3p.json` is gen 0 and byte-identical to `DEFAULT_WEIGHTS` — censusing
it would measure the defaults, exactly as [`SYSTEM_COVERAGE.md`](SYSTEM_COVERAGE.md) says.  40 games
at 2p, 28 at 3p.

| per seat-game | 2p before | 2p after | 2p human | 3p before | 3p after | 3p human |
|---|---|---|---|---|---|---|
| **tech: red (units)** | **0.20** | **1.06** | 3.84 | **0.08** | **4.16** | 2.79 |
| tech: yellow | 0.26 | 0.11 | 2.52 | 0.13 | 0.01 | 2.47 |
| tech: blue | 5.88 | 5.73 | 3.71 | 2.69 | 2.23 | 3.86 |
| tech: green | 1.74 | 1.71 | 3.08 | 3.08 | 2.61 | 2.45 |
| civil cards taken | 23.15 | 24.14 | 34.3 | 22.10 | 20.35 | 29.6 |
| wonders completed | 1.84 | 1.84 | 2.74 | 0.23 | 0.13 | 2.45 |
| wonder stages | 6.01 | 6.21 | 8.77 | 1.54 | 0.95 | 8.01 |
| wars declared | 0.59 | 0.58 | 0.26 | 1.25 | 1.12 | 0.16 |
| aggressions played | 0.88 | 0.85 | 1.39 | 0.91 | 0.88 | 1.63 |
| colonies held at end | 0.59 | 0.63 | 1.51 | 1.55 | 1.46 | 1.15 |
| leaders played | 2.36 | 2.33 | 3.69 | 2.89 | 2.37 | 3.61 |
| government changes | 0.95 | 0.96 | 1.14 | 1.30 | 0.80 | 1.16 |
| units disbanded | 0.44 | 0.35 | — | 1.01 | 0.68 | — |
| tactics played | 1.05 | 0.88 | — | 0.85 | 1.02 | — |
| **final score /seat** | **197.7** | **191.4** | 160 | **126.1** | **108.7** | 176 |

**The zero is gone at both counts.**  2p goes 5.3× (0.20 → 1.06), still 3.6×
short of the human rate; 3p goes **50×** (0.08 → 4.16) and lands 1.5× *above*
it.  The two arms differ that much because the vectors do: the archived 3p
champion carries `strength` = 3.42 and `science` = 0.19, so an upgrade is cheap
and valuable to it, where the live 2p champion carries `strength` = 0.19 and
`resource_stock` = 1.73 and thinks a bank resource is worth nine points of
army.  The fix does not impose a rate; it lets each vector's own opinion of
strength reach the card, which is the point.

**What it traded, reported rather than buried.**  At 2p almost nothing moves:
wonders completed identical, wars/aggressions/colonies flat, one extra civil
card a game, mirror score −3.2%.  At 3p the trade is real — 1.75 fewer civil
cards, 0.5 fewer leaders, 0.5 fewer government changes, 40% fewer wonders
completed, and a mirror score of −14%.  A mirror score is not a strength
measurement (both seats play the same policy), which is what §14.5 is for, but it
is a warning that at 3p the army is being bought out of the culture budget.

Yellow (farms/mines) falls at both counts.  It was already near zero and this
did not touch it: that hole is [`docs/UNCOVERED_TYPES.md`](UNCOVERED_TYPES.md#0-summary) §0's absolute-not-
delta pricing and is a different lane.

### 14.5. Strength: two nulls and one severe regression, and the regression is the
### most interesting number in this document

`experiments.evaluate`, the fix against **itself** — the identical vector with
`unit_tech_credit` 1.0 against 0.0, so the two arms differ in exactly one number
and are paired on the deal.  Seat-balanced.

| vector | games | deals | win rate | paired CI | null | p | culture margin |
|---|---|---|---|---|---|---|---|
| 2p, **live** champion (gen 72) | 300 | 150 | 49.83% | ±3.92pp | 50% | 0.93 | −1.3 |
| 3p, **archived** champion (gen 1314) | 240 | 80 | **14.58%** | ±4.84pp | 33.3% | **1.3e−14** | **−37.8** |
| 3p, `DEFAULT_WEIGHTS` | 180 | 60 | 34.72% | ±5.03pp | 33.3% | 0.58 | +4.0 |

#### 14.5.1 2p: a real null, not an underpowered one

`rho_deal` = −0.52 and a design effect of 0.48 — pairing halved the variance —
and ±3.9pp over 300 games would have found a 3.5-point effect.  There is not
one.  Culture margin −1.3 on a mean of 185, so the −3.2% the mirror census
showed is **symmetric**: both arms lose it.  That is exactly the distinction a
mirror census cannot make and a duel can.

#### 14.5.2 3p on the archived champion: a large, unambiguous regression

**14.6% against a 33.3% null is the worst A/B result this lane has produced and
it is not noise.**  It has to be read together with what that vector believes:

    strength            3.4191        resource_stock   2.7188
    strength_rel_early  7.3498        science          0.1897
    strength_lead       0.4682        culture_rate     9.7921

`strength_marginal` on that vector is up to **11 eval points per point of
army**, against 9.79 for a whole point of culture *per turn*.  It thinks one
soldier is worth about one culture rate.  Handed that opinion, the fix buys
**4.16 unit technologies a seat-game** (§14.4) and the culture collapses 134 → 97.

The fix is transmitting the vector's own stated price faithfully.  The price is
nonsense — and the reason it is nonsense is the defect itself:

> **`strength` and `strength_rel_early` were unconstrained coordinates.**  On
> every vector this league has trained, the only ways to gain army were things
> you were taking anyway (a wonder, a leader, a tactic) or things nothing
> priced.  **Nothing in the evaluator ever made the climb pay for a point of
> strength**, so the weight on it could drift as high as noise carried it
> without ever costing a game.  `strength_marginal` is the first term in this
> project that charges the evaluator its own stated price for army, and the
> first thing it did was expose that the price was fitted on a free lunch.

So the regression is a *measurement of a stale champion*, not of the change —
which is a claim that has to be testable, and the third row is the test.

#### 14.5.3 3p on `DEFAULT_WEIGHTS`: null, and this is the row that matters
operationally

34.7% ± 5.0pp against 33.3%, p = 0.58, margin **+4.0 culture** — if anything
mildly positive.  `DEFAULT_WEIGHTS` carries `strength` 0.35 and
`strength_rel_early` −0.1, so `strength_marginal` is ~0.9 there rather than
~11, and the fix buys army at a sane rate.

**This is the vector the live 3p arm actually starts from.**
`experiments/league_state/champion_3p.json` is gen 0 and byte-identical to
`DEFAULT_WEIGHTS` ([`SYSTEM_COVERAGE.md`](SYSTEM_COVERAGE.md)'s method note), as is `champion_4p`.
So nothing in the league today is in the regime of §14.5.2, and the arm that is
live and trained — 2p — is the clean null of §14.5.1.

#### 14.5.4 What to do about it, stated plainly

* **Do not warm-start a 3p or 4p league arm from `archive_prequiescent_
  20260730`** without re-fitting `strength` / `strength_rel_early` first.  That
  vector plus this change is a 14.6% player.
* `unit_tech_credit` is a weight, so **any champion file can opt out with
  `"unit_tech_credit": 0.0`**, which recovers the old pricing byte for byte —
  a zero-risk escape hatch that needs no code change.
* The standing policy is that correct modelling is worth committing whether or
  not it strengthens the bot.  On that basis this lands: two nulls, one
  regression that is attributable to a stale weight and is reproducible in the
  other direction on the defaults.  It is not being landed as an improvement,
  and §14.7 keeps the 3p question open.

### 14.6. Fingerprints

Six of the eight `tools/gate.sh` arms moved and two did not; the table, the
cause and the attribution are in the block above `WNARROW` in that file.  The
short version:

* **The two GreedyBot arms held still** (`NARROW` ca255af3, `WIDE` f223cea1).
  GreedyBot never calls `card_potential`, so an arm of it moving would have
  meant the change had leaked into the rules.  It did not.
* **All six evaluator arms moved** — WeightedBot, QuiescentBot and PlanBot,
  narrow and wide.  Expected, and predicted before the run: `DEFAULT_WEIGHTS`
  carries `unit_tech_credit` at 1.0, so every unit card in the row and in the
  civil hand prices differently for all three searching bots.
* **Two-sided** per `docs/PYPY.md` (deleted) §9.0: derived from scratch in two separate
  clones of the same commit, which agreed byte for byte on all eight arms —
  including the two that did not move.  A clean-base control on the parent
  commit reproduced every pre-change constant first.
* **Attributed to one constant.**  A third clone of the same tree with
  `"unit_tech_credit": 1.0` changed to `0.0` and nothing else touched
  reproduces **all eight** pre-change digests byte for byte.  So the six moves
  are that one default and nothing else in the change; the plumbing
  (`unit_upgrade`, `strength_marginal`, `rival_strength`, `_is_unit`) is
  provably inert on its own.

Nothing was re-derived to make the gate pass: it failed by design in both
clones and the committed constants are the computed values.  `bash
tools/gate.sh` on the pushed tree then reported **GATE PASS** on all eight.

Test count 1040 → 1053.  +12 from `tests/test_unit_pricing.py`, +1 from
splitting `test_zero_credit_is_the_static_answer_for_every_card` in
`tests/test_board_yields.py`, which needed a sibling once units stopped being
gated on `card_board_credit`.

**Negative control on the regression test**, in the sense
`tests/test_search_root_is_determinized.py` uses: dropped onto a clean tree at
the parent commit, `tests/test_unit_pricing.py` gives **4 failures and 5
errors** of 12.  The three that still pass there are exactly the ones written
to pass — the two `TestTheDefect` controls (the static table is still strictly
negative; the old credit cannot flip a sign) and the credit-0.0 equivalence,
which is trivially true when there is no credit.

### 14.7. Open, and deliberately not done here

1. **`tech_levels` is unpriced on every technology card.**  Developing any tech
   adds its level to `tech_levels`, whose live 2p weight is 5.84 plus phase
   terms — comparable to everything else on the card put together — and
   `_card_yields` maps nothing to it, for farms, labs, units or specials
   alike.  Same for the `best_*` family.  Adding it for units *only* would be
   the same asymmetry this document is about, pointing the other way, so it is
   not done here.  It is the most likely single explanation left for "civil
   cards taken 23.5 vs a human 34.3".
2. **Leaders and wonders still price strength through `w["strength"]`.**
   `board_yields._STATS_FEATURES` maps `Stats.strength` → `strength`, so a
   leader that grants strength is under-counted by the same factor of fifteen
   `strength_marginal` exists to fix.  Routing the swap diff through it too is
   a one-line change with a much wider blast radius and belongs in its own
   commit with its own measurement.
3. **The 3p regression on the archived champion (§14.5.2) is not closed, it is
   only attributed.**  The attribution is strong — the same A/B on
   `DEFAULT_WEIGHTS` is a null in the other direction — but nobody has yet
   re-fitted `strength` / `strength_rel_early` on a vector that has to pay for
   them, and until somebody does, "the weight was stale" is an inference from
   two rows rather than a demonstration.  The cheap version is to take the
   archived 3p champion, scale its military group down, and re-run §14.5.2.
4. **Every other feature that was never paid for is suspect for the same
   reason.**  The mechanism in §14.5.2 is general: a coordinate the evaluator can
   read but never has to buy is unconstrained, and it will drift.  `strength`
   was one because no card priced it.  Worth a sweep.
4. **Tactics remain confounded with this.**  [`SYSTEM_COVERAGE.md`](SYSTEM_COVERAGE.md#9-the-one-off-systems) §9 asked for
   tactics to be re-measured after the unit hole, not in parallel with it.
   Tactics played moved 1.05 → 0.88 at 2p and 0.85 → 1.02 at 3p; that is now
   measurable and was not before.

### 14.8. A Warriors worker cannot become a Cannon (2026-07-31)

Closes `docs/OPEN_ITEMS.md` §2 item 20, opened by `docs/CARD_BLINDNESS.md`
§15.6.1 (the former `YELLOW_TECH_PRICING.md`) and deliberately left alone there so that lane's digest moves had one
cause.

#### 14.8.1 The defect, verified against the engine before anything changed

`unit_upgrade` answered "develop this and move **every unit worker I have**
onto it".  `engine/actions.py:_action_moves` offers `("upgrade", lo, hi)` only
out of `_tableau`'s `higher` relation, and `higher[n]` is built from
`by_type[type_of[n]]` — **same type, strictly higher level**.  A Warriors
worker can never become a Cannon.  So the red price was optimistic for
cavalry, artillery and air on every board where the player held only infantry,
which is most of the game.

Measured on the parent tree (`d15cb5b`), a 2p board with four Warriors
workers, `unit_upgrade("Cavalrymen")`: **`(8.0, 6.0, 12.0)`** — eight strength
bought and `upgrade_cost` charged four times, for a move the engine never
generates.  After: `(0.0, 6.0, 0.0)`, the science and nothing else.
`tests/test_unit_pricing.py:test_a_warriors_worker_cannot_become_a_cannon` is
that number as a test, and it fails on the parent tree.

#### 14.8.2 What changed

`unit_upgrade` now calls `_upgradable_onto` and `_with_tech` — **the same two
helpers `tech_upgrade`'s non-red half has used since it landed**, moved up the
file rather than copied — so both halves of the module mean the same thing by
"upgrade".  `_unit_workers` and `_with_unit`, the two functions that expressed
the pooled version, are deleted.  There is no new weight and no new constant:
this is a legality rule the price was contradicting.

#### 14.8.3 What it did — take rates, 2p, `default` (WeightedBot on
#### `DEFAULT_WEIGHTS`), 20 games / 40 seat-games, same seeds

Descriptive, not a strength claim; n = 40 seat-games is below
`docs/HAZARDS.md` §1's n≥200 bar and is reported as counts, not as evidence
about win rate.

| takes per seat-game | human 2p | before | after |
|---|---|---|---|
| infantry | 1.120 | 0.400 | **0.475** |
| cavalry | 1.222 | 0.350 | **0.150** |
| artillery | 0.846 | 0.225 | **0.100** |
| air | 0.653 | 0.075 | **0.000** |
| **all red** | 3.841 | 1.050 | **0.725** |

The direction is the derivation's: infantry (the only line the starting
Warriors can actually upgrade into) goes **up**, and cavalry, artillery and air
— the three the old price was inventing upgrades for — fall.  The bot moves
*further* from the human rate on those three, and that is the correct
consequence of a correct price: what remains is `docs/OPEN_ITEMS.md` §2 item
21, "nothing prices the build one fresh plan", which is now the binding
constraint on the red lane rather than a footnote.

#### 14.8.4 The invisibility check, with numbers

`row_pressure` skips any card whose `card_potential` is `<= 0.0`, so a price
that falls has to be checked against zero and not just against itself.  On a
fresh 2p board under `DEFAULT_WEIGHTS`, four workers on the highest card of
each card's own type:

| card | type | price |
|---|---|---|
| Swordsmen / Riflemen / Modern Infantry | infantry | +2.83 / +5.16 / +10.29 |
| Cavalrymen / Tanks | cavalry | +1.15 / +3.38 |
| Rockets | artillery | +3.88 |
| **Knights** | cavalry | **−0.28** |
| **Cannon** | artillery | +1.15 |
| **Air Forces** | air | +0.07 |

**All four red types still price strictly positive**, so no class went
invisible, and `tests/test_coordinate_registry.py:NoCardClassIsInvisible`
agrees over its 6-game corpus.  But **Knights is now negative under
`DEFAULT_WEIGHTS` on a fresh board**, and that is worth stating plainly rather
than burying: Knights, Cannon and Air Forces are the lowest card of their own
type in the deck, so **no board can ever offer an upgrade onto them** and their
whole price is the develop half (`tech_levels`, `num_techs`, `best_unit`)
against their science.  Under `DEFAULT_WEIGHTS` one technology level is worth
1.5 eval points and Knights costs 5 science at 0.5, so it lands just under
zero; at `tech_levels` 3.0 (the live 2p champion carries 5.84) all three are
positive.  It is a weights judgement, not a sign lock, and the test asserts
exactly that distinction.  The gateway cards are the sharpest instance of item
21 in the game and are recorded there.

#### 14.8.5 Fingerprints

Six arms moved, two held.  **NARROW and WIDE are GreedyBot, which never calls
`card_potential`** — they are the control, and they held.

| arm | parent `d15cb5b` | this commit |
|---|---|---|
| NARROW (greedy) | ca255af3 | ca255af3 |
| WIDE (greedy) | f223cea1 | f223cea1 |
| WNARROW | ba77b499 | **7a6f6639** |
| WWIDE | f4d6a545 | **996f4ef7** |
| QNARROW | 4ab439b2 | **79e8503b** |
| QWIDE | 5d05f578 | **bb8d74c7** |
| PNARROW | 0a637b40 | **7e0f7a3b** |
| PWIDE | ccc96764 | **dee840cc** |

Attribution is the change itself: it is a single hunk in one function, on the
path `card_potential -> tech_value -> unit_upgrade`, which every evaluator bot
reaches and GreedyBot does not.

#### 14.8.6 The ratchet moved, and that is a warning as much as a result

`tests/test_coordinate_registry.py` landed the day before this change with a
`KNOWN_DEAD` list that can only shrink.  Re-pricing the red cards re-rolled the
six deterministic corpus games and **seven entries stopped being dead**:
`best_arena` (the bot now builds one) and the six `discontent` / `uprising` /
`best_arena` encoding slices.  They are deleted, per that file's rule.

The lesson is in `docs/OPEN_ITEMS.md` §9.5: those entries are pinned to six
games, and *any* pricing change re-rolls them.  `best_arena` went 0 → 314
non-zero states of ~2000 on this change alone.

### 14.9. Nothing priced the "build one fresh" plan (2026-07-31)

Closes `docs/OPEN_ITEMS.md` §2 item 28 and most of item 21, the residual §14.8
promoted "from a footnote to the binding constraint on the red lane".

#### 14.9.1 First, a correction to the brief

[`docs/SYSTEM_COVERAGE.md`](SYSTEM_COVERAGE.md#5-technology-by-colour--the-biggest-structural-hole-in-the-whole-census) §5 still names `unit_strength_credit` = 0.0 as
the gate on the red lane and calls it "the most actionable finding in this
document".  **That sentence is stale and the mechanism it names is gone.**
`d8a2172` (§14) routes a unit technology through `weighted.tech_value` on
`unit_tech_credit` = 1.0; `8b972ef` (§15) does the same for the other eleven
types; `e35d5f5` and §14.8 made the upgrade legality the engine's own.
Re-measured on `1b63421` — the tree this section starts from — with
`tools/take_census.py`, 100 games at 2p on `DEFAULT_WEIGHTS`:

| takes per seat-game | human 2p | `1b63421` |
|---|---|---|
| infantry | 1.120 | 0.425 |
| cavalry | 1.222 | 0.155 |
| artillery | 0.846 | 0.135 |
| air | 0.653 | **0.015** |
| **all red** | **3.841** | **0.730** |

Five times under, not the twenty-six times §5 measured, and still the largest
per-colour gap in that table.  §5 has been annotated rather than rewritten.

#### 14.9.2 The defect, which is one sentence and one consequence

`board_yields.tech_upgrade` answers exactly one question — *"develop it and
upgrade the workers I already have"* — and `_upgradable_onto` is the engine's
own relation: same type, strictly lower level, at least one worker standing on
it.  **Knights, Cannon and Air Forces are the lowest card of their own type in
the base game.**  There is no lower cavalry, artillery or air card to stand a
worker on, so that set is empty for them on every board that will ever exist
and their whole price is the develop half against their science, forever.  The
same is true, board by board rather than forever, of the first laboratory, the
first theatre and every other technology of a type the player has never
staffed.

Under `DEFAULT_WEIGHTS` on a fresh 2p board that put Knights at **−0.28** —
inside `weighted.row_pressure`'s `if val <= 0.0: continue`, so the card was not
merely under-valued, it was *invisible* to both row terms.

#### 14.9.3 The fix: the other staffing plan, and it is the engine's

`board_yields.build_fresh` answers *"develop it and BUILD one fresh worker on
it"*.  Nothing in it is a restatement of a rule:

* the gate is `engine/actions.py:_action_moves`' own `if p.workers_free > 0`,
  plus `effects.build_cost is not None` and, for an urban type,
  `urban_workers[typ] < Stats.urban_limit`;
* the resource cost is `actions.build_cost_net`, the function that charges the
  player, net of the per-turn `mil_discount` pool;
* the gain is an `effects.compute` diff with the technology developed and one
  worker on it, priced at `weighted.feature_marginal` — so Great Wall's
  `strengthPerInfantry`, the tactic army re-forming and the rating clamp all
  come free, exactly as they do for the upgrade plan.

`weighted.tech_value` takes the **better of the two staffing plans**, a `max`
and not a sum: they compete for the same turn, and the better of them is a
lower bound on doing both.  The develop half (`tech_levels`, `num_techs`,
`best_*`) stays outside the max because it is paid on either plan.

**FOUR FEATURES MOVE THAT NO `Stats` DIFF CAN SEE**, and this is why the
change needed its own triples rather than `_delta_triples` alone.
`weighted.features` reads `free_workers`, `workers`, `<class>_workers` and
`uprising` straight off the player.  An *upgrade* moves none of them — a
worker steps from one technology to another and every total is unchanged — so
no previous lane had to notice.  A *build* moves all four, and one of them is
a cliff: `uprising` is `discontent > p.workers_free`, weighted **−12.0**, so
staffing your last free worker while in discontent is a catastrophe the rules
already describe — RULES_SPEC §6.3, *"if discontent workers > unused workers,
skip the entire Production Phase"*, and *"unused workers do not reduce
discontent workers; they only prevent the uprising"*, which is exactly the
worker this plan spends.  That term is the reason this plan cannot be a
constant.

**ONE worker, and that is a derivation plus a measurement, not caution.**
`unit_upgrade` moves *all* eligible workers because the upgrade trade is
linear in the count and a linear optimum sits at an endpoint.  The build trade
is not linear, in three rule-level ways: `mil_discount` is a per-turn pool
`_spend_mil_discount` draws down, `uprising` is a threshold and `happy_margin`
is clamped at 3, and an urban type is capped at `urban_limit`.  Measured, at
2p under `DEFAULT_WEIGHTS` over 8 games (1,932 decisions), `p.workers_free` is
**0 at 68%** of decisions, **1 at 27%** and **≥2 at 4.7%** — so a plan of two
is legal on one board in twenty and "build one" is the whole plan on the rest.

**Deliberately NOT charged: the action.**  A build costs one military action
(unit) or one civil action (everything else) — and so does an upgrade, exactly
one per worker moved, and `tech_value` does not charge that either.  Charging
it on one of two competing plans and not the other would bias the argmax
between them for no reason in the rules.  The omission is symmetric and is
worth `ma_left` / `ca_left` = 0.05 apiece in `DEFAULT_WEIGHTS`.

#### 14.9.4 What the tests caught that inspection did not

`tests/test_build_fresh.py` (18 tests) plays each plan out for real and diffs
`weighted.features()` against the price, which is the standard
[`docs/GOVERNMENT_PRICING.md`](GOVERNMENT_PRICING.md) set for the revolution burn.  It found two
things:

1. **A cache-key collision.**  `effects.stats_key` names every field
   `effects.compute` reads *and nothing more*, so two boards differing only in
   `yellow_bank` collide on it — and `yellow_bank` is exactly what
   `economy.happy_required` reads, i.e. one side of the `uprising` threshold.
   The key now carries `workers_free`, `yellow_bank` and `mil_discount`.
2. **An inherited unpriced channel, now `docs/OPEN_ITEMS.md` §2 item 30.**
   `blue_free` and `corruption_loss` are not `Stats` fields — they come from
   `effects.blue_available`, which counts the blue tokens your food and
   resource banks stand on, and a higher-level farm or mine holds more per
   token.  Staffing one frees blue tokens and cuts corruption, and *nothing
   prices it*: upgrading Bronze → Iron moves `blue_free` 8 → 13 and
   `corruption_loss` 2 → 0 today, on the existing upgrade path.  Left alone so
   this lane's digest moves have one cause; the ratchet in
   `TestThePriceIsWhatFeaturesActuallyMove.UNPRICED` fails if a *new* channel
   joins it.

#### 14.9.5 What it did — take rates, 2p, paired, `DEFAULT_WEIGHTS`

`tools/take_census.py`, **100 games / 200 seat-games** at 2p, the same seeds
either side, the only difference being `build_fresh_credit` 0.0 → 1.0.
Descriptive, not a strength claim — and note this is the credit **turned on**,
which §14.9.6 then measures and decides against shipping.

| takes per seat-game | human 2p | before | after |
|---|---|---|---|
| infantry | 1.120 | 0.425 | 0.340 |
| cavalry | 1.222 | 0.155 | **0.280** |
| artillery | 0.846 | 0.135 | 0.150 |
| air | 0.653 | **0.015** | **0.075** |
| **all red** | **3.841** | 0.730 | **0.845** |
| yellow (farm/mine) | 2.520 | 2.690 | 2.460 |
| blue (urban) | 3.710 | 4.440 | **4.970** |
| green (special) | 3.080 | 0.345 | 0.285 |
| action | 12.820 | 7.905 | 7.085 |
| all civil cards | 34.300 | 19.950 | 19.195 |

**Read this honestly: it is a real move and a small one.**  The three cards
item 28 named are the ones that move — cavalry 1.8×, air 5× off an
effective zero — and the direction is the derivation's, because those are
exactly the types with no upgrade to ride.  Infantry falls, which is the same
substitution §14.8 saw: infantry always had a plan and now competes with three
types that also do.  Red as a whole is still **4.5× under** the human rate and
that is stated, not explained away; the residual is item 21's two open halves
(the free-worker pool is empty at 68% of decisions, and only one build is
priced) plus the fact that the bot takes 19.2 civil cards a seat-game against
a human 34.3 — a budget problem no per-card price can fix.

**Blue goes the wrong way** — 4.44 → 4.97 against a human 3.71 — and it is
reported rather than tuned against, for the reason
[`docs/GOVERNMENT_PRICING.md`](GOVERNMENT_PRICING.md) gives for the same surprise: there is no free
constant in the price to tune.  A first theatre really does produce +3
culture, which is the *example item 21 itself uses*, and pricing it correctly
is not made wrong by the level being off.  The level is what the league
trains — and §14.9.6 is the measurement that says so.

#### 14.9.6 The credit ships at **0.0**, and that is the A/B's decision

Its four siblings (`unit_tech_credit`, `tech_board_credit`,
`action_board_credit`, `gov_board_credit`) all ship at 1.0.  This one does
not, because it was measured and 1.0 is worse.  `experiments.evaluate`, the
credit against **itself** at 0.0 with nothing else touched, paired on the deal
(`hillclimb_league._series`, one `arena.duel` per arm on identical seeds),
400 games / 200 deals at 2p:

| vector | credit | win rate vs a 50% null | p | culture |
|---|---|---|---|---|
| `DEFAULT_WEIGHTS` | 1.0 | **44.1% ± 4.6** | 0.0125 | 105 vs 112 |
| live 2p champion (gen 83) | 1.0 | **44.5% ± 4.8** | 0.0229 | 145 vs 156 |
| `DEFAULT_WEIGHTS` | 0.5 | **44.9% ± 3.6** | 0.0055 | 112 vs 114 |

Two independent vectors and two settings, the same ~5.5pp loss, all
significant, all above [`docs/HAZARDS.md`](HAZARDS.md) §1's n ≥ 200 bar.

**THE CHAMPION ROW IS THE INFORMATIVE ONE, and it kills the obvious
explanation.**  The first guess was `docs/OPEN_ITEMS.md` §2 item 19(b): this
is the first card price in the project that ever charged `workers`,
`free_workers`, `<class>_workers` or `uprising`, and `DEFAULT_WEIGHTS` carries
untrained priors for them — `workers` 1.4 against `free_workers` 0.4, i.e. a
flat **+1.0 eval points for moving a token from the spare pile onto a card**,
before any production at all.  That guess is wrong: the live gen-83 champion
has already climbed `workers` to **0.0** and `free_workers` to **0.0046**, so
that term is absent on it, and the loss is the same size.  Something else in
the plan is over-valued and this lane did not find it.  Two candidates are
recorded rather than asserted: `_hand_total` **sums** technology prices over
the civil hand, so every buildable technology in hand credits the *same one*
free worker (inherited from the upgrade plan, but larger here); and the plan
is priced without any test that the player can afford it *by the time the
worker is still free*, which the 68%-empty pool above makes a real risk.

**THE SWEEP IS A STEP, NOT A SLOPE, AND THAT IS THE MOST USEFUL THING IN THIS
TABLE.**  0.5 and 1.0 are the same number.  That is what a credit multiplying
*one branch of a `max`* has to look like: on a card whose upgrade plan is
worth exactly nothing — which is Knights, Cannon, Air Forces on every board,
and any technology of a type the player has never staffed — *any* ε > 0 makes
the build branch win the max, and scaling ε after that changes the price but
not the argmax.  So `build_fresh_credit` is not a level knob the league can
tune down gently; it is a switch with a cliff at zero, and
`hillclimb.mutate`'s `gauss(0, s) * (abs(w) + 0.15)` will step it straight
over that cliff on the first generation that scatters onto it.  Written down
here because "the league will find the level" is the sentence this lane would
otherwise have ended on, and for this coordinate it is not true.

**So what actually lands is the shape and the measurement**, and the
measurement is a finding rather than a disappointment: pricing a plan the
rules plainly offer, from the engine's own move generator, with every triple
pinned against `features()` itself, makes the bot *weaker* — which says
something in the evaluator is wrong somewhere else.  `docs/OPEN_ITEMS.md` §2
item 31 is that question with the two surviving candidates written down.  The
precedent for landing it at 0.0 is `free_action_credit` (item 27): buying a
take rate by turning a credit up is measurably wrong, so it is not turned up.
Every non-league caller — the tests, `tools/take_census.py` with its own
vector, `tools/card_census.py` — gets the price today by passing the credit.

**The consequence for the eight fingerprint arms is a prediction, and it is
the strong form of "provably inert":** at 0.0 `card_potential` never calls
`build_fresh` at all (`tests/test_build_fresh.py:
test_credit_zero_makes_the_branch_unreachable` monkeypatches it to raise and
prices all 236 cards on two boards), the fingerprints play `DEFAULT_WEIGHTS`,
so **all eight arms had to hold** — including the six that move for every
other pricing lane.  See `tools/gate.sh`.

## 15. The bot builds one civilization and it is blue, because `card_potential` reads weights `evaluate` does not use (merged from the former `YELLOW_TECH_PRICING.md`, 2026-07-31)

2026-07-30.  Closes the largest **non-inert** discrepancy in
[`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md) (§17.5.1, "why yellow is dead and blue is doubled") and
open item 1 of [`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md) ("`tech_levels` is unpriced on every
technology card").  They are one problem and are fixed together.  Base game
(2015), all three player counts.

### 15.0. The finding, and the numbers it is

[`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md) measured, per seat-game, bot against human:

| type | 2p h | 2p bot | 3p h | 3p bot |
|---|---|---|---|---|
| **lab** | 1.62 | **0.03** | 1.27 | **0.00** |
| **mine** | 1.18 | **0.05** | 1.21 | **0.00** |
| **farm** | 1.34 | 0.18 | 1.26 | 0.13 |
| temple | 0.51 | **1.26** | 0.46 | 0.06 |
| library | 0.70 | **2.19** | 0.95 | 0.66 |
| theater | 0.65 | **2.27** | 0.99 | 1.96 |

Alchemy, Scientific Method and Coal are taken **zero times at any table size**.
The 2p bot takes 23.2 civil cards a seat-game against a human 34.2 and spends
that smaller budget almost entirely on one colour.

### 15.1. The mechanism, verified before anything was changed

Four claims, each re-derived here rather than taken from the audit.

**(a) Every yellow production technology prices strictly negative.**
`card_potential`, no changes, live 2p champion (gen 72):

| card | type | price |
|---|---|---|
| Irrigation | farm | **−4.02** |
| Selective Breeding | farm | **−6.19** |
| Iron | mine | **−6.72** |
| Coal | mine | **−10.78** |
| Oil | mine | **−13.05** |
| Alchemy | lab | **−11.19** |
| Scientific Method | lab | **−15.06** |
| Computers | lab | **−20.41** |

Reproduces [`CARD_BLINDNESS.md`](CARD_BLINDNESS.md#1751-why-yellow-is-dead-and-blue-is-doubled--class-c-and-probably-wrong) §17.5.1 to the digit.  The same table gives
Opera **+79.48**, Drama **+56.04**, Religion **+27.08**, Printing Press
**+25.75** — the over-played half, and the ordering is the whole finding.

**(b) `row_pressure` really does skip them.**  `weighted.py`: `val =
card_potential(...)`, `if val <= 0.0: continue`.  So a laboratory in the civil
row is invisible to `row_urgency` and `row_bargain_forgone` at any weight, on
top of *lowering* `hand_potential` while it is held.  Pinned in
`tests/test_yellow_pricing.py:TestRowPressureCanSeeAYellowTechnology`.

**(c) It is what suppresses the take, and — exactly as in the red lane — it is
not the whole gap.**  Reference play, live 2p champion under
`plan:width=2,det=1`, 6 games, every legal move scored 1-ply with the bot's own
evaluator:

* a farm/mine/lab take was **legally available at 304 of 1,546 decisions
  (20%)**;
* the best yellow take was the **best move of all 0 times in 304**;
* at the 301 decisions where a yellow take *and* a non-yellow take were both
  legal, the best yellow take was the best take **1 time in 301**, a median
  **1.43 eval points** behind the best other take.

The counterfactual that isolates the bias: floor a yellow card's
`card_potential` at zero — remove the negative, add nothing — and the same
probe gives **12 in 295** and a median gap of **0.92**.  A twelvefold move, so
the negative pricing is load-bearing; and 12/295 is 4.1%, so **a card worth
exactly zero is still not a card worth taking**.  Identical shape to
[`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md) §14.1c (1/437 → 20/437), and it is why this is a
repricing and not a clamp.

**(d) The cause is NOT the one the audit named — it is bigger.**
[`CARD_BLINDNESS.md`](CARD_BLINDNESS.md) attributed the collapse to the ratio `culture_rate` 31.68
against `science_rate` 0.25.  That ratio is real but it is not what `evaluate`
uses.  `science_rate` is in `weighted.PHASE_KEYS`, so the evaluator prices it at
`w[k] + (1−L)·w[k_early] + L·w[k_late]`, and `card_potential` looked up the
bare `w[k]`:

| feature | `w[k]` | marginal, early | marginal, late |
|---|---|---|---|
| `science_rate` | 0.250 | **5.291** | −0.009 |
| `tech_levels` | 5.841 | **9.230** | 6.759 |
| `culture_rate` | 31.678 | 31.292 | 33.169 |

Two half-priced gains, then, not one ratio:

1. **`tech_levels` was mapped to nothing at all**, on every technology card in
   the game — farms, mines, labs, urban buildings, units and special
   technologies alike.  At up to 9.23 eval points per level it is worth more
   than the rest of a yellow card put together, and `_card_yields` has no entry
   for it.  So is `num_techs`, and so is the whole `best_*` family.
2. **A rate was priced at the bare weight.**  For `science_rate` that is a
   factor of **twenty-one** early — the same shape as the factor of fifteen
   `strength_marginal` was written for in the red lane, and the same cause
   (phase multipliers the card path never applied).

`culture_rate` happens to be the one rate whose phase pair nearly cancels, so
the blue half of the row was the only half priced approximately right.  That,
not a preference, is why the bot builds theatres.

### 15.2. What changed

Three functions, one new weight.

**2.1 `weighted.feature_marginal(key, state, idx, w, late, ctx)`** —
d(`evaluate`)/d(`features()[key]`), for any key.  `strength_marginal` is the
`strength` case and is delegated to rather than respelled (the board expresses
one point of army through four features, and that function is the one place
that sums them).  Every other key is linear in exactly one feature, so its
marginal is its weight plus the phase pair when `PHASE_KEYS` gives it one.

It is checked **numerically against `evaluate` itself**
(`TestFeatureMarginal.test_it_equals_the_numerical_derivative_of_evaluate`):
`evaluate` accepts a precomputed feature dict, so the derivative is taken
exactly — bump one entry by one, re-evaluate, require the difference to equal
the claimed marginal to nine places, over 100+ self-play positions and eight
keys.  A comment claiming to be a derivative is not a derivative.

**2.2 `board_yields.tech_upgrade(name, state, idx)`** — one board query for all
fifteen technology types, returning `(staff, develop, science, resources)`:

* **develop** — `tech_levels` (the age level), `num_techs`, the `best_*` step
  and `special_techs`.  Paid the moment the card is developed, staffed or not.
  This is the half that was missing entirely.
* **staff** — the `effects.compute` diff of developing the card and moving
  every worker that could **legally** upgrade onto it, which is same type and
  strictly lower level (`engine/actions.py:_tableau`'s `higher` relation, read
  backwards).  The resources are `actions.upgrade_cost`, the science is
  `effects.tech_cost` — the functions that charge the player.  Nothing is
  restated: the diff picks up Transcontinental Railroad's `doubleBestMine`,
  St. Peter's happy clamp and the government's urban limit for free.

The `Stats`-to-feature mapping is `_delta_triples`, **factored out of the
existing swap diff rather than copied**, so the leader/government/wonder path
and the technology path read the same fields through the same feature names.
Two copies of that list is exactly how `_PROD_TO_FEATURE` and
`_YIELD_TO_FEATURE` drifted apart.

The red half delegates to `unit_upgrade`, unchanged, so
[`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md)'s measured result is not silently re-opened.

**2.3 `weighted.tech_value`** — `unit_tech_value`, generalised and renamed.
Staffing is `max(0, Σ amount·feature_marginal − resources·w[resource_stock])`,
because you develop first and decide how many workers to move second and the
trade is linear in that count (so the optimum is an endpoint); the develop half
and the science cost are charged **outside** that max, so a technology nobody
will staff still reads as its levels minus its science.

**2.4 The new weight is `tech_board_credit`, default 1.0.**  It gates the
whole board price for the eleven non-red technology types, and the *develop
half only* on the four red ones — so **one constant at 0.0 recovers the parent
commit's pricing byte for byte on all 236 cards**, which is what makes every
measurement below a paired A/B in one process on the same deal.  It is a new
key, so `load_weights` fills it in from `DEFAULT_WEIGHTS` on every champion
file in the league and the change is live on all three arms at once.

1.0 is not a guess at a magnitude, for the same reason `unit_tech_credit`'s is
not: at 1.0 the number *is* "the eval points `evaluate` itself assigns to the
technology levels and the production this development buys, minus the eval
points it assigns to the resources and science it costs".  There is no free
constant left in it.

#### 15.2.5 One implementation, not two

* the production/strength delta is an `effects.compute` diff — the engine's own
  arithmetic;
* the resources are `actions.upgrade_cost`, the science `effects.tech_cost`;
* the `Stats`→feature table is shared with the swap diff, not duplicated;
* `feature_marginal` is checked numerically against `evaluate`;
* `unit_upgrade` is called, not re-derived.

No information is added: `tech_upgrade` reads the acting player's own tableau,
which is public.

### 15.3. What it did — before/after, `tools/play_rate.py`

Mirror table, `plan:width=2,det=1`, same seeds, same vector, the only
difference being `tech_board_credit` 0.0 → 1.0.  `tools/play_rate.py bot`
reuses `tools/system_census.py` unchanged, so the take counts and the
subsystem counts come out of one run.

**Which vector is which column matters.**  2p is the **live** champion (gen
72).  3p is **`DEFAULT_WEIGHTS`**, which is byte-identical to the live
`champion_3p.json` and `champion_4p.json` (both gen 0), and is therefore the
configuration the league will actually train — deliberately NOT the archived
pre-restart 3p vector [`CARD_BLINDNESS.md`](CARD_BLINDNESS.md) censused, whose 0.00 laboratories
and 0.00 mines belong to that vector and not to the defaults.  30 games at 2p
(60 seat-games), 20 at 3p (60 seat-games).

| per seat-game | 2p before | 2p after | 2p human | 3p before | 3p after | 3p human |
|---|---|---|---|---|---|---|
| **lab** | **0.02** | **1.77** | 1.62 | 1.87 | 1.87 | 1.27 |
| **mine** | **0.03** | **0.85** | 1.18 | 0.72 | **1.67** | 1.21 |
| **farm** | **0.07** | **0.87** | 1.34 | 0.58 | **1.40** | 1.26 |
| tech: yellow | 0.10 | 1.72 | 2.52 | 1.30 | 3.07 | 2.47 |
| tech: red | 0.95 | 3.18 | 3.84 | 1.53 | 3.57 | 2.79 |
| tech: blue | 5.75 | 6.00 | 3.71 | 7.67 | 5.12 | 3.86 |
| tech: green (special) | 1.75 | 2.40 | 3.08 | 0.47 | 0.87 | 2.45 |
| temple | 1.23 | 0.97 | 0.51 | 1.23 | 1.20 | 0.46 |
| library | 2.20 | 1.77 | 0.70 | 1.93 | 0.83 | 0.95 |
| theater | 2.23 | **0.82** | 0.65 | 1.93 | **0.85** | 0.99 |
| arena | 0.07 | 0.68 | 0.32 | 0.70 | 0.37 | 0.30 |
| **action** | 8.62 | **2.72** | 12.98 | 8.13 | **5.90** | 10.25 |
| leader | 3.05 | 2.38 | 3.70 | 2.92 | 2.63 | 3.62 |
| wonder | 2.50 | 2.57 | 2.87 | 0.02 | 0.00 | 2.58 |
| civil cards taken | 24.23 | 22.05 | 34.3 | 23.65 | 22.62 | 29.6 |
| develops | 6.37 | 7.40 | — | 5.97 | 7.13 | — |
| upgrades | 2.73 | 3.57 | — | 6.88 | 10.05 | — |
| builds | 6.62 | 8.65 | — | 9.75 | 9.97 | — |
| wonders completed | 1.88 | 1.73 | 2.74 | 0.02 | 0.00 | 2.45 |
| wars declared | 0.60 | 0.42 | 0.26 | 0.72 | 0.73 | 0.16 |
| aggressions played | 0.87 | 0.78 | 1.39 | 0.17 | 0.18 | 1.63 |
| colonies held at end | 0.62 | 0.70 | 1.51 | 0.87 | 1.15 | 1.15 |
| government changes | 0.68 | 0.50 | 1.14 | 0.62 | 0.65 | 1.16 |
| **mirror score /seat** | 191.7 | 181.9 | 160 | 126.6 | **147.4** | 176 |

**The zero is gone.**  Laboratories go 0.02 → 1.77 at 2p against a human 1.62
— from 65× under to *within 10%* — mines 0.03 → 0.85 and farms 0.07 → 0.87.
Alchemy, Scientific Method and Coal, taken zero times at any table size in the
audit, are taken 0.65 / 0.58 / 0.15 times a seat-game at 2p after.  At 3p on
the defaults the mines and farms roughly double and land on the human rate.

**And the blue over-play is cured by the same change**, which is the part that
was not asked for and is the better evidence that the model is right rather
than merely louder: theatres go 2.23 → 0.82 against a human 0.65, libraries
2.20 → 1.77, temples 1.23 → 0.97.  Nothing in this change penalises a theatre.
What happened is that an urban card is now priced as the **upgrade delta** it
actually is — Opera off Drama is +1 culture for 4 resources, not +3 culture for
8 — so the static table's free fresh building stopped being free.  On the live
2p champion Opera goes +79.48 → +15.47 and Alchemy −11.19 → +9.75 in the same
edit.

**What it traded, reported rather than buried.**  Action cards.  They fall
8.62 → 2.72 at 2p and 8.13 → 5.90 at 3p, against a human 12.98 / 10.25, and
they are now the largest single deficit in the game.  Civil cards taken barely
move (24.2 → 22.1 and 23.7 → 22.6 against a human 34.3 / 29.6), so this is a
**substitution, not a widening**: the bot swapped roughly six action cards for
five technologies.  An action card is priced by the static table and nothing in
this change touched it, so the whole of the movement is relative — technologies
got more expensive to pass up.  [`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md) §14.7.1's guess that
`tech_levels` was "the most likely single explanation for 23.5 civil cards vs a
human 34.3" is **not** supported: pricing it moved the mix and left the count
alone.

Wonders, wars, aggressions and colonies are flat at both counts.  The 3p wonder
column is 0.02 → 0.00 because `DEFAULT_WEIGHTS` already completes no wonders at
3p; that is [`docs/OPEN_ITEMS.md`](OPEN_ITEMS.md#11-wonder_potentials-scale-has-no-trustworthy-evidence) §1.1's hole, not this one.

### 15.4. Strength: a large win on the defaults, a severe regression on the live 2p
### champion, and the regression attributes to one stale weight

`experiments.evaluate`, the fix against **itself** — the identical vector with
`tech_board_credit` 1.0 against 0.0, so the two arms differ in exactly one
number and are paired on the deal.  Seat-balanced, `WeightedBot`.

| vector | games | win rate | paired CI | null | p | culture margin |
|---|---|---|---|---|---|---|
| 2p, **`DEFAULT_WEIGHTS`** | 300 | **70.50%** | ±5.03pp | 50% | 8.6e−16 | **+26.0** |
| 3p, **`DEFAULT_WEIGHTS`** | 240 | **41.67%** | ±6.93pp | 33.3% | 0.017 | **+14.3** |
| 2p, **live** champion (gen 72) | 300 | **12.17%** | ±3.46pp | 50% | 1.1e−103 | **−95.0** |
| 3p, archived champion (gen 1314) | 240 | *see below* | | 33.3% | | |

#### 15.4.1 `DEFAULT_WEIGHTS` is the row that decides this operationally

70.5% at 2p and 41.7% at 3p, both against their nulls, both with a positive
culture margin.  **`experiments/league_state/champion_3p.json` and
`champion_4p.json` are gen 0 and byte-identical to `DEFAULT_WEIGHTS` today**,
so this is the configuration two of the three live arms start from and the one
every fresh arm will ever start from.  On it the change is a large,
unambiguous improvement, and the 2p number is the biggest single-change win
this project has measured in a while.

#### 15.4.2 The live 2p champion: a severe regression, attributed

12.2% against a 50% null is worse than anything the unit lane produced, and it
is not noise.  It has to be read together with what that vector believes:

    tech_levels        5.8409       culture_rate      31.6777
    tech_levels_early  3.3894       culture_rate_early -0.3858
    tech_levels_late   0.9186       science_rate       0.2501

`feature_marginal("tech_levels")` on that vector is **9.23 eval points per age
level early**, so an Age III technology is worth 27.7 points of "technology"
before anything it produces is counted.  Handed that opinion, the fix buys
technology instead of culture and the culture collapses.

The attribution is a measurement, not an argument.  Three arms, each the same
paired A/B on the same champion with one further thing changed:

| arm | change | win rate | null | culture margin |
|---|---|---|---|---|
| as measured | — | **12.17%** | 50% | −95.0 |
| **`tech_levels` group reset to the defaults** (5.84/3.39/0.92 → 1.0/0.5/−0.4) | | **63.00%** ±5.2pp | 50% | **+29.9** |
| `row_urgency` / `row_bargain_forgone` zeroed | | 21.50% | 50% | −55.0 |
| `hand_potential` zeroed | | 34.17% | 50% | −44.8 |

**Reset that one weight group and the −38pp regression becomes a +13pp win.**
Muting the two channels the price flows through only recovers part of it,
which is what says the problem is the *price* and not the plumbing.

> **`tech_levels` was an unconstrained coordinate, exactly as `strength` was.**
> Nothing in `card_potential` had ever mentioned it, so the only way the climb
> could pay for a technology level was the `develop` move itself — where the
> alternative on the table is usually another action of the same turn, not a
> point of culture rate.  The weight could therefore drift as high as noise
> carried it without ever costing a game.  `tech_value` is the first term in
> this project that charges the evaluator its own stated price for a technology
> level **at the moment the card is taken**, and the first thing it did was
> expose that the price was fitted on a free lunch.  This is the second
> instance of the mechanism [`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md#1452-3p-on-the-archived-champion-a-large-unambiguous-regression) §14.5.2 named, found by
> looking exactly where that section said to look.

#### 15.4.3 What to do about it, stated plainly

* **`tech_board_credit` = 0.0 in `experiments/league_state/champion_2p.json`
  recovers the parent commit's pricing byte for byte**, needs no code change,
  and is the zero-risk option for the live 2p arm.
* The better option is to **re-fit the `tech_levels` group** on a vector that
  has to pay for it.  §15.4.2's second row is the evidence that this is not merely
  possible but likely to be a strengthening: the champion with a *default*
  `tech_levels` and the fix on beats the champion with a *trained*
  `tech_levels` and the fix off by 13pp.
* Nothing at 3p or 4p is in the regressing regime today, because both live
  champions are `DEFAULT_WEIGHTS`.
* The standing policy is that correct modelling is worth committing whether or
  not it strengthens the bot.  On that basis this lands: two large wins on the
  configuration that will be trained, one severe regression on one stale
  vector, attributed and reproducibly reversed by resetting one weight group.

#### 15.4.4 The archived 3p champion, reported separately

`archive_prequiescent_20260730/ladder_3p/gen01314`, the vector
[`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md#1452-3p-on-the-archived-champion-a-large-unambiguous-regression) §14.5.2 already warns against warm-starting from:
**10.83% ± 4.18pp against a 33.3% null, culture margin −31.5**, n = 240.  It
carries `tech_levels` 5.04 — the same over-fitted coordinate the live 2p
champion has — so it is the same finding on a second stale vector and not an
independent one.  It is reported because the brief asked for it and NOT used
as evidence either way; nothing in the league is in that regime today.

### 15.5. Fingerprints

Six of the eight `tools/gate.sh` arms moved and two did not.

| arm | parent | this commit | third clone, credit 0.0 |
|---|---|---|---|
| NARROW (greedy) | ca255af3 | ca255af3 | ca255af3 |
| WIDE (greedy) | f223cea1 | f223cea1 | f223cea1 |
| WNARROW | dc1e3bbe | **16dc9a1a** | dc1e3bbe |
| WWIDE | f401b342 | **a1b74078** | f401b342 |
| QNARROW | 02f63fe7 | **2f59c5c0** | 02f63fe7 |
| QWIDE | 2f1f774e | **23b8d66e** | 2f1f774e |
| PNARROW | b17d2aa1 | **15bd49fc** | b17d2aa1 |
| PWIDE | d2240d3c | **c8fe5d3a** | d2240d3c |

* **The two GreedyBot arms held still.**  GreedyBot never calls
  `card_potential`, so an arm of it moving would have meant the change had
  leaked into the rules.  It did not.
* **All six evaluator arms moved** — WeightedBot, QuiescentBot and PlanBot,
  narrow and wide.  Expected and predicted before the run: `DEFAULT_WEIGHTS`
  carries `tech_board_credit` at 1.0, so every technology in the row and in the
  civil hand prices differently for all three searching bots.
* **Two-sided** per `docs/PYPY.md` (deleted) §9.0: derived from scratch in `/tmp/gateA`
  and independently in `/tmp/gateB`, two separate copies of the same tree,
  which agreed byte for byte on all eight arms — **including the two that did
  not move**.  A clean-base control on the parent commit (`c0525c4`, in
  `/tmp/base`) reproduced all eight pre-change constants first.
* **Attributed to one constant.**  A third clone with `"tech_board_credit":
  1.0` changed to `0.0` and nothing else touched reproduces **all eight**
  pre-change digests byte for byte.  So the six moves are that one default and
  nothing else in the change; `feature_marginal`, `tech_upgrade`,
  `_delta_triples`, `_upgradable_onto` and `_is_levelled_tech` are provably
  inert on their own.

Nothing was re-derived to make the gate pass: it failed by design in both
clones and the committed constants are the computed values.

Test count **1053 → 1070**.  +16 from `tests/test_yellow_pricing.py`, +1 from
splitting `test_zero_credit_is_the_static_answer_for_every_card` in
`tests/test_board_yields.py` a second time, which needed a third sibling once
the non-red technologies started being gated on `tech_board_credit`.

**Negative control on the regression test**, in the sense
`tests/test_search_root_is_determinized.py` uses: dropped onto a clean tree at
the parent commit, `tests/test_yellow_pricing.py` gives **5 failures and 8
errors** of 16.  The three that still pass there are exactly the ones written
to pass — the two `TestTheDefect` controls (the static table still cannot see
`tech_levels`, and still prices a rate at the bare weight) and
`test_zero_credit_recovers_the_static_answer`, which is trivially true when
there is no credit.

### 15.6. Open, and deliberately not done here

1. **`unit_upgrade` pools workers across all four red types.**  It moves every
   unit worker onto the candidate technology and charges `upgrade_cost` from
   each, but `engine/actions.py:_action_moves` only offers an upgrade between
   cards of the **same** type — a Warriors worker cannot become a Cannon.  So
   the red price is optimistic for cavalry, artillery and air whenever the
   player holds only infantry, which is most of the game.  Found while
   generalising it; **not fixed here**, because fixing it would put a second
   cause under this lane's digest moves and re-open a settled A/B.  Its own
   commit, with its own measurement.  `tech_upgrade`'s non-red half is already
   same-type-only (`_upgradable_onto`), so the defect is confined to the four
   red types.
2. **Nothing prices the "build one fresh" plan.**  Both halves of the query
   answer "develop it and upgrade what I have"; a player with no laboratory
   worker gets a laboratory priced at its levels minus its science, and the
   +3 culture a *new* theatre would produce is priced at nothing.  That is the
   honest answer for a one-ply appraisal (building is its own decision, needs a
   free worker, and has its own price) and it is the position `unit_upgrade`
   already took, but it systematically under-prices the first building of a
   type.  Pricing it needs the free-worker and food cost modelled, which is a
   larger change.
3. **A government's level is still unpriced.**  `features()` adds
   `meta[p.government][1]` into `tech_levels`, and neither the static table nor
   the swap diff emits it — so a government card is missing exactly the term
   this document adds to every other technology.  `gov_level` has the same
   hole.  Governments are already over-played (1.63 vs a human 1.37 at 2p,
   2.44 vs 1.41 at 3p), so this one plausibly cuts the other way and belongs
   in the lane that re-measures governments.
4. **`happy_margin` is priced linearly through the clamp.**  `features()`
   computes `min(3, margin)` and `max(0, -margin)`; `_delta_triples` maps
   `Stats.happy` straight onto `happy_margin` with no conditioning, so a
   temple's happy face is worth the same to a player at margin 0 and a player
   at margin 3.  Inherited from the swap diff, widened by this change to the
   five urban types, and it is the obvious next term for
   `feature_marginal` to learn (the `strength_deficit`/`strength_lead`
   treatment, one feature over).
5. **Four sampling tests re-rolled and had to be widened.**  Any pricing
   change re-rolls self-play, and `test_event_scoring`,
   `test_harness_fields` (×2) and `test_search_root_is_determinized` each
   pinned a property to a single fixture position that stopped exhibiting it.
   All four were verified to hold on the parent tree at other positions before
   being widened; `test_the_expensive_opponent_board_is_never_mandatory_today`
   additionally had its bar corrected from MANDATORY to MOVE, because
   `rival.techs` reaches RANK on the parent tree too and the operator doc's
   claim rests on the recommended move, not on the tail ordering.

## 16. Sixteen of the thirty-three action cards are worth exactly nothing, because three of the coordinates they are priced in are not features (merged from the former `ACTION_CARD_PRICING.md`, 2026-07-31)

*2026-07-30.  Companion to [`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md) and
[`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md); same sentence, third colour:* **a card is worth
what `evaluate` pays for what it does.**

### 16.0. The finding, and the numbers it is

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
the evaluator *could* read but never had to *buy* ([`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md)
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

### 16.1. The classification, per card

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
  mechanisms (see §16.2).
* **(c) priced, weight live, the bot declines.**  **Fourteen cards** — and for
  eleven of them the decline is defensible (they are at or above the human take
  rate).  Cultural Heritage is the one taken *more* than humans take it (0.78
  against 0.28) and the reason is §16.2.3: its four culture is priced at the bare
  `w["culture"]` where `evaluate` pays a phase blend that is *lower* than the
  bare weight for most of the game.
* **(d) NOT PRICED AT ALL.**  **No whole card**, but one real sub-item: nothing
  prices **which** action a `freeCivilAction` orders.  Rich Land ("a farm or a
  mine") and Urban Growth ("an urban building") are the same card to
  `card_potential` apart from their discount, and always will be until
  something asks the board what the best legal free build would be.  Left open,
  §16.6.

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

### 16.2. The mechanism, in three parts

#### 16.2.1 Two of the coordinates are not features (13 cards at exactly 0.000)

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

#### 16.2.2 A choice is multiplied by the board credit (3 cards at exactly 0.000)

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

#### 16.2.3 A one-shot `gainCulture` is priced at the bare weight

`culture` is in `PHASE_KEYS`.  `evaluate` pays
`w[k] + (1-L)w[k_early] + L·w[k_late]`; `card_potential` reads the bare `w[k]`.
On the defaults that is 1.0 against a marginal of 0.6 in Age A rising to 2.5 in
Age IV.  This is the **same** phase-blend mismatch `feature_marginal` was
written for in [`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md), still live on the one-shot gains
because that lane only routed technologies through it.  It is why Cultural
Heritage is the one action card the bot *over*-takes.

### 16.3. What changed

`weighted.action_value(name, state, idx, w)` — `tech_value`'s sibling for the
one civil type still on the static table.  Everything in it is a derivation:

1. **A one-shot gain is worth `feature_marginal`, not `w[key]`.**  Closes §16.2.3
   for every action card at once.
2. **A free civil action is worth `free_action_credit` civil actions, and
   that credit ships at 0.0 because the action economy is a wash.**  This is
   the one number here that is not a pure derivation, the first draft of it got
   the derivation backwards, and §16.5 is the correction written out.  RB §3.11 /
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
4. **A choice is a max and needs no board credit.**  Closes §16.2.2.
5. **The three per-table-size cards keep their board scaling.**
   `board_yields.board_extra` already computed rivals-with-more-culture and
   rivals-stronger-than-me from the live boards; it is **called**, not
   reimplemented, so there is one implementation and it cannot drift.

Not charged, deliberately: the civil action playing the card costs (point 2),
and the card leaving the hand, which `hand_value` already prices on the board
side.

No information is added: the effects are printed on the card, and
`board_extra` reads only public culture totals and public strengths.

#### 16.3.1 The gate, and it is one constant

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

### 16.4. What it did — before/after, `tools/play_rate.py`

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
§16.0's "priced at 0.000" half goes up and every card in the live-weight half
stands still.  Wave of Nationalism, the single worst outlier in the game at
0.02 against a human 0.31, goes to 0.25 and starts being **played** (0.02 →
0.18).  Endowment for the Arts more than doubles at both counts.  Rich Land,
Urban Growth, Efficient Upgrade and Engineering Genius all rise 35–140%.

**What did NOT happen, reported rather than buried: the 4.8x gap is not
closed.**  7.30 → 9.00 at 2p against a human 12.98, and 5.83 → 7.53 against
10.25 at 3p.  About a quarter of the gap.  The residual is not a bug — it is that a
`freeCivilAction` card is genuinely worth only its discount (§16.3 point 2), and
2–4 resources is a smaller thing than the technology it competes with in the
row.  Whether humans are right to take twelve of these a game is a question
about the *rest* of the evaluator, not about whether these sixteen cards are
priced; that half is now unambiguously fixed.

Total civil takes are flat on the old base too (23.35 → 22.63 at 2p, 22.58 →
22.52 at 3p) and developments near-flat (6.05 → 5.67, 7.15 → 6.92).


### 16.5. Strength: the paired A/Bs, and the modelling error the first one caught

**These A/Bs were run before the owner's 2026-07-30 instruction to stop
running them and let the league logs be the measurement.**  They are reported
because they exist and because the first one is what caught the modelling error
in §16.3 point 2 — not because anything was waiting on them.  `experiments.
evaluate`, the fix against **itself** on `DEFAULT_WEIGHTS`, `WeightedBot`,
paired on the deal, the two arms differing in exactly one number.

| arm | games | win rate | null | p |
|---|---|---|---|---|
| 2p, `free_action_credit` **1.0** | 300 | **32.83%** ±5.2pp | 50% | ~0 |
| 2p, `free_action_credit` **0.5** | 300 | **41.33%** ±5.5pp | 50% | 0.0019 |
| 2p, `free_action_credit` **0.0** (shipped) | 300 | **47.67%** ±4.6pp | 50% | 0.31 |
| 3p, `free_action_credit` **0.0** (shipped) | 240 | **32.71%** ±5.6pp | 33.3% | 0.82 |

**The shipped default is a null at both counts, and the credit sweep is
monotone.**  That monotonicity is the evidence for §16.3 point 2 and it is the
reason the default is 0.0 rather than a guess: at 1.0 the behaviour census
looked like a triumph — action takes 7.35 → **11.80** against a human 12.98,
landing exactly on the human number — while the bot lost 17pp of win rate,
because the same civil actions stopped buying technologies (11.9 → 8.0
developed a game) and 5.5 action cards a seat-game were taken and never played.
**Right rate, wrong reason.**  A behaviour census alone would have shipped it.

This is the standing policy case: correct modelling gets committed on the
modelling, and the null is what a correct model of sixteen previously-invisible
cards should look like when the rest of the vector was fitted without them.


### 16.5.1 Fingerprints

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
across a base change is exactly the laundering `docs/PYPY.md` (deleted) §9.0 forbids, so
the clean-base control, both derivations and the attribution were all re-run
from scratch on `7bf483a`.  The discarded first set is recorded in
`tools/gate.sh` so a reader can see it was discarded rather than reconciled.

* **The two GreedyBot arms held still**, which is the informative half:
  GreedyBot never calls `card_potential`, so an arm of it moving would have
  meant a card-pricing change had leaked into the rules.
* **All six evaluator arms moved**, predicted before the run: `DEFAULT_WEIGHTS`
  carries `action_board_credit` at 1.0, so every action card in the row and in
  the civil hand prices differently for all three searching bots.
* **Two-sided per `docs/PYPY.md` (deleted) §9.0**: derived from scratch in
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

### 16.6. Open, and deliberately not done here

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

## 17. Does the bot actually PLAY the cards? A per-card play-rate audit (2026-07-30) (merged from the former `PLAY_RATE_AUDIT.md`, 2026-07-31)

Four audits have now measured card **pricing** — [`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md),
[`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md), [`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md),
[`docs/UNCOVERED_TYPES.md`](UNCOVERED_TYPES.md) — and [`CARD_BLINDNESS.md`](CARD_BLINDNESS.md) states the gap all four share
in its own words: *"the suite checks that a card is priced, never that its
price is read."*

`unit_strength_credit` is what that gap cost. The ten military unit cards were
found blind, a feature was added, four tests in `tests/test_card_pricing.py`
were written and pass, and the weight shipped at **0.0** — so
`card_potential` multiplied the entire new channel by zero and the ten cards
priced *exactly* as they had before the fix. Nothing failed for days while
[`docs/SYSTEM_COVERAGE.md`](SYSTEM_COVERAGE.md) measured the bot taking military unit technology
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

### 17.1. Method

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
* `tests/test_play_rate.py` — the standing check, section 17.6.

```
python3 tools/play_rate.py human --out /tmp/human_cards.json
nice -n 15 python3 tools/play_rate.py bot --players 2 --games 20 --seed 0 \
    --spec plan:experiments/league_state/champion_2p.json,width=2,det=1 \
    --out /tmp/cards_2p_a.json
python3 tools/play_rate.py report --human /tmp/human_cards.json --exact \
    /tmp/cards_2p_*.json /tmp/cards_3p_*.json /tmp/cards_4p_*.json
```

#### Two measurement contracts, and they are not interchangeable

* **TAKE** (civil deck, 127 cards): the journal prints `X takes <card> in
  hand` and the bot emits a `take` move. Both sides are a free choice from a
  visible row, so the rates compare directly.
* **PLAY** (military deck, 109 cards): nobody chooses to *take* these, they
  are drawn blind. Only the decision to *use* one compares, so those rows
  count plays, declarations, tactic set-ups, colonizations and defence spends
  on both sides. Territories are counted as **colonies held**, because a
  territory is won at auction and never "played"; events are counted as
  **revealed**, because nobody chooses to play one.

#### The name join is at base name, and that is a real limit

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
reproduces [`docs/SYSTEM_COVERAGE.md`](SYSTEM_COVERAGE.md)'s human unit-technology rate to the
digit — **3.84 / 2.79 / 3.43** per seat-game at 2p/3p/4p.

### 17.2. Headline: the bot builds one civilization and it is blue

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
0.16 and 0.15), both of which [`docs/SYSTEM_COVERAGE.md`](SYSTEM_COVERAGE.md) already reported at the
whole-subsystem level. This document adds the card identities under them.

### 17.3. The (b) findings — priced, but the price is never read

#### 17.3.1 `unit_strength_credit` — the ten military unit cards

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

#### 17.3.2 `defense_bonus` — the three Military Bonus cards

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

#### 17.3.3 `free_civil_action` — the 18 action cards that grant one

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

### 17.4. Ranked discrepancy table

Worst 24 by 2p delta (per seat-game). Full table:
`python3 tools/play_rate.py report ...`.

| card (base) | type | 2p h | 2p bot | Δ | 3p h | 3p bot | 4p h | 4p bot | class |
|---|---|---|---|---|---|---|---|---|---|
| Urban Growth | action | 1.814 | 0.894 | −0.920 | 1.504 | 0.972 | 1.417 | 1.406 | (c) |
| Breakthrough | action | 1.357 | 0.463 | −0.894 | 1.023 | 0.972 | 0.903 | 0.844 | (c) |
| Military Bonus (def 4) | bonus | 0.920 | 0.106* | −0.814 | 0.679 | 0.000 | 0.831 | 0.000 | **(b)** §17.3.2 |
| Iron | mine | 0.786 | 0.013 | −0.774 | 0.539 | 0.000 | 0.719 | 0.000 | (c) §17.5.1 |
| Irrigation | farm | 0.816 | 0.081 | −0.735 | 0.459 | 0.056 | 0.452 | 0.125 | (c) §17.5.1 |
| Engineering Genius | action | 1.482 | 0.762 | −0.719 | 1.063 | 0.463 | 0.926 | 0.844 | (c) |
| Cannon | artillery | 0.705 | 0.013 | −0.693 | 0.434 | 0.000 | 0.516 | 0.062 | **(b)** §17.3.1 |
| Revolutionary Idea | action | 1.118 | 0.419 | −0.699 | 0.820 | 0.889 | 0.702 | 0.719 | (c) |
| Military Bonus (def 6) | bonus | 0.630 | 0.006* | −0.624 | 0.439 | 0.000 | 0.512 | 0.000 | **(b)** §17.3.2 |
| Air Forces | air | 0.653 | 0.031 | −0.622 | 0.321 | 0.009 | 0.555 | 0.031 | **(b)** §17.3.1 |
| Knights | cavalry | 0.640 | 0.025 | −0.615 | 0.409 | 0.000 | 0.566 | 0.031 | **(b)** §17.3.1 |
| Alchemy | lab | 0.601 | **0.000** | −0.601 | 0.479 | **0.000** | 0.602 | **0.000** | (c) §17.5.1 |
| Computers | lab | 0.619 | 0.025 | −0.594 | 0.436 | 0.000 | 0.462 | 0.000 | (c) §17.5.1 |
| Reserves | action | 2.048 | 1.456 | −0.592 | 1.717 | 1.574 | 1.519 | 1.500 | (c) |
| Military Bonus (def 2) | bonus | 0.588 | 0.075* | −0.513 | 0.581 | 0.000 | 0.735 | 0.000 | **(b)** §17.3.2 |
| Swordsmen | infantry | 0.609 | 0.044 | −0.565 | 0.253 | 0.009 | 0.352 | 0.000 | **(b)** §17.3.1 |
| Frugality | action | 0.848 | 0.350 | −0.498 | 0.732 | 0.806 | 0.757 | 1.031 | (c) |
| Scientific Method | lab | 0.397 | **0.000** | −0.397 | 0.351 | **0.000** | 0.344 | **0.000** | (c) §17.5.1 |
| Efficient Upgrade | action | 1.092 | 0.713 | −0.379 | 1.025 | 0.778 | 0.905 | 1.000 | (c) |
| Cavalrymen | cavalry | 0.389 | 0.019 | −0.371 | 0.283 | **0.000** | 0.253 | **0.000** | **(b)** §17.3.1 |
| Pyramids | wonder | 0.361 | 0.013 | −0.349 | 0.256 | **0.000** | 0.242 | **0.000** | (c) |
| Medieval Army | tactic | 0.375 | 0.031 | −0.344 | 0.216 | 0.019 | 0.403 | 0.062 | (c) §17.5.3 |
| Rich Land | action | 1.137 | 0.819 | −0.318 | 0.962 | 0.583 | 0.968 | 0.906 | (c) |
| Engineering | special-tech | 0.353 | 0.056 | −0.296 | 0.228 | 0.056 | 0.206 | 0.219 | (c) |

\* The three Military Bonus rows are diluted twice over: the `defend` move was
only captured on 80 of the 160 2p seat-games (§17.3.2), and the human number folds
in a colonization use the bot side cannot count at all.  On the 80 seat-games
that did carry it the bot plays a bonus card 0.375 times a seat-game against a
human **defence-only** rate of 0.397.  Read §17.3.2 before reading these three
rows as a behavioural gap.

#### The inverse: cards the bot plays far MORE than humans

| card (base) | type | 2p h | 2p bot | Δ | 3p h | 3p bot | 4p h | 4p bot | class |
|---|---|---|---|---|---|---|---|---|---|
| Printing Press | library | 0.175 | 0.863 | +0.688 | 0.291 | 0.509 | 0.200 | 0.031 | (c) §17.5.1 |
| Opera | theater | 0.214 | 0.887 | +0.674 | 0.258 | 0.648 | 0.253 | 0.000 | (c) §17.5.1 |
| Movies | theater | 0.317 | 0.881 | +0.564 | 0.386 | 0.657 | 0.344 | 0.000 | (c) §17.5.1 |
| Multimedia | library | 0.332 | 0.856 | +0.524 | 0.338 | 0.074 | 0.345 | 0.031 | (c) §17.5.1 |
| Patriotism | action | 0.746 | 1.200 | +0.454 | 0.439 | 1.157 | 0.566 | 0.969 | (c) |
| Organized Religion | temple | 0.390 | 0.819 | +0.429 | 0.271 | 0.009 | 0.315 | 0.000 | (c) §17.5.1 |
| War over Culture | war | 0.160 | 0.569 | +0.408 | 0.105 | 1.009 | 0.112 | 1.062 | (c) §17.5.2 |
| Drama | theater | 0.122 | 0.500 | +0.378 | 0.341 | 0.657 | 0.207 | 0.062 | (c) §17.5.1 |
| Aggression: Raid | aggression | 0.168 | 0.537 | +0.370 | 0.113 | 0.231 | 0.157 | 0.000 | (c) |
| Theocracy | government | 0.085 | 0.450 | +0.365 | 0.098 | 0.333 | 0.055 | 0.250 | (c) |
| Theology | temple | 0.115 | 0.438 | +0.323 | 0.188 | 0.046 | 0.226 | 0.031 | (c) §17.5.1 |
| Taj Mahal | wonder | 0.104 | 0.406 | +0.302 | 0.118 | 0.056 | 0.138 | 0.031 | (c) |
| Journalism | library | 0.195 | 0.469 | +0.274 | 0.316 | 0.074 | 0.327 | 0.000 | (c) §17.5.1 |
| Mahatma Gandhi | leader | 0.043 | 0.312 | +0.269 | 0.103 | 0.287 | 0.112 | 0.250 | (c) |
| Fighting Band | tactic | 0.313 | 0.475 | +0.162 | 0.486 | 0.870 | 0.421 | 0.844 | (c) §17.5.3 |
| Warfare | special-tech | 0.086 | 0.263 | +0.177 | 0.193 | 0.648 | 0.210 | 0.500 | (c) |

The over-plays cluster the same way the under-plays do: **blue urban buildings
and the Age III culture wonders at 2p, wars and the cheapest Age I tactic
everywhere.** `War over Culture` at 3.6× / 9.6× / 9.5× the human rate is the
single largest over-play in the game and is the card-level restatement of
[`docs/SYSTEM_COVERAGE.md`](SYSTEM_COVERAGE.md)'s "wars declared 2.2× / 6.6× / 7.9× over".

### 17.5. Cards the bot never plays

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

#### 17.5.1 Why yellow is dead and blue is doubled — class (c), and probably wrong

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
an inert weight, and [`docs/EXPERT_STRATEGY.md`](EXPERT_STRATEGY.md)'s framing says a civilization
that buys no science and no resources is not a strategy the corpus supports.

#### 17.5.2 Wars — class (c), already open

`War over Culture` at 0.57 / 1.01 / 1.06 against a human 0.16 / 0.11 / 0.11,
and `War over Technology` / `War over Territory` at zero on the bot side at
2p. The bot declares the one war it can evaluate and never the two it cannot.
[`docs/COMBAT_AUDIT.md`](COMBAT_AUDIT.md) and [`docs/SYSTEM_COVERAGE.md`](SYSTEM_COVERAGE.md#4-colonies--alive-everywhere-thin-at-2p) §4 own this;
nothing new is claimed here beyond the card identities.

#### 17.5.3 Tactics — class (c)

The bot plays 1.57 / 2.21 / 2.13 tactic cards a seat-game against a human 2.12
/ 1.95 / 2.32, so the *class* is healthy, but the mix is not: it over-plays
`Fighting Band` (the cheapest Age I tactic) 1.5× / 1.8× / 2.0× and under-plays
`Medieval Army` 12× / 11× / 6×, and never plays Phalanx, Fortifications,
Entrenchments, Napoleonic Army or Shock Troops at 3p. `tactic_level` is live
(0.033 / 0.070 / 0.148) and `tactic_gain` is 0.111 / 0.0 / 0.052, so this is a
weighting question, not a blind spot.

#### 17.5.4 What has no human baseline at all

* **Events (55 cards).** Nobody plays an event; it is prepared face down and
  revealed. The bot reveals 7.5 / 6.4 / 8.3 a seat-game. The journal names the
  revealed card but the choice being measured — which card to *prepare* — is
  face down in the corpus, so no rate exists to compare against.
* **Pacts (10 cards).** BGO prints `accepts pact offer` and never the card
  name. The bot's aggregate is 0.00 / 1.03 / 1.50 offers a seat-game, and the
  2p zero is the rulebook, not a gap. [`docs/COMBAT_AUDIT.md`](COMBAT_AUDIT.md) owns this.
* **Colonization use of a Military Bonus card.** `interact.force_value`
  assembles the sacrifice; it is not a bot decision, so there is nothing to
  count on the bot side (§17.3.2).

### 17.6. What is now permanent

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
   free_civil_action}` (§17.3). "Wrong-signed" is measured against the default's
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

### 17.7. Open

* The 4p column is 8 games. It agrees with 2p and 3p on every sign, but no 4p
  number here should be quoted as a magnitude.
* `free_civil_action` (§17.3.3) is a measured inert weight with no isolated
  behavioural signature. Whether it costs anything is unmeasured.
* The yellow-technology collapse (§17.5.1) is the largest non-inert discrepancy
  in this document and has no owner. The mechanism is identified
  (`culture_rate` 31.68 against `science_rate` 0.25 on the 2p champion, and a
  cost model that charges `techCost` at a higher weight than the output it
  buys); whether that is a pricing bug or a real 2p optimum is not settled
  here.
* The bot's colonization use of Military Bonus cards is unmeasurable through
  the bot's own move stream (§17.3.2). Instrumenting `interact.force_value` would
  close it.

---

## 18. 2026-08-02: the three §17.3 gates are no longer inert

§17.3 named three coordinates that were 0.0 on every trained vector on disk,
and `tests/test_play_rate.py::TestNoClassIsDeadOnEveryTrainedVector` pinned
that fact so it could not quietly stay true. It has now fired in the
direction the project wanted: on the live champions (2p gen 119, 3p gen 32,
4p gen 12) all three are off zero.

| coordinate | 2p g119 | 3p g32 | 4p g12 | was (2026-07-30) |
|---|---|---|---|---|
| `unit_strength_credit` | 0.15835 | 0.00000 | 0.00449 | 0.0 / 0.0 / −0.01713 |
| `defense_bonus`        | 0.00000 | 0.00000 | 0.07136 | 0.0 / 0.0 / absent |
| `free_civil_action`    | 0.12849 | 0.00078 | 0.04118 | 0.0 / −0.16007 / −0.08449 |

Nothing was repriced by hand. The only input was generations: the 2p arm went
54 → 119 and the 3p and 4p arms restarted and climbed. `unit_strength_credit`
is THE named historical case of this document — priced, shipped at 0.0, and
inert through four audits — and it is now the largest of the three. All three
signs are positive, which is the direction §17.3 argued was the correct one:
a card carrying the good thing should not look worse for carrying it.

`DEAD_ON_EVERY_TRAINED_VECTOR` is therefore empty, and
`KNOWN_DEAD`'s whole `inert-live` section is gone from
`tests/test_coordinate_registry.py` along with `build_discount`,
`colonize_bonus`, `event_scoring_margin`, `hand_mil_potential`,
`hand_swap_extra` and `unit_strength_credit`. Both lists only shrink.

What this does NOT establish is a play-rate change. These are weights, not
behaviour; §17.4's discrepancy table has not been re-measured against a
champion that carries them, and until it is, "the gate opened" is the whole
claim. §17.7's open items all stand.
