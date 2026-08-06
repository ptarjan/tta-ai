//! The GPU training loop: candle's autograd plus `candle_nn::AdamW`, driving
//! the SAME combined value+ranking objective `tta::bots::neural::train`
//! computes by hand on the CPU. Read that module's own top doc comment
//! first for the objective's shape and for why epoch 0's VACUITY check
//! exists (`docs/NEURAL_LOOP_NULL.md`'s 41-hour null) -- `main.rs` is what
//! reproduces that reporting; this module is just the arithmetic and the
//! optimizer step.
//!
//! ## Why this loop looks different from `train.rs`'s
//!
//! The CPU trainer hand-accumulates gradients over per-thread chunks of a
//! minibatch, then calls one `AdamW::step` (`Trainer::zero_grad` /
//! `accumulate_pair_batch` / `accumulate_value_batch` / `optim_step` in
//! `train.rs`) because there is no autograd to do that bookkeeping for it.
//! candle's `Optimizer::backward_step` already IS "build the loss, call
//! `.backward()`, call `.step()`" in one call -- the accumulate/zero_grad
//! split earns nothing here and is not reproduced; this is closer to what
//! `experiments/neural_train_rank.py`'s own `opt.zero_grad(); loss.backward();
//! opt.step()` looked like before `train.rs` had to hand-roll the same
//! result.

use candle_core::{Result as CResult, Tensor};
use candle_nn::{AdamW, Optimizer, ParamsAdamW};

use crate::net::GpuValueNet;

/// Numerically stable `softplus(x) = ln(1 + e^x)` as `max(x,0) + ln(1 +
/// e^{-|x|})`, batched -- the same rewrite `train.rs::softplus_stable` uses
/// for the same overflow reason (`x.exp()` overflows `f64` past ~709; this
/// form never evaluates `exp` of anything positive).
pub fn softplus_stable(x: &Tensor) -> CResult<Tensor> {
    let relu_x = x.maximum(0f64)?;
    let log1p = ((x.abs()?.neg()?.exp()? + 1.0)?).log()?;
    relu_x + log1p
}

/// The Bradley-Terry pairwise ranking loss, batched: `softplus(vb - va)` for
/// each pair -- pushes the CHOSEN sibling's value `va` above the REJECTED
/// sibling's `vb`, exactly `train.rs::rank_pair_loss`'s loss term (that
/// function also hand-derives the gradient; here candle's autograd takes
/// the derivative, so only the loss expression itself needs to match).
pub fn rank_pair_loss_batch(va: &Tensor, vb: &Tensor) -> CResult<Tensor> {
    let z = (vb - va)?;
    softplus_stable(&z)
}

/// `torch.nn.functional.smooth_l1_loss(pred, target, beta=1.0)`, batched
/// (quadratic for `|diff| < 1`, linear beyond -- Huber loss): the same
/// expression `train.rs::smooth_l1` computes for the value head, and
/// `neural_train_rank.py`'s own CODE (its docstring says "MSE"; the code is
/// smooth-L1 -- `train.rs`'s top doc comment already flags this
/// docstring/code mismatch, and this port follows the code, same as that
/// one did).
pub fn smooth_l1_loss_batch(pred: &Tensor, target: &Tensor) -> CResult<Tensor> {
    let diff = (pred - target)?;
    let abs = diff.abs()?;
    let quad = (diff.sqr()? * 0.5)?;
    let lin = (abs.affine(1.0, -0.5))?;
    let is_quad = abs.lt(1f64)?;
    is_quad.where_cond(&quad, &lin)
}

/// Owns the GPU network and its `AdamW` optimizer state. One [`Trainer`]
/// per run, matching `train.rs::Trainer`'s own lifetime.
pub struct Trainer {
    pub net: GpuValueNet,
    optim: AdamW,
}

impl Trainer {
    pub fn new(net: GpuValueNet, lr: f64, wd: f64) -> CResult<Trainer> {
        let params = ParamsAdamW { lr, beta1: 0.9, beta2: 0.999, eps: 1e-8, weight_decay: wd };
        let optim = AdamW::new(net.vars(), params)?;
        Ok(Trainer { net, optim })
    }

    pub fn set_lr(&mut self, lr: f64) {
        self.optim.set_learning_rate(lr);
    }

    /// One training step over one minibatch: forward both sides of every
    /// ranking pair and (if given) the value-anchor batch, combine into
    /// `lam * rank_loss.mean() + vweight * value_loss.mean()` (matching
    /// `neuraltrain.rs`'s own combination, which mirrors `neural_train_
    /// rank.py`), then `backward_step` -- one `loss.backward()` plus one
    /// `AdamW::step` for the whole batch, same granularity as `train.rs`'s
    /// "one AdamW step per minibatch, not per row" (see that file's own
    /// comment on why stepping per row was an earlier, measured bug there).
    ///
    /// Returns `(mean_rank_loss, mean_value_loss)`, the UNSCALED means (not
    /// multiplied by `lam`/`vweight`) -- what `main.rs` accumulates for the
    /// per-epoch `rank`/`vloss` report, matching `neuraltrain.rs`'s own
    /// `tot_r`/`tot_v` running averages.
    pub fn train_step(&mut self, chosen: &Tensor, rejected: &Tensor, value_x: Option<&Tensor>, value_y: Option<&Tensor>, lam: f64, vweight: f64, dropout_p: f64) -> CResult<(f64, f64)> {
        let va = self.net.forward(chosen, true, dropout_p)?;
        let vb = self.net.forward(rejected, true, dropout_p)?;
        let rank_losses = rank_pair_loss_batch(&va, &vb)?;
        let rank_mean = rank_losses.mean_all()?;
        let mut total = rank_mean.affine(lam, 0.0)?;

        let vloss_mean_f64 = match (value_x, value_y) {
            (Some(vx), Some(vy)) => {
                let pred = self.net.forward(vx, true, dropout_p)?;
                let vlosses = smooth_l1_loss_batch(&pred, vy)?;
                let vmean = vlosses.mean_all()?;
                let vloss_f64 = vmean.to_scalar::<f64>()?;
                total = (total + vmean.affine(vweight, 0.0)?)?;
                vloss_f64
            }
            _ => 0.0,
        };

        let rank_mean_f64 = rank_mean.to_scalar::<f64>()?;
        self.optim.backward_step(&total)?;
        Ok((rank_mean_f64, vloss_mean_f64))
    }
}

// ===================================================================== tests

#[cfg(test)]
mod tests {
    use super::*;
    use candle_core::Device;

    /// `smooth_l1_loss_batch`/`rank_pair_loss_batch` against HAND-WORKED
    /// values on a fixed small batch, following `net.rs`'s own precedent
    /// for a module with no external oracle to check against (its top doc
    /// comment: "this module's own `#[cfg(test)]` block instead pins the
    /// arithmetic directly ... against values worked out by hand"). This
    /// crate cannot import `train.rs::smooth_l1`/`rank_pair_loss` to
    /// compare against directly -- they are `pub(crate)` *within* the `tta`
    /// crate, and `train.rs` is one of the three files this port is
    /// explicitly required to leave untouched (owned by a concurrent
    /// CPU-multithreading change) -- so instead this test recomputes the
    /// formula those functions' own doc comments spell out (Huber loss,
    /// `beta=1`; Bradley-Terry `softplus(vb-va)`) independently in plain
    /// `f64`, and checks candle's tensor computation against THAT, the same
    /// "worked out by hand" role `net.rs`'s hand-computed LayerNorm example
    /// plays for arithmetic with no live reference implementation to call.
    #[test]
    fn losses_match_hand_computed_values_on_a_fixed_batch() {
        let device = Device::Cpu;
        // Chosen picked deliberately to exercise smooth_l1's quadratic AND
        // linear branches, and rank_pair_loss's both signs of `va - vb`.
        let pred = vec![0.2_f64, 1.5, -0.6, 2.4];
        let target = vec![0.0_f64, 0.0, 0.0, 0.0];
        let va = vec![1.0_f64, -0.5, 0.0, 3.0];
        let vb = vec![0.5_f64, 0.5, 0.0, -1.0];

        let hand_smooth_l1 = |p: f64, t: f64| -> f64 {
            let d = p - t;
            if d.abs() < 1.0 {
                0.5 * d * d
            } else {
                d.abs() - 0.5
            }
        };
        let hand_softplus = |z: f64| -> f64 { z.max(0.0) + (1.0 + (-z.abs()).exp()).ln() };
        let hand_rank = |a: f64, b: f64| -> f64 { hand_softplus(b - a) };

        let want_l1: Vec<f64> = pred.iter().zip(target.iter()).map(|(&p, &t)| hand_smooth_l1(p, t)).collect();
        let want_rank: Vec<f64> = va.iter().zip(vb.iter()).map(|(&a, &b)| hand_rank(a, b)).collect();

        let pred_t = Tensor::from_vec(pred.clone(), pred.len(), &device).unwrap();
        let target_t = Tensor::from_vec(target.clone(), target.len(), &device).unwrap();
        let got_l1: Vec<f64> = smooth_l1_loss_batch(&pred_t, &target_t).unwrap().to_vec1().unwrap();

        let va_t = Tensor::from_vec(va.clone(), va.len(), &device).unwrap();
        let vb_t = Tensor::from_vec(vb.clone(), vb.len(), &device).unwrap();
        let got_rank: Vec<f64> = rank_pair_loss_batch(&va_t, &vb_t).unwrap().to_vec1().unwrap();

        for (g, w) in got_l1.iter().zip(want_l1.iter()) {
            assert!((g - w).abs() < 1e-12, "smooth_l1: got={g} want={w}");
        }
        for (g, w) in got_rank.iter().zip(want_rank.iter()) {
            assert!((g - w).abs() < 1e-12, "rank_pair_loss: got={g} want={w}");
        }
    }

    /// `softplus_stable` must stay finite at extremes the naive `(1+e^x).ln()`
    /// would overflow at -- same property `train.rs::softplus_stable_
    /// derivative_matches_finite_differences_and_stays_finite_at_extremes`
    /// pins on the CPU side.
    #[test]
    fn softplus_stable_is_finite_at_extremes_and_matches_the_linear_asymptote() {
        let device = Device::Cpu;
        let x = Tensor::from_vec(vec![1e6_f64, -1e6, 0.0], 3, &device).unwrap();
        let y: Vec<f64> = softplus_stable(&x).unwrap().to_vec1().unwrap();
        assert!(y.iter().all(|v| v.is_finite()), "{y:?}");
        assert!((y[0] - 1e6).abs() < 1.0, "softplus(1e6) should be ~1e6, got {}", y[0]);
        assert!(y[1] < 1e-9, "softplus(-1e6) should be ~0, got {}", y[1]);
        assert!((y[2] - 2f64.ln()).abs() < 1e-9, "softplus(0) should be ln(2), got {}", y[2]);
    }

    /// End-to-end sanity: one `train_step` on a tiny random-initialised net
    /// must actually move the loss (not NaN, not unchanged) -- the same
    /// "the optimizer wiring works at all" role `train.rs`'s own
    /// `trainer_end_to_end...` smoke test plays for the CPU path.
    #[test]
    fn one_train_step_on_a_tiny_net_produces_finite_decreasing_loss() {
        let device = Device::Cpu;
        let cpu_net = tta::bots::neural::train::random_init(3, 4, 1, 20260806);
        let gpu_net = GpuValueNet::from_cpu(&cpu_net, &device).unwrap();
        let mut trainer = Trainer::new(gpu_net, 1e-2, 0.0).unwrap();

        let chosen = Tensor::from_vec(vec![0.5_f64, -0.2, 1.0, 0.1, 0.3, -0.4], (2, 3), &device).unwrap();
        let rejected = Tensor::from_vec(vec![-0.5_f64, 0.2, -1.0, -0.1, -0.3, 0.4], (2, 3), &device).unwrap();

        let (r0, _) = trainer.train_step(&chosen, &rejected, None, None, 1.0, 1.0, 0.0).unwrap();
        assert!(r0.is_finite());
        let mut last = r0;
        for _ in 0..20 {
            let (r, _) = trainer.train_step(&chosen, &rejected, None, None, 1.0, 1.0, 0.0).unwrap();
            assert!(r.is_finite(), "loss went non-finite");
            last = r;
        }
        assert!(last < r0, "20 gradient steps on a fixed, separable batch should reduce the ranking loss: {r0} -> {last}");
    }
}
