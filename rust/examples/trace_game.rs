//! Replay ONE corpus game and print where it diverged.
//!
//! `replaystats --game <id>` reports the mismatch kind and line; this prints
//! the engine's own legal-move list at the point of an `IllegalMove`, which is
//! what you actually need to name the rule BGO applied and the engine did not.
//!
//! ```text
//! cargo run --profile difftest --example trace_game -- 7522649
//! ```
//!
//! Paths come from the environment so a worker's clone can point at its own
//! copies: `TTA_INDEX` (default the repo's `sources/bgo/index.tsv`) and
//! `TTA_JOURNALS` (default `/tmp/bgo-journals/journals`).

use std::io::Read;
use std::process::exit;

use tta::replay_common::MismatchKind;

fn main() {
    let id = std::env::args().nth(1).expect("usage: trace_game <game_id>");
    let index = std::env::var("TTA_INDEX")
        .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../sources/bgo/index.tsv").into());
    let journals =
        std::env::var("TTA_JOURNALS").unwrap_or_else(|_| "/tmp/bgo-journals/journals".into());

    let mut games = tta::corpus::parse_index(&index).expect("index.tsv is readable");
    tta::corpus::fill_index_order(&mut games, &journals);
    let meta = games
        .into_iter()
        .find(|g| g.id == id)
        .unwrap_or_else(|| panic!("{id} is not in {index}"));

    let mut text = String::new();
    std::fs::File::open(format!("{journals}/{id}.tsv"))
        .unwrap_or_else(|e| panic!("{journals}/{id}.tsv: {e}"))
        .read_to_string(&mut text)
        .expect("the journal is valid UTF-8");

    let card_index = tta::corpus::build_card_index();
    let res = tta::replay_common::replay_game(&meta, &text, &card_index, true);
    match &res.mismatch {
        Some(m) => {
            println!("MISMATCH line={} round={}", m.lineno, m.round);
            match &m.kind {
                MismatchKind::IllegalMove { attempted, legal_moves } => {
                    println!("ATTEMPTED: {attempted}");
                    println!("LEGAL (first 4000 chars):");
                    println!("{}", &legal_moves[..legal_moves.len().min(4000)]);
                }
                MismatchKind::UnrecoverableHiddenInfo(s)
                | MismatchKind::StuckPending(s)
                | MismatchKind::ParserGap(s)
                | MismatchKind::EventPlanInfeasible(s) => println!("{:?}: {s}", m.kind),
            }
        }
        None => println!("no mismatch; completed={}", res.completed),
    }
    // The card index and the replayed state are large and about to be thrown
    // away; skipping their drop saves seconds on a tool run interactively.
    exit(0);
}
