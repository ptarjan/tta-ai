# Neural value evaluator — build log and findings

Status: **Stage 1 in progress.** Branch `neural-eval`, worktree
`/Users/pt/tta-ai-neural`. Base game only (2015 "A New Story of Civilization"),
2 players unless stated. This document is for the next engineer: negatives and
nulls included, not a glossy report.

The bet (owner's instruction): replace the hand-crafted linear evaluator with a
trained neural value net — the AlphaZero-shaped path, justified by our having a
FAST, VALIDATED simulator (docs/SCORE_VALIDATION.md: scoring exact vs 1011 human
games) which is exactly what self-play needs. The linear evaluator has a proven
ceiling — it is a linear function of ~89 features and cannot express nonlinear
value, and it is BLIND to card identity in the row and in hands
(docs/INFORMATION_AUDIT.md), which is most of the real skill of the game.

## The load-bearing prior warning (read before trusting any number here)

`docs/BOT_ARCHITECTURE.md` §2.3b/§3b measured that a value function fit by
**Monte-Carlo regression on outcomes**, dropped into the 1-ply greedy search,
gets **monotonically worse as its prediction improves**: a ridge fit reached
0.81 held-out ranking accuracy and won **0 of 400** against the champion, and a
lambda ladder showed win rate 0.53 → 0.00 as ranking accuracy rose 0.67 → 0.81.
The reason: a greedy bot needs the *difference between sibling states one action
apart*, and squared-error regression spends ~none of its capacity there.

So: **ranking accuracy and val loss are NOT the deliverable. Head-to-head play
strength is.** Every strength claim in this doc is an out-of-sample duel with
error bars. The neural net's edge over that prior failure is that it (a) is
nonlinear and (b) sees *card identity* (the raw encoding), which the blind
linear features could not — but the MC-vs-sibling problem is not automatically
solved by either, so it is measured, not assumed.

## Stage 0 — GPU toolchain (DONE, PASS)

RTX 3090 (24 GB, driver 610.47) at `ssh micro@100.68.145.15`. Installed
`torch==2.6.0+cu124` with the real interpreter
(`C:\Users\micro\AppData\Local\Programs\Python\Python312\python.exe`, NOT the
Store stubs). Verified: `torch.cuda.is_available()` True, device
`NVIDIA GeForce RTX 3090`, a 4096² GPU matmul runs. The engine (401 base tests)
already passes on the desktop; the neural code was synced on top via the
tar/scp path in the desktop-compute-node memo.

## Stage 1 — encoder, value net, NeuralBot

### The state encoder (`engine/bots/neural_encode.py`, torch-free)

A flat `list[float]` of length **1897**, from one player's viewpoint. Kept
torch-free on purpose so the engine's stdlib tests and `tools/gate.sh` run it on
the Mac (torch-less); 7 unit tests in `tests/test_neural_encode.py` cover shape,
determinism, cross-player-count stability, and a **no-leak** check (shuffling
the hidden civil-deck order must not change the encoding).

Faithful to what a player can legally SEE (docs/INFORMATION_AUDIT.md):

* **Global** (30 dims): civil/military/current-event ages (one-hot), round,
  turn, an exact/estimated `rounds_left` and lateness L (reused from
  `weighted.py`), player-count one-hot, phase, last-round / scoring-event flags,
  row occupancy.
* **Card row** (13 × 51 = 663 dims): every slot's **card identity** via a
  49-dim `card_vec` (type one-hot + level + printed production + numeric effect
  keys + techCost/buildCost/stage-sum/strength) plus an occupied flag and the
  slot's civil-action cost. This is the information GAP 1/2 said the linear eval
  was blind to.
* **Four player blocks** (301 dims each, me + up to 3 rivals in seat order,
  zero-padded): resources/food/science/culture (stock and rate via
  `effects.compute`), civil/military actions (total and left), tech-curve (best
  farm/mine/lab/temple/theater/library/arena/unit), workers by category,
  government / leader / in-progress-wonder identities (`card_vec`), wonders,
  happiness, tactic, colonies, pacts, relative strength, and the **civil hand**
  (public — "open civil cards convention"). My own **military hand** contents
  are encoded; a rival's military hand is a **count only** (contents hidden).
  My own **seeded events** (`seeded_by == me`) are summarized; other players'
  seeds and the current-events order are not.

Deliberately NOT encoded (cheating or unknowable): civil/military deck ORDER
(only counts, via `rounds_left`); rival military-hand contents; other players'
event seeds. Because the encoder reads row card identity, the known `end_turn`
information leak (docs/BOT_ARCHITECTURE.md §2.3 — an `end_turn` trial refills the
row from the REAL civil deck) becomes *live* for a neural bot; NeuralBot
therefore **determinizes at the search root** (§below).

### The value net (`engine/bots/neural_net.py`, torch-guarded)

Residual MLP: `1897 → 256`, 3 residual blocks (LayerNorm + ReLU + dropout),
linear head → 1 scalar. ~1M params. Predicts the eventual **final-culture
margin** of the perspective player (my final culture − best rival's), scaled by
1/100. Margin (not win/loss) because it is a dense label present on every state
and preserves the sibling ordering the greedy policy needs; the MC-vs-sibling
caveat above is why strength is still measured head-to-head. `import torch` is
guarded so the module fails cleanly on the Mac and the engine stays stdlib.

### NeuralBot (`engine/bots/neural_bot.py`, torch-guarded)

The **exact 1-ply search shape of `WeightedBot`** with one thing swapped: the
linear `evaluate(trial, idx, w)` scalar is replaced by the net's margin
prediction. Two honesty measures:

* **Batched eval** — all ~11 candidate trial states of a decision are encoded
  and scored in ONE forward pass (policy unchanged; just affordable on the GPU).
* **Root determinization** — the two draw decks are re-shuffled once at the root
  (`plan.determinize`) before candidates are scored, so `end_turn` trials cannot
  peek at the real next row. `determinize=0` disables it, to measure the leak.

Wired into `experiments/arena.py` as a `neural:CKPT.pt,det=1,etb=0` spec (lazy
torch import, so `load_spec` and the gate stay torch-free on the Mac). For
multi-game duels use `experiments/neural_eval.py` (single process, one
GPU-resident model shared across games) rather than arena's process pool.

### Training data (self-play, `experiments/neural_selfplay.py`)

Engine self-play, PURE-PYTHON (no torch), labels = the engine's own final
scores — unlimited, free, and independent of the human-replay fidelity problem.
Every `--stride`-th ply, one row per active seat: `encode(state, seat)` and that
seat's eventual final-culture margin. `--epsilon` mixes in random legal moves so
the set covers off-policy siblings (which the 1-ply bot will be asked to rank).
Data comes from a MIX of policies (DEFAULT_WEIGHTS 1-ply, the frozen gen-209
champion, the strength-check champion) for state diversity. 28-core desktop
generates ~1800 games in ~35 s; float16 shards.

### Training (`experiments/neural_train.py`, 3090)

Shards split train/val **by shard** (never by row — rows from one game share an
outcome and would leak). AdamW + cosine LR, Huber loss, dropout. Reports val MSE,
val MAE in culture, and val pairwise ranking accuracy. **Early-stopping keeps the
best-val-ranking checkpoint**, because the net overfits: on the first 269k-row
run val ranking-acc peaked **0.768 at epoch 5** then decayed to 0.739 by epoch 30
while train loss kept falling. (0.77 is right in the band where BOT_ARCH's ridge
fit still won 0/400 — hence the head-to-head gate below.)

Checkpoints live on the desktop at `~/tta-ai/checkpoints/` (git-ignored; not
committed per the brief).

## Stage 1 results — head-to-head play strength

Net trained on **1.07M** self-play rows (3 policies, best-val checkpoint = epoch
2; the net overfits within ~2-5 epochs regardless of data size, val ranking
accuracy plateaus at **0.771**). NeuralBot = the net inside the 1-ply search,
root-determinized. Duels are 2p, seat-rotated, **n=200 each** (1200 games total,
**0 engine errors**). ± is a 95% CI; null win rate 0.500.

| opponent | NeuralBot win rate | neural culture | opp culture | margin |
|---|---|---|---|---|
| **self (control)** | **0.500** ± 0.068 | 52.6 | 52.6 | +0.0 ± 5.7 |
| `default` (1-ply linear, DEFAULT_WEIGHTS) | 0.297 ± 0.063 | 56.0 | 79.2 | −23.3 ± 5.5 |
| **linear champion, gen-209 (1-ply)** | **0.070** ± 0.035 | 54.9 | 129.1 | **−74.1** ± 6.0 |
| quiescent champion (`levels=1`) | 0.060 ± 0.033 | 54.9 | 129.9 | −75.0 ± 6.1 |
| **BookBot** | 0.075 ± 0.037 | 50.4 | 136.2 | −85.8 ± 7.8 |
| `default`, **determinize OFF** (leak on) | 0.312 ± 0.064 | 51.6 | 75.4 | −23.8 ± 5.4 |

Reference points (2p, docs/SCORE_VALIDATION.md §5): human **159.5**, quiescent
champion **64.7**, 1-ply lineage vector **139.8**. NeuralBot's own culture is
**~50-56** whatever it plays — the weakest of every named bot, below even the
suppression champion's native 64.7.

### What this says, plainly

* **The pipeline works and is honest.** CUDA on the 3090 works; the encoder is
  faithful and tested; the self-play → train → NeuralBot → duel loop runs
  end-to-end with **zero engine errors over 1200 games**; the self-control lands
  at exactly **0.500** (unbiased harness, deterministic model). Determinize
  on/off is **identical within noise** (0.297 vs 0.312), so (a) the row
  information-leak is inert for this net just as it is for the linear one, and
  (b) my head-to-head numbers are neither inflated by peeking nor propped up by
  determinization.
* **But the value net is a WEAK 1-ply policy.** It does **not** beat the linear
  champion (0.070, −74 margin) or BookBot (0.075, −86); it only approaches
  `default` (0.297, −23) and still loses. This is an **honest null** on "beat
  the champion."
* **It is, however, NOT the ridge-fit collapse.** BOT_ARCHITECTURE §3b's
  Monte-Carlo linear fit won **0/400** and scored **21** culture; NeuralBot wins
  0.06-0.31 and scores ~54. The nonlinear, card-aware net is meaningfully
  non-degenerate — it plays coherent full-length games (181-198 moves) — it is
  just not strong.
* **The diagnosis is the one the repo already wrote down.** BOT_ARCHITECTURE
  §2.3b/§3b: **Monte-Carlo value regression is the wrong objective for a greedy
  policy** — it optimises "am I ahead?" and spends ~none of its capacity on the
  sibling-state *differences* the 1-ply argmax actually consumes. **That thesis
  survives the jump to a nonlinear net that sees card identity.** Rich features
  and nonlinearity were necessary but not sufficient; the objective is the
  binding constraint. The plateau of val ranking-accuracy at 0.771 (right where
  the ridge fit sat) is the same fact from the prediction side.

### Verdict on Stage 1

**Qualified pass on infrastructure, honest null on strength.** Delivered: CUDA
toolchain, a faithful tested encoder, a value net, a `NeuralBot` in the existing
search, and a measured head-to-head battery with error bars against the linear
champion, the quiescent champion and BookBot, plus the reference points. The net
plays competently *enough* to be non-degenerate but does not reach the linear
champion.

### Do NOT do this next (and what to do instead)

Do **not** start the Stage-2 self-play improvement loop on top of this MC value
net — it would iterate on a foundation the evidence says is mis-objected. The
right Stage-2 move, exactly as docs/BOT_ARCHITECTURE.md §3b prescribes and calls
"arguably the single most actionable finding", is to **change the training
objective before adding self-play**:

1. **A pairwise / learning-to-rank objective** over the sibling states the data
   policy actually chose between (`(state, chosen, alternatives)`), which is the
   objective the 1-ply policy is graded on. The self-play generator already has
   these siblings in hand at pick time; it currently throws them away. This is
   the cheapest high-value change and it reuses this whole pipeline.
2. **TD(0)/TD(λ)** along trajectories (`V(s) ≈ V(s')`), which buys the *local*
   consistency needed to rank near-identical siblings — the Samuel/TD-Gammon
   recipe, and the same one docs/EXTERNAL_AIS.md §4d singled out.
3. Only once the net-in-1-ply beats the linear champion should the AlphaZero
   self-play loop (Stage 2) and a policy head / search (Stage 3) go on top.

Everything needed for those is in place: encoder, net, batched GPU inference,
data generator, trainer, and the duel harness. The one thing to add is the
objective.

## Reproducing

```
# desktop (GPU + engine co-located)
bash ~/gen_data2.sh                       # self-play shards -> data2/*.npz
python experiments/neural_train.py --data data2/*.npz --epochs 30 \
    --out checkpoints/value2p.pt --device cuda
bash ~/eval_all.sh checkpoints/value2p.pt 200   # the duel battery
```
