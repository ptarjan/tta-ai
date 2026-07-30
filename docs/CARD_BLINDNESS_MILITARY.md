# Territories, units and tactics: the 37 cards with no dropped keys (2026-07-29)

A follow-up to `docs/CARD_BLINDNESS.md`, which fixed the cards whose printed
value was being **dropped** by `_card_yields`. This one is about the 37 cards
the census reported as **zero visible gain with zero dropped keys** — 12
territories, 10 military units, 15 tactics. Those two numbers look like a
contradiction and the contradiction is the finding.

## One-paragraph answer

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

## 1. Why the census said "0 dropped, all blind"

| type | n | where the value actually is | seen by `_card_yields` before |
|---|---|---|---|
| territory | 12 | `immediateEffects` (one-shot) + `permanentEffects` (ongoing); `effects` is literally `{}` | nothing |
| infantry/cavalry/artillery/air | 10 | top-level `strength`, per worker | **only the costs** |
| tactic | 15 | `tacticBonus` × armies formable from the board | nothing |

### 1.1 Units were negative, not zero

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
differs between them is **which tactic they can fill**, which is §4.

### 1.2 The guardrail had the same blind spot

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

## 2. What was changed — and which parts are facts

Paul's rule: **a fact from the rules lands unconditionally; a modelling choice
with a free parameter needs evidence.** Sorting this lane that way:

| change | fact or choice |
|---|---|
| a unit's top-level `strength` is its per-worker yield | **fact** — `effects._tech_prog` already treats it as one |
| a territory yields its `immediateEffects` / `permanentEffects` | **fact** — `interact.gain_colony` applies exactly those |
| `_TERR_TO_FEATURE` agrees with the auction path's map | **fact** — same card, same blocks, same engine |
| a tactic's bonus is `tacticBonus` × armies formable | **fact** — `effects._army_value` |
| **how much** of a unit's strength to believe (`unit_strength_credit`) | **choice** — free parameter, defaulted 0.0, §3 |
| **how much** the military hand is worth (`hand_mil_potential`) | **choice** — free parameter, defaulted 0.0 |
| the shape of `tactic_short` and its sign | **choice** — free parameter, defaulted 0.0 |

Every fact is landed. Every choice is a weight at 0.0 for the league to
learn, and the A/B numbers below are reported as findings, not as a condition
of landing.



### 2.1 Units — the mapping is not a judgement call

`engine/effects.py:_tech_prog` puts a unit card's top-level `strength` into
the same per-worker programme slot it puts a farm's `production.food` into.
The engine already treats it as that unit's production. So `_card_yields`
reports it as the `strength` feature, and `tests/test_card_pricing.py` asserts
the agreement directly — the same argument, and the same drift guard, as
`culture` → `culture_rate`.

### 2.2 Territories — priced from the applied effect, not the card text

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

### 2.3 The hook: `hand_mil_potential`

None of that reaches the evaluator on its own, because `hand_potential` walks
`hand_civil` **only** — which is the real reason all 12 territories were
invisible and why mapping `tacticBonus` would have changed nothing.
`hand_mil_potential` is its military sibling. It is the piece the other
military card types need too: of the 94 military-deck cards it prices, 12
territories and 1 aggression are non-zero and the rest are 0, so it is
currently an almost pure territory probe and a hook for lanes C/D.

**It does not open an information leak**, and that is worth stating rather
than assuming, because reading a hand across a turn boundary is exactly the
`end_turn` leak of `docs/INFORMATION_AUDIT.md` §6. Two reasons, both already
established there: it reads `state.players[idx].hand_military`, my own hand,
never a rival's (§6 checks that every `hand_military` read in `weighted.py` is
`p = state.players[idx]`); and §6.2 measured that `hand_mil_value` "varies in
**0 of 1583** `end_turn` candidates and structurally cannot vary" — a 1-ply
trial does not reach my next military draw. `hand_mil_potential` is a
different function of *the same hand*, so it inherits that result exactly.

### 2.4 Tactics — the deadlock, and why one feature is not enough

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

## 3. Inert, and why that is the measured choice rather than the timid one

**Every fingerprint digest is byte-identical to master's.** All four new
weights that could change behaviour default to 0.0: `unit_strength_credit`,
`hand_mil_potential`, `tactic_gain`, `tactic_short`. `territory_credit` is
1.0 but is gated behind `hand_mil_potential`, so it costs nothing either.

`unit_strength_credit` is the interesting one, because the precedent in
`docs/CARD_BLINDNESS.md` §2.3 argued the opposite way for `card_rate_credit`
(1.0, live, "0.0 would leave the champions playing blind"). Two measurements
say units are not that case:

1. **At 1.0 it is a no-op for every trained vector.** `champion_2p` against
   itself with the credit flipped is 60 games **byte-identical** — same win
   rate, same cultures, mirrored seat by seat. Not "not significant":
   identical. The mechanism is §5.1 below.
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

## 4. What the bot actually does with military cards

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
it is the deadlock of §2.4 seen from outside. (The last two rows are §5.3's
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

## 5. Results

### 5.1 Units: a null, and the reason it cannot be otherwise

`unit_strength_credit` 1.0 vs 0.0, paired on the deal, was **60 games
byte-identical** (§3), so the A/B was stopped rather than extended: an
experiment cannot resolve an effect on games that are the same games.

The reason is the table in §4 and it generalizes past this fix. At credit 1.0
the mapping raises Swordsmen from −1.66 to −1.36 and Modern Infantry from
−4.00 to −3.25 (at the shipped 0.0 they stay at the §1.1 values, since the
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

This is the §5.1 finding of `docs/CARD_BLINDNESS.md` — "giving a card a weight
does not help until the bot takes the card" — with the causal chain filled in
rather than inferred from a variance census.

### 5.2 Which knobs can change a game at all — run this first

`docs/CARD_BLINDNESS.md` §5.1 spent 1200 games finding out that
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
the best tactic you can reach — and §4 says the champion **already holds a
tactic in every game** and has **zero units**, so the best reachable tactic
adds 0 and the one in play also adds 0. The term is 0 on every candidate
move. A feature designed to break a deadlock cannot break it from the side
the bot is already stuck on.

`tactic_short` fires, barely, and only at −1.0. It is the gradient toward
building the unit that completes an army, and the bot cannot build a unit it
has not developed, and has not developed one (§4). The deadlock is three
moves deep — take, develop, build — and `tactic_short` only rewards the last.

`hand_mil_potential` is the only arm that earns a full A/B, and its
monotonicity in the weight is the sanity check that the term is wired up.

### 5.3 Territories

**Behaviour first, because it frames the win rate.** Mirror table, 30 games,
`hand_mil_potential` 0.5 (the strongest arm in §5.2) against the base
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
predictable — §5.2 measured the term changing 0.93% of decisions, so an
effect that size was never on the table. It is a bound, not a discovery, and
the top of this section says why the bound is uninteresting: the decision it
informs happens 0.167 times a game.

**And it is the wrong test, which Lane C established after I had started it.**
`docs/CARD_BLINDNESS.md` §5.2: a fresh 0.0-default feature does nothing until
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

### 5.4 Tactics are a plumbing problem, not a pricing problem

Checked after the wonder lane showed that repricing 8 wonders moved wonder
completions by a measured zero, because a wonder never enters `hand_civil` and
so reaches the policy only through a take-timing heuristic. The same question
for tactics: **which term actually carries a tactic's value into the policy?**

> **Note on the premise (2026-07-30).** That "measured zero" was an arithmetic
> identity, not a result: the take-timing heuristic is gated on `row_urgency`,
> which is 0.0 in the frozen champion, so the wonder reprice could not have
> moved anything. See `docs/CARD_BLINDNESS.md` §5.3 and
> `analysis/frozen/README.md`. **This section's own finding does not depend on
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

It also explains §4's strangest number mechanically. `copy_tactic` costs 2
military actions but takes **no card out of hand**, so `hand_military` and
`hand_mil_value` are unchanged while `tactic_level` rises: a clean +0.27
against `ma_left` at 0.080. Playing a tactic *from hand* removes the card and
gives up 0.464 + 0.283. **The evaluator therefore prefers copying to playing,
and the champion copies 10.5 tactics a game.** It is paying two military
actions a time to avoid the bookkeeping penalty for having a smaller hand.

So the §5.2 null on `tactic_gain` should be read narrowly. `tactic_gain` and
`tactic_short` are state features evaluated on every candidate, so unlike card
pricing they are not blocked by the plumbing — their 0-of-967 is a genuine
measurement of the feature. But it is a measurement taken on a board where the
champion already holds a tactic and owns no units, so the best reachable
tactic and the one in play are both worth zero. The feature is not wrong; the
position it was built to detect never arises, because the plumbing above stops
the bot ever getting units.

## 6. What is still broken, and what would actually fix it

This lane closes the *pricing* gap for 22 of its 37 cards and leaves a precise
statement of why the other 15 are not a pricing problem at all.

* **Units need a board-conditional term, not a table entry.** §5.1: the only
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
* **`copy_tactic` is a live pathology and it is not a card-pricing bug.** §4:
  10-12 copies a game at 2 military actions each, into zero armies. It is
  trained-weight behaviour — `tactic_level` is +0.44 and `ma_left` is −0.04,
  so *spending* two military actions to raise the tactic level scores as a
  gain twice over. None of the features here touch it. Somebody should.
* **Governments have the unit bug.** Top-level `civilActions` /
  `militaryActions` that `_card_yields` never reads (§1.2). Written off, not
  fixed; another lane's card type.

## 7. Reproducing

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
