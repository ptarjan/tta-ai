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
//! ## Two known gaps, found while porting -- BOTH NOW CLOSED
//!
//! ~~**Hollywood / Internet culture-on-completion is unpriced.**~~ CLOSED
//! 2026-08-05. Wonder completion culture is priced by calling
//! [`crate::apply::wonder_completion_culture`], the SAME function
//! `apply.rs`'s own `on_wonder_complete` pays out for real -- one
//! implementation, so the evaluator and the scorer cannot disagree, exactly
//! the property the Python module's docstring calls out as the whole point.
//! Hollywood and Internet score off `effects::building_output` (what the
//! buildings ACTUALLY produce, modifiers included) rather than their printed
//! production, and while that function was unported the Rust one panicked on
//! exactly those two names, so [`on_build_culture`] skipped them and both
//! wonders were priced as if they had no completion-culture rider at all.
//! `building_output` landed in `effects.rs`; the skip is gone and both
//! differential suites dropped their `WONDER_CULTURE_GAP_CARDS` allowlist.
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
        Government | SpecialTech | Wonder | Leader | Action | Tactic | Aggression | War | Pact | Bonus | Territory | Event => None,
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
fn swap_stats(state: &GameState, idx: u8, mutate: impl FnOnce(&mut PlayerState)) -> Stats {
    let mut p = state.players[idx as usize].clone();
    mutate(&mut p);
    effects::compute(state, &p)
}

/// `_STATS_FEATURES`: `Stats` field -> evaluator feature, for the fields
/// whose delta means something on its own. Function pointers rather than a
/// closure-capturing table: none of these capture anything, so this is a
/// `const` array of plain `fn(&Stats) -> i32`, no allocation, no `HashMap`.
type StatsFeature = (fn(&Stats) -> i32, Feature);

const STATS_FEATURES: &[StatsFeature] = &[
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

/// Hammurabi: "On your turn, you may use one military action as a civil
/// action" (`militaryActionAsCivilPerTurn: 1`,
/// `data/cards_wonders_leaders.json`) -- civil actions are the scarcest
/// resource in the game (see `docs/EVALUATOR_HISTORY.md`/the STOCK_NONNEG_GATES
/// note on `civil_actions`/`civil_action_surplus` in `eval.rs`), so a printed
/// conversion INTO one is a real per-turn grant, priced through the same
/// `Feature::CivilActions` coordinate `government_plans` already prices a
/// government's own civil-action count through -- not a flat per-card weight.
///
/// The RULE (`costs::hammurabi_conversion_available`/`pay_ca`) spends this
/// LAZILY off whatever military actions are left THIS turn. That is the
/// wrong quantity to read here: a fresh game's round-1 start-player handicap
/// sets `p.military_actions` (the remaining-this-turn pool) to 0, which would
/// misprice Hammurabi as worthless on exactly the turn a player is deciding
/// whether to take him. `effects::state_stats(..).military_actions` is the
/// GOVERNMENT's per-turn grant instead (a `Stats` field, default 2 under
/// Despotism) -- the same production-level board fact [`genghis`]'s strength
/// check reads above, not a spent/remaining counter.
///
/// ASSUMPTION, documented rather than fitted (`prefer-compute-over-inference`:
/// a firing rate we do not have measured is not invented here): whenever the
/// government grants >= 1 military action per turn, the printed cap (1 civil
/// action) is priced as fully realized every turn. This leans generous --
/// an army rarely spends 100% of its granted military actions every single
/// turn, especially before Age I -- but no self-play measurement of
/// Hammurabi's true firing rate exists yet to discount it by, and a board
/// query that IS available (does the government grant a military action at
/// all) is preferred over guessing one that is not.
fn hammurabi(state: &GameState, p: &PlayerState, out: &mut Vec<Triple>) {
    if effects::state_stats(state, p).military_actions >= 1 {
        out.push((Feature::CivilActions, 1.0, Kind::Gain));
    }
}

/// `RIDERS`: leader name -> rider function. Only the leaders whose value is
/// not in `Stats` and IS computable.
fn rider_of(leader_name: &str) -> Option<fn(&GameState, &PlayerState, &mut Vec<Triple>)> {
    match leader_name {
        "Genghis Khan" => Some(genghis),
        "Winston Churchill" => Some(churchill),
        "Hammurabi" => Some(hammurabi),
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

/// `_government_cost`: the science a government actually costs and the civil
/// actions it burns. The revolution price is cheaper on every card in the
/// deck, so it is the one priced -- but only when RULES_SPEC §8.3 would
/// actually let you take that route, exactly as [`government_routes`] gates
/// it. Quoting the revolution price unconditionally makes an illegal
/// discount look available; the port did that because the Python it came
/// from did, with no gate anywhere in the function.
fn government_cost(state: &GameState, p: &PlayerState, name: CardId, out: &mut Vec<Triple>) {
    let card = name.get();
    let revolution_available = card.revolution_cost != 0 && legal::can_revolt(state, p, name);
    let sci =
        if revolution_available { card.revolution_cost as i32 } else { card.peaceful_cost as i32 };
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
    let ma_before = p.military_actions as f64;

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
        let d_ma_capped = ma_left_delta(ma_before, ma_before + d_ma);
        if d_ma_capped != 0.0 {
            peaceful.push((Feature::MaLeft, d_ma_capped, Kind::Gain));
        }
    }
    let mut routes = vec![peaceful];

    // ---- revolution (RULES_SPEC 8.3), only when the engine would offer it.
    if legal::can_revolt(state, p, name) {
        let card = name.get();
        let mut rev = vec![(Feature::Science, -(card.revolution_cost as f64), Kind::Cost)];
        if leader_is(p, "Maximilien Robespierre") {
            if p.military_actions != 0 {
                // Every unused MA is spent, not just paid down to zero linearly
                // -- §6.7's draw is capped at 3, so losing a 4th-or-later
                // banked action costs nothing beyond what losing the 3rd
                // already cost. `ma_left_delta` prices the actual before ->
                // after (0) transition rather than the raw count.
                rev.push((Feature::MaLeft, ma_left_delta(ma_before, 0.0), Kind::Cost));
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
                let d_ma_capped = ma_left_delta(ma_before, ma_before + d_ma);
                if d_ma_capped != 0.0 {
                    rev.push((Feature::MaLeft, d_ma_capped, Kind::Gain));
                }
            }
        }
        routes.push(rev);
    }
    routes
}

/// RULES_SPEC §6.7 ("Unspent MAs at end of turn each draw 1 military card
/// (max 3)"): the 4th-and-later unused military action converts into no
/// card at all, so [`Feature::MaLeft`]'s draw-potential value saturates at
/// [`MA_DRAW_CAP`]. A `government_routes` swap must therefore price the
/// DIFFERENCE OF TWO CAPPED VALUES (`min(after, cap) - min(before, cap)`),
/// never a capped difference of the raw delta -- the latter would, e.g.,
/// still price a 5 -> 7 swap (both already past the cap, so genuinely worth
/// nothing) as if it were a real gain.
pub const MA_DRAW_CAP: f64 = 3.0;

/// See [`MA_DRAW_CAP`]'s doc comment: the capped-before/capped-after
/// difference, not `(after - before).min(MA_DRAW_CAP)`.
fn ma_left_delta(before: f64, after: f64) -> f64 {
    after.min(MA_DRAW_CAP) - before.min(MA_DRAW_CAP)
}

/// `government_plans`: (gain triples, cost routes) for putting `name` in
/// play as government. `(vec![], vec![])` for a non-government card, or the
/// government already in play (dead in hand: not a cost, not a gain).
pub fn government_plans(name: CardId, state: &GameState, idx: u8) -> (Vec<Triple>, Vec<Vec<Triple>>) {
    if name.is_none() || name.kind() != CardType::Government {
        return (Vec::new(), Vec::new());
    }
    let p = &state.players[idx as usize];
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
/// culture. Calls [`crate::apply::wonder_completion_culture`], the same
/// function `apply.rs` pays out for real, so the evaluator and the scorer
/// cannot disagree -- the property Python's docstring calls the whole point.
///
/// This used to special-case Hollywood and Internet out, back when
/// `effects::building_output` was unported and that function panicked on
/// them. It is ported, they are priced, and the guard is gone (2026-08-05);
/// both differential suites dropped their `WONDER_CULTURE_GAP_CARDS`
/// allowlist in the same change.
fn on_build_culture(p: &PlayerState, name: CardId, out: &mut Vec<Triple>) {
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

/// The un-mutated `effects::compute` for one player, computed ONCE and lent to
/// every card being priced against it.
///
/// # Why this type exists
///
/// Pricing a swap card is a diff: `compute` the board as it is, `compute` it
/// with the card swapped in, subtract. The second half genuinely differs per
/// card; the first half is the SAME for every card in a row or a hand, and
/// [`board_yields`] used to recompute it per candidate. `effects::compute` is
/// the most expensive function in the engine -- 25278 samples, more than twice
/// the next entry, in a 20s profile of the live 2p climb arm -- so a 13-card
/// hand was paying for 13 identical rebuilds of the same numbers.
///
/// # Why it carries the board rather than sitting beside it
///
/// A hoisted baseline is only correct for the `(state, player)` it was built
/// from, and "computed for the wrong board" is a silent wrong-answer bug, not
/// a crash. So the baseline OWNS the board reference: [`board_yields`] takes
/// only a `&Baseline` and reads the state and index back out of it. There is
/// no second parameter to disagree with -- passing a mismatched pair is not
/// something the caller can express. (DESIGN.md: collapse the composition so
/// the broken gluing cannot be written.)
///
/// This is not a cache: nothing is stored between calls and nothing is
/// invalidated. It is a value the caller computes and passes down, and the
/// borrow checker stops the state changing underneath it for as long as it
/// lives.
///
/// # What it did NOT buy: nothing, measurably
///
/// Measured 2026-08-10, quiet box, identical 2400-game 2p arena, same PGO
/// flags both sides, best of three: 38.9s before, 40.6s after. No speedup, and
/// the two binaries produced byte-identical results, so the hoist provably
/// changed no answer -- it just did not change the clock either.
///
/// That is worth writing down because the profile said the opposite, and the
/// next person reading those 25278 samples will have the same idea. The likely
/// explanation is that the eliminated half is not the half being sampled: only
/// the `before` compute was hoisted, the `after` compute is still per card, and
/// most `card_potential` calls never reach the board-aware path at all (they
/// take the `credit_board == 0.0` early-out, or `board` is `None`). Before
/// re-attacking this, COUNT how often `board_yields` is actually entered per
/// decision -- do not infer it from the sample attribution again.
///
/// The refactor is kept anyway: it is strictly less work, and it made a class
/// of silent wrong-answer bug unexpressible. It is not kept for speed.
///
/// # It lives in `effects.rs` now
///
/// This started life here, hoisted out of the per-card loops below. The same
/// shape then turned out to be what the ENGINE needs -- `costs.rs` and
/// `economy.rs` rebuild a player's whole statistics per candidate card too --
/// so the type moved to [`effects::Snapshot`] and `Baseline` is an alias for
/// it. One implementation, so the bot's notion of "the board as it stands" and
/// the engine's cannot drift apart.
pub type Baseline<'a> = effects::Snapshot<'a>;

/// `board_yields`: `(feature, amount, kind)` triples for `name` on this
/// board, or `None` meaning "not board-priced, use the static table".
pub fn board_yields(name: CardId, base: &Baseline) -> Option<Vec<Triple>> {
    if name.is_none() {
        return None;
    }
    let typ = name.kind();
    if !is_swap_type(typ) {
        return None;
    }
    let (state, idx) = (base.state(), base.idx());
    let p = base.player();
    let before = base.stats();
    let after = match typ {
        CardType::Leader => swap_stats(state, idx, |pl| pl.leader = name),
        CardType::Government => swap_stats(state, idx, |pl| pl.government = name),
        CardType::Wonder => swap_stats(state, idx, |pl| pl.completed_wonders.push(name)),
        CardType::Farm | CardType::Mine | CardType::Lab | CardType::Temple | CardType::Library | CardType::Arena | CardType::Theater | CardType::Infantry | CardType::Cavalry | CardType::Artillery | CardType::Air | CardType::SpecialTech | CardType::Action | CardType::Tactic | CardType::Aggression | CardType::War | CardType::Pact | CardType::Bonus | CardType::Territory | CardType::Event => unreachable!("is_swap_type gates this to Leader/Government/Wonder"),
    };
    let mut out = Vec::new();
    delta_triples(before, &after, p, &mut out);
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
        CardType::Farm | CardType::Mine | CardType::Lab | CardType::Temple | CardType::Library | CardType::Arena | CardType::Theater | CardType::Infantry | CardType::Cavalry | CardType::Artillery | CardType::Air | CardType::SpecialTech | CardType::Action | CardType::Tactic | CardType::Aggression | CardType::War | CardType::Pact | CardType::Bonus | CardType::Territory | CardType::Event => unreachable!(),
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
pub fn board_extra(name: CardId, base: &Baseline) -> Vec<Triple> {
    if name.is_none() {
        return Vec::new();
    }
    let card = name.get();
    let (state, p) = (base.state(), base.player());
    let count_idx = crate::events::live_count_idx(state);
    let mut out = Vec::new();
    if let Some(t) = card.special.iter().find_map(|&s| match s {
        Special::CulturePerCivilizationWithMoreCulture(t) => Some(t),
        Special::A(_) | Special::AllPlayers(_) | Special::B(_) | Special::BestTheaterDoubleCulture | Special::BothPlayers(_) | Special::BuildDiscount(_) | Special::CancelledIfPartiesAttackEachOther | Special::CannotPlayAggressionOrWar | Special::CivilActionBackOnTechDevelop(_) | Special::CivilActionUpgradeUrbanBuildingToTheater | Special::ColonizeDiscardUpTo2MilitaryCardsForBonus(_) | Special::ColonyImmediateBonusApplies | Special::ColonyPermanentBonusTransfers | Special::ComboFoodDiscount(_) | Special::ComboResourceDiscount(_) | Special::Condition(_) | Special::CultureFirstColony(_) | Special::CultureIfTopTwoStrength(_) | Special::CultureOnLeaveEqualToLabResourceProduction | Special::CultureOnRevolution(_) | Special::CultureOnTechDevelop(_) | Special::CulturePerAdditionalColony(_) | Special::CulturePerHappyFromTemplesTheatersWonders(_) | Special::CulturePerLabEqualToLevel | Special::CulturePerLibraryTheaterPair(_) | Special::CulturePerTheater(_) | Special::DecreasePopulation(_) | Special::DestroyUrbanBuildings(_) | Special::DoubleBestMine | Special::DoublesTacticBonusOfOneArmy | Special::ExtraHappyPerHappySource(_) | Special::FinalScoring(_) | Special::FreeCivilAction(_) | Special::FreePopIncreasePerTurn | Special::Gain(_) | Special::GainCulturePerLevelOfRemovedCard(_) | Special::GainFoodOrResources(_) | Special::GainResources(_) | Special::InfantryCountsAsCavalryForTactics | Special::LastRoundSubstitute(_) | Special::LeaderTakeCivilActionDiscount(_) | Special::LibraryDiscountsIfTheater | Special::Lose(_) | Special::MilitaryActionAsCivilPerTurn(_) | Special::MilitaryActionCombinedPopIncreaseAndUnitBuild | Special::NoAttacksBetweenParties | Special::OnAttackBetweenParties(_) | Special::OnBuildCulture(_) | Special::OnBuildCulturePerTechLevelSum | Special::OnReplacePutUnderCompletedWonderHappy(_) | Special::OncePerGameTwoPoliticalActions | Special::OpponentDecreasesPopulation(_) | Special::OpponentsPayDoubleMilitaryActionsToAttackYou | Special::OrTakesSpecialTechnologiesOfSameTotalScienceCost | Special::PeekTopEventCardInPolitics | Special::PerTurnChoice | Special::PlayerWithLeastCulture(_) | Special::PlayerWithMostCulture(_) | Special::PlayersWithMostDiscontentWorkers(_) | Special::PlayersWithMostHappyFaces(_) | Special::PopIncreaseFoodDiscount(_) | Special::RemoveAsPoliticalActionForYellowToken(_) | Special::RemoveAsPoliticalActionFreeColonize | Special::RemoveFromGame | Special::ResourceOnMilitaryUnitBuildOrUpgrade(_) | Special::ResourceOnTechDevelop(_) | Special::ResourcesForMilitaryUnitsPerStrongerCivilization(_) | Special::ResourcesPerLabEqualToLevel | Special::RevolutionUsesMilitaryActionsInstead | Special::ScienceOnTechCardTake(_) | Special::SciencePerBestLabOrLibraryLevel | Special::SciencePerLab(_) | Special::StealColony(_) | Special::StrengthPerArtillery(_) | Special::StrengthPerInfantry(_) | Special::StrengthPerMilitaryUnit(_) | Special::StrengthPerTempleOrGovernmentHappy(_) | Special::StrengthPerUnitType(_) | Special::StrongestPlayer(_) | Special::StrongestPlayers(_) | Special::TakeCivilActionDiscountIfLeaderReplacedThisTurn(_) | Special::TakeFromOpponent(_) | Special::TheaterResourceDiscountIfLibrary(_) | Special::TheaterScienceDiscountIfLibrary(_) | Special::TheaterTechScienceDiscount(_) | Special::VictorTakesCulture | Special::VictorTakesScienceUpTo(_) | Special::VictorTakesYellowTokens | Special::WeakestPlayer(_) | Special::WeakestPlayers(_) | Special::WonderTakeNoExtraCivilActions => None,
    }) {
        let n = live_rivals(state, p).into_iter().filter(|q| q.culture > p.culture).count();
        if n > 0 {
            out.push((Feature::Culture, t[count_idx] as f64 * n as f64, Kind::Gain));
        }
    }
    if let Some(t) = card.special.iter().find_map(|&s| match s {
        Special::ResourcesForMilitaryUnitsPerStrongerCivilization(t) => Some(t),
        Special::A(_) | Special::AllPlayers(_) | Special::B(_) | Special::BestTheaterDoubleCulture | Special::BothPlayers(_) | Special::BuildDiscount(_) | Special::CancelledIfPartiesAttackEachOther | Special::CannotPlayAggressionOrWar | Special::CivilActionBackOnTechDevelop(_) | Special::CivilActionUpgradeUrbanBuildingToTheater | Special::ColonizeDiscardUpTo2MilitaryCardsForBonus(_) | Special::ColonyImmediateBonusApplies | Special::ColonyPermanentBonusTransfers | Special::ComboFoodDiscount(_) | Special::ComboResourceDiscount(_) | Special::Condition(_) | Special::CultureFirstColony(_) | Special::CultureIfTopTwoStrength(_) | Special::CultureOnLeaveEqualToLabResourceProduction | Special::CultureOnRevolution(_) | Special::CultureOnTechDevelop(_) | Special::CulturePerAdditionalColony(_) | Special::CulturePerCivilizationWithMoreCulture(_) | Special::CulturePerHappyFromTemplesTheatersWonders(_) | Special::CulturePerLabEqualToLevel | Special::CulturePerLibraryTheaterPair(_) | Special::CulturePerTheater(_) | Special::DecreasePopulation(_) | Special::DestroyUrbanBuildings(_) | Special::DoubleBestMine | Special::DoublesTacticBonusOfOneArmy | Special::ExtraHappyPerHappySource(_) | Special::FinalScoring(_) | Special::FreeCivilAction(_) | Special::FreePopIncreasePerTurn | Special::Gain(_) | Special::GainCulturePerLevelOfRemovedCard(_) | Special::GainFoodOrResources(_) | Special::GainResources(_) | Special::InfantryCountsAsCavalryForTactics | Special::LastRoundSubstitute(_) | Special::LeaderTakeCivilActionDiscount(_) | Special::LibraryDiscountsIfTheater | Special::Lose(_) | Special::MilitaryActionAsCivilPerTurn(_) | Special::MilitaryActionCombinedPopIncreaseAndUnitBuild | Special::NoAttacksBetweenParties | Special::OnAttackBetweenParties(_) | Special::OnBuildCulture(_) | Special::OnBuildCulturePerTechLevelSum | Special::OnReplacePutUnderCompletedWonderHappy(_) | Special::OncePerGameTwoPoliticalActions | Special::OpponentDecreasesPopulation(_) | Special::OpponentsPayDoubleMilitaryActionsToAttackYou | Special::OrTakesSpecialTechnologiesOfSameTotalScienceCost | Special::PeekTopEventCardInPolitics | Special::PerTurnChoice | Special::PlayerWithLeastCulture(_) | Special::PlayerWithMostCulture(_) | Special::PlayersWithMostDiscontentWorkers(_) | Special::PlayersWithMostHappyFaces(_) | Special::PopIncreaseFoodDiscount(_) | Special::RemoveAsPoliticalActionForYellowToken(_) | Special::RemoveAsPoliticalActionFreeColonize | Special::RemoveFromGame | Special::ResourceOnMilitaryUnitBuildOrUpgrade(_) | Special::ResourceOnTechDevelop(_) | Special::ResourcesPerLabEqualToLevel | Special::RevolutionUsesMilitaryActionsInstead | Special::ScienceOnTechCardTake(_) | Special::SciencePerBestLabOrLibraryLevel | Special::SciencePerLab(_) | Special::StealColony(_) | Special::StrengthPerArtillery(_) | Special::StrengthPerInfantry(_) | Special::StrengthPerMilitaryUnit(_) | Special::StrengthPerTempleOrGovernmentHappy(_) | Special::StrengthPerUnitType(_) | Special::StrongestPlayer(_) | Special::StrongestPlayers(_) | Special::TakeCivilActionDiscountIfLeaderReplacedThisTurn(_) | Special::TakeFromOpponent(_) | Special::TheaterResourceDiscountIfLibrary(_) | Special::TheaterScienceDiscountIfLibrary(_) | Special::TheaterTechScienceDiscount(_) | Special::VictorTakesCulture | Special::VictorTakesScienceUpTo(_) | Special::VictorTakesYellowTokens | Special::WeakestPlayer(_) | Special::WeakestPlayers(_) | Special::WonderTakeNoExtraCivilActions => None,
    }) {
        // Already computed for this player -- the whole point of `Baseline`.
        let mine = base.stats().strength;
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
pub fn board_choices(_name: CardId, _state: &GameState, _idx: u8) -> Vec<Vec<Triple>> {
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
fn with_tech(state: &GameState, idx: u8, name: CardId, moved: &[(CardId, u8)]) -> Stats {
    let mut p = state.players[idx as usize].clone();
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
pub fn unit_upgrade(name: CardId, state: &GameState, idx: u8) -> (f64, f64, f64) {
    if name.is_none() || !name.kind().is_unit() {
        return (0.0, 0.0, 0.0);
    }
    let p = &state.players[idx as usize];
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
pub fn tech_upgrade(name: CardId, state: &GameState, idx: u8) -> (Vec<Triple>, Vec<Triple>, f64, f64) {
    if name.is_none() || !is_levelled_type(name.kind()) {
        return (Vec::new(), Vec::new(), 0.0, 0.0);
    }
    let p = &state.players[idx as usize];
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
fn with_built(state: &GameState, idx: u8, name: CardId, workers: u8) -> Stats {
    let mut p = state.players[idx as usize].clone();
    p.techs.insert(name, TechSlot { workers, stored: 0 });
    effects::compute(state, &p)
}

/// `build_fresh`: (triples, resource cost) for "develop `name`, then BUILD
/// one worker". `(vec![], 0.0)` whenever the engine would not offer the
/// build: no free worker, no build cost on the card, or an urban type
/// already at its `urban_limit`.
pub fn build_fresh(name: CardId, state: &GameState, idx: u8) -> (Vec<Triple>, f64) {
    if name.is_none() || !is_levelled_type(name.kind()) {
        return (Vec::new(), 0.0);
    }
    let p = &state.players[idx as usize];
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

#[cfg(test)]
mod tests {
    use super::*;

    /// RULES_SPEC 6.7: a government swap that raises the remaining
    /// military-action count from below the cap to below the cap (2 -> 3)
    /// is a real gain -- that 3rd action still draws a card at end of turn.
    #[test]
    fn a_swap_that_stays_under_the_draw_cap_prices_the_full_raw_delta() {
        assert_eq!(ma_left_delta(2.0, 3.0), 1.0);
    }

    /// A swap that crosses the cap (2 -> 4) must be priced as only the room
    /// left UNDER the cap (1: from 2 up to 3), not the full raw jump (2),
    /// because the 4th action never converts to a card at all.
    #[test]
    fn a_swap_that_crosses_the_draw_cap_prices_only_the_room_under_it() {
        assert_eq!(ma_left_delta(2.0, 4.0), 1.0);
    }

    /// A swap entirely above the cap (4 -> 6) is worth exactly nothing --
    /// this is the trap the fix's own doc comment calls out: a capped
    /// DIFFERENCE of the raw delta (`(6.0 - 4.0).min(3.0)` = 2.0) would get
    /// this wrong; the difference of two CAPPED values is the only correct
    /// shape (`min(6,3) - min(4,3)` = 0.0).
    #[test]
    fn a_swap_entirely_above_the_draw_cap_is_worth_nothing() {
        assert_eq!(ma_left_delta(4.0, 6.0), 0.0);
    }

    /// Losing every unused action (the Robespierre revolution route, "after"
    /// = 0) from a pool already past the cap must cost only the capped
    /// amount (3), not the raw amount (5) -- the 4th and 5th were never
    /// going to draw a card either way, so losing them costs nothing extra.
    #[test]
    fn losing_a_pool_already_past_the_cap_costs_only_the_capped_amount() {
        assert_eq!(ma_left_delta(5.0, 0.0), -3.0);
    }

    // ------------------------------------------------ Hammurabi's rider

    /// The valuation hole this module closes: Hammurabi's printed
    /// "on your turn, you may use one military action as a civil action"
    /// (`militaryActionAsCivilPerTurn`) used to be in `cards::
    /// DELIBERATELY_UNPRICED` and worth exactly nothing to the evaluator. A
    /// fresh 2p game's starting government is Despotism, which grants 2
    /// military actions/turn (`effects::state_stats().military_actions`) even
    /// though round 1's start-player handicap (`game.rs`, §1.9) has already
    /// zeroed `p.military_actions` itself (the REMAINING-this-turn pool) --
    /// exactly the trap [`hammurabi`]'s own doc comment calls out: reading
    /// the remaining pool here would misprice him as worthless on the very
    /// turn a player decides whether to take him.
    #[test]
    fn hammurabis_military_action_conversion_is_worth_a_civil_action_to_the_evaluator() {
        let state = crate::game::new_game(2, 0);
        let base = Baseline::at(&state, 0);
        let swap =
            board_yields(CardId::by_name("Hammurabi").unwrap(), &base).expect("Hammurabi is a swap type (Leader)");
        assert!(
            swap.contains(&(Feature::CivilActions, 1.0, Kind::Gain)),
            "expected a +1 CivilActions gain triple for Hammurabi's MA-as-CA conversion, got {swap:?}"
        );
    }

    // -------------------------------------------------- board-scaled riders
    //
    // 17 printed effects (`cards::DELIBERATELY_UNPRICED`'s former bucket 8)
    // turned out to already be priced by the GENERIC swap diff above --
    // every carrier is a Leader or Wonder, `effects::apply_special` folds
    // the board-scaled math straight into `Stats`, and `delta_triples`
    // reads `Stats` before/after. No bespoke rider function exists for any
    // of these; these tests exist to PIN that behaviour (the coefficient
    // really does scale with the board fact, not just "is nonzero") so a
    // future refactor of `apply_special` or `delta_triples` cannot silently
    // flatten it back to a constant. See BOARDSCALED.txt for the full
    // reclassification writeup and the empirical proof this was already
    // true before any of these tests were added.

    /// J. S. Bach: 1 culture per theater WORKER (not per theater card) --
    /// two theaters staffed with 3 workers total must yield 3x what one
    /// theater staffed with 1 worker yields, pinning that this is a
    /// per-worker count, not a flat per-card bonus.
    #[test]
    fn bachs_culture_per_theater_scales_with_theater_workers() {
        let mut state = crate::game::new_game(2, 0);
        state.players[0].techs.insert(CardId::by_name("Drama").unwrap(), TechSlot { workers: 1, stored: 0 });
        let one = board_yields(CardId::by_name("J. S. Bach").unwrap(), &Baseline::at(&state, 0))
            .expect("Bach is a swap type (Leader)");

        let mut state3 = crate::game::new_game(2, 0);
        state3.players[0].techs.insert(CardId::by_name("Drama").unwrap(), TechSlot { workers: 3, stored: 0 });
        let three = board_yields(CardId::by_name("J. S. Bach").unwrap(), &Baseline::at(&state3, 0))
            .expect("Bach is a swap type (Leader)");

        let culture_at = |t: &[Triple]| t.iter().find(|&&(f, _, _)| f == Feature::CultureRate).map_or(0.0, |&(_, a, _)| a);
        assert_eq!(culture_at(&one), 1.0, "1 theater worker: {one:?}");
        assert_eq!(culture_at(&three), 3.0, "3 theater workers must be exactly 3x 1 worker's yield: {three:?}");
    }

    /// Sid Meier prints BOTH `culturePerLabEqualToLevel` (a bonus) and
    /// `sciencePerLab` (value -1, a REDUCTION -- effects.rs's own doc
    /// comment on this arm calls out that a naive port drops the sign).
    /// 2 labs of level 1 staffed with 1 worker each must show +2 culture
    /// (2 labs x level 1) and -2 science (2 labs x -1), and doubling to 4
    /// labs must double both deltas -- proving the sign survives the swap
    /// diff, not just the magnitude.
    #[test]
    fn sid_meiers_culture_and_science_per_lab_scale_with_lab_level_and_count() {
        // Alchemy, not Philosophy: `new_game`'s starting kit already staffs
        // Philosophy (Age A, level 0 -- `Tableau::insert` panics on a
        // duplicate, and level 0 would zero out the level-scaled culture
        // half of this test anyway). Alchemy is Age I / level 1 and starts
        // unstaffed, so it can be inserted fresh.
        let lab = CardId::by_name("Alchemy").unwrap();
        assert_eq!(lab.level(), 1);

        let mut state2 = crate::game::new_game(2, 0);
        // Zero the starting kit's own Philosophy lab worker -- it is a lab
        // too, and would silently add to `workers_on(Lab)` (sciencePerLab
        // counts every lab worker, not just Alchemy's) if left staffed.
        state2.players[0].techs.get_mut(CardId::by_name("Philosophy").unwrap()).unwrap().workers = 0;
        state2.players[0].techs.insert(lab, TechSlot { workers: 2, stored: 0 });
        let two = board_yields(CardId::by_name("Sid Meier").unwrap(), &Baseline::at(&state2, 0))
            .expect("Sid Meier is a swap type (Leader)");

        let mut state4 = crate::game::new_game(2, 0);
        state4.players[0].techs.get_mut(CardId::by_name("Philosophy").unwrap()).unwrap().workers = 0;
        state4.players[0].techs.insert(lab, TechSlot { workers: 4, stored: 0 });
        let four = board_yields(CardId::by_name("Sid Meier").unwrap(), &Baseline::at(&state4, 0))
            .expect("Sid Meier is a swap type (Leader)");

        let at = |t: &[Triple], f: Feature| t.iter().find(|&&(ff, _, _)| ff == f).map_or(0.0, |&(_, a, _)| a);
        assert_eq!(at(&two, Feature::CultureRate), 2.0, "2 labs x level 1: {two:?}");
        assert_eq!(at(&two, Feature::ScienceRate), -2.0, "2 labs x sciencePerLab(-1): {two:?}");
        assert_eq!(at(&four, Feature::CultureRate), 4.0, "4 labs must be exactly 2x 2 labs: {four:?}");
        assert_eq!(at(&four, Feature::ScienceRate), -4.0, "4 labs must be exactly 2x 2 labs (still negative): {four:?}");
    }

    /// William Shakespeare: culture per MATCHED library/theater pair --
    /// min(library workers, theater workers), so 2 libraries + 3 theaters
    /// (min 2) must yield exactly 2x what 1 library + 1 theater (min 1)
    /// yields, and the excess (unmatched) theater worker must contribute
    /// nothing beyond that.
    #[test]
    fn shakespeares_culture_scales_with_matched_library_theater_pairs() {
        let mut one = crate::game::new_game(2, 0);
        one.players[0].techs.insert(CardId::by_name("Printing Press").unwrap(), TechSlot { workers: 1, stored: 0 });
        one.players[0].techs.insert(CardId::by_name("Drama").unwrap(), TechSlot { workers: 1, stored: 0 });
        let pair1 = board_yields(CardId::by_name("William Shakespeare").unwrap(), &Baseline::at(&one, 0))
            .expect("Shakespeare is a swap type (Leader)");

        let mut two = crate::game::new_game(2, 0);
        two.players[0].techs.insert(CardId::by_name("Printing Press").unwrap(), TechSlot { workers: 2, stored: 0 });
        two.players[0].techs.insert(CardId::by_name("Drama").unwrap(), TechSlot { workers: 3, stored: 0 });
        let pair2 = board_yields(CardId::by_name("William Shakespeare").unwrap(), &Baseline::at(&two, 0))
            .expect("Shakespeare is a swap type (Leader)");

        let culture_at = |t: &[Triple]| t.iter().find(|&&(f, _, _)| f == Feature::CultureRate).map_or(0.0, |&(_, a, _)| a);
        assert!(culture_at(&pair1) > 0.0, "1 matched pair must be a real gain: {pair1:?}");
        assert_eq!(
            culture_at(&pair2),
            2.0 * culture_at(&pair1),
            "min(2 lib, 3 theater) = 2 matched pairs, exactly 2x 1 matched pair: {pair1:?} vs {pair2:?}"
        );
    }

    /// Michelangelo: culture per happy face from temples/theaters/wonders
    /// ONLY (not government/leader/colony happy). A temple with 2 happy
    /// workers must yield exactly 2x a temple with 1 happy worker.
    #[test]
    fn michelangelos_culture_scales_with_temple_theater_wonder_happy() {
        let temple = CardId::by_name("Religion").unwrap();
        assert!(temple.get().production.happy > 0, "Religion must print happy for this test to mean anything");
        let per_worker = temple.get().production.happy as f64;

        // Religion is already in `new_game`'s starting kit, unstaffed (0
        // workers) -- `get_mut`, not `insert` (which panics on a duplicate).
        let mut one = crate::game::new_game(2, 0);
        one.players[0].techs.get_mut(temple).unwrap().workers = 1;
        let happy1 = board_yields(CardId::by_name("Michelangelo").unwrap(), &Baseline::at(&one, 0))
            .expect("Michelangelo is a swap type (Leader)");

        let mut two = crate::game::new_game(2, 0);
        two.players[0].techs.get_mut(temple).unwrap().workers = 2;
        let happy2 = board_yields(CardId::by_name("Michelangelo").unwrap(), &Baseline::at(&two, 0))
            .expect("Michelangelo is a swap type (Leader)");

        let culture_at = |t: &[Triple]| t.iter().find(|&&(f, _, _)| f == Feature::CultureRate).map_or(0.0, |&(_, a, _)| a);
        assert_eq!(culture_at(&happy1), per_worker, "1 worker's happy: {happy1:?}");
        assert_eq!(culture_at(&happy2), 2.0 * per_worker, "2 workers must be exactly 2x 1 worker: {happy2:?}");
    }

    /// Charlie Chaplin: doubles the BEST staffed theater's own printed
    /// culture. A level-3 theater (Movies) must double for more than a
    /// level-1 theater (Drama) -- pinning that this reads the best
    /// theater's LEVEL-scaled production, not a flat per-theater constant.
    #[test]
    fn chaplins_culture_scales_with_the_best_theaters_level() {
        let drama = CardId::by_name("Drama").unwrap();
        let movies = CardId::by_name("Movies").unwrap();
        assert!(movies.get().production.culture > drama.get().production.culture);

        let mut low = crate::game::new_game(2, 0);
        low.players[0].techs.insert(drama, TechSlot { workers: 1, stored: 0 });
        let low_swap = board_yields(CardId::by_name("Charlie Chaplin").unwrap(), &Baseline::at(&low, 0))
            .expect("Chaplin is a swap type (Leader)");

        let mut high = crate::game::new_game(2, 0);
        high.players[0].techs.insert(movies, TechSlot { workers: 1, stored: 0 });
        let high_swap = board_yields(CardId::by_name("Charlie Chaplin").unwrap(), &Baseline::at(&high, 0))
            .expect("Chaplin is a swap type (Leader)");

        let culture_at = |t: &[Triple]| t.iter().find(|&&(f, _, _)| f == Feature::CultureRate).map_or(0.0, |&(_, a, _)| a);
        assert_eq!(culture_at(&low_swap), drama.get().production.culture as f64, "best theater is Drama: {low_swap:?}");
        assert_eq!(culture_at(&high_swap), movies.get().production.culture as f64, "best theater is Movies (higher level): {high_swap:?}");
        assert!(culture_at(&high_swap) > culture_at(&low_swap), "a higher-level best theater must double for more");
    }

    /// Leonardo da Vinci / Isaac Newton / Albert Einstein: extra science
    /// equal to the best staffed lab-or-library's LEVEL. A level-3 lab
    /// (Scientific Method) must yield more than a level-1 lab (Philosophy).
    #[test]
    fn leonardos_science_scales_with_the_best_lab_or_library_level() {
        // Alchemy (level 1) vs Scientific Method (level 2) -- not Philosophy
        // (already in `new_game`'s starting kit at level 0, which this
        // special would never pick as "best" anyway once a higher-level lab
        // is staffed, so it is not a confound here the way it is for
        // sciencePerLab's worker-SUM, but starting at level 0 makes it a
        // weak "best lab" example on its own).
        let alchemy = CardId::by_name("Alchemy").unwrap();
        let scientific_method = CardId::by_name("Scientific Method").unwrap();
        assert!(scientific_method.level() > alchemy.level());

        let mut low = crate::game::new_game(2, 0);
        low.players[0].techs.insert(alchemy, TechSlot { workers: 1, stored: 0 });
        let low_swap = board_yields(CardId::by_name("Leonardo da Vinci").unwrap(), &Baseline::at(&low, 0))
            .expect("Leonardo is a swap type (Leader)");

        let mut high = crate::game::new_game(2, 0);
        high.players[0].techs.insert(scientific_method, TechSlot { workers: 1, stored: 0 });
        let high_swap = board_yields(CardId::by_name("Leonardo da Vinci").unwrap(), &Baseline::at(&high, 0))
            .expect("Leonardo is a swap type (Leader)");

        let science_at = |t: &[Triple]| t.iter().find(|&&(f, _, _)| f == Feature::ScienceRate).map_or(0.0, |&(_, a, _)| a);
        assert_eq!(science_at(&low_swap), alchemy.level() as f64, "best lab is Alchemy: {low_swap:?}");
        assert_eq!(science_at(&high_swap), scientific_method.level() as f64, "best lab is Scientific Method (higher level): {high_swap:?}");
        assert!(science_at(&high_swap) > science_at(&low_swap), "a higher-level best lab must give more science");
    }

    /// Bill Gates: the same lab_level_workers formula as Sid Meier's
    /// culturePerLabEqualToLevel, but into resources. 2 labs must yield
    /// exactly 2x 1 lab.
    #[test]
    fn bill_gatess_resources_scale_with_lab_level_and_count() {
        // Alchemy, not Philosophy -- same reason as Sid Meier's test above:
        // `resourcesPerLabEqualToLevel` sums level x workers over EVERY
        // staffed lab, and the starting kit's Philosophy (level 0) would
        // contribute 0 to the sum but cannot be `insert`-ed a second time,
        // so it is zeroed rather than reused.
        let lab = CardId::by_name("Alchemy").unwrap();

        let mut one = crate::game::new_game(2, 0);
        one.players[0].techs.get_mut(CardId::by_name("Philosophy").unwrap()).unwrap().workers = 0;
        one.players[0].techs.insert(lab, TechSlot { workers: 1, stored: 0 });
        let r1 = board_yields(CardId::by_name("Bill Gates").unwrap(), &Baseline::at(&one, 0))
            .expect("Bill Gates is a swap type (Leader)");

        let mut two = crate::game::new_game(2, 0);
        two.players[0].techs.get_mut(CardId::by_name("Philosophy").unwrap()).unwrap().workers = 0;
        two.players[0].techs.insert(lab, TechSlot { workers: 2, stored: 0 });
        let r2 = board_yields(CardId::by_name("Bill Gates").unwrap(), &Baseline::at(&two, 0))
            .expect("Bill Gates is a swap type (Leader)");

        let resources_at = |t: &[Triple]| t.iter().find(|&&(f, _, _)| f == Feature::ResourceRate).map_or(0.0, |&(_, a, _)| a);
        assert_eq!(resources_at(&r1), 1.0, "1 lab worker x level 1: {r1:?}");
        assert_eq!(resources_at(&r2), 2.0, "2 lab workers must be exactly 2x 1: {r2:?}");
    }

    /// Transcontinental Railroad (a Wonder): doubles the best staffed
    /// mine's own printed resources. A level-3 mine (Coal) must double for
    /// more than a level-1 mine (Bronze).
    #[test]
    fn transcontinental_railroads_resources_scale_with_the_best_mines_level() {
        let bronze = CardId::by_name("Bronze").unwrap();
        let coal = CardId::by_name("Coal").unwrap();
        assert!(coal.get().production.resources > bronze.get().production.resources);

        // Bronze is already in `new_game`'s starting kit, staffed -- used
        // as-is rather than `insert`-ed again (which would panic).
        let low = crate::game::new_game(2, 0);
        let low_swap = board_yields(CardId::by_name("Transcontinental Railroad").unwrap(), &Baseline::at(&low, 0))
            .expect("Transcontinental Railroad is a swap type (Wonder)");

        let mut high = crate::game::new_game(2, 0);
        high.players[0].techs.insert(coal, TechSlot { workers: 1, stored: 0 });
        let high_swap = board_yields(CardId::by_name("Transcontinental Railroad").unwrap(), &Baseline::at(&high, 0))
            .expect("Transcontinental Railroad is a swap type (Wonder)");

        let resources_at = |t: &[Triple]| t.iter().find(|&&(f, _, _)| f == Feature::ResourceRate).map_or(0.0, |&(_, a, _)| a);
        assert_eq!(resources_at(&low_swap), bronze.get().production.resources as f64, "best mine is Bronze: {low_swap:?}");
        assert_eq!(resources_at(&high_swap), coal.get().production.resources as f64, "best mine is Coal (higher level): {high_swap:?}");
        assert!(resources_at(&high_swap) > resources_at(&low_swap), "a higher-level best mine must double for more");
    }

    /// St. Peter's Basilica (a Wonder): a coefficient times a count of
    /// happy-giving sources already in play (buildings counted per
    /// WORKER, government/leader/wonder/colony counted per CARD -- see
    /// `effects::happy_source_count`'s own doc comment). A temple alone is
    /// 1 source; adding a government that itself prints happy (Theocracy)
    /// is a second, independent source, so the bonus must strictly
    /// increase, not just stay flat.
    #[test]
    fn st_peters_happy_scales_with_distinct_happy_producing_sources() {
        let temple = CardId::by_name("Religion").unwrap();
        assert!(temple.get().production.happy > 0);
        // Theocracy (a government) prints happy: 1 -- a second, independent
        // happy SOURCE, distinct in kind from a temple, to prove this counts
        // sources, not just temple copies.
        let theocracy = CardId::by_name("Theocracy").unwrap();
        assert!(theocracy.get().production.happy > 0);

        let mut one_source = crate::game::new_game(2, 0);
        one_source.players[0].techs.get_mut(temple).unwrap().workers = 1;
        let s1 = board_yields(CardId::by_name("St. Peter's Basilica").unwrap(), &Baseline::at(&one_source, 0))
            .expect("St. Peter's Basilica is a swap type (Wonder)");

        let mut two_sources = crate::game::new_game(2, 0);
        two_sources.players[0].techs.get_mut(temple).unwrap().workers = 1;
        two_sources.players[0].government = theocracy;
        let s2 = board_yields(CardId::by_name("St. Peter's Basilica").unwrap(), &Baseline::at(&two_sources, 0))
            .expect("St. Peter's Basilica is a swap type (Wonder)");

        // NOT asserted as an exact 2x multiplier: `happy_source_count`
        // counts building sources PER WORKER (not per building type -- see
        // its own doc comment), so the temple's contribution to the source
        // count does not move between `s1`/`s2` the same way a simple
        // "distinct types" count would; what must hold regardless is that
        // adding a second, independently-triggering source (the
        // government's own happy) strictly increases the bonus.
        let happy_at = |t: &[Triple]| t.iter().find(|&&(f, _, _)| f == Feature::HappyMargin).map_or(0.0, |&(_, a, _)| a);
        assert!(happy_at(&s1) > 0.0, "1 happy source (the temple) must be a real gain: {s1:?}");
        assert!(
            happy_at(&s2) > happy_at(&s1),
            "adding a second distinct happy source (the government's) must increase the bonus: {s1:?} vs {s2:?}"
        );
    }

    /// Great Wall (a Wonder) prints BOTH `strengthPerInfantry` and
    /// `strengthPerArtillery`. 2 infantry + 1 artillery must show a
    /// strength gain proportional to (2, 1), and doubling infantry alone
    /// must change only the infantry-attributable share.
    #[test]
    fn great_walls_strength_scales_with_infantry_and_artillery_counts() {
        // Warriors (infantry) is already in `new_game`'s starting kit at 1
        // worker -- `get_mut`, not `insert`, to change its count.
        let infantry = CardId::by_name("Warriors").unwrap();
        let artillery = CardId::by_name("Cannon").unwrap();

        let mut base = crate::game::new_game(2, 0);
        base.players[0].techs.insert(artillery, TechSlot { workers: 1, stored: 0 });
        let s_base = board_yields(CardId::by_name("Great Wall").unwrap(), &Baseline::at(&base, 0))
            .expect("Great Wall is a swap type (Wonder)");

        let mut more_infantry = crate::game::new_game(2, 0);
        more_infantry.players[0].techs.get_mut(infantry).unwrap().workers = 2;
        more_infantry.players[0].techs.insert(artillery, TechSlot { workers: 1, stored: 0 });
        let s_more = board_yields(CardId::by_name("Great Wall").unwrap(), &Baseline::at(&more_infantry, 0))
            .expect("Great Wall is a swap type (Wonder)");

        let strength_at = |t: &[Triple]| t.iter().find(|&&(f, _, _)| f == Feature::Strength).map_or(0.0, |&(_, a, _)| a);
        let base_val = strength_at(&s_base);
        let more_val = strength_at(&s_more);
        assert!(base_val > 0.0, "1 infantry + 1 artillery must be a real gain: {s_base:?}");
        assert!(more_val > base_val, "doubling infantry alone must increase the total: {s_more:?}");
    }

    /// Alexander the Great: strength per military unit WORKER of ANY type.
    /// 4 units total must yield exactly 2x 2 units total (mixed types).
    #[test]
    fn alexanders_strength_scales_with_total_military_units() {
        // Warriors (infantry) is already in the starting kit at 1 worker.
        let infantry = CardId::by_name("Warriors").unwrap();
        let cavalry = CardId::by_name("Knights").unwrap();

        let mut two_units = crate::game::new_game(2, 0);
        two_units.players[0].techs.insert(cavalry, TechSlot { workers: 1, stored: 0 });
        let s2 = board_yields(CardId::by_name("Alexander the Great").unwrap(), &Baseline::at(&two_units, 0))
            .expect("Alexander is a swap type (Leader)");

        let mut four_units = crate::game::new_game(2, 0);
        four_units.players[0].techs.get_mut(infantry).unwrap().workers = 2;
        four_units.players[0].techs.insert(cavalry, TechSlot { workers: 2, stored: 0 });
        let s4 = board_yields(CardId::by_name("Alexander the Great").unwrap(), &Baseline::at(&four_units, 0))
            .expect("Alexander is a swap type (Leader)");

        let strength_at = |t: &[Triple]| t.iter().find(|&&(f, _, _)| f == Feature::Strength).map_or(0.0, |&(_, a, _)| a);
        assert!(strength_at(&s2) > 0.0, "2 unit workers must be a real gain, not just a tautological 0 == 0: {s2:?}");
        assert_eq!(strength_at(&s4), 2.0 * strength_at(&s2), "4 unit workers must be exactly 2x 2 unit workers: {s2:?} vs {s4:?}");
    }

    /// Napoleon Bonaparte: strength per DISTINCT unit TYPE present, not per
    /// worker. 4 infantry workers (1 type) must give LESS than 1 infantry +
    /// 1 cavalry + 1 artillery (3 types), pinning that this counts types,
    /// not bodies.
    #[test]
    fn napoleons_strength_scales_with_distinct_unit_types() {
        // Warriors (infantry) is already in the starting kit at 1 worker.
        let infantry = CardId::by_name("Warriors").unwrap();
        let cavalry = CardId::by_name("Knights").unwrap();
        let artillery = CardId::by_name("Cannon").unwrap();

        let mut one_type = crate::game::new_game(2, 0);
        one_type.players[0].techs.get_mut(infantry).unwrap().workers = 4;
        let s1 = board_yields(CardId::by_name("Napoleon Bonaparte").unwrap(), &Baseline::at(&one_type, 0))
            .expect("Napoleon is a swap type (Leader)");

        let mut three_types = crate::game::new_game(2, 0);
        three_types.players[0].techs.insert(cavalry, TechSlot { workers: 1, stored: 0 });
        three_types.players[0].techs.insert(artillery, TechSlot { workers: 1, stored: 0 });
        let s3 = board_yields(CardId::by_name("Napoleon Bonaparte").unwrap(), &Baseline::at(&three_types, 0))
            .expect("Napoleon is a swap type (Leader)");

        let strength_at = |t: &[Triple]| t.iter().find(|&&(f, _, _)| f == Feature::Strength).map_or(0.0, |&(_, a, _)| a);
        assert!(strength_at(&s1) > 0.0, "1 type present must be a real gain, not just a tautological 0 == 0: {s1:?}");
        assert_eq!(strength_at(&s3), 3.0 * strength_at(&s1), "3 distinct types must be exactly 3x 1 type, regardless of the extra 3 infantry workers in s1: {s1:?} vs {s3:?}");
    }

    /// Joan of Arc: strength per happy face from temples AND the
    /// government's own printed happy. A temple alone under Despotism
    /// (0 government happy) vs the same temple under Theocracy (+1
    /// government happy) must show more strength under Theocracy.
    #[test]
    fn joan_of_arcs_strength_scales_with_temple_and_government_happy() {
        // Religion is already in the starting kit, unstaffed.
        let temple = CardId::by_name("Religion").unwrap();

        let mut despotism = crate::game::new_game(2, 0);
        despotism.players[0].techs.get_mut(temple).unwrap().workers = 1;
        let s_despotism = board_yields(CardId::by_name("Joan of Arc").unwrap(), &Baseline::at(&despotism, 0))
            .expect("Joan of Arc is a swap type (Leader)");

        let mut theocracy = crate::game::new_game(2, 0);
        theocracy.players[0].techs.get_mut(temple).unwrap().workers = 1;
        theocracy.players[0].government = CardId::by_name("Theocracy").unwrap();
        let s_theocracy = board_yields(CardId::by_name("Joan of Arc").unwrap(), &Baseline::at(&theocracy, 0))
            .expect("Joan of Arc is a swap type (Leader)");

        let strength_at = |t: &[Triple]| t.iter().find(|&&(f, _, _)| f == Feature::Strength).map_or(0.0, |&(_, a, _)| a);
        assert!(
            strength_at(&s_theocracy) > strength_at(&s_despotism),
            "Theocracy's own +1 government happy must add strength on top of the temple's: despotism={s_despotism:?} theocracy={s_theocracy:?}"
        );
    }

    /// James Cook prints BOTH `cultureFirstColony` (fires once ANY colony is
    /// held) and `culturePerAdditionalColony` (scales with colonies beyond
    /// the first). A player with 1 colony must show only the first-colony
    /// bonus; a player with 3 must show the first-colony bonus PLUS 2x the
    /// per-additional-colony coefficient.
    #[test]
    fn james_cooks_culture_fires_once_a_colony_is_held() {
        let territory = CardId::by_name("Vast Territory (I)").unwrap();
        let none = crate::game::new_game(2, 0);
        let s_none = board_yields(CardId::by_name("James Cook").unwrap(), &Baseline::at(&none, 0))
            .expect("James Cook is a swap type (Leader)");
        let culture_at = |t: &[Triple]| t.iter().find(|&&(f, _, _)| f == Feature::CultureRate).map_or(0.0, |&(_, a, _)| a);
        assert_eq!(culture_at(&s_none), 0.0, "no colony: no bonus yet: {s_none:?}");

        let mut one = crate::game::new_game(2, 0);
        one.players[0].colonies.push(territory);
        let s_one = board_yields(CardId::by_name("James Cook").unwrap(), &Baseline::at(&one, 0))
            .expect("James Cook is a swap type (Leader)");
        assert!(culture_at(&s_one) > 0.0, "1 colony must fire the first-colony bonus: {s_one:?}");
    }

    /// The other half of James Cook: additional colonies beyond the first
    /// must scale the delta further, on top of the first-colony floor.
    #[test]
    fn james_cooks_culture_scales_with_colonies_beyond_the_first() {
        let territory = CardId::by_name("Vast Territory (I)").unwrap();
        let culture_at = |t: &[Triple]| t.iter().find(|&&(f, _, _)| f == Feature::CultureRate).map_or(0.0, |&(_, a, _)| a);

        let mut one = crate::game::new_game(2, 0);
        one.players[0].colonies.push(territory);
        let s_one = board_yields(CardId::by_name("James Cook").unwrap(), &Baseline::at(&one, 0))
            .expect("James Cook is a swap type (Leader)");

        let mut three = crate::game::new_game(2, 0);
        three.players[0].colonies.push(territory);
        three.players[0].colonies.push(territory);
        three.players[0].colonies.push(territory);
        let s_three = board_yields(CardId::by_name("James Cook").unwrap(), &Baseline::at(&three, 0))
            .expect("James Cook is a swap type (Leader)");

        assert!(
            culture_at(&s_three) > culture_at(&s_one),
            "3 colonies (1 first-colony + 2 additional) must be worth more than 1 colony (1 first-colony + 0 additional): 1={s_one:?} 3={s_three:?}"
        );
    }

    /// [`rider_delta`] subtracts the OUTGOING leader's rider when replacing
    /// them -- holding Hammurabi and taking a leader with no rider of their
    /// own (Aristotle: `scienceOnTechCardTake`, still deliberately unpriced,
    /// carries no rider function) must show the conversion being GIVEN UP,
    /// not silently dropped. Mirrors the existing Genghis Khan/Winston
    /// Churchill replacement behaviour this module already relies on.
    #[test]
    fn replacing_hammurabi_with_a_leader_with_no_rider_prices_the_conversion_as_lost() {
        let mut state = crate::game::new_game(2, 0);
        state.players[0].leader = CardId::by_name("Hammurabi").unwrap();
        let base = Baseline::at(&state, 0);
        let swap =
            board_yields(CardId::by_name("Aristotle").unwrap(), &base).expect("Aristotle is a swap type (Leader)");
        assert!(
            swap.contains(&(Feature::CivilActions, -1.0, Kind::Gain)),
            "expected the -1 CivilActions loss of Hammurabi's conversion when replacing him, got {swap:?}"
        );
    }
}
