# The wonder rank-deficiency: what landed, and what T1 says about it

Workspace `/private/tmp/cardfeat3`, branched from `origin/master` at `11aa39b`.
Implementation commit `5d5e64a`.

## Trap 1 -- widening the weight vector: no breakage
`eval::parse_weights` starts from `Weights::defaults()` and overwrites only the
names the file carries, so a file that predates a key keeps that key's default.
Adding keys whose default is 0.0 is therefore inert for every champion on disk;
`analysis/frozen/gauntlet/`'s members are 140-key files already exercising that
path against a 158-wide vector. Guarded by
`eval::tests::a_champion_file_saved_before_these_keys_existed_still_loads_with_them_at_zero`,
which checks all six frozen members and asserts each new key's DEFAULT is 0.0.

`civil_action_gap`'s default was deliberately NOT raised off 0.0, which is the
one place this batch departs from the proposal's wording ("un-gated from 0.0").
Raising a default is not inert: every 140-key gauntlet member and the anchor
(`Weights::defaults()`) inherit it. Measured, not argued -- seeding
`wonder_promise` at 4.0 instead of 0.0 turned nine tests red, including
`bots::plan::tests::pick_output_on_fixed_positions_matches_the_pre_policy_order_baseline`
("2p seed=1: move regressed from the pre-policy baseline"). The key is
reachable and priceable; what it is worth is the league's to find.

## Trap 2 -- constraints under MUTATION, not only at load
`bin/climb.rs`'s `mutate` runs `dominance_repair` on every mutant, and its
guards iterate `eval::NON_POSITIVE_GATES` / `NON_NEGATIVE_GATES`, so registering
a new key in those tables arms load-time AND mutation-time enforcement with no
second edit:

* `wonder_promise` -> `WONDER_VALUE_GATES` (non-negative) + `DOMINATES`
  (`wonder_potential >= wonder_promise`)
* `wonder_age_overrun` -> `WONDER_DEBT_GATES` (non-positive)
* `hand_perishable` -> `PERISHABLE_GATES` (non-positive), a new list
* `take_cost_share` -> ungated, matching its numerator `take_cost_paid`

`DOMINATES` had no mutation-time test at all; added
`climb::tests::no_mutant_ever_walks_a_dominated_weight_above_the_one_that_dominates_it`,
table-driven so a future pair is armed with no new test. `dominance_repair` now
runs the sign gates BEFORE the ordering repair -- an ordering copies one weight
onto another, so running it first propagated an illegally-signed operand and
reported the wrong rule for it.

## T1, the census sweep

`behavcensus --games 200 --players 2 --threads 2` per point, all on top of the
unmodified live 2p champion `champion_2p_now.json` (gen 39258). 87 runs total.

### The (0,0) regression check passes exactly
The (0,0) grid point is **byte-identical** to a `behavcensus` built at the
parent commit `11aa39b` -- diffed whole-output, not just the wonder block. The
wiring is inert at default.

It does NOT reproduce `census_now.md` (0.12 / 1.79 / 66.5%), and that is not
this batch: `census_now.md` was taken before commits `3aec933`, `66881c6`,
`e7b7527`, `11aa39b` landed. The honest on-master baseline is
**0.23 completed / 1.70 abandoned / 59.2% antiquated**, and both are reported
below.

### Grid 1 -- the proposal's 6x6 (`wonder_promise` x `wonder_age_overrun`)
`wonder_promise` in {0, 0.25, 0.5, 1, 2, 4}; `wonder_age_overrun` in
{0, -0.25, -0.5, -1, -2, -4} (negative because `WONDER_DEBT_GATES` repairs a
positive value to 0.0). Note the coupling: `DOMINATES` raises
`wonder_potential` to equal `wonder_promise` at every nonzero point.

| promise | best completed | best abandoned | best antiq% |
|---|---|---|---|
| 0 | 0.23 | 1.60 | 59.2 |
| 0.25 | 0.35 | 1.40 | 53.5 |
| 0.5 | **0.45** | 1.35 | 47.8 |
| 1.0 | 0.06 | 1.67 | 70.8 |
| 2.0 | 0.04 | 1.67 | 69.5 |
| 4.0 | 0.05 | 1.63 | 67.6 |

A cliff, not a slope: everything collapses somewhere between promise 0.6
(completed 0.47) and 0.7 (0.29), and final score falls with it (47.9 -> 45.5),
so the collapse is the bot playing worse, not a different trade.

### Grid 2 -- payoff priced strictly above the promise
The proposal's condition is strict (`w[wonder_promise] < w[wonder_potential]`),
and grid 1 only ever reached equality. Sweeping `wonder_potential` explicitly:

| promise | payoff | age_ovr | completed | abandoned | antiq% | score |
|---|---|---|---|---|---|---|
| 0.15 | 0.45 | 0 | 0.60 | 1.25 | **39.9** | 48.0 |
| 0.15 | 0.45 | -60 | 0.61 | 1.03 | 44.3 | 49.1 |
| 0.20 | 0.60 | 0 | 0.58 | 1.25 | 40.7 | 47.2 |
| 0.25 | 0.75 | 0 | 0.60 | 1.24 | 40.6 | 47.0 |
| 0.25 | 0.75 | -60 | **0.63** | **1.03** | 44.1 | 48.5 |
| 0.25 | 1.25 | 0 | 0.10 | 1.70 | 67.9 | 43.5 |
| 0.50 | 1.50 | 0 | 0.07 | 1.75 | 68.1 | 43.8 |

The same cliff, at `wonder_potential` ~ 1.0 rather than at the promise.

### Verdict against the three hard pass numbers

| metric | baseline (census_now / on-master) | pass bar | best observed | verdict |
|---|---|---|---|---|
| completed / player-game | 0.12 / 0.23 | >= 0.50 | **0.63** | PASS |
| started-then-abandoned | 1.79 / 1.70 | <= 1.00 | **1.03** | FAIL (just) |
| lost to antiquation | 66.5% / 59.2% | <= 40% | **39.9%** | PASS, but not at the same point |

**T1 FAILS on the conjunction: no single grid point clears all three.** Against
the proposal's own FAIL criteria ("< 0.30 at every grid point", "> 1.4
everywhere", "> 55% everywhere") none of the three fail conditions is met --
every metric moves decisively in the designed direction and two of three cross
their bar somewhere.

Two structural notes on the abandoned bar. `abandoned = started - completed` to
within rounding in every run, so at a start rate of ~1.85 the <= 1.00 bar
implicitly demands completed >= 0.85, well above its own 0.50 bar. The only
lever that lowers STARTS is `wonder_age_overrun` (refusing a wonder that cannot
beat its boundary), and it does exactly that -- 1.85 -> 1.64 starts going from 0
to -60 -- but it buys the abandoned number by not starting rather than by
finishing, and it costs antiquation share (a smaller denominator of mostly the
same doomed wonders). 1.03 at (0.25, 0.75, -60) is the closest any point gets.

Final score rises at every good point (46.6 at (0,0) -> 49.1-49.6), which is
weak but positive evidence for T2; it is not a win-rate measurement and should
not be read as one.
