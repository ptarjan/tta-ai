# The 39 cards no lane had looked at

The 12 `special-tech` cards, the 24 production buildings (farm 4, mine 4, lab 4,
temple 3, library 3, arena 3, theater 3) and the 3 military `bonus` cards.

`docs/CARD_BLINDNESS.md` gave the 24 production buildings a clean bill of health
on the strength of a census that counts *dropped keys*: "the evaluator prices a
card correctly when the card is a bag of numbers ... every farm, mine, lab,
temple, library, arena and theater ... is priced exactly right." That census
cannot see three of the four things that can be wrong with a card, so this audit
re-asked all four questions from scratch:

1. **Is the DATA right** for the 2015 base game (not the expansion, not 2006)?
2. **Does the ENGINE implement the rule** the card prints?
3. **Does the value reach the POLICY**, through a term the search optimises?
4. **Does the bot actually take it**, measured, conditional on it being on offer?

Question 1 came back clean on all 39. Questions 2, 3 and 4 did not.

---

## 0. Summary

| type | n | data | engine | pricing | measured take rate | verdict |
|---|---|---|---|---|---|---|
| special-tech | 12 | clean | clean | **all 12 net negative** | **0.87%** | **BROKEN** |
| farm/mine | 8 | clean | clean | absolute-not-delta | 1.7 – 11.5% | healthy-with-caveats |
| lab/temple/library/arena/theater | 16 | clean | clean | absolute-not-delta | 0 – 60.6% | healthy |
| bonus | 3 | clean | **rule violation** | n/a by construction | no take decision exists | **BROKEN** (handed off) |

"Pricing" above is the *net* sign of `card_potential`, not whether the keys are
mapped. Every key on all 39 cards is mapped; the census that reports mapping was
right about mapping and that is why it missed all three defects.

Three real defects, all confirmed by measurement, none previously known:

* **D1** — the end-of-turn military hand-limit discard is FIFO with no decision,
  in a step `docs/RULES_SPEC.md:188` explicitly calls "the only step requiring a
  decision". It discards ~31–37 cards per 2p game, and on a third of the turns
  it fires it destroys the best defence card in hand when a worse one was
  available. *Handed to a dedicated lane; the
  diagnosis is in section 4 and the fix is not in this change.*
* **D2** — all 12 special technologies price at a strictly **negative** hand
  value, so the bot is actively repelled from a sixth of the civil deck. Six of
  the twelve are taken zero times in 40 player-games. The cause turns out to be
  a *sanctioned* deferral (every key is mapped; only the belief is deferred to
  0.0 weights) whose cost had simply never been measured. Sections 2.3–2.7.
* **D3** — `_card_yields` reduced `buildDiscount` by **summing** the per-age
  entries, which are mutually exclusive. It scaled the three Construction techs
  3 : 5 : 6 where the rules scale them 1 : 2 : 3 — the relative order wrong, not
  just the magnitude. *Fixed here, section 2.5, proven inert.*

A verdict of "BROKEN" below means broken **in effect**: measured behaviour that
no player would recognise as play. It does not always mean somebody wrote a bug;
D2's cause is a convention followed correctly, whose price nobody had put a
number on until now.

---

## 1. What was checked and found clean (so it is not re-checked)

**The card data.** All 24 production buildings and all 12 special techs were
diffed field-by-field against `sources/bga_throughtheages_material.inc.php`, the
BGA Studio implementation of the 2015 edition: `techcost`, `resscost`, every
`production` rating, `CA`, `MA`, `strength`, `colonize`, `tokendelta.blue` and
the per-player-count copy counts. **Every field matches on all 36.** The only
diffs are the four Age A cards' `count`, which is 0 in our data by convention
(printed on the player board, not a deck card) against BGA's one-per-player.

`sources/github_gmetola_cards.csv` disagrees on Warfare (tech 4 vs our 5) and
Military Theory (12 vs 11); that file is the **2006** edition and BGA 2015 says
5 and 11, which is what we carry. Our `text` fields already record the other
2006→2015 deltas (Printing Press build 4→3, Computers 10→11, Professional
Sports strength 4 is the *expansion* value and base 2015 is 3). This rules out
a whole class of explanation: nothing below is a data-entry bug.

**Six things that looked like defects and are not.** Written down because each
cost real time and the next auditor should not spend it again:

1. `colonizeBonus` (special techs) vs `colonizationBonus` (bonus cards) — two
   spellings for one concept, which is exactly the shape of the government
   `techCost`/`revolutionCost` bug. **Not a bug.** They are two different
   quantities summed in two different places and both land in the colonisation
   force: `interact.py:496` adds `Stats.colonize` (the permanent, from
   `FLAT_KEYS` at `effects.py:70`), `interact.py:497` adds the played bonus
   cards. Both verified read.
2. `wonderStagesPerAction` looked like it would be dead the way wonder pricing
   is dead. **It is live**: `actions.py:471` generates `("wonder_step", k)` for
   `k` up to `min(left, s.wonder_stages)`.
3. Special techs stacking. `RULES_SPEC` 7.6 says max one per type icon, and
   developing a same-icon tech removes the lower one *from the game*.
   Correctly implemented at `actions.py:823` `_develop_special`, with
   `special_icon` deriving the icon from the effect keys and partitioning the
   12 into exactly 4 groups of 3. The `blueTokens` bookkeeping across a
   replacement nets out correctly too (`on_leave_play` −3, `on_enter_play` +3 =
   the +3 the surviving card promises, not +6).
4. `buildDiscount` on upgrades. The discount applies to **both** sides of
   `upgrade_cost = build_cost(hi) − build_cost(lo)` (`actions.py:176`), so it
   partially cancels — which is exactly `RULES_SPEC` 7.4 / FAQ p.7's Masonry
   table ("treat modified values as if they had always been the price").
5. The urban building limit. Enforced on **build** (`actions.py:436`, per type,
   counting workers, against `s.urban_limit`) and deliberately not on
   **upgrade**, which is right (`RULES_SPEC` 7.5: an upgrade keeps the count
   constant). Farms and mines correctly carry no `urbanLimitCategory` and are
   unlimited.
6. Population and food costs, the other half of what a production building
   really costs. `economy.pop_cost_base`, `consumption` and `happy_required`
   were checked square-by-square against the yellow-bank table in
   `RULES_SPEC` 6.1. All three are exactly right, including the 0-tokens
   corner (consumption 6, happy required 8).

One genuinely dead data field: **`urbanLimitCategory`**, on all 15 urban
buildings, is read by nothing. It is harmless because its value is always
identical to `type` and the limit is enforced off `type`
(`cards.py:38 URBAN_TYPES`). Left in place as documentation; noted here so the
next census does not report it as a finding.

---

## 2. Special technologies — BROKEN, and the reason generalises

### 2.1 The measurement

`tools/uncovered_census.py`, 2p × 20 games, `analysis/frozen/champion_2p.json`.
The denominator is what makes this readable: **offers** counts decision points
at which `("take", slot)` for that card was a *legal move*, so a low take count
is only damning if the card was repeatedly on the table.

    special techs   14 takes / 1,606 offers  =  0.87%

Six of the twelve are taken **zero** times in 40 player-games:

| card | offers | takes |
|---|---|---|
| Architecture | 137 | 0 |
| Engineering | 132 | 0 |
| Cartography | 144 | 0 |
| Navigation | 132 | 0 |
| Satellites | 129 | 0 |
| Military Theory | 126 | 0 |
| Masonry | 145 | 1 |
| Civil Service | 134 | 1 (never developed) |
| Justice System | 121 | 2 |
| Warfare | 141 | 2 |
| Strategy | 136 | 3 |
| Code of Laws | 129 | 5 |

The best of the twelve is Code of Laws at 3.9%. For scale, section 3 measures
the production buildings in the same run at 5–60%.

### 2.2 The cause: every special tech prices NEGATIVE

`card_potential` under `analysis/frozen/champion_{2,3,4}p.json` — the same
vectors the census above was run with:

| card | 2p | 3p | 4p |
|---|---|---|---|
| Masonry | **−0.60** | 0.00 | 0.00 |
| Architecture | **−1.20** | 0.00 | 0.00 |
| Engineering | **−1.81** | 0.00 | 0.00 |
| Cartography | **−0.65** | +0.37 | +0.78 |
| Navigation | **−0.90** | +0.73 | +1.56 |
| Satellites | **−1.15** | +1.10 | +2.33 |
| Warfare | **−0.19** | +0.28 | +1.68 |
| Code of Laws | +0.16 | +1.18 | **−2.86** |
| Justice System | +0.41 | +2.11 | **−1.41** |
| Civil Service | +1.17 | +3.29 | **−4.27** |

Across the whole 236-card deck, **19 / 31 / 30** cards price below zero under
the 2p / 3p / 4p frozen champions respectively. The three zeroes in the 3p and
4p Construction rows are not health: those vectors carry a *negative* `science`
weight, so `_Y_COST`'s clamp zeroes the card's cost as well as its (already
zero) benefit, and the card resolves to "free and worthless" — the same double
failure the government cards had.

Under `DEFAULT_WEIGHTS` — the vector the fingerprint bots and every untrained
arm use — **all twelve are negative**, and so are all 11 military units and
five production buildings: 34 of 236 cards price below zero.

The mechanism is not that the value is missing. It is that **only one side of
the trade is priced**:

* Masonry's *cost* (3 science) goes through `science`, a fully trained weight.
* Masonry's *benefit* — `buildDiscount` and `wonderStagesPerAction` — goes
  through `build_discount` and `wonder_stages_per_action`, both defaulting to
  **0.0**.

So the card resolves to "pay 3 science, receive nothing". Same story for the
exploration line (`colonize_bonus` = 0.0) and, more weakly, for the civil and
military lines, whose gains *are* priced but not enough to clear the science
cost at every player count.

### 2.3 THE GENERAL RULE — read this before adding another 0.0-default weight

> **Adding a 0.0-default feature for one side of a trade whose other side is
> already priced does not leave the card neutral. It biases it, and the
> direction depends on which side you just made visible.**

"Inert" is a claim about the *weight vector* — a champion trained before the
change is numerically unchanged, which is true and worth having. It is **not**
a claim about the *card*. A card whose cost is priced at a trained weight and
whose benefit is priced at 0.0 is not un-modelled, it is **mis**-modelled, and
it is mis-modelled in a specific direction. A half-priced card is a live
behavioural change wearing an inert label.

This has now bitten twice in one night, in opposite directions:

* **here** — benefit at 0.0, cost trained ⇒ the card reads as pure cost and is
  never taken (special techs, 0.87%);
* **governments** — cost at 0.0, benefit trained ⇒ the card reads as too cheap.

The check is mechanical, and it no longer has to be remembered — it is
`tests/test_half_priced_cards.py`, which runs on every gate. See section 2.7.

The reason a negative matters more than it looks: `row_pressure` skips any card
whose `card_potential` is `<= 0`, so such a card is not merely unattractive in
hand, it is **invisible in the row**. The evaluator cannot want it and cannot
notice it being swept away. That is two channels, not one, and it is why a
half-priced card behaves so much worse than an unpriced one.

### 2.4 The fix I proposed, tested, and did NOT land

The obvious repair is to floor `card_potential` at zero. The argument is sound
as far as it goes: developing a technology is never mandatory (`RULES_SPEC` 7.2),
so a card in hand that is not worth developing is worth **zero**, not minus
something — you simply never play it. A negative `card_potential` models a
compulsion the rules do not contain, and it is the exact mirror of the guard
already present (`_Y_COST` clamps a negative *stock* weight so that paying a
cost can never read as a gain).

**It is still the wrong fix, and the reason is worth more than the fix would
have been: the negatives are load-bearing instrumentation.**

`tests/test_card_pricing.py::test_an_age_ii_cavalry_and_artillery_are_no_longer_the_same_card`
asserts that turning `unit_strength_credit` from 0.0 to 1.0 makes Modern
Infantry *more* valuable — that is how the military-card lane demonstrates its
own fix works. Under a floor, measured rather than assumed:

| card | credit 0.0 | credit 1.0 | floored 0.0 | floored 1.0 |
|---|---|---|---|---|
| Modern Infantry | −7.100 | −5.350 | 0.000 | 0.000 |
| Air Forces | −8.100 | −6.350 | 0.000 | 0.000 |
| Riflemen | −4.500 | −3.450 | 0.000 | 0.000 |

A clamp makes a genuine pricing improvement **unmeasurable**. It would hide
this audit's own subject matter rather than report it.

Three further reasons it is the wrong shape, one per caller of
`card_potential` — the enumeration is the answer, not the intuition:

1. **`hand_potential`** (own civil hand, scale 0.125) — the floor helps here and
   only here. But note what the un-floored version actually does: because
   playing a card *removes* it from the hand, a negative `card_potential` means
   the evaluator is **rewarded for developing a bad card** to clear the
   liability, and punished for holding it. Both are backwards. This is real,
   and it is an argument for pricing the gain, not for clamping the total.
2. **`row_urgency` / `row_bargain_forgone`** — already skips any card whose
   `card_potential` is `<= 0`. A floor is a **literal no-op** here: negative and
   zero are treated identically. So the floor would not restore row visibility
   to a single one of the 13 cards, which is half of what is wrong with them.
3. **`rival_hand_potential`** — asks "how dangerous is the most dangerous rival
   hand". A rival's unplayable card is not a threat, so a floor is defensible;
   but the weight is 0.0 by default, so it is also moot.

**The house pattern is to price the missing gain, not to clamp the total**, and
the military-card lane got there first: `engine/bots/weighted.py` now maps a
unit's `strength` and defers only *how much of it to believe* to
`unit_strength_credit`. Its note is the standard to meet — 1.0 is privileged
only when it is exactly what the engine does with the key, and where it is not,
the value becomes a weight the league has a gradient for rather than a constant
somebody picked.

By that standard the 12 special techs are already in the **sanctioned** state:
every key on all twelve is mapped, and only the belief is deferred. What this
audit adds is the measurement of what the deferral costs — 0.87%, six cards
never taken — and section 2.7's mechanical detector so the next deferral is a
visible event.

### 2.5 Fix B — `buildDiscount` reduces by MAX, not sum *(rule fact)*

`buildDiscount` is `{age: resources off}` and the ages are **mutually
exclusive**: a building has exactly one age, so Engineering's
`{"I":1,"II":2,"III":3}` takes at most **3** off any one urban building, never
6. `effects.build_cost:980` already does the right thing in the engine —
`cost -= bd.get(card["age"], 0)`, one lookup — so the summing in `_card_yields`
priced a payout the rules engine will never make.

It was not a uniform overstatement either, which is why it matters: it scaled
the three Construction techs **3 : 5 : 6** where the rules scale them
**1 : 2 : 3**. It had their relative order wrong, not just their size.

Labelled **rule fact**. It introduces no constant.
`tests/test_card_pricing.py::test_build_discount_is_the_best_single_build_not_the_sum`
pins all three.

### 2.6 What is deliberately NOT fixed

`build_discount`, `wonder_stages_per_action` and `colonize_bonus` keep their
**0.0 defaults**. Fixes A and B remove the *repulsion* — the three Construction
techs stop being worse than a blank card — but they do not make any special tech
*attractive*. That still requires the league to find the three weights, and
until it does, these cards will be taken rarely rather than never.

That is deliberate, and it is the convention: a value we do not know is exposed
as a coordinate for the league, not guessed at with a constant. The honest
statement of the remaining gap:

* `build_discount` has a natural unit — it is **resources**, the same unit
  `resource_stock` already prices `buildCost` in — so it is the one of the three
  a future change could convert rather than train. What is unknown is not the
  unit but the *count*: how many urban buildings the discount will still apply
  to. Measured here at **4.45 urban builds per player-game** (178 builds over 40
  player-games), which is the anchor if anyone wants to make that conversion.
* `colonize_bonus` has no such anchor. Colonisation force is not a unit any
  existing weight converts (`colonies: 2.0` counts colonies, not force). Leave
  it to the league.
* `wonder_stages_per_action` is denominated in civil actions saved, but only on
  a wonder programme the bot does not currently run at all (see
  `docs/CARD_BLINDNESS.md` on wonders never being completed). Converting it
  would be pricing a path nothing walks.

### 2.7 The rule, made mechanical — `tests/test_half_priced_cards.py`

Section 2.3's rule is only useful if it survives this document. It is now a
test. It computes, for every card, the gain contribution and the cost
contribution under `DEFAULT_WEIGHTS` separately (through `_sum_yields`, so it
cannot drift from what `card_potential` actually does) and reports every card
whose cost is priced and whose gain contributes **exactly zero**.

Today that set is **13 cards**, and it is written down rather than asserted
empty, on the same terms as `DELIBERATELY_UNPRICED`:

* the **10 military units**, waiting on `unit_strength_credit` — a deliberate,
  argued deferral;
* **Masonry, Architecture, Engineering**, waiting on `build_discount` and
  `wonder_stages_per_action`.

Adding a fourteenth is now a test failure that says, in the failure message,
*"you have not made a card inert, you have made the bot refuse to take it"*.
Each entry must name the 0.0 weight its value is waiting behind, and the test
fails if that weight stops being 0.0 — so an entry cannot rot into a lie. A
fourth case checks the set is a function of the weights and not a constant of
the code: flipping `unit_strength_credit` to 1.0 must leave exactly the three
Construction techs.

This is the deliverable that outlives the audit. The measurement says the
deferral is expensive; the test says the next one will at least be seen.

---

## 3. Production buildings — healthy, and here is what healthy looks like

**This is the first measured take-rate baseline for any card in this project.**
Recorded so that a future collapse is visible as a collapse. Same run as
section 2.1 (2p × 20 games, frozen 2p champion), take rate conditional on the
take being a legal move:

| card | type | offers | takes | rate |
|---|---|---|---|---|
| Drama | theater | 33 | 20 | **60.6%** |
| Journalism | library | 44 | 20 | 45.5% |
| Opera | theater | 69 | 31 | 44.9% |
| Printing Press | library | 98 | 38 | 38.8% |
| Theology | temple | 70 | 19 | 27.1% |
| Scientific Method | lab | 103 | 25 | 24.3% |
| Alchemy | lab | 121 | 28 | 23.1% |
| Multimedia | library | 98 | 21 | 21.4% |
| Organized Religion | temple | 141 | 23 | 16.3% |
| Irrigation | farm | 174 | 20 | 11.5% |
| Movies | theater | 143 | 15 | 10.5% |
| Bread and Circuses | arena | 124 | 11 | 8.9% |
| Computers | lab | 172 | 12 | 7.0% |
| Iron | mine | 226 | 15 | 6.6% |
| Team Sports | arena | 116 | 7 | 6.0% |
| Selective Breeding | farm | 109 | 6 | 5.5% |
| Coal | mine | 123 | 6 | 4.9% |
| Mechanized Agriculture | farm | 125 | 3 | 2.4% |
| Oil | mine | 115 | 2 | 1.7% |
| Professional Sports | arena | 127 | 0 | 0.0% |

The shape is plausible for the culture-heavy 2p champion: theaters and
libraries at the top, arenas at the bottom, and the Age III economy cards low
because the game ends before they pay back. **Professional Sports at 0/127 is
the one entry that should be watched** — it is the only production building that
never gets taken, and it shares the arena line's problem that `happy_margin` and
`strength` are both weights this vector holds cheaply.

Two structural caveats. Neither is a bug in the sense of a wrong number; both
are places where the hand term is a documented approximation, and both are
recorded because they bias the *comparison between types*, which is what
decides which card gets taken.

### 3.1 The upgrade path prices as an ABSOLUTE, not a delta

Asked directly by the brief: it is an absolute. `_card_yields` is
`lru_cache`d on the card **name** and has no board, so Selective Breeding always
prices as `+3 food_rate, −5 science, −6 resource_stock`. For a player who
already has Irrigation — which is nearly always the case by the time Selective
Breeding is on offer — the true marginal gain is `+1` food per worker moved and
the true cost is the delta `6 − 4 = 2` resources, not 6. The absolute
overstates both sides by roughly 3×, and the overstatement grows with age.

This is mitigated, not silent: the board-side features `best_farm`, `best_mine`,
`best_lab`, `best_temple`, `best_theater`, `best_library`, `best_arena` do move
by the delta when the upgrade actually happens, and `hand_potential` carries a
scale of only 0.125.

**The machine that fixes this now exists and does not yet cover these cards.**
`engine/bots/board_yields.py` prices a card by swapping it onto the real board
and diffing `effects.compute` — exactly the delta this section is asking for —
but `SWAP_TYPES` is `{leader, government, wonder}`, the three single-slot card
types where playing the card *replaces* what is there. A production technology
is not single-slot, so it is out of scope by construction.

It is nonetheless the same question: upgrading a worker from Irrigation to
Selective Breeding replaces one card's contribution with another's, which is a
swap in everything but the slot. Extending the swap diff to "the highest-level
card of this type that already has workers" is the natural next step and would
price all 24 of these cards by the engine's own arithmetic instead of by their
printed numbers. **Not fixed here — it belongs to whoever owns `board_yields` —
but scoped, and no longer waiting on a machine that had not been built.**

### 3.2 A production building's price omits the WORKER

`_card_yields` prices a production building as `techCost` (science) +
`buildCost` (resources). In the rules that is not what it costs. A building
needs a **worker**, and a worker costs an Increase Population action: one civil
action plus food equal to the yellow-bank price, which starts at 2 and climbs to
7 (`RULES_SPEC` 6.1, verified correct in `economy.pop_cost_base`).

This matters less as a level error than as a **between-type bias**: the 24
production buildings all need a worker, and the 12 special technologies need
**none** (`RULES_SPEC` 7.6: "no workers ever"). Pricing the worker at zero
therefore makes production buildings look systematically cheaper than special
techs, on top of the special techs' benefits being priced at 0.0. Two
independent biases pushing the same way, and the measurement in sections 2.1
and 3 is what they add up to: 0.87% against 20%+.

Genuinely not fixable in `_card_yields`, which cannot see the yellow bank.
**Recorded as the second thing the board-aware card evaluator should carry.**

---

## 4. Bonus cards — the take question does not exist, and the discard is broken

The three `Military Bonus` cards (defence 2/4/6, colonisation 1/2/3, six copies
each per age) are correct against `RULES_SPEC` 125 and the engine implements
both halves:

* **defence** — `interact.py:623` adds the printed `defenseBonus`; any other
  military card discarded gives +1; the count of cards played is capped at the
  defender's military action total. Matches the rule exactly, including the
  2015 change that neither side sacrifices units.
* **colonisation** — `interact.py:479 bonus_pool` / `:497 force_value` add every
  bonus card in hand to the force.

The census reports them as "dropped key, zero visible gain" and that is
**true but not the finding**: `hand_potential` walks `hand_civil` only, so
`_card_yields` is never called for a military card at all. The measured
`offers` for all three is **0, by construction** — bonus cards are never in the
civil row. There is no take decision to get wrong. They arrive by random draw.

Every decision that touches them is therefore one of three, and two are fine
(BookBot picks the highest `defenseBonus` when defending, `book.py:863`;
colonisation auto-uses the whole pool). The third is **D1**.

### D1 — the hand-limit discard makes no decision *(handed off, not fixed here)*

`engine/economy.py:112-116`:

```python
limit = s.military_actions + s.military_hand_limit
while len(p.hand_military) > limit:
    name = journal.touch(p.hand_military).pop(0)   # oldest card, no choice
    discard_military(state, name)
```

`docs/RULES_SPEC.md:188`, describing this exact step of the end-of-turn
sequence: *"Discard excess military cards — down to military action total (red
tokens), face down. **Only step requiring a decision.**"* The engine makes no
decision.

Measured over 20 2p games: **743 discards, 37.2 per game, across 395
player-turns that were over the limit.** The hand limit is `military_actions`
(2 under Despotism) and the draw is up to 3 per turn, so the military hand
overflows on most turns and *which cards to keep* is settled by `pop(0)`. 158 of
the discards were bonus cards. On **132 of the 395 over-limit turns (33%)** the
single highest-defence card in hand was inside the FIFO-doomed prefix while a
strictly worse card was available to pitch instead.

> **CORRECTION, and it is the most useful thing in this section.** The first
> version of this document said **129 discards per game**. That was an
> instrument bug, not a measurement. The probe wrapped `economy.end_of_turn`,
> and the searching bots apply and roll back thousands of speculative moves per
> decision — so it counted the *search* as well as the game, inflating the
> figure ~3.5×. `tools/uncovered_census.py` now counts at the moment
> `("end_turn",)` is **chosen** by the real bot, which fires exactly once per
> real player-turn. The take rates in sections 2.1 and 3 are unaffected and
> re-ran byte-identical (still 14 takes / 1,606 offers, still the same six
> cards never taken), because they were always counted at the decision.
>
> The lane that owns D1 re-measured the premise independently rather than
> inheriting it from me and got **30.7 per game**, and replicated the harm
> ratio at **20.7%** against my 19%. Both corrections are theirs and both are
> right; a ratio survives the inflation because it cancels, a rate does not.
> An audit whose whole subject is instruments that measured the wrong thing
> has no business hiding that its own instrument did too.

The correct machinery already exists and is simply never called:
`interact.py:386 _q_discard_military` / `push_choice("discard_military")`, and
BookBot already carries a preference function for that tag at `book.py:800`.
Because turning this into a real pending decision changes the move stream and
will move bot fingerprints, and because it touches every military card type
rather than these three, it was handed to a dedicated lane rather than landed
here.

---

## 5. The instrument

`tools/uncovered_census.py` — take/develop/build/upgrade counts per card with
**offers** as the denominator, plus the two counters that size D1.

    nice -n 19 python3 tools/uncovered_census.py --players 2 --games 20 \
        --spec analysis/frozen/champion_2p.json --json out.json

Provenance of every number in sections 2.1, 3 and 4: one run of that exact
command, on master `6968256` plus this change. Nothing in the pricing path for
these 39 cards moved between that commit and the rebase onto `7084a04` — the
Construction fix is 0.0-weighted and `card_board_credit` defaults to 0.0 — so
the rates stand, but a re-run is cheap and is the right first move if anything
downstream disagrees.

Note for whoever owns the full 236-card census: the number to reconcile against
is `takes / offers`, not takes per game. A card that is rarely in the row and a
card the bot refuses are the same number under a per-game count and opposite
findings under this one — that distinction is the entire content of section 2.
