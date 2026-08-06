//! The GPU-trained value network -- a candle port of the SAME arithmetic
//! `tta::bots::neural::net::ValueNet::forward` computes, built and checked
//! against it directly (not trusted on inspection: see this module's
//! `#[cfg(test)]` block, the first thing written in this crate).
//!
//! Why a second implementation exists at all, and why that is the dangerous
//! part: `net.rs`'s hand-rolled forward pass is what BOTS run at play time,
//! on any machine, dependency-free (`rust/Cargo.toml`'s `[dependencies]`
//! stays empty). This module is what TRAINS the weights those bots load,
//! using candle's autograd on a GPU instead of `train.rs`'s hand-derived
//! CPU backprop. Two implementations of the same maths, with nothing
//! checking they agree, is this project's own named recurring bug class
//! (`rust/DESIGN.md`: "present in this registry, absent from that one, and
//! nothing fails when they disagree") -- so the tests below are the reason
//! this module is allowed to exist as a second implementation at all, not
//! an afterthought bolted on once it already worked.

use candle_core::{Device, Result as CResult, Tensor, Var, D};
use candle_nn::ops;

use tta::bots::neural::net::{ResBlock as CpuResBlock, ValueNet as CpuValueNet};

/// PyTorch's `nn.LayerNorm` default epsilon -- must equal `tta::bots::
/// neural::net::LAYER_NORM_EPS` exactly. That constant is `pub(crate)`
/// *within* the `tta` crate (deliberately: `net.rs`'s own doc comment says
/// it exists so `train.rs`'s hand-rolled backward pass cannot drift from
/// the forward pass's constant), so it is not visible here across the crate
/// boundary -- this is a second copy of the same literal, not a shared one.
/// `forward_matches_the_hand_rolled_cpu_net_to_a_tight_tolerance` below is
/// what would catch the two ever drifting apart; nothing at the type level
/// ties them together.
const LAYER_NORM_EPS: f64 = 1e-5;

struct GpuResBlock {
    fc1_w: Var,
    fc1_b: Var,
    fc2_w: Var,
    fc2_b: Var,
    ln_gamma: Var,
    ln_beta: Var,
}

/// The GPU-trained mirror of [`tta::bots::neural::net::ValueNet`]. Every
/// weight lives in a candle [`Var`] (a mutable, gradient-tracked `Tensor`)
/// on `device` -- CPU by default; CUDA when this crate is built with
/// `--features cuda` and `device` is a `Device::Cuda` (see `main.rs`'s
/// `--device` flag).
pub struct GpuValueNet {
    pub in_dim: usize,
    pub hidden: usize,
    stem_w: Var,
    stem_b: Var,
    stem_ln_gamma: Var,
    stem_ln_beta: Var,
    blocks: Vec<GpuResBlock>,
    head_w: Var,
    head_b: Var,
    device: Device,
}

fn var_from_f64_2d(data: &[f64], shape: (usize, usize), device: &Device) -> CResult<Var> {
    Var::from_vec(data.to_vec(), shape, device)
}

fn var_from_f64_1d(data: &[f64], device: &Device) -> CResult<Var> {
    Var::from_vec(data.to_vec(), data.len(), device)
}

impl GpuValueNet {
    /// Build a GPU net whose weights are copies of `net`'s, `f64` all the
    /// way through (an `f64` `Vec` into an `f64` candle `Tensor` loses
    /// nothing -- there is no narrowing here, unlike the shard format's
    /// deliberate `f64` -> `f32` storage narrowing in `rankdata.rs`). This
    /// is how BOTH a warm-start checkpoint and a freshly-initialised
    /// network enter this crate: there is exactly one initialisation scheme
    /// in this whole project (`train.rs`'s `random_init`, reused as-is by
    /// `main.rs`), never a second one invented here for candle.
    pub fn from_cpu(net: &CpuValueNet, device: &Device) -> CResult<GpuValueNet> {
        let hidden = net.hidden;
        let in_dim = net.in_dim;
        let blocks = net
            .blocks
            .iter()
            .map(|b: &CpuResBlock| -> CResult<GpuResBlock> {
                Ok(GpuResBlock {
                    fc1_w: var_from_f64_2d(&b.fc1_w, (hidden, hidden), device)?,
                    fc1_b: var_from_f64_1d(&b.fc1_b, device)?,
                    fc2_w: var_from_f64_2d(&b.fc2_w, (hidden, hidden), device)?,
                    fc2_b: var_from_f64_1d(&b.fc2_b, device)?,
                    ln_gamma: var_from_f64_1d(&b.ln_gamma, device)?,
                    ln_beta: var_from_f64_1d(&b.ln_beta, device)?,
                })
            })
            .collect::<CResult<Vec<_>>>()?;
        Ok(GpuValueNet {
            in_dim,
            hidden,
            stem_w: var_from_f64_2d(&net.stem_w, (hidden, in_dim), device)?,
            stem_b: var_from_f64_1d(&net.stem_b, device)?,
            stem_ln_gamma: var_from_f64_1d(&net.stem_ln_gamma, device)?,
            stem_ln_beta: var_from_f64_1d(&net.stem_ln_beta, device)?,
            blocks,
            head_w: var_from_f64_2d(&net.head_w, (1, hidden), device)?,
            head_b: var_from_f64_1d(std::slice::from_ref(&net.head_b), device)?,
            device: device.clone(),
        })
    }

    /// Read every weight back out to an ordinary [`CpuValueNet`] -- the
    /// path a trained network takes to reach `net::save_checkpoint` (the
    /// SAME function `net.rs`'s own tests and `neuraltrain` write with), so
    /// a checkpoint this crate produces is not a second file format, it is
    /// the one format. See `checkpoint_round_trips_through_the_cpu_crates_
    /// own_load_checkpoint` below for the test that actually exercises the
    /// disk round trip through `net::save_checkpoint`/`load_checkpoint`.
    pub fn to_cpu(&self) -> CResult<CpuValueNet> {
        let vec1 = |t: &Var| -> CResult<Vec<f64>> { t.as_tensor().flatten_all()?.to_vec1::<f64>() };
        let blocks = self
            .blocks
            .iter()
            .map(|b| -> CResult<CpuResBlock> {
                Ok(CpuResBlock {
                    fc1_w: vec1(&b.fc1_w)?,
                    fc1_b: vec1(&b.fc1_b)?,
                    fc2_w: vec1(&b.fc2_w)?,
                    fc2_b: vec1(&b.fc2_b)?,
                    ln_gamma: vec1(&b.ln_gamma)?,
                    ln_beta: vec1(&b.ln_beta)?,
                })
            })
            .collect::<CResult<Vec<_>>>()?;
        Ok(CpuValueNet {
            in_dim: self.in_dim,
            hidden: self.hidden,
            stem_w: vec1(&self.stem_w)?,
            stem_b: vec1(&self.stem_b)?,
            stem_ln_gamma: vec1(&self.stem_ln_gamma)?,
            stem_ln_beta: vec1(&self.stem_ln_beta)?,
            blocks,
            head_w: vec1(&self.head_w)?,
            head_b: vec1(&self.head_b)?[0],
        })
    }

    /// Every trainable [`Var`], in a stable order -- what `train::Trainer`
    /// builds the `candle_nn::AdamW` optimizer over.
    pub fn vars(&self) -> Vec<Var> {
        let mut v = vec![self.stem_w.clone(), self.stem_b.clone(), self.stem_ln_gamma.clone(), self.stem_ln_beta.clone()];
        for b in &self.blocks {
            v.extend([b.fc1_w.clone(), b.fc1_b.clone(), b.fc2_w.clone(), b.fc2_b.clone(), b.ln_gamma.clone(), b.ln_beta.clone()]);
        }
        v.push(self.head_w.clone());
        v.push(self.head_b.clone());
        v
    }

    pub fn device(&self) -> &Device {
        &self.device
    }

    /// `y = x W^T + b`, batched: `x` is `[batch, in_dim]`, `w` is
    /// `[out_dim, in_dim]` (`net.rs`'s own row-major convention -- see its
    /// `linear` doc comment), `b` is `[out_dim]`. Mirrors `nn.Linear`/
    /// `net::linear` exactly, batched over rows instead of one row at a
    /// time (candle's `matmul`, not a hand-tiled loop -- there is no
    /// equivalent here to `train.rs`'s `BATCH_TILE`; that is candle's job
    /// now, which is the entire reason to take this dependency).
    fn linear(x: &Tensor, w: &Var, b: &Var) -> CResult<Tensor> {
        x.matmul(&w.as_tensor().t()?)?.broadcast_add(b.as_tensor())
    }

    /// `nn.LayerNorm` over the last dimension: mean, BIASED variance (`nn.
    /// LayerNorm`'s `unbiased=False`, matching `net::layer_norm_stats`
    /// exactly), normalise, then affine by `gamma`/`beta`. Written out with
    /// plain Tensor ops rather than calling `candle_nn::LayerNorm::forward`
    /// because that helper's fast contiguous-input path casts `eps` to
    /// `f32` regardless of the tensor's own dtype (see `candle_nn::
    /// layer_norm::LayerNorm::forward`'s `crate::ops::layer_norm(x, w, b,
    /// eps as f32)` branch, checked against the vendored 0.11.0 source) --
    /// this crate runs every tensor in `f64` specifically so its numbers
    /// can be compared against `net.rs`'s `f64` arithmetic to a tight
    /// tolerance, and a silently-narrowed epsilon would undermine exactly
    /// that, for a saving of nothing (this function is not the bottleneck).
    fn layer_norm(x: &Tensor, gamma: &Var, beta: &Var, eps: f64) -> CResult<Tensor> {
        let hidden = x.dim(D::Minus1)?;
        let mean = (x.sum_keepdim(D::Minus1)? / hidden as f64)?;
        let centered = x.broadcast_sub(&mean)?;
        let var = (centered.sqr()?.sum_keepdim(D::Minus1)? / hidden as f64)?;
        let denom = (var + eps)?.sqrt()?;
        let normed = centered.broadcast_div(&denom)?;
        normed.broadcast_mul(gamma.as_tensor())?.broadcast_add(beta.as_tensor())
    }

    /// Batched forward pass, `x: [batch, in_dim]` -> `[batch]` predictions
    /// in the network's own normalised units (a training label divided by
    /// `MARGIN_NORM`, matching every doc comment in `net.rs`/`train.rs`).
    /// Mirrors `ValueNet::forward`/`ResBlock::forward` (`net.rs`) row for
    /// row: stem linear -> LayerNorm -> ReLU, then each block's
    /// `relu(fc1(x))` -> `fc2` -> dropout -> residual add -> LayerNorm ->
    /// ReLU, then the head linear.
    ///
    /// `train`/`dropout_p`: when `train` is true and `dropout_p > 0`,
    /// applies candle's own inverted dropout ([`ops::dropout`]) after each
    /// block's `fc2` -- the same place `train.rs`'s `forward_train` draws
    /// its mask. The RNG stream does NOT need to match `train.rs`'s
    /// `PyRandom` draws bit for bit (each trainer's dropout noise is its
    /// own; only the DETERMINISTIC eval-mode arithmetic is a claimed
    /// agreement here -- see this module's tests). `train=false` makes
    /// dropout the identity, exactly as `nn.Dropout` in eval mode and as
    /// `net.rs`'s `ValueNet::forward` (which has no dropout at all, matching
    /// `model.eval()` being called unconditionally -- see `net.rs`'s own
    /// top doc comment).
    pub fn forward(&self, x: &Tensor, train: bool, dropout_p: f64) -> CResult<Tensor> {
        let mut h = Self::linear(x, &self.stem_w, &self.stem_b)?;
        h = Self::layer_norm(&h, &self.stem_ln_gamma, &self.stem_ln_beta, LAYER_NORM_EPS)?;
        h = h.relu()?;
        for block in &self.blocks {
            let a1 = Self::linear(&h, &block.fc1_w, &block.fc1_b)?.relu()?;
            let mut a2 = Self::linear(&a1, &block.fc2_w, &block.fc2_b)?;
            if train && dropout_p > 0.0 {
                a2 = ops::dropout(&a2, dropout_p as f32)?;
            }
            let resid = (h + a2)?;
            let normed = Self::layer_norm(&resid, &block.ln_gamma, &block.ln_beta, LAYER_NORM_EPS)?;
            h = normed.relu()?;
        }
        let out = Self::linear(&h, &self.head_w, &self.head_b)?; // [batch, 1]
        out.squeeze(1)
    }
}

// ===================================================================== tests
//
// THE safety net this whole crate exists to earn -- written before anything
// here was optimised or wired into a training loop, per this crate's own
// mandate. `net.rs`'s own top doc comment explains why there is no Python/
// torch oracle for the CPU implementation either: the same is true here, so
// these tests pin this module against `net.rs`'s ACTUAL Rust function calls
// directly (not a re-derivation) wherever that is possible, and against
// hand-worked arithmetic where it is not (`net.rs`'s own precedent for a
// module with no external oracle).

#[cfg(test)]
mod tests {
    use super::*;
    use tta::bots::neural::net::{load_checkpoint, save_checkpoint, ResBlock};

    /// A small, asymmetric, non-trivial network -- every field a distinct
    /// value, deliberately not reusing `net.rs`'s own `sample_net_for_
    /// round_trip` fixture (a bug that coincidentally cancelled out against
    /// that exact fixture would not be this crate's problem to inherit).
    fn sample_cpu_net() -> CpuValueNet {
        let mk = |base: f64, n: usize| (0..n).map(|i| base - (i as f64) * 0.013 + ((i * i) as f64) * 0.0007).collect::<Vec<f64>>();
        CpuValueNet {
            in_dim: 5,
            hidden: 4,
            stem_w: mk(0.31, 20),
            stem_b: mk(-0.12, 4),
            stem_ln_gamma: mk(1.05, 4),
            stem_ln_beta: mk(0.02, 4),
            blocks: vec![
                ResBlock {
                    fc1_w: mk(0.18, 16),
                    fc1_b: mk(0.04, 4),
                    fc2_w: mk(-0.22, 16),
                    fc2_b: mk(0.01, 4),
                    ln_gamma: mk(0.97, 4),
                    ln_beta: mk(-0.03, 4),
                },
                ResBlock {
                    fc1_w: mk(-0.11, 16),
                    fc1_b: mk(0.07, 4),
                    fc2_w: mk(0.29, 16),
                    fc2_b: mk(-0.02, 4),
                    ln_gamma: mk(1.02, 4),
                    ln_beta: mk(0.015, 4),
                },
            ],
            head_w: mk(0.4, 4),
            head_b: 0.271,
        }
    }

    fn sample_inputs() -> Vec<Vec<f64>> {
        vec![
            vec![0.5, -1.2, 0.3, 2.0, -0.7],
            vec![-0.1, 0.0, 1.5, -2.3, 0.9],
            vec![3.1, -0.4, -0.9, 0.2, 1.1],
        ]
    }

    /// THE core agreement test: identical weights, identical input, through
    /// `net.rs`'s hand-rolled `ValueNet::forward` (what bots call at play
    /// time) and through this module's candle `forward` (what training
    /// calls), and the two scalar predictions must match to a tight
    /// tolerance. Exercises the stem linear, LayerNorm (eps included, since
    /// this net's hidden width is 4 -- not the degenerate width-1 case where
    /// LayerNorm always returns `beta`), ReLU, TWO residual blocks (so a bug
    /// that only shows up after the first block's output feeds the second
    /// as input cannot hide), and the head. Dropout is off (`train=false`)
    /// on the candle side to match `net.rs::ValueNet::forward`'s unconditional
    /// eval-mode semantics -- see this module's `forward` doc comment.
    #[test]
    fn forward_matches_the_hand_rolled_cpu_net_to_a_tight_tolerance() {
        let cpu_net = sample_cpu_net();
        let device = Device::Cpu;
        let gpu_net = GpuValueNet::from_cpu(&cpu_net, &device).expect("build gpu net");

        for x in sample_inputs() {
            let want = cpu_net.forward(&x);
            let xt = Tensor::from_vec(x.clone(), (1, cpu_net.in_dim), &device).unwrap();
            let got_t = gpu_net.forward(&xt, false, 0.0).unwrap();
            let got: Vec<f64> = got_t.to_vec1().unwrap();
            assert_eq!(got.len(), 1);
            assert!((got[0] - want).abs() < 1e-9, "x={x:?} want={want} got={}", got[0]);
        }
    }

    /// The same agreement, batched: every sample row through ONE candle
    /// forward call at once must match the per-row hand-rolled results --
    /// pins the batching itself (broadcasting `w`/`b` across rows), not just
    /// the single-row path the test above already covers.
    #[test]
    fn batched_forward_matches_row_by_row_hand_rolled_predictions() {
        let cpu_net = sample_cpu_net();
        let device = Device::Cpu;
        let gpu_net = GpuValueNet::from_cpu(&cpu_net, &device).expect("build gpu net");
        let inputs = sample_inputs();

        let want: Vec<f64> = inputs.iter().map(|x| cpu_net.forward(x)).collect();
        let flat: Vec<f64> = inputs.iter().flatten().copied().collect();
        let xt = Tensor::from_vec(flat, (inputs.len(), cpu_net.in_dim), &device).unwrap();
        let got: Vec<f64> = gpu_net.forward(&xt, false, 0.0).unwrap().to_vec1().unwrap();

        assert_eq!(got.len(), want.len());
        for (g, w) in got.iter().zip(want.iter()) {
            assert!((g - w).abs() < 1e-9, "got={g} want={w}");
        }
    }

    /// `from_cpu` then `to_cpu` must reproduce the source network exactly
    /// (an `f64` -> `Tensor` -> `f64` round trip loses nothing) -- the
    /// property the checkpoint save path below depends on.
    #[test]
    fn from_cpu_then_to_cpu_round_trips_every_weight_exactly() {
        let cpu_net = sample_cpu_net();
        let gpu_net = GpuValueNet::from_cpu(&cpu_net, &Device::Cpu).unwrap();
        let back = gpu_net.to_cpu().unwrap();
        assert_eq!(back, cpu_net);
    }

    /// THE checkpoint-compatibility property this crate exists to deliver:
    /// a network trained here must be usable by the bots. Convert a GPU net
    /// to a [`CpuValueNet`] and hand it to `net.rs`'s OWN
    /// `save_checkpoint`/`load_checkpoint` (not a reimplementation of the
    /// format -- the exact functions `neuraltrain` and the bots' own
    /// loading path use), then check the round-tripped network's forward
    /// pass agrees with this module's candle forward on fresh inputs.
    #[test]
    fn checkpoint_round_trips_through_the_cpu_crates_own_load_checkpoint() {
        let cpu_net = sample_cpu_net();
        let gpu_net = GpuValueNet::from_cpu(&cpu_net, &Device::Cpu).unwrap();
        let trained_cpu_view = gpu_net.to_cpu().unwrap();

        let dir = std::env::temp_dir().join(format!("gputrain_test_ckpt_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("gpu_trained.ckpt");
        save_checkpoint(&path, &trained_cpu_view, &[("epoch", 3.0)]).expect("save checkpoint");
        let (loaded, meta) = load_checkpoint(&path).expect("load checkpoint");
        std::fs::remove_file(&path).ok();
        std::fs::remove_dir(&dir).ok();

        assert_eq!(loaded, trained_cpu_view, "checkpoint must round-trip the exact weights net.rs saved");
        assert_eq!(meta, vec![("epoch".to_string(), 3.0)]);

        // And the loaded net (what a bot actually runs) must still agree
        // with this crate's own forward pass, not just with the
        // pre-checkpoint in-memory copy -- closes the loop end to end.
        for x in sample_inputs() {
            let want = loaded.forward(&x);
            let xt = Tensor::from_vec(x.clone(), (1, gpu_net.in_dim), &Device::Cpu).unwrap();
            let got: Vec<f64> = gpu_net.forward(&xt, false, 0.0).unwrap().to_vec1().unwrap();
            assert!((got[0] - want).abs() < 1e-9, "x={x:?} want={want} got={}", got[0]);
        }
    }

    /// Dropout must be the identity in eval mode (`train=false`), exactly
    /// as `nn.Dropout`/`net.rs`'s dropout-free `ValueNet::forward` -- a
    /// non-zero `dropout_p` passed at eval time must not perturb the
    /// output, matching `train.rs`'s own `dropout_is_the_identity_in_eval`
    /// precedent for the CPU path.
    #[test]
    fn dropout_probability_is_ignored_in_eval_mode() {
        let cpu_net = sample_cpu_net();
        let gpu_net = GpuValueNet::from_cpu(&cpu_net, &Device::Cpu).unwrap();
        let x = sample_inputs().into_iter().next().unwrap();
        let xt = Tensor::from_vec(x, (1, cpu_net.in_dim), &Device::Cpu).unwrap();
        let a: Vec<f64> = gpu_net.forward(&xt, false, 0.0).unwrap().to_vec1().unwrap();
        let b: Vec<f64> = gpu_net.forward(&xt, false, 0.9).unwrap().to_vec1().unwrap();
        assert_eq!(a, b, "eval-mode forward must be identical regardless of dropout_p");
    }
}
