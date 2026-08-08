//! `humanpaired` -- the PAIRED HumanBot-vs-`WeightedBot` comparison
//! `docs/HUMAN_MODEL.md` reports: both bots scored on the EXACT SAME held-out
//! decisions (`human_policy::is_held_out`'s game-id split), broken down by
//! game age. An unpaired comparison against a differently-sampled reference
//! number already produced one retracted, inverted finding in this project
//! (see `docs/HUMAN_MODEL.md`'s "RETRACTION" section) -- this binary exists
//! so that mistake cannot repeat: it always measures both bots on identical
//! decision points, never two separately-drawn samples.
//!
//! This does NOT read `human_dataset.tsv` (that file is 1+ GB and untracked)
//! -- `is_held_out` is a pure function of the game id, so the held-out slice
//! is re-derived straight from `sources/bgo/index.tsv` plus the journal
//! corpus, the exact same [`tta::replay_common::replay_game`]
//! (`record_decisions: true`) path `bin/humandata.rs` uses to build the
//! dataset in the first place -- no new reconstruction logic here either.
//!
//! # Regeneration
//!
//! ```text
//! cargo run --profile difftest --bin humanpaired -- \
//!     ../sources/bgo/index.tsv /tmp/bgo-journals/journals Warlord \
//!     ../analysis/frozen/human_weights.json \
//!     ../analysis/frozen/gauntlet/champion_2p_gen1454_140key_2026-08-06.json \
//!     ../analysis/frozen/gauntlet/champion_3p_gen1384_140key_2026-08-06.json \
//!     ../analysis/frozen/gauntlet/champion_4p_gen448_140key_2026-08-06.json
//! ```
//!
//! The last three arguments are the 2p/3p/4p champion reference files. This
//! project has no LIVE league champion committed anywhere
//! (`experiments/rust_champion_*.json` is gitignored, regenerated-only
//! trainer output -- absent in a fresh clone), so a report built from this
//! binary should name whichever FROZEN, committed reference vector it was
//! run against (`analysis/frozen/gauntlet/` has the most recent one, full
//! 140-key vocabulary, dated 2026-08-06) rather than presenting it as
//! today's live league champion -- see `analysis/frozen/README.md`. That is
//! a real difference from the original (pre-corpus-deepening) HUMAN_MODEL.md
//! comparison, which ran against the then-live champion, worth stating
//! plainly in any report this produces.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use tta::bots::weighted::eval::{load_weights, WeightedBot};
use tta::corpus::{self, GameMeta, Tier};
use tta::human_policy;
use tta::replay_common::{build_card_index, replay_game};

#[derive(Default, Clone, Copy)]
struct Bucket {
    human_correct: usize,
    weighted_correct: usize,
    n: usize,
}

impl Bucket {
    fn add(&mut self, human_correct: bool, weighted_correct: bool) {
        self.n += 1;
        self.human_correct += usize::from(human_correct);
        self.weighted_correct += usize::from(weighted_correct);
    }
}

fn pct(k: usize, n: usize) -> f64 {
    if n == 0 {
        0.0
    } else {
        100.0 * k as f64 / n as f64
    }
}

fn print_bucket(label: &str, b: &Bucket) {
    println!(
        "  {label:<22} n={:<6} human {}/{} = {:.1}%   weighted {}/{} = {:.1}%",
        b.n,
        b.human_correct,
        b.n,
        pct(b.human_correct, b.n),
        b.weighted_correct,
        b.n,
        pct(b.weighted_correct, b.n),
    );
}

fn run(
    index_path: &str,
    journals_dir: &str,
    min_tier_str: &str,
    human_weights_path: &str,
    champion_2p_path: &str,
    champion_3p_path: &str,
    champion_4p_path: &str,
) -> Result<(), String> {
    let min_tier = Tier::parse(min_tier_str)?;
    let card_index = build_card_index();
    let games = corpus::parse_index(index_path)?;

    let human_w = human_policy::load_weights(Path::new(human_weights_path))?;
    let human_vec = human_policy::vector_from_weights(&human_w);

    let mut weighted_bots: HashMap<u8, WeightedBot> = HashMap::new();
    for (players, path) in
        [(2u8, champion_2p_path), (3, champion_3p_path), (4, champion_4p_path)]
    {
        let w = load_weights(Path::new(path))?;
        weighted_bots.insert(players, WeightedBot::new(w));
    }

    // Held out BY GAME, exactly `human_policy::is_held_out` -- restricting
    // to this slice before replaying anything is what makes this binary
    // never need the (untracked, 1GB+) dataset file at all.
    let held_out: Vec<&GameMeta> =
        games.iter().filter(|g| g.tier >= min_tier && human_policy::is_held_out(&g.id)).collect();

    let mut games_processed = 0usize;
    let mut decisions_skipped = 0usize;
    let mut overall = Bucket::default();
    let mut by_age: HashMap<String, Bucket> = HashMap::new();

    for meta in &held_out {
        let path = format!("{journals_dir}/{}.tsv", meta.id);
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        games_processed += 1;
        let weighted_bot = weighted_bots
            .get(&meta.players)
            .ok_or_else(|| format!("no champion weights loaded for {}p", meta.players))?;

        let result = replay_game(meta, &text, &card_index, true);
        for d in &result.decisions {
            let idx = d.state.decider();
            let Some(chosen_idx) = d.legal_moves.iter().position(|&m| m == d.human_move) else {
                decisions_skipped += 1;
                continue;
            };
            let _ = chosen_idx; // parity with humandata.rs's own extraction; unused here directly

            let candidates = human_policy::candidate_features(&d.state, idx, &d.legal_moves);
            let dense: Vec<Vec<f64>> = candidates.iter().map(human_policy::features_to_dense).collect();
            let human_pred = d.legal_moves[human_policy::predict_top1(&human_vec, &dense)];
            let human_correct = human_pred == d.human_move;

            let ranked = weighted_bot.rank_moves(&d.state, &d.legal_moves);
            let weighted_correct = ranked.first().map(|(mv, _)| *mv == d.human_move).unwrap_or(false);

            let age = format!("{:?}", d.state.age_civil);
            overall.add(human_correct, weighted_correct);
            by_age.entry(age).or_default().add(human_correct, weighted_correct);
        }
    }

    println!("=== humanpaired: HumanBot vs WeightedBot, PAIRED on identical held-out decisions ===");
    println!(
        "held-out games matched (tier >= {}): {} total, {} with a journal processed",
        min_tier.as_str(),
        held_out.len(),
        games_processed
    );
    println!("human_move-not-in-legal_moves skips: {decisions_skipped}");
    println!();
    print_bucket("overall", &overall);
    println!("-- by game age --");
    let mut ages: Vec<(&String, &Bucket)> = by_age.iter().collect();
    ages.sort_by(|a, b| a.0.cmp(b.0));
    for (age, b) in ages {
        print_bucket(age, b);
    }
    Ok(())
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.len() != 7 {
        eprintln!(
            "usage: humanpaired <index.tsv> <journals_dir> <min_tier> <human_weights.json> <champion_2p.json> <champion_3p.json> <champion_4p.json>"
        );
        return ExitCode::FAILURE;
    }
    match run(&argv[0], &argv[1], &argv[2], &argv[3], &argv[4], &argv[5], &argv[6]) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
