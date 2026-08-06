//! Differential test for `bots::counting` against `tools/dump_counting.py`'s
//! output (`rust/tests/counting_fixtures/*.jsonl`, one line per sampled ply:
//! `{"ply": N, "players": {"0": {"live_ages": [...], "civil_outlook":
//! {name: float}, "event_pool_unknown": {name: int}, "event_pool_p": float},
//! ...}}` -- see that script's own doc comment for the exact dump shape).
//!
//! Same split as `rust/tests/board_yields.rs`: the ground-truth STATE for
//! each sampled ply comes from the ordinary differential fixtures
//! (`rust/tests/fixtures/*.jsonl`), loaded the same way `tests/
//! differential.rs` does; the dump file only records `counting.py`'s
//! ANSWERS, keyed by ply number.
//!
//! `civil_outlook`/`event_pool`'s `unknown` are dict-shaped in Python (every
//! comp-key present, including the ones priced at exactly `0.0`/`0`), so
//! this checks both directions -- every entry Rust returns is in the dump
//! AND every entry the dump has is in Rust's output -- rather than only
//! checking one side, which would pass a port that silently dropped entries.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tta::bots::counting;
use tta::cards::{Age, CardId};
use tta::fixtures::{self, Json, Record};
use tta::state::GameState;

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn expected_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/counting_fixtures")
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

// ------------------------------------------------------------ comparisons

/// `engine.cards.AGES` spellings -- the exact strings `json.dumps` wrote for
/// each `Age` (Python dumps `live_ages` as a plain list of these).
fn age_str(a: Age) -> &'static str {
    match a {
        Age::A => "A",
        Age::I => "I",
        Age::II => "II",
        Age::III => "III",
        Age::IV => "IV",
    }
}

fn live_ages_ok(got: &[Age], expected: Option<&Json>) -> bool {
    let expected_arr: &[Json] = expected.and_then(Json::as_arr).unwrap_or(&[]);
    if got.len() != expected_arr.len() {
        return false;
    }
    got.iter().zip(expected_arr.iter()).all(|(&a, v)| v.as_str() == Some(age_str(a)))
}

/// `{name: float}` (or `{name: int}`) dict comparison, both directions: same
/// key set, same value at every key within `eps`. `got` is name-resolved
/// through `CardId::by_name` (never the reverse) so an unknown Python card
/// name is a hard `panic!`, not a silent skip.
fn dict_ok(got: &[(CardId, f64)], expected: Option<&Json>, eps: f64) -> Result<(), String> {
    let obj = match expected {
        Some(Json::Obj(fields)) => fields.as_slice(),
        _ => &[],
    };
    if got.len() != obj.len() {
        return Err(format!("{} entries in rust, {} in python", got.len(), obj.len()));
    }
    for &(id, v) in got {
        let name = id.name();
        let Some(ev) = obj.iter().find(|(k, _)| k == name).map(|(_, v)| v) else {
            return Err(format!("{name}: rust has it, python does not"));
        };
        let Some(ev) = ev.as_f64() else {
            return Err(format!("{name}: python value is not a number"));
        };
        if (v - ev).abs() >= eps {
            return Err(format!("{name}: rust={v} python={ev}"));
        }
    }
    Ok(())
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
        let players = rec.get("players").expect("players object");
        for idx in 0..state.num_players {
            let Some(p_expected) = players.get(&idx.to_string()) else { continue };
            check_player(path, ply, state, idx, p_expected, report);
        }
    }
}

fn check_player(path: &Path, ply: u32, state: &GameState, idx: u8, expected: &Json, report: &mut Report) {
    let mut note = |what: String| {
        report.mismatches.push(format!("{}: ply {ply} player {idx}: {what}", path.display()));
    };

    report.checked += 1;
    let ages = counting::live_ages(state);
    if !live_ages_ok(&ages, expected.get("live_ages")) {
        note(format!("live_ages: rust={ages:?} python={:?}", expected.get("live_ages")));
    }

    report.checked += 1;
    let outlook = counting::civil_outlook(state, idx);
    if let Err(msg) = dict_ok(&outlook, expected.get("civil_outlook"), 1e-9) {
        note(format!("civil_outlook: {msg}"));
    }

    report.checked += 1;
    let (unknown, p) = counting::event_pool(state, idx);
    let unknown_f: Vec<(CardId, f64)> = unknown.iter().map(|&(id, n)| (id, n as f64)).collect();
    if let Err(msg) = dict_ok(&unknown_f, expected.get("event_pool_unknown"), 1e-9) {
        note(format!("event_pool unknown: {msg}"));
    }
    let expected_p = expected.get("event_pool_p").and_then(Json::as_f64).unwrap_or(f64::NAN);
    if (p - expected_p).abs() >= 1e-9 {
        note(format!("event_pool p: rust={p} python={expected_p}"));
    }
}

#[test]
fn counting_matches_python_on_sampled_fixture_states() {
    let dir = fixtures_dir();
    let edir = expected_dir();
    assert!(
        edir.is_dir(),
        "{} does not exist -- generate it with tools/dump_counting.py (see that \
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
    assert!(files_checked >= 3, "expected counting_fixtures for at least 2p/3p/4p, found {files_checked}");
    eprintln!(
        "counting differential: {files_checked} files, {} checks, {} mismatches",
        report.checked,
        report.mismatches.len()
    );
    assert!(
        report.mismatches.is_empty(),
        "{} counting mismatch(es):\n{}",
        report.mismatches.len(),
        report.mismatches.join("\n")
    );
}
