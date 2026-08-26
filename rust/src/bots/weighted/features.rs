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

use crate::cards::{CardId, CardType, Special};
use crate::combat;
use crate::costs;
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

/// Where [`WeightKey::StrengthLead`] stops counting. A lead this large already
/// beats every aggression and war card in the box, so the feature saturates
/// rather than rewarding overkill.
///
/// It lives here, shared, because three files have to agree on it or the
/// arithmetic stops meaning anything: this file builds the clamped feature,
/// `rivals::strength_marginal` prices ONE point of strength as that feature's
/// derivative and so must stop adding `w[strength_lead]` at exactly the same
/// boundary, and `neural::encode` normalises by it. Written as a literal in
/// each, a change to one silently turns the marginal into the derivative of a
/// function nobody computes.
pub const STRENGTH_LEAD_CAP: f64 = 6.0;

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
    /// Developed-but-unstaffed slots -- a tech in `p.techs` (so already
    /// developed) with `slot.workers == 0` (so no physical copy staffed
    /// yet): `do_build` is the ONLY writer of a positive `workers` count
    /// (`apply.rs`), so a zero here means "known, not yet built", the real,
    /// actionable "unfilled slot" the worker marginal-need axis prices --
    /// see `features()`'s own comment on [`WeightKey::WorkerGap`]/
    /// [`WeightKey::WorkerSurplus`]. Scoped to [`CardType::takes_workers`]
    /// (urban + production + unit), matching every other "does this type
    /// take a worker" check in this crate (e.g. `economy.rs`,
    /// `board_yields::is_levelled_type`).
    pub unbuilt_slots: i32,
    /// The cheapest [`unbuilt_slots`](Self::unbuilt_slots) entry's printed
    /// resource cost -- `None` when there are none. The RAW printed
    /// `card.resource_cost`, not `costs::build_cost_for`'s discount-adjusted
    /// figure: that function recomputes a full `effects::state_stats` per
    /// call, and calling it once per unbuilt tableau entry here (this loop
    /// already runs on every candidate move of the search) would multiply
    /// the cost of an already-expensive computation by the tableau size. A
    /// linear feature's "need" signal does not require discount-exact
    /// precision, only the right shape.
    pub unbuilt_min_resource_cost: Option<i32>,
    /// [`WeightKey::ResourceCommitmentTurns`]'s own numerator half: the SUM
    /// (not the min) of every [`unbuilt_slots`](Self::unbuilt_slots) entry's
    /// printed `resource_cost` -- every RAW printed obligation still owed for
    /// standing tableau slots, same discount-precision reasoning as
    /// [`unbuilt_min_resource_cost`](Self::unbuilt_min_resource_cost)
    /// immediately above, computed in the SAME pass rather than a second walk
    /// of the tableau.
    pub unbuilt_resource_cost_sum: i32,
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
    let mut unbuilt_slots = 0i32;
    let mut unbuilt_min_resource_cost: Option<i32> = None;
    let mut unbuilt_resource_cost_sum = 0i32;

    for (id, slot) in p.techs.iter() {
        let kind = id.kind();
        let lv = id.level();

        if slot.workers == 0 && kind.takes_workers() {
            unbuilt_slots += 1;
            let cost = i32::from(id.get().resource_cost);
            unbuilt_min_resource_cost =
                Some(unbuilt_min_resource_cost.map_or(cost, |m: i32| m.min(cost)));
            unbuilt_resource_cost_sum += cost;
        }

        match kind {
            CardType::Farm if lv > best.farm => best.farm = lv,
            CardType::Mine if lv > best.mine => best.mine = lv,
            CardType::Lab if lv > best.lab => best.lab = lv,
            CardType::Temple if lv > best.temple => best.temple = lv,
            CardType::Theater if lv > best.theater => best.theater = lv,
            CardType::Library if lv > best.library => best.library = lv,
            CardType::Arena if lv > best.arena => best.arena = lv,
            CardType::Farm | CardType::Mine | CardType::Lab | CardType::Temple | CardType::Library | CardType::Arena | CardType::Theater | CardType::Infantry | CardType::Cavalry | CardType::Artillery | CardType::Air | CardType::Government | CardType::SpecialTech | CardType::Wonder | CardType::Leader | CardType::Action | CardType::Tactic | CardType::Aggression | CardType::War | CardType::Pact | CardType::Bonus | CardType::Territory | CardType::Event => {}
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
            CardType::Government | CardType::Wonder | CardType::Leader | CardType::Action | CardType::Tactic | CardType::Aggression | CardType::War | CardType::Pact | CardType::Bonus | CardType::Territory | CardType::Event => {}
        }

        workers += slot.workers as i32;
    }

    TableauSweep {
        workers,
        prod_workers,
        urban_workers,
        unit_workers,
        tech_levels,
        special_techs,
        best_unit,
        best,
        unbuilt_slots,
        unbuilt_min_resource_cost,
        unbuilt_resource_cost_sum,
    }
}

/// The four marginal-need THRESHOLDS: how much food, how many resources, how
/// much science and how many free workers a rule actually demands of this
/// player right now. Rulebook quantities, one per axis, computed once.
///
/// Two readers, and that is the point. [`features`] turns each threshold into
/// its `*Gap`/`*Surplus` raw-level pair for `eval::evaluate`'s dot product;
/// `rivals::feature_marginal` divides the shortfall by the threshold to get a
/// dimensionless fraction and hinges the per-unit marginal on it. A second
/// copy of any of these four formulas is the defect this struct exists to
/// prevent -- both readers must agree on what "need" means or the pricer and
/// the evaluator disagree about the same board.
///
/// THRESHOLDS ONLY, never the "have" side. [`features`] adds its pending-gain
/// adjustments ([`GainFeature`]) to every stock it reads and
/// `feature_marginal` prices the board as it stands, so the two readers
/// legitimately differ on "have" and must not share it.
pub struct MarginalNeed {
    /// Food to raise population once, [`economy::pop_food_cost`]. 8 is that
    /// function's "cannot increase population at all" sentinel (empty yellow
    /// bank); `one_time` is deliberately not passed -- see below.
    pub food: f64,
    /// Resources for the cheapest unstaffed tableau slot, `0.0` with none --
    /// [`TableauSweep::unbuilt_min_resource_cost`].
    pub resource: f64,
    /// Science for the cheapest developable card in the civil hand, `0.0`
    /// with none. `Develop` is what science pays for (see `costs::tech_cost`),
    /// so a hand with nothing developable left has no science need at all.
    pub science: f64,
    /// Free workers for the unstaffed tableau slots --
    /// [`TableauSweep::unbuilt_slots`].
    pub worker: f64,
}

/// [`MarginalNeed`] for one player. Takes the already-computed [`effects::
/// Stats`] and [`TableauSweep`] rather than recomputing them: both are
/// expensive and both callers already hold one.
pub fn marginal_needs(
    p: &crate::state::PlayerState,
    s: &effects::Stats,
    sweep: &TableauSweep,
) -> MarginalNeed {
    // `pop_food_cost`'s `one_time` is deliberately NOT passed here: closing
    // that (small) blind spot changes what the bot plays and belongs in its
    // own measured change. The formula itself lives in exactly one place,
    // `economy::pop_food_cost`.
    let food = f64::from(economy::pop_food_cost(s.pop_food_discount, p.yellow_bank, 0).unwrap_or(8));

    // One more pass over the same bounded hand `features`'s `hand_value`
    // already walks, not a scan of the tableau or the row.
    let mut science: Option<u8> = None;
    for &c in p.hand_civil.as_slice() {
        if crate::bots::board_yields::is_levelled_type(c.kind()) {
            let cost = c.get().science_cost;
            science = Some(science.map_or(cost, |m: u8| m.min(cost)));
        }
    }

    MarginalNeed {
        food,
        resource: f64::from(sweep.unbuilt_min_resource_cost.unwrap_or(0)),
        science: f64::from(science.unwrap_or(0)),
        worker: f64::from(sweep.unbuilt_slots),
    }
}

/// Whether `card`, sitting in `p.hand_civil`, could be paid for RIGHT NOW --
/// the classifier [`WeightKey::HandOverCapacity`]'s "K" counts against (see
/// that key's own doc comment in `weights.rs` for the full derivation).
/// Printed costs only, never `costs::tech_cost`/`costs::build_cost_for` --
/// same discount-precision reasoning as [`MarginalNeed::resource`].
///
/// Exhaustive over every `CardType`, no wildcard:
///
/// * a levelled/tech type ([`crate::bots::board_yields::is_levelled_type`],
///   which already covers [`CardType::SpecialTech`]) is paid for by
///   DEVELOPING it -- its printed `science_cost` against the player's
///   science.
/// * [`CardType::Government`] is not `is_levelled_type`, and its
///   `science_cost` is always 0 -- its real price is `Card::peaceful_cost`,
///   paid in science through the ordinary develop action (RULES_SPEC 8.3).
/// * [`CardType::Leader`]/[`CardType::Action`] print zero for every cost
///   field and genuinely cost nothing to play from hand.
/// * [`CardType::Wonder`] cannot physically reach `p.hand_civil`: a taken
///   wonder goes straight to `PlayerState::wonder`, never the hand
///   (`apply.rs`'s take-move branch; RULES_SPEC 2.4/6.7). Every military-
///   deck type is drafted into `hand_military`, a different field, never
///   this one. Both groups are named explicitly rather than folded into a
///   wildcard -- see `sweep_tableau`'s own precedent for a state nothing in
///   this function enforces but the rules make impossible: an inert answer
///   is the correct response if either ever fires.
fn hand_card_affordable(card: CardId, science: f64) -> bool {
    let kind = card.kind();
    // The ONE classifier for "does this type need Develop's science before
    // anything else" (`marginal_needs`'s own science-threshold loop above
    // uses the identical call) -- restated as a second, hand-written type
    // list here is exactly the drift this crate's own doc comments warn
    // against elsewhere.
    if crate::bots::board_yields::is_levelled_type(kind) {
        return f64::from(card.get().science_cost) <= science;
    }
    match kind {
        CardType::Government => f64::from(card.get().peaceful_cost) <= science,
        CardType::Leader | CardType::Action => true,
        // Cannot occur in `p.hand_civil` -- see this function's own doc
        // comment. `false` (not counted as affordable) is the inert, no-op
        // answer if this invariant is ever broken elsewhere.
        CardType::Wonder
        | CardType::Tactic
        | CardType::Aggression
        | CardType::War
        | CardType::Pact
        | CardType::Bonus
        | CardType::Territory
        | CardType::Event => false,
        // Unreachable: `is_levelled_type` above already returned for every
        // one of these -- kept as an explicit arm rather than a wildcard so
        // the match stays exhaustive by construction if `CardType` ever
        // grows a new levelled type.
        CardType::Farm
        | CardType::Mine
        | CardType::Lab
        | CardType::Temple
        | CardType::Library
        | CardType::Arena
        | CardType::Theater
        | CardType::Infantry
        | CardType::Cavalry
        | CardType::Artillery
        | CardType::Air
        | CardType::SpecialTech => unreachable!("is_levelled_type already handled this type above"),
    }
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
    // The four marginal-need thresholds, shared verbatim with `rivals::
    // feature_marginal`'s need hinge -- see `MarginalNeed`. `unbuilt_slots`
    // and `unbuilt_min_resource_cost` are therefore read through `needs`
    // below rather than destructured here: they have exactly one consumer
    // and it is that struct.
    let needs = marginal_needs(p, &s, &sweep);
    let TableauSweep {
        workers,
        prod_workers,
        urban_workers,
        unit_workers,
        mut tech_levels,
        special_techs,
        best_unit,
        best,
        unbuilt_resource_cost_sum,
        ..
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
    // state cannot show (docs/AUDIT_HISTORY.md). `deferred_credit` no-ops
    // (every field zero) when nothing is pending, so this is safe to call
    // unconditionally rather than gating on `state.pending` first.
    let dc = rivals::deferred_credit(state, idx);
    blocks_attack += dc.blocks_attack;
    let g = |gf: GainFeature| dc.gains.get(gf);

    let happy_req = economy::happy_required(p.yellow_bank);
    let margin = f64::from(s.happy) - f64::from(happy_req) + g(GainFeature::Happy);
    let discontent = (-margin).max(0.0);
    // `WeightKey::HappyMarginAfterNextPop`: the identical margin one
    // population increase forward, under UNCHANGED staffing.
    // `economy::increase_population` only ever touches `p.yellow_bank`
    // (decrementing it, or leaving it floored at 0 once already empty --
    // `saturating_sub(1)` mirrors that floor exactly) for a player who has
    // not yet re-placed the new worker; `effects::Stats.happy` is staffing-
    // driven, not population-COUNT-driven (`add_production`/`happy_from`
    // scale by `slot.workers`, and a freshly born worker lands unassigned in
    // `p.workers_free`), so `s.happy` carries over unchanged -- see
    // `WeightKey::HappyMarginAfterNextPop`'s own doc comment in `weights.rs`
    // for the full derivation off `economy.rs`. Reuses `economy::
    // happy_required`'s own VERIFIED band table a second time rather than a
    // new formula, and needs no second `effects::compute`.
    let next_yellow_bank = p.yellow_bank.saturating_sub(1);
    let happy_req_next = economy::happy_required(next_yellow_bank);
    let margin_next = f64::from(s.happy) - f64::from(happy_req_next) + g(GainFeature::Happy);
    let discontent_next_pop = (-margin_next).max(0.0);
    let blue_have = economy::blue_available(p);
    let blue_free = f64::from(blue_have) + g(GainFeature::BlueFree);
    // --------------------------------------------------------- wonders
    // How far in, and whether it can possibly be finished -- see Python's
    // own extensive comment on this block (`engine/bots/weighted.py:1530`)
    // for the "0-for-58" motivation; not reproduced here. The arithmetic
    // itself lives in [`horizon::wonder_outlook`], which `cards::
    // wonder_potential` also reads: two copies of "how far from finishing
    // this wonder am I" is exactly this codebase's defining bug class, so
    // there is one.
    let outlook = horizon::wonder_outlook(state, p, s.resources);
    let (progress, remaining, stages_left, turns_to_finish, overrun, age_overrun) = match outlook {
        Some(o) => {
            (o.progress, o.remaining, o.stages_left, o.turns_to_finish, o.overrun, o.age_overrun)
        }
        None => (0, 0, 0.0, 0.0, 0.0, 0.0),
    };

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
    // NET rates, not gross. Corruption (§6.2) and consumption (§6.4) are
    // exact, already-known step tables -- `economy` computes both from state
    // the evaluator is holding -- so the rules do the arithmetic and the
    // league is handed the answer. They used to be separate weighted
    // coordinates, which was wrong twice over: it let a search PRICE a
    // deduction the rulebook fixes (and it duly priced both as BENEFITS,
    // because a big civilization pays more of each than a small one), and,
    // because both are step functions of another weighted coordinate
    // (`BlueFree`, `YellowBank`), a positive price inverted the cliff and
    // made the evaluator PREFER crossing into a worse band. Netting removes
    // the free parameter entirely: what reaches the weights is the food and
    // resources a player will actually keep. Both keys are now retired --
    // see `weights::RETIRED_KEYS`.
    //
    // Both bills are read off the PROJECTED banks, not the raw ones.
    // `deferred_credit` exists to show a pending gain the trial state cannot,
    // and a gain that fills the blue bank is precisely the move that tips a
    // player over a corruption edge -- charging the old bank would hide the
    // cost of the very move being considered.
    let yellow_bank = f64::from(p.yellow_bank) + g(GainFeature::YellowBank);
    let proj_blue = blue_free.max(0.0).round() as u16;
    let proj_yellow = yellow_bank.clamp(0.0, f64::from(u8::MAX)).round() as u8;
    let consumption = f64::from(economy::consumption(proj_yellow));
    let corruption = f64::from(economy::corruption(proj_blue));
    f.set(WeightKey::FoodRate, f64::from(s.food) + g(GainFeature::FoodRate) - consumption);
    f.set(
        WeightKey::ResourceRate,
        f64::from(s.resources) + g(GainFeature::ResourceRate) - corruption,
    );
    // `WeightKey::ResourceCommitmentTurns`: turns of this player's ENTIRE
    // resource production already spoken for by the in-progress wonder's
    // own remaining cost (`remaining`, already computed above from `horizon::
    // wonder_outlook`) plus every developed-but-unstaffed tableau slot's
    // printed resource cost (`unbuilt_resource_cost_sum`, folded into
    // `sweep_tableau`'s own loop above rather than a second walk of the
    // tableau). Reads `WeightKey::ResourceRate` back rather than
    // recomputing it a second time -- the same "one true computation" idiom
    // `WeightKey::HandOverCapacity` uses for `Science` -- so this picks up
    // the NET rate (corruption and pending gains already folded in) that was
    // just set immediately above. `max(resource_rate, 1)` per the design
    // note's own formula: a stalled/negative economy reads the raw
    // obligation as its own turn count rather than dividing by zero or
    // going negative.
    f.set(
        WeightKey::ResourceCommitmentTurns,
        (f64::from(remaining) + f64::from(unbuilt_resource_cost_sum)) / f.get(WeightKey::ResourceRate).max(1.0),
    );
    f.set(WeightKey::FoodStock, f64::from(p.food) + g(GainFeature::FoodStock));
    f.set(WeightKey::ResourceStock, f64::from(p.resources) + g(GainFeature::ResourceStock));
    f.set(WeightKey::BlueFree, blue_free);
    // How much slack is left before the next band, as opposed to what the
    // current band already costs (which is now netted into the rates above
    // and carries no free parameter). A strong player plans a turn around
    // this number -- spending down to exactly the edge and no further -- and
    // without it the bot is blind to a cliff until it has already fallen off:
    // nothing else in the vector distinguishes storing one resource at 11
    // free blue, which costs 2 per turn forever after, from storing one at
    // 15, which is free. Deliberately NOT sign-gated: headroom is a
    // deterministic function of `BlueFree`/`YellowBank`, so "good all else
    // equal" is vacuous here -- the two coordinates cannot move
    // independently, and the league is left to price the pair.
    f.set(WeightKey::CorruptionHeadroom, f64::from(economy::corruption_headroom(proj_blue)));
    f.set(WeightKey::ConsumptionHeadroom, f64::from(economy::consumption_headroom(proj_yellow)));
    f.set(WeightKey::PopCost, needs.food);
    f.set(WeightKey::YellowBank, yellow_bank);
    f.set(WeightKey::FreeWorkers, f64::from(p.workers_free) + g(GainFeature::FreeWorkers));
    f.set(WeightKey::Workers, f64::from(workers));
    f.set(WeightKey::ProdWorkers, f64::from(prod_workers));
    f.set(WeightKey::UrbanWorkers, f64::from(urban_workers));
    f.set(WeightKey::UnitWorkers, f64::from(unit_workers));

    // --- happiness
    f.set(WeightKey::HappyMargin, margin.min(3.0));
    f.set(WeightKey::Discontent, discontent);
    f.set(WeightKey::HappyMarginAfterNextPop, discontent_next_pop);
    // UNITS FIX: this used to be a bare 0/1 indicator, so the fitted
    // `WeightKey::Uprising` coefficient had to stand for "the cost of an
    // uprising" at EVERY board size at once -- a catastrophic mid/late-game
    // uprising and a nearly-free turn-1 one primed the identical penalty,
    // and a climb chasing the worse case drove the single shared coefficient
    // to the clamp trying (and failing) to also price the milder one (see
    // the live log: "uprising = -60.000 (clamp 60.0) -- pinned
    // coordinate"). RULES_SPEC 6.3: on an uprising "score/corruption/
    // production/consumption all skipped" -- `economy::end_of_turn`'s step
    // 2 (this file's own economy::end_of_turn doc comment) forfeits the
    // ENTIRE production phase, not a fixed amount: science+culture scoring
    // (step 3a), food production (3c), and resource production (3e). `s`
    // (this function's own `effects::compute` reading, same Stats step 3
    // itself applies) already carries exactly those four rates, so scaling
    // the indicator by their sum makes the coefficient mean "evaluator-value
    // per point of production actually at stake" -- fixed across positions
    // -- instead of "value of an uprising at some unstated, unscalable board
    // size". A rulebook fact computed by the engine, not a fitted constant:
    // the climb still has to learn the PRICE, only the magnitude it prices
    // is now the one the rules actually forfeit.
    let uprising_production_at_stake = f64::from(s.science + s.culture + s.food + s.resources);
    f.set(
        WeightKey::Uprising,
        if discontent > f64::from(p.workers_free) { uprising_production_at_stake } else { 0.0 },
    );

    // --- actions
    f.set(WeightKey::CivilActions, f64::from(s.civil_actions) + g(GainFeature::CivilActions));
    f.set(WeightKey::MilitaryActions, f64::from(s.military_actions) + g(GainFeature::MilitaryActions));
    f.set(WeightKey::CaLeft, f64::from(p.civil_actions));
    // RULES_SPEC 6.7 / the summary line "Unspent MAs at end of turn each
    // draw 1 military card (max 3)": the 4th-and-later unused military
    // action converts into nothing, so its draw-potential value saturates
    // at `MA_DRAW_CAP` instead of scaling linearly with the raw count --
    // see `board_yields::MA_DRAW_CAP`'s own doc comment for why a delta
    // derived from this feature must cap BEFORE differencing, not after.
    // `military_actions` is never negative (an `i8` the engine floors at 0
    // on every write -- see `game.rs`/`economy.rs`'s "reset actions" step),
    // so only the upper bound needs clamping here.
    f.set(WeightKey::MaLeft, f64::from(p.military_actions).min(crate::bots::board_yields::MA_DRAW_CAP));
    // Civil actions spent THIS turn reaching into the row (GAP 1) -- a
    // separate channel from `ca_left` on purpose; see Python's own comment
    // (docs/ANALYSIS_HISTORY.md, INFORMATION_AUDIT.md verdict) for why
    // conflating the two used to score paying 3 CA for a card as a GAIN.
    f.set(WeightKey::TakeCostPaid, f64::from(p.ca_spent_taking));
    // The same spend as a SHARE of the whole allowance. A quotient is
    // deliberately outside the linear span of the two coordinates it is built
    // from (`take_cost_paid` above and the pool it is drawn from), which is
    // the only way this vector can say that 3 actions out of 4 is a different
    // decision from 3 out of 7 -- see `WeightKey::TakeCostShare`'s own doc
    // comment. `costs::ca_total` is the government's printed allowance plus
    // every effect on it, and is at least 1 for every legal government, so
    // the guard is defensive rather than reachable.
    let ca_total = costs::ca_total(state, p);
    f.set(
        WeightKey::TakeCostShare,
        if ca_total > 0 { f64::from(p.ca_spent_taking) / f64::from(ca_total) } else { 0.0 },
    );

    // --- military
    f.set(WeightKey::Strength, strength);
    f.set(WeightKey::StrengthRel, rel);
    f.set(WeightKey::StrengthDeficit, (-rel).max(0.0));
    f.set(WeightKey::StrengthLead, rel.clamp(0.0, STRENGTH_LEAD_CAP));
    f.set(WeightKey::TacticLevel, if p.tactic.is_none() { 0.0 } else { f64::from(p.tactic.level()) });
    // RULES_SPEC 11.3 cliff, not a slope -- see `WeightKey::HasUnit`'s own
    // doc comment in `weights.rs`. `unit_workers` (the sweep total above)
    // already counts every military unit staffed with a worker; this just
    // asks whether that count is zero or not.
    f.set(WeightKey::HasUnit, if unit_workers > 0 { 1.0 } else { 0.0 });
    f.set(WeightKey::Colonies, p.colonies.len() as f64);
    // RULES_SPEC 5.4 cliff, not a slope -- see `WeightKey::HasColony`'s own
    // doc comment in `weights.rs`. `legal.rs`'s `aggression_target_qualifies`
    // reads `q.colonies.is_empty()` for exactly this fact (Annex's printed
    // target clause); this asks the same question of `p.colonies` here.
    f.set(WeightKey::HasColony, if p.colonies.is_empty() { 0.0 } else { 1.0 });
    f.set(WeightKey::Pacts, pacts + dc.pact_offers);
    f.set(WeightKey::PactBlocksAttack, blocks_attack);
    // RULES_SPEC 5.6 cliff, not a slope -- see `WeightKey::WarImmune`'s own
    // doc comment in `weights.rs`. `combat::war_forbidden` ORs `s.war_immune`
    // (a pact side printing `cannotBeDeclaredWarOnByAnyone`) alongside
    // `pact_forbids_attack` (already `blocks_attack` above); this reads the
    // same `Stats` field `s` already carries for this player.
    f.set(WeightKey::WarImmune, if s.war_immune { 1.0 } else { 0.0 });
    // RULES_SPEC 5.4/5.6 cliff, not a slope -- see `WeightKey::
    // AttackCostDoubled`'s own doc comment in `weights.rs`. `legal.rs` and
    // `combat.rs` both apply this doubling off `leader_is(q, "Mahatma
    // Gandhi")`; read here off the printed special the leader card
    // actually carries (`Special::OpponentsPayDoubleMilitaryActionsToAttackYou`)
    // rather than the name, so the fact and the gate cannot drift onto two
    // different leaders if a future card ever shares it.
    let doubles_attack_cost = !p.leader.is_none()
        && p.leader.get().special.contains(&Special::OpponentsPayDoubleMilitaryActionsToAttackYou);
    f.set(WeightKey::AttackCostDoubled, if doubles_attack_cost { 1.0 } else { 0.0 });
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
    // `WeightKey::WonderPoolRivalClaimed`'s own doc comment in `weights.rs`:
    // raw count (0..=4) of `state.age_civil` wonders sitting in OTHER
    // players' `completed_wonders`. Deliberately excludes `idx`'s own
    // completions -- those are already priced by `Wonders` above, and
    // folding them in here would average two opposite-sign cases into one
    // scalar, the exact defect this coordinate exists to avoid one level up
    // from where `LeaderReplacement` avoids it. Fresh O(players * 16) scan;
    // no `RivalContext` field already carries this tally.
    let rival_wonders_this_age: usize = state.players[..state.num_players as usize]
        .iter()
        .filter(|q| q.idx != idx)
        .flat_map(|q| q.completed_wonders.as_slice().iter())
        .filter(|w| w.get().age == state.age_civil)
        .count();
    f.set(WeightKey::WonderPoolRivalClaimed, rival_wonders_this_age as f64);
    f.set(WeightKey::WonderProgress, f64::from(progress));
    f.set(WeightKey::WonderRemaining, f64::from(remaining));
    // Finish discipline -- all 0.0 with nothing in progress.
    f.set(WeightKey::WonderStagesLeft, stages_left);
    f.set(WeightKey::WonderTurnsToFinish, turns_to_finish);
    f.set(WeightKey::WonderOverrun, overrun);
    // The same shortfall against the deadline the RULES impose (RULES_SPEC
    // 12.2, `game::antiquate`) rather than against the end of the game --
    // for an Age A wonder taken in Age A the two differ by two whole ages,
    // and 66.5% of every wonder started in the 200-game 2p census died to the
    // one nothing in the vector could see. `wonder_overrun` above is left
    // exactly as it was on purpose: correcting it in place would move every
    // champion on disk.
    f.set(WeightKey::WonderAgeOverrun, age_overrun);
    f.set(WeightKey::WonderStagesPerAction, f64::from(s.wonder_stages - 1));
    f.set(WeightKey::Leader, if p.leader.is_none() { 0.0 } else { 1.0 });
    // `WeightKey::LeaderReplacement`'s own doc comment in `weights.rs` has
    // the full derivation: §2.5/§9.1 (one leader per age) means
    // `taken_leader_ages.count_ones()` is the exact number of leader cards
    // this player has EVER taken, so holding a leader (`!p.leader.is_none()`)
    // while that count is 2+ can only mean at least one earlier leader was
    // swapped out for the current one.
    f.set(
        WeightKey::LeaderReplacement,
        if !p.leader.is_none() && p.taken_leader_ages.count_ones() >= 2 { 1.0 } else { 0.0 },
    );
    // RULES_SPEC 5.5 cliff, not a slope -- see `WeightKey::WonderInProgress`'s
    // own doc comment in `weights.rs`. `legal.rs`'s `aggression_target_
    // qualifies` reads `q.wonder.is_none()` (the same field `horizon::
    // wonder_outlook` gates its `None` case on, hence `outlook.is_none()`
    // implying `progress == 0` above) for the wonder half of Infiltrate's
    // OR'd target clause; this asks the same question of `p.wonder` here.
    f.set(WeightKey::WonderInProgress, if p.wonder.is_none() { 0.0 } else { 1.0 });

    // --- board side of the card-pricing keys (docs/ANALYSIS_HISTORY.md,
    // CARD_BLINDNESS.md verdict): the
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
    // How much of that hand the next age boundary is about to throw away
    // (RULES_SPEC 12.2 -- `game::antiquate` culls hands as well as the
    // board). 0.0 per card for a fresh one, rising to 1.0 for one that is
    // out of rounds. ONE clock for the whole hand: the expensive half of the
    // answer depends on the position, not the card -- see
    // `horizon::AntiquationClock`'s own doc comment.
    let clock = horizon::AntiquationClock::at(state, horizon::live_count(state));
    let mut hand_perishable = 0.0;
    // `WeightKey::HandOverCapacity`'s "K": how many civil-hand cards this
    // player could pay to play right now (`hand_card_affordable`'s own doc
    // comment has the full derivation). `WeightKey::Science` is already set
    // above, so this reads it back rather than recomputing `p.science +
    // g(GainFeature::Science)` a second time -- the same "one true
    // computation" idiom the marginal-need gap/surplus block below uses for
    // its own "have" sides. Folded into this same civil-hand pass rather
    // than a fourth walk of the same bounded list.
    let my_science = f.get(WeightKey::Science);
    let mut hand_affordable = 0.0;
    for &card in p.hand_civil.as_slice() {
        let left = clock.rounds_until_antiquation(card.get().age);
        hand_perishable += (1.0 - left / clock.rounds_left()).clamp(0.0, 1.0);
        if hand_card_affordable(card, my_science) {
            hand_affordable += 1.0;
        }
    }
    f.set(WeightKey::HandPerishable, hand_perishable);
    f.set(
        WeightKey::HandOverCapacity,
        (f.get(WeightKey::HandCivil) - hand_affordable).max(0.0),
    );
    f.set(WeightKey::HandMilitary, p.hand_military.len() as f64 + g(GainFeature::HandMilitary));
    f.set(WeightKey::HandMilValue, hand_mil_value);

    // --- the Age III scoring events already in play. 0.0 whenever none are
    // pending. Skipped when unpriced and only then -- this is the single
    // most expensive entry in the vector by a wide margin (Python's own
    // profiling: 22% of ALL evaluation time on a vector that prices it at
    // 0.0).
    let unpriced = priced_only && w.is_none_or(|w| w.get(WeightKey::EventScoringMargin) == 0.0);
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

    // --- marginal need (gap/surplus): see `WeightKey`'s own doc comment on
    // this block (`weights.rs`) for the shape and why gap/surplus are two
    // coordinates, never one signed difference. Every "have" side re-reads a
    // coordinate this function already set above rather than recomputing the
    // same expression a second time -- one true computation, not restated.
    let food_have = f.get(WeightKey::FoodStock);
    f.set(WeightKey::FoodGap, (needs.food - food_have).max(0.0));
    f.set(WeightKey::FoodSurplus, (food_have - needs.food).max(0.0));

    let resource_have = f.get(WeightKey::ResourceStock);
    f.set(WeightKey::ResourceGap, (needs.resource - resource_have).max(0.0));
    f.set(WeightKey::ResourceSurplus, (resource_have - needs.resource).max(0.0));

    let science_have = f.get(WeightKey::Science);
    f.set(WeightKey::ScienceGap, (needs.science - science_have).max(0.0));
    f.set(WeightKey::ScienceSurplus, (science_have - needs.science).max(0.0));

    // Culture has no absolute threshold a rule ever converts into a cost --
    // the live pressure is competitive, so "need" is the strongest rival's
    // culture (`rival_culture`, computed above), the same "relative to the
    // field" shape `strength_rel`/`strength_deficit`/`strength_lead` already
    // use for military.
    let culture_have = f.get(WeightKey::Culture);
    let culture_need = f64::from(rival_culture);
    f.set(WeightKey::CultureGap, (culture_need - culture_have).max(0.0));
    f.set(WeightKey::CultureSurplus, (culture_have - culture_need).max(0.0));

    // Happiness already has a shortfall coordinate (`Discontent`, above,
    // `max(0, -margin)`); this is only the missing surplus half of that same
    // hinge -- uncapped, unlike `HappyMargin`'s `.min(3.0)`, which mixes the
    // negative tail back in for reasons unrelated to this block.
    f.set(WeightKey::HappySurplus, margin.max(0.0));

    let ca_have = f.get(WeightKey::CaLeft);
    let ca_need = f.get(WeightKey::HandCivil);
    f.set(WeightKey::CivilActionGap, (ca_need - ca_have).max(0.0));
    f.set(WeightKey::CivilActionSurplus, (ca_have - ca_need).max(0.0));

    // Deliberately the RAW military-action pool, not `MaLeft` (capped at
    // `board_yields::MA_DRAW_CAP` for the end-of-turn CARD-DRAW conversion --
    // a different question from "can I play what is in my military hand
    // this turn").
    let ma_have = f64::from(p.military_actions);
    let ma_need = f.get(WeightKey::HandMilitary);
    f.set(WeightKey::MilitaryActionGap, (ma_need - ma_have).max(0.0));
    f.set(WeightKey::MilitaryActionSurplus, (ma_have - ma_need).max(0.0));

    let worker_have = f.get(WeightKey::FreeWorkers);
    f.set(WeightKey::WorkerGap, (needs.worker - worker_have).max(0.0));
    f.set(WeightKey::WorkerSurplus, (worker_have - needs.worker).max(0.0));

    f
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game as G;

    /// THE MOSES SHAPE (`WeightKey::FoodGap`'s own doc comment in
    /// `weights.rs`): reducing a player's food NEED (Moses's real special,
    /// `PopIncreaseFoodDiscount`) lowers `food_gap` -- with NO Moses-specific
    /// code anywhere in `features()`, only the generic `economy::
    /// pop_food_cost` read every player already goes through. Built with an
    /// empty food stock and a yellow bank in the `pop_cost_base` == 7 band
    /// (`economy::pop_cost_base`'s own test pins `1..=4` there) so
    /// `food_gap` starts well above zero -- a state where it was already
    /// 0.0 would make this test vacuous. Confirmed RED by reverting `food_
    /// gap`'s formula to a constant `p.food` read that ignores `pop_cost`.
    #[test]
    fn a_reduced_food_need_lowers_the_food_gap_with_no_card_specific_code() {
        let mut state = G::new_game(2, 51);
        state.players[0].food = 0;
        state.players[0].yellow_bank = 1;
        let before = features(&state, 0, None, None, false);
        assert!(
            before.get(WeightKey::FoodGap) > 0.0,
            "food_gap must be positive before any discount, got {}",
            before.get(WeightKey::FoodGap)
        );

        let moses = crate::cards::CardId::by_name("Moses").expect("a base-game leader");
        state.players[0].leader = moses;
        let after = features(&state, 0, None, None, false);
        assert!(
            after.get(WeightKey::FoodGap) < before.get(WeightKey::FoodGap),
            "Moses's PopIncreaseFoodDiscount must lower food_gap: before={} after={}",
            before.get(WeightKey::FoodGap),
            after.get(WeightKey::FoodGap)
        );
        // The surplus half of the same hinge must stay at its floor
        // throughout -- there is no surplus to have while the gap is open.
        assert_eq!(before.get(WeightKey::FoodSurplus), 0.0);
        assert_eq!(after.get(WeightKey::FoodSurplus), 0.0);
    }

    /// The blindness this fixes: before `corruption_headroom` existed, two
    /// positions paying the SAME corruption looked identical on every
    /// coordinate that mattered for the cliff, even when one was a single
    /// stored resource from paying more and the other had room to spare.
    /// Sweeping stored resources must therefore produce at least one pair of
    /// states with an equal bill and a different headroom -- and the headroom
    /// must always be the one the bank actually implies, so the feature
    /// cannot quietly drift away from `economy`'s table.
    #[test]
    fn two_positions_paying_the_same_corruption_are_still_told_apart_by_their_headroom() {
        let mut seen: Vec<(u16, f64)> = Vec::new();
        for stored in 0u16..=12 {
            let mut state = G::new_game(2, 51);
            state.players[0].resources += stored;
            let blue = economy::blue_available(&state.players[0]);
            let head = features(&state, 0, None, None, false).get(WeightKey::CorruptionHeadroom);
            assert_eq!(
                head,
                f64::from(economy::corruption_headroom(blue)),
                "with {stored} stored the feature must match the bank it is derived from"
            );
            seen.push((economy::corruption(blue), head));
        }
        assert!(
            seen.iter().any(|&(bill, head)| seen
                .iter()
                .any(|&(b2, h2)| b2 == bill && h2 != head)),
            "the sweep never produced two states with the same bill but different slack, \
             so it cannot show that the cliff is now visible: {seen:?}"
        );
    }

    /// Corruption is an EXACT rulebook deduction, so the evaluator is handed
    /// the answer rather than a coordinate to price: crossing a corruption
    /// band must move `resource_rate` down by exactly the resources §6.2
    /// takes, no more and no less. Stored resources are the lever because
    /// they occupy blue tokens without changing production, so the gross
    /// rate is held fixed and the whole delta is attributable.
    #[test]
    fn crossing_a_corruption_band_costs_the_resource_rate_exactly_the_rulebook_amount() {
        let mut state = G::new_game(2, 51);
        let before_corr = economy::corruption(economy::blue_available(&state.players[0]));
        let before_rate = features(&state, 0, None, None, false).get(WeightKey::ResourceRate);

        state.players[0].resources += 10;
        let after_corr = economy::corruption(economy::blue_available(&state.players[0]));
        let after_rate = features(&state, 0, None, None, false).get(WeightKey::ResourceRate);

        assert!(
            after_corr > before_corr,
            "the setup must actually cross a band, got {before_corr} -> {after_corr}"
        );
        assert_eq!(
            before_rate - after_rate,
            f64::from(after_corr - before_corr),
            "resource_rate must fall by exactly the corruption incurred"
        );
    }

    /// The food half of the same property (§6.4 consumption). Together these
    /// two pin the reason `corruption_loss` and `consumption` are in
    /// `weights::RETIRED_KEYS`: there is no free parameter left for a league
    /// to price, because the rulebook's own arithmetic already reached the
    /// weights.
    #[test]
    fn a_bigger_population_costs_the_food_rate_exactly_the_consumption_it_owes() {
        let mut state = G::new_game(2, 51);
        state.players[0].yellow_bank = 17;
        let before_use = economy::consumption(state.players[0].yellow_bank);
        let before_rate = features(&state, 0, None, None, false).get(WeightKey::FoodRate);

        state.players[0].yellow_bank = 4;
        let after_use = economy::consumption(state.players[0].yellow_bank);
        let after_rate = features(&state, 0, None, None, false).get(WeightKey::FoodRate);

        assert!(
            after_use > before_use,
            "the setup must actually raise consumption, got {before_use} -> {after_use}"
        );
        assert_eq!(
            before_rate - after_rate,
            f64::from(after_use - before_use),
            "food_rate must fall by exactly the consumption owed"
        );
    }

    /// UNITS FIX for `WeightKey::Uprising`: it used to be a bare 0/1
    /// indicator (`if discontent > workers_free { 1.0 } else { 0.0 }`), so a
    /// single fitted coefficient had to price "an uprising" identically at
    /// every board size -- a civilization with a big production forfeits
    /// far more (RULES_SPEC 6.3: the WHOLE production phase is skipped) than
    /// one with a small one, and no fixed number can be "correct" for both.
    /// Two boards with the IDENTICAL discontent-vs-workers_free overshoot
    /// (so the 0/1 condition never changes) but very different production
    /// must now read different `Uprising` magnitudes, proportional to what
    /// is actually at stake. Confirmed RED by reverting to the bare 0/1
    /// formula: both boards read exactly `1.0`, this test's inequality
    /// failed with `small=1 big=1`.
    #[test]
    fn uprising_scales_with_the_production_actually_at_stake_not_a_flat_0_1() {
        let mut small = G::new_game(2, 51);
        small.players[0].yellow_bank = 7; // happy_required(7) == 4, fresh happy == 0
        small.players[0].workers_free = 3; // discontent 4 > 3 free -> an uprising

        let small_feat = features(&small, 0, None, None, false).get(WeightKey::Uprising);
        assert!(small_feat > 0.0, "must actually be an uprising, got feature {small_feat}");

        // A bigger production base, same discontent/workers_free overshoot:
        // the 0/1 CONDITION is identical, only the magnitude at stake grows.
        let mut big = small.clone();
        big.players[0].science_rate_extra = 20;
        big.players[0].culture_rate_extra = 20;
        let big_feat = features(&big, 0, None, None, false).get(WeightKey::Uprising);

        assert!(
            big_feat > small_feat,
            "a bigger production base facing the IDENTICAL uprising condition must forfeit more: \
             small={small_feat} big={big_feat}"
        );

        // No uprising at all must still read exactly zero, regardless of
        // how much production is on the board -- the magnitude only prices
        // the turns an uprising actually happens.
        big.players[0].workers_free = 4; // discontent 4 == 4 free -> NOT an uprising
        assert_eq!(
            features(&big, 0, None, None, false).get(WeightKey::Uprising),
            0.0,
            "no uprising must still read exactly 0.0, no matter how much production is at stake"
        );
    }

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

    /// `has_colony` (RULES_SPEC 5.4, `WeightKey::HasColony`'s own doc
    /// comment in `weights.rs`): 0.0 with no colonies at all, the state a
    /// fresh deal starts in.
    #[test]
    fn has_colony_is_zero_with_no_colonies() {
        let state = G::new_game(2, 40);
        let f = features(&state, 0, None, None, false);
        assert_eq!(f.get(WeightKey::Colonies), 0.0, "a fresh deal starts with no colonies");
        assert_eq!(f.get(WeightKey::HasColony), 0.0, "has_colony must be 0.0 with no colonies");
    }

    /// `has_colony` must flip to 1.0 the instant a player owns their FIRST
    /// colony, moving `colonies` by exactly 1 -- the same +1 a second or
    /// third colony would also produce on the linear `colonies` count,
    /// which is exactly the distinction `colonies` cannot express and
    /// `has_colony` exists to add (same shape as `has_unit`/`unit_workers`
    /// above).
    #[test]
    fn has_colony_flips_to_one_with_the_first_colony() {
        let mut state = G::new_game(2, 40);
        let territory =
            crate::cards::CardId::by_name("Vast Territory (I)").expect("Vast Territory (I) is a base-game colony");
        state.players[0].colonies.push(territory);

        let f = features(&state, 0, None, None, false);
        assert_eq!(f.get(WeightKey::Colonies), 1.0, "one colony must move the linear count by exactly 1");
        assert_eq!(f.get(WeightKey::HasColony), 1.0, "has_colony must flip to 1.0 the instant any colony is owned");
    }

    /// The cliff, not a slope: `has_colony` must stay pinned at 1.0 as more
    /// colonies pile up, while `colonies` keeps climbing linearly -- a
    /// third colony is not a legal-target event the way the first one was
    /// (`legal.rs`'s `aggression_target_qualifies` only ever asks
    /// `q.colonies.is_empty()`, never how many).
    #[test]
    fn has_colony_stays_one_with_several_colonies_it_is_a_cliff_not_a_count() {
        let mut state = G::new_game(2, 40);
        for name in ["Vast Territory (I)", "Strategic Territory (I)", "Historic Territory (I)"] {
            let territory = crate::cards::CardId::by_name(name).unwrap_or_else(|| panic!("no such card: {name}"));
            state.players[0].colonies.push(territory);
        }

        let f = features(&state, 0, None, None, false);
        assert_eq!(f.get(WeightKey::Colonies), 3.0, "three colonies must move the linear count to 3");
        assert_eq!(f.get(WeightKey::HasColony), 1.0, "has_colony must stay at 1.0, not climb to 3.0, with a third colony");
    }

    /// `war_immune` (RULES_SPEC 5.6, `WeightKey::WarImmune`'s own doc
    /// comment): 0.0 with no pacts at all, the state a fresh deal starts in.
    #[test]
    fn war_immune_is_zero_with_no_pacts() {
        let state = G::new_game(2, 40);
        let f = features(&state, 0, None, None, false);
        assert_eq!(f.get(WeightKey::WarImmune), 0.0, "a fresh deal starts with no pacts, so no war immunity");
    }

    /// `war_immune` must flip to 1.0 the instant a player holds a pact side
    /// printing `cannotBeDeclaredWarOnByAnyone` -- "Loss of Sovereignty"'s B
    /// side, the same fixture `combat.rs`'s own
    /// `war_forbidden_true_when_defender_is_war_immune` uses as ground truth.
    #[test]
    fn war_immune_flips_to_one_with_a_war_immunity_pact_side() {
        let mut state = G::new_game(2, 40);
        let pact = crate::cards::CardId::by_name("Loss of Sovereignty").expect("a base-game pact card");
        state.players[0].pacts.push(crate::state::Pact { card: pact, owner: 0, partner: 1, a: 1, b: 0 });
        let f = features(&state, 0, None, None, false);
        assert_eq!(f.get(WeightKey::WarImmune), 1.0, "war_immune must flip to 1.0 the instant its holder's own side (b == 0) grants it");
    }

    /// `attack_cost_doubled` (RULES_SPEC 5.4/5.6, `WeightKey::
    /// AttackCostDoubled`'s own doc comment): 0.0 with no leader at all, the
    /// state a fresh deal starts in.
    #[test]
    fn attack_cost_doubled_is_zero_with_no_leader() {
        let state = G::new_game(2, 40);
        let f = features(&state, 0, None, None, false);
        assert_eq!(f.get(WeightKey::AttackCostDoubled), 0.0, "a fresh deal starts with no leader in play");
    }

    /// `attack_cost_doubled` must flip to 1.0 the instant Mahatma Gandhi is
    /// the player's leader -- and must NOT fire for an unrelated leader, so
    /// this is reading the printed special, not just "any leader at all"
    /// (`WeightKey::Leader` already covers that separate fact).
    #[test]
    fn attack_cost_doubled_flips_to_one_only_for_gandhi() {
        let mut state = G::new_game(2, 40);
        let moses = crate::cards::CardId::by_name("Moses").expect("a base-game leader");
        state.players[0].leader = moses;
        let f = features(&state, 0, None, None, false);
        assert_eq!(f.get(WeightKey::Leader), 1.0, "Moses is still a leader in play");
        assert_eq!(f.get(WeightKey::AttackCostDoubled), 0.0, "Moses does not print the double-cost special");

        let gandhi = crate::cards::CardId::by_name("Mahatma Gandhi").expect("a base-game leader");
        state.players[0].leader = gandhi;
        let f = features(&state, 0, None, None, false);
        assert_eq!(f.get(WeightKey::AttackCostDoubled), 1.0, "Gandhi's own special must flip this to 1.0");
    }

    /// `wonder_in_progress` (RULES_SPEC 5.5, `WeightKey::WonderInProgress`'s
    /// own doc comment): 0.0 with no wonder under construction, the state a
    /// fresh deal starts in.
    #[test]
    fn wonder_in_progress_is_zero_with_no_wonder() {
        let state = G::new_game(2, 40);
        let f = features(&state, 0, None, None, false);
        assert_eq!(f.get(WeightKey::WonderProgress), 0.0, "a fresh deal has no wonder in progress");
        assert_eq!(f.get(WeightKey::WonderInProgress), 0.0, "wonder_in_progress must be 0.0 with nothing under construction");
    }

    /// The cliff, not a slope: `wonder_in_progress` must flip to 1.0 the
    /// instant ANY wonder is taken, before a single stage is paid -- the
    /// same "the step matters, not the count" shape `has_colony` exists to
    /// add beside the linear `colonies`, mirrored here beside the linear
    /// (sunk-cost) `wonder_progress`.
    #[test]
    fn wonder_in_progress_flips_to_one_the_instant_a_wonder_is_taken() {
        let mut state = G::new_game(2, 40);
        let pyramids = crate::cards::CardId::by_name("Pyramids").expect("a base-game wonder");
        state.players[0].wonder = pyramids;
        let f = features(&state, 0, None, None, false);
        assert_eq!(f.get(WeightKey::WonderProgress), 0.0, "no stage has been paid yet, so the linear progress term is still 0");
        assert_eq!(f.get(WeightKey::WonderInProgress), 1.0, "wonder_in_progress must flip to 1.0 the instant the wonder is taken, before any stage is paid");
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

    /// RULES_SPEC 6.7 / the summary line "Unspent MAs at end of turn each
    /// draw 1 military card (max 3)": a player's THIRD unused military
    /// action is still worth a card, so `ma_left` must keep rising through
    /// it -- but the FOURTH converts into nothing at all, so `ma_left` must
    /// NOT rise again. A linear-count `ma_left` (the pre-fix shape) prices
    /// the 4th exactly like the 3rd; capping at 3 is what makes the two
    /// marginal actions read differently.
    #[test]
    fn the_third_unused_military_action_is_worth_more_but_the_fourth_is_worth_nothing() {
        let state = G::new_game(2, 43);
        let ma_left_at = |n: i8| -> f64 {
            let mut s = state.clone();
            s.players[0].military_actions = n;
            features(&s, 0, None, None, false).get(WeightKey::MaLeft)
        };
        let at2 = ma_left_at(2);
        let at3 = ma_left_at(3);
        let at4 = ma_left_at(4);
        assert!(at3 > at2, "the 3rd unused military action must still add value (it still draws a card): {at2} -> {at3}");
        assert_eq!(
            at4, at3,
            "the 4th unused military action must add nothing -- RULES_SPEC 6.7 caps the draw at 3, got {at3} -> {at4}"
        );
    }

    /// THE QUOTIENT'S WHOLE POINT (`WeightKey::TakeCostShare`): three civil
    /// actions out of four is a different decision from three out of seven,
    /// and `take_cost_paid`/`ca_left` cannot say so -- the first is identical
    /// in both positions by construction, and no assignment of the two
    /// weights can make an absolute spend depend on the size of the pool it
    /// came out of. The share does say so, which is exactly why it is a
    /// quotient and therefore outside their linear span.
    ///
    /// The government is the lever because `costs::ca_total` reads its
    /// printed civil-action allowance: Despotism (the starting government)
    /// against a later one, with the spend held fixed at 3 in both.
    #[test]
    fn taking_a_card_for_three_of_four_actions_reads_differently_from_three_of_seven() {
        let reading_under = |government: &str| -> (f64, f64) {
            let mut state = G::new_game(2, 61);
            state.players[0].government =
                crate::cards::CardId::by_name(government).unwrap_or_else(|| panic!("no such government: {government}"));
            state.players[0].ca_spent_taking = 3;
            let f = features(&state, 0, None, None, false);
            (f.get(WeightKey::TakeCostPaid), f.get(WeightKey::TakeCostShare))
        };
        let (small_paid, small_share) = reading_under("Despotism");
        let (big_paid, big_share) = reading_under("Constitutional Monarchy");
        assert_eq!(
            small_paid, big_paid,
            "the fixture must hold the ABSOLUTE spend fixed, or it proves nothing about the share"
        );
        assert!(
            small_share > big_share,
            "3 actions out of a small allowance must read as a bigger share than 3 out of a large \
             one: {small_share} vs {big_share}"
        );
    }

    /// `hand_perishable` reads remaining useful LIFETIME, which nothing else
    /// in the vector does: the same hand, held at the same size and value,
    /// is worth less the closer the age boundary that will discard it
    /// (RULES_SPEC 12.2). Built by holding the hand fixed and moving only the
    /// deal -- an Age A hand at the start of Age A, versus the same cards
    /// once the deal has reached Age I and that deck is nearly out.
    #[test]
    fn a_hand_the_next_age_boundary_is_about_to_discard_reads_as_perishable() {
        let age_a_card = crate::cards::CardId::by_name("Bronze").expect("an Age A technology");
        let hand_of_three = |age_civil: crate::cards::Age, deck: usize| -> f64 {
            let mut state = G::new_game(2, 62);
            state.age_civil = age_civil;
            while state.civil_deck.len() > deck {
                state.civil_deck.pop();
            }
            state.players[0].hand_civil = crate::state::CardList::new();
            for _ in 0..3 {
                state.players[0].hand_civil.push(age_a_card);
            }
            features(&state, 0, None, None, false).get(WeightKey::HandPerishable)
        };
        let fresh = hand_of_three(crate::cards::Age::A, 7);
        let expiring = hand_of_three(crate::cards::Age::I, 4);
        assert!(
            expiring > fresh,
            "an Age A hand with the Age I deck nearly out must be more perishable than the same \
             hand at the deal: {fresh} -> {expiring}"
        );
        assert!(
            expiring <= 3.0,
            "the coordinate is a sum of per-card shares, so three cards cap it at 3.0, got {expiring}"
        );
    }

    /// `wonder_age_overrun` is the coordinate `wonder_overrun` could not be:
    /// it fires on a wonder that the GAME has plenty of time for but the age
    /// boundary does not. Same fixture shape as
    /// `horizon::tests::a_wonder_can_be_finishable_before_the_game_ends_and_
    /// still_be_doomed_by_its_age`, asserted here on the feature vector the
    /// evaluator actually reads, so a future edit that computes the outlook
    /// correctly but forgets to write the coordinate still fails.
    #[test]
    fn wonder_age_overrun_fires_where_the_game_end_overrun_reads_exactly_zero() {
        let mut state = G::new_game(2, 63);
        state.age_civil = crate::cards::Age::I;
        while state.civil_deck.len() > 8 {
            state.civil_deck.pop();
        }
        state.players[0].wonder = crate::cards::CardId::by_name("Pyramids").expect("a base-game wonder");
        state.players[0].wonder_steps = 0;
        state.players[0].resources = 0;
        let f = features(&state, 0, None, None, false);
        assert_eq!(
            f.get(WeightKey::WonderOverrun),
            0.0,
            "the fixture must be comfortably finishable before the GAME ends, or it proves nothing"
        );
        assert!(
            f.get(WeightKey::WonderAgeOverrun) > 0.0,
            "an Age A wonder with the Age I deck nearly out is past its own deadline, got {}",
            f.get(WeightKey::WonderAgeOverrun)
        );
    }

    /// `WeightKey::LeaderReplacement`'s three branches, named after the
    /// popcount cases in the derivation this feature is built on
    /// (`weights.rs`'s own doc comment on the variant): an empty slot that
    /// never took a leader, a leader that is still the ONLY one ever taken,
    /// and a leader held after `taken_leader_ages.count_ones() >= 2` proves
    /// at least one earlier leader was swapped out (§2.5/§9.1's one-leader-
    /// per-age rule is what makes 2+ distinct take-events imply a swap).
    #[test]
    fn leader_replacement_feature_distinguishes_an_original_leader_from_a_swapped_in_one() {
        let moses = crate::cards::CardId::by_name("Moses").expect("a base-game leader");

        // Never taken a leader: slot empty, popcount 0.
        let mut never_taken = G::new_game(2, 71);
        never_taken.players[0].leader = crate::cards::CardId::NONE;
        never_taken.players[0].taken_leader_ages = 0;
        assert_eq!(
            features(&never_taken, 0, None, None, false).get(WeightKey::LeaderReplacement),
            0.0,
            "an empty slot that never took a leader must not read as a replacement"
        );

        // Holding the ONLY leader ever taken: popcount 1, slot occupied.
        let mut original = G::new_game(2, 71);
        original.players[0].leader = moses;
        original.players[0].taken_leader_ages = 1 << (crate::cards::Age::A as u8);
        assert_eq!(
            features(&original, 0, None, None, false).get(WeightKey::LeaderReplacement),
            0.0,
            "a first, never-swapped leader must not read as a replacement"
        );

        // A second leader-take event happened (popcount >= 2) and the slot
        // is occupied: the current leader can only be the replacement.
        let mut replaced = G::new_game(2, 71);
        replaced.players[0].leader = moses;
        replaced.players[0].taken_leader_ages =
            (1 << (crate::cards::Age::A as u8)) | (1 << (crate::cards::Age::I as u8));
        assert_eq!(
            features(&replaced, 0, None, None, false).get(WeightKey::LeaderReplacement),
            1.0,
            "a leader held after a second leader-take event must read as a replacement"
        );
    }

    /// `WeightKey::WonderPoolRivalClaimed` counts RIVALS' completed wonders
    /// of `state.age_civil` only -- the evaluated player's own completions
    /// are excluded because `Wonders` already prices them, and folding them
    /// in here would reproduce the sign-averaging defect this whole task
    /// exists to avoid (see the variant's own doc comment in `weights.rs`).
    /// Three players each complete one Age::A wonder: the evaluated player
    /// (idx 0, `Pyramids`) plus two rivals (`Hanging Gardens`, `Colossus`).
    /// The feature must read 2.0 -- NOT 3.0 -- which is the one assertion
    /// that pins the rivals-only decision rather than the plan's original
    /// all-players proposal.
    #[test]
    fn wonder_pool_rival_claimed_counts_only_rivals_completed_wonders_of_the_current_age() {
        let pyramids = crate::cards::CardId::by_name("Pyramids").expect("an Age A wonder");
        let hanging_gardens = crate::cards::CardId::by_name("Hanging Gardens").expect("an Age A wonder");
        let colossus = crate::cards::CardId::by_name("Colossus").expect("an Age A wonder");

        let mut state = G::new_game(3, 72);
        state.age_civil = crate::cards::Age::A;
        state.players[0].completed_wonders.push(pyramids);
        state.players[1].completed_wonders.push(hanging_gardens);
        state.players[2].completed_wonders.push(colossus);

        let f = features(&state, 0, None, None, false);
        assert_eq!(
            f.get(WeightKey::WonderPoolRivalClaimed),
            2.0,
            "two RIVALS completed an Age::A wonder; the evaluated player's own Pyramids must not \
             be counted, or this would reproduce the all-players sign-averaging defect the task \
             exists to fix"
        );
    }

    /// `WeightKey::HandOverCapacity` (`max(0, hand_civil - K)`) reads 0.0
    /// with an empty civil hand -- there is nothing to be over capacity on.
    /// Baseline for the non-empty cases below.
    #[test]
    fn hand_over_capacity_is_zero_with_an_empty_civil_hand() {
        let state = G::new_game(2, 51);
        assert!(state.players[0].hand_civil.as_slice().is_empty(), "test setup: hand must start empty");
        let f = features(&state, 0, None, None, false);
        assert_eq!(f.get(WeightKey::HandOverCapacity), 0.0);
    }

    /// A levelled card the player can already pay to DEVELOP (printed
    /// `science_cost <= science`) must not count toward the shortfall --
    /// `hand_over_capacity` is 0.0 for "a player who can afford to play
    /// everything they hold" (the key's own doc comment in `weights.rs`).
    /// Irrigation (Farm, Age I) prints `science_cost: 3`; 3 science exactly
    /// covers it. Confirmed RED by making `hand_card_affordable`'s levelled
    /// branch always return `false` (so an affordable card is wrongly
    /// counted as a shortfall) -- reverted after confirming.
    #[test]
    fn an_affordable_levelled_card_does_not_count_toward_hand_over_capacity() {
        let irrigation = crate::cards::CardId::by_name("Irrigation").expect("a base-game Age I farm tech");
        let mut state = G::new_game(2, 51);
        state.players[0].hand_civil.push(irrigation);
        state.players[0].science = 3;
        let f = features(&state, 0, None, None, false);
        assert_eq!(
            f.get(WeightKey::HandOverCapacity),
            0.0,
            "science (3) exactly covers Irrigation's printed science_cost (3)"
        );
    }

    /// The mirror case: the same card, but science is one short of the
    /// printed cost. `hand_civil` (1) minus `K` (0, unaffordable) must read
    /// 1.0. Confirmed RED by making `hand_card_affordable`'s levelled branch
    /// always return `true` (so an unaffordable card is wrongly counted as
    /// affordable, `HandOverCapacity` reading 0.0 instead of 1.0) --
    /// reverted after confirming.
    #[test]
    fn an_unaffordable_levelled_card_counts_toward_hand_over_capacity() {
        let irrigation = crate::cards::CardId::by_name("Irrigation").expect("a base-game Age I farm tech");
        let mut state = G::new_game(2, 51);
        state.players[0].hand_civil.push(irrigation);
        state.players[0].science = 2; // one short of Irrigation's science_cost: 3
        let f = features(&state, 0, None, None, false);
        assert_eq!(f.get(WeightKey::HandOverCapacity), 1.0);
    }

    /// The conflict this key's implementation was stopped and re-briefed
    /// over: `Card::science_cost` is always 0 for every government
    /// (`costs::tech_cost`'s own doc comment) -- a government's real price
    /// is `Card::peaceful_cost`, paid in SCIENCE (RULES_SPEC 8.3). Monarchy
    /// prints `peaceful_cost: 8`; at 3 science it is nowhere close to
    /// affordable, so it must count toward the shortfall. A naive
    /// implementation reading `science_cost` (0) for the non-levelled
    /// branch would read this as ALWAYS affordable regardless of science,
    /// exactly the blindness this feature exists to remove. Confirmed RED
    /// by changing `hand_card_affordable`'s `Government` arm to read
    /// `card.get().science_cost` instead of `card.get().peaceful_cost`
    /// (the literal bug): `HandOverCapacity` drops from 1.0 to 0.0 --
    /// reverted after confirming.
    #[test]
    fn a_government_whose_peaceful_cost_exceeds_science_counts_as_unaffordable() {
        let monarchy = crate::cards::CardId::by_name("Monarchy").expect("a base-game government");
        assert_eq!(monarchy.get().science_cost, 0, "test premise: science_cost is always 0 for a government");
        assert!(
            monarchy.get().peaceful_cost > 3,
            "test premise: Monarchy's peaceful_cost must exceed the science this test grants"
        );
        let mut state = G::new_game(2, 51);
        state.players[0].hand_civil.push(monarchy);
        state.players[0].science = 3;
        let f = features(&state, 0, None, None, false);
        assert_eq!(
            f.get(WeightKey::HandOverCapacity),
            1.0,
            "Monarchy's real price (peaceful_cost: {}) exceeds 3 science; reading the always-zero \
             science_cost instead would wrongly read this as affordable",
            monarchy.get().peaceful_cost
        );
    }

    /// Leader and Action cards print zero for every cost field and
    /// genuinely cost nothing to play from hand -- they must never count
    /// toward the shortfall, even at zero science. Confirmed RED by making
    /// `hand_card_affordable`'s `Leader | Action` arm return `false`
    /// instead of `true` (so two genuinely free cards are wrongly counted
    /// as a 2.0 shortfall) -- reverted after confirming.
    #[test]
    fn leader_and_action_cards_are_always_affordable_even_at_zero_science() {
        let moses = crate::cards::CardId::by_name("Moses").expect("a base-game leader");
        let action = crate::cards::CardId::by_name("Rich Land (A)").expect("a base-game Age A action card");
        assert_eq!(action.kind(), CardType::Action, "test premise: Rich Land (A) is an Action card");
        let mut state = G::new_game(2, 51);
        state.players[0].hand_civil.push(moses);
        state.players[0].hand_civil.push(action);
        state.players[0].science = 0;
        let f = features(&state, 0, None, None, false);
        assert_eq!(f.get(WeightKey::HandOverCapacity), 0.0);
    }

    /// The boundary the round-1 measurement (analysis/
    /// feature_design_gap_conditional_2026-08-26.txt, section 2d) motivates
    /// this whole key with: `hand_over_capacity` must be the CARD COUNT of
    /// the shortfall, not a 0/1 flag that merely notices one exists. Two
    /// unaffordable Farms (Irrigation, science_cost 3) plus one affordable
    /// Leader (free) at 0 science must read exactly 2.0, not 1.0 and not
    /// 3.0. Confirmed RED by reading `f.get(WeightKey::HandCivil)` alone
    /// (dropping the `- hand_affordable` term entirely, which would read
    /// 3.0 instead of 2.0) -- reverted after confirming.
    #[test]
    fn hand_over_capacity_counts_every_unaffordable_card_not_just_whether_any_exist() {
        let irrigation = crate::cards::CardId::by_name("Irrigation").expect("a base-game Age I farm tech");
        let moses = crate::cards::CardId::by_name("Moses").expect("a base-game leader");
        let mut state = G::new_game(2, 51);
        state.players[0].hand_civil.push(irrigation);
        state.players[0].hand_civil.push(irrigation);
        state.players[0].hand_civil.push(moses);
        state.players[0].science = 0;
        let f = features(&state, 0, None, None, false);
        assert_eq!(f.get(WeightKey::HandCivil), 3.0, "test setup: three cards in hand");
        assert_eq!(
            f.get(WeightKey::HandOverCapacity),
            2.0,
            "two unaffordable Irrigations and one free Leader: shortfall must be 2.0"
        );
    }

    /// `WeightKey::HappyMarginAfterNextPop` reads 0.0 when the next
    /// population increase would create no discontent at all -- the "free of
    /// consequence" baseline the key's own doc comment names. `yellow_bank`
    /// 17 has `happy_required(17) == 0` now; the decrement drops it to 16,
    /// where `happy_required(16) == 1`, still comfortably covered by
    /// `happy_extra` 5 (post-pop margin +4, strictly POSITIVE, not merely
    /// zero) -- chosen deliberately non-degenerate so this test cannot pass
    /// merely because every number involved happens to be zero. Confirmed
    /// RED by writing the raw `margin_next` (4.0) instead of the hinged
    /// `discontent_next_pop` (0.0) on the production `f.set` call --
    /// reverted after confirming (same break
    /// `happy_margin_after_next_pop_is_a_hinge_not_the_raw_post_pop_margin`
    /// below documents in full).
    #[test]
    fn happy_margin_after_next_pop_is_zero_when_the_next_worker_is_free_of_consequence() {
        assert_eq!(economy::happy_required(17), 0, "test premise: happy_required(17) == 0");
        assert_eq!(economy::happy_required(16), 1, "test premise: one pop increase raises the band to 1");
        let mut state = G::new_game(2, 51);
        state.players[0].yellow_bank = 17;
        state.players[0].happy_extra = 5;
        let f = features(&state, 0, None, None, false);
        assert_eq!(f.get(WeightKey::Discontent), 0.0, "test setup: no current discontent either");
        assert_eq!(f.get(WeightKey::HappyMarginAfterNextPop), 0.0, "post-pop margin is +4, not a shortfall");
    }

    /// The exact scenario `happy_margin_after_next_pop` exists for (design
    /// note, analysis/feature_design_gap_conditional_2026-08-26.txt,
    /// proposal 3.5): the CURRENT board is perfectly content, but the next
    /// population increase crosses a `happy_required` band edge and creates
    /// real discontent that `happy_margin`/`discontent`/`happy_surplus`
    /// (all board-as-it-stands) cannot see. `yellow_bank` 9 has
    /// `happy_required(9) == 3`; `happy_extra` 3 makes `s.happy == 3`
    /// exactly, so current `margin == 0` and `discontent == 0.0`. One more
    /// population increase drops the bank to 8, where `happy_required(8) ==
    /// 4` -- one MORE than the unchanged `s.happy == 3` can cover, so the
    /// next-pop margin is -1 and the hinge reads 1.0. Confirmed RED by
    /// changing the production line's `p.yellow_bank.saturating_sub(1)` to
    /// plain `p.yellow_bank` (i.e. not decrementing at all, so the
    /// next-pop lookup silently reused the CURRENT band): `HappyMarginAfter
    /// NextPop` dropped from 1.0 to 0.0 -- reverted after confirming.
    #[test]
    fn happy_margin_after_next_pop_sees_a_band_tip_the_current_board_state_cannot() {
        assert_eq!(economy::happy_required(9), 3, "test premise: happy_required(9) == 3");
        assert_eq!(economy::happy_required(8), 4, "test premise: one pop increase raises the band to 4");
        let mut state = G::new_game(2, 51);
        state.players[0].yellow_bank = 9;
        state.players[0].happy_extra = 3;
        let f = features(&state, 0, None, None, false);
        assert_eq!(f.get(WeightKey::Discontent), 0.0, "test setup: perfectly content RIGHT NOW");
        assert_eq!(
            f.get(WeightKey::HappyMarginAfterNextPop),
            1.0,
            "one more population increase tips happy_required from 3 to 4 against an unchanged s.happy of 3"
        );
    }

    /// `economy::increase_population`'s own floor: once the yellow bank is
    /// already empty, a further increase does not go negative, it stays at
    /// 0 (`p.yellow_granted` bookkeeping takes over instead) --
    /// `saturating_sub(1)` must mirror that floor exactly rather than
    /// underflowing. `yellow_bank` 0 has `happy_required(0) == 8`; with
    /// `happy_extra` 8 the player exactly meets it both before and after
    /// the (floored) decrement, so the hinge stays 0.0, "the bank stays at
    /// zero, not negative" (`economy.rs`'s own phrase for this floor).
    /// Confirmed RED by replacing the production line's
    /// `p.yellow_bank.saturating_sub(1)` with plain `p.yellow_bank - 1`: the
    /// test panicked on `u8` subtraction overflow (debug build) instead of
    /// computing a next-pop margin at all -- reverted after confirming.
    #[test]
    fn happy_margin_after_next_pop_floors_at_the_empty_bank_exactly_like_increase_population_does() {
        assert_eq!(economy::happy_required(0), 8, "test premise: an empty bank demands every happy face");
        let mut state = G::new_game(2, 51);
        state.players[0].yellow_bank = 0;
        state.players[0].happy_extra = 8;
        let f = features(&state, 0, None, None, false);
        assert_eq!(f.get(WeightKey::Discontent), 0.0, "test setup: exactly meets happy_required(0) right now");
        assert_eq!(f.get(WeightKey::HappyMarginAfterNextPop), 0.0, "the bank stays at zero, not negative");
    }

    /// `happy_margin_after_next_pop` is the HINGE `max(0, -(margin after the
    /// next pop))`, not the raw post-pop margin -- the key's own doc comment
    /// in `weights.rs` is explicit that a next pop merely eating into a
    /// surplus (never actually going negative) must read 0.0, the same way
    /// `Discontent` hinges `HappyMargin` today. `yellow_bank` 13
    /// (`happy_required == 1`) with `happy_extra` 5 gives a current margin
    /// of 4; one more pop drops the bank to 12 (`happy_required == 2`), a
    /// still-positive margin of 3 -- a real drop, but not a shortfall.
    /// Confirmed RED by setting the production `f.set` call to write the
    /// raw `margin_next` (3.0) instead of the hinged `discontent_next_pop`
    /// (0.0) -- reverted after confirming.
    #[test]
    fn happy_margin_after_next_pop_is_a_hinge_not_the_raw_post_pop_margin() {
        assert_eq!(economy::happy_required(13), 1, "test premise: happy_required(13) == 1");
        assert_eq!(economy::happy_required(12), 2, "test premise: one pop increase raises the band to 2");
        let mut state = G::new_game(2, 51);
        state.players[0].yellow_bank = 13;
        state.players[0].happy_extra = 5;
        let f = features(&state, 0, None, None, false);
        assert_eq!(
            f.get(WeightKey::HappyMargin),
            3.0,
            "test setup: currently in surplus (raw margin 4, clamped to HappyMargin's own 3.0 cap)"
        );
        assert_eq!(
            f.get(WeightKey::HappyMarginAfterNextPop),
            0.0,
            "the next pop still leaves a positive margin (3), which is not a shortfall"
        );
    }

    /// `WeightKey::ResourceCommitmentTurns` on an untouched fresh game --
    /// deliberately NOT a hand-built "everything is zero" state, since a
    /// fresh game already has one: `game::START_TECHS` stages Religion
    /// (Temple, printed `resource_cost: 3`) with 0 workers, so it is already
    /// a developed-but-unstaffed slot the moment the game starts, and Bronze
    /// (Mine, `production.resources: 1`) starts staffed with 2 workers, so
    /// `resource_rate` is 2 before any corruption (a fresh 16-token blue
    /// bank is nowhere near the `< 11` band `economy::corruption` charges
    /// for). No wonder is in progress, so the numerator is Religion's 3
    /// alone. Confirmed RED by dropping the `+= cost` line from
    /// `sweep_tableau`'s new `unbuilt_resource_cost_sum` accumulator (so the
    /// sum silently stayed 0 forever): `ResourceCommitmentTurns` dropped from
    /// 1.5 to 0.0 -- reverted after confirming.
    #[test]
    fn resource_commitment_turns_on_a_fresh_game_counts_religions_own_unstaffed_slot() {
        let state = G::new_game(2, 51);
        let f = features(&state, 0, None, None, false);
        assert_eq!(
            f.get(WeightKey::ResourceRate),
            2.0,
            "test premise: Bronze's 2 staffed workers * 1 resource each, no corruption on a fresh blue bank"
        );
        assert_eq!(f.get(WeightKey::WonderRemaining), 0.0, "test premise: no wonder in progress yet");
        assert_eq!(
            f.get(WeightKey::ResourceCommitmentTurns),
            1.5,
            "Religion's own unstaffed printed resource_cost (3) / a resource_rate of 2"
        );
    }

    /// The SUM, not the MIN, of every unstaffed slot's printed resource cost
    /// -- `sweep_tableau` already tracks a MIN (`unbuilt_min_resource_cost`,
    /// for the marginal-need threshold), and reusing that field here instead
    /// of a real sum is exactly the mistake this test exists to catch. Adds
    /// Iron (Mine, `resource_cost: 5`) as a second unstaffed slot beside the
    /// fresh game's own Religion (`resource_cost: 3`): the sum is 8, not
    /// `min(3, 5) == 3`. Confirmed RED by reading `sweep.unbuilt_min_
    /// resource_cost` instead of `unbuilt_resource_cost_sum` on the
    /// production `f.set` call: `ResourceCommitmentTurns` dropped from 4.0
    /// to 1.5 (`3 / 2`, the min instead of the sum) -- reverted after
    /// confirming.
    #[test]
    fn resource_commitment_turns_sums_every_unstaffed_slots_cost_not_just_the_cheapest() {
        let iron = crate::cards::CardId::by_name("Iron").expect("a base-game Age I mine");
        assert_eq!(iron.get().resource_cost, 5, "test premise: Iron's printed resource_cost is 5");
        let mut state = G::new_game(2, 51);
        state.players[0].techs.insert(iron, crate::state::TechSlot { workers: 0, stored: 0 });
        let f = features(&state, 0, None, None, false);
        assert_eq!(f.get(WeightKey::ResourceRate), 2.0, "test premise: unchanged from the fresh-game baseline");
        assert_eq!(
            f.get(WeightKey::ResourceCommitmentTurns),
            4.0,
            "Religion (3) + Iron (5) = 8 unstaffed resource obligation / a resource_rate of 2"
        );
    }

    /// The in-progress wonder's own `WonderRemaining` is HALF the numerator,
    /// not the whole of it and not absent from it -- `resource_commitment_
    /// turns` prices "wonder debt AND standing tableau debt together", the
    /// exact owner's sentence (design note section 2c) this feature exists
    /// to answer. Pyramids taken with no stages paid owes `3 + 2 + 1 == 6`
    /// (`stages: &[3, 2, 1]`, `card_table.rs`); added to the fresh game's own
    /// Religion (3), the numerator is 9, over the unchanged resource_rate of
    /// 2. Confirmed RED by dropping the `f64::from(remaining) +` term from
    /// the production `f.set` call (numerator = `unbuilt_resource_cost_sum`
    /// alone): `ResourceCommitmentTurns` dropped from 4.5 to 1.5 -- reverted
    /// after confirming.
    #[test]
    fn resource_commitment_turns_adds_the_in_progress_wonders_own_remaining_cost() {
        let pyramids = crate::cards::CardId::by_name("Pyramids").expect("a base-game wonder");
        let mut state = G::new_game(2, 51);
        state.players[0].wonder = pyramids;
        let f = features(&state, 0, None, None, false);
        assert_eq!(f.get(WeightKey::WonderRemaining), 6.0, "test premise: Pyramids owes 3 + 2 + 1 with no stages paid");
        assert_eq!(f.get(WeightKey::ResourceRate), 2.0, "test premise: unchanged from the fresh-game baseline");
        assert_eq!(
            f.get(WeightKey::ResourceCommitmentTurns),
            4.5,
            "Religion's unstaffed 3 plus Pyramids' unpaid 6 = 9, over a resource_rate of 2"
        );
    }

    /// `max(resource_rate, 1)`: a stalled economy must not divide by zero or
    /// go through a negative denominator, per the design note's own formula
    /// (analysis/feature_design_gap_conditional_2026-08-26.txt, proposal
    /// 3.2). Un-staffing the fresh game's own Bronze mine (workers -> 0)
    /// removes the ONLY resource producer, so `resource_rate` collapses to
    /// 0.0 and Bronze itself joins Religion as a second unstaffed slot
    /// (printed `resource_cost: 2`), numerator `3 + 2 == 5`. Confirmed RED
    /// by changing the production `f.set` call's denominator from
    /// `.max(1.0)` to `.max(0.0)`: `ResourceCommitmentTurns` came back `inf`
    /// (`5.0 / 0.0`, `assert_eq!` correctly treats `inf != 5.0` as a
    /// mismatch) instead of `5.0` -- reverted after confirming.
    #[test]
    fn resource_commitment_turns_floors_the_denominator_when_the_economy_has_stalled() {
        let bronze = crate::cards::CardId::by_name("Bronze").expect("a base-game Age A mine");
        assert_eq!(bronze.get().resource_cost, 2, "test premise: Bronze's printed resource_cost is 2");
        let mut state = G::new_game(2, 51);
        state.players[0].techs.get_mut(bronze).expect("Bronze is in the starting tableau").workers = 0;
        let f = features(&state, 0, None, None, false);
        assert_eq!(f.get(WeightKey::ResourceRate), 0.0, "test premise: Bronze was the only staffed producer");
        assert_eq!(
            f.get(WeightKey::ResourceCommitmentTurns),
            5.0,
            "Religion (3) + the now-unstaffed Bronze (2) = 5, over a floored denominator of 1"
        );
    }
}
