//! `tacticprice` -- throwaway measurement probe for the Tactic-pricing
//! blackout investigation (2026-08-25). For every Tactic `CardId`, prints
//! `card_potential` under the 2p/3p/4p trained champions at a representative
//! mid-game board, plus a handful of Aggression/War cards for comparison
//! (same `if credit != 0.0` dispatch shape in `card_potential_core`).
//!
//! State construction: `game::new_game(n, 42)` advanced with
//! `RandomBot::new(1)` picking every move (`allow_resign = false`, so games
//! run long rather than resigning out) until 40 `Move::EndTurn`s have been
//! applied or the game ends first -- a few rounds in, board established,
//! same recipe for all three player counts so the comparison is apples to
//! apples. Not claimed "typical", just deterministic and reproducible.
//!
//! ```text
//! cargo run --profile difftest --bin tacticprice
//! ```
use std::path::Path;

use tta::bots::board_yields::Baseline;
use tta::bots::greedy::RandomBot;
use tta::bots::weighted::cards::{card_potential, tactic_value};
use tta::bots::weighted::eval::load_weights;
use tta::effects;
use tta::bots::weighted::weights::WeightKey;
use tta::card_table::NUM_CARDS;
use tta::cards::{CardId, CardType};
use tta::game;
use tta::state::GameState;

fn mid_game_state(num_players: u8) -> GameState {
    let mut state = game::new_game(num_players, 42);
    let mut bot = RandomBot::new(1);
    let mut end_turns = 0;
    while end_turns < 40 && !game::is_over(&state) {
        let mv = bot.pick(&state);
        if mv == tta::moves::Move::EndTurn {
            end_turns += 1;
        }
        game::step(&mut state, mv);
    }
    state
}

fn all_cards_of_kind(kind: CardType) -> Vec<CardId> {
    (0..NUM_CARDS as u16).map(CardId).filter(|id| id.kind() == kind).collect()
}

fn price_table(label: &str, kind: CardType, cap: Option<usize>) {
    let w2 = load_weights(Path::new("../experiments/rust_champion_2p.json")).expect("2p weights");
    let w3 = load_weights(Path::new("../experiments/rust_champion_3p.json")).expect("3p weights");
    let w4 = load_weights(Path::new("../experiments/rust_champion_4p.json")).expect("4p weights");

    let st2 = mid_game_state(2);
    let st3 = mid_game_state(3);
    let st4 = mid_game_state(4);
    let b2 = Baseline::at(&st2, 0);
    let b3 = Baseline::at(&st3, 0);
    let b4 = Baseline::at(&st4, 0);

    let mut scratch = Vec::new();
    println!("\n=== {label} ===");
    println!("{:<24} {:>6} {:>12} {:>12} {:>12}", "card", "id", "price@2p", "price@3p", "price@4p");
    let mut ids = all_cards_of_kind(kind);
    if let Some(n) = cap {
        ids.truncate(n);
    }
    for id in ids {
        let p2 = card_potential(id, &w2, Some(&b2), None, &mut scratch);
        let p3 = card_potential(id, &w3, Some(&b3), None, &mut scratch);
        let p4 = card_potential(id, &w4, Some(&b4), None, &mut scratch);
        println!("{:<24} {:>6} {:>12.6} {:>12.6} {:>12.6}", id.name(), id.0, p2, p3, p4);
    }
}

fn main() {
    let w2 = load_weights(Path::new("../experiments/rust_champion_2p.json")).expect("2p weights");
    let w3 = load_weights(Path::new("../experiments/rust_champion_3p.json")).expect("3p weights");
    let w4 = load_weights(Path::new("../experiments/rust_champion_4p.json")).expect("4p weights");
    println!(
        "tactic_board_credit  2p={} 3p={} 4p={}",
        w2.get(WeightKey::TacticBoardCredit),
        w3.get(WeightKey::TacticBoardCredit),
        w4.get(WeightKey::TacticBoardCredit)
    );
    println!(
        "aggression_board_credit  2p={} 3p={} 4p={}",
        w2.get(WeightKey::AggressionBoardCredit),
        w3.get(WeightKey::AggressionBoardCredit),
        w4.get(WeightKey::AggressionBoardCredit)
    );
    println!(
        "war_board_credit  2p={} 3p={} 4p={}",
        w2.get(WeightKey::WarBoardCredit),
        w3.get(WeightKey::WarBoardCredit),
        w4.get(WeightKey::WarBoardCredit)
    );

    price_table("Tactic (all)", CardType::Tactic, None);
    price_table("Aggression (first 6)", CardType::Aggression, Some(6));
    price_table("War (first 6)", CardType::War, Some(6));

    println!(
        "\ntactic_reach_credit (post dominance_repair)  2p={} 3p={} 4p={}",
        w2.get(WeightKey::TacticReachCredit),
        w3.get(WeightKey::TacticReachCredit),
        w4.get(WeightKey::TacticReachCredit),
    );

    println!("\n=== diagnostic: why is tactic_value 0.0 at 3p? ===");
    let st3 = mid_game_state(3);
    let cur = effects::army_strength(&st3.players[0]);
    println!("army_strength(player0 @3p baseline) = {cur}");
    let mut ids = all_cards_of_kind(CardType::Tactic);
    ids.truncate(5);
    for id in ids {
        let (val, armies, short_by_type) = effects::tactic_shortfall(&st3.players[0], id);
        let gain = (val - cur).max(0);
        let tv = tactic_value(id, &st3, 0, &w3);
        println!(
            "{:<20} val={val:>4} armies={armies:>2} short={short_by_type:?} gain={gain:>4} tactic_value={tv:.6}",
            id.name()
        );
    }
}
