# The neural stack: distilled

Source docs for this file (`NEURAL_SEARCH_LOOP.md`, `NEURAL_EVAL.md`,
`NEURAL_LOOP_NULL.md`, `PROXY_GUARDRAIL.md`, `DESKTOP_QUIET.md`) were deleted
2026-08-06 after this distillation; recover them from git history if you need
the full narrative, run logs, or error bars. The Python engine (`engine/`,
`experiments/*.py`) is gone. Everything below is current against the Rust
tree unless marked historical.

## What the value net is

A residual MLP, state → scalar, predicting the eventual final-culture margin
(mover's final culture − best rival's) of the position. Implementation:
`rust/src/bots/neural/net.rs` (`ValueNet`, forward pass, checkpoint
parse/serialize — this repo's own binary format, not torch). Encoder:
`rust/src/bots/neural/encode.rs`, `ENCODING_DIM = 1906`
(`GLOBAL_DIM + ROW_DIM + MAX_PLAYERS * PLAYER_BLOCK_DIM`). Faithful to what a
player can legally see: card identity in the row, public tableaus, own
hidden hands; NOT civil/military deck order, rival military hand contents,
other players' event seeds.

**Checkpoint versioning.** The encoder width changed 1907 → 1906
(`state.scoring_events` was a permanently-0 column in both engines; Rust
dropped it, Python never got the fix before deletion). `net.rs::parse_checkpoint`
hard-errors on a width mismatch instead of silently loading a stale
checkpoint against a shifted encoding — see the comment at `net.rs:53` and
the test `encoding_dim_matches_python` (`encode.rs:832`, now purely a
historical name). **Any checkpoint from before this change is unusable and
the guard is why loading one fails loudly instead of scoring garbage.**

Bots that wrap the net: `rust/src/bots/neural/plan.rs` (`NeuralPlanBot`,
formerly `neural_plan.py`) — PlanBot's whole-turn beam with the net as the
leaf evaluator, root-determinized, war-lookahead substitution applied.

## How training data is generated, and the leak that invalidates pre-08-06 results

**The defining trap.** Before 2026-08-06, the Python generator
(`neural_rankdata.py` / `neural_gen_iter.py`) applied each candidate move to
the **real** (non-determinized) game state to build its encoding. For an
`end_turn` candidate this refills the row from the true civil deck, so the
encoding — and the label built from it — carries the actual next card.
Measured on `WeightedBot` at 2p: **94.9% of `end_turn` candidates draw a
card**, i.e. leak the real next card in ~95% of the single most-evaluated
move class in the game. Any neural checkpoint or result trained/measured
before 2026-08-06 is downstream of this and should not be trusted.

**The Rust fix**, `rust/src/bots/neural/rankdata.rs`: determinize **once per
decision** (`bots::plan::determinize`, reshuffling exactly the hidden piles),
then apply every candidate move to that one shuffled copy. All siblings of a
decision see the same world; none sees the truth. `rankdata` reports which
value-row distribution a shard used (`values=search-leaves` or
`values=pre-move-state`) as a recorded fact on its own output line, not
something a reader has to infer.

**Value rows must match what the leaf evaluator is actually served.** A beam
leaf is asked about quiet, end-of-turn, determinized, war-substituted
positions — nothing else. Value rows are collected from exactly those
states via an explicit collection hook (`bots::plan::Bank`, a `push`
closure passed into the beam) rather than a separate generator subclassing
the search, so an ordinary `pick` pays nothing for it and the distribution
can't drift from what the beam prices.

## The self-play loop: two failure modes already found

### v1 — the identity operator (41-hour null, historical/Python)

`neural_loop.sh` v1 ran 74 iterations / 20,700 gate games and promoted
**zero** candidates; the pooled candidate win rate was 0.4413, ~17 SE below
the 0.5 null — not a failed search, a *reliably worse* one. Root cause: the
"chosen" side of every training pair was the net's own 1-ply argmax, so the
untrained incumbent already satisfied 97.6% of its own training target
(`pair_acc = 0.9764` measured with **zero training applied**). With no
search, nothing in the loop knew anything the net didn't already know —
generalized policy iteration with the improvement operator equal to the
identity. Compounding bug: validation was the alphabetically-first 15% of
shard files, which silently made "held-out accuracy" a measure of agreement
with the incumbent, not of generalization; `--select pair` therefore always
kept epoch 1.

**Reusable lesson: any self-play loop must state its improvement operator,
and it must be measurable.** Before trusting a loop, score the untrained
incumbent on its own training pairs — if pair accuracy is already ~1.0,
stop. A validation metric computed on labels the model itself produced is a
conservatism meter, not a validation metric.

### v2 — the search-backed loop (current design)

Fix: the improvement operator is a **beam search** (`NeuralPlanBot`), not
the net's own argmax — on identical linear weights the beam beats 1-ply
88.6% ± 3.1, so its labels are genuinely stronger than the model being
trained. `DISAGREE` (fraction of decisions where the beam overrules the
net's 1-ply argmax) is the loop's health meter; a run whose `DISAGREE`
decays toward 0 has gone vacuous again (pre-registered kill condition,
`DISAGREE < 0.02` for two consecutive iterations).

**Promotion needs two arms**, both head-to-head under the search that will
actually ship — no offline metric gates anything:
- **Arm A (self-play):** candidate vs. incumbent beam-vs-beam, 95% CI lower
  bound over 0.5.
- **Arm B (anchor):** candidate must not regress vs. a frozen external
  reference (`plan:champion_2p`) by more than one SE of the difference from
  the incumbent's own score against that reference. This exists because a
  self-play-only gate is closed-loop and can't tell learning from drift —
  a prior run's self-play culture climbed 116→143 over 7 iterations with 4
  promotions while its score against the frozen anchor never moved
  (0.40→0.35→0.37→0.40, all within each other's ±0.11 CI). Arm B is
  deliberately *not* "beat the champion" (the net trails by ~14pp; that gate
  would freeze the run) — it's "don't get worse than what you're replacing
  on the one opponent that can't move."

Other pre-registered kill conditions: no promotion in 15 iterations with the
pooled win-rate CI excluding 0.5; the anchor score flat across 10
consecutive readings.

Rust-port notes worth keeping: the interval clusters on the **deal**, not a
shard (no more multi-process fan-out — one Rust process does the whole
match with `--threads`), reported as `se=` in `rust/src/stats.rs`, published
separately from `ci=` (which carries a `t` critical value baked in — don't
divide it to reconstruct an SE). `DISAGREE=NA` (not `0.0000`) when the
teacher has no net to compare against, so "never overruled" and "nothing to
overrule" can't be confused.

## A related trap in the (now-gone) linear-weight league

`PROXY_GUARDRAIL.md`'s machinery (`experiments/proxy_check.py`, the
Python `blend`/`own_share` hillclimb league) was Python and is deleted with
no Rust successor monitor. It doesn't concern the value net, but the shape
of its finding is the same class of trap as the arm-B design above and is
worth carrying forward: the 3p weight-vector arm's ship-policy strength
(own culture vs. a fixed opponent, under the actual deployed search) **fell
−76.6 ± 17.8 culture over 918 generations** of accepts that were all
positive under the *training-time* proxy metric — the arm's best vector
under the policy it would ship was the untrained one it started from,
because the training proxy was reward-hackable in a way the deployed search
punished. Nothing today watches for the neural loop's equivalent; arm B
above is the current defense, and if it's ever removed this is why.

## Training now runs on CPU in Rust

The GPU was never the bottleneck — game simulation dominated, and 8
unthrottled probe processes measured 0.25 core each competing for CPU while
the GPU sat idle. Training moved to hand-rolled CPU backprop,
`rust/trainer/` (candle, CPU-only), an explicitly authorised exception to
the repo's empty-dependencies rule, by owner decision 2026-08-06. Do not
fold it into the core crate. Binaries: `rankdata` (data gen), `neuraltrain`
(training), `neuraleval` (gate/eval) — see `rust/src/bin/`.

## Operating the shared desktop box

The desktop is a gaming PC first. The GPU guard (`gpu_guard.py`, killed
torch on a foreign GPU process) is retired — no GPU/torch left to guard.
What's still load-bearing for anything launched there: **always launch
through `tools/hidden_launch.vbs`** (`wscript.exe` has no console of its
own, so children inherit a hidden one instead of flashing a window — hiding
via `-WindowStyle Hidden` alone does not work, git's `bash.exe` allocates
its own console regardless); **every Scheduled Task trigger needs an
explicit `<Duration>`** on its `<Repetition>` or Task Scheduler silently
drops the repeat and the task fires once, ever; **reap old workers by
stored PID before relaunching**, not by log-mtime liveness (a survivor
process plus a mtime-based liveness check duplicates workers forever).
`PAUSE` (read by `experiments/neural_search_loop.sh` before every worker
launch) is now an operator-only control with no automatic writer: `touch
PAUSE` parks training, delete it to resume.

## The policy head (2026-08-12)

A prior over LEGAL MOVES for move ordering — deliberately not another value
net. Value-only nets have been trained and gated on this project twice and
lost both times; this head never emits a value, and nothing downstream may
ask it for one.

Pipeline: `bots/neural/action.rs` encodes a `Move` (37 kinds, enumerated by
an exhaustive match off `moves::Move` with no wildcard arm, so a new variant
is a compile error rather than a silently-zeroed vector; `ACTION_DIM = 160`,
reusing `encode::card_vec_array` so there is one per-card encoding, not two).
`bots/neural/dump.rs` writes self-play decisions; `bin/dump_selfplay`
generates them; `bots/neural/policy_train.rs` + `bin/policytrain` fit a
softmax over the legal set only, cross-entropy against the champion's chosen
move.

**Legality:** the action encoder takes no `GameState` — every field it reads
belongs to the deciding player's own `Move`. There is no path from it to a
rival's hand.

**Dump v2 is 4x smaller than v1** (~30 KB/decision -> ~7.7 KB): `f32` on
disk, and the compact `Move` stored rather than its dense 160-float
expansion, which is rebuilt at training time through the same
`encode_action` so the two cannot drift. v1 would have made a real corpus
tens of GB and it would not have fit on the mini.

**First result, 97,994 self-play decisions (2p/3p/4p), split BY GAME —
never by decision, which leaks badly because decisions inside one game are
heavily correlated.** 87,716 train / 10,278 held-out, 3 epochs, loss still
falling (1.276 -> 1.205 -> 1.168): the net is undertrained, not converged.

| held-out | top-1 | random | top-3 | random |
|---|---|---|---|---|
| all (mean legal set 11.57) | 0.5850 | 0.2079 | 0.8328 | 0.4725 |
| legal_count >= 4 | 0.5236 | 0.1005 | 0.7787 | 0.3016 |

The restricted row is the one that matters — forced and near-forced
decisions inflate the unrestricted number. 5.2x the base rate on genuinely
open decisions, and top-3 covers 78% of them, which is the shape move
ordering actually wants.

By phase, top-1 falls early -> mid -> late (0.658 / 0.585 / 0.513). Late
positions are where the champion's own search is doing the most work and
where a cheap prior helps least; expect the search-integration win to come
from early and mid ordering.

## The disagreement teacher is a negative result (2026-08-12)

The 2026-08-05 work order was: the old value-net loop's improvement
operator was the identity (the untrained net already agreed with 0.9764 of
its own labels), so train instead on the positions where the deeper search
OVERRULES the net. `bin/policy_disagreement_experiment.rs` ran that against
a control, both arms branched from one shared training prefix via
`PolicyTrainer::snapshot`/`from_state` (net plus warm Adam moments), so
they differ only in the final epoch.

Epoch budget N=7 was chosen by early stopping on held-out top-1 — not on
train loss, which falls forever. CONTROL is a plain 7th epoch; TEACHER is a
7th epoch on only the 32,578/87,716 train decisions (37.1%) where the
pre-final-epoch net's own pick disagreed with the champion's search.

| held-out top-1 | control | teacher | delta |
|---|---|---|---|
| overall | 0.6003 | 0.5144 | -0.0859 |
| legal_count >= 4 | 0.5378 | 0.4411 | -0.0967 |
| early / mid / late | .668/.595/.538 | .575/.508/.460 | worse in all three |

top-3 overall 0.8507 -> 0.8096 (-0.0411).

**Noise-validated.** Re-branching both arms from the same snapshot under a
different seed moved control by 0.0006 and teacher by 0.0063; the -0.0859
delta is 13-140x run-to-run noise. This is not "helped on hard cases, hurt
overall" — the teacher lost on the restricted slice too, by more than it
lost overall.

**Do not re-try disagreement emphasis on the policy head.** The mechanism
that fixed nothing here is the same one proposed in `NEURAL_LOOP_NULL.md`;
that proposal is now closed. Fine-tuning on a hard subset discards the
distribution the head is meant to reproduce — a move-ordering prior is
judged on the whole legal set it will actually rank, not on the tail.

**What DID work: more epochs.** Held-out top-1 rose every epoch, 0.5698 ->
0.6003 across 7, and early stopping never fired — N=7 is a compute cap, not
a plateau. The cheapest available gain on the policy head today is simply
training the plain objective longer.
