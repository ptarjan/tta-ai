# Pricing the cards whose value is a sentence (2026-07-29)

Lane C of the follow-up to `docs/CARD_BLINDNESS.md`: **leaders (24), actions
(33) and governments (8)**.

That document ends by naming its own biggest gap — "a board-aware card
evaluator — one that takes `(name, state, idx, w)` rather than `(name, w)` —
is what closes buckets 1 and 4, and it is the single highest-value follow-up
this census suggests." This is that evaluator, for the three card types where
the gap was worst.

**Result in one line:** the pricing lands and is inert (all eight fingerprint
digests unmoved); the census and the behavioural counter both move a long way
(16 → 4 blind leaders; leaders taken 3.6 → 5.6 per game); and the win-rate A/B
is **flat in aggregate, with one half significant and one half not** —
governments help (culture margin +1.85, z = 3.4), leaders are a **null**
(−1.8pp, z = −1.46, p = 0.15 once the over-dispersed blocks are accounted for)
— so the governments half is the reverse of what I predicted (§5.2).

> This line originally read "decomposes into two opposite signs … leaders hurt
> slightly (−1.8pp, z = −2.1)". The leaders half did not survive the
> unit-of-analysis audit of 2026-07-30; see §5.2's correction box and
> `docs/CARD_BLINDNESS.md` §10.5. I was wrong about governments, and about
> leaders I was right for the wrong reason — I predicted "neutral-to-positive"
> and the answer is "neutral", but the −2.1 I then reported as a real negative
> was an artefact of clustering on the wrong unit.

## 0. One-paragraph answer

All 24 leaders had a dropped key and **16 of the 24 were worth nothing to the
evaluator beyond "it is a leader"**. The fix is not a table of handlers. The
engine already implements every one of these rules in
`engine/effects.py:_apply_modifier`, so `engine/bots/board_yields.py` prices a
leader by **swapping it onto the player's board and asking
`effects.compute` what changed**. That reuses the rules instead of copying
them, and it gets three things right that no per-key handler can: leader
*replacement* is a diff and can be negative, the engine's clamps apply, and
**governments fall out for free** — which turned out to matter, because a
government's whole value is its top-level `civilActions` /
`militaryActions` / `urbanBuildingLimit`, which live in no `production` or
`effects` block and which `_card_yields` has therefore never read at all.

## 1. The governments finding, which is a result on its own

`_card_yields` walks exactly two blocks of a card, `production` and
`effects`. A government keeps its most important numbers outside both:

```json
{"name": "Republic", "civilActions": 7, "militaryActions": 2,
 "urbanBuildingLimit": 3, "peacefulCost": 13, "revolutionCost": 3,
 "techCost": null, "effects": {}}
```

Despotism grants 4 civil actions. Republic grants 7. **Civil actions are the
core currency of Through the Ages and the evaluator could not see the largest
single source of them.** Four of the eight governments (Despotism, Monarchy,
Constitutional Monarchy, Republic) have an empty `production` and an empty
`effects`, so they were *literally* the empty card to `card_potential`.

The cost side was blind in the same way and for a related reason.
`_card_yields` reads `card["techCost"]` — and `techCost` is `null` on every
government, because a government is paid for either peacefully
(`peacefulCost` science, charged by `effects.tech_cost`) or by revolution
(`revolutionCost` science plus the whole civil action pool, charged by
`actions._h_revolution`). So all eight governments were priced as **free and
worthless simultaneously**, which is the only reason the blindness was not
already obvious in play.

`board_yields` prices the revolution route, because `revolutionCost` is
cheaper in science on every card in the deck (Monarchy 2 vs 8, Democracy 9 vs
17) and is the route the engine's own `_can_revolt` makes available. The
science goes on `science` as a clamped cost; the burned action pool goes on
its own `gov_action_cost` weight, board-aware, because emptying a 7-action
Republic turn is not the same price as emptying a 4-action Despotism turn.
Splitting them rather than summing them is what lets the league discover the
exchange rate instead of being told it.

This is confined to governments: no card of any other type carries a
top-level action count, so there is nothing here to route to another lane.

## 2. Why a swap diff and not a handler table

The obvious implementation is a dispatch table, one handler per effect key:
`culturePerTheater` → `val * workers_on_types(p, {"theater"})`. It is also
the implementation `engine/effects.py:1197-1202` exists to warn about:

> Hollywood and Internet score off `_BUILDING_OUTPUT`, not their printed
> production [...] before that fix the code summed printed values with an
> ad-hoc Sid Meier special case, which under-scored every Chaplin,
> Shakespeare, Newton and Einstein completion.

Two implementations of one rule drift, and the evaluator's copy drifts
silently — nothing fails, the bot just misprices. So:

```python
old = p.leader
p.leader = "Michelangelo"
after = effects.compute(state, p)     # the real rules engine
p.leader = old
delta = after - effects.state_stats(state, p)
```

All thirteen of the `effects.MODIFIER_KEYS` that any leader carries are then
priced exactly, for free, by the code that actually runs them (19 keys in
total once `_apply_special`'s two and the two riders are counted). Three
things fall out that a per-key handler gets wrong by construction:

1. **Replacement.** A leader replaces the leader you have, so the value of
   taking one is a *difference*. Taking Gandhi (+2 printed culture) while you
   hold Churchill (+3 culture a turn) is a **loss of 1 culture a turn**, and
   the diff says so. `_card_yields` says `+2` regardless of what you hold, and
   always did.
2. **Clamps.** `compute` ends with `happy = max(0, min(8, happy))` and every
   rating floored at 0. A leader's ninth happy face is worth nothing and the
   diff knows.
3. **Governments**, per §1, with no extra code at all — `compute` reads
   `p.government` the same way it reads `p.leader`.

### 2.1 The trap, written down because it fails silently

**Use `compute` for the hypothetical, never `state_stats`.**

`state_stats` is a per-mutation cache keyed on `p.idx`, validated against
`stats_key(state, p)` and *only rebuilt when the entry is marked dirty*.
Assigning `p.leader` does not mark it dirty. So the natural-looking version of
the code above returns the stats of the **old** leader and every diff comes
out as exactly zero — no exception, no warning, every leader priced at nothing,
which is indistinguishable from the bug being fixed.
`tests/test_board_yields.py:TestTheComputeVsStateStatsTrap` reproduces the
trap directly and fails if the two calls are ever swapped.

### 2.2 The memo key, verified rather than trusted

`compute` is hot, so the diff is memoised on
`(name, effects.stats_key(state, p))`. `stats_key` carries a documented
invariant that it names every field `compute` reads, which is exactly the
completeness this key needs.

A docstring is not evidence. `TestStatsKeyIsACompleteMemoKey` plays 2p and 3p
self-play games, and for every player at every ply — under six different
hypothetical leader/government swaps, so the *hypothetical* side of the diff
is covered too — records `stats_key -> compute`. It fails if one key ever maps
to two different `Stats`. Over ~1300 distinct keys there are **no
collisions**. A key that missed a field would serve silently stale card
valuations, which is a worse bug than the blindness this module fixes.

### 2.3 Which parts are rules and which are judgement calls

Worth separating explicitly, because the two need different kinds of
justification and only the second kind needs evidence.

**Rule-faithful — no free parameter, no discount, nothing to tune.** These are
not models of the rules, they are the rules, obtained by running them:

* every `Stats` delta from the leader/government swap — all thirteen
  `MODIFIER_KEYS` on leaders, both `_apply_special` keys, and the top-level
  government action counts;
* the replacement semantics (a leader replaces a leader), the engine's clamps,
  and the government science cost, which is read off the card;
* **Genghis Khan.** "One of the two strongest, ties in your favour" is
  computed exactly from rival strengths. It looks like a judgement call and is
  not one.

**Judgement calls — a choice was made and could be made differently.** Each is
flagged here so nobody later mistakes it for a derivation:

| choice | what was chosen | the alternative |
|---|---|---|
| **Churchill's `perTurnChoice`** | value him at the culture option, 3/turn | model the military option's 6 ring-fenced points as worth more |
| **Which government route to price** | revolution (`revolutionCost`), always the cheaper science | price the peaceful route, or the min of the two under current stats |
| **Revolution's action cost** | its own `gov_action_cost` feature at 0.0 | fold it into `civil_actions`, i.e. assert an exchange rate |
| **`resourcesForMilitaryUnits`** | own `restricted_resources` feature at 0.0 | treat ring-fenced resources as plain `resource_stock` |
| **Reserves' "food OR resources"** | max under the current weights | a fixed 50/50, or always the resource side |

Note the shape of that table: every judgement call except Churchill's was
resolved by **creating a 0.0-weight feature rather than by picking a number**.
That is deliberate — it converts a choice-with-a-free-parameter into something
the league fits, so the only genuinely hand-set constant in the whole change is
Churchill's 3.

## 3. The census

`tools/card_blindness.py` grew a `--board` mode that counts the board-aware
evaluator as well as the static table, on a board stocked with one staffed
example of everything a card can be paid for. Without stocking the board the
question is meaningless: Bach with no theaters really is worth nothing, and
counting that as blindness would be counting the wrong thing.

| card type | n | zero visible gain: master → static now → **board** |
|---|---|---|
| **leader** | 24 | 17 → 16 → **4** |
| **government** | 8 | 4 → 4 → **0** |
| **action** | 33 | 19 → 6 → **2** |
| TOTAL (all types) | 236 | 168 → 146 → **129** |

`--legacy` still reproduces the published master column exactly (171 dropped /
168 zero-gain), and now has a test pinning it — see §7.

The four leaders that remain flat are **Aristotle** (1 science per technology
card taken), **Hammurabi** (a military action usable as a civil action),
**Christopher Columbus** (remove him to colonize free) and **Frederick
Barbarossa** (a combined pop-increase and unit build). Every one is a trigger
or a rule change, not an omission, each has a written reason, and
`TestEveryLeaderIsPriced.STILL_FLAT` fails if the list grows.

## 4. The cards, individually

Two leaders are priced by a **rider** rather than by `compute`, because their
payout is a turn-end trigger and `compute` builds only the production phase.
Both are exactly computable, so neither is a guess:

* **Winston Churchill** — "once each turn, choose: 3 culture; or 3 restricted
  science and 3 restricted resources." The culture option needs no board, no
  other card and no condition, and is available every turn, so his floor is a
  flat **+3 culture production** — more than any wonder in the game prints.
  The military option is taken as worth no more, because both its halves are
  ring-fenced.
* **Genghis Khan** — "3 culture at end of turn if you are one of the two
  strongest civilizations, ties in your favour." Computed exactly from rival
  strengths. Note what it says **at two players**: "one of the two strongest"
  out of two civilizations is vacuously true, so Genghis is an unconditional
  +3 culture a turn at 2p and a real condition at 3p and 4p. No static table
  can express that, and it is the cleanest single argument for board-aware
  pricing in the set.

Both riders **subtract the outgoing leader's rider**, for the same reason the
Stats side is a diff. Forgetting that subtraction is how Gandhi-over-Churchill
comes out as +2 instead of −1.

Three action cards were board-scaled and are now priced additively (they are
not swaps, so nothing is replaced): **Endowment for the Arts** (culture per
civilization ahead of you on culture — 6 per rival at 2p, so worth 6 or
nothing and never anything in between), **Wave of Nationalism** and **Military
Build-Up** (resources per stronger civilization, ring-fenced to military
units, hence `restricted_resources` rather than `resource_stock`).

The three **Reserves** needed something different again: "gain N food **or** N
resources". Summing both would be a lie in the opposite direction from
dropping the key, so `_card_choices` returns mutually exclusive groups and
`card_potential` takes the better one under the current weights.

## 5. Inert, then live

Everything is gated on one new weight, `card_board_credit`, defaulting to
**0.0** — the exact analogue of `card_rate_credit` and for the same reason:
at 0.0 `card_potential` returns the byte-identical pre-change answer, so the
A/B is paired, same-process, on the same deal, and the eight fingerprint
digests do not move. Six new features (`urban_limit`, `gov_action_cost`,
`pop_food_discount`, `no_aggression`, `restricted_resources`, plus the credit
itself) all default to 0.0.

`tests/test_board_yields.py:TestTheCreditGateIsExact` asserts that at 0.0 the
board-aware and static answers agree for **all 236 cards**.

A 0.0 default has a second consequence worth naming: `hillclimb_league`
derives `NONNEG` / `NONPOS` from the *sign* of the default, so a weight
defaulting to 0.0 is in neither set and the climber may move it in either
direction. `card_board_credit` is therefore not merely inert — **the league
can switch this on by itself** if it is worth switching on, without anybody
editing a constant. That is the same reasoning
`docs/CARD_BLINDNESS.md` §2.2 gives for refusing to put a negative prior on
the finish-discipline terms.

All eight fingerprint digests were checked and **none moved**:

```
narrow 0a6ed6ad  wide 4a8c6ca6   (GreedyBot, plain and FASTCOPY_PARANOID)
weighted narrow 5eff41eb   weighted wide d03e0964
quiescent narrow eff1bef5  quiescent wide 9e9695d4
plan narrow c534ac3d
```

One note on process, because the run looked alarming for a while. The
`weighted wide` arm first reported `FAIL`, and the recorded digest was
**empty** rather than wrong — the `perf_check` subprocess had been killed
under load rather than producing a different answer. A control run on a
pristine `6968256` checkout died in exactly the same way, which is what
identified it as environmental. Re-derived on its own, the arm produces
`d03e096414d7adb4af7b6d22cd534195a45f27beb91678cde547a7b05e47597c`, which is
the constant already in `tools/gate.sh`. The constant was not touched. Worth
recording as a gate-reading habit: `check_fp` renders a crashed arm and a
genuinely moved digest almost identically, and the tell is that the "got"
field is blank.

### 5.1 How large is the perturbation?

Under the frozen 2p champion's own weights, on a round-11 board where it holds
Joan of Arc:

| card | credit 0.0 | credit 1.0 |
|---|---|---|
| Michelangelo | 0.00 | **+10.64** |
| Winston Churchill | 0.00 | **+10.64** |
| Genghis Khan | 0.00 | **+10.64** |
| Endowment for the Arts | 0.00 | +6.00 |
| Republic | 0.00 | +3.49 |
| Fundamentalism | −7.62 | −0.12 |
| Reserves (III) | 0.00 | +1.87 |
| **Sid Meier** | 0.00 | **−11.18** |
| Eiffel Tower (control) | 27.45 | 27.45 |

Sid Meier is the one to look at. He prices *negative* because that board's
only lab is level-0 Philosophy, so his "each lab makes culture equal to its
level" pays nothing while his "−1 science per lab" still bites, and on top of
that he would replace Joan of Arc. He is genuinely a bad card on that board,
and this is the first time the evaluator has been able to say so about any
card.

The negatives are not a bug and are worth being explicit about: once you hold
a leader, a *worse* leader correctly prices below zero. That suppresses
downgrades while leaving upgrades (the +10.64 rows) firmly attractive.

### 5.2 Win rate: a flat aggregate that decomposes into two opposite signs

Method as in `docs/CARD_BLINDNESS.md` §4: `experiments.evaluate` at 2 players
plays each deal twice with the seats swapped, so the comparison is paired on
the deal; both arms are `analysis/frozen/champion_2p.json` differing in
`card_board_credit` alone (verified: exactly 1 of 105 weights differs). Each
arm is 8 disjoint blocks of 400, n = 3200 games / 1600 deals, SE ≈ 0.7pp on
the paired win rate, **MDE ≈ 2.0pp**. Run on the desktop pinned at `664cdfc`,
12 workers.

`TTA_BOARD_TYPES` restricts board pricing to a subset of card types, which is
what makes the decomposition possible at all:

| arm | win rate (paired) | culture margin | own culture |
|---|---|---|---|
| everything on | 49.95% ± 1.68pp (z = −0.1) | +0.95 ± 1.31 (z = +1.4) | 150.8 vs 149.8 |
| **governments only** | 51.02% ± 1.40pp (z = +1.4) | **+1.85 ± 1.07 (z = +3.4)** | 149.4 vs 147.5 |
| **leaders only** | 48.20% ± **2.92pp** (z = **−1.46**, p = 0.15) | −0.48 ± 2.56 (z = −0.4) | 149.0 vs 149.5 |

> **Corrected 2026-07-30** (`docs/CARD_BLINDNESS.md` §10). The leaders row
> previously read **48.20% ± 1.69pp (z = −2.1)** and was read as "leaders hurt
> slightly". That interval is correctly clustered on the deal; the problem is
> one level up. **The eight blocks are over-dispersed**: per-block win rates
> 43.8, 47.8, 52.4, 46.3, 53.8, 46.3, 45.6, 49.9, a spread of 3.49pp where
> deal-level noise predicts 2.44pp, χ² = 14.41 on 7 df against a critical
> 14.07. Clustering on the block instead gives **z = −1.46, p = 0.15**.
>
> **The leaders effect is not statistically significant and this document
> should not be read as showing that leaders hurt.** The honest summary of
> §5.2 is now "governments help on the culture margin; leaders are a null with
> an unstable point estimate", not "two opposite signs".
>
> This is a borderline call, stated as one: the escalation trigger is only just
> tripped and a heterogeneity test on eight blocks is not powerful. But a
> result whose significance depends on which of two defensible clusterings you
> choose is not a result. If the leaders arm matters, it needs more blocks, not
> a different formula.
>
> **The governments half is unaffected.** It was already deal-clustered, its
> blocks agree (χ² = 2.59 on 7 df), and **+1.85 ± 1.07 (z = 3.4)** stands. The
> "everything on" aggregate is likewise unchanged at 49.95% ± 1.68pp — but note
> that its flatness can no longer be explained as two significant opposite
> signs cancelling, since only one of the two is significant.

The aggregate is a textbook flat null — 49.95% against a 50% null, z = −0.1,
which is as close to nothing as 3200 paired games can report.

**The aggregate is flat because the two halves point in opposite directions
and roughly cancel.** That is the whole reason to decompose, and an aggregate
null would have hidden it completely.

#### I predicted this backwards, in both directions

Before running, and on the record: *"governments negative, leaders
neutral-to-positive"*. The reasoning was that `gov_action_cost` defaults to
0.0, so the on-arm prices a revolution's science but not the civil-action pool
it burns, and the behavioural counter showed governments taken **doubling**
(1.1 → 2.1 per game) — a bot revolting twice as often while blind to the cost
of revolting looked like an obvious way to lose.

**That mechanism is refuted.** Governments are the half that *helps*: the
culture margin is +1.85 with z = 3.4, which is the only individually
significant effect in the whole experiment. So the doubled revolution rate is
apparently closer to correct play than the frozen champion's, even with the
action cost unpriced — which is a much more interesting statement about the
game than my prediction was, and it makes the §1 finding stand on its own two
feet: **the evaluator not being able to see Republic's 7 civil actions was
costing something real and measurable.**

Leaders are the half that hurts, by −1.8pp, marginally (z = −2.1, p ≈ 0.04,
and one arm out of two at that threshold is roughly what you expect by
chance). It is small, it is not the "markedly worse" that would indicate a
broken implementation, and the culture margin does not corroborate it
(z = −0.7). But it is the direction that warrants a look rather than a shrug,
and the two candidates worth checking first are both in §8 already:

1. **`hand_potential` double-counts leaders.** Every leader in hand is priced
   as replacing the *current* leader, but only one of them can be. That
   over-count was harmless when the bot held ~0 leaders in hand and is not
   harmless now that it takes 55% more of them. This is the strongest
   candidate and it is a defect in the *hand term*, not in the pricing.
2. **A leader's upside lands on well-fitted weights and its restrictions land
   on 0.0 ones**, per the asymmetry table below.

Neither is a reason to unship rule-faithful pricing, and neither is settled by
this experiment.

#### What this means for shipping

`card_board_credit` stays at **0.0**, which is what is committed. Concretely:

* Nothing needs to change before a league restart — the shipped engine is
  byte-identical to master in behaviour, so the arms can be restarted on it
  safely.
* If anyone turns this on, **turn the government half on first**. It is the
  half with a positive, individually significant signal, and
  `TTA_BOARD_TYPES=government` already expresses exactly that configuration.
  Making it a weight rather than an env knob is the obvious follow-up.
* The leader half should wait on the `hand_potential` double-count.

## 6. Does the bot actually take these cards?

`docs/CARD_BLINDNESS.md` §5.1 names the trap that makes this question
mandatory rather than optional: **giving a card a weight does not help until
the bot takes the card.** Three of the keys added there sit at exactly 0.000
variance because the champion never takes Masonry or Library of Alexandria,
so those weights have no gradient at all and a hill climb only drifts them.

`tools/take_census.py` counts every card taken from the civil row under a
given vector. Under the frozen 2p champion, 8 games, credit 0.0 vs 1.0:

| | credit 0.0 | credit 1.0 |
|---|---|---|
| leaders taken per game | 3.62 | **5.62** |
| governments taken per game | 1.12 | **2.12** |
| leaders NEVER taken (of 24) | **14** | **7** |

So this is not the §5.1 situation: the bot takes these cards constantly, both
before and after. The interesting part is *which* ones, and it lines up with
the census exactly. Before the change the leaders it took were precisely the
ones the static table could already see — Joan of Arc, Homer, Gandhi,
Shakespeare, all of which print a plain `happy` or `culture` — and the 14 it
had never once taken included Michelangelo, Napoleon, Sid Meier, Churchill,
Bach and Genghis Khan, precisely the ones that priced at zero.

That was written down as a prediction before the arm was run, and it holds:

| leader | taken, credit 0.0 | taken, credit 1.0 |
|---|---|---|
| Genghis Khan | 0 | 7 |
| Michelangelo | 0 | 5 |
| Winston Churchill | 0 | 4 |
| Charlie Chaplin | 0 | 3 |
| Isaac Newton | 0 | 3 |
| J. S. Bach / Sid Meier / Moses | 0 | 1 each |
| Joan of Arc | 8 | 3 |

Joan of Arc going *down* is the other half of the same fact: she is no longer
the only leader the evaluator can see.

## 7. Guardrails added

* `tests/test_board_yields.py` — 28 cases: the `compute`/`state_stats` trap,
  the memo-key completeness sweep, the government blindness stated as tests
  against the *old* behaviour, per-leader pricing, monotonicity in the board
  count (a leader must grow with the thing it pays for, not merely be
  non-zero), the credit gate's exactness, and the choice cards.
* `tests/test_card_pricing.py` — the existing key-coverage guardrail now also
  reads `_EFF_CHOICE` and `board_yields.BOARD_PRICED`; new cases reject a
  board-priced key with no written reason, a stale one, and a key claimed
  both priced and unpriced.
* **`--legacy` is now pinned.** This one caught a real bug rather than being
  written for tidiness. `_card_choices` read the card data directly instead of
  being gated on its registry the way `_EFF_SPECIAL` is, so
  `use_legacy_maps()` could not switch it off and `--legacy` silently stopped
  reproducing the published 171/168 "before" numbers — quietly rewriting the
  baseline that every later result is measured against. Now a test.
* `DELIBERATELY_UNPRICED` lost 17 keys to `BOARD_PRICED` and 2 to the static
  tables. Bucket 1 ("board-scaled") is down from 16 keys to 4, and the four
  that remain are on wonders and an event — where a swap is the wrong
  question, because a wonder accumulates rather than replaces.

## 8. Known limitations, stated rather than discovered later

* **Two leaders in one hand are both priced as replacing the current one.**
  You can only play one, so this over-counts. It is the pre-existing shape of
  `hand_potential` (two wonders in hand double-count the same way) rather than
  something this change introduces, and fixing it means making
  `hand_potential` a max-plus-remainder rather than a sum — a change to the
  hand term, not to card pricing.
* **`gov_action_cost` sits at 0.0**, so in the on-arm a revolution's science
  cost is priced but the civil-action pool it burns is not. That makes
  governments somewhat too attractive until the league prices that weight. It
  is deliberate — a non-zero default would not be inert — but if the
  government decomposition arm comes out negative, this is the first thing to
  suspect.
* **Cost, measured.** Board pricing calls `effects.compute` once per
  swap-type card in the hand and the row per leaf. 2p self-play, 6 games,
  same seeds, cache cleared between arms:

  | arm | ms/ply | |
  |---|---|---|
  | `card_board_credit` 0.0 | 4.07 | |
  | `card_board_credit` 1.0 | 5.14 | **1.26×** |

  **At the shipped default the cost is exactly zero**, not merely small:
  `card_potential` returns on `if not board` before touching
  `board_yields` at all, which is the same early return that makes the
  pricing byte-identical to master. The 1.26× is what turning it on costs,
  and it is the number to beat if it is ever turned on for a league run.
  The memo helps less than it looks like it should — 1894 distinct entries
  over ~1100 plies — because `stats_key` includes the worker counts and so
  changes on nearly every move; it collapses the several cards priced within
  one decision, not across decisions.
* **The four remaining flat leaders need a measured trigger rate**, not a
  guessed one. Aristotle pays 1 science per technology card taken, Newton
  refunds a civil action per technology played; pricing either honestly needs
  a count of how often those events actually happen per round, which
  `tools/take_census.py` is now most of the machinery for.

## 9. Reproducing

```bash
python3 -m tools.card_blindness --legacy          # 171 / 168, the baseline
python3 -m tools.card_blindness                   # static table only
python3 -m tools.card_blindness --board           # with board_yields
python3 -m tools.card_blindness --board --cards leader

python3 -m unittest tests.test_board_yields tests.test_card_pricing

# what the vector actually TAKES -- the docs/CARD_BLINDNESS.md 5.1 check
python3 tools/take_census.py --w analysis/laneC/off.json --games 40 \
    --type leader

# the A/B, and the two decomposition arms
bash analysis/laneC/run_ab.sh main
TTA_BOARD_TYPES=government bash analysis/laneC/run_ab.sh government
TTA_BOARD_TYPES=leader     bash analysis/laneC/run_ab.sh leader

bash tools/gate.sh
```
