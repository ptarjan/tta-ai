//! Differential test against `tools/dump_fixtures.py`'s output
//! (DESIGN.md "How correctness is established").
//!
//! There is no `actions.rs` yet -- no `legal_moves`, no `apply`, no game
//! logic at all, only the type layer (`cards.rs`, `state.rs`, `moves.rs`).
//! So this cannot yet replay a game and diff states; what it CAN do, and
//! what it is FOR right now, is prove the type layer is complete against
//! real games before a porting worker starts writing logic against it:
//!
//!   * every move tag any real game produced parses into a `Move`
//!   * every card name any real game touched resolves to a `CardId`
//!   * nothing in a real fixture is a shape the type layer cannot represent
//!
//! A failure here is not "a test broke" -- it is "the type layer is missing
//! something a real game does", which is exactly the finding this harness
//! exists to surface before anyone ports logic against a type layer with a
//! hole in it.
//!
//! The state-replay assertions (legal-move-list equality, digest equality
//! after `apply`) are written but `#[ignore]`d: they turn on once
//! `actions.rs` lands and there is a `GameState::from_json` /
//! `legal_moves` / `apply` to check against these fixtures for real.

use std::path::{Path, PathBuf};

use tta::cards::CardId;
use tta::fixtures::{self, Record};

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Every fixture file, parsed. Panics with a precise `file:line: message`
/// (via `FixtureError`'s `Display`) on the first record this reader or the
/// type layer cannot make sense of -- that message IS the finding.
fn load_all() -> Vec<(PathBuf, Vec<Record>)> {
    let dir = fixtures_dir();
    let files = fixtures::fixture_files(&dir)
        .unwrap_or_else(|e| panic!("reading fixtures dir {}: {e}", dir.display()));
    assert!(
        !files.is_empty(),
        "no *.jsonl fixtures in {} -- generate some with tools/dump_fixtures.py \
         (see rust/tests/fixtures/README or the worker's report for the exact command)",
        dir.display()
    );
    files
        .into_iter()
        .map(|path| {
            let records = fixtures::read_fixture_file(&path)
                .unwrap_or_else(|e| panic!("{e}"));
            (path, records)
        })
        .collect()
}

/// The type layer is complete: every move tag and every card name that
/// `tools/dump_fixtures.py` recorded from real self-play games (across 2p,
/// 3p and 4p, both a greedy bot and a legality-fuzzing random bot, including
/// war, aggression, pacts, colonization auctions and defense -- the
/// rarer/harder-to-reach corners of the move space) is representable by
/// `Move` and `CardId`.
///
/// `fixtures::parse_move` and `fixtures::read_fixture_file` already fail
/// loudly (an `Err`, turned into a panic by `load_all`) on the first tag or
/// card name they cannot map, so simply parsing everything without a panic
/// IS the assertion. This test additionally counts what it saw so a
/// coverage regression (a fixture directory that quietly lost its war/pact/
/// colonize game) is visible in the test output rather than only in a diff.
#[test]
fn every_real_move_and_card_maps_onto_the_type_layer() {
    let all = load_all();
    let mut games = 0usize;
    let mut plies = 0usize;
    let mut tags_seen: std::collections::BTreeSet<&'static str> = Default::default();

    for (path, records) in &all {
        games += 1;
        let mut saw_header = false;
        let mut saw_footer = false;
        for rec in records {
            match rec {
                Record::Header(h) => {
                    saw_header = true;
                    assert!(
                        (2..=4).contains(&h.players),
                        "{}: header claims {}p, outside 2..=4",
                        path.display(),
                        h.players
                    );
                }
                Record::Ply(p) => {
                    plies += 1;
                    // Every legal move parsed (parse_move already ran in
                    // read_fixture_file); re-derive the tag purely for the
                    // coverage tally below.
                    assert!(
                        p.legal.iter().any(|m| moves_eq(m, &p.chosen)),
                        "{}: ply {}: chosen move is not a member of its own \
                         legal list -- python and this reader disagree about \
                         move identity",
                        path.display(),
                        p.ply
                    );
                    if let Some(card) = p.chosen.card() {
                        assert_ne!(
                            card,
                            CardId::NONE,
                            "{}: ply {}: chosen move names CardId::NONE",
                            path.display(),
                            p.ply
                        );
                    }
                    tags_seen.insert(tag_of(&p.chosen));
                }
                Record::Footer(_) => saw_footer = true,
            }
        }
        assert!(saw_header, "{}: no header record", path.display());
        assert!(saw_footer, "{}: no footer record", path.display());
    }

    eprintln!(
        "differential: {games} fixture files, {plies} plies, {} distinct move tags: {tags_seen:?}",
        tags_seen.len()
    );
    assert!(games >= 3, "expected fixtures for at least 2p/3p/4p, found {games} files");
}

/// Structural move equality good enough for "is `chosen` one of `legal`"
/// (`Move` derives `PartialEq`, but that is exactly what this calls --
/// spelled out as its own function only so the assertion message above can
/// name what it's really asking).
fn moves_eq(a: &tta::moves::Move, b: &tta::moves::Move) -> bool {
    a == b
}

/// A short move-shape name for the coverage tally in the test above.
/// Deliberately NOT the reverse of `fixtures::parse_move`'s tag table --
/// that table maps STRINGS to variants; this is Rust matching its own enum,
/// and the two are checked against each other by both existing and by
/// construction (a variant added to `Move` without a matching arm here is a
/// non-exhaustive-match compile error, same as in `moves.rs` itself).
fn tag_of(m: &tta::moves::Move) -> &'static str {
    use tta::moves::Move::*;
    match m {
        Take { .. } => "take",
        Build { .. } => "build",
        Develop { .. } => "develop",
        Upgrade { .. } => "upgrade",
        WonderStep { .. } => "wonder_step",
        Pop => "pop",
        PopFree => "pop_free",
        Revolution { .. } => "revolution",
        PlayLeader { .. } => "play_leader",
        PlayAction { .. } => "play_action",
        Destroy { .. } => "destroy",
        PlayTactic { .. } => "play_tactic",
        CopyTactic { .. } => "copy_tactic",
        Aggression { .. } => "aggression",
        War { .. } => "war",
        OfferPact { .. } => "offer_pact",
        CancelPact { .. } => "cancel_pact",
        PrepareEvent { .. } => "prepare_event",
        Bid { .. } => "bid",
        BidPass => "bid_pass",
        Defend { .. } => "defend",
        DefendDone => "defend_done",
        SendUnit { .. } => "send_unit",
        SendBonus { .. } => "send_bonus",
        SendDone => "send_done",
        Choose { .. } => "choose",
        Churchill { .. } => "churchill",
        EndTurn => "end_turn",
        PolPass => "pol_pass",
        Resign => "resign",
    }
}

/// Every ply's `digest` field is present and looks like a blake2b hex digest
/// (`tools/dump_fixtures.py`'s `state_digest`: 64 bytes -> 128 hex chars).
/// This cannot check the digest is CORRECT yet -- that needs `apply` -- but
/// a malformed digest would mean the fixture format itself has drifted from
/// what this reader expects, which is worth catching on its own.
#[test]
fn every_digest_is_well_formed() {
    for (path, records) in load_all() {
        for rec in records {
            if let Record::Ply(p) = rec {
                assert_eq!(
                    p.digest.len(),
                    128,
                    "{}: ply {}: digest {:?} is not 128 hex chars",
                    path.display(),
                    p.ply,
                    p.digest
                );
                assert!(
                    p.digest.chars().all(|c| c.is_ascii_hexdigit()),
                    "{}: ply {}: digest {:?} has non-hex characters",
                    path.display(),
                    p.ply,
                    p.digest
                );
            }
        }
    }
}

/// The last `Ply` record in every fixture carries a `state` snapshot ("always
/// include the final state" -- `tools/dump_fixtures.py`'s header docstring).
#[test]
fn every_fixture_ends_with_a_state_snapshot() {
    for (path, records) in load_all() {
        let last_ply = records.iter().rev().find_map(|r| match r {
            Record::Ply(p) => Some(p),
            _ => None,
        });
        let last_ply = last_ply.unwrap_or_else(|| panic!("{}: no ply records", path.display()));
        assert!(
            last_ply.state.is_some(),
            "{}: last ply ({}) has no state snapshot",
            path.display(),
            last_ply.ply
        );
    }
}

// ===================================================================
// State-replay assertions. These are the real point of the harness and are
// deliberately written now, against the types that exist, so the FIRST thing
// a worker landing `actions.rs` does is delete two `#[ignore]` attributes and
// get a real green/red signal -- not invent a test harness from scratch.
// ===================================================================

/// Rust's `legal_moves(state)` must produce the SAME moves in the SAME order
/// as the Python fixture's `legal` list, for every ply. Move ordering is
/// part of the contract (DESIGN.md): the bots break ties by index, so a
/// reordered list silently changes play.
#[test]
#[ignore = "turns on when actions.rs lands: needs GameState::from_json + legal_moves"]
fn legal_moves_match_python_order() {
    unimplemented!("replay each fixture's ply through legal_moves() and assert Vec<Move> equality");
}

/// Rust's `apply(state, chosen)` must produce a state whose digest -- computed
/// the same way `tools/dump_fixtures.py::state_digest` does -- matches the
/// fixture's `digest` field, for every ply. A divergence here should name one
/// ply and, once there is a Rust-side structural differ (this crate's
/// equivalent of `engine/statediff.py`), one field.
#[test]
#[ignore = "turns on when actions.rs lands: needs GameState::from_json + apply + a digest fn"]
fn apply_matches_python_digest_stream() {
    unimplemented!("replay each fixture's chosen move through apply() and assert digest equality");
}
