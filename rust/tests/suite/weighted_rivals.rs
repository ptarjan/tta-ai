//! Differential test for `bots::weighted::rivals` against `tools/
//! dump_weighted_rivals.py`'s output (`rust/tests/weighted_rivals_fixtures/`)
//! -- see that script's own doc comment for the exact dump shape and for why
//! its sample always includes every `pact_offer`/auction state, not just a
//! stride sample (those are the states that exercise `deferred_credit`'s two
//! interesting branches at all).
//!
//! Same split as `rust/tests/counting.rs`: the ground-truth STATE for each
//! sampled ply comes from the ordinary differential fixtures (`rust/tests/
//! fixtures/*.jsonl`); the dump file only records the Python answers, keyed
//! by ply number and then by player index.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tta::bots::weighted::rivals::{self, GainFeature, Gains};
use tta::bots::weighted::weights::{WeightKey, Weights};
use tta::fixtures::{self, Json, Record};
use tta::state::{GameState, MAX_PLAYERS};

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn expected_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/weighted_rivals_fixtures")
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

/// The same `feature_marginal` keys `tools/dump_weighted_rivals.py` sampled,
/// as real `WeightKey`s.
const FEATURE_KEYS: &[WeightKey] = &[
    WeightKey::Colonies,
    WeightKey::Workers,
    WeightKey::StrengthRel,
    WeightKey::TechLevels,
    WeightKey::HandValue,
    WeightKey::CultureRate,
    WeightKey::ScienceRate,
    WeightKey::FoodRate,
    WeightKey::ResourceRate,
    WeightKey::Strength,
    WeightKey::CivilActions,
    WeightKey::HappyMargin,
];

fn nearly(a: f64, b: f64, eps: f64) -> bool {
    (a - b).abs() < eps
}

#[derive(Default)]
struct Report {
    checked: usize,
    mismatches: Vec<String>,
}

/// `Gains` against a `{feature_name: amount}` Python dict, both directions:
/// every nonzero Rust entry must match the dump at that key, and every key
/// the dump has must be a real `GainFeature` at the matching value. A
/// missing dump key reads as `0.0` (Python's `_add_block` never writes an
/// explicit-zero entry for real card data -- see `rivals.rs`'s own doc
/// comment on `Gains`).
fn gains_ok(got: &Gains, expected: Option<&Json>, what: &str, mismatches: &mut Vec<String>) {
    let obj: &[(String, Json)] = match expected {
        Some(Json::Obj(fields)) => fields.as_slice(),
        _ => &[],
    };
    for &f in GainFeature::ALL {
        let rust_v = got.get(f);
        let python_v = obj.iter().find(|(k, _)| k == f.name()).and_then(|(_, v)| v.as_f64()).unwrap_or(0.0);
        if !nearly(rust_v, python_v, 1e-9) {
            mismatches.push(format!("{what}[{}]: rust={rust_v} python={python_v}", f.name()));
        }
    }
    for (k, v) in obj {
        if GainFeature::ALL.iter().any(|f| f.name() == k) {
            continue;
        }
        mismatches.push(format!("{what}[{k}]: in python dump, not a known GainFeature (value {v:?})"));
    }
}

fn check_player(path: &Path, ply: u32, state: &GameState, idx: u8, expected: &Json, w: &Weights, report: &mut Report) {
    macro_rules! note {
        ($($arg:tt)*) => {
            report.mismatches.push(format!("{}: ply {ply} player {idx}: {}", path.display(), format!($($arg)*)))
        };
    }

    // ---------------------------------------------------------- rival_board
    report.checked += 1;
    let rb = rivals::rival_board(state, idx);
    let rb_expected = expected.get("rival_board");
    let rb_fields: &[(&str, f64)] = &[
        ("rival_science_stock", rb.rival_science_stock),
        ("rival_food_stock", rb.rival_food_stock),
        ("rival_resource_stock", rb.rival_resource_stock),
        ("rival_free_workers", rb.rival_free_workers),
        ("rival_yellow_bank", rb.rival_yellow_bank),
        ("rival_colonies", rb.rival_colonies),
        ("rival_mil_actions", rb.rival_mil_actions),
        ("rival_building_wonder", rb.rival_building_wonder),
    ];
    for &(name, got) in rb_fields {
        let want = rb_expected.and_then(|o| o.get(name)).and_then(Json::as_f64).unwrap_or(f64::NAN);
        if !nearly(got, want, 1e-9) {
            note!("rival_board.{name}: rust={got} python={want}");
        }
    }

    // ------------------------------------------------------- deferred_credit
    report.checked += 1;
    let dc = rivals::deferred_credit(state, idx);
    let dc_expected = expected.get("deferred_credit");
    let dc_fields: &[(&str, f64)] = &[
        ("pact_offers", dc.pact_offers),
        ("auction_committed", dc.auction_committed),
        ("auction_bid", dc.auction_bid),
        ("blocks_attack", dc.blocks_attack),
    ];
    for &(name, got) in dc_fields {
        let want = dc_expected.and_then(|o| o.get(name)).and_then(Json::as_f64).unwrap_or(f64::NAN);
        if !nearly(got, want, 1e-9) {
            note!("deferred_credit.{name}: rust={got} python={want}");
        }
    }
    gains_ok(&dc.gains, dc_expected.and_then(|o| o.get("gains")), "deferred_credit.gains", &mut report.mismatches);

    let partner_expected: &[(String, Json)] = match dc_expected.and_then(|o| o.get("partner_gains")) {
        Some(Json::Obj(fields)) => fields.as_slice(),
        _ => &[],
    };
    for pi in 0..MAX_PLAYERS as u8 {
        let key = pi.to_string();
        let entry = partner_expected.iter().find(|(k, _)| k == &key).map(|(_, v)| v);
        gains_ok(
            &dc.partner_gains[pi as usize],
            entry,
            &format!("deferred_credit.partner_gains[{pi}]"),
            &mut report.mismatches,
        );
    }
    for (k, _) in partner_expected {
        let idx_ok = k.parse::<u8>().map(|n| (n as usize) < MAX_PLAYERS).unwrap_or(false);
        if !idx_ok {
            note!("deferred_credit.partner_gains: python key {k:?} is not a valid player index");
        }
    }

    // -------------------------------------------------------- rival_strength
    report.checked += 1;
    let rs = rivals::rival_strength(state, idx);
    let rs_expected = expected.get("rival_strength").and_then(Json::as_f64).unwrap_or(f64::NAN);
    if !nearly(rs as f64, rs_expected, 1e-9) {
        note!("rival_strength: rust={rs} python={rs_expected}");
    }

    // --------------------------------------------------------- rival_context
    report.checked += 1;
    let ctx = rivals::rival_context(state, idx, None, None);
    let ctx_expected = expected.get("rival_context");
    let ctx_fields: &[(&str, i32)] =
        &[("rival_culture_rate", ctx.rival_culture_rate), ("rival_science_rate", ctx.rival_science_rate), ("rival_strength", ctx.rival_strength)];
    for &(name, got) in ctx_fields {
        let want = ctx_expected.and_then(|o| o.get(name)).and_then(Json::as_f64).unwrap_or(f64::NAN);
        if !nearly(got as f64, want, 1e-9) {
            note!("rival_context.{name}: rust={got} python={want}");
        }
    }
    assert_eq!(rs, ctx.rival_strength, "ply {ply} player {idx}: rival_strength must agree with RivalContext::rival_strength");

    let rates_expected: &[(String, Json)] = match ctx_expected.and_then(|o| o.get("rival_rates")) {
        Some(Json::Obj(fields)) => fields.as_slice(),
        _ => &[],
    };
    for idx2 in 0..state.num_players {
        let got = ctx.rival_rates[idx2 as usize];
        let key = idx2.to_string();
        let want = rates_expected.iter().find(|(k, _)| k == &key).and_then(|(_, v)| v.as_arr());
        let want = match want {
            Some([a, b, c]) => (
                a.as_f64().unwrap_or(f64::NAN),
                b.as_f64().unwrap_or(f64::NAN),
                c.as_f64().unwrap_or(f64::NAN),
            ),
            _ => (0.0, 0.0, 0.0),
        };
        if !nearly(got.0 as f64, want.0, 1e-9) || !nearly(got.1 as f64, want.1, 1e-9) || !nearly(got.2 as f64, want.2, 1e-9) {
            note!("rival_context.rival_rates[{idx2}]: rust={got:?} python={want:?}");
        }
    }

    // -------------------------------------------------------- strength_marginal
    report.checked += 1;
    let sm = rivals::strength_marginal(state, idx, w, None);
    let sm_expected = expected.get("strength_marginal").and_then(Json::as_f64).unwrap_or(f64::NAN);
    if !nearly(sm, sm_expected, 1e-9) {
        note!("strength_marginal: rust={sm} python={sm_expected}");
    }

    // -------------------------------------------------------- feature_marginal
    let fm_expected = expected.get("feature_marginal");
    for &key in FEATURE_KEYS {
        report.checked += 1;
        let got = rivals::feature_marginal(key, state, idx, w, None, None);
        let want = fm_expected.and_then(|o| o.get(key.name())).and_then(Json::as_f64).unwrap_or(f64::NAN);
        if !nearly(got, want, 1e-9) {
            note!("feature_marginal[{}]: rust={got} python={want}", key.name());
        }
    }
}

fn check_file(path: &Path, expected_path: &Path, w: &Weights, report: &mut Report) {
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

        // -------------------------------------------------------- root_row
        report.checked += 1;
        let root_row = rivals::root_row_budget(state);
        let expected_row: &[Json] = rec.get("root_row").and_then(Json::as_arr).unwrap_or(&[]);
        let got_names: Vec<&str> = root_row.as_slice().iter().map(|&id| id.name()).collect();
        let want_names: Vec<&str> = expected_row.iter().filter_map(Json::as_str).collect();
        if got_names != want_names {
            report.mismatches.push(format!("{}: ply {ply}: root_row: rust={got_names:?} python={want_names:?}", path.display()));
        }

        let players = rec.get("players").expect("players object");
        for idx in 0..state.num_players {
            let Some(p_expected) = players.get(&idx.to_string()) else { continue };
            check_player(path, ply, state, idx, p_expected, w, report);
        }
    }
}

#[test]
fn rivals_match_python_on_sampled_fixture_states() {
    let dir = fixtures_dir();
    let edir = expected_dir();
    assert!(
        edir.is_dir(),
        "{} does not exist -- generate it with tools/dump_weighted_rivals.py (see that \
         script's doc comment, or this file's own doc comment)",
        edir.display()
    );
    let files = fixtures::fixture_files(&dir).unwrap_or_else(|e| panic!("{e}"));
    assert!(!files.is_empty(), "no fixtures in {}", dir.display());

    let w = Weights::default();
    let mut report = Report::default();
    let mut files_checked = 0usize;
    for path in &files {
        let expected_path = edir.join(path.file_name().unwrap());
        if !expected_path.exists() {
            continue;
        }
        files_checked += 1;
        check_file(path, &expected_path, &w, &mut report);
    }
    assert!(files_checked >= 3, "expected weighted_rivals_fixtures for at least 2p/3p/4p, found {files_checked}");
    eprintln!("weighted rivals differential: {files_checked} files, {} checks, {} mismatches", report.checked, report.mismatches.len());
    assert!(report.mismatches.is_empty(), "{} weighted rivals mismatch(es):\n{}", report.mismatches.len(), report.mismatches.join("\n"));
}
