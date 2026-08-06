//! Differential test for `bots::weighted::{weights, horizon}` against
//! `tools/dump_weighted_horizon.py`'s output
//! (`rust/tests/weighted_horizon_fixtures/`) -- see that script's own doc
//! comment for the exact dump shape and for the two dead-code fixes that
//! landed in `engine/bots/weighted.py` alongside this port (a genuinely
//! unread `_ROW` constant, and `horizon_scale`'s unread `w` parameter).
//!
//! Same split as `rust/tests/counting.rs`: the ground-truth STATE for each
//! sampled ply comes from the ordinary differential fixtures
//! (`rust/tests/fixtures/*.jsonl`); the dump file only records the answers.
//! `default_weights.json` is not per-state at all -- `DEFAULT_WEIGHTS`/
//! `RETIRED_KEYS` are constants, dumped once and checked in both directions
//! against [`weights::WeightKey::ALL`] so a key present in one registry and
//! absent from the other (this project's signature bug shape) fails loudly.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tta::bots::weighted::horizon;
use tta::bots::weighted::weights::{self, WeightKey, Weights};
use tta::fixtures::{self, Json, Record};
use tta::state::GameState;

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn expected_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/weighted_horizon_fixtures")
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

// -------------------------------------------------------- default_weights

/// `WeightKey::ALL`'s defaults against Python's `DEFAULT_WEIGHTS`, both
/// directions -- every Rust key must be in the Python dump at the same
/// value, and every Python dump key must be a Rust `WeightKey`.
/// `RETIRED_KEYS` gets the same both-directions treatment.
#[test]
fn default_weights_matches_python_both_directions() {
    let path = expected_dir().join("default_weights.json");
    assert!(
        path.is_file(),
        "{} does not exist -- generate it with tools/dump_weighted_horizon.py",
        path.display()
    );
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    let root = fixtures::parse_json(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));

    let weights_obj = match root.get("weights") {
        Some(Json::Obj(fields)) => fields.as_slice(),
        _ => panic!("default_weights.json has no \"weights\" object"),
    };

    let mut mismatches = Vec::new();

    // Rust -> Python: every WeightKey's default_weight() must appear in the
    // Python dump under the same name at the same value.
    for &k in WeightKey::ALL {
        match weights_obj.iter().find(|(name, _)| name == k.name()) {
            None => mismatches.push(format!("{}: in Rust WeightKey::ALL, not in Python DEFAULT_WEIGHTS", k.name())),
            Some((_, v)) => {
                let pv = v.as_f64().unwrap_or_else(|| panic!("{}: python value is not a number", k.name()));
                let rv = k.default_weight();
                if (pv - rv).abs() >= 1e-12 {
                    mismatches.push(format!("{}: rust={rv} python={pv}", k.name()));
                }
            }
        }
    }
    // Python -> Rust: every DEFAULT_WEIGHTS key must be a real WeightKey.
    for (name, _) in weights_obj {
        if WeightKey::by_name(name).is_none() {
            mismatches.push(format!("{name}: in Python DEFAULT_WEIGHTS, not a Rust WeightKey"));
        }
    }
    assert_eq!(weights_obj.len(), WeightKey::ALL.len(), "DEFAULT_WEIGHTS and WeightKey::ALL differ in size");

    // RETIRED_KEYS, both directions.
    let retired_py: &[Json] = root.get("retired_keys").and_then(Json::as_arr).unwrap_or(&[]);
    let retired_py_names: Vec<&str> = retired_py.iter().filter_map(Json::as_str).collect();
    for &name in weights::RETIRED_KEYS {
        if !retired_py_names.contains(&name) {
            mismatches.push(format!("{name}: in Rust RETIRED_KEYS, not in Python RETIRED_KEYS"));
        }
    }
    for &name in &retired_py_names {
        if !weights::RETIRED_KEYS.contains(&name) {
            mismatches.push(format!("{name}: in Python RETIRED_KEYS, not in Rust RETIRED_KEYS"));
        }
    }

    assert!(mismatches.is_empty(), "{} weight-registry mismatch(es):\n{}", mismatches.len(), mismatches.join("\n"));
}

// ------------------------------------------------------------------ horizon

fn nearly(a: f64, b: f64, eps: f64) -> bool {
    (a - b).abs() < eps
}

/// The `rate_multiplier` credits `dump_weighted_horizon.py` sampled, parsed
/// back out of the dump's `repr(float)` string keys.
fn credit_from_key(key: &str) -> f64 {
    key.parse().unwrap_or_else(|e| panic!("credit key {key:?}: {e}"))
}

fn check_ply(path: &Path, ply: u32, state: &GameState, expected: &Json, mismatches: &mut Vec<String>) {
    let mut note = |what: String| mismatches.push(format!("{}: ply {ply}: {what}", path.display()));

    let n = horizon::live_count(state);
    let expected_n = expected.get("n").and_then(Json::as_f64).unwrap_or(-1.0) as usize;
    if n != expected_n {
        note(format!("n: rust={n} python={expected_n}"));
    }

    let cards_unseen = horizon::cards_unseen(state, n);
    let expected_cards_unseen = expected.get("cards_unseen").and_then(Json::as_f64).unwrap_or(-1.0) as u32;
    if cards_unseen != expected_cards_unseen {
        note(format!("cards_unseen: rust={cards_unseen} python={expected_cards_unseen}"));
    }

    let checks: [(&str, f64); 4] = [
        ("take_rate", horizon::take_rate(state, n)),
        ("rounds_left", horizon::rounds_left(state, n)),
        ("lateness", horizon::lateness(state)),
        ("horizon_scale", horizon::horizon_scale(state, n)),
    ];
    for (name, got) in checks {
        let want = expected.get(name).and_then(Json::as_f64).unwrap_or(f64::NAN);
        if !nearly(got, want, 1e-9) {
            note(format!("{name}: rust={got} python={want}"));
        }
    }

    let rm_obj = match expected.get("rate_multiplier") {
        Some(Json::Obj(fields)) => fields.as_slice(),
        _ => {
            note("rate_multiplier: missing or not an object in dump".to_string());
            return;
        }
    };
    for (key, want_json) in rm_obj {
        let credit = credit_from_key(key);
        let mut w = Weights::default();
        w.set(WeightKey::RateHorizon, credit);
        let got = horizon::rate_multiplier(state, &w, n);
        let want = want_json.as_f64().unwrap_or(f64::NAN);
        if !nearly(got, want, 1e-9) {
            note(format!("rate_multiplier[{credit}]: rust={got} python={want}"));
        }
    }
}

#[test]
fn horizon_matches_python_on_sampled_fixture_states() {
    let dir = fixtures_dir();
    let edir = expected_dir();
    assert!(
        edir.is_dir(),
        "{} does not exist -- generate it with tools/dump_weighted_horizon.py (see \
         that script's doc comment, or this file's own doc comment)",
        edir.display()
    );
    let files = fixtures::fixture_files(&dir).unwrap_or_else(|e| panic!("{e}"));
    assert!(!files.is_empty(), "no fixtures in {}", dir.display());

    let mut mismatches = Vec::new();
    let mut checked = 0usize;
    let mut files_checked = 0usize;
    for path in &files {
        let expected_path = edir.join(path.file_name().unwrap());
        if !expected_path.exists() {
            continue;
        }
        files_checked += 1;
        let states = load_states(path);
        let text = std::fs::read_to_string(&expected_path)
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
            checked += 1;
            check_ply(path, ply, state, &rec, &mut mismatches);
        }
    }
    assert!(files_checked >= 3, "expected weighted_horizon_fixtures for at least 2p/3p/4p, found {files_checked}");
    eprintln!("weighted horizon differential: {files_checked} files, {checked} states, {} mismatches", mismatches.len());
    assert!(mismatches.is_empty(), "{} horizon mismatch(es):\n{}", mismatches.len(), mismatches.join("\n"));
}
