//! Differential test for `bots::greedy::{features, evaluate}` against
//! `tools/dump_greedy_features.py`'s output (`rust/tests/
//! greedy_features_fixtures/*.jsonl`, one line per sampled ply:
//! `{"ply": N, "players": {"0": {"features": {<every GreedyKey>: f64, ...},
//! "evaluate": f64}, ...}}`).
//!
//! Same split as `rust/tests/weighted_features.rs`: the ground-truth STATE
//! for each sampled ply comes from the ordinary differential fixtures
//! (`rust/tests/fixtures/*.jsonl`); the dump file only records
//! `engine.bots.features()`/`evaluate()`'s ANSWER, keyed by ply number and
//! player index.
//!
//! Checks **every `GreedyKey`, on every sampled state, in both directions**
//! (a missing dump key reads back `0.0`, matching Python's own
//! `dict.get(name, 0.0)` semantics elsewhere in this codebase; an extra dump
//! key that is not a real `GreedyKey` name is a failure), plus the scalar
//! `evaluate()` result -- `bots::greedy`'s own module doc comment explains why
//! `GreedyBot`'s vocabulary is a SEPARATE enum from `weighted::weights::
//! WeightKey` rather than shared with it, which is why this is its own test
//! file rather than a few more rows in `weighted_features.rs`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tta::bots::greedy::{evaluate, features, GreedyKey, GreedyWeights};
use tta::fixtures::{self, Json, Record};
use tta::state::GameState;

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn expected_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/greedy_features_fixtures")
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

    let feat_obj: &[(String, Json)] = match expected.get("features") {
        Some(Json::Obj(fields)) => fields.as_slice(),
        _ => {
            note("dump has no \"features\" object".to_string());
            &[]
        }
    };

    let got = features(state, idx);

    for &key in GreedyKey::ALL {
        report.checked += 1;
        let want = feat_obj.iter().find(|(k, _)| k == key.name()).and_then(|(_, v)| v.as_f64()).unwrap_or(0.0);
        let have = got.get(key);
        if !nearly(have, want, 1e-6) {
            note(format!("features.{}: rust={have} python={want}", key.name()));
        }
    }

    for (k, _) in feat_obj {
        report.checked += 1;
        if GreedyKey::by_name(k).is_none() {
            note(format!("python dump feature key {k:?} is not a real GreedyKey"));
        }
    }

    report.checked += 1;
    let want_eval = expected.get("evaluate").and_then(Json::as_f64).unwrap_or_else(|| {
        note("dump has no \"evaluate\" field".to_string());
        f64::NAN
    });
    let have_eval = evaluate(state, idx, &GreedyWeights::default());
    if !nearly(have_eval, want_eval, 1e-6) {
        note(format!("evaluate: rust={have_eval} python={want_eval}"));
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
fn greedy_features_and_evaluate_match_python_on_every_sampled_state() {
    let dir = fixtures_dir();
    let edir = expected_dir();
    assert!(
        edir.is_dir(),
        "{} does not exist -- generate it with tools/dump_greedy_features.py",
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
    assert!(files_checked >= 3, "expected greedy_features_fixtures for at least 2p/3p/4p, found {files_checked}");
    eprintln!(
        "greedy features/evaluate differential: {files_checked} files, {} checks, {} mismatches",
        report.checked,
        report.mismatches.len()
    );
    assert!(
        report.mismatches.is_empty(),
        "{} greedy features/evaluate mismatch(es):\n{}",
        report.mismatches.len(),
        report.mismatches.join("\n")
    );
}
