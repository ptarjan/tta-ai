//! `humandata` -- extracts the (position, move-the-human-played) dataset
//! `docs/HUMAN_MODEL.md` describes, filtered to BGO's own STRONG skill
//! tiers (`corpus::Tier::Warlord`/`Emperor` -- `sources/bgo/index.tsv`'s
//! `level` column, already parsed by `corpus::GameMeta::tier`; no proxy).
//!
//! This binary adds NO new reconstruction or encoding logic of its own: it
//! is a thin consumer of [`tta::replay_common::replay_game`]'s
//! `record_decisions: true` path (the exact machinery `bin/agreement.rs`
//! already uses) and [`tta::human_policy::candidate_features`] (which
//! mirrors `WeightedBot::rank_moves`'s own trial-and-encode loop -- see that
//! function's doc comment for the legality argument). What this binary adds
//! is: (1) selecting games by TIER rather than an explicit id list, (2)
//! writing the sparse TSV dataset [`tta::human_policy::write_decision`]
//! defines, and (3) a stats report -- volume by player count/age/category,
//! the late-game sampling bias, and the discard-taint fraction -- printed to
//! stderr so stdout stays a clean data file.
//!
//! # Regeneration
//!
//! ```text
//! tar -xzf sources/bgo/journals.tar.gz -C /tmp/bgo-journals   # once
//! cargo run --profile difftest --bin humandata -- \
//!     sources/bgo/index.tsv /tmp/bgo-journals/journals Warlord \
//!     > human_dataset.tsv 2> human_dataset_report.txt
//! ```
//!
//! `Warlord` is the minimum tier (inclusive) to include -- `Tier` derives
//! `Ord` in ladder order (`Prince < King < Warlord < Emperor`), so this
//! selects every game whose tier is Warlord or Emperor. Every game meeting
//! the filter is used -- no subsample, no cherry-picking (unlike
//! `docs/AGREEMENT.md`'s own 150-game subsample, chosen there only because
//! that analysis needed no dataset file; this one wants the honest full
//! count, see `docs/HUMAN_MODEL.md`'s own "how much data exists" section).

use std::collections::HashMap;
use std::env;
use std::fs;
use std::process::ExitCode;

use tta::corpus::{self, GameMeta, Tier};
use tta::human_policy::{self, ExtractedDecision};
use tta::replay_common::{build_card_index, replay_game};

/// Tallies kept while writing the dataset -- printed as the stderr report at
/// the end. A flat struct of counters rather than deferring to a second pass
/// over the written file: the file can be large (one line per decision, each
/// carrying every candidate's sparse feature vector), so this binary counts
/// once, on the fly, rather than reading it back.
#[derive(Default)]
struct Stats {
    games_matched: usize,
    games_no_journal: usize,
    games_processed: usize,
    decisions: usize,
    tainted: usize,
    by_players: HashMap<u8, usize>,
    by_age: HashMap<String, usize>,
    by_category: HashMap<&'static str, usize>,
    by_tier: HashMap<&'static str, usize>,
    // Late-game sampling bias: for every game, the true total round count
    // (`GameMeta::rounds`, `index.tsv`'s own ground truth for the REAL,
    // completed game) versus the highest round any decision this binary
    // actually recorded reached -- `replay.rs`'s reconstruction stops partway
    // through almost every game (`docs/REPLAY.md`), so this ratio quantifies
    // exactly how much of a real game's LENGTH this dataset captures, not
    // just how many decisions it recorded.
    true_rounds_sum: u64,
    reached_round_sum: u64,
    games_with_a_decision: usize,
}

fn tier_name(t: Tier) -> &'static str {
    // `Tier::as_str` already exists and is used for display elsewhere
    // (`bin/agreement.rs`'s own `tier` column) -- reused, not restated.
    t.as_str()
}

fn run(index_path: &str, journals_dir: &str, min_tier_str: &str) -> Result<(), String> {
    let min_tier = Tier::parse(min_tier_str)?;
    let card_index = build_card_index();
    let games = corpus::parse_index(index_path)?;

    let mut stats = Stats::default();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    let matched: Vec<&GameMeta> = games.iter().filter(|g| g.tier >= min_tier).collect();
    stats.games_matched = matched.len();

    for meta in matched {
        let path = format!("{journals_dir}/{}.tsv", meta.id);
        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(_) => {
                stats.games_no_journal += 1;
                continue;
            }
        };
        stats.games_processed += 1;
        *stats.by_tier.entry(tier_name(meta.tier)).or_insert(0) += 1;

        let result = replay_game(meta, &text, &card_index, true);
        let mut max_round_reached: u16 = 0;
        for d in &result.decisions {
            let idx = d.state.decider();
            let candidates = human_policy::candidate_features(&d.state, idx, &d.legal_moves);
            let chosen_idx = match d.legal_moves.iter().position(|&m| m == d.human_move) {
                Some(i) => i,
                // `human_move` is always drawn from `legal_moves` by
                // construction (`replay_common::Decision`'s own contract,
                // matching `bin/agreement.rs`'s identical assumption) -- this
                // arm is defensive, not expected to fire on real data.
                None => {
                    eprintln!("{}: human_move not found in legal_moves at line {}, skipping", meta.id, d.lineno);
                    continue;
                }
            };
            let category = human_policy::categorize(d.human_move);
            let age = format!("{:?}", d.state.age_civil);

            max_round_reached = max_round_reached.max(d.state.round);
            stats.decisions += 1;
            if d.after_arbitrary_discard {
                stats.tainted += 1;
            }
            *stats.by_players.entry(d.state.num_players).or_insert(0) += 1;
            *stats.by_age.entry(age.clone()).or_insert(0) += 1;
            *stats.by_category.entry(category.name()).or_insert(0) += 1;

            let row = ExtractedDecision {
                game_id: meta.id.clone(),
                tier: tier_name(meta.tier),
                players: d.state.num_players,
                age,
                round: u32::from(d.state.round),
                lineno: d.lineno,
                category: category.name(),
                discard_tainted: d.after_arbitrary_discard,
                chosen_idx,
                candidates,
            };
            human_policy::write_decision(&mut out, &row).map_err(|e| e.to_string())?;
        }
        if !result.decisions.is_empty() {
            stats.games_with_a_decision += 1;
            stats.true_rounds_sum += u64::from(meta.rounds);
            stats.reached_round_sum += u64::from(u32::from(max_round_reached));
        }
    }

    print_report(&stats, min_tier);
    Ok(())
}

fn print_report(s: &Stats, min_tier: Tier) {
    eprintln!("=== humandata extraction report (tier >= {}) ===", min_tier.as_str());
    eprintln!(
        "games matching tier filter: {}  (journal missing: {}, processed: {})",
        s.games_matched, s.games_no_journal, s.games_processed
    );
    let mut tiers: Vec<(&&str, &usize)> = s.by_tier.iter().collect();
    tiers.sort();
    for (t, n) in tiers {
        eprintln!("  tier {t}: {n} games processed");
    }
    eprintln!("decisions recorded: {}", s.decisions);
    eprintln!(
        "discard-tainted decisions: {} ({:.1}%)",
        s.tainted,
        100.0 * s.tainted as f64 / s.decisions.max(1) as f64
    );
    eprintln!("-- by player count --");
    let mut pc: Vec<(&u8, &usize)> = s.by_players.iter().collect();
    pc.sort();
    for (p, n) in pc {
        eprintln!("  {p}p: {n} decisions");
    }
    eprintln!("-- by game age (GameState::age_civil at the decision point) --");
    let mut ac: Vec<(&String, &usize)> = s.by_age.iter().collect();
    ac.sort();
    for (a, n) in ac {
        eprintln!(
            "  {a}: {n} decisions ({:.1}%)",
            100.0 * *n as f64 / s.decisions.max(1) as f64
        );
    }
    eprintln!("-- by move category --");
    let mut cc: Vec<(&&str, &usize)> = s.by_category.iter().collect();
    cc.sort();
    for (c, n) in cc {
        eprintln!("  {c}: {n} decisions");
    }
    eprintln!("-- late-game sampling bias (replay's own early-stop ceiling) --");
    eprintln!(
        "games with >=1 recorded decision: {} of {} processed",
        s.games_with_a_decision, s.games_processed
    );
    if s.games_with_a_decision > 0 {
        let mean_true = s.true_rounds_sum as f64 / s.games_with_a_decision as f64;
        let mean_reached = s.reached_round_sum as f64 / s.games_with_a_decision as f64;
        eprintln!(
            "mean true game length (index.tsv 'rounds', the REAL completed game): {mean_true:.1} rounds"
        );
        eprintln!(
            "mean highest round any recorded decision reached: {mean_reached:.1} rounds ({:.1}% of true length)",
            100.0 * mean_reached / mean_true.max(1.0)
        );
    }
}

fn main() -> ExitCode {
    let argv: Vec<String> = env::args().skip(1).collect();
    if argv.len() != 3 {
        eprintln!("usage: humandata <index.tsv> <journals_dir> <min_tier: Prince|King|Warlord|Emperor>");
        return ExitCode::FAILURE;
    }
    match run(&argv[0], &argv[1], &argv[2]) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
