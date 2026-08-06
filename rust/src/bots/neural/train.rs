//! Backpropagation, AdamW, and the combined value+ranking training
//! objective for [`ValueNet`] -- the training half of the neural port whose
//! forward pass lives in `net.rs`. Mirrors `experiments/neural_train_rank.py`
//! (244 lines); read its own module docstring first for the objective's
//! shape (value regression PLUS a Bradley-Terry pairwise ranking loss) and
//! for why `rust/src/bin/neuraltrain.rs` prints an epoch-0 VACUITY line
//! before the first gradient step -- that reporting exists because of a
//! measured 41-hour null (`docs/NEURAL_LOOP_NULL.md`) where nobody looked
//! at epoch 0 and a vacuous target went unnoticed for 74 iterations.
//!
//! ## Hand-rolled backprop, and how it is checked
//!
//! `Cargo.toml`'s `[dependencies]` stays empty -- no autodiff crate. Every
//! gradient below ([`ValueNetGrad`], the `*_backward*` functions) is worked
//! out by hand and pinned against finite differences in `#[cfg(test)]`. A
//! hand gradient is easy to get subtly wrong in a way that trains slowly
//! rather than crashes, so `the_analytic_gradient_of_*` below covers every
//! parameter TYPE (stem linear+LayerNorm, each block's two linears and
//! LayerNorm, the head) rather than only the last layer, which would pass
//! for the wrong reason.
//!
//! ## Loss, exactly as the Python script computes it (not as its prose
//! simplifies it)
//!
//! `neural_train_rank.py`'s module docstring says "value_mse"; its CODE
//! calls `F.smooth_l1_loss` (Huber, `beta=1.0`), and that is what
//! [`smooth_l1`] ports -- the script is the spec, and where the docstring
//! and the code disagree, the code wins.
//!
//! ## Data format
//!
//! Training data is [`super::rankdata::Shard`] (`RKD1`, `f32` rows) --
//! that format landed on master from the other half of this port
//! (`rust/src/bots/neural/rankdata.rs`) while this file was being written,
//! so per the plan this file ADOPTS it rather than defining its own; see
//! [`Trainer::accumulate_pair`]/[`Trainer::accumulate_value`] for exactly
//! where the `f32` shard rows get converted to the `f64` this file's
//! arithmetic runs in (a conversion into a buffer [`Trainer`] owns, not a
//! fresh allocation per row).

use crate::rng::PyRandom;

use super::net::{layer_norm_stats, relu_inplace, ValueNet, LAYER_NORM_EPS, MARGIN_NORM};
use super::rankdata::{RankPair, ValueRow};

// ================================================================ gradients

/// Gradient accumulator for one [`ResBlock`], same shape as the block
/// itself -- each array is tied to its parameter by FIELD NAME, the same
/// structural shadow [`ValueNetGrad`] is for [`ValueNet`], not a
/// separately-indexed parallel collection.
#[derive(Clone)]
struct ResBlockGrad {
    fc1_w: Vec<f64>,
    fc1_b: Vec<f64>,
    fc2_w: Vec<f64>,
    fc2_b: Vec<f64>,
    ln_gamma: Vec<f64>,
    ln_beta: Vec<f64>,
}

impl ResBlockGrad {
    fn zeros(dim: usize) -> Self {
        ResBlockGrad {
            fc1_w: vec![0.0; dim * dim],
            fc1_b: vec![0.0; dim],
            fc2_w: vec![0.0; dim * dim],
            fc2_b: vec![0.0; dim],
            ln_gamma: vec![0.0; dim],
            ln_beta: vec![0.0; dim],
        }
    }

    fn zero(&mut self) {
        self.fc1_w.iter_mut().for_each(|x| *x = 0.0);
        self.fc1_b.iter_mut().for_each(|x| *x = 0.0);
        self.fc2_w.iter_mut().for_each(|x| *x = 0.0);
        self.fc2_b.iter_mut().for_each(|x| *x = 0.0);
        self.ln_gamma.iter_mut().for_each(|x| *x = 0.0);
        self.ln_beta.iter_mut().for_each(|x| *x = 0.0);
    }
}

/// Gradient accumulator for a whole [`ValueNet`]. Also reused, with the
/// same shape, as the first/second-moment buffers `AdamW` needs per
/// parameter (`m`/`v` in the optimizer are literally two more values of
/// this type).
struct ValueNetGrad {
    stem_w: Vec<f64>,
    stem_b: Vec<f64>,
    stem_ln_gamma: Vec<f64>,
    stem_ln_beta: Vec<f64>,
    blocks: Vec<ResBlockGrad>,
    head_w: Vec<f64>,
    head_b: f64,
}

impl ValueNetGrad {
    fn zeros_like(net: &ValueNet) -> Self {
        ValueNetGrad {
            stem_w: vec![0.0; net.hidden * net.in_dim],
            stem_b: vec![0.0; net.hidden],
            stem_ln_gamma: vec![0.0; net.hidden],
            stem_ln_beta: vec![0.0; net.hidden],
            blocks: net.blocks.iter().map(|_| ResBlockGrad::zeros(net.hidden)).collect(),
            head_w: vec![0.0; net.hidden],
            head_b: 0.0,
        }
    }

    fn zero(&mut self) {
        self.stem_w.iter_mut().for_each(|x| *x = 0.0);
        self.stem_b.iter_mut().for_each(|x| *x = 0.0);
        self.stem_ln_gamma.iter_mut().for_each(|x| *x = 0.0);
        self.stem_ln_beta.iter_mut().for_each(|x| *x = 0.0);
        for b in &mut self.blocks {
            b.zero();
        }
        self.head_w.iter_mut().for_each(|x| *x = 0.0);
        self.head_b = 0.0;
    }
}

// ============================================================ forward cache

/// Everything one [`ResBlock::forward`] needs remembered for backprop.
struct BlockCache {
    /// The block's input (a copy, not a borrow -- see `forward_train`'s doc
    /// comment on why: it lets each block's cache be self-contained rather
    /// than holding a lifetime into the previous one).
    h_in: Vec<f64>,
    a1: Vec<f64>,
    r1: Vec<f64>,
    a2: Vec<f64>,
    /// Per-unit dropout multiplier actually applied this forward pass:
    /// `1/(1-p)` where kept, `0.0` where dropped, all `1.0` in eval mode
    /// (inverted dropout, matching `nn.Dropout`'s train/eval split).
    drop_mask: Vec<f64>,
    resid: Vec<f64>,
    ln_mean: f64,
    ln_var: f64,
    h_out: Vec<f64>,
}

impl BlockCache {
    fn zeros(dim: usize) -> Self {
        BlockCache {
            h_in: vec![0.0; dim],
            a1: vec![0.0; dim],
            r1: vec![0.0; dim],
            a2: vec![0.0; dim],
            drop_mask: vec![1.0; dim],
            resid: vec![0.0; dim],
            ln_mean: 0.0,
            ln_var: 0.0,
            h_out: vec![0.0; dim],
        }
    }
}

/// Recorded intermediates for one forward pass, reused across calls (every
/// `Vec` is pre-sized once and overwritten in place -- no allocation in the
/// per-row hot loop).
struct ForwardCache {
    stem_z: Vec<f64>,
    stem_ln_mean: f64,
    stem_ln_var: f64,
    stem_out: Vec<f64>,
    blocks: Vec<BlockCache>,
}

impl ForwardCache {
    fn zeros(net: &ValueNet) -> Self {
        ForwardCache {
            stem_z: vec![0.0; net.hidden],
            stem_ln_mean: 0.0,
            stem_ln_var: 0.0,
            stem_out: vec![0.0; net.hidden],
            blocks: net.blocks.iter().map(|_| BlockCache::zeros(net.hidden)).collect(),
        }
    }

    /// The block-input this cache should have recorded for the FIRST block
    /// (`stem_out`) -- named so `forward_train` reads as "propagate the
    /// stem's output forward" rather than a bare field access.
    fn last_hidden(&self) -> &[f64] {
        match self.blocks.last() {
            Some(b) => &b.h_out,
            None => &self.stem_out,
        }
    }
}

// ============================================================== arithmetic
//
// `linear_into`/`layer_norm_into` duplicate `net::linear`/`net::layer_norm`'s
// FORMULA (same accumulation order, checked in
// `forward_train_at_eval_matches_value_net_forward` below) but write into a
// caller-supplied buffer instead of allocating -- the training hot loop
// calls these millions of times per epoch.

fn linear_into(w: &[f64], b: &[f64], x: &[f64], in_dim: usize, out: &mut [f64]) {
    let out_dim = out.len();
    debug_assert_eq!(w.len(), out_dim * in_dim);
    debug_assert_eq!(b.len(), out_dim);
    debug_assert_eq!(x.len(), in_dim);
    for o in 0..out_dim {
        let row = &w[o * in_dim..(o + 1) * in_dim];
        let mut acc = b[o];
        for i in 0..in_dim {
            acc += row[i] * x[i];
        }
        out[o] = acc;
    }
}

fn layer_norm_into(x: &[f64], mean: f64, var: f64, gamma: &[f64], beta: &[f64], out: &mut [f64]) {
    let denom = (var + LAYER_NORM_EPS).sqrt();
    for i in 0..x.len() {
        out[i] = (x[i] - mean) / denom * gamma[i] + beta[i];
    }
}

/// Run [`ValueNet`]'s forward pass, recording everything backprop needs
/// into `cache` (pre-sized by [`ForwardCache::zeros`], overwritten in
/// place), and return the scalar prediction.
///
/// When `rng` is `Some`, dropout is ACTIVE: inverted dropout, so a kept
/// unit is scaled by `1/(1-p)` at train time and eval needs no rescaling
/// (matches `nn.Dropout`). When `rng` is `None`, every `drop_mask` entry is
/// `1.0` -- the eval-mode identity `net.rs`'s own `ValueNet::forward` uses,
/// checked directly against it in
/// `forward_train_at_eval_matches_value_net_forward` below.
fn forward_train(net: &ValueNet, x: &[f64], dropout_p: f64, mut rng: Option<&mut PyRandom>, cache: &mut ForwardCache) -> f64 {
    let hidden = net.hidden;
    linear_into(&net.stem_w, &net.stem_b, x, net.in_dim, &mut cache.stem_z);
    let (mean, var) = layer_norm_stats(&cache.stem_z);
    cache.stem_ln_mean = mean;
    cache.stem_ln_var = var;
    layer_norm_into(&cache.stem_z, mean, var, &net.stem_ln_gamma, &net.stem_ln_beta, &mut cache.stem_out);
    relu_inplace(&mut cache.stem_out);

    if let Some(first) = cache.blocks.first_mut() {
        first.h_in.copy_from_slice(&cache.stem_out);
    }

    for i in 0..net.blocks.len() {
        let block = &net.blocks[i];
        let bc = &mut cache.blocks[i];

        linear_into(&block.fc1_w, &block.fc1_b, &bc.h_in, hidden, &mut bc.a1);
        bc.r1.copy_from_slice(&bc.a1);
        relu_inplace(&mut bc.r1);
        linear_into(&block.fc2_w, &block.fc2_b, &bc.r1, hidden, &mut bc.a2);

        match rng.as_deref_mut() {
            Some(r) => {
                let keep_prob = 1.0 - dropout_p;
                for j in 0..hidden {
                    bc.drop_mask[j] = if r.random() < keep_prob { 1.0 / keep_prob } else { 0.0 };
                }
            }
            None => bc.drop_mask.iter_mut().for_each(|m| *m = 1.0),
        }

        for j in 0..hidden {
            bc.resid[j] = bc.h_in[j] + bc.a2[j] * bc.drop_mask[j];
        }
        let (mean, var) = layer_norm_stats(&bc.resid);
        bc.ln_mean = mean;
        bc.ln_var = var;
        layer_norm_into(&bc.resid, mean, var, &block.ln_gamma, &block.ln_beta, &mut bc.h_out);
        relu_inplace(&mut bc.h_out);

        if i + 1 < cache.blocks.len() {
            let (left, right) = cache.blocks.split_at_mut(i + 1);
            right[0].h_in.copy_from_slice(&left[i].h_out);
        }
    }

    let last_h = cache.last_hidden();
    let mut y = net.head_b;
    for j in 0..hidden {
        y += net.head_w[j] * last_h[j];
    }
    y
}

/// Scratch buffers backprop needs, all sized `hidden` and reused across
/// every call within a training run -- the backward-pass analogue of
/// [`ForwardCache`]'s "pre-sized, overwritten in place" rule.
struct BackScratch {
    g_out: Vec<f64>,
    g_lnout: Vec<f64>,
    g_resid: Vec<f64>,
    g_a2: Vec<f64>,
    g_r1: Vec<f64>,
    g_a1: Vec<f64>,
    g_hin_fc1: Vec<f64>,
    xhat: Vec<f64>,
    dxhat: Vec<f64>,
}

impl BackScratch {
    fn zeros(hidden: usize) -> Self {
        BackScratch {
            g_out: vec![0.0; hidden],
            g_lnout: vec![0.0; hidden],
            g_resid: vec![0.0; hidden],
            g_a2: vec![0.0; hidden],
            g_r1: vec![0.0; hidden],
            g_a1: vec![0.0; hidden],
            g_hin_fc1: vec![0.0; hidden],
            xhat: vec![0.0; hidden],
            dxhat: vec![0.0; hidden],
        }
    }
}

fn relu_backward_into(post_relu: &[f64], dy: &[f64], dx_out: &mut [f64]) {
    for i in 0..dy.len() {
        dx_out[i] = if post_relu[i] > 0.0 { dy[i] } else { 0.0 };
    }
}

/// LayerNorm backward: given `dy` (gradient wrt the affine output) and the
/// pre-normalisation `x`/`mean`/`var` that produced it, accumulate into
/// `dgamma`/`dbeta` and write `dx` (gradient wrt the pre-normalisation
/// input) into `dx_out`.
///
/// Standard closed form (the same identity BatchNorm-backward uses, here
/// normalising over the FEATURE axis instead of the batch axis): with
/// `xhat_i = (x_i - mean)/std` and `dxhat_i = dy_i * gamma_i`,
///
/// ```text
/// dx_i = (N*dxhat_i - sum(dxhat) - xhat_i * sum(dxhat*xhat)) / (N*std)
/// ```
///
/// pinned against finite differences below rather than trusted on
/// derivation alone.
#[allow(clippy::too_many_arguments)]
fn layer_norm_backward_into(
    x: &[f64],
    mean: f64,
    var: f64,
    gamma: &[f64],
    dy: &[f64],
    dgamma: &mut [f64],
    dbeta: &mut [f64],
    dx_out: &mut [f64],
    xhat: &mut [f64],
    dxhat: &mut [f64],
) {
    let n = x.len();
    let std = (var + LAYER_NORM_EPS).sqrt();
    for i in 0..n {
        xhat[i] = (x[i] - mean) / std;
        dxhat[i] = dy[i] * gamma[i];
        dgamma[i] += dy[i] * xhat[i];
        dbeta[i] += dy[i];
    }
    let sum_dxhat: f64 = dxhat.iter().sum();
    let sum_dxhat_xhat: f64 = dxhat.iter().zip(xhat.iter()).map(|(&a, &b)| a * b).sum();
    let nf = n as f64;
    for i in 0..n {
        dx_out[i] = (nf * dxhat[i] - sum_dxhat - xhat[i] * sum_dxhat_xhat) / (nf * std);
    }
}

/// Linear-layer backward: accumulate into `dw`/`db`, and (if `dx_out` is
/// `Some`) write the gradient wrt the input. `dx_out` is `None` for the
/// stem, whose input is DATA (an encoding), not a parameter -- skipping it
/// there saves a full `hidden * in_dim` pass with `in_dim` in the
/// thousands.
fn linear_backward_accumulate(w: &[f64], x: &[f64], dy: &[f64], in_dim: usize, dw: &mut [f64], db: &mut [f64], mut dx_out: Option<&mut [f64]>) {
    let out_dim = dy.len();
    if let Some(dx) = dx_out.as_deref_mut() {
        dx.iter_mut().for_each(|v| *v = 0.0);
    }
    for o in 0..out_dim {
        let dyo = dy[o];
        db[o] += dyo;
        let row = &w[o * in_dim..(o + 1) * in_dim];
        let dw_row = &mut dw[o * in_dim..(o + 1) * in_dim];
        for i in 0..in_dim {
            dw_row[i] += dyo * x[i];
        }
        if let Some(dx) = dx_out.as_deref_mut() {
            for i in 0..in_dim {
                dx[i] += dyo * row[i];
            }
        }
    }
}

/// Backpropagate `dy` (`d(loss)/d(net output)`) through the forward pass
/// recorded in `cache`, accumulating into `grad`. `x` must be the same
/// input `forward_train` was called with to produce `cache`.
fn backward_train(net: &ValueNet, cache: &ForwardCache, x: &[f64], dy: f64, grad: &mut ValueNetGrad, s: &mut BackScratch) {
    let hidden = net.hidden;
    let last_h = cache.last_hidden();
    for j in 0..hidden {
        grad.head_w[j] += dy * last_h[j];
        s.g_out[j] = dy * net.head_w[j];
    }
    grad.head_b += dy;

    for i in (0..cache.blocks.len()).rev() {
        let block = &net.blocks[i];
        let bc = &cache.blocks[i];
        let g = &mut grad.blocks[i];

        relu_backward_into(&bc.h_out, &s.g_out, &mut s.g_lnout);
        layer_norm_backward_into(&bc.resid, bc.ln_mean, bc.ln_var, &block.ln_gamma, &s.g_lnout, &mut g.ln_gamma, &mut g.ln_beta, &mut s.g_resid, &mut s.xhat, &mut s.dxhat);
        for j in 0..hidden {
            s.g_a2[j] = s.g_resid[j] * bc.drop_mask[j];
        }
        linear_backward_accumulate(&block.fc2_w, &bc.r1, &s.g_a2, hidden, &mut g.fc2_w, &mut g.fc2_b, Some(&mut s.g_r1));
        relu_backward_into(&bc.r1, &s.g_r1, &mut s.g_a1);
        linear_backward_accumulate(&block.fc1_w, &bc.h_in, &s.g_a1, hidden, &mut g.fc1_w, &mut g.fc1_b, Some(&mut s.g_hin_fc1));
        // The residual add splits its incoming gradient unchanged to both
        // branches: the direct `h_in` path (`s.g_resid`, already computed
        // above) and the `fc1` path just backpropagated into `g_hin_fc1`.
        for j in 0..hidden {
            s.g_out[j] = s.g_resid[j] + s.g_hin_fc1[j];
        }
    }

    relu_backward_into(&cache.stem_out, &s.g_out, &mut s.g_lnout);
    layer_norm_backward_into(&cache.stem_z, cache.stem_ln_mean, cache.stem_ln_var, &net.stem_ln_gamma, &s.g_lnout, &mut grad.stem_ln_gamma, &mut grad.stem_ln_beta, &mut s.g_resid, &mut s.xhat, &mut s.dxhat);
    linear_backward_accumulate(&net.stem_w, x, &s.g_resid, net.in_dim, &mut grad.stem_w, &mut grad.stem_b, None);
}

// ===================================================================== loss

/// `torch.nn.functional.smooth_l1_loss(pred, target, beta=1.0)` for ONE
/// element (quadratic for `|diff| < 1`, linear beyond -- Huber loss): the
/// loss `neural_train_rank.py` actually calls for the value head (its own
/// module docstring simplifies this to "MSE" in prose; the CODE is
/// smooth-L1, and the code is the spec here). Returns `(loss, d(loss)/d(pred))`.
pub(crate) fn smooth_l1(pred: f64, target: f64) -> (f64, f64) {
    let diff = pred - target;
    let a = diff.abs();
    if a < 1.0 {
        (0.5 * diff * diff, diff)
    } else {
        (a - 0.5, diff.signum())
    }
}

/// Numerically stable `softplus(x) = ln(1 + e^x)`, as `max(x,0) + ln(1 +
/// e^{-|x|})` so it never overflows for large `|x|` (the naive
/// `(1.0+x.exp()).ln()` does, for `x` past a few hundred).
pub(crate) fn softplus_stable(x: f64) -> f64 {
    x.max(0.0) + (1.0 + (-x.abs()).exp()).ln()
}

/// Stable sigmoid `1/(1+e^{-x})`, matching `softplus`'s derivative
/// (`d/dx softplus(x) = sigmoid(x)`).
fn sigmoid_stable(x: f64) -> f64 {
    if x >= 0.0 {
        1.0 / (1.0 + (-x).exp())
    } else {
        let e = x.exp();
        e / (1.0 + e)
    }
}

/// The Bradley-Terry pairwise ranking loss for one pair,
/// `softplus(-(va - vb))` -- pushes the CHOSEN sibling's value `va` above
/// the REJECTED sibling's `vb`. With `z = -(va-vb) = vb-va`,
/// `d/dz softplus(z) = sigmoid(z)`, so `d(loss)/dva = -sigmoid(z)` and
/// `d(loss)/dvb = +sigmoid(z)`. Returns `(loss, d/dva, d/dvb)`.
pub(crate) fn rank_pair_loss(va: f64, vb: f64) -> (f64, f64, f64) {
    let z = vb - va;
    let loss = softplus_stable(z);
    let s = sigmoid_stable(z);
    (loss, -s, s)
}

// ================================================================= AdamW

/// `torch.optim.AdamW` semantics: decoupled weight decay (subtracted
/// straight from the parameter, not folded into the gradient the way plain
/// `Adam(weight_decay=...)` does), applied every step regardless of the
/// gradient's sign -- `neural_train_rank.py` constructs
/// `torch.optim.AdamW(net.parameters(), lr=args.lr, weight_decay=args.wd)`,
/// so that is the optimizer this ports, not plain `Adam`.
pub struct AdamW {
    lr: f64,
    beta1: f64,
    beta2: f64,
    eps: f64,
    wd: f64,
    t: u64,
    m: ValueNetGrad,
    v: ValueNetGrad,
}

impl AdamW {
    /// `lr`/`wd` are the script's `--lr`/`--wd`; `beta1`/`beta2`/`eps` are
    /// torch's own `AdamW` defaults (`0.9`, `0.999`, `1e-8`), which the
    /// script never overrides.
    fn new(net: &ValueNet, lr: f64, wd: f64) -> Self {
        AdamW {
            lr,
            beta1: 0.9,
            beta2: 0.999,
            eps: 1e-8,
            wd,
            t: 0,
            m: ValueNetGrad::zeros_like(net),
            v: ValueNetGrad::zeros_like(net),
        }
    }

    /// One step: `param -= lr*wd*param` (decoupled decay) THEN the usual
    /// bias-corrected Adam update, matching torch's documented `AdamW`
    /// update order.
    fn step(&mut self, net: &mut ValueNet, grad: &ValueNetGrad) {
        self.t += 1;
        let (t, lr, b1, b2, eps, wd) = (self.t, self.lr, self.beta1, self.beta2, self.eps, self.wd);
        let upd = |p: &mut [f64], g: &[f64], m: &mut [f64], v: &mut [f64]| {
            adamw_update(p, g, m, v, t, lr, b1, b2, eps, wd);
        };
        upd(&mut net.stem_w, &grad.stem_w, &mut self.m.stem_w, &mut self.v.stem_w);
        upd(&mut net.stem_b, &grad.stem_b, &mut self.m.stem_b, &mut self.v.stem_b);
        upd(&mut net.stem_ln_gamma, &grad.stem_ln_gamma, &mut self.m.stem_ln_gamma, &mut self.v.stem_ln_gamma);
        upd(&mut net.stem_ln_beta, &grad.stem_ln_beta, &mut self.m.stem_ln_beta, &mut self.v.stem_ln_beta);
        for i in 0..net.blocks.len() {
            upd(&mut net.blocks[i].fc1_w, &grad.blocks[i].fc1_w, &mut self.m.blocks[i].fc1_w, &mut self.v.blocks[i].fc1_w);
            upd(&mut net.blocks[i].fc1_b, &grad.blocks[i].fc1_b, &mut self.m.blocks[i].fc1_b, &mut self.v.blocks[i].fc1_b);
            upd(&mut net.blocks[i].fc2_w, &grad.blocks[i].fc2_w, &mut self.m.blocks[i].fc2_w, &mut self.v.blocks[i].fc2_w);
            upd(&mut net.blocks[i].fc2_b, &grad.blocks[i].fc2_b, &mut self.m.blocks[i].fc2_b, &mut self.v.blocks[i].fc2_b);
            upd(&mut net.blocks[i].ln_gamma, &grad.blocks[i].ln_gamma, &mut self.m.blocks[i].ln_gamma, &mut self.v.blocks[i].ln_gamma);
            upd(&mut net.blocks[i].ln_beta, &grad.blocks[i].ln_beta, &mut self.m.blocks[i].ln_beta, &mut self.v.blocks[i].ln_beta);
        }
        upd(&mut net.head_w, &grad.head_w, &mut self.m.head_w, &mut self.v.head_w);
        let (mut hb, mut mhb, mut vhb) = ([net.head_b], [self.m.head_b], [self.v.head_b]);
        upd(&mut hb, &[grad.head_b], &mut mhb, &mut vhb);
        net.head_b = hb[0];
        self.m.head_b = mhb[0];
        self.v.head_b = vhb[0];
    }
}

#[allow(clippy::too_many_arguments)]
fn adamw_update(param: &mut [f64], grad: &[f64], m: &mut [f64], v: &mut [f64], t: u64, lr: f64, beta1: f64, beta2: f64, eps: f64, wd: f64) {
    let bias_correct1 = 1.0 - beta1.powi(t as i32);
    let bias_correct2 = 1.0 - beta2.powi(t as i32);
    for i in 0..param.len() {
        param[i] -= lr * wd * param[i];
        m[i] = beta1 * m[i] + (1.0 - beta1) * grad[i];
        v[i] = beta2 * v[i] + (1.0 - beta2) * grad[i] * grad[i];
        let mhat = m[i] / bias_correct1;
        let vhat = v[i] / bias_correct2;
        param[i] -= lr * mhat / (vhat.sqrt() + eps);
    }
}

// =================================================================== trainer

/// Owns the network, its optimizer state, and every reusable buffer the
/// hot per-row loop needs -- constructed once per run, never reallocating
/// its `Vec`s afterward (see [`ForwardCache`]/[`BackScratch`]'s own doc
/// comments).
pub struct Trainer {
    pub net: ValueNet,
    grad: ValueNetGrad,
    adamw: AdamW,
    cache_a: ForwardCache,
    cache_b: ForwardCache,
    cache_v: ForwardCache,
    scratch: BackScratch,
    /// `f64` copies of a shard row's `f32` encoding -- [`RankPair`]/
    /// [`ValueRow`] store `f32` (half the shard's footprint on disk), but
    /// every arithmetic routine in this file runs in `f64`, so each
    /// `accumulate_*` call converts into one of these THREE reusable
    /// buffers rather than allocating a fresh `Vec` per row.
    conv_a: Vec<f64>,
    conv_b: Vec<f64>,
    conv_v: Vec<f64>,
    rng: PyRandom,
    dropout_p: f64,
}

/// Write `src` (an `f32` shard row) into `dst` (a same-length `f64`
/// buffer), widening element by element -- the one place a shard row's
/// storage width and this file's arithmetic width meet.
fn widen_into(src: &[f32], dst: &mut [f64]) {
    debug_assert_eq!(src.len(), dst.len());
    for i in 0..src.len() {
        dst[i] = src[i] as f64;
    }
}

impl Trainer {
    pub fn new(net: ValueNet, dropout_p: f64, lr: f64, wd: f64, seed: i64) -> Self {
        let grad = ValueNetGrad::zeros_like(&net);
        let adamw = AdamW::new(&net, lr, wd);
        let cache_a = ForwardCache::zeros(&net);
        let cache_b = ForwardCache::zeros(&net);
        let cache_v = ForwardCache::zeros(&net);
        let scratch = BackScratch::zeros(net.hidden);
        let in_dim = net.in_dim;
        Trainer {
            net,
            grad,
            adamw,
            cache_a,
            cache_b,
            cache_v,
            scratch,
            conv_a: vec![0.0; in_dim],
            conv_b: vec![0.0; in_dim],
            conv_v: vec![0.0; in_dim],
            rng: PyRandom::new(seed),
            dropout_p,
        }
    }

    pub fn zero_grad(&mut self) {
        self.grad.zero();
    }

    pub fn optim_step(&mut self) {
        self.adamw.step(&mut self.net, &self.grad);
    }

    /// Forward+backward ONE ranking pair (`pair.chosen` vs `pair.rejected`,
    /// [`super::rankdata::RankPair`]'s own naming) with training-mode
    /// dropout, scaling its gradient contribution by `lam /
    /// n_pairs_in_batch` (so accumulating over a whole minibatch then
    /// calling [`Self::optim_step`] reproduces `loss = lam * rank.mean();
    /// loss.backward()`). Returns the pair's own (unscaled) loss value,
    /// for the reported running average.
    pub fn accumulate_pair(&mut self, pair: &RankPair, lam: f64, n_pairs_in_batch: usize) -> f64 {
        widen_into(&pair.chosen, &mut self.conv_a);
        widen_into(&pair.rejected, &mut self.conv_b);
        let va = forward_train(&self.net, &self.conv_a, self.dropout_p, Some(&mut self.rng), &mut self.cache_a);
        let vb = forward_train(&self.net, &self.conv_b, self.dropout_p, Some(&mut self.rng), &mut self.cache_b);
        let (loss, dva, dvb) = rank_pair_loss(va, vb);
        let scale = lam / n_pairs_in_batch as f64;
        backward_train(&self.net, &self.cache_a, &self.conv_a, dva * scale, &mut self.grad, &mut self.scratch);
        backward_train(&self.net, &self.cache_b, &self.conv_b, dvb * scale, &mut self.grad, &mut self.scratch);
        loss
    }

    /// Forward+backward ONE value-anchor row. `row.margin` is CULTURE
    /// units (as the shard stores it); this divides by [`MARGIN_NORM`]
    /// itself, matching `yv_t = torch.tensor(yv / MARGIN_NORM)` in the
    /// Python script. Scales the gradient by `vweight / n_rows_in_batch`;
    /// returns the row's own (unscaled) loss.
    pub fn accumulate_value(&mut self, row: &ValueRow, vweight: f64, n_rows_in_batch: usize) -> f64 {
        widen_into(&row.state, &mut self.conv_v);
        let target = row.margin as f64 / MARGIN_NORM;
        let pred = forward_train(&self.net, &self.conv_v, self.dropout_p, Some(&mut self.rng), &mut self.cache_v);
        let (loss, dpred) = smooth_l1(pred, target);
        let scale = vweight / n_rows_in_batch as f64;
        backward_train(&self.net, &self.cache_v, &self.conv_v, dpred * scale, &mut self.grad, &mut self.scratch);
        loss
    }

    /// Draw a `[0, n)` index, used by the training binary to build a
    /// shuffled row order without a second RNG in the process (see
    /// `rng.rs`'s own note on exactly that, on `PyRandom::random`).
    pub fn rand_index(&mut self, n: usize) -> usize {
        ((self.rng.random()) * n as f64) as usize
    }
}

/// Eval-mode forward pass (dropout off, no gradient bookkeeping) -- what
/// the training binary's `evaluate_val`-equivalent calls. A thin wrapper
/// over [`ValueNet::forward`] rather than a second implementation, so
/// "eval is unchanged" is true by construction, not by coincidence.
pub fn predict(net: &ValueNet, x: &[f64]) -> f64 {
    net.forward(x)
}

// ===================================================================== tests

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::net::ResBlock;

    // ---------------------------------------------------------- fixtures

    /// A deterministic, non-constant "random-looking" fill -- NOT a real
    /// RNG (no dependency; this is test-only anyway). Constant or all-zero
    /// weights would hide bugs (every ReLU mask identical, every LayerNorm
    /// variance zero), so this exists purely to give gradient checks a
    /// generic point with a real mix of signs and magnitudes.
    fn wobble(seed: u64, n: usize) -> Vec<f64> {
        (0..n)
            .map(|i| {
                let h = seed.wrapping_mul(1_000_003).wrapping_add(i as u64 * 7919 + 11);
                ((h % 2000) as f64 - 1000.0) / 1000.0 * 0.6
            })
            .collect()
    }

    fn tiny_net(in_dim: usize, hidden: usize, n_blocks: usize) -> ValueNet {
        let mut seed = 1u64;
        let mut next = |n: usize| {
            seed = seed.wrapping_add(97);
            wobble(seed, n)
        };
        let gamma_away_from_zero = |mut v: Vec<f64>| {
            v.iter_mut().for_each(|g| *g += 1.2);
            v
        };
        let stem_ln_gamma = gamma_away_from_zero(next(hidden));
        let stem_w = next(hidden * in_dim);
        let stem_b = next(hidden);
        let stem_ln_beta = next(hidden);
        let blocks = (0..n_blocks)
            .map(|_| ResBlock {
                fc1_w: next(hidden * hidden),
                fc1_b: next(hidden),
                fc2_w: next(hidden * hidden),
                fc2_b: next(hidden),
                ln_gamma: gamma_away_from_zero(next(hidden)),
                ln_beta: next(hidden),
            })
            .collect();
        let head_w = next(hidden);
        ValueNet { in_dim, hidden, stem_w, stem_b, stem_ln_gamma, stem_ln_beta, blocks, head_w, head_b: 0.37 }
    }

    fn tiny_input(seed: u64, n: usize) -> Vec<f64> {
        wobble(seed, n)
    }

    // --------------------------------------------------- loss/net helpers

    fn rank_loss_for(net: &ValueNet, xa: &[f64], xb: &[f64], dropout_p: f64, seed: i64) -> f64 {
        let mut cache_a = ForwardCache::zeros(net);
        let mut cache_b = ForwardCache::zeros(net);
        let mut rng_a = PyRandom::new(seed);
        let mut rng_b = PyRandom::new(seed + 1);
        let va = forward_train(net, xa, dropout_p, Some(&mut rng_a), &mut cache_a);
        let vb = forward_train(net, xb, dropout_p, Some(&mut rng_b), &mut cache_b);
        rank_pair_loss(va, vb).0
    }

    fn value_loss_for(net: &ValueNet, xv: &[f64], target: f64, dropout_p: f64, seed: i64) -> f64 {
        let mut cache = ForwardCache::zeros(net);
        let mut rng = PyRandom::new(seed);
        let pred = forward_train(net, xv, dropout_p, Some(&mut rng), &mut cache);
        smooth_l1(pred, target).0
    }

    /// Central finite difference of `f` at `net`'s field selected by
    /// `get`/`set`, compared against `analytic` (the value backprop
    /// computed). `get`/`set` are transient per-call borrows of `net`, not
    /// held across the two perturbed evaluations, so there is no aliasing
    /// conflict with `f` also borrowing `net`.
    fn assert_matches_finite_diff(
        net: &mut ValueNet,
        get: impl Fn(&ValueNet) -> f64,
        set: impl Fn(&mut ValueNet, f64),
        analytic: f64,
        mut f: impl FnMut(&ValueNet) -> f64,
        label: &str,
    ) {
        let h = 1e-6;
        let orig = get(net);
        set(net, orig + h);
        let lp = f(net);
        set(net, orig - h);
        let lm = f(net);
        set(net, orig);
        let numeric = (lp - lm) / (2.0 * h);
        let tol = (1e-4_f64).max(1e-4 * analytic.abs());
        assert!((numeric - analytic).abs() < tol, "{label}: analytic={analytic} numeric={numeric}");
    }

    /// Walk every parameter TYPE in `net` (stem linear+LayerNorm, each
    /// block's two linears and LayerNorm, the head) and check it against
    /// finite differences of `loss_fn`, using the matching entry of `grad`.
    /// The single place both the rank-loss test and the value-loss test
    /// below call into, so "every parameter type" is enforced once, not
    /// copy-pasted per loss.
    fn check_every_parameter_type(net: &mut ValueNet, grad: &ValueNetGrad, mut loss_fn: impl FnMut(&ValueNet) -> f64) {
        for i in 0..net.stem_w.len() {
            assert_matches_finite_diff(net, |n| n.stem_w[i], |n, v| n.stem_w[i] = v, grad.stem_w[i], &mut loss_fn, &format!("stem_w[{i}]"));
        }
        for i in 0..net.stem_b.len() {
            assert_matches_finite_diff(net, |n| n.stem_b[i], |n, v| n.stem_b[i] = v, grad.stem_b[i], &mut loss_fn, &format!("stem_b[{i}]"));
        }
        for i in 0..net.stem_ln_gamma.len() {
            assert_matches_finite_diff(net, |n| n.stem_ln_gamma[i], |n, v| n.stem_ln_gamma[i] = v, grad.stem_ln_gamma[i], &mut loss_fn, &format!("stem_ln_gamma[{i}]"));
        }
        for i in 0..net.stem_ln_beta.len() {
            assert_matches_finite_diff(net, |n| n.stem_ln_beta[i], |n, v| n.stem_ln_beta[i] = v, grad.stem_ln_beta[i], &mut loss_fn, &format!("stem_ln_beta[{i}]"));
        }
        for bi in 0..net.blocks.len() {
            for i in 0..net.blocks[bi].fc1_w.len() {
                assert_matches_finite_diff(net, |n| n.blocks[bi].fc1_w[i], |n, v| n.blocks[bi].fc1_w[i] = v, grad.blocks[bi].fc1_w[i], &mut loss_fn, &format!("blocks[{bi}].fc1_w[{i}]"));
            }
            for i in 0..net.blocks[bi].fc1_b.len() {
                assert_matches_finite_diff(net, |n| n.blocks[bi].fc1_b[i], |n, v| n.blocks[bi].fc1_b[i] = v, grad.blocks[bi].fc1_b[i], &mut loss_fn, &format!("blocks[{bi}].fc1_b[{i}]"));
            }
            for i in 0..net.blocks[bi].fc2_w.len() {
                assert_matches_finite_diff(net, |n| n.blocks[bi].fc2_w[i], |n, v| n.blocks[bi].fc2_w[i] = v, grad.blocks[bi].fc2_w[i], &mut loss_fn, &format!("blocks[{bi}].fc2_w[{i}]"));
            }
            for i in 0..net.blocks[bi].fc2_b.len() {
                assert_matches_finite_diff(net, |n| n.blocks[bi].fc2_b[i], |n, v| n.blocks[bi].fc2_b[i] = v, grad.blocks[bi].fc2_b[i], &mut loss_fn, &format!("blocks[{bi}].fc2_b[{i}]"));
            }
            for i in 0..net.blocks[bi].ln_gamma.len() {
                assert_matches_finite_diff(net, |n| n.blocks[bi].ln_gamma[i], |n, v| n.blocks[bi].ln_gamma[i] = v, grad.blocks[bi].ln_gamma[i], &mut loss_fn, &format!("blocks[{bi}].ln_gamma[{i}]"));
            }
            for i in 0..net.blocks[bi].ln_beta.len() {
                assert_matches_finite_diff(net, |n| n.blocks[bi].ln_beta[i], |n, v| n.blocks[bi].ln_beta[i] = v, grad.blocks[bi].ln_beta[i], &mut loss_fn, &format!("blocks[{bi}].ln_beta[{i}]"));
            }
        }
        for i in 0..net.head_w.len() {
            assert_matches_finite_diff(net, |n| n.head_w[i], |n, v| n.head_w[i] = v, grad.head_w[i], &mut loss_fn, &format!("head_w[{i}]"));
        }
        assert_matches_finite_diff(net, |n| n.head_b, |n, v| n.head_b = v, grad.head_b, &mut loss_fn, "head_b");
    }

    // ------------------------------------------------- gradient-check tests

    /// The main correctness test: EVERY parameter type (stem linear, stem
    /// LayerNorm, each of 2 blocks' two linears and LayerNorm, and the
    /// head) checked against finite differences, under the RANKING loss,
    /// WITH dropout active -- the training-mode configuration the real
    /// optimizer loop runs in. A gradient check that only covered the last
    /// layer, or only ran in eval mode, would pass for the wrong reason.
    #[test]
    fn the_analytic_gradient_of_every_parameter_type_matches_finite_differences_under_rank_loss_with_dropout() {
        let in_dim = 3;
        let hidden = 4;
        let n_blocks = 2;
        let dropout_p = 0.3;
        let seed = 4242i64;
        let mut net = tiny_net(in_dim, hidden, n_blocks);
        let xa = tiny_input(11, in_dim);
        let xb = tiny_input(22, in_dim);

        // Analytic gradient: one forward/backward pass through the rank loss.
        let mut cache_a = ForwardCache::zeros(&net);
        let mut cache_b = ForwardCache::zeros(&net);
        let mut scratch = BackScratch::zeros(hidden);
        let mut grad = ValueNetGrad::zeros_like(&net);
        let mut rng_a = PyRandom::new(seed);
        let mut rng_b = PyRandom::new(seed + 1);
        let va = forward_train(&net, &xa, dropout_p, Some(&mut rng_a), &mut cache_a);
        let vb = forward_train(&net, &xb, dropout_p, Some(&mut rng_b), &mut cache_b);
        let (_, dva, dvb) = rank_pair_loss(va, vb);
        backward_train(&net, &cache_a, &xa, dva, &mut grad, &mut scratch);
        backward_train(&net, &cache_b, &xb, dvb, &mut grad, &mut scratch);

        check_every_parameter_type(&mut net, &grad, |n| rank_loss_for(n, &xa, &xb, dropout_p, seed));
    }

    /// The same sweep, but WITHOUT dropout (eval-shaped forward inside a
    /// still-training backward pass, `dropout_p = 0.0`) and under the VALUE
    /// (smooth-L1) loss instead of the ranking loss -- an independent
    /// second path through `backward_train` (one forward, not two) and
    /// through a different loss's gradient.
    #[test]
    fn the_analytic_gradient_of_every_parameter_type_matches_finite_differences_under_value_loss_no_dropout() {
        let in_dim = 3;
        let hidden = 4;
        let n_blocks = 2;
        let dropout_p = 0.0;
        let seed = 777i64;
        let mut net = tiny_net(in_dim, hidden, n_blocks);
        let xv = tiny_input(33, in_dim);
        let target = 0.42;

        let mut cache = ForwardCache::zeros(&net);
        let mut scratch = BackScratch::zeros(hidden);
        let mut grad = ValueNetGrad::zeros_like(&net);
        let mut rng = PyRandom::new(seed);
        let pred = forward_train(&net, &xv, dropout_p, Some(&mut rng), &mut cache);
        let (_, dpred) = smooth_l1(pred, target);
        backward_train(&net, &cache, &xv, dpred, &mut grad, &mut scratch);

        check_every_parameter_type(&mut net, &grad, |n| value_loss_for(n, &xv, target, dropout_p, seed));
    }

    /// `blocks: vec![]` (no residual blocks at all) is the degenerate shape
    /// `net.rs`'s own hand-computed test also exercises for the forward
    /// pass; the backward pass has a different code path for it (the stem
    /// receives the head's gradient directly, skipping the block loop
    /// entirely), so it needs its own check rather than trusting the
    /// `n_blocks=2` case to cover it.
    #[test]
    fn the_analytic_gradient_matches_finite_differences_with_zero_residual_blocks() {
        let in_dim = 2;
        let hidden = 3;
        let dropout_p = 0.0;
        let seed = 55i64;
        let mut net = tiny_net(in_dim, hidden, 0);
        let xa = tiny_input(1, in_dim);
        let xb = tiny_input(2, in_dim);

        let mut cache_a = ForwardCache::zeros(&net);
        let mut cache_b = ForwardCache::zeros(&net);
        let mut scratch = BackScratch::zeros(hidden);
        let mut grad = ValueNetGrad::zeros_like(&net);
        let mut rng_a = PyRandom::new(seed);
        let mut rng_b = PyRandom::new(seed + 1);
        let va = forward_train(&net, &xa, dropout_p, Some(&mut rng_a), &mut cache_a);
        let vb = forward_train(&net, &xb, dropout_p, Some(&mut rng_b), &mut cache_b);
        let (_, dva, dvb) = rank_pair_loss(va, vb);
        backward_train(&net, &cache_a, &xa, dva, &mut grad, &mut scratch);
        backward_train(&net, &cache_b, &xb, dvb, &mut grad, &mut scratch);

        check_every_parameter_type(&mut net, &grad, |n| rank_loss_for(n, &xa, &xb, dropout_p, seed));
    }

    // -------------------------------------------------------- loss-fn tests

    /// `d/dx softplus(x) = sigmoid(x)`, checked against finite differences
    /// at moderate `x` (where the naive numeric derivative itself is
    /// well-conditioned), plus a direct finiteness check at extreme `x`
    /// where `softplus_stable`'s whole reason to exist -- avoiding
    /// `(1.0 + x.exp()).ln()` overflowing to `inf` -- would otherwise show
    /// up as a silent `NaN` gradient far from the origin a real training
    /// run's logits can actually reach.
    #[test]
    fn softplus_stable_derivative_matches_finite_differences_and_stays_finite_at_extremes() {
        for &x in &[-5.0, -1.0, -0.01, 0.0, 0.01, 1.0, 5.0] {
            let h = 1e-6;
            let numeric = (softplus_stable(x + h) - softplus_stable(x - h)) / (2.0 * h);
            let analytic = sigmoid_stable(x);
            assert!((numeric - analytic).abs() < 1e-4, "x={x}: numeric={numeric} analytic={analytic}");
        }
        for &x in &[-1e6, -1e3, 1e3, 1e6] {
            assert!(softplus_stable(x).is_finite(), "softplus_stable({x}) was not finite");
            assert!(sigmoid_stable(x).is_finite(), "sigmoid_stable({x}) was not finite");
        }
        // At large positive x, softplus(x) ~ x (the linear part dominates).
        assert!((softplus_stable(1e6) - 1e6).abs() < 1.0);
        // At large negative x, softplus(x) ~ 0.
        assert!(softplus_stable(-1e6) < 1e-9);
    }

    /// `smooth_l1`'s derivative, on both sides of the Huber `|diff| = 1`
    /// boundary (quadratic just inside it, linear just outside), plus the
    /// exact boundary point where the two branches must agree (both give
    /// slope magnitude 1) -- the one place a Huber implementation is most
    /// likely to have an off-by-a-branch bug.
    #[test]
    fn smooth_l1_derivative_matches_finite_differences_on_both_sides_of_the_huber_boundary() {
        for &(pred, target) in &[(0.0, 0.001), (0.3, 0.0), (2.0, 0.0), (-3.0, 1.0), (1.0, 0.0), (-1.0, 0.0)] {
            let h = 1e-6;
            let numeric = (smooth_l1(pred + h, target).0 - smooth_l1(pred - h, target).0) / (2.0 * h);
            let analytic = smooth_l1(pred, target).1;
            assert!((numeric - analytic).abs() < 1e-3, "pred={pred} target={target}: numeric={numeric} analytic={analytic}");
        }
    }

    // ------------------------------------------------------ behaviour tests

    /// `net.rs`'s own module doc comment promises the eval-mode forward
    /// pass is unchanged by this file's existence. `forward_train` with
    /// `rng: None` (so every `drop_mask` entry is `1.0`, dropout's eval
    /// identity) must therefore reproduce `ValueNet::forward` EXACTLY, not
    /// approximately -- same arithmetic, same order, so nothing to round.
    #[test]
    fn forward_train_at_eval_matches_value_net_forward_exactly() {
        let net = tiny_net(5, 4, 2);
        let x = tiny_input(9, 5);
        let mut cache = ForwardCache::zeros(&net);
        let via_forward_train = forward_train(&net, &x, 0.5 /* ignored when rng is None */, None, &mut cache);
        let via_net_forward = net.forward(&x);
        assert_eq!(via_forward_train, via_net_forward);
    }

    /// Dropout must be the identity (`drop_mask` all `1.0`) with `rng:
    /// None`, and must actually zero SOME units (not all, not none) over
    /// enough draws with `rng: Some` at a mid-range `p` -- the two halves
    /// of "dropout is active in training and inert in eval" this module's
    /// doc comment claims.
    #[test]
    fn dropout_is_the_identity_in_eval_and_actually_drops_units_in_training() {
        let net = tiny_net(4, 8, 1);
        let x = tiny_input(3, 4);
        let mut cache = ForwardCache::zeros(&net);

        forward_train(&net, &x, 0.5, None, &mut cache);
        assert!(cache.blocks[0].drop_mask.iter().all(|&m| m == 1.0), "eval-mode drop_mask must be all 1.0");

        let mut rng = PyRandom::new(123);
        let mut zeros = 0;
        let mut kept = 0;
        let trials = 400;
        for _ in 0..trials {
            forward_train(&net, &x, 0.5, Some(&mut rng), &mut cache);
            for &m in &cache.blocks[0].drop_mask {
                if m == 0.0 {
                    zeros += 1;
                } else {
                    assert!((m - 2.0).abs() < 1e-12, "kept unit should be scaled by 1/(1-p)=2.0, got {m}");
                    kept += 1;
                }
            }
        }
        assert!(zeros > 0, "p=0.5 over {trials} trials dropped nothing");
        assert!(kept > 0, "p=0.5 over {trials} trials kept nothing");
        // Loose sanity band around the expected 50/50 split -- not a tight
        // statistical test, just a check that the mask isn't degenerate.
        let frac_dropped = zeros as f64 / (zeros + kept) as f64;
        assert!((0.3..0.7).contains(&frac_dropped), "fraction dropped {frac_dropped} far from p=0.5");
    }

    // ------------------------------------------------------- training tests

    /// Trains a tiny net purely on the VALUE loss against a fixed table of
    /// (input, target) rows and checks the loss actually drops -- the
    /// end-to-end sanity check that `Trainer`'s forward/backward/AdamW
    /// wiring, not just each gradient in isolation, adds up to something
    /// that learns.
    #[test]
    fn training_on_a_trivial_value_regression_drives_the_loss_down() {
        let in_dim = 3;
        let net = tiny_net(in_dim, 6, 1);
        let mut trainer = Trainer::new(net, 0.0, 5e-3, 0.0, 999);

        // y = x0 + 2*x1 - x2, a simple linear target the net can fit.
        // `margin` is stored in CULTURE units (MARGIN_NORM'd back inside
        // `accumulate_value`, matching real shard rows), so the target is
        // scaled up here and predictions are scaled back down for the loss
        // check below.
        let rows: Vec<ValueRow> = (0..24)
            .map(|i| {
                let x = tiny_input(1000 + i, in_dim);
                let y = x[0] + 2.0 * x[1] - x[2];
                ValueRow { state: x.iter().map(|&v| v as f32).collect(), margin: (y * MARGIN_NORM) as f32 }
            })
            .collect();

        let mean_loss = |trainer: &Trainer| -> f64 {
            rows.iter()
                .map(|row| {
                    let x: Vec<f64> = row.state.iter().map(|&v| v as f64).collect();
                    let target = row.margin as f64 / MARGIN_NORM;
                    smooth_l1(predict(&trainer.net, &x), target).0
                })
                .sum::<f64>()
                / rows.len() as f64
        };
        let initial = mean_loss(&trainer);

        for _epoch in 0..200 {
            trainer.zero_grad();
            for row in &rows {
                trainer.accumulate_value(row, 1.0, rows.len());
            }
            trainer.optim_step();
        }
        let final_loss = mean_loss(&trainer);
        assert!(final_loss < initial * 0.2, "loss did not drop: initial={initial} final={final_loss}");
    }

    /// Trains a tiny net purely on the RANKING loss for ONE fixed pair and
    /// checks the chosen sibling ends up scored above the rejected one --
    /// the property the whole Bradley-Terry objective exists to produce,
    /// checked end to end through `Trainer` rather than by inspecting
    /// gradients.
    #[test]
    fn training_on_the_ranking_loss_alone_orders_the_pair_correctly() {
        let in_dim = 4;
        let net = tiny_net(in_dim, 6, 1);
        let xa = tiny_input(501, in_dim); // chosen
        let xb = tiny_input(502, in_dim); // rejected
        let pair = RankPair { chosen: xa.iter().map(|&v| v as f32).collect(), rejected: xb.iter().map(|&v| v as f32).collect() };
        let mut trainer = Trainer::new(net, 0.0, 5e-3, 0.0, 314);

        let before_gap = predict(&trainer.net, &xa) - predict(&trainer.net, &xb);
        for _step in 0..300 {
            trainer.zero_grad();
            trainer.accumulate_pair(&pair, 1.0, 1);
            trainer.optim_step();
        }
        let after_gap = predict(&trainer.net, &xa) - predict(&trainer.net, &xb);
        assert!(after_gap > 0.0, "chosen did not end up above rejected: gap={after_gap}");
        assert!(after_gap > before_gap, "gap did not widen: before={before_gap} after={after_gap}");
    }
}
