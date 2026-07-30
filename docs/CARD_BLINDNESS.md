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

> **CORRECTION, 2026-07-29 (docs/EVENT_SEEDING.md).** The table below
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
`--board` (the board-aware evaluator of `docs/CARD_PRICING_LEADERS.md`
counted too) the totals are **125 dropped / 129 zero-gain**.

**Military deck — `_card_yields` is never asked.** These rows are recorded for
completeness. They are *not* a measure of how well the bot values these cards,
because the tool cannot see where they are actually priced.

> **CORRECTION, 2026-07-30 (docs/MILITARY_SEAM.md).** "Never asked" is true of
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
| bonus | 3 | 3 | 3 | ~~genuinely unpriced~~ → `_BONUS_TO_FEATURE`: `defenseBonus−1` (the increment over the +1 any face-down card is worth) and `colonizationBonus` → `colonize_bonus`, both derived from `engine/interact.py`. docs/MILITARY_SEAM.md |
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
removes the penalty. That is the shape `docs/HEURISTICS.md` asks for — "start
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
   Gandhi's aggression immunity is here, and `docs/HEURISTICS.md` already
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
   docs/MILITARY_SEAM.md. `tacticBonus` stays unpriced, but for the sharper
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
good reason: `docs/SCORE_VALIDATION.md` §6.2 measured that *forcing* wonders
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
* **The champion was trained blind.** Its 78 weights were fitted while
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

`docs/STRENGTH_CHECK.md` is explicit that the tournament-derived 2p result did
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

> **CORRECTION (see `analysis/frozen/README.md`). The wonder null below is an
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
`docs/SCORE_VALIDATION.md` verified the rules exact, and §1 here has now given
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
credit0 arm is *also* being made worse by facing a stronger opponent —
`docs/LEAGUE_OBJECTIVE.md`'s point that a stolen point moves the margin twice
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
  `docs/SCORE_VALIDATION.md` §6.1: all 16 wonders and all 53 stages match the
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
treatment is applied. See `analysis/frozen/README.md`.

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
  units and tactics — are taken up in `docs/CARD_BLINDNESS_MILITARY.md`. Short
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
  and `docs/STRENGTH_CHECK.md`'s BookBot v2 result (+2.1%, p=0.098 at 2p,
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

1. Stop the arms with `experiments/logs/stop_league_{2,3,4}p.json` and wait for
   the generation boundary -- climbers do not poll mid-generation.
2. `git pull` only once no climber is running.
3. Back up `state_Np.json`, then delete its `last_full_check` key.
4. Remove the sentinels and let `watchdog.sh` relaunch, so the REQUIRED-flag
   assertion re-checks the arg list instead of you hand-typing it.
5. Confirm each arm logs `0 opponents measured` on its startup pool line and
   then advances a generation.

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

`docs/CARD_PRICING_LEADERS.md` §5.2 reports the leaders-only arm at
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
failure `docs/NEURAL_LOOP_NULL.md` documents at length.

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
