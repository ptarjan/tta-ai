# League training: scoring a candidate against a pool, not against itself

**Status: built, smoke-tested end to end at 2p, 3p and 4p, NOT yet launched.**
The full run is a deliberate single clean restart and is launched by hand — see
[Launching](#launching). Read
[Go / no-go for a multi-hour run](#go--no-go-for-a-multi-hour-run) first: the
smoke run found one bug that would have wasted the whole run (the variant tier
was collapsed to a single opponent, now fixed) and one open calibration
problem (the gate tier is currently unbeatable from the untrained vector, so it
returns no gradient).

## Why the old loop had to be replaced

`experiments/hillclimb.py` scores a mutant against a *mirror* of its own
parent, plus a thin ladder of that parent's own ancestors. That answers **"is
this a better response to my lineage"**, which is not the same question as
**"is this stronger"**.

[`docs/STRENGTH_CHECK.md`](STRENGTH_CHECK.md) is the receipt. A hand-written
rule list — `engine/bots/book.py`, no learned weights, no lookahead — beats
the trained champion at every player count:

| matchup | players | n | win rate | 95% CI | null |
|---|---|---|---|---|---|
| BookBot vs champion_2p | 2 | 400 | **62.9%** | ±4.7% | 50.0% |
| BookBot vs champion_3p | 3 | 300 | **42.2%** | ±5.6% | 33.3% |
| BookBot vs champion_4p | 4 | 300 | **64.3%** | ±5.4% | 25.0% |

Meanwhile the champion beat GreedyBot — the baseline it was trained against —
by 88%. Both facts are true at once, and that is the whole lesson: **beating a
weak baseline told us nothing**, because BookBot beats that same baseline
96.4% of the time. Every internal metric reported progress the entire time the
population was converging on weak play.

The fix is not a better mutation operator or more games. It is a better
*question*: a candidate must prove itself against a **diverse pool of
opponents it did not produce**.

## The design

Three new files, and the old entry point is untouched:

| file | what it is |
|---|---|
| `experiments/hillclimb_pool.py` | the pool abstraction: entries, tiers, weighting, dynamic discovery, the rotating acceptance subset, the weighted statistics |
| `experiments/hillclimb_league.py` | the trainer: (1+λ) climbing, per-opponent paired scoring, the gate veto, the full-pool re-check, single-weight ablation |
| `experiments/run_league.sh` | the detached hourly-restart supervisor |

`experiments/hillclimb.py` and `experiments/run_hillclimb.sh` still work
exactly as before — other agents' jobs depend on them. The league trainer
*imports* `mutate` and `FROZEN` from `hillclimb.py` rather than copying them,
so there is still one implementation of the mutation operators.

Nothing under `engine/bots/` is modified. Pool bots are constructed by a
`make_bot` installed **over** `arena.make_bot`, the same trick
`experiments/bookmatch.py` already uses: arena workers are forked, so they
inherit the patch, and `arena.py`, `book.py`, `quiescent.py` and `variants/`
are all left alone.

### The pool

A pool entry is `(spec, weight, label)` plus a tier.

| tier | members | why it is in |
|---|---|---|
| `book` | `BookBot`, `BookBot` v2 | The external yardstick. Derived from published human strategy, not from our training loop, so beating it means something in absolute terms. This is the tier we currently **lose** to. |
| `variant` | everything in `engine/bots/variants/` | Strategy archetypes (tempo, infrastructure, military, culture, science, wonder-heavy…) built by another agent. **Discovered dynamically** — see below. |
| `quiescent` | `QuiescentBot` (`docs/DEEPER_SEARCH.md`) | Search-based opponent. Opt-in (`--with-quiescent`); it costs ~1.2× per game and its strength is not yet measured. |
| `mirror` | the candidate vs a table of its **parent** | Self-play. Kept — it is still a real signal — but demoted below every external bot. |
| `past` | archived champions, including the legacy `experiments/league_*p/` ladders | The anti-**cycling** guard: without a historical ladder a new champion can beat the current one while losing to an older one, and the loop will happily walk in a circle forever. |
| `floor` | `greedy`, `random`, `default` | Cheap floor checks. Weighted like it. |

Note that mirror is *candidate vs parent*, not candidate vs itself. A bot
against a table of itself is worthless by construction: the seats hold the
identical deterministic policy, so over a complete seat rotation the shares
sum to 1 and the mean is exactly `1/players` for **every** policy — it
measures the deal, not the bot. Candidate-vs-parent has a reference value of
exactly `1/players` for the same symmetry reason, which is why the mirror
entry costs no champion reference games at all.

### Dynamic variant discovery

`engine/bots/variants/` is being built by another agent and did not exist when
this was written, so discovery makes no assumptions and never raises:

* `*.py` modules exposing bot **classes** — an explicit `BOTS` (dict
  `label -> class`, or a list) or `BOT` attribute wins; otherwise every public
  class *defined in that module* whose name ends in `Bot` is taken;
* `*.json` files — a strategy archetype expressed as a WeightedBot weight
  vector;
* constructors are tried as `cls(seed=…)`, then `cls(rng=…)`, then `cls()`;
* a missing package, a module that will not import, a class that will not
  construct: each is logged on one line and skipped.

So the pool **grows by itself** as variants land — the hourly supervisor
restart picks them up without touching the running job — and training works
fine before any of them exist.

### The weighting, stated explicitly

Each **tier** carries a total weight; a tier's total is split evenly across
its members, so an entry's weight is `tier_total / len(tier)`.

```
book 3.0   variant 2.5   quiescent 2.0   mirror 1.0   past 1.0   floor 0.5
```

Splitting per tier rather than per entry is deliberate: landing a seventh
strategy variant must not let the variant tier outvote BookBot, so it splits
the variant tier's say seven ways instead. Setting a tier to `0` removes it.

```
--pool-weights book=4,variant=2.5,past=0.5,floor=0.25
```

The aggregate is the weight-weighted mean of **per-game paired edges**, where
each opponent's fixed total weight is divided by its own game count. That
matters: playing an opponent more games never silently increases its vote in
the aggregate, it only sharpens that opponent's own estimate.

### The scoring statistic

For every `(opponent, seed, seat)` the candidate plays, the **champion plays
the byte-identical game** — same seed, same seat, same opponent — and we
accumulate the difference in win share. So:

* the null is exactly **0**, whatever the pool contains and however strong or
  weird any single opponent is;
* seed luck cancels, which is what makes a decision affordable in tens of
  games rather than hundreds;
* seats are rotated by `arena.duel` (game *g* plays seat *g mod K*), so seat
  order is exactly balanced at 2p, 3p and 4p alike;
* the champion's reference games are played **once per generation** and shared
  by all λ candidates, so a generation costs `(λ+1) × P` duels, not
  `2 × λ × P`.

Acceptance keeps the existing machinery: accept when the lower bound of a
one-sided CI on the aggregate edge (`--accept-z`, default 1.2816 = 90%) is
above 0, with the 1/5th-success-rule σ adaptation and the stall kick unchanged.

### The gate veto — the aggregate is not allowed to hide a loss

> Do not let the aggregate hide a candidate that beats five weak bots and
> loses to BookBot.

Tiers `book`, `variant` and `quiescent` are **gate** tiers. If the candidate's
edge against any gate opponent it was tested on is *significantly* negative
(`edge + veto_z × se < 0`, `--veto-z` default 1.0), the candidate is rejected
**regardless of the aggregate**. The veto and the vetoing opponent are logged.

Every accept/reject also prints the full per-opponent table — label, tier,
weight, n, candidate win rate, champion win rate, edge — so a rejection says
*which* opponent killed the candidate. The old loop produced one number.

### Anti-overfit: rotation plus a full re-check

1. **The acceptance subset rotates.** Each generation scores against
   `--subset` opponents (default 4), never the whole pool, so weights cannot
   be fitted to the entire pool at once. Two invariants: mirror is always in
   (it is nearly free), and **at least one gate opponent is always in**,
   rotating through them — so "beat the weak half of the pool" can never be a
   winning strategy for even one generation.
2. **Periodic full-pool re-check.** Every `--full-check-every` generations
   (default 10) the champion is measured against **every** pool opponent at
   `--check-games` each, and the result is diffed against the previous check.
   Any opponent whose win rate fell by more than
   `max(3 points, combined CI)` is logged as a regression — and if the
   champion was **not tested against that opponent** in the intervening
   generations, it is logged separately and loudly as an
   `untested_regression`. That is precisely the overfitting signature this
   design exists to catch.

## Single-weight credit attribution

The known measurement flaw: acceptance mutates a bundle of weights at once and
keeps or discards the whole bundle on one test, so **nothing is ever learned
about any individual weight**. `wonder_remaining` is a trained weight that
measures at 27.6% ± 6.3% against a 25% null — indistinguishable from nothing.

Every `--ablate-every` generations (default 25) the trainer takes the next
`--ablate-k` weights off a rotating cursor (all 82 weights are covered in
turn, the cursor persists across restarts), **zeroes that one weight** in the
champion, and plays the result against the pool's gate opponents, paired
against the unablated champion on identical seeds. The champion's reference
games are played once and shared by every weight in the cycle.

| result | verdict |
|---|---|
| edge significantly < 0 | `load-bearing` — removing it measurably hurts |
| edge significantly > 0 | `harmful` — removing it measurably *helps* |
| CI covers 0 | `no-measurable-effect` — noise at this sample size |

Verdicts accumulate across cycles in `weight_credit_{K}p.json`
(`mean_edge`, total `n`, verdict counts), and each cycle appends to
`ablation_{K}p.jsonl` with the per-opponent breakdown. `--ablate-mode default`
ablates to the `DEFAULT_WEIGHTS` value instead of to zero.

This is a measurement, not a training signal: nothing is currently pruned
automatically. Read `weight_credit_{K}p.json` before trusting any weight.

## The degeneracy guard, and why 4p must start clean

The 4p champion the old loop produced has **`science` = −6.09**. A negative
science weight *inverts a cost term*: expensive cards start looking like
bargains (Alchemy priced at +67.04 at 4p against +5.86 at 2p), and 4p play
collapsed to 9.7% ± 2.7% until it was clamped by hand. Nothing in the old loop
noticed, because acceptance only ever compared a bundle against a mirror of
itself — and a mirror is equally happy to be wrong in the same way.

Running the guard over that champion finds **nine** inverted terms, not one:

| weight | trained value | default |
|---|---|---|
| `science` | −6.089 | +0.5 |
| `civil_actions` | −2.859 | +2.0 |
| `workers` | −1.941 | +1.4 |
| `colonies` | −0.962 | +2.0 |
| `hand_civil` | −0.680 | +0.3 |
| `best_farm` | −0.431 | +0.5 |
| `num_techs` | −0.407 | +0.3 |
| `food_stock` | −0.356 | +0.2 |
| `strength_rel` | −0.025 | +0.35 |

> **Re-measured on the smoke branch: eight, not nine.** `colonies` was reset to
> its `+2.0` default by `15b9764` ("Reset colonies/pacts weights to defaults")
> after the row above was written, so running the guard over
> `experiments/champion_4p.json` at `ddb04fe` now reports the other eight. The
> conclusion is unchanged and the row is left in as the record of what the
> vector looked like when the guard was designed.

The 4p champion believes **civil actions and workers are bad**. That is
exactly what `docs/STRENGTH_CHECK.md` measured it *doing* — "it under-buys
civil actions", "−4.5 workers by the endgame" — arrived at completely
independently, from the weights instead of from the games.

### The rule

A term whose `DEFAULT_WEIGHTS` value is **strictly positive** means "more of
this is better", so a trained value below zero is a sign inversion rather than
a strategy. The set is derived from the weight vector rather than hand-listed,
so it stays correct as the vector grows: it currently covers **57 of the 82
weights** and leaves every legitimately negative term alone — `rival_*`, the
`*_late` phase multipliers, `discontent`, `uprising`, `pop_cost`,
`wonder_remaining` and `end_turn_bias`.

> **`end_turn_bias` is not a bug.** It looks like one. Removing it was
> measured twice, five ways, and makes the bot much weaker (38.4%, 39.8%,
> 29.8%, 11.0%, and 39.8% on top of `hand_potential`, against a 50% null —
> see the comment on the weight in `engine/bots/weighted.py` and
> `docs/WASTED_ACTIONS.md` §6). The guard does not touch it, nothing in this
> loop rewards removing it, and nothing in this document should be read as
> licence to "fix" it.

`--weight-guard clamp|flag|reject` (default `clamp`) applies at champion load
— a resumed or warm-started file may already be degenerate — and to every
mutant at the moment it is proposed, so an inversion is caught when it appears
rather than hundreds of generations later. **Every occurrence is logged**, to
`guard_{K}p.jsonl` and into the generation record, so we can see whether the
pool-based loop still produces them.

### Starting vectors for the restart

* **Start from `DEFAULT_WEIGHTS`, at every player count.** With an empty state
  dir that is what happens; it needs no flag. Carrying an old champion forward
  carries its degeneracy.
* **4p especially must not be warm-started** from `experiments/champion_4p.json`
  — that is the nine-inversion vector above.
* `hand_potential`, the card-identity fix (master `5f39804`), is worth
  72.5% ± 4.4% at 2p but **4p currently regresses with it**, so `INIT_OVERRIDES`
  sets it to `0.0` at 4p only and lets the pool price it from there. 2p and 3p
  keep the 0.125 default.
* A warm start (`--init <path>`) still works and now warns that champions
  trained by the old mirror loop can carry sign-inverted weights.

## State on disk (restart safety)

The supervisor restarts the climber every hour and the Discord bridge kills
agents constantly, so **every** per-generation artefact is fsynced to disk.
All of it lives under `--state-dir` (default `experiments/league_state/`),
separate from the old loop's files so the two can run side by side:

| file | contents |
|---|---|
| `champion_{K}p.json` | current champion weights (atomic replace) |
| `state_{K}p.json` | gen, σ, since_accept, ablation cursor, opponents tested since the last full check, last full-check result |
| `generations_{K}p.jsonl` | one line per generation: subset used, full pool + weights, every candidate's per-opponent table, vetoes, ablation, regressions |
| `ladder_{K}p/gen*.json` | archived champions — these become `past` tier opponents |
| `fullcheck_{K}p.jsonl` | each full-pool check and its regressions |
| `ablation_{K}p.jsonl` | each single-weight ablation |
| `weight_credit_{K}p.json` | accumulated per-weight verdicts |
| `guard_{K}p.jsonl` | every sign-inversion the weight guard caught |

A kill costs at most the generation in flight; re-running the same command
resumes from `state_{K}p.json`.

## Launching

Detached, so it survives the agent being killed:

```bash
cd ~/tta-ai
nohup experiments/run_league.sh 2 48 6 2 12 4 >/dev/null 2>&1 &
#                               K  H  W L  B S
tail -f experiments/logs/league_2p.log
```

Positional arguments are `PLAYERS HOURS WORKERS LAMBDA BLOCK SUBSET
[ACCEPT_Z]`; anything after that is passed straight through to the trainer
(e.g. `--with-quiescent`, `--pool-weights book=4`).

A clean restart from the untrained weight vector is the default: with an empty
state dir the champion starts at `DEFAULT_WEIGHTS`. To warm-start from an
existing champion instead, `--init experiments/champion_2p.json`.

Useful dials:

| flag | default | meaning |
|---|---|---|
| `--block` | 12 | games per opponent per evaluation block |
| `--max-blocks` | 4 | cap on blocks spent on one candidate |
| `--subset` | 4 | opponents used for this generation's decision |
| `--accept-z` | 1.2816 | one-sided accept CI (90%) |
| `--veto-z` | 1.0 | a gate opponent vetoes when `edge + z·se < 0` |
| `--past-k` | 3 | archived champions in the pool, spread oldest→newest |
| `--full-check-every` / `--check-games` | 10 / 48 | full-pool re-check cadence and size |
| `--ablate-every` / `--ablate-k` / `--ablate-games` | 25 / 3 / 24 | weight-credit cadence, weights per cycle, games per opponent |
| `--no-legacy-ladders` | off | ignore the pre-existing `experiments/league_Np` archives |
| `--weight-guard` | `clamp` | `clamp` / `flag` / `reject` a sign-inverted value term |

`--block` is rounded down to a whole number of seat rotations (a multiple of
`--players`), because a partial rotation leaves the seats unbalanced and the
"same seeds, same seats" pairing stops being apples-to-apples. 12 works at
2p, 3p and 4p alike.

To read a running job without parsing the JSONL:

```bash
python3 -m experiments.hillclimb_league --report --players 2
```

which prints the last full-pool check and the accumulated weight-credit
ledger (`load-bearing` / `harmful` / `no-measurable-effect`, with mean edges).

## Smoke run

Proof the loop works end to end. **Not a strength claim** — the game counts
here are tiny by design (24 games per opponent, ±19% CIs), and the champion
is the untrained `DEFAULT_WEIGHTS` vector, four generations old.

```
python3 -m experiments.hillclimb_league --players 2 --workers 6 --lambda 2 \
    --block 12 --max-blocks 2 --subset 4 --max-gens 4 \
    --state-dir /tmp/league_demo --full-check-every 4 --check-games 24 \
    --ablate-every 4 --ablate-k 3 --ablate-games 12
```

### The full-pool check, and why this document exists

Champion vs every pool opponent, 24 games each, at 2p:

| opponent | tier | win rate | ±95% | null |
|---|---|---|---|---|
| `book` | book | **31.2%** | ±18.5% | 50% |
| `book2` | book | **33.3%** | ±19.3% | 50% |
| `past:ladder_2p/gen00000` | past | 50.0% | ±20.4% | 50% |
| `past:league_2p/gen00186` | past | 20.8% | ±16.6% | 50% |
| `past:league_2p/gen00221` | past | 12.5% | ±13.5% | 50% |
| `default` | floor | 50.0% | ±19.6% | 50% |
| `greedy` | floor | **100.0%** | ±0.0% | 50% |
| `random` | floor | 93.8% | ±9.0% | 50% |

Read the last three rows and the first two rows together, because that is the
entire point of this rebuild:

| how you aggregate | reads |
|---|---|
| unweighted mean over the pool | **49.0%** — dead par, nothing to see |
| tier-weighted (this design) | **36.8%** |
| vs BookBot alone | **31.2%** |

A bot that beats GreedyBot 100% of the time and RandomBot 93.8% of the time,
while losing to BookBot 31.2%, produces an **unweighted aggregate of 49.0%** —
indistinguishable from par. That is exactly the number the old loop would have
been reassured by. Tier weighting drags it to 36.8%, and the per-opponent
table plus the gate veto make the loss impossible to average away.

### The mechanics, observed

* **Gate veto fires.** `gen 2 cand 0 … VETO=['book']` — the candidate went
  +16.7% against a past champion and −33.3% against BookBot, and was rejected
  despite the aggregate.
* **Early stop fires.** A candidate whose mean edge was negative after the
  first block stopped at 48 games instead of the 96 the budget allowed.
* **Subset rotation covers the pool.** Generations 1–4 used
  `[mirror, book2, past/gen00000, …]`, `[mirror, book, default, greedy]`,
  `[mirror, book2, random, book]`, … — mirror and a gate opponent in every
  one, everything else cycling.
* **Four generations, four rejections.** Expected, and the correct behaviour:
  a random mutation off the untrained vector rarely beats its parent against a
  strong field at a 90% one-sided bar. The old loop's readiness to accept was
  a symptom, not a feature.

### Ablation

```
ablate auction_bid         edge=+0.0000 +/-0.0000 n=24 -> no-measurable-effect
ablate auction_committed   edge=+0.0833 +/-0.0739 n=24 -> harmful
ablate best_arena          edge=+0.0000 +/-0.0000 n=24 -> no-measurable-effect
```

Two weights whose removal changes *literally nothing* at 2p, and one whose
removal measurably **helps**. At n=24 these are weak claims — the point is
that the mechanism produces per-weight verdicts at all, which is what
`wonder_remaining` (a trained weight that measures at 27.6% ± 6.3% against a
25% null) motivated.

### The guard, observed live

On generation 1 of the 3p run, the very first mutant proposed **seven** sign
inversions at once, including `culture_rate = −2.28` — a negative weight on
the game's central engine:

```
[3p] gen 1 cand 0 weight guard (clamp): best_unit=-0.2426, culture_rate=-2.2751,
     hand_value=-0.1138, num_techs=-0.1153, prod_workers=-0.1079,
     special_techs=-1.4353, workers_early=-0.193
```

That is at σ=0.25 on generation 1. The old loop had no such check and ran for
hundreds of generations, which is how the 4p champion ended up with nine of
them baked in.

## Confirmed at 3p

**The loop produces valid generations at 3p, against every opponent in the
pool.** Two generations, all 15 opponents in the acceptance subset every
generation (`--subset 15`), `--block 12` — a whole number of seat rotations at
2p, 3p and 4p alike — and a full-pool champion check after each:

```
python3 -m experiments.hillclimb_league --players 3 --workers 3 --lambda 2 \
    --block 12 --min-blocks 1 --max-blocks 1 --subset 15 --max-gens 2 \
    --full-check-every 1 --check-games 36 --ablate-every 0 \
    --state-dir /tmp/smoke3 --weight-guard clamp --seed 20260726
```

Both generations rejected (`best_lo` −0.019 and −0.037 against a 90% one-sided
bar), 105.0 s and 67.9 s, 180 candidate games per candidate. Champion vs every
pool opponent, gen 1, **untrained `DEFAULT_WEIGHTS`**, 36 games each, 3p:

| opponent | tier | champion score | ±95% | n | wall clock | vs 33.3% null |
|---|---|---|---|---|---|---|
| `book` | book | **0.0%** | ±0.0% | 36 | 4.1 s | −33.3 pp |
| `book2` | book | **2.8%** | ±5.4% | 36 | 4.5 s | −30.6 pp |
| `var:culture` | variant | **2.8%** | ±5.4% | 36 | 4.0 s | −30.6 pp |
| `var:infra` | variant | **9.7%** | ±9.4% | 36 | 4.3 s | −23.6 pp |
| `var:military` | variant | **11.1%** | ±10.4% | 36 | 4.8 s | −22.2 pp |
| `var:science` | variant | **0.0%** | ±0.0% | 36 | 4.0 s | −33.3 pp |
| `var:tempo` | variant | **0.0%** | ±0.0% | 36 | 4.2 s | −33.3 pp |
| `var:wonder` | variant | **5.6%** | ±7.6% | 36 | 3.9 s | −27.8 pp |
| `past:ladder_3p/gen00000` | past | 33.3% | ±15.6% | 36 | 10.2 s | +0.0 pp |
| `past:league_3p/gen00042` | past | **0.0%** | ±0.0% | 36 | 11.7 s | −33.3 pp |
| `past:league_3p/gen00120` | past | **2.8%** | ±5.4% | 36 | 12.5 s | −30.6 pp |
| `default` | floor | 33.3% | ±14.6% | 36 | 8.2 s | +0.0 pp |
| `greedy` | floor | 55.6% | ±16.5% | 36 | 7.2 s | +22.2 pp |
| `random` | floor | **94.4%** | ±7.6% | 36 | 3.3 s | +61.1 pp |

Total 87 s for the check. Wall clock is on a 6-core box shared with two other
jobs, so treat the absolute numbers as an upper bound and the *ratios* as the
real result.

**Read this table as the answer to "is the pool the right difficulty?", and the
answer at 3p is: not yet, at the bottom end.** See
[The pool is too hard at the bottom](#the-pool-is-too-hard-at-the-bottom).

### Determinism

The whole generation replays exactly. The same command into a second empty
state dir reproduces generation 1 bit for bit — same acceptance subset, same
mutation operators (`group:economy`, `scatter`), same number of weights moved
(29, 20), same aggregate edge (−0.0174, +0.0052), same lower bound, **and every
one of the 15 per-opponent rows identical**, plus every row of the full-pool
check. The only difference between the two `champion_3p.json` files is the
generation counter; the weight vectors are byte-identical and both equal
`DEFAULT_WEIGHTS`.

So a crash costs exactly the generation in flight and nothing else, and any
two runs of this loop are comparable.

## Confirmed at 4p

**The loop produces valid generations at 4p, against every opponent in the
pool**, and it starts from `DEFAULT_WEIGHTS` rather than the degenerate
champion. Same command with `--players 4 --state-dir /tmp/smoke4`. The
override announces itself on the first line of the log:

```
[4p] init override hand_potential: 0.125 -> 0.0 (known 4p regression;
     the pool decides its value from here)
[4p] league trainer: 15 opponents, gen=0 sigma=0.250 state=/tmp/smoke4
```

Both generations rejected, 189.0 s and 180.7 s, 180 candidate games each,
zero arena errors. Champion vs every pool opponent, gen 1, 36 games each, 4p:

| opponent | tier | champion score | ±95% | n | wall clock | vs 25% null |
|---|---|---|---|---|---|---|
| `book` | book | **0.0%** | ±0.0% | 36 | 4.9 s | −25.0 pp |
| `book2` | book | **0.0%** | ±0.0% | 36 | 4.4 s | −25.0 pp |
| `var:culture` | variant | **0.0%** | ±0.0% | 36 | 4.5 s | −25.0 pp |
| `var:infra` | variant | **0.0%** | ±0.0% | 36 | 4.7 s | −25.0 pp |
| `var:military` | variant | **2.8%** | ±5.4% | 36 | 4.6 s | −22.2 pp |
| `var:science` | variant | **0.0%** | ±0.0% | 36 | 4.6 s | −25.0 pp |
| `var:tempo` | variant | **0.0%** | ±0.0% | 36 | 4.4 s | −25.0 pp |
| `var:wonder` | variant | **0.0%** | ±0.0% | 36 | 4.3 s | −25.0 pp |
| `past:ladder_4p/gen00000` | past | 25.0% | ±14.3% | 36 | 15.6 s | +0.0 pp |
| `past:league_4p/gen00051` | past | 16.7% | ±12.3% | 36 | 13.4 s | −8.3 pp |
| `past:league_4p/gen00103` | past | **0.0%** | ±0.0% | 36 | 21.9 s | −25.0 pp |
| `default` | floor | 38.9% | ±16.2% | 36 | 17.7 s | +13.9 pp |
| `greedy` | floor | 44.4% | ±16.5% | 36 | 13.5 s | +19.4 pp |
| `random` | floor | **83.3%** | ±12.3% | 36 | 4.8 s | +58.3 pp |

Total 126 s for the check — 1.4× the 3p check, as expected from the extra seat.

### The clean 4p start, verified

The starting champion written to `/tmp/smoke4/champion_4p.json` was compared
term by term against both candidate origins:

| claim | result |
|---|---|
| `== DEFAULT_WEIGHTS` with `hand_potential = 0.0` | **True** |
| `== experiments/champion_4p.json` (the degenerate one) | **False** |
| `science` | **+0.5**, against −6.089 in the degenerate file |
| any positive-default term left negative | **none** |
| `guard_4p.jsonl` written | **no** — nothing to report, which is the point |

An incidental corroboration of `INIT_OVERRIDES`: this vector is exactly the
`default` floor opponent except for `hand_potential`, and it scores **38.9%
± 16.2% against that opponent at 4p** on a 25% null. Weak evidence at n=36,
but it points the same way as the regression that motivated the override.

## Go / no-go for a multi-hour run

### The variant tier was one opponent, not seven (fixed)

Every roster class inherits `BookBot`'s `name` attribute, and that attribute is
literally `"book"`. `discover_variants` took the label off the class, so all six
variants **plus the abstract `VariantBot` base** were enrolled under the single
label `var:book`:

```
book(w=1.50) book2(w=1.50) var:book(w=0.36) var:book(w=0.36) var:book(w=0.36)
var:book(w=0.36) var:book(w=0.36) var:book(w=0.36) var:book(w=0.36) mirror ...
```

Labels are an opponent's identity everywhere downstream, so this was not
cosmetic:

* `acceptance_subset` de-duplicates by label, so **at most one variant could
  ever enter a generation's accept decision** — the entire point of the
  variant tier, and the largest non-`book` block of weight in the pool, was
  reduced to whichever single entry happened to be first;
* the per-opponent tables and the full-pool check are keyed by label, so the
  six variants' results overwrote each other and no per-variant number could be
  reported at all;
* the blind module scan also enrolled `variants/base.py`'s `VariantBot`, which
  is just BookBot v2 with the default profile, as a seventh "strategy".

Fixed in `hillclimb_pool.py`: the package's own `VARIANTS` registry is the
source of labels when it exists (its keys are distinct by construction and it
omits the base class), the module-scan fallback prefers a class's own `NAME`
over the inherited `name`, and `emit()` refuses to hand out a label twice.
The pool is now 15 distinct opponents at both 3p and 4p.

**This is the one finding that would have wasted the whole run**: the variant
tier is the reason the pool exists, and it was contributing one opponent.

### The pool is too hard at the bottom

The champion starts at `DEFAULT_WEIGHTS`, and against the entire gate tier
(`book` + the six variants) that vector scores **0–11% at 3p** on a 33.3% null
and **0–2.8% at 4p** on a 25% null — seven of the eight gate opponents are at a
flat 0.0% ± 0.0% at 4p. In per-generation scoring at `--block 12` that shows up
as this, on every candidate of every generation at both player counts:

```
book         12 games   cand 0.0%   champ 0.0%   edge +0.0000 GATE
book2        12 games   cand 0.0%   champ 0.0%   edge +0.0000 GATE
var:culture  12 games   cand 0.0%   champ 0.0%   edge +0.0000 GATE
var:science  12 games   cand 0.0%   champ 0.0%   edge +0.0000 GATE
```

**An edge of exactly zero is not a signal, it is the absence of one.** Both the
candidate and the champion lose every game, so the strongest, highest-weighted
half of the pool contributes literally nothing to the accept decision, and the
decision falls back onto `mirror`, `past` and `floor` — which is the weak-
baseline problem the league was built to replace, re-entering through the back
door.

The gate veto has the same hole: it fires on `edge + z·se < 0`, and an
all-zeros row gives `edge = 0`, `se = 0`. **A gate opponent nobody can beat can
neither reward nor veto.**

This is a *starting-vector* problem rather than a design flaw — the pool is
correctly calibrated for a champion that can occasionally win — but it decides
how the first hours are spent. Options, cheapest first:

1. **Raise `--block` for the gate tier.** At 12 games a 3%-per-game edge is
   invisible. The information is there; the sample is not.
2. **Score on culture margin, not win share, when win share is degenerate.**
   `arena.duel` already returns `culture_a` / `culture_b`, so "lost by 40
   culture instead of 90" is a gradient that exists on every single game.
3. **Warm-start the gate tier's weight.** Give `book`/`variant` a low tier
   weight for the first N generations and raise it once the champion is
   winning >10%, so early progress is measured where progress is measurable.

Option 2 is the real fix and the only one that does not trade away information.

### Where the wall clock goes

Per 36-game duel, on a contended box. The pattern is the same at both counts,
and it is a property of the *opponent's* evaluator, not of the player count:

| opponent kind | 3p | 4p | relative |
|---|---|---|---|
| `random` | 3.3 s | 4.8 s | 1.0× |
| `book`, `book2`, `var:*` | 3.9–4.8 s | 4.3–4.9 s | ~1.1× |
| `greedy` | 7.2 s | 13.5 s | 2.5× |
| `default` | 8.2 s | 17.7 s | 3.2× |
| `past:*` (trained `WeightedBot`s) | 10.2–12.5 s | 13.4–**21.9 s** | **3.5–4.4×** |

**The `past` tier is the wall-clock hog, and it is also the cheapest tier to
cut.** Three archived champions at ~3.5× the cost of a BookBot duel is ~30% of
a full check spent on the anti-cycling ladder, whose whole job is a
regression tripwire. `--past-k 2` or moving the `past` tier off the
per-generation subset and onto the full check only would buy back most of it.

`mirror` is the same story inside the acceptance loop: it is a `WeightedBot`
table and therefore the most expensive entry there, which is worth knowing
given it is also the least informative one.

Nothing else in the run is close. No opponent dominates by an order of
magnitude, so the run is not going to be held hostage by one duel.

### Nothing is beaten 100% of the time

The closest is `random`, at 94.4% ± 7.6% (3p) and 83.3% ± 12.3% (4p) — high,
but not saturated, and it never returned a 100% row in any check. `greedy` and
`default` are the only other opponents above their null. All three are `floor`
tier and already carry the pool's lowest weight (0.167), so none of them can
drag an acceptance on its own. **No pool member needs removing for being
trivially beaten** — the problem is entirely at the other end of the table.

### The weight guard, exercised live

Both modes were driven through the real loop rather than inspected.

**`reject` discards a sign-inverted mutant before it plays a game.** With
`--weight-guard reject --sigma-floor 0.9 --lambda 3` at 3p, generation 1
proposed three mutants and threw two of them away:

```
[3p] gen 1 cand 0 op=group:cards+happiness edge=+0.1000 lo=+0.0189 games=24
[3p] gen 1 cand 1 weight guard (reject): best_arena=-0.6097, best_library=-0.9721,
     best_mine=-1.237, civil_actions=-3.1595, culture_rate_early=-2.0423,
     leader=-1.549, workers=-3.0217, ... (13 inversions)
[3p] gen 1 cand 2 weight guard (reject): num_techs=-0.3846
[3p] gen 1 ACCEPT ... op=group:cards+happiness
```

Candidates 1 and 2 have no per-opponent table because they never reached the
arena, both were written to `guard_3p.jsonl`, and the clean candidate was
accepted normally. The guard is not merely logging.

**Two things to know before turning `reject` on for a long run:**

* It is *per-violation*, not per-severity. Candidate 2 above was discarded for
  a single `num_techs = −0.385`, a term whose default is +0.3. At σ = 0.9 that
  cost two thirds of the generation's candidates; the budget goes into
  proposing mutations instead of playing games. **`clamp`, the default, is the
  right mode for the long run** — it keeps the candidate and neutralises the
  term, and every occurrence is still logged.
* At the σ = 0.25 the loop actually runs at, no mutant proposed an inversion
  in any of the four smoke generations at 3p or 4p — `guard_{K}p.jsonl` was
  never created. The guard is cheap insurance, not a constant intervention.

**Gap: `flag` and `reject` do not neutralise a warm-started champion.** The
guard runs at champion load, but `guard_weights` only rewrites the vector in
`clamp` mode, so:

| `--weight-guard` | `--init experiments/champion_4p.json` | `science` after load |
|---|---|---|
| `clamp` | 8 violations logged, vector fixed | **+0.000** |
| `flag` | 8 violations logged, vector untouched | −6.089 |
| `reject` | 8 violations logged, vector untouched | −6.089 |

`--weight-guard reject` reads as "refuse to start from a degenerate vector",
and it does not do that — it trains from `science = −6.089` after telling you
about it. The clean-start path makes this moot (that is the intended launch),
but if a warm start ever happens under `reject`, the name will have lied.
Either make `reject` fatal at champion load or rename the champion-load
behaviour.

### Crashes and nondeterminism

Zero crashes across six smoke runs and ten generations. `arena.duel` reported
`errors=0` on every duel at both player counts — no game hit the move cap, no
worker died. The loop is bit-for-bit reproducible (see
[Determinism](#determinism)). The full-pool check re-seeds per generation, so
the *same* champion re-measured at gen 1 and gen 2 moved `greedy` from 55.6% to
72.2% — that is ordinary n=36 noise, not nondeterminism, but it does mean
`--check-games 36` is too small to trust a single regression report. The
default of 48 is not much better; use 100+ if the regression tripwire is meant
to fire on real regressions only.

## What this does and does not fix

It fixes the *measurement*: a candidate can no longer be accepted for beating
its own lineage, and losing to BookBot can no longer be averaged away.

It does not by itself make the bot beat BookBot. The pool tells the truth
about where the champion stands; closing the gap is the job of the engine
fixes, the evaluator features and the strategy work in
`docs/EXPERT_STRATEGY.md`. What changes is that from now on, progress reported
by the training loop is progress against opponents the training loop did not
invent.
