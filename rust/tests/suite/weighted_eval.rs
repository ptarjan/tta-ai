//! Differential test for `bots::weighted::eval::evaluate` against `tools/
//! dump_weighted_eval.py`'s output (`rust/tests/weighted_eval_fixtures/
//! *.jsonl`, one line per sampled ply: `{"ply": N, "players": {"0":
//! {"default": f64, "rate_horizon_off": f64, "all_optional_on": f64}, ...}}`
//! -- see that script's own doc comment for exactly what each of the three
//! named weight vectors turns on and why).
//!
//! Same split as `rust/tests/weighted_features.rs`: the ground-truth STATE
//! for each sampled ply comes from the ordinary differential fixtures
//! (`rust/tests/fixtures/*.jsonl`); the dump file only records `evaluate`'s
//! ANSWER, keyed by ply number, player index and weight-vector name.
//!
//! `evaluate` returns one scalar, not a keyed vector -- there is no
//! "coordinate present in one side and not the other" direction to check the
//! way `weighted_features.rs` checks one. What plays that role here is the
//! THREE vectors: `"default"` alone would leave every eval-only term other
//! than `hand_potential` untested on every real trained champion (they all
//! default to 0.0), so `"all_optional_on"` turns each one on at a distinct
//! probe value and `"rate_horizon_off"` forces the `hz == 1.0` short-circuit
//! deterministically rather than relying on a sampled state happening to
//! land there.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tta::bots::weighted::eval::evaluate;
use tta::bots::weighted::rivals;
use tta::bots::weighted::weights::{WeightKey, Weights};
use tta::fixtures::{self, Json, Record};
use tta::state::GameState;

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn expected_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/weighted_eval_fixtures")
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

// ------------------------------------------------------------- weight vectors
//
// Must match `tools/dump_weighted_eval.py`'s `_WEIGHT_VECTORS` exactly --
// same names, same probe values.

fn rate_horizon_off() -> Weights {
    let mut w = Weights::default();
    w.set(WeightKey::RateHorizon, 0.0);
    w
}

fn all_optional_on() -> Weights {
    let mut w = Weights::default();
    w.set(WeightKey::WonderPotential, 0.7);
    w.set(WeightKey::HandMilPotential, 0.6);
    w.set(WeightKey::TacticGain, 0.5);
    w.set(WeightKey::TacticShort, 0.4);
    w.set(WeightKey::RivalHandPotential, 0.3);
    w.set(WeightKey::RowUrgency, 0.2);
    w.set(WeightKey::RowBargainForgone, 0.15);
    w.set(WeightKey::RowLastCopy, 0.1);
    w.set(WeightKey::MyEventThreat, 0.05);
    w
}

/// `(name, weights)`, in the same order `dump_weighted_eval.py` names them.
fn weight_vectors() -> [(&'static str, Weights); 3] {
    [("default", Weights::default()), ("rate_horizon_off", rate_horizon_off()), ("all_optional_on", all_optional_on())]
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

    // `tools/dump_weighted_eval.py::_one_player` computes `ctx =
    // rival_context(state, idx)` once per player and hands it to `evaluate`
    // -- exactly what every real caller does (a fresh root decision). `ctx:
    // None` is a DIFFERENT, degraded path (`row_pressure`/`row_last_copy`
    // mask nothing without a real `root_row`), so this must build the same
    // real context Python does rather than pass `None`, or `"all_optional_on"`
    // (the vector that turns the row terms on) would disagree for a reason
    // that has nothing to do with a port bug.
    let ctx = rivals::rival_context(state, idx, None, None);

    for (name, w) in weight_vectors() {
        report.checked += 1;
        let want = expected.get(name).and_then(Json::as_f64);
        let Some(want) = want else {
            note(format!("python dump has no entry for weight vector {name:?}"));
            continue;
        };
        let have = evaluate(state, idx, &w, Some(&ctx), None);
        if !nearly(have, want, 1e-6) {
            note(format!("{name}: rust={have} python={want}"));
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
fn evaluate_matches_python_on_every_sampled_state_and_weight_vector() {
    let dir = fixtures_dir();
    let edir = expected_dir();
    assert!(
        edir.is_dir(),
        "{} does not exist -- generate it with tools/dump_weighted_eval.py",
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
    assert!(files_checked >= 3, "expected weighted_eval_fixtures for at least 2p/3p/4p, found {files_checked}");
    eprintln!(
        "weighted eval differential: {files_checked} files, {} checks, {} mismatches",
        report.checked,
        report.mismatches.len()
    );
    assert!(
        report.mismatches.is_empty(),
        "{} weighted eval mismatch(es):\n{}",
        report.mismatches.len(),
        report.mismatches.join("\n")
    );
}
