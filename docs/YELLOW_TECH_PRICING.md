# The bot builds one civilization and it is blue, because `card_potential` reads weights `evaluate` does not use

2026-07-30.  Closes the largest **non-inert** discrepancy in
`docs/PLAY_RATE_AUDIT.md` (§5.1, "why yellow is dead and blue is doubled") and
open item 1 of `docs/UNIT_TECH_PRICING.md` ("`tech_levels` is unpriced on every
technology card").  They are one problem and are fixed together.  Base game
(2015), all three player counts.

## 0. The finding, and the numbers it is

`docs/PLAY_RATE_AUDIT.md` measured, per seat-game, bot against human:

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

## 1. The mechanism, verified before anything was changed

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

Reproduces `PLAY_RATE_AUDIT.md` §5.1 to the digit.  The same table gives
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
`docs/UNIT_TECH_PRICING.md` §1c (1/437 → 20/437), and it is why this is a
repricing and not a clamp.

**(d) The cause is NOT the one the audit named — it is bigger.**
`PLAY_RATE_AUDIT.md` attributed the collapse to the ratio `culture_rate` 31.68
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

## 2. What changed

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
`docs/UNIT_TECH_PRICING.md`'s measured result is not silently re-opened.

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

### 2.5 One implementation, not two

* the production/strength delta is an `effects.compute` diff — the engine's own
  arithmetic;
* the resources are `actions.upgrade_cost`, the science `effects.tech_cost`;
* the `Stats`→feature table is shared with the swap diff, not duplicated;
* `feature_marginal` is checked numerically against `evaluate`;
* `unit_upgrade` is called, not re-derived.

No information is added: `tech_upgrade` reads the acting player's own tableau,
which is public.

## 3. What it did — before/after, `tools/play_rate.py`

Mirror table, `plan:width=2,det=1`, same seeds, same vector, the only
difference being `tech_board_credit` 0.0 → 1.0.  `tools/play_rate.py bot`
reuses `tools/system_census.py` unchanged, so the take counts and the
subsystem counts come out of one run.

**Which vector is which column matters.**  2p is the **live** champion (gen
72).  3p is **`DEFAULT_WEIGHTS`**, which is byte-identical to the live
`champion_3p.json` and `champion_4p.json` (both gen 0), and is therefore the
configuration the league will actually train — deliberately NOT the archived
pre-restart 3p vector `PLAY_RATE_AUDIT.md` censused, whose 0.00 laboratories
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
got more expensive to pass up.  `docs/UNIT_TECH_PRICING.md` §7.1's guess that
`tech_levels` was "the most likely single explanation for 23.5 civil cards vs a
human 34.3" is **not** supported: pricing it moved the mix and left the count
alone.

Wonders, wars, aggressions and colonies are flat at both counts.  The 3p wonder
column is 0.02 → 0.00 because `DEFAULT_WEIGHTS` already completes no wonders at
3p; that is `docs/OPEN_ITEMS.md` §1.1's hole, not this one.

## 4. Strength: a large win on the defaults, a severe regression on the live 2p
## champion, and the regression attributes to one stale weight

`experiments.evaluate`, the fix against **itself** — the identical vector with
`tech_board_credit` 1.0 against 0.0, so the two arms differ in exactly one
number and are paired on the deal.  Seat-balanced, `WeightedBot`.

| vector | games | win rate | paired CI | null | p | culture margin |
|---|---|---|---|---|---|---|
| 2p, **`DEFAULT_WEIGHTS`** | 300 | **70.50%** | ±5.03pp | 50% | 8.6e−16 | **+26.0** |
| 3p, **`DEFAULT_WEIGHTS`** | 240 | **41.67%** | ±6.93pp | 33.3% | 0.017 | **+14.3** |
| 2p, **live** champion (gen 72) | 300 | **12.17%** | ±3.46pp | 50% | 1.1e−103 | **−95.0** |
| 3p, archived champion (gen 1314) | 240 | *see below* | | 33.3% | | |

### 4.1 `DEFAULT_WEIGHTS` is the row that decides this operationally

70.5% at 2p and 41.7% at 3p, both against their nulls, both with a positive
culture margin.  **`experiments/league_state/champion_3p.json` and
`champion_4p.json` are gen 0 and byte-identical to `DEFAULT_WEIGHTS` today**,
so this is the configuration two of the three live arms start from and the one
every fresh arm will ever start from.  On it the change is a large,
unambiguous improvement, and the 2p number is the biggest single-change win
this project has measured in a while.

### 4.2 The live 2p champion: a severe regression, attributed

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
> instance of the mechanism `docs/UNIT_TECH_PRICING.md` §5.2 named, found by
> looking exactly where that section said to look.

### 4.3 What to do about it, stated plainly

* **`tech_board_credit` = 0.0 in `experiments/league_state/champion_2p.json`
  recovers the parent commit's pricing byte for byte**, needs no code change,
  and is the zero-risk option for the live 2p arm.
* The better option is to **re-fit the `tech_levels` group** on a vector that
  has to pay for it.  §4.2's second row is the evidence that this is not merely
  possible but likely to be a strengthening: the champion with a *default*
  `tech_levels` and the fix on beats the champion with a *trained*
  `tech_levels` and the fix off by 13pp.
* Nothing at 3p or 4p is in the regressing regime today, because both live
  champions are `DEFAULT_WEIGHTS`.
* The standing policy is that correct modelling is worth committing whether or
  not it strengthens the bot.  On that basis this lands: two large wins on the
  configuration that will be trained, one severe regression on one stale
  vector, attributed and reproducibly reversed by resetting one weight group.

### 4.4 The archived 3p champion, reported separately

`archive_prequiescent_20260730/ladder_3p/gen01314`, the vector
`docs/UNIT_TECH_PRICING.md` §5.2 already warns against warm-starting from:
**10.83% ± 4.18pp against a 33.3% null, culture margin −31.5**, n = 240.  It
carries `tech_levels` 5.04 — the same over-fitted coordinate the live 2p
champion has — so it is the same finding on a second stale vector and not an
independent one.  It is reported because the brief asked for it and NOT used
as evidence either way; nothing in the league is in that regime today.

## 5. Fingerprints

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
* **Two-sided** per `docs/PYPY.md` §9.0: derived from scratch in `/tmp/gateA`
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

## 6. Open, and deliberately not done here

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
