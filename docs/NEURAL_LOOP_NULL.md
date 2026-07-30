# The Stage-2 self-play loop: a 41-hour null, and why

**Status: closed negative.** 74 iterations, 20,700 gate games, **zero
promotions**, and the candidate was not merely no better -- it was reliably
*worse*. This document records what ran, the measurement that explains it, and
what replaced it. Raw logs and the frozen `best.pt` are archived at
`~/tta-ai-archive/neural_loop_v1_null_2026-07-29.tgz` (Mac) and
`C:\Users\micro\tta-ai\archive\run_v1_selfimitation_null\` (desktop).

## 1. What ran

`experiments/neural_loop.sh` at commit `6e5061e`, on the RTX 3090 desktop,
2026-07-27 14:26 to 2026-07-29 07:00. Per iteration: 480 self-play games from
`checkpoints/best.pt` (`neural_gen_iter.py`, epsilon 0.1), train a candidate
warm-started from `best.pt` on a 3-iteration replay window plus a BookBot
anchor, gate candidate-vs-best at n=300, promote iff the 95% CI lower bound
clears 0.5.

## 2. The result

| statistic | value |
|---|---|
| iterations | 74 |
| promotions | **0** |
| gate games | 20,700 |
| pooled candidate win rate | **0.4413** |
| pooled standard error | ~0.0035 |
| iterations with win > 0.5 | **2 of 69** |
| best single iteration | 0.4833 (still below 0.5) |
| it73 alone | 0.415 +/- 0.056, p = 0.0028, n=300 |

The pooled number is ~17 SE below the null. This is not a loop that failed to
find an improvement; it is a loop that found the same *regression* 74 times.

Reference curve, unchanged across the whole run because nothing ever promoted:
`best` vs linear champion **0.095**, vs BookBot **0.150**, vs `default`
**0.4275**. Culture in the gate hovered 78-90, against a human mean of 159.5
and `plan:champion` at 189-213.

## 3. Diagnosis

### 3.1 The training target was a fixed point of the model being trained

`neural_gen_iter.py` built its ranking pairs like this:

```python
vals = value.value(encs)
gi = max(range(len(vals)), key=lambda i: vals[i])   # greedy argmax
...
for j in rej[:krej]:
    pa.append(encs[gi])      # "chosen"
    pb.append(encs[j])       # "rejected"
```

The "chosen" label **is the net's own argmax**. The Bradley-Terry loss
`softplus(-(v(chosen) - v(rejected)))` therefore asks the net to prefer what it
already prefers.

Measured directly, on the run's own shards, with the incumbent and **no
training at all** (`probe.py` against `iterdata/it72_*, it73_*`):

```
SELF-PLAY (label = net's own argmax): 24 shards, 257,099 pairs
   pair_acc (incumbent, UNTRAINED) = 0.9764
   chosen-minus-rejected margin: mean +282.5 culture, median +121.2
```

97.6% of the ranking signal was already satisfied before the first gradient
step (the residual 2.4% is float16 shard storage and near-ties). The only
gradient available was **margin inflation** on preferences the net already
held -- and the margins were already +282 culture points wide on a target whose
own standard deviation is 54.

This is generalised policy iteration with the **identity** as its improvement
operator. AlphaZero's loop works because MCTS makes the target policy strictly
stronger than the raw net. With no search, there is nothing in the loop that
knows anything the net does not already know.

### 3.2 The "held-out metric" measured agreement with the incumbent

`neural_train_rank.py` splits validation **by shard, taking the alphabetically
first 15%**:

```python
files = sorted(set(files))
nval = max(1, int(round(len(files) * args.val_frac)))
vfiles, tfiles = files[:nval], (files[nval:] or files)
```

With `--data iterdata/it71_w*.npz ... rankdata/rk_*.npz`, `iterdata` sorts
before `rankdata`, so all 8 validation shards were pure self-play from the
oldest iteration in the window -- data whose labels are `best.pt`'s own argmax.
`val_pair_acc` was therefore, literally, *the fraction of decisions on which the
candidate still agrees with the incumbent*, and `--select pair` kept the
checkpoint that agreed **most**. That is why "best epoch" was always epoch 1:
the selection criterion rewards doing nothing.

This also dissolves the apparent paradox in `loop/train_it73.log`:

```
epoch 1  rank 0.3414  vloss 0.1516  val_pair_acc 0.9330  val_mae 55.2  *best
epoch 8  rank 0.2686  vloss 0.0752  val_pair_acc 0.9126  val_mae 75.9
```

Training loss includes the BookBot anchor pairs, which pull the net *away* from
the incumbent's argmax. Train loss down and agreement-with-incumbent down are
not anti-correlated by accident -- they are the same fact stated twice. Neither
number measures play strength, and neither ever did.

### 3.3 The value head was worse than predicting zero on its own distribution

From the same probe:

| | self-play rows | BookBot rows |
|---|---|---|
| target `yv` mean / sd | +2.1 / 54.4 | +2.7 / 75.1 |
| net prediction mean / sd | -31.2 / **156.1** | -9.1 / 88.8 |
| net MAE | **81.6** | 38.4 |
| MAE of the constant predictor `v=0` | **43.7** | 59.6 |

On the states it actually plays from, the value head was nearly twice as bad as
a constant. The BT ranking loss is scale-hungry and had inflated the output
standard deviation to 3x the target's. At 1-ply argmax the scale is irrelevant
to play, so this never showed up as a strength number -- but the value-loss
gradient was still perturbing the sibling ordering, which is the only thing
that does matter.

### 3.4 The BookBot anchor taught a feature the bot cannot see when it plays

`neural_rankdata.py` encodes each candidate child from the **real** state:

```python
def _child_enc(state, mv, seat):
    t = copy_state(state)
    actions.apply(t, mv, _TRIAL_RNG)
```

No determinization. `tools/infoleak.py` measures 94.9% of `end_turn` candidates
at 2p drawing the *real* next civil card. So the anchor taught the net to price
`end_turn` -- the single most-evaluated move in the game -- off a card that is
re-shuffled away before `NeuralBot` ever scores it (`NeuralBot` does determinize
at the root). Train/serve skew on the highest-frequency move class.

### 3.5 The loop had no state, so it was one experiment run 74 times

Because nothing ever promoted, `best.pt` never changed. The replay window was
therefore always three iterations of data generated by the *same frozen net*,
and each iteration recomputed approximately the same gradient step from the same
starting point and re-measured the same comparison. The 74 "iterations" are 74
noisy repeats of a single A/B: *`best.pt` versus `best.pt` plus one epoch*. The
run had no mechanism by which iteration 74 could differ from iteration 2.

Pooled over all of them, that single perturbation is worse by 0.059 +/- 0.007.
An information-free perturbation of a policy that sits at a local optimum of its
own lineage is expected to be worse, not neutral -- which is exactly the shape
of the observed 0.44, and why it never once looked like noise around 0.5.

## 4. What this does and does not prove

**Does:** self-play policy iteration with a 1-ply argmax as both the behaviour
policy *and* the improvement operator cannot improve a value net at this game.
It is the same trap the 1-ply linear hillclimb hit, and the same category error
`docs/BEHAVIOUR_CLONE.md` 6.4 names: an objective that does not contain new
information about the outcome cannot produce a better evaluator.

**Does not:** say anything about whether a neural evaluator can be good here.
Every number above is about the *loop*, not the *net*.

## 5. What replaced it

`docs/NEURAL_SEARCH_LOOP.md`. Three changes, each aimed at one finding above:

1. **The improvement operator is now a search.**
   `engine/bots/neural_plan.py` (`NeuralPlanBot`) runs PlanBot's whole-turn beam
   with the value net as the leaf, and `experiments/neural_gen_plan.py` labels
   ranking pairs with **the beam's** root choice, not the net's argmax. On
   identical linear weights that beam beats the 1-ply bot 88.6% +/- 3.1
   (`docs/BOT_ARCHITECTURE.md` 3), so the label is genuinely stronger than the
   model. The generator prints `DISAGREE=`, the fraction of decisions where the
   beam overrules the 1-ply argmax; it is the loop's health meter and a run
   whose `DISAGREE` decays to zero has gone vacuous again and must stop.
2. **The value rows are the states the leaf evaluator is actually served** --
   quiet, end-of-turn, determinized, war-substituted -- targeted at the final
   culture margin, per `docs/BOT_ARCHITECTURE.md` 3b ("PlanBot evaluates only at
   turn boundaries ... the fitted vector and PlanBot are a matched pair by
   construction") and `docs/BEHAVIOUR_CLONE.md` 6.4 ("the objective has to
   contain the outcome").
3. **The metrics were replaced.** Validation is a random ROW split, not the
   alphabetically-first shards, so it is the same distribution as training;
   `--select` defaults to the last epoch rather than to the checkpoint that
   agrees most with the incumbent; and the gate -- head-to-head play under the
   search that will actually be deployed -- is the only promotion criterion.

### 5.1 Results, 21 iterations in (2026-07-30)

This section described the design with no results attached; there are now
results, from `loop2/` on the desktop (`C:\Users\micro\tta-ai`), exactly as
recorded in `loop2/curve.tsv` and `loop2/master.log`. This is a genuine and
important contrast with the v1 null above, not a repeat of it.

| statistic | v1 (this document, §2) | loop2 |
|---|---|---|
| iterations | 74 | 21 |
| promotions | **0** | **4** (it1, it2, it4, it7) |
| `best` vs linear champion | 0.095 | see reference metric below (no single equivalent figure) |

The `DISAGREE` health meter named in item 1 above -- "a run whose `DISAGREE`
decays to zero has gone vacuous again and must stop" -- is the direct test of
whether this loop fell into the same trap as v1. It has not: `DISAGREE` has
held at **0.5375-0.5843 across all 21 iterations**, currently 0.5632. The beam
still overrules the net's 1-ply argmax on well over half of decisions, so the
training target is still demonstrably stronger than the net it labels, unlike
v1's 97.6%-already-satisfied self-play pairs (§3.1).

Reference metric, candidate vs `plan:champion` (same beam, linear evaluator),
n=240 per point from it10 on: it1 **0.4028** (culture margin -16.2), rising to
it12 **0.5042** (margin -1.4), it17 **0.4854** (-0.2), it20 **0.4833** (-2.1).

**But it has plateaued: no promotion since it7.** From it10 through it21 the
self-play gate (ARM A) BLOCKs every time -- win rate 0.476-0.5685, the 95% CI
lower bound never clears 0.5 -- while the anchor gate (ARM B) PASSes every
time. So: the loop reached rough parity with the linear (`plan:champion`)
champion by around it12-it20 and stopped improving there. That is parity, not
victory -- it is not yet better than the linear champion, and "approaching
parity" should not be rounded up to "better" when quoting this run.

## 6. Reusable lessons

* **Any self-play loop must state its improvement operator, and it must be
  measurable.** If the target policy is not demonstrably stronger than the
  network that generated it, the loop is a no-op with a compute bill. The cheap
  check is the one in 3.1: score the incumbent on its own training pairs
  *before* training. If pair accuracy is already ~1.0, stop.
* **A validation metric computed on labels the model produced is not a
  validation metric.** It is a conservatism meter, and early-stopping on it
  selects for inaction.
* **A flat learning curve is not the same as a flat curve at 0.5.** 74/74 below
  the null was visible from iteration ~10 and should have killed the run there;
  a pre-registered kill condition ("no promotion in 15 iterations AND pooled win
  rate CI excludes 0.5") would have saved 38 hours.
* **Check that the loop has state.** If the incumbent never changes, iteration N
  is not an experiment, it is a replicate.
