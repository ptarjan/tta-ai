# The search-backed loop: what replaced the Stage-2 null

Read [`docs/NEURAL_LOOP_NULL.md`](NEURAL_LOOP_NULL.md) first. In one line: the old loop's improvement
operator was the identity, so 41 hours of compute re-measured a single no-op 74
times. This document is the replacement, and the measurements that justify it.

> **The pipeline is Rust now (2026-08-06).** Every stage below used to be a
> Python script importing `engine/`; all of them have been ported and the
> Python is deleted. The DESIGN in this document is unchanged -- the same
> improvement operator, the same two gate arms, the same kill conditions --
> so read it as written and substitute the commands from this table. Section
> 9 records what genuinely changed.
>
> | stage | was | is |
> |---|---|---|
> | stage 0 teacher | `plan_teacher_gen.py` | `rankdata --teacher plan:CHAMP,width=8` |
> | generation | `neural_gen_plan.py` | `rankdata --teacher nplan:BEST,width=8,nodes=1200` |
> | training | `neural_train_rank.py` | `neuraltrain` |
> | gate arm A | `neural_eval.py` x8 shards | `neuraleval --a nplan:cand --b nplan:best` |
> | gate arm B | `neural_eval.py` x8 shards | `neuraleval --a nplan:cand --b plan:CHAMP,width=8` |
> | shard pooling | `pool_summary.py` | *retired -- there are no shards* |
> | the bots | `engine/bots/neural_plan.py` | `rust/src/bots/neural/plan.rs` |
>
> The driver is still `experiments/neural_search_loop.sh`, still under the
> `tta_neural_loop` Scheduled Task, and still holds every lock, heartbeat and
> atomic-install guarantee section 8 describes.

## 1. The lever, measured on this net

`engine/bots/neural_plan.py` (`NeuralPlanBot`) is PlanBot's whole-turn beam with
the value net as the leaf: one thing swapped, everything else identical
(determinize at the root, quiesce the pending stack, war-lookahead
substitution, score every candidate at the same end-of-my-turn horizon).

Search-only A/B, **the same checkpoint on both sides**, 2p, seat-balanced,
`checkpoints/best.pt` (the frozen incumbent of the failed run):

| candidate search | vs | win rate | culture | n |
|---|---|---|---|---|
| `nplan` width=1, nodes=200 | `neural` (1-ply) | **0.763 +/- 0.131** | 108.1 vs 70.1 (margin +38.0) | 40 |
| `nplan` width=2, nodes=300 | `neural` (1-ply) | **0.700 +/- 0.144** | 93.9 vs 61.5 (margin +32.4) | 40 |
| `nplan` width=8, nodes=1200 | `neural` (1-ply) | **0.825 +/- 0.119** | 94.3 vs 54.4 (margin +40.0) | 40 |
| `nplan` width=2 | `nplan` width=2 (mirror control) | 0.543 +/- 0.167 | 85.8 vs 81.5 | 35 |

The null is 0.500 and the mirror control sits on it. Even width=1 -- which adds
no lookahead at all, only the horizon equalisation and the quiescence -- is
worth ~26 points. This reproduces, on a neural evaluator, the ladder
[`docs/BOT_ARCHITECTURE.md`](BOT_ARCHITECTURE.md) 3 measured on linear weights (width=1 at 62.3%,
width=8 at 85.1% against the same vector at 1 ply).

### The same checkpoint against external opponents, 1 ply vs beam

Nothing was trained between these two columns. The only change is the search
wrapped around an unchanged `checkpoints/best.pt`.

| opponent (2p) | `neural` 1-ply | `nplan` width=2 | culture (beam) |
|---|---|---|---|
| linear champion | 0.095 +/- 0.041 | **0.425 +/- 0.155** | 125.5 vs 133.4 |
| BookBot | 0.150 +/- 0.050 | **0.550 +/- 0.156** | 112.2 vs 110.0 |

The 1-ply column is from [`docs/NEURAL_EVAL.md`](NEURAL_EVAL.md) (n=200); the beam column is n=40,
at the *cheap* width -- width=8 is stronger still. A net that lost to BookBot
five times in six now splits with it, and one that won 1 game in 10 against the
linear champion now takes four.

**This is why the redirect is to depth.** The old loop spent 41 hours trying to
move the left-hand column by fractions. The right-hand column was available for
free, from the same weights, by changing the search.

## 2. The target now carries information

The health meter is `DISAGREE`: the fraction of sampled decisions where the beam
overrules the net's own 1-ply argmax. That fraction is the entire information
content of the ranking label.

| loop | improvement operator | fraction of pairs the untrained net already gets right |
|---|---|---|
| old (`neural_gen_iter.py`) | the net's own argmax (identity) | **0.9764** |
| new (`neural_gen_plan.py`) | `NeuralPlanBot` beam, width 8 | 0.29 (`DISAGREE=0.7101`) |

Measured on the desktop, 2 games, 138 sampled decisions: the beam chose a
different root move than the raw net on **98 of 138**. That is a target the
network does not already satisfy, which is the whole point.

`neural_gen_plan.py` writes only the disagreeing decisions by default (the
agreeing ones are the vacuous 97.6% again) and always keeps the 1-ply argmax as
a negative -- it is the single most informative rejected sibling, being exactly
what the net would have played and what the search says not to.

## 3. The value rows moved to the distribution the leaf is served

A beam leaf evaluator is asked about **quiet, end-of-my-turn positions on a
determinized state, with the war substitution applied**, and about nothing else.
The old pipeline trained it on pre-move, mid-turn, non-determinized states.

* `neural_gen_plan.py` takes its value rows straight out of
  `NeuralPlanBot._score_many`, so they are by construction the positions the
  beam prices.
* `experiments/plan_teacher_gen.py` does the same for the **teacher**: it
  subclasses `PlanBot`, hooks `_score`, and records the champion's own scored
  positions.
* The target is the mover's eventual **final culture margin** --
  [`docs/BEHAVIOUR_CLONE.md`](BEHAVIOUR_CLONE.md) 6.4: "the objective has to contain the outcome."

This is [`docs/BOT_ARCHITECTURE.md`](BOT_ARCHITECTURE.md) 3b's own prescription, previously untested:
"PlanBot evaluates only at turn boundaries, which is exactly the distribution a
boundary-only fit is trained on ... the fitted vector and PlanBot are a matched
pair by construction."

It also fixes a leak the old anchor had. `neural_rankdata.py` applied candidate
moves to the **real** state, with no determinization at all. `tools/infoleak.py`
measured, on `WeightedBot` at 2p, that 94.9% of `end_turn` candidates draw a
card -- a draw count, not proof of a leak by itself, since it is unaffected by
whether the root is determinized ([`docs/AGGRESSION_RATE.md`](AGGRESSION_RATE.md#9a-the-bigger-leak-was-in-the-sentence-above-not-the-one-below-it) §9a). But
`neural_rankdata.py`'s undeterminized root makes the two coincide the same way
they do for `WeightedBot`: a draw there really is the real next civil card.
The BookBot anchor was therefore teaching the net to price the most-evaluated
move in the game off a card that is reshuffled away before the bot ever plays.
Every child in both new generators is applied to a determinized copy.

## 4. Stage 0: bootstrap from the strongest bot on record

`plan:champion_2p` scores 189.0 culture in a 2p mirror and 213.4 against BookBot
([`docs/BEHAVIOUR_CLONE.md`](BEHAVIOUR_CLONE.md), [`docs/LEAGUE_OBJECTIVE.md`](LEAGUE_OBJECTIVE.md)) against a human mean of
159.5 -- the only configuration in this repo that clears the human. The value
net had never seen a single state that bot evaluates.

So the loop's first act is supervised: generate `plan:champion` self-play, keep
its scored turn-boundary positions and its search-backed root choices, and fit
the net to them (`--select last --val-split rows`). That gives the beam a leaf
evaluator matched to it by construction, instead of starting self-play from a
net whose 1-ply lineage the beam does not share. Stage 0 is skipped once
`checkpoints/best_search.pt` exists, so a reboot resumes the loop.

## 5. The metrics were replaced

| what | old | new | why |
|---|---|---|---|
| val split | first 15% of files, sorted | random **row** split | the sorted split silently made val a single data *source*; every reported number was a distribution-shift artefact ([`NEURAL_LOOP_NULL.md`](NEURAL_LOOP_NULL.md) 3.2) |
| checkpoint selection | `--select pair` | `--select last` | `pair` was an agreement-with-incumbent meter, so selecting on it picked epoch 1 every time |
| epoch 0 | never printed | printed, plus a `VACUITY` line | the defect was invisible because nobody measured the untrained warm-start on its own targets |
| vacuity guard | none | hard warning at pair_acc >= 0.95 | a target the model already satisfies must fail loudly, not silently burn 41 hours |
| gate | 1-ply vs 1-ply | **beam vs beam** | gate the policy that actually ships |
| yardstick | vs linear champion at 1 ply | vs `plan:champion,width=8` | compare at equal search, per [`docs/TRANSFER_TEST.md`](TRANSFER_TEST.md) |
| yardstick cadence | promotion iterations only | **every iteration** | iterations 3, 6 and 8 have `-` in `vs_planchamp`; an anchor curve with holes in it cannot show a trend |
| yardstick n | 72 | **240** | at n=72 the CI is +-0.11 and every anchor score ever recorded sits inside every other one's interval |
| missing measurement | written as `win=0.0000` | written as `-`, never a number | a reference run that completed zero games was recorded as a 0-72 defeat (row 4 of `curve.tsv`) |
| promotion rule | self-play only | **self-play AND anchor** | see below |

### 5.1 The promotion rule is no longer self-referential

It used to be: promote iff the 95% CI lower bound of the candidate's
beam-vs-beam win rate over the incumbent clears 0.5. That rule is necessary and
was never wrong, but on its own it is a closed loop -- it only ever asks
whether the candidate beats the last thing this same process produced, and
drift satisfies that as readily as learning does.

It did. Over iterations 1-7 self-play culture climbed 116 -> 143 and four
candidates promoted, while the fixed anchor (`plan:champion_2p`, width 8) did
not move: 0.4028, 0.3472, 0.3680, 0.3958, all n=72, all inside each other's
+-0.11 intervals, culture margin -16.2, -21.1, -18.7, -14.1. The loop was
measuring its own reflection.

This is the same treadmill [`docs/CULTURE_GAP.md`](CULTURE_GAP.md) describes for the weight hill
climb, and it is *not* the v1 failure -- v1 never promoted at all
([`docs/NEURAL_LOOP_NULL.md`](NEURAL_LOOP_NULL.md)). v2 promotes and gets stronger against itself. The
question this change answers is whether it is getting stronger against anything
else.

**Read the flatness narrowly.** It is flatness *within this run's first seven
iterations*, not a verdict on the neural line, which has climbed a long way
against the champion:

| stage | vs the champion | source |
|---|---|---|
| MC value regression, 1 ply | 0.070 +- 0.035 | [`docs/NEURAL_EVAL.md`](NEURAL_EVAL.md) §results |
| pairwise ranking, 1 ply | 0.095 +- 0.041 | [`docs/NEURAL_EVAL.md`](NEURAL_EVAL.md) Stage 1b |
| this loop, beam vs beam | ~0.40 +- 0.11 | `loop2/curve.tsv`, iterations 1-7 |

0.095 -> ~0.40 is most of the distance from "not a contender" to "a contender",
and the search-backed target plus the Stage 0 bootstrap is what bought it. What
has stalled is the *last* stretch, and the reason it is hard to see is that the
gate measuring it was pointed at the loop's own output. Nothing here says the
approach is a dead end; it says the instrument was.

So promotion now requires **both**, logged separately as `ARM A` and `ARM B` in
`loop2/master.log` so a blocked promotion always names the arm that blocked it:

* **Arm A, self-play** -- the existing test: candidate vs incumbent, beam vs
  beam, CI lower bound over 0.500.
* **Arm B, anchor** -- no significant regression against the frozen champion:
  the candidate's win rate vs `plan:champion_2p` must not sit more than one
  standard error *of the difference* (`sqrt(se_cand^2 + se_inc^2)`, se = ci/1.96)
  below the incumbent's own score against that same opponent. The incumbent's
  score is carried in `loop2/anchor_best.txt`, written only on promotion, so it
  always describes whatever `best_search.pt` currently is.

Arm B is deliberately **not** "beat the champion". The net is ~14pp behind it;
a beat-it gate would freeze the run rather than fix it. It is "do not get worse
than the net you are replacing on the one opponent that cannot move". At n=240
the band is ~0.045, so a candidate has to give up about 4.5pp on the anchor
before arm B blocks it -- wide enough to pass real progress, narrow enough that
seven iterations of sideways drift do not all pass.

This respects [`NEURAL_LOOP_NULL.md`](NEURAL_LOOP_NULL.md) 5.3 ("the gate -- head-to-head play under
the search that will actually be deployed -- is the only promotion criterion").
Arm B is also head-to-head play under the deployed search; it is a second
opponent, not a second *kind* of evidence. No offline metric gates anything.

Arm B fails **closed** if its measurement produced no games, because promoting
on an unevaluated gate is the hole this change exists to close. It fails
**open** in exactly one place -- seeding, when no incumbent baseline exists yet
-- and says so in the log when it does.

**The floor is loose for the first iteration or two after a restart.** The band
is `sqrt(se_cand^2 + se_inc^2)`, so it is only as tight as the *incumbent's*
estimate, and the incumbent's estimate is whatever precision it was measured at
when it was promoted. Carrying forward iteration 7's real anchor (0.3958
+-0.1130, n=72) against a candidate measured at n=240 gives

    floor = 0.3958 - sqrt((0.063/1.96)^2 + (0.1130/1.96)^2) = 0.3298

against a steady-state floor of ~0.351 once both sides are n=240. So a
candidate that has given up ~6.6pp on the anchor can still promote immediately
after the upgrade, where later it would need to stay within ~4.5pp. This is
correct behaviour -- a gate must not reject on a baseline it does not trust --
but it means **arm B gates more weakly for the first iteration or two than it
will at steady state**, and an early promotion should not be read as evidence
the anchor is holding. The seeding run measures the incumbent at n=240
precisely to shorten that window to one iteration.

### 5.2 When the anchor stops being the same anchor

Arm B works because `plan:champion_2p` cannot move. It moves anyway if the
*evaluator* underneath it changes: the champion is a weight vector, and
`engine/bots/weighted.py` decides what those weights are applied to. Commit
`96a5db2` started pricing `effects.culture` and `effects.science`, keys the
evaluator had been dropping outright. Same JSON, different player -- worth
+59.5% head-to-head at 2p.

So an anchor score measured before such a change and one measured after are
**not on the same ruler**, and averaging them or plotting them as one series
invents a trend. Whenever the evaluator changes:

1. Truncate `loop2/anchor_best.txt`. The loop re-seeds it automatically on its
   next start -- that is the fail-open seeding path above, and it is the same
   `anchor_run` at the same `REFN=240` that produced the number being replaced,
   so the new baseline is comparable to the candidates it will gate.
2. Write a comment row into `loop2/curve.tsv` recording what changed and what
   the old baseline was. A line whose first character is `#` is an annotation:
   it does not advance the iteration counter and the schema migration passes it
   through verbatim (pinned by `tests/test_neural_loop_gate.py` arm 5). Do
   **not** delete the pre-change rows -- they are valid measurements of a
   different configuration, and the marker is what stops a reader from reading
   across them.

The same applies to the cached pool weights, for the same reason (`6968256`).

## 6. Kill conditions, pre-registered

The old run had none, which is why it ran 38 hours past the point of being
informative. This one stops if any of these fire:

1. `DISAGREE < 0.02` for two consecutive iterations -- the search no longer
   overrules the net, so the operator has nothing left to give and the loop has
   degenerated back into self-imitation. The loop prints a `*** WARNING` line.
2. No promotion in **15** iterations **and** the pooled candidate win rate's CI
   excludes 0.5. That is the signature of a systematic regression, not of a
   hard search; it was visible by iteration ~10 last time.
3. `vs plan:champion` flat across 10 reference measurements. These are now one
   per iteration rather than one per promotion, so this condition is reached in
   10 iterations (~5.5h) instead of "whenever enough promotions happen to
   accumulate ten of them" -- which over iterations 1-8 meant four measurements
   in nine hours. Flat is judged at n=240 (+-0.063), not n=72 (+-0.11); the old
   sizing could not have distinguished flat from a 5pp climb either way, which
   is the more embarrassing half of this condition's history.

Arm B of the promotion gate (§5.1) is not a kill condition and must not be
mistaken for one. It stops a *regression* on the anchor from being promoted; it
does not stop a run that is merely going nowhere. Condition 3 is still what
ends such a run.

## 7. Cost

Measured on the desktop (i7-14700F, one torch thread per process):

| configuration | s/game |
|---|---|
| `neural` 1-ply mirror | 1.3 |
| `nplan` width=1, nodes=200 | 5.9 |
| `nplan` width=2, nodes=300 | 11.8 |
| `nplan` width=8, nodes=1200 | 17.7 (175 nodes/decision) |

So the beam is ~14x the 1-ply bot, which is why the gate is fanned out over
disjoint seed ranges (`experiments/pool_summary.py`) instead of run serially.

**Wall clock per iteration**, read off `loop2/master.log`'s `== ITER` timestamps
on the desktop (2p, `GENW=GATEW=6`, six clean iterations):

| phase | before | after |
|---|---|---|
| gen + train + self-play gate | ~22m40s | ~22m40s |
| anchor match | ~3m at n=72, on promotion iterations only | ~10m at n=240, **every** iteration |
| **iteration total** | 23m (no anchor) / 26m (anchor) | **~33m** |

That is the price of a yardstick that can resolve the effects we are chasing,
and it is the one cost of this change. It stays under the 45min `STALE_BEAT`
reap threshold in the driver with room to spare -- but note that threshold is
about the gap between *heartbeats*, not iteration length, and `beat_wait`
covers the anchor fan-out like every other phase, so a longer anchor cannot
cause a false reap.

Two throughput notes that cost real time to find:

* **Pin torch to one thread.** Torch grabs all 28 logical cores for a 200x1897
  GEMM that takes 4 ms. Eight unthrottled probe processes measured **0.25 core
  each** on a box already running the league arms. `--threads 1` everywhere.
* **The tensor conversion, not the net, is the cost of neural search.** For
  200x1897 rows: `torch.tensor(list_of_lists)` 28 ms, `torch.from_numpy(
  np.asarray(...))` 16 ms, forward pass 4-12 ms. `NeuralValue.value` now goes
  through numpy.

## 8. Durability (unchanged contract with the box owner)

The owner games on this machine and that outranks everything here.

* `experiments/gpu_guard.py` runs as the `tta_gpu_guard` Scheduled Task and is
  the **single writer** of `C:\Users\micro\tta-ai\PAUSE`. It detects a game by
  GPU-process path, writes `PAUSE`, and hard-kills our python to free VRAM.
* The loop only ever **reads** `PAUSE`, and blocks before launching any python
  (`wait_if_paused`, checked before generation, before training, before the
  gate, and inside every fanned-out worker).
* `tta_neural_loop` is a Scheduled Task with a logon trigger, hourly repetition,
  `RestartOnFailure` (999 x 1 min), `MultipleInstancesPolicy=IgnoreNew` and
  `<Priority>7</Priority>` = below-normal, which every child python inherits.
  So the loop survives reboot, crash, a guard kill, and the SSH session ending,
  and never outranks a game.
* `rm -f checkpoints/cand.pt` before each training run: a guard kill mid-train
  used to leave the *previous* iteration's candidate on disk to be gated as if
  it were this one's.

### 8.1 The heartbeat is continuous, and promotion is atomic

The relaunch guard reaps a driver whose `loop2/driver.beat` is older than
`STALE_BEAT` with `kill -9`. As first written, `driver.beat` was touched only at
script start, once per iteration, and every 30s while parked for a game — so the
heartbeat measured *iteration boundaries*, and `STALE_BEAT=2700` was implicitly a
bet that no iteration ever exceeds 45 minutes. The script's own estimate is
20–40 min, i.e. about **5 minutes of margin**, and that estimate is not a ceiling:
generation retries up to 4x on incomplete workers and stage-0 teacher generation
up to 6x, each retry a full regeneration. **A 1441-second quiet stall was already
observed and logged during Stage 0** — over half the threshold consumed by a
single phase.

The failure this produces is not two concurrent drivers (the takeover path
SIGKILLs the recorded PID before writing its own, so they never overlap). It is
the reaper `kill -9`-ing a live-but-slow driver *mid-write* at the checkpoint
promotion, leaving a **truncated `checkpoints/best_search.pt`** — the one artifact
the whole run is building.

Two fixes, deliberately independent:

1. **`beat_wait` replaces every bare `wait`.** It touches `$BEAT` every 15s for
   as long as any background worker is running, and `beat_run` gives the two
   foreground trainers the same coverage by backgrounding them and waiting
   through `beat_wait`. `low()` also beats at each launch. Every long-quiet
   stretch of this driver is one of those waits, so `$BEAT` now means "the driver
   is executing", not "an iteration boundary just went by". `STALE_BEAT` is
   thereby **decoupled from iteration length entirely**: it now only has to
   exceed the gap between two beats (15s), which is ~180x of slack instead of
   ~1.1x.
2. **`install_ckpt` replaces `cp` at every site that writes `$BEST`.** It stages
   into `${dst}.tmp.$$` — the same directory, hence the same filesystem, or the
   rename would degrade to a copy — and then `mv -f`s it over the destination.
   A rename within a filesystem is atomic, so a reader (and a `kill -9`) sees
   either the whole old file or the whole new one, never a half-written one.
   Stale `checkpoints/*.pt.tmp.*` from a kill mid-staging are swept at startup.
   Windows can refuse to rename over a file another process still holds open, so
   the rename is retried 3x and only then falls back to a plain `cp` with a loud
   `WARNING` — that fallback is exactly the old behaviour, never worse.

**Raising `STALE_BEAT` was considered and rejected**: it swaps one guess about
iteration length for another, and since retries make iteration length unbounded
there is no correct value. Fix 1 removes the coupling that made the number load-
bearing; fix 2 removes the consequence of getting it wrong anyway. If you ever
see a false reap, do not raise the number — find out why the beat stopped.

Editing this script while it is running requires care: **bash reads a script
lazily by byte offset**, so an in-place edit can make the live driver jump to a
wrong offset and execute garbage. Write the new content to a temp file in the
same directory and `mv` it over the original; the rename swaps the inode and the
running bash keeps reading the old one to completion. The change takes effect on
the next relaunch, not immediately.

---

## 9. What the Rust port changed (2026-08-06)

Four things, none of them the design.

**1. The checkpoints are not torch files.** `neuraltrain` writes this repo's
own format (`rust/src/bots/neural/net.rs`); nothing can confuse it with a
`.pt`. The loop's paths are `.ckpt`, so a box still holding a Python-era
`checkpoints/best_search.pt` will not pick it up: stage 0 fires and the
lineage restarts from the frozen champion. That is correct rather than
unfortunate, and there is deliberately no converter — a lineage half of whose
provenance is torch and half is not is exactly what §5.2's comment-row
convention exists to stop someone plotting straight through. The loop writes
that comment row into `curve.tsv` itself, once, on first run.

**2. No fan-out, so no `pool_summary.py`.** The eight-worker fan-out over
disjoint `--seed0` ranges existed because CPython could not use the box any
other way. One Rust process with `--threads` does the whole match. Everything
that existed to reassemble those shards — `ci_cluster`, `se_cluster`, `chi2`,
`overdispersed`, and the forty lines of comment warning against dividing
`ci_cluster` by 1.96 — is gone with the cause.

**3. The interval clusters on the deal, not the shard.** `neuraleval` reports
`se=` from `rust/src/stats.rs`, clustered on the deal. This is a strictly
finer clustering of the same games (a shard contained whole deals; a deal is
the smallest unit the seat pairing makes independent) and it is computed from
the games themselves rather than from six summary numbers. It is published
*separately* from `ci=` for the same reason `pool_summary.py` published
`se_cluster` separately from `ci_cluster`: `ci` already carries a `t_{k-1}`
critical value, so a caller reconstructing an SE by dividing it leaves
`t_{k-1}/1.96` behind. Arm B reads `se=` directly and divides nothing.

Because it is a *different estimator*, the incumbent baseline moved to a
differently named file, `loop2/anchor_best_deal.txt`. A shard-clustered SE
must not be read into a deal-clustered floor and there is no arithmetic that
converts one to the other; a new name makes that impossible rather than
merely discouraged. The file is simply absent on the first run and is seeded
by measuring the incumbent once, exactly as §5.1's seeding path already did.

**4. Value rows come from the beam's own leaves.** §3 argued that the value
rows must be the positions the beam actually prices. In Python that was a
`_score_many` override on a `NeuralPlanBot` subclass; in Rust it is
`bots::plan::Bank`, an explicit collection hook on both beams whose `push`
takes a closure, so an ordinary `pick` pays nothing for it. `rankdata` reports
which distribution a run used on its DONE line (`values=search-leaves` or
`values=pre-move-state`), so a shard's distribution is a recorded fact rather
than something a reader has to infer from which script wrote it.

### `DISAGREE` abstains rather than reporting zero

§6 makes `DISAGREE < 0.02` a pre-registered kill condition. A teacher with no
net cannot take a 1-ply argmax to compare its search against, so `rankdata`
prints `DISAGREE=NA`, never `0.0000`, and the driver refuses to test an `NA`
against the threshold. "The search never overruled the net" and "there was no
net to overrule" are two different facts, one of which is a reason to stop the
run, and a bare zero cannot tell them apart.

### The gaming guard is retired

`experiments/gpu_guard.py` freed VRAM by hard-killing torch. There is no GPU
and no torch in this pipeline, so it has nothing to detect and nothing to
kill. `register_tasks.ps1` now *deregisters* `tta_gpu_guard` rather than
registering it. The `PAUSE` flag it used to write survives as an operator
control — the loop still reads it before every worker launch, so `touch PAUSE`
parks training and deleting the file resumes it. Automatic CPU politeness is
what it always actually was: the task's Priority 7, inherited by every child,
plus the loop's `--threads` budget, which leaves cores for the hill-climb
league.

### Two bugs the port found by running the driver

* `sfield` used `-\?`, a GNU sed extension. BSD sed reads it as a literal `?`,
  so on any non-GNU box **every** field parsed as empty and every measurement
  looked like a run that had produced no games — a portability bug wearing
  the exact costume of a real failure, hidden all along by the desktop's GNU
  sed. A bracket expression means the same thing to both.
* The driver `cd`'d to a hard-coded `~/tta-ai`, which is why it had never been
  run anywhere else. It now resolves the repo from its own location, as
  `experiments/rust_league.sh` already did.
