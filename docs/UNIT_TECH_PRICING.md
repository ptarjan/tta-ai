# The bot never upgrades its army, and the reason is a table that cannot say "it depends"

2026-07-30.  Closes the top-ranked hole in [`docs/SYSTEM_COVERAGE.md`](SYSTEM_COVERAGE.md) ("What the
bot never does", #1).  Base game (2015), all three player counts.

## 0. The finding, and the number it is

[`docs/SYSTEM_COVERAGE.md`](SYSTEM_COVERAGE.md#5-technology-by-colour--the-biggest-structural-hole-in-the-whole-census) §5 measured unit-technology takes per seat-game at

| | 2p | 3p | 4p |
|---|---|---|---|
| bot | **0.15** | **0.06** | **0.45** |
| human (BGO corpus) | 3.84 | 2.79 | 3.43 |

— 8× to 47× under.  The bot fights the whole game with its Age A Warriors, and
[`docs/CARD_BLINDNESS_MILITARY.md`](CARD_BLINDNESS_MILITARY.md#4-what-the-bot-actually-does-with-military-cards) §4 measured the downstream consequence:
across 30 games there were **five unit workers standing on the whole table**,
while ~29 military actions a game went into playing and copying tactics that
could form no army at all.

## 1. The mechanism, verified before anything was changed

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

## 2. Why turning the existing credit up cannot work

`unit_strength_credit` multiplies the printed strength.  On the live 2p
champion that buys 0.39 eval points per unit of credit against a cost of 6.51,
so **the sign flips somewhere past 16** — and every step between 0 and 16
changes no argmax at all.  `hillclimb.mutate` perturbs by `gauss(0, s) *
(abs(w) + 0.15)`; from 0.0 that is a flat plateau sixteen units long walked in
steps of ~0.15.  [`docs/CARD_BLINDNESS_MILITARY.md`](CARD_BLINDNESS_MILITARY.md#52-which-knobs-can-change-a-game-at-all--run-this-first) §5.2 measured the plateau
directly: **0 argmax divergences in 967 decisions at credit 1.0, one at 3.0.**

So this needed reshaping, not retuning.  `tests/test_unit_pricing.py:
TestTheDefect` pins both halves of that argument.

## 3. What changed

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

[`docs/CARD_BLINDNESS_MILITARY.md`](CARD_BLINDNESS_MILITARY.md#51-units-a-null-and-the-reason-it-cannot-be-otherwise) §5.1 named this — "the board expresses one
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

### 3.4 The seam the brief warned about does not block this

`hand_mil_potential` calling `card_potential` without a state was real and is
**already fixed** ([`docs/COMBAT_AUDIT.md`](COMBAT_AUDIT.md#11-hand_mil_potential-never-passed-the-board) §1.1 — the arguments are forwarded).
It would not have blocked this fix in any case: **unit technologies are CIVIL
cards.**  They arrive in the civil row and go to `hand_civil`, so they reach
`card_potential` through `row_pressure`, `hand_potential` and
`rival_hand_potential`, every one of which has always passed the state.
`hand_mil_potential` never sees a unit card at all.

### 3.5 One implementation, not two

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

## 4. What it did — before/after, `tools/system_census.py`

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
measurement (both seats play the same policy), which is what §5 is for, but it
is a warning that at 3p the army is being bought out of the culture budget.

Yellow (farms/mines) falls at both counts.  It was already near zero and this
did not touch it: that hole is [`docs/UNCOVERED_TYPES.md`](UNCOVERED_TYPES.md#0-summary) §0's absolute-not-
delta pricing and is a different lane.

## 5. Strength: two nulls and one severe regression, and the regression is the
## most interesting number in this document

`experiments.evaluate`, the fix against **itself** — the identical vector with
`unit_tech_credit` 1.0 against 0.0, so the two arms differ in exactly one number
and are paired on the deal.  Seat-balanced.

| vector | games | deals | win rate | paired CI | null | p | culture margin |
|---|---|---|---|---|---|---|---|
| 2p, **live** champion (gen 72) | 300 | 150 | 49.83% | ±3.92pp | 50% | 0.93 | −1.3 |
| 3p, **archived** champion (gen 1314) | 240 | 80 | **14.58%** | ±4.84pp | 33.3% | **1.3e−14** | **−37.8** |
| 3p, `DEFAULT_WEIGHTS` | 180 | 60 | 34.72% | ±5.03pp | 33.3% | 0.58 | +4.0 |

### 5.1 2p: a real null, not an underpowered one

`rho_deal` = −0.52 and a design effect of 0.48 — pairing halved the variance —
and ±3.9pp over 300 games would have found a 3.5-point effect.  There is not
one.  Culture margin −1.3 on a mean of 185, so the −3.2% the mirror census
showed is **symmetric**: both arms lose it.  That is exactly the distinction a
mirror census cannot make and a duel can.

### 5.2 3p on the archived champion: a large, unambiguous regression

**14.6% against a 33.3% null is the worst A/B result this lane has produced and
it is not noise.**  It has to be read together with what that vector believes:

    strength            3.4191        resource_stock   2.7188
    strength_rel_early  7.3498        science          0.1897
    strength_lead       0.4682        culture_rate     9.7921

`strength_marginal` on that vector is up to **11 eval points per point of
army**, against 9.79 for a whole point of culture *per turn*.  It thinks one
soldier is worth about one culture rate.  Handed that opinion, the fix buys
**4.16 unit technologies a seat-game** (§4) and the culture collapses 134 → 97.

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

### 5.3 3p on `DEFAULT_WEIGHTS`: null, and this is the row that matters
operationally

34.7% ± 5.0pp against 33.3%, p = 0.58, margin **+4.0 culture** — if anything
mildly positive.  `DEFAULT_WEIGHTS` carries `strength` 0.35 and
`strength_rel_early` −0.1, so `strength_marginal` is ~0.9 there rather than
~11, and the fix buys army at a sane rate.

**This is the vector the live 3p arm actually starts from.**
`experiments/league_state/champion_3p.json` is gen 0 and byte-identical to
`DEFAULT_WEIGHTS` ([`SYSTEM_COVERAGE.md`](SYSTEM_COVERAGE.md)'s method note), as is `champion_4p`.
So nothing in the league today is in the regime of §5.2, and the arm that is
live and trained — 2p — is the clean null of §5.1.

### 5.4 What to do about it, stated plainly

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
  and §7 keeps the 3p question open.

## 6. Fingerprints

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
* **Two-sided** per [`docs/PYPY.md`](PYPY.md#90-a-trap-found-before-any-code-was-written-the-fingerprint-files-are-stale) §9.0: derived from scratch in two separate
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

## 7. Open, and deliberately not done here

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
3. **The 3p regression on the archived champion (§5.2) is not closed, it is
   only attributed.**  The attribution is strong — the same A/B on
   `DEFAULT_WEIGHTS` is a null in the other direction — but nobody has yet
   re-fitted `strength` / `strength_rel_early` on a vector that has to pay for
   them, and until somebody does, "the weight was stale" is an inference from
   two rows rather than a demonstration.  The cheap version is to take the
   archived 3p champion, scale its military group down, and re-run §5.2.
4. **Every other feature that was never paid for is suspect for the same
   reason.**  The mechanism in §5.2 is general: a coordinate the evaluator can
   read but never has to buy is unconstrained, and it will drift.  `strength`
   was one because no card priced it.  Worth a sweep.
4. **Tactics remain confounded with this.**  [`SYSTEM_COVERAGE.md`](SYSTEM_COVERAGE.md#9-the-one-off-systems) §9 asked for
   tactics to be re-measured after the unit hole, not in parallel with it.
   Tactics played moved 1.05 → 0.88 at 2p and 0.85 → 1.02 at 3p; that is now
   measurable and was not before.

## 8. A Warriors worker cannot become a Cannon (2026-07-31)

Closes `docs/OPEN_ITEMS.md` §2 item 20, opened by `docs/YELLOW_TECH_PRICING.md`
§6.1 and deliberately left alone there so that lane's digest moves had one
cause.

### 8.1 The defect, verified against the engine before anything changed

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

### 8.2 What changed

`unit_upgrade` now calls `_upgradable_onto` and `_with_tech` — **the same two
helpers `tech_upgrade`'s non-red half has used since it landed**, moved up the
file rather than copied — so both halves of the module mean the same thing by
"upgrade".  `_unit_workers` and `_with_unit`, the two functions that expressed
the pooled version, are deleted.  There is no new weight and no new constant:
this is a legality rule the price was contradicting.

### 8.3 What it did — take rates, 2p, `default` (WeightedBot on
### `DEFAULT_WEIGHTS`), 20 games / 40 seat-games, same seeds

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

### 8.4 The invisibility check, with numbers

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

### 8.5 Fingerprints

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

### 8.6 The ratchet moved, and that is a warning as much as a result

`tests/test_coordinate_registry.py` landed the day before this change with a
`KNOWN_DEAD` list that can only shrink.  Re-pricing the red cards re-rolled the
six deterministic corpus games and **seven entries stopped being dead**:
`best_arena` (the bot now builds one) and the six `discontent` / `uprising` /
`best_arena` encoding slices.  They are deleted, per that file's rule.

The lesson is in `docs/OPEN_ITEMS.md` §9.5: those entries are pinned to six
games, and *any* pricing change re-rolls them.  `best_arena` went 0 → 314
non-zero states of ~2000 on this change alone.
