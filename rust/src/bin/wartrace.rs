//! Temporary diagnostic: replay one game, print strength decomposition at
//! every war resolution. Delete after the War-over-Culture trace is done.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::process::ExitCode;

use tta::corpus::{self, GameMeta};
use tta::replay_common::{build_card_index, replay_game};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.len() < 3 {
        eprintln!("usage: wartrace <index.tsv> <journals_dir> <game_id> [game_id ...]");
        return ExitCode::FAILURE;
    }
    let card_index = build_card_index();
    let games = corpus::parse_index(&args[0]).unwrap();
    let by_id: HashMap<&str, &GameMeta> = games.iter().map(|g| (g.id.as_str(), g)).collect();

    for id in &args[2..] {
        let Some(meta) = by_id.get(id.as_str()) else {
            println!("{id}: not found in index.tsv");
            continue;
        };
        let path = format!("{}/{}.tsv", args[1], id);
        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                println!("{id}: no journal file ({e})");
                continue;
            }
        };
        let result = replay_game(meta, &text, &card_index, false);
        let mut a = result.engine_scores.clone().unwrap_or_default();
        let mut b = result.index_scores.clone();
        a.sort_unstable();
        b.sort_unstable();
        println!(
            "{id}: completed={} engine={a:?} index={b:?} match={} divergences={:?}",
            result.completed,
            a == b,
            result.final_event_award_divergences
        );
    }
    ExitCode::SUCCESS
}
