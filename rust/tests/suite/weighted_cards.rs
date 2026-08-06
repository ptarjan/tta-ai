//! Differential test for `bots::weighted::cards` -- both layers -- against
//! `tools/dump_weighted_cards.py`'s output.
//!
//! Two dumps, two shapes of test in this one file, matching the dump
//! script's own split (see its doc comment for the full rationale):
//!
//! * `card_yields_matches_python_for_every_card` and the registry tests below
//!   it check the YIELD-PLUMBING layer against `card_yields.json`, a single
//!   JSON object covering every one of the 236 base-game cards once
//!   (`card_yields`/`card_choice`/`swap_type`/`board_credit_key`/`is_unit`/
//!   `is_levelled_tech`/`is_action`/`is_government` are pure functions of
//!   card identity, no board, so full coverage is cheap here).
//! * `valuation_matches_python_on_sampled_fixture_states` checks the
//!   VALUATION layer (`action_value`/`tech_value`/`gov_value`/
//!   `card_potential`/`hand_potential`/`wonder_potential`/
//!   `hand_mil_potential`/`rival_hand_potential`/`tactic_terms`) against
//!   `<fixture-name>.jsonl` -- one JSON object per sampled ply, the same
//!   shape `rust/tests/board_yields.rs`/`rust/tests/weighted_row.rs` read,
//!   because this layer genuinely IS board-aware and needs real sampled
//!   states, not just every card once.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tta::bots::board_yields;
use tta::bots::weighted::cards::{self, CardYield, YieldKind};
use tta::bots::weighted::weights::{Weights, WeightKey};
use tta::cards::{CardId, CardType};
use tta::fixtures::{self, Json, Record};
use tta::state::GameState;

fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/weighted_cards_fixtures/card_yields.json")
}

fn kind_name(k: YieldKind) -> &'static str {
    match k {
        YieldKind::Gain => "gain",
        YieldKind::Cost => "cost",
        YieldKind::Rate => "rate",
        YieldKind::Unit => "unit",
        YieldKind::Territory => "territory",
        YieldKind::Bonus => "bonus",
    }
}

/// Python's printed `card["type"]` string for a [`CardType`] -- the
/// vocabulary `_swap_type`'s dump uses, matched against `swap_type`'s own
/// [`CardType`] return value.
fn type_name(k: CardType) -> &'static str {
    match k {
        CardType::Farm => "farm",
        CardType::Mine => "mine",
        CardType::Lab => "lab",
        CardType::Temple => "temple",
        CardType::Library => "library",
        CardType::Arena => "arena",
        CardType::Theater => "theater",
        CardType::Infantry => "infantry",
        CardType::Cavalry => "cavalry",
        CardType::Artillery => "artillery",
        CardType::Air => "air",
        CardType::Government => "government",
        CardType::SpecialTech => "special-tech",
        CardType::Wonder => "wonder",
        CardType::Leader => "leader",
        CardType::Action => "action",
        CardType::Tactic => "tactic",
        CardType::Aggression => "aggression",
        CardType::War => "war",
        CardType::Pact => "pact",
        CardType::Bonus => "bonus",
        CardType::Territory => "territory",
        CardType::Event => "event",
    }
}

fn triple_from_json(j: &Json) -> (String, f64, String) {
    let arr = j.as_arr().unwrap_or_else(|| panic!("triple is not an array: {j:?}"));
    assert_eq!(arr.len(), 3, "triple is not length 3: {j:?}");
    let key = arr[0].as_str().unwrap_or_else(|| panic!("triple[0] not a string: {j:?}")).to_string();
    let amt = arr[1].as_f64().unwrap_or_else(|| panic!("triple[1] not a number: {j:?}"));
    let kind = arr[2].as_str().unwrap_or_else(|| panic!("triple[2] not a string: {j:?}")).to_string();
    (key, amt, kind)
}

fn rust_triples_as_json(triples: &[CardYield]) -> Vec<(String, f64, String)> {
    triples.iter().map(|&(k, a, kd)| (k.name().to_string(), a, kind_name(kd).to_string())).collect()
}

/// Sorted by `(key, kind, amount)` -- Python's dict-iteration order over a
/// card's printed `effects` block and this port's field-declaration push
/// order legitimately differ (a card's yields are a SET of facts, not a
/// sequence: `sum_yields` does not care what order it folds them in), so the
/// comparison this test makes has to be order-independent. `f64::to_bits` is
/// a valid total order for every amount this file ever produces (all finite,
/// no signed zero ambiguity that would matter for a card yield).
fn sorted_triples(mut v: Vec<(String, f64, String)>) -> Vec<(String, f64, String)> {
    v.sort_by(|(ka, aa, ta), (kb, ab, tb)| (ka, ta, aa.to_bits()).cmp(&(kb, tb, ab.to_bits())));
    v
}


#[test]
fn card_yields_matches_python_for_every_card() {
    let path = fixture_path();
    assert!(
        path.is_file(),
        "{} does not exist -- generate it with tools/dump_weighted_cards.py",
        path.display()
    );
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    let root = fixtures::parse_json(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));

    let card_yields_obj = match root.get("card_yields") {
        Some(Json::Obj(fields)) => fields.as_slice(),
        _ => panic!("no \"card_yields\" object in dump"),
    };
    let card_choice_obj = match root.get("card_choice") {
        Some(Json::Obj(fields)) => fields.as_slice(),
        _ => panic!("no \"card_choice\" object in dump"),
    };
    let sum_yields_obj = match root.get("sum_yields") {
        Some(Json::Obj(fields)) => fields.as_slice(),
        _ => panic!("no \"sum_yields\" object in dump"),
    };
    let board_credit_key_obj = match root.get("board_credit_key") {
        Some(Json::Obj(fields)) => fields.as_slice(),
        _ => panic!("no \"board_credit_key\" object in dump"),
    };
    let swap_type_obj = match root.get("swap_type") {
        Some(Json::Obj(fields)) => fields.as_slice(),
        _ => panic!("no \"swap_type\" object in dump"),
    };
    let is_unit_obj = match root.get("is_unit") {
        Some(Json::Obj(fields)) => fields.as_slice(),
        _ => panic!("no \"is_unit\" object in dump"),
    };
    let is_levelled_obj = match root.get("is_levelled_tech") {
        Some(Json::Obj(fields)) => fields.as_slice(),
        _ => panic!("no \"is_levelled_tech\" object in dump"),
    };
    let is_action_obj = match root.get("is_action") {
        Some(Json::Obj(fields)) => fields.as_slice(),
        _ => panic!("no \"is_action\" object in dump"),
    };
    let is_gov_obj = match root.get("is_government") {
        Some(Json::Obj(fields)) => fields.as_slice(),
        _ => panic!("no \"is_government\" object in dump"),
    };

    let mut mismatches = Vec::new();
    let mut checked = 0usize;

    for (name, expected) in card_yields_obj {
        let Some(id) = CardId::by_name(name) else {
            mismatches.push(format!("{name}: not a Rust CardId"));
            continue;
        };
        checked += 1;

        // -------------------------------------------------------- card_yields
        let mut got = Vec::new();
        cards::card_yields(id, &mut got);
        let got_json = sorted_triples(rust_triples_as_json(&got));
        let want_json: Vec<(String, f64, String)> = sorted_triples(
            expected.as_arr().unwrap_or_else(|| panic!("{name}: card_yields entry is not an array")).iter().map(triple_from_json).collect(),
        );
        if got_json != want_json {
            mismatches.push(format!("{name}: card_yields rust={got_json:?} python={want_json:?}"));
        }

        // -------------------------------------------------------- card_choice
        let choice = cards::card_choice(id);
        let want_choice = card_choice_obj.iter().find(|(n, _)| n == name).map(|(_, v)| v);
        match (choice, want_choice) {
            (None, Some(Json::Null)) | (None, None) => {}
            (Some((a, b)), Some(Json::Arr(arr))) if arr.len() == 2 => {
                let want_a = triple_from_json(&arr[0]);
                let want_b = triple_from_json(&arr[1]);
                let got_a = (a.0.name().to_string(), a.1, kind_name(a.2).to_string());
                let got_b = (b.0.name().to_string(), b.1, kind_name(b.2).to_string());
                if (got_a, got_b) != (want_a, want_b) {
                    mismatches.push(format!("{name}: card_choice mismatch"));
                }
            }
            (rust, python) => {
                mismatches.push(format!("{name}: card_choice shape mismatch rust={rust:?} python={python:?}"));
            }
        }

        // --------------------------------------------------------- sum_yields
        //
        // The dump only records `sum_yields`' RESULT per named vector, not
        // the vector itself -- `local_vector` (below) rebuilds each named
        // vector independently, matching `tools/dump_weighted_cards.py::
        // _weight_vectors` by construction, so this is a check on
        // `sum_yields`' arithmetic and not a tautology against whatever the
        // dump happened to compute.
        if let Some((_, Json::Obj(vectors))) = sum_yields_obj.iter().find(|(n, _)| n == name) {
            for (vname, want_json) in vectors {
                let want = want_json.as_f64().unwrap_or_else(|| panic!("{name}/{vname}: not a number"));
                let w = local_vector(vname);
                let credit = w.get(WeightKey::CardRateCredit);
                let got = cards::sum_yields(&got, &w, credit);
                if (got - want).abs() >= 1e-9 {
                    mismatches.push(format!("{name}: sum_yields[{vname}] rust={got} python={want}"));
                }
            }
        } else {
            mismatches.push(format!("{name}: no sum_yields entry in dump"));
        }

        // --------------------------------------------------- board_credit_key
        let got_bck = cards::board_credit_key(id).map(|k| k.name());
        let want_bck = board_credit_key_obj.iter().find(|(n, _)| n == name).map(|(_, v)| v).and_then(Json::as_str);
        if got_bck != want_bck {
            mismatches.push(format!("{name}: board_credit_key rust={got_bck:?} python={want_bck:?}"));
        }

        // ---------------------------------------------------------- swap_type
        let got_st = cards::swap_type(id).map(type_name);
        let want_st = swap_type_obj.iter().find(|(n, _)| n == name).map(|(_, v)| v).and_then(Json::as_str);
        if got_st != want_st {
            mismatches.push(format!("{name}: swap_type rust={got_st:?} python={want_st:?}"));
        }

        // ------------------------------------------- the four CardType predicates
        let checks: [(&str, bool, &[(String, Json)]); 4] = [
            ("is_unit", id.kind().is_unit(), is_unit_obj),
            ("is_levelled_tech", board_yields::is_levelled_type(id.kind()), is_levelled_obj),
            ("is_action", id.kind().is_action(), is_action_obj),
            ("is_government", id.kind().is_government(), is_gov_obj),
        ];
        for (label, got_b, obj) in checks {
            let want_b = obj.iter().find(|(n, _)| n == name).and_then(|(_, v)| v.as_bool());
            if Some(got_b) != want_b {
                mismatches.push(format!("{name}: {label} rust={got_b} python={want_b:?}"));
            }
        }
    }

    assert_eq!(checked, 236, "expected 236 cards in the dump, found {checked}");
    eprintln!("weighted cards differential: {checked} cards, {} mismatches", mismatches.len());
    assert!(mismatches.is_empty(), "{} card mismatch(es):\n{}", mismatches.len(), mismatches.join("\n"));
    let _ = card_yields_obj;
}

/// The same four representative weight vectors `tools/dump_weighted_cards
/// .py::_weight_vectors` builds, reconstructed here by name rather than read
/// back out of the dump (the dump only records `sum_yields`' RESULT per
/// vector, not the vector itself -- rebuilding the inputs independently, by
/// name, is what makes this a check on `sum_yields`' arithmetic rather than
/// a tautology against whatever the dump happened to compute).
fn local_vector(name: &str) -> Weights {
    let mut w = Weights::default();
    match name {
        "default" => {}
        "neg_cost" => {
            w.set(WeightKey::Science, -3.0);
            w.set(WeightKey::ResourceStock, -2.0);
        }
        "zero_credit" => {
            w.set(WeightKey::CardRateCredit, 0.0);
            w.set(WeightKey::UnitStrengthCredit, 0.0);
            w.set(WeightKey::TerritoryCredit, 0.0);
            w.set(WeightKey::BonusCardCredit, 0.0);
        }
        "boosted_credit" => {
            w.set(WeightKey::CardRateCredit, 2.0);
            w.set(WeightKey::UnitStrengthCredit, 3.0);
            w.set(WeightKey::TerritoryCredit, 0.5);
            w.set(WeightKey::BonusCardCredit, 4.0);
            w.set(WeightKey::RestrictedResourceCredit, 0.7);
        }
        // The two extra vectors `tools/dump_weighted_cards.py::
        // _valuation_vectors` adds for the VALUATION layer -- see that
        // function's own doc comment for why the plumbing layer's four
        // vectors above never exercise these credits.
        "board_on" => {
            w.set(WeightKey::CardBoardCredit, 1.0);
            w.set(WeightKey::CardBoardLeader, 0.5);
            w.set(WeightKey::CardBoardGovernment, 0.3);
            w.set(WeightKey::CardBoardAction, 0.4);
            w.set(WeightKey::CardBoardWonder, 0.6);
            w.set(WeightKey::HandSwapExtra, 0.5);
            w.set(WeightKey::FreeActionCredit, 0.3);
        }
        "credits_off" => {
            w.set(WeightKey::TechBoardCredit, 0.0);
            w.set(WeightKey::GovBoardCredit, 0.0);
            w.set(WeightKey::ActionBoardCredit, 0.0);
            w.set(WeightKey::UnitTechCredit, 0.0);
        }
        other => panic!("unknown vector name in dump: {other}"),
    }
    w
}

// ------------------------------------------------- documented registries

#[test]
fn deliberately_unpriced_matches_python_both_directions() {
    let path = fixture_path();
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    let root = fixtures::parse_json(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let obj = match root.get("deliberately_unpriced") {
        Some(Json::Obj(fields)) => fields.as_slice(),
        _ => panic!("no \"deliberately_unpriced\" object in dump"),
    };

    let mut mismatches = Vec::new();
    for &(key, _reason) in cards::DELIBERATELY_UNPRICED {
        if !obj.iter().any(|(k, _)| k == key) {
            mismatches.push(format!("{key}: in Rust DELIBERATELY_UNPRICED, not in Python"));
        }
    }
    for (key, _) in obj {
        if !cards::DELIBERATELY_UNPRICED.iter().any(|&(k, _)| k == key) {
            mismatches.push(format!("{key}: in Python DELIBERATELY_UNPRICED, not in Rust"));
        }
    }
    assert!(mismatches.is_empty(), "{} mismatch(es):\n{}", mismatches.len(), mismatches.join("\n"));
}

#[test]
fn unpriced_values_matches_python_both_directions() {
    let path = fixture_path();
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    let root = fixtures::parse_json(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let arr = match root.get("unpriced_values") {
        Some(Json::Arr(items)) => items.as_slice(),
        _ => panic!("no \"unpriced_values\" array in dump"),
    };
    let python_pairs: Vec<(String, String)> = arr
        .iter()
        .map(|j| {
            let a = j.as_arr().unwrap_or_else(|| panic!("unpriced_values entry not an array: {j:?}"));
            (a[0].as_str().unwrap().to_string(), a[1].as_str().unwrap().to_string())
        })
        .collect();

    let mut mismatches = Vec::new();
    for &(name, key, _reason) in cards::UNPRICED_VALUES {
        if !python_pairs.iter().any(|(n, k)| n == name && k == key) {
            mismatches.push(format!("{name}/{key}: in Rust UNPRICED_VALUES, not in Python"));
        }
    }
    for (name, key) in &python_pairs {
        if !cards::UNPRICED_VALUES.iter().any(|&(n, k, _)| n == name && k == key) {
            mismatches.push(format!("{name}/{key}: in Python UNPRICED_VALUES, not in Rust"));
        }
    }
    assert!(mismatches.is_empty(), "{} mismatch(es):\n{}", mismatches.len(), mismatches.join("\n"));
}

// ==================================== the valuation layer (sampled states)

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn valuation_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/weighted_cards_fixtures")
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

fn f64_close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[derive(Default)]
struct ValuationReport {
    checked: usize,
    mismatches: Vec<String>,
}

/// Compares one `{name: value}` table (`card_potential`/`action_value`/
/// `tech_value`/`gov_value`) against `got`, called once per name the dump
/// recorded. `got` is `FnMut` rather than `Fn`: `card_potential`'s own check
/// threads a `&mut Vec<CardYield>` scratch buffer through it (mirroring how
/// a real caller reuses one buffer across a whole hand/row loop -- see
/// `cards::card_potential`'s own doc comment), which the other three tables
/// do not need but `FnMut` accepts either way.
fn check_named_table(label: &str, mut got: impl FnMut(CardId) -> f64, table: &Json, report: &mut ValuationReport, ctx: &str) {
    let Some(Json::Obj(fields)) = table.get(label) else {
        report.mismatches.push(format!("{ctx}: missing {label} table"));
        return;
    };
    for (name, want_json) in fields {
        let Some(id) = CardId::by_name(name) else {
            report.mismatches.push(format!("{ctx}: {label}[{name}]: not a Rust CardId"));
            continue;
        };
        let want = want_json.as_f64().unwrap_or_else(|| panic!("{ctx}: {label}[{name}] not a number"));
        report.checked += 1;
        let g = got(id);
        if !f64_close(g, want) {
            report.mismatches.push(format!("{ctx}: {label}[{name}]: rust={g} python={want}"));
        }
    }
}

fn check_valuation_player(path: &Path, ply: u32, state: &GameState, idx: u8, expected: &Json, report: &mut ValuationReport) {
    let ctx = format!("{}: ply {ply} player {idx}", path.display());

    // `tactic_terms` takes no weight vector -- dumped (and checked) once.
    report.checked += 1;
    let (gain, short) = cards::tactic_terms(state, idx);
    match expected.get("tactic_terms").and_then(Json::as_arr) {
        Some([g, s]) => {
            let (wg, ws) = (g.as_f64().unwrap_or(f64::NAN), s.as_f64().unwrap_or(f64::NAN));
            if !f64_close(gain, wg) || !f64_close(short, ws) {
                report.mismatches.push(format!("{ctx}: tactic_terms: rust=({gain},{short}) python=({wg},{ws})"));
            }
        }
        _ => report.mismatches.push(format!("{ctx}: missing tactic_terms")),
    }

    let vectors_obj = match expected.get("vectors") {
        Some(Json::Obj(fields)) => fields.as_slice(),
        _ => {
            report.mismatches.push(format!("{ctx}: missing vectors object"));
            return;
        }
    };

    for (vname, rec) in vectors_obj {
        let w = local_vector(vname);
        let vctx = format!("{ctx} [{vname}]");

        let mut scratch: Vec<CardYield> = Vec::new();
        check_named_table(
            "card_potential",
            |id| cards::card_potential(id, &w, Some(state), Some(idx), None, &mut scratch),
            rec,
            report,
            &vctx,
        );
        check_named_table("action_value", |id| cards::action_value(id, state, idx, &w, None), rec, report, &vctx);
        check_named_table("tech_value", |id| cards::tech_value(id, state, idx, &w, 1.0, None), rec, report, &vctx);
        check_named_table("gov_value", |id| cards::gov_value(id, state, idx, &w, None), rec, report, &vctx);

        // `wonder_potential` used to be skipped whenever the wonder actually
        // in progress was Hollywood or Internet, inheriting `board_yields`'s
        // unpriced-completion-culture gap through the same swap diff. That
        // gap closed 2026-08-05 (`effects::building_output` is ported), so
        // this is an ordinary comparison like the three beside it.
        let checks = [
            ("hand_potential", cards::hand_potential(state, idx, &w)),
            ("hand_mil_potential", cards::hand_mil_potential(state, idx, &w)),
            ("rival_hand_potential", cards::rival_hand_potential(state, idx, &w)),
            ("wonder_potential", cards::wonder_potential(state, idx, &w)),
        ];
        for (label, got) in checks {
            report.checked += 1;
            let want = rec.get(label).and_then(Json::as_f64).unwrap_or(f64::NAN);
            if !f64_close(got, want) {
                report.mismatches.push(format!("{vctx}: {label}: rust={got} python={want}"));
            }
        }
    }
}

fn check_valuation_file(path: &Path, expected_path: &Path, report: &mut ValuationReport) {
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
            check_valuation_player(path, ply, state, idx, p_expected, report);
        }
    }
}

/// The valuation-layer counterpart of `card_yields_matches_python_for_every_card`
/// above: board-aware, so it samples real states (`rust/tests/fixtures/*
/// .jsonl`) rather than walking every card once. See this file's own top doc
/// comment and `tools/dump_weighted_cards.py`'s for the exact dump shape.
#[test]
fn valuation_matches_python_on_sampled_fixture_states() {
    let dir = fixtures_dir();
    let edir = valuation_dir();
    let files = fixtures::fixture_files(&dir).unwrap_or_else(|e| panic!("{e}"));
    assert!(!files.is_empty(), "no fixtures in {}", dir.display());

    let mut report = ValuationReport::default();
    let mut files_checked = 0usize;
    for path in &files {
        let expected_path = edir.join(path.file_name().unwrap());
        if !expected_path.exists() {
            continue;
        }
        files_checked += 1;
        check_valuation_file(path, &expected_path, &mut report);
    }
    assert!(files_checked >= 3, "expected weighted_cards_fixtures per-ply jsonl for at least 2p/3p/4p, found {files_checked}");
    eprintln!(
        "weighted valuation differential: {files_checked} files, {} checks, {} mismatches",
        report.checked,
        report.mismatches.len()
    );
    assert!(
        report.mismatches.is_empty(),
        "{} valuation mismatch(es):\n{}",
        report.mismatches.len(),
        report.mismatches.join("\n")
    );
}
