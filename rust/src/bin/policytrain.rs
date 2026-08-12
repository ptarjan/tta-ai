//! `policytrain` -- train the policy head (slice 2 of `docs/NEURAL.md`'s
//! planned third net) on a corpus written by `dump_selfplay`, and measure
//! it honestly on a held-out split.
//!
//! ```text
//! policytrain --dump corpus_2p.tpd --dump corpus_3p.tpd --dump corpus_4p.tpd \
//!     --hidden 64 --epochs 3 --lr 0.001 --held-out-frac 0.1 \
//!     --out policy.ckpt
//! ```
//!
//! See `bots::neural::policy_train`'s top doc comment for the architecture
//! (a [`tta::bots::neural::net::ValueNet`] with zero residual blocks, reused
//! as a per-`(state, action)` logit scorer) and the loss (softmax
//! cross-entropy over one decision's legal-action logits only). This binary
//! is: load every `--dump` file, split train/held-out BY GAME
//! ([`split_by_game`]), train, then report held-out top-1/top-3 agreement
//! with the champion's actual choice ALONGSIDE the base rate a uniform
//! random legal move would score -- a bare top-1 number is not a result
//! without knowing how often the legal set was too small to be a real test
//! (see [`held_out_report`]'s own doc comment).

use std::path::PathBuf;
use std::time::Instant;

use tta::bots::neural::dump::read_dump;
use tta::bots::neural::net::ValueNet;
use tta::bots::neural::policy_train::{
    held_out_report, phase_breakdown, random_policy_net, save_policy_checkpoint, split_by_game, train_epoch,
    train_until_early_stop, HeldOutReport, PolicyTrainer,
};
use tta::rng::PyRandom;

#[derive(Clone, Debug)]
struct Args {
    dumps: Vec<PathBuf>,
    hidden: usize,
    /// Set only if `--epochs` was passed; see [`EpochPolicy`] for how this
    /// and `max_epochs` combine into one training mode.
    epochs: Option<usize>,
    /// Set only if `--max-epochs` was passed.
    max_epochs: Option<usize>,
    patience: usize,
    min_delta: f64,
    lr: f64,
    wd: f64,
    held_out_frac: f64,
    seed: i64,
    out: PathBuf,
}

impl Default for Args {
    fn default() -> Args {
        Args {
            dumps: Vec::new(),
            hidden: 64,
            epochs: None,
            max_epochs: None,
            patience: 2,
            min_delta: 0.001,
            lr: 1e-3,
            wd: 1e-4,
            held_out_frac: 0.1,
            seed: 1,
            out: PathBuf::from("policy.ckpt"),
        }
    }
}

/// How many epochs to train, and whether that count is fixed up front or
/// decided by early stopping. Kept as an enum (not two `Option<usize>`
/// fields poked at ad hoc from `run`) so the two modes' behaviour cannot
/// silently blend -- `Fixed` reproduces the original driver's numbers
/// exactly (no per-epoch held-out pass, no early stop), `EarlyStop` is the
/// new behaviour this change adds.
#[derive(Debug, PartialEq)]
enum EpochPolicy {
    /// `--epochs N` (or neither flag given): train exactly `N` epochs, as
    /// this binary always has -- unchanged, so existing recorded results
    /// stay reproducible.
    Fixed(usize),
    /// `--max-epochs N`: train until held-out top-1 stops improving (see
    /// [`train_until_early_stop`]), capped at `N` epochs.
    EarlyStop { max_epochs: usize, patience: usize, min_delta: f64 },
}

const USAGE: &str = "\
usage: policytrain --dump PATH [--dump PATH ...] [options]

  --dump PATH        a dump_selfplay .tpd file (repeatable)
  --hidden N         hidden width of the (zero-block) scorer net (default 64)
  --epochs N         train exactly N epochs, no early stopping (default 3;
                      mutually exclusive with --max-epochs)
  --max-epochs N     train until held-out top-1 stops improving, capped at N
                      epochs (mutually exclusive with --epochs)
  --patience N       epochs of no held-out top-1 improvement before an
                      early-stopped run stops (default 2, only used with
                      --max-epochs)
  --min-delta F      minimum held-out top-1 improvement that resets patience
                      (default 0.001, only used with --max-epochs)
  --lr F             AdamW learning rate (default 0.001)
  --wd F             AdamW weight decay (default 0.0001)
  --held-out-frac F  fraction of GAMES (not decisions) held out (default 0.1)
  --seed N           split shuffle + net init seed (default 1)
  --out PATH         checkpoint output path (default policy.ckpt)
  --help
";

fn parse_num<T: std::str::FromStr>(s: &str, flag: &str) -> Result<T, String> {
    s.parse().map_err(|_| format!("{flag}: {s:?} is not a number"))
}

fn parse_args(argv: &[String]) -> Result<Option<Args>, String> {
    let mut a = Args::default();
    let mut it = argv.iter();
    while let Some(flag) = it.next() {
        let mut value = |flag: &str| -> Result<String, String> {
            it.next().cloned().ok_or_else(|| format!("{flag} needs a value"))
        };
        match flag.as_str() {
            "--dump" => a.dumps.push(PathBuf::from(value(flag)?)),
            "--hidden" => a.hidden = parse_num(&value(flag)?, flag)?,
            "--epochs" => a.epochs = Some(parse_num(&value(flag)?, flag)?),
            "--max-epochs" => a.max_epochs = Some(parse_num(&value(flag)?, flag)?),
            "--patience" => a.patience = parse_num(&value(flag)?, flag)?,
            "--min-delta" => a.min_delta = parse_num(&value(flag)?, flag)?,
            "--lr" => a.lr = parse_num(&value(flag)?, flag)?,
            "--wd" => a.wd = parse_num(&value(flag)?, flag)?,
            "--held-out-frac" => a.held_out_frac = parse_num(&value(flag)?, flag)?,
            "--seed" => a.seed = parse_num(&value(flag)?, flag)?,
            "--out" => a.out = PathBuf::from(value(flag)?),
            "--help" | "-h" => {
                print!("{USAGE}");
                return Ok(None);
            }
            other => return Err(format!("unknown flag {other}\n\n{USAGE}")),
        }
    }
    if a.dumps.is_empty() {
        return Err(format!("at least one --dump is required\n\n{USAGE}"));
    }
    if a.epochs.is_some() && a.max_epochs.is_some() {
        return Err(format!("--epochs and --max-epochs are mutually exclusive -- pick a fixed epoch count or early stopping\n\n{USAGE}"));
    }
    Ok(Some(a))
}

/// Resolve the parsed flags into one [`EpochPolicy`] -- the single place
/// that decides which of the two mutually-exclusive modes `run` takes,
/// mirroring [`parse_args`]'s own mutual-exclusion check rather than
/// re-deciding it a second, possibly inconsistent way.
fn epoch_policy(args: &Args) -> EpochPolicy {
    match args.max_epochs {
        Some(max_epochs) => EpochPolicy::EarlyStop { max_epochs, patience: args.patience, min_delta: args.min_delta },
        None => EpochPolicy::Fixed(args.epochs.unwrap_or(3)),
    }
}

fn run(args: &Args) -> Result<(), String> {
    let start = Instant::now();
    let mut records = Vec::new();
    for p in &args.dumps {
        let recs = read_dump(p)?;
        println!("{}: {} decision records", p.display(), recs.len());
        records.extend(recs);
    }
    let n_total = records.len();
    let (train, held) = split_by_game(records, args.held_out_frac, args.seed);
    println!(
        "loaded {n_total} decisions in {:.1}s; split -> {} train, {} held-out",
        start.elapsed().as_secs_f64(),
        train.len(),
        held.len()
    );

    let net = random_policy_net(args.hidden, args.seed);
    let mut trainer = PolicyTrainer::new(net, args.lr, args.wd);
    let mut order: Vec<usize> = (0..train.len()).collect();
    let mut rng = PyRandom::new(args.seed.into());

    // `saved_epochs` is what goes into the checkpoint's `epochs` meta field:
    // the fixed count for `Fixed`, or (best epoch index + 1) -- the size of
    // the actually-useful prefix -- for `EarlyStop`, never the epoch count
    // the loop happened to run through before its patience ran out.
    let (final_net, report, saved_epochs): (ValueNet, HeldOutReport, usize) = match epoch_policy(args) {
        EpochPolicy::Fixed(epochs) => {
            for epoch in 0..epochs {
                let t0 = Instant::now();
                let mean_loss = train_epoch(&mut trainer, &train, &mut order, &mut rng);
                println!("epoch {epoch}: mean train loss {mean_loss:.4}  [{:.1}s]", t0.elapsed().as_secs_f64());
            }
            let report = held_out_report(&trainer.net, &held);
            (trainer.net.clone(), report, epochs)
        }
        EpochPolicy::EarlyStop { max_epochs, patience, min_delta } => {
            let outcome = train_until_early_stop(&mut trainer, &train, &held, &mut order, &mut rng, max_epochs, patience, min_delta)?;
            println!(
                "\nbest epoch: {} of {} run (held-out top-1 {:.4})",
                outcome.best_epoch,
                outcome.epochs_run,
                outcome.best_report.top1_rate()
            );
            (outcome.best_net, outcome.best_report, outcome.best_epoch + 1)
        }
    };

    let top1_rate = report.top1 as f64 / report.n as f64;
    let top3_rate = report.top3 as f64 / report.n as f64;
    let random_top1_rate = report.random_top1 / report.n as f64;
    let random_top3_rate = report.random_top3 / report.n as f64;
    let mean_legal = report.mean_legal / report.n as f64;
    println!("\n=== held-out ({} decisions) ===", report.n);
    println!("mean legal-set size: {mean_legal:.2}");
    println!("top-1 agreement:  {top1_rate:.4}  (random baseline {random_top1_rate:.4})");
    println!("top-3 agreement:  {top3_rate:.4}  (random baseline {random_top3_rate:.4})");
    if report.n_ge4 > 0 {
        let t1 = report.top1_ge4 as f64 / report.n_ge4 as f64;
        let t3 = report.top3_ge4 as f64 / report.n_ge4 as f64;
        let r1 = report.random_top1_ge4 / report.n_ge4 as f64;
        let r3 = report.random_top3_ge4 / report.n_ge4 as f64;
        println!("\n=== held-out, restricted to legal_count >= 4 ({} decisions) ===", report.n_ge4);
        println!("top-1 agreement:  {t1:.4}  (random baseline {r1:.4})");
        println!("top-3 agreement:  {t3:.4}  (random baseline {r3:.4})");
    }

    let phases = phase_breakdown(&final_net, &held);
    let names = ["early", "mid", "late"];
    println!("\n=== held-out top-1 by game phase (thirds of each game's own length) ===");
    for (name, (agree, total)) in names.iter().zip(phases.iter()) {
        if *total > 0 {
            println!("{name:>5}: {:.4}  ({agree}/{total})", *agree as f64 / *total as f64);
        }
    }

    save_policy_checkpoint(
        &args.out,
        &final_net,
        &[
            ("epochs", saved_epochs as f64),
            ("held_out_top1", top1_rate),
            ("held_out_top3", top3_rate),
            ("held_out_n", report.n as f64),
        ],
    )?;
    println!("\nsaved checkpoint to {}  [total {:.1}s]", args.out.display(), start.elapsed().as_secs_f64());
    Ok(())
}

fn main() -> std::process::ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    match parse_args(&argv) {
        Ok(None) => std::process::ExitCode::SUCCESS,
        Ok(Some(args)) => match run(&args) {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("policytrain: {e}");
                std::process::ExitCode::FAILURE
            }
        },
        Err(e) => {
            eprintln!("policytrain: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// With neither `--epochs` nor `--max-epochs` given, this binary must
    /// keep behaving exactly as it always has -- a fixed 3-epoch run, no
    /// early stopping -- so numbers already recorded from past runs
    /// (`docs/NEURAL.md`) stay reproducible.
    #[test]
    fn no_epoch_flag_defaults_to_the_original_fixed_three_epoch_behaviour() {
        let a = parse_args(&["--dump".to_string(), "x".to_string()]).unwrap().unwrap();
        assert_eq!(epoch_policy(&a), EpochPolicy::Fixed(3));
    }

    /// `--epochs N` alone selects the fixed mode at N, still with no early
    /// stopping -- the flag's pre-existing meaning, untouched by this change.
    #[test]
    fn epochs_flag_alone_selects_fixed_mode_at_the_given_count() {
        let a = parse_args(&["--dump".to_string(), "x".to_string(), "--epochs".to_string(), "7".to_string()]).unwrap().unwrap();
        assert_eq!(epoch_policy(&a), EpochPolicy::Fixed(7));
    }

    /// `--max-epochs N` selects early-stopping mode, picking up `--patience`/
    /// `--min-delta` (or their defaults) alongside it.
    #[test]
    fn max_epochs_flag_selects_early_stop_mode_with_patience_and_min_delta() {
        let a = parse_args(&[
            "--dump".to_string(),
            "x".to_string(),
            "--max-epochs".to_string(),
            "20".to_string(),
            "--patience".to_string(),
            "5".to_string(),
            "--min-delta".to_string(),
            "0.01".to_string(),
        ])
        .unwrap()
        .unwrap();
        assert_eq!(epoch_policy(&a), EpochPolicy::EarlyStop { max_epochs: 20, patience: 5, min_delta: 0.01 });
    }

    /// `--patience`/`--min-delta` default to 2 and 0.001 respectively when
    /// `--max-epochs` is given without them.
    #[test]
    fn patience_and_min_delta_default_when_only_max_epochs_is_given() {
        let a = parse_args(&["--dump".to_string(), "x".to_string(), "--max-epochs".to_string(), "20".to_string()]).unwrap().unwrap();
        assert_eq!(epoch_policy(&a), EpochPolicy::EarlyStop { max_epochs: 20, patience: 2, min_delta: 0.001 });
    }

    /// `--epochs` and `--max-epochs` together is a contradiction (one says
    /// "train exactly this many", the other says "let held-out top-1 decide
    /// how many") -- `parse_args` must reject it rather than silently
    /// picking one, which is exactly the kind of ambiguity `EpochPolicy`
    /// exists to make unrepresentable everywhere except this one check.
    #[test]
    fn epochs_and_max_epochs_together_is_rejected() {
        let err = parse_args(&[
            "--dump".to_string(),
            "x".to_string(),
            "--epochs".to_string(),
            "3".to_string(),
            "--max-epochs".to_string(),
            "10".to_string(),
        ])
        .unwrap_err();
        assert!(err.contains("mutually exclusive"), "{err}");
    }
}
