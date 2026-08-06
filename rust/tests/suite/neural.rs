//! Differential test for `bots::neural::encode::encode` against `tools/
//! dump_neural.py`'s output (`rust/tests/neural_fixtures/*.jsonl`, one line
//! per sampled ply: `{"ply": N, "players": {"0": [f64, ...], ...}}` -- see
//! that script's own doc comment for the exact dump shape).
//!
//! Same split as `rust/tests/weighted_features.rs`: the ground-truth STATE
//! for each sampled ply comes from the ordinary differential fixtures
//! (`rust/tests/fixtures/*.jsonl`); the dump file only records `encode()`'s
//! ANSWER, keyed by ply number and player index.
//!
//! Unlike `weighted_features.rs` (whose coordinates are NAMED `WeightKey`s,
//! compared by name), `encode()`'s output is POSITIONAL -- a flat
//! `Vec<f64>`, no names -- so this test compares index by index and checks
//! the LENGTH first: a length mismatch would make every later index compare
//! against the wrong coordinate, so it is reported as its own failure rather
//! than cascading into thousands of misleading per-coordinate mismatches.
//!
//! Every sampled state is checked for EVERY live player, and every one of
//! the (up to) 1907 coordinates on each -- no coordinate sampling, matching
//! `weighted_features.rs`'s own reasoning: `encode()` is the coordinate
//! source for the entire value network, so a sampled subset of STATES is
//! fine (`tools/dump_neural.py`'s stride) but a sampled subset of
//! COORDINATES is not.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tta::bots::neural::encode::encode;
use tta::fixtures::{self, Json, Record};
use tta::state::GameState;

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn expected_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/neural_fixtures")
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

fn nearly(a: f64, b: f64, eps: f64) -> bool {
    (a - b).abs() < eps
}

#[derive(Default)]
struct Report {
    checked: usize,
    mismatches: Vec<String>,
}

fn check_player(path: &Path, ply: u32, state: &GameState, idx: u8, expected: &Json, report: &mut Report) {
    let mut note = |what: String| {
        report.mismatches.push(format!("{}: ply {ply} player {idx}: {what}", path.display()));
    };

    let want: &[Json] = match expected {
        Json::Arr(items) => items.as_slice(),
        _ => {
            note("python dump value is not an array".to_string());
            return;
        }
    };

    let got = encode(state, idx);

    if got.len() != want.len() {
        report.checked += 1;
        note(format!("length mismatch: rust={} python={}", got.len(), want.len()));
        return;
    }

    for (i, (g, w)) in got.iter().zip(want.iter()).enumerate() {
        report.checked += 1;
        let wv = w.as_f64().unwrap_or_else(|| panic!("{}: ply {ply} player {idx} coord {i}: not a number", path.display()));
        if !nearly(*g, wv, 1e-6) {
            note(format!("coord {i}: rust={g} python={wv}"));
        }
    }
}

fn check_file(path: &Path, expected_path: &Path, report: &mut Report) {
    let states = load_states(path);
    let text = std::fs::read_to_string(expected_path).unwrap_or_else(|e| panic!("reading {}: {e}", expected_path.display()));
    for (lineno, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let rec = fixtures::parse_json(line).unwrap_or_else(|e| panic!("{}:{}: {e}", expected_path.display(), lineno + 1));
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

#[test]
fn encode_matches_python_on_every_sampled_state_and_coordinate() {
    let dir = fixtures_dir();
    let edir = expected_dir();
    assert!(
        edir.is_dir(),
        "{} does not exist -- generate it with tools/dump_neural.py",
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
    assert!(files_checked >= 3, "expected neural_fixtures for at least 2p/3p/4p, found {files_checked}");
    eprintln!(
        "neural encode differential: {files_checked} files, {} checks, {} mismatches",
        report.checked,
        report.mismatches.len()
    );
    assert!(
        report.mismatches.is_empty(),
        "{} neural encode mismatch(es):\n{}",
        report.mismatches.len(),
        report.mismatches[..report.mismatches.len().min(50)].join("\n")
    );
}
