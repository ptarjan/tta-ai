//! `neuraltrain` -- train the value net with the combined value+ranking
//! objective, entirely in Rust (no torch). Rust port of
//! `experiments/neural_train_rank.py` (244 lines); see
//! `rust/src/bots/neural/train.rs`'s top doc comment for the loss shape and
//! why this binary prints an epoch-0 VACUITY line before the first
//! gradient step.
//!
//! ```text
//! neuraltrain --data rankdata/rk.0000.rkd --data rankdata/rk.0001.rkd \
//!     --epochs 25 --lam 1.0 --out checkpoints/value_rank.ckpt
//! ```
//!
//! `--data` may be repeated and each value may be a single `.rkd` shard OR
//! a directory (every `*.rkd` file inside it, sorted, is loaded) -- there is
//! no glob crate here (`Cargo.toml` stays empty), so a directory argument is
//! this binary's equivalent of the Python script's `glob.glob` shell
//! pattern.
//!
//! Flags mirror the script's own: `--data --epochs --batch --lr --wd
//! --hidden --blocks --dropout --lam --vweight --init --out`. Three of the
//! script's flags are intentionally NOT exposed here and are fixed at its
//! own defaults instead (`--select` always `last` -- the value the script's
//! own `--select` help text says is now the right default, see
//! `docs/NEURAL_LOOP_NULL.md`; `--val-frac` fixed at `0.15`; `--val-split`
//! fixed at `rows`, the same default): the smaller flag surface keeps this
//! binary's job to "does the loop reproduce, end to end" rather than
//! re-exposing every knob the null run's post-mortem already recommends
//! against reaching for.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use tta::bots::neural::encode::ENCODING_DIM;
use tta::bots::neural::net::{load_checkpoint, save_checkpoint, MARGIN_NORM};
use tta::bots::neural::rankdata::{read_shard, RankPair, ValueRow};
use tta::bots::neural::train::{predict, random_init, Trainer};
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
    /// Worker threads the batched forward/backward pass splits each
    /// minibatch across (`Trainer::accumulate_pair_batch`/
    /// `accumulate_value_batch`, `std::thread::scope`). The box this runs
    /// on has 6 physical cores and no hyperthreading, and a hill-climbing
    /// league is ALSO running on it at higher priority -- default to
    /// something that leaves headroom rather than claiming every core.
    threads: usize,
    /// Fraction of rows held out for validation, a random ROW split (the
    /// script's `rows` default -- see this file's top doc comment for why
    /// `shards` is not offered here). Not a flag; fixed at the script's own
    /// default.
    val_frac: f64,
    /// If the untrained warm start already orders this fraction of TRAIN
    /// pairs correctly, the ranking target is a fixed point and training
    /// cannot learn anything from it (`docs/NEURAL_LOOP_NULL.md` 3.1).
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
            threads: 4,
            val_frac: 0.15,
            vacuity_warn: 0.95,
        }
    }
}

const USAGE: &str = "\
usage: neuraltrain --data PATH [--data PATH ...] [options]

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
  --vweight F      value-loss weight -- raise it to pin the output SCALE;
                    the ranking loss is scale-hungry and decalibrates the
                    value head at vweight=1 (default 1.0)
  --init PATH      warm-start checkpoint (this crate's own format, not a
                    .pt file -- see net.rs)
  --out PATH       where to write the checkpoint every epoch (default
                    checkpoints/value_rank.ckpt)
  --threads N      worker threads the batched forward/backward pass splits
                    each minibatch across (default 4 -- this box has 6
                    physical cores and a higher-priority league also runs
                    on it, so this deliberately leaves headroom rather than
                    defaulting to 6)
  --help
";

fn parse_args(argv: &[String]) -> Result<Option<Args>, String> {
    let mut a = Args::default();
    let mut it = argv.iter();
    while let Some(flag) = it.next() {
        let mut value = |flag: &str| -> Result<String, String> {
            it.next().cloned().ok_or_else(|| format!("{flag} needs a value"))
        };
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
            "--threads" => a.threads = parse_num(&value(flag)?, flag)?,
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
    if a.threads == 0 {
        return Err("--threads must be at least 1".to_string());
    }
    if !(0.0..1.0).contains(&a.dropout) {
        return Err(format!("--dropout must be in [0, 1), got {}", a.dropout));
    }
    Ok(Some(a))
}

fn parse_num<T: std::str::FromStr>(s: &str, flag: &str) -> Result<T, String> {
    s.parse().map_err(|_| format!("{flag}: {s:?} is not a number"))
}

/// Expand `--data` arguments into a sorted, de-duplicated list of shard
/// files: a file argument is used as-is, a directory argument contributes
/// every `*.rkd` file inside it. This crate's own stand-in for the
/// script's `glob.glob(pattern)` -- there is no glob crate here.
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

// ================================================================= data prep

/// Fisher-Yates over `v`, matching `rankdata.rs`'s own `shuffle_indices`
/// (not re-exported from there; duplicating four lines beats reaching into
/// another module's private helper for something this small).
fn shuffle_indices(rng: &mut PyRandom, v: &mut [usize]) {
    for i in (1..v.len()).rev() {
        let j = ((rng.random() * (i + 1) as f64) as usize).min(i);
        v.swap(i, j);
    }
}

/// A random ROW split (this binary's only mode -- see the top doc comment
/// on why `shards` is not offered): shuffle `n` indices with a FIXED seed
/// (so a rerun on the same data reproduces the same split) and peel off
/// the first `val_frac` fraction as validation.
fn split_rows(n: usize, val_frac: f64, seed: i64) -> (Vec<usize>, Vec<usize>) {
    let mut idx: Vec<usize> = (0..n).collect();
    let mut rng = PyRandom::new(seed);
    shuffle_indices(&mut rng, &mut idx);
    let k = ((n as f64) * val_frac).round() as usize;
    let k = k.clamp(if n > 0 { 1 } else { 0 }, n);
    let (val, train) = idx.split_at(k);
    (train.to_vec(), val.to_vec())
}

/// `pair_acc` (fraction of validation pairs the net orders correctly) and
/// `val_mae` (mean absolute error of validation value rows, in CULTURE
/// units) -- `neural_train_rank.py`'s `evaluate_val`.
fn evaluate_val(trainer: &Trainer, val_pairs: &[&RankPair], val_values: &[&ValueRow]) -> (f64, f64) {
    let correct = val_pairs
        .iter()
        .filter(|p| {
            let xa: Vec<f64> = p.chosen.iter().map(|&v| v as f64).collect();
            let xb: Vec<f64> = p.rejected.iter().map(|&v| v as f64).collect();
            predict(&trainer.net, &xa) > predict(&trainer.net, &xb)
        })
        .count();
    let pair_acc = if val_pairs.is_empty() { 0.0 } else { correct as f64 / val_pairs.len() as f64 };

    let mae: f64 = val_values
        .iter()
        .map(|row| {
            let x: Vec<f64> = row.state.iter().map(|&v| v as f64).collect();
            let pred_culture = predict(&trainer.net, &x) * MARGIN_NORM;
            (pred_culture - row.margin as f64).abs()
        })
        .sum();
    let val_mae = if val_values.is_empty() { 0.0 } else { mae / val_values.len() as f64 };
    (pair_acc, val_mae)
}

/// `torch.optim.lr_scheduler.CosineAnnealingLR(opt, T_max=epochs)`'s
/// default `eta_min=0` schedule, evaluated at (zero-indexed) epoch `t`:
/// `lr(t) = base_lr * (1 + cos(pi*t/T_max)) / 2`.
fn cosine_lr(base_lr: f64, t: usize, t_max: usize) -> f64 {
    if t_max <= 1 {
        return base_lr;
    }
    base_lr * (1.0 + (std::f64::consts::PI * t as f64 / t_max as f64).cos()) / 2.0
}

// ====================================================================== main

fn run(args: &Args) -> Result<(), String> {
    let files = expand_data_paths(&args.data)?;
    let mut pairs: Vec<RankPair> = Vec::new();
    let mut values: Vec<ValueRow> = Vec::new();
    for f in &files {
        let shard = read_shard(f)?;
        pairs.extend(shard.pairs);
        values.extend(shard.values);
    }
    println!("{} shards -> random ROW split {:.2}", files.len(), args.val_frac);

    let (train_pair_idx, val_pair_idx) = split_rows(pairs.len(), args.val_frac, 12345);
    let (train_val_idx, val_val_idx) = split_rows(values.len(), args.val_frac, 12346);
    println!(
        "train pairs {}  val pairs {}  train vals {}  dim {}",
        train_pair_idx.len(),
        val_pair_idx.len(),
        train_val_idx.len(),
        ENCODING_DIM
    );
    let val_pairs: Vec<&RankPair> = val_pair_idx.iter().map(|&i| &pairs[i]).collect();
    let val_values: Vec<&ValueRow> = val_val_idx.iter().map(|&i| &values[i]).collect();

    let net = match &args.init {
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
        None => random_init(ENCODING_DIM, args.hidden, args.blocks, 20260806),
    };
    let mut trainer = Trainer::new(net, args.dropout, args.lr, args.wd, 20260807, args.batch, args.threads);

    if let Some(dir) = args.out.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
    }

    // --- epoch 0: the vacuity check ------------------------------------
    // docs/NEURAL_LOOP_NULL.md 3.1: a target already satisfied by the
    // UNTRAINED net is a fixed point, not something gradient descent can
    // learn from. Printed before the first gradient step, unconditionally
    // -- see train.rs's top doc comment for why this line exists at all.
    let (pa0, mae0) = evaluate_val(&trainer, &val_pairs, &val_values);
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
        // Confusingly named in the Python script too (`Xv_t`/`vperm`): this
        // is the TRAIN value-row order, cycled alongside the pair batches --
        // not the held-out validation set (`val_pairs`/`val_values` above).
        let mut train_value_order = train_val_idx.clone();
        shuffle_indices(&mut shuffle_rng, &mut train_value_order);

        let (mut tot_r, mut tot_v, mut nb) = (0.0, 0.0, 0usize);
        let mut vcursor = 0usize;
        let mut i = 0usize;
        while i < pair_order.len() {
            let end = (i + args.batch).min(pair_order.len());
            let bsz = end - i;

            // One AdamW step per MINIBATCH, not per row: `zero_grad` once,
            // accumulate the WHOLE minibatch's (already `1/bsz`-scaled)
            // gradient contribution via the batched, multi-threaded
            // `accumulate_*_batch` (see `train.rs`'s own doc comment on
            // why row-at-a-time forward/backward was the measured
            // bottleneck), THEN step once -- matching
            // `opt.zero_grad(); loss.backward(); opt.step()` running once
            // per batch in the Python script. Stepping per row was an
            // earlier bug here: AdamW's own update touches every one of
            // the net's ~700k parameters regardless of batch size, so
            // calling it once per ROW instead of once per BATCH made the
            // optimizer, not the forward/backward math, the dominant cost
            // -- roughly `batch` times more O(params) work than intended.
            trainer.zero_grad();
            let pair_refs: Vec<&RankPair> = pair_order[i..end].iter().map(|&pi| &pairs[pi]).collect();
            let r_sum = trainer.accumulate_pair_batch(&pair_refs, args.lam, bsz);

            let mut v_sum = 0.0;
            let mut v_count = 0usize;
            if !train_value_order.is_empty() {
                // Same batch size `bsz` as the pair side -- exactly `bsz`
                // value rows get accumulated into this step (cycling/
                // repeating rows when there are fewer value rows than
                // pairs), so the mean-reduction scale matches.
                let value_refs: Vec<&ValueRow> = (0..bsz)
                    .map(|_| {
                        let vi = train_value_order[vcursor % train_value_order.len()];
                        vcursor += 1;
                        &values[vi]
                    })
                    .collect();
                v_count = value_refs.len();
                v_sum = trainer.accumulate_value_batch(&value_refs, args.vweight, bsz);
            }
            trainer.optim_step();
            tot_r += r_sum / bsz as f64;
            if v_count > 0 {
                tot_v += v_sum / v_count as f64;
            }
            nb += 1;
            i = end;
        }

        let (pair_acc, val_mae) = evaluate_val(&trainer, &val_pairs, &val_values);
        // `--select last` (see this file's top doc comment): every epoch's
        // checkpoint is the new "best" -- the gate that actually decides
        // promotion runs elsewhere (arena/climb), not on this loss.
        best_ep = ep + 1;
        best_pa = pair_acc;
        best_mae = val_mae;
        println!(
            "epoch {:3}  rank {:.4}  vloss {:.4}  val_pair_acc {pair_acc:.4}  val_mae {val_mae:.1}  *best",
            ep + 1,
            tot_r / nb.max(1) as f64,
            tot_v / nb.max(1) as f64
        );
        save_checkpoint(
            &args.out,
            &trainer.net,
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

    println!(
        "best epoch {best_ep}: pair_acc {best_pa:.4} mae {best_mae:.1}; saved {}  ({:.1}s)",
        args.out.display(),
        start.elapsed().as_secs_f64()
    );
    Ok(())
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_args(&argv) {
        Ok(Some(a)) => a,
        Ok(None) => return ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("neuraltrain: {e}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(e) = run(&args) {
        eprintln!("neuraltrain: {e}");
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
    fn args_default_to_the_scripts_own_defaults() {
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
    }

    #[test]
    fn data_flag_is_repeatable() {
        let a = parse_args(&["--data".to_string(), "a".to_string(), "--data".to_string(), "b".to_string()]).unwrap().unwrap();
        assert_eq!(a.data, vec![PathBuf::from("a"), PathBuf::from("b")]);
    }

    #[test]
    fn a_dropout_of_exactly_one_is_rejected() {
        let argv = ["--data".to_string(), "x".to_string(), "--dropout".to_string(), "1.0".to_string()];
        assert!(parse_args(&argv).is_err());
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
        assert!(!val.is_empty(), "val split must not be empty for n=20 val_frac=0.15");
    }

    #[test]
    fn split_rows_is_reproducible_for_the_same_seed() {
        let (t1, v1) = split_rows(50, 0.15, 7);
        let (t2, v2) = split_rows(50, 0.15, 7);
        assert_eq!(t1, t2);
        assert_eq!(v1, v2);
    }

    #[test]
    fn expand_data_paths_rejects_a_directory_with_no_shards() {
        let dir = std::env::temp_dir().join(format!("neuraltrain_test_empty_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let err = expand_data_paths(&[dir.clone()]).unwrap_err();
        assert!(err.contains("no .rkd"), "{err}");
        std::fs::remove_dir(&dir).ok();
    }
}
