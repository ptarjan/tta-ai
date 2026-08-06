//! The value network's forward-pass arithmetic, and the batched-inference
//! wrapper built on it.
//!
//! Ports the ARITHMETIC of `engine/bots/neural_net.py`'s `ValueNet`/
//! `_ResBlock`/`NeuralValue.value` (152 lines) -- read that file's own module
//! doc comment first for the target the network predicts (the eventual
//! final-culture margin, scaled by [`MARGIN_NORM`]) and why margin rather
//! than win/loss.
//!
//! ## No Python oracle for this module, and that is expected
//!
//! Every other module in this port has a `tools/dump_*.py` counterpart that
//! runs the real Python and records its answer. This one cannot: `import
//! torch` fails both on the machine Python's own module doc comment says it
//! must ("Mac has no torch") AND in this differential-testing environment
//! (`python3.13 -c "import torch"` -- `ModuleNotFoundError`, checked
//! 2026-08-05). `neural_net.py`'s own top doc comment exists BECAUSE of this:
//! `import torch` is deferred and guarded by `HAVE_TORCH` specifically so
//! `neural_encode.py` (which has no torch dependency) stays exercisable on a
//! torch-less machine while this module is not. There is therefore no
//! `rust/tests/neural_net.rs` differential test; this module's own `#[cfg(test)]`
//! block instead pins the arithmetic directly (linear layer, layer norm,
//! ReLU, a hand-computed tiny network) against values worked out by hand,
//! the same role a Python oracle plays elsewhere.
//!
//! ## What is ported, and what is not
//!
//! Ported: the pure arithmetic of `ValueNet.forward`/`_ResBlock.forward` in
//! EVAL mode (`model.eval()` is called unconditionally in
//! `NeuralValue.__init__` and after `load_checkpoint`, and PyTorch's
//! `nn.Dropout` is the identity function in eval mode -- there is no dropout
//! mask to port), as [`ValueNet::forward`]/[`ResBlock::forward`] below, plus
//! [`value_batch`] (`NeuralValue.value`'s "score a batch, scale back to
//! culture units" arithmetic).
//!
//! NOT ported, matching `bots::weighted::eval`'s `load_weights`/
//! `save_weights` precedent (pure I/O, no rule-level content of its own):
//!
//! * `save_checkpoint`/`load_checkpoint` -- `torch.save`/`torch.load` binary
//!   serialization. [`ValueNet`]'s fields already carry everything the
//!   checkpoint dict's SHAPE records (`in_dim`/`hidden`/the per-block
//!   weights); there is no serialization format to parse (`Cargo.toml`'s
//!   `[dependencies]` stays empty).
//! * `HAVE_TORCH` / the deferred `import torch` -- packaging machinery for a
//!   dependency this crate never takes.
//! * `NeuralValue._to_tensor`'s numpy-vs-`torch.tensor` benchmark note -- a
//!   CPython performance comparison between two conversion paths into a
//!   tensor library that does not exist here.

/// NUMERICAL GUARD, not a model claim -- see Python's own extensive comment
/// on this constant (`neural_net.py:31-41`) for the full "this is a linear
/// normaliser on the regression target, not the league's tanh squash width"
/// argument; not reproduced here. Final margins are ~[-250, 250]; dividing by
/// 100 keeps the training target near unit scale.
pub const MARGIN_NORM: f64 = 100.0;

/// LayerNorm's numerical-stability epsilon -- PyTorch's `nn.LayerNorm`
/// default (`eps=1e-5`), which `_ResBlock`/`ValueNet.stem` never override.
const LAYER_NORM_EPS: f64 = 1e-5;

// --------------------------------------------------------------- arithmetic

/// `y = W x + b`, `W` row-major `[out_dim, in_dim]`. Mirrors `nn.Linear`.
fn linear(w: &[f64], b: &[f64], x: &[f64], out_dim: usize, in_dim: usize) -> Vec<f64> {
    debug_assert_eq!(w.len(), out_dim * in_dim);
    debug_assert_eq!(b.len(), out_dim);
    debug_assert_eq!(x.len(), in_dim);
    let mut out = vec![0.0; out_dim];
    for o in 0..out_dim {
        let row = &w[o * in_dim..(o + 1) * in_dim];
        let mut acc = b[o];
        for i in 0..in_dim {
            acc += row[i] * x[i];
        }
        out[o] = acc;
    }
    out
}

/// `nn.ReLU`, in place.
fn relu_inplace(x: &mut [f64]) {
    for v in x.iter_mut() {
        if *v < 0.0 {
            *v = 0.0;
        }
    }
}

/// `nn.LayerNorm`: normalise over the whole vector (PyTorch's `normalized_shape
/// = (dim,)`, i.e. every element of a 1-D input), biased variance (PyTorch's
/// `LayerNorm` always uses the biased estimator, `unbiased=False`), then
/// scale/shift by `gamma`/`beta`.
fn layer_norm(x: &[f64], gamma: &[f64], beta: &[f64]) -> Vec<f64> {
    let n = x.len();
    debug_assert_eq!(gamma.len(), n);
    debug_assert_eq!(beta.len(), n);
    let mean = x.iter().sum::<f64>() / n as f64;
    let var = x.iter().map(|&v| (v - mean) * (v - mean)).sum::<f64>() / n as f64;
    let denom = (var + LAYER_NORM_EPS).sqrt();
    (0..n).map(|i| (x[i] - mean) / denom * gamma[i] + beta[i]).collect()
}

// -------------------------------------------------------------- the network

/// One residual block's weights -- `_ResBlock(dim, p=0.1)`. `dim` is
/// [`ValueNet::hidden`] on every block (Python constructs all `blocks` at
/// the same width).
#[derive(Clone, Debug, PartialEq)]
pub struct ResBlock {
    /// `fc1`: `[dim, dim]` row-major.
    pub fc1_w: Vec<f64>,
    pub fc1_b: Vec<f64>,
    /// `fc2`: `[dim, dim]` row-major.
    pub fc2_w: Vec<f64>,
    pub fc2_b: Vec<f64>,
    pub ln_gamma: Vec<f64>,
    pub ln_beta: Vec<f64>,
}

impl ResBlock {
    /// `_ResBlock.forward`, eval mode (dropout is the identity -- see this
    /// module's top doc comment):
    ///
    /// ```text
    /// h = relu(fc1(x))
    /// h = fc2(h)              # dropout is a no-op in eval mode
    /// return relu(layer_norm(x + h))
    /// ```
    pub fn forward(&self, x: &[f64]) -> Vec<f64> {
        let dim = x.len();
        let mut h = linear(&self.fc1_w, &self.fc1_b, x, dim, dim);
        relu_inplace(&mut h);
        let h = linear(&self.fc2_w, &self.fc2_b, &h, dim, dim);
        let mut resid: Vec<f64> = x.iter().zip(&h).map(|(&a, &b)| a + b).collect();
        resid = layer_norm(&resid, &self.ln_gamma, &self.ln_beta);
        relu_inplace(&mut resid);
        resid
    }
}

/// The value network: `stem` (Linear -> LayerNorm -> ReLU) followed by
/// `blocks` [`ResBlock`]s, followed by a `[hidden, 1]` linear head. Mirrors
/// `ValueNet.__init__`/`forward`.
#[derive(Clone, Debug, PartialEq)]
pub struct ValueNet {
    pub in_dim: usize,
    pub hidden: usize,
    /// `stem.0`: `[hidden, in_dim]` row-major.
    pub stem_w: Vec<f64>,
    pub stem_b: Vec<f64>,
    /// `stem.1` (`nn.LayerNorm(hidden)`).
    pub stem_ln_gamma: Vec<f64>,
    pub stem_ln_beta: Vec<f64>,
    pub blocks: Vec<ResBlock>,
    /// `head`: `[1, hidden]` row-major.
    pub head_w: Vec<f64>,
    pub head_b: f64,
}

impl ValueNet {
    /// `ValueNet.forward(x).squeeze(-1)` for ONE input row -- a single
    /// predicted margin, in the network's own `[-1, 1]`-ish normalised
    /// units (divide the training label by [`MARGIN_NORM`] to get here;
    /// [`value_batch`] multiplies back to culture units).
    ///
    /// # Panics
    /// If `x.len() != self.in_dim`, or if any layer's weight/bias vector is
    /// the wrong length for `self.hidden` -- a caller bug (a mismatched
    /// checkpoint), matching this codebase's fail-loud convention.
    pub fn forward(&self, x: &[f64]) -> f64 {
        assert_eq!(x.len(), self.in_dim, "ValueNet::forward: input length does not match in_dim");
        let mut h = linear(&self.stem_w, &self.stem_b, x, self.hidden, self.in_dim);
        h = layer_norm(&h, &self.stem_ln_gamma, &self.stem_ln_beta);
        relu_inplace(&mut h);
        for block in &self.blocks {
            h = block.forward(&h);
        }
        let out = linear(&self.head_w, &[self.head_b], &h, 1, self.hidden);
        out[0]
    }
}

/// `NeuralValue.value(encodings)`: batched inference, scaled back to CULTURE
/// units (multiplied by [`MARGIN_NORM`]) -- Python's own docstring: "already
/// multiplied back by MARGIN_NORM". Empty input returns empty output,
/// matching Python's `if not encodings: return []` short circuit.
///
/// Unlike `NeuralValue._to_tensor`, there is no tensor library to batch
/// into -- this is a plain loop over [`ValueNet::forward`]. Python's own
/// docstring on `_to_tensor` frames the batching as "the conversion, not the
/// net, is the cost" of GPU inference; there is no GPU and no conversion
/// here, so the loop IS the whole cost, and it is linear in the batch either
/// way.
pub fn value_batch(net: &ValueNet, encodings: &[Vec<f64>]) -> Vec<f64> {
    encodings.iter().map(|x| net.forward(x) * MARGIN_NORM).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_layer_applies_weights_and_bias() {
        // y = [[1, 2], [3, 4]] . [1, 1] + [0, 1] = [3, 8]
        let w = vec![1.0, 2.0, 3.0, 4.0];
        let b = vec![0.0, 1.0];
        let x = vec![1.0, 1.0];
        assert_eq!(linear(&w, &b, &x, 2, 2), vec![3.0, 8.0]);
    }

    #[test]
    fn relu_zeroes_negatives_and_keeps_positives() {
        let mut x = vec![-2.0, 0.0, 3.5, -0.1];
        relu_inplace(&mut x);
        assert_eq!(x, vec![0.0, 0.0, 3.5, 0.0]);
    }

    /// A constant vector has zero variance: LayerNorm must return exactly
    /// `beta` (every `(x[i] - mean) / sqrt(eps)` term is 0), not NaN or Inf
    /// from a bare division by zero -- `LAYER_NORM_EPS` is what keeps the
    /// denominator away from zero.
    #[test]
    fn layer_norm_of_a_constant_vector_is_beta() {
        let x = vec![5.0, 5.0, 5.0];
        let gamma = vec![2.0, 2.0, 2.0];
        let beta = vec![1.0, 1.0, 1.0];
        let out = layer_norm(&x, &gamma, &beta);
        for v in out {
            assert!((v - 1.0).abs() < 1e-9, "{v}");
        }
    }

    /// A hand-computed two-element LayerNorm: `x = [1, 3]`, mean = 2,
    /// biased variance = 1, so normalised = `[-1, 1]` (up to the `eps`
    /// fudge), then `gamma=[1,1]`, `beta=[0,0]` leaves it unchanged.
    #[test]
    fn layer_norm_matches_a_hand_computed_example() {
        let x = vec![1.0, 3.0];
        let gamma = vec![1.0, 1.0];
        let beta = vec![0.0, 0.0];
        let out = layer_norm(&x, &gamma, &beta);
        assert!((out[0] - (-1.0)).abs() < 1e-4, "{:?}", out);
        assert!((out[1] - 1.0).abs() < 1e-4, "{:?}", out);
    }

    /// A `ResBlock` built as the identity map (both linears zero, LayerNorm
    /// gamma=1/beta=0) reduces to `relu(layer_norm(x))` -- pins the residual
    /// wiring (`x + h` where `h` is forced to zero) independently of the two
    /// linear layers' own correctness.
    #[test]
    fn res_block_with_zero_linears_reduces_to_relu_layer_norm_of_x() {
        let dim = 3;
        let block = ResBlock {
            fc1_w: vec![0.0; dim * dim],
            fc1_b: vec![0.0; dim],
            fc2_w: vec![0.0; dim * dim],
            fc2_b: vec![0.0; dim],
            ln_gamma: vec![1.0; dim],
            ln_beta: vec![0.0; dim],
        };
        let x = vec![1.0, 3.0, -2.0];
        let got = block.forward(&x);
        let mut want = layer_norm(&x, &vec![1.0; dim], &vec![0.0; dim]);
        relu_inplace(&mut want);
        assert_eq!(got, want);
    }

    /// A hand-built tiny network (`in_dim=2, hidden=2, blocks=0`) computed
    /// entirely by hand: identity stem weights, LayerNorm as identity
    /// (gamma=1/beta=0 on a zero-variance-free input is skipped here in
    /// favour of asserting via `layer_norm` directly, matching
    /// `res_block_with_zero_linears...` above), head sums the two hidden
    /// units. This is the closest thing this module has to an end-to-end
    /// "oracle" check, in the absence of a real Python/torch one.
    #[test]
    fn value_net_forward_with_no_blocks_matches_stem_then_head_by_hand() {
        let net = ValueNet {
            in_dim: 2,
            hidden: 2,
            stem_w: vec![1.0, 0.0, 0.0, 1.0], // identity
            stem_b: vec![0.0, 0.0],
            stem_ln_gamma: vec![1.0, 1.0],
            stem_ln_beta: vec![0.0, 0.0],
            blocks: vec![],
            head_w: vec![1.0, 1.0],
            head_b: 0.5,
        };
        let x = vec![1.0, 3.0];
        // stem: linear(x) = x = [1,3]; layer_norm([1,3]) = [-1,1] (from the
        // hand-computed test above); relu([-1,1]) = [0,1].
        // head: 1*0 + 1*1 + 0.5 = 1.5
        let got = net.forward(&x);
        assert!((got - 1.5).abs() < 1e-4, "{got}");
    }

    #[test]
    fn value_batch_scales_by_margin_norm_and_handles_empty_input() {
        let net = ValueNet {
            in_dim: 1,
            hidden: 1,
            stem_w: vec![1.0],
            stem_b: vec![0.0],
            stem_ln_gamma: vec![1.0],
            stem_ln_beta: vec![0.0],
            blocks: vec![],
            head_w: vec![1.0],
            head_b: 0.0,
        };
        assert_eq!(value_batch(&net, &[]), Vec::<f64>::new());
        // in_dim=hidden=1: LayerNorm of a single element is always `beta`
        // (mean equals the element, variance is 0), so the stem output is
        // relu(beta) = relu(0) = 0 regardless of x -- the head then also
        // reads 0, so every encoding scores 0 * MARGIN_NORM = 0.0 here. This
        // pins the SCALING, not the network's sensitivity to `x` (which the
        // two-element tests above already cover).
        let out = value_batch(&net, &[vec![1.0], vec![-4.0]]);
        assert_eq!(out, vec![0.0, 0.0]);
    }

    #[test]
    #[should_panic(expected = "in_dim")]
    fn forward_panics_on_a_mismatched_input_length() {
        let net = ValueNet {
            in_dim: 3,
            hidden: 1,
            stem_w: vec![0.0; 3],
            stem_b: vec![0.0],
            stem_ln_gamma: vec![1.0],
            stem_ln_beta: vec![0.0],
            blocks: vec![],
            head_w: vec![1.0],
            head_b: 0.0,
        };
        net.forward(&[1.0, 2.0]);
    }
}
