//! Differential test for the yield-plumbing layer of `bots::weighted::cards`
//! against `tools/dump_weighted_cards.py`'s output
//! (`rust/tests/weighted_cards_fixtures/card_yields.json`) -- see that
//! script's own doc comment for the exact dump shape and for why this checks
//! every card in the database rather than sampling (`card_yields`/
//! `card_choice`/`swap_type`/`board_credit_key`/`is_unit`/`is_levelled_tech`/
//! `is_action`/`is_government` are pure functions of card identity, no board,
//! so full coverage of 236 cards is cheap where a board-aware question would
//! need to sample states).
//!
//! `yield_marginal` is not checked here -- see the dump script's doc comment
//! for why (it depends on `feature_marginal`, not yet ported); its own
//! arithmetic is unit-tested in `bots::weighted::cards` against a mock
//! closure instead.

use std::path::{Path, PathBuf};

use tta::bots::board_yields;
use tta::bots::weighted::cards::{self, CardYield, YieldKind};
use tta::bots::weighted::weights::{Weights, WeightKey};
use tta::cards::{CardId, CardType};
use tta::fixtures::{self, Json};

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
