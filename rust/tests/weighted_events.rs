//! Differential test for `bots::weighted::events` against
//! `tools/dump_weighted_events.py`'s output (`rust/tests/
//! weighted_events_fixtures/*.jsonl`, one line per sampled ply:
//! `{"ply": N, "players": {"0": {"my_seeds": [...], "my_seeded_pending":
//! f64, "scoring_weights": {name: f64}, "event_scoring_margin": f64,
//! "my_event_threat": f64, "attack_targets": [u8], "attack_target_lead":
//! f64, "attack_target_weakness": f64, "pact_partner_lead": f64}, ...}}` --
//! see that script's own doc comment for the exact dump shape).
//!
//! Same split as `rust/tests/counting.rs`: the ground-truth STATE for each
//! sampled ply comes from the ordinary differential fixtures (`rust/tests/
//! fixtures/*.jsonl`); the dump file only records `weighted.py`'s ANSWERS,
//! keyed by ply number. `scoring_weights`/`event_scoring_margin` are dumped
//! with `ctx=None` on the Python side and `pool: None` on the Rust side, so
//! both independently recompute `counting::event_pool` -- already pinned
//! byte-for-byte by `rust/tests/counting.rs` -- rather than needing a second
//! plumbed-through fixture for it. `my_event_threat` is priced through
//! `DEFAULT_WEIGHTS`/`Weights::default()` on both sides.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tta::bots::weighted::events;
use tta::bots::weighted::weights::Weights;
use tta::cards::CardId;
use tta::fixtures::{self, Json, Record};
use tta::state::GameState;

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn expected_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/weighted_events_fixtures")
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

/// `{name: float}` dict comparison, both directions -- an entry present on
/// only one side is exactly the "one registry knows, the other doesn't, and
/// nothing fails" shape this project watches for.
fn dict_ok(got: &[(CardId, f64)], expected: Option<&Json>, eps: f64) -> Result<(), String> {
    let obj = match expected {
        Some(Json::Obj(fields)) => fields.as_slice(),
        _ => &[],
    };
    if got.len() != obj.len() {
        return Err(format!(
            "{} entries in rust, {} in python (rust={got:?})",
            got.len(),
            obj.len()
        ));
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

/// `my_seeds`' list of names, both directions -- order-independent (Python
/// dumps `sorted(...)`, and this sorts the Rust side to match).
fn names_ok(got: &[CardId], expected: Option<&Json>) -> Result<(), String> {
    let arr: &[Json] = expected.and_then(Json::as_arr).unwrap_or(&[]);
    let mut want: Vec<&str> = arr.iter().filter_map(Json::as_str).collect();
    want.sort_unstable();
    let mut have: Vec<&str> = got.iter().map(|c| c.name()).collect();
    have.sort_unstable();
    if have != want {
        return Err(format!("rust={have:?} python={want:?}"));
    }
    Ok(())
}

/// `attack_targets`' player-index list, both directions, order-independent.
fn targets_ok(got: events::AttackTargets, expected: Option<&Json>) -> Result<(), String> {
    let arr: &[Json] = expected.and_then(Json::as_arr).unwrap_or(&[]);
    let mut want: Vec<i64> = arr.iter().filter_map(|v| v.as_f64()).map(|f| f as i64).collect();
    want.sort_unstable();
    let mut have: Vec<i64> = got.iter().map(i64::from).collect();
    have.sort_unstable();
    if have != want {
        return Err(format!("rust={have:?} python={want:?}"));
    }
    Ok(())
}

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
    let mine = events::my_seeds(state, idx);
    if let Err(msg) = names_ok(&mine, expected.get("my_seeds")) {
        note(format!("my_seeds: {msg}"));
    }

    report.checked += 1;
    let pending = events::my_seeded_pending(state, idx);
    let want = expected.get("my_seeded_pending").and_then(Json::as_f64).unwrap_or(f64::NAN);
    if !nearly(pending, want, 1e-9) {
        note(format!("my_seeded_pending: rust={pending} python={want}"));
    }

    report.checked += 1;
    let sw = events::scoring_weights(state, idx, None);
    if let Err(msg) = dict_ok(&sw, expected.get("scoring_weights"), 1e-9) {
        note(format!("scoring_weights: {msg}"));
    }

    report.checked += 1;
    let margin = events::event_scoring_margin(state, idx, None);
    let want = expected.get("event_scoring_margin").and_then(Json::as_f64).unwrap_or(f64::NAN);
    if !nearly(margin, want, 1e-6) {
        note(format!("event_scoring_margin: rust={margin} python={want}"));
    }

    report.checked += 1;
    let threat = events::my_event_threat(state, idx, &Weights::default());
    let want = expected.get("my_event_threat").and_then(Json::as_f64).unwrap_or(f64::NAN);
    if !nearly(threat, want, 1e-9) {
        note(format!("my_event_threat: rust={threat} python={want}"));
    }

    report.checked += 1;
    let targets = events::attack_targets(state, idx);
    if let Err(msg) = targets_ok(targets, expected.get("attack_targets")) {
        note(format!("attack_targets: {msg}"));
    }

    report.checked += 1;
    let (lead, weakness) = events::attack_target_terms(state, idx);
    let want_lead = expected.get("attack_target_lead").and_then(Json::as_f64).unwrap_or(f64::NAN);
    let want_weakness = expected.get("attack_target_weakness").and_then(Json::as_f64).unwrap_or(f64::NAN);
    if !nearly(lead, want_lead, 1e-9) {
        note(format!("attack_target_lead: rust={lead} python={want_lead}"));
    }
    if !nearly(weakness, want_weakness, 1e-9) {
        note(format!("attack_target_weakness: rust={weakness} python={want_weakness}"));
    }

    report.checked += 1;
    let pact_lead = events::pact_partner_lead(state, idx);
    let want = expected.get("pact_partner_lead").and_then(Json::as_f64).unwrap_or(f64::NAN);
    if !nearly(pact_lead, want, 1e-9) {
        note(format!("pact_partner_lead: rust={pact_lead} python={want}"));
    }
}

#[test]
fn weighted_events_matches_python_on_sampled_fixture_states() {
    let dir = fixtures_dir();
    let edir = expected_dir();
    assert!(
        edir.is_dir(),
        "{} does not exist -- generate it with tools/dump_weighted_events.py",
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
    assert!(files_checked >= 3, "expected weighted_events_fixtures for at least 2p/3p/4p, found {files_checked}");
    eprintln!(
        "weighted events differential: {files_checked} files, {} checks, {} mismatches",
        report.checked,
        report.mismatches.len()
    );
    assert!(report.mismatches.is_empty(), "{} events mismatch(es):\n{}", report.mismatches.len(), report.mismatches.join("\n"));
}
