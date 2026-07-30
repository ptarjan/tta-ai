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

| card type | n | dropped key | "zero visible gain" | where it is really priced |
|---|---|---|---|---|
| event | 55 | 55 | 55 | Age III scoring events: `weighted.event_scoring_margin` → `events.final_event_culture`. The other 40 deliberately unpriced, reasons in EVENT_SEEDING §6 |
| tactic | 15 | 15 | 15 | **genuinely unpriced** — needs a military sibling to `hand_potential`, not a table entry |
| territory | 12 | 0 | 12 | `deferred_credit`'s auction branch |
| aggression | 11 | 11 | 10 | quiescence: the defender's `defense` pending is drained and the quiet position scored |
| pact | 10 | 10 | 10 | `deferred_credit`; and `count 2p: 0`, so absent from 2p entirely |
| bonus | 3 | 3 | 3 | **genuinely unpriced** |
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
| win rate | 38.0% | 40.3% | 38.2% | **38.83% ± 3.18pp** (z = 3.4) |
| own culture vs rival | 170 / 162 | 177 / 167 | 172 / 165 | **172.9 / 164.8** |

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

**That is why repricing Eiffel Tower by +23.50 moved nothing and repricing
Joan of Arc by +5.88 moved the policy 5.9x.** The size of the reprice is
irrelevant when the term it lands in is not one the search can act on.

This reframes the question this document started from. "The bot does not build
wonders — are wonders modelled wrong?" The answer is no: §6.1 of
`docs/SCORE_VALIDATION.md` verified the rules exact, and §1 here has now given
the evaluator the printed numbers. The bot still does not build them, because
**a correctly priced wonder has almost no path into the decision.** It is a
plumbing bug, not a pricing bug, and fixing it means giving wonder-in-progress
its own evaluator term — something the search can actually optimise — not
another row in a lookup table.

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
