//! `policy_disagreement_experiment` -- the control-vs-teacher comparison
//! `docs/NEURAL.md`'s disagreement-teacher work order asks for (calling
//! task, 2026-08-05): does training extra emphasis onto the decisions
//! where the trained net's argmax disagrees with the CHAMPION'S SEARCH
//! (never the net's own prior prediction -- see
//! `policy_train::disagreement_indices`'s doc comment for why that
//! distinction is the entire point) beat simply training the plain recipe
//! for longer? `bin/policytrain.rs`'s original 3-epoch run was undertrained
//! (loss still falling), so "disagreement training helped" could just be
//! "more training helped" unless both arms are compared against a properly
//! early-stopped control.
//!
//! ## Design: one process, one shared prefix, two branches
//!
//! Both arms train the IDENTICAL first `best_epoch` epochs (same net init,
//! same shuffle order, same everything) because they are, literally, the
//! same run up to that point -- there is only one training loop, not two
//! copies of one. Early stopping (`--patience`/`--eps`, watched on HELD-OUT
//! top-1, never train loss) decides where that shared prefix ends: `N =
//! best_epoch + 1` is the "same epoch budget" both arms are held to.
//!
//! At that point the run snapshots weights AND optimiser moments together
//! ([`PolicyTrainer::snapshot`]) and BRANCHES:
//!   - the CONTROL branch is simply the best epoch this run already trained
//!     (no extra work -- it is already sitting in `best_net`);
//!   - the TEACHER branch resumes from the pre-best-epoch snapshot
//!     ([`PolicyTrainer::from_state`], so its Adam moments are exactly as
//!     warm as control's were, not restarted cold) and trains one epoch
//!     over ONLY the training decisions where the pre-best-epoch net's
//!     argmax disagreed with the champion's chosen move.
//!
//! Mechanism chosen: a FINE-TUNE PASS restricted to the disagreement
//! subset, rather than upweighting every epoch or resampling the whole
//! corpus, because it isolates the comparison to "what did the last epoch
//! of the shared budget train on" while sharing 100% of the earlier
//! compute and code path with control -- no separate weighting logic to
//! keep in sync with `train_epoch`.

use std::path::PathBuf;
use std::time::Instant;

use tta::bots::neural::dump::read_dump;
use tta::bots::neural::net::ValueNet;
use tta::bots::neural::policy_train::{
    disagreement_indices, held_out_report, phase_breakdown, random_policy_net, save_policy_checkpoint, split_by_game,
    train_epoch, HeldOutReport, PolicyTrainer,
};
use tta::bots::neural::train::AdamW;
use tta::rng::PyRandom;

#[derive(Clone, Debug)]
struct Args {
    dumps: Vec<PathBuf>,
    hidden: usize,
    lr: f64,
    wd: f64,
    held_out_frac: f64,
    seed: i64,
    max_epochs: usize,
    patience: usize,
    eps: f64,
    out_dir: PathBuf,
    noise_check: bool,
}

impl Default for Args {
    fn default() -> Args {
        Args {
            dumps: Vec::new(),
            hidden: 64,
            lr: 1e-3,
            wd: 1e-4,
            held_out_frac: 0.1,
            seed: 1,
            max_epochs: 10,
            patience: 1,
            eps: 0.002,
            out_dir: PathBuf::from("scratch"),
            noise_check: false,
        }
    }
}

const USAGE: &str = "\
usage: policy_disagreement_experiment --dump PATH [--dump PATH ...] [options]

  --dump PATH        a dump_selfplay .tpd file (repeatable)
  --hidden N         hidden width of the scorer net (default 64)
  --lr F             AdamW learning rate (default 0.001)
  --wd F             AdamW weight decay (default 0.0001)
  --held-out-frac F  fraction of GAMES held out (default 0.1)
  --seed N           split shuffle + net init seed (default 1)
  --max-epochs N     hard cap on the shared-prefix training loop (default 10)
  --patience N       epochs of no held-out top-1 improvement before stopping (default 1)
  --eps F            minimum held-out top-1 improvement to reset patience (default 0.002)
  --out-dir PATH     where to write checkpoints (default scratch)
  --noise-check      also re-run both branches with a second seed for a noise estimate
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
            "--lr" => a.lr = parse_num(&value(flag)?, flag)?,
            "--wd" => a.wd = parse_num(&value(flag)?, flag)?,
            "--held-out-frac" => a.held_out_frac = parse_num(&value(flag)?, flag)?,
            "--seed" => a.seed = parse_num(&value(flag)?, flag)?,
            "--max-epochs" => a.max_epochs = parse_num(&value(flag)?, flag)?,
            "--patience" => a.patience = parse_num(&value(flag)?, flag)?,
            "--eps" => a.eps = parse_num(&value(flag)?, flag)?,
            "--out-dir" => a.out_dir = PathBuf::from(value(flag)?),
            "--noise-check" => a.noise_check = true,
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
    Ok(Some(a))
}

fn print_report(label: &str, report: &HeldOutReport, phases: [(usize, usize); 3]) {
    let mean_legal = report.mean_legal / report.n as f64;
    println!("\n--- {label}: held-out ({} decisions) ---", report.n);
    println!("mean legal-set size: {mean_legal:.2}");
    println!(
        "top-1 agreement:  {:.4}  (random baseline {:.4})",
        report.top1_rate(),
        report.random_top1 / report.n as f64
    );
    println!(
        "top-3 agreement:  {:.4}  (random baseline {:.4})",
        report.top3_rate(),
        report.random_top3 / report.n as f64
    );
    if report.n_ge4 > 0 {
        let t1 = report.top1_ge4 as f64 / report.n_ge4 as f64;
        let t3 = report.top3_ge4 as f64 / report.n_ge4 as f64;
        let r1 = report.random_top1_ge4 / report.n_ge4 as f64;
        let r3 = report.random_top3_ge4 / report.n_ge4 as f64;
        println!("legal_count >= 4 ({} decisions): top-1 {t1:.4} (random {r1:.4})  top-3 {t3:.4} (random {r3:.4})", report.n_ge4);
    }
    let names = ["early", "mid", "late"];
    for (name, (agree, total)) in names.iter().zip(phases.iter()) {
        if *total > 0 {
            println!("  phase {name:>5}: {:.4}  ({agree}/{total})", *agree as f64 / *total as f64);
        }
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

    let mut best_top1 = f64::NEG_INFINITY;
    let mut best_epoch = 0usize;
    let mut best_net: Option<ValueNet> = None;
    let mut best_report: Option<HeldOutReport> = None;
    let mut snapshot_before_best: Option<(ValueNet, AdamW)> = None;
    let mut patience_left = args.patience;

    for epoch in 0..args.max_epochs {
        let snapshot_before = trainer.snapshot();
        let t0 = Instant::now();
        let mean_loss = train_epoch(&mut trainer, &train, &mut order, &mut rng);
        let report = held_out_report(&trainer.net, &held);
        let top1 = report.top1_rate();
        println!(
            "epoch {epoch}: mean train loss {mean_loss:.4}  held-out top-1 {top1:.4}  [{:.1}s]",
            t0.elapsed().as_secs_f64()
        );
        if top1 > best_top1 + args.eps {
            best_top1 = top1;
            best_epoch = epoch;
            best_net = Some(trainer.net.clone());
            best_report = Some(report);
            snapshot_before_best = Some(snapshot_before);
            patience_left = args.patience;
        } else {
            if patience_left == 0 {
                println!("early stop: held-out top-1 has not improved by >= {} for {} epoch(s)", args.eps, args.patience);
                break;
            }
            patience_left -= 1;
        }
    }

    let control_net = best_net.ok_or_else(|| "no epoch ever improved held-out top-1".to_string())?;
    let control_report = best_report.expect("best_report set alongside best_net");
    let (snap_net, snap_adamw) = snapshot_before_best.expect("snapshot_before_best set alongside best_net");
    let n_epochs = best_epoch + 1;
    println!("\n=== shared epoch budget N = {n_epochs} (best epoch index {best_epoch}, held-out top-1 {best_top1:.4}) ===");

    let control_phases = phase_breakdown(&control_net, &held);
    print_report("CONTROL", &control_report, control_phases);

    // ---- teacher branch: resume from the pre-best-epoch snapshot, warm
    // Adam moments and all, and fine-tune one epoch on ONLY the training
    // decisions where THAT snapshot's argmax disagreed with the champion's
    // chosen move (never the post-training net's own argmax on some other
    // record -- see `disagreement_indices`'s doc comment).
    let t_disagree = Instant::now();
    let disagree_idx = disagreement_indices(&snap_net, &train);
    let disagree_records: Vec<_> = disagree_idx.iter().map(|&i| train[i].clone()).collect();
    println!(
        "\ndisagreement set: {} / {} train decisions ({:.1}%) where the pre-final-epoch net disagreed with the champion's search  [{:.1}s]",
        disagree_records.len(),
        train.len(),
        100.0 * disagree_records.len() as f64 / train.len() as f64,
        t_disagree.elapsed().as_secs_f64()
    );

    let mut teacher_trainer = PolicyTrainer::from_state(snap_net.clone(), snap_adamw.clone());
    let mut teacher_order: Vec<usize> = (0..disagree_records.len()).collect();
    let mut teacher_rng = PyRandom::new((args.seed + 1000).into());
    let t0 = Instant::now();
    let teacher_loss = train_epoch(&mut teacher_trainer, &disagree_records, &mut teacher_order, &mut teacher_rng);
    println!("teacher fine-tune epoch: mean train loss {teacher_loss:.4}  [{:.1}s]", t0.elapsed().as_secs_f64());

    let teacher_report = held_out_report(&teacher_trainer.net, &held);
    let teacher_phases = phase_breakdown(&teacher_trainer.net, &held);
    print_report("TEACHER", &teacher_report, teacher_phases);

    println!(
        "\n=== delta (teacher - control) ===\ntop-1 overall: {:+.4}\ntop-3 overall: {:+.4}",
        teacher_report.top1_rate() - control_report.top1_rate(),
        teacher_report.top3_rate() - control_report.top3_rate(),
    );

    if args.noise_check {
        println!("\n=== noise check: re-branch both arms from the same snapshot with a different seed ===");
        let mut control_repeat = PolicyTrainer::from_state(snap_net.clone(), snap_adamw.clone());
        let mut order2: Vec<usize> = (0..train.len()).collect();
        let mut rng2 = PyRandom::new((args.seed + 2000).into());
        let t0 = Instant::now();
        train_epoch(&mut control_repeat, &train, &mut order2, &mut rng2);
        let control_repeat_report = held_out_report(&control_repeat.net, &held);
        println!(
            "control repeat (seed {}): top-1 {:.4}  (vs original {:.4}, |delta| {:.4})  [{:.1}s]",
            args.seed + 2000,
            control_repeat_report.top1_rate(),
            control_report.top1_rate(),
            (control_repeat_report.top1_rate() - control_report.top1_rate()).abs(),
            t0.elapsed().as_secs_f64()
        );

        let mut teacher_repeat = PolicyTrainer::from_state(snap_net.clone(), snap_adamw.clone());
        let mut teacher_order2: Vec<usize> = (0..disagree_records.len()).collect();
        let mut teacher_rng2 = PyRandom::new((args.seed + 3000).into());
        let t0 = Instant::now();
        train_epoch(&mut teacher_repeat, &disagree_records, &mut teacher_order2, &mut teacher_rng2);
        let teacher_repeat_report = held_out_report(&teacher_repeat.net, &held);
        println!(
            "teacher repeat (seed {}): top-1 {:.4}  (vs original {:.4}, |delta| {:.4})  [{:.1}s]",
            args.seed + 3000,
            teacher_repeat_report.top1_rate(),
            teacher_report.top1_rate(),
            (teacher_repeat_report.top1_rate() - teacher_report.top1_rate()).abs(),
            t0.elapsed().as_secs_f64()
        );
    }

    std::fs::create_dir_all(&args.out_dir).map_err(|e| format!("{}: {e}", args.out_dir.display()))?;
    save_policy_checkpoint(
        &args.out_dir.join("control.ckpt"),
        &control_net,
        &[("epochs", n_epochs as f64), ("held_out_top1", control_report.top1_rate()), ("held_out_n", control_report.n as f64)],
    )?;
    save_policy_checkpoint(
        &args.out_dir.join("teacher.ckpt"),
        &teacher_trainer.net,
        &[
            ("epochs", n_epochs as f64),
            ("held_out_top1", teacher_report.top1_rate()),
            ("held_out_n", teacher_report.n as f64),
            ("disagreement_train_n", disagree_records.len() as f64),
        ],
    )?;
    println!("\nsaved checkpoints to {}  [total {:.1}s]", args.out_dir.display(), start.elapsed().as_secs_f64());
    Ok(())
}

fn main() -> std::process::ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    match parse_args(&argv) {
        Ok(None) => std::process::ExitCode::SUCCESS,
        Ok(Some(args)) => match run(&args) {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("policy_disagreement_experiment: {e}");
                std::process::ExitCode::FAILURE
            }
        },
        Err(e) => {
            eprintln!("policy_disagreement_experiment: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}
