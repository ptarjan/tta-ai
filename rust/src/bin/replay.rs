//! `replay` -- the game-state reconstruction spike (`docs/REPLAY.md`).
//!
//! Thin CLI over [`tta::replay_common::replay_game`], which holds the actual
//! reconstruction machinery -- see that module's own doc comment for what is
//! RECONSTRUCTED vs SIMULATED, the Event/Territory preparation inference,
//! and what this file gives up on. This split exists so `bin/agreement.rs`
//! (`docs/REPLAY.md`'s planned move-agreement analysis) can reuse the exact
//! same machinery: `bin/*.rs` files are separate crates and cannot import
//! from one another directly, only from the shared `tta` library.
//!
//! ```text
//! tar -xzf sources/bgo/journals.tar.gz -C /tmp/bgo-journals
//! cargo run --profile difftest --bin replay -- \
//!     sources/bgo/index.tsv /tmp/bgo-journals/journals <game_id> [game_id ...]
//! ```

use std::collections::HashMap;
use std::env;
use std::fs;
use std::process::ExitCode;

use tta::corpus::{self, GameMeta};
use tta::replay_common::{build_card_index, replay_game, GameResult};

fn run(index_path: &str, journals_dir: &str, ids: &[String]) -> Result<(), String> {
    let card_index = build_card_index();
    let games = corpus::parse_index(index_path)?;
    let by_id: HashMap<&str, &GameMeta> = games.iter().map(|g| (g.id.as_str(), g)).collect();

    let mut n_completed = 0usize;
    let mut n_score_match = 0usize;
    let mut n_score_checked = 0usize;
    let mut n_approx = 0usize;
    let mut discards_solved = 0u32;
    let mut discards_chosen = 0u32;
    let mut discards_forced_collision = 0u32;

    for id in ids {
        let Some(meta) = by_id.get(id.as_str()) else {
            println!("{id}: not found in index.tsv");
            continue;
        };
        let path = format!("{journals_dir}/{id}.tsv");
        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                println!("{id}: no journal file ({e})");
                continue;
            }
        };
        let result = replay_game(meta, &text, &card_index, false);
        print_result(&result);
        discards_solved += result.discards_solved;
        discards_chosen += result.discards_chosen;
        discards_forced_collision += result.discards_forced_collision;
        if result.completed {
            n_completed += 1;
            if result.colonize_approximated {
                n_approx += 1;
            }
            if let Some(engine) = &result.engine_scores {
                n_score_checked += 1;
                let mut a = engine.clone();
                let mut b = result.index_scores.clone();
                a.sort_unstable();
                b.sort_unstable();
                if a == b {
                    n_score_match += 1;
                }
            }
        }
    }

    println!(
        "\n{n_completed}/{} games replayed to completion with every human action legal ({n_approx} used the colonize approximation).",
        ids.len()
    );
    println!("{n_score_match}/{n_score_checked} completed games' final scores matched index.tsv (sorted multiset comparison).");
    let total_discards = discards_solved + discards_chosen + discards_forced_collision;
    println!(
        "Military discards resolved: {total_discards} ({discards_solved} solved uniquely, \
         {discards_chosen} chosen arbitrarily among valid candidates, {discards_forced_collision} \
         forced collisions -- see docs/REPLAY.md's discard_solver section)."
    );
    Ok(())
}

fn print_result(g: &GameResult) {
    let status = if g.completed { "COMPLETE" } else { "STOPPED" };
    print!("{} [{}p] {status} after {} actions", g.id, g.players, g.actions_consumed);
    if g.colonize_approximated {
        print!(" (colonize approximated)");
    }
    if g.bid_ceilings_grounded > 0 {
        print!(" ({} hand card(s) grounded from a bid's force ceiling)", g.bid_ceilings_grounded);
    }
    if let Some(engine) = &g.engine_scores {
        let mut a = engine.clone();
        let mut b = g.index_scores.clone();
        a.sort_unstable();
        b.sort_unstable();
        print!(" scores engine={engine:?} index={:?} match={}", g.index_scores, a == b);
    }
    let total_discards = g.discards_solved + g.discards_chosen + g.discards_forced_collision;
    if total_discards > 0 {
        print!(
            " discards={total_discards} (solved={} chosen={} forced_collision={})",
            g.discards_solved, g.discards_chosen, g.discards_forced_collision
        );
    }
    println!();
    if let Some(p) = &g.civil_deck_premature_advance {
        println!(
            "    civil_deck_premature_advance: line {} reconstructed {:?} ahead of journal's own {:?}",
            p.lineno, p.reconstructed_age, p.journal_age
        );
    }
    if let Some(m) = &g.mismatch {
        println!(
            "    line {} (age {} round {}): {}",
            m.lineno, m.age, m.round, m.raw_text
        );
        println!("    -> {:?}", m.kind);
    }
}

fn main() -> ExitCode {
    let argv: Vec<String> = env::args().skip(1).collect();
    if argv.len() < 3 {
        eprintln!("usage: replay <index.tsv> <journals_dir> <game_id> [game_id ...]");
        return ExitCode::FAILURE;
    }
    match run(&argv[0], &argv[1], &argv[2..]) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
