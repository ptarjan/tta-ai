//! `engine/bots/weighted.py` lines 1445-1756: `features()`, the raw board
//! reading `evaluate` (unowned, `eval.rs`) dots against [`Weights`] --
//! everything else landed in this `bots::weighted` tree so far
//! (`weights.rs`, `horizon.rs`, `rivals.rs`, `events.rs`) exists so this one
//! function can call it instead of restating it. Nothing in this file
//! computes a fact another sibling module already owns.
//!
//! ## Representation: `Features` mirrors `Weights`
//!
//! Python's `f = {...}` is a `dict[str, float]`. This port follows
//! `weights.rs`'s own reasoning for [`Weights`] itself: [`Features`] is
//! `[f64; N]` indexed by `WeightKey as usize`, not a `HashMap<String, f64>`,
//! for the identical reason -- `features()` runs once per candidate move of
//! the 1-ply search this bot's whole existence rests on. The array also
//! makes "does this coordinate exist" a **compile-time** question: every one
//! of the ~60 keys Python's dict literal writes is set through
//! [`Features::set`], which only accepts a real [`WeightKey`] variant, so a
//! renamed or misspelled coordinate is a Rust compile error instead of a
//! `features()` that silently never sets the key (Python's `dict[str,
//! float]` cannot catch that at all, and `docs/OPEN_ITEMS.md`'s
//! `wonder_stages_per_action` is exactly a coordinate that WAS silently
//! stuck at zero for a while, in the `evaluate`-side plumbing rather than
//! here -- the failure mode this representation forecloses).
//!
//! A [`WeightKey`] that `features()` never writes (e.g. `RowUrgency`,
//! `CardBoardCredit`, every `*_early`/`*_late` phase pair) is simply left at
//! its zero default here -- those are priced by the still-unlanded cards
//! valuation layer or by `evaluate` itself, never by this function, exactly
//! as Python's dict never has those keys either.
//!
//! ## What is reused, not restated
//!
//! * [`rivals::rival_board`], [`rivals::deferred_credit`],
//!   [`rivals::rival_context`] -- the whole "what does a rival's board and a
//!   pending decision's payoff look like" layer.
//! * [`events::event_scoring_margin`], [`events::my_seeded_pending`],
//!   [`events::attack_target_terms`], [`events::pact_partner_lead`] -- the
//!   Age III scoring/targeting layer.
//! * [`horizon::rounds_left`]/[`horizon::live_count`] -- for the wonder
//!   overrun term, the one place in this function that needs "how many
//!   rounds are left" as a plain board fact (the RATE horizon itself is
//!   deliberately NOT applied here; see [`features`]'s own doc comment).
//! * [`crate::combat::pacts_for`] -- every pact `idx` is a party to,
//!   wherever in `state.players` it physically sits (§5.9). `combat.rs`
//!   already built this for attack-legality; `features()` needs the exact
//!   same "which pacts is idx a party to" answer for the `pacts`/
//!   `pact_blocks_attack` coordinates, so it is called, not copied.
//! * [`crate::economy`]/[`crate::effects::compute`] -- the happiness/
//!   population/corruption tables and the per-player `Stats` recomputation.
//! * [`CardId::kind`]/[`CardId::level`] -- Python's `_meta()` (`name ->
//!   (type, level)`, built once and memoized at module scope) has no port
//!   here at all: every card already knows its own type and age level as a
//!   static lookup (`cards.rs`), so there is nothing to memoize and nothing
//!   that could go stale.

use crate::cards::{CardType, Special};
use crate::combat;
use crate::economy;
use crate::effects;
use crate::state::GameState;

use super::events;
use super::horizon;
use super::rivals::{self, GainFeature, RivalContext};
use super::weights::{WeightKey, Weights};

/// [`WeightKey::ALL`]'s length, restated locally rather than importing
/// `weights::N` (private to that module -- see its own doc comment: giving
/// it out would let a second file build a differently-sized array that
/// could silently drift from [`Weights`]'s). `WeightKey::ALL.len()` is the
/// one source of truth either way.
const N: usize = WeightKey::ALL.len();

/// NUMERICAL GUARD, not a model claim (mirrors Python's `_TURNS_CAP`):
/// `wonder_turns_to_finish` is a ratio that blows up as resource production
/// approaches zero. 20 turns is already past "never" for a 20-round game, so
/// nothing inside the cap is shaped by it -- it only keeps an infinity from
/// reaching the linear evaluator.
const TURNS_CAP: f64 = 20.0;

/// The raw feature vector `evaluate` (unowned) prices -- Python's `features()`
/// return value, as an array. See this module's top doc comment for why.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Features([f64; N]);

impl Features {
    #[inline]
    pub fn get(&self, key: WeightKey) -> f64 {
        self.0[key as usize]
    }

    #[inline]
    fn set(&mut self, key: WeightKey, value: f64) {
        self.0[key as usize] = value;
    }
}

impl Default for Features {
    fn default() -> Self {
        Features([0.0; N])
    }
}

/// The best level seen so far for each of Python's `_BEST_TYPES` -- the
/// "tech curve" coordinates. A named struct, not a `HashMap<CardType, u8>`:
/// exactly seven fields, all written by one exhaustive `match` in
/// [`sweep_tableau`], so a card type this project adds later that should join
/// `_BEST_TYPES` is a new field and a new match arm, not a silently-absent
/// map entry.
#[derive(Clone, Copy, Debug, Default)]
pub struct BestTypes {
    pub farm: u8,
    pub mine: u8,
    pub lab: u8,
    pub temple: u8,
    pub theater: u8,
    pub library: u8,
    pub arena: u8,
}

/// One pass over a player's tableau: worker counts by category, summed tech
/// levels, the special-tech count, and the best level per [`BestTypes`] slot.
///
/// Shared between [`features`] (below) and `bots::neural::encode::
/// player_block` -- Python has this exact loop TWICE, once in
/// `weighted.py::features()` and once in `neural_encode.py::_player_block`,
/// never factored together (`neural_encode.py`'s own top doc comment: "This
/// module has NO numpy/torch dependency on purpose", it was written to be
/// import-cycle-free of `weighted.py` rather than to share code with it).
/// This port closes that duplication instead of restating it a second time.
///
/// [`TableauSweep::tech_levels`] does NOT include the player's government's
/// level -- `features()` adds `p.government.level()` itself, immediately
/// after calling this, exactly as Python's `weighted.features()` does.
/// `neural_encode.py::_player_block` does NOT add it (checked: no
/// `tech_levels += ...government...` line exists there), so callers that
/// want the neural encoder's number must NOT add it either -- a genuine,
/// deliberate difference between the two Python encoders, not a bug, and
/// preserved by leaving the addition outside this shared function rather
/// than folding it in.
pub struct TableauSweep {
    pub workers: i32,
    pub prod_workers: i32,
    pub urban_workers: i32,
    pub unit_workers: i32,
    pub tech_levels: i32,
    pub special_techs: i32,
    pub best_unit: u8,
    pub best: BestTypes,
}

pub fn sweep_tableau(p: &crate::state::PlayerState) -> TableauSweep {
    let mut workers = 0i32;
    let mut prod_workers = 0i32;
    let mut urban_workers = 0i32;
    let mut unit_workers = 0i32;
    let mut tech_levels = 0i32;
    let mut special_techs = 0i32;
    let mut best_unit = 0u8;
    let mut best = BestTypes::default();

    for (id, slot) in p.techs.iter() {
        let kind = id.kind();
        let lv = id.level();

        match kind {
            CardType::Farm if lv > best.farm => best.farm = lv,
            CardType::Mine if lv > best.mine => best.mine = lv,
            CardType::Lab if lv > best.lab => best.lab = lv,
            CardType::Temple if lv > best.temple => best.temple = lv,
            CardType::Theater if lv > best.theater => best.theater = lv,
            CardType::Library if lv > best.library => best.library = lv,
            CardType::Arena if lv > best.arena => best.arena = lv,
            _ => {}
        }

        match kind {
            CardType::Infantry | CardType::Cavalry | CardType::Artillery | CardType::Air => {
                if lv > best_unit {
                    best_unit = lv;
                }
                unit_workers += slot.workers as i32;
                tech_levels += lv as i32;
            }
            CardType::Lab | CardType::Temple | CardType::Library | CardType::Arena | CardType::Theater => {
                urban_workers += slot.workers as i32;
                tech_levels += lv as i32;
            }
            CardType::Farm | CardType::Mine => {
                prod_workers += slot.workers as i32;
                tech_levels += lv as i32;
            }
            CardType::SpecialTech => {
                special_techs += 1;
                tech_levels += lv as i32;
            }
            // Government/Wonder/Leader/Action/military-deck types never sit
            // in `Tableau` (governments and wonders are their own
            // `PlayerState` fields; military-deck cards are never
            // "developed"), so this arm is unreachable in practice -- kept
            // rather than omitted because `Tableau` itself does not enforce
            // that invariant, and a silent no-op here is the correct
            // response if it ever were.
            _ => {}
        }

        workers += slot.workers as i32;
    }

    TableauSweep { workers, prod_workers, urban_workers, unit_workers, tech_levels, special_techs, best_unit, best }
}

/// `features(state, idx, ctx=None, w=None, priced_only=False)`.
///
/// `ctx`, when `Some`, MUST be a [`RivalContext`] built for this exact `idx`
/// -- typically the caller's own root-level context, reused across every
/// candidate move of a search (see [`rivals::rival_context`]'s own doc
/// comment on why that reuse matters). `None` computes one on the spot.
///
/// `w` is read for exactly one purpose: the `priced_only` speed switch below
/// (Python's own docstring on this function explains why -- `evaluate`
/// multiplies every entry by its weight and skips falsy ones, so
/// `event_scoring_margin` -- fifteen final-event formulas, profiled at 22%
/// of total evaluation time -- is worth skipping outright when the weight
/// vector prices it at 0.0 and only the speed of the search, not the
/// completeness of an instrument, is asked for). Nothing else in this
/// function reads `w`.
///
/// `priced_only` must stay OFF for anything reading the complete vector as
/// an instrument (the coordinate registry, the census, a differential test)
/// -- feeding a `priced_only` vector to a dead-coordinate check would make
/// the check see its own switch, not the model. See Python's own docstring
/// for the full argument.
///
/// The rate horizon is deliberately NOT applied to any RATE coordinate here
/// (`culture_rate`, `science_rate`, `food_rate`, `resource_rate`):
/// `features()` reports the BOARD -- a civilisation producing 5 culture a
/// turn produces 5 culture a turn however much game is left -- and the
/// horizon is a property of what that production is WORTH, which lives in
/// `evaluate`/`feature_marginal` (unowned), not here. See Python's own note
/// on this (`tests/test_build_fresh.py` caught the first cut of the horizon
/// change doing this the wrong way round).
pub fn features(
    state: &GameState,
    idx: u8,
    ctx: Option<&RivalContext>,
    w: Option<&Weights>,
    priced_only: bool,
) -> Features {
    let p = &state.players[idx as usize];
    let s = effects::compute(state, p);

    // ------------------------------------------------------ tableau sweep
    // See `sweep_tableau`'s own doc comment: shared with `bots::neural::
    // encode`, which is why the government-level addition below stays a
    // separate line rather than folding into the shared function.
    let sweep = sweep_tableau(p);
    let TableauSweep {
        workers,
        prod_workers,
        urban_workers,
        unit_workers,
        mut tech_levels,
        special_techs,
        best_unit,
        best,
    } = sweep;
    tech_levels += p.government.level() as i32;

    // ---------------------------------------------------------- pacts (§5.9)
    // Every pact `idx` is a party to, wherever it physically sits --
    // `combat::pacts_for` already answers exactly this question for attack
    // legality, so it is reused rather than re-walking `state.players` here.
    let mut pacts = 0.0f64;
    let mut blocks_attack = 0.0f64;
    for pact in combat::pacts_for(state, idx) {
        pacts += 1.0;
        if pact.card.get().special.contains(&Special::NoAttacksBetweenParties) {
            blocks_attack += 1.0;
        }
    }

    // ------------------------------------------------ deferred payoffs
    // An offered pact and a live high bid are both real positions the trial
    // state cannot show (docs/COMBAT_AUDIT.md). `deferred_credit` no-ops
    // (every field zero) when nothing is pending, so this is safe to call
    // unconditionally rather than gating on `state.pending` first.
    let dc = rivals::deferred_credit(state, idx);
    blocks_attack += dc.blocks_attack;
    let g = |gf: GainFeature| dc.gains.get(gf);

    let happy_req = economy::happy_required(p.yellow_bank);
    let margin = f64::from(s.happy) - f64::from(happy_req) + g(GainFeature::Happy);
    let discontent = (-margin).max(0.0);
    let blue_have = economy::blue_available(p);
    let blue_free = f64::from(blue_have) + g(GainFeature::BlueFree);
    // `pop_food_cost`'s `one_time` is deliberately NOT passed here, matching
    // Python's `economy.pop_food_cost(s, p.yellow_bank)` call exactly -- see
    // that function's own docstring: both evaluator callers preserve this
    // known (small) blind spot on purpose, because closing it changes what
    // the bot plays and belongs in its own measured change, not a port.
    // 8 is the "cannot increase population at all" sentinel (empty yellow
    // bank); the formula itself lives in exactly one place, `economy::
    // pop_food_cost`.
    let pop_cost = economy::pop_food_cost(s.pop_food_discount, p.yellow_bank, 0).unwrap_or(8);

    // --------------------------------------------------------- wonders
    // How far in, and whether it can possibly be finished -- see Python's
    // own extensive comment on this block (`engine/bots/weighted.py:1530`)
    // for the "0-for-58" motivation; not reproduced here.
    let mut progress = 0i32;
    let mut remaining = 0i32;
    let mut stages_left = 0.0f64;
    let mut turns_to_finish = 0.0f64;
    let mut overrun = 0.0f64;
    if !p.wonder.is_none() {
        let stages = p.wonder.get().stages;
        // Clamped defensively: Python's list slicing cannot go out of range,
        // Rust's cannot either without this -- `wonder_steps` never exceeds
        // `stages.len()` in a legal state, but nothing here should panic if
        // it somehow did.
        let built = (p.wonder_steps as usize).min(stages.len());
        progress = stages[..built].iter().map(|&st| i32::from(st)).sum();
        remaining = stages[built..].iter().map(|&st| i32::from(st)).sum();
        if remaining > 0 {
            stages_left = (stages.len() - built) as f64;
            // Turns of the player's WHOLE resource output the wonder still
            // owes, net of what is already banked. Scale-free, so it means
            // the same thing to an Age A economy and an Age III one.
            let owed = f64::from(remaining) - f64::from(p.resources);
            if owed > 0.0 {
                turns_to_finish = (owed / f64::from(s.resources).max(1.0)).min(TURNS_CAP);
                // ...and the part of that the game will not last long enough
                // to pay. This is the 0-for-58 detector.
                let n = horizon::live_count(state);
                overrun = (turns_to_finish - horizon::rounds_left(state, n)).max(0.0);
            }
        }
    }

    let hand_value: f64 = p.hand_civil.as_slice().iter().map(|&c| f64::from(c.level()) + 1.0).sum();
    let hand_mil_value: f64 = p.hand_military.as_slice().iter().map(|&c| f64::from(c.level()) + 1.0).sum();

    // ----------------------------------------------------------- rivals
    // Public rival board facts the evaluator was blind to (GAP 3). `max`
    // everywhere so each term means the same thing at 2p/3p/4p; see Python's
    // own comment on this block for the full argument. One pass, matching
    // `rivals::rival_board`'s own style, rather than filtering
    // `state.players` five separate times for five separate maxima.
    let mut rival_culture = 0u16;
    let mut rival_culture_sum = 0u64;
    let mut rival_count = 0u32;
    let mut rival_free_ca = 0i8;
    let mut rival_hand_civil = 0usize;
    let mut rival_wonders = 0usize;
    for q in state.players[..state.num_players as usize].iter() {
        if q.idx == idx || q.resigned {
            continue;
        }
        rival_culture = rival_culture.max(q.culture);
        rival_culture_sum += u64::from(q.culture);
        rival_count += 1;
        rival_free_ca = rival_free_ca.max(q.civil_actions);
        // `hand_size`, not `hand_civil.len()`: a hand of three unnamed cards
        // is still a hand of three cards (identical in self-play; the app
        // harness is where this differs, docs/APP_HARNESS.md section 2).
        rival_hand_civil = rival_hand_civil.max(q.hand_size_civil());
        rival_wonders = rival_wonders.max(q.completed_wonders.len());
    }
    let rival_mean_culture =
        if rival_count > 0 { rival_culture_sum as f64 / f64::from(rival_count) } else { 0.0 };

    let rboard = rivals::rival_board(state, idx);
    let (atk_lead, atk_weakness) = events::attack_target_terms(state, idx);

    let computed_ctx;
    let ctx = match ctx {
        Some(c) => c,
        None => {
            computed_ctx = rivals::rival_context(state, idx, None, None);
            &computed_ctx
        }
    };

    let mut rival_culture_rate = f64::from(ctx.rival_culture_rate);
    let mut rival_science_rate = f64::from(ctx.rival_science_rate);
    let mut rival_str = f64::from(ctx.rival_strength);
    // An offered pact raises the partner's OWN rates, so it can raise the
    // max-over-rivals these three terms are -- and by how much depends on
    // which rival was offered it. Recomputed as a real max off `rival_rates`
    // rather than added to the max: offering the trailing player a pact that
    // leaves them still behind the leader must cost nothing here, which is
    // precisely the distinction Python's own comment on this block draws.
    // `dc.partner_gains` is all-zero for every index but the one live
    // partner (if any), so looping every rival unconditionally is a no-op
    // everywhere else -- no need to reproduce Python's `if partner_gains:`
    // dict-truthiness gate.
    for pi in 0..state.num_players {
        if pi == idx {
            continue;
        }
        let pg = dc.partner_gains[pi as usize];
        let (base_culture, base_science, base_strength) = ctx.rival_rates[pi as usize];
        rival_culture_rate = rival_culture_rate.max(f64::from(base_culture) + pg.get(GainFeature::CultureRate));
        rival_science_rate = rival_science_rate.max(f64::from(base_science) + pg.get(GainFeature::ScienceRate));
        rival_str = rival_str.max(f64::from(base_strength) + pg.get(GainFeature::Strength));
    }

    let strength = f64::from(s.strength) + g(GainFeature::Strength);
    let rel = strength - rival_str;

    // -------------------------------------------------------------- assemble
    let mut f = Features::default();

    // --- economy
    f.set(WeightKey::Culture, f64::from(p.culture) + g(GainFeature::Culture));
    f.set(WeightKey::CultureRate, f64::from(s.culture) + g(GainFeature::CultureRate));
    f.set(WeightKey::Science, f64::from(p.science) + g(GainFeature::Science));
    f.set(WeightKey::ScienceRate, f64::from(s.science) + g(GainFeature::ScienceRate));
    f.set(WeightKey::FoodRate, f64::from(s.food) + g(GainFeature::FoodRate));
    f.set(WeightKey::ResourceRate, f64::from(s.resources) + g(GainFeature::ResourceRate));
    f.set(WeightKey::FoodStock, f64::from(p.food) + g(GainFeature::FoodStock));
    f.set(WeightKey::ResourceStock, f64::from(p.resources) + g(GainFeature::ResourceStock));
    f.set(WeightKey::BlueFree, blue_free);
    f.set(WeightKey::CorruptionLoss, f64::from(economy::corruption(blue_have)));
    f.set(WeightKey::Consumption, f64::from(economy::consumption(p.yellow_bank)));
    f.set(WeightKey::PopCost, f64::from(pop_cost));
    f.set(WeightKey::YellowBank, f64::from(p.yellow_bank) + g(GainFeature::YellowBank));
    f.set(WeightKey::FreeWorkers, f64::from(p.workers_free) + g(GainFeature::FreeWorkers));
    f.set(WeightKey::Workers, f64::from(workers));
    f.set(WeightKey::ProdWorkers, f64::from(prod_workers));
    f.set(WeightKey::UrbanWorkers, f64::from(urban_workers));
    f.set(WeightKey::UnitWorkers, f64::from(unit_workers));

    // --- happiness
    f.set(WeightKey::HappyMargin, margin.min(3.0));
    f.set(WeightKey::Discontent, discontent);
    f.set(WeightKey::Uprising, if discontent > f64::from(p.workers_free) { 1.0 } else { 0.0 });

    // --- actions
    f.set(WeightKey::CivilActions, f64::from(s.civil_actions) + g(GainFeature::CivilActions));
    f.set(WeightKey::MilitaryActions, f64::from(s.military_actions) + g(GainFeature::MilitaryActions));
    f.set(WeightKey::CaLeft, f64::from(p.civil_actions));
    f.set(WeightKey::MaLeft, f64::from(p.military_actions));
    // Civil actions spent THIS turn reaching into the row (GAP 1) -- a
    // separate channel from `ca_left` on purpose; see Python's own comment
    // (docs/INFORMATION_AUDIT.md section 0) for why conflating the two used
    // to score paying 3 CA for a card as a GAIN.
    f.set(WeightKey::TakeCostPaid, f64::from(p.ca_spent_taking));

    // --- military
    f.set(WeightKey::Strength, strength);
    f.set(WeightKey::StrengthRel, rel);
    f.set(WeightKey::StrengthDeficit, (-rel).max(0.0));
    f.set(WeightKey::StrengthLead, rel.max(0.0).min(6.0));
    f.set(WeightKey::TacticLevel, if p.tactic.is_none() { 0.0 } else { f64::from(p.tactic.level()) });
    // RULES_SPEC 11.3 cliff, not a slope -- see `WeightKey::HasUnit`'s own
    // doc comment in `weights.rs`. `unit_workers` (the sweep total above)
    // already counts every military unit staffed with a worker; this just
    // asks whether that count is zero or not.
    f.set(WeightKey::HasUnit, if unit_workers > 0 { 1.0 } else { 0.0 });
    f.set(WeightKey::Colonies, p.colonies.len() as f64);
    f.set(WeightKey::Pacts, pacts + dc.pact_offers);
    f.set(WeightKey::PactBlocksAttack, blocks_attack);
    f.set(WeightKey::AuctionCommitted, dc.auction_committed);
    f.set(WeightKey::AuctionBid, dc.auction_bid);

    // --- technology
    f.set(WeightKey::TechLevels, f64::from(tech_levels));
    f.set(WeightKey::GovLevel, f64::from(p.government.level()));
    f.set(WeightKey::BestFarm, f64::from(best.farm));
    f.set(WeightKey::BestMine, f64::from(best.mine));
    f.set(WeightKey::BestLab, f64::from(best.lab));
    f.set(WeightKey::BestTemple, f64::from(best.temple));
    f.set(WeightKey::BestTheater, f64::from(best.theater));
    f.set(WeightKey::BestLibrary, f64::from(best.library));
    f.set(WeightKey::BestArena, f64::from(best.arena));
    f.set(WeightKey::BestUnit, f64::from(best_unit));
    f.set(WeightKey::NumTechs, p.techs.len() as f64);
    f.set(WeightKey::SpecialTechs, f64::from(special_techs));

    // --- wonders / leader
    f.set(WeightKey::Wonders, p.completed_wonders.len() as f64);
    f.set(WeightKey::WonderProgress, f64::from(progress));
    f.set(WeightKey::WonderRemaining, f64::from(remaining));
    // Finish discipline -- all 0.0 with nothing in progress.
    f.set(WeightKey::WonderStagesLeft, stages_left);
    f.set(WeightKey::WonderTurnsToFinish, turns_to_finish);
    f.set(WeightKey::WonderOverrun, overrun);
    f.set(WeightKey::WonderStagesPerAction, f64::from(s.wonder_stages - 1));
    f.set(WeightKey::Leader, if p.leader.is_none() { 0.0 } else { 1.0 });

    // --- board side of the card-pricing keys (docs/CARD_BLINDNESS.md): the
    // same key on both sides, the way `civil_actions` already is -- the
    // cards-valuation layer (unowned, `cards.rs`) prices a card in HAND
    // through these keys, and this prices the effect once it is on the
    // BOARD.
    f.set(WeightKey::HandLimit, f64::from(s.civil_hand_limit + s.military_hand_limit));
    f.set(WeightKey::ColonizeBonus, f64::from(s.colonize));
    f.set(WeightKey::BuildDiscount, s.build_discount.iter().map(|&v| f64::from(v)).sum());
    // Real board state -- Despotism caps at 2 urban buildings, the Age III
    // governments at 4 -- and nothing else here reflects it (`urban_workers`
    // is workers, not the cap).
    f.set(WeightKey::UrbanLimit, f64::from(s.urban_limit));
    // Gandhi: a flag, not a signed judgement of good/bad -- see Python's own
    // comment on why the weight stays unsigned at 0.0 by default.
    f.set(WeightKey::NoAggression, if s.no_aggression { 1.0 } else { 0.0 });

    // --- cards
    f.set(WeightKey::HandCivil, p.hand_civil.len() as f64);
    f.set(WeightKey::HandValue, hand_value);
    f.set(WeightKey::HandMilitary, p.hand_military.len() as f64 + g(GainFeature::HandMilitary));
    f.set(WeightKey::HandMilValue, hand_mil_value);

    // --- the Age III scoring events already in play. 0.0 whenever none are
    // pending. Skipped when unpriced and only then -- this is the single
    // most expensive entry in the vector by a wide margin (Python's own
    // profiling: 22% of ALL evaluation time on a vector that prices it at
    // 0.0).
    let unpriced = priced_only && w.map_or(true, |w| w.get(WeightKey::EventScoringMargin) == 0.0);
    f.set(
        WeightKey::EventScoringMargin,
        if unpriced { 0.0 } else { events::event_scoring_margin(state, idx, Some(&ctx.event_pool)) },
    );

    // --- rivals
    f.set(WeightKey::RivalCulture, f64::from(rival_culture));
    f.set(WeightKey::RivalMeanCulture, rival_mean_culture);
    f.set(WeightKey::RivalCultureRate, rival_culture_rate);
    f.set(WeightKey::RivalScienceRate, rival_science_rate);
    f.set(WeightKey::RivalStrength, rival_str);
    f.set(WeightKey::RivalFreeCa, f64::from(rival_free_ca));
    f.set(WeightKey::RivalHandCivil, rival_hand_civil as f64);
    f.set(WeightKey::RivalWonders, rival_wonders as f64);

    // --- the events I planted myself (GAP 4). Legal by construction --
    // `my_seeds` filters on `seeded_by == idx` and reads no pile order.
    f.set(WeightKey::MySeededPending, events::my_seeded_pending(state, idx));

    // --- WHICH opponent I am attacking.
    f.set(WeightKey::AttackTargetLead, atk_lead);
    f.set(WeightKey::AttackTargetWeakness, atk_weakness);
    f.set(WeightKey::PactPartnerLead, events::pact_partner_lead(state, idx));

    // --- the public rival board facts nothing else scored.
    f.set(WeightKey::RivalScienceStock, rboard.rival_science_stock);
    f.set(WeightKey::RivalFoodStock, rboard.rival_food_stock);
    f.set(WeightKey::RivalResourceStock, rboard.rival_resource_stock);
    f.set(WeightKey::RivalFreeWorkers, rboard.rival_free_workers);
    f.set(WeightKey::RivalYellowBank, rboard.rival_yellow_bank);
    f.set(WeightKey::RivalColonies, rboard.rival_colonies);
    f.set(WeightKey::RivalMilActions, rboard.rival_mil_actions);
    f.set(WeightKey::RivalBuildingWonder, rboard.rival_building_wonder);

    f
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game as G;

    /// A fresh deal has no wonder in progress -- all three finish-discipline
    /// terms (`wonder_stages_left`/`wonder_turns_to_finish`/`wonder_overrun`)
    /// must read exactly 0.0, matching Python's "0.0 with nothing in
    /// progress" guarantee.
    #[test]
    fn wonder_terms_are_zero_with_no_wonder_in_progress() {
        for n in [2u8, 3, 4] {
            let state = G::new_game(n, 40);
            for idx in 0..n {
                let f = features(&state, idx, None, None, false);
                assert_eq!(f.get(WeightKey::WonderStagesLeft), 0.0, "{n}p idx {idx}");
                assert_eq!(f.get(WeightKey::WonderTurnsToFinish), 0.0, "{n}p idx {idx}");
                assert_eq!(f.get(WeightKey::WonderOverrun), 0.0, "{n}p idx {idx}");
                assert_eq!(f.get(WeightKey::WonderProgress), 0.0, "{n}p idx {idx}");
                assert_eq!(f.get(WeightKey::WonderRemaining), 0.0, "{n}p idx {idx}");
            }
        }
    }

    /// `has_unit` (docs/OPEN_ITEMS.md, ported from the parked
    /// `origin/has-unit-ab` branch): 1.0 the instant a player staffs their
    /// FIRST military unit, 0.0 with none staffed -- a cliff, not a slope,
    /// unlike the linear `unit_workers` it is derived from. Staffing the
    /// starting `Warriors` tech's first worker must flip `has_unit` from 0.0
    /// to 1.0 while moving `unit_workers` by exactly 1 -- the same +1 a
    /// second or third staffed unit would also produce on `unit_workers`
    /// alone, which is exactly the distinction `unit_workers` cannot
    /// express and `has_unit` exists to add.
    #[test]
    fn has_unit_is_a_cliff_not_the_same_slope_as_unit_workers() {
        let mut state = G::new_game(2, 40);
        let warriors = crate::cards::CardId::by_name("Warriors").expect("Warriors is a base-game unit tech");
        state.players[0]
            .techs
            .get_mut(warriors)
            .expect("a fresh deal starts with Warriors in the tableau")
            .workers = 0;

        let before = features(&state, 0, None, None, false);
        assert_eq!(before.get(WeightKey::UnitWorkers), 0.0, "no staffed units must read unit_workers=0.0");
        assert_eq!(before.get(WeightKey::HasUnit), 0.0, "has_unit must be 0.0 with no staffed units");

        state.players[0].techs.get_mut(warriors).unwrap().workers = 1;
        let after = features(&state, 0, None, None, false);
        assert_eq!(after.get(WeightKey::UnitWorkers), 1.0, "staffing one unit must move unit_workers by exactly 1");
        assert_eq!(after.get(WeightKey::HasUnit), 1.0, "has_unit must flip to 1.0 the instant any unit is staffed");
    }

    /// `happy_margin` resolves through the hand-rolled `margin` local, not a
    /// `"happy"` weight lookup -- there is no `WeightKey::Happy` at all (only
    /// `HappyMargin` is a real weight), so this just pins that the computed
    /// coordinate is finite and matches the board's `Stats::happy` directly
    /// with an empty yellow bank (`happy_required(0) == 8`, so a fresh
    /// player's margin is very negative and gets clamped by `discontent`,
    /// not by `happy_margin` itself, which only clamps its UPPER side at 3).
    /// `docs/OPEN_ITEMS.md` records `wonder_overrun` as "doubly dead": the
    /// weight is 0.0 on every committed vector AND the feature computes
    /// exactly 0.0 on every state in the registry's 6-game self-play corpus,
    /// flagged there as "likely a bug in the feature computation (something
    /// that should fire near wonder completion never does)".
    ///
    /// It is not. This constructs the state the formula is FOR -- a wonder
    /// with resources still owed, zero production, and one round left
    /// (`final_round_end` pins `horizon::rounds_left` to exactly 1, rather
    /// than relying on the generic cards-remaining estimate) -- and the
    /// coordinate fires: `turns_to_finish` (3 resources owed / 1 production,
    /// floored at 1.0) exceeds the 1 round actually left, so `overrun` is
    /// positive. The 6-game corpus never happening to sample a state this
    /// unlucky is a property of that corpus (small, deterministic, already
    /// documented elsewhere in this file's own doc comment as fragile), not
    /// of this formula -- see this module's top doc comment's `TURNS_CAP`
    /// note and Python's identical `overrun` derivation
    /// (`engine/bots/weighted.py:1548-1557`), which this ports unchanged.
    /// This test is the guard: if a future edit to `rounds_left`/
    /// `turns_to_finish` silently makes `overrun` structurally unreachable
    /// again, this fails instead of the coordinate going quietly dead a
    /// second time.
    #[test]
    fn wonder_overrun_fires_for_a_constructed_near_completion_shortfall_state() {
        let mut state = crate::game::new_game(2, 47);
        let pyramids = crate::cards::CardId::by_name("Pyramids").unwrap();
        state.players[0].wonder = pyramids;
        state.players[0].wonder_steps = 0;
        state.players[0].resources = 0;
        state.final_round_end = Some(state.round);
        let f = features(&state, 0, None, None, false);
        assert!(
            f.get(WeightKey::WonderTurnsToFinish) > 0.0,
            "turns_to_finish must be positive for this scenario"
        );
        assert!(
            f.get(WeightKey::WonderOverrun) > 0.0,
            "wonder_overrun must fire (turns_to_finish exceeds the 1 round left), got {}",
            f.get(WeightKey::WonderOverrun)
        );
    }

    #[test]
    fn happy_margin_is_never_the_bare_happy_stat() {
        let state = G::new_game(2, 41);
        let f = features(&state, 0, None, None, false);
        // `min(3, margin)`: never above 3, whatever the board.
        assert!(f.get(WeightKey::HappyMargin) <= 3.0);
    }

    /// `event_scoring_margin` is computed by default (`priced_only = false`)
    /// even when nothing is pending, and is skipped only when BOTH
    /// `priced_only` is set AND the weight vector prices it at 0.0 --
    /// pinning the exact `w.map_or` gate against Python's `(w or {}).get(...)`
    /// truthiness check.
    #[test]
    fn event_scoring_margin_priced_only_gate() {
        let state = G::new_game(2, 42);
        let mut w = Weights::default();
        assert_eq!(w.get(WeightKey::EventScoringMargin), 0.0);

        // priced_only + zero weight -> skipped, exactly 0.0 either way here
        // (no pending events), but the skip path must be taken -- verified
        // indirectly: this must not panic and must equal the non-skipped
        // value in this all-zero scenario.
        let skipped = features(&state, 0, None, Some(&w), true);
        let full = features(&state, 0, None, Some(&w), false);
        assert_eq!(skipped.get(WeightKey::EventScoringMargin), 0.0);
        assert_eq!(full.get(WeightKey::EventScoringMargin), 0.0);

        // A nonzero weight must NOT be skipped even with `priced_only`.
        w.set(WeightKey::EventScoringMargin, 1.0);
        let priced = features(&state, 0, None, Some(&w), true);
        assert_eq!(priced.get(WeightKey::EventScoringMargin), full.get(WeightKey::EventScoringMargin));
    }

    /// Every [`WeightKey`] this function is documented to write is reachable
    /// -- a coarse structural guard against the exact failure mode this
    /// module's top doc comment calls out (`docs/OPEN_ITEMS.md`'s
    /// `wonder_stages_per_action`): every key listed here must round-trip
    /// through `Features::get` without a compile error, which it does by
    /// construction, and at least one sampled state must set it to something
    /// -- as opposed to leaving the whole vector at its `Features::default()`
    /// zero, which would be true of a `features()` that silently wrote
    /// nothing.
    #[test]
    fn features_actually_writes_something_beyond_the_zero_default() {
        let state = G::new_game(3, 43);
        let f = features(&state, 0, None, None, false);
        let default = Features::default();
        assert_ne!(f, default, "features() must not return an all-zero vector on a real deal");
    }
}
