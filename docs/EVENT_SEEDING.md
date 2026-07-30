# Events, aggressions, pacts and wars: what the evaluator can see (2026-07-29)

This is the opponent-interactive lane of the card-pricing work that
`docs/CARD_BLINDNESS.md` opened: **55 events, 11 aggressions, 10 pacts, 3
wars.** The census in that document counts all 79 of them as "has a dropped
key" and all 79 as "zero visible gain", and the brief for this work was to
price them.

**The census is the wrong instrument for these cards, and 24 of the 79 were
never broken.** That is the first result here and it changes the shape of
everything after it. The second is that the one category which *is* genuinely
broken — events — is broken much worse than the census suggests, in a way a
`production`/`effects` table could not have fixed, and it is measurable: the
bot seeds 8.75 Age III scoring events per game and chooses which ones
essentially at random.

Everything here is base game (2015 "A New Story of Civilization"), measured at
2 players unless stated.

## One-paragraph answer

`_card_yields` is not the hook for any card in this lane, because
`hand_potential` walks `hand_civil` only and **all 79 of these cards are in the
military deck** — `_card_yields` is never called for one of them, and adding a
table entry would change nothing. For aggressions and wars that does not
matter, because the search already prices them by *resolution* rather than by
table: `QuiescentBot` drains the defender's pending `defense` decision and
evaluates the quiet position, and `quiescent.war_value` calls the engine's own
`events.resolve_war` on a scratch copy and substitutes the result at the leaf.
`PlanBot`, which the league actually trains, inherits both. Pacts have
`count 2p: 0` — they are not in a two-player deck at all. That leaves events,
where the gap is real: `engine/bots/weighted.py` contains **no reference to
`future_events`, `current_events` or `seeded_by`**, so the entire visible
consequence of seeding a card is the `+level_of(name)` culture
`_h_prepare_event` grants on the spot. For the fifteen Age III "Impact of ..."
events that omits the card. This change adds one feature,
`event_scoring_margin`, which asks the **engine's own scorer** what the
scoring events already in play will pay out, differenced against the best
rival; it defaults to 0.0, so it is inert and no fingerprint digest moves.

## 1. `_card_yields` is the wrong hook, and here is the proof

`data/cards_military_actions.json` holds every card in this lane. The
evaluator's card-pricing path is `hand_potential` → `card_potential` →
`_card_yields`, and `hand_potential`'s docstring and body agree: it is
`hand_civil` only. `weighted.py` says so itself, in the block that writes off
`tacticBonus`:

> `hand_potential` walks `hand_civil` ONLY, so `_card_yields` is never called
> for a tactic, war, aggression, territory or bonus card and mapping these
> keys would change nothing today.

So the whole framing of "add the missing entries to `_EFF_TO_FEATURE`" is
inapplicable to this lane. The census counts a *key* that `_card_yields`
would drop if it were ever asked; for these 79 cards it is never asked.

Reading the census rows for `event`, `aggression`, `pact` and `war` as "these
cards are unpriced" is therefore a false positive on 24 of them and an
*understatement* on the other 55. What the evaluator actually sees of the
military hand, in full, is two numbers:

```python
"hand_military": len(p.hand_military)          # weight 0.30
"hand_mil_value": sum(age_level + 1 ...)       # weight 0.15
```

An Age III war, an Age III event and an Age III pact all read as exactly
`4.0 x 0.15`.

## 2. Aggressions and wars are already priced, better than a table could

This is the part of the brief that turned out not to need building, and the
reason is worth writing down because it is the *right* pattern and it already
exists in the codebase.

**Aggressions (11).** `actions._politics_moves` only offers an aggression whose
target it can already beat:

```python
if effects.defense_strength(state, p, q) >= effects.attack_strength(state, p, q):
    continue
```

and `_h_aggression` pushes a `kind="defense"` pending decision for the
defender. A 1-ply `WeightedBot` scoring that trial state sees the spent
military action and the lost card and none of the loot — the failure
`docs/AGGRESSION_FIX.md` documents. `QuiescentBot` fixes it structurally by
draining the pending stack with real picks and scoring the quiet position, and
`weighted.features` only applies `deferred_credit` `if state.pending`, so a
candidate that reached quiescence is scored with the hand-priced credit
contributing exactly zero. **The loot is whatever the engine awards, on this
board, against this defender's actual holdings.**

**Wars (3).** `quiescent.war_value(state, idx, weights, ctx)` calls
`events.resolve_war` on a scratch copy and returns the evaluation of the
resolved position; `PlanBot._leaf` substitutes it whenever
`war_declared_by_me is not None`. `docs/BOT_ARCHITECTURE.md` states the
principle directly: *"Do not hand-price wars. `events.resolve_war` is a pure
deterministic function... Reuse that."*

So `victorTakesCulture`, `takeFromOpponent`, `stealColony` and friends stay in
`DELIBERATELY_UNPRICED`, but under a **new and stronger reason** — not "we
cannot see it" but "it is already resolved, and adding a table price would
double count against a resolution that has already happened". That reason is
now written in `weighted.py` as its own `_unpriced()` bucket (5a) rather than
being lumped in with the tactics.

**On the double-counting warning in the brief.** The concern was that
`margin_share` pays twice for a stolen point, so pricing an aggression at face
value would over-value the category. It does not arise, for two independent
reasons. First, nothing in this change prices an aggression at all. Second,
the league **already moved off `margin_share`** for exactly this reason —
`experiments/hillclimb_pool.py` documents that the margin-trained 2p champion
scored 64.7 own culture against a human 159.5 while holding its rival to 26,
and the default is now `own_share`, which "pays a stolen point exactly once,
which is what the rules do."

**Pacts (10).** Every pact card carries `count: {"2p": 0, "3p": 1, "4p": 1}`.
There are no pacts in a two-player game. They are additionally already priced,
where they exist, by `deferred_credit`'s `pact_offer` branch, which reads
*inside* the addressing blocks and prices each side through the same weight
vector. Nothing measured at 2p — including every A/B in this document — can
say anything about them, and I have not pretended otherwise.

## 3. Events: the gap, and its size

`actions._h_prepare_event` in full:

```python
name = move[1]
journal.touch(p.hand_military).remove(name)
p.culture += _DB.level_of(name)              # <- the only visible gain
journal.touch(state.future_events).append(name)
journal.touch(state.seeded_by)[name] = p.idx
events.reveal_current_event(state, rng)
```

Three points of culture, one fewer military card, and the top of the current
pile is revealed and resolved. `weighted.py` has no reference to
`future_events`, `current_events`, `past_events` or `seeded_by`, and
`docs/INFORMATION_AUDIT.md` independently confirms that deleting all three
"moves no feature". So the plant is a small guaranteed gain and therefore the
*default* politics move — `docs/AGGRESSION_FIX.md` shows
`('prepare_event', 'Rebellion')` beating every attack in its probe for exactly
this reason.

What that omits, for the fifteen Age III events, is the card. Each awards
`events.scoring_culture` to **every** player — 5/4/3/2 culture per completed
wonder by age, 2 per content worker above ten, a 10/0 ranking on strength
rating — either when it is revealed or, if it never is, at game end via
`events.evaluate_final_events`.

`docs/RULES_SPEC.md` §12.5.2 states the strategic point in one sentence:

> After the last turn: evaluate ALL Age III events remaining in the current
> AND future events decks... **Preparing an Age III event guarantees its
> evaluation.**

That is exactly the decision this feature prices. **Seeding an Age III event
is not a way to cycle a card — it is the act of choosing what the game gets
scored on.** A player with three wonders who seeds "Impact of Wonders" has
moved the win condition toward his own board, and the evaluator could not tell
that card from any other Age III military card in his hand.

### The measurement

`tools/event_plants.py`, 20 games, frozen 2p champion, `WeightedBot`:

| | value |
|---|---|
| events seeded per game (all ages) | 22.35 |
| **Age III scoring events seeded per game** | **8.75** |
| Age III events entering play that nobody seeded | **0** |
| final-scoring culture swing per game (abs) | **12.9** (sd 9.8) |
| **margin the seeder's own choice bought, per plant** | **+0.62** (sd 3.84, n=175) |

Read those last two rows together. The Age III scoring events move 12.9
culture of final margin per game. The bot makes 8.75 choices that control
which of the fifteen enter play, each choice worth ±3.8 culture of margin, and
the mean margin it captures is **+0.62** — statistically distinguishable from
zero only because the sample is large. It is not choosing badly; it is not
choosing at all. Nothing it can see distinguishes "Impact of Wonders" (when it
has three wonders and the rival none) from "Impact of Science" (when the rival
leads science): both are one Age III military card and three culture.

**This is not the trap `docs/CARD_BLINDNESS.md` §5.1 warns about.** That
section's lesson is that giving a card a weight does nothing until the bot
takes the card, and `wonder_stages_per_action`, `hand_limit` and
`build_discount` are all dead because the champion never takes those cards.
The opposite holds here: the bot already plants constantly and has already
made the decision 8.75 times a game. The information to make it well is the
only thing missing.

## 4. What was built

### 4.1 The engine's scorer, made callable

`engine/events.py` gains two pure functions, and `evaluate_final_events` is
rewritten to use the first:

```python
def pending_final_events(state):   # -> [(name, block)] still owed a payout
def final_event_culture(state):    # -> per-player culture it will award
```

`pending_final_events` walks `current_events + future_events` and excludes
`past_events`, because an Age III event revealed during play already paid out
through `_apply_player_block` and its culture is banked in `p.culture`.

The point of the refactor is that **the forecast and the payout are the same
code**. The fifteen scoring formulas are stated once, in the rules engine,
where they already were. Restating them in the evaluator is precisely the
failure mode `docs/CARD_BLINDNESS.md` is a document about, one level up:
`tests/test_event_scoring.py` plays real games and asserts that the forecast
equals the culture `evaluate_final_events` then adds.

### 4.2 One feature, default 0.0

`engine/bots/weighted.py` gains `event_scoring_margin(state, idx)`:

> Final-scoring culture the pending Age III events owe me, less the best
> rival's, clamped to ±60.

and `features()` emits it. `DEFAULT_WEIGHTS["event_scoring_margin"] = 0.0`, so
this is **inert**: every trained vector plays exactly as it did, and no
fingerprint digest moves. `experiments/summarize.py` gets an `events` group
for it — its own group, like `row`, so hillclimb's `rescale`/`group` operators
do not drag it around with the strength terms.

Three design decisions, with the reasons:

**It is a margin, not a pair of own/rival terms.** An event that pays me 8 and
my rival 14 is a bad plant, and a feature that knew only the 8 would rate it a
good one — which is today's failure exactly. One coordinate is also far more
likely to be found by the hill climb than two (§5.1 on dead coordinates). The
cost of this choice is that under a pure own-culture objective the differenced
form slightly over-weights denial; the weight defaults to 0.0 and the league
can price that.

**It is gated on `state.game_over`.** `game._finish_game` pays these events
into `p.culture` and the decks still hold the names afterwards, so a forecast
that did not check would double the endgame at every leaf of a search that
reached the end of the game. `tests/test_event_scoring.py` has a test for this
and for the `past_events` case, because neither would make a single game fail.

**The forecast is the current board.** The payout is the board at reveal or at
game end, which is not the same thing. This is an approximation and it is the
honest one available: it is the same estimate a human makes when deciding what
to seed, and the alternative is a model of one's own future development that
the bot has not got. It has a useful side effect — with "Impact of Wonders" in
play, *finishing a wonder* raises the feature too, which is a second and
correct source of gradient.

### 4.3 It is a live coordinate, checked before spending games on it

`tools/feature_variance.py`, 843 decisions of 2p self-play, weight 1.0:

| feature | weight | varying | mean_range | **flip** |
|---|---|---|---|---|
| `culture` | 1.000 | 0.790 | 2.567 | 0.167 |
| `culture_rate` | 5.876 | 0.547 | 0.840 | 0.085 |
| **`event_scoring_margin`** | **1.000** | **0.066** | **0.371** | **0.018** |
| `hand_military` | 0.208 | 0.902 | 2.374 | 0.015 |
| `hand_mil_value` | 0.041 | 0.916 | 6.630 | 0.012 |
| `strength_lead` | 0.267 | 0.261 | 0.343 | 0.001 |
| `wonder_overrun` | 0.000 | 0.015 | 0.098 | **0.000** |
| `hand_limit` | 0.000 | 0.000 | 0.000 | **0.000** |

`flip` is the fraction of decisions where zeroing the weight changes the
chosen move. At 1.8% the new feature changes more decisions than
`hand_mil_value` or `strength_lead` do, and unlike `wonder_overrun` — the
feature §5.1 dissected as a near-dead coordinate — it is not zero. `varying`
is low (6.6%) for a good reason rather than a bad one: it is identically zero
until the first Age III event is in play, which is most of the game.

## 5. Result

**+7.4pp of win rate and +6.5 culture of margin** to the frozen 2p champion
over 3200 paired games, seat-audited and with the frozen-champion upside
artifact checked and excluded. §5.3 is the headline; §5.1 is the behavioural
counter that says the bot changed what it plants, not how often.

### 5.1 Behavioural counter

`tools/event_plants.py`, 20 games each, self-play, on vs off:

| | `esm` = 0.0 | `esm` = 1.0 |
|---|---|---|
| Age III plants per game | 8.75 | 8.75 |
| **margin per plant** | **+0.62** (sd 3.84) | **+1.04** (sd 3.78) |
| final scoring abs swing | 12.9 | 10.7 |

The bot plants exactly as often and picks better: the margin its own choice
buys rises by two thirds. The swing *falls* because this is self-play — both
seats now grab the events that favour them, so the outcomes converge. That is
the expected signature and it is also why a self-play counter cannot be the
headline; the head-to-head A/B is §5.2.

### 5.2 Weight scan

Four weights against the identical vector at 0.0, 300 paired games each, same
seed block. This is a *shape* check, not the result — n = 300 gives SE 2.9pp
and cannot resolve anything under ~8pp on its own.

| `event_scoring_margin` | win rate | p | culture (A vs B) |
|---|---|---|---|
| 0.25 | 53.5% ± 5.6% | 0.224 | 151 vs 146 |
| 0.50 | 53.5% ± 5.6% | 0.224 | 151 vs 146 |
| **1.00** | **55.3% ± 5.6%** | **0.064** | **151 vs 145** |
| 2.00 | **58.2% ± 5.6%** | **0.004** | 150 vs 142 |

**The response is monotone in the weight and has not saturated at 2.0.** Four
arms, all positive, rising. Monotonicity is a much stronger signal than any
one underpowered arm: noise does not usually arrange itself in weight order.

It also says something I did not expect and should not paper over. The feature
is denominated in culture and `culture` is priced at 1.0, so "believe the
forecast" predicts the optimum near 1.0 — and the data says the best of the
four tried is twice that, with the curve still climbing. The likely reason is
that this is a **margin** and the objective being measured is a win rate: a
point of forecast margin moves the comparison by one on my side *and* one on
the rival's, so it is worth about twice a point of own culture. That is an
argument for letting the league find the weight rather than for me picking a
bigger one by hand.

**The 0.25 and 0.50 rows are bit-identical**, and that is worth reading
carefully rather than as a coincidence: identical win rate, identical
cultures, at weights differing by a factor of two. As
`docs/CARD_BLINDNESS.md` §5.1 established for `wonder_overrun`, different
weights cannot produce identical games unless the term never changed a
decision differently between them. So those two rows are **one experiment, not
two independent samples**, and must not be pooled. The 1.00 row is a genuinely
different set of games, which is the direct evidence that the weight is still
buying additional argmax flips at that magnitude.

The powered run in §5.3 uses **1.0**, and the reason is principle rather than
scan-picking: the feature is denominated in culture and `DEFAULT_WEIGHTS`
prices `culture` at 1.0, so 1.0 means "believe the forecast at exactly face
value". Selecting the best of four scanned arms and then reporting its p-value
would be the garden of forking paths.

### 5.3 The powered paired A/B

**Not a null.** `event_scoring_margin` = 1.0 against the identical vector at
0.0, 8 disjoint blocks of 400, 3200 games / 1600 deals, `WeightedBot` 1-ply.
**MDE 2.47pp** at 80% power and α = 0.05 two-sided, so the experiment can
resolve anything above ~2.5pp.

| | n | win rate | culture margin |
|---|---|---|---|
| `esm` 1.0 vs 0.0 | **3200 / 1600 deals** | **57.38% ± 0.91pp** (z = 15.90) | **+6.52 ± 0.34** (z = 38.1) |

> **Corrected 2026-07-30** (`docs/CARD_BLINDNESS.md` §10). This row previously
> read **57.38% ± 1.70pp (z = 8.49)** and **+6.52 ± 1.49 (z = 8.59)**, which
> were independent-samples intervals over 3200 *games* in a design that plays
> 1600 *deals* twice each. Deal-clustered, ρ = −0.72: **±0.91pp, z = 15.90**
> on the win rate and **±0.34, z = 38.1** on the margin. The eight blocks agree
> (χ² = 8.24 on 7 df). The point estimates do not move.
>
> The corrected win-rate figure is not new to this document — the seat audit
> immediately below already derived it as "paired per-deal share sum
> 1.1475 ± 0.0182, z = 15.90", which is the same number times two. The audit
> was right and the headline table was wrong, in the same document. That is
> the tell worth remembering: the paired statistic was computed *as a check*
> and then not used as *the result*.
>
> The margin correction is the larger one and has a clean explanation. The raw
> per-game culture margin has sd 42.9; the per-deal mean has sd 6.84, because
> `per_game_margin` is antisymmetric at 2p and swapping the seats cancels the
> deal's own culture swing almost exactly. A block bootstrap over deals agrees:
> ±0.33.

Eight blocks on disjoint deals:

| block | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 |
|---|---|---|---|---|---|---|---|---|
| win rate | 59.9% | 55.4% | 56.2% | 58.8% | 57.8% | 57.2% | 56.6% | 57.1% |
| margin | +6.7 | +5.6 | +7.5 | +7.2 | +6.9 | +6.2 | +5.9 | +6.2 |

Every block on the same side, spread 55.4–59.9, no block carrying the result.

#### The seat audit, because a uniformly positive result deserves one

Eight blocks with none below the null is also the signature of a systematic
asymmetry, so this was checked rather than assumed. `arena.duel` sets
`seat = g % num_players` and `seed = seed0 + g // num_players`, i.e. **each
deal is played twice with the arms swapped between seats**, and the two games
are adjacent in the task list. That makes the audit exact:

| | win rate | z |
|---|---|---|
| challenger in seat 0 | 59.94% ± 2.39pp | 8.15 |
| challenger in seat 1 | 54.81% ± 2.42pp | 3.90 |

There **is** a seat effect — about 5pp — and it is not the result: both seats
beat the null on their own, the weaker one still at z = 3.9.

The statistic that removes seat entirely is the paired one. Summing the
challenger's two shares within a deal gives 1.0 under the null whatever the
seat advantage is:

* **paired per-deal share sum: 1.1475 ± 0.0182, null 1.0, z = 15.90**, 1600
  deals.
* Non-parametrically: the challenger **swept both seats of 234 deals and was
  swept in 12**, with 1318 split. A sign test on the 246 decided deals is
  p < 10⁻⁴⁰. This uses no distributional assumption and no seat correction at
  all.

#### The frozen-champion upside artifact, checked not asserted

A frozen-champion A/B systematically flatters a new feature that only ever
adds value: the other 78 weights were fitted without it, so the bot collects
the upside at full price and pays no downside. That is a real effect and it is
the reason to be suspicious of a positive here.

It does not apply to this feature, and the check is direct. `event_scoring_margin` is a
**signed difference** — `owed[me] − owed[best rival]` — so at 2p it is exactly
antisymmetric between the players. Measured over 244 non-zero observations in
8 self-play games:

| | share | mean |
|---|---|---|
| positive | 50.4% | +8.35 |
| negative | 49.6% | −7.67 |

Half the time the feature tells the bot a plant is *bad*, at the same
magnitude, and a negative weight-times-feature is exactly as capable of
losing an argmax as a positive one is of winning it. So the artifact is not
available to explain this result. What the artifact argument does still
correctly say is that **the champion was trained blind**, and the right
comparison — a champion retrained with the feature against one retrained
without — needs a league run and is not in this document.

#### What this does not establish

* **1-ply `WeightedBot`.** The league trains `plan:width=2`. Deeper search has
  more chances to reach the endgame by rollout, so the effect could shrink.
* **2p only.** No 3p transfer measured, for want of CPU, not for want of
  interest.
* **It does not price 40 of the 55 events.** See §6.

## 6. What is deliberately not priced, and why

**A wrong price is worse than a known zero**, and 40 of the 55 events are left
at zero on purpose. The reasons are per bucket, and written into
`weighted.py`'s `DELIBERATELY_UNPRICED` block as well as here.

### 6.1 The 17 rank-addressed Age I/II events — NOT PRICED

Barbarians, Border Conflict, Crusades, Foray, Immigration, Raiders, Reign of
Terror, Uncertain Borders (Age I); Civil Unrest, Cold War, Crime Wave,
Independence Declaration, International Agreement, National Pride, Politics of
Strength, Refugees, Terrorism (Age II).

These are the genuinely asymmetric events — `strongestPlayer` gains,
`weakestPlayer` loses — and they are the ones it is most tempting to price.
Two facts stop it:

* **They resolve against the ranking at reveal time, not at plant time.**
  `events._recycle_future_events` shuffles the future pile and pops it
  lowest-age-first, so a card seeded now surfaces at an unpredictable point at
  least one full recycle later. Pricing Crusades as "+4 to me because I am
  strongest today" asserts a rank ordering several rounds out. The bot has no
  model of the rival's future strength; `rival_context` is a snapshot.
* **The stakes are small.** The printed swings are ±3 to 4 culture, against
  the 10-40 of an Age III scoring event.

This is the subset that genuinely needs opponent modelling the bot does not
have. Building it on a current-board rank would be a confident guess about the
future dressed as a measurement.

### 6.2 The 23 symmetric `allPlayers` events — NOT PRICED

All 10 Age A "Development of ..." events, plus Cultural Influence, Good
Harvest, New Deposits, Pestilence, Rats, Rebellion, Scientific Breakthrough
(I) and Economic Progress, Emigration, Iconoclasm, Popularization of Science,
Prosperity, Ravages of Time (II).

These apply the same block to every player. They are not *exactly* a wash — a
player with more mines gains more from New Deposits — but the board-scaled
ones are the same problem as §6.1 (they fire later, against a different
board), and the flat ones (`+2 science` to everyone) are a genuine wash under
any margin. Pricing them would add coordinates with no gradient.

### 6.3 The 10 pacts — NOT PRICED AT 2p, AND NOT PRICEABLE THERE

`count 2p: 0`. They do not exist in a two-player deck. Any 2p A/B on them
would be measuring noise. At 3p/4p they are already priced by
`deferred_credit`, which reads inside the `A`/`B`/`bothPlayers` blocks and
values each side through the same weights. **Whether that pricing is any good
is an open question this lane did not answer**, and it is the natural next
piece of work; it needs a 3p or 4p experiment, and the 4p champion is
separately known to be degenerate.

### 6.4 The 11 aggressions and 3 wars — PRICED, BY RESOLUTION

See §2. Not a gap.

## 7. The obvious follow-ups, in the order I would do them

1. **Let the league tune the weight.** The feature ships at 0.0. Everything in
   §5 is the *frozen* champion handed better information, not a champion
   trained with it — the same caveat `docs/CARD_BLINDNESS.md` §5 makes about
   itself. Note §9 of that document: changing the evaluator invalidates the
   cached pool weights, so `last_full_check` must be deleted from
   `state_2p.json` before an arm restarts on this.
2. **Measure it under `plan:width=2`.** §5 is 1-ply `WeightedBot`, which is
   how `hand_potential` was measured and is the cheapest thing that exercises
   the feature. The league trains `plan:width=2`, and a deeper search has more
   chances to find the endgame by rollout, so the effect could shrink there.
3. **3p transfer.** Untested here purely for want of CPU. It should transfer
   for the same reason the `effects.culture` mapping did — "Impact of Wonders
   pays 5 culture per Age A wonder" is a fact about the card at every player
   count, not an opinion about a metagame.
4. **The neural encoder already reads `seeded_by`** (`neural_encode.py` emits
   `seeded_n` and `seeded_lv`, count and summed level of my unresolved seeded
   events). It does *not* have the scoring forecast, and giving it
   `final_event_culture` would be a strictly better signal than a count.
5. **Pacts at 3p/4p** — the one part of this lane genuinely left unexamined.
   See §6.3.

## 8. Reproducing

```bash
# the behavioural baseline and counter
python3 tools/event_plants.py --players 2 --games 20 --bot weighted \
    --weights analysis/events/champ2p_esm0.0.json
python3 tools/event_plants.py --players 2 --games 20 --bot weighted \
    --weights analysis/events/champ2p_esm1.0.json

# is it a dead coordinate?
python3 tools/feature_variance.py --players 2 --games 6 \
    --champ analysis/events/champ2p_esm1.0.json

# the guardrail: forecast == payout
python3 -m unittest tests.test_event_scoring

# the weight scan, then the paired A/B on the winner
for v in 0.25 0.5 1.0 2.0; do
  python3 -m experiments.evaluate --a analysis/events/champ2p_esm$v.json \
      --b analysis/events/champ2p_esm0.0.json \
      --players 2 --games 300 --seed 0 --workers 2
done

# the gate
bash tools/gate.sh
```

## 9. Two live bugs found on the way, not fixed here

Both are outside this lane and neither is touched by this change, but they
should not stay unrecorded.

* **`interact.py` `_c_pact_offer` assigns where it should append.**
  `owner.pacts = [{...}]` replaces the list, so accepting a pact silently
  destroys every other pact the owner already had in play. Reachable only at
  3p/4p, where a player can legitimately hold more than one.
* **`book.py`'s pact-offer response always accepts.** It reads
  `pend["ctx"]["from"]`, but `_h_offer_pact` writes `owner`, `name`, `a` and
  `b` — never `from` — so `partner` is always `None` and the "refuse if the
  partner leads by more than 5 culture" rule never fires.

Also worth knowing: `state.scoring_events` (`state.py:157`) is declared and
never read or written anywhere in the engine. Dead field.
