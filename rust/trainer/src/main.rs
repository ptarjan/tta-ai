//! `gputrain` -- train `tta`'s value net on a GPU (or CPU) with candle's
//! autograd, driving the SAME combined value+ranking objective `neuraltrain`
//! trains by hand on the CPU (`rust/src/bots/neural/train.rs`'s top doc
//! comment has the objective's shape and the reason epoch 0 gets a VACUITY
//! report before any gradient step -- `docs/NEURAL.md`'s 41-hour
//! null). Flags mirror `neuraltrain`'s: `--data --epochs --batch --lr --wd
//! --hidden --blocks --dropout --lam --vweight --init --out`, plus
//! `--device` (`cpu` by default; `cuda` needs this crate built with
//! `--features cuda`, and a CUDA toolkit -- see `--help`).
//!
//! ```text
//! gputrain --data rankdata/rk.0000.rkd --epochs 25 --lam 1.0 \
//!     --out checkpoints/value_rank.ckpt
//! ```
//!
//! Same reduced flag surface as `neuraltrain` for the same reason (that
//! binary's own top doc comment): `--select` is always `last`, `--val-frac`
//! fixed at `0.15`, split mode fixed at a random ROW split. Also fixed: the
//! vacuity warning threshold (`0.95`, `neuraltrain`'s own default).

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use candle_core::{Device, Tensor};

use gputrain::net::GpuValueNet;
use gputrain::train::Trainer;
use tta::bots::neural::encode::ENCODING_DIM;
use tta::bots::neural::net::{load_checkpoint, save_checkpoint, MARGIN_NORM};
use tta::bots::neural::rankdata::{read_shard, RankPair, ValueRow};
use tta::bots::neural::train::random_init;
use tta::rng::PyRandom;

// ====================================================================== args

#[derive(Clone, Debug)]
struct Args {
    data: Vec<PathBuf>,
    epochs: usize,
    batch: usize,
    lr: f64,
    wd: f64,
    hidden: usize,
    blocks: usize,
    dropout: f64,
    lam: f64,
    vweight: f64,
    init: Option<PathBuf>,
    out: PathBuf,
    /// `cpu` (default -- always available) or `cuda` (needs this crate
    /// built with `--features cuda` AND a CUDA toolkit; see `run`'s device
    /// selection for the exact failure message when either is missing).
    device: String,
    val_frac: f64,
    vacuity_warn: f64,
}

impl Default for Args {
    fn default() -> Args {
        Args {
            data: Vec::new(),
            epochs: 25,
            batch: 4096,
            lr: 1e-3,
            wd: 2e-4,
            hidden: 256,
            blocks: 3,
            dropout: 0.15,
            lam: 1.0,
            vweight: 1.0,
            init: None,
            out: PathBuf::from("checkpoints/value_rank.ckpt"),
            device: "cpu".to_string(),
            val_frac: 0.15,
            vacuity_warn: 0.95,
        }
    }
}

const USAGE: &str = "\
usage: gputrain --data PATH [--data PATH ...] [options]

  --data PATH      a .rkd shard file, or a directory of them (repeatable,
                    required)
  --epochs N       (default 25)
  --batch N        minibatch size, for both the ranking pairs and the
                    cycled value rows (default 4096)
  --lr F           AdamW learning rate, cosine-annealed over --epochs
                    (default 1e-3)
  --wd F           AdamW decoupled weight decay (default 2e-4)
  --hidden N       hidden width; ignored (must match) when --init is given
                    (default 256)
  --blocks N       residual block count; ignored (must match) when --init
                    is given (default 3)
  --dropout F      dropout probability inside each residual block
                    (default 0.15)
  --lam F          ranking-loss weight (default 1.0)
  --vweight F      value-loss weight (default 1.0)
  --init PATH      warm-start checkpoint (this crate's own format, not a
                    .pt file -- see net.rs)
  --out PATH       where to write the checkpoint every epoch (default
                    checkpoints/value_rank.ckpt)
  --device D       cpu (default) or cuda -- cuda needs this binary built
                    with `cargo build --features cuda` AND a CUDA toolkit
                    (nvcc) on the build machine, not just an NVIDIA driver
  --help
";

fn parse_args(argv: &[String]) -> Result<Option<Args>, String> {
    let mut a = Args::default();
    let mut it = argv.iter();
    while let Some(flag) = it.next() {
        let mut value = |flag: &str| -> Result<String, String> { it.next().cloned().ok_or_else(|| format!("{flag} needs a value")) };
        match flag.as_str() {
            "--data" => a.data.push(PathBuf::from(value(flag)?)),
            "--epochs" => a.epochs = parse_num(&value(flag)?, flag)?,
            "--batch" => a.batch = parse_num(&value(flag)?, flag)?,
            "--lr" => a.lr = parse_num(&value(flag)?, flag)?,
            "--wd" => a.wd = parse_num(&value(flag)?, flag)?,
            "--hidden" => a.hidden = parse_num(&value(flag)?, flag)?,
            "--blocks" => a.blocks = parse_num(&value(flag)?, flag)?,
            "--dropout" => a.dropout = parse_num(&value(flag)?, flag)?,
            "--lam" => a.lam = parse_num(&value(flag)?, flag)?,
            "--vweight" => a.vweight = parse_num(&value(flag)?, flag)?,
            "--init" => a.init = Some(PathBuf::from(value(flag)?)),
            "--out" => a.out = PathBuf::from(value(flag)?),
            "--device" => a.device = value(flag)?,
            "--help" | "-h" => {
                print!("{USAGE}");
                return Ok(None);
            }
            other => return Err(format!("unknown flag {other}\n\n{USAGE}")),
        }
    }
    if a.data.is_empty() {
        return Err(format!("--data is required (at least one shard or directory)\n\n{USAGE}"));
    }
    if a.epochs == 0 {
        return Err("--epochs must be at least 1".to_string());
    }
    if a.batch == 0 {
        return Err("--batch must be at least 1".to_string());
    }
    if !(0.0..1.0).contains(&a.dropout) {
        return Err(format!("--dropout must be in [0, 1), got {}", a.dropout));
    }
    if a.device != "cpu" && a.device != "cuda" {
        return Err(format!("--device must be cpu or cuda, got {:?}", a.device));
    }
    Ok(Some(a))
}

fn parse_num<T: std::str::FromStr>(s: &str, flag: &str) -> Result<T, String> {
    s.parse().map_err(|_| format!("{flag}: {s:?} is not a number"))
}

/// Same job as `neuraltrain.rs::expand_data_paths` (this crate's own stand-in
/// for `glob.glob`, since neither crate takes a glob dependency): a file
/// argument is used as-is, a directory argument contributes every `*.rkd`
/// file inside it, sorted and de-duplicated.
fn expand_data_paths(data: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut files: Vec<PathBuf> = Vec::new();
    for p in data {
        let meta = std::fs::metadata(p).map_err(|e| format!("{}: {e}", p.display()))?;
        if meta.is_dir() {
            let mut entries: Vec<PathBuf> = std::fs::read_dir(p)
                .map_err(|e| format!("{}: {e}", p.display()))?
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|ext| ext == "rkd"))
                .collect();
            entries.sort();
            files.extend(entries);
        } else {
            files.push(p.clone());
        }
    }
    files.sort();
    files.dedup();
    if files.is_empty() {
        return Err("--data matched no .rkd files".to_string());
    }
    Ok(files)
}

fn shuffle_indices(rng: &mut PyRandom, v: &mut [usize]) {
    for i in (1..v.len()).rev() {
        let j = ((rng.random() * (i + 1) as f64) as usize).min(i);
        v.swap(i, j);
    }
}

/// A random ROW split with a FIXED seed, identical in shape to
/// `neuraltrain.rs::split_rows` (same seeds too, `12345`/`12346`, passed by
/// the caller below) -- so running both trainers over the same shards uses
/// the same train/val partition, which is what makes their reported
/// `val_pair_acc`/`val_mae` comparable at all.
fn split_rows(n: usize, val_frac: f64, seed: i64) -> (Vec<usize>, Vec<usize>) {
    let mut idx: Vec<usize> = (0..n).collect();
    let mut rng = PyRandom::new(seed);
    shuffle_indices(&mut rng, &mut idx);
    let k = ((n as f64) * val_frac).round() as usize;
    let k = k.clamp(if n > 0 { 1 } else { 0 }, n);
    let (val, train) = idx.split_at(k);
    (train.to_vec(), val.to_vec())
}

/// `torch.optim.lr_scheduler.CosineAnnealingLR(opt, T_max=epochs)`'s default
/// `eta_min=0` schedule -- identical to `neuraltrain.rs::cosine_lr`.
fn cosine_lr(base_lr: f64, t: usize, t_max: usize) -> f64 {
    if t_max <= 1 {
        return base_lr;
    }
    base_lr * (1.0 + (std::f64::consts::PI * t as f64 / t_max as f64).cos()) / 2.0
}

/// Build a `[batch, ENCODING_DIM]` `f64` tensor from `f32` shard rows,
/// widening in the same pass -- this crate's analogue of `train.rs::
/// widen_into`/`transpose_widen_into`, except candle wants ROW-major
/// `(batch, dim)`, not the CPU trainer's transposed `(dim, batch)` (that
/// layout was a hand-tuned SIMD/cache trick for the scalar loop in
/// `train.rs`; candle's own `matmul` is what plays that role here, so there
/// is nothing to tile by hand).
fn rows_to_tensor(rows: &[&[f32]], dim: usize, device: &Device) -> Result<Tensor, String> {
    let mut flat = Vec::with_capacity(rows.len() * dim);
    for r in rows {
        flat.extend(r.iter().map(|&v| v as f64));
    }
    Tensor::from_vec(flat, (rows.len(), dim), device).map_err(|e| e.to_string())
}

/// `(pair_acc, val_mae)` over the WHOLE validation set in one eval-mode
/// forward pass each (chosen, rejected, values) -- `neuraltrain.rs::
/// evaluate_val`'s GPU analogue. Not chunked: validation sets are the
/// `val_frac` (0.15) tail of a shard, an order of magnitude smaller than a
/// training epoch's total rows, so one batch is not the concern batching
/// exists for during training.
fn evaluate_val(trainer: &Trainer, val_pairs: &[&RankPair], val_values: &[&ValueRow], device: &Device) -> Result<(f64, f64), String> {
    let pair_acc = if val_pairs.is_empty() {
        0.0
    } else {
        let chosen: Vec<&[f32]> = val_pairs.iter().map(|p| p.chosen.as_slice()).collect();
        let rejected: Vec<&[f32]> = val_pairs.iter().map(|p| p.rejected.as_slice()).collect();
        let xa = rows_to_tensor(&chosen, ENCODING_DIM, device)?;
        let xb = rows_to_tensor(&rejected, ENCODING_DIM, device)?;
        let va: Vec<f64> = trainer.net.forward(&xa, false, 0.0).map_err(|e| e.to_string())?.to_vec1().map_err(|e| e.to_string())?;
        let vb: Vec<f64> = trainer.net.forward(&xb, false, 0.0).map_err(|e| e.to_string())?.to_vec1().map_err(|e| e.to_string())?;
        let correct = va.iter().zip(vb.iter()).filter(|(a, b)| a > b).count();
        correct as f64 / val_pairs.len() as f64
    };

    let val_mae = if val_values.is_empty() {
        0.0
    } else {
        let states: Vec<&[f32]> = val_values.iter().map(|r| r.state.as_slice()).collect();
        let x = rows_to_tensor(&states, ENCODING_DIM, device)?;
        let preds: Vec<f64> = trainer.net.forward(&x, false, 0.0).map_err(|e| e.to_string())?.to_vec1().map_err(|e| e.to_string())?;
        let sum: f64 = preds.iter().zip(val_values.iter()).map(|(p, row)| (p * MARGIN_NORM - row.margin as f64).abs()).sum();
        sum / val_values.len() as f64
    };
    Ok((pair_acc, val_mae))
}

// ====================================================================== main

fn select_device(name: &str) -> Result<Device, String> {
    match name {
        "cpu" => Ok(Device::Cpu),
        "cuda" => Device::new_cuda(0).map_err(|e| {
            format!(
                "--device cuda: {e}\n\
                 This needs the crate built with `cargo build --features cuda` AND a CUDA \
                 toolkit (nvcc, cuBLAS headers) on THIS machine at build time -- an NVIDIA \
                 driver alone (what `nvidia-smi` reports) is not enough."
            )
        }),
        other => Err(format!("--device must be cpu or cuda, got {other:?}")),
    }
}

fn run(args: &Args) -> Result<(), String> {
    let device = select_device(&args.device)?;

    let files = expand_data_paths(&args.data)?;
    let mut pairs: Vec<RankPair> = Vec::new();
    let mut values: Vec<ValueRow> = Vec::new();
    for f in &files {
        let shard = read_shard(f)?;
        pairs.extend(shard.pairs);
        values.extend(shard.values);
    }
    println!("{} shards -> random ROW split {:.2}  device={}", files.len(), args.val_frac, args.device);

    let (train_pair_idx, val_pair_idx) = split_rows(pairs.len(), args.val_frac, 12345);
    let (train_val_idx, val_val_idx) = split_rows(values.len(), args.val_frac, 12346);
    println!("train pairs {}  val pairs {}  train vals {}  dim {}", train_pair_idx.len(), val_pair_idx.len(), train_val_idx.len(), ENCODING_DIM);
    let val_pairs: Vec<&RankPair> = val_pair_idx.iter().map(|&i| &pairs[i]).collect();
    let val_values: Vec<&ValueRow> = val_val_idx.iter().map(|&i| &values[i]).collect();

    let cpu_net = match &args.init {
        Some(path) => {
            let (net, _meta) = load_checkpoint(path)?;
            if net.in_dim != ENCODING_DIM || net.hidden != args.hidden || net.blocks.len() != args.blocks {
                return Err(format!(
                    "--init {}: shape (in_dim={} hidden={} blocks={}) does not match --hidden {} --blocks {} at encoder width {ENCODING_DIM}",
                    path.display(),
                    net.in_dim,
                    net.hidden,
                    net.blocks.len(),
                    args.hidden,
                    args.blocks
                ));
            }
            println!("warm-started from {}", path.display());
            net
        }
        // Reuses train.rs's OWN init scheme (`random_init`) rather than a
        // second one invented for candle -- see net.rs's `from_cpu` doc
        // comment.
        None => random_init(ENCODING_DIM, args.hidden, args.blocks, 20260806),
    };
    let gpu_net = GpuValueNet::from_cpu(&cpu_net, &device).map_err(|e| e.to_string())?;
    let mut trainer = Trainer::new(gpu_net, args.lr, args.wd).map_err(|e| e.to_string())?;

    if let Some(dir) = args.out.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
    }

    // --- epoch 0: the vacuity check -------------------------------------
    // See train.rs's top doc comment / docs/NEURAL.md: a
    // target the UNTRAINED warm start already satisfies is a fixed point,
    // not something gradient descent can learn from. Printed before the
    // first gradient step, unconditionally, same wording as neuraltrain.rs
    // so the two binaries' logs are directly diffable.
    let (pa0, mae0) = evaluate_val(&trainer, &val_pairs, &val_values, &device)?;
    println!("epoch   0  (warm start, UNTRAINED)          val_pair_acc {pa0:.4}  val_mae {mae0:.1}");
    println!("VACUITY pair_acc_at_epoch0={pa0:.4} threshold={:.2}", args.vacuity_warn);
    if pa0 >= args.vacuity_warn {
        println!(
            "*** VACUOUS TARGET: the warm-start already satisfies {:.1}% of the ranking pairs. \
             The label is a fixed point of the model being trained -- gradient descent can only \
             inflate margins it already holds. Do not trust anything downstream of this run. ***",
            pa0 * 100.0
        );
    }

    let mut shuffle_rng = PyRandom::new(20260808);
    let mut best_ep = 0usize;
    let (mut best_pa, mut best_mae) = (0.0, 0.0);
    let start = Instant::now();

    for ep in 0..args.epochs {
        trainer.set_lr(cosine_lr(args.lr, ep, args.epochs));

        let mut pair_order = train_pair_idx.clone();
        shuffle_indices(&mut shuffle_rng, &mut pair_order);
        let mut train_value_order = train_val_idx.clone();
        shuffle_indices(&mut shuffle_rng, &mut train_value_order);

        let (mut tot_r, mut tot_v, mut nb) = (0.0, 0.0, 0usize);
        let mut vcursor = 0usize;
        let mut i = 0usize;
        while i < pair_order.len() {
            let end = (i + args.batch).min(pair_order.len());
            let chosen: Vec<&[f32]> = pair_order[i..end].iter().map(|&pi| pairs[pi].chosen.as_slice()).collect();
            let rejected: Vec<&[f32]> = pair_order[i..end].iter().map(|&pi| pairs[pi].rejected.as_slice()).collect();
            let bsz = chosen.len();
            let xa = rows_to_tensor(&chosen, ENCODING_DIM, &device)?;
            let xb = rows_to_tensor(&rejected, ENCODING_DIM, &device)?;

            let (vx, vy) = if train_value_order.is_empty() {
                (None, None)
            } else {
                let states: Vec<&[f32]> = (0..bsz)
                    .map(|_| {
                        let vi = train_value_order[vcursor % train_value_order.len()];
                        vcursor += 1;
                        values[vi].state.as_slice()
                    })
                    .collect();
                let targets: Vec<f64> = (vcursor - bsz..vcursor).map(|c| values[train_value_order[c % train_value_order.len()]].margin as f64 / MARGIN_NORM).collect();
                let x = rows_to_tensor(&states, ENCODING_DIM, &device)?;
                let y = Tensor::from_vec(targets, bsz, &device).map_err(|e| e.to_string())?;
                (Some(x), Some(y))
            };

            let (r_mean, v_mean) = trainer.train_step(&xa, &xb, vx.as_ref(), vy.as_ref(), args.lam, args.vweight, args.dropout).map_err(|e| e.to_string())?;
            tot_r += r_mean;
            if vx.is_some() {
                tot_v += v_mean;
            }
            nb += 1;
            i = end;
        }

        let (pair_acc, val_mae) = evaluate_val(&trainer, &val_pairs, &val_values, &device)?;
        best_ep = ep + 1;
        best_pa = pair_acc;
        best_mae = val_mae;
        println!("epoch {:3}  rank {:.4}  vloss {:.4}  val_pair_acc {pair_acc:.4}  val_mae {val_mae:.1}  *best", ep + 1, tot_r / nb.max(1) as f64, tot_v / nb.max(1) as f64);

        let cpu_view = trainer.net.to_cpu().map_err(|e| e.to_string())?;
        save_checkpoint(
            &args.out,
            &cpu_view,
            &[
                ("val_pair_acc", pair_acc),
                ("val_mae_culture", val_mae),
                ("epoch", (ep + 1) as f64),
                ("lam", args.lam),
                ("vweight", args.vweight),
                ("hidden", args.hidden as f64),
                ("blocks", args.blocks as f64),
            ],
        )?;
    }

    println!("best epoch {best_ep}: pair_acc {best_pa:.4} mae {best_mae:.1}; saved {}  ({:.1}s)", args.out.display(), start.elapsed().as_secs_f64());
    Ok(())
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_args(&argv) {
        Ok(Some(a)) => a,
        Ok(None) => return ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("gputrain: {e}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(e) = run(&args) {
        eprintln!("gputrain: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_require_at_least_one_data_path() {
        assert!(parse_args(&[]).unwrap_err().contains("--data"));
    }

    #[test]
    fn args_default_to_the_same_defaults_neuraltrain_uses() {
        let a = parse_args(&["--data".to_string(), "x".to_string()]).unwrap().unwrap();
        assert_eq!(a.epochs, 25);
        assert_eq!(a.batch, 4096);
        assert!((a.lr - 1e-3).abs() < 1e-12);
        assert!((a.wd - 2e-4).abs() < 1e-12);
        assert_eq!(a.hidden, 256);
        assert_eq!(a.blocks, 3);
        assert!((a.dropout - 0.15).abs() < 1e-12);
        assert!((a.lam - 1.0).abs() < 1e-12);
        assert!((a.vweight - 1.0).abs() < 1e-12);
        assert_eq!(a.device, "cpu");
    }

    #[test]
    fn data_flag_is_repeatable() {
        let a = parse_args(&["--data".to_string(), "a".to_string(), "--data".to_string(), "b".to_string()]).unwrap().unwrap();
        assert_eq!(a.data, vec![PathBuf::from("a"), PathBuf::from("b")]);
    }

    #[test]
    fn an_unknown_device_is_rejected_at_parse_time_not_at_run_time() {
        let argv = ["--data".to_string(), "x".to_string(), "--device".to_string(), "tpu".to_string()];
        assert!(parse_args(&argv).unwrap_err().contains("cpu or cuda"));
    }

    #[test]
    fn zero_epochs_is_rejected_rather_than_running_nothing_silently() {
        let argv = ["--data".to_string(), "x".to_string(), "--epochs".to_string(), "0".to_string()];
        assert!(parse_args(&argv).is_err());
    }

    #[test]
    fn cosine_lr_starts_at_base_and_decays_to_near_zero_at_the_last_epoch() {
        let base = 1e-3;
        let t_max = 10;
        assert!((cosine_lr(base, 0, t_max) - base).abs() < 1e-12);
        assert!(cosine_lr(base, t_max - 1, t_max) < base * 0.1);
        assert!(cosine_lr(base, t_max - 1, t_max) > 0.0);
    }

    #[test]
    fn split_rows_partitions_every_index_exactly_once() {
        let (train, val) = split_rows(20, 0.15, 42);
        assert_eq!(train.len() + val.len(), 20);
        let mut all: Vec<usize> = train.iter().chain(val.iter()).copied().collect();
        all.sort_unstable();
        assert_eq!(all, (0..20).collect::<Vec<_>>());
    }

    #[test]
    fn split_rows_agrees_with_neuraltrains_own_split_for_the_same_seed() {
        // Same seed, same shuffle algorithm (PyRandom Fisher-Yates) as
        // neuraltrain.rs::split_rows -- the two binaries must partition an
        // identical shard into an identical train/val split for their
        // reported val_pair_acc/val_mae to be comparable at all.
        let (t1, v1) = split_rows(50, 0.15, 12345);
        let (t2, v2) = split_rows(50, 0.15, 12345);
        assert_eq!(t1, t2);
        assert_eq!(v1, v2);
        assert!(!v1.is_empty());
    }

    #[test]
    fn select_device_cpu_always_succeeds() {
        assert!(select_device("cpu").is_ok());
    }

    #[test]
    fn expand_data_paths_rejects_a_directory_with_no_shards() {
        let dir = std::env::temp_dir().join(format!("gputrain_test_empty_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let err = expand_data_paths(&[dir.clone()]).unwrap_err();
        assert!(err.contains("no .rkd"), "{err}");
        std::fs::remove_dir(&dir).ok();
    }
}
