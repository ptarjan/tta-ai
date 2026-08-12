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
use tta::bots::neural::policy_train::{
    held_out_report, phase_breakdown, random_policy_net, save_policy_checkpoint, split_by_game, train_epoch, PolicyTrainer,
};
use tta::rng::PyRandom;

#[derive(Clone, Debug)]
struct Args {
    dumps: Vec<PathBuf>,
    hidden: usize,
    epochs: usize,
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
            epochs: 3,
            lr: 1e-3,
            wd: 1e-4,
            held_out_frac: 0.1,
            seed: 1,
            out: PathBuf::from("policy.ckpt"),
        }
    }
}

const USAGE: &str = "\
usage: policytrain --dump PATH [--dump PATH ...] [options]

  --dump PATH        a dump_selfplay .tpd file (repeatable)
  --hidden N         hidden width of the (zero-block) scorer net (default 64)
  --epochs N         training epochs over the train split (default 3)
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
            "--epochs" => a.epochs = parse_num(&value(flag)?, flag)?,
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
    Ok(Some(a))
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

    for epoch in 0..args.epochs {
        let t0 = Instant::now();
        let mean_loss = train_epoch(&mut trainer, &train, &mut order, &mut rng);
        println!("epoch {epoch}: mean train loss {mean_loss:.4}  [{:.1}s]", t0.elapsed().as_secs_f64());
    }

    let report = held_out_report(&trainer.net, &held);
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

    let phases = phase_breakdown(&trainer.net, &held);
    let names = ["early", "mid", "late"];
    println!("\n=== held-out top-1 by game phase (thirds of each game's own length) ===");
    for (name, (agree, total)) in names.iter().zip(phases.iter()) {
        if *total > 0 {
            println!("{name:>5}: {:.4}  ({agree}/{total})", *agree as f64 / *total as f64);
        }
    }

    save_policy_checkpoint(
        &args.out,
        &trainer.net,
        &[
            ("epochs", args.epochs as f64),
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
