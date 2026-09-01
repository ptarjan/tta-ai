//! `farmgap` -- exact per-[`WeightKey`] decomposition of the champion's
//! Farm-vs-Mine score gap, for the "why does the champion build Mine and
//! never Farm" question (Agriculture/Bronze at 2p).
//!
//! Reuses [`eval::candidate_features`] -- the SAME vector `WeightedBot::
//! choose`/`rank_moves` dot against `w` -- rather than hand-replicating
//! `evaluate()` term by term, so there is no separate arithmetic to get
//! wrong: `score(Farm) - score(Mine) = sum_k w[k] * (phi_Farm[k] -
//! phi_Mine[k])` exactly, by construction.
//!
//! # Method
//!
//! Self-play mirror match (`WeightedBot::choose` on both seats, the frozen
//! weights at `/tmp/farmgap/champ.json`) across a handful of seeds. At every
//! own-turn decision point (`Move::EndTurn` legal, matching `openerprobe`'s
//! convention for "the player's own choice" as opposed to answering a
//! pending sub-decision) where the legal-move list contains at least one
//! `Build` of a `CardType::Farm` card AND at least one `Build` of a
//! `CardType::Mine` card, this dumps the top-15-by-|contribution| WeightKey
//! table for that single comparison and moves on. Stops once it has 3
//! opener-window (round <= 4) and 3 mid-game (round in 8..=14) points, or
//! runs out of seeds.
//!
//! Diagnosis only -- no weight is read for anything but printing, and
//! nothing in `features.rs`/`cards.rs`/`weights.rs` is touched.
//!
//! ```text
//! cargo run --release --bin farmgap
//! ```
use std::path::Path;

use tta::bots::weighted::eval::{candidate_features, dot, load_weights};
use tta::bots::weighted::eval::WeightedBot;
use tta::bots::weighted::weights::{WeightKey, Weights};
use tta::cards::CardType;
use tta::game;
use tta::legal;
use tta::moves::Move;

/// One reconciled Farm-vs-Mine comparison at a real decision point.
struct Point {
    seed: u64,
    seat: u8,
    round: u16,
    farm: Move,
    mine: Move,
    /// `(key, w_k, phi_farm_k, phi_mine_k, contribution)`, sorted by
    /// `|contribution|` descending, truncated to the top 15.
    rows: Vec<(WeightKey, f64, f64, f64, f64)>,
    total_gap: f64,
    /// Independent check: `dot(w, phi_farm) - dot(w, phi_mine)`, which must
    /// equal `total_gap` (sum of the printed contributions) to within
    /// float noise -- printed so the table is visibly self-consistent.
    dot_gap: f64,
}

fn first_build_of(moves: &[Move], kind: CardType) -> Option<Move> {
    moves.iter().copied().find(|m| matches!(m, Move::Build { card } if card.kind() == kind))
}

/// Walk one self-play game, yielding every own-turn decision point where
/// both a Farm build and a Mine build are legal, in a `round_lo..=round_hi`
/// window. Stops early once `want` points are collected.
fn scan_game(seed: u64, weights: &Weights, round_lo: u16, round_hi: u16, want: usize, out: &mut Vec<Point>) {
    let bot = WeightedBot::new(*weights);
    let mut state = game::new_game(2, seed);
    while !game::is_over(&state) && out.len() < want {
        let move_list = legal::legal_moves(&state);
        let moves = move_list.as_slice();
        if moves.is_empty() {
            break;
        }
        let is_own_turn = moves.iter().any(|m| matches!(m, Move::EndTurn));
        if is_own_turn && (round_lo..=round_hi).contains(&state.round) {
            if let (Some(farm), Some(mine)) =
                (first_build_of(moves, CardType::Farm), first_build_of(moves, CardType::Mine))
            {
                let cf = candidate_features(&state, moves, false, weights);
                let phi_farm = cf.iter().find(|(m, _)| *m == farm).map(|(_, f)| f.clone());
                let phi_mine = cf.iter().find(|(m, _)| *m == mine).map(|(_, f)| f.clone());
                if let (Some(phi_farm), Some(phi_mine)) = (phi_farm, phi_mine) {
                    let mut rows: Vec<(WeightKey, f64, f64, f64, f64)> = WeightKey::ALL
                        .iter()
                        .enumerate()
                        .map(|(i, &k)| {
                            let contrib = weights.get(k) * (phi_farm[i] - phi_mine[i]);
                            (k, weights.get(k), phi_farm[i], phi_mine[i], contrib)
                        })
                        .collect();
                    rows.sort_by(|a, b| b.4.abs().partial_cmp(&a.4.abs()).expect("no NaN contributions"));
                    let total_gap: f64 = rows.iter().map(|r| r.4).sum();
                    let dot_gap = dot(weights, &phi_farm) - dot(weights, &phi_mine);
                    rows.truncate(15);
                    out.push(Point {
                        seed,
                        seat: state.decider(),
                        round: state.round,
                        farm,
                        mine,
                        rows,
                        total_gap,
                        dot_gap,
                    });
                }
            }
        }
        let mv = bot.choose(&state, moves);
        game::step(&mut state, mv);
    }
}

fn print_point(label: &str, p: &Point) {
    println!(
        "\n=== {label}: seed={} seat={} round={} ===",
        p.seed, p.seat, p.round
    );
    println!("candidates: farm={:?}  mine={:?}", p.farm, p.mine);
    println!(
        "{:<24} {:>14} {:>12} {:>12} {:>14}",
        "WeightKey", "w_k", "phi_Farm", "phi_Mine", "w_k*(dF-dM)"
    );
    for (k, w, pf, pm, c) in &p.rows {
        println!("{:<24} {:>14.6} {:>12.6} {:>12.6} {:>14.6}", format!("{k:?}"), w, pf, pm, c);
    }
    println!(
        "TOTAL (sum of printed rows only, may be < full total if >15 nonzero): {:.6}",
        p.rows.iter().map(|r| r.4).sum::<f64>()
    );
    println!("TOTAL GAP score(Farm)-score(Mine) [full-vector sum]: {:.6}", p.total_gap);
    println!("independent dot(w,phi_Farm)-dot(w,phi_Mine):          {:.6}", p.dot_gap);
    println!("reconciliation residual: {:.3e}", (p.total_gap - p.dot_gap).abs());
}

fn main() {
    let path = Path::new("/tmp/farmgap/champ.json");
    let weights = load_weights(path).expect("load frozen champion weights");

    let mut opener: Vec<Point> = Vec::new();
    let mut midgame: Vec<Point> = Vec::new();
    for seed in 1u64..200 {
        if opener.len() >= 3 {
            break;
        }
        scan_game(seed, &weights, 2, 4, 3, &mut opener);
    }
    for seed in 1u64..200 {
        if midgame.len() >= 3 {
            break;
        }
        scan_game(seed, &weights, 8, 14, 3, &mut midgame);
    }

    println!("frozen champion: {} (gen field read separately -- see /tmp/farmgap/champ.json)", path.display());
    println!("opener-window points found: {}", opener.len());
    println!("mid-game-window points found: {}", midgame.len());

    for (i, p) in opener.iter().enumerate() {
        print_point(&format!("OPENER point {}", i + 1), p);
    }
    for (i, p) in midgame.iter().enumerate() {
        print_point(&format!("MID-GAME point {}", i + 1), p);
    }
}
