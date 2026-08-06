//! Differential test for `bots::plan::determinize` against `tools/
//! dump_plan.py`'s output (`rust/tests/plan_fixtures/*.jsonl`, one line per
//! sampled ply: `{"ply": N, "seeds": {"<seed>": {"civil_deck": [...],
//! "military_deck": [...], "current_events": [...]}, ...}}` -- see that
//! script's own doc comment for exactly which seeds and why this is the ONE
//! piece of `plan.py`'s search this dump can check cross-engine at all).
//!
//! Same split as `weighted_eval.rs`: the ground-truth STATE for each sampled
//! ply comes from the ordinary differential fixtures (`rust/tests/
//! fixtures/*.jsonl`); the dump file only records `determinize`'s answer,
//! keyed by ply number and seed.
//!
//! `determinize`'s own [`PlanConfig`]/beam/`pick` machinery is NOT checked
//! here -- see `dump_plan.py`'s doc comment for why a chosen-move comparison
//! against Python would be comparing two different trial-apply rng streams
//! by construction, not a port bug. `rust/src/bots/plan.rs`'s own
//! `#[cfg(test)]` module carries that verification instead, the same split
//! `quiescent.rs` already uses (no Python differential test of its own
//! `pick` either).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tta::bots::plan::determinize;
use tta::cards::CardId;
use tta::fixtures::{self, Json, Record};
use tta::rng::PyRandom;
use tta::state::GameState;

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn expected_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/plan_fixtures")
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

/// Must match `tools/dump_plan.py`'s `SEEDS` exactly.
const SEEDS: &[i64] = &[0, 1, 2, -1, 4294967296, 123456789];

#[derive(Default)]
struct Report {
    checked: usize,
    mismatches: Vec<String>,
}

fn names(cards: &[CardId]) -> Vec<&'static str> {
    cards.iter().map(|c| c.name()).collect()
}

fn expected_names(j: &Json) -> Option<Vec<String>> {
    let arr = j.as_arr()?;
    Some(arr.iter().map(|v| v.as_str().unwrap_or_default().to_string()).collect())
}

fn check_pile(path: &Path, ply: u32, seed: i64, pile: &str, have: &[CardId], expected: &Json, report: &mut Report) {
    report.checked += 1;
    let Some(want) = expected_names(expected) else {
        report.mismatches.push(format!("{}: ply {ply} seed {seed}: {pile}: no array in python dump", path.display()));
        return;
    };
    let have_names: Vec<&str> = names(have);
    if have_names != want.iter().map(String::as_str).collect::<Vec<_>>() {
        report.mismatches.push(format!(
            "{}: ply {ply} seed {seed}: {pile}: rust={have_names:?} python={want:?}",
            path.display()
        ));
    }
}

fn check_state(path: &Path, ply: u32, state: &GameState, seeds_json: &Json, report: &mut Report) {
    for &seed in SEEDS {
        let Some(entry) = seeds_json.get(&seed.to_string()) else {
            report.mismatches.push(format!("{}: ply {ply}: no python entry for seed {seed}", path.display()));
            continue;
        };
        let mut trial = state.clone();
        let mut rng = PyRandom::new(seed);
        determinize(&mut trial, &mut rng);

        let civil = entry.get("civil_deck").expect("civil_deck field");
        check_pile(path, ply, seed, "civil_deck", trial.civil_deck.as_slice(), civil, report);
        let military = entry.get("military_deck").expect("military_deck field");
        check_pile(path, ply, seed, "military_deck", trial.military_deck.as_slice(), military, report);
        let events = entry.get("current_events").expect("current_events field");
        check_pile(path, ply, seed, "current_events", trial.current_events.as_slice(), events, report);
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
        let seeds_json = rec.get("seeds").expect("seeds object");
        check_state(path, ply, state, seeds_json, report);
    }
}

#[test]
fn determinize_matches_python_on_every_sampled_state_and_seed() {
    let dir = fixtures_dir();
    let edir = expected_dir();
    assert!(
        edir.is_dir(),
        "{} does not exist -- generate it with tools/dump_plan.py",
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
    assert!(files_checked >= 3, "expected plan_fixtures for at least 2p/3p/4p, found {files_checked}");
    eprintln!(
        "plan determinize differential: {files_checked} files, {} checks, {} mismatches",
        report.checked,
        report.mismatches.len()
    );
    assert!(
        report.mismatches.is_empty(),
        "{} determinize mismatch(es):\n{}",
        report.mismatches.len(),
        report.mismatches.join("\n")
    );
}
