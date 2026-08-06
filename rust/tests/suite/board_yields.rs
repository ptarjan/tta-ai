//! `bots::board_yields` tests that do not depend on the Python-parity
//! fixture corpus (retired with the Python engine -- see `src/fixtures.rs`'s
//! module doc comment). This file used to also carry
//! `board_yields_matches_python_on_sampled_fixture_states`, a differential
//! test against `tools/dump_board_yields.py`'s output; that test and its
//! dump-reading plumbing are gone, but the two tests below were always
//! genuine rules/behaviour assertions rather than Python comparisons, so
//! they stay -- the mutation-safety test below now builds its own state via
//! `game::new_game` instead of loading a recorded fixture.

use tta::bots::board_yields as by;
use tta::bots::board_yields::{Feature, Kind};
use tta::cards::CardId;
use tta::game;

/// `_swap_stats`'s clone-based swap must never mutate the ORIGINAL state --
/// this is the Rust replacement for Python's "the trap" (`_swapped` mutating
/// `p.leader` behind `state_stats`' cache's back). Asserted directly: call
/// `board_yields` for a leader, then confirm the player's real leader field
/// (and a `state_stats` call on the untouched player) are unchanged.
#[test]
fn board_yields_never_mutates_the_state_it_was_asked_to_price() {
    let state = game::new_game(2, 1);
    let before_leader = state.players[0].leader;
    let before_gov = state.players[0].government;
    let before_wonders = state.players[0].completed_wonders.len();
    let before_stats = tta::effects::state_stats(&state, &state.players[0]);

    let einstein = CardId::by_name("Albert Einstein").unwrap();
    let _ = by::board_yields(einstein, &state, 0);
    let despotism = CardId::by_name("Despotism").unwrap();
    let _ = by::board_yields(despotism, &state, 0);
    let pyramids = CardId::by_name("Pyramids").unwrap();
    let _ = by::board_yields(pyramids, &state, 0);

    assert_eq!(state.players[0].leader, before_leader);
    assert_eq!(state.players[0].government, before_gov);
    assert_eq!(state.players[0].completed_wonders.len(), before_wonders);
    assert_eq!(tta::effects::state_stats(&state, &state.players[0]), before_stats);
}

/// `merge`'s whole reason to exist: two triples for the same `(feature,
/// kind)` are summed, not last-write-wins. Mirrors
/// `tests/test_board_yields.py`'s Gandhi-over-Churchill case structurally
/// (a synthetic pair of triples with the same feature/kind and opposite
/// sign), without needing a real board.
#[test]
fn merge_sums_same_feature_and_kind_rather_than_keeping_the_last() {
    let triples = vec![
        (Feature::CultureRate, 2.0, Kind::Gain),
        (Feature::Strength, 1.0, Kind::Gain),
        (Feature::CultureRate, -3.0, Kind::Gain),
    ];
    let merged = by::merge(triples);
    assert_eq!(merged, vec![(Feature::CultureRate, -1.0, Kind::Gain), (Feature::Strength, 1.0, Kind::Gain)]);
}

/// A triple whose merged amount lands exactly on zero is dropped -- mirrors
/// Python's `if merged[(f, kd)]` truthiness filter.
#[test]
fn merge_drops_a_triple_that_sums_to_exactly_zero() {
    let triples = vec![(Feature::Science, 5.0, Kind::Cost), (Feature::Science, -5.0, Kind::Cost)];
    assert!(by::merge(triples).is_empty());
}
