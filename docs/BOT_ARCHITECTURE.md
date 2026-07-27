# Bot architecture: what shape the TtA bot should be

Status: **in progress**, written incrementally so a crash loses nothing.
Branch `arch/bot-shape`, worktree `.claude/worktrees/arch`, based on master
`8e751cb`. Base game only (2015 "A New Story of Civilization"), no expansion.

Every number below is labelled **MEASURED** (I ran it, on this branch, with the
command given) or **INFERRED** (arithmetic or argument on top of measured
numbers) or **INHERITED** (someone else's number, with its n). Strength claims
need n>=400 and a 95% CI; anything smaller is called a lead, not a result.

Measurement environment: Mac mini 8,1, i5-8500B, 6 physical cores, no SMT.
CPython 3.14.6. All timings `time.process_time` under `nice -n 15`, because the
2p training arm is live and wall clock is meaningless. `TTA_JOURNAL` **off** for
every number here, so the copy path is the oracle (docs/PYPY.md 9.16).

Archaeology of the 22 existing docs is **not** in this file; it has its own
agent and its own document (`docs/ARCHAEOLOGY.md`). Where I lean on an existing
result I cite it with its n.

---

## 1. Engine cost census — the budget everything else has to fit in

MEASURED, `tools/cost_census.py`, 2 players, 10 games, champion weights:

```
nice -n 15 python3 tools/cost_census.py --players 2 --games 10 \
    --weights experiments/champion_2p.json
```

| quantity | 2p value |
|---|---|
| moves per game, WeightedBot mirror | 185 |
| of which real decisions (>1 legal move) | 163 |
| branching factor | mean **11.6**, median 10, p90 24, max 40 |
| decisions with only one legal move | 12.2% |
| decisions with a non-empty pending stack | 2.8% |
| `copy_state` | **52 us** |
| `legal_moves` | **49 us** |
| `apply` (mid-game, sampled) | ~80 us |
| `evaluate` (one linear eval, ~57 features) | **30 us** |
| **one forward-model step** (`legal_moves` + `apply`) | **~115 us** |
| full random playout from a mid-game state | **10.3 ms** (103 moves) |
| WeightedBot 1-ply self-play game | **0.451 cpu-s** (2.22 games/cpu-s) |
| RandomBot game | 0.019 cpu-s (52 games/cpu-s) |

The single number that governs every architecture decision:

> **The forward model runs at about 8,700 steps per CPU-second.**

For comparison, a search-based engine in a compiled language works at 10^6-10^7
nodes/s. We are three orders of magnitude below that, in a game whose branching
factor (11.6) is *lower* than chess's. **Throughput, not algorithm choice, is
the binding constraint**, and any design that needs thousands of simulations per
decision is not affordable on this box in this language.

MEASURED, profile of the raw forward model (`cProfile`, 30 random 2p games):
`legal_moves` (`actions._action_moves` and callees) is **60% of cumulative
time**; `apply` is 33%. Move generation, not move application, is the hot path.
That is unusual and it is good news: `legal_moves` recomputes `build_cost` /
`cost_of` / `_can_take_gated` for every card in the row on every call, and it is
called once per node. An incremental or memoised generator is the highest-value
engine optimisation available, and it would speed up *every* design below.

### What that budget buys

INFERRED from the above. A flagship A/B at n=400 2p games costs 400 x (per-game
cpu-s), run on 4 workers:

| per-game cost | x current bot | n=400 on 4 workers |
|---|---|---|
| 0.45 cpu-s | 1x | 45 s |
| 4.5 cpu-s | 10x | 7.5 min |
| 45 cpu-s | 100x | 75 min |
| 450 cpu-s | 1000x | 12.5 h |

So **~100x the current bot is the affordable ceiling for one flagship
experiment**, and ~10-20x is the ceiling for a bot you would actually *train*
(training needs thousands of games, not hundreds).

At 163 decisions/game, 100x = 45 cpu-s/game = **0.27 cpu-s per decision =
~2,400 forward-model steps per decision**. Hold that number; section 4 spends
it.

---

## 2. Honest diagnosis

### 2.1 The pending-stack defect is real, and it is already fixed in code but not in the trained bot

INHERITED and re-verified by reading the code: `apply()` returns with
`state.pending` non-empty for `offer_pact`, aggressions, colony `bid` and
action cards, so the trial state shows the whole cost and none of the gain.
`docs/PACTS_DIAGNOSIS.md` proves it by exact arithmetic (the `offer_pact` delta
is exactly -1.10445 and *no other feature moves*), which makes those moves
strictly dominated under **any** weight vector. That is not a tuning problem.

`engine/bots/quiescent.py` already resolves the stack before evaluating, at a
measured 1.16-1.29x cost (n=8 games/count, docs/DEEPER_SEARCH.md §3). Its
**strength was never measured** (§4/§5/§6 are still "RESULTS PENDING"; another
agent is running that A/B now) and, per docs/PYPY.md 9.14, `QuiescentBot`
occupies **0% of league training seats**. So the fix exists and the trained
champion has never seen it.

### 2.2 The trained champion's zeros are a compensation fingerprint — VERIFIED

MEASURED (read of `experiments/league_state/champion_2p.json`, the live arm at
**generation 344**): **13 of 82 weights are at exactly 0.0**.

```
ca_left  civil_actions  colonies  corruption_loss  culture_rate_early
hand_military  leader  pact_blocks_attack  pacts  rival_culture
rival_science_rate  strength_rel  uprising
```

Exact zeros do not arise from a multiplicative random walk; they are the
signature of `guard_weights` clamping a search that keeps trying to push these
*negative* (docs/CULTURE_GAP.md §12a measures the pile-up at Fisher p=0.012, and
§15's 300-run drift simulation shows the guard is necessary *and* sufficient for
it). The brief's claim is confirmed and is if anything understated: it is not
three weights, it is thirteen.

Read the list. Seven of the thirteen are **the entire model of the other
players** — `colonies`, `pacts`, `pact_blocks_attack`, `rival_culture`,
`rival_science_rate`, `strength_rel`, `hand_military`. The trained 2p champion
has, by search, deleted its representation of interaction. That is exactly what
you would expect from §2.1: if every interactive move is strictly dominated,
then every feature that prices interaction is pure noise to the fitness
function, and noise features get walked to whatever the guard allows.

The other two are worse. `civil_actions` and `ca_left` are the *resource the
game is metered in*, and both are pinned at zero. The mechanism is visible:
under 1-ply greedy scoring, the CA count is an asset in the feature vector and
every action spends one, so a positive `civil_actions` weight taxes every move
the bot could make. Since acting is worth an enormous amount (docs/WASTED_ACTIONS.md
§6 measures a pass-more-often bot at an **11.0% +/- 4.3% win rate, n=200**), the
climb correctly learns to stop taxing action — by deleting the feature. The
weight is not encoding strategy. It is cancelling a search artifact.

`end_turn_bias` is the same thing in the open: -14.44 at gen 344 against a
-3.0 default, and docs/WASTED_ACTIONS.md §1 measures the artifact it fights at
+12.6 evaluation points at 2p, rising to +26.3 in Age IV — a constant fighting a
term that scales with the economy.

**How much of the hill-climbing result is real?** Partly. The climb is
demonstrably optimising *something*: `docs/OPENING_AUDIT.md` §5 measures trained
vectors well above their untrained start (n=96/cell, so a lead not a result).
But `docs/BOT_ROSTER.md` (n=240/cell, 47,520 games) ranks the trained 2p
champion **10th of 12** and at 1.02x par, and `docs/STRENGTH_CHECK.md` measures
a ~200-line rule-based BookBot beating it **62.9% +/- 4.7% (n=400)**. The
honest summary is: **332+ generations of hill climbing have produced a bot that
loses to a hand-written priority list, and a measurable fraction of its
parameters are spent cancelling defects in its own search rather than describing
the game.** The training loop is not broken; it is optimising the wrong object.

There is also a sample-efficiency point that no doc makes. The climb's signal is
a win-rate comparison over a batch of games: on the order of **one bit per
batch**, for 82 parameters. A regression-based value learner extracts a
real-valued target from **every one of the ~185 states in every game**. The
current trainer is throwing away between three and four orders of magnitude of
signal. See §4.3.

### 2.3 A new defect: the search reads cards the player cannot know — MEASURED

`engine/state.py` keeps `civil_deck` and `military_deck` as full ordered lists
inside `GameState`, and `fastcopy.copy_state` copies them verbatim. There is no
information-set abstraction anywhere in the engine. So a trial `apply` that
draws a card draws **the real next card**.

MEASURED, `tools/infoleak.py --players 2 --games 15` (2,458 decisions, 32,437
candidate moves, champion weights):

| | share of candidates |
|---|---|
| candidate whose trial drew from a real deck or revealed a real row card | **5.46%** |
| ...of which: civil deck drawn | 5.03% |
| ...military deck drawn | 4.94% |
| ...own military hand grew | 4.73% |
| ...card row revealed a card not previously visible | 5.41% |
| **decisions with at least one such candidate** | **71.1%** |

By move kind: **`end_turn` is 94.9% leaky** (1735/1829). `prepare_event` 2.0%,
`choose` 9.1%; nothing else leaks at all. That is because `apply(("end_turn",))`
runs my end-of-turn economy, advances the turn *and* runs the next player's
`game.start_turn`, which replenishes the row from the real deck. The single
most-evaluated move in the game is the one that peeks.

**But the leak is currently inert, and that is the interesting part.**

MEASURED, `tools/leak_impact.py --players 2 --games 25 --k 8`: over **3,957
comparable decisions**, re-shuffling the unseen decks before scoring (8
determinizations, averaged) changed the bot's chosen move **0 times**
(95% CI [0, 0.09%]). The `end_turn` candidate's evaluation was **bit-identical**
across all 8 determinizations (spread sd 0.000 over 2,931 comparisons).

Why? MEASURED directly:

```
turn 6, my military hand ['Aggression: Enslave'], deck 39 cards
  k=0 hand [Enslave, Military Bonus, Crusades]        hand_mil_value 6  eval 37.517925
  k=1 hand [Enslave, Rats, Scientific Breakthrough]   hand_mil_value 6  eval 37.517925
  k=2 hand [Enslave, Heavy Cavalry, Aggression: Raid] hand_mil_value 6  eval 37.517925
  k=3 hand [Enslave, Phalanx, Aggression: Raid]       hand_mil_value 6  eval 37.517925
```

Four completely different military hands, one identical evaluation.
`features()` reduces the military hand to a count and a sum of age levels, so
**`Crusades` and `Rats` are literally the same feature vector**. This is the
exact mirror, on the military side, of the civil-hand card-identity blindness
that docs/WASTED_ACTIONS.md §7 measured as worth **+20 points of win rate
(69.6% +/- 4.5%, n=400)** when fixed. The civil side was fixed
(`hand_potential`); the military side was not.

Three consequences, and they are architectural rather than cosmetic:

1. **The leak is harmless today only because the evaluator is blind.** Any
   improvement to military-card valuation — which is the obvious next eval fix,
   and the one that AGGRESSION_FIX §B and CULTURE_GAP §2b both need — turns the
   leak live on the same day. The two must be fixed together.
2. **Any deeper search makes it worse.** Depth draws more cards. A search that
   plays two rounds ahead is reading two rounds of the real deck. **Any
   MCTS/rollout design must determinize from the start**; it cannot be
   retrofitted.
3. The observation set is unusually clean, which makes determinization cheap:
   everything about the *board* is public (this is a large part of TtA's
   design), and the only hidden state is (a) the order of the two draw decks and
   (b) other players' military hands. `weighted.features()` reads only public
   rival aggregates, so the evaluator itself does not cheat. Re-shuffling two
   lists is the whole of determinization: `engine/bots/plan.py:determinize`.

I do **not** claim the trained weights are tuned against a cheat. The measured
answer is the opposite: the cheat is currently unreadable, so it cannot have
influenced training. It is a loaded gun, not a fired one.

### 2.3b The evaluation does not predict the outcome — MEASURED, and this is the headline

Nobody in this repo appears to have asked the simplest possible question about
the evaluation function: **does it predict who wins?**

`tools/eval_quality.py` scores every state of a self-play game with a candidate
evaluation and asks, within each game-turn, whether the player it ranks higher
is the player who actually ends with more culture. Every scorer is judged on
**exactly the same pairs** — a pair counts only if all scorers separate it —
because raw culture is 0 for everybody in the opening and would otherwise be
silently graded on a later, easier subset.

MEASURED, 2p, champion-mirror self-play, **955 pairs from held-out games**
(the regression below never saw them):

| scorer | pairwise ranking accuracy |
|---|---|
| the trained champion's `evaluate` (gen 344, 82 weights, ~57 features) | **0.6984 +/- 0.0291** |
| `culture` — one number | 0.7005 +/- 0.0291 |
| `culture + 5 * culture_rate` — two numbers | 0.7037 +/- 0.0290 |
| **ridge fit on the SAME 80 columns, ~470 games of data** | **0.7843 +/- 0.0261** |

Two readings, both blunt:

1. **344 generations of hill climbing have bought nothing over counting
   culture.** The champion's whole evaluation is statistically indistinguishable
   from a single feature. I cannot say it is *worse* — the intervals overlap —
   but I can say it is not better, and that is enough.
2. **The hypothesis class is not the problem; the trainer is.** Identical
   features, identical linear parameterisation, identical 80 free parameters,
   fitted by ridge regression on a few hundred games instead of by 344
   generations of mutate-and-select: **+8.6 points of ranking accuracy, on
   held-out games, non-overlapping intervals.** Held-out R2 against the realised
   culture margin is 0.29-0.35.

The by-round table shows the shape of the failure. The champion's eval beats
raw culture in rounds 5-13 (0.604 vs 0.569 at round 6) — so the features *do*
carry early-game information — and then loses to it from round 15 on (0.846 vs
0.932 at round 19), because late in the game culture simply *is* the answer and
the other 79 terms are adding noise on top of it.

**Why this settles the search-versus-evaluation question.** A search is an
amplifier: it converts evaluation *differences* into move choices. The bot uses
`evaluate` to rank sibling states that differ by a single action — a far finer
discrimination than ranking two whole positions. If the evaluation cannot
reliably rank two whole positions, its marginal preferences between
nearly-identical positions are mostly noise, and searching harder on it
amplifies the noise. That is not a theory: it is exactly what
docs/WASTED_ACTIONS.md §6 measured five separate times, and §7 said so in
words — "what is broken is the bot's ability to tell one action from another".
This measurement puts a number on it.

Caveats, stated because this project's failure mode is exactly this: the
regression's target is the *realised* margin under the champion's own policy,
so it estimates V^pi, not V*; ranking accuracy is not a win rate; and a
value function that predicts well can still induce a bad greedy policy. The
A/B in §7 is the test that counts. But the champion-versus-culture row needs no
such caveat — that is a straight comparison of two evaluations on the same data.

### 2.4 So what is actually wrong with the bot

Ranked by the evidence, most-supported first:

1. **The evaluation cannot tell one card from another** — fixed for civil cards
   (+20 pts, n=400), unfixed for military cards (MEASURED above), and unfixed
   for the *row* (the bot cannot see what it could take next turn at all).
2. **The evaluation has no representation of conflict.** No feature reads
   `war_declared_by_me` / `wars_declared_on_me` (AGGRESSION_FIX §B), so wars are
   a structural zero: 0.00 wars per game in 360 games. Every aggression scores
   exactly `hand_military` below `pol_pass` — the cost of the card leaving hand,
   and nothing else.
3. **Scoring happens at whatever horizon `apply` happens to stop at**, so
   candidates are not comparable: `end_turn` has banked a production phase that
   nothing else has, and pending-stack moves have banked a cost with no gain.
4. **A whole turn's worth of actions is chosen one action at a time.** The game
   meters players in civil actions (2-7 a turn); the bot commits action 1 before
   it knows what actions 2..4 will be.
5. **The trainer's signal is ~1 bit per batch for 82 parameters**, and a
   measurable share of those parameters is spent cancelling 1-4.

Note that 1 and 2 are *evaluation* problems and 3-5 are *search and training*
problems, and the only two interventions that have ever produced a large
measured gain in this repo (`hand_potential`, +20 pts; BookBot's hand-written
priority list, +12.9 pts over the champion) were both about **knowing what a
card is worth**, not about looking further ahead. Every attempt so far to fix
the horizon alone made the bot *worse* (five thresholds, two methods, n=200-400,
docs/WASTED_ACTIONS.md §6). That is the strongest prior in this project and any
search proposal has to survive it.

---

## 3. Prototype: PlanBot

`engine/bots/plan.py`. A beam search over whole-turn action *sequences*, scored
at one fixed horizon, on a determinized state. It attacks 2.4's items 3, 4 and
2.3 simultaneously, because they are the same bug seen from three sides.

* **One horizon.** A candidate is not a move, it is a *sequence ending in
  `end_turn`*, and it is scored on the state immediately after my turn ends.
  "End turn now" is just the length-1 sequence. The `end_turn` flattery cannot
  exist because every leaf has banked exactly one production phase. No
  `end_turn_bias` term is used at all.
* **Plan, not action.** Beam width `W` (default 8) over sequences, so the bot
  prices "increase population, then build the lab, then take the card" as one
  object.
* **Determinized.** The two draw decks are re-shuffled before the search.
* **Quiet leaves.** Pending decisions are drained with 1-ply picks for whoever
  the decider is, so the strictly-dominated class of §2.1 is visible here too
  (the same idea as `QuiescentBot`, reimplemented inside the beam).

Why this is not the `HorizonBot` that already failed at 29.8% (n=400): that bot
rolled each *single* candidate forward through production but still chose one
action at a time. It paid the whole cost of removing the flattery (which
docs/WASTED_ACTIONS.md §6 shows was acting as a move-quality filter) and bought
none of the lookahead. PlanBot only makes sense *with* the sequence search, and
it inherits `hand_potential`, which did not exist when HorizonBot was measured.

MEASURED cost, 2p mirror vs WeightedBot, champion weights: **~280 expanded
nodes per decision**, 7.5 cpu-s for a PlanBot-vs-WeightedBot game pair, i.e.
roughly **16x the 1-ply bot** — comfortably inside the affordable band of §1.

### Result

n=400 A/B running; results are appended here when it lands. The A/B is
search-only: both sides load the *identical* weight file, so anything measured
is attributable to the search and not to tuning. Note that this handicaps
PlanBot, because those weights were hill-climbed to compensate for the very
artifacts PlanBot removes (§2.2).

<!-- RESULT PENDING -->

---

## 4. The architecture options, priced

All costs INFERRED from the §1 census; the multipliers are against the current
1-ply WeightedBot at 2p (0.451 cpu-s/game, 163 decisions/game, 115 us per
forward-model step, 30 us per evaluation).

### 4.1 Vanilla MCTS with full random rollouts — NO-GO

One simulation costs one full playout: **10.3 ms** measured from a mid-game
position. At S simulations per decision the game costs `163 * S * 10.3 ms`.

| S | cpu-s/game | x current | n=400 A/B on 4 workers |
|---|---|---|---|
| 100 | 168 | 373x | 4.7 h |
| 400 | 671 | 1490x | 18.6 h |
| 1000 | 1679 | 3720x | 46.6 h |

With a branching factor of 11.6, S=100 is barely one visit per child — it is
noise, not search. The budget that would actually be a search (S>=1000) is
**46 hours for a single A/B** and is unusable for training. Rule it out.

### 4.2 MCTS with truncated rollouts + linear eval at the leaf — affordable once, not trainable

A depth-`d` truncated rollout costs `d * 115 us + 30 us`. At d=10 that is
1.18 ms/sim.

| S | cpu-s/game | x current | n=400 on 4 workers |
|---|---|---|---|
| 100 | 19 | 43x | 32 min |
| 400 | 77 | 170x | 2.1 h |

Affordable for one experiment. Not affordable for a self-play training loop,
which needs thousands of games per generation.

### 4.3 MCTS with eval-only leaves (no rollout) — the affordable MCTS

One simulation is one `apply` + one `legal_moves` + one `evaluate` plus Python
tree bookkeeping: call it **200 us**.

| S | cpu-s/game | x current | n=400 on 4 workers |
|---|---|---|---|
| 400 | 13 | 29x | 22 min |
| 1600 | 52 | 116x | 1.4 h |

This is the only MCTS variant that is both affordable and trainable. But price
what it *buys*. With branching 11.6, S=1600 spread uniformly is 2.9 plies; MCTS
concentrates, so a realistic principal line is 6-10 plies. A 2p player-turn is
**~4.6 moves** (185 moves / ~40 player-turns). So an affordable MCTS sees
**my turn plus roughly the opponent's reply** — one turn more than §4.4 gets for
2-7x less money.

In a game whose payoffs land 5-15 turns out (a wonder started in round 12
completes 59% of the time; started round 13+, 14% — docs/HEURISTICS.md, 235
builds over 120 games), one extra turn of depth is not where the value is. And
at 3-4 players the scalar backup breaks: you need max^n or paranoid, and the
opponent's reply has to be modelled by *its own* turn-level search, so the cost
squares.

**Verdict: MCTS is the wrong tool for TtA on this hardware in this language.**
Not because MCTS is bad — because the forward model is three orders of magnitude
too slow relative to the game's payoff horizon. The leverage in TtA is in the
leaf evaluation, not in the tree. This is the opposite of chess and it follows
from the game's structure (economic engine-building, low branching, long payoff
horizon, almost all information public) rather than from an analogy.

The measured record backs this: every intervention in this repo that produced a
large gain was about *knowing what a thing is worth* (`hand_potential`, +20
points, n=400; BookBot's priority list, +12.9 points over the champion, n=400),
and every intervention that changed *when/where to look* without changing what
the evaluator knows made the bot worse (five thresholds, two methods, n=200-400).

**Re-open this if the engine gets ~10x faster.** At 10x, §4.3 at S=400 costs 3x
current and becomes trainable. `legal_moves` is 60% of the forward model
(MEASURED, cProfile) and recomputes every card's cost from scratch on every
call, so an incremental generator plus the journal (1.44x measured on
WeightedBot, docs/PYPY.md 9.15) plausibly gets 2.5-4x. 10x needs a compiled
core. **A 10x forward-model speedup is worth more than any algorithmic
cleverness available here**, exactly as the brief suspected.

### 4.4 Turn-level beam search with one horizon and determinization — the prototype

`PlanBot`, §3. MEASURED at **~16x** current. Affordable for experiments *and*
for training. It is the smallest change that removes three defects at once
(horizon asymmetry, single-action myopia, information leak) and it is the only
search proposal that survives the §6 prior, because it buys lookahead rather
than only removing the flattery.

### 4.5 A learned value function — the largest available lever

Two things are true at once about the current trainer:

* the hypothesis class is a **linear function of 80 engineered features**, and
* it is fitted by a hill climb whose signal is a win-rate comparison over a
  batch of games — on the order of **one bit per batch, for 82 parameters**.

A regression on outcomes extracts a real-valued target from **every state of
every game**. MEASURED: `tools/gen_value_data.py` emits ~76 rows per 2p game
(one per player per turn boundary), so 500 games is ~38,000 labelled rows for
225 cpu-s of self-play. That is three to four orders of magnitude more signal
per CPU-second than the climb.

Crucially, the design matrix can be made **exactly** `evaluate`'s
parameterisation — base feature, `_early` = (1-L) * feature, `_late` = L *
feature — so a fitted coefficient vector is a **drop-in weight file**. A
head-to-head between a fitted vector and a climbed vector, in the same bot, is
therefore a clean test of the *trainer* with everything else held fixed. That
experiment is `tools/fit_value.py` and it is the second prototype; results
below.

Beyond linear, on a 6-core CPU box with **no numpy, no sklearn and no torch in
the engine's interpreter** (verified: all three absent; the engine is stdlib by
design):

* **Linear (80 params).** Inference 30 us. Fit by stdlib Cholesky. Available
  tonight.
* **Linear + hand-picked crosses** (e.g. rate features x rounds-left, strength
  x rival strength). Inference ~50 us. Cheap and keeps the eval smooth, which
  matters: a search needs to *rank* near-identical positions.
* **Small MLP**, 80->32->16->1 = ~3,200 multiply-adds. Pure-Python inference is
  ~500 us, i.e. **16x the linear eval and 4x a whole search node** — it would
  dominate everything. Train it under numpy in a venv and export JSON, but it is
  only affordable inside a *shallow* bot (1-ply or PlanBot with a small beam),
  not inside MCTS.
* **Gradient-boosted trees**, 100 trees x depth 4 = ~400 comparisons, ~40 us
  inference: the best accuracy-per-microsecond on tabular features. The risk is
  that a tree ensemble is piecewise constant, so near-identical candidate moves
  get **identical** scores and the search cannot rank them — which is precisely
  the failure mode docs/WASTED_ACTIONS.md §7 diagnosed ("taking any card scores
  ~0, and identical for every card in the row"). Use it only with a linear term
  added back for tie-breaking.

The honest ordering is: **fix what the features can see, then fit them properly,
and only then consider a bigger hypothesis class.** A nonlinear model over
features that cannot tell `Crusades` from `Rats` (§2.3) will learn a better
function of the wrong inputs.

### 4.6 What to do with the existing linear eval

Keep it. It is a working, fast (30 us), smooth ranking function, and it is the
right *prior* and *rollout policy* for anything built on top. Nothing in the
diagnosis argues for throwing it away; the arguments are all about (a) what it
can see, (b) where it is called, and (c) how its coefficients are chosen.

---

## 5. The external anchor (Phase 4) — the cheapest credible option

`docs/EXTERNAL_AIS.md` §7 recommends 10-15 logged games against the CGE app's
Hard AI, costed at **12-18 hours of the user's time**. That is still the only
way to get a *win rate* against a named external opponent, and its §6c power
analysis is right that a win rate is the expensive statistic (33% -> 42% needs
~220 games).

There is a much cheaper anchor that nobody has costed, and it follows from
§6c's own observation that **score margin is the dense statistic**:

> **Use the distribution of final culture scores in real human games as an
> absolute yardstick.**

Mean final culture per player is a population statistic of *how well the table
plays*, and it needs no bot-versus-human matches at all. Our 2p champion scores
~124 mean culture and BookBot ~155 (n=400, docs/STRENGTH_CHECK.md). If human 2p
base-game finals average, say, 210, then both bots are far below competent human
play and we know it for the cost of reading a few hundred game summaries. If
they average 150, BookBot is already at human average.

This is worth doing because it answers the question the win-rate harness does
*not*: **how strong are these bots on an absolute scale**, which is the question
this project has never been able to answer.

**Correction, landed while this was being written.** `docs/BGO_PILOT.md`
(commit `42cfdb7`) establishes that BGO's finished-games index is **not**
readable without a login — `docs/EXTERNAL_AIS.md` §5a reached its "readable"
conclusion inside an authenticated session and that does not generalise. So BGO
is not a free data source for either the anchor or the corpus; it is an
account-and-consent source. That does not kill the anchor, it changes where the
numbers should come from. In descending order of cheapness:

1. **A published aggregate.** BGG threads already cited in
   `docs/EXPERT_STRATEGY.md` reference a "30k-game data with skill-filtering"
   analysis. A published score distribution costs zero scraping, zero user time
   and zero credentials. This should be checked first.
2. **The tournament corpus already in hand.** `docs/EXPERT_STRATEGY.md` is built
   on 39 games from 3 Internationals and 3 Intermezzos, scored by civil actions
   spent per card. If those records carry final scores, the anchor is already
   sitting in the repo.
3. **A user-authorised BGO metadata pull**, which is now an explicit ask for
   credentials and consent, not a free read.

There is also a weak anchor available immediately, from expert commentary
rather than data, and it is worth stating because it points the same way:
`docs/EXPERT_STRATEGY.md` records a competent Age III culture rate of **10-15
per round** over ~6 rounds, Age III wonders at **20-35 culture each**, single
events worth **20+**, and "it is very common to be down by **100 points** and
still comfortably win". Against that, our 2p champion's **124** mean final
culture and BookBot's **155** (n=400, docs/STRENGTH_CHECK.md) look like a
sub-competent table. That is INFERRED from prose, not measured, and it is
exactly the kind of claim this project keeps getting burned by — but it is the
only absolute reading available today and it is not flattering.

Cost, and what it asks of the user:

* It needs **final scores only**, not journals — the summary/outcome metadata.
* n=200-500 finished 2p and 3p base-game games is enough: the per-game sd of
  final culture is 40-50, so a 500-game sample pins the population mean to about
  +/- 4 culture.
* **It requires the user's decision to run any scrape at all.** I have not
  scraped anything, and a pilot is being run separately. The ask is a yes/no on
  a rate-limited metadata pull, not hours of play.
* Caveats to state up front: mixed skill pool, unknown timeout/abandon rate,
  and score inflation from long games. All of those bias the human mean in
  knowable directions and none of them destroy the comparison.

**Recommendation: do the score-distribution anchor first** (cheap, needs one
decision from the user, answers the absolute-strength question), and hold the
12-18 hour Hard-AI harness in reserve for when there is a bot worth spending
the user's evenings on.

### On the BGO corpus as *training* data

The coordinator is right that `docs/EXTERNAL_AIS.md` §7 ranked move-level BGO
logs #6 ("defer") on a reason — "the choice set is unrecoverable" — that is an
objection to **imitation learning** (which needs `(state, choice set, chosen
move)`) and not to **value learning** (which needs only `(state, outcome)`).
The ranking should move up. But not to #1, for three reasons that are about cost
rather than principle:

1. **Reconstructing state is not parsing, it is replaying.** Our ~57 features
   need workers per tech, resources, food, science, happiness, government,
   wonders, units, colonies. Getting them from a journal means executing the
   logged moves through our engine in a *forced-replay* mode that bypasses
   `legal_moves` — a new engine entry point, a mapping from every journal event
   type to an engine mutation, and a reconciliation loop for the events the
   journal summarises rather than states. That is real engineering, and it is
   also a superb correctness test of the engine, which is a genuine bonus.
2. **The reconstructed state is partial in exactly the place we most need it.**
   Military hands are hidden until played and the card row is never logged, so
   we would learn a value function over a *reduced* observation. That is fine
   for a public-information value head and useless for the military-card
   blindness of §2.3.
3. **A value function fitted to mixed-skill human play estimates V under that
   population's policy, not V\*.** It is a good *initialiser* and a good
   *regulariser* against self-play blind spots — the coordinator's argument for
   it is sound — but it is not a substitute for policy iteration, and AlphaGo's
   own experience was that the human-data value net was the weaker of the two.

So: **self-play value regression first** (free, immediate, already built),
**human score distribution as the absolute anchor second** (cheap, one user
decision), **BGO value learning third** (real, expensive, do it once self-play
has visibly plateaued), **BGO imitation last** (the original objection stands).

---

## 5b. Threats to validity, stated before the results

Written before the win rates landed, so it cannot be tuned to them.

**On the ranking-accuracy result (§2.3b).**
* The states come from champion-versus-champion self-play, so they are on the
  champion's *own* state distribution. If anything that favours the champion.
* The ridge fit was trained on the same distribution and evaluated on games it
  never saw, split **by game seed**, never by row — rows from one game share an
  outcome, so a row-level split would leak.
* At 2p, `margin(p0) = -margin(p1)`, so a within-turn pair asks exactly "which
  of these two players is ahead", which is the right question for an evaluator
  in a race. It does not test discrimination between two states one action
  apart, which is what the bot actually does — see the next bullet.
* **Ranking accuracy is not a win rate.** A value function that predicts the
  outcome well can still induce a bad greedy policy, and the fitted V estimates
  V^pi for the champion's policy rather than V*. The §7 duels are the test that
  counts, and if they come back null the honest conclusion is "better predictor,
  no better policy", not "the measurement was wrong".
* The fitted vector is fitted **only on turn-boundary states**, but a 1-ply
  `WeightedBot` compares *mid-turn* states, so it is being asked to extrapolate.
  `tools/gen_value_data.py --rows every` exists to close that gap and is the
  obvious follow-up if the 1-ply duel disappoints. `PlanBot`, by contrast,
  evaluates only at turn boundaries — exactly the distribution the fit was
  trained on — so the fitted vector and `PlanBot` are a matched pair by
  construction.

**On PlanBot.**
* It runs on weights hill-climbed to compensate for the artifacts it removes
  (§2.2), so the A/B *understates* what the design is worth. A fair number needs
  a re-fit, which is M4.
* `end_turn_bias` is deliberately not applied. Against a champion tuned to
  -14.44 this is a genuine change of policy, not just of search.
* The determinization re-shuffles only the two decks. Rival military *hands* are
  also hidden and are not re-dealt; `weighted.features` reads no rival hand, so
  nothing currently reads them, but a future evaluator would need them handled.

**On the cost census.**
* All timings were taken on a box running 20-25 runnable processes on 6 cores.
  `time.process_time` is immune to that for *totals*, but cache pressure is not,
  so absolute microsecond figures may be 10-20% pessimistic. Ratios are safe.

**What I did not verify.** I did not re-derive the archaeology (owned by
`docs/ARCHAEOLOGY.md`), the combat rules conformance (`docs/COMBAT_AUDIT.md`),
the coverage census (`docs/COVERAGE_AUDIT.md`) or the QuiescentBot strength A/B
(`docs/DEEPER_SEARCH.md` §4). Numbers taken from those and from the older docs
are marked INHERITED with their n.

---

## 6. Staged roadmap

Each stage is independently verifiable, ships on its own, and is a no-op for the
next one if it fails. Ordered by measured-evidence-per-hour, not by ambition.

| # | stage | what it fixes | verification | cost |
|---|---|---|---|---|
| **M1** | **PlanBot** — turn-level beam, one horizon, determinized | §2.4 items 3, 4 and the §2.3 leak | n=400 A/B vs champion, identical weights | built; A/B running |
| **M2** | **Military card identity** (`mil_potential`, the mirror of `hand_potential`) | §2.4 item 1 — the blindness MEASURED in §2.3 | n=400 A/B *and* behaviour counts (aggressions/game must leave 0) | ~1 day |
| **M3** | **War / aggression features** — write `docs/AGGRESSION_FIX.md` §B's fix | §2.4 item 2 | behaviour counts + n=400 no-harm | ~1 day |
| **M4** | **Value regression replaces hill climbing** (`tools/fit_value.py`), then iterate as approximate policy iteration | §2.4 item 5 | fitted-vs-climbed n=400 *in the same bot* | built; pending data |
| **M5** | **Engine throughput**: incremental `legal_moves` + land the journal | unlocks §4.3 | `tools/cost_census.py` re-run; target >=3x | ~2-3 days |
| **M6** | **Nonlinear value head** (linear + crosses, then MLP) | expressiveness, once the inputs are right | holdout R2 *and* n=400 | after M2-M4 |
| **M7** | **Absolute anchor** (§5) | tells us where we actually are | one number with a CI | one user decision |

Ordering rules that fall out of the measurements, and that should be treated as
hard constraints:

* **M2 must not ship without M1's determinization.** The moment the evaluator
  can read military-card identity, the 94.9%-leaky `end_turn` candidate starts
  reading the real future (§2.3). Today it is inert; after M2 it is a cheat.
* **M4 must not be run on top of the climbed vector.** Those 13 zeros and the
  -14.4 `end_turn_bias` are fitted to the artifacts M1 removes
  (docs/WASTED_ACTIONS.md §11 makes the same argument about `hand_value_late`).
  Fit from data, do not seed from the champion.
* **M5 is the only stage that changes what is *possible*** rather than what is
  good. If it lands at >=3x, re-open §4.3.
* Nothing here needs the expansion, an external AI, or a GPU.

---

## 7. Is the goal reachable? A straight answer

**"Beat the app AI handily": plausible, and it should be the working target.**
`docs/EXTERNAL_AIS.md` §1 concludes — from community consensus, not
measurement — that the CGE app's Hard AI is a weighting/scoring heuristic, i.e.
**the same architectural class as `WeightedBot`**, with a ceiling around a strong
club human. A bot that fixes the evaluation defects in §2.4 and searches a turn
at a time should beat that class. It is unproven only because we have never
played it.

**"Beat all humans, as Stockfish does for chess": no. Not with these
resources.** Blunt version:

* **The bots are weak on an absolute scale.** The most damning number in the
  repo is that a ~200-line hand-written priority list beats 344 generations of
  hill climbing **62.9% +/- 4.7% (n=400)**, and the trained champion places
  **10th of 12** in a 47,520-game round robin at 1.02x par. The learned bot has
  not yet reached the level of a competent human writing down what they know.
  We are not near the human frontier; we are below the "wrote the obvious
  heuristics down" line.
* **The compute gap is 4-5 orders of magnitude in the wrong place.** Stockfish
  searches ~10^8 nodes/s. Our forward model runs at **8.7 x 10^3 steps/cpu-s**
  (MEASURED). TtA rewards depth far less than chess does, which helps — but not
  by four orders of magnitude.
* **The data engine is, surprisingly, the *least* of the problems.** At 2.22
  games/cpu-s x 4 cores we can produce **~760,000 self-play games per day** with
  the 1-ply bot. 10^7 games — AlphaZero-lite territory for a small model — is
  ~13 days of the box. That is genuinely feasible. It is only feasible with a
  *cheap* bot, though: at PlanBot's 16x it becomes 200+ days, so a strong
  training loop needs M5.
* **What superhuman would actually require:** a compiled or heavily optimised
  forward model (10-100x), a learned nonlinear value function with a real
  training loop rather than a hill climb, and 10^7-10^8 self-play games — plus
  an external anchor to know when you have got there. Items 1 and 3 are
  multi-week projects on this box; item 2 is the work in M4/M6; item 4 does not
  exist yet at all.

**Quantifying the gap, in the only currency we have.** There is no external
anchor, so this is a calibrated statement rather than a number: the distance
from today's champion to BookBot is one investigation's worth of work
(`hand_potential` closed a similar-sized gap in one day). Expert commentary
implies competent human play scores roughly **1.5-2x** the champion's mean
culture (§5, INFERRED from prose). Strong tournament humans are further again.
So the honest shape is: **several BookBot-sized improvements to reach competent
human, and an unknown but larger number beyond that** — with the crucial caveat
that we cannot currently measure any of it, which is why §5 and M7 matter more
than their apparent glamour.

**Recommendation.** Set the goal to *beat the app's Hard AI handily and beat
BookBot decisively*, get an absolute anchor so the claim means something, and
treat "beat all humans" as a direction rather than a destination. Doing this
well is worth more than doing it ambitiously; this project's recurring failure
is not lack of ambition, it is confident measurement of the wrong thing.

