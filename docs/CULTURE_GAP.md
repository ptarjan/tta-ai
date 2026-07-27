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
