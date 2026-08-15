//! STEP-1 DIAGNOSTIC (earlymil task, not part of the shipped fix).
//!
//! Dumps the per-WeightKey contribution to `evaluate`'s score DELTA
//! (after-move minus before-move) for two real round-2 candidate moves --
//! Build{Bronze} (the Mine-kind starting tech) vs Build{Warriors} (the
//! Military-kind starting tech) -- from the exact same real game state, so
//! we can see WHICH weight keys make the unit win the comparison in a real
//! position, not just read the isolated `strength_marginal` formula.
//!
//! Not wired into any test suite; run manually:
//!   cargo run --profile difftest --bin dumpweights

use std::collections::HashMap;
use std::path::Path;

use tta::bots::weighted::eval::evaluate;
use tta::bots::weighted::rivals;
use tta::bots::weighted::weights::{Weights, WeightKey, PHASE_KEYS};
use tta::bots::weighted::{eval, features};
use tta::cards::CardId;
use tta::game;
use tta::legal;
use tta::moves::Move;

fn per_key_contrib(before: &tta::bots::weighted::features::Features, after: &tta::bots::weighted::features::Features, w: &Weights) -> Vec<(WeightKey, f64)> {
    let mut out: Vec<(WeightKey, f64)> = Vec::new();
    for &k in WeightKey::ALL {
        let wk = w.get(k);
        if wk == 0.0 {
            continue;
        }
        let d = after.get(k) - before.get(k);
        if d != 0.0 {
            out.push((k, wk * d));
        }
    }
    out
}

fn per_phase_key_contrib(before: &tta::bots::weighted::features::Features, after: &tta::bots::weighted::features::Features, w: &Weights, late: f64) -> Vec<(WeightKey, f64)> {
    let early = 1.0 - late;
    let mut out = Vec::new();
    for &k in PHASE_KEYS {
        let vb = before.get(k);
        let va = after.get(k);
        let we = w.get(k.early());
        let wl = w.get(k.late());
        let contrib_before = we * early * vb + wl * late * vb;
        let contrib_after = we * early * va + wl * late * va;
        let d = contrib_after - contrib_before;
        if d != 0.0 {
            out.push((k, d));
        }
    }
    out
}

fn main() {
    let w = eval::load_weights(Path::new("/private/tmp/rowdig/frozen_champion_2p.json"))
        .expect("frozen champion 2p must load");

    let mut state = game::new_game(2, 7);
    state.round = 2;
    state.players[0].resources = 5;
    // A unit build ALSO needs a free military action (`have_ma`,
    // `legal.rs:438`, distinct from `have_ca` which a Mine build needs) --
    // without this, Build(Warriors) is illegal even with cash and a free
    // worker, and only Build(Bronze) shows up in the legal list.
    state.players[0].military_actions = 1;

    let bronze = CardId::by_name("Bronze").expect("Bronze must exist");
    let warriors = CardId::by_name("Warriors").expect("Warriors must exist");

    let legal = legal::legal_moves(&state);
    let has_bronze = legal.as_slice().iter().any(|m| matches!(m, Move::Build { card } if *card == bronze));
    let has_warriors = legal.as_slice().iter().any(|m| matches!(m, Move::Build { card } if *card == warriors));
    println!("fixture: round={} p0.resources={} legal has Build(Bronze)={} Build(Warriors)={}", state.round, state.players[0].resources, has_bronze, has_warriors);

    let idx = 0u8;
    let ctx = rivals::rival_context(&state, idx, None, None);
    let before = features::features(&state, idx, Some(&ctx), Some(&w), false);
    let before_score = evaluate(&state, idx, &w, Some(&ctx), Some(&before));
    let late = tta::bots::weighted::horizon::lateness(&state);
    println!("lateness at this state = {late}");

    for (label, mv) in [("Build(Bronze=Mine)", Move::Build { card: bronze }), ("Build(Warriors=Military)", Move::Build { card: warriors })] {
        let mut trial = state.clone();
        tta::apply::apply(&mut trial, mv);
        let after = features::features(&trial, idx, Some(&ctx), Some(&w), false);
        let after_score = evaluate(&trial, idx, &w, Some(&ctx), Some(&after));
        println!("\n=== {label} ===");
        println!("total evaluate() delta = {:.4}  (before {:.4} -> after {:.4})", after_score - before_score, before_score, after_score);

        let mut flat = per_key_contrib(&before, &after, &w);
        flat.sort_by(|a, b| b.1.abs().partial_cmp(&a.1.abs()).unwrap());
        println!("-- flat WeightKey::ALL contributions (nonzero only) --");
        for (k, v) in &flat {
            println!("  {:?}: {:+.4}", k, v);
        }

        let mut phase = per_phase_key_contrib(&before, &after, &w, late);
        phase.sort_by(|a, b| b.1.abs().partial_cmp(&a.1.abs()).unwrap());
        println!("-- PHASE_KEYS blended contributions (nonzero only) --");
        for (k, v) in &phase {
            println!("  {:?}: {:+.4}", k, v);
        }

        let flat_sum: f64 = flat.iter().map(|(_, v)| v).sum();
        let phase_sum: f64 = phase.iter().map(|(_, v)| v).sum();
        println!("flat_sum={:.4} phase_sum={:.4} (identity-aware terms make up the rest of the total delta above, if any)", flat_sum, phase_sum);
    }

    let _ = HashMap::<WeightKey, f64>::new(); // silence unused import if trimmed later
}
