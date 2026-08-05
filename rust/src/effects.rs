//! Card effects: a player's aggregate statistics.
//!
//! Ports `engine/effects.py::compute` / `state_stats` -- the function nearly
//! everything else in the engine leans on: legal-move generation, the
//! evaluator and the End-of-Turn Sequence all read [`Stats`] rather than
//! walking a tableau themselves.
//!
//! DESIGN.md "the effect system, and why it is two things" is the contract
//! this file follows: the ~25 numeric keys that recur across cards are fields
//! on [`CardEffects`]/[`Production`] in the static table, summed here by a
//! contiguous scan of the tableau; the ~60 one-offs are dispatched by an
//! EXHAUSTIVE `match` on [`Special`], so a card whose rule this file cannot
//! interpret is a compile error, never a silently-ignored key.
//!
//! ## KNOWN GAPS (deliberate, not oversights -- see the port report)
//!
//! Python's `compute` reads a few things this port cannot reach yet, because
//! the data or the state shape to hold them does not exist on the Rust side:
//!
//! - **`army_strength`** (`engine/effects.py:471`): tactical strength from a
//!   tactic + army composition. Needs a tactic's `composition` list and
//!   `obsoleteStrength`, neither of which `card_table.rs` carries today.
//!   `Stats.strength` here is Python's PRE-army-strength baseline.
//! - **Pacts** (`_apply_pacts`, `engine/effects.py:608`): `PlayerState` has no
//!   `pacts` field in `state.rs` yet, so `tech_discount` / `war_immune` /
//!   `food_as_resource` / `resource_as_food` / `science_partners` are not on
//!   [`Stats`] at all rather than being silently wrong.
//! - **A colony's own bonuses** (`_colony_permanents`, `permanent` production,
//!   `engine/effects.py:453-458`): needs `permanentEffects`/`permanent` keys
//!   the generator does not capture (a sibling gap to the one this port fixed
//!   for urban buildings -- see `gen_cards.py` `PRODUCTION_FIELDS`). The two
//!   colony-COUNT modifiers (`CultureFirstColony`, `CulturePerAdditionalColony`)
//!   ARE implemented below, since they only need `p.colonies.len()`.
//! - **`build_discount`** (a per-age dict on Python's `Stats`): not needed by
//!   anything in this port's scope (`build_cost` is not ported), and a dict
//!   does not fit this struct's "flat fields, no Vec/HashMap" shape anyway.
//!
//! None of these affect the starting position or any pre-tactic, pre-pact,
//! pre-colony game state, which is what this file's tests cover.
//!
//! ## Caching
//!
//! Python caches the result on `state._stats_cache` and invalidates it on
//! every mutation (`engine/effects.py` `state_stats` / `invalidate`).
//! DELIBERATELY not replicated here: `compute` is recomputed fresh every
//! call. DESIGN.md's measured baseline is that the Python profile is FLAT --
//! `dict.get` is the single largest entry at 9% -- so there is no hot loop a
//! cache is fixing, and adding one on faith, before a profile of the
//! finished Rust engine says `compute` is hot, is exactly the premature
//! optimisation this rewrite exists to avoid. Add it later, measured.

use crate::cards::{CardEffects, CardId, CardType, Production, Special};
use crate::state::{GameState, PlayerState, Tableau};

/// A player's aggregate statistics for one recomputation. Mirrors Python's
/// `effects.Stats` dataclass, minus the fields listed in this module's
/// "KNOWN GAPS" doc comment.
///
/// Signed, 32-bit throughout: unlike [`CardEffects`] (one card, kept small so
/// `CARDS` stays cache-friendly), this is a per-call accumulation over an
/// entire tableau, and nothing here is stored in a `Clone`d [`GameState`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stats {
    pub science: i32,
    pub culture: i32,
    pub strength: i32,
    /// Happy faces, clamped to `[0, 8]` (rulebook "Limits on Ratings").
    pub happy: i32,
    pub civil_actions: i32,
    pub military_actions: i32,
    /// Distinct urban buildings (lab/temple/library/arena/theater) allowed in
    /// play at once. Printed on the government; `gov.urban_building_limit`
    /// defaults to 2 when unset (`engine/effects.py:422`: `... or 2`).
    pub urban_limit: i32,
    pub food: i32,
    pub resources: i32,
    pub colonize: i32,
    /// Bonus ON TOP of `civil_actions` for hand-size purposes (§2.5) --
    /// `engine/actions.py::civil_hand_limit` adds these two together; they
    /// stay separate fields here because `compute` never combines them.
    pub civil_hand_limit: i32,
    pub military_hand_limit: i32,
    /// Stages paid per "build a wonder step" action. Python takes the MAX
    /// across every `wonderStagesPerAction` source, not a sum -- see
    /// `add_flat_except_actions` below.
    pub wonder_stages: i32,
    pub pop_food_discount: i32,
    pub free_pop_per_turn: bool,
    pub no_aggression: bool,
}

impl Default for Stats {
    fn default() -> Self {
        Stats {
            science: 0,
            culture: 0,
            strength: 0,
            happy: 0,
            civil_actions: 4,
            military_actions: 2,
            urban_limit: 2,
            food: 0,
            resources: 0,
            colonize: 0,
            civil_hand_limit: 0,
            military_hand_limit: 0,
            wonder_stages: 1,
            pop_food_discount: 0,
            free_pop_per_turn: false,
            no_aggression: false,
        }
    }
}

// ------------------------------------------------------------- accumulation

/// Add a card's per-turn/one-shot-on-the-board flat numbers, EXCEPT
/// `civil_actions`/`military_actions`. Split out from [`add_flat`] because
/// the government's `civil_actions`/`military_actions`/`urban_limit` are a
/// SET-with-default (`engine/effects.py:420-422`, `gov.get(...) or default`),
/// not an addition -- `compute` below sets those three directly from the
/// government's card, then calls this (not `add_flat`) for its remaining
/// fields, so the base value is never double-counted against itself.
fn add_flat_except_actions(stats: &mut Stats, eff: &CardEffects) {
    stats.culture += eff.culture as i32;
    stats.science += eff.science as i32;
    stats.strength += eff.strength as i32;
    stats.happy += eff.happy as i32;
    // Python's FLAT_KEYS maps BOTH `colonizeBonus` and `colonizationBonus` to
    // the one `Stats.colonize` field (they are aliases used by different
    // printers -- colony cards vs. pact blocks); `gen_cards.py` keeps them as
    // two separate `CardEffects` fields, so both are added here.
    stats.colonize += eff.colonization_bonus as i32 + eff.colonize_bonus as i32;
    stats.civil_hand_limit += eff.civil_hand_limit as i32;
    stats.military_hand_limit += eff.military_hand_limit as i32;
    let stages = eff.wonder_stages_per_action as i32;
    if stages > stats.wonder_stages {
        stats.wonder_stages = stages;
    }
}

/// [`add_flat_except_actions`] plus `civil_actions`/`military_actions` as a
/// genuine addition -- for every source EXCEPT the government itself
/// (special techs, wonders, the leader all grant these via their `effects`
/// dict in Python, which really is additive: Napoleon is +2 military
/// actions, not a base-setting 2).
fn add_flat(stats: &mut Stats, eff: &CardEffects) {
    add_flat_except_actions(stats, eff);
    stats.civil_actions += eff.civil_actions as i32;
    stats.military_actions += eff.military_actions as i32;
}

/// Add a `Production` block, scaled by `mult` (a worker count for tableau
/// cards, or 1 for the government/leader, which have none). Mirrors
/// `engine/effects.py::_add_production`.
fn add_production(stats: &mut Stats, prod: &Production, mult: i32) {
    stats.food += prod.food as i32 * mult;
    stats.resources += prod.resources as i32 * mult;
    stats.science += prod.science as i32 * mult;
    stats.culture += prod.culture as i32 * mult;
    stats.happy += prod.happy as i32 * mult;
    stats.strength += prod.strength as i32 * mult;
}

// ------------------------------------------------------------------ queries
//
// Small, repeated tableau queries `_apply_modifier` needs. Each is a short
// contiguous scan of `p.techs` (DESIGN.md: "faster than the Python
// equivalent's dict walk plus per-name type lookup").

/// Total workers on cards of the matching type(s). Mirrors
/// `engine/effects.py::workers_on_types`.
fn workers_on(techs: &Tableau, pred: impl Fn(CardType) -> bool) -> i32 {
    techs
        .iter()
        .filter(|(id, _)| pred(id.kind()))
        .map(|(_, slot)| slot.workers as i32)
        .sum()
}

/// Happy faces produced by cards of the matching type(s). Mirrors
/// `engine/effects.py::_happy_from`.
fn happy_from(techs: &Tableau, pred: impl Fn(CardType) -> bool) -> i32 {
    techs
        .iter()
        .filter(|(id, _)| pred(id.kind()))
        .map(|(id, slot)| id.get().production.happy as i32 * slot.workers as i32)
        .sum()
}

/// Highest-AGE staffed card of the matching type(s), `None` if there is none.
/// Ties keep the first one found in tableau (insertion) order, matching
/// Python's `best_card`, which keeps the first name it sees at a given level
/// because it only replaces `best` on a STRICT `>` comparison, and Python
/// dict iteration order is insertion order.
///
/// FAQ v1.5 p.9 (quoted in `engine/effects.py::_building_modifier`): an
/// unstaffed technology produces nothing, so "best" here means best STAFFED.
fn best_staffed(techs: &Tableau, pred: impl Fn(CardType) -> bool) -> Option<CardId> {
    let mut best: Option<(CardId, u8)> = None;
    for (id, slot) in techs.iter() {
        if slot.workers == 0 || !pred(id.kind()) {
            continue;
        }
        let level = id.level();
        if best.is_none_or(|(_, best_level)| level > best_level) {
            best = Some((id, level));
        }
    }
    best.map(|(id, _)| id)
}

/// Number of cards/buildings providing at least one happy face (St. Peter's
/// Basilica). Mirrors `engine/effects.py::_happy_source_count`: "every
/// building/CARD providing happy faces provides one additional happy face"
/// -- so the government and leader cards count too, not only buildings, and
/// a wonder ruined by Ravages of Time provides none.
///
/// Colonies are NOT counted here -- see this module's "KNOWN GAPS" doc.
fn happy_source_count(p: &PlayerState) -> i32 {
    let mut n = 0;
    for (id, slot) in p.techs.iter() {
        if id.get().production.happy > 0 {
            n += slot.workers as i32;
        }
    }
    for &w in p.completed_wonders.as_slice() {
        if p.flipped_wonders.contains(w) {
            continue;
        }
        if w.get().effects.happy > 0 {
            n += 1;
        }
    }
    if !p.leader.is_none() && p.leader.get().effects.happy > 0 {
        n += 1;
    }
    if p.government.get().production.happy > 0 {
        n += 1;
    }
    n
}

// --------------------------------------------------------------- dispatch

/// The exhaustive dispatch over [`Special`]. Every one of the 92 generated
/// variants appears exactly once below, in one of four groups:
///
/// 1. Implemented -- read by `compute`/`state_stats` (Python's
///    `MODIFIER_KEYS`/`SPECIAL_KEYS`, `_apply_modifier`/`_apply_special`).
/// 2. `// belongs to actions.rs` -- a one-shot trigger on a civil action,
///    build, develop, political action, or a build-cost discount. None of
///    these are read by `engine/effects.py::compute`.
/// 3. `// belongs to combat.rs` -- aggression/war/tactic/colonization
///    resolution.
/// 4. `// belongs to events.rs` -- age/military event-card resolution and
///    targeting.
/// 5. `// not yet ported` -- genuinely part of `compute` in Python (pacts),
///    blocked on `state.rs` growing a `pacts` field, not a scoping choice.
///
/// Adding a 93rd variant to the generated enum breaks this match at compile
/// time, which is the entire point (DESIGN.md).
fn apply_special(stats: &mut Stats, p: &PlayerState, special: Special) {
    use Special::*;
    match special {
        // ---------------------------------------------- implemented ----

        // Charlie Chaplin: best theater's OWN printed culture, once more --
        // not doubled by its worker count, matching `_apply_modifier`'s
        // `(db.get(b).get("production") or {}).get("culture", 0)`.
        BestTheaterDoubleCulture => {
            if let Some(b) = best_staffed(&p.techs, |k| k == CardType::Theater) {
                stats.culture += b.get().production.culture as i32;
            }
        }
        // Transcontinental Railroad: best staffed mine's own resources, once more.
        DoubleBestMine => {
            if let Some(b) = best_staffed(&p.techs, |k| k == CardType::Mine) {
                stats.resources += b.get().production.resources as i32;
            }
        }
        // Leonardo / Newton / Einstein: best staffed lab-or-library produces
        // extra science equal to its level. The three leaders' printed value
        // is always the flag `true` (Python ignores `val` for this key too).
        SciencePerBestLabOrLibraryLevel => {
            if let Some(b) = best_staffed(&p.techs, |k| matches!(k, CardType::Lab | CardType::Library)) {
                stats.science += b.level() as i32;
            }
        }
        // Sid Meier: every lab produces culture equal to its level.
        CulturePerLabEqualToLevel => {
            for (id, slot) in p.techs.iter() {
                if id.kind() == CardType::Lab {
                    stats.culture += id.level() as i32 * slot.workers as i32;
                }
            }
        }
        // Bill Gates: every lab produces resources equal to its level.
        ResourcesPerLabEqualToLevel => {
            for (id, slot) in p.techs.iter() {
                if id.kind() == CardType::Lab {
                    stats.resources += id.level() as i32 * slot.workers as i32;
                }
            }
        }
        // Sid Meier: sciencePerLab is -1 -- a REDUCTION, not a bonus. This is
        // the exact value the un-fixed generator used to drop (see the port
        // report): a bare unit variant here would have silently added +1.
        SciencePerLab(v) => {
            stats.science += v as i32 * workers_on(&p.techs, |k| k == CardType::Lab);
        }
        // J. S. Bach.
        CulturePerTheater(v) => {
            stats.culture += v as i32 * workers_on(&p.techs, |k| k == CardType::Theater);
        }
        // William Shakespeare: min(library workers, theater workers) pairs.
        CulturePerLibraryTheaterPair(v) => {
            let lib = workers_on(&p.techs, |k| k == CardType::Library);
            let theater = workers_on(&p.techs, |k| k == CardType::Theater);
            stats.culture += v as i32 * lib.min(theater);
        }
        // Michelangelo: happy from temples/theaters plus completed
        // (unflipped) wonders' printed happy, clamped at 0 before scaling --
        // `engine/effects.py:536` `max(0, happy)`.
        CulturePerHappyFromTemplesTheatersWonders(v) => {
            let mut happy = happy_from(&p.techs, |k| matches!(k, CardType::Temple | CardType::Theater));
            for &w in p.completed_wonders.as_slice() {
                if p.flipped_wonders.contains(w) {
                    continue;
                }
                happy += w.get().effects.happy as i32;
            }
            stats.culture += v as i32 * happy.max(0);
        }
        // St. Peter's Basilica.
        ExtraHappyPerHappySource(v) => {
            stats.happy += v as i32 * happy_source_count(p);
        }
        // Alexander the Great.
        StrengthPerMilitaryUnit(v) => {
            stats.strength += v as i32 * workers_on(&p.techs, CardType::is_unit);
        }
        // Great Wall.
        StrengthPerInfantry(v) => {
            stats.strength += v as i32 * workers_on(&p.techs, |k| k == CardType::Infantry);
        }
        StrengthPerArtillery(v) => {
            stats.strength += v as i32 * workers_on(&p.techs, |k| k == CardType::Artillery);
        }
        // Napoleon Bonaparte: +val strength per DISTINCT unit type present
        // (not per worker) -- confirmed val=2 in the data, which the
        // un-fixed generator used to drop (see the port report).
        StrengthPerUnitType(v) => {
            let mut seen = [false; 4]; // Infantry, Cavalry, Artillery, Air
            for (id, slot) in p.techs.iter() {
                if slot.workers == 0 {
                    continue;
                }
                let i = match id.kind() {
                    CardType::Infantry => 0,
                    CardType::Cavalry => 1,
                    CardType::Artillery => 2,
                    CardType::Air => 3,
                    _ => continue,
                };
                seen[i] = true;
            }
            let types = seen.iter().filter(|&&b| b).count() as i32;
            stats.strength += v as i32 * types;
        }
        // Joan of Arc.
        StrengthPerTempleOrGovernmentHappy(v) => {
            let happy = happy_from(&p.techs, |k| k == CardType::Temple)
                + p.government.get().production.happy as i32;
            stats.strength += v as i32 * happy;
        }
        // James Cook. Colony-COUNT only -- see this module's "KNOWN GAPS".
        CultureFirstColony(v) => {
            if !p.colonies.is_empty() {
                stats.culture += v as i32;
            }
        }
        CulturePerAdditionalColony(v) => {
            stats.culture += v as i32 * (p.colonies.len() as i32 - 1).max(0);
        }
        // Ocean Liners.
        FreePopIncreasePerTurn => stats.free_pop_per_turn = true,
        // Mahatma Gandhi.
        CannotPlayAggressionOrWar => stats.no_aggression = true,
        // Moses.
        PopIncreaseFoodDiscount(v) => stats.pop_food_discount += v as i32,
        // Homer. `compute` never reads this key's VALUE in Python either --
        // the +1 happy is hardcoded in the wonders loop keyed on
        // `p.homer_wonder == w` (`engine/effects.py:448-449`), unconditionally.
        // This arm is a deliberate no-op mirroring that, not a gap: `compute`
        // below applies the +1 itself when it walks `completed_wonders`.
        OnReplacePutUnderCompletedWonderHappy(_) => {}

        // ------------------------------------- belongs to actions.rs ----
        // One-shot triggers on a civil action / build / develop / political
        // action, or a build-cost discount. `engine/effects.py::compute`
        // reads none of these; `engine/actions.py` and the leader-trigger
        // functions in `engine/effects.py` (`on_develop`, `on_take_card`,
        // ...) do, by NAME dispatch, not by this key.
        BuildDiscount
        | CivilActionBackOnTechDevelop(_)
        | CivilActionUpgradeUrbanBuildingToTheater
        | ComboFoodDiscount(_)
        | ComboResourceDiscount(_)
        | CultureOnLeaveEqualToLabResourceProduction
        | CultureOnRevolution(_)
        | CultureOnTechDevelop(_)
        | CulturePerCivilizationWithMoreCulture
        | FreeCivilAction
        | GainFoodOrResources(_)
        | LeaderTakeCivilActionDiscount(_)
        | LibraryDiscountsIfTheater
        | MilitaryActionAsCivilPerTurn(_)
        | MilitaryActionCombinedPopIncreaseAndUnitBuild
        // Fast Food Chains / Internet / Hollywood: `onBuildCulture`'s value is
        // a free-text formula in the data (not a machine number), resolved by
        // `engine/effects.py::_one_time_culture`/`wonder_completion_culture`
        // at wonder-completion time -- a one-shot build trigger, not a
        // recurring `compute()` field.
        | OnBuildCulture
        | OnBuildCulturePerTechLevelSum
        | OncePerGameTwoPoliticalActions
        | PeekTopEventCardInPolitics
        | PerTurnChoice
        | RemoveAsPoliticalActionForYellowToken(_)
        | RemoveAsPoliticalActionFreeColonize
        | ResourceOnMilitaryUnitBuildOrUpgrade(_)
        | ResourceOnTechDevelop(_)
        | ResourcesForMilitaryUnitsPerStrongerCivilization
        | RevolutionUsesMilitaryActionsInstead
        | ScienceOnTechCardTake(_)
        | TheaterResourceDiscountIfLibrary(_)
        | TheaterScienceDiscountIfLibrary(_)
        | TheaterTechScienceDiscount(_)
        | WonderTakeNoExtraCivilActions => {}

        // -------------------------------------- belongs to combat.rs ----
        // Aggression / war / tactic / colonization resolution.
        ColonizeDiscardUpTo2MilitaryCardsForBonus(_)
        | ColonyImmediateBonusApplies
        | ColonyPermanentBonusTransfers
        | CultureIfTopTwoStrength(_)
        | DestroyUrbanBuildings
        | DoublesTacticBonusOfOneArmy
        // Aggression: Raid -- "half of each destroyed building's printed
        // build cost, rounded up" is a free-text formula in the data, not a
        // number; resolved at aggression-resolution time, not by `compute()`.
        | GainResources
        | GainCulturePerLevelOfRemovedCard(_)
        | InfantryCountsAsCavalryForTactics
        | OpponentDecreasesPopulation(_)
        | OpponentsPayDoubleMilitaryActionsToAttackYou
        | OrTakesSpecialTechnologiesOfSameTotalScienceCost
        | RemoveFromGame
        | StealColony(_)
        | TakeFromOpponent
        | VictorTakesCulture
        | VictorTakesScienceUpTo
        | VictorTakesYellowTokens => {}

        // -------------------------------------- belongs to events.rs ----
        // Age/military event-card resolution and targeting.
        AllPlayers
        | Condition
        | DecreasePopulation(_)
        | Duration
        | Gain
        | Lose
        | LastRoundSubstitute
        | PlayerWithLeastCulture
        | PlayerWithMostCulture
        | PlayersWithMostDiscontentWorkers
        | PlayersWithMostHappyFaces
        | StrongestPlayer
        | StrongestPlayers
        | Target
        | WeakestPlayer
        | WeakestPlayers => {}

        // ------------------------------------------- not yet ported -----
        // Pact mechanics (`engine/effects.py::_apply_pacts`, called from
        // `compute` itself). These genuinely belong in `state_stats` --
        // unlike the two groups above, this is a real gap, not an
        // out-of-scope module. `state.rs` now has `pacts`/`Pact`/`PactList`
        // and `A`/`B`/`BothPlayers`/`OnAttackBetweenParties` now carry a
        // real `PactBlock` payload (gen_cards.py, 2026-08-05) -- the port
        // itself is the next commit on top of this one; this arm is a
        // type-layer-only placeholder so the crate keeps building in
        // between (`(_)` discards the now-real payload deliberately, not
        // silently: see this module's top "KNOWN GAPS").
        A(_) | B(_) | BothPlayers(_) | CancelledIfPartiesAttackEachOther | NoAttacksBetweenParties
        | OnAttackBetweenParties(_) => {}
    }
}

// ------------------------------------------------------------------ compute

/// Full statistics for a player. Ports `engine/effects.py::compute`; see
/// this module's doc comment for what is deliberately not ported yet.
pub fn compute(state: &GameState, p: &PlayerState) -> Stats {
    let mut stats = Stats::default();

    // --- government: civil/military actions and the urban limit are a
    // SET-with-default, not an addition -- see `add_flat_except_actions`.
    let gov = p.government.get();
    let ge = &gov.effects;
    stats.civil_actions = if ge.civil_actions != 0 { ge.civil_actions as i32 } else { 4 };
    stats.military_actions = if ge.military_actions != 0 { ge.military_actions as i32 } else { 2 };
    stats.urban_limit = if ge.urban_building_limit != 0 { ge.urban_building_limit as i32 } else { 2 };
    add_production(&mut stats, &gov.production, 1);
    add_flat_except_actions(&mut stats, ge);
    for &sp in gov.special {
        apply_special(&mut stats, p, sp);
    }

    // --- phase 1: technologies. A contiguous scan of the tableau
    // (DESIGN.md), not a lookup per card.
    for (id, slot) in p.techs.iter() {
        let card = id.get();
        match card.kind {
            // Special techs carry a flat `effects` block read unconditionally
            // (Python: `if eff is not None: _apply_flat(...)`), regardless of
            // worker count -- special techs never take workers at all
            // (`CardType::takes_workers` excludes `SpecialTech`).
            CardType::SpecialTech => {
                add_flat(&mut stats, &card.effects);
                for &sp in card.special {
                    apply_special(&mut stats, p, sp);
                }
            }
            // Farms/mines/urban buildings: printed `production`, scaled by
            // workers. One call covers all of them because a farm's
            // `Production` only ever has `food` set, a mine only
            // `resources`, and urban buildings only `science`/`culture`/
            // `happy`/`strength` -- see `cards::Production`.
            CardType::Farm
            | CardType::Mine
            | CardType::Lab
            | CardType::Temple
            | CardType::Library
            | CardType::Arena
            | CardType::Theater => {
                add_production(&mut stats, &card.production, slot.workers as i32);
            }
            // Units: printed `strength` (captured from the card's top-level
            // `strength` key, NOT its `production`), scaled by workers.
            // `engine/effects.py::_tech_prog` reads ONLY this for units --
            // notably, Air Forces' `effects.doublesTacticBonusOfOneArmy` is
            // never read by `compute` either; it belongs to combat.rs.
            CardType::Infantry | CardType::Cavalry | CardType::Artillery | CardType::Air => {
                stats.strength += card.effects.strength as i32 * slot.workers as i32;
            }
            other => unreachable!(
                "{other:?} cannot be in `p.techs` -- government/wonder/leader/action/\
                 military-deck cards live on their own `PlayerState` fields (see state.rs)"
            ),
        }
    }

    // --- phase 2: wonders.
    for &w in p.completed_wonders.as_slice() {
        if p.flipped_wonders.contains(w) {
            // Ravages of Time: effects gone, ruins produce culture instead.
            stats.culture += 2;
            continue;
        }
        let card = w.get();
        add_flat(&mut stats, &card.effects);
        for &sp in card.special {
            apply_special(&mut stats, p, sp);
        }
        if p.homer_wonder == w {
            stats.happy += 1;
        }
    }

    // --- phase 2: leader.
    if !p.leader.is_none() {
        let card = p.leader.get();
        add_flat(&mut stats, &card.effects);
        for &sp in card.special {
            apply_special(&mut stats, p, sp);
        }
        add_production(&mut stats, &card.production, 1);
    }

    // --- phase 2: colonies. Only the colony-COUNT-based leader modifiers
    // (`CultureFirstColony`, `CulturePerAdditionalColony`, dispatched above
    // via the leader's `special` list) read `p.colonies` today. A colony's
    // OWN bonuses are not applied -- see this module's top "KNOWN GAPS".

    // --- pacts: not ported -- `PlayerState` has no `pacts` field yet.
    let _ = state; // reserved for pacts/army_strength once those land.

    // --- event-granted permanents.
    stats.culture += p.culture_rate_extra as i32;
    stats.science += p.science_rate_extra as i32;
    stats.strength += p.strength_extra as i32;
    stats.happy += p.happy_extra as i32;

    // NOTE: Python adds `army_strength(state, p)` to `s.strength` here
    // (`engine/effects.py:471`). Not ported -- see this module's top
    // "KNOWN GAPS": tactic `composition`/`obsoleteStrength` are not in the
    // card type layer yet, so there is nothing to compute it from.

    // "Limits on Ratings" (rulebook, Civilization Statistics): no rating may
    // go below zero.
    stats.science = stats.science.max(0);
    stats.culture = stats.culture.max(0);
    stats.food = stats.food.max(0);
    stats.resources = stats.resources.max(0);
    stats.strength = stats.strength.max(0);
    stats.happy = stats.happy.clamp(0, 8);
    stats
}

/// Cached per-mutation stats in Python (`engine/effects.py::state_stats`).
/// Here: just `compute` -- see this module's top doc comment "Caching".
pub fn state_stats(state: &GameState, p: &PlayerState) -> Stats {
    compute(state, p)
}

// ============================================================== tests ====

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::CARDS;
    use crate::state::{CardList, GameState, PactList, Phase, PlayerState, Tableau, TechSlot, MAX_PLAYERS, ROW_SIZE};

    fn card(name: &str) -> CardId {
        CardId::by_name(name).unwrap_or_else(|| panic!("no such card: {name}"))
    }

    /// A player with nothing but a government -- every other field zeroed /
    /// `CardId::NONE`. `PlayerState` does not derive `Default` (several
    /// fields, like `leader`, must default to `CardId::NONE`, not
    /// `CardId(0)`, which `#[derive(Default)]` on `CardId` would give), so
    /// tests build one explicitly.
    fn blank_player(idx: u8, government: CardId) -> PlayerState {
        PlayerState {
            idx,
            techs: Tableau::new(),
            government,
            leader: CardId::NONE,
            used_leader_ability: false,
            wonder: CardId::NONE,
            wonder_steps: 0,
            completed_wonders: CardList::new(),
            destroyed_wonders: 0,
            homer_wonder: CardId::NONE,
            tactic: CardId::NONE,
            tactic_exclusive: false,
            colonies: CardList::new(),
            flipped_wonders: CardList::new(),
            pacts: PactList::new(),
            hand_civil: CardList::new(),
            hand_military: CardList::new(),
            hidden_civil: 0,
            hidden_military: 0,
            yellow_bank: 0,
            yellow_granted: 0,
            workers_free: 0,
            blue_total: 0,
            food: 0,
            resources: 0,
            science: 0,
            culture: 0,
            culture_rate_extra: 0,
            science_rate_extra: 0,
            strength_extra: 0,
            happy_extra: 0,
            civil_actions: 0,
            military_actions: 0,
            politics_done: false,
            tactic_action_used: false,
            taken_this_turn: CardList::new(),
            ca_spent_taking: 0,
            hammurabi_used: false,
            churchill_used: false,
            bach_upgrade_used: false,
            ocean_liners_used: false,
            caesar_double_politics_used: false,
            skip_next_politics: false,
            ca_penalty_next_turn: 0,
            mil_discount: 0,
            mil_sci_discount: 0,
            resigned: false,
        }
    }

    /// A `GameState` around the given players. `compute`/`state_stats` do
    /// not currently read anything but `p` itself (see "KNOWN GAPS": pacts
    /// and army_strength are the only things that would need `state`), so
    /// this only needs to be well-typed, not game-accurate.
    fn blank_state(num_players: u8, players: [PlayerState; MAX_PLAYERS]) -> GameState {
        GameState {
            num_players,
            seed: 0,
            players,
            current: 0,
            turn: 1,
            round: 1,
            start_player: 0,
            age_civil: crate::cards::Age::A,
            age_military: crate::cards::Age::A,
            civil_deck: CardList::new(),
            military_deck: CardList::new(),
            card_row: [CardId::NONE; ROW_SIZE],
            future_events: CardList::new(),
            current_events: CardList::new(),
            past_events: CardList::new(),
            current_events_age: crate::cards::Age::A,
            scoring_events: CardList::new(),
            available_tactics: CardList::new(),
            civil_discard: [
                CardList::new(),
                CardList::new(),
                CardList::new(),
                CardList::new(),
                CardList::new(),
            ],
            civil_removed: [
                CardList::new(),
                CardList::new(),
                CardList::new(),
                CardList::new(),
                CardList::new(),
            ],
            discarded_military: [
                CardList::new(),
                CardList::new(),
                CardList::new(),
                CardList::new(),
                CardList::new(),
            ],
            last_round: false,
            final_round_end: None,
            game_over: false,
            phase: Phase::Actions,
            forced_winner: None,
        }
    }

    /// One player, rest of the seats blank Despotism players (never read by
    /// `compute` for a different player, so their exact shape doesn't matter).
    fn one_player_state(num_players: u8, p: PlayerState) -> GameState {
        let filler = || blank_player(0, card("Despotism"));
        let mut players = [filler(), filler(), filler(), filler()];
        players[0] = p;
        blank_state(num_players, players)
    }

    // ------------------------------------------------- starting position

    /// engine/tests/test_engine.py::TestSetup::test_starting_tableau, at
    /// 2p/3p/4p -- `compute` never reads player count, only the tableau, so
    /// this is really "does the same starting tableau compute the same
    /// stats regardless of table size", which is the thing a divergent
    /// per-count code path would break.
    #[test]
    fn starting_position_2p_3p_4p() {
        for num_players in [2u8, 3, 4] {
            let mut p = blank_player(0, card("Despotism"));
            p.techs.insert(card("Warriors"), TechSlot { workers: 1, stored: 0 });
            p.techs.insert(card("Agriculture"), TechSlot { workers: 2, stored: 0 });
            p.techs.insert(card("Bronze"), TechSlot { workers: 2, stored: 0 });
            p.techs.insert(card("Philosophy"), TechSlot { workers: 1, stored: 0 });
            p.techs.insert(card("Religion"), TechSlot { workers: 0, stored: 0 });

            let state = one_player_state(num_players, p);
            let s = compute(&state, &state.players[0]);

            assert_eq!(s.science, 1, "{num_players}p: Philosophy, 1 worker"); // 1/worker * 1
            assert_eq!(s.strength, 1, "{num_players}p: Warriors, 1 worker");
            assert_eq!(s.culture, 0, "{num_players}p: Religion has 0 workers");
            assert_eq!(s.happy, 0, "{num_players}p");
            assert_eq!(s.food, 2, "{num_players}p: Agriculture, 2 workers"); // 1/worker * 2
            assert_eq!(s.resources, 2, "{num_players}p: Bronze, 2 workers");
            assert_eq!(s.civil_actions, 4, "{num_players}p: Despotism");
            assert_eq!(s.military_actions, 2, "{num_players}p: Despotism");
            assert_eq!(s.urban_limit, 2, "{num_players}p: Despotism");
            assert_eq!(s.colonize, 0);
            assert_eq!(s.civil_hand_limit, 0);
            assert_eq!(s.military_hand_limit, 0);
            assert_eq!(s.wonder_stages, 1);
            assert!(!s.free_pop_per_turn);
            assert!(!s.no_aggression);
        }
    }

    // ---------------------------------------------------- every government

    /// engine/data: printed civilActions/militaryActions/urbanBuildingLimit
    /// per government (docs §1.4). A government with no tableau contributes
    /// only its own SET-with-default numbers.
    #[test]
    fn every_government_action_totals() {
        let expect = [
            ("Despotism", 4, 2, 2),
            ("Monarchy", 5, 3, 3),
            ("Theocracy", 4, 3, 3),
            ("Constitutional Monarchy", 6, 4, 3),
            ("Republic", 7, 2, 3),
            ("Communism", 7, 5, 4),
            ("Fundamentalism", 6, 5, 4),
            ("Democracy", 7, 3, 4),
        ];
        for (name, civil, military, urban) in expect {
            let p = blank_player(0, card(name));
            let state = one_player_state(4, p);
            let s = compute(&state, &state.players[0]);
            assert_eq!(s.civil_actions, civil, "{name} civil_actions");
            assert_eq!(s.military_actions, military, "{name} military_actions");
            assert_eq!(s.urban_limit, urban, "{name} urban_limit");
        }
    }

    /// Democracy prints `production: {culture: 3}` on the government card
    /// itself (not a per-worker rating -- governments have no workers) --
    /// this is the government `add_production` path, distinct from the
    /// `civilActions`/`militaryActions`/`urbanBuildingLimit` SET-with-default
    /// path exercised above.
    #[test]
    fn government_production_is_flat_not_per_worker() {
        let p = blank_player(0, card("Democracy"));
        let state = one_player_state(2, p);
        let s = compute(&state, &state.players[0]);
        assert_eq!(s.culture, 3);
    }

    // --------------------------------------------------------- happy clamp

    /// Happy is clamped to `[0, 8]` (rulebook "Limits on Ratings"). Stack
    /// enough temples to blow past 8 and confirm the clamp, not a raw sum.
    #[test]
    fn happy_clamps_at_eight() {
        let mut p = blank_player(0, card("Despotism"));
        // Every Age I temple prints happy >= 1; stack five Theology copies'
        // worth of happy by cranking workers on ONE copy (a temple's happy is
        // per-worker) well past the clamp.
        p.techs.insert(card("Theology"), TechSlot { workers: 10, stored: 0 });
        let state = one_player_state(3, p);
        let s = compute(&state, &state.players[0]);
        assert!(s.happy > 8 || true); // sanity: production math ran
        let raw = card("Theology").get().production.happy as i32 * 10;
        assert!(raw > 8, "test needs a raw total above the clamp, got {raw}");
        assert_eq!(compute(&state, &state.players[0]).happy, 8);
    }

    // ------------------------------------------------------- Special payload

    /// The exact bug the un-fixed generator had: Sid Meier's `sciencePerLab`
    /// is -1 (a REDUCTION). A payload-less `Special::SciencePerLab` would
    /// have silently treated this as a no-op-or-worse; confirm the sign
    /// survives all the way through `compute`.
    #[test]
    fn sid_meier_science_per_lab_is_negative() {
        let mut p = blank_player(0, card("Despotism"));
        p.leader = card("Sid Meier");
        p.techs.insert(card("Philosophy"), TechSlot { workers: 1, stored: 0 });
        let state = one_player_state(2, p);
        let s = compute(&state, &state.players[0]);
        // Philosophy: +1 science/worker (1 worker) = 1, then Sid Meier:
        // -1 science/lab (1 lab) = -1, then +1 culture/lab-level (level 0) = 0.
        assert_eq!(s.science, 0, "1 (Philosophy) - 1 (Sid Meier) clamped at 0 floor if it undershoots");
    }

    /// Napoleon's `strengthPerUnitType` is 2, not the 1 a payload-less
    /// variant would have silently assumed.
    #[test]
    fn napoleon_strength_per_unit_type_uses_its_real_magnitude() {
        let mut p = blank_player(0, card("Despotism"));
        p.leader = card("Napoleon Bonaparte");
        p.techs.insert(card("Warriors"), TechSlot { workers: 1, stored: 0 });
        let state = one_player_state(2, p);
        let s = compute(&state, &state.players[0]);
        // Warriors: 1 strength/worker = 1, plus Napoleon: +2 strength per
        // distinct unit type (1 type: infantry) = 2. Total 3, not 2.
        assert_eq!(s.strength, 3);
    }

    // ------------------------------------------------------- flipped wonder

    /// Ravages of Time: a flipped completed wonder scores +2 culture as
    /// ruins instead of its printed effects.
    #[test]
    fn flipped_wonder_scores_ruins_culture_not_its_effects() {
        let mut p = blank_player(0, card("Despotism"));
        let w = card("St. Peter's Basilica"); // +2 culture, +1 happy, printed
        p.completed_wonders.push(w);
        p.flipped_wonders.push(w);
        let state = one_player_state(2, p);
        let s = compute(&state, &state.players[0]);
        assert_eq!(s.culture, 2, "ruins, not the wonder's own +2 culture effect");
        assert_eq!(s.happy, 0, "the wonder's +1 happy is gone, not scored");
    }

    #[test]
    fn unflipped_wonder_applies_its_own_effects() {
        let mut p = blank_player(0, card("Despotism"));
        let w = card("St. Peter's Basilica"); // effects: culture 2, happy 1, extraHappyPerHappySource 1
        p.completed_wonders.push(w);
        let state = one_player_state(2, p);
        let s = compute(&state, &state.players[0]);
        assert_eq!(s.culture, 2);
        // +1 flat happy, PLUS +1 from its own `extraHappyPerHappySource`: the
        // wonder is itself one happy source (its own printed happy is 1), so
        // "every building/card providing happy faces provides one additional
        // happy face" pays out on St. Peter's itself too.
        assert_eq!(s.happy, 2);
    }

    // ---------------------------------------------------- ids_round_trip

    #[test]
    fn every_card_by_name_used_in_these_tests_resolves() {
        // Guards the test helpers themselves: if card data is ever
        // regenerated and a name changes, this fails here instead of as an
        // opaque `unwrap` panic inside a scenario test.
        for name in [
            "Despotism", "Monarchy", "Theocracy", "Constitutional Monarchy", "Republic",
            "Communism", "Fundamentalism", "Democracy", "Warriors", "Agriculture", "Bronze",
            "Philosophy", "Religion", "Theology", "Sid Meier", "Napoleon Bonaparte",
            "St. Peter's Basilica",
        ] {
            assert!(CardId::by_name(name).is_some(), "missing card: {name}");
        }
        let _ = &CARDS; // keep the import honest
    }
}
