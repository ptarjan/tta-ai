# Why the league champion loses to `var:culture`

Date: 2026-07-26, working notes (branch `diag/culture-gap`).
Diagnosis only — **nothing is fixed here.**

Instrument: `tools/culture_probe.py` (new on this branch). Wraps every seat,
records what was legal vs what was picked, and re-scores the champion's own
1-ply evaluation of every attack it declined. All runs `TTA_JOURNAL=1`, seat
rotated, champion snapshots taken at 19:05 from `experiments/league_state/`
(2p gen 155, 3p gen 102, 4p gen 61) and frozen in `/tmp/champ{2,3,4}p_snap.json`
so the live trainers rewriting those files could not move the target mid-run.

**Error bars.** Every duel below is n=48, one opponent, seat-rotated. That is
±0.12-0.14 on a win rate and roughly ±12-15 culture points on a margin. Two of
the four headline experiments are null results *at that resolution* and are
labelled as such. Do not read a 0.04 win-rate difference as a result.

---

## 0. First, the brief's numbers are one fullcheck stale

The brief quotes 4p `var:culture` 0.0% / margin −86.6 and
`past:league_4p/gen00103` 27.1%. Those are the **gen 50** fullcheck (18:46).
The gen 60 fullcheck (18:59) reads:

| 4p opponent | gen 40 | gen 50 | gen 60 |
|---|---|---|---|
| var:culture | 0.042 / −97 | 0.000 / −87 | **0.229 / −27** |
| book | 0.104 / −28 | 0.104 / −30 | 0.229 / −0 |
| var:infra | 0.146 / −3 | 0.250 / +23 | 0.396 / +58 |
| var:tempo | 0.417 / +9 | 0.198 / +12 | 0.458 / +50 |
| past:league_4p/gen00103 | 0.167 / −20 | 0.271 / +7 | 0.302 / +14 |

The 0/48 shutout was real when it was measured and is no longer the state of
the arm. What *is* durable is section 1.

---

## 1. The culture-specific deficit is real, and 170 generations have not closed it

Rank of `var:culture` among the eight book/variant pool members, 1 = the
opponent the champion does worst against, across every fullcheck ever logged
in this run:

| arm | fullchecks | `var:culture` ranked worst | ranked worst-or-second |
|---|---|---|---|
| 2p (to gen 170) | 17 | 11 | 15 |
| 3p (to gen 110) | 11 | 5 | 11 |
| 4p (to gen 60) | 6 | 2 | 4 |
| **total** | **34** | **18** | **30** |

At 2p, where the arm is most mature, the last six fullchecks put `var:culture`
at ~0.29 against a pool mean of ~0.44 — a persistent ~15-point deficit that has
not narrowed since gen 60. **The search closes the gap against every other pool
member and does not close this one.** That is the signature of something the
evaluator cannot represent, not of a search that needs more time.

At 4p it is *not* culture-specific: at gen 60 the champion sits at ~null
against book (0.229), var:military (0.229), var:science (0.250) and
var:culture (0.229) alike. The 4p arm is behind the whole pool, not
specifically behind culture. See section 5.

---

## 2. Q1 — feature space or weight search? Both, and here is the split

### 2a. What the evaluator can actually see about aggression

`engine/bots/weighted.py:281-350` is the whole feature vector. The
military/rival terms are:

| feature | what it reads |
|---|---|
| `strength` | own strength |
| `strength_rel` | own − strongest rival |
| `strength_deficit` / `strength_lead` | `max(0,−rel)` / **`min(6, max(0, rel))`** |
| `tactic_level`, `best_unit`, `unit_workers` | army composition |
| `colonies`, `pacts`, `pact_blocks_attack` | |
| `auction_committed`, `auction_bid` | live colony auction (deferred-credited) |
| `hand_military`, `hand_mil_value` | count and summed level of the military hand |
| `rival_culture` | **max** rival culture |
| `rival_mean_culture` | mean rival culture |
| `rival_culture_rate`, `rival_science_rate`, `rival_strength` | best rival's rates |

There is **no feature that reads `p.war_declared_by_me` or
`p.wars_declared_on_me`**, and no per-rival vector — `rival_culture` is a
single scalar over the max. So:

* "I have a war standing against the culture leader" is **not representable at
  all**. It is a state field no weight touches.
* "attack *that* player" is representable only implicitly: an attack that
  actually moves culture in the trial state moves `rival_culture`. It does
  not need a per-rival feature, but it does need the payoff to be visible —
  and it is not (2b).
* "the culture leader is running away" **is** representable, via
  `rival_culture` / `rival_culture_rate`. The champion is not blind to it.

### 2b. Attacks are priced at exactly the cost of the card leaving hand

`docs/AGGRESSION_FIX.md` section B diagnosed this on 2026-07-26 and ends with
"**Fix (same shape as the pact/colony fix). See the next section for the
implementation and the A/B result.**" — *there is no next section.* The
document trails off, and `git log` shows one commit (`8d24aff`, the diagnosis).
`deferred_credit()` in `weighted.py:121` still handles only `pact_offer` and
`auction` pendings. The fix was never written.

Mechanism, unchanged from that document:

* `_h_aggression` (`engine/actions.py:973`) → `events.start_aggression`
  (`engine/events.py:480`) spends the MA, discards the card, and hands a
  `defense` pending to the target (`engine/interact.py:603`). Unless the
  defender has zero MA budget or an empty military hand — in which case
  `_finish_defense` resolves inline — the trial state the attacker scores shows
  only the cost.
* `_h_war` (`engine/actions.py:1037`) writes `war_declared_by_me` /
  `wars_declared_on_me` and nothing else. Spoils land in
  `events.resolve_war`, called from `game.start_turn` **one full turn later**.
  Zero features read those fields, so a war declaration is a pure cost with no
  representable benefit — a structural, not statistical, zero.

Measured, 4p champion vs 3×CultureBot, n=48 (`/tmp/probe_base48.json`):

| per game | champion | each CultureBot seat |
|---|---|---|
| politics decisions | 21.6 | 21.7 |
| **`war` legal** | **6.50** | 6.3 |
| **`war` played** | **0.00** | 0.49 |
| **`aggression` legal** | **7.35** | 8.7 |
| **`aggression` played** | **0.08** | 3.63 |
| positions where an attack existed | 10.56 | — |
| ...best attack outscored the chosen move | **0.083** | — |
| mean eval gap (chosen − best attack) | **+12.21** | — |

312 legal war declarations across 48 games; **zero taken**. 353 legal
aggressions (`legal_moves` already filters out ones that would fail —
`actions.py:281`, attacker strength must exceed the defender's — so these are
all *winnable*); 4 taken.

2p, n=48 (`/tmp/probe_base48_2p.json`): war legal 6.25/game, played **0.00**;
aggression legal 4.21/game, played **0.00**; in 405 probe positions the best
attack beat the chosen move **zero** times.

The eval gap is not merely negative, it is a *constant*. Sampled positions:

```
r14 chosen ('pol_pass',)                      1106.221 | best attack ('aggression','Plunder (II)',1)  1105.997 | pol_pass 1106.221
r17 chosen ('pol_pass',)                      1264.748 | best attack ('aggression','Raid (II)',1)     1264.524 | pol_pass 1264.748
r19 chosen ('pol_pass',)                      1572.357 | best attack ('aggression','Plunder (III)',1) 1572.132 | pol_pass 1572.357
r20 chosen ('prepare_event','Impact of Colonies') 1660.526 | best attack ('aggression','Plunder (III)',1) 1631.900 | pol_pass 1632.125
```

Every attack scores **exactly 0.224-0.225 below `pol_pass`**, and the 4p
champion's `hand_military` weight is **0.224** with `hand_mil_value` and
`ma_left` both 0.000. The delta *is* the card leaving the hand. The payoff term
is not small — it is identically zero.

**Verdict on Q1: the war channel is a feature-space hole, exactly as
`AGGRESSION_FIX.md` said and never fixed. No amount of hill climbing can find
a weight that makes `war` attractive, because the vector has no coordinate that
moves when a war is declared.** But see section 4 — closing it is not by
itself sufficient, and may not even be worth much.

### 2c. A second, independent defect: `rival_culture` has an inverted sign at 4p

| weight | default | 2p (gen 155) | 3p (gen 102) | **4p (gen 61)** |
|---|---|---|---|---|
| `rival_culture` | **−0.35** | +0.008 | +0.135 | **+5.611** |
| `rival_mean_culture` | −0.10 | +0.079 | −0.047 | −1.459 |
| `culture` (FROZEN) | 1.00 | 1.00 | 1.00 | 1.00 |

`culture` is in `hillclimb.FROZEN`, so every weight is denominated in culture
points. `rival_culture = +5.611` therefore reads literally: *one culture point
in the leading rival's hands is worth +5.6 of my own.*

Measured on a real round-24 4p position (cultures `[354, 215, 175, 229]`,
champion in seat 0, leader seat 3): moving 10 culture from the leader to the
champion changes the champion's own evaluation by

```
eval before 3241.621   after stealing 10 culture from the leader 3200.369   delta -41.252
```

and that is exactly the linear arithmetic: `10×1.000` (own culture)
`− 10×5.611` (max rival culture) `+ 3.333×1.459` (mean rival culture) `= −41.25`.

**The 4p champion scores winning a War over Culture as a large loss.** Even if
2b were fixed and the payoff were visible, this weight would make the champion
*decline* it — and, symmetrically, actively prefer `prepare_event` choices that
pay the culture leader.

#### How it got there, and why nothing caught it

`ladder_4p` archives the accepted champion each generation:

```
gen   rival_cult  rival_mean  culture_rate
00029     -0.321      -0.100        4.321
00030     +5.611      -0.175       12.837      <- op = "kick", edge +0.0291
```

One accepted generation. `generations_4p.jsonl` gen 30 records
`op: "kick"` — `hillclimb.mutate`'s deliberate big restart, which perturbs 60%
of the vector at `sigma*3`. It moved `rival_culture −0.3207 → 5.6114`,
`culture_rate 4.32 → 12.84`, `rival_strength −0.15 → −0.297` in one bundle and
was accepted on an aggregate edge of +0.029. The sign flip was never
individually evaluated; it rode along.

`hillclimb_league.guard_weights` exists precisely to catch this class (its
docstring cites the old `science = −6.089` collapse). It cannot catch this one:

```python
NONNEG = frozenset(k for k, v in DEFAULT_WEIGHTS.items() if v > 0)
```

It only clamps weights whose **default is positive**. Its own comment says
"Every legitimately negative term already has a negative default and is
therefore untouched — `rival_*` … included." *Untouched* is the bug: the guard
is one-sided, so **25 of the 78 weights** — every one with a negative default —
can invert freely and nothing logs a thing. The full unprotected set is
`auction_bid, consumption, corruption_loss, culture_early, culture_rate_late,
discontent, end_turn_bias, food_rate_late, hand_value_late, pop_cost,
resource_rate_late, rival_culture, rival_culture_rate, rival_mean_culture,
rival_science_rate, rival_strength, science_rate_late, strength_deficit,
strength_rel_early, tech_levels_late, uprising, wonder_progress_late,
wonder_remaining, workers_late, yellow_bank`.

(`end_turn_bias` is the one entry that would need an explicit exemption rather
than a clamp — `weighted.py` and `docs/WASTED_ACTIONS.md` §6 measured five ways
that it must stay negative and must not be "fixed", so a two-sided guard would
be *protecting* it, not breaking it. `strength_deficit` and `discontent`
likewise: a positive value there means "being behind is good", which is the
same inversion class.)

Note all three arms have lost the negative `rival_culture` (2p +0.008, 3p
+0.135, 4p +5.611). Only 4p made it large.

---

## 3. Q3 — engine bug check: no bug found in the war/aggression path

Taken seriously, not as a formality. Three checks:

**(a) War over Culture resolves exactly per the rulebook.** Direct engine
probe (script inlined, not committed):

```
equal strengths            -> no transfer                        (RULES_SPEC 5.7.2 "Equal strength = no effect")
attacker +4 strength       -> 9 culture moved, 40 -> 31 / 5 -> 14 (5 + advantage)
defender +4 strength       -> 9 culture moved the OTHER way       ("EITHER side can win")
```

`events.resolve_war:587` is `take = min(5 + adv, loser.culture)`, matching
`docs/RULES_SPEC.md` §5.8 "5 + advantage culture points (capped at what the
victim has; victim cannot go negative)" and `data/cards_military_actions.json`
(`"victorTakesCulture": {"base": 5, "plus": "strengthAdvantage"}`, Age III,
3 MA, 6 copies). `resolve_war` is called from `game.start_turn:221` at the
start of the attacker's next turn, per §5.7.

**(b) Declaring is legal against a culture bot.** `actions.legal_moves:285`
gates war on `cost <= p.military_actions`, `not state.last_round`, no existing
`war_declared_by_me`, and `effects.war_forbidden` (pact or `war_immune` only —
correctly *not* restricted by relative strength, §5.6). It fires 6.5×/game at
4p and 6.25×/game at 2p in the measured runs. The champion can legally do the
thing; it chooses not to.

One deviation, in the *conservative* direction: `legal_moves:281-283` refuses
to emit an aggression whose defender's strength already equals or exceeds the
attacker's, i.e. one that would fail. The rulebook lets you play a doomed
aggression. This removes only strictly-losing moves and cannot explain
anything here — but it does mean the "7.35 legal aggressions/game" above are
all winnable ones.

**(c) Is `culture.py` exploiting over-credited culture?** No evidence.
Production values match card text (Drama 2 culture, Opera 3, Movies 4). The
Age III `Impact of …` scoring in `events.scoring_culture` was read key by key
against `data/cards_military_actions.json`; the three CultureBot's docstring
names all check out, including the awkward ones — `culturePerHappyFace: 2` with
`maxCultureFromHappyFaces: 16` applies the cap
(`events.py:426-430`), `culturePerDiscontentWorker: -2` is applied,
`culturePerLevelOfUrbanBuildings` is `level × workers`. Measured split of the
final score, 4p, n=12: end-of-game Age III scoring is 54.2 for the champion and
**42.0** for each CultureBot seat — the culture bot gets *less* out of the
scoring events than the champion does. It is not winning on an end-scoring
exploit.

**Verdict on Q3: no engine bug. This is a strategy/representation gap.**

---

## 4. Q2/Q4 — two counterfactuals, both null at n=48. Read this before ranking fixes

Paired against `base48` (identical seeds, 4p, n=48):

| run | rival_culture | attack policy | win rate (null 0.25) | culture margin | wars/game | aggr/game |
|---|---|---|---|---|---|---|
| `base48` | +5.611 (as trained) | the champion's own | 0.208 | **−45.1** | 0.00 | 0.08 |
| `patch48` | **−0.35** (default restored) | the champion's own | 0.146 | **−48.2** | 0.00 | 0.17 |
| `force48` | +5.611 | **oracle: always attack the culture leader when legal** | 0.167 | **−46.4** | 2.08 | 1.90 |

and at 2p (n=48, null 0.50):

| run | attack policy | win rate | culture margin | wars/game | aggr/game |
|---|---|---|---|---|---|
| `base48_2p` | the champion's own | 0.271 | **−17.8** | 0.00 | 0.00 |
| `force48_2p` | **same oracle** | 0.354 | **−23.8** | 2.77 | 2.48 |

* Restoring `rival_culture` to its default **did not help**. It did what the
  arithmetic predicts to the evaluation — mean eval gap between the chosen move
  and the best attack fell from 12.21 to 4.35, and aggressions doubled — but the
  win rate and margin moved less than one standard error. **Null result, not a
  refutation**: ±0.12 cannot see anything smaller than a 12-point swing.
* An oracle that declares war (else an aggression) on the current culture
  leader every single time it is legal — i.e. the exact behaviour
  `culture.py`'s docstring predicts should punish it — **also did not help**
  (−46.4 vs −45.1). It attacked 4.0×/game and gained nothing.

The same oracle at 2p (n=48, `/tmp/probe_force48_2p.json` vs
`/tmp/probe_base48_2p.json`): win 0.354 vs 0.271, margin **−23.8 vs −17.8**.
Win rate nominally up, margin nominally down, both well inside ±0.19 / ±15 —
a third null. Note *why* the margin moves the wrong way: the 2p champion's
strength is **2.46 against CultureBot's 3.35**, so the 2.77 wars/game the
oracle forces are wars it *loses*, handing 5+advantage culture to the culture
bot. A punish policy needs an army first, and the 2p champion does not have
one (§4, last paragraph).

The oracle bounds a *crude* policy, not a learned one: it spends the politics
phase on attacks instead of `prepare_event`, which the probe shows is worth
+8 to +28 eval points a turn, and it declares wars at a strength advantage of
~+1.5 where War over Culture pays only 5+1. A learned valuation would be
strictly better than both "never" and "always". But the honest read is that
**`culture.py`'s stated weakness is not, at these strength levels, the lever
the docstring claims it is,** and adding a war feature should not be sold as
the fix for the culture matchup.

### What the champion actually loses on: the middle-game culture race

4p, n=16, sampled every 3 rounds:

| round | culture champ/opp | culture **rate** champ/opp | urban workers champ/opp |
|---|---|---|---|
| 6 | 6.2 / 7.8 | 1.69 / **2.17** | 2.94 / 2.96 |
| 9 | 17.4 / 22.0 | 2.69 / **3.77** | 4.12 / 3.46 |
| 12 | 32.2 / 40.4 | 3.50 / **4.85** | 4.69 / 3.98 |
| 15 | 48.3 / 61.2 | 4.38 / **5.88** | 5.50 / 4.62 |
| 18 | 69.6 / 83.0 | **6.94** / 6.88 | 6.19 / 4.62 |
| 21 | 112.6 / 124.5 | **8.44** / 7.44 | 6.81 / 4.60 |

CultureBot leads on culture *rate* from round 6 to round 15 and the champion
only overtakes at round 18 — with three rounds left to collect it. The
champion has **more** urban workers throughout (6.81 vs 4.60 at round 21) and
less culture from them: it is staffing labs/libraries, not theatres. It buys
the right asset too late and the wrong urban mix.

The evaluator's only handle on "a rate is worth what remains to collect" is
`lateness() = min(1.0, C.level(state.age_civil)/3.0)` — a **four-step function
of the civil deck's age, saturated at 1.0 from Age III onward**. Age III and
Age IV price a culture rate identically, which is exactly the stretch where
the true value collapses from ~8 turns' worth to ~1. At 4p the champion has
`culture_rate = 35.574` with `culture_rate_early = 0.000` and
`culture_rate_late = −0.316`: it has thrown away even the coarse shaping the
default vector ships with (+2.0 / −2.0), and pays 35.6 culture points for a
+1 rate on the last turn of the game. That mispricing is *partially*
representable (the two phase weights) but not at the resolution where it
matters, and the search has drifted away from using it at all.

At 2p the shape is different and worth recording: the champion is ahead on
rate through round 12 and is overtaken at round 15, and its army is **behind**
CultureBot's (strength 2.50 vs 3.40, n=48) despite CultureBot running
`mil_stance: "floor"`. The 2p champion's `unit_workers` weight is 0.000 and
`strength` is 0.118, while `strength_lead` — capped at 6 — carries a weight of
**6.392**, i.e. a 38-culture-point prize it can never collect because every
individual step towards it (a worker on Warriors) is priced at ~0.1. That is
a textbook non-convex trap for a 1-ply linear evaluator, and it means the 2p
champion could not punish a culture rush even if it wanted to.

---

## 5. Q4 — is the 4p arm plateaued? No, and the evidence for "plateau" was misread

`past:league_4p/gen00103` is **not the champion's own recent past self.**
`experiments/league_4p/` has an mtime of 11:01, hours before this run launched
at 16:29; it is the archive of an *earlier, longer* run that reached gen 103.
The current run's own archive is `experiments/league_state/ladder_4p/`, whose
latest entry is `gen00064`. So "27.1% against gen00103" is a gen-50 champion
losing to a *more trained* champion from a previous run, not a bot failing to
beat itself.

Against its true starting point, `past:ladder_4p/gen00000`, the 4p champion is
at **0.979**. Against `league_4p/gen00103` it went 0.000 (gen 10) → 0.021
(gen 20) → 0.167 (gen 40) → 0.271 (gen 50) → **0.302 (gen 60)**.

Accept rates from `generations_Kp.jsonl`:

| arm | generations | accepts | rate | accepts in last 20 gens |
|---|---|---|---|---|
| 2p | 168 | 24 | 14.3% | 4 |
| 3p | 110 | 22 | 20.0% | 3 |
| **4p** | **66** | **17** | **25.8%** | **3** |

The 4p arm has the **highest** accept rate of the three and the fewest
generations (4p games are the slowest, so it gets fewer per hour). Between
gen 50 and gen 60 it improved against every single pool member. It is the
least converged arm, not a converged one.

---

## 6. Ranked candidate fixes

Confidence is in **the claim that the change is correct and worth making**;
the separate "expected win-rate return" column is what I would bet on it
moving the `var:culture` matchup, and they are deliberately not the same
number.

| # | change | correctness confidence | expected return vs culture | cost |
|---|---|---|---|---|
| 1 | **Make `guard_weights` two-sided**: clamp any weight whose sign is opposite its `DEFAULT_WEIGHTS` sign, not just those with a positive default. | **high** — exact, non-noisy evidence (§2c) | **low** — measured null at n=48 (§4) | ~3 lines |
| 2 | **Make rate features scale with turns remaining**: either replace `lateness()`'s 4-step age proxy with a continuous rounds-remaining fraction, or add `culture_rate × rounds_left` as its own feature. | medium-high — matches the measured rate curves (§4) and the Age III/IV saturation is plainly wrong | **medium-high** — this is the axis the champion actually loses on | small feature change + full re-measure |
| 3 | **Close the war/aggression representation hole** (finish `AGGRESSION_FIX.md` B): deferred credit for a pending `defense`, and features for `war_declared_by_me` / `wars_declared_on_me` priced by the spoils formula. | **high** — the hole is proven and exact (§2b) | **low-medium** — the forced-attack oracle gained nothing (§4), but it bounds a crude policy only | medium; new features + climb restart |
| 4 | **Per-rival targeting**: `rival_culture` is a single max. Nothing distinguishes "the leader is 80 ahead" from "two rivals are tied 5 ahead". A `culture_lead_over_me` / `runaway_leader` feature would let the climb learn a threshold. | medium | medium | medium |
| 5 | **Break the 2p military trap**: `strength_lead` is capped at 6 with weight 6.392 that the bot can never earn because `unit_workers`=0.000 and `strength`=0.118. Either uncap, or credit the *first* unit worker specially. | medium | low-medium (2p only) | small |
| 6 | **Do nothing to 4p; let it run.** It is the least-converged arm with the highest accept rate (§5). | high | n/a | free |

### The single change I would make first: #1, the two-sided weight guard

Not because it is the biggest lever — §4 measured it as a null at n=48 and I
am not going to round that up. Because:

* it is the **only defect here I have exact, noise-free evidence for**: a
  10-culture theft off the culture leader scores −41.25 to the 4p champion,
  reproduced to three decimal places by the linear arithmetic;
* it is a **recurrence-prevention** fix, not a tuning fix. The guard already
  exists, was written for exactly this failure class after the
  `science = −6.089` collapse, and is one-sided by accident. Every future
  `kick` can invert any of the eleven legitimately-negative weights and
  nothing will log it. Leaving it means the next inversion is a coin flip;
* it costs three lines and no re-measurement of the engine;
* it does not depend on any of the noisy results above being right.

Two caveats to carry into the implementation:

1. **Do not hot-patch the live 4p champion.** The n=48 counterfactual that set
   `rival_culture` back to −0.35 scored 0.146 vs the champion's 0.208 — within
   noise, but not obviously an improvement. Apply the guard to newly proposed
   mutants and let the ladder re-derive, or gate the champion edit behind the
   league's own ablation machinery.
2. **Settle the counterfactual properly before spending effort on #3 or #4.**
   The repo already has the right instrument: `experiments/league_state/
   ablation_4p.jsonl` and the `--ablate` path in `hillclimb_league.py`. An
   ablation of `rival_culture` at n≥200 would replace my null result with an
   answer. My n=48 runs cannot distinguish a 10-point effect from zero, and I
   would not want #3's cost paid on the strength of them.

---

# Part 2 — implementing fixes #1 and #2 (2026-07-26, branch `fix/rate-horizon`)

Working notes, appended by the implementation agent. Worktree `/tmp/tta-ratefix`,
branched off master `f3f4352`, one commit per fix, nothing pushed or merged.
The three live trainers in the main checkout were not touched.

Everything below is `TTA_JOURNAL=1`. Champion vectors are the same 19:05
snapshots Part 1 froze (`/tmp/champ{2,3,4}p_snap.json`, gen 160/106/62) so the live trainers
could not move the target mid-run.

## 7. Fix #1 — the two-sided weight guard (commit `7ab57c7`)

`NONNEG` is joined by `NONPOS`, the **15** negative-default *value* terms:
`auction_bid, consumption, corruption_loss, discontent, end_turn_bias,
pop_cost, rival_culture, rival_culture_rate, rival_mean_culture,
rival_science_rate, rival_strength, strength_deficit, uprising,
wonder_remaining, yellow_bank`. A weight in either set may not cross zero.

### The 10 that are exempt, and why

Part 1 listed 25 unprotected negative-default weights. Ten of them are the
early/late **phase multipliers** (`culture_early`, `culture_rate_late`,
`science_rate_late`, `food_rate_late`, `resource_rate_late`, `workers_late`,
`strength_rel_early`, `tech_levels_late`, `wonder_progress_late`,
`hand_value_late`) and they are deliberately left free. The argument is not
"they might be strategies", it is that **their sign is not gauge-invariant**:

    contribution = (w[k] + (1-L)*w[k_early] + L*w[k_late]) * v

Add `c` to both phase weights and subtract `c` from the base and you have the
*identical policy*, so `culture_rate_late` can be driven positive without
moving a single decision. "The sign flipped" therefore carries no information
about whether the policy inverted, which is the only thing the guard is for.
What a phase weight's sign *does* encode is a hypothesis about **when** a thing
matters — `strength_rel_early < 0` says "being ahead on army early is not worth
paying for" — and the search is entitled to disagree with the default.

Part 1 also nominated `strength_deficit` and `discontent` for exemption. They
are **not** exempt here and should not be: those are value terms, a positive
value really does read "being behind / being unhappy is good", and that is the
same inversion class as `rival_culture`. `end_turn_bias` is likewise locked,
and locking it is *protecting* it — driving it positive is the "pass MORE"
regression measured at 11.0% in `docs/WASTED_ACTIONS.md` §6.

**Left alone on purpose:** the ten *positive*-default phase multipliers are
still clamped one-sided, exactly as today. By the gauge argument those clamps
are equally meaningless, and they fire often in the live logs (`culture_late`
27 times at 3p, `wonder_progress_early` 28, `culture_rate_early` 15 at 4p) —
but dropping them *loosens* a constraint the live run is currently training
under, which is a search change, not a correctness fix, and wants its own
separately-measured commit. Flagged, not done.

### Recurrence test

`tests/test_weight_guard.py`, 8 cases. Against the one-sided guard **5 of the 8
fail** (the imports are resolved defensively so the failures are assertions
naming the unprotected weight, not an `ImportError`).

## 8. Fix #2 — the horizon (commit `ba9ad71`)

`lateness()` was `min(1.0, C.level(state.age_civil) / 3.0)`. It is now an
affine function of **estimated rounds remaining**.

### 8a. What the engine actually knows about "remaining"

There is no fixed turn count. The game ends when the Age III civil deck runs
out: `game._deal` calls `_advance_age` the moment the last card leaves the
deck, Age IV has no deck, and `_set_last_round` pins `final_round_end` to this
round or the next depending on whether the current player is the start player
(§12.2/12.3). So

* once `state.final_round_end` is set, remaining rounds are **exact**;
* before that, **cards still to be dealt is exact** — `len(state.civil_deck)`
  plus every later age's deck, whose sizes come from the card data and the live
  player count (A/I/II/III = 20/44/44/44 at 2p, 20/50/50/50 at 3p, 20/53/53/53
  at 4p, trimmed on resignation per §13);
* the **only** estimated quantity is the rate the row eats them. `SWEEP[n]`
  cards are swept and redealt per *player-turn* (3/2/1 for 2/3/4p, so 6/6/4 per
  round) which is exact; the rest is cards players take off the row, which is
  policy-dependent. That is the guess, and it is the only guess.

Calibrated and validated on 46 `WeightedBot` self-play games (20 at 2p, 14 at
3p, 12 at 4p), recording every seat-0 turn and the round the game actually
ended. `CARDS_PER_ROUND = {2: 6.29, 3: 6.73, 4: 5.71}`, `AGE_IV_ROUNDS = 2.0`:

| | n decisions | bias | resid sd | mean abs err | worst |
|---|---|---|---|---|---|
| 2p | 434 | −0.21 | **0.68** | 0.76 | 2.1 |
| 3p | 317 | −0.25 | **1.00** | 0.93 | 2.8 |
| 4p | 327 | +0.03 | **1.13** | 0.93 | 3.1 |

all in rounds. For comparison the age bucket it replaces cannot beat **2.72
rounds** of residual sd at 4p *even if you hand it the per-age mean* — it is a
4-level predictor of a quantity that ranges 2..31. Per age at 4p the estimate's
sd is 2.40 (A) / 1.34 (I) / 1.08 (II) / **0.53 (III)** / 0.06 (IV): tightest
exactly where the defect was.

Measured game lengths, for the record: 2p 21–23 rounds (mean 22.7), 3p 22–25
(23.6), 4p 28–31 (28.9). Rounds left by age, 4p: A 28.9, I 24.9, II 15.9,
**III 6.4, IV 2.0** — the two the old function could not tell apart.

Honest limits, all stated in the code: it is calibrated on WeightedBot
self-play and a much more card-hungry policy would drain the row faster and
make this run long; it treats the 7 leftover Age A cards as dealable when they
are actually discarded; and an `end_turn` candidate is scored ~0.2 rounds
"later" than its siblings because `apply` runs the next player's replenish,
worth ~0.1 evaluation points against an `end_turn_bias` of −3.0.

### 8b. Choosing the constants: the gauge is free, so spend it on calibration

The phase blend is `w[k] + (1-L)*w[k_early] + L*w[k_late]`, so **any affine
change of L is pure gauge** — absorbable into (base, early, late) with no
decision moved. Only the non-affine part is real, and that part is the entire
fix, because no affine function of rounds-left can be flat inside an age.

The gauge is therefore spent on disturbing the trained champions as little as
possible: `_L_ZERO`/`_L_ONE` make the new L the least-squares best
linear-in-rounds-left approximation of the *old* L, fitted per player count
over the same 1078 decisions. `_L_ONE` came out 5.00 / 5.17 / 5.05
independently at 2p/3p/4p and is rounded to one constant 5.0 — the old
schedule's "late" was, in effect, "about five rounds from the end".

Resulting L, measured on the same decisions (old value → new mean [range]):

| age | 2p | 3p | 4p |
|---|---|---|---|
| A | 0.00 → 0.14 | 0.00 → 0.14 | 0.00 → 0.16 |
| I | 0.33 → 0.35 [0.21–0.50] | 0.33 → 0.36 [0.21–0.49] | 0.33 → 0.37 [0.23–0.49] |
| II | 0.67 → 0.66 [0.50–0.81] | 0.67 → 0.66 [0.50–0.81] | 0.67 → 0.65 [0.50–0.79] |
| III | 1.00 → 0.95 [0.82–1.00] | 1.00 → 0.94 [0.81–1.00] | 1.00 → 0.95 [0.81–1.00] |
| IV | 1.00 → 1.00 | 1.00 → 1.00 | 1.00 → 1.00 |

Pooled shift: bias **−0.007**, sd **0.088**. Ages I–III keep their old *mean*
to within 0.05 and gain a real within-age spread — early Age III (11 rounds
left) is now 0.81 where Age IV is 1.00, a separation the old function did not
have at all. Age A is the one systematic offset (0 → ~0.14).

### 8b(i). L is clamped to [0, 1] — a safety fix, and a hypothesis that failed

The first cut shipped an **unclamped** line, on the reasoning that with 2 rounds
left `L ≈ 1.1` is the honest continuation rather than a fifth flat step. That is
wrong on principle: `1 - L` going negative **flips the sign of every `_early`
term**, and the champions carry large ones. On the 4p champion the arithmetic is
exact — `culture_early` is 8.792, so at `1 - L = −0.096` its own culture, the
FROZEN weight the whole evaluation is denominated in, is priced at
`1.000 − 0.096 × 8.792 = 0.156` instead of 1.000. It stops caring about the
score in the last two rounds. A linear evaluator does not extrapolate outside
the range it has been scored on, so the clamp stays and
`test_never_leaves_the_unit_interval` pins it.

**But it was not the cause of anything.** I predicted the sign flip was driving
the champion regression in 8d and re-ran the whole battery to check. It is not,
to three significant figures:

| head-to-head, n=400 | unclamped | clamped |
|---|---|---|
| 3p champion | 13.6% ± 3.4% | **13.9% ± 3.4%** |
| 4p champion | 19.9% ± 3.9% | **20.1% ± 3.9%** |
| 2p champion | 49.6% ± 4.9% | **49.9% ± 4.9%** |

Recorded because a plausible mechanism with exact arithmetic behind it turned
out to explain none of the effect, and that is worth knowing before anyone
reaches for it again. Every number in 8d below is the clamped version.

### 8c. How much of each champion this can possibly move

The horizon can only move an evaluation through the phase weights, and its
whole leverage is `dL * |w_late - w_early|` summed over `PHASE_KEYS`. That sum,
per vector:

| vector | Σ\|late−early\| | where it sits |
|---|---|---|
| DEFAULT_WEIGHTS | **16.9** | culture_rate 4.0, science_rate 5.0, culture 1.9 |
| champion 2p (gen 155) | 7.5 | science_rate 1.9, tech_levels 1.7, strength_rel 0.9 |
| champion 3p (gen 102) | 7.5 | **culture_rate 3.9**, tech_levels 1.3 |
| champion 4p (gen 61) | **31.0** | science_rate 11.3, culture 8.8, food_rate 7.4 |

Note what is *not* there: the 2p and 4p champions have driven
`culture_rate_early/late` to (0.415, 0.148) and (0.000, −0.316) against a
default of (+2.0, −2.0), on base weights of 20.1 and 35.6. **The two arms that
lose the culture-rate race have thrown away the horizon shaping entirely and
price a culture rate as a constant.** That is the search declining to use a
representation it had; giving it a better one is necessary but plainly not
sufficient.

### 8d. A/B — head-to-head, new horizon vs old, same weights, same table

`horizon_age` in a weight file restores the old schedule for that vector alone,
so this is a direct duel at one table, not two runs compared across processes.
The pool bots are `BookBot` subclasses and never call `lateness()`, so nothing
here is confounded by the opponents changing too. n=400, seat-rotated,
`experiments/evaluate.py`, error bars 95%.

| table | vector | NEW win rate | null | p | culture NEW vs OLD |
|---|---|---|---|---|---|
| 2p | DEFAULT_WEIGHTS | 47.2% ± 4.9% | 50.0% | 0.27 | 103 vs 106 |
| 3p | DEFAULT_WEIGHTS | **38.8% ± 4.8%** | 33.3% | **0.026** | 120 vs 111 |
| 4p | DEFAULT_WEIGHTS | **32.5% ± 4.6%** | 25.0% | **0.0013** | 148 vs 127 |
| 2p | champion 2p | 49.9% ± 4.9% | 50.0% | 0.96 | 110 vs 109 |
| 3p | champion 3p | **13.9% ± 3.4%** | 33.3% | **<0.0001** | 54 vs 147 |
| 4p | champion 4p | **20.1% ± 3.9%** | 25.0% | **0.015** | 55 vs 73 |

Two opposite results, and both are real:

* **On the hand-designed default vector the fix is a clear win at 3p and 4p**
  (+5.5 and +7.5 points over the null, +9 and +21 culture) and a null at 2p.
  4p is where Part 1 located the culture gap and 4p games are the longest
  (~29 rounds), which is where the age bucket is most wrong; 2p games are
  ~23 rounds and the two schedules disagree least.
* **On the trained 3p and 4p champions the fix is a large loss.** The 2p
  champion, whose phase weights are small and carry almost no culture-rate
  shaping, does not notice it at all.

Also measured, paired against the pool with `tools/horizon_ab.py` (n=200 per
opponent, on the *unclamped* build — see 8b(i), which found clamping moves the
head-to-heads by <0.3 points, so read these as indicative): 4p champion pooled
win edge −0.012 ± 0.041 but **culture-margin edge −24.1 ± 9.6** (var:culture
−19.2 ± 14.3, book −29.1 ± 13.0); 4p DEFAULT pooled margin edge +1.8 ± 4.6,
a null. Same story: the champion is hurt, the default vector is not.

### 8e. Is the champion loss the fix, or just "any change hurts a hill-climbed
vector"?  Controlled: it is the fix.

Three controls, all with the OLD horizon on **both** sides so the only variable
is the weight vector, and all sized against the horizon change by the fraction
of the champion's own decisions whose argmax moves (measured by shadow-scoring
every decision of 6 full games under both vectors, 1858 champion decisions):

| 3p, n=400 | argmax moved | win rate | null | p |
|---|---|---|---|---|
| identical vectors (machinery check, n=120) | 0.0% | **33.3% ± 8.4%** | 33.3% | 1.00 |
| phase weights × 0.90 | 1.3% | 33.8% ± 4.6% | 33.3% | 0.86 |
| phase weights × 0.75 | **3.0%** | 32.2% ± 4.6% | 33.3% | 0.64 |
| **the new horizon** | **2.6%** | **13.9% ± 3.4%** | 33.3% | **<0.0001** |

and at 4p, phase weights × 0.90 gives 25.9% ± 4.3% against a 25.0% null
(p=0.69) where the new horizon gives 20.1%.

A control that moves **more** of the champion's decisions than the horizon does
is a flat null. So this is not "hill-climbed vectors are knife-edged and any
perturbation hurts". The horizon change moves a *specific* 2.6% of decisions
and they are specifically worse for a vector trained against the step function.

Where they move, at 3p: Age A 0.0%, Age I 3.3%, Age II 3.1%, Age III 1.8%,
Age IV 0.0% — i.e. in the middle game, where |ΔL| is largest (0.077 / 0.081),
not at the Age III/IV boundary the fix was aimed at. The champions have fitted
their phase weights to the *shape of the four steps*, and the steps are what
moved.

### 8f. Are existing champions invalidated? Yes — 3p and 4p, decisively

This is the adoption question and the answer is not comfortable.

* **champion 2p — not invalidated.** 49.9% against a 50.0% null, n=400.
* **champion 4p — invalidated.** 20.1% against 25.0%, p=0.015, and −24 culture
  margin against the pool.
* **champion 3p — invalidated badly.** 13.9% against 33.3%, culture 54 vs 147.

The reason is visible in 8c: the champions have trained their phase weights
against the exact old step function, and two of the three have simultaneously
*abandoned* the culture-rate shaping the fix is meant to improve
(`culture_rate_early/late` = (0.415, 0.148) at 2p and (0.000, −0.316) at 4p
against a default of (+2.0, −2.0)). Handing them a better-shaped L while
holding their weights fixed moves them off their optimum without giving them
anything, which is exactly what the numbers say.

**So fix #2 cannot be dropped into the running arms.** It is a change to what
the trained weights mean, and the vectors that mean it are the ones that lose.
On the vector a clean restart actually begins from — `DEFAULT_WEIGHTS` — it is
worth +7.5 points at 4p and +5.5 at 3p, measured at n=400.



## 9. Fix #1's strength effect, settled at n=400

§6 asked for exactly this and could not afford it: "an ablation of
`rival_culture` at n≥200 would replace my null result with an answer."

`/tmp/ab/guard_ab.py` — the same `hillclimb_league._series` pairing `ablate()`
uses, 4p champion snapshot, 200 games each against `var:culture` and `book`,
paired on identical seeds:

| `rival_culture` +5.611 → | opponent | win NEW vs OLD | win edge | culture margin edge |
|---|---|---|---|---|
| **0.0** (what the guard does) | var:culture | 0.150 vs 0.150 | +0.000 ± 0.034 | +1.01 ± 4.87 |
| | book | 0.223 vs 0.198 | +0.025 ± 0.035 | +4.95 ± 4.24 |
| | **pooled n=400** | | **+0.013 ± 0.024** | **+2.98 ± 3.23** |
| **−0.35** (the default) | var:culture | 0.145 vs 0.150 | −0.005 ± 0.033 | −0.23 ± 4.91 |
| | book | 0.217 vs 0.198 | +0.020 ± 0.039 | +3.88 ± 4.29 |
| | **pooled n=400** | | **+0.008 ± 0.025** | **+1.83 ± 3.26** |

**Answer: a null, and now a tight one.** Both point estimates are positive on
both metrics and both confidence intervals cover zero. Part 1's n=48 could not
distinguish a 12-point swing from nothing; this bounds the effect of undoing
the inversion at **±0.024 win rate and ±3.2 culture points**, roughly 4× tighter.

Read it as "the clamp is free", not as "the clamp helps". That is exactly what
a recurrence-prevention fix should measure, and §6's own ranking predicted it:
high correctness confidence, low expected return. Note also that the clamp to
0.0 and the restore to the −0.35 default are indistinguishable from each other,
so the guard's choice of 0.0 costs nothing.

## 10. Recommendation

**Fix #1 (two-sided `guard_weights`) — land it on master now.** It changes no
bot's play; its only effect is on which mutants the trainer will accept from
here on. Measured effect on the one champion that already carries an inversion:
null at ±0.024 win rate / ±3.2 culture points, n=400. The defect it prevents is
exact and reproduced to three decimals. It costs ~15 lines, carries a
recurrence test that fails 5 ways against the old guard, and closes a hole
through which 25 of 82 weights could invert unlogged. The one judgement call in
it — exempting the ten phase multipliers, because their sign is not
gauge-invariant — is argued in §7 and in the code, not silent.

*Caveat carried forward from Part 1 §6:* landing it does **not** repair the
running 4p champion, whose `rival_culture` is already +5.611. The guard only
runs at champion load and on new mutants, so the arm will clamp it to 0.0 the
next time the supervisor restarts the climber, which is hourly. Given §9 that
is a free move, but it is a move — expect a `guard_4p.jsonl` entry and a
champion whose `rival_culture` is 0.0 rather than +5.611 within the hour.

**Fix #2 (turns-remaining horizon) — do not land it into the running arms.
Land it at the next clean restart.** The change is right (the age bucket is a
4-level predictor of a quantity that ranges 2 to 31 rounds, and the replacement
is accurate to ~0.9 rounds), and on the vector a clean run actually starts from
it is worth **+7.5 ± 4.6 points at 4p (p=0.0013)** and **+5.5 ± 4.8 at 3p
(p=0.026)**, null at 2p. But it invalidates the 3p and 4p champions — 13.9%
against a 33.3% null and 20.1% against 25.0% — and §8e's matched controls show
that is the fix specifically, not generic perturbation. Dropping it into the
live run would throw away 106 and 62 generations of 3p and 4p training.

If it must go in mid-run, the only safe route is to move
`experiments/league_state/` aside for the 3p and 4p arms and restart them from
`--init default`, per `docs/TRAINING_RUN.md`. The 2p arm is unaffected either
way (49.9% vs a 50.0% null) and could take the change in place.

### What I would look at next, in order

1. **The champions have thrown away the horizon they already had.**
   `culture_rate_early/late` is (0.415, 0.148) at 2p and (0.000, −0.316) at 4p
   against a default of (+2.0, −2.0), on base weights of 20.1 and 35.6. Both
   arms that lose the culture-rate race price a culture rate as a *constant*.
   A better-shaped L is necessary and demonstrably not sufficient; something in
   the search is actively flattening this axis, and finding out what is worth
   more than any further shaping.
2. **Re-measure `--ablate` on `culture_rate_early`/`_late` at n≥200.** The
   trainer's own credit machinery should already be able to say whether those
   two weights are load-bearing or harmless, and if it says "harmless" while
   §4's rate curves say the race is lost on exactly that axis, the gate metric
   is not seeing the middle game.
3. Part 1's #3 (the war/aggression representation hole) is still unclosed and
   still bounded only by a crude oracle. §9 does not change that ranking.

### Loose ends, stated plainly

* The pool-paired numbers in 8d are from the pre-clamp build. The head-to-heads
  were re-run after clamping and moved by <0.3 points, so I did not re-run the
  pool arm; that is an assumption, not a measurement.
* `CARDS_PER_ROUND` is calibrated on WeightedBot self-play. Against a much more
  card-hungry policy the horizon would run long. It is not adaptive.
* Six head-to-head duels were run; two came back at p≈0.03-0.05 in the SAME
  direction on the default vector at 3p and 4p and the third (4p, p=0.0013) is
  well clear of any multiplicity correction. The 2p default result moved from
  45.6% (unclamped) to 47.2% (clamped) and is a null either way; I am not
  claiming a 2p regression.
* Throughput cost of the horizon: `lateness` goes from 0.35µs to 1.5µs against
  `features()`'s 82µs per candidate, i.e. ~1%. Three alternating
  `engine.perf_check` rounds on a loaded box read 511 vs 478 moves/cpu-s, which
  is ~6.5% — but the base arm's own spread across those rounds was 486/548/499,
  so that measurement cannot resolve 1% from 6%. Take the micro-benchmark.


# Part 3 — the horizon probe, and why the culture-rate axis goes flat (2026-07-26, branch `probe/horizon-4p`)

Working notes, appended by the probe agent. Worktree `/tmp/tta-probe`, branch
`probe/horizon-4p` = current master `8543933` (which already carries fix #1)
with fix #2 rebased on top. Nothing pushed or merged. The three live trainers
in the main checkout were not touched and `experiments/league_state/` was read
only.

Part 2 ended with two open items. §10's "what I would look at next" #1 asked
*why* the champions have thrown away the phase shaping they already had, and #2
asked whether `--ablate` calls those weights load-bearing. **Both are answered
below, and the answer to #1 is a mechanism with exact arithmetic behind it.**
The probe that motivated the branch is §13, and it is the weaker half of this
document.

---

## 11. §10 #2, answered from data already on disk: the shaping is not load-bearing in any arm

`experiments/league_state/ablation_{2,3,4}p.jsonl` — the trainer's own
`--ablate` machinery, n=72, paired, against `book`/`book2`/`var:culture`. The
cursor walks the weight list alphabetically, so `culture_rate*` has come up at
2p and 3p and not yet at 4p:

| arm | weight | value → 0 | edge | verdict |
|---|---|---|---|---|
| 2p (gen 175) | `culture_rate` | 20.108 → 0 | **−0.1104 ± 0.0285** | load-bearing |
| | `culture_rate_early` | 0.271 → 0 | **+0.0000 ± 0.0000** | no measurable effect |
| | `culture_rate_late` | 0.126 → 0 | **+0.0000 ± 0.0000** | no measurable effect |
| 3p (gen 175) | `culture_rate` | 6.136 → 0 | **−0.1791 ± 0.0231** | load-bearing |
| | `culture_rate_early` | 2.042 → 0 | −0.0003 ± 0.0054 | no measurable effect |
| | `culture_rate_late` | −1.187 → 0 | −0.0010 ± 0.0044 | no measurable effect |

Read the standard errors. At 2p both phase multipliers ablate to **exactly
zero edge with exactly zero standard error** — that is not a statistical null,
it is the discrete statement that deleting them changed **not one game**. The
2p shaping is literally inert.

At 3p it is not inert (some games differ, so `se` is non-zero) but it is worth
`0.000 ± 0.005` win share — and 3p is the arm that *kept* its shaping
(2.042 / −1.187, close to the default's 2.0 / −2.0). **So even where the
shaping survives, the gate cannot see it, while the level term next to it is
worth 0.11–0.18 win share.**

§10 #2 predicted exactly this fork: "if it says harmless while §4's rate curves
say the race is lost on exactly that axis, the gate metric is not seeing the
middle game." The fork is now live and I cannot close it from these numbers
alone. Two readings, both consistent with everything measured:

* **(A) the shaping really does not matter.** The ablation is a local
  derivative at the champion's own operating point against three opponents; it
  says removing the shaping from *this* vector costs nothing.
* **(B) the gate is blind to it.** §8d measured the new horizon as **+7.5
  points at 4p (p=0.0013)** on `DEFAULT_WEIGHTS`, whose `Σ|late−early|` is 16.9
  — a vector with real shaping. If shaping were globally worthless, changing
  the function it multiplies could not be worth 7.5 points on any vector.

(B) is not proof, because the horizon changes `L`, not the weights, and its
leverage is exactly `dL × |w_late − w_early|` (§8c). A vector with no shaping
is *immune* to the horizon by construction — which is the same fact viewed from
the other side, and is also why the 2p champion (`Σ = 7.5`, culture-rate
shaping ≈ 0) did not notice the fix at all.

## 12. §10 #1, answered: the search cannot afford to explore the shaping axis

Three facts, none of them noisy.

### 12a. The exactly-zero values are the weight guard's fingerprint

Current champions, all 20 phase multipliers each:

| arm | positive-default multipliers at **exactly** 0.000 | negative-default multipliers at exactly 0.000 |
|---|---|---|
| 2p | 2/10 (`culture_rate_early`, `food_rate_early`) | 0/10 |
| 3p | 2/10 (`culture_late`, `wonder_progress_early`) | 0/10 |
| 4p | 2/10 (`tech_levels_early`, `wonder_progress_early`) | 0/10 |
| **total** | **6/30** | **0/30** |

Fisher exact, one-sided: **p = 0.012**. And several more of the positive set sit
one step off the floor — 2p `resource_rate_early` 0.006, `workers_early` 0.006;
4p `culture_rate_early` 0.002, `culture_late` 0.018.

The negative-default half is **not** merely non-zero, it crosses zero freely:
10 of those 30 currently hold the *opposite* sign to their default (2p
`culture_rate_late` +0.429, 3p `tech_levels_late` +1.771, 4p `culture_early`
+8.32, …). Sign crossing is a ~33%-frequency event in this search for the
multipliers that are allowed to do it, and a 0%-frequency event with a pile-up
at the boundary for the ones that are not.

The asymmetry is `guard_weights`. `NONNEG` is still `{k: DEFAULT_WEIGHTS[k] >
0}` with no phase exemption, so the ten **positive**-default multipliers are
sign-locked while fix #1's `_PHASE_MULT` exemption freed only the ten
**negative**-default ones. §7 flagged this and deferred it: *"By the gauge
argument above those clamps are probably also meaningless and they do fire in
practice … but removing them belongs in its own separately-measured commit."*
The evidence below upgrades "probably meaningless" to "measurably not".

**How `culture_rate_early` reached 0.000, traced exactly.**
`experiments/league_state/ladder_4p/` archives every accepted champion:

```
gen   culture_rate  _early    _late     late-early
00034      12.837   27.470   -0.113       -27.582
00037      17.453    0.000   -0.367        -0.367
```

`guard_4p.jsonl` gen 37: mutant:0, op `group:economy`, proposed
`culture_rate_early = -8.0696`, **clamped to 0.0**. `generations_4p.jsonl` gen
37: that mutant was **accepted**, edge +0.1551, lo +0.0616. One accepted
generation destroyed the entire culture-rate shaping and the base weight moved
12.837 → 17.453 to partly absorb it.

2p is the same event at gen 241: `guard_2p.jsonl` clamps mutant:0 (`kick`)
`culture_rate_early = -0.505` → 0.0, `generations_2p.jsonl` accepts it (edge
+0.0826, lo +0.0027), and `ladder_2p/gen00241.json` reads 0.000. It has not
moved off 0.000 in the 33 generations since.

### 12b. The clamp is *not* an expressiveness bug — and that is the point

Being fair to the guard: the constraint `early ≥ 0` restricts **nothing** in
policy space. §7's own gauge argument cuts both ways — `(base, early, late) →
(base−c, early+c, late+c)` is the identical policy, so any vector with
`early < 0` can be re-expressed with `early = 0`. The clamp is a legal gauge
fixing.

What it is not is a *gauge-preserving* one. `guard_weights` sets `early := 0`
**without** the compensating `−c` on the base and `+c` on the late. So the
clamped mutant is a genuinely different policy from the one the operator
proposed, substituted silently, and the difference is concentrated exactly on
the early-game valuation. Above, `culture_rate_early = −8.07` proposed a
price of `17.45 − 8.07 = 9.4` at `L=0`; what was actually evaluated and
accepted priced it at `17.45`.

### 12c. The step size is the real trap: level moves 61× faster than shape

`hillclimb.mutate:207` is

```python
new = _clamp(out[k] + rng.gauss(0.0, s) * (abs(out[k]) + 0.15))
```

— the proposal step is **proportional to the weight's own magnitude**, with a
floor of `0.15·s`. Sampling 4000 real `mutate()` calls on each live champion at
its own current `sigma`, and projecting onto the two gauge-meaningful
coordinates (level `= base + early`, i.e. the price at `L=0`; shape
`= late − early`, the entire horizon signal):

| arm | sigma | `culture_rate` base | step sd, **level** | step sd, **shape** | ratio | shaping today |
|---|---|---|---|---|---|---|
| 2p | 0.50 | 23.927 | 10.82 | **0.43** | **25×** | −0.27 (flat) |
| 3p | 0.50 | 6.136 | 4.27 | **1.59** | **2.7×** | −3.23 (intact) |
| 4p | 0.08 | 35.574 | 3.42 | **0.056** | **61×** | −0.32 (flat) |

**The three arms rank on this ratio exactly as they rank on whether they kept
their shaping, and 3p — the one arm with an intact culture-rate horizon — is
also the one whose base weight never ran away.** That is the mechanism: once
`culture_rate` grows large, the multiplicative step hands the *level* an
enormous exploration budget and starves the *shape*, which is pinned near the
`0.15·s` floor. The gauge degeneracy guarantees the search can always cash a
shape change out as a level change, and the level is where all the step size
is, so it does.

The arithmetic of the trap at 4p: rebuilding the default's shaping (`|late −
early| = 4.0`) from the current 0.32 at a step sd of 0.056 needs ~66 σ of
*coherent drift*, or ~4400 steps of undirected random walk, in a coordinate the
ablation in §11 says the gate cannot even see. Meanwhile one accepted move on
the base shifts it by ~3.4. The search is not "declining to use the
representation"; it is being offered the shape coordinate at 1/61 the resolution
of the level coordinate and priced on a metric that scores the shape at
0.000 ± 0.005.

### 12d. What the flattened axis actually costs, in culture points

`culture_rate` is `s.culture`, culture **production per turn**, and `culture`
is the FROZEN stock weight = 1.0. So the true value of `+1` culture rate
acquired with `R` rounds left is exactly `R` culture points. Measured game
lengths (§8a): 2p 22.7, 3p 23.6, 4p 28.9 rounds; 4p rounds-left by age is
A 28.9 / I 24.9 / II 15.9 / III 6.4 / IV 2.0.

| vector | price at `L=0` | price at `L=1` | true value, start → end |
|---|---|---|---|
| DEFAULT | 7.00 | 3.00 | ~29 → 2 |
| 2p champion | 23.93 | **24.36** | ~23 → 2 |
| 3p champion | 8.18 | 4.95 | ~24 → 2 |
| **4p champion** | **35.58** | **35.26** | ~29 → 2 |

The 4p champion prices a culture rate at 35.6 culture points *flat* — **above
the theoretical ceiling at every single point in the game**, and ~18× over on
the last two turns. The 2p champion's slope is outright inverted (24.36 late >
23.93 early). Only 3p has a correctly-signed slope, and it is the arm that
under- rather than over-prices.

This is the same defect §4 measured behaviourally ("it buys the right asset too
late"), now in closed form: the hill climb correctly discovered that the
default's level was far too low and pushed it up by 7×, and the step geometry
meant the only way it could do that was through the base term, which flattened
the slope on the way. **The level was cheap to fix and the slope was
unaffordable, so it fixed the level and paid for it with the slope.**

### 12e. Two changes this argues for — neither of them measured, both cheap

Stated as proposals, not results. I did not implement either, because editing
`experiments/hillclimb_league.py` or `experiments/hillclimb.py` in this worktree
would be picked up by the probe's own hourly supervisor restart and confound the
one experiment this branch exists to run.

1. **Make the phase exemption symmetric.** Change `NONNEG` to
   `frozenset(k for k, v in DEFAULT_WEIGHTS.items() if v > 0 and k not in
   _PHASE_MULT)`, matching `NONPOS`. Two lines, plus a test case. §7's own gauge
   argument already justifies it; §12a supplies the evidence that the
   asymmetry is doing damage rather than nothing. **Caveat: by §12b this is not
   an expressiveness fix**, it only stops the silent non-gauge substitution and
   the pile-up at 0. On its own it will not rebuild a slope — §12c will still
   starve it.
2. **Decouple the phase multipliers from the base's step size.** The floor
   `abs(w) + 0.15` is right for value terms and wrong for multipliers, whose
   natural scale is O(1) regardless of how large the base grew. Something like
   `abs(w) + 0.15 + 0.5*(k in _PHASE_MULT)` — or, better, mutating in the
   (level, shape) basis so the two coordinates get comparable budgets — would
   restore the search's access to the axis. This is the load-bearing change of
   the two and it is a real search change: it needs its own A/B and it must not
   be dropped into a running arm.

Ordering, if only one: (2). (1) is tidy and nearly free; (2) is the one §12c
says is actually holding the axis shut.

---

## 13. The probe: 4p, clean restart, horizon fix on. Design and pre-registration

Written and committed **before the probe's first full-pool check landed**, so
the reading rule is not fitted to the result.

### 13a. What is running

```
cd /tmp/tta-probe
nohup experiments/run_league.sh 4 12 1 2 12 4 1.2816 \
    --init default --weight-guard clamp --past-k 2 \
    --state-dir /tmp/tta-probe/experiments/probe_state_4p &
```

Supervisor PID **54418**, started 20:50:23 MDT, `TTA_JOURNAL=1` (set by
`run_league.sh`). State dir is `/tmp/tta-probe/experiments/probe_state_4p`,
which did not exist before launch — mandatory, because `--init` is ignored once
a state dir holds a champion (`docs/TRAINING_RUN.md`). Log
`/tmp/tta-probe/experiments/logs/league_4p.log`. `experiments/league_state/` in
the main checkout was read and never written.

**One worker, not the live arms' two.** The box is a 6-core machine already
running 3 supervisors × 2 workers = exactly 6. One extra is acceptable
oversubscription; two is not. Measured cost of that choice: the probe is
running at **124–137 s/generation against the live arm's 122 s/gen average**,
i.e. the second worker was buying the live arms almost nothing on an already
saturated box, and the probe is not meaningfully slower per generation. It is
still ~100 generations behind in *total* because it started 4.3 hours later.

### 13b. Why this is a paired A/B and not two unrelated runs

Better than the brief assumed, and worth stating because it is what makes a
handful of generations readable at all:

* **Identical gen-0 champion.** `ladder_4p/gen00000.json` is byte-identical
  between the probe and the live arm (`DEFAULT_WEIGHTS` + the 4p
  `hand_potential → 0.0` init override). Verified, not assumed.
* **Identical mutation stream.** `hillclimb_league.py:650` is
  `rng = random.Random(seed * 7919 + players * 101 + gen)` — seeded by
  generation number alone. At gen 1 both arms drew the same operator pair
  (`kick`, `scatter`) against the same accept subset
  (`mirror, book2, var:infra, var:military`).
* **Identical evaluation seeds.** `seed0 = (gen*1_000_003 + j*7717 + seed) %
  10_000_019` and `seed + label_seed(label)`; `label_seed` is crc32, explicitly
  chosen to be stable across processes. **None of these read `workers`.** Worker
  count changes wall clock and nothing else.
* So gen 1 is a true paired duel: same start, same mutants, same games, one
  variable. Live gen 1 scored the pair at edges (−0.135, −0.0001); the probe
  scored the same pair at (−0.1032, −0.047). Both rejected.
* The pairing decays from gen 2, where the arms diverged: both drew
  (`rescale:actions`, `group:actions`), the live arm accepted the second and the
  probe accepted the first. From there the mutants differ because `mutate`
  reads the champion. The *seeds* stay matched forever; the *proposals* do not.

### 13c. The confounds, enumerated

1. **Worker count — not a confound on play.** See 13b. Wall clock only, and
   measured at ~1.05×, not 2×.
2. **The two-sided guard (fix #1) — the dominant confound, and unavoidable.**
   The live arm's generations 1–108 ran the **one-sided** guard; master gained
   the two-sided version at `8543933` and the live arms only picked it up at
   their 20:50 restart. The probe has had it since gen 1. Measured: the
   `NONPOS` half fires **~0.8 times per generation, 27% of all guard hits**, in
   the probe's early generations. So the probe is not "the live arm plus the
   horizon" — it is "the live arm plus the horizon **plus** a different guard".
   §9 bounded the guard's effect on a *fixed champion's play* at ±0.024 win
   rate, but that is not the same quantity: §12a is a worked example of a single
   guard clamp permanently redirecting a *trajectory* in one accepted
   generation. This confound cannot be removed without disobeying the
   instruction to rebase onto current master, and it should not be — the live
   arms now run the guard too, so the probe matches their future, not their past.
   (Verified prediction, incidentally: §10 said the live 4p champion's
   `rival_culture` would be clamped from +5.611 to 0.0 within the hour. It reads
   **0.0** as of the 20:50 restart.)
3. **Engine version.** The live arm's generations 1–49 ran on the pre-journal
   engine; it relaunched on the journal engine at 18:41
   (`docs/TRAINING_RUN.md`). Per `docs/PYPY.md` 9.14 the journal branch's
   `perf_check` baselines were required to agree with master's, i.e. it is a
   bit-identical-play optimisation. **That is taken from the doc, not
   re-verified here.**
4. **Pool drift at matched generation.** Both arms open with the identical
   14-member pool. The `past:` tier (`--past-k 2`, weight 1.0 of 8.0 total)
   rotates in each arm's **own** archived champions as its ladder fills, so from
   ~gen 30 the two arms are graded against different `past:` opponents — each
   against its own ancestors. Symmetric by design, but not identical, and it
   means the fullcheck is not the same exam at matched gen.
5. **n = 1 trajectory per arm.** There are no error bars on "this hill climb
   went higher than that one". The per-point sampling error (13d) is *not* the
   relevant noise; the relevant noise is trajectory-to-trajectory variance in a
   stochastic search, and one run of each measures it at zero degrees of freedom.
6. **Sigma.** The live arm's `sigma` has collapsed to 0.08 at 4p; the probe
   starts at 0.25. Endogenous — part of what is being compared, not a nuisance —
   but it means "matched generation" is not "matched search temperature".

### 13d. The metric and its resolution

Both arms write `fullcheck_4p.jsonl`: every 10 generations, 48 games against
each of the 13 pool opponents. `tools/probe_compare.py` reports the
pool-weight-averaged win rate and culture margin. Its quoted standard error
treats opponents as independent binomials, which is optimistic (one candidate
plays all 13, so its own strength is a shared term). Sanity check: the live
arm's own converged plateau reads 0.399 / 0.364 / 0.392 / 0.381 / 0.375 at gens
60–100, sd **0.013**, comfortably inside the ±0.022 the script quotes.

**So a difference between the two arms smaller than about ±0.05 in pool win
rate at a single generation is not interpretable, and even a consistent
difference is one trajectory against one trajectory.**

### 13e. Reading rule, fixed in advance

The gen-0 anchor already exists: §8d measured the horizon fix on
`DEFAULT_WEIGHTS` at 4p as **+7.5 points over a 25% null, n=400, p=0.0013**.
That is the head start the probe begins with. It is in a different metric
(BookBot-pool duels) from the fullcheck, so it is directional, not subtractable.

| pattern over the matched generations | reading |
|---|---|
| probe above live by ≳0.05 at 3+ consecutive matched gens, gap roughly **constant** | the fix is a level shift; a clean restart banks §8d's +7.5 and training neither amplifies nor erodes it. **Supports restarting.** |
| gap **widens** with generation | the fix raised the ceiling. Strongest possible support for restarting. |
| gap **closes** toward zero | training routes around the bad shaping; the 174/99 generations already invested are worth more than the fix. **Argues against restarting.** |
| everything inside ±0.05, or sign flips between points | **the probe cannot distinguish the hypotheses.** Report that and do not round it up. |

Given 13c#5 and the number of matched points available before the window closes,
the last row is the most likely outcome and it is the one I expect to be
writing.

### 13f. A second, sharper observable — also pre-registered

The win-rate trajectory is the noisy half of this probe. §12 supplies a
mechanism observable that is discrete rather than statistical, and the probe
tests it for free.

§12c predicts that **the horizon fix alone will not keep the shaping alive.**
The probe starts from `DEFAULT_WEIGHTS`, whose culture-rate shaping is the full
`(2.0, −2.0)`, `Σ|late−early| = 16.9` across all ten phase keys — so it begins
with everything the horizon has leverage on. But the step-size trap is
untouched by fix #2: as soon as `culture_rate`'s base runs away, the level
coordinate's step outruns the shape coordinate's, and a `guard_weights` clamp
of `culture_rate_early` becomes an available one-way exit.

The live 4p arm's own record of that happening, from `ladder_4p/`:

```
gen 00000  base  5.000  early  2.000  late -2.000   shape  -4.00
gen 00018  base  3.116  early  2.900  late -2.920   shape  -5.82   (shaping intact)
gen 00030  base 12.837  early 11.907  late -0.637   shape -12.54   (base begins to run)
gen 00037  base 17.453  early  0.000  late -0.367   shape  -0.37   (clamp; collapsed)
gen 00051  base 35.574  early  0.000  late -0.316   shape  -0.32   (base absorbs it)
```

So: **if the probe's `culture_rate` base also runs away and its shape also
collapses toward zero within its first ~40 generations, that is direct evidence
that a clean restart on fix #2 alone buys a transient starting advantage which
the search then spends** — and it makes fix #2 conditional on the step-size
change in §12e(2) rather than sufficient on its own. If the probe holds its
shaping through gen 40+, §12c is wrong or at least not the whole story, and
fix #2 stands on its own.

This is n=1 either way and a search is stochastic, so a single arm holding or
losing its shaping is suggestive, not proof. But it is a *discrete* event with a
named cause and a logged trigger, which is a better class of evidence than a
0.03 difference in a pooled win rate.

### 13g. A code-review note on what a restart should restart *on*

Not a probe result — read off the implementation and the gauge argument, while
waiting for generations. Relevant because the question on the table is what to
restart the 3p/4p arms on, and the answer should not be "exactly the commit that
happened to exist".

`_L_ZERO = {2: 27.1, 3: 28.7, 4: 36.1}` are the rounds-left values at which the
new `L` reaches 0. They were chosen (§8b, and the commit message says so
outright) as the least-squares fit to the **old** age-bucket schedule, with the
gauge "spent on not breaking the three already-trained champions."

Evaluated at the opening position of a real game:

| players | `_L_ZERO` | `rounds_left()` at game start | **L at game start** | usable span of L |
|---|---|---|---|---|
| 2 | 27.1 | 24.10 | 0.136 | 0.864 |
| 3 | 28.7 | 25.33 | 0.142 | 0.858 |
| 4 | 36.1 | 31.07 | **0.162** | **0.838** |

**L never reaches 0 in a game that is actually played.** `1 − L` tops out at
0.84–0.86, so an `_early` phase weight has ~16% less leverage over the opening
position than it had under the old schedule, and the top ~16% of L's declared
range is dead. That is not a bug — it is the exact price of the calibration
target, and §8b states the Age A offset — but **the calibration target is void
at a clean restart.** There are no trained champions left to protect; the gauge
is free again and is currently being spent on preserving vectors that a restart
throws away.

If the arms do restart on fix #2, set `_L_ZERO[n]` to `rounds_left()` at the
opening position (24.1 / 25.3 / 31.1) so L spans the full [0, 1]. By the gauge
argument this changes **no reachable policy** — it is an affine reparametrisation
— so it cannot be sold as a strategy improvement. What it changes is the *scale
the search's steps act on*, handing the phase weights back ~16% of their
dynamic range on an axis §12c already measures as starved by 61×. Cheap,
principled, and it should ride along with any restart rather than be measured
separately, because there is nothing to measure: it is gauge.

(Reviewed the rest of `db5aa4d` at the same time. `rounds_left` is exact once
`final_round_end` is set, `_tail` is memoised per (players, age) so the
`C.db()` call stays out of the search loop, `horizon_age` is correctly kept out
of `DEFAULT_WEIGHTS` and inside `hillclimb.FROZEN` so no mutation can reach it,
and the [0,1] clamp is pinned by a test. No defects found.)

---

## 14. The time box was lifted mid-run: a matched control arm, and a metric bug

Two changes to the experiment, both made after §13 was written and committed.

### 14a. A control arm, which removes confounds 13c#2 and 13c#3 outright

`/tmp/tta-control` is a **detached** worktree at master `8543933` — that is
exactly the probe's branch minus the horizon commit, since everything else on
`probe/horizon-4p` is documentation and `tools/`. Launched 21:22 MDT:

```
cd /tmp/tta-control
nohup experiments/run_league.sh 4 24 1 2 12 4 1.2816 \
    --init default --weight-guard clamp --past-k 2 \
    --state-dir /tmp/tta-control/experiments/control_state_4p &
```

Supervisor PID **60757**. Same fresh-state-dir setup, same `--init default`,
same one worker, same current master, same two-sided guard, same journal
engine — **the horizon function is the only difference.** That deletes the two
confounds §13c could only enumerate:

* **13c#2 (the two-sided guard) — gone.** Both arms have it from gen 1.
* **13c#3 (pre-journal engine) — gone.** Both arms are journal-only throughout.

The live arm's on-disk history stays in the tables as a third reference point,
but it is now the *weak* comparison and the probe-vs-control pair is the
experiment. §13c#1 (worker count) was already shown not to affect play, #4
(pool drift) and #5 (n=1 per arm) and #6 (sigma) survive unchanged — with the
important improvement that #4 is now symmetric between two arms started the
same way rather than between a fresh arm and a 4-hour-old one.

**Core budget.** The box is 6 cores with 3 live arms × 2 workers = 6. Probe and
control are 1 worker each, so the box now runs 8 workers on 6 cores — 33%
oversubscribed. That is a deliberate choice, authorised, and it costs wall clock
only: §13b established that nothing in the seed derivation reads `--workers`, so
the *play* is unaffected and the generation-matched comparison is not damaged.
The live arms are slowed proportionally and were not otherwise touched.

### 14b. The obvious metric was measuring the opponents, not the arm

Caught at the probe's first full-pool check, and it is large enough to have
produced a false positive if it had gone unnoticed.

`fullcheck_4p.jsonl` grades against 13 opponents. **Three of them are
`WeightedBot`s and therefore change when `lateness()` changes:**

| opponent | how it is built | horizon-sensitive? |
|---|---|---|
| `default` | `arena.make_bot:74` → `WeightedBot(seed)` | **yes** |
| `past:ladder_4p/gen00000` | `WeightedBot(weights=…)` | **yes** |
| `past:league_4p/gen00103` | `WeightedBot(weights=…)` | **yes** |
| `book`, `book2` | `BookBot` v1/v2 — rule-based, no evaluator | no |
| `var:`×6 | `VariantBot`, a `BookBot` subclass | no |
| `greedy` | `GreedyBot` — its own `WEIGHTS` and its own `evaluate()` in `engine/bots/__init__.py` | no |
| `random` | `RandomBot` | no |

And §8d/§8f already measured which way each moves: the new horizon makes
`DEFAULT_WEIGHTS` **stronger** (+7.5 points at 4p) and makes an
already-trained champion **weaker** (20.1% against a 25% null). So an arm on
the new horizon is graded against a *harder* `default` and a *crippled*
`past:league_4p/gen00103`.

That is exactly what the raw gen-10 numbers showed, and they showed it loudly:

| gen-10 opponent | probe | live | Δ |
|---|---|---|---|
| `past:league_4p/gen00103` | 0.396 | 0.000 | **+0.396** |
| `greedy` | 0.917 | 0.688 | +0.229 |
| `default` | 0.229 | 0.427 | **−0.198** |
| all 8 book/variant opponents | **0.000** | **0.000** | 0.000 |

The two biggest movements are both on horizon-sensitive opponents and they
point in *opposite* directions — precisely the signature of the opponent
changing rather than the arm improving.

`tools/probe_compare.py` therefore reports a **horizon-invariant** metric over
the 10 unaffected opponents, and that is the one to read. Effect of the
correction at gen 10:

| metric | probe | live | Δ |
|---|---|---|---|
| all 13 opponents (confounded) | 0.092 ± 0.007 | 0.065 ± 0.005 | **+0.027** |
| **horizon-invariant, 10 opponents** | **0.053 ± 0.002** | **0.047 ± 0.003** | **+0.006** |

**The +0.027 was four fifths artifact.** Anyone comparing two horizons through
this file without the subset will read a win that is not there.

### 14c. Matched generation 10: a null, and the reason it must be

Horizon-invariant, probe vs the live arm's own gen 10:

| | probe | live |
|---|---|---|
| win rate | 0.053 ± 0.002 | 0.047 ± 0.003 |
| culture margin | −114.6 | −117.8 |

Both arms score **exactly 0.000 against all eight book/variant opponents**;
the entire invariant win rate at this stage is `greedy` and `random`, the two
floor bots. Ten generations from `DEFAULT_WEIGHTS` is far too early for this
metric to have any resolution — the live arm did not clear 0.10 on the
invariant metric until gen 40. Culture margin is the more sensitive statistic
while win rates are pinned at zero, and it too is a wash (+3.2 points, against
per-opponent margin swings of ±10).

Nothing is claimed from gen 10. The comparison starts to mean something around
gen 40, where the live arm's own trajectory finally moves
(0.047 → 0.050 → 0.074 → **0.175** → 0.189 → **0.340** at gens 10–60), and the
probe-vs-control pair is the one that will carry it.

---

# Part 4 — the mechanism, separated into the part that needs selection and the part that does not

The probe was stood down at 21:26 MDT (the horizon fix was adopted on the
strength of §8d rather than waiting for it; supervisors 54418 and 60757 killed,
box returned to the three live arms). Everything below is about §10 #1.

Branch rebased onto master `8e751cb`. The 3p/4p evidence base is now
`experiments/archive_prehorizon/`, which reproduces §12's numbers exactly
(4p champion `culture_rate` 35.574 / `_early` 0.002 / `_late` −0.316 at gen 119;
3p 6.250 / 2.158 / −1.320 at gen 212). The 2p arm was not restarted, so it is
still live at gen 335 — and it is still reading `culture_rate_early` **0.000**
while its base has grown 23.927 → **32.246** in the ninety minutes since §12 was
written. The mechanism is still running.

## 15. A pure-drift simulation: which half of this needs selection?

`tools/drift_sim.py`. The §12 argument is about the **search's move generator**,
not about Through the Ages, so it can be tested without playing a single game:
run the real `hillclimb.mutate()` and the real `guard_weights()` for 120
generations, two proposals per generation exactly as the trainer does, and
**accept on a coin flip** (p=0.20, the live arms' measured accept rate) with no
reference to whether the candidate is any good. 300 independent runs.

| | simulated, pure drift | observed, live/archived champions |
|---|---|---|
| exactly-zero rate, the **10 positive-default** phase multipliers (guard-clamped) | **15.7%** | **6/30 = 20.0%** |
| exactly-zero rate, the **10 negative-default** ones (guard-exempt, §7) | **0.0%** | **0/30 = 0.0%** |
| same, with the guard turned OFF (counterfactual) | **0.0% / 0.0%** | — |
| `culture_rate` base, median [p90] | **3.22 [10.54]** | 32.25 (2p) · 6.25 (3p) · **35.57 (4p)** |
| `culture_rate` shape retention `|late−early| / 4.0`, median | **0.90** | 0.11 (2p) · 0.87 (3p) · **0.08 (4p)** |

Three conclusions, and the second and third **correct §12c**.

### 15a. The exactly-zero pile-up needs no selection at all — and the guard is exactly the cause

Drift plus the one-sided clamp reproduces the observed pile-up quantitatively
(15.7% simulated vs 20.0% observed) *and* reproduces the asymmetry exactly
(0.0% on the guard-exempt half, in both). Turning the guard off in the
simulation drives the rate to **0.0% on both halves** — so the clamp is not
merely correlated with the pile-up, it is necessary and sufficient for it.

§12a's Fisher p=0.012 said the asymmetry was real. This says what produces it,
with a working counterfactual, and says that **no selection pressure is
involved**: the optimiser does this to itself.

### 15b. The base runaway is NOT drift — it is selected

Drift leaves `culture_rate` at a median of 3.22 after 120 generations, with a
90th percentile of 10.54 — *below* its own 5.0 starting value at the median,
because the step is multiplicative and multiplicative random walks have a
negative log-drift. The observed 32.2 (2p, 335 gens) and 35.6 (4p, 119 gens)
are far outside that. **Something is paying the search to inflate this weight.**

§12c framed the level/shape step-size ratio as the trap. That framing was
wrong in its causality and I am correcting it: the ratio is *downstream*. A
large base produces a large step, but drift alone does not produce a large base.
The ratio is what makes the collapse **irreversible**, not what causes it.

### 15c. The shape collapse is not drift either

Drift's median shape retention is **0.90** — the shaping survives 120
generations of undirected random walk essentially intact. The observed 0.11
(2p) and 0.08 (4p) are not drift. Note the consistency check that falls out:
**3p, the one arm that kept its shaping, reads 0.87 retention on a base of 6.25
— which is almost exactly what pure drift predicts (0.90 on a base of ~3-5).**
3p looks like an arm where nothing selected on this axis at all; 2p and 4p look
like arms where something selected hard on the base.

### 15d. So the chain is now, with each link separately established

1. **Selection inflates the `culture_rate` base.** Measured as not-drift (15b).
   The candidate cause is §16.
2. **A large base starves the shape coordinate.** Once base = 35.6, `mutate`'s
   step `gauss(0,s)·(|w|+0.15)` gives the level 39–41× the shape's step sd
   (§12c, recomputed on the archived vectors). Exact, from the code.
3. **The one-sided guard clamp then pins `*_early` at exactly 0.** Drift alone
   suffices once step 2 has made the coordinate small; reproduced in simulation
   at 15.7% vs 20.0% observed, and eliminated by turning the guard off (15a).
4. **The result cannot be undone.** Rebuilding the default's `|late−early| = 4.0`
   from 0.32 at a shape step sd of 0.056 needs ~66σ of coherent drift, on a
   coordinate the trainer's own ablation scores at 0.000 ± 0.005 (§11).

Steps 2, 3 and 4 are settled. Step 1 is the open one and is §16.

## 16. Correction to §15b: the base runaway is not selected either. Nothing is selecting on this axis at all.

§15b said "something is paying the search to inflate this weight". Two further
tests say that is wrong, and I am retracting it.

### 16a. A sign test on the accepted steps finds no directional selection anywhere

`mutate`'s proposal is `new = w + gauss(0, s)·(|w| + 0.15)` — **symmetric around
the current value**. So under "no selection on this coordinate", the accepted
steps in it are a 50/50 coin flip, with no model of the game required. Every
accepted champion is on disk in the ladders, so every accepted step is directly
observable.

| arm | accepted champions | global up-rate, all weights | `culture_rate` up-rate | p (two-sided) |
|---|---|---|---|---|
| 2p | 48 | **454/907 = 0.501** | 9/13 = 0.69 | 0.267 |
| 3p | 31 | 38/79 = 0.481 | — (<8 moves) | — |
| 4p | 23 | 41/80 = 0.512 | — (<8 moves) | — |

The global up-rate is **0.501 across 907 accepted moves** — the null is exactly
right, which is the sanity check that the test works. And *no weight in any arm
is significant after multiplicity*: the smallest p anywhere is 0.0215
(`strength_deficit`, 2p) against 75 weights tested, i.e. an expected 1.6 hits at
that level by chance. `culture_rate` itself is p=0.27.

**There is no directional selection on `culture_rate`, or on anything else.**

### 16b. Then how did 5.0 become 32.2? Because the step is multiplicative

A 9-up/4-down sign pattern moves a weight enormously when the step size is
proportional to the weight. An up-move at w=20 adds ~5; a down-move at w=5
removes ~1.25. That is a geometric random walk, and geometric random walks have
a very fat right tail. Matched drift nulls (`/tmp/drift_pct.py`: same generation
count, same accept rate, real `mutate` and real `guard_weights`, coin-flip
acceptance, 200 runs):

| arm | gens | accept | observed `culture_rate` | drift median | drift p90 | drift p99 | **P(drift ≥ observed)** |
|---|---|---|---|---|---|---|---|
| 2p | 335 | 14.0% | **32.25** | 0.99 | 18.4 | 60.0 | **3.5%** (σ=.25) / 5.5% (σ=.5) |
| 3p | 212 | 14.2% | 6.25 | 2.54 | 13.9 | 52.0 | 27.5% / 20.5% |
| 4p | 119 | 18.5% | **35.57** | 4.72 | 18.1 | 60.0 | **2.5%** (σ=.25) |

Note the p99 column: **undirected drift reaches the `_clamp` ceiling of ±60
within a few hundred generations.** The observed values sit at the ~3% tail at
2p and 4p and at the *median-ish* 20–27% at 3p — and `culture_rate` was picked
out post hoc from ten phase keys, so a 3% tail in two arms is not a result. It
is what this optimiser does.

The median is the other half of the story: drift takes `culture_rate` from 5.0
down to **0.99** at the median, because a multiplicative walk has negative
log-drift. So the *typical* outcome of this optimiser is not "the weight stays
near its sensible default" — it is "the weight ends up somewhere between 0.1 and
60, essentially at random". Two arms went up. One went nowhere.

### 16c. The revised answer to §10 #1

**Nothing is actively flattening the culture-rate axis. The axis was never
explored.** The whole effect is the optimiser's move generator plus one guard
asymmetry, with the game playing no part:

1. `mutate`'s step is proportional to `|w|`, so every unbounded weight performs
   a **geometric random walk** that the accept test is too weak to constrain in
   the flat region the champion occupies. (`culture_rate` *is* load-bearing —
   ablating it to 0 costs 0.11–0.18 win share, §11 — but the evaluation is flat
   between 20 and 35, so the walk is free there.)
2. Two of three arms happened to walk up. That inflates the level coordinate's
   step by the same factor, so the walk accelerates — no selection needed.
3. The phase multipliers are O(1) and stay at the `0.15·s` step floor. Once the
   base is 35, the level is explored **39–41× faster than the shape** (2.6× at
   3p, the arm whose base did not run). That is why the shape stops moving.
4. `guard_weights`' one-sided clamp then pins the ten *positive*-default
   multipliers at exactly 0.000 — 15.7% per multiplier under pure drift,
   against 20.0% observed, and 0.0% with the guard off (§15a).

So the honest headline is not "the search is being paid to do something
perverse". It is: **the trained weight vector's large entries are substantially
random, and the phase-shaping entries are frozen — and the guard freezes half of
them at exactly zero.** That both arms which lost the culture-rate race have a
flat culture rate is then not a strategic signature; it is the same optimiser
artefact showing up twice, and the correlation with losing the race is
unestablished and may be coincidence at n=3 arms.

### 16d. What stands, and what I retracted

| claim | status |
|---|---|
| Exactly-zero pile-up on positive-default phase multipliers is caused by the one-sided guard clamp | **stands** — Fisher p=0.012 observed, 15.7% vs 20.0% in simulation, 0.0% with the guard off, and two specific accepted generations traced (4p gen 37, 2p gen 241) |
| `mutate`'s step is multiplicative, giving a 39–41× level/shape exploration asymmetry at 2p/4p and 2.6× at 3p | **stands** — exact, from the code |
| The 4p champion prices +1 culture/turn at 35.6 flat, above the theoretical ceiling (rounds remaining, ~29) everywhere | **stands** — exact arithmetic |
| The trainer's own ablation scores the culture-rate phase multipliers at 0.000 ± 0.005 while the base is worth 0.11–0.18 | **stands** — its own logs |
| §12c: "the step-size ratio is the trap that flattens the axis" | **corrected** (§15b) — the ratio is downstream; it makes the collapse irreversible, it does not cause it |
| §15b: "something is paying the search to inflate the base" | **retracted** (§16a/b) — no directional selection is detectable; a multiplicative random walk explains it |

## 17. §11's fork, settled: the shape IS load-bearing, and the gate cannot see it

§11 could not distinguish "(A) the shaping really does not matter" from "(B) the
gate is blind to it", because the trainer's `--ablate` zeroes one multiplier at
a time, which changes the *average price* of a rate as well as its shape. A
null there is ambiguous between "no effect" and "two effects cancelled".

`tools/shape_ab.py` removes the ambiguity by construction. It replaces
`(base, early, late)` with `(base + (1−L̄)·early + L̄·late, 0, 0)` — **the same
average price of a culture rate, no shape at all** — where `L̄` is the measured
mean of `lateness()` over real decisions (0.6348 / 0.6672 / 0.6819 at 2/3/4p,
from 9.8k/16.3k/30.0k candidate scorings). At 4p that is
`(5.000, +2.000, −2.000)` → `(4.272, 0, 0)`: a price that runs 6.35 → 3.00 over
the game, against a flat 4.27.

Paired on identical seeds, `hillclimb_league._series`, 4p, `DEFAULT_WEIGHTS`,
n=200 per opponent:

| opponent | win rate flat vs shaped | **culture margin edge (flat − shaped)** |
|---|---|---|
| `var:culture` | 0.000 vs 0.000 | −1.90 ± 4.03 |
| `book` | 0.000 vs 0.000 | −2.95 ± 4.07 |
| `book2` | 0.000 vs 0.000 | −6.54 ± 4.70 |
| **POOLED, n=600** | **0.000 vs 0.000** | **−3.79 ± 2.47** |

**The shape is worth 3.79 ± 2.47 culture points** — negative edge means removing
it hurts, and the 95% interval [−6.26, −1.32] excludes zero. So it is real, it
is small, and at these opponents it converts to **exactly zero** additional wins.

### The number that matters: what that is worth to the trainer's accept test

The gate scores these tiers with `margin_share(m) = 0.5·(1 + tanh(m/120))`. At
the measured operating point (`margin ≈ −166`),

```
d(gate score)/d(culture point) = (1/240)·sech²(−166/120) = 0.000928
so 3.79 culture points  ->  0.0035 of gate score
```

against a measured gate-score standard error of **0.0081 at n=200** and
therefore **0.0165 at the trainer's own 48-game evaluation block**.

**The entire culture-rate horizon signal is 0.21σ of the trainer's accept
statistic at its own block size.** Resolving it at 1σ would take ~1050 games per
candidate; the trainer spends 48–192. §11's reading **(B) is correct**: the
shaping is load-bearing and the gate is blind to it, by a factor of about five
in standard errors. The trainer's own n=72 ablation reporting `0.000 ± 0.005`
was not wrong — it was under-powered by ~20× in games.

## 18. The synthesis, and the thing that turned out to be bigger than the question

### 18a. Most of a trained weight vector is not distinguishable from a random walk

`/tmp/drift_gof.py`: for every free weight, build the matched pure-drift null
(same generation count, same accept rate, real `mutate`, real `guard_weights`,
coin-flip acceptance, 300 runs) and locate the champion's actual value in it.
Under "training moved this weight somewhere the accept test could resolve", the
percentiles pile at the extremes. Under "it wandered", they are Uniform(0,1).
Kolmogorov–Smirnov against uniform, swept over σ because the arms' σ varied
0.08–0.8 with a mean near 0.35:

| arm | gens | σ=0.15 | σ=0.25 | σ=0.35 | weights outside the null's central 90% (10% expected) |
|---|---|---|---|---|---|
| **2p** | 335 | p=0.14 | **p=0.59** | p=0.50 | 9 / **4** / 1 of 81 |
| **3p** | 212 | p=0.52 | **p=0.80** | p=0.46 | 12 / **5** / 4 of 81 |
| 4p | 119 | p<0.0001 | p=0.0041 | p=0.077 | 28 / 12 / 8 of 81 |

**The 2p and 3p champions' weight vectors are statistically indistinguishable
from an undirected random walk, at every σ tested.** Only 4p — the *youngest*
arm at 119 generations — is distinguishable, and in the direction "weights are
somewhat larger than drift" (median percentile 0.59–0.63).

That the youngest arm is the most distinguishable is the tell: **the drift null
widens with generation count faster than the recoverable signal accumulates.**

### 18b. And yet the arms genuinely, massively improve

This is not "training does nothing". The 2p arm, horizon-invariant pool win rate
by generation:

```
gen  10   50  100  150  200  250  300  330
    .200 .353 .450 .447 .654 .616 .645 .758
```

0.20 → 0.76 over 342 generations, still climbing, culture margin −58 → +48.
Real, large, sustained.

**Both facts are true at once, and that is the finding.** The search is making
large real improvements while each individual weight's marginal value is where
an undirected walk would have put it. The improvement therefore lives in the
*joint* structure — the combination — not in any individual coordinate. This is
the classic signature of a **sloppy model**: a few stiff directions carry the
objective, the rest are nearly flat, and parameters along flat directions
random-walk freely while performance improves along the stiff ones.

*Caveat, stated plainly:* §18a tests **marginals**, one weight at a time. It is
by construction blind to correlations, so it cannot and does not show that the
*vector* is drift — §18b proves it is not. What it shows is that **no individual
weight's value can be read as a strategic statement.**

### 18c. Which dissolves the question §10 #1 asked

§10 #1 said: *"Both arms that lose the culture-rate race price a culture rate as
a constant. Something in the search is actively flattening this axis, and
finding out what is worth more than any further shaping."*

The premise does not survive:

* **Nothing is actively flattening it.** No directional selection is detectable
  on any weight in any arm (§16a: global up-rate 454/907 = 0.501; nothing
  significant after multiplicity).
* **`(0.000, −0.316)` and `(0.415, 0.148)` are not strategic statements.** They
  are two coordinates of a vector whose marginals are indistinguishable from
  drift (§18a), on an axis the gate cannot resolve to within a factor of five in
  σ (§17).
* **That two of three arms did it is not a signature.** Undirected drift plus
  the one-sided guard clamp produces an exactly-zero positive-default phase
  multiplier 15.7% of the time per multiplier (§15a), against 20.0% observed;
  and `culture_rate` sits at the ~3% tail of drift at 2p and 4p but at an
  ordinary 20–27% at 3p (§16b), on a weight selected post hoc from ten.

What *does* survive, and is worth acting on:

1. **The gate is under-powered on this axis by ~5× in σ** (§17). Measured, exact.
2. **The one-sided guard clamp creates a spurious attractor at exactly 0** for
   the ten positive-default phase multipliers (§15a). Measured, with a working
   counterfactual.
3. **`mutate`'s multiplicative step makes every unbounded weight a geometric
   random walk** whose median takes `culture_rate` from 5.0 to 0.99 and whose
   p99 reaches the ±60 clamp (§16b). The trained large values are, individually,
   substantially arbitrary.
4. **The 4p champion's 35.6 flat price for a culture rate is above the
   theoretical ceiling everywhere in the game** (§12d). Exact. It is not
   *caused* by anything strategic, but it is still wrong, and it is invisible to
   the gate that is supposed to correct it.

### 18d. Three proposed fixes, ranked, none landed

Per instruction, these are proposals. I have not touched the climber.

| # | change | evidence | cost | confidence |
|---|---|---|---|---|
| **1** | **Make the phase exemption symmetric.** `NONNEG = frozenset(k for k, v in DEFAULT_WEIGHTS.items() if v > 0 and k not in _PHASE_MULT)`, matching `NONPOS`. | §15a: the clamp is *necessary and sufficient* for the exactly-zero attractor — 15.7% per multiplier with the guard on, **0.0% with it off**, against 20.0% observed. §7's own gauge argument already says these clamps are meaningless. | 2 lines + a test case | **high.** The only judgement is that §7 already made this argument and deferred it; the counterfactual now supports it. |
| **2** | **Decouple the phase multipliers' step size from the base.** In `mutate`, the step scale `(abs(w) + 0.15)` is right for value terms and wrong for O(1) multipliers. Either add a constant for `_PHASE_MULT` keys, or — better — propose in the (level, shape) basis so both coordinates get comparable budgets. | §12c/§16c: the level is explored **39–41×** faster than the shape at 2p/4p once the base runs, versus 2.6× at 3p whose base did not. Exact, from the code. | small code, **large validation** | medium. This is a real search change and must not be dropped into a running arm; it needs its own multi-hundred-generation A/B. |
| **3** | **Give the geometric walk a restoring force.** `_clamp` at ±60 is the only bound, and drift reaches it by p99. Options: shrink the multiplicative component (`0.3·|w| + 0.3`), add a weak log-prior toward `DEFAULT_WEIGHTS`, or bound each weight at a sane multiple of its default. | §16b: drift's median takes `culture_rate` 5.0 → **0.99** and its p99 to the ±60 clamp; §18a: 2p/3p champions' marginals are indistinguishable from that walk. | small code, **large validation** | medium-low as stated. The *diagnosis* is solid; which of the three remedies is right is not measured, and scale-adaptive steps exist for good reasons. |

Fix 1 is cheap, well-evidenced and self-contained. Fixes 2 and 3 are the ones
that matter and neither can be validated without a training run of its own —
which is exactly the experiment the probe arm was set up to do, and is the
natural use for that machinery now that the horizon question is settled.

**What I would NOT do:** add more features or more shaping to the evaluator. §17
shows the gate cannot resolve the shaping that already exists to within a factor
of five. Until the accept test can see an effect of that size, more
representation is more parameters for the walk to wander in.

## 19. §16's retraction was itself too strong: there IS a gradient, and it is sub-threshold

§15b claimed selection. §16 retracted it on a sign test that came back 9/13
(p=0.27) and a drift null that put the observed base at a ~3% tail. Neither was
significant, so I called it drift. **Measuring the gradient directly shows the
retraction over-corrected.** The truth needs both halves.

`tools/level_sweep.py`: `DEFAULT_WEIGHTS` with `culture_rate` set to each of
4 levels, everything else untouched (including `_early`/`_late`), same seeds,
4p, n=200 per cell. `gate score` is the trainer's *own* accept statistic for
that opponent — `margin_share(culture margin)` on margin tiers, win share on
the floor tier.

| `culture_rate` | `var:culture` gate | `book` gate | `greedy` **win rate** |
|---|---|---|---|
| **5.0** (default) | 0.0760 ± 0.0081 | 0.1074 ± 0.0104 | 0.840 ± 0.050 |
| 10.0 | 0.0725 ± 0.0076 | 0.1150 ± 0.0109 | 0.848 ± 0.050 |
| 20.0 | 0.0826 ± 0.0084 | 0.1216 ± 0.0123 | 0.853 ± 0.049 |
| **35.574** (the 4p champion's) | **0.0886 ± 0.0098** | **0.1311 ± 0.0139** | **0.807 ± 0.055** |
| Δ over the 7× range | **+0.0126** | **+0.0237** (monotone) | −0.033 (n.s.) |

**Win rate against both gate opponents is 0.000 at every level.** A seven-fold
increase in the weight buys the trainer's accept statistic +0.013 to +0.024 and
**not one additional game won.** Pool-weighted across the tiers measured (6.0 of
the 8.0 total pool weight; `mirror` and `past` unmeasured and counted as zero):

```
(+0.0237 x 3.0  +0.0126 x 2.5  -0.0325 x 0.5) / 8.0  =  +0.0108
```

**+0.011 of accept statistic, for free, in exchange for zero wins.**

### 19a. Why neither of my earlier tests could see it

Against the trainer's own 48-game evaluation block, whose accept-statistic
standard error is ~0.021, that bias is **0.51σ — spread over the *entire* 7×
range of the weight.** Per generation, per mutation, it is a small fraction of
that. So:

* the **sign test (§16a) had no power.** A bias of a fraction of a σ produces
  something like 9-up/4-down over 13 moves, which is exactly what was observed
  (p=0.27). I read a null as evidence of absence.
* the **drift null (§16b) was the wrong comparison.** A weak, never-changing-sign
  gradient superimposed on a geometric random walk does not look like selection
  at any single step; it looks like a walk that happens to have gone up. Over
  335 generations it integrates.

### 19b. The correct statement, combining §16 and §19

**Both components are needed and neither is sufficient:**

1. **A sub-threshold perverse gradient.** The gate scores 5.5 of 8.0 pool weight
   on *culture margin*, explicitly because those opponents are "the ones it
   loses to ~100% of the time, where win share carries no information". That is
   a defensible design — culture margin is the game's real score. But it means
   the trainer is paid for **losing by less in games it always loses**, and the
   most direct lever on culture margin is the weight on culture production. The
   payment is +0.011 for a 7× inflation, ~0.5σ of one evaluation block across
   the whole range. Invisible per generation; **it never changes sign.**
2. **A geometric random walk that offers no resistance.** `mutate`'s step is
   proportional to `|w|` (§16b), so there is no restoring force at all — the
   walk's own median takes `culture_rate` from 5.0 to 0.99 and its p99 to the
   ±60 clamp. A weight under a persistent sub-threshold push, with no restoring
   force and a step that grows as it grows, ratchets.

So §10 #1's "something in the search is actively flattening this axis" was
**half right for the wrong reason.** Nothing is flattening the *shape*. Something
is very slowly inflating the *level*, and the shape is collateral damage:
inflating the level starves the shape's step size 39–41× (§12c) and the
one-sided guard clamp then pins `_early` at exactly zero (§15a).

### 19c. This changes the fix ranking

§18d's fix #3 ("give the geometric walk a restoring force") was ranked
medium-**low** because the diagnosis was "the walk is arbitrary". It is now
"the walk is arbitrary **and there is a persistent perverse push along it**",
which is worse and more actionable. Revised:

| # | change | why, now | confidence |
|---|---|---|---|
| **1** | Symmetric phase exemption in `guard_weights` | unchanged — §15a, necessary and sufficient for the exactly-zero attractor, 2 lines | **high** |
| **2** | **Cap the margin credit, or score gate tiers on margin *rank* rather than raw margin** | §19: the gate pays +0.011 for a 7× weight inflation that wins zero games. `margin_share`'s `tanh(m/120)` is nearly linear at `m ≈ −140`, so it hands out unbounded credit for narrowing a hopeless loss. Saturating it sooner, or scoring on within-block margin rank, removes the push without giving up the fine gradation the margin tiers exist for. | **medium-high** — the defect is measured and monotone; the specific remedy is not yet A/B'd |
| **3** | Decouple the phase multipliers' step size from the base | unchanged — §12c, 39–41× vs 2.6× | medium |
| **4** | Restoring force on the geometric walk | now **more** important (§19b#2), but #2 addresses the same failure closer to its source | medium |

**Note the interaction with the horizon fix that was just adopted.** Fix #2 of
Part 2 makes the *shape* more accurate. §17 measured the entire shape signal at
0.21σ of one evaluation block, and §19 measures a competing incentive on the
*level* at 0.51σ pointing the wrong way. **The level's perverse gradient is
about 2.4× stronger than the whole shape signal the horizon fix improves.**
That does not make the horizon fix wrong — it is a correctness fix and §8d
measured it at +7.5 points from default — but it does predict that the restarted
3p/4p arms will re-inflate `culture_rate` and re-flatten their phase weights
unless #1 and #2 land as well.

## 20. The confirmation: the champion crushes the vectors that merely drifted

§18 asserted that the improvement lives in the joint structure while no
individual marginal is distinguishable from drift. That was an argument. This is
the measurement.

`tools/champ_vs_drift.py` plays the 2p champion (335 generations, 47 accepts)
head-to-head against **its own drift siblings** — vectors produced by
`tools/drift_sim.py` from the same `DEFAULT_WEIGHTS`, with the same generation
count, the same accept rate and the real `mutate`/`guard_weights`, differing
only in that acceptance was a coin flip. n=200 each, null 0.500:

| opponent | champion win rate | culture margin |
|---|---|---|
| `DEFAULT_WEIGHTS` (the shared starting point) | **0.927 ± 0.036** | +97.0 ± 8.6 |
| drift sibling #1 | **0.985 ± 0.017** | +161.6 ± 8.0 |
| drift sibling #2 | **0.940 ± 0.033** | +112.8 ± 9.0 |
| drift sibling #3 | **0.960 ± 0.027** | +130.7 ± 8.9 |

The champion beats drift siblings **more** easily than it beats the default
(0.94–0.99 vs 0.93) — undirected drift makes a vector *worse* than the
hand-designed starting point, which is the expected behaviour of a random walk
through a space where most directions are bad.

So both halves of §18 are now measured, not argued:

* **Training works, enormously.** 0.96 average against vectors that had exactly
  the same number of generations and accepts and differed only in whether the
  accept test was consulted.
* **And none of it is legible in any individual weight.** The same champion's
  81 free weights sit at a Uniform(0,1) position inside the drift null
  (KS p=0.14–0.59, §18a).

**A trained weight is not a strategic statement.** The value `culture_rate_early
= 0.000` carries about as much information about how this bot plays as any
single coordinate of a random walk does — which is what makes §10 #1's premise
(*"both arms that lose the culture-rate race price a culture rate as a
constant"*) a pattern read into noise, even though every number in it was
correctly measured.

---

# Summary of Part 3–4, for anyone who does not want to read two self-corrections

**The question:** why have the 2p and 4p champions driven
`culture_rate_early/late` to near-constant against a default of (+2.0, −2.0)?
What is flattening that axis?

**The answer, in one line:** the *shape* is not being flattened by anything —
it is collateral damage from the *level* being slowly, perversely inflated, and
neither is visible to the accept test.

| finding | evidence | confidence |
|---|---|---|
| The gate scores 5.5 of 8.0 pool weight on **culture margin**, so it pays for *losing by less* in games lost 100% of the time. Inflating `culture_rate` 7× buys **+0.011 accept statistic and zero extra wins**. | §19, n=200/cell, monotone in `book` | **high** |
| That bias is **0.51σ of one 48-game block across the whole 7× range** — never individually detectable, never changes sign. | §19a | high |
| `mutate`'s step is proportional to `|w|`, so there is **no restoring force**: drift's own median takes `culture_rate` 5.0 → 0.99 and its p99 to the ±60 clamp. Weak push + no restoring force = ratchet. | §16b, §19b | high |
| Once the base runs, the **level is explored 39–41× faster than the shape** (2.6× at 3p, whose base did not run). | §12c, exact from code | high |
| `guard_weights`' one-sided clamp then pins the ten *positive*-default phase multipliers at **exactly 0.000** — 15.7% per multiplier under pure drift, 20.0% observed, **0.0% with the guard off**. | §15a, simulated counterfactual | **high** |
| The horizon shape is real but worth **3.79 ± 2.47 culture points = 0.21σ** of one block. The gate is blind to it by ~5×. | §17, n=600 paired | high |
| The 2p/3p champions' weight **marginals are indistinguishable from a random walk** (KS p=0.14–0.80) — while the same champion beats its drift siblings **0.94–0.99**. | §18a, §20 | high |
| Therefore no individual trained weight can be read as a strategic statement, and §10 #1's premise does not survive. | §18c, §20 | high |

**Fixes, ranked** (§19c; none landed, per instruction):
1. Symmetric phase exemption in `guard_weights` — 2 lines, high confidence.
2. Cap the margin credit or score gate tiers on margin **rank** — removes the
   perverse push at its source.
3. Decouple phase-multiplier step size from the base.
4. A restoring force on the geometric walk.

**Prediction, on the record:** the level's perverse gradient (0.51σ) is ~2.4×
stronger than the entire horizon signal (0.21σ) that the just-adopted fix
improves. Unless #1 and #2 land, the freshly restarted 3p/4p arms should
re-inflate `culture_rate` and re-flatten their phase weights within a few
hundred generations. `experiments/league_state/ladder_{3,4}p/` will show it.
