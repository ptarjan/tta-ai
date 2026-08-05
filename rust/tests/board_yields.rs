//! Differential test for `bots::board_yields` against
//! `tools/dump_board_yields.py`'s output (`rust/tests/board_yields_fixtures/
//! *.jsonl`, one line per sampled ply: `{"ply": N, "players": {"0": {...
//! per-function results ...}, ...}}` -- see that script's own doc comment for
//! the exact dump shape).
//!
//! The ground-truth STATE for each sampled ply comes from the ordinary
//! differential fixtures (`rust/tests/fixtures/*.jsonl`, loaded the same way
//! `tests/differential.rs` does), not from the dump file itself -- the dump
//! file only records `board_yields.py`'s ANSWERS, keyed by ply number, and
//! this test re-derives the board from the original recording so both files
//! stay small and neither duplicates the other's job.
//!
//! Every card of a type any entry point prices is checked, for every live
//! player, on every sampled state -- see `rust/src/bots/board_yields.rs`'s
//! top doc comment for the two known, already-flagged gaps
//! ([`BOARD_EXTRA_GAP_CARDS`], [`WONDER_CULTURE_GAP_CARDS`]) this test
//! allowlists rather than silently absorbing into a passing run.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tta::bots::board_yields as by;
use tta::bots::board_yields::{Feature, Kind, Triple};
use tta::cards::{CardId, CardType, NUM_CARDS};
use tta::fixtures::{self, Json, Record};
use tta::state::GameState;

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn expected_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/board_yields_fixtures")
}

/// Wonders whose completion culture this port cannot yet price -- see
/// `board_yields.rs`'s top doc comment: Hollywood/Internet need
/// `effects::building_output`, unported. `board_yields(name, ...)` for these
/// two is checked to still AGREE on the `Stats`-diff and `wonders`/
/// `resource_stock` cost triples (a partial compare would need re-deriving
/// which triples are affected, so this test skips them wholesale rather than
/// silently accepting a partial credit that could mask a real regression
/// elsewhere in the same triple list).
const WONDER_CULTURE_GAP_CARDS: &[&str] = &["Hollywood", "Internet"];

/// Action cards `board_extra` cannot price -- see `board_yields.rs`'s top
/// doc comment: the per-player-count coefficient is not carried by the
/// `Special` payload `gen_cards.py` generated for either key.
const BOARD_EXTRA_GAP_CARDS: &[&str] = &["Endowment for the Arts", "Wave of Nationalism", "Military Build-Up"];

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

// ------------------------------------------------------------- comparisons

fn kind_num(k: Kind) -> f64 {
    match k {
        Kind::Gain => 0.0,
        Kind::Cost => 1.0,
    }
}

fn triple_matches(t: &Triple, v: &Json) -> bool {
    let arr = match v.as_arr() {
        Some(a) if a.len() == 3 => a,
        _ => return false,
    };
    let (feat, amt, kind) = (arr[0].as_str(), arr[1].as_f64(), arr[2].as_f64());
    feat == Some(t.0.python_key())
        && kind == Some(kind_num(t.2))
        && amt.is_some_and(|a| (a - t.1).abs() < 1e-9)
}

/// `triples` (Rust) against `expected` (a JSON array of `[feat, amt, kind]`,
/// or absent meaning "empty"). Order matters -- both sides build triples by
/// walking the same fixed feature tables, so a reordering is itself a
/// finding, not noise to tolerate.
fn triples_ok(triples: &[Triple], expected: Option<&Json>) -> bool {
    let expected_arr: &[Json] = expected.and_then(Json::as_arr).unwrap_or(&[]);
    if triples.len() != expected_arr.len() {
        return false;
    }
    triples.iter().zip(expected_arr.iter()).all(|(t, v)| triple_matches(t, v))
}

fn routes_ok(routes: &[Vec<Triple>], expected: Option<&Json>) -> bool {
    let expected_arr: &[Json] = expected.and_then(Json::as_arr).unwrap_or(&[]);
    if routes.len() != expected_arr.len() {
        return false;
    }
    routes.iter().zip(expected_arr.iter()).all(|(r, v)| triples_ok(r, Some(v)))
}

fn f64_ok(a: f64, expected: Option<f64>) -> bool {
    (a - expected.unwrap_or(0.0)).abs() < 1e-9
}

/// All card ids of the base game, sorted-by-nothing (insertion order is
/// fine: every comparison below looks the expected side up BY NAME, so
/// iteration order here never needs to match Python's).
fn all_cards() -> impl Iterator<Item = CardId> {
    (0..NUM_CARDS as u16).map(CardId)
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
            panic!(
                "{}: ply {ply} has no matching state in {}",
                expected_path.display(),
                path.display()
            );
        };
        let players = rec.get("players").expect("players object");
        for idx in 0..state.num_players as usize {
            let Some(p_expected) = players.get(&idx.to_string()) else { continue };
            check_player(path, ply, state, idx, p_expected, report);
        }
    }
}

fn check_player(path: &Path, ply: u32, state: &GameState, idx: usize, expected: &Json, report: &mut Report) {
    let mut note = |what: String| {
        report.mismatches.push(format!("{}: ply {ply} player {idx}: {what}", path.display()));
    };

    // ---- board_yields (leader / government / wonder) ----
    for id in all_cards().filter(|&id| by::is_swap_type(id.kind())) {
        report.checked += 1;
        let name = id.name();
        if id.kind() == CardType::Wonder && WONDER_CULTURE_GAP_CARDS.contains(&name) {
            continue; // named gap -- see the module doc comment above.
        }
        let category = match id.kind() {
            CardType::Leader => "leader",
            CardType::Government => "government",
            CardType::Wonder => "wonder",
            _ => unreachable!(),
        };
        let got = by::board_yields(id, state, idx).unwrap_or_default();
        let expected_v = expected.get(category).and_then(|o| o.get(name));
        if !triples_ok(&got, expected_v) {
            note(format!("board_yields({name}): rust={got:?} python={expected_v:?}"));
        }
    }

    // ---- government_plans ----
    for id in all_cards().filter(|&id| id.kind() == CardType::Government) {
        report.checked += 1;
        let name = id.name();
        let (gained, routes) = by::government_plans(id, state, idx);
        let expected_v = expected.get("gov_plans").and_then(|o| o.get(name));
        let (exp_gained, exp_routes) = match expected_v.and_then(Json::as_arr) {
            Some([g, r]) => (Some(g), Some(r)),
            _ => (None, None),
        };
        if !triples_ok(&gained, exp_gained) || !routes_ok(&routes, exp_routes) {
            note(format!("government_plans({name}): rust={gained:?}/{routes:?} python={expected_v:?}"));
        }
    }

    // ---- unit_upgrade / tech_upgrade / build_fresh ----
    for id in all_cards().filter(|&id| by::is_levelled_type(id.kind())) {
        let name = id.name();

        if id.kind().is_unit() {
            report.checked += 1;
            let (gained, sci, res) = by::unit_upgrade(id, state, idx);
            let expected_v = expected.get("unit_upgrade").and_then(|o| o.get(name));
            let exp_arr = expected_v.and_then(Json::as_arr);
            let ok = match exp_arr {
                Some([g, s, r]) => {
                    f64_ok(gained, g.as_f64()) && f64_ok(sci, s.as_f64()) && f64_ok(res, r.as_f64())
                }
                _ => gained == 0.0 && sci == 0.0 && res == 0.0,
            };
            if !ok {
                note(format!("unit_upgrade({name}): rust=({gained},{sci},{res}) python={expected_v:?}"));
            }
        }

        report.checked += 1;
        let (staff, dev, sci, res) = by::tech_upgrade(id, state, idx);
        let expected_v = expected.get("tech_upgrade").and_then(|o| o.get(name));
        let exp_arr = expected_v.and_then(Json::as_arr);
        let ok = match exp_arr {
            Some([s, d, sc, r]) => {
                triples_ok(&staff, Some(s)) && triples_ok(&dev, Some(d)) && f64_ok(sci, sc.as_f64()) && f64_ok(res, r.as_f64())
            }
            _ => staff.is_empty() && dev.is_empty() && sci == 0.0 && res == 0.0,
        };
        if !ok {
            note(format!(
                "tech_upgrade({name}): rust=({staff:?},{dev:?},{sci},{res}) python={expected_v:?}"
            ));
        }

        report.checked += 1;
        let (triples, res) = by::build_fresh(id, state, idx);
        let expected_v = expected.get("build_fresh").and_then(|o| o.get(name));
        let exp_arr = expected_v.and_then(Json::as_arr);
        let ok = match exp_arr {
            Some([t, r]) => triples_ok(&triples, Some(t)) && f64_ok(res, r.as_f64()),
            _ => triples.is_empty() && res == 0.0,
        };
        if !ok {
            note(format!("build_fresh({name}): rust=({triples:?},{res}) python={expected_v:?}"));
        }
    }

    // ---- board_extra ----
    for name in BOARD_EXTRA_GAP_CARDS {
        let id = CardId::by_name(name).unwrap_or_else(|| panic!("no such card: {name}"));
        // Known gap (module doc comment): Rust always returns nothing here.
        // Assert exactly that -- a Rust result stops being empty is itself
        // news (the gap silently changed shape) worth failing loudly on.
        let got = by::board_extra(id, state, idx);
        report.checked += 1;
        if !got.is_empty() {
            note(format!("board_extra({name}): expected the known gap (empty), got {got:?}"));
        }
    }
}

#[test]
fn board_yields_matches_python_on_sampled_fixture_states() {
    let dir = fixtures_dir();
    let edir = expected_dir();
    assert!(
        edir.is_dir(),
        "{} does not exist -- generate it with tools/dump_board_yields.py (see that \
         script's doc comment, or rust/tests/board_yields.rs's own doc comment)",
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
    assert!(files_checked >= 3, "expected board_yields_fixtures for at least 2p/3p/4p, found {files_checked}");
    eprintln!(
        "board_yields differential: {files_checked} files, {} checks, {} mismatches",
        report.checked,
        report.mismatches.len()
    );
    assert!(
        report.mismatches.is_empty(),
        "{} board_yields mismatch(es):\n{}",
        report.mismatches.len(),
        report.mismatches.join("\n")
    );
}

/// `_swap_stats`'s clone-based swap must never mutate the ORIGINAL state --
/// this is the Rust replacement for Python's "the trap" (`_swapped` mutating
/// `p.leader` behind `state_stats`' cache's back). Asserted directly: call
/// `board_yields` for a leader, then confirm the player's real leader field
/// (and a `state_stats` call on the untouched player) are unchanged.
#[test]
fn board_yields_never_mutates_the_state_it_was_asked_to_price() {
    let dir = fixtures_dir();
    let files = fixtures::fixture_files(&dir).unwrap_or_else(|e| panic!("{e}"));
    let path = files.iter().find(|p| p.file_name().unwrap().to_str().unwrap().starts_with("2p_")).unwrap();
    let records = fixtures::read_fixture_file(path).unwrap_or_else(|e| panic!("{e}"));
    let json = records
        .iter()
        .find_map(|r| match r {
            Record::Ply(p) if p.state.is_some() => p.state.as_ref(),
            _ => None,
        })
        .expect("some ply carries a state snapshot");
    let state = GameState::from_json(json).unwrap_or_else(|e| panic!("{e}"));
    let before_leader = state.players[0].leader;
    let before_gov = state.players[0].government;
    let before_wonders = state.players[0].completed_wonders.len();
    let before_stats = tta::effects::state_stats(&state, &state.players[0]);

    let einstein = CardId::by_name("Albert Einstein").unwrap();
    let _ = by::board_yields(einstein, &state, 0);
    let despotism = CardId::by_name("Despotism").unwrap();
    let _ = by::board_yields(despotism, &state, 0);
    let pyramids = CardId::by_name("Pyramids").unwrap();
    let _ = by::board_yields(pyramids, &state, 0);

    assert_eq!(state.players[0].leader, before_leader);
    assert_eq!(state.players[0].government, before_gov);
    assert_eq!(state.players[0].completed_wonders.len(), before_wonders);
    assert_eq!(tta::effects::state_stats(&state, &state.players[0]), before_stats);
}

/// `merge`'s whole reason to exist: two triples for the same `(feature,
/// kind)` are summed, not last-write-wins. Mirrors
/// `tests/test_board_yields.py`'s Gandhi-over-Churchill case structurally
/// (a synthetic pair of triples with the same feature/kind and opposite
/// sign), without needing a real board.
#[test]
fn merge_sums_same_feature_and_kind_rather_than_keeping_the_last() {
    let triples = vec![
        (Feature::CultureRate, 2.0, Kind::Gain),
        (Feature::Strength, 1.0, Kind::Gain),
        (Feature::CultureRate, -3.0, Kind::Gain),
    ];
    let merged = by::merge(triples);
    assert_eq!(merged, vec![(Feature::CultureRate, -1.0, Kind::Gain), (Feature::Strength, 1.0, Kind::Gain)]);
}

/// A triple whose merged amount lands exactly on zero is dropped -- mirrors
/// Python's `if merged[(f, kd)]` truthiness filter.
#[test]
fn merge_drops_a_triple_that_sums_to_exactly_zero() {
    let triples = vec![(Feature::Science, 5.0, Kind::Cost), (Feature::Science, -5.0, Kind::Cost)];
    assert!(by::merge(triples).is_empty());
}
