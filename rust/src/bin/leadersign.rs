//! `leadersign` -- one-off analysis binary for the LEADERSIGN investigation:
//! does `cards::card_potential`'s board-swap pricing invert sign for leaders
//! (and other single-slot swap types) when `card_board_credit +
//! board_credit_key(id)` (the effective scale on the swap diff) is negative?
//!
//! For a fixed deterministic 2p round-1 board, prints, per leader:
//!   - the raw `sum_board_triples(board_yields(id, board), w)` diff (what the
//!     leader is worth on the board, sign alone -- positive means helpful)
//!   - `credit_board`, the effective multiplier `card_board_credit +
//!     card_board_leader`
//!   - the final `card_potential` result (what the evaluator actually prices
//!     it at)
//!
//! ```text
//! cargo run --profile difftest --bin leadersign -- path/to/weights.json
//! ```
//!
//! No `--weights` defaults to the built-in `Weights::default()` so the tool
//! also works with no argument at all, but the interesting case is always a
//! trained champion file, since `Weights::default()` prices nothing (every
//! `card_board_*` key defaults to 0.0).

use std::path::Path;

use tta::bots::board_yields::{self, Baseline};
use tta::bots::weighted::cards::{board_credit_key, card_potential, sum_board_triples};
use tta::bots::weighted::eval::load_weights;
use tta::bots::weighted::weights::{WeightKey, Weights};
use tta::cards::CardId;
use tta::game;

const LEADERS: &[&str] =
    &["Hammurabi", "Julius Caesar", "Aristotle", "Alexander the Great", "Leonardo da Vinci", "Genghis Khan"];

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let w = match argv.first() {
        Some(p) => load_weights(Path::new(p)).unwrap_or_else(|e| {
            eprintln!("leadersign: {e}");
            std::process::exit(1);
        }),
        None => Weights::default(),
    };

    let base = w.get(WeightKey::CardBoardCredit);
    println!("card_board_credit (base)   = {base}");
    println!("card_board_leader           = {}", w.get(WeightKey::CardBoardLeader));
    println!("card_board_bonus            = {}", w.get(WeightKey::CardBoardBonus));
    // `card_board_wonder`/`card_board_government`/`card_board_action` were
    // retired 2026-08-13 (SIGNAUDIT.txt) -- see `weights::RETIRED_KEYS`.
    // Their pricing now flows entirely through `wonder_board_credit`/
    // `gov_board_credit`/`action_board_credit`'s own dedicated functions,
    // which this tool never priced (it is specifically about the swap-diff
    // path `board_credit_key` still covers: Leader and Bonus).
    println!(
        "wonder_board_credit         = {}",
        w.get(WeightKey::WonderBoardCredit)
    );
    println!();

    // Deterministic 2p round-1 board -- same seed `cards.rs`'s own tests use
    // for a fresh-game baseline (see e.g. `card_potential_dispatch_credits_
    // are_real_weight_keys`'s neighbours in that file).
    let state = game::new_game(2, 42);
    let baseline = Baseline::at(&state, 0);
    let mut scratch = Vec::new();

    println!(
        "{:<22} {:>14} {:>14} {:>14}",
        "leader", "raw_diff", "credit_board", "card_potential"
    );
    for name in LEADERS {
        let Some(id) = CardId::by_name(name) else {
            println!("{name:<22} (not found in card table)");
            continue;
        };
        let key = board_credit_key(id).expect("leaders always have a board credit key");
        let credit_board = base + w.get(key);
        let raw_diff = match board_yields::board_yields(id, &baseline) {
            Some(swap) => sum_board_triples(&swap, &w),
            None => f64::NAN,
        };
        let potential = card_potential(id, &w, Some(&baseline), None, &mut scratch);
        println!("{name:<22} {raw_diff:>14.4} {credit_board:>14.4} {potential:>14.4}");
    }
}
