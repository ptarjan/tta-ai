//! `typeprice` -- measurement probe for the type-price census (2026-08-25),
//! extending `tacticprice.rs`'s shape to EVERY card type that has its own
//! dedicated credit branch in `card_potential_core`
//! (`rust/src/bots/weighted/cards.rs`, around line 1945): Wonder, Tactic,
//! Aggression, War, Pact, Event, plus the three other credit-gated groups
//! named in the census -- Technology (the `tech_board_credit`-gated
//! non-unit levelled types: Farm/Mine/Lab/Temple/Library/Arena/Theater/
//! SpecialTech), Government and Action (their own dedicated `*_value`
//! functions, same dispatch shape) -- and Territory, which is priced a
//! different way (the static `card_yields` table, gated on
//! `territory_credit`) but is still a single credit key naming a whole
//! type.
//!
//! Read-only measurement: no bot behaviour changed by this bin.
//!
//! State construction is IDENTICAL to `tacticprice.rs`'s `mid_game_state`
//! (same seed, same `RandomBot`, same 40-`EndTurn` cutoff) so the numbers
//! here are directly comparable to `analysis/tactic_price_2026-08-25.txt`.
//!
//! `dominance_repair` silently clamps sign-gated weights at load time, so a
//! number read straight out of a champion JSON file is not necessarily the
//! number the bot actually uses -- every credit is printed twice below,
//! once as the raw JSON text and once post-load.
//!
//! ```text
//! cargo run --profile difftest --bin typeprice
//! ```
use std::path::Path;

use tta::bots::board_yields::{is_levelled_type, Baseline};
use tta::bots::greedy::RandomBot;
use tta::bots::weighted::cards::card_potential;
use tta::bots::weighted::eval::load_weights;
use tta::bots::weighted::weights::{WeightKey, Weights};
use tta::card_table::NUM_CARDS;
use tta::cards::{CardId, CardType};
use tta::game;
use tta::state::GameState;

fn mid_game_state(num_players: u8) -> GameState {
    let mut state = game::new_game(num_players, 42);
    let mut bot = RandomBot::new(1);
    let mut end_turns = 0;
    while end_turns < 40 && !game::is_over(&state) {
        let mv = bot.pick(&state);
        if mv == tta::moves::Move::EndTurn {
            end_turns += 1;
        }
        game::step(&mut state, mv);
    }
    state
}

/// Raw JSON text scrape for one weight's value -- deliberately NOT a full
/// JSON parser, just enough to pull `"<key>": <number>` back out of the
/// champion files for the load-time-clamp comparison `dominance_repair`
/// makes necessary. `key_name` comes from `WeightKey::name()` at the call
/// site, never a string literal typed here.
fn raw_json_value(text: &str, key_name: &str) -> Option<f64> {
    let needle = format!("\"{key_name}\":");
    let start = text.find(&needle)? + needle.len();
    let rest = &text[start..];
    let end = rest.find([',', '\n']).unwrap_or(rest.len());
    rest[..end].trim().parse::<f64>().ok()
}

fn is_wonder(k: CardType) -> bool {
    k == CardType::Wonder
}
fn is_tactic(k: CardType) -> bool {
    k == CardType::Tactic
}
fn is_aggression(k: CardType) -> bool {
    k == CardType::Aggression
}
fn is_war(k: CardType) -> bool {
    k == CardType::War
}
fn is_pact(k: CardType) -> bool {
    k == CardType::Pact
}
fn is_event(k: CardType) -> bool {
    k == CardType::Event
}
fn is_government(k: CardType) -> bool {
    k.is_government()
}
fn is_action(k: CardType) -> bool {
    k.is_action()
}
fn is_territory(k: CardType) -> bool {
    k == CardType::Territory
}
/// The `tech_board_credit`-gated group in `card_potential_core`: every
/// levelled type EXCEPT units, which are gated by their own
/// `unit_tech_credit` instead (see the `kind.is_unit()` branch that
/// intercepts first).
fn is_technology(k: CardType) -> bool {
    !k.is_unit() && is_levelled_type(k)
}

struct Group {
    label: &'static str,
    key: WeightKey,
    matches: fn(CardType) -> bool,
}

const GROUPS: &[Group] = &[
    Group { label: "Wonder", key: WeightKey::WonderBoardCredit, matches: is_wonder },
    Group { label: "Tactic", key: WeightKey::TacticBoardCredit, matches: is_tactic },
    Group { label: "Aggression", key: WeightKey::AggressionBoardCredit, matches: is_aggression },
    Group { label: "War", key: WeightKey::WarBoardCredit, matches: is_war },
    Group { label: "Pact", key: WeightKey::PactBoardCredit, matches: is_pact },
    Group { label: "Event", key: WeightKey::EventBoardCredit, matches: is_event },
    Group { label: "Technology", key: WeightKey::TechBoardCredit, matches: is_technology },
    Group { label: "Government", key: WeightKey::GovBoardCredit, matches: is_government },
    Group { label: "Action", key: WeightKey::ActionBoardCredit, matches: is_action },
    Group { label: "Territory", key: WeightKey::TerritoryCredit, matches: is_territory },
];

fn cards_of(matches: fn(CardType) -> bool) -> Vec<CardId> {
    (0..NUM_CARDS as u16).map(CardId).filter(|id| matches(id.kind())).collect()
}

fn group_prices(ids: &[CardId], w: &Weights, baseline: &Baseline) -> Vec<f64> {
    let mut scratch = Vec::new();
    ids.iter().map(|&id| card_potential(id, w, Some(baseline), None, &mut scratch)).collect()
}

struct Stats {
    n: usize,
    zero: usize,
    min_nonzero: Option<f64>,
    median_nonzero: Option<f64>,
    max_nonzero: Option<f64>,
    blackout: bool,
    flatline_value: Option<f64>,
}

fn median(sorted: &[f64]) -> f64 {
    let n = sorted.len();
    if n % 2 == 1 {
        sorted[n / 2]
    } else {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
    }
}

fn stats_of(prices: &[f64]) -> Stats {
    let n = prices.len();
    let zero = prices.iter().filter(|&&p| p == 0.0).count();
    let mut nonzero: Vec<f64> = prices.iter().copied().filter(|&p| p != 0.0).collect();
    nonzero.sort_by(|a, b| a.partial_cmp(b).expect("no NaN prices"));
    let min_nonzero = nonzero.first().copied();
    let max_nonzero = nonzero.last().copied();
    let median_nonzero = if nonzero.is_empty() { None } else { Some(median(&nonzero)) };
    let blackout = n > 0 && zero == n;
    // Flatline: every card of the type prices to the exact same NONZERO
    // value -- as diagnostic as a blackout, because it means the eval
    // cannot distinguish two cards of the type from each other.
    let flatline_value = if n > 0 && zero == 0 && !prices.is_empty() {
        let first = prices[0];
        if prices.iter().all(|&p| (p - first).abs() < 1e-9) {
            Some(first)
        } else {
            None
        }
    } else {
        None
    };
    Stats { n, zero, min_nonzero, median_nonzero, max_nonzero, blackout, flatline_value }
}

fn fmt_opt(v: Option<f64>) -> String {
    v.map_or_else(|| "--".to_string(), |x| format!("{x:.6}"))
}

fn main() {
    let w2 = load_weights(Path::new("../experiments/rust_champion_2p.json")).expect("2p weights");
    let w3 = load_weights(Path::new("../experiments/rust_champion_3p.json")).expect("3p weights");
    let w4 = load_weights(Path::new("../experiments/rust_champion_4p.json")).expect("4p weights");
    let raw2 = std::fs::read_to_string("../experiments/rust_champion_2p.json").expect("2p json text");
    let raw3 = std::fs::read_to_string("../experiments/rust_champion_3p.json").expect("3p json text");
    let raw4 = std::fs::read_to_string("../experiments/rust_champion_4p.json").expect("4p json text");

    let st2 = mid_game_state(2);
    let st3 = mid_game_state(3);
    let st4 = mid_game_state(4);
    let b2 = Baseline::at(&st2, 0);
    let b3 = Baseline::at(&st3, 0);
    let b4 = Baseline::at(&st4, 0);

    // Per-group price vectors, computed once, reused for both the summary
    // table and the raw per-card dump below.
    let mut all_ids: Vec<Vec<CardId>> = Vec::new();
    let mut all_p2: Vec<Vec<f64>> = Vec::new();
    let mut all_p3: Vec<Vec<f64>> = Vec::new();
    let mut all_p4: Vec<Vec<f64>> = Vec::new();
    for g in GROUPS {
        let ids = cards_of(g.matches);
        let p2 = group_prices(&ids, &w2, &b2);
        let p3 = group_prices(&ids, &w3, &b3);
        let p4 = group_prices(&ids, &w4, &b4);
        all_p2.push(p2);
        all_p3.push(p3);
        all_p4.push(p4);
        all_ids.push(ids);
    }

    println!("=== HEADLINE: blackout / flatline cells ===");
    println!(
        "{:<12} {:>4} {:>8} {:>9} {:>9} {:>9} {:>9} {:>9} {:>9} {:>10} {:>10}",
        "type", "size", "credit", "raw_json", "n", "zero", "min_nz", "median_nz", "max_nz", "blackout", "flatline"
    );
    for (gi, g) in GROUPS.iter().enumerate() {
        for (size, w, raw, prices) in
            [(2u8, &w2, &raw2, &all_p2[gi]), (3, &w3, &raw3, &all_p3[gi]), (4, &w4, &raw4, &all_p4[gi])]
        {
            let post_load = w.get(g.key);
            let raw_val = raw_json_value(raw, g.key.name());
            let s = stats_of(prices);
            let flag = if s.blackout {
                "BLACKOUT"
            } else if s.flatline_value.is_some() {
                "FLATLINE"
            } else {
                "-"
            };
            println!(
                "{:<12} {:>4}p {:>8.5} {:>9} {:>9} {:>9} {:>9} {:>9} {:>9} {:>10} {:>10}",
                g.label,
                size,
                post_load,
                raw_val.map_or_else(|| "--".to_string(), |x| format!("{x:.5}")),
                s.n,
                s.zero,
                fmt_opt(s.min_nonzero),
                fmt_opt(s.median_nonzero),
                fmt_opt(s.max_nonzero),
                if s.blackout { "yes" } else { "no" },
                s.flatline_value.map_or_else(|| "no".to_string(), |v| format!("yes({v:.4})")),
            );
            if flag != "-" {
                println!("  ^^^ {flag}: {} @ {size}p", g.label);
            }
        }
    }

    println!("\n=== ANCHOR CHECK (commit 66588fe): Tactic 3p in [53.49, 695.32], Tactic 2p in [14.26, 185.42] ===");
    let tactic_idx = GROUPS.iter().position(|g| g.label == "Tactic").expect("Tactic group present");
    let t2 = stats_of(&all_p2[tactic_idx]);
    let t3 = stats_of(&all_p3[tactic_idx]);
    println!(
        "Tactic 2p observed min/max nonzero = {} / {} (expect within 14.26..185.42)",
        fmt_opt(t2.min_nonzero),
        fmt_opt(t2.max_nonzero)
    );
    println!(
        "Tactic 3p observed min/max nonzero = {} / {} (expect within 53.49..695.32)",
        fmt_opt(t3.min_nonzero),
        fmt_opt(t3.max_nonzero)
    );
    let anchor_2p_ok = t2.min_nonzero.is_some_and(|v| (14.26..=185.42).contains(&v))
        || t2.max_nonzero.is_some_and(|v| (14.26..=185.42).contains(&v));
    let anchor_3p_ok = t3.min_nonzero.is_some_and(|v| (53.49..=695.32).contains(&v))
        || t3.max_nonzero.is_some_and(|v| (53.49..=695.32).contains(&v));
    println!("ANCHOR_2P_IN_RANGE={anchor_2p_ok} ANCHOR_3P_IN_RANGE={anchor_3p_ok}");

    println!("\n=== RAW PER-CARD DUMP ===");
    for (gi, g) in GROUPS.iter().enumerate() {
        println!("\n--- {} (credit={:?}) ---", g.label, g.key);
        println!("{:<32} {:>6} {:>14} {:>14} {:>14}", "card", "id", "price@2p", "price@3p", "price@4p");
        for (ci, &id) in all_ids[gi].iter().enumerate() {
            println!(
                "{:<32} {:>6} {:>14.6} {:>14.6} {:>14.6}",
                id.name(),
                id.0,
                all_p2[gi][ci],
                all_p3[gi][ci],
                all_p4[gi][ci]
            );
        }
    }
}
