//! Temporary: dump final government/temple/special-tech levels for every
//! seat of one replayed game, recomputing Impact of Progress exactly as
//! `events::scoring_culture` does. Delete after the trace is done.

use std::process::ExitCode;

use tta::{
    corpus::{self},
    replay_common::{build_card_index, replay_game},
    CardType,
};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        eprintln!("usage: progress_probe <index.tsv> <journals_dir> <game_id>");
        return ExitCode::FAILURE;
    }
    let card_index = build_card_index();
    let games = corpus::parse_index(&args[0]).unwrap();
    let Some(meta) = games.iter().find(|g| g.id == args[2]) else {
        println!("{}: not found", args[2]);
        return ExitCode::SUCCESS;
    };
    let path = format!("{}/{}.tsv", args[1], meta.id);
    let text = std::fs::read_to_string(&path).unwrap();
    let result = replay_game(meta, &text, &card_index, false);
    println!(
        "engine={:?} index={:?}",
        result.engine_scores, result.index_scores
    );
    for d in &result.final_event_award_divergences {
        println!(
            "AWARD DIVERGE: {} seat={} journal={} engine={}",
            d.card, d.seat, d.journal_amount, d.engine_amount
        );
    }
    // The `replay_game` API does not expose the final GameState, so
    // re-derive the per-seat temple/special-tech/government levels from
    // the journal text directly: the engine's Impact of Progress counts
    // `p.government.level()` + sum of SpecialTech levels per seat, and
    // the journal's own "builds X" / "upgrades X" / "discovers X" /
    // "loses X" lines are the ground truth for what each seat held at
    // game end.
    let interests = [
        "builds Theology", "builds Organized Religion", "builds Religion",
        "upgrades Theology", "upgrades Organized Religion",
        "discovers Theology", "discovers Organized Religion",
        "loses Theology", "loses Organized Religion",
        "builds Democracy", "builds Communism", "builds Feudalism",
        "builds Monarchy", "builds Theocracy", "builds Republic",
        "builds Constitutional Monarchy", "builds Constitutional Democracy",
        "builds Fundamentalism", "builds Masonry", "builds Socialism",
        "builds Capitalism", "builds Anarchy", "builds Oligarchy",
    ];
    for line in text.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 5 { continue; }
        let t = f[4];
        if interests.iter().any(|k| t.contains(k)) {
            println!("LINE: {} | {}", f[1], t);
        }
    }
    ExitCode::SUCCESS
}
