# Every card type, measured: what the bot plays and what can reach the policy (2026-07-30)

`docs/CARD_BLINDNESS.md` asked what the evaluator can *see* on a card and
fixed a real omission. Then eight wonders were repriced and wonder
completions moved by a measured **zero** (0.0997 → 0.1047, p=0.12, n=12,800
seat-games). The pricing was right and it bought nothing, because a wonder's
price has no wire to the policy. Nothing in the test suite noticed, and
nothing could have: the suite checks that a card is priced, never that its
price is *read*.

This document generalises that failure into an instrument and runs it over
all 236 cards.

## One-paragraph answer

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
plainly: **each frozen champion is a 78-key file and the evaluator now has
112 weights, so 34 of the shipped policy's weights were never trained, and 28
of those default to `0.0`.** The entire card-identity channel is a single
untrained weight — `hand_potential` = 0.125 — and everything that does not
flow through it flows through a zero. That count is *growing*: it was 110
weights when this census was measured and 112 by the time it landed, because
sibling lanes are correctly adding pricing behind 0.0 defaults faster than
anything turns one on. See §9.

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
> champions", and §8 re-derived it from `load_weights` rather than
> `DEFAULT_WEIGHTS` for exactly the right reason. The gap it could not see is
> that **the frozen champions are not the bot the league is training.** They
> are a 2026-07-26 snapshot of a 78-key climb; the live league champions carry
> **99** keys, and `experiments/league_state/champion_2p.json` has
> **`row_urgency = −0.19109`**. The wire is connected on the live bot.
>
> Re-running `docs/CARD_BLINDNESS.md` §5.3's wonder A/B against the live 2p
> champion on the same 12,800 seat-games moves wonder completions
> **+0.5731 (+88%, p<1e-4)**, against the −0.0050 null (MDE 0.0089) the frozen
> vector produced. See §5.4 there and `analysis/frozen/README.md`.
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
>   both can be redone. **Read the 3p caveat in `analysis/frozen/README.md`
>   first:** the 3p champion's `row_urgency` is `+0.16269`, the wrong sign for
>   a post-move residual, and a seed-paired A/B (n=600) shows flipping it is
>   worth `+0.0025 ± 0.0305` — a tight null. The weight is active on 35% of
>   decisions but has no gradient at the strength level, so **3p card-ordering
>   concordance is measured against an arbitrary sign.** That is a real
>   caveat on any recomputed 3p concordance figure, not a blocker.
> * **The 4p column is separately unreliable** — `analysis/frozen/champion_4p`
>   is the known-degenerate vector; see `analysis/frozen/README.md`.
> * **The ranking in §4 may reorder.** Wonders were ranked suspect #1 on the
>   strength of a severed pipe. Territories (`hand_mil_potential = 0.0`) are
>   still 0.0 in the live champions and are the better candidate for "a pure
>   severed pipe a single non-zero weight fixes".
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

## 1. The instrument

Two subcommands, both landed in `tools/card_census.py`, both re-runnable
after any evaluator change. Neither touches the engine: the census replays
`game.play_game`'s own loop and diffs a snapshot across each *real* `apply`,
so the trial states inside the bot's search are never counted, and the probe
reuses `evaluate` / `rival_context` / `copy_state` unmodified.

### 1.1 What "conditional on availability" means, and why the denominator is per type

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

### 1.2 The probe: does card identity move the score?

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

### 1.3 It fails loudly, and only when it should

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

## 2. The plumbing map

Traced in code, not inferred. The two columns that matter are the last two:
which term carries this card's **identity** into the policy, and **what
weight does that term actually have** in the shipped vectors. The verdicts
are the ones §4.1 arrives at, after the search control in §4.0 — a type that
is dead at 1 ply and alive under `plan:width=2` is Tier B, not a bug.

**The critical fact, and reading `DEFAULT_WEIGHTS` alone will not give it to
you.** Each frozen champion is a **78-key** file. The evaluator has **112**
weights (110 when the census was measured at `50ba471`). `load_weights`
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
it flows through a zero. That is the whole of §4 in one sentence, and it is
why "the weight exists, defaulted to 0.0, so the trainer can decide what it
is worth" — the reasoning `docs/CARD_BLINDNESS.md` §2.2 uses, correctly, for
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

### 2.1 Wonders: the archetype, and the mechanism is more specific than "blind"

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

### 2.2 The military hand: same defect, four more types

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
`docs/CARD_BLINDNESS.md` §3 bucket 5 wrote off `tacticBonus` and friends as
"military hand: never reaches `_card_yields`", which is true, and the summary
table then reported all 55 events, 11 aggressions, 10 pacts and 3 wars as
"zero visible gain" — a claim about a function it never called on them.
Aggressions and wars are in fact priced by *resolution*, not by
`_card_yields`, which is a different mechanism with a different failure mode
(§2.3). **Verifying which path a type takes before claiming anything about it
is the discipline this document is trying to install.**

### 2.3 War and aggression: priced by resolution, and the resolution is not in the trial

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
the census below is 1-ply. That caveat is stated again in §6.

### 2.4 Units: the rarer defect — a live pipe carrying the wrong sign

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

## 3. The census

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
wonders 75× more often than 2p for a reason §3.2 makes exact. Wonders are the
one type where pooling across player counts destroys the finding, and it is
worth noticing that a less careful census would have reported "wonders:
take 0.045, finish 0.19" and buried the actual result.

### 3.1 The four numbers that carry the finding

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

### 3.2 Wonders by player count: the plumbing map predicting its own exception

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

## 4. The cross: ranked suspects

The two axes are "seldom played" (§3) and "can its value reach the policy"
(§2). Only the corner where both are true is a bug.

### 4.0 The control that decides the ranking

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

### 4.1 The ranking

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
"bag of numbers" cards, exactly as `docs/CARD_BLINDNESS.md` predicted),
special-tech (12), leader (24), action (33), pact (10). That is **11 types
and 103 of 236 cards**, all reached by `hand_potential` at 0.125 or, for
pacts, by `deferred_credit` into `features()`. Their probe controls are the reference
the broken types are measured against: `take | leader` 0.843–0.968,
`take | special-tech` 0.848–0.987, `take | action` 0.792–0.972.

### 4.2 How a bug was distinguished from a bad card

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
and `docs/SCORE_VALIDATION.md` §6.2 — forcing wonders cost 34.3 ± 7.0 margin
— is the standing reminder that low use is not automatically wrong. It is
also why §4.0's search control comes before any of this: "seldom played" and
"cannot see it" are both necessary, and neither is sufficient.

### 4.3 If you fix one thing

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
   this document is the measurement it should be judged against, and §3.2 is
   the specific table that should move.**
4. **event** — the largest type (55 cards) and the most work: `_card_yields`
   returns nothing for any of them, so plumbing alone changes nothing. Needs
   a mapping first, and the `allPlayers`/rank-block tree is exactly the
   board-scaled and trigger-shaped pricing `docs/CARD_BLINDNESS.md` §3
   already wrote off as hard. Lowest ratio of value to effort of the four.

And one that is not on the list but should be somebody's: **`cost.militaryActions`
on 54 cards** (§5.1) is a genuine top-level-field blind spot of exactly the
kind that just cost a season on governments, and it is cheap.

## 5. Incidental findings, for routing

None of these are mine to fix; they are named here so they are not lost.

### 5.1 `cost.militaryActions` is the government bug, still open, on 54 cards

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

### 5.2 `hand_mil_potential` cannot ever use board pricing

`weighted.py:1269` calls `card_potential(n, w)` with **no `state`/`idx`**,
while its civil sibling `hand_potential` passes both. So even with
`card_board_credit` turned up, board-aware pricing can never fire for a
military card. Whoever turns on `hand_mil_potential` will get the printed
numbers only, and will not be told.

### 5.3 The degenerate-champion guard has a hole, and 4p walks through it

`arena.refuse_if_degenerate_champion` compares weight files by **exact
content**. `analysis/frozen/champion_4p.json` is **76 of 78 weights identical**
to `experiments/champion_4p.json` — the vector `docs/TRAINING_RUN.md` says
never to warm-start from — differing only in `colonies` and `pacts`, and
keeping the thing that makes it degenerate: **`science = -6.0888`**. It
passes the guard silently. `tools/card_census.py` now warns on ≥95%
similarity as well as exact identity, and **every 4p number in this document
carries that caveat.**

### 5.4 Dead data and dead code

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

## 6. What this does not establish

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
* **4p is measured on a known-degenerate vector.** See §5.3.
* **It does not measure what any fix is worth.** Every claim here is about
  what the policy *can see*, not about win rate. The wonder lane's A/B is the
  template for turning one of these into a number.
* **`offered` is an opportunity count, not a preference.** A card offered in
  slot 12 at 3 civil actions is not the same offer as one in slot 1, and the
  census does not weight by slot cost.

## 7. Reproducing

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

## 8. Provenance

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
  claim in §2 carrying a file:line. The three claims the conclusions rest on
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

## 9. Rebase note: what landed while this was measuring, and why the numbers stand

This census was measured against master `50ba471`. By the time it landed,
eleven commits from sibling lanes had gone in, four of them squarely in the
territory of §4's Tier A:

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

So the wonder lane has now built the term §4.3 asks for — `wonder_potential`,
a wonder-in-progress sibling to `hand_potential` — and at the shipped default
it contributes exactly zero, which is precisely the state this document
exists to make visible. The count of untrained weights went **110 → 112**
while I was measuring. Nothing here argues those defaults are wrong: 0.0 is
the correct way to land a new channel without invalidating three frozen
champions. The argument is narrower and it is the point of §2's weight
column: **"the trainer will decide what it is worth" stops being true the
moment the champions that would do the deciding are frozen and the leagues
warm-start from them**, and at that point a 0.0 default is not a neutral
prior, it is a decision to ship the blind version.

The concrete ask that follows: `tools/card_census.py check` should be run
against `analysis/census/baseline.json` **after** any of these weights is
turned up, and §3.2's wonder table is the specific thing that should move.
If it does not, the fix is inert for the same reason the last one was.

## 10. The territory suspect and the defence drain are ONE defect (2026-07-30)

§4.1 ranked **territory** its number-one confirmed-broken suspect and §2.3
described war and aggression as "priced by resolution, and the resolution is not
in the trial". Both are the same defect, and the defence lane found it from the
other end — see `docs/AGGRESSION_RATE.md` §8.

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
position that does not yet contain the outcome.** §4.3 ("if you fix one thing")
should be read with that in front of it.

Consequence for this document's numbers: the census counts and the plumbing map
stand — they are about which features exist and fire. The *ranking* of territory
as a pricing defect is confounded, because part of what looked like a mispriced
card is a mispriced position.
