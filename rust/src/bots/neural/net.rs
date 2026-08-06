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
//! * `HAVE_TORCH` / the deferred `import torch` -- packaging machinery for a
//!   dependency this crate never takes.
//! * `NeuralValue._to_tensor`'s numpy-vs-`torch.tensor` benchmark note -- a
//!   CPython performance comparison between two conversion paths into a
//!   tensor library that does not exist here.
//!
//! `save_checkpoint`/`load_checkpoint` below ARE now ported, as of the
//! training half of this port (`train.rs`) -- but NOT as a port of
//! `torch.save`/`torch.load`. Those are opaque pickle-based binary formats
//! this crate has no reason to read or write (`Cargo.toml`'s `[dependencies]`
//! stays empty: no torch, no pickle-compatible parser). Below is a
//! hand-rolled, explicitly-versioned binary format of this crate's own
//! design that round-trips [`ValueNet`] exactly; see its own doc comment for
//! the layout. It cannot read an existing `.pt` file and does not try to --
//! moot anyway, since a recent encoder-width change (1907 -> 1906) already
//! invalidated every checkpoint that predates this module.

use super::encode::ENCODING_DIM;

/// NUMERICAL GUARD, not a model claim -- see Python's own extensive comment
/// on this constant (`neural_net.py:31-41`) for the full "this is a linear
/// normaliser on the regression target, not the league's tanh squash width"
/// argument; not reproduced here. Final margins are ~[-250, 250]; dividing by
/// 100 keeps the training target near unit scale.
pub const MARGIN_NORM: f64 = 100.0;

/// LayerNorm's numerical-stability epsilon -- PyTorch's `nn.LayerNorm`
/// default (`eps=1e-5`), which `_ResBlock`/`ValueNet.stem` never override.
///
/// `pub(crate)`: `train.rs`'s backward pass needs the exact same constant
/// the forward pass used, not a second copy that could drift from it.
pub(crate) const LAYER_NORM_EPS: f64 = 1e-5;

// --------------------------------------------------------------- arithmetic
//
// `linear`/`relu_inplace`/`layer_norm`/`layer_norm_stats` are `pub(crate)`
// (not just `fn`) so `train.rs`'s forward-with-cache pass can call the
// IDENTICAL arithmetic this eval-mode forward pass uses, in the same
// summation order, rather than a hand-duplicated copy that could silently
// diverge in rounding -- the "eval forward pass is unchanged" property the
// module doc comment promises depends on there being exactly one
// implementation of each op, not two that happen to agree today.

/// `y = W x + b`, `W` row-major `[out_dim, in_dim]`. Mirrors `nn.Linear`.
pub(crate) fn linear(w: &[f64], b: &[f64], x: &[f64], out_dim: usize, in_dim: usize) -> Vec<f64> {
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
pub(crate) fn relu_inplace(x: &mut [f64]) {
    for v in x.iter_mut() {
        if *v < 0.0 {
            *v = 0.0;
        }
    }
}

/// The mean/biased-variance pair `layer_norm` below normalises by, exposed
/// separately because the backward pass needs both numbers (not just the
/// normalised output) to compute `dx`.
pub(crate) fn layer_norm_stats(x: &[f64]) -> (f64, f64) {
    let n = x.len();
    let mean = x.iter().sum::<f64>() / n as f64;
    let var = x.iter().map(|&v| (v - mean) * (v - mean)).sum::<f64>() / n as f64;
    (mean, var)
}

/// `nn.LayerNorm`: normalise over the whole vector (PyTorch's `normalized_shape
/// = (dim,)`, i.e. every element of a 1-D input), biased variance (PyTorch's
/// `LayerNorm` always uses the biased estimator, `unbiased=False`), then
/// scale/shift by `gamma`/`beta`.
pub(crate) fn layer_norm(x: &[f64], gamma: &[f64], beta: &[f64]) -> Vec<f64> {
    let n = x.len();
    debug_assert_eq!(gamma.len(), n);
    debug_assert_eq!(beta.len(), n);
    let (mean, var) = layer_norm_stats(x);
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

// ----------------------------------------------------------------- checkpoint
//
// A hand-rolled, explicitly-versioned binary format -- NOT `torch.save`/
// `torch.load` (see the module doc comment). Layout, in order:
//
// ```text
// magic     8 bytes   b"TTANET01"
// version   u32 LE    1
// in_dim    u32 LE
// hidden    u32 LE
// n_blocks  u32 LE
// stem_w    f64[hidden*in_dim] LE      stem_b        f64[hidden] LE
// stem_ln_gamma f64[hidden] LE          stem_ln_beta  f64[hidden] LE
// -- for each of n_blocks, in order --
//   fc1_w f64[hidden*hidden]  fc1_b f64[hidden]
//   fc2_w f64[hidden*hidden]  fc2_b f64[hidden]
//   ln_gamma f64[hidden]      ln_beta f64[hidden]
// head_w    f64[hidden] LE   head_b  f64 LE
// n_meta    u32 LE
// -- for each of n_meta --
//   key_len u32 LE   key <key_len> bytes UTF-8   value f64 LE
// ```
//
// Every weight is a raw 8-byte IEEE754 `f64`, never decimal text, so a
// round trip is bit-for-bit exact -- see `checkpoint_round_trip_is_bit_for_
// bit_exact` below -- and there is nothing to lose to rounding the way there
// would be through a decimal format.

/// Magic bytes identifying this crate's own checkpoint format.
const CHECKPOINT_MAGIC: &[u8] = b"TTANET01";
/// Format version. Bump on any layout change and branch on it in
/// [`load_checkpoint`]; never reinterpret an old version's bytes under a
/// new layout.
const CHECKPOINT_VERSION: u32 = 1;

fn check_finite_slice(xs: &[f64], what: &str) -> Result<(), String> {
    match xs.iter().find(|v| !v.is_finite()) {
        Some(v) => Err(format!("checkpoint: non-finite value in {what}: {v}")),
        None => Ok(()),
    }
}

pub(crate) fn push_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// `pub(crate)`: `train.rs`'s own `RankData` binary format (record counts
/// can run past `u32::MAX` bits of precision headroom is nice to have, if
/// never strictly needed at today's dataset sizes) reuses this rather than
/// hand-rolling a second little-endian writer.
pub(crate) fn push_u64(out: &mut Vec<u8>, v: u64) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// Append every element of `xs` as 8-byte little-endian `f64`s, in order.
pub(crate) fn push_f64_slice(out: &mut Vec<u8>, xs: &[f64]) {
    for &x in xs {
        out.extend_from_slice(&x.to_le_bytes());
    }
}

/// A read cursor over binary-format bytes (checkpoints here, `train.rs`'s
/// `RankData` shards there -- `pub(crate)` for exactly that reuse). Every
/// accessor bounds-checks and returns `Err` on a short read instead of
/// panicking -- a truncated or corrupt file is an expected failure mode of
/// loading an artifact off disk, not a programming bug.
pub(crate) struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Reader { bytes, pos: 0 }
    }

    pub(crate) fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.pos.checked_add(n).ok_or_else(|| "checkpoint: length overflow".to_string())?;
        let slice = self.bytes.get(self.pos..end).ok_or_else(|| {
            format!("checkpoint: truncated (wanted {n} bytes at offset {}, file has {})", self.pos, self.bytes.len())
        })?;
        self.pos = end;
        Ok(slice)
    }

    pub(crate) fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    pub(crate) fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    pub(crate) fn f64(&mut self) -> Result<f64, String> {
        Ok(f64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    pub(crate) fn f64_vec(&mut self, n: usize) -> Result<Vec<f64>, String> {
        (0..n).map(|_| self.f64()).collect()
    }

    pub(crate) fn string(&mut self, n: usize) -> Result<String, String> {
        String::from_utf8(self.take(n)?.to_vec()).map_err(|e| format!("checkpoint: meta key is not utf8: {e}"))
    }

    /// True once every byte has been consumed -- callers use this to catch
    /// trailing garbage after an otherwise well-formed record.
    pub(crate) fn at_end(&self) -> bool {
        self.pos == self.bytes.len()
    }

    /// Bytes consumed so far, for an error message pointing at exactly
    /// where a "trailing bytes" mismatch was detected.
    pub(crate) fn pos(&self) -> usize {
        self.pos
    }
}

/// Serialise `net` plus `meta` (arbitrary named numbers -- the trainer's
/// bookkeeping, e.g. `val_pair_acc`/`epoch`, mirroring `save_checkpoint`'s
/// `meta` dict in `neural_net.py`) to `path` in this crate's own binary
/// format (see the block comment above). Writes a `.tmp` sibling and
/// renames over `path`, matching `bots::weighted::eval::save_weights`'s
/// crash-safety precedent (a reader never observes a half-written file).
///
/// # Errors
/// If any weight or meta value is non-finite (a NaN/Inf checkpoint is a
/// training bug, not something worth writing to disk), or the write fails.
pub fn save_checkpoint(path: &std::path::Path, net: &ValueNet, meta: &[(&str, f64)]) -> Result<(), String> {
    check_finite_slice(&net.stem_w, "stem_w")?;
    check_finite_slice(&net.stem_b, "stem_b")?;
    check_finite_slice(&net.stem_ln_gamma, "stem_ln_gamma")?;
    check_finite_slice(&net.stem_ln_beta, "stem_ln_beta")?;
    for (i, b) in net.blocks.iter().enumerate() {
        check_finite_slice(&b.fc1_w, &format!("blocks[{i}].fc1_w"))?;
        check_finite_slice(&b.fc1_b, &format!("blocks[{i}].fc1_b"))?;
        check_finite_slice(&b.fc2_w, &format!("blocks[{i}].fc2_w"))?;
        check_finite_slice(&b.fc2_b, &format!("blocks[{i}].fc2_b"))?;
        check_finite_slice(&b.ln_gamma, &format!("blocks[{i}].ln_gamma"))?;
        check_finite_slice(&b.ln_beta, &format!("blocks[{i}].ln_beta"))?;
    }
    check_finite_slice(&net.head_w, "head_w")?;
    if !net.head_b.is_finite() {
        return Err(format!("checkpoint: non-finite head_b: {}", net.head_b));
    }
    for (name, v) in meta {
        if !v.is_finite() {
            return Err(format!("checkpoint: non-finite meta {name:?}: {v}"));
        }
    }

    let mut out = Vec::new();
    out.extend_from_slice(CHECKPOINT_MAGIC);
    push_u32(&mut out, CHECKPOINT_VERSION);
    push_u32(&mut out, net.in_dim as u32);
    push_u32(&mut out, net.hidden as u32);
    push_u32(&mut out, net.blocks.len() as u32);
    push_f64_slice(&mut out, &net.stem_w);
    push_f64_slice(&mut out, &net.stem_b);
    push_f64_slice(&mut out, &net.stem_ln_gamma);
    push_f64_slice(&mut out, &net.stem_ln_beta);
    for b in &net.blocks {
        push_f64_slice(&mut out, &b.fc1_w);
        push_f64_slice(&mut out, &b.fc1_b);
        push_f64_slice(&mut out, &b.fc2_w);
        push_f64_slice(&mut out, &b.fc2_b);
        push_f64_slice(&mut out, &b.ln_gamma);
        push_f64_slice(&mut out, &b.ln_beta);
    }
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

/// Read back what [`save_checkpoint`] wrote: the network plus its meta
/// entries in the order they were written.
///
/// # Errors
/// A wrong magic, an unsupported version, a truncated/oversized file, or a
/// non-utf8 meta key -- every case fails loudly with a `String` rather than
/// silently loading a mismatched or garbage network (a caller reading a
/// pre-1906-encoder-width checkpoint, in particular, gets a clear error
/// instead of a `ValueNet` with the wrong `in_dim`).
pub fn load_checkpoint(path: &std::path::Path) -> Result<(ValueNet, Vec<(String, f64)>), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse_checkpoint(&bytes).map_err(|e| format!("{}: {e}", path.display()))
}

fn parse_checkpoint(bytes: &[u8]) -> Result<(ValueNet, Vec<(String, f64)>), String> {
    let mut r = Reader::new(bytes);
    let magic = r.take(CHECKPOINT_MAGIC.len())?;
    if magic != CHECKPOINT_MAGIC {
        return Err(format!("bad magic {magic:?}: not a TTANET checkpoint"));
    }
    let version = r.u32()?;
    if version != CHECKPOINT_VERSION {
        return Err(format!("checkpoint version {version} is not supported (this build reads only version {CHECKPOINT_VERSION})"));
    }
    let in_dim = r.u32()? as usize;
    // The encoder's width changed once already (1907 -> 1906) with nothing
    // checking that a checkpoint's `in_dim` and the encoder's current width
    // still agreed -- a stale checkpoint parsed cleanly and produced a
    // `ValueNet` whose first mismatched `forward` call panicked deep inside
    // `linear`'s arithmetic instead of failing loudly here, at load time,
    // by name. Mirrors `rankdata::read_shard`'s `dim != ENCODING_DIM` guard.
    if in_dim != ENCODING_DIM {
        return Err(format!(
            "checkpoint in_dim {in_dim} does not match this build's encoder width {ENCODING_DIM} -- \
             this checkpoint was trained against a different encoder, do not load it as-is"
        ));
    }
    let hidden = r.u32()? as usize;
    let n_blocks = r.u32()? as usize;

    let stem_w = r.f64_vec(hidden * in_dim)?;
    let stem_b = r.f64_vec(hidden)?;
    let stem_ln_gamma = r.f64_vec(hidden)?;
    let stem_ln_beta = r.f64_vec(hidden)?;
    let mut blocks = Vec::with_capacity(n_blocks);
    for _ in 0..n_blocks {
        blocks.push(ResBlock {
            fc1_w: r.f64_vec(hidden * hidden)?,
            fc1_b: r.f64_vec(hidden)?,
            fc2_w: r.f64_vec(hidden * hidden)?,
            fc2_b: r.f64_vec(hidden)?,
            ln_gamma: r.f64_vec(hidden)?,
            ln_beta: r.f64_vec(hidden)?,
        });
    }
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
    if r.pos != bytes.len() {
        return Err(format!("checkpoint: {} trailing bytes after a well-formed record", bytes.len() - r.pos));
    }
    Ok((ValueNet { in_dim, hidden, stem_w, stem_b, stem_ln_gamma, stem_ln_beta, blocks, head_w, head_b }, meta))
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

    /// Build a small but non-trivial net (2 blocks, asymmetric dims) with
    /// every field a distinct value, so a bug that swaps two same-shaped
    /// fields (e.g. `fc1_w`/`fc2_w`, or block 0 with block 1) would fail
    /// this rather than hide behind coincidentally-equal test fixtures.
    ///
    /// `in_dim` is [`ENCODING_DIM`], not some small placeholder, because
    /// `load_checkpoint` now refuses any other width (see
    /// `a_checkpoint_saved_at_one_encoder_width_refuses_to_load_at_another`
    /// below) -- every test that round-trips through `load_checkpoint` needs
    /// a net this check accepts.
    fn sample_net_for_round_trip() -> ValueNet {
        let mk = |base: f64, n: usize| (0..n).map(|i| base + i as f64 * 0.01).collect::<Vec<f64>>();
        ValueNet {
            in_dim: ENCODING_DIM,
            hidden: 3,
            stem_w: mk(1.0, 3 * ENCODING_DIM),
            stem_b: mk(2.0, 3),
            stem_ln_gamma: mk(3.0, 3),
            stem_ln_beta: mk(4.0, 3),
            blocks: vec![
                ResBlock {
                    fc1_w: mk(5.0, 9),
                    fc1_b: mk(6.0, 3),
                    fc2_w: mk(7.0, 9),
                    fc2_b: mk(8.0, 3),
                    ln_gamma: mk(9.0, 3),
                    ln_beta: mk(10.0, 3),
                },
                ResBlock {
                    fc1_w: mk(11.0, 9),
                    fc1_b: mk(12.0, 3),
                    fc2_w: mk(13.0, 9),
                    fc2_b: mk(14.0, 3),
                    ln_gamma: mk(15.0, 3),
                    ln_beta: mk(16.0, 3),
                },
            ],
            head_w: mk(17.0, 3),
            head_b: 42.5,
        }
    }

    /// The property `save_checkpoint`/`load_checkpoint` exist for:
    /// `load(save(net)) == net` bit-for-bit on every `f64` field, not just
    /// approximately -- the format stores raw IEEE754 bytes precisely so
    /// there is nothing to round.
    #[test]
    fn checkpoint_round_trip_is_bit_for_bit_exact() {
        let net = sample_net_for_round_trip();
        let dir = std::env::temp_dir().join(format!("ttanet_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("round_trip.ckpt");
        save_checkpoint(&path, &net, &[("epoch", 7.0), ("val_mae_culture", 12.5)]).unwrap();
        let (loaded, meta) = load_checkpoint(&path).unwrap();
        assert_eq!(loaded, net);
        assert_eq!(meta, vec![("epoch".to_string(), 7.0), ("val_mae_culture".to_string(), 12.5)]);
        std::fs::remove_file(&path).ok();
        std::fs::remove_dir(&dir).ok();
    }

    /// A checkpoint with an unsupported version number must fail loudly,
    /// not silently reinterpret the following bytes under today's layout --
    /// the exact failure mode this format exists to rule out relative to
    /// "there is no serialization format to parse".
    #[test]
    fn load_checkpoint_rejects_an_unsupported_version() {
        let net = sample_net_for_round_trip();
        let dir = std::env::temp_dir().join(format!("ttanet_test_v_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad_version.ckpt");
        save_checkpoint(&path, &net, &[]).unwrap();
        let mut bytes = std::fs::read(&path).unwrap();
        // Version is the 4 bytes right after the 8-byte magic.
        bytes[8..12].copy_from_slice(&99u32.to_le_bytes());
        std::fs::write(&path, &bytes).unwrap();
        let err = load_checkpoint(&path).unwrap_err();
        assert!(err.contains("version 99"), "{err}");
        std::fs::remove_file(&path).ok();
        std::fs::remove_dir(&dir).ok();
    }

    /// A file truncated mid-record (e.g. copy interrupted) must fail
    /// loudly rather than read a short weight vector as if it were complete.
    #[test]
    fn load_checkpoint_rejects_a_truncated_file() {
        let net = sample_net_for_round_trip();
        let dir = std::env::temp_dir().join(format!("ttanet_test_t_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("truncated.ckpt");
        save_checkpoint(&path, &net, &[]).unwrap();
        let mut bytes = std::fs::read(&path).unwrap();
        bytes.truncate(bytes.len() - 5);
        std::fs::write(&path, &bytes).unwrap();
        let err = load_checkpoint(&path).unwrap_err();
        assert!(err.contains("truncated"), "{err}");
        std::fs::remove_file(&path).ok();
        std::fs::remove_dir(&dir).ok();
    }

    /// The bug this check exists to make impossible: the encoder's width
    /// changed once already (1907 -> 1906) with nothing checking that a
    /// checkpoint's `in_dim` and the encoder's current width still agreed,
    /// so a stale checkpoint parsed cleanly and produced a `ValueNet`
    /// carrying the wrong `in_dim` -- the mismatch only surfaced later, as
    /// an `assert_eq!` panic deep inside `forward`, instead of a clear
    /// error at load time. Hand-assemble a checkpoint whose header claims a
    /// width one wider than [`ENCODING_DIM`] (rather than reusing
    /// `sample_net_for_round_trip`, which is now pinned at the real width)
    /// and confirm `load_checkpoint` refuses it by name rather than
    /// misinterpreting the byte stream or returning a mis-sized net.
    #[test]
    fn a_checkpoint_saved_at_one_encoder_width_refuses_to_load_at_another() {
        let mut net = sample_net_for_round_trip();
        let bad_dim = ENCODING_DIM + 1;
        // Give the net the bad width and matching-length weight vectors so
        // `save_checkpoint` (which has no opinion on `in_dim`) writes a
        // well-formed file; the check under test lives entirely in load.
        net.in_dim = bad_dim;
        net.stem_w = vec![0.0; net.hidden * bad_dim];
        let dir = std::env::temp_dir().join(format!("ttanet_test_dim_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("wrong_width.ckpt");
        save_checkpoint(&path, &net, &[]).unwrap();
        let err = load_checkpoint(&path).unwrap_err();
        assert!(
            err.contains(&bad_dim.to_string()) && err.contains(&ENCODING_DIM.to_string()),
            "error should name both widths, got: {err}"
        );
        std::fs::remove_file(&path).ok();
        std::fs::remove_dir(&dir).ok();
    }

    /// `save_checkpoint` must refuse to write a NaN weight -- a checkpoint
    /// that "round trips" a NaN is not the safety property the finite
    /// check is for; the bug should surface at save time, at the source.
    #[test]
    fn save_checkpoint_rejects_a_non_finite_weight() {
        let mut net = sample_net_for_round_trip();
        net.blocks[1].fc2_b[0] = f64::NAN;
        let dir = std::env::temp_dir().join(format!("ttanet_test_nan_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("nan.ckpt");
        let err = save_checkpoint(&path, &net, &[]).unwrap_err();
        assert!(err.contains("fc2_b"), "{err}");
        std::fs::remove_dir(&dir).ok();
    }
}
