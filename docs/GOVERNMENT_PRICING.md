# A government has two prices and the evaluator paid neither

2026-07-31.  Closes [`docs/OPEN_ITEMS.md`](OPEN_ITEMS.md) §2 item 22 ("a government's level is
unpriced on both sides") and §9.1's open question about which coordinate a
revolution's civil actions come out of.  Base game (2015).

The shape is the one [`docs/YELLOW_TECH_PRICING.md`](YELLOW_TECH_PRICING.md) and
[`docs/ACTION_CARD_PRICING.md`](ACTION_CARD_PRICING.md) already used twice: **a card is worth what
`evaluate` pays for what it does**, and the static table kept pricing cards
through coordinates `evaluate` does not use — or, here, through nothing at
all.

## 1. The defect, verified before anything changed

**Three of the eight governments price at exactly 0.000, and none of the eight
is charged a single point of science.**  `weighted._card_yields` reads
`techCost`, `production` and `effects`.  On a government card:

* `techCost` is `null` on all eight — a government prints **two** science
  costs, `peacefulCost` and `revolutionCost`, and the table reads neither;
* `civilActions`, `militaryActions` and `urbanBuildingLimit` are **top-level**
  fields that only `effects.compute` reads, so they are not in `production` or
  `effects` either.

Monarchy, Republic, Constitutional Monarchy and Despotism therefore have no
gain and no cost at all; Democracy and Theocracy price only their printed
culture.  `tests/test_government_pricing.py:TestTheDefect` pins both halves so
the tests below cannot pass by the defect having quietly moved.

**And the level was missing on both paths.**  `features()` reads a
government's age level twice — into `tech_levels`, alongside every other
technology in the game, and again on its own as `gov_level` (2.0 in
`DEFAULT_WEIGHTS`) — and neither `_card_yields` nor the `board_yields` swap
diff emitted either.  That is exactly the term `YELLOW_TECH_PRICING` added to
every *other* technology and skipped here.

## 2. What changed

**2.1 `board_yields._government_level`** — the `tech_levels` / `gov_level`
delta, emitted by BOTH paths (the swap diff reached through
`card_board_credit`, and the live path below), from one function so they
cannot drift.  It is a **difference**, and that is the rules rather than a
choice: RULES_SPEC 8.1, "new government always replaces the old regardless of
level", so `features()` stops counting the old one.

**2.2 `board_yields.government_plans`** — `(gains, routes)`.  The gains are
the `effects.compute` swap diff plus the two level terms.  Each route is what
one **legal** way of getting there spends:

| route | RULES_SPEC | science | actions |
|---|---|---|---|
| peaceful change | 8.2 | `peacefulCost`, net of `tech_discount` (`effects.tech_cost`) | 1 civil action, and the new government's extra actions are available the same turn |
| revolution | 8.3 | `revolutionCost`, raw (the engine applies no discount) | **every civil action you have**, and the new government's extra actions are spent immediately without effect |

The revolution route is offered **only when `actions._can_revolt` says so** —
the engine's own predicate, called and not restated — because RULES_SPEC 8.3.1
requires the whole civil-action pool to be available.  When it is not, the
peaceful route is the price, which is the conservative direction.  Nothing
here fits a rate for "how often a revolution is worth it": which route is
cheaper is read off the board, per card, every time.  Same pattern as
`ef2024e` (Ocean Liners: price both branches, gate on the board).

**One known optimism, stated rather than left to be found.**  The gate reads
the pool as it is NOW.  For a card in the *row*, taking it costs a civil
action, so by RULES_SPEC 8.4 you cannot revolt to it this turn even though
`_can_revolt` is true before the take — the revolution route is real but one
turn later than the price implies.  Correcting it would mean pricing "the pool
I will have next turn", which is a different and larger claim than "what the
engine offers on this board", and the error is one turn of delay rather than a
route that does not exist.

**2.3 `weighted.gov_value`** — prices the gains at `feature_marginal` (not the
bare `w[k]`: `tech_levels` and `culture_rate` are `PHASE_KEYS` features, the
factor-of-twenty-one error `YELLOW_TECH_PRICING` §1d found), takes the
**cheaper route** — a `max` over negative cost triples — and clamps every cost
through `max(0, w)` so a negative stock weight can never turn a cost into a
gain.

**2.4 The new weight is `gov_board_credit`, default 1.0**, and 0.0 recovers
the parent commit's pricing byte for byte on all 236 cards, which is what
makes this A/B-able against itself in one process.  It is absent from every
champion file, so `load_weights` fills it from `DEFAULT_WEIGHTS` and the
change is live on all three league arms at once.

## 3. Which coordinate a revolution burns — the derivation `OPEN_ITEMS` §9.1
## asked for

That item recorded `gov_action_cost` as a dead coordinate (`board_yields`
prices the burnt pool through it; `features()` does not emit it) and
deliberately did **not** fix it, because it was not obvious whether the right
replacement is `civil_actions` (the per-turn allotment, weight 2.0) or
`ca_left` (the actions remaining, weight 0.05) — a 40x difference.

**The rules settle it, and the two candidates are not even different
numbers.**  RULES_SPEC 8.3.1 requires *all* civil actions to be available
before a revolution is legal, so at the only moment the move exists the
remainder **equals** the allotment.  What differs is which coordinate
`evaluate` watches move:

* `actions._h_revolution` sets `p.civil_actions = 0`, and `features()` emits
  `p.civil_actions` as **`ca_left`**.  That is the loss.
* `civil_actions` is `s.civil_actions`, the allotment, and it does not fall —
  it **rises**, by the new government's own total.  It is the *gain* side, and
  §2.2's swap diff already prices it.  Charging the burn there would have
  charged the gain twice with the wrong sign.

So the burn is `ca_left`, and `tests/test_government_pricing.py:
test_the_burn_is_the_ca_left_the_engine_actually_destroys` checks it the only
way worth checking: it **applies** the revolution and requires the priced
amount to equal the `ca_left` `features()` actually loses, and requires
`civil_actions` to have gone up in the same step.

`gov_action_cost` itself is left exactly where it is, on the legacy
`card_board_credit` path and in `KNOWN_DEAD`, for the same reason `5ab4943`
left `free_civil_action` on the static path: the live path stopped using it,
the opt-out still needs it, and deleting a weight that champion files carry
would fail `VectorFilesMatchTheRegistry`.

### 3.1 The RULES_SPEC 8.3.5 exceptions, priced or written off

* **Isaac Newton** — regains 1 civil action after a revolution, so the burn is
  one smaller.  Priced (`_government_routes`), because `_h_revolution` does it.
* **Maximilien Robespierre** — pays with the **military** pool instead, and
  takes 3 culture.  Priced on `ma_left` and `culture`, again because the
  handler does exactly that.
* **Breakthrough** — orders a revolution out of its own free civil action.
  **Not priced separately, and it does not need to be**: `apply_free_action`
  runs the SAME `_h_revolution`, which empties the pool anyway, so the price
  here is already the engine's answer for that route.  If that handler is ever
  changed to spare the pool, this function inherits the fix.
* **Development of Civilization** — **out of scope with a reason**: the card
  is not in the base-game database at all (it is a Code of Laws event, and
  this repo is locked to the 2015 base game), so there is nothing to price.

## 4. What it did — and it is the opposite of what the register guessed

`tools/play_rate.py bot --players 2 --games 20 --spec default` (WeightedBot on
`DEFAULT_WEIGHTS`, 40 seat-games), before = the parent commit with the red fix
already in, after = this one.  Human column is the 692-game 2p BGO corpus.
Descriptive: n = 40 seat-games is below [`docs/HAZARDS.md`](HAZARDS.md) §1's n>=200 bar and
none of this is a strength claim.

| takes per seat-game | human 2p | before | after |
|---|---|---|---|
| **Monarchy** | 0.145 | **0.000** | **0.250** |
| **Constitutional Monarchy** | 0.413 | **0.000** | **0.125** |
| **Republic** | 0.280 | **0.000** | **0.125** |
| Theocracy | 0.085 | 0.475 | 0.475 |
| Democracy | 0.296 | 0.500 | 0.400 |
| Fundamentalism | 0.079 | 0.025 | 0.250 |
| Communism | 0.074 | 0.050 | 0.000 |
| **all governments** | **1.370** | **1.050** | **1.625** |
| government changes | 1.108 | 0.975 | 1.500 |
| — peaceful (RULES_SPEC 8.2) | — | 0.350 | 0.500 |
| — revolution (RULES_SPEC 8.3) | — | 0.625 | 1.000 |

**The three cards that priced at exactly 0.000 were taken exactly zero times,
and are now taken.**  That is the same line the action-card lane found: the
per-card rates split precisely along "was this card priced at all".  Before the
fix the bot only ever changed government to Democracy or Theocracy — the two
that print production the static table could see — and **10 of 40 seats ended
the game still on Despotism**; after, that is **1 of 40**.

[`docs/OPEN_ITEMS.md`](OPEN_ITEMS.md) §2 item 22 guessed that "governments are already
over-played, so this plausibly cuts the other way".  **It does not.**  Takes go
from 23% under the human rate to 19% over it, and changes from 12% under to
35% over.  Reported, not tuned against: the price contains no free constant to
tune, and the composition it produces is far closer to the human one than the
count is far from it.

The revolution/peaceful ratio holds at roughly 2:1 across the change (0.625 :
0.350 before, 1.000 : 0.500 after), which is the sanity check on §2.2 — pricing
both branches did not collapse the bot onto one of them.

## 5. The invisibility check, with numbers

`row_pressure` skips any card whose `card_potential` is `<= 0.0`, so every
price that moves has to be checked against zero.  Fresh 2p board,
`DEFAULT_WEIGHTS`, government = Despotism:

| card | before | after |
|---|---|---|
| Monarchy | **0.000** | **+2.17** |
| Constitutional Monarchy | **0.000** | **+6.40** |
| Republic | **0.000** | **+6.45** |
| Theocracy | +6.55 | +9.93 |
| **Communism** | **−1.20** | **+9.12** |
| **Fundamentalism** | **−6.25** | **+6.25** |
| Democracy | +15.00 | +28.59 |
| Despotism (already in play) | 0.000 | 0.000 |

**Five of the seven takeable governments were unreachable before**: three at
exactly 0.000 and two strictly NEGATIVE, which `row_pressure`'s `val <= 0.0`
skip turns into invisible.  Fundamentalism at −6.25 is the static table at its
worst — it reads the printed `science: -2` as a pure loss and cannot see the
two civil actions, three military actions and two urban slots on the same card.
All seven now price **strictly positive**, and
`tests/test_government_pricing.py:
test_every_government_can_price_positive_on_some_board` pins it.

The mid-game numbers are negative and that is the fix working, not a
regression: from Theocracy, Monarchy prices **−7.39** and Republic **−1.46**,
because a government REPLACES a government and giving up Theocracy's culture,
happy face and strength for one extra civil action is a bad trade.  A card the
bot correctly does not want is exactly what `row_pressure`'s skip is for; the
class is visible, this instance of it is not wanted.

`tests/test_coordinate_registry.py:test_the_credit_gated_classes_are_not_
counted_as_blind` still passes, which is worth noting: it requires some
governments to be dark under `DEFAULT_WEIGHTS` and more of them lit with the
credits on.  Both halves are still true — the dark ones are now the
*downgrades* rather than the *unpriced*.

## 6. Fingerprints

Six arms moved, two held.

| arm | previous | this commit |
|---|---|---|
| NARROW (greedy) | ca255af3 | ca255af3 |
| WIDE (greedy) | f223cea1 | f223cea1 |
| WNARROW | 7a6f6639 | **f82746d4** |
| WWIDE | 996f4ef7 | **5c8e3505** |
| QNARROW | 79e8503b | **b24a738b** |
| QWIDE | bb8d74c7 | **23e36e12** |
| PNARROW | 7e0f7a3b | **5cd3554a** |
| PWIDE | dee840cc | **05f77e2f** |

* **The two GreedyBot arms held still.**  GreedyBot never calls
  `card_potential`, so an arm of it moving would have meant the change had
  leaked into the rules.  It did not.
* **All six evaluator arms moved**, predicted before the run: `DEFAULT_WEIGHTS`
  carries `gov_board_credit` at 1.0, so every government in the row and in the
  civil hand prices differently for all three searching bots.
* **Two-sided** per `docs/PYPY.md` §9.0: derived from scratch in `/tmp/work`
  and independently in `/tmp/gateB2`, which agreed byte for byte on all eight
  arms **including the two that did not move**.
* **Derived a third time after the base moved.**  Master gained the
  documentation-consolidation commits mid-lane, and those touch `engine/` in
  docstring prose only (`engine/bots/pending.py`,
  `engine/bots/variants/wonder.py`), so the prediction was that every arm holds
  across the rebase.  It was checked rather than assumed.
* **Attributed to one constant.**  A third clone with `gov_board_credit`
  changed from 1.0 to 0.0 and nothing else touched reproduces the previous
  column byte for byte: NARROW, WIDE and **WNARROW = 7a6f6639** — the arm this
  change moves furthest — confirmed before this was written, the three wide
  arms and the two remaining narrow ones computing after it.  So
  `government_plans`, `_government_level`, `_government_routes`, `gov_value`
  and `_is_government` are inert on their own and the six moves are that one
  default.

Nothing was re-derived to make the gate pass: it failed by design in both
clones and the committed constants are the computed values.

Test count 1180 → 1198, +18 from `tests/test_government_pricing.py`.  **Negative
control**, in the sense `tests/test_search_root_is_determinized.py` uses:
dropped onto a clean tree at the parent commit, that file gives **5 failures
and 8 errors of 18**.  The five that still pass there are exactly the ones
written to pass — the two `TestTheDefect` controls (the static table still
charges no science for a government and still prices four of them at exactly
0.000), the credit-0.0 equivalence, which is trivially true when there is no
credit, the credit's linearity, and `test_the_engine_really_offers_both`, which
is a statement about `legal_moves` and was true before this change.
