//! Differential test for `bots::weighted::row` against
//! `tools/dump_weighted_row.py`'s output (`rust/tests/weighted_row_fixtures/
//! *.jsonl`, one line per sampled ply -- see that script's own doc comment
//! for the exact dump shape).
//!
//! An earlier revision of this file took `card_potential` as an injected
//! closure backed by the dump's `{viewer_idx: {name: value}}` table, because
//! `cards::card_potential` (owned by `cards.rs`) had not landed its
//! valuation layer yet. It has now, and `row.rs` calls it directly (no more
//! injected dependency, see that module's own top doc comment) -- so this
//! test does too: `row::row_pressure`/`row::row_last_copy` read the real
//! `cards::card_potential` internally, exercising both `row.rs`'s own
//! arithmetic AND its integration with the real card pricing in one pass.
//! The dump's `card_potential` table is unused by this file as a result (it
//! predates the valuation layer landing and is left in the fixture rather
//! than regenerated, since `weighted_cards.rs`'s own differential coverage
//! is what pins `cards::card_potential` now).
//!
//! Same split as `rust/tests/counting.rs`: the ground-truth STATE for each
//! sampled ply comes from the ordinary differential fixtures
//! (`rust/tests/fixtures/*.jsonl`); the dump file only records ANSWERS,
//! keyed by ply and player index. `rivals::rival_context` is NOT re-derived
//! from the dump -- it is rebuilt directly from the loaded state via the
//! real (already landed and separately differentially tested,
//! `weighted_rivals.rs`) `rivals::rival_context`, exactly as a real caller
//! would, so this test is genuinely exercising the integration, not a
//! second copy of `rival_context`'s own arithmetic.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tta::bots::weighted::row;
use tta::bots::weighted::rivals;
use tta::bots::weighted::weights::{WeightKey, Weights};
use tta::fixtures::{self, Json, Record};
use tta::state::GameState;

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn expected_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/weighted_row_fixtures")
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

/// The exact `_WEIGHT_VARIANTS` table `tools/dump_weighted_row.py` dumped
/// under, rebuilt here as real `Weights` -- see that script's own doc
/// comment for why only these two keys ever need to move.
fn weights_for_variant(tag: &str) -> Weights {
    let mut w = Weights::default();
    match tag {
        "default" => {}
        "desire_half" => w.set(WeightKey::RivalDesire, 0.5),
        "desire_one" => w.set(WeightKey::RivalDesire, 1.0),
        "share_low" => w.set(WeightKey::RivalTakeShare, 0.1),
        "share_high_desire" => {
            w.set(WeightKey::RivalTakeShare, 0.9);
            w.set(WeightKey::RivalDesire, 0.7);
        }
        other => panic!("unknown weight variant {other:?} -- dump/test have drifted"),
    }
    w
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

    let ctx = rivals::rival_context(state, idx, None, None);

    let results_obj = match expected.get("results") {
        Some(Json::Obj(fields)) => fields.as_slice(),
        _ => {
            note("missing results object in dump".to_string());
            return;
        }
    };
    for (tag, rec) in results_obj {
        let w = weights_for_variant(tag);
        report.checked += 1;

        let (urgency, bargain) = row::row_pressure(state, idx, &w, Some(&ctx));
        let last_copy = row::row_last_copy(state, idx, &w, Some(&ctx));

        let want_urgency = rec.get("row_urgency").and_then(Json::as_f64).unwrap_or(f64::NAN);
        let want_bargain = rec.get("row_bargain_forgone").and_then(Json::as_f64).unwrap_or(f64::NAN);
        let want_last_copy = rec.get("row_last_copy").and_then(Json::as_f64).unwrap_or(f64::NAN);

        if !nearly(urgency, want_urgency, 1e-6) {
            note(format!("{tag}: row_urgency: rust={urgency} python={want_urgency}"));
        }
        if !nearly(bargain, want_bargain, 1e-6) {
            note(format!("{tag}: row_bargain_forgone: rust={bargain} python={want_bargain}"));
        }
        if !nearly(last_copy, want_last_copy, 1e-6) {
            note(format!("{tag}: row_last_copy: rust={last_copy} python={want_last_copy}"));
        }
    }
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

#[test]
fn row_matches_python_on_sampled_fixture_states() {
    let dir = fixtures_dir();
    let edir = expected_dir();
    assert!(
        edir.is_dir(),
        "{} does not exist -- generate it with tools/dump_weighted_row.py (see that \
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
    assert!(files_checked >= 3, "expected weighted_row_fixtures for at least 2p/3p/4p, found {files_checked}");
    eprintln!(
        "weighted row differential: {files_checked} files, {} checks, {} mismatches",
        report.checked,
        report.mismatches.len()
    );
    assert!(
        report.mismatches.is_empty(),
        "{} row mismatch(es):\n{}",
        report.mismatches.len(),
        report.mismatches.join("\n")
    );
}
