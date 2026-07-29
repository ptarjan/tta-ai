# The search-backed loop: what replaced the Stage-2 null

Read `docs/NEURAL_LOOP_NULL.md` first. In one line: the old loop's improvement
operator was the identity, so 41 hours of compute re-measured a single no-op 74
times. This document is the replacement, and the measurements that justify it.

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
`docs/BOT_ARCHITECTURE.md` 3 measured on linear weights (width=1 at 62.3%,
width=8 at 85.1% against the same vector at 1 ply).

### The same checkpoint against external opponents, 1 ply vs beam

Nothing was trained between these two columns. The only change is the search
wrapped around an unchanged `checkpoints/best.pt`.

| opponent (2p) | `neural` 1-ply | `nplan` width=2 | culture (beam) |
|---|---|---|---|
| linear champion | 0.095 +/- 0.041 | **0.425 +/- 0.155** | 125.5 vs 133.4 |
| BookBot | 0.150 +/- 0.050 | **0.550 +/- 0.156** | 112.2 vs 110.0 |

The 1-ply column is from `docs/NEURAL_EVAL.md` (n=200); the beam column is n=40,
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
  `docs/BEHAVIOUR_CLONE.md` 6.4: "the objective has to contain the outcome."

This is `docs/BOT_ARCHITECTURE.md` 3b's own prescription, previously untested:
"PlanBot evaluates only at turn boundaries, which is exactly the distribution a
boundary-only fit is trained on ... the fitted vector and PlanBot are a matched
pair by construction."

It also fixes a leak the old anchor had. `neural_rankdata.py` applied candidate
moves to the **real** state, and `tools/infoleak.py` measures 94.9% of `end_turn`
candidates at 2p drawing the real next civil card. The BookBot anchor was
therefore teaching the net to price the most-evaluated move in the game off a
card that is reshuffled away before the bot ever plays. Every child in both new
generators is applied to a determinized copy.

## 4. Stage 0: bootstrap from the strongest bot on record

`plan:champion_2p` scores 189.0 culture in a 2p mirror and 213.4 against BookBot
(`docs/BEHAVIOUR_CLONE.md`, `docs/LEAGUE_OBJECTIVE.md`) against a human mean of
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
| val split | first 15% of files, sorted | random **row** split | the sorted split silently made val a single data *source*; every reported number was a distribution-shift artefact (`NEURAL_LOOP_NULL.md` 3.2) |
| checkpoint selection | `--select pair` | `--select last` | `pair` was an agreement-with-incumbent meter, so selecting on it picked epoch 1 every time |
| epoch 0 | never printed | printed, plus a `VACUITY` line | the defect was invisible because nobody measured the untrained warm-start on its own targets |
| vacuity guard | none | hard warning at pair_acc >= 0.95 | a target the model already satisfies must fail loudly, not silently burn 41 hours |
| gate | 1-ply vs 1-ply | **beam vs beam** | gate the policy that actually ships |
| yardstick | vs linear champion at 1 ply | vs `plan:champion,width=8` | compare at equal search, per `docs/TRANSFER_TEST.md` |

The promotion rule is unchanged and was never the problem: promote iff the 95%
CI lower bound of the candidate's win rate clears 0.5.

## 6. Kill conditions, pre-registered

The old run had none, which is why it ran 38 hours past the point of being
informative. This one stops if any of these fire:

1. `DISAGREE < 0.02` for two consecutive iterations -- the search no longer
   overrules the net, so the operator has nothing left to give and the loop has
   degenerated back into self-imitation. The loop prints a `*** WARNING` line.
2. No promotion in **15** iterations **and** the pooled candidate win rate's CI
   excludes 0.5. That is the signature of a systematic regression, not of a
   hard search; it was visible by iteration ~10 last time.
3. `vs plan:champion` flat across 10 reference measurements.

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
