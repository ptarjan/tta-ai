# Neural value evaluator — build log and findings

Status: **Stage 0 PASS (CUDA on the 3090); Stage 1 complete — infrastructure
pass, strength a genuine step but not yet champion-level.** Branch `neural-eval`,
worktree `/Users/pt/tta-ai-neural`. Base game only (2015 "A New Story of
Civilization"), 2 players unless stated. This document is for the next engineer:
negatives and nulls included, not a glossy report.

TL;DR: MC value regression → weak (0.07 vs the linear champion, ~54 culture),
exactly as BOT_ARCHITECTURE §3b predicted. Switching to a pairwise-ranking
objective (the prescribed fix), same pipeline, roughly DOUBLED the net's culture
(~85-94) and took it to ~even with the `default` linear bot (0.427) — but it
still loses to the trained champion (0.095) and BookBot (0.15). The objective was
the binding constraint; the remaining gap is strength, so Stage 2 self-play is
unblocked. See §Stage 1b.

The bet (owner's instruction): replace the hand-crafted linear evaluator with a
trained neural value net — the AlphaZero-shaped path, justified by our having a
FAST, VALIDATED simulator ([`docs/SCORE_AUDIT.md`](SCORE_AUDIT.md): scoring exact vs 1011 human
games) which is exactly what self-play needs. The linear evaluator has a proven
ceiling — it is a linear function of ~89 features and cannot express nonlinear
value, and it is BLIND to card identity in the row and in hands
([`docs/INFORMATION_AUDIT.md`](INFORMATION_AUDIT.md)), which is most of the real skill of the game.

## The load-bearing prior warning (read before trusting any number here)

[`docs/BOT_ARCHITECTURE.md`](BOT_ARCHITECTURE.md#23b-the-evaluation-does-not-predict-the-outcome--measured) §2.3b/§3b measured that a value function fit by
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

> **Width note, added 2026-08-06 (not a rewrite of the section below, which
> predates the Rust port and is otherwise left as history).** The dimension
> grew from the 1897 this section quotes to 1907 at some point before the
> Rust port. The Rust encoder (`rust/src/bots/neural/encode.rs`) then
> deliberately dropped it to **1906**: `state.scoring_events` was a
> write-never field in both engines (a permanently-0 feature), and removing
> it from the Rust encoder's `GLOBAL_DIM` dropped the total by one
> (`encode.rs`'s `encoding_dim_matches_python` test, and its own comment,
> record this). Python's `neural_encode.py` was not touched by that fix and
> — as of `engine/`'s 2026-08-06 deletion — is gone, so there is no live
> Python encoder left to disagree with the Rust one; the 1906/1907 split is
> now purely historical. This paragraph is the authority `encode.rs` cites
> for that number; it previously (and incorrectly) cited `docs/OPEN_ITEMS.md`,
> which has never recorded an encoder width.

A flat `list[float]` of length **1897**, from one player's viewpoint. Kept
torch-free on purpose so the engine's stdlib tests and `tools/gate.sh` run it on
the Mac (torch-less); 7 unit tests in `tests/test_neural_encode.py` cover shape,
determinism, cross-player-count stability, and a **no-leak** check (shuffling
the hidden civil-deck order must not change the encoding).

Faithful to what a player can legally SEE ([`docs/INFORMATION_AUDIT.md`](INFORMATION_AUDIT.md)):

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
information leak ([`docs/BOT_ARCHITECTURE.md`](BOT_ARCHITECTURE.md#23-a-new-defect-the-search-reads-cards-the-player-cannot-know--measured) §2.3 — an `end_turn` trial refills the
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

Reference points (2p, [`docs/SCORE_AUDIT.md`](SCORE_AUDIT.md#105-the-score-gap-is-a-property-of-the-vector-not-of-the-engine) §10.5): human **159.5**, quiescent
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

## Stage 1b — the pairwise-ranking objective (the prescribed fix), MEASURED

The MC null above pointed at the objective, so I built the fix BOT_ARCHITECTURE
§3b prescribes and ran it, same pipeline. `experiments/neural_rankdata.py`
generates **sibling-preference** data from a strong teacher (BookBot, which
beats the linear champion 62.9%): at each sampled decision it records the
encoding of the child state BookBot CHOSE and up to 6 REJECTED sibling children,
plus a value anchor row (pre-move state + mover's eventual margin). 613k pairs +
126k value rows. `experiments/neural_train_rank.py` trains the SAME net with a
**combined loss = value Huber + λ·Bradley-Terry ranking** (softplus(−(v(chosen)−
v(rejected)))) — the ranking term is exactly the sibling discrimination the
1-ply argmax consumes. Best held-out **pair accuracy 0.821** (net orders
BookBot's chosen sibling above a rejected one 82% of the time).

Same n=200 duel battery, ranking net vs the MC net:

| opponent | MC-net win | **rank-net win** | MC cult | **rank cult** | rank margin |
|---|---|---|---|---|---|
| self (control) | 0.500 | **0.500** ± 0.069 | 52.6 | 84.8 | +0.0 |
| `default` (1-ply linear) | 0.297 | **0.427** ± 0.068 | 56.0 | **94.4** | −7.2 ± 6.8 |
| linear champion (gen-209) | 0.070 | **0.095** ± 0.041 | 54.9 | 89.8 | −62.5 ± 6.6 |
| quiescent champion | 0.060 | 0.070 ± 0.035 | 54.9 | 84.0 | −69.8 ± 6.5 |
| BookBot | 0.075 | **0.150** ± 0.050 | 50.4 | 71.6 | −75.6 ± 9.9 |
| `default`, determinize OFF | 0.312 | 0.410 ± 0.068 | 51.6 | 94.5 | −7.6 ± 6.3 |

**The objective was the binding constraint, confirmed.** Switching MC → ranking,
with everything else identical, lifts NeuralBot's own culture by **~+40** (54 →
~85-94), takes it from losing to `default` (0.297, −23) to **within one CI of
even** (0.427, −7.2 ± 6.8), and **doubles** the win rate vs BookBot (0.075 →
0.150). Determinize on/off is still identical (0.427 vs 0.410) — no
information-leak dependence. This is a real, measured step and it is exactly the
direction [`docs/BOT_ARCHITECTURE.md`](BOT_ARCHITECTURE.md#3b-prototype-the-fitted-value-vector-and-its-first-duel--a-hard-null) §3b predicted.

**But it still does NOT beat the trained champion** (0.095, −62.5). The champion
is a suppression engine: against a book-style production bot it runs its own
score to ~152. The ranking net cloned BookBot's move *ordering* but at 82% pair
accuracy the 1-ply argmax compounds ~18% wrong sibling orderings over ~180
moves, and the value head's calibration degraded (val MAE 84 culture at the
best-ranking epoch) — so it plays a *weaker* BookBot, ~even with the default
linear bot, not yet at champion strength.

### Verdict on Stage 1

**Infrastructure: pass. Strength: a genuine step, not yet champion-level.**
Delivered end-to-end and measured with error bars: the CUDA toolchain on the
3090; a faithful, tested, torch-free encoder; a value net; a `NeuralBot` in the
existing 1-ply search; two training objectives (MC and pairwise-ranking); and a
full head-to-head battery against the linear champion, the quiescent champion and
BookBot with the human/champion/lineage reference points. The best NeuralBot
(ranking) reaches ~even with the `default` linear bot and ~85-94 culture (vs the
suppression champion's native 64.7), but loses to the trained champion (0.095)
and BookBot (0.15). Zero engine errors across ~2400 duel games; self-control
exactly 0.500 both nets.

### What to do next (Stage 2/3)

The remaining gap to the champion is now a *strength* gap, not an *objective*
gap, so the AlphaZero-shaped path is unblocked and worth it:

1. **Better value calibration in the combined loss** (cheap): the val MAE
   ballooned to 84 culture as the ranking term dominated. Sweep λ and use a
   separate value head / a value-only warm start so the net both ranks siblings
   AND is calibrated — calibration is what the champion is exploiting.
2. **Self-play iteration (Stage 2)** now stands on a sound objective: play
   ranking-net-in-1-ply games, regenerate sibling data from the NET's own
   improving choices (not just BookBot's), retrain, gate each net against the
   previous best by head-to-head win rate. This is where the net can pass its
   BookBot teacher instead of imitating it.
3. **Policy head + shallow search (Stage 3)** — PlanBot already shows whole-turn
   beam search is worth ~+35 pts over 1-ply ([`docs/BOT_ARCHITECTURE.md`](BOT_ARCHITECTURE.md#3-prototype-planbot) §3);
   NeuralBot's evaluator dropped into that beam is the natural combination.

Everything for these is in place: encoder, net, batched GPU inference, two data
generators (`neural_selfplay.py` MC, `neural_rankdata.py` sibling), two trainers,
and the duel harness. Checkpoints on the desktop: `~/tta-ai/checkpoints/`
(`value2p.pt` = MC, `value2p_rank.pt` = ranking; both git-ignored).

## Reproducing (updated)

```
# desktop (GPU + engine co-located)
bash ~/gen_data2.sh            # MC self-play shards       -> data2/*.npz
bash ~/gen_rank.sh             # BookBot sibling pairs      -> rankdata/*.npz
python experiments/neural_train.py      --data data2/*.npz      --out checkpoints/value2p.pt
python experiments/neural_train_rank.py --data rankdata/*.npz --lam 1.0 --out checkpoints/value2p_rank.pt
bash ~/eval_all.sh checkpoints/value2p_rank.pt 200      # the duel battery
```

## Reproducing

```
# desktop (GPU + engine co-located)
bash ~/gen_data2.sh                       # self-play shards -> data2/*.npz
python experiments/neural_train.py --data data2/*.npz --epochs 30 \
    --out checkpoints/value2p.pt --device cuda
bash ~/eval_all.sh checkpoints/value2p.pt 200   # the duel battery
```

## Stage 2 — overnight self-play loop + the box-wide gaming guard

Authorised by the owner as the best-in-world path, to run autonomously overnight
on the 3090. Two pieces: a gaming guard (so training yields the whole box the
instant the owner games) and the self-play improvement loop.

### The gaming guard (`experiments/gpu_guard.py`, + `experiments/deploy/`)

A standalone forever-loop (15s poll) that is the single arbiter for the whole
box: it writes/removes the `C:\Users\micro\tta-ai\PAUSE` flag that every training
loop reads, and hard-kills our training python to free VRAM cleanly when a game
appears.

**Detection — and a real constraint.** This is a consumer **WDDM** box, so
`nvidia-smi` reports **per-process VRAM as N/A** (the coordinator's ">500 MB
VRAM" signal is a TCC/datacenter feature, unavailable here — verified on the
box). What IS available: `nvidia-smi --query-compute-apps=pid,process_name` lists
every GPU process **with its full path**, including graphics (C+G) apps. So the
guard detects games by **path**: a GPU process is FOREIGN if its path is neither
our python nor one of many benign desktop/launcher apps (dwm, chrome, discord,
steam helper, powertoys, nvidia, razer, battle.net launcher, …). Games live in
their own dirs (`World of Warcraft\Wow.exe`, `steamapps\common\<game>\…`) and
never match the benign set. The cost asymmetry is deliberate — a false pause just
stops training briefly (it relaunches); a missed game causes the stutter the
owner forbids — so anything unrecognized on the GPU is treated as a game. A
2-poll (~30 s) debounce prevents thrashing. The guard runs **elevated**
(scheduled-task RunLevel HighestAvailable, since `micro` is an admin) so
`nvidia-smi` can read every process path; unreadable paths (`[Insufficient
Permissions]`) are treated as benign so a lower-integrity fallback can't
false-trigger.

**Verified end to end (2p, on the box):** at idle it correctly finds no foreign
process and never pauses; on a simulated game (`GUARD_TEST_FOREIGN` hook) it
detected within ~2 polls, wrote PAUSE, **killed all 16 training pythons and
survived itself**, then on clear removed PAUSE after the 30 s debounce and
training resumed — a full `PAUSE ON → PAUSE OFF` cycle in the guard log.

### Durability without a human (Windows Scheduled Tasks)

Both the guard and the neural loop are registered as scheduled tasks
(`schtasks`, XML in `experiments/deploy/`): trigger ONLOGON **with an hourly
repetition** and `RestartOnFailure`, `MultipleInstancesPolicy=IgnoreNew`
(a per-process lockfile also enforces single-instance), no execution time limit.
So they survive a reboot (auto-login → ONLOGON), a crash (restart / hourly
re-fire), and this SSH session / the driving agent ending. The loop runs at
task **Priority 7 (below-normal)** so its python children yield CPU to a game
even if the guard's kill lags; the guard runs at Priority 5. The CPU league-arm
worker reads the SAME PAUSE flag and runs LOW — the guard is the only writer.

Confirm it is working:
```
schtasks /Query /TN tta_gpu_guard      # Status: Running
schtasks /Query /TN tta_neural_loop
type C:\Users\micro\tta-ai\experiments\logs\gpu_guard.log   # START / PAUSE transitions
dir C:\Users\micro\tta-ai\PAUSE         # present only while a game is up
```

### The self-play loop (`experiments/neural_loop.sh`)

AlphaZero-style generalized policy iteration on the 1-ply value-net policy.
Per iteration: (1) parallel CPU self-play (12 workers, ε-greedy) with the current
BEST net → value rows (state→eventual margin, the on-policy GPI signal) + ranking
pairs (the net's greedy child vs rejected, keeping siblings sharp)
(`neural_gen_iter.py`); (2) train a candidate warm-started from BEST on a 3-iter
replay window (`neural_train_rank.py`); (3) **gate** candidate-vs-best
head-to-head n=300 (`neural_eval.py --opponent neural:BEST`); **promote only if
the 95% CI lower bound clears 0.5**; (4) every 3 iters, a reference curve vs the
linear champion / BookBot / default → `loop/curve.tsv`. It honors the PAUSE flag
before every python launch and is resume-safe (keeps the promoted champion
across restarts).

**Calibration finding (measured, before iterating).** The coordinator asked to
fix the value-head calibration first (Stage-1b MAE was ~84). A sweep of the
value-loss weight did fix it (MAE 84 → ~41 at vweight=3) **but it HURT 1-ply play
strength** (0.427 → 0.258 vs default): a 1-ply value-argmax policy is driven by
the sibling **ordering** (pair-accuracy), not value MAE, and this loop regresses
value to real game outcomes each iteration (no bootstrapping), so a miscalibrated
head is **not** amplified. So the loop starts from the strongest net (vw=1, pair
0.821) with `select=pair`, and explores with scale-independent ε-greedy. This is
an honest deviation from the letter of the instruction, justified by the
measurement; documented here for the next engineer.

_(learning curve to follow in loop/curve.tsv; the headline question is whether
self-play takes the 1-ply net past the linear champion, currently 0.095 vs it.)_
