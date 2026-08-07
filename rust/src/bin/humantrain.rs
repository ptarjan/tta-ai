//! `humantrain` -- fits [`tta::human_policy::train`]'s linear scorer on a
//! [`tta::human_policy`] dataset (`bin/humandata.rs`'s output) and reports
//! held-out top-1 accuracy: overall, per move category, per player count,
//! and per BGO skill tier -- the baseline-model deliverable of
//! `docs/HUMAN_MODEL.md`.
//!
//! No new modelling logic lives here -- this binary is I/O (read the TSV,
//! split by game) and reporting (aggregate + print) around
//! [`tta::human_policy::Normalizer`]/[`tta::human_policy::train`]/
//! [`tta::human_policy::predict_top1`].
//!
//! # Methodology
//!
//! Split BY GAME, never by decision (`human_policy::is_held_out`) --
//! positions from the same game are correlated (the same board evolving
//! turn to turn), so a decision-level split would leak the held-out
//! evaluation. A [`tta::human_policy::Normalizer`] is fit on the TRAIN split
//! ONLY and applied to both, so held-out accuracy is never inflated by
//! peeking the held-out set's own feature statistics.
//!
//! # Regeneration
//!
//! ```text
//! cargo run --profile difftest --bin humandata -- \
//!     sources/bgo/index.tsv /tmp/bgo-journals/journals Warlord \
//!     > human_dataset.tsv 2> human_dataset_report.txt
//! cargo run --profile difftest --bin humantrain -- human_dataset.tsv
//! ```

use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{BufRead, BufReader};
use std::process::ExitCode;

use tta::human_policy::{self, Normalizer, ParsedDecision, TrainConfig};

/// A held-out decision, normalized and ready to score -- kept separate from
/// the training rows (which are consumed once, into `train`'s gradient
/// loop) so evaluation can iterate them repeatedly (once for the fitted
/// model, once each for any per-slice breakdown) without re-reading the
/// file.
struct EvalRow {
    category: String,
    players: u8,
    tier: String,
    discard_tainted: bool,
    candidates: Vec<Vec<f64>>,
    chosen_idx: usize,
}

#[derive(Default)]
struct Bucket {
    correct: usize,
    total: usize,
}

impl Bucket {
    fn hit(&mut self, correct: bool) {
        self.total += 1;
        if correct {
            self.correct += 1;
        }
    }
    fn rate(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.correct as f64 / self.total as f64
        }
    }
}

fn run(dataset_path: &str) -> Result<(), String> {
    let file = fs::File::open(dataset_path).map_err(|e| format!("{dataset_path}: {e}"))?;
    let reader = BufReader::new(file);

    let mut train_rows: Vec<ParsedDecision> = Vec::new();
    let mut held_out: Vec<ParsedDecision> = Vec::new();
    let mut train_games: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut held_out_games: std::collections::HashSet<String> = std::collections::HashSet::new();

    for (lineno, line) in reader.lines().enumerate() {
        let line = line.map_err(|e| format!("{dataset_path}:{}: {e}", lineno + 1))?;
        if line.is_empty() {
            continue;
        }
        let row = human_policy::parse_decision(&line).map_err(|e| format!("{dataset_path}:{}: {e}", lineno + 1))?;
        if human_policy::is_held_out(&row.game_id) {
            held_out_games.insert(row.game_id.clone());
            held_out.push(row);
        } else {
            train_games.insert(row.game_id.clone());
            train_rows.push(row);
        }
    }

    let dim = human_policy::feature_dim();
    eprintln!(
        "loaded {} decisions ({} games train / {} games held-out), {} train decisions / {} held-out decisions",
        train_rows.len() + held_out.len(),
        train_games.len(),
        held_out_games.len(),
        train_rows.len(),
        held_out.len(),
    );

    let norm = Normalizer::fit(&train_rows, dim);

    let normalize = |rows: Vec<ParsedDecision>| -> Vec<ParsedDecision> {
        rows.into_iter()
            .map(|r| ParsedDecision {
                candidates: r.candidates.iter().map(|c| norm.apply(c)).collect(),
                ..r
            })
            .collect()
    };
    let train_norm = normalize(train_rows);
    let eval_rows: Vec<EvalRow> = normalize(held_out)
        .into_iter()
        .map(|r| EvalRow {
            category: r.category,
            players: r.players,
            tier: r.tier,
            discard_tainted: r.discard_tainted,
            candidates: r.candidates,
            chosen_idx: r.chosen_idx,
        })
        .collect();

    let cfg = TrainConfig { epochs: 300, learning_rate: 0.3, l2: 1e-4 };
    eprintln!(
        "training: {} epochs, lr={}, l2={} over {} decisions ({dim}-dim features)...",
        cfg.epochs,
        cfg.learning_rate,
        cfg.l2,
        train_norm.len()
    );
    let w = human_policy::train(&train_norm, dim, &cfg);

    // ------------------------------------------------------------- report
    let mut overall = Bucket::default();
    let mut by_category: HashMap<String, Bucket> = HashMap::new();
    let mut by_players: HashMap<u8, Bucket> = HashMap::new();
    let mut by_tier: HashMap<String, Bucket> = HashMap::new();
    let mut tainted = Bucket::default();
    let mut untainted = Bucket::default();

    for r in &eval_rows {
        let pred = human_policy::predict_top1(&w, &r.candidates);
        let correct = pred == r.chosen_idx;
        overall.hit(correct);
        by_category.entry(r.category.clone()).or_default().hit(correct);
        by_players.entry(r.players).or_default().hit(correct);
        by_tier.entry(r.tier.clone()).or_default().hit(correct);
        if r.discard_tainted {
            tainted.hit(correct);
        } else {
            untainted.hit(correct);
        }
    }

    println!("=== humantrain: held-out top-1 accuracy ===");
    println!(
        "overall: {}/{} = {:.1}%   (reference: trained bot vs human, 2,493/9,428 = 26.4%, 95% CI 25.6-27.3%)",
        overall.correct,
        overall.total,
        100.0 * overall.rate()
    );
    println!("-- by discard taint --");
    println!("  untainted: {}/{} = {:.1}%", untainted.correct, untainted.total, 100.0 * untainted.rate());
    println!("  tainted:   {}/{} = {:.1}%", tainted.correct, tainted.total, 100.0 * tainted.rate());
    println!("-- by player count --");
    let mut pc: Vec<(&u8, &Bucket)> = by_players.iter().collect();
    pc.sort_by(|a, b| a.0.cmp(b.0));
    for (p, b) in pc {
        println!("  {p}p: {}/{} = {:.1}%", b.correct, b.total, 100.0 * b.rate());
    }
    println!("-- by BGO skill tier --");
    let mut tc: Vec<(&String, &Bucket)> = by_tier.iter().collect();
    tc.sort_by(|a, b| a.0.cmp(b.0));
    for (t, b) in tc {
        println!("  {t}: {}/{} = {:.1}%", b.correct, b.total, 100.0 * b.rate());
    }
    println!("-- by move category --");
    let mut cc: Vec<(&String, &Bucket)> = by_category.iter().collect();
    cc.sort_by(|a, b| a.0.cmp(b.0));
    for (c, b) in cc {
        println!("  {c}: {}/{} = {:.1}%", b.correct, b.total, 100.0 * b.rate());
    }

    Ok(())
}

fn main() -> ExitCode {
    let argv: Vec<String> = env::args().skip(1).collect();
    if argv.len() != 1 {
        eprintln!("usage: humantrain <dataset.tsv>");
        return ExitCode::FAILURE;
    }
    match run(&argv[0]) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
