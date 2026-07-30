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

| card type | n | with a dropped key (master → now) | zero visible gain (master → now) |
|---|---|---|---|
| event | 55 | 55 → 55 | 55 → 55 |
| action | 33 | 28 → 10 | 19 → 6 |
| leader | 24 | 24 → 24 | 17 → 16 |
| wonder | 16 | 15 → 8 | **7 → 5** |
| tactic | 15 | 15 → 15 | 15 → 15 |
| special-tech | 12 | 6 → 0 | 3 → 0 |
| territory | 12 | 0 → 0 | 12 → 12 |
| aggression | 11 | 11 → 11 | 10 → 10 |
| pact | 10 | 10 → 10 | 10 → 10 |
| government | 8 | 0 → 0 | 4 → 4 |
| war | 3 | 3 → 3 | 3 → 3 |
| bonus | 3 | 3 → 3 | 3 → 3 |
| units (infantry/cavalry/artillery/air) | 10 | 1 → 1 | 10 → 10 |
| farm/mine/lab/temple/library/arena/theater | 24 | 0 → 0 | 0 → 0 |
| **TOTAL** | **236** | **171 → 140** | **168 → 149** |

The pattern is crisp and worth stating on its own: **the evaluator prices a
card correctly when the card is a bag of numbers, and not at all when its
value is written in prose.** Every farm, mine, lab, temple, library, arena and
theater — the 24 cards whose whole content is a production number — is priced
exactly right. Every tactic, every event, every territory is invisible.

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
  cards, six of them wonders and two of them leaders, and this experiment
  cannot attribute the 9.5pp among them.

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
