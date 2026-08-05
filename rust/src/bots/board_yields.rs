//! What a player's board yields, computed by asking the engine.
//!
//! Ports `engine/bots/board_yields.py` (1,506 lines) -- read that file's own
//! module doc comment first; it is the design rationale for everything below
//! and is not restated here except where the Rust shape earns its own note.
//! Short version: a card whose value is written in prose (a leader, a
//! government, a wonder) is priced by SWAPPING it into the player's board and
//! diffing `effects::compute` before/after, rather than by a hand-written
//! table that drifts from the rules the moment a leader like Michelangelo or
//! Sid Meier is involved. `effects::compute` is the rules; this module never
//! reimplements a rating, only reads the delta.
//!
//! ## Two deliberate shape changes from the Python
//!
//! **No mutate-and-restore, no cache, no `_plan_key`.** Python mutates
//! `p.leader` (etc.), calls `effects.compute`, and restores it in a
//! `try`/`finally` -- and then memoises on a hand-rolled key
//! (`_plan_key`/`_DELTA_CACHE`/`_UNIT_CACHE`/`_TECH_CACHE`/`_BUILD_CACHE`)
//! because the mutate/restore/recompute round trip is expensive enough to be
//! worth caching, and because Python's `state_stats` has a SEPARATE cache
//! that mutating `p.leader` without invalidating would silently return the
//! wrong (pre-swap) answer for -- "the trap" the Python module's own doc
//! comment devotes a whole section to.
//!
//! `PlayerState` here is `Clone` (`state.rs`: cloning the search tree is
//! already the hot path this engine is built around), so [`swap_stats`]
//! below clones the player, mutates the CLONE, and calls `effects::compute`
//! on it -- the original is never touched, so there is no restore step and
//! no trap to fall into: the bug class "the cache was read behind the
//! mutation's back" cannot occur because nothing here is cached.
//! `effects::compute` is recomputed on every call, exactly as `effects.rs`'s
//! own top doc comment already establishes for the engine itself ("there is
//! no hot loop a cache is fixing... add it later, measured"); this module
//! inherits that policy rather than re-litigating it, and Python's `_plan_key`
//! machinery (which exists ONLY to make that cache safe) has no Rust
//! counterpart to port.
//!
//! ## Two known gaps, found while porting, NOT fixed here
//!
//! **Hollywood / Internet culture-on-completion is unpriced.** Wonder
//! completion culture is priced by calling
//! [`crate::apply::wonder_completion_culture`], the SAME function
//! `apply.rs`'s own `on_wonder_complete` pays out for real -- one
//! implementation, so the evaluator and the scorer cannot disagree, exactly
//! the property the Python module's docstring calls out as the whole point.
//! That function already carries a NAMED, TESTED gap
//! (`apply.rs::one_time_culture`, and its own test
//! `wonder_completion_culture_hollywood_is_a_named_gap`): Hollywood and
//! Internet score off `effects::building_output` in Python (what the
//! buildings ACTUALLY produce, modifiers included), and `building_output` is
//! not ported to `effects.rs` yet, so the Rust function panics for exactly
//! those two card names. [`on_build_culture`] below guards both names and
//! returns no rider rather than calling into that panic -- so a Hollywood or
//! Internet wonder in hand or in the row is priced as if it had no
//! completion-culture rider at all, understating it by whatever
//! `building_output` would have said. This is `apply.rs`'s/`effects.rs`'s
//! gap, not this module's to close; flagged here, and in the port report.
//!
//! ~~**`board_extra`'s per-player-count coefficient is not carried by the
//! type layer.**~~ CLOSED 2026-08-05. Endowment for the Arts / Wave of
//! Nationalism / Military Build-Up each print a `{"2p": N, "3p": N, "4p": N}`
//! table as the VALUE of `culturePerCivilizationWithMoreCulture` /
//! `resourcesForMilitaryUnitsPerStrongerCivilization`. `gen_cards.py` now
//! folds that table into a real `Special::<Name>([i16; 3])` payload (the same
//! shape `strongestPlayers`/`weakestPlayers`/`condition` already used), so
//! [`board_extra`] below reads the coefficient off the card directly rather
//! than merely detecting the key's presence.

use crate::apply;
use crate::cards::{CardId, CardType, Special};
use crate::costs;
use crate::economy;
use crate::effects::{self, Stats};
use crate::legal;
use crate::state::{GameState, PlayerState, TechSlot};

// ------------------------------------------------------------------ triples
//
// Python's triples are `(feature: str, amount: float, kind: int)`. A string
// key is exactly DESIGN.md rule 2's `HashMap<String, f64>` in miniature, so
// [`Feature`] is a closed enum instead -- every string key `board_yields.py`
// ever emits, named once, matched exhaustively nowhere needed (this module
// only ever CONSTRUCTS these, never dispatches on them) but comparable and
// hashable for [`merge`].

/// Every feature name `engine/bots/board_yields.py` emits into a yield
/// triple, named 1:1 (`python_key` below is the exact string). What each one
/// means to the evaluator is `weighted.py`'s concern, not this module's --
/// this enum only needs to be a value `weighted.rs` can eventually match on
/// without hashing a string.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Feature {
    CultureRate,
    ScienceRate,
    FoodRate,
    ResourceRate,
    Strength,
    HappyMargin,
    CivilActions,
    MilitaryActions,
    ColonizeBonus,
    UrbanLimit,
    WonderStagesPerAction,
    HandLimit,
    BuildDiscount,
    PopCost,
    NoAggression,
    Leader,
    GovLevel,
    TechLevels,
    GovActionCost,
    Science,
    CaLeft,
    MaLeft,
    Culture,
    FreeWorkers,
    BlueFree,
    Wonders,
    ResourceStock,
    NumTechs,
    SpecialTechs,
    BestFarm,
    BestMine,
    BestLab,
    BestTemple,
    BestTheater,
    BestLibrary,
    BestArena,
    BestUnit,
    Workers,
    ProdWorkers,
    UrbanWorkers,
    UnitWorkers,
    Uprising,
    RestrictedResources,
}

impl Feature {
    /// The exact Python string this variant mirrors -- for tests and for the
    /// differential dump/compare, so a mismatch names itself rather than
    /// requiring a side-by-side enum reading.
    pub fn python_key(self) -> &'static str {
        use Feature::*;
        match self {
            CultureRate => "culture_rate",
            ScienceRate => "science_rate",
            FoodRate => "food_rate",
            ResourceRate => "resource_rate",
            Strength => "strength",
            HappyMargin => "happy_margin",
            CivilActions => "civil_actions",
            MilitaryActions => "military_actions",
            ColonizeBonus => "colonize_bonus",
            UrbanLimit => "urban_limit",
            WonderStagesPerAction => "wonder_stages_per_action",
            HandLimit => "hand_limit",
            BuildDiscount => "build_discount",
            PopCost => "pop_cost",
            NoAggression => "no_aggression",
            Leader => "leader",
            GovLevel => "gov_level",
            TechLevels => "tech_levels",
            GovActionCost => "gov_action_cost",
            Science => "science",
            CaLeft => "ca_left",
            MaLeft => "ma_left",
            Culture => "culture",
            FreeWorkers => "free_workers",
            BlueFree => "blue_free",
            Wonders => "wonders",
            ResourceStock => "resource_stock",
            NumTechs => "num_techs",
            SpecialTechs => "special_techs",
            BestFarm => "best_farm",
            BestMine => "best_mine",
            BestLab => "best_lab",
            BestTemple => "best_temple",
            BestTheater => "best_theater",
            BestLibrary => "best_library",
            BestArena => "best_arena",
            BestUnit => "best_unit",
            Workers => "workers",
            ProdWorkers => "prod_workers",
            UrbanWorkers => "urban_workers",
            UnitWorkers => "unit_workers",
            Uprising => "uprising",
            RestrictedResources => "restricted_resources",
        }
    }
}

/// The third slot of a yield triple. Mirrors Python's `_GAIN = 0` / `_COST =
/// 1` (`weighted._Y_GAIN` / `_Y_COST`, imported by value there to avoid a
/// circular import -- no such constraint here, so this is just an enum).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Gain,
    Cost,
}

/// `(feature, amount, kind)` -- what a card is worth, one line item at a
/// time. A `Vec` rather than a fixed array: these are short-lived scratch
/// values built and consumed within one bot decision, never a field of
/// [`GameState`] (which is what DESIGN.md rule 3's "no `Vec`" is actually
/// protecting -- a clone-as-memcpy state that the search copies constantly).
pub type Triple = (Feature, f64, Kind);

/// One triple per `(feature, kind)`, summed, in first-seen order. Mirrors
/// Python's `_merge`: without this, a caller that folds triples into a table
/// keyed on feature (a census, a test) would silently keep only the LAST of
/// two entries for the same feature -- Gandhi replacing Churchill is exactly
/// this, +2 culture from the `Stats` diff and -3 from the rider subtraction,
/// both landing on `culture_rate`.
///
/// A single `Vec<((Feature, Kind), f64)>` rather than a parallel `order`
/// (the key) and `sums` (the running total) kept in lockstep by index: two
/// `Vec`s updated in the same two places by construction can drift out of
/// step the moment one of the two push/update sites is edited without the
/// other, which is exactly the Python-dict-in-two-arrays shape this type
/// exists to avoid.
pub fn merge(triples: Vec<Triple>) -> Vec<Triple> {
    let mut merged: Vec<((Feature, Kind), f64)> = Vec::new();
    for (feat, amt, kind) in triples {
        match merged.iter_mut().find(|((f, k), _)| *f == feat && *k == kind) {
            Some((_, sum)) => *sum += amt,
            None => merged.push(((feat, kind), amt)),
        }
    }
    merged
        .into_iter()
        .filter(|&(_, amt)| amt != 0.0)
        .map(|((f, k), amt)| (f, amt, k))
        .collect()
}

// ------------------------------------------------------------ card-type sets
//
// Mirrors `SWAP_TYPES` / `SINGLE_SLOT` / `LEVELLED_TYPES` -- predicates over
// `CardType` rather than a `frozenset` of strings, per DESIGN.md rule 1.

/// Card types priced by swapping the card in and diffing `effects::compute`.
/// See this module's Python counterpart's doc comment for why wonders are
/// included (a pure gain, nothing netted off) alongside leader/government
/// (a replacement, netted against the incumbent).
pub fn is_swap_type(kind: CardType) -> bool {
    matches!(kind, CardType::Leader | CardType::Government | CardType::Wonder)
}

/// Strictly narrower than [`is_swap_type`]: you hold at most one leader and
/// one government, so two in hand are two candidates for ONE replacement.
/// A wonder is not single-slot -- see the Python module's doc comment.
pub fn is_single_slot(kind: CardType) -> bool {
    matches!(kind, CardType::Leader | CardType::Government)
}

/// Types whose development adds to `tech_levels` -- every worker type plus
/// special technologies. Mirrors `C.WORKER_TYPES | {"special-tech"}`.
pub fn is_levelled_type(kind: CardType) -> bool {
    kind.takes_workers() || kind == CardType::SpecialTech
}

/// `_WORKER_CLASS`: which worker-count feature a build of this type feeds.
fn worker_class(kind: CardType) -> Option<Feature> {
    if kind.is_unit() {
        Some(Feature::UnitWorkers)
    } else if kind.is_urban() {
        Some(Feature::UrbanWorkers)
    } else if kind.is_production() {
        Some(Feature::ProdWorkers)
    } else {
        None
    }
}

/// `_BEST_FEATURE`: which `best_*` feature developing this type feeds. The
/// four unit types share `best_unit`, exactly as `weighted.features` computes
/// it.
fn best_feature(kind: CardType) -> Option<Feature> {
    use CardType::*;
    match kind {
        Farm => Some(Feature::BestFarm),
        Mine => Some(Feature::BestMine),
        Lab => Some(Feature::BestLab),
        Temple => Some(Feature::BestTemple),
        Theater => Some(Feature::BestTheater),
        Library => Some(Feature::BestLibrary),
        Arena => Some(Feature::BestArena),
        Infantry | Cavalry | Artillery | Air => Some(Feature::BestUnit),
        _ => None,
    }
}

/// Whether `p`'s active leader's printed name is `name`. Mirrors
/// `costs.rs::leader_is` (private to that module, so restated here rather
/// than exported across an unrelated boundary) -- see that module's "A note
/// on leader identity" for why this is a name compare and not a `Special`.
fn leader_is(p: &PlayerState, name: &str) -> bool {
    !p.leader.is_none() && p.leader.get().name == name
}

// ------------------------------------------------------------- the swap diff

/// `effects::compute` with a hypothetical change applied to a CLONE of
/// `state.players[idx]`. See this module's top doc comment for why this
/// replaces Python's mutate/`effects.compute`/restore -- the original player
/// is never touched, so there is nothing to restore and nothing a stale cache
/// could read.
fn swap_stats(state: &GameState, idx: usize, mutate: impl FnOnce(&mut PlayerState)) -> Stats {
    let mut p = state.players[idx].clone();
    mutate(&mut p);
    effects::compute(state, &p)
}

/// `_STATS_FEATURES`: `Stats` field -> evaluator feature, for the fields
/// whose delta means something on its own. Function pointers rather than a
/// closure-capturing table: none of these capture anything, so this is a
/// `const` array of plain `fn(&Stats) -> i32`, no allocation, no `HashMap`.
const STATS_FEATURES: &[(fn(&Stats) -> i32, Feature)] = &[
    (|s| s.culture, Feature::CultureRate),
    (|s| s.science, Feature::ScienceRate),
    (|s| s.food, Feature::FoodRate),
    (|s| s.resources, Feature::ResourceRate),
    (|s| s.strength, Feature::Strength),
    (|s| s.happy, Feature::HappyMargin),
    (|s| s.civil_actions, Feature::CivilActions),
    (|s| s.military_actions, Feature::MilitaryActions),
    (|s| s.colonize, Feature::ColonizeBonus),
    (|s| s.urban_limit, Feature::UrbanLimit),
    (|s| s.wonder_stages, Feature::WonderStagesPerAction),
];

/// `_pop_cost`: `weighted.features`' `pop_cost`, to the letter, INCLUDING its
/// deliberate blind spot -- Python passes no `one_time` argument here
/// (`economy.pop_food_cost(stats, p.yellow_bank)`, two arguments, matching
/// `weighted.features`' own call), so the event-granted one-time population
/// discount is NOT applied on this path even though `economy::pop_food_cost`
/// can take one. That is Python's `pop_food_cost`'s own doc comment calling
/// this "a real (small) blind spot ... but fixing it changes what the bot
/// plays" -- reproduced here verbatim by passing `0`, not silently corrected.
fn pop_cost_feature(stats: &Stats, p: &PlayerState) -> f64 {
    match economy::pop_food_cost(stats.pop_food_discount, p.yellow_bank, 0) {
        Some(v) => v as f64,
        // `_POP_SENTINEL`: `weighted.features`' "cannot increase population
        // at all" value, so the diff and the board cannot disagree about
        // Moses turning a possible increase into an impossible one or back.
        None => 8.0,
    }
}

/// `_delta_triples`: yield triples for the difference between two `Stats` of
/// one player. Shared by the swap diff (leader/government/wonder) and the
/// technology diff (`tech_upgrade`) so both read the SAME fields through the
/// SAME feature names -- two copies of this list is exactly how Python's
/// `_PROD_TO_FEATURE` and `_YIELD_TO_FEATURE` drifted apart once. Pushes
/// into `out`: every call site immediately folds this into a larger
/// accumulator (the swap diff, the government gain list, the tech-upgrade
/// staff triples), so there is nothing for an intermediate `Vec` to buy.
fn delta_triples(before: &Stats, after: &Stats, p: &PlayerState, out: &mut Vec<Triple>) {
    for &(get, feat) in STATS_FEATURES {
        let d = get(after) - get(before);
        if d != 0 {
            out.push((feat, d as f64, Kind::Gain));
        }
    }
    let d = (after.civil_hand_limit + after.military_hand_limit)
        - (before.civil_hand_limit + before.military_hand_limit);
    if d != 0 {
        out.push((Feature::HandLimit, d as f64, Kind::Gain));
    }
    let before_bd: i32 = before.build_discount.iter().sum();
    let after_bd: i32 = after.build_discount.iter().sum();
    if after_bd != before_bd {
        out.push((Feature::BuildDiscount, (after_bd - before_bd) as f64, Kind::Gain));
    }
    // Moses, priced through the feature the board evaluation actually reads
    // -- see `pop_cost_feature`'s doc comment.
    let d = pop_cost_feature(after, p) - pop_cost_feature(before, p);
    if d != 0.0 {
        out.push((Feature::PopCost, d, Kind::Gain));
    }
    // Gandhi's `cannotPlayAggressionOrWar`: symmetric on purpose (replacing
    // Gandhi LIFTS the restriction, a real change the other way too).
    let d = after.no_aggression as i32 - before.no_aggression as i32;
    if d != 0 {
        out.push((Feature::NoAggression, d as f64, Kind::Gain));
    }
}

// ------------------------------------------------------------------- riders
//
// What the swap diff CANNOT see, card by card. Mirrors Python's `RIDERS`.

fn live_rivals<'a>(state: &'a GameState, p: &PlayerState) -> Vec<&'a PlayerState> {
    state.players[..state.num_players as usize]
        .iter()
        .filter(|q| q.idx != p.idx && !q.resigned)
        .collect()
}

/// Genghis Khan: 3 culture at the end of your turn if you are one of the two
/// strongest civilizations, ties in your favour. Pushes into `out` rather
/// than returning a fresh `Vec` -- see this module's top doc comment on the
/// out-param convention shared by every rider helper below; this one only
/// ever has 0 or 1 item to add.
fn genghis(state: &GameState, p: &PlayerState, out: &mut Vec<Triple>) {
    let mine = effects::state_stats(state, p).strength;
    let stronger = live_rivals(state, p)
        .into_iter()
        .filter(|q| effects::state_stats(state, q).strength > mine)
        .count();
    if stronger <= 1 {
        out.push((Feature::CultureRate, 3.0, Kind::Gain));
    }
}

/// Winston Churchill: the unconditional culture option, priced as the floor
/// on the card's value -- see the Python module's doc comment for why the
/// military option is not (it is ring-fenced and cannot be worth its face).
fn churchill(_state: &GameState, _p: &PlayerState, out: &mut Vec<Triple>) {
    out.push((Feature::CultureRate, 3.0, Kind::Gain));
}

/// `RIDERS`: leader name -> rider function. Only the leaders whose value is
/// not in `Stats` and IS computable.
fn rider_of(leader_name: &str) -> Option<fn(&GameState, &PlayerState, &mut Vec<Triple>)> {
    match leader_name {
        "Genghis Khan" => Some(genghis),
        "Winston Churchill" => Some(churchill),
        _ => None,
    }
}

/// `_rider_delta`: rider triples for taking `name`, MINUS the rider of the
/// leader it replaces -- the subtraction is the whole point (taking Gandhi
/// while holding Churchill is a LOSS of Churchill's culture rider). Pushes
/// into `out`: the negation of the replaced leader's rider still needs its
/// own scratch `Vec` (there is no way to push a negated amount without
/// computing it first), but the net-new rider goes straight into the
/// caller's accumulator with no intermediate allocation.
fn rider_delta(state: &GameState, p: &PlayerState, name: CardId, out: &mut Vec<Triple>) {
    if let Some(f) = rider_of(name.get().name) {
        f(state, p, out);
    }
    if !p.leader.is_none() {
        if let Some(f) = rider_of(p.leader.get().name) {
            let mut replaced = Vec::new();
            f(state, p, &mut replaced);
            for (feat, amt, kind) in replaced {
                out.push((feat, -amt, kind));
            }
        }
    }
}

// ------------------------------------------------------- government costs

/// `_government_cost`: the science a government actually costs (the
/// revolution price, cheaper on every card in the deck -- see the Python
/// module's doc comment for why that route is the one priced) and the civil
/// actions it burns.
fn government_cost(state: &GameState, p: &PlayerState, name: CardId, out: &mut Vec<Triple>) {
    let card = name.get();
    let sci = if card.revolution_cost != 0 { card.revolution_cost as i32 } else { card.peaceful_cost as i32 };
    if sci != 0 {
        out.push((Feature::Science, -(sci as f64), Kind::Cost));
    }
    let burned = effects::state_stats(state, p).civil_actions;
    if burned != 0 {
        out.push((Feature::GovActionCost, -(burned as f64), Kind::Gain));
    }
}

/// `_government_level`: the `tech_levels` / `gov_level` delta of replacing
/// the government -- RULES_SPEC 8.1, "new government always replaces the old
/// regardless of level", so a lateral or downgrade move is representable
/// (and typically never legal to choose, but the number is honest either
/// way). Pushes into `out`, like [`government_cost`]/[`wonder_cost`] beside
/// it -- 0 or 2 items, never its own independent collection.
fn government_level(p: &PlayerState, name: CardId, out: &mut Vec<Triple>) {
    let d = name.level() as i32 - p.government.level() as i32;
    if d != 0 {
        out.push((Feature::TechLevels, d as f64, Kind::Gain));
        out.push((Feature::GovLevel, d as f64, Kind::Gain));
    }
}

/// `_government_routes`: the cost triples of each LEGAL route to `name`,
/// cheapest chosen later by the caller (which holds the weights) -- the same
/// division of labour `board_choices` uses for a card that makes you pick.
/// Left returning `Vec<Vec<Triple>>` rather than an out-param, unlike most
/// of this file's other small helpers: this genuinely IS an independent
/// collection -- a set of mutually exclusive alternatives, 1 or 2 of them,
/// each its own multi-item route -- not a handful of triples that get
/// folded flat into one accumulator. [`government_plans`] returns it
/// unmodified, so there is no accumulator downstream for an out-param to
/// write into.
/// `gained` is the swap diff, read for the allotment deltas rather than
/// recomputing them.
fn government_routes(state: &GameState, p: &PlayerState, name: CardId, gained: &[Triple]) -> Vec<Vec<Triple>> {
    let find = |feat: Feature| gained.iter().find(|&&(f, _, _)| f == feat).map_or(0.0, |&(_, a, _)| a);
    let d_ca = find(Feature::CivilActions);
    let d_ma = find(Feature::MilitaryActions);

    // ---- peaceful (RULES_SPEC 8.2): one civil action, the HIGHER science
    // cost (`costs::tech_cost` -- a government's peaceful price).
    let mut peaceful = vec![(Feature::CaLeft, -1.0, Kind::Cost)];
    let sci = costs::tech_cost(state, p, name).unwrap_or(0);
    if sci != 0 {
        peaceful.push((Feature::Science, -(sci as f64), Kind::Cost));
    }
    if d_ca != 0.0 {
        peaceful.push((Feature::CaLeft, d_ca, Kind::Gain));
    }
    if d_ma != 0.0 {
        peaceful.push((Feature::MaLeft, d_ma, Kind::Gain));
    }
    let mut routes = vec![peaceful];

    // ---- revolution (RULES_SPEC 8.3), only when the engine would offer it.
    if legal::can_revolt(state, p, name) {
        let card = name.get();
        let mut rev = vec![(Feature::Science, -(card.revolution_cost as f64), Kind::Cost)];
        if leader_is(p, "Maximilien Robespierre") {
            if p.military_actions != 0 {
                rev.push((Feature::MaLeft, -(p.military_actions as f64), Kind::Cost));
            }
            if d_ca != 0.0 {
                rev.push((Feature::CaLeft, d_ca, Kind::Gain));
            }
            rev.push((Feature::Culture, 3.0, Kind::Gain));
        } else {
            let mut left = p.civil_actions as f64;
            if leader_is(p, "Isaac Newton") {
                left = (left - 1.0).max(0.0);
            }
            if left != 0.0 {
                rev.push((Feature::CaLeft, -left, Kind::Cost));
            }
            if d_ma != 0.0 {
                rev.push((Feature::MaLeft, d_ma, Kind::Gain));
            }
        }
        routes.push(rev);
    }
    routes
}

/// `government_plans`: (gain triples, cost routes) for putting `name` in
/// play as government. `(vec![], vec![])` for a non-government card, or the
/// government already in play (dead in hand: not a cost, not a gain).
pub fn government_plans(name: CardId, state: &GameState, idx: usize) -> (Vec<Triple>, Vec<Vec<Triple>>) {
    if name.is_none() || name.kind() != CardType::Government {
        return (Vec::new(), Vec::new());
    }
    let p = &state.players[idx];
    if p.government == name {
        return (Vec::new(), Vec::new());
    }
    let before = effects::state_stats(state, p);
    let after = swap_stats(state, idx, |pl| pl.government = name);
    let mut gained = Vec::new();
    delta_triples(&before, &after, p, &mut gained);
    government_level(p, name, &mut gained);
    let gained = merge(gained);
    let routes = government_routes(state, p, name, &gained);
    (gained, routes)
}

// ----------------------------------------------------------- wonder riders

/// `_on_build_culture`: the four Age III wonders' one-time completion
/// culture. See this module's top doc comment for the Hollywood/Internet
/// gap this guards against.
fn on_build_culture(p: &PlayerState, name: CardId, out: &mut Vec<Triple>) {
    let base = name.get().base_name;
    if base == "Hollywood" || base == "Internet" {
        return;
    }
    let got = apply::wonder_completion_culture(p, name);
    if got != 0 {
        out.push((Feature::Culture, got as f64, Kind::Gain));
    }
}

/// Fraction of player-turns on which a population increase happens at all --
/// see `engine/bots/board_yields.py::FREE_POP_UTIL`'s extensive doc comment
/// for how this was measured and what it trades off; the value is a MEASURED
/// constant, not a Rust-side choice, and moves only if the Python one does.
const FREE_POP_UTIL: f64 = 0.17;

/// `_free_pop_increase`: Ocean Liners. Neither argument passes a one-time
/// discount, matching Python's `economy.pop_food_cost(s, p.yellow_bank)` --
/// see [`pop_cost_feature`]'s doc comment for why that omission is
/// deliberate and shared.
fn free_pop_increase(state: &GameState, p: &PlayerState, out: &mut Vec<Triple>) {
    let s = effects::state_stats(state, p);
    let Some(food) = economy::pop_food_cost(s.pop_food_discount, p.yellow_bank, 0) else {
        return;
    };
    if s.happy < economy::happy_required(p.yellow_bank.saturating_sub(1)) as i32 {
        return;
    }
    out.push((Feature::CivilActions, FREE_POP_UTIL, Kind::Gain));
    out.push((Feature::FoodRate, FREE_POP_UTIL * food as f64, Kind::Gain));
    out.push((Feature::FreeWorkers, 1.0 - FREE_POP_UTIL, Kind::Gain));
}

/// `_blue_tokens`: Taj Mahal's `blueTokens`, invisible to the swap diff
/// because `effects::compute` never reads `CardEffects::blue_tokens` --
/// `apply::on_enter_play` adds it to `p.blue_total` directly.
fn blue_tokens_rider(name: CardId, out: &mut Vec<Triple>) {
    let n = name.get().effects.blue_tokens;
    if n != 0 {
        out.push((Feature::BlueFree, n as f64, Kind::Gain));
    }
}

/// `_wonder_rider_delta`: every rider this wonder's printed keys trigger.
/// Python iterates the raw `effects` dict and looks each key up in
/// `WONDER_RIDERS`; here the four keys are checked directly against the
/// typed `Special`/`CardEffects` fields that carry them. `onBuildCulture`
/// and `onBuildCulturePerTechLevelSum` share one handler and are checked
/// together (both route to the same [`on_build_culture`]) rather than as two
/// separate `if`s that could double-fire -- verified against the live card
/// data (2026-08-05) that no base-game wonder ever prints both at once, so
/// this is a structural simplification, not a behaviour change.
fn wonder_rider_delta(state: &GameState, p: &PlayerState, name: CardId, out: &mut Vec<Triple>) {
    let card = name.get();
    let has_on_build_culture = card.special.iter().any(|s| matches!(s, Special::OnBuildCulture(_)))
        || card.special.contains(&Special::OnBuildCulturePerTechLevelSum);
    if has_on_build_culture {
        on_build_culture(p, name, out);
    }
    if card.special.contains(&Special::FreePopIncreasePerTurn) {
        free_pop_increase(state, p, out);
    }
    if card.effects.blue_tokens != 0 {
        blue_tokens_rider(name, out);
    }
}

/// `_wonder_cost`: what a wonder costs, which the swap diff does not charge
/// for (`card_potential` prices a swap card by the diff ALONE). `state`/`p`
/// are unused in the Python original too (its signature carries them without
/// reading either), so they are dropped here rather than threaded through
/// for nothing.
fn wonder_cost(name: CardId, out: &mut Vec<Triple>) {
    out.push((Feature::Wonders, 1.0, Kind::Gain));
    let stages = name.get().stages;
    if !stages.is_empty() {
        let total: i32 = stages.iter().map(|&s| s as i32).sum();
        out.push((Feature::ResourceStock, -(total as f64), Kind::Cost));
    }
}

// -------------------------------------------------------------- entry point

/// `board_yields`: `(feature, amount, kind)` triples for `name` on this
/// board, or `None` meaning "not board-priced, use the static table".
pub fn board_yields(name: CardId, state: &GameState, idx: usize) -> Option<Vec<Triple>> {
    if name.is_none() {
        return None;
    }
    let typ = name.kind();
    if !is_swap_type(typ) {
        return None;
    }
    let p = &state.players[idx];
    let before = effects::state_stats(state, p);
    let after = match typ {
        CardType::Leader => swap_stats(state, idx, |pl| pl.leader = name),
        CardType::Government => swap_stats(state, idx, |pl| pl.government = name),
        CardType::Wonder => swap_stats(state, idx, |pl| pl.completed_wonders.push(name)),
        _ => unreachable!("is_swap_type gates this to Leader/Government/Wonder"),
    };
    let mut out = Vec::new();
    delta_triples(&before, &after, p, &mut out);
    match typ {
        CardType::Wonder => {
            wonder_cost(name, &mut out);
            wonder_rider_delta(state, p, name, &mut out);
        }
        CardType::Leader => {
            if p.leader.is_none() {
                // the generic "it is a leader" term -- not a gain when you
                // already have one: a leader replaces a leader.
                out.push((Feature::Leader, 1.0, Kind::Gain));
            }
            rider_delta(state, p, name, &mut out);
        }
        CardType::Government => {
            government_level(p, name, &mut out);
            government_cost(state, p, name, &mut out);
        }
        _ => unreachable!(),
    }
    Some(merge(out))
}

// ------------------------------------- board-scaled action cards (additive)

/// `board_extra`: board-scaled triples to ADD to the static card-yield
/// table. Mirrors `engine/bots/board_yields.py::board_extra` exactly for the
/// two per-player-count action-card magnitudes it prices -- Endowment for
/// the Arts (`culturePerCivilizationWithMoreCulture`, a one-shot `culture`
/// stock gain) and Wave of Nationalism / Military Build-Up
/// (`resourcesForMilitaryUnitsPerStrongerCivilization`, ring-fenced to
/// military units, hence `RestrictedResources` rather than `ResourceStock`
/// -- see `apply.rs::h_play_action`'s use of `p.mil_discount` for why that
/// pool, not the general resource stock, is the honest price). Used to be a
/// KNOWN GAP (see this module's former top doc comment note, removed
/// 2026-08-05 once `gen_cards.py` gave both `Special` variants a real
/// `[i16; 3]` payload): both keys were detected but the coefficient could
/// not be recovered, so this always returned nothing.
pub fn board_extra(name: CardId, state: &GameState, idx: usize) -> Vec<Triple> {
    if name.is_none() {
        return Vec::new();
    }
    let card = name.get();
    let p = &state.players[idx];
    let count_idx = crate::events::live_count_idx(state);
    let mut out = Vec::new();
    if let Some(t) = card.special.iter().find_map(|&s| match s {
        Special::CulturePerCivilizationWithMoreCulture(t) => Some(t),
        _ => None,
    }) {
        let n = live_rivals(state, p).into_iter().filter(|q| q.culture > p.culture).count();
        if n > 0 {
            out.push((Feature::Culture, t[count_idx] as f64 * n as f64, Kind::Gain));
        }
    }
    if let Some(t) = card.special.iter().find_map(|&s| match s {
        Special::ResourcesForMilitaryUnitsPerStrongerCivilization(t) => Some(t),
        _ => None,
    }) {
        let mine = effects::state_stats(state, p).strength;
        let n = live_rivals(state, p)
            .into_iter()
            .filter(|q| effects::state_stats(state, q).strength > mine)
            .count();
        if n > 0 {
            out.push((Feature::RestrictedResources, t[count_idx] as f64 * n as f64, Kind::Gain));
        }
    }
    out
}

/// `board_choices`: mutually exclusive alternatives. Mirrors Python exactly:
/// nothing board-priced needs one yet, so this always returns empty; the
/// entry point exists so a future card can grow one without a new function
/// signature appearing out of nowhere.
pub fn board_choices(_name: CardId, _state: &GameState, _idx: usize) -> Vec<Vec<Triple>> {
    Vec::new()
}

// ------------------------------------------------------- unit technologies

/// `_upgradable_onto`: `[(tech, workers)]` this player could LEGALLY upgrade
/// onto `name` -- same type, strictly lower level, at least one worker
/// standing on it. Shared by [`unit_upgrade`] and [`tech_upgrade`]. Not
/// converted to an out-param like this file's `Triple`-producing helpers:
/// this returns `(CardId, u8)` pairs, not triples, and both callers use it
/// as a genuinely independent value -- held by a `let`, checked with
/// `is_empty()`, and iterated MORE THAN ONCE (once for the resource-cost
/// sum, once inside [`with_tech`]) rather than being folded once into a
/// growing accumulator. That is exactly the shape this module's other
/// helpers were changed away from and this one already has.
fn upgradable_onto(p: &PlayerState, name: CardId) -> Vec<(CardId, u8)> {
    let typ = name.kind();
    let lv = name.level();
    p.techs
        .iter()
        .filter(|(id, slot)| slot.workers > 0 && id.kind() == typ && id.level() < lv)
        .map(|(id, slot)| (id, slot.workers))
        .collect()
}

/// `_with_tech`: `effects::compute` with `name` developed and `moved`
/// workers moved onto it, off the technologies they are standing on. A clone
/// of the player, per this module's top doc comment -- no mutate/restore.
fn with_tech(state: &GameState, idx: usize, name: CardId, moved: &[(CardId, u8)]) -> Stats {
    let mut p = state.players[idx].clone();
    let mut total: u8 = 0;
    for &(from, k) in moved {
        if let Some(slot) = p.techs.get_mut(from) {
            slot.workers -= k;
        }
        total += k;
    }
    p.techs.insert(name, TechSlot { workers: total, stored: 0 });
    effects::compute(state, &p)
}

/// `unit_upgrade`: (strength gained, science cost, resource cost) for
/// "develop `name`, then move every worker that could LEGALLY upgrade onto
/// it". `(0.0, 0.0, 0.0)` for a non-unit-technology card, or one already
/// developed (dead in hand).
pub fn unit_upgrade(name: CardId, state: &GameState, idx: usize) -> (f64, f64, f64) {
    if name.is_none() || !name.kind().is_unit() {
        return (0.0, 0.0, 0.0);
    }
    let p = &state.players[idx];
    if p.techs.has(name) {
        return (0.0, 0.0, 0.0);
    }
    let sci = costs::tech_cost(state, p, name).unwrap_or(0);
    let held = upgradable_onto(p, name);
    if held.is_empty() {
        // Nobody eligible to move -- science cost only, honest rather than a
        // floor; `tech_upgrade`'s `build_fresh` sibling is the other plan.
        return (0.0, sci as f64, 0.0);
    }
    let before = effects::state_stats(state, p);
    let after = with_tech(state, idx, name, &held);
    let gained = (after.strength - before.strength) as f64;
    let res: i32 = held.iter().map(|&(lo, k)| k as i32 * costs::upgrade_cost(state, p, lo, name)).sum();
    (gained, sci as f64, res as f64)
}

// --------------------------------------------- EVERY technology, not just red

/// `tech_upgrade`: (staff triples, develop triples, science cost, resource
/// cost). See the Python module's extensive doc comment above this function
/// for the three things a static table cannot say (tech_levels, upgrade vs
/// fresh build, phase-weighted rates) -- none of that is re-derived here,
/// only the mechanics of computing it.
pub fn tech_upgrade(name: CardId, state: &GameState, idx: usize) -> (Vec<Triple>, Vec<Triple>, f64, f64) {
    if name.is_none() || !is_levelled_type(name.kind()) {
        return (Vec::new(), Vec::new(), 0.0, 0.0);
    }
    let p = &state.players[idx];
    if p.techs.has(name) {
        return (Vec::new(), Vec::new(), 0.0, 0.0);
    }
    let typ = name.kind();
    let lv = name.level();
    let mut dev = vec![(Feature::TechLevels, lv as f64, Kind::Gain), (Feature::NumTechs, 1.0, Kind::Gain)];
    if typ == CardType::SpecialTech {
        dev.push((Feature::SpecialTechs, 1.0, Kind::Gain));
    }
    if let Some(feat) = best_feature(typ) {
        // `best_unit` is the max over all four red types; every other
        // `best_*` is the max over its own type. NOT gated on `workers > 0`
        // -- Python iterates every key of `p.techs` regardless of staffing.
        let unit_family = typ.is_unit();
        let cur = p
            .techs
            .iter()
            .filter(|(id, _)| if unit_family { id.kind().is_unit() } else { id.kind() == typ })
            .map(|(id, _)| id.level())
            .max()
            .unwrap_or(0);
        if lv > cur {
            dev.push((feat, (lv - cur) as f64, Kind::Gain));
        }
    }

    let (staff, sci, res) = if typ.is_unit() {
        // The red half is `unit_upgrade`, called and not re-derived -- both
        // halves of this function must mean the same thing by "upgrade".
        let (gained, sci, res) = unit_upgrade(name, state, idx);
        let staff = if gained != 0.0 { vec![(Feature::Strength, gained, Kind::Gain)] } else { Vec::new() };
        (staff, sci, res)
    } else {
        let sci = costs::tech_cost(state, p, name).unwrap_or(0) as f64;
        let held = upgradable_onto(p, name);
        if held.is_empty() {
            (Vec::new(), sci, 0.0)
        } else {
            let before = effects::state_stats(state, p);
            let after = with_tech(state, idx, name, &held);
            let mut staff = Vec::new();
            delta_triples(&before, &after, p, &mut staff);
            let res: i32 = held.iter().map(|&(lo, k)| k as i32 * costs::upgrade_cost(state, p, lo, name)).sum();
            (staff, sci, res as f64)
        }
    };
    (staff, dev, sci, res)
}

// ------------------------------------------------- the OTHER staffing plan

/// `_build_triples`: the four features a fresh build moves that a `Stats`
/// diff cannot see -- `weighted.features` reads all four off the player
/// rather than off `Stats`. `uprising` is a THRESHOLD, recomputed the same
/// way `economy::uprising`/`features()` do, off the same two numbers. Pushes
/// into `out`, same convention as [`delta_triples`] beside it.
fn build_triples(p: &PlayerState, typ: CardType, before: &Stats, after: &Stats, workers: i32, out: &mut Vec<Triple>) {
    out.push((Feature::FreeWorkers, -(workers as f64), Kind::Gain));
    out.push((Feature::Workers, workers as f64, Kind::Gain));
    if let Some(cls) = worker_class(typ) {
        out.push((cls, workers as f64, Kind::Gain));
    }
    let req = economy::happy_required(p.yellow_bank) as i32;
    let was = if (req - before.happy).max(0) > p.workers_free as i32 { 1.0 } else { 0.0 };
    let now = if (req - after.happy).max(0) > p.workers_free as i32 - workers { 1.0 } else { 0.0 };
    if now != was {
        out.push((Feature::Uprising, now - was, Kind::Gain));
    }
}

/// `_with_built`: `effects::compute` with `name` developed and `workers`
/// fresh workers on it. `p.workers_free` is NOT touched here -- neither is
/// it in Python -- `effects::compute` does not read it; the free-worker side
/// of the trade is priced in [`build_triples`].
fn with_built(state: &GameState, idx: usize, name: CardId, workers: u8) -> Stats {
    let mut p = state.players[idx].clone();
    p.techs.insert(name, TechSlot { workers, stored: 0 });
    effects::compute(state, &p)
}

/// `build_fresh`: (triples, resource cost) for "develop `name`, then BUILD
/// one worker". `(vec![], 0.0)` whenever the engine would not offer the
/// build: no free worker, no build cost on the card, or an urban type
/// already at its `urban_limit`.
pub fn build_fresh(name: CardId, state: &GameState, idx: usize) -> (Vec<Triple>, f64) {
    if name.is_none() || !is_levelled_type(name.kind()) {
        return (Vec::new(), 0.0);
    }
    let p = &state.players[idx];
    if p.techs.has(name) || p.workers_free == 0 {
        return (Vec::new(), 0.0);
    }
    let Some(res) = costs::build_cost_net(state, p, name) else {
        return (Vec::new(), 0.0);
    };
    let typ = name.kind();
    let before = effects::state_stats(state, p);
    if typ.is_urban() {
        // An urban build is illegal once this TYPE (not this technology)
        // already stands at the urban limit.
        let have: i32 = p.techs.of_type(typ).map(|(_, s)| s.workers as i32).sum();
        if have >= before.urban_limit {
            return (Vec::new(), 0.0);
        }
    }
    let after = with_built(state, idx, name, 1);
    let mut out = Vec::new();
    delta_triples(&before, &after, p, &mut out);
    build_triples(p, typ, &before, &after, 1, &mut out);
    (merge(out), res as f64)
}
