//! `takecharacterise` -- read-only diagnostic for the 48 "uncounted" takes.
//!
//! `bin/agreement.rs`'s `print_decision` computes
//! `human_rank = ranked.iter().position(|&(mv, _)| mv == d.human_move)`; when
//! the human's real move is ABSENT from the engine's own `legal_moves` list
//! the rank is `None` and the line prints `uncounted`. Every one of those 48
//! is a `Move::Take`, and -- by construction in
//! `replay_common::Replayer::try_apply_take` -- only the hand-full override
//! branch records a `Decision` (the wonder-in-progress and budget-only
//! branches call `apply::h_take`/`apply::take_card` directly and record
//! nothing), so each of the 48 was a take the engine called ILLEGAL and the
//! replayer knowingly overrode.
//!
//! This binary adds NO engine logic. For each target game it runs the real
//! `replay_game` (record_decisions: true), then for every recorded
//! `Take` decision it re-runs the SAME public legality primitives the engine
//! uses (`costs::take_gate` + `costs::take_rejection`) against the
//! decision's own pre-move `d.state` to NAME the exact rejecting condition,
//! and prints a board-position snapshot (age, round, actor, hand size vs
//! limit, spare CA, the target card, and the full card row). `costs.rs` and
//! `legal.rs` are read, never modified.
//!
//! Usage:
//! ```text
//! cargo run --release --bin takecharacterise -- \
//!     sources/bgo/index.tsv /private/tmp/bgo-journals/journals \
//!     <game_id> [game_id ...]
//! ```

use std::collections::HashMap;
use std::fs;

use tta::corpus::{build_card_index, parse_index, GameMeta, Color};
use tta::costs;
use tta::replay_common::{replay_game, Decision};

/// Render a `TakeRejection` as the exact legal.rs gate that fired.
fn reject_name(r: Option<costs::TakeRejection>) -> String {
    match r {
        None => "LEGAL (no rejection)".to_string(),
        Some(costs::TakeRejection::EmptySlot) => "EmptySlot".to_string(),
        Some(costs::TakeRejection::WonderBudget) => "WonderBudget".to_string(),
        Some(costs::TakeRejection::WonderInProgress) => "WonderInProgress".to_string(),
        Some(costs::TakeRejection::Budget) => "Budget".to_string(),
        Some(costs::TakeRejection::HandFull) => "HandFull".to_string(),
        Some(costs::TakeRejection::LeaderAgeTaken) => "LeaderAgeTaken".to_string(),
        Some(costs::TakeRejection::DuplicateCard) => "DuplicateCard".to_string(),
    }
}

/// A `Decision` is a take the engine called illegal and overrode iff its
/// `human_move` is a `Take` ABSENT from its own `legal_moves` list (the only
/// place the replayer records a take `Decision` with that property is the
/// hand-full override branch). Returns the slot, or `None` if not such a take.
fn uncounted_take_slot(d: &Decision) -> Option<usize> {
    // `if let`, not a wildcard `match` arm: the repo's Cargo.toml denies
    // `wildcard_enum_match_arm`, and `Move` has 30+ variants.
    let tta::Move::Take { slot } = d.human_move else {
        return None;
    };
    if d.legal_moves.contains(&d.human_move) {
        return None; // legal -- not one of the 48
    }
    Some(slot as usize)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        eprintln!("usage: takecharacterise <index.tsv> <journals_dir> <game_id> [game_id ...]");
        std::process::exit(2);
    }
    let index_path = &args[0];
    let journals_dir = &args[1];
    let ids: Vec<&str> = args[2..].iter().map(|s| s.as_str()).collect();

    let card_index = build_card_index();
    let games: Vec<GameMeta> = match parse_index(index_path) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("parse_index: {e}");
            std::process::exit(1);
        }
    };
    let by_id: HashMap<&str, &GameMeta> = games.iter().map(|g| (g.id.as_str(), g)).collect();

    let mut per_condition: HashMap<String, u32> = HashMap::new();
    let mut n_takes = 0u32;
    let mut n_games_done = 0u32;

    for id in ids {
        let Some(meta) = by_id.get(id) else {
            eprintln!("{id}: not found in index.tsv");
            continue;
        };
        let path = format!("{journals_dir}/{id}.tsv");
        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("{id}: read journal: {e}");
                continue;
            }
        };
        let result = replay_game(meta, &text, &card_index, true);
        n_games_done += 1;

        for d in &result.decisions {
            let Some(slot) = uncounted_take_slot(d) else {
                continue;
            };
            n_takes += 1;
            let state = &d.state;
            let actor = state.current;
            let p = &state.players[actor as usize];
            let gate = costs::take_gate(state, p, None);
            let rejection = costs::take_rejection(state, p, slot, &gate, None);

            let hand = p.hand_size_civil();
            let limit = costs::civil_hand_limit(state, p);
            let spare = costs::spare_ca(p);
            let row_cost = costs::row_cost(slot);
            let target = state.card_row[slot];
            let target_name = if target.is_none() {
                "<empty>".to_string()
            } else {
                format!("{} ({:?} {})", target.name(), target.kind(), target.level())
            };

            // Full card row: slot -> "Name(age)" or "."
            let mut row = Vec::with_capacity(state.card_row.len());
            for &cid in state.card_row.iter() {
                row.push(if cid.is_none() {
                    ".".to_string()
                } else {
                    format!("{}{}", cid.name(), cid.level())
                });
            }

            let color = Color::from_seat(actor).map(|c| c.as_str().to_string()).unwrap_or_else(|| format!("seat{actor}"));
            let cond = reject_name(rejection);
            *per_condition.entry(cond.clone()).or_insert(0) += 1;

            println!(
                "{id}\t{}p\tround {} ln {}\tactor {}\tslot {}\tcard {} (row_cost {})\tage {:?}\thand {hand}/{limit}\tspare_ca {spare}\treject={cond}",
                meta.players, state.round, d.lineno, color, slot, target_name, row_cost, state.age_civil
            );
            println!("    card_row: {}", row.join(" "));
        }
    }

    eprintln!("\n== summary: {n_takes} uncounted takes across {n_games_done} games ==");
    let mut v: Vec<(String, u32)> = per_condition.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    for (cond, n) in &v {
        eprintln!("  {cond}: {n}");
    }
}
