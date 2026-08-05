//! Differential test for `bots::book::BookBot` against `tools/dump_book.py`'s
//! output (`rust/tests/book_fixtures/*.jsonl`, one line per sampled ply:
//! `{"ply": N, "phase": "actions"|"politics"|"done", "pending": bool,
//! "v1": [tag, ...] | null, "v2": [tag, ...] | null}` -- see that script's
//! own doc comment for the exact dump shape and why every pending ply is
//! included unconditionally rather than only a strided sample).
//!
//! Same split as `rust/tests/counting.rs`: the ground-truth STATE for each
//! sampled ply comes from the ordinary differential fixtures (`rust/tests/
//! fixtures/*.jsonl`); the dump file only records `book.py`'s ANSWERS, keyed
//! by ply number. For each ply this computes `crate::legal::legal_moves`
//! (the SAME move list Python's `engine.actions.legal_moves` produced when
//! the dump was made -- move-list ORDER is itself part of the differential
//! test elsewhere in this port, see `moves.rs`'s own doc comment) and runs
//! `BookBot::choose` at both `version=1` and `version=2`, checked against
//! `v1`/`v2` independently so a version-specific regression cannot hide
//! behind the other version's agreement.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tta::bots::book::{BookBot, V2Tunables};
use tta::fixtures::{self, Json, Record};
use tta::moves::{Move, PactSide};
use tta::state::GameState;

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn expected_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/book_fixtures")
}

fn load_states(path: &Path) -> HashMap<u32, GameState> {
    let records = fixtures::read_fixture_file(path).unwrap_or_else(|e| panic!("{e}"));
    let mut out = HashMap::new();
    for rec in records {
        if let Record::Ply(p) = rec {
            if let Some(json) = &p.state {
                if let Ok(s) = GameState::from_json(json) {
                    out.insert(p.ply, s);
                }
            }
        }
    }
    out
}

// ------------------------------------------------------------ move matching

/// Does `mv` encode the same move Python dumped as `expected` (a JSON array
/// `[tag, ...args]`, or `null` if Python's `choose` returned nothing to
/// offer -- which cannot happen here since `dump_book.py` skips any ply
/// where `legal_moves` was empty)? Every `Move` variant is listed explicitly
/// -- an unmatched variant is a compile error via the exhaustive `match`,
/// not a silently-approved guess.
fn move_matches(mv: Move, expected: &Json) -> Result<(), String> {
    let arr = expected.as_arr().ok_or_else(|| format!("expected move is not an array: {expected:?}"))?;
    let tag = arr.first().and_then(Json::as_str).unwrap_or("");
    let args = &arr[1.min(arr.len())..];
    let name_at = |i: usize| -> Option<&str> { args.get(i).and_then(Json::as_str) };
    let num_at = |i: usize| -> Option<f64> { args.get(i).and_then(Json::as_f64) };

    let ok = match mv {
        Move::Take { slot } => tag == "take" && num_at(0) == Some(slot as f64),
        Move::Build { card } => tag == "build" && name_at(0) == Some(card.name()),
        Move::Develop { card } => tag == "develop" && name_at(0) == Some(card.name()),
        Move::Upgrade { from, to } => {
            tag == "upgrade" && name_at(0) == Some(from.name()) && name_at(1) == Some(to.name())
        }
        Move::WonderStep { steps } => tag == "wonder_step" && num_at(0) == Some(steps as f64),
        Move::Pop => tag == "pop",
        Move::PopFree => tag == "pop_free",
        Move::Revolution { card } => tag == "revolution" && name_at(0) == Some(card.name()),
        Move::PlayLeader { card } => tag == "play_leader" && name_at(0) == Some(card.name()),
        Move::PlayAction { card } => tag == "play_action" && name_at(0) == Some(card.name()),
        Move::Destroy { card } => tag == "destroy" && name_at(0) == Some(card.name()),
        Move::PlayTactic { card } => tag == "play_tactic" && name_at(0) == Some(card.name()),
        Move::CopyTactic { card } => tag == "copy_tactic" && name_at(0) == Some(card.name()),
        Move::Aggression { card, target } => {
            tag == "aggression" && name_at(0) == Some(card.name()) && num_at(1) == Some(target as f64)
        }
        Move::War { card, target } => {
            tag == "war" && name_at(0) == Some(card.name()) && num_at(1) == Some(target as f64)
        }
        Move::OfferPact { card, target, side } => {
            let side_str = match side {
                PactSide::Unspecified => "",
                PactSide::A => "A",
                PactSide::B => "B",
            };
            tag == "offer_pact"
                && name_at(0) == Some(card.name())
                && num_at(1) == Some(target as f64)
                && name_at(2) == Some(side_str)
        }
        Move::CancelPact { owner } => tag == "cancel_pact" && num_at(0) == Some(owner as f64),
        Move::PrepareEvent { card } => tag == "prepare_event" && name_at(0) == Some(card.name()),
        Move::RemoveLeaderYellow => tag == "remove_leader_yellow",
        Move::ColumbusColonize { card } => tag == "columbus_colonize" && name_at(0) == Some(card.name()),
        Move::Barbarossa { card } => tag == "barbarossa" && name_at(0) == Some(card.name()),
        Move::BachTheater { from, to } => {
            tag == "bach_theater" && name_at(0) == Some(from.name()) && name_at(1) == Some(to.name())
        }
        Move::Bid { n } => tag == "bid" && num_at(0) == Some(n as f64),
        Move::BidPass => tag == "bid_pass",
        Move::Defend { card } => tag == "defend" && name_at(0) == Some(card.name()),
        Move::DefendDone => tag == "defend_done",
        Move::SendUnit { card } => tag == "send_unit" && name_at(0) == Some(card.name()),
        Move::SendBonus { card } => tag == "send_bonus" && name_at(0) == Some(card.name()),
        Move::SendDiscard { card } => tag == "send_discard" && name_at(0) == Some(card.name()),
        Move::SendDone => tag == "send_done",
        Move::Choose { n } => tag == "choose" && num_at(0) == Some(n as f64),
        // book.py never plays Churchill (see `bots::book`'s module doc
        // comment); no Python tuple shape exists to compare against.
        Move::Churchill { .. } => false,
        Move::EndTurn => tag == "end_turn",
        Move::PolPass => tag == "pol_pass",
        Move::Resign => tag == "resign",
    };
    if ok {
        Ok(())
    } else {
        Err(format!("rust chose {mv:?}, python chose {expected:?}"))
    }
}

// ------------------------------------------------------------------ driver

#[derive(Default)]
struct Report {
    checked: usize,
    mismatches: Vec<String>,
}

fn check_file(path: &Path, expected_path: &Path, report: &mut Report) {
    let states = load_states(path);
    let text = std::fs::read_to_string(expected_path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", expected_path.display()));
    let bot1 = BookBot { version: 1, tunables: V2Tunables::default() };
    let bot2 = BookBot { version: 2, tunables: V2Tunables::default() };
    for (lineno, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let rec = fixtures::parse_json(line)
            .unwrap_or_else(|e| panic!("{}:{}: {e}", expected_path.display(), lineno + 1));
        let ply = rec.get("ply").and_then(Json::as_f64).unwrap_or(-1.0) as u32;
        let Some(state) = states.get(&ply) else {
            panic!("{}: ply {ply} has no matching state in {}", expected_path.display(), path.display());
        };
        let moves = tta::legal::legal_moves(state);
        assert!(!moves.is_empty(), "{}: ply {ply}: legal_moves is empty", path.display());

        for (version, bot, key) in [(1u8, &bot1, "v1"), (2u8, &bot2, "v2")] {
            report.checked += 1;
            let Some(expected_mv) = rec.get(key) else {
                report.mismatches.push(format!("{}: ply {ply}: dump has no {key:?} key", path.display()));
                continue;
            };
            if matches!(expected_mv, Json::Null) {
                report.mismatches.push(format!(
                    "{}: ply {ply}: python {key} is null but legal_moves was non-empty",
                    path.display()
                ));
                continue;
            }
            let mv = bot.choose(state, moves.as_slice());
            if let Err(msg) = move_matches(mv, expected_mv) {
                report.mismatches.push(format!("{}: ply {ply} version={version}: {msg}", path.display()));
            }
        }
    }
}

#[test]
fn book_bot_matches_python_on_sampled_fixture_states() {
    let dir = fixtures_dir();
    let edir = expected_dir();
    assert!(
        edir.is_dir(),
        "{} does not exist -- generate it with tools/dump_book.py (see that \
         script's doc comment, or this file's own doc comment)",
        edir.display()
    );
    let files = fixtures::fixture_files(&dir).unwrap_or_else(|e| panic!("{e}"));
    assert!(!files.is_empty(), "no fixtures in {}", dir.display());

    let mut report = Report::default();
    let mut files_checked = 0usize;
    for path in &files {
        let expected_path = edir.join(path.file_name().unwrap());
        if !expected_path.exists() {
            continue;
        }
        files_checked += 1;
        check_file(path, &expected_path, &mut report);
    }
    assert!(files_checked >= 3, "expected book_fixtures for at least 2p/3p/4p, found {files_checked}");
    eprintln!(
        "book differential: {files_checked} files, {} checks, {} mismatches",
        report.checked,
        report.mismatches.len()
    );
    assert!(
        report.mismatches.is_empty(),
        "{} book mismatch(es):\n{}",
        report.mismatches.len(),
        report.mismatches.join("\n")
    );
}
