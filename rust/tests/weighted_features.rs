//! Differential test for `bots::weighted::features` against `tools/
//! dump_weighted_features.py`'s output (`rust/tests/
//! weighted_features_fixtures/*.jsonl`, one line per sampled ply:
//! `{"ply": N, "players": {"0": {<every features() key>: f64, ...}, ...}}`
//! -- see that script's own doc comment for the exact dump shape).
//!
//! Same split as `rust/tests/weighted_events.rs`/`weighted_rivals.rs`: the
//! ground-truth STATE for each sampled ply comes from the ordinary
//! differential fixtures (`rust/tests/fixtures/*.jsonl`); the dump file only
//! records `weighted.features()`'s ANSWER, keyed by ply number and player
//! index.
//!
//! Unlike those two siblings (which sample a handful of representative keys
//! per call), this checks **every [`WeightKey`], on every sampled state, in
//! both directions**:
//!
//! * every `WeightKey` `features()` is documented to write must match the
//!   Python dump's value for that name (a missing dump key would mean
//!   Python never wrote it either, read as `0.0` -- matches Python's own
//!   `dict.get(name, 0.0)` semantics elsewhere in this codebase);
//! * every `WeightKey` `features()` does NOT write must read back exactly
//!   `0.0` here too, so a coordinate this port accidentally starts (or stops)
//!   setting cannot go unnoticed;
//! * every key the Python dump DOES carry must resolve through
//!   [`WeightKey::by_name`] -- the exact shape of bug `docs/OPEN_ITEMS.md`'s
//!   `wonder_stages_per_action` was (a feature name that is not a real
//!   `DEFAULT_WEIGHTS` key, silently priced at 0.0 forever).
//!
//! This is deliberately NOT a sampled-coordinate test the way `weighted_
//! rivals.rs`'s `FEATURE_KEYS` is: `features()` is the coordinate source for
//! the entire evaluator, so a sampled subset of states is fine (`tools/
//! dump_weighted_features.py`'s stride) but a sampled subset of COORDINATES
//! is not.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tta::bots::weighted::features::features;
use tta::bots::weighted::weights::WeightKey;
use tta::fixtures::{self, Json, Record};
use tta::state::GameState;

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn expected_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/weighted_features_fixtures")
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

    let obj: &[(String, Json)] = match expected {
        Json::Obj(fields) => fields.as_slice(),
        _ => &[],
    };

    let got = features(state, idx, None, None, false);

    // Every real WeightKey, both directions: Python absent -> 0.0, and this
    // is checked for EVERY key, not just the ones `features()` is expected
    // to write -- a key this port starts (or stops) setting shows up here
    // either way.
    for &key in WeightKey::ALL {
        report.checked += 1;
        let want = obj.iter().find(|(k, _)| k == key.name()).and_then(|(_, v)| v.as_f64()).unwrap_or(0.0);
        let have = got.get(key);
        if !nearly(have, want, 1e-6) {
            note(format!("{}: rust={have} python={want}", key.name()));
        }
    }

    // Every key the Python dump carries must be a real weight name -- catches
    // a coordinate that is not a `DEFAULT_WEIGHTS` key at all (the
    // `wonder_stages_per_action`-shaped bug this module's own doc comment
    // guards against), which the loop above cannot see because it only walks
    // KNOWN `WeightKey`s.
    for (k, _) in obj {
        report.checked += 1;
        if WeightKey::by_name(k).is_none() {
            note(format!("python dump key {k:?} is not a real WeightKey"));
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
fn features_match_python_on_every_sampled_state_and_coordinate() {
    let dir = fixtures_dir();
    let edir = expected_dir();
    assert!(
        edir.is_dir(),
        "{} does not exist -- generate it with tools/dump_weighted_features.py",
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
    assert!(files_checked >= 3, "expected weighted_features_fixtures for at least 2p/3p/4p, found {files_checked}");
    eprintln!(
        "weighted features differential: {files_checked} files, {} checks, {} mismatches",
        report.checked,
        report.mismatches.len()
    );
    assert!(
        report.mismatches.is_empty(),
        "{} weighted features mismatch(es):\n{}",
        report.mismatches.len(),
        report.mismatches.join("\n")
    );
}
