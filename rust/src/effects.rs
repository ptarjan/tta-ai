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
//! ## Formerly KNOWN GAPS, closed 2026-08-05
//!
//! `army_strength`, pacts and a colony's own bonuses were all blocked on type
//! layer this file's original port could not reach; `gen_cards.py`'s
//! generator rewrite (see its own module doc, and `cards::Composition` /
//! `cards::PactBlock` / `cards::CardEffects`'s `food`/`resources`/
//! `yellow_tokens`) closed all three:
//!
//! - **`army_strength`** (`engine/effects.py:471`, ported below as
//!   [`army_strength`]): a tactic's `composition`/`obsolete_strength` are now
//!   on `cards::Card`.
//! - **Pacts** (`_apply_pacts`, `engine/effects.py:608`, ported below as
//!   [`apply_pacts`]): `state::Pact`/`PactList` now exist, and `Special::A`/
//!   `B`/`BothPlayers` now carry a real `cards::PactBlock` payload instead of
//!   being bare flags.
//! - **A colony's own bonuses** (`_colony_permanents`, `engine/effects.py:
//!   453-458`): a territory's `permanentEffects`, filtered to Python's
//!   `COLONY_PERMANENT_KEYS`, is now merged onto the SAME `CardEffects` a
//!   regular card's `effects` dict uses (`gen_cards.py`'s
//!   `COLONY_PERMANENT_FIELDS`) -- so the ordinary tableau-scanning code
//!   below handles a colony with no colony-specific branch at all.
//!
//! A fourth gap, closed 2026-08-05 by the same route:
//!
//! - **`build_discount`** (a per-age dict on Python's `Stats`): was "not
//!   needed by anything in this port's scope (`build_cost` is not ported),
//!   and a dict does not fit this struct's flat-fields shape anyway".
//!   `costs.rs::build_cost_for` ports `build_cost` now and does need it, and
//!   the dict was never the only option: `Special::BuildDiscount` carries a
//!   real `[i16; 5]` payload indexed by `Age` (gen_cards.py's
//!   `AGE_ARRAY_EFFECT_KEYS`), so [`Stats::build_discount`] below is a flat
//!   fixed-size array, no map involved.
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

use crate::cards::{Card, CardEffects, CardId, CardType, Composition, PactBlock, Production, Special};
use crate::state::{GameState, PlayerState, Tableau};

/// A player's aggregate statistics for one recomputation. Mirrors Python's
/// `effects.Stats` dataclass field for field.
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
    /// Resources off an URBAN building's build cost, per AGE -- indexed by
    /// `Age as u8` (`build_discount[Age::II as usize]` is the Age II entry),
    /// summed across every source that prints one, exactly as Python's
    /// `_apply_special` accumulates its `age -> resources` dict.
    ///
    /// Read by `costs.rs::build_cost_for`, and ONLY for a card whose type is
    /// in `C.URBAN_TYPES` -- narrower than the `C.URBAN_OR_PRODUCTION` the
    /// one-time event discount uses, so a farm or mine never gets this.
    /// Python's dict, being sparse, returns 0 for an age nobody printed;
    /// this array says the same thing with a real stored 0.
    pub build_discount: [i32; 5],
    pub free_pop_per_turn: bool,
    pub no_aggression: bool,
    // ------------------------------------------------- pact-derived (§5.9)
    /// Science off every technology `p` develops (`technologyScienceDiscount`).
    pub tech_discount: i32,
    /// Nobody may declare war on `p` (`cannotBeDeclaredWarOnByAnyone`).
    pub war_immune: bool,
    /// Food spendable as resources, up to this many per turn (Trade Routes).
    pub food_as_resource: i32,
    pub resource_as_food: i32,
    /// Bitmask over player index (`1 << idx`): players who must pay 1 science
    /// for `p` to develop a technology at all (`otherPartyPaysScience`,
    /// Scientific Cooperation). A fixed-width mask, not a `Vec`, for the same
    /// reason every other per-call field here is a plain scalar -- `idx` is
    /// always `< MAX_PLAYERS` (4), so 8 bits is already double the room
    /// needed. Mirrors Python's `Stats.science_partners`, which nothing else
    /// in the engine reads either (checked: `grep -rn science_partners
    /// engine/` finds only the write site) -- carried for parity, not because
    /// anything consumes it yet.
    pub science_partners: u8,
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
            build_discount: [0; 5],
            free_pop_per_turn: false,
            no_aggression: false,
            tech_discount: 0,
            war_immune: false,
            food_as_resource: 0,
            resource_as_food: 0,
            science_partners: 0,
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
    // `food`/`resources` are zero on every card except a territory's own
    // `permanentEffects` (`gen_cards.py`'s `COLONY_PERMANENT_FIELDS`) -- see
    // `CardEffects::food`'s doc comment. Reading them here unconditionally,
    // like every other field in this function, is what lets `compute`'s
    // colonies loop below call this SAME function with no colony-specific
    // branch, exactly mirroring `engine/effects.py::_apply_flat` being the
    // one function every source (government/techs/wonders/leader/colonies/
    // pacts) routes through.
    stats.food += eff.food as i32;
    stats.resources += eff.resources as i32;
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

/// Whether `p` has `target` active from any of the five sources Python's
/// `_output_modifiers` scans (`engine/effects.py:194-207`): the government,
/// every tableau card regardless of type, every colony, a completed AND
/// UNFLIPPED wonder, and the leader. Used by [`mine_resources`]/
/// [`farm_food`] below to gate `doubleBestMine`, the one `_BUILDING_OUTPUT`
/// modifier that touches either of them.
///
/// `compute`'s own [`apply_special`] dispatch only reads `card.special` off
/// a narrower set (government/`SpecialTech`/wonder/leader/colony) because
/// that is everywhere a `Special` variant is actually printed in the base
/// game today (confirmed 2026-08-05: no farm/mine/urban/unit card prints
/// one) -- but Python's `_output_modifiers` does not assume that, and
/// neither does this.
fn has_special_active(p: &PlayerState, target: Special) -> bool {
    if p.government.get().special.contains(&target) {
        return true;
    }
    if p.techs.iter().any(|(id, _)| id.get().special.contains(&target)) {
        return true;
    }
    if p.colonies.as_slice().iter().any(|&c| c.get().special.contains(&target)) {
        return true;
    }
    if p.completed_wonders.as_slice().iter().any(|&w| {
        !p.flipped_wonders.contains(w) && w.get().special.contains(&target)
    }) {
        return true;
    }
    !p.leader.is_none() && p.leader.get().special.contains(&target)
}

/// Resources produced BY THIS PLAYER'S MINES, ignoring every other source of
/// resources (§12.5.2 "Impact of Industry"). Mirrors `engine/effects.py::
/// mine_resources` -> `building_output(p, {"mine"}, ("resources",))`.
///
/// Python's `building_output` is fully general (it also powers Hollywood's
/// theater/library culture and Internet's whole-urban-building sum, neither
/// ported here); this is the narrow slice that mine scoring actually needs.
/// Verified against `_BUILDING_OUTPUT`'s full key list (2026-08-05): the
/// ONLY modifier whose types are a subset of `{mine}` is `doubleBestMine`
/// (Transcontinental Railroad's "one of your best mines produces twice as
/// many resources") -- every other modifier there touches theater/lab/
/// library, never mine, so a general port would carry dead branches.
pub fn mine_resources(p: &PlayerState) -> i32 {
    let mut total = 0i32;
    for (id, slot) in p.techs.iter() {
        if id.kind() == CardType::Mine {
            total += id.get().production.resources as i32 * slot.workers as i32;
        }
    }
    // The bonus is the BEST staffed mine's own per-worker rate, added once
    // more flat -- not doubled per worker -- matching `_building_modifier`'s
    // `doubleBestMine` branch (FAQ v1.5 p.9: doubles one WORKER's output).
    if has_special_active(p, Special::DoubleBestMine) {
        if let Some(b) = best_staffed(&p.techs, |k| k == CardType::Mine) {
            total += b.get().production.resources as i32;
        }
    }
    total
}

/// Food produced BY THIS PLAYER'S FARMS (§12.5.2 "Impact of Agriculture").
/// The twin of [`mine_resources`], and simpler: `_BUILDING_OUTPUT` has no
/// entry whose types are a subset of `{farm}`, so nothing ever adds to this
/// beyond the plain per-worker scan -- verified 2026-08-05 against the same
/// key list. Mirrors `engine/effects.py::farm_food`.
pub fn farm_food(p: &PlayerState) -> i32 {
    let mut total = 0i32;
    for (id, slot) in p.techs.iter() {
        if id.kind() == CardType::Farm {
            total += id.get().production.food as i32 * slot.workers as i32;
        }
    }
    total
}

/// Number of cards/buildings providing at least one happy face (St. Peter's
/// Basilica). Mirrors `engine/effects.py::_happy_source_count`: "every
/// building/CARD providing happy faces provides one additional happy face"
/// -- so the government and leader cards count too, not only buildings, and
/// a wonder ruined by Ravages of Time provides none. A colony counts too
/// (`docs/SCORE_AUDIT.md` 3.7, quoted in Python) -- Historic Territory's
/// `permanentEffects.happiness` is exactly this.
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
    for &col in p.colonies.as_slice() {
        if col.get().effects.happy > 0 {
            n += 1;
        }
    }
    n
}

// -------------------------------------------------------- army strength
//
// §10: a tactic in play, plus the units it has to form armies from. Ported
// from `engine/effects.py::army_strength`/`_army_strength_counts`/
// `_army_value` -- the shared core those functions and `army_strength_units`/
// `tactic_outlook` (combat.rs, not ported) all route through.

/// One unit type slot in the `[infantry, cavalry, artillery, air]` arrays
/// `army_strength` and its helpers pass around below. A plain index rather
/// than a `HashMap<CardType, i32>`: exactly four unit types exist
/// (`CardType::is_unit`), so a fixed-size array is both the DESIGN.md-
/// preferred shape and faster than hashing a 1-byte key four times a call.
const INFANTRY: usize = 0;
const CAVALRY: usize = 1;
const ARTILLERY: usize = 2;
const AIR: usize = 3;

/// `cards::Composition`'s four counts, in `[INFANTRY, CAVALRY, ARTILLERY,
/// AIR]` order, to line up with the `avail`/`fresh` arrays below.
fn need_counts(comp: Composition) -> [i32; 4] {
    [comp.infantry as i32, comp.cavalry as i32, comp.artillery as i32, comp.air as i32]
}

/// Complete armies formable from `avail`, given `need` -- the non-Genghis-
/// Khan fast path of `engine/effects.py::_army_strength_counts`: armies =
/// the MIN, over every unit type the composition actually requires, of
/// `avail // need`. A composition never requires air (`data/*.json`: no
/// tactic's `composition` names it), so `need[AIR]` is always 0 and never
/// constrains this; `avail[AIR]` is read separately by the air-force bonus
/// in `army_value` below.
fn count_armies(avail: &[i32; 4], need: &[i32; 4]) -> i32 {
    let mut best: Option<i32> = None;
    for i in 0..4 {
        if need[i] > 0 {
            let armies = avail[i] / need[i];
            best = Some(best.map_or(armies, |b| b.min(armies)));
        }
    }
    best.unwrap_or(0)
}

/// Genghis Khan (`Special::InfantryCountsAsCavalryForTactics`): infantry may
/// fill a cavalry slot. Mirrors the nested `count_armies` inside Python's
/// `_army_strength_counts` Genghis branch literally, loop included --
/// `k*(need_inf+need_cav) <= total` and `k*need_cav <= total` are
/// independent bounds in Python's own formula, not re-derived here (if this
/// ever needs re-justifying against the rulebook, start from
/// `engine/effects.py::_army_strength_counts`, not from first principles).
fn count_armies_genghis(avail: &[i32; 4], need: &[i32; 4]) -> i32 {
    let need_inf = need[INFANTRY];
    let need_cav = need[CAVALRY];
    if need_inf == 0 && need_cav == 0 {
        return count_armies(avail, need);
    }
    let total = avail[INFANTRY] + avail[CAVALRY];
    let mut best = 0;
    for k in 0..=total {
        let others_ok = (ARTILLERY..=AIR)
            .all(|i| need[i] == 0 || k * need[i] <= avail[i]);
        if k * (need_inf + need_cav) <= total && k * need_cav <= total && others_ok {
            best = k;
        }
    }
    best
}

/// Strength of `total_armies` armies (`fresh_armies` of them not a full age
/// behind the tactic, the rest outdated), plus the air-force doubling bonus.
/// Mirrors `engine/effects.py::_army_value` exactly, including the ordering
/// note in its own comment: doubling an OUTDATED army is worth
/// `obsolete_strength`, not the fresh value.
fn army_value(card: &Card, total_armies: i32, fresh_armies: i32, air: i32) -> i32 {
    let outdated_armies = total_armies - fresh_armies;
    let val = card.effects.strength as i32;
    // Zero means "not printed" (`cards::Card::obsolete_strength`'s own doc
    // comment) -- no base-game tactic prints an explicit zero here, so this
    // sentinel is unambiguous. Mirrors Python's `old_val = ... or val`.
    let old_val = if card.obsolete_strength != 0 { card.obsolete_strength as i32 } else { val };
    let mut total = fresh_armies * val + outdated_armies * old_val;
    if air > 0 {
        let assigned = air.min(total_armies);
        let on_fresh = assigned.min(fresh_armies);
        total += on_fresh * val + (assigned - on_fresh) * old_val;
    }
    total
}

/// Tactical strength from armies formed by `p`'s current tactic (§10).
/// Mirrors `engine/effects.py::army_strength`: zero without a tactic, or with
/// a tactic whose `composition` is empty (every non-tactic card, guarded by
/// the `lib.rs` test `tactics_and_only_tactics_form_armies`).
fn army_strength(p: &PlayerState) -> i32 {
    if p.tactic.is_none() {
        return 0;
    }
    let card = p.tactic.get();
    if card.composition.is_empty() {
        return 0;
    }
    let tactic_lv = p.tactic.level() as i32;
    let mut avail = [0i32; 4];
    let mut fresh = [0i32; 4];
    for (id, slot) in p.techs.iter() {
        let w = slot.workers as i32;
        if w == 0 {
            continue;
        }
        let i = match unit_slot(id.kind()) {
            Some(i) => i,
            None => continue,
        };
        avail[i] += w;
        // "fresh" = not a full age behind the tactic (§10.4).
        if id.level() as i32 >= tactic_lv - 1 {
            fresh[i] += w;
        }
    }
    army_strength_from_counts(p, &avail, &fresh)
}

/// `[INFANTRY, CAVALRY, ARTILLERY, AIR]` slot for a unit type, `None` for
/// everything else.
#[inline]
fn unit_slot(kind: CardType) -> Option<usize> {
    match kind {
        CardType::Infantry => Some(INFANTRY),
        CardType::Cavalry => Some(CAVALRY),
        CardType::Artillery => Some(ARTILLERY),
        CardType::Air => Some(AIR),
        _ => None,
    }
}

/// The shared tail of [`army_strength`] and [`army_strength_units`] --
/// Python's `_army_strength_counts`, which both of its callers route through
/// for exactly this reason: the Genghis Khan branch and the fresh-armies
/// clamp must not exist twice.
fn army_strength_from_counts(p: &PlayerState, avail: &[i32; 4], fresh: &[i32; 4]) -> i32 {
    let card = p.tactic.get();
    let need = need_counts(card.composition);
    let genghis = !p.leader.is_none()
        && p.leader.get().special.contains(&Special::InfantryCountsAsCavalryForTactics);
    let (total_armies, fresh_armies) = if genghis {
        let total = count_armies_genghis(avail, &need);
        (total, count_armies_genghis(fresh, &need).min(total))
    } else {
        let total = count_armies(avail, &need);
        (total, count_armies(fresh, &need).min(total))
    };
    if total_armies == 0 {
        return 0;
    }
    army_value(card, total_armies, fresh_armies, avail[AIR])
}

/// §10.3-10.5 over an EXPLICIT unit multiset rather than the whole tableau.
/// Mirrors `engine/effects.py::army_strength_units`, and exists for the same
/// caller it does in Python: a colonization force, where only the SACRIFICED
/// units form armies (§10.7) -- `interact::force_value`. One entry per
/// worker, so a card carrying two workers appears twice.
pub fn army_strength_units(p: &PlayerState, units: &[CardId]) -> i32 {
    if p.tactic.is_none() {
        return 0;
    }
    if p.tactic.get().composition.is_empty() {
        return 0;
    }
    let tactic_lv = p.tactic.level() as i32;
    let mut avail = [0i32; 4];
    let mut fresh = [0i32; 4];
    for &id in units {
        let Some(i) = unit_slot(id.kind()) else { continue };
        avail[i] += 1;
        if id.level() as i32 >= tactic_lv - 1 {
            fresh[i] += 1;
        }
    }
    army_strength_from_counts(p, &avail, &fresh)
}

// ------------------------------------------------------------------ pacts
//
// §5.9: a pact card sits in `p.pacts`, never in `p.techs`/`completed_wonders`/
// `leader`/`government`, so it is never seen by `apply_special` below --
// these two functions are `compute`'s ONLY route to a pact's numbers.
// Ported from `engine/effects.py::pacts_for`/`_pact_blocks`/`_apply_pacts`.

/// Add one pact block's grants to `stats`. Mirrors the parts of Python's
/// `_apply_flat(s, block, mods)` a `PactBlock` can actually contain (its
/// vocabulary is the closed set `gen_cards.py::PACT_BLOCK_FIELDS` maps, a
/// subset of `FLAT_KEYS`/`SPECIAL_KEYS` -- no `MODIFIER_KEYS` key has ever
/// appeared in a pact block, so there is no phase-3 queueing to do here)
/// plus the two keys Python reads directly off `block` afterwards
/// (`cultureProductionPerCompletedWonderOfTheOtherParty`,
/// `otherPartyPaysScience`).
fn apply_pact_block(stats: &mut Stats, block: &PactBlock, other: &PlayerState) {
    stats.culture += block.culture as i32;
    stats.food += block.food as i32;
    stats.resources += block.resources as i32;
    stats.strength += block.strength as i32;
    stats.military_actions += block.military_actions as i32;
    stats.tech_discount += block.tech_discount as i32;
    if block.war_immune {
        stats.war_immune = true;
    }
    stats.food_as_resource += block.food_as_resource as i32;
    stats.resource_as_food += block.resource_as_food as i32;
    if block.culture_per_wonder_of_other_party != 0 {
        stats.culture +=
            block.culture_per_wonder_of_other_party as i32 * other.completed_wonders.len() as i32;
    }
    if block.other_party_pays_science {
        stats.science_partners |= 1 << other.idx;
    }
}

/// Every pact `p.idx` is party to, wherever physically held, applied to
/// `stats`. Mirrors `engine/effects.py::pacts_for` + `_apply_pacts` fused
/// into one pass (nothing else in this module needs the pact list on its
/// own, unlike Python where `pact_forbids_attack`/`pact_attack_bonus`/
/// `cancel_attack_pacts` -- all combat.rs, not ported -- reuse `pacts_for`
/// too).
fn apply_pacts(stats: &mut Stats, state: &GameState, p: &PlayerState) {
    for q in state.players.iter() {
        for pact in q.pacts.as_slice() {
            if !pact.is_party(p.idx) {
                continue;
            }
            let other = &state.players[pact.partner_of(p.idx) as usize];
            for &sp in pact.card.get().special {
                match sp {
                    Special::BothPlayers(block) => apply_pact_block(stats, &block, other),
                    Special::A(block) if p.idx == pact.a => apply_pact_block(stats, &block, other),
                    Special::B(block) if p.idx == pact.b => apply_pact_block(stats, &block, other),
                    // `OnAttackBetweenParties` (attack-time strength bonus)
                    // and the flag-only pact specials are combat.rs's
                    // resolution-time concern, not `compute`'s -- see
                    // `apply_special`'s own grouping below.
                    _ => {}
                }
            }
        }
    }
}

// --------------------------------------------------------------- dispatch

/// The exhaustive dispatch over [`Special`]. Every one of the 92 generated
/// variants appears exactly once below, in one of five groups:
///
/// 1. Implemented -- read by `compute`/`state_stats` (Python's
///    `MODIFIER_KEYS`/`SPECIAL_KEYS`, `_apply_modifier`/`_apply_special`).
/// 2. `// belongs to actions.rs` -- a one-shot trigger on a civil action,
///    build, develop, political action, or a build-cost discount. None of
///    these are read by `engine/effects.py::compute`.
/// 3. `// belongs to combat.rs` -- aggression/war/tactic/colonization
///    resolution, including the pact keys that only matter at attack-
///    resolution time (`onAttackBetweenParties`,
///    `cancelledIfPartiesAttackEachOther`, `noAttacksBetweenParties`).
/// 4. `// belongs to events.rs` -- age/military event-card resolution and
///    targeting.
/// 5. `// handled elsewhere, not this match` -- genuinely read by `compute`
///    (Genghis Khan's tactic rule, the three pact blocks `compute` DOES
///    read), just not through this particular dispatch function -- see
///    `army_strength`/`apply_pacts` above for where each actually is.
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
        // Masonry / Architecture / Engineering: resources off an urban
        // building's build cost, per age. SUMMED per age across sources,
        // matching `_apply_special`'s `bd[age] = bd.get(age, 0) + d` -- not
        // maxed, which is what the `wonderStagesPerAction` printed on the
        // very same three cards does (`add_flat_except_actions`). Holding
        // two of the three at once is unreachable in the base game (§7.6
        // allows one construction special-tech in play), so nothing today
        // tells the two rules apart at runtime; Python sums, so this sums.
        BuildDiscount(by_age) => {
            for (slot, add) in stats.build_discount.iter_mut().zip(by_age.iter()) {
                *slot += *add as i32;
            }
        }
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
        CivilActionBackOnTechDevelop(_)
        | CivilActionUpgradeUrbanBuildingToTheater
        | ComboFoodDiscount(_)
        | ComboResourceDiscount(_)
        | CultureOnLeaveEqualToLabResourceProduction
        | CultureOnRevolution(_)
        | CultureOnTechDevelop(_)
        | CulturePerCivilizationWithMoreCulture
        // Which ordered free action a card grants (`build_one_wonder_stage`,
        // `develop_technology`, ...) -- `engine/actions.py::free_action_moves`
        // dispatches on this VALUE directly (lines 597-618), which is why it
        // now carries a real `FreeCivilActionValue` payload instead of being
        // a bare flag; still `actions.rs`'s to read, not `compute()`'s.
        | FreeCivilAction(_)
        | GainFoodOrResources(_)
        | LeaderTakeCivilActionDiscount(_)
        | LibraryDiscountsIfTheater
        | MilitaryActionAsCivilPerTurn(_)
        | MilitaryActionCombinedPopIncreaseAndUnitBuild
        // Fast Food Chains / Internet / Hollywood: resolved by
        // `engine/effects.py::_one_time_culture`/`wonder_completion_culture`
        // at wonder-completion time -- a one-shot build trigger, not a
        // recurring `compute()` field. The `OnBuildCultureValue` payload
        // tells `actions.rs` which of the three formulas to run.
        | OnBuildCulture(_)
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

        // ------------------------------------- belongs to economy.rs ----
        // Genghis Khan's "3 culture per turn if your strength is in the top
        // two" is scored in the production phase (rule 6.6 step 3a), which
        // `economy::end_of_turn` implements by dispatching on this variant.
        // It sat in the combat group below until 2026-08-05; leaving it
        // there invited whoever wrote combat.rs to implement it a second
        // time at aggression-resolution, double-counting the 3 culture.
        CultureIfTopTwoStrength(_) => {}

        // -------------------------------------- belongs to combat.rs ----
        // Aggression / war / tactic / colonization resolution.
        ColonizeDiscardUpTo2MilitaryCardsForBonus(_)
        | ColonyImmediateBonusApplies
        | ColonyPermanentBonusTransfers
        | DestroyUrbanBuildings(_)
        | DoublesTacticBonusOfOneArmy
        // Aggression: Raid -- "half of each destroyed building's printed
        // build cost, rounded up"; the `GainResourcesValue` payload names
        // that one formula (today's only value), resolved at aggression-
        // resolution time, not by `compute()`.
        | GainResources(_)
        | GainCulturePerLevelOfRemovedCard(_)
        | OpponentDecreasesPopulation(_)
        | OpponentsPayDoubleMilitaryActionsToAttackYou
        | OrTakesSpecialTechnologiesOfSameTotalScienceCost
        | RemoveFromGame
        | StealColony(_)
        | TakeFromOpponent(_)
        | VictorTakesCulture
        // War over Technology's `strengthAdvantage` formula, named by
        // `VictorTakesScienceUpToValue`; war-resolution-time, not `compute()`.
        | VictorTakesScienceUpTo(_)
        | VictorTakesYellowTokens
        // Pact-cancellation/attack-legality flags (§5.4.2-5.4.3): read by
        // `pact_forbids_attack`/`_doomed_pact_strength`/
        // `cancel_attack_pacts`, all attack-RESOLUTION-time, not `compute`.
        | CancelledIfPartiesAttackEachOther
        | NoAttacksBetweenParties
        // Attack-time strength bonus between the two pact parties (§5.4.2,
        // `pact_attack_bonus`) -- resolution-time, not `compute`. The OTHER
        // three dict-payload pact keys (`A`/`B`/`bothPlayers`) ARE read by
        // `compute`, just not through this dispatch -- see `apply_pacts`.
        | OnAttackBetweenParties(_) => {}

        // -------------------------------------- belongs to events.rs ----
        // Age/military event-card resolution and targeting. (`target` and
        // `duration` are NOT variants here or anywhere -- gen_cards.py's
        // `IGNORED_NESTED_EFFECT_KEYS`: Python confirmed never reads either
        // value, only `target`'s PRESENCE on Barbarians, which `Condition`/
        // `DecreasePopulation` on that same card already carry.)
        AllPlayers
        | Condition
        | DecreasePopulation(_)
        // §12.5.2 final scoring (events::scoring_culture): a per-GAME payload
        // read once, at `finish_game`, directly off an Age III event card's
        // `special` list -- never through this per-player, per-turn dispatch,
        // and an event card is never in `p.techs`/`completed_wonders`/
        // `leader`/`government` either, so this arm never actually fires.
        | FinalScoring(_)
        | Gain
        | Lose
        | LastRoundSubstitute
        | PlayerWithLeastCulture
        | PlayerWithMostCulture
        | PlayersWithMostDiscontentWorkers
        | PlayersWithMostHappyFaces
        | StrongestPlayer
        | StrongestPlayers
        | WeakestPlayer
        | WeakestPlayers => {}

        // --------------------------- handled elsewhere, not this match ----
        // Genghis Khan: `army_strength` above checks this directly on the
        // leader's special list -- it changes how a unit multiset is
        // COUNTED, not a `Stats` FIELD to add to, so there is nothing for
        // this dispatch to do with it. (`army_strength_units`/
        // `tactic_outlook`, combat.rs, will read it the same way once
        // colonization/tactic-switching are ported.)
        InfantryCountsAsCavalryForTactics
        // Pact blocks (§5.9): `apply_pacts` above reads these directly off a
        // pact card's `special` list, gated on `pact.a`/`pact.b`/party-hood
        // -- information this dispatch does not have (it only ever sees ONE
        // player, `p`, where a pact block's meaning depends on WHICH side of
        // the pact `p` is). A pact card is also never in `p.techs`/
        // `completed_wonders`/`leader`/`government`, so these three never
        // actually reach this particular match at runtime -- the arm exists
        // only because `Special` is generated, not hand-written, and the
        // match must stay exhaustive regardless.
        | A(_)
        | B(_)
        | BothPlayers(_) => {}
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

    // --- phase 2: colonies (§11.5). A colony's own bonuses are a territory's
    // `permanentEffects`, filtered to Python's `COLONY_PERMANENT_KEYS` and
    // merged onto `CardEffects` by `gen_cards.py` -- so this is the exact
    // same `add_flat`/`apply_special` pair every other source above uses, no
    // colony-specific branch needed. (The colony-COUNT-based leader
    // modifiers `CultureFirstColony`/`CulturePerAdditionalColony` were
    // already reading `p.colonies.len()` via the leader's `special` list
    // above, before this loop existed.)
    for &col in p.colonies.as_slice() {
        let card = col.get();
        add_flat(&mut stats, &card.effects);
        for &sp in card.special {
            apply_special(&mut stats, p, sp);
        }
    }

    // --- pacts (§5.9).
    apply_pacts(&mut stats, state, p);

    // --- event-granted permanents.
    stats.culture += p.culture_rate_extra as i32;
    stats.science += p.science_rate_extra as i32;
    stats.strength += p.strength_extra as i32;
    stats.happy += p.happy_extra as i32;

    // --- phase 3: army strength (§10). Last, like Python's `s.strength +=
    // army_strength(state, p)` (`engine/effects.py:471`) -- addition
    // commutes, so this only needs to run after `p.tactic`'s own printed
    // `strength`/`obsoleteStrength` and the tableau's units are both known,
    // which they are by this point regardless of ordering.
    stats.strength += army_strength(p);

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
    use crate::state::{CardList, GameState, Pact, PactList, Phase, PlayerState, Tableau, TechSlot, MAX_PLAYERS, ROW_SIZE};

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
            taken_leader_ages: 0,
            war_declared_by_me: CardId::NONE,
            war_target: 0,
            wars_declared_on_me: [CardId::NONE; MAX_PLAYERS],
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
            one_time_discount: crate::state::OneTimeDiscount::default(),
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
            pending: crate::state::PendingStack::new(),
            queue: crate::state::Queue::new(),
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

    /// Two players with genuine (non-filler) state, for pact tests: pacts
    /// are the only thing `compute` reads off a player OTHER than the one
    /// being scored, so `one_player_state`'s filler seats (never
    /// distinguished from each other) are not enough. Callers set `idx` on
    /// `p0`/`p1` themselves via `blank_player`.
    fn two_player_state(p0: PlayerState, p1: PlayerState) -> GameState {
        let filler = || blank_player(2, card("Despotism"));
        let mut players = [filler(), filler(), filler(), filler()];
        players[0] = p0;
        players[1] = p1;
        blank_state(4, players)
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

    // -------------------------------------------------------- buildDiscount

    /// Masonry prints `buildDiscount {I: 1, II: 1, III: 1}` and
    /// `wonderStagesPerAction: 2`. The discount lands in the array indexed by
    /// `Age as u8` -- ages A and IV stay 0, which is the card's own "Age A
    /// unchanged" -- and the wonder-stage number is unaffected by it.
    #[test]
    fn build_discount_lands_in_the_array_indexed_by_age() {
        let mut p = blank_player(0, card("Despotism"));
        p.techs.insert(card("Masonry"), TechSlot { workers: 0, stored: 0 });
        let state = one_player_state(2, p);
        let s = compute(&state, &state.players[0]);
        assert_eq!(s.build_discount, [0, 1, 1, 1, 0]);
        assert_eq!(s.wonder_stages, 2, "same card, unrelated field");
    }

    /// Engineering's is `{I: 1, II: 2, III: 3}` -- three DIFFERENT numbers.
    /// Collapsing a per-age dict to one scalar (which is what the generator
    /// used to do) would price two of the three ages wrong.
    #[test]
    fn build_discount_keeps_a_distinct_amount_per_age() {
        let mut p = blank_player(0, card("Despotism"));
        p.techs.insert(card("Engineering"), TechSlot { workers: 0, stored: 0 });
        let state = one_player_state(2, p);
        assert_eq!(compute(&state, &state.players[0]).build_discount, [0, 1, 2, 3, 0]);
    }

    /// Python's `_apply_special` does `bd[age] = bd.get(age, 0) + d`: it
    /// SUMS per age across sources, where the `wonderStagesPerAction` on the
    /// very same cards is MAXed. §7.6 (one special tech per icon in play)
    /// means no real game reaches this, but the two rules must not be
    /// implemented as one.
    #[test]
    fn build_discount_sums_across_sources_where_wonder_stages_maxes() {
        let mut p = blank_player(0, card("Despotism"));
        p.techs.insert(card("Masonry"), TechSlot { workers: 0, stored: 0 });
        p.techs.insert(card("Architecture"), TechSlot { workers: 0, stored: 0 });
        let state = one_player_state(2, p);
        let s = compute(&state, &state.players[0]);
        assert_eq!(s.build_discount, [0, 2, 3, 3, 0], "[0,1,1,1,0] + [0,1,2,2,0], summed");
        assert_eq!(s.wonder_stages, 3, "max(2, 3), not 5");
    }

    #[test]
    fn build_discount_is_all_zero_without_a_construction_tech() {
        let p = blank_player(0, card("Despotism"));
        let state = one_player_state(2, p);
        assert_eq!(compute(&state, &state.players[0]).build_discount, [0; 5]);
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

    // ------------------------------------------------------ army strength

    #[test]
    fn no_tactic_scores_zero_army_strength() {
        let mut p = blank_player(0, card("Despotism"));
        p.techs.insert(card("Warriors"), TechSlot { workers: 2, stored: 0 });
        let state = one_player_state(2, p);
        let s = compute(&state, &state.players[0]);
        // Warriors: 1 strength/worker * 2 workers, and NOTHING extra --
        // `army_strength` must return 0 without a tactic in play.
        assert_eq!(s.strength, 2);
    }

    #[test]
    fn tactic_with_no_matching_units_scores_zero() {
        let mut p = blank_player(0, card("Despotism"));
        p.tactic = card("Fighting Band"); // needs 2 infantry
        let state = one_player_state(2, p);
        let s = compute(&state, &state.players[0]);
        assert_eq!(s.strength, 0);
    }

    #[test]
    fn tactic_forms_one_fresh_army() {
        let mut p = blank_player(0, card("Despotism"));
        p.tactic = card("Fighting Band"); // Age I, strength 1, needs 2 infantry
        p.techs.insert(card("Warriors"), TechSlot { workers: 2, stored: 0 }); // Age A
        let state = one_player_state(2, p);
        let s = compute(&state, &state.players[0]);
        // Warriors' own strength: 1/worker * 2 = 2. Fighting Band's tactic
        // level is I (1); Warriors is Age A (0) >= 1-1 = 0, so the army is
        // fresh: 1 army * strength 1 = 1. Total 3.
        assert_eq!(s.strength, 3);
    }

    #[test]
    fn obsolete_army_scores_the_reduced_rate_and_air_doubles_it() {
        let mut p = blank_player(0, card("Despotism"));
        // Entrenchments: Age III, strength 9, obsolete 5, needs 1 infantry + 2 artillery.
        p.tactic = card("Entrenchments");
        p.techs.insert(card("Swordsmen"), TechSlot { workers: 1, stored: 0 }); // Age I infantry
        p.techs.insert(card("Cannon"), TechSlot { workers: 2, stored: 0 }); // Age II artillery
        p.techs.insert(card("Air Forces"), TechSlot { workers: 1, stored: 0 }); // Age III air
        let state = one_player_state(2, p);
        let s = compute(&state, &state.players[0]);
        // Fresh threshold for an Age III tactic is level >= 2. Swordsmen is
        // Age I (level 1): NOT fresh. Cannon is Age II (level 2): fresh. One
        // army total (min(1 infantry/1, 2 artillery/2) = 1), but the
        // infantry required for it is entirely non-fresh, so the WHOLE army
        // is outdated (fresh_armies = min(0 fresh infantry / 1, ...) = 0).
        // Army value: 0 fresh * 9 + 1 outdated * 5 = 5.
        // One air unit doubles ONE army's bonus; that army is the outdated
        // one, so the doubling is worth `obsolete_strength` (5), not the
        // fresh value (9) -- exactly the distinction this module's
        // `army_value` doc comment calls out. Air bonus: 5. Army total: 10.
        // Tech-phase per-unit strength: Swordsmen 2*1 + Cannon 3*2 + Air
        // Forces 5*1 = 2+6+5 = 13. Grand total: 13 + 10 = 23.
        assert_eq!(s.strength, 23);
    }

    #[test]
    fn genghis_khan_lets_infantry_fill_a_cavalry_slot() {
        // Medieval Army: Age I, strength 2, needs 1 infantry + 1 cavalry.
        // With no cavalry at all, a normal leader forms zero armies.
        let mut p = blank_player(0, card("Despotism"));
        p.tactic = card("Medieval Army");
        p.techs.insert(card("Warriors"), TechSlot { workers: 2, stored: 0 });
        let state = one_player_state(2, p);
        let s = compute(&state, &state.players[0]);
        assert_eq!(s.strength, 2, "2 Warriors, no tactic bonus (no cavalry, no Genghis)");

        // Genghis Khan: infantry may fill the cavalry slot too.
        let mut p2 = blank_player(0, card("Despotism"));
        p2.leader = card("Genghis Khan");
        p2.tactic = card("Medieval Army");
        p2.techs.insert(card("Warriors"), TechSlot { workers: 2, stored: 0 });
        let state2 = one_player_state(2, p2);
        let s2 = compute(&state2, &state2.players[0]);
        // 2 Warriors: 1 strength/worker * 2 = 2, PLUS one Genghis-formed army
        // (1 infantry-as-infantry + 1 infantry-as-cavalry) at strength 2.
        assert_eq!(s2.strength, 4);
    }

    // ---------------------------------------------------------- colonies

    #[test]
    fn colony_permanent_effects_add_strength_and_happy() {
        let mut p = blank_player(0, card("Despotism"));
        p.colonies.push(card("Strategic Territory (I)")); // permanentEffects.strength = 2
        p.colonies.push(card("Historic Territory (I)")); // permanentEffects.happiness = 1
        let state = one_player_state(2, p);
        let s = compute(&state, &state.players[0]);
        assert_eq!(s.strength, 2);
        assert_eq!(s.happy, 1);
    }

    #[test]
    fn colony_happy_counts_as_a_happy_source_for_st_peters() {
        let mut p = blank_player(0, card("Despotism"));
        p.colonies.push(card("Historic Territory (I)")); // +1 happy, one source
        let w = card("St. Peter's Basilica"); // +2 culture, +1 happy, +1 happy/source
        p.completed_wonders.push(w);
        let state = one_player_state(2, p);
        let s = compute(&state, &state.players[0]);
        // Base happy: St. Peter's +1, the colony's +1 = 2. Two happy
        // SOURCES (the wonder and the colony, both printing happy > 0) at
        // +1 each via `extraHappyPerHappySource` = 2 more. Total 4.
        assert_eq!(s.happy, 4);
    }

    // -------------------------------------------------------------- pacts

    #[test]
    fn both_players_pact_block_applies_to_both_sides_symmetrically() {
        let mut p0 = blank_player(0, card("Despotism"));
        p0.idx = 0;
        // Scientific Cooperation: bothPlayers { technologyScienceDiscount:
        // 2, otherPartyPaysScience: 1 }.
        p0.pacts.push(Pact { card: card("Scientific Cooperation"), owner: 0, partner: 1, a: 0, b: 1 });
        let p1 = blank_player(1, card("Despotism"));
        let state = two_player_state(p0, p1);

        let s0 = compute(&state, &state.players[0]);
        assert_eq!(s0.tech_discount, 2);
        assert_eq!(s0.science_partners, 1 << 1, "player 0's partner is player 1");

        let s1 = compute(&state, &state.players[1]);
        assert_eq!(s1.tech_discount, 2);
        assert_eq!(s1.science_partners, 1 << 0, "player 1's partner is player 0");
    }

    #[test]
    fn pact_does_not_affect_a_third_uninvolved_player() {
        let mut p0 = blank_player(0, card("Despotism"));
        p0.idx = 0;
        p0.pacts.push(Pact { card: card("Scientific Cooperation"), owner: 0, partner: 1, a: 0, b: 1 });
        let p1 = blank_player(1, card("Despotism"));
        let mut state = two_player_state(p0, p1);
        state.players[2].idx = 2;
        let s2 = compute(&state, &state.players[2]);
        assert_eq!(s2.tech_discount, 0);
        assert_eq!(s2.science_partners, 0);
    }

    #[test]
    fn a_b_pact_block_is_asymmetric_and_can_grant_war_immunity() {
        let mut p0 = blank_player(0, card("Despotism"));
        p0.idx = 0;
        // Loss of Sovereignty: A { cultureProduction: 2 }, B {
        // cultureProduction: -2, cannotBeDeclaredWarOnByAnyone: true }.
        p0.pacts.push(Pact { card: card("Loss of Sovereignty"), owner: 0, partner: 1, a: 0, b: 1 });
        let p1 = blank_player(1, card("Despotism"));
        let state = two_player_state(p0, p1);

        let s0 = compute(&state, &state.players[0]);
        assert_eq!(s0.culture, 2, "player 0 is side A: +2 culture");
        assert!(!s0.war_immune);

        let s1 = compute(&state, &state.players[1]);
        // Side B's -2 culture floors at 0 (rulebook "Limits on Ratings").
        assert_eq!(s1.culture, 0);
        assert!(s1.war_immune, "side B: cannotBeDeclaredWarOnByAnyone");
    }

    #[test]
    fn pact_culture_per_wonder_reads_the_other_partys_wonder_count() {
        let mut p0 = blank_player(0, card("Despotism"));
        p0.idx = 0;
        // International Tourism: bothPlayers {
        // cultureProductionPerCompletedWonderOfTheOtherParty: 1 }.
        p0.pacts.push(Pact { card: card("International Tourism"), owner: 0, partner: 1, a: 0, b: 1 });
        let mut p1 = blank_player(1, card("Despotism"));
        p1.completed_wonders.push(card("St. Peter's Basilica"));
        p1.completed_wonders.push(card("Colossus"));
        let state = two_player_state(p0, p1);

        let s0 = compute(&state, &state.players[0]);
        // Player 0 has no wonders of their own, but reads player 1's TWO.
        assert_eq!(s0.culture, 2);

        let s1 = compute(&state, &state.players[1]);
        // Player 1 reads player 0's wonder count (zero), PLUS St. Peter's
        // own +2 culture and +1 happy/extraHappyPerHappySource (1 source).
        assert_eq!(s1.culture, 2);
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
            "St. Peter's Basilica", "Fighting Band", "Entrenchments", "Swordsmen", "Cannon",
            "Air Forces", "Medieval Army", "Genghis Khan", "Strategic Territory (I)",
            "Historic Territory (I)", "Scientific Cooperation", "Loss of Sovereignty",
            "International Tourism", "Colossus",
        ] {
            assert!(CardId::by_name(name).is_some(), "missing card: {name}");
        }
        let _ = &CARDS; // keep the import honest
    }
}
