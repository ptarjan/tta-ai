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

use tta::bots::neural::dump::{read_dump, DecisionRecord};
use tta::bots::neural::policy_train::{
    expand_row, random_policy_net, save_policy_checkpoint, score_row, split_by_game, PolicyTrainer,
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

// ================================================================== training

fn shuffle_indices(rng: &mut PyRandom, v: &mut [usize]) {
    for i in (1..v.len()).rev() {
        let j = ((rng.random() * (i + 1) as f64) as usize).min(i);
        v.swap(i, j);
    }
}

/// One decision's legal actions expanded to `state ++ action` rows -- built
/// once per decision per epoch (not cached across epochs: at ~7 KB/row this
/// project's "disk is tight" constraint applies to memory too, for a corpus
/// this size).
fn rows_for(rec: &DecisionRecord) -> Vec<Vec<f32>> {
    rec.legal.iter().map(|&mv| expand_row(&rec.state, rec.actor, mv)).collect()
}

fn train_epoch(trainer: &mut PolicyTrainer, train: &[DecisionRecord], order: &mut [usize], rng: &mut PyRandom) -> f64 {
    shuffle_indices(rng, order);
    let mut total_loss = 0.0;
    for &idx in order.iter() {
        let rec = &train[idx];
        let rows = rows_for(rec);
        trainer.zero_grad();
        let (loss, _logits) = trainer.train_decision(&rows, rec.chosen as usize, 1.0);
        trainer.optim_step();
        total_loss += loss;
    }
    total_loss / order.len() as f64
}

// ================================================================= reporting

/// Held-out agreement, reported alongside the base rate a uniform-random
/// legal move would score -- "a number without its base rate is not a
/// result" (calling task). `top1`/`top3` count how often the champion's
/// actual move ranks 1st / in the top 3 BY THE NET'S OWN LOGIT ORDER;
/// `random_top1`/`random_top3` are `mean(1/L)`/`mean(min(3,L)/L)` over the
/// same decisions -- the exact probability a uniform-random pick from the
/// legal set would have matched, decision by decision, not `1/mean(L)`
/// (a different and less meaningful quantity for decisions of very
/// different sizes).
struct HeldOutReport {
    n: usize,
    top1: usize,
    top3: usize,
    random_top1: f64,
    random_top3: f64,
    mean_legal: f64,
    n_ge4: usize,
    top1_ge4: usize,
    top3_ge4: usize,
    random_top1_ge4: f64,
    random_top3_ge4: f64,
}

fn held_out_report(net: &tta::bots::neural::net::ValueNet, held: &[DecisionRecord]) -> HeldOutReport {
    let mut r = HeldOutReport {
        n: 0,
        top1: 0,
        top3: 0,
        random_top1: 0.0,
        random_top3: 0.0,
        mean_legal: 0.0,
        n_ge4: 0,
        top1_ge4: 0,
        top3_ge4: 0,
        random_top1_ge4: 0.0,
        random_top3_ge4: 0.0,
    };
    for rec in held {
        let l = rec.legal.len();
        if l == 0 {
            continue;
        }
        let rows = rows_for(rec);
        let mut scored: Vec<(usize, f64)> = rows.iter().enumerate().map(|(i, row)| (i, score_row(net, row))).collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let chosen = rec.chosen as usize;
        let rank = scored.iter().position(|&(i, _)| i == chosen).expect("chosen must be one of the scored rows");

        r.n += 1;
        r.mean_legal += l as f64;
        let rand_top1 = 1.0 / l as f64;
        let rand_top3 = (3.min(l)) as f64 / l as f64;
        r.random_top1 += rand_top1;
        r.random_top3 += rand_top3;
        if rank == 0 {
            r.top1 += 1;
        }
        if rank < 3 {
            r.top3 += 1;
        }
        if l >= 4 {
            r.n_ge4 += 1;
            r.random_top1_ge4 += rand_top1;
            r.random_top3_ge4 += rand_top3;
            if rank == 0 {
                r.top1_ge4 += 1;
            }
            if rank < 3 {
                r.top3_ge4 += 1;
            }
        }
    }
    r
}

/// Bucket held-out decisions into thirds of EACH GAME's own length (early /
/// mid / late) and report top-1 agreement per bucket -- cheap because
/// `read_dump` preserves each game's decisions in play order and
/// `split_by_game` only ever filters (never reorders), so a game's own
/// decisions are still consecutive, in order, inside `held`.
fn phase_breakdown(net: &tta::bots::neural::net::ValueNet, held: &[DecisionRecord]) -> [(usize, usize); 3] {
    // Group by game_id, preserving first-seen order within each group.
    let mut groups: std::collections::BTreeMap<u32, Vec<usize>> = std::collections::BTreeMap::new();
    for (i, rec) in held.iter().enumerate() {
        groups.entry(rec.game_id).or_default().push(i);
    }
    let mut buckets = [(0usize, 0usize); 3]; // (agree, total) for early/mid/late
    for idxs in groups.values() {
        let len = idxs.len();
        for (pos, &i) in idxs.iter().enumerate() {
            let rec = &held[i];
            if rec.legal.is_empty() {
                continue;
            }
            let rows = rows_for(rec);
            let mut best = 0usize;
            let mut best_score = f64::NEG_INFINITY;
            for (j, row) in rows.iter().enumerate() {
                let s = score_row(net, row);
                if s > best_score {
                    best_score = s;
                    best = j;
                }
            }
            let frac = if len <= 1 { 0.0 } else { pos as f64 / (len - 1) as f64 };
            let bucket = if frac < 1.0 / 3.0 {
                0
            } else if frac < 2.0 / 3.0 {
                1
            } else {
                2
            };
            buckets[bucket].1 += 1;
            if best == rec.chosen as usize {
                buckets[bucket].0 += 1;
            }
        }
    }
    buckets
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
