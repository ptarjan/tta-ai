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
}
