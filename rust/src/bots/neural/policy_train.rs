//! Training the policy head (slice 2 of `docs/NEURAL.md`'s planned third
//! net): a PRIOR OVER LEGAL ACTIONS, not a value net. Read `action.rs`'s top
//! doc comment first for why the network's output is one logit per
//! `(state, action)` pair, soft-maxed over WHICHEVER actions are legal at
//! one decision point, rather than a fixed-vocabulary classifier.
//!
//! ## Architecture: [`ValueNet`] with zero residual blocks, reused rather
//! than reforked
//!
//! A "linear stem -> LayerNorm -> ReLU -> linear head producing one scalar"
//! is exactly [`ValueNet`] with `blocks: vec![]` -- [`net::ValueNet::forward`]
//! and `train.rs`'s `forward_train`/`backward_train` already handle the
//! empty-`blocks` case (`ForwardCache::last_hidden` falls back to
//! `stem_out`). This module therefore reuses [`ValueNet`] as the per-action
//! SCORER, and `train.rs`'s hand-rolled backprop (`forward_train`/
//! `backward_train`/[`ValueNetGrad`]/[`AdamW`]) as the optimizer, rather
//! than hand-rolling a second copy of the same arithmetic under a new name
//! -- this crate's `[dependencies]` stays empty either way (no autodiff, no
//! GPU crate), and `train.rs`'s own gradients are already pinned against
//! finite differences.
//!
//! The scalar this net produces is a LOGIT, never treated as a value on its
//! own: [`PolicyTrainer::train_decision`] always consumes a whole decision's
//! worth of logits together, through [`softmax_cross_entropy`], and nothing
//! in this module reads a lone logit as a win-probability or margin. Value
//! heads have been tried twice on this project and lost both times
//! (calling task); reusing [`ValueNet`]'s ARITHMETIC is not reusing its
//! OBJECTIVE -- the loss here is entirely different (see below).
//!
//! ## The row a candidate action becomes: `state ++ action`
//!
//! [`expand_row`] concatenates a decision's state encoding
//! ([`encode::ENCODING_DIM`] floats) with ONE legal move's
//! [`action::encode_action`] expansion ([`action::ACTION_DIM`] floats),
//! producing the [`POLICY_IN_DIM`]-wide row [`ValueNet::forward`] scores.
//! `encode_action` is called HERE, at train time, off whatever `Move` a
//! [`super::dump::DecisionRecord`] stored -- never at dump time -- so there
//! is exactly one place in the crate that expands a compact `Move` into its
//! dense features (`dump.rs`'s own top doc comment on the storage-cost fix
//! makes the same argument for the on-disk format).
//!
//! ## The loss: cross-entropy over the LEGAL SET only
//!
//! [`softmax_cross_entropy`] never sees any logit outside the one decision
//! it is called for -- there is no fixed-width output layer to soft-max
//! over instead, unlike an Atari DQN's action head. A decision with two
//! legal moves and a decision with thirty never share a normalising
//! constant. [`PolicyTrainer::train_decision`] is the only call site: it
//! forwards every legal action's row through the SAME net (one
//! [`ForwardCache`] per candidate, since softmax needs every candidate's
//! logit before ANY of them can be backpropagated), softmaxes, then
//! backpropagates `probs[i] - 1{i==chosen}` through each candidate in turn.

use std::time::Instant;

use crate::moves::Move;
use crate::rng::PyRandom;

use super::action::{self, encode_action};
use super::dump::DecisionRecord;
use super::encode::ENCODING_DIM;
use super::net::{push_f64_slice, push_u32, Reader, ValueNet};
use super::train::{backward_train, forward_train, random_init, AdamW, BackScratch, ForwardCache, ValueNetGrad};

/// Width of one `state ++ action` row: the state encoding plus one legal
/// move's dense expansion. See this module's top doc comment.
pub const POLICY_IN_DIM: usize = ENCODING_DIM + action::ACTION_DIM;

/// Build the `state ++ action` row for one legal move at one decision --
/// the ONE place a stored [`Move`] is expanded back to its dense features
/// (via [`encode_action`], the same function the training-data DUMPER would
/// have called if version 1's dense-on-disk format still existed -- see
/// `dump.rs`'s top doc comment). `state` is already [`ENCODING_DIM`] `f32`s
/// (a [`super::dump::DecisionRecord`]'s own width); `actor`/`mv` come from
/// that same record.
pub fn expand_row(state: &[f32], actor: u8, mv: Move) -> Vec<f32> {
    debug_assert_eq!(state.len(), ENCODING_DIM, "expand_row: state width");
    let action_vec = encode_action(actor, mv);
    let mut row = Vec::with_capacity(POLICY_IN_DIM);
    row.extend_from_slice(state);
    row.extend(action_vec.iter().map(|&x| x as f32));
    row
}

/// Build a freshly-initialised [`ValueNet`] shaped for this module: `blocks:
/// vec![]` (see this module's top doc comment), input width [`POLICY_IN_DIM`].
pub fn random_policy_net(hidden: usize, seed: i64) -> ValueNet {
    random_init(POLICY_IN_DIM, hidden, 0, seed)
}

/// `f32 -> f64` in place into a caller-owned buffer -- the one place a
/// dataset row's storage width meets this module's `f64` arithmetic (the
/// same role `train.rs`'s own private `widen_into` plays for shard rows,
/// duplicated here rather than exposed across the module boundary for one
/// three-line loop).
fn widen_into(src: &[f32], dst: &mut [f64]) {
    debug_assert_eq!(src.len(), dst.len());
    for i in 0..src.len() {
        dst[i] = src[i] as f64;
    }
}

/// Numerically-stable softmax cross-entropy over EXACTLY `logits.len()`
/// candidates (no fixed-width padding, no candidates from any other
/// decision) against `chosen`. Returns `(loss, d(loss)/d(logits))`; the
/// gradient is the textbook `softmax(logits) - one_hot(chosen)`.
///
/// # Panics
/// If `logits` is empty (`chosen` could not have indexed into it) or
/// `chosen >= logits.len()`.
pub fn softmax_cross_entropy(logits: &[f64], chosen: usize) -> (f64, Vec<f64>) {
    assert!(!logits.is_empty(), "softmax_cross_entropy: no candidates");
    assert!(chosen < logits.len(), "softmax_cross_entropy: chosen {chosen} out of range for {} logits", logits.len());
    let max = logits.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let exps: Vec<f64> = logits.iter().map(|&l| (l - max).exp()).collect();
    let sum: f64 = exps.iter().sum();
    let mut probs: Vec<f64> = exps.iter().map(|&e| e / sum).collect();
    // Clamp before the log, not the probability itself, so the RETURNED
    // probabilities (what a caller measuring top-1/top-3 agreement reads)
    // are the true softmax values; only the loss's own log is guarded
    // against a vacuous log(0) from float underflow on a very confident,
    // wrong prediction.
    let loss = -(probs[chosen].max(1e-300)).ln();
    probs[chosen] -= 1.0;
    (loss, probs)
}

/// Trains a [`ValueNet`] (zero blocks, see this module's top doc comment)
/// against the policy softmax-cross-entropy objective, one decision at a
/// time. Single-threaded by design -- this project's training corpora are
/// modest (hundreds of thousands of decisions, not the value net's shard
/// sizes) and the calling task's own constraint is to leave every other
/// core free for concurrent hill-climb arms.
pub struct PolicyTrainer {
    pub net: ValueNet,
    grad: ValueNetGrad,
    adamw: AdamW,
    /// One [`ForwardCache`]/widen buffer PER CANDIDATE action in the
    /// largest decision seen so far, grown on demand and reused -- softmax
    /// needs every candidate's logit before any of them can be
    /// backpropagated, so (unlike `train.rs`'s value/ranking trainer, which
    /// only ever needs one or two live caches at a time) this trainer's hot
    /// loop genuinely needs `legal_count` caches alive simultaneously.
    caches: Vec<ForwardCache>,
    conv: Vec<Vec<f64>>,
    scratch: BackScratch,
}

impl PolicyTrainer {
    pub fn new(net: ValueNet, lr: f64, wd: f64) -> Self {
        let grad = ValueNetGrad::zeros_like(&net);
        let adamw = AdamW::new(&net, lr, wd);
        let scratch = BackScratch::zeros(net.hidden);
        PolicyTrainer { net, grad, adamw, caches: Vec::new(), conv: Vec::new(), scratch }
    }

    /// Resume a trainer from an exact prior state -- both the weights AND
    /// the optimiser's moments, produced by an earlier [`Self::snapshot`].
    /// Exists for the disagreement-teacher experiment (`docs/NEURAL.md`):
    /// two training runs that need to branch from the identical point (one
    /// continuing the plain recipe, one switching to disagreement-weighted
    /// decisions) must both start Adam warm from the SAME moments, not one
    /// warm and one cold -- a cold-started branch trains measurably
    /// differently for the first several steps for reasons that have
    /// nothing to do with which decisions it was trained on.
    pub fn from_state(net: ValueNet, adamw: AdamW) -> Self {
        let grad = ValueNetGrad::zeros_like(&net);
        let scratch = BackScratch::zeros(net.hidden);
        PolicyTrainer { net, grad, adamw, caches: Vec::new(), conv: Vec::new(), scratch }
    }

    /// Clone this trainer's weights and optimiser moments -- NOT its
    /// scratch buffers (`caches`/`conv`/`scratch`), which are pure working
    /// memory reconstructed on demand ([`Self::ensure_capacity`]) and carry
    /// no state that survives a decision boundary. Pair with
    /// [`Self::from_state`] to branch two runs from one point in training.
    pub fn snapshot(&self) -> (ValueNet, AdamW) {
        (self.net.clone(), self.adamw.clone())
    }

    pub fn zero_grad(&mut self) {
        self.grad.zero();
    }

    pub fn optim_step(&mut self) {
        self.adamw.step(&mut self.net, &self.grad);
    }

    pub fn set_lr(&mut self, lr: f64) {
        self.adamw.set_lr(lr);
    }

    fn ensure_capacity(&mut self, n: usize) {
        while self.caches.len() < n {
            self.caches.push(ForwardCache::zeros(&self.net));
            self.conv.push(vec![0.0; self.net.in_dim]);
        }
    }

    /// Forward every row in `rows` (one `state ++ action` vector per legal
    /// move, [`expand_row`]'s own output, in `legal_moves`'s order),
    /// compute the softmax-cross-entropy loss against `chosen`, and
    /// backpropagate into this trainer's gradient accumulator, scaling each
    /// row's contribution by `scale` (typically `1 / decisions_in_batch`,
    /// so accumulating over a minibatch then calling [`Self::optim_step`]
    /// reproduces `loss = batch.mean(); loss.backward()`).
    ///
    /// Returns `(loss, logits)` -- `logits` (this decision's raw scores,
    /// not softmaxed) so a caller measuring top-1/top-3 agreement does not
    /// need to forward the same rows a second time.
    ///
    /// # Panics
    /// If `rows` is empty or any row's width is not [`POLICY_IN_DIM`].
    pub fn train_decision(&mut self, rows: &[Vec<f32>], chosen: usize, scale: f64) -> (f64, Vec<f64>) {
        let n = rows.len();
        assert!(n > 0, "train_decision: no legal moves");
        self.ensure_capacity(n);
        let mut logits = Vec::with_capacity(n);
        for i in 0..n {
            debug_assert_eq!(rows[i].len(), self.net.in_dim, "train_decision: row {i} width");
            widen_into(&rows[i], &mut self.conv[i]);
            let y = forward_train(&self.net, &self.conv[i], 0.0, None, &mut self.caches[i]);
            logits.push(y);
        }
        let (loss, dlogits) = softmax_cross_entropy(&logits, chosen);
        for i in 0..n {
            backward_train(&self.net, &self.caches[i], &self.conv[i], dlogits[i] * scale, &mut self.grad, &mut self.scratch);
        }
        (loss, logits)
    }
}

/// Forward-only scoring (eval mode: no dropout, no cache kept) for
/// measuring held-out agreement -- a `PolicyTrainer` is for TRAINING; this
/// is what a plain [`ValueNet::forward`] call already does, named here so
/// call sites reporting held-out metrics read as scoring, not training.
pub fn score_row(net: &ValueNet, row: &[f32]) -> f64 {
    let x: Vec<f64> = row.iter().map(|&v| v as f64).collect();
    net.forward(&x)
}

// ========================================================== epoch / measurement
//
// Promoted out of `bin/policytrain.rs` (which originally hand-rolled its
// own copy of every function below) so that `bin/policytrain.rs` and
// `bin/policy_disagreement_experiment.rs` (the control-vs-teacher
// experiment, `docs/NEURAL.md`) measure held-out agreement through the
// EXACT SAME CODE -- two independently-typed copies of a metric are a
// standing risk that a future edit fixes one and not the other, which
// would make any control/teacher comparison worthless without anyone
// noticing.

/// Fisher-Yates shuffle of `v` in place, using this crate's own seeded RNG
/// (never the platform RNG, so a run is exactly reproducible from a seed).
pub fn shuffle_indices(rng: &mut PyRandom, v: &mut [usize]) {
    for i in (1..v.len()).rev() {
        let j = ((rng.random() * (i + 1) as f64) as usize).min(i);
        v.swap(i, j);
    }
}

/// One decision's legal actions expanded to `state ++ action` rows -- built
/// fresh per call, never cached across epochs (at ~7 KB/row this project's
/// "disk is tight" constraint applies to memory too, for a corpus this
/// size).
pub fn rows_for(rec: &DecisionRecord) -> Vec<Vec<f32>> {
    rec.legal.iter().map(|&mv| expand_row(&rec.state, rec.actor, mv)).collect()
}

/// One pass over `records` in a freshly shuffled order, training on every
/// decision and returning the mean per-decision loss. `order` is caller-
/// owned scratch (reused across epochs) sized to `records.len()`.
pub fn train_epoch(trainer: &mut PolicyTrainer, records: &[DecisionRecord], order: &mut [usize], rng: &mut PyRandom) -> f64 {
    shuffle_indices(rng, order);
    let mut total_loss = 0.0;
    for &idx in order.iter() {
        let rec = &records[idx];
        let rows = rows_for(rec);
        trainer.zero_grad();
        let (loss, _logits) = trainer.train_decision(&rows, rec.chosen as usize, 1.0);
        trainer.optim_step();
        total_loss += loss;
    }
    total_loss / order.len() as f64
}

/// Outcome of [`train_until_early_stop`]: the BEST epoch seen (by held-out
/// top-1, never train loss -- see this module's own doc comment on why a
/// fixed-epoch driver undertrains) together with the snapshot taken
/// immediately BEFORE that epoch trained. `snapshot_before_best` exists for
/// callers that branch a second run off the winning epoch (the
/// disagreement-teacher experiment resumes Adam warm from it, never cold --
/// see [`PolicyTrainer::from_state`]'s own doc comment); a caller that only
/// wants a single best checkpoint can ignore it.
pub struct EarlyStopOutcome {
    /// Index of the epoch (0-based) whose held-out top-1 was best.
    pub best_epoch: usize,
    /// How many epochs actually ran before the loop stopped (by patience or
    /// by hitting `max_epochs`) -- always `> best_epoch`, since stopping by
    /// patience requires at least one non-improving epoch after the best.
    pub epochs_run: usize,
    pub best_net: ValueNet,
    pub best_report: HeldOutReport,
    pub snapshot_before_best: (ValueNet, AdamW),
}

/// Train `trainer` for up to `max_epochs` epochs, measuring held-out top-1
/// after every epoch ([`held_out_report`]) and stopping once `patience`
/// consecutive epochs fail to beat the best top-1 seen so far by more than
/// `min_delta`. Prints one `epoch K: ...` line per epoch (the format
/// `bin/policytrain.rs` and `bin/policy_disagreement_experiment.rs` both
/// need to stay comparable) and, on an early stop, one more line naming why.
///
/// Returns the BEST epoch's net and report -- never the last epoch's. By
/// construction the last epoch to run is either the best one itself (loop
/// hit `max_epochs` still improving) or one of the `patience` epochs that
/// already failed to beat it; saving that instead would checkpoint a net a
/// caller already knows is worse than one sitting right there.
///
/// `order`/`rng` are caller-owned scratch, identical in role to
/// [`train_epoch`]'s own parameters of the same name.
///
/// # Errors
/// If `max_epochs == 0`, so no epoch ever ran to produce a best net.
/// One epoch's held-out top-1 against the best seen so far: whether it
/// becomes the new BEST epoch (checkpointed at the end of the run), and
/// whether it resets the PATIENCE counter. Split into its own function,
/// separate from [`train_until_early_stop`]'s loop, so these two questions
/// -- "what's the best net to keep" vs "has training stalled enough to
/// stop" -- can be pinned down directly by a test rather than only through
/// a full (and comparatively expensive) training run.
struct EpochVerdict {
    is_new_best: bool,
    resets_patience: bool,
}

/// `min_delta` gates ONLY `resets_patience`. `is_new_best` is a plain
/// argmax -- if `top1` beats every epoch seen so far, it IS the best,
/// full stop, whether or not the margin clears `min_delta`.
fn epoch_verdict(top1: f64, best_top1: f64, min_delta: f64) -> EpochVerdict {
    EpochVerdict { is_new_best: top1 > best_top1, resets_patience: top1 > best_top1 + min_delta }
}

pub fn train_until_early_stop(
    trainer: &mut PolicyTrainer,
    train: &[DecisionRecord],
    held: &[DecisionRecord],
    order: &mut [usize],
    rng: &mut PyRandom,
    max_epochs: usize,
    patience: usize,
    min_delta: f64,
) -> Result<EarlyStopOutcome, String> {
    let mut best_top1 = f64::NEG_INFINITY;
    let mut best_epoch = 0usize;
    let mut best_net: Option<ValueNet> = None;
    let mut best_report: Option<HeldOutReport> = None;
    let mut snapshot_before_best: Option<(ValueNet, AdamW)> = None;
    let mut patience_left = patience;
    let mut epochs_run = 0usize;

    for epoch in 0..max_epochs {
        let snapshot_before = trainer.snapshot();
        let t0 = Instant::now();
        let mean_loss = train_epoch(trainer, train, order, rng);
        let report = held_out_report(&trainer.net, held);
        let top1 = report.top1_rate();
        epochs_run = epoch + 1;
        println!(
            "epoch {epoch}: mean train loss {mean_loss:.4}  held-out top-1 {top1:.4}  [{:.1}s]",
            t0.elapsed().as_secs_f64()
        );
        let verdict = epoch_verdict(top1, best_top1, min_delta);
        if verdict.is_new_best {
            best_top1 = top1;
            best_epoch = epoch;
            best_net = Some(trainer.net.clone());
            best_report = Some(report);
            snapshot_before_best = Some(snapshot_before);
        }
        if verdict.resets_patience {
            patience_left = patience;
        } else if patience_left == 0 {
            println!("early stop: held-out top-1 has not improved by >= {min_delta} for {patience} epoch(s)");
            break;
        } else {
            patience_left -= 1;
        }
    }

    let best_net = best_net.ok_or_else(|| "train_until_early_stop: max_epochs is 0, no epoch ever ran".to_string())?;
    let best_report = best_report.expect("best_report set alongside best_net");
    let snapshot_before_best = snapshot_before_best.expect("snapshot_before_best set alongside best_net");
    Ok(EarlyStopOutcome { best_epoch, epochs_run, best_net, best_report, snapshot_before_best })
}

/// Held-out agreement, reported alongside the base rate a uniform-random
/// legal move would score -- "a number without its base rate is not a
/// result" (calling task). `top1`/`top3` count how often the champion's
/// actual move ranks 1st / in the top 3 BY THE NET'S OWN LOGIT ORDER;
/// `random_top1`/`random_top3` are `mean(1/L)`/`mean(min(3,L)/L)` over the
/// same decisions -- the exact probability a uniform-random pick from the
/// legal set would have matched, decision by decision, not `1/mean(L)` (a
/// different and less meaningful quantity for decisions of very different
/// sizes).
pub struct HeldOutReport {
    pub n: usize,
    pub top1: usize,
    pub top3: usize,
    pub random_top1: f64,
    pub random_top3: f64,
    pub mean_legal: f64,
    pub n_ge4: usize,
    pub top1_ge4: usize,
    pub top3_ge4: usize,
    pub random_top1_ge4: f64,
    pub random_top3_ge4: f64,
}

impl HeldOutReport {
    pub fn top1_rate(&self) -> f64 {
        self.top1 as f64 / self.n as f64
    }
    pub fn top3_rate(&self) -> f64 {
        self.top3 as f64 / self.n as f64
    }
}

pub fn held_out_report(net: &ValueNet, held: &[DecisionRecord]) -> HeldOutReport {
    let mut r = HeldOutReport {
        n: 0,
        top1: 0,
        top3: 0,
        random_top1: 0.0,
        random_top3: 0.0,
        mean_legal: 0.0,
        n_ge4: 0,
        top1_ge4: 0,
        top3_ge4: 0,
        random_top1_ge4: 0.0,
        random_top3_ge4: 0.0,
    };
    for rec in held {
        let l = rec.legal.len();
        if l == 0 {
            continue;
        }
        let rows = rows_for(rec);
        let mut scored: Vec<(usize, f64)> = rows.iter().enumerate().map(|(i, row)| (i, score_row(net, row))).collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let chosen = rec.chosen as usize;
        let rank = scored.iter().position(|&(i, _)| i == chosen).expect("chosen must be one of the scored rows");

        r.n += 1;
        r.mean_legal += l as f64;
        let rand_top1 = 1.0 / l as f64;
        let rand_top3 = (3.min(l)) as f64 / l as f64;
        r.random_top1 += rand_top1;
        r.random_top3 += rand_top3;
        if rank == 0 {
            r.top1 += 1;
        }
        if rank < 3 {
            r.top3 += 1;
        }
        if l >= 4 {
            r.n_ge4 += 1;
            r.random_top1_ge4 += rand_top1;
            r.random_top3_ge4 += rand_top3;
            if rank == 0 {
                r.top1_ge4 += 1;
            }
            if rank < 3 {
                r.top3_ge4 += 1;
            }
        }
    }
    r
}

/// Bucket held-out decisions into thirds of EACH GAME's own length (early /
/// mid / late) and report top-1 agreement per bucket -- cheap because
/// `read_dump` preserves each game's decisions in play order and
/// `split_by_game` only ever filters (never reorders), so a game's own
/// decisions are still consecutive, in order, inside `held`. Returns
/// `[(agree, total); 3]` for early/mid/late.
pub fn phase_breakdown(net: &ValueNet, held: &[DecisionRecord]) -> [(usize, usize); 3] {
    let mut groups: std::collections::BTreeMap<u32, Vec<usize>> = std::collections::BTreeMap::new();
    for (i, rec) in held.iter().enumerate() {
        groups.entry(rec.game_id).or_default().push(i);
    }
    let mut buckets = [(0usize, 0usize); 3];
    for idxs in groups.values() {
        let len = idxs.len();
        for (pos, &i) in idxs.iter().enumerate() {
            let rec = &held[i];
            if rec.legal.is_empty() {
                continue;
            }
            let rows = rows_for(rec);
            let mut best = 0usize;
            let mut best_score = f64::NEG_INFINITY;
            for (j, row) in rows.iter().enumerate() {
                let s = score_row(net, row);
                if s > best_score {
                    best_score = s;
                    best = j;
                }
            }
            let frac = if len <= 1 { 0.0 } else { pos as f64 / (len - 1) as f64 };
            let bucket = if frac < 1.0 / 3.0 {
                0
            } else if frac < 2.0 / 3.0 {
                1
            } else {
                2
            };
            buckets[bucket].1 += 1;
            if best == rec.chosen as usize {
                buckets[bucket].0 += 1;
            }
        }
    }
    buckets
}

/// Indices into `records` (meant to be called with a TRAIN split only --
/// see this function's own leakage note) where `net`'s own top-ranked
/// legal move disagrees with `rec.chosen`, the move the CHAMPION'S SEARCH
/// actually played. This is the entire point of the disagreement-teacher
/// experiment (calling task, 2026-08-05 work order): the old value-net loop
/// trained on the net's own argmax and measured 0.9764 self-agreement
/// before a single gradient step, so its "improvement operator" was
/// approximately the identity. Comparing against `rec.chosen` -- ground
/// truth recorded at DUMP time by `dump.rs`, from the champion's search,
/// never touched by `net` -- is what keeps this function out of that same
/// trap; it would be a silent regression back into it if this ever
/// compared against `net`'s own prediction on some OTHER record, or against
/// a second call to `net` instead of the stored `chosen` field.
///
/// # Leakage
/// This function only ever reads `records[i].legal`/`.state`/`.actor`/
/// `.chosen` for `i` in `0..records.len()` -- it has no access to any
/// record outside the slice a caller passes it. A caller MUST pass the
/// TRAIN half of [`split_by_game`]'s output, never the held-out half or the
/// pre-split whole corpus; passing anything else leaks held-out games into
/// the emphasis set this function's output is meant to build. This module's
/// tests exercise that boundary explicitly.
pub fn disagreement_indices(net: &ValueNet, records: &[DecisionRecord]) -> Vec<usize> {
    let mut out = Vec::new();
    for (i, rec) in records.iter().enumerate() {
        if rec.legal.is_empty() {
            continue;
        }
        let rows = rows_for(rec);
        let mut best = 0usize;
        let mut best_score = f64::NEG_INFINITY;
        for (j, row) in rows.iter().enumerate() {
            let s = score_row(net, row);
            if s > best_score {
                best_score = s;
                best = j;
            }
        }
        if best != rec.chosen as usize {
            out.push(i);
        }
    }
    out
}

// ============================================================ train/held-out

/// Split `records` into (train, held-out) BY GAME, never by decision:
/// decisions inside one game are heavily correlated (the same actor,
/// tableau and hand evolve turn over turn), so a decision-level split lets
/// near-duplicate rows leak across the split and flatters the held-out
/// numbers. This shuffles the distinct `game_id`s deterministically (seeded
/// [`PyRandom`], this crate's existing RNG rather than a second one) and
/// assigns the first `held_out_frac` fraction of GAMES -- every one of
/// their decisions moves together -- to held-out.
///
/// # Panics
/// If `held_out_frac` is not in `[0.0, 1.0)`.
pub fn split_by_game(records: Vec<DecisionRecord>, held_out_frac: f64, seed: i64) -> (Vec<DecisionRecord>, Vec<DecisionRecord>) {
    assert!((0.0..1.0).contains(&held_out_frac), "split_by_game: held_out_frac must be in [0.0, 1.0)");
    let mut game_ids: Vec<u32> = records.iter().map(|r| r.game_id).collect();
    game_ids.sort_unstable();
    game_ids.dedup();

    let mut rng = PyRandom::new(seed.into());
    for i in (1..game_ids.len()).rev() {
        let j = (rng.random() * (i as f64 + 1.0)) as usize;
        game_ids.swap(i, j.min(i));
    }

    let n_held = ((game_ids.len() as f64) * held_out_frac).round() as usize;
    let held_ids: std::collections::HashSet<u32> = game_ids[..n_held].iter().copied().collect();

    let mut train = Vec::new();
    let mut held = Vec::new();
    for rec in records {
        if held_ids.contains(&rec.game_id) {
            held.push(rec);
        } else {
            train.push(rec);
        }
    }
    (train, held)
}

// ================================================================ checkpoint
//
// A hand-rolled binary format specific to this module's `ValueNet` shape
// (`blocks` always empty, `in_dim` always [`POLICY_IN_DIM`]) -- NOT
// `net::save_checkpoint`/`load_checkpoint`, which hard-codes `in_dim ==
// encode::ENCODING_DIM` (the value net's own width) and would refuse this
// wider policy net outright. Reuses `net.rs`'s `Reader`/`push_u32`/
// `push_f64_slice` primitives (the same byte-level helpers `dump.rs` reuses
// for an unrelated format), not a hand-duplicated copy of them.

const POLICY_CHECKPOINT_MAGIC: &[u8] = b"TTAPOL01";
const POLICY_CHECKPOINT_VERSION: u32 = 1;

/// Serialise a policy [`ValueNet`] (`blocks` must be empty -- see this
/// module's top doc comment) plus `meta` to `path`. Writes a `.tmp` sibling
/// and renames over `path`, matching `net::save_checkpoint`'s crash-safety
/// precedent.
///
/// # Errors
/// If `net.blocks` is non-empty (this format has nowhere to put them --
/// this module never constructs such a net, so this is a defensive check,
/// not a reachable case today), any weight is non-finite, or the write fails.
pub fn save_policy_checkpoint(path: &std::path::Path, net: &ValueNet, meta: &[(&str, f64)]) -> Result<(), String> {
    if !net.blocks.is_empty() {
        return Err(format!("save_policy_checkpoint: expected zero residual blocks, got {}", net.blocks.len()));
    }
    for (name, v) in meta {
        if !v.is_finite() {
            return Err(format!("save_policy_checkpoint: non-finite meta {name:?}: {v}"));
        }
    }
    let mut out = Vec::new();
    out.extend_from_slice(POLICY_CHECKPOINT_MAGIC);
    push_u32(&mut out, POLICY_CHECKPOINT_VERSION);
    push_u32(&mut out, net.in_dim as u32);
    push_u32(&mut out, net.hidden as u32);
    push_f64_slice(&mut out, &net.stem_w);
    push_f64_slice(&mut out, &net.stem_b);
    push_f64_slice(&mut out, &net.stem_ln_gamma);
    push_f64_slice(&mut out, &net.stem_ln_beta);
    push_f64_slice(&mut out, &net.head_w);
    push_f64_slice(&mut out, &[net.head_b]);
    push_u32(&mut out, meta.len() as u32);
    for (name, v) in meta {
        push_u32(&mut out, name.len() as u32);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&v.to_le_bytes());
    }
    let tmp = path.with_extension("tmp");
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
    }
    std::fs::write(&tmp, &out).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Read back what [`save_policy_checkpoint`] wrote.
///
/// # Errors
/// A wrong magic, unsupported version, truncated file, non-utf8 meta key,
/// or an `in_dim` that does not match this build's [`POLICY_IN_DIM`] --
/// mirroring `net::load_checkpoint`'s guard against a checkpoint trained
/// against a stale encoder.
pub fn load_policy_checkpoint(path: &std::path::Path) -> Result<(ValueNet, Vec<(String, f64)>), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut r = Reader::new(&bytes);
    let magic = r.take(POLICY_CHECKPOINT_MAGIC.len())?;
    if magic != POLICY_CHECKPOINT_MAGIC {
        return Err(format!("{}: bad magic {magic:?}: not a policy checkpoint", path.display()));
    }
    let version = r.u32()?;
    if version != POLICY_CHECKPOINT_VERSION {
        return Err(format!(
            "{}: checkpoint version {version}, this build reads version {POLICY_CHECKPOINT_VERSION}",
            path.display()
        ));
    }
    let in_dim = r.u32()? as usize;
    if in_dim != POLICY_IN_DIM {
        return Err(format!(
            "{}: checkpoint in_dim {in_dim} does not match this build's POLICY_IN_DIM {POLICY_IN_DIM} -- \
             this checkpoint was trained against a different encoder, do not load it as-is",
            path.display()
        ));
    }
    let hidden = r.u32()? as usize;
    let stem_w = r.f64_vec(hidden * in_dim)?;
    let stem_b = r.f64_vec(hidden)?;
    let stem_ln_gamma = r.f64_vec(hidden)?;
    let stem_ln_beta = r.f64_vec(hidden)?;
    let head_w = r.f64_vec(hidden)?;
    let head_b = r.f64()?;
    let n_meta = r.u32()? as usize;
    let mut meta = Vec::with_capacity(n_meta);
    for _ in 0..n_meta {
        let klen = r.u32()? as usize;
        let key = r.string(klen)?;
        let val = r.f64()?;
        meta.push((key, val));
    }
    if r.remaining() != 0 {
        return Err(format!("{}: {} trailing bytes after a well-formed checkpoint", path.display(), r.remaining()));
    }
    let net = ValueNet { in_dim, hidden, stem_w, stem_b, stem_ln_gamma, stem_ln_beta, blocks: vec![], head_w, head_b };
    Ok((net, meta))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::CardId;

    /// Softmax cross-entropy over a decision's own logits: probabilities
    /// sum to 1 and the gradient is the textbook `softmax - one_hot`,
    /// checked against a hand-computed 3-candidate example (logits
    /// `[0, 1, 2]`) rather than trusted on derivation alone.
    #[test]
    fn softmax_cross_entropy_matches_a_hand_computed_example() {
        let logits = vec![0.0, 1.0, 2.0];
        let (loss, grad) = softmax_cross_entropy(&logits, 2);
        let exps: Vec<f64> = logits.iter().map(|&l| l.exp()).collect();
        let sum: f64 = exps.iter().sum();
        let probs: Vec<f64> = exps.iter().map(|&e| e / sum).collect();
        for i in 0..3 {
            let want = if i == 2 { probs[i] - 1.0 } else { probs[i] };
            assert!((grad[i] - want).abs() < 1e-9, "grad[{i}] = {}, want {}", grad[i], want);
        }
        assert!((loss - (-probs[2].ln())).abs() < 1e-9);
    }

    /// The gradient [`softmax_cross_entropy`] returns matches a finite
    /// difference of the loss it also returns -- the same discipline
    /// `train.rs`'s own gradient tests apply to every hand-derived
    /// gradient in this crate.
    #[test]
    fn softmax_cross_entropy_gradient_matches_finite_differences() {
        let base = vec![0.3, -1.2, 2.1, 0.05];
        let chosen = 1;
        let (_, grad) = softmax_cross_entropy(&base, chosen);
        let eps = 1e-6;
        for i in 0..base.len() {
            let mut up = base.clone();
            up[i] += eps;
            let mut down = base.clone();
            down[i] -= eps;
            let (loss_up, _) = softmax_cross_entropy(&up, chosen);
            let (loss_down, _) = softmax_cross_entropy(&down, chosen);
            let numeric = (loss_up - loss_down) / (2.0 * eps);
            assert!((numeric - grad[i]).abs() < 1e-4, "logit {i}: analytic {}, numeric {numeric}", grad[i]);
        }
    }

    /// Softmax is normalised over EXACTLY the candidates passed in -- never
    /// mixed with a different decision's legal set. Two decisions share the
    /// SAME chosen-candidate logit value but have different-sized legal
    /// sets around it; if the implementation ever normalised over a fixed
    /// width (padding, or a global candidate pool) instead of the given
    /// slice, these two calls would disagree with a per-decision softmax
    /// computed by hand for each size.
    #[test]
    fn softmax_normalises_over_the_given_legal_set_only_not_a_fixed_width() {
        let small = vec![1.0, 2.0]; // chosen = 1
        let large = vec![1.0, 2.0, -5.0, -5.0, -5.0, -5.0, -5.0, -5.0]; // chosen = 1, padded with near-zero-probability filler
        let (_, grad_small) = softmax_cross_entropy(&small, 1);
        let (_, grad_large) = softmax_cross_entropy(&large, 1);
        // The two-candidate probability for the chosen slot must differ
        // from the eight-candidate one (more competitors soak up some
        // probability mass, even if only a little here) -- proof the
        // normalising constant is a function of the ACTUAL slice length,
        // not a constant baked in some other way.
        assert!((grad_small[1] - grad_large[1]).abs() > 1e-6, "the two softmaxes must not agree by coincidence of a shared fixed width");
    }

    fn tiny_row(state_val: f32, action_val: f32) -> Vec<f32> {
        let mut row = vec![state_val; ENCODING_DIM];
        row.extend(vec![action_val; action::ACTION_DIM]);
        row
    }

    /// [`PolicyTrainer::train_decision`] repeatedly trained on the SAME
    /// trivial decision (two candidates, one obviously distinguishable by a
    /// large feature-value gap) drives the loss down -- the same "is the
    /// gradient flowing at all" sanity check `train.rs`'s own
    /// `training_on_a_trivial_value_regression_drives_the_loss_down` runs
    /// for the value net's loss.
    #[test]
    fn training_on_a_trivial_repeated_decision_drives_the_loss_down() {
        let net = random_policy_net(8, 42);
        let mut trainer = PolicyTrainer::new(net, 0.05, 0.0);
        let rows = vec![tiny_row(0.0, 0.0), tiny_row(1.0, 1.0)];
        let chosen = 1;

        let (first_loss, _) = trainer.train_decision(&rows, chosen, 1.0);
        trainer.optim_step();
        let mut last_loss = first_loss;
        for _ in 0..200 {
            trainer.zero_grad();
            let (loss, _) = trainer.train_decision(&rows, chosen, 1.0);
            trainer.optim_step();
            last_loss = loss;
        }
        assert!(last_loss < first_loss * 0.5, "loss did not drop: first {first_loss}, last {last_loss}");
    }

    /// [`expand_row`] is exactly `state ++ encode_action(actor, mv)`,
    /// narrowed to `f32` -- the property `bin/policytrain.rs` depends on to
    /// reconstruct the dense row a `DecisionRecord`'s compact `Move` never
    /// stored on disk.
    #[test]
    fn expand_row_concatenates_state_and_the_actions_dense_encoding() {
        let state: Vec<f32> = (0..ENCODING_DIM).map(|i| i as f32 * 0.001).collect();
        let card = CardId::by_name("Warriors").unwrap();
        let mv = Move::Build { card };
        let row = expand_row(&state, 1, mv);
        assert_eq!(row.len(), POLICY_IN_DIM);
        assert_eq!(&row[..ENCODING_DIM], &state[..]);
        let want_action = encode_action(1, mv);
        for (got, want) in row[ENCODING_DIM..].iter().zip(want_action.iter()) {
            assert!((*got as f64 - want).abs() < 1e-6);
        }
    }

    /// `save_policy_checkpoint`/`load_policy_checkpoint` round-trip a
    /// policy net exactly -- same property `net::checkpoint_round_trip_is_
    /// bit_for_bit_exact` pins for the value net's own format.
    #[test]
    fn policy_checkpoint_round_trips() {
        let net = random_policy_net(6, 7);
        let dir = std::env::temp_dir().join(format!("ttapolicy_ckpt_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("policy.ckpt");
        save_policy_checkpoint(&path, &net, &[("epoch", 3.0)]).unwrap();
        let (loaded, meta) = load_policy_checkpoint(&path).unwrap();
        assert_eq!(loaded, net);
        assert_eq!(meta, vec![("epoch".to_string(), 3.0)]);
        std::fs::remove_file(&path).ok();
        std::fs::remove_dir(&dir).ok();
    }

    fn tiny_record(game_id: u32, actor: u8) -> DecisionRecord {
        DecisionRecord {
            players: 2,
            actor,
            game_id,
            state: vec![0.0; ENCODING_DIM],
            legal: vec![Move::EndTurn, Move::PolPass],
            chosen: 0,
            result: 1.0,
        }
    }

    /// The property [`split_by_game`] exists for: no `game_id` ever has
    /// decisions on BOTH sides of the split -- several games contribute
    /// multiple decisions each here, which is exactly the shape that would
    /// expose a decision-level (rather than game-level) split.
    #[test]
    fn split_by_game_never_puts_one_games_decisions_on_both_sides() {
        let mut records = Vec::new();
        for game in 0..40u32 {
            for actor in 0..5u8 {
                records.push(tiny_record(game, actor % 2));
            }
        }
        let (train, held) = split_by_game(records, 0.25, 99);
        let train_games: std::collections::HashSet<u32> = train.iter().map(|r| r.game_id).collect();
        let held_games: std::collections::HashSet<u32> = held.iter().map(|r| r.game_id).collect();
        assert!(train_games.is_disjoint(&held_games), "a game_id appears on both sides of the split");
        assert!(!held_games.is_empty(), "held_out_frac=0.25 over 40 games must hold some out");
        assert_eq!(train.len() + held.len(), 40 * 5);
    }

    /// A checkpoint trained at one `in_dim` (e.g. a stale `ACTION_DIM`)
    /// refuses to load under today's `POLICY_IN_DIM` -- mirrors
    /// `net::a_checkpoint_saved_at_one_encoder_width_refuses_to_load_at_
    /// another`.
    #[test]
    fn load_policy_checkpoint_rejects_a_mismatched_in_dim() {
        let mut net = random_policy_net(4, 1);
        net.in_dim = POLICY_IN_DIM + 1;
        net.stem_w = vec![0.0; net.hidden * net.in_dim];
        let dir = std::env::temp_dir().join(format!("ttapolicy_ckpt_test_dim_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad_dim.ckpt");
        save_policy_checkpoint(&path, &net, &[]).unwrap();
        let err = load_policy_checkpoint(&path).unwrap_err();
        assert!(err.contains(&(POLICY_IN_DIM + 1).to_string()) && err.contains(&POLICY_IN_DIM.to_string()), "{err}");
        std::fs::remove_file(&path).ok();
        std::fs::remove_dir(&dir).ok();
    }

    /// [`disagreement_indices`] must compare each record's own top-scoring
    /// legal move against `rec.chosen` -- the move the CHAMPION'S SEARCH
    /// actually played -- and nothing else. This is the identity trap the
    /// whole disagreement-teacher work order exists to avoid: the old
    /// value-net loop trained on the net's own argmax and measured 0.9764
    /// self-agreement before a single gradient step. Two records share the
    /// SAME net and the SAME candidate rows; only their `chosen` field
    /// differs (one set to the net's own preferred candidate, one set to
    /// the other one), so a broken implementation that ignored `chosen`
    /// entirely (always flags nothing, or always flags everything, or
    /// flags by some other rule such as "index 0") would fail this
    /// specific pairing.
    #[test]
    fn disagreement_indices_flags_exactly_the_records_where_chosen_differs_from_the_nets_own_top_pick() {
        let net = random_policy_net(8, 123);
        let probe = tiny_record(0, 0);
        let rows = rows_for(&probe);
        let scores: Vec<f64> = rows.iter().map(|r| score_row(&net, r)).collect();
        let preferred: u32 = if scores[0] >= scores[1] { 0 } else { 1 };
        let other: u32 = 1 - preferred;

        let agree = DecisionRecord { chosen: preferred, ..tiny_record(1, 0) };
        let disagree = DecisionRecord { chosen: other, ..tiny_record(2, 0) };
        let idxs = disagreement_indices(&net, &[agree, disagree]);
        assert_eq!(idxs, vec![1], "only the record whose `chosen` differs from the net's own top pick should be flagged");
    }

    /// [`disagreement_indices`] only ever reads the slice it is handed --
    /// calling it with the TRAIN half of [`split_by_game`]'s output must
    /// never surface a held-out game. Rather than trusting that property on
    /// the function's signature alone, this test demonstrates the failure
    /// mode it guards against: calling the SAME function with the
    /// pre-split corpus (the bug this guards `bin/policy_disagreement_
    /// experiment.rs`'s call site against) DOES leak held-out games for
    /// this seed, so the first assertion is not vacuously true.
    #[test]
    fn disagreement_indices_on_the_train_split_never_surfaces_a_held_out_game() {
        let net = random_policy_net(8, 5);
        // Probe each actor's preferred candidate on this net, then set every
        // record's `chosen` to the OTHER candidate -- guaranteeing a 100%
        // disagreement rate (not a lucky RNG draw) so the sanity check below
        // is deterministic rather than occasionally vacuous.
        let preferred_for = |actor: u8| -> u32 {
            let probe = tiny_record(0, actor);
            let rows = rows_for(&probe);
            let scores: Vec<f64> = rows.iter().map(|r| score_row(&net, r)).collect();
            if scores[0] >= scores[1] { 0 } else { 1 }
        };
        let other0 = 1 - preferred_for(0);
        let other1 = 1 - preferred_for(1);

        let mut all_records = Vec::new();
        for game in 0..20u32 {
            all_records.push(DecisionRecord { chosen: other0, ..tiny_record(game, 0) });
            all_records.push(DecisionRecord { chosen: other1, ..tiny_record(game, 1) });
        }
        let everything = all_records.clone();
        let (train, held) = split_by_game(all_records, 0.3, 7);
        assert!(!held.is_empty(), "this seed/frac must hold at least one game out for the test to mean anything");
        let held_games: std::collections::HashSet<u32> = held.iter().map(|r| r.game_id).collect();

        let idxs_train = disagreement_indices(&net, &train);
        for &i in &idxs_train {
            assert!(!held_games.contains(&train[i].game_id), "disagreement_indices flagged a record from a held-out game when called with the train split alone");
        }

        // Sanity check: calling it with the UNSPLIT corpus (the bug this
        // test exists to catch at the call site) must actually produce a
        // held-out hit for this seed, otherwise the assertion above would
        // pass even with a leaking implementation.
        let idxs_all = disagreement_indices(&net, &everything);
        let leaked = idxs_all.iter().any(|&i| held_games.contains(&everything[i].game_id));
        assert!(leaked, "sanity check failed: this net/corpus/seed produced no held-out disagreement even when queried directly, so this test cannot distinguish a leaking call from a safe one");
    }

    /// [`PolicyTrainer::snapshot`]/[`PolicyTrainer::from_state`] must
    /// preserve BOTH the weights and the optimiser's Adam moments, not just
    /// the weights -- the disagreement-teacher experiment relies on this to
    /// give both the control and teacher branches equally warm-started
    /// optimiser state. Training straight through two decisions and
    /// training the first decision, snapshotting, then resuming for the
    /// second decision from a FRESH `PolicyTrainer` must land on the exact
    /// same weights either way; if `snapshot`/`from_state` dropped the Adam
    /// moments (restarting `t`/`m`/`v` from zero), the resumed run's second
    /// step would take a measurably different step and this would fail.
    #[test]
    fn resuming_a_trainer_from_a_snapshot_reproduces_uninterrupted_training() {
        let net = random_policy_net(6, 11);
        let rows_a = vec![tiny_row(0.0, 0.0), tiny_row(1.0, 1.0)];
        let rows_b = vec![tiny_row(0.2, 0.3), tiny_row(0.9, 0.1), tiny_row(0.5, 0.5)];

        let mut continuous = PolicyTrainer::new(net, 0.01, 1e-4);
        continuous.zero_grad();
        continuous.train_decision(&rows_a, 1, 1.0);
        continuous.optim_step();
        let (snap_net, snap_adamw) = continuous.snapshot();
        continuous.zero_grad();
        continuous.train_decision(&rows_b, 2, 1.0);
        continuous.optim_step();

        let mut resumed = PolicyTrainer::from_state(snap_net, snap_adamw);
        resumed.zero_grad();
        resumed.train_decision(&rows_b, 2, 1.0);
        resumed.optim_step();

        assert_eq!(
            continuous.net, resumed.net,
            "resuming from a snapshot must reproduce the same next step as training straight through -- otherwise a branch that resumes trains differently for reasons that have nothing to do with which decisions it saw"
        );
    }

    /// [`train_until_early_stop`] must stop BEFORE `max_epochs` once
    /// `patience` consecutive epochs fail to improve held-out top-1 -- the
    /// entire point of adding it (`docs/NEURAL.md`'s negative result:
    /// `bin/policytrain.rs`'s old fixed-`--epochs` driver had no way to know
    /// it was still improving, or had stopped, at all). Learning rate 0
    /// makes every epoch a no-op (see `train.rs`'s `adamw_update`: `param -=
    /// lr * wd * param` then `param -= lr * mhat / ...`, both terms zero
    /// when `lr == 0`), so held-out top-1 is EXACTLY equal every epoch --
    /// deterministically "no improvement" from epoch 1 onward, with no
    /// dependence on how well this particular tiny net happens to train.
    #[test]
    fn early_stopping_fires_before_max_epochs_once_patience_is_exhausted() {
        let net = random_policy_net(6, 3);
        let mut trainer = PolicyTrainer::new(net, 0.0, 0.0);
        let train = vec![tiny_record(0, 0), tiny_record(1, 0)];
        let held = vec![tiny_record(2, 0)];
        let mut order: Vec<usize> = (0..train.len()).collect();
        let mut rng = PyRandom::new(7);

        let outcome = train_until_early_stop(&mut trainer, &train, &held, &mut order, &mut rng, 10, 2, 0.001).unwrap();

        // epoch 0 is always "best" (any finite top-1 beats the initial
        // -infinity); epochs 1, 2, 3 then each measure the SAME top-1 (lr=0
        // froze the weights), consuming patience=2 worth of grace before the
        // loop breaks at epoch 3 -- well short of the max_epochs=10 cap.
        assert_eq!(outcome.best_epoch, 0, "with a frozen net, epoch 0 is the only epoch that can ever count as an improvement");
        assert_eq!(outcome.epochs_run, 4, "patience=2 should exhaust after epochs 1-3 report no improvement over epoch 0");
        assert!(outcome.epochs_run < 10, "early stopping must fire before the max_epochs cap, or this test proves nothing");
    }

    /// [`train_until_early_stop`] must hand back the net from the BEST
    /// epoch, not whatever the trainer holds after the loop exits -- saving
    /// the last epoch once patience is exhausted would checkpoint a net
    /// already known to be worse (this change's entire reason for
    /// existing). A single held-out decision with a clean feature gap
    /// (`EndTurn` vs `PolPass`, repeated identically in `train`) reaches
    /// top-1 = 1.0 -- the maximum possible with `n == 1` -- within a couple
    /// of epochs and then CANNOT improve further, so every epoch after that
    /// consumes patience while still mutating `trainer`'s live weights
    /// (AdamW's residual gradient/decay never becomes exactly zero for a
    /// softmax that has not saturated to +/-infinity). That gives a
    /// deterministic mismatch between the returned `best_net` and
    /// `trainer.net` after the call, without relying on the exact epoch
    /// convergence happens to land on.
    #[test]
    fn train_until_early_stop_returns_the_best_epochs_net_not_the_last_ones() {
        let net = random_policy_net(8, 42);
        let mut trainer = PolicyTrainer::new(net, 0.2, 0.0);
        // 30 identical copies of the same two-move decision, always choosing
        // index 1 -- enough repeated gradient steps within ONE epoch to
        // reliably drive the net's preference toward index 1.
        let train: Vec<DecisionRecord> = (0..30u32).map(|g| DecisionRecord { chosen: 1, ..tiny_record(g, 0) }).collect();
        let held = vec![DecisionRecord { chosen: 1, ..tiny_record(999, 0) }];
        let mut order: Vec<usize> = (0..train.len()).collect();
        let mut rng = PyRandom::new(11);

        let outcome = train_until_early_stop(&mut trainer, &train, &held, &mut order, &mut rng, 8, 1, 0.001).unwrap();

        assert_eq!(outcome.best_report.n, 1);
        assert_eq!(outcome.best_report.top1, 1, "the trivially-separable decision must be learned well within 8 epochs");
        assert!(
            outcome.best_epoch < outcome.epochs_run - 1,
            "at least one more epoch must have run after the best one (patience={}, epochs_run={}, best_epoch={}) -- \
             otherwise this test cannot distinguish 'saved the best' from 'saved the last'",
            1,
            outcome.epochs_run,
            outcome.best_epoch
        );
        assert_ne!(
            outcome.best_net, trainer.net,
            "the returned net must be the BEST epoch's weights, not the LAST epoch's -- trainer.net keeps mutating \
             after the best epoch (patience epochs still train) so these must differ"
        );
    }

    /// A real bug (calling task, 2026-08-12): a smoke test showed epoch 2
    /// scoring 0.5272 and epoch 0 scoring 0.5102, yet the run reported epoch
    /// 0 as best, because `min_delta` was gating BOTH "is this the best
    /// epoch" and "does this reset patience" through one shared condition.
    /// `min_delta` must gate patience ONLY -- an epoch that improves by
    /// less than `min_delta` still has to become the new best (a plain
    /// argmax has no threshold), while still counting as a non-improving
    /// epoch for patience purposes. `0.52` vs a prior best of `0.51` with
    /// `min_delta = 0.02` is exactly that: a real (if small) improvement
    /// that must win the "best" question and lose the "reset patience"
    /// question, not the same answer to both.
    #[test]
    fn an_epoch_that_improves_by_less_than_min_delta_still_becomes_the_new_best() {
        let verdict = epoch_verdict(0.52, 0.51, 0.02);
        assert!(
            verdict.is_new_best,
            "0.52 beats the prior best of 0.51 -- it must become the new best regardless of min_delta, or a later, \
             genuinely-better epoch never displaces an earlier lucky one"
        );
        assert!(
            !verdict.resets_patience,
            "the 0.01 gain is below min_delta=0.02, so patience must still treat this epoch as non-improving -- \
             min_delta governs ONLY when to stop, never which net to keep"
        );
    }
}
