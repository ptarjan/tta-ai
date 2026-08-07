//! Costs and take-gating: what a civil action costs, what a card costs to
//! build/upgrade/develop, and which row slots a player may reach into.
//!
//! Ports `engine/actions.py` lines 43-267 (`row_cost` through `is_unit`) --
//! the cost-and-gate layer every move-generation and move-application
//! function sits on. Two other workers consume this module: one for
//! `legal_moves`/`_action_moves`/`_politics_moves`, one for `apply`/the
//! `_h_*` handlers. Neither of those is ported here.
//!
//! ## KNOWN GAPS: none left. What closed each (see the port reports)
//!
//! Every gap this module ever carried came from the same place -- a Python
//! field/value the Rust type layer (`cards.rs`, `card_table.rs`, `state.rs`,
//! `effects.rs`) did not carry yet -- and each was worked around explicitly
//! and flagged here rather than silently approximated, until the type layer
//! caught up. All five are now closed:
//!
//! - **`Card` had no `stages` field** ([`wonder_stage_cost`] panicked): the
//!   field landed in `cards.rs` and `wonder_stage_cost` reads it.
//! - **`PlayerState` had no `taken_leader_ages` field** ([`take_gate`]/
//!   [`can_take`] took it as an explicit parameter): the field landed in
//!   `state.rs`, both read it directly, and the parameter is retired.
//! - **`Card.science_cost` only ever captured `techCost`**, so every
//!   government (which prints `techCost: null` and its real develop cost in
//!   `peacefulCost`) priced as `None`: `Card::peaceful_cost` now exists and
//!   [`tech_cost`] reads it for governments.
//! - **`effects::Stats` had no `build_discount` / `tech_discount`**, so
//!   [`build_cost_for`]/[`tech_cost`] treated both pools as zero
//!   (`effects.rs`'s own KNOWN GAPS said `build_discount` was "not needed by
//!   anything in this port's scope" -- written before this module existed).
//!   `Special::BuildDiscount` now carries a real `[i16; 5]` payload indexed
//!   by `Age` (`gen_cards.py`'s `AGE_ARRAY_EFFECT_KEYS`), `Stats` accumulates
//!   it into `build_discount: [i32; 5]`, and both cost functions read the
//!   pools: [`build_cost_for`] subtracts `build_discount[card.age]` for
//!   URBAN cards only, [`tech_cost`] subtracts `tech_discount` from every
//!   technology.
//! - **`PlayerState` had no `one_time_discount` field** (Python's
//!   event-granted build/develop/population discount, `engine/events.py:360`).
//!   `state::OneTimeDiscount` now exists -- three scalars, since exactly one
//!   card in the game writes it and its schema is closed -- and all three
//!   readers are wired: [`build_cost_for`] (`URBAN_OR_PRODUCTION` only),
//!   [`tech_cost`] (every technology), and `economy::pop_food_cost`. Read
//!   `state::OneTimeDiscount`'s doc comment before touching it: it is
//!   knowingly never consumed, mirroring a Python defect, and nothing in
//!   this crate can set it until `events.rs` is ported.
//!
//! The two type-set distinctions above are load-bearing and easy to get
//! backwards: `buildDiscount` is `C.URBAN_TYPES`, the one-time build
//! discount is `C.URBAN_OR_PRODUCTION`, and `tech_discount` is gated on
//! nothing at all.
//!
//! ## A note on leader identity
//!
//! Four leaders (Hammurabi, Michelangelo, J. S. Bach, William Shakespeare)
//! are checked by NAME in Python (`p.leader == "Hammurabi"`), because leader
//! IDENTITY -- not an effect key -- is the rule: nothing in `data/*.json`
//! tags "the Hammurabi CA/MA conversion" with a machine-readable key the way
//! `Special` tags recurring one-offs. [`leader_is`] mirrors that with a
//! `&'static str` compare against `Card.name`, not a `HashMap<String, _>` --
//! DESIGN.md rule 1 is about lookup keys, and no name here is ever used as
//! one, only compared against a literal.

use crate::cards::{Card, CardId, CardType, Special};
use crate::effects;
use crate::state::{GameState, PlayerState, ROW_SIZE};

// ------------------------------------------------------------- row cost

/// Civil actions to take the card in row slot `idx` (0-based) (§2.3). The
/// printed table is 1,1,1,1,1 / 2,2,2,2 / 3,3,3,3 across the 13 slots.
pub fn row_cost(idx: usize) -> i32 {
    if idx < 5 {
        1
    } else if idx < 9 {
        2
    } else {
        3
    }
}

// ------------------------------------------------------------- helpers

/// Whether `p`'s active leader is the named leader. See this module's top
/// doc "A note on leader identity" for why this is a name compare rather
/// than a `Special` dispatch.
#[inline]
fn leader_is(p: &PlayerState, name: &str) -> bool {
    !p.leader.is_none() && p.leader.get().name == name
}

/// Total civil actions per turn (before Hammurabi's MA-as-CA conversion).
pub fn ca_total(state: &GameState, p: &PlayerState) -> i32 {
    effects::state_stats(state, p).civil_actions
}

/// Cards a civil hand may hold (§2.5): base civil actions plus any
/// `civilHandLimit` bonus. Two separate `Stats` fields added together here,
/// exactly as `effects::compute` keeps them (they are never combined inside
/// `compute` itself).
pub fn civil_hand_limit(state: &GameState, p: &PlayerState) -> i32 {
    let s = effects::state_stats(state, p);
    s.civil_actions + s.civil_hand_limit
}

/// Whether Hammurabi's once-per-turn "use one military action as one civil
/// action" conversion is still available to `p` right now.
///
/// The entitlement is per TURN, not per instant: `p.
/// hammurabi_replaced_this_turn` keeps it alive after Hammurabi has been
/// swapped out for a new leader, because the rulebook explicitly allows
/// using a leader's benefit and then replacing him on the same turn (see
/// that field's doc comment). Every other consumer of the conversion --
/// including `take_gate`'s SEPARATE `leaderTakeCivilActionDiscount`, which
/// is a continuous in-play effect and not a once-per-turn use -- still keys
/// off the live leader.
#[inline]
fn hammurabi_conversion_available(p: &PlayerState) -> bool {
    (leader_is(p, "Hammurabi") || p.hammurabi_replaced_this_turn)
        && !p.hammurabi_used
        && p.military_actions > 0
}

/// Civil actions available right now, counting Hammurabi's once-per-turn
/// MA-as-CA conversion. Unlike Python's `spare_ca`, `state` is dropped from
/// the signature: the Python body never reads it either (it only reads raw
/// per-turn pools off `p`, not `effects.state_stats`).
pub fn spare_ca(p: &PlayerState) -> i32 {
    let extra = i32::from(hammurabi_conversion_available(p));
    p.civil_actions as i32 + extra
}

/// Whether an Increase-Population / non-unit-Build / Develop action is
/// payable RIGHT NOW even with `spare_ca(p) == 0`, because Development of
/// Civil Life ("Immediately, each civilization may either: increase its
/// population; or build a farm, mine or urban building; or develop a
/// technology. It costs 1 [resource] less than usual") already banked `p.
/// one_time_discount`'s matching field. ENGINE BUG (docs/REPLAY.md Finding
/// 1, 2026-08): this ordered action is the SAME shape as an action card's
/// own ordered build/pop/develop (Rich Land/Urban Growth/Frugality, whose
/// nearly identical "pay 1 less resource" card text IS already wired
/// CA-free via `Special::FreeCivilAction`/`apply_free_civil_move`) -- rule
/// item 11's "perform it under normal rules but paying no civil ... action
/// for it" governs BOTH, but only the action-card path was ever wired to
/// the exemption. `costs::pay_ca` cannot express this fallback itself (it
/// has no way to know WHICH action a shortfall is being paid for, so it
/// cannot pick the right one of three discount fields the way Hammurabi's
/// type-agnostic MA-as-CA conversion can) -- each of the three call sites
/// (`legal.rs`'s Pop/Build/Develop gates, `apply.rs`'s `h_pop`/`do_build`/
/// `h_develop`) checks its own matching field directly instead.
#[inline]
pub fn civil_life_ca_free(discount_field: i16) -> bool {
    discount_field != 0
}

/// Pay `n` civil actions, falling back to Hammurabi's MA-as-CA conversion
/// once per turn if the civil-action pool alone is not enough. Mutates `p`.
/// `state` is dropped for the same reason as [`spare_ca`].
///
/// Panics (debug builds only, matching Python's bare `assert`, which is
/// itself only checked under normal non`-O` interpretation) if `n` civil
/// actions were not actually available -- this is an internal invariant a
/// caller must have checked via [`can_take`]/[`spare_ca`] first, not a
/// legality gate of its own.
pub fn pay_ca(p: &mut PlayerState, n: i32) {
    let used = (p.civil_actions as i32).min(n);
    p.civil_actions -= used as i8;
    let mut remaining = n - used;
    if remaining > 0 && hammurabi_conversion_available(p) {
        p.military_actions -= 1;
        p.hammurabi_used = true;
        remaining -= 1;
    }
    debug_assert_eq!(remaining, 0, "paid more civil actions than available");
}

/// Civil actions to take the card currently sitting in row slot `idx`
/// (§2.3), including the wonder take-surcharge and Hammurabi's leader
/// discount. Clamped at zero (`max(0, ...)` applied ONCE at the end, not per
/// adjustment -- matching Python; see [`can_take_gated`] for why the clamp
/// never actually binds on the leader branch with real card data).
pub fn take_cost(state: &GameState, p: &PlayerState, idx: usize) -> i32 {
    debug_assert!(idx < ROW_SIZE, "row slot {idx} out of range");
    let id = state.card_row[idx];
    debug_assert!(!id.is_none(), "take_cost: row slot {idx} is empty");
    let card = id.get();
    let mut cost = row_cost(idx);
    if card.kind == CardType::Wonder {
        if !leader_is(p, "Michelangelo") {
            cost += p.completed_wonders.len() as i32 + p.destroyed_wonders as i32;
        }
    } else if card.kind == CardType::Leader && leader_is(p, "Hammurabi") {
        cost -= 1;
    }
    cost -= leader_replacement_take_discount(card, p);
    cost.max(0)
}

/// Taj Mahal's own printed 2015 ability, read off the card SITTING IN THE ROW
/// rather than off anything in play: "If you replaced your leader this turn,
/// taking this wonder costs you 2 civil actions less."
/// (`Special::TakeCivilActionDiscountIfLeaderReplacedThisTurn`, the only card
/// in the base game that carries it.)
///
/// Unlike Michelangelo's waiver -- which only cancels the wonder surcharge and
/// so can never take a take below the row's own 1 -- this one is a flat
/// subtraction off the whole cost, and it routinely reaches the `max(0)`
/// clamp: it is the ONLY way a wonder take in this game costs zero civil
/// actions. That is not an inference; it is what real players do. Across the
/// 1,011-game BGO corpus, 150 of Taj Mahal's 317 takes carry no cost clause at
/// all (BGO prints one only for a nonzero cost) and 149 of those 150 sit in a
/// turn where that player had already replaced a leader -- while no other
/// wonder is ever taken for free even once in ~6,700 takes. See
/// `docs/REPLAY.md`'s seventh pass.
#[inline]
fn leader_replacement_take_discount(card: &Card, p: &PlayerState) -> i32 {
    if !p.replaced_leader_this_turn {
        return 0;
    }
    card.special
        .iter()
        .find_map(|s| match s {
            Special::TakeCivilActionDiscountIfLeaderReplacedThisTurn(n) => Some(*n as i32),
            _ => None,
        })
        .unwrap_or(0)
}

/// Blue special-technology category (max one per icon in play, §7.6).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SpecialIcon {
    Construction,
    Exploration,
    Warfare,
    Law,
    Other,
}

/// Which icon a card's printed effects carry. Mirrors Python's membership
/// test ("is this KEY present in `effects`", not "is its value truthy").
///
/// `buildDiscount`'s value is a per-age dict in the source data, so
/// `gen_cards.py` records it as `Special::BuildDiscount([i16; 5])` rather
/// than a `CardEffects` field; matching the VARIANT here, whatever its
/// payload, is therefore the accurate translation of Python's
/// `"buildDiscount" in eff`, not an approximation. Written as a `matches!`
/// on the variant rather than `contains(&...)` on a value precisely because
/// this is a presence test: the magnitudes are `build_cost_for`'s business,
/// and a second construction tech with different numbers must still read as
/// the Construction icon.
/// `colonizeBonus`/`militaryActions`/`civilActions` ARE flat `CardEffects`
/// fields, so "key present" is approximated as "field nonzero" -- verified
/// against `data/*.json` (2026-08-05): every special-tech that prints one of
/// these three keys prints a nonzero value, so the two conditions coincide
/// for every card that exists today.
pub fn special_icon(card: &Card) -> SpecialIcon {
    if card.special.iter().any(|s| matches!(s, Special::BuildDiscount(_))) {
        return SpecialIcon::Construction;
    }
    if card.effects.colonize_bonus != 0 {
        return SpecialIcon::Exploration;
    }
    if card.effects.military_actions != 0 {
        return SpecialIcon::Warfare;
    }
    if card.effects.civil_actions != 0 {
        return SpecialIcon::Law;
    }
    SpecialIcon::Other
}

/// Total workers on technologies of the given type. Mirrors
/// `engine/actions.py::urban_count` (the name is Python's; the function is
/// not restricted to urban types -- it is a general per-type worker count).
pub fn urban_count(p: &PlayerState, kind: CardType) -> i32 {
    p.techs
        .iter()
        .filter(|(id, _)| id.kind() == kind)
        .map(|(_, slot)| slot.workers as i32)
        .sum()
}

// ------------------------------------------------------------- take gate

/// Loop invariants of [`can_take`], computed once per move-generation pass
/// (mirrors `engine/actions.py::_take_gate`): everything in §2.5 that does
/// not depend on the row slot being tested.
///
/// `taken_leader_ages` is `p.taken_leader_ages` verbatim -- a bitmask, bit
/// `card.age as u8` set once a leader of that age has ever been taken
/// (§ one leader per age); `Age` has 5 values (A..IV) so a `u8` has ample
/// headroom. See `state.rs`'s doc comment on the field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TakeGate {
    pub have: i32,
    pub hand_full: bool,
    pub surcharge: i32,
    pub leader_discount: i32,
    pub taken_leader_ages: u8,
}

/// Build a [`TakeGate`]. `budget`, if given, overrides `spare_ca` the same
/// way Python's `_take_gate(state, p, budget=None)` does.
pub fn take_gate(state: &GameState, p: &PlayerState, budget: Option<i32>) -> TakeGate {
    let have = budget.unwrap_or_else(|| spare_ca(p));
    let surcharge = if leader_is(p, "Michelangelo") {
        0
    } else {
        p.completed_wonders.len() as i32 + p.destroyed_wonders as i32
    };
    let leader_discount = if leader_is(p, "Hammurabi") { 1 } else { 0 };
    // `hand_size_civil`, not `hand_civil.len()`: §2.5 counts CARDS, and the
    // app harness can hold a rival's hand as a bare count (`hidden_civil`)
    // without names. Identical to `hand_civil.len()` in self-play.
    let hand_full = p.hand_size_civil() as i32 >= civil_hand_limit(state, p);
    TakeGate { have, hand_full, surcharge, leader_discount, taken_leader_ages: p.taken_leader_ages }
}

/// §2.5 taking limits for one row slot, given a precomputed [`TakeGate`].
/// `name` overrides reading `state.card_row[idx]` (used once move-gen wants
/// to probe a card that is not actually sitting in the row).
///
/// Mirrors `engine/actions.py::_can_take_gated`.
pub fn can_take_gated(
    state: &GameState,
    p: &PlayerState,
    idx: usize,
    gate: &TakeGate,
    name: Option<CardId>,
) -> bool {
    let id = match name {
        Some(id) => id,
        None => {
            let id = state.card_row[idx];
            if id.is_none() {
                return false;
            }
            id
        }
    };
    let card = id.get();
    // Cost is floored at 0 by `max(0, ...)` in Python's `take_cost`, but
    // `_can_take_gated` computes its own local `cost` and never calls
    // `take_cost` or clamps it. That clamp never binds here with real data:
    // `row_cost` is >= 1 for every slot and `leader_discount` is at most 1,
    // so the only place `cost` can be reduced (the leader branch below)
    // bottoms out at exactly 0, never negative.
    let mut cost = row_cost(idx);
    if card.kind == CardType::Wonder {
        cost += gate.surcharge;
        // Taj Mahal's printed take discount (see `leader_replacement_take_
        // discount`) can drive this below zero, unlike every other adjustment
        // here -- and an affordability test that compares a NEGATIVE cost
        // against `have` still answers correctly, so no clamp is needed.
        cost -= leader_replacement_take_discount(card, p);
        if cost > gate.have {
            return false;
        }
        return p.wonder.is_none();
    }
    if card.kind == CardType::Leader {
        cost -= gate.leader_discount;
    }
    if cost > gate.have {
        return false;
    }
    // Hand limit (§2.5) applies to everything that goes to hand -- wonders
    // (handled above, always return before here) are the one exception.
    if gate.hand_full {
        return false;
    }
    if card.kind == CardType::Leader {
        return gate.taken_leader_ages & (1 << (card.age as u8)) == 0;
    }
    // §2.5/§7.1: the one-per-name rule is about TECHNOLOGIES -- civil cards
    // with a science cost. Yellow ACTION cards have none, are not
    // technologies, and several exist in multiple copies in the same deck
    // (all sharing one `CardId`, since identity is by name), so holding one
    // must not block taking another.
    if card.kind != CardType::Action
        && (p.hand_civil.contains(id) || p.techs.has(id) || id == p.government)
    {
        return false;
    }
    true
}

/// §2.5 taking limits. `budget` overrides the civil-action check.
///
/// Mirrors `engine/actions.py::can_take`; see [`take_gate`] for
/// `taken_leader_ages`.
pub fn can_take(state: &GameState, p: &PlayerState, idx: usize, budget: Option<i32>) -> bool {
    can_take_gated(state, p, idx, &take_gate(state, p, budget), None)
}

/// Diagnostic classification of WHICH check inside [`can_take_gated`]
/// rejected a take, for the `IllegalMove: Take` corpus bucket
/// (`docs/REPLAY.md`'s Take/Bid handoff). Not consumed by any legality
/// path -- `can_take_gated` remains the sole source of truth for what IS
/// legal -- this exists purely to NAME the rejecting gate so the replayer
/// can report it instead of a bare "illegal move" and a full-state dump.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TakeRejection {
    /// The row slot itself is empty.
    EmptySlot,
    /// A wonder's (surcharge- and leader-discount-adjusted) cost exceeds
    /// `gate.have`.
    WonderBudget,
    /// The wonder is affordable but `p` already has one in progress.
    WonderInProgress,
    /// A non-wonder's (leader-discount-adjusted) cost exceeds `gate.have`.
    Budget,
    /// `gate.hand_full`: civil hand already at `civil_hand_limit`.
    HandFull,
    /// A leader of this age was already taken this game.
    LeaderAgeTaken,
    /// The exact card is already in hand, on the tech board, or is the
    /// current government (technologies/governments/leaders are one-per-name;
    /// action cards are exempt, see `can_take_gated`).
    DuplicateCard,
}

/// Mirrors `can_take_gated`'s branch order EXACTLY (same `state`/`p`/`idx`/
/// `gate` inputs) so the two can never silently drift -- enforced every test
/// run by `take_rejection_agrees_with_can_take_gated` below, which checks
/// `take_rejection(..).is_none() == can_take_gated(..)` for every fixture in
/// this module's `can_take*` tests. Returns `None` iff the take is legal.
pub fn take_rejection(
    state: &GameState,
    p: &PlayerState,
    idx: usize,
    gate: &TakeGate,
) -> Option<TakeRejection> {
    let id = state.card_row[idx];
    if id.is_none() {
        return Some(TakeRejection::EmptySlot);
    }
    let card = id.get();
    let mut cost = row_cost(idx);
    if card.kind == CardType::Wonder {
        cost += gate.surcharge;
        cost -= leader_replacement_take_discount(card, p);
        if cost > gate.have {
            return Some(TakeRejection::WonderBudget);
        }
        return if p.wonder.is_none() { None } else { Some(TakeRejection::WonderInProgress) };
    }
    if card.kind == CardType::Leader {
        cost -= gate.leader_discount;
    }
    if cost > gate.have {
        return Some(TakeRejection::Budget);
    }
    if gate.hand_full {
        return Some(TakeRejection::HandFull);
    }
    if card.kind == CardType::Leader {
        return if gate.taken_leader_ages & (1 << (card.age as u8)) == 0 {
            None
        } else {
            Some(TakeRejection::LeaderAgeTaken)
        };
    }
    if card.kind != CardType::Action
        && (p.hand_civil.contains(id) || p.techs.has(id) || id == p.government)
    {
        return Some(TakeRejection::DuplicateCard);
    }
    None
}

// ------------------------------------------------------------- build/tech costs

/// Resource cost to build a worker onto technology `id`, or `None` if `id`
/// is not a buildable technology at all (mirrors `engine/effects.py::
/// build_cost`'s `cost is None` branch: `Card.resource_cost` is 0 both when
/// nothing is printed and -- unreachably for every card in today's data,
/// see `lib.rs`'s own `every_card_has_a_known_type_and_age` test -- for a
/// genuinely free build, so "0 means unbuildable" is exact here).
///
/// Ported into `costs.rs` rather than left in `effects.rs`/called through
/// it: `build_cost` is explicitly out of `effects.rs`'s scope (see that
/// module's KNOWN GAPS), and this module is not allowed to add it there.
pub fn build_cost_for(state: &GameState, p: &PlayerState, id: CardId) -> Option<i32> {
    let card = id.get();
    if card.resource_cost == 0 {
        return None;
    }
    let mut cost = card.resource_cost as i32;
    // The two discounts are gated on DIFFERENT type sets, and the difference
    // is the whole rule (Python: `C.URBAN_OR_PRODUCTION` on the first,
    // `C.URBAN_TYPES` on the second, four lines apart):
    //
    //   * the event-granted one-time discount reaches farms and mines too;
    //   * the per-age `buildDiscount` pool -- Masonry/Architecture/
    //     Engineering, "urban buildings cost less" -- does NOT.
    //
    // Collapsing them onto one predicate would silently make Engineering pay
    // for a player's farms, which is not what the card says.
    if card.kind.is_urban() || card.kind.is_production() {
        cost -= p.one_time_discount.build_resources as i32;
    }
    if card.kind.is_urban() {
        // `state_stats` is consulted only on this branch, matching Python's
        // own note that the lookup is hot (once per buildable card per
        // move-generation pass) and belongs inside the gate.
        cost -= effects::state_stats(state, p).build_discount[card.age as usize];
        if leader_is(p, "William Shakespeare") {
            // `workers > 0`, not mere presence in `p.techs`: Shakespeare's
            // ability needs the counterpart building actually BUILT (a
            // worker placed), not just its technology developed. `p.techs`
            // holds every developed technology whether or not a worker was
            // ever placed on it (developing a library and building one are
            // two separate actions/payments, §3.5 vs §3.7) -- confirmed
            // wrong against game `7520718`: Orange developed Printing Press
            // (a Library) at round 13 but did not BUILD one until round 15,
            // and paid full price for a Drama (Theater) build at round 14,
            // in between -- an `UnrecoverableHiddenInfo: build cost
            // mismatch` this reconstruction manufactured by granting the
            // discount one build too early. `effects::output_modifier_value`'s
            // sibling Shakespeare ability (`CulturePerLibraryTheaterPair`)
            // already gates on `slot.workers` via its own `workers_on`
            // helper -- this was the one place that didn't match it.
            let has_library = p.techs.iter().any(|(t, slot)| t.kind() == CardType::Library && slot.workers > 0);
            let has_theater = p.techs.iter().any(|(t, slot)| t.kind() == CardType::Theater && slot.workers > 0);
            if card.kind == CardType::Theater && has_library {
                cost -= 1;
            } else if card.kind == CardType::Library && has_theater {
                cost -= 1;
            }
        }
    }
    // ONE clamp, at the end -- never per term. A build discount larger than
    // the printed cost must not turn into a credit against the next term.
    Some(cost.max(0))
}

/// Science cost to develop technology `id`, or `None` if `id` has no
/// develop cost at all (mirrors `engine/effects.py::tech_cost`).
///
/// Governments price off `peaceful_cost` (their PEACEFUL revolution price,
/// paid through the ordinary `develop` action, §8.3), never `science_cost`
/// (which is always 0 for a government -- `techCost` is always printed
/// `null`, see `Card::science_cost`'s doc comment). This is a DIFFERENT
/// number from `Card::revolution_cost` (the VIOLENT-revolution price,
/// `apply.rs::revolution_cost`, paid with `Move::Revolution` instead) for
/// the same government -- Paul has ruled both change types must stay
/// representable at once, so neither is derived from the other here.
///
/// Both science discounts -- the standing `technologyScienceDiscount` pool
/// (`Stats::tech_discount`, from a pact) and the event-granted one-time
/// `developTechnology.science` -- apply to EVERY technology, unconditionally
/// and before the leader adjustments, governments very much included: Python
/// subtracts both from `cost` after the `government`/`else` split, not
/// inside either branch. A government that returned early here would be the
/// one card type quietly paying full price.
pub fn tech_cost(state: &GameState, p: &PlayerState, id: CardId) -> Option<i32> {
    let card = id.get();
    let printed = if card.kind == CardType::Government {
        card.peaceful_cost
    } else {
        card.science_cost
    };
    if printed == 0 {
        return None;
    }
    let mut cost = printed as i32;
    cost -= effects::state_stats(state, p).tech_discount;
    cost -= p.one_time_discount.develop_science as i32;
    if card.kind == CardType::Theater {
        if leader_is(p, "J. S. Bach") {
            cost -= 2;
        }
        // `workers > 0`: see [`build_cost_for`]'s identical Shakespeare
        // check for why mere presence in `p.techs` is wrong here too --
        // the SAME leader ability, the SAME "built, not just developed"
        // requirement, gating the develop-cost discount instead of the
        // build-cost one.
        if leader_is(p, "William Shakespeare")
            && p.techs.iter().any(|(t, slot)| t.kind() == CardType::Library && slot.workers > 0)
        {
            cost -= 1;
        }
    } else if card.kind == CardType::Library
        && leader_is(p, "William Shakespeare")
        && p.techs.iter().any(|(t, slot)| t.kind() == CardType::Theater && slot.workers > 0)
    {
        cost -= 1;
    }
    Some(cost.max(0))
}

/// `hi`'s build cost minus `lo`'s (§3.7 upgrading a production/urban card
/// in place), floored at zero. Either leg missing a build cost (e.g. `lo` is
/// the Age-A starting card with no printed `buildCost`) counts as zero,
/// matching Python's `build_cost_for(...) or 0`.
pub fn upgrade_cost(state: &GameState, p: &PlayerState, lo: CardId, hi: CardId) -> i32 {
    let a = build_cost_for(state, p, lo).unwrap_or(0);
    let b = build_cost_for(state, p, hi).unwrap_or(0);
    (b - a).max(0)
}

/// Resource cost to build the next `k` stages of the wonder currently under
/// construction (§ wonders). Mirrors `engine/actions.py::wonder_stage_cost`:
/// `stages[done:done+k]`, summed. `p.wonder_steps` is `done`; `Card::stages`
/// (Pyramids `[3, 2, 1]`, Internet `[2, 3, 4, 3, 2]`, First Space Flight
/// `[1, 2, 4, 9]`) is the per-stage price list.
///
/// Matches Python's forgiving slice rather than panicking on an over-wide
/// `k`: `stages[done:done+k]` on a Python list silently truncates at the end
/// rather than raising, so `end` is clamped to `stages.len()` here too --
/// every real caller already bounds `k` to what is left (`legal.rs`'s
/// `min(left, s.wonder_stages)`), so this is a safety net, not a rule.
pub fn wonder_stage_cost(_state: &GameState, p: &PlayerState, k: u8) -> i32 {
    debug_assert!(!p.wonder.is_none(), "wonder_stage_cost: no wonder in progress");
    let stages = p.wonder.get().stages;
    let done = (p.wonder_steps as usize).min(stages.len());
    let end = (done + k as usize).min(stages.len());
    stages[done..end].iter().map(|&s| s as i32).sum()
}

/// Whether `id` is a military unit technology (infantry/cavalry/artillery/
/// air). Mirrors `engine/actions.py::is_unit`; reuses `CardType::is_unit`
/// rather than re-deriving `C.UNIT_TYPES`'s membership by hand.
pub fn is_unit(id: CardId) -> bool {
    id.kind().is_unit()
}

/// Homer: builds or upgrades a military unit for 1 resource less, ONCE PER
/// TURN -- `p.homer_used_this_turn` gates it, not just a standing leader
/// identity check. The official leader text (`sources/
/// bga_throughtheages_material.inc.php`) is "On your turn, you have an
/// extra 1 resource for building and upgrading military units" -- AN extra
/// 1 resource (singular), not one per build/upgrade action. A previous pass
/// fixed this from a POST-payment resource GAIN (`apply::on_build_unit`,
/// called after the full price was already charged) to a discount applied
/// BEFORE the affordability check -- correct at the margin (a player with
/// exactly `raw_cost - 1` resources was illegally rejected by
/// `legal::legal_moves` under the old model even though real BGO humans are
/// observed completing exactly this build, `7523341`) -- but that fix never
/// added a per-turn cap, so a player who built/upgraded TWO units in the
/// SAME turn got the discount TWICE. Corpus-confirmed wrong (full
/// 1,011-game corpus: every turn with Homer active and 2+ same-turn unit
/// build/upgrade lines shows the `"loses N military resource"` clause on AT
/// MOST ONE of them, 45/45 turns, 0 counterexamples) -- see
/// [`spend_homer_unit_discount`]'s own test for the minimal real-game
/// repro (`7521819` round 6).
pub fn homer_unit_discount(p: &PlayerState, id: CardId) -> i32 {
    if is_unit(id) && leader_is(p, "Homer") && !p.homer_used_this_turn {
        1
    } else {
        0
    }
}

/// [`build_cost_for`] after the per-turn military-build discount pool
/// (§3.11): `p.mil_discount`, spent by [`spend_mil_discount`] once the build
/// actually happens -- and Homer's standing 1-resource discount
/// ([`homer_unit_discount`]).
pub fn build_cost_net(state: &GameState, p: &PlayerState, id: CardId) -> Option<i32> {
    let cost = build_cost_for(state, p, id)?;
    if is_unit(id) {
        Some((cost - p.mil_discount as i32 - homer_unit_discount(p, id)).max(0))
    } else {
        Some(cost)
    }
}

/// [`upgrade_cost`] after `p.mil_discount` and Homer's discount, gated on
/// `lo`'s type (matching Python exactly -- the discount pool is checked
/// against the FROM card, not the TO card, since an upgrade is priced as
/// "how much MORE resource to reach `hi`", and that marginal cost is what
/// the unit-build discount pool exists to reduce).
pub fn upgrade_cost_net(state: &GameState, p: &PlayerState, lo: CardId, hi: CardId) -> i32 {
    let cost = upgrade_cost(state, p, lo, hi);
    if is_unit(lo) {
        (cost - p.mil_discount as i32 - homer_unit_discount(p, lo)).max(0)
    } else {
        cost
    }
}

/// [`tech_cost`] after the per-turn military-tech science discount pool
/// (Winston Churchill's military option: 3 science usable only to develop
/// military unit technologies, `docs/AUDIT_HISTORY.md`).
pub fn tech_cost_net(state: &GameState, p: &PlayerState, id: CardId) -> Option<i32> {
    let cost = tech_cost(state, p, id)?;
    if is_unit(id) {
        Some((cost - p.mil_sci_discount as i32).max(0))
    } else {
        Some(cost)
    }
}

/// Consume as much of the military-tech science discount pool as this
/// development uses; returns the science actually still owed. Mutates `p`
/// -- unlike every cost function above, this is a SPEND, not a query, which
/// is why it takes `&mut PlayerState` rather than `&PlayerState`.
pub fn spend_mil_sci_discount(p: &mut PlayerState, id: CardId, raw: i32) -> i32 {
    if !is_unit(id) || p.mil_sci_discount <= 0 {
        return raw;
    }
    let used = (p.mil_sci_discount as i32).min(raw);
    p.mil_sci_discount -= used as i16;
    raw - used
}

/// Consume as much of the military-build resource discount pool as this
/// build/upgrade uses; returns the resources actually still owed. Mutates
/// `p` -- see [`spend_mil_sci_discount`].
pub fn spend_mil_discount(p: &mut PlayerState, id: CardId, raw: i32) -> i32 {
    if !is_unit(id) || p.mil_discount <= 0 {
        return raw;
    }
    let used = (p.mil_discount as i32).min(raw);
    p.mil_discount -= used as i16;
    if std::env::var("REPLAY_DEBUG_ALL").is_ok() && used != 0 {
        eprintln!("DEBUG mil_discount site=spend_mil_discount(id={id:?}) -= {used} -> {}", p.mil_discount);
    }
    raw - used
}

/// Applies [`homer_unit_discount`] at payment time and, unlike the OLD
/// version of this function, now DOES mutate `p`: the once-per-turn nature
/// of Homer's discount (this module's own `homer_unit_discount` doc
/// comment) means using it has to be recorded somewhere, exactly like
/// [`spend_mil_discount`]/[`spend_mil_sci_discount`] above -- the only
/// difference is the "pool" is a single-use flag
/// (`p.homer_used_this_turn`), not a decrementable count. Only marks it used
/// when the discount is ACTUALLY the thing that reduced `raw` (an already-
/// free build, `raw == 0`, e.g. a Rich Land/Urban Growth free civil build,
/// leaves the once-per-turn allowance untouched for a LATER real payment
/// this same turn -- corpus-confirmed: `homer_unit_discount`'s own doc
/// comment's 45-turn sweep found 10 turns where the FIRST unit action of
/// the turn carried no cost clause at all and the discount showed up on the
/// SECOND instead).
pub fn spend_homer_unit_discount(p: &mut PlayerState, id: CardId, raw: i32) -> i32 {
    let discount = homer_unit_discount(p, id);
    if discount > 0 && raw > 0 {
        p.homer_used_this_turn = true;
    }
    (raw - discount).max(0)
}

// ============================================================== tests ====

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::Age;
    use crate::state::{CardList, GameState, Phase, PlayerState, Tableau, TechSlot, MAX_PLAYERS};

    fn card(name: &str) -> CardId {
        CardId::by_name(name).unwrap_or_else(|| panic!("no such card: {name}"))
    }

    /// A player with nothing but a government. Mirrors `effects.rs`'s test
    /// helper of the same shape (duplicated rather than shared: that one is
    /// private to `effects.rs`'s own `#[cfg(test)]` module).
    fn blank_player(idx: u8, government: CardId) -> PlayerState {
        PlayerState {
            idx,
            techs: Tableau::new(),
            government,
            leader: CardId::NONE,
            wonder: CardId::NONE,
            wonder_steps: 0,
            completed_wonders: CardList::new(),
            destroyed_wonders: 0,
            homer_wonder: CardId::NONE,
            tactic: CardId::NONE,
            tactic_exclusive: false,
            colonies: CardList::new(),
            flipped_wonders: CardList::new(),
            pacts: crate::state::PactList::new(),
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
            hammurabi_replaced_this_turn: false,
            replaced_leader_this_turn: false,
            trade_food_as_resource_used_this_turn: 0,
            trade_resource_as_food_used_this_turn: 0,
            churchill_used: false,
            homer_used_this_turn: false,
            bach_upgrade_used: false,
            ocean_liners_used: false,
            caesar_double_politics_used: false,
            skip_next_politics: false,
            caesar_second_politics: false,
            peeked_event: CardId::NONE,
            ca_penalty_next_turn: 0,
            mil_discount: 0,
            mil_sci_discount: 0,
            one_time_discount: crate::state::OneTimeDiscount::default(),
            resigned: false,
            taken_leader_ages: 0,
            war_declared_by_me: CardId::NONE,
            war_target: 0,
            wars_declared_on_me: [CardId::NONE; MAX_PLAYERS],
        }
    }

    fn blank_state(num_players: u8, players: [PlayerState; MAX_PLAYERS]) -> GameState {
        GameState {
            num_players,
            seed: 0,
            players,
            current: 0,
            turn: 1,
            round: 1,
            start_player: 0,
            age_civil: Age::A,
            age_military: Age::A,
            civil_deck: CardList::new(),
            military_deck: CardList::new(),
            card_row: [CardId::NONE; ROW_SIZE],
            future_events: CardList::new(),
            current_events: CardList::new(),
            past_events: CardList::new(),
            current_events_age: Age::A,
            seeded_by: [crate::state::NOT_SEEDED; crate::cards::NUM_CARDS],
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

    fn one_player_state(p: PlayerState) -> GameState {
        let filler = || blank_player(0, card("Despotism"));
        let mut players = [filler(), filler(), filler(), filler()];
        players[0] = p;
        blank_state(4, players)
    }

    // ------------------------------------------------------------ row_cost

    /// §2.3's printed board table: assert the actual values, not the
    /// formula that happens to produce them.
    #[test]
    fn row_cost_matches_the_printed_board() {
        let expected = [1, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3];
        for (idx, &want) in expected.iter().enumerate() {
            assert_eq!(row_cost(idx), want, "slot {idx}");
        }
    }

    // ---------------------------------------------------------- ca_total

    #[test]
    fn ca_total_reads_government_civil_actions() {
        let p = blank_player(0, card("Despotism"));
        let state = one_player_state(p);
        assert_eq!(ca_total(&state, &state.players[0]), 4);
    }

    #[test]
    fn civil_hand_limit_adds_civil_actions_and_the_bonus() {
        let mut p = blank_player(0, card("Despotism"));
        // Library of Alexandria: civilHandLimit +1, militaryHandLimit +1.
        p.completed_wonders.push(card("Library of Alexandria"));
        let state = one_player_state(p);
        assert_eq!(civil_hand_limit(&state, &state.players[0]), 4 + 1);
    }

    // ------------------------------------------------------------ spare_ca

    #[test]
    fn spare_ca_is_just_the_pool_without_hammurabi() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 3;
        p.military_actions = 2;
        assert_eq!(spare_ca(&p), 3);
    }

    #[test]
    fn spare_ca_adds_one_for_unused_hammurabi_conversion() {
        let mut p = blank_player(0, card("Despotism"));
        p.leader = card("Hammurabi");
        p.civil_actions = 0;
        p.military_actions = 1;
        assert_eq!(spare_ca(&p), 1, "0 CA + Hammurabi's 1 MA-as-CA");
    }

    #[test]
    fn spare_ca_ignores_hammurabi_once_used_or_out_of_ma() {
        let mut p = blank_player(0, card("Despotism"));
        p.leader = card("Hammurabi");
        p.civil_actions = 0;
        p.military_actions = 1;
        p.hammurabi_used = true;
        assert_eq!(spare_ca(&p), 0, "already used this turn");
        p.hammurabi_used = false;
        p.military_actions = 0;
        assert_eq!(spare_ca(&p), 0, "no military action left to convert");
    }

    // -------------------------------------------------------------- pay_ca

    #[test]
    fn pay_ca_spends_the_civil_pool_first() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 3;
        pay_ca(&mut p, 2);
        assert_eq!(p.civil_actions, 1);
        assert!(!p.hammurabi_used);
    }

    #[test]
    fn pay_ca_falls_back_to_hammurabi_conversion() {
        let mut p = blank_player(0, card("Despotism"));
        p.leader = card("Hammurabi");
        p.civil_actions = 1;
        p.military_actions = 2;
        pay_ca(&mut p, 2);
        assert_eq!(p.civil_actions, 0);
        assert_eq!(p.military_actions, 1);
        assert!(p.hammurabi_used);
    }

    /// The guard this asserts is a `debug_assert!`, which `--release` strips,
    /// so in release the call simply does not panic and `should_panic` fails
    /// the test rather than the code. Gated rather than promoted to a real
    /// `assert!`: underfunding is an engine bug, not a game state, and the
    /// release build is the one the league runs -- paying for that check on
    /// every action spent, forever, to catch a bug the debug build already
    /// catches, is the wrong trade. Found 2026-08-05 by the card-coverage
    /// pass, which ran `cargo test --release` and hit this.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "paid more civil actions than available")]
    fn pay_ca_panics_if_underfunded() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 1;
        pay_ca(&mut p, 2);
    }

    // ----------------------------------------------------------- take_cost

    #[test]
    fn take_cost_is_bare_row_cost_with_no_leader_or_wonder() {
        let p = blank_player(0, card("Despotism"));
        let mut state = one_player_state(p);
        state.card_row[6] = card("Bronze"); // a plain tech, slot cost 2
        assert_eq!(take_cost(&state, &state.players[0], 6), 2);
    }

    /// The `destroyed_wonders = 1` here is a hand-built state, not a reachable
    /// one: no base-game card destroys a completed wonder (see the field's doc
    /// on `PlayerState`). It is pinned anyway because §2.3 counts the term and
    /// the expansion will reach it.
    #[test]
    fn take_cost_wonder_adds_completed_and_destroyed_surcharge() {
        let mut p = blank_player(0, card("Despotism"));
        p.completed_wonders.push(card("Pyramids"));
        p.destroyed_wonders = 1;
        let mut state = one_player_state(p);
        state.card_row[0] = card("Colossus"); // wonder, slot cost 1
        let p = &state.players[0];
        assert_eq!(take_cost(&state, p, 0), 1 + 1 + 1);
    }

    #[test]
    fn take_cost_michelangelo_waives_the_wonder_surcharge() {
        let mut p = blank_player(0, card("Despotism"));
        p.leader = card("Michelangelo");
        p.completed_wonders.push(card("Pyramids"));
        let mut state = one_player_state(p);
        state.card_row[0] = card("Colossus");
        let p = &state.players[0];
        assert_eq!(take_cost(&state, p, 0), 1);
    }

    #[test]
    fn take_cost_hammurabi_discounts_leaders_by_one_floored_at_zero() {
        let mut p = blank_player(0, card("Despotism"));
        p.leader = card("Hammurabi");
        let mut state = one_player_state(p);
        state.card_row[0] = card("Napoleon Bonaparte"); // leader, slot cost 1
        let p = &state.players[0];
        assert_eq!(take_cost(&state, p, 0), 0, "1 - 1 floored at 0, not -1");
    }

    /// Taj Mahal's printed 2015 clause, and the reason it matters: it is the
    /// only thing in the base game that can make a wonder take cost NOTHING.
    /// The row's cheapest slot is 1 and the wonder surcharge only ever adds,
    /// so 1 (slot) + 1 (one completed wonder) - 2 = 0.
    #[test]
    fn taking_taj_mahal_costs_two_civil_actions_less_when_a_leader_was_replaced_this_turn() {
        let mut p = blank_player(0, card("Despotism"));
        p.completed_wonders.push(card("Pyramids"));
        p.replaced_leader_this_turn = true;
        let mut state = one_player_state(p);
        state.card_row[0] = card("Taj Mahal"); // slot cost 1, surcharge 1
        assert_eq!(take_cost(&state, &state.players[0], 0), 0);
    }

    /// The discount is a flat subtraction off the WHOLE cost, not a waiver of
    /// the surcharge alone: from an expensive slot it still leaves something
    /// to pay.
    #[test]
    fn taking_taj_mahal_from_an_expensive_slot_after_a_replacement_still_costs_the_remainder() {
        let mut p = blank_player(0, card("Despotism"));
        p.replaced_leader_this_turn = true;
        let mut state = one_player_state(p);
        state.card_row[10] = card("Taj Mahal"); // slot cost 3, no surcharge
        assert_eq!(take_cost(&state, &state.players[0], 10), 1);
    }

    #[test]
    fn taking_taj_mahal_costs_full_price_when_no_leader_was_replaced_this_turn() {
        let mut p = blank_player(0, card("Despotism"));
        p.completed_wonders.push(card("Pyramids"));
        let mut state = one_player_state(p);
        state.card_row[0] = card("Taj Mahal");
        assert_eq!(take_cost(&state, &state.players[0], 0), 2);
    }

    /// The negative control the corpus itself provides: no OTHER wonder is
    /// ever taken for free, in ~6,700 real human wonder takes.
    #[test]
    fn no_wonder_other_than_taj_mahal_is_discounted_by_a_leader_replacement() {
        let mut p = blank_player(0, card("Despotism"));
        p.replaced_leader_this_turn = true;
        let mut state = one_player_state(p);
        state.card_row[0] = card("Colossus");
        assert_eq!(take_cost(&state, &state.players[0], 0), 1);
    }

    /// `can_take_gated` prices the row independently of `take_cost` (it is
    /// move-generation's own affordability check), so the discount has to be
    /// wired into BOTH or `legal_moves` will refuse to offer a take the
    /// player can afford.
    #[test]
    fn move_generation_offers_taj_mahal_with_no_civil_actions_left_after_a_replacement() {
        let mut p = blank_player(0, card("Despotism"));
        p.completed_wonders.push(card("Pyramids"));
        p.replaced_leader_this_turn = true;
        p.civil_actions = 0;
        let mut state = one_player_state(p);
        state.card_row[0] = card("Taj Mahal");
        let p = &state.players[0];
        let gate = take_gate(&state, p, None);
        assert!(can_take_gated(&state, p, 0, &gate, None));
    }

    // -------------------------------------------------------- special_icon

    #[test]
    fn special_icon_covers_every_category() {
        assert_eq!(special_icon(card("Masonry").get()), SpecialIcon::Construction);
        assert_eq!(special_icon(card("Cartography").get()), SpecialIcon::Exploration);
        assert_eq!(special_icon(card("Warfare").get()), SpecialIcon::Warfare);
        assert_eq!(special_icon(card("Code of Laws").get()), SpecialIcon::Law);
        assert_eq!(special_icon(card("Bronze").get()), SpecialIcon::Other);
    }

    // -------------------------------------------------------- urban_count

    #[test]
    fn urban_count_sums_workers_of_one_type() {
        let mut p = blank_player(0, card("Despotism"));
        p.techs.insert(card("Bronze"), TechSlot { workers: 2, stored: 0 });
        p.techs.insert(card("Agriculture"), TechSlot { workers: 3, stored: 0 });
        assert_eq!(urban_count(&p, CardType::Mine), 2);
        assert_eq!(urban_count(&p, CardType::Farm), 3);
        assert_eq!(urban_count(&p, CardType::Lab), 0);
    }

    // ------------------------------------------------------------ take gate

    #[test]
    fn can_take_respects_the_civil_action_budget() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 10;
        let mut state = one_player_state(p);
        state.card_row[9] = card("Bronze"); // slot cost 3
        let p = &state.players[0];
        assert!(can_take(&state, p, 9, None));
        assert!(can_take(&state, p, 9, Some(3)));
        assert!(!can_take(&state, p, 9, Some(2)), "budget one short");
    }

    #[test]
    fn can_take_blocks_on_a_full_civil_hand() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 10;
        for _ in 0..4 {
            p.hand_civil.push(card("Irrigation")); // fills to the 4 CA limit
        }
        let mut state = one_player_state(p);
        state.card_row[0] = card("Selective Breeding");
        let p = &state.players[0];
        assert!(!can_take(&state, p, 0, None), "hand at civil_hand_limit");
    }

    #[test]
    fn can_take_blocks_a_second_copy_of_a_technology_but_not_an_action_card() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 10;
        p.hand_civil.push(card("Irrigation"));
        let mut state = one_player_state(p);
        state.card_row[0] = card("Irrigation");
        // "Rich Land (A)" prints 2 physical copies in the Age A deck under
        // ONE `CardId` (`_disambiguate` suffixes by AGE, not by copy) -- an
        // action card, not a tech, so holding one must not block taking the
        // other.
        state.card_row[1] = card("Rich Land (A)");
        state.players[0].hand_civil.push(card("Rich Land (A)"));
        let p = &state.players[0];
        assert!(!can_take(&state, p, 0, None), "already holding an Irrigation");
        assert!(can_take(&state, p, 1, None), "action cards are exempt from one-per-name");
    }

    #[test]
    fn can_take_wonder_requires_no_wonder_in_progress() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 10;
        let mut state = one_player_state(p);
        state.card_row[0] = card("Colossus");
        assert!(can_take(&state, &state.players[0], 0, None));
        state.players[0].wonder = card("Pyramids");
        assert!(!can_take(&state, &state.players[0], 0, None), "already building a wonder");
    }

    #[test]
    fn can_take_leader_respects_taken_leader_ages() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 10;
        let mut state = one_player_state(p);
        let leader_slot = card("Napoleon Bonaparte");
        state.card_row[0] = leader_slot;
        let age_bit = 1u8 << (leader_slot.get().age as u8);
        assert!(can_take(&state, &state.players[0], 0, None));
        state.players[0].taken_leader_ages = age_bit;
        assert!(
            !can_take(&state, &state.players[0], 0, None),
            "that age's leader was already taken"
        );
    }

    /// `take_rejection` mirrors `can_take_gated`'s branch order by
    /// construction; this pins that the two never drift by re-running EVERY
    /// scenario the `can_take*` tests above already built (budget, hand-full,
    /// duplicate tech, wonder-in-progress, leader-age) and checking
    /// `take_rejection(..).is_none()` agrees with `can_take_gated(..)` on
    /// each. A future edit to one function without the other trips this.
    #[test]
    fn take_rejection_agrees_with_can_take_gated_on_every_gate() {
        fn check(state: &GameState, p: &PlayerState, idx: usize) {
            let gate = take_gate(state, p, None);
            let legal = can_take_gated(state, p, idx, &gate, None);
            let rejection = take_rejection(state, p, idx, &gate);
            assert_eq!(rejection.is_none(), legal, "idx={idx} rejection={rejection:?}");
        }

        // Budget: affordable vs. one short (row_cost(9) == 3).
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 10;
        let mut state = one_player_state(p);
        state.card_row[9] = card("Bronze");
        check(&state, &state.players[0], 9);
        state.players[0].civil_actions = 2;
        check(&state, &state.players[0], 9);

        // Hand full.
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 10;
        for _ in 0..4 {
            p.hand_civil.push(card("Irrigation"));
        }
        let mut state = one_player_state(p);
        state.card_row[0] = card("Selective Breeding");
        check(&state, &state.players[0], 0);

        // Duplicate technology vs. exempt action card.
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 10;
        p.hand_civil.push(card("Irrigation"));
        let mut state = one_player_state(p);
        state.card_row[0] = card("Irrigation");
        state.card_row[1] = card("Rich Land (A)");
        state.players[0].hand_civil.push(card("Rich Land (A)"));
        check(&state, &state.players[0], 0);
        check(&state, &state.players[0], 1);

        // Wonder in progress.
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 10;
        let mut state = one_player_state(p);
        state.card_row[0] = card("Colossus");
        check(&state, &state.players[0], 0);
        state.players[0].wonder = card("Pyramids");
        check(&state, &state.players[0], 0);

        // Leader age already taken.
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 10;
        let mut state = one_player_state(p);
        let leader_slot = card("Napoleon Bonaparte");
        state.card_row[0] = leader_slot;
        check(&state, &state.players[0], 0);
        state.players[0].taken_leader_ages = 1u8 << (leader_slot.get().age as u8);
        check(&state, &state.players[0], 0);

        // Empty slot.
        let p = blank_player(0, card("Despotism"));
        let state = one_player_state(p);
        check(&state, &state.players[0], 0);
    }

    // ------------------------------------------------------- build_cost_for

    #[test]
    fn build_cost_for_reads_the_printed_resource_cost() {
        let p = blank_player(0, card("Despotism"));
        let state = one_player_state(p);
        assert_eq!(build_cost_for(&state, &state.players[0], card("Irrigation")), Some(4));
    }

    #[test]
    fn build_cost_for_none_when_unbuildable() {
        let p = blank_player(0, card("Despotism"));
        let state = one_player_state(p);
        // Governments print no buildCost at all.
        assert_eq!(build_cost_for(&state, &state.players[0], card("Monarchy")), None);
    }

    #[test]
    fn build_cost_for_shakespeare_discounts_theater_with_a_library_present() {
        let mut p = blank_player(0, card("Despotism"));
        p.leader = card("William Shakespeare");
        p.techs.insert(card("Printing Press"), TechSlot { workers: 1, stored: 0 });
        let state = one_player_state(p);
        // Drama build cost 4, minus 1 for Shakespeare + a library in play.
        assert_eq!(build_cost_for(&state, &state.players[0], card("Drama")), Some(3));
    }

    // ------------------------------------ build_cost_for: the two discounts

    /// Masonry prints `buildDiscount {I: 1, II: 1, III: 1}`; Bread and
    /// Circuses is an ARENA (urban) of Age I with a printed build cost of 3.
    /// This is the exact case the (since-retired) Python-parity differential
    /// suite had allowlisted: Python charges 2, Rust used to charge 3.
    #[test]
    fn build_cost_for_applies_the_per_age_build_discount_to_urban_cards() {
        let mut p = blank_player(0, card("Despotism"));
        p.techs.insert(card("Masonry"), TechSlot { workers: 0, stored: 0 });
        let state = one_player_state(p);
        assert_eq!(
            build_cost_for(&state, &state.players[0], card("Bread and Circuses")),
            Some(2),
            "3 printed - 1 for Masonry's Age I entry"
        );
    }

    /// The discount is indexed by the CARD's age, not by the player's
    /// current age or by "any age the tech prints". Masonry's array is
    /// `[0, 1, 1, 1, 0]`, so an Age A temple gets nothing and an Age IV
    /// card would too -- indexing by the wrong thing would silently make
    /// Religion cost 2.
    #[test]
    fn build_cost_for_build_discount_is_indexed_by_the_cards_own_age() {
        let mut p = blank_player(0, card("Despotism"));
        p.techs.insert(card("Masonry"), TechSlot { workers: 0, stored: 0 });
        let state = one_player_state(p);
        // Religion: temple, Age A, printed build cost 3. Masonry's Age A
        // entry is 0 (the card says "Age A unchanged").
        assert_eq!(build_cost_for(&state, &state.players[0], card("Religion")), Some(3));
    }

    /// Engineering prints `{I: 1, II: 2, III: 3}` -- a per-age array, not one
    /// number. Reading the wrong slot would make an Age III lab cost 10
    /// instead of 8, or an Age I arena cost 0 instead of 2.
    #[test]
    fn build_cost_for_build_discount_reads_a_different_amount_per_age() {
        let mut p = blank_player(0, card("Despotism"));
        p.techs.insert(card("Engineering"), TechSlot { workers: 0, stored: 0 });
        let state = one_player_state(p);
        let s = &state.players[0];
        assert_eq!(build_cost_for(&state, s, card("Bread and Circuses")), Some(3 - 1), "Age I");
        assert_eq!(build_cost_for(&state, s, card("Team Sports")), Some(5 - 2), "Age II");
        assert_eq!(build_cost_for(&state, s, card("Professional Sports")), Some(8 - 3), "Age III");
    }

    /// The two build discounts are gated on DIFFERENT type sets -- Python's
    /// `C.URBAN_OR_PRODUCTION` for the event-granted one-time discount,
    /// `C.URBAN_TYPES` for the per-age pool, four lines apart. A farm must
    /// get the first and NOT the second: Irrigation is an Age I farm costing
    /// 4, so the right answer is 3, and collapsing the two predicates would
    /// give 2 (Masonry paying for a farm, which the card does not say).
    #[test]
    fn build_cost_for_production_card_gets_the_one_time_discount_but_not_the_per_age_pool() {
        let mut p = blank_player(0, card("Despotism"));
        p.techs.insert(card("Masonry"), TechSlot { workers: 0, stored: 0 });
        p.one_time_discount.build_resources = 1;
        let state = one_player_state(p);
        assert_eq!(build_cost_for(&state, &state.players[0], card("Irrigation")), Some(3));
    }

    /// The same player, same discounts, on an URBAN card of the same age:
    /// both apply. Read together with the test above, this pins down that
    /// the difference is the TYPE SET and nothing else.
    #[test]
    fn build_cost_for_urban_card_gets_both_discounts() {
        let mut p = blank_player(0, card("Despotism"));
        p.techs.insert(card("Masonry"), TechSlot { workers: 0, stored: 0 });
        p.one_time_discount.build_resources = 1;
        let state = one_player_state(p);
        assert_eq!(
            build_cost_for(&state, &state.players[0], card("Bread and Circuses")),
            Some(1),
            "3 printed - 1 one-time - 1 Masonry"
        );
    }

    /// A unit is neither urban nor production, so neither build discount
    /// touches it (the military pool `build_cost_net` spends is the one that
    /// does -- see the `*_net` tests below).
    #[test]
    fn build_cost_for_ignores_both_discounts_for_a_military_unit() {
        let mut p = blank_player(0, card("Despotism"));
        p.techs.insert(card("Engineering"), TechSlot { workers: 0, stored: 0 });
        p.one_time_discount.build_resources = 1;
        let state = one_player_state(p);
        assert_eq!(build_cost_for(&state, &state.players[0], card("Swordsmen")), Some(3));
    }

    /// ONE clamp, at the very end. With a discount stack bigger than the
    /// printed cost the answer is 0, not a negative number and not a credit
    /// carried into the next term -- Python's `cost if cost > 0 else 0` runs
    /// once, after every subtraction.
    #[test]
    fn build_cost_for_clamps_at_zero_once_not_per_term() {
        let mut p = blank_player(0, card("Despotism"));
        p.techs.insert(card("Engineering"), TechSlot { workers: 0, stored: 0 });
        p.one_time_discount.build_resources = 9;
        let state = one_player_state(p);
        assert_eq!(build_cost_for(&state, &state.players[0], card("Bread and Circuses")), Some(0));
    }

    /// An unbuildable card is `None` BEFORE any discount is considered: a
    /// discount must never conjure a build cost for a card that prints none.
    #[test]
    fn build_cost_for_stays_none_for_an_unbuildable_card_even_with_discounts() {
        let mut p = blank_player(0, card("Despotism"));
        p.one_time_discount.build_resources = 1;
        let state = one_player_state(p);
        assert_eq!(build_cost_for(&state, &state.players[0], card("Monarchy")), None);
    }

    #[test]
    fn build_cost_for_shakespeare_needs_the_matching_building_present() {
        let mut p = blank_player(0, card("Despotism"));
        p.leader = card("William Shakespeare");
        let state = one_player_state(p);
        // No library in play: full price.
        assert_eq!(build_cost_for(&state, &state.players[0], card("Drama")), Some(4));
    }

    /// A library the player has DEVELOPED (paid its science cost) but never
    /// BUILT (no worker placed, `workers: 0`) must not count as "in play"
    /// -- these are two separate actions/payments (§3.5 develop, §3.7
    /// build) and BGO corpus game `7520718` shows a real human paying full
    /// price for a Drama build with Printing Press developed-but-unbuilt at
    /// the time. Regression test for the `UnrecoverableHiddenInfo: build
    /// cost mismatch for Drama` this reconstruction manufactured before the
    /// fix (`docs/REPLAY.md`).
    #[test]
    fn build_cost_for_shakespeare_ignores_a_library_that_is_developed_but_not_built() {
        let mut p = blank_player(0, card("Despotism"));
        p.leader = card("William Shakespeare");
        p.techs.insert(card("Printing Press"), TechSlot { workers: 0, stored: 0 });
        let state = one_player_state(p);
        assert_eq!(build_cost_for(&state, &state.players[0], card("Drama")), Some(4));
    }

    // ----------------------------------------------------------- tech_cost

    #[test]
    fn tech_cost_reads_the_printed_science_cost() {
        let p = blank_player(0, card("Despotism"));
        let state = one_player_state(p);
        assert_eq!(tech_cost(&state, &state.players[0], card("Irrigation")), Some(3));
    }

    #[test]
    fn tech_cost_is_none_for_despotism_which_prints_no_peaceful_cost() {
        let p = blank_player(0, card("Despotism"));
        let state = one_player_state(p);
        // Despotism prints `peacefulCost: null` for real -- the one
        // government that is genuinely undevelopable this way (it is the
        // starting government, never taken from hand).
        assert_eq!(tech_cost(&state, &state.players[0], card("Despotism")), None);
    }

    #[test]
    fn tech_cost_reads_peaceful_cost_for_governments() {
        let p = blank_player(0, card("Despotism"));
        let state = one_player_state(p);
        // data/cards_civil.json peacefulCost, verified 2026-08-05.
        assert_eq!(tech_cost(&state, &state.players[0], card("Monarchy")), Some(8));
        assert_eq!(tech_cost(&state, &state.players[0], card("Theocracy")), Some(6));
        assert_eq!(
            tech_cost(&state, &state.players[0], card("Constitutional Monarchy")),
            Some(12)
        );
        assert_eq!(tech_cost(&state, &state.players[0], card("Republic")), Some(13));
        assert_eq!(tech_cost(&state, &state.players[0], card("Communism")), Some(19));
        assert_eq!(tech_cost(&state, &state.players[0], card("Fundamentalism")), Some(18));
        assert_eq!(tech_cost(&state, &state.players[0], card("Democracy")), Some(17));
    }

    // ------------------------------------------- tech_cost: the discounts

    /// A player with a `technologyScienceDiscount` pact in play. Scientific
    /// Cooperation prints `bothPlayers { technologyScienceDiscount: 2 }`, so
    /// both parties get 2 science off every technology.
    fn state_with_science_pact() -> GameState {
        let mut p0 = blank_player(0, card("Despotism"));
        p0.pacts.push(crate::state::Pact {
            card: card("Scientific Cooperation"),
            owner: 0,
            partner: 1,
            a: 0,
            b: 1,
        });
        let filler = || blank_player(1, card("Despotism"));
        let mut players = [p0, filler(), filler(), filler()];
        players[1].idx = 1;
        blank_state(4, players)
    }

    #[test]
    fn tech_cost_applies_the_pact_science_discount_to_an_ordinary_technology() {
        let state = state_with_science_pact();
        // Irrigation's printed techCost is 3, minus the pact's 2.
        assert_eq!(tech_cost(&state, &state.players[0], card("Irrigation")), Some(1));
    }

    /// Python subtracts `tech_discount` AFTER the government/non-government
    /// split, so a government pays it too. Returning `peaceful_cost`
    /// straight off the card -- which this function used to do -- made
    /// governments the one card type quietly paying full price.
    #[test]
    fn tech_cost_applies_the_science_discounts_to_governments_too() {
        let state = state_with_science_pact();
        // Monarchy's peacefulCost is 8, minus the pact's 2.
        assert_eq!(tech_cost(&state, &state.players[0], card("Monarchy")), Some(6));

        let mut p = blank_player(0, card("Despotism"));
        p.one_time_discount.develop_science = 1;
        let state = one_player_state(p);
        assert_eq!(tech_cost(&state, &state.players[0], card("Monarchy")), Some(7));
    }

    /// The one-time develop discount is gated on NOTHING -- not a type set,
    /// unlike its build-side sibling. A unit technology gets it just as a
    /// lab does.
    #[test]
    fn tech_cost_one_time_develop_discount_applies_to_every_technology_type() {
        let mut p = blank_player(0, card("Despotism"));
        p.one_time_discount.develop_science = 1;
        let state = one_player_state(p);
        let s = &state.players[0];
        assert_eq!(tech_cost(&state, s, card("Irrigation")), Some(3 - 1), "a farm");
        assert_eq!(tech_cost(&state, s, card("Alchemy")), Some(4 - 1), "a lab");
        assert_eq!(tech_cost(&state, s, card("Swordsmen")), Some(4 - 1), "a military unit");
        assert_eq!(tech_cost(&state, s, card("Masonry")), Some(3 - 1), "a special tech");
    }

    /// Both science discounts stack, and the clamp is applied ONCE at the
    /// end -- a discount stack larger than the printed cost is 0, never
    /// negative, and never a credit against the leader adjustments below it.
    #[test]
    fn tech_cost_stacks_both_discounts_and_clamps_at_zero_once() {
        let mut state = state_with_science_pact();
        state.players[0].one_time_discount.develop_science = 1;
        // Masonry's printed techCost is 3; 3 - 2 (pact) - 1 (one-time) = 0,
        // and a fourth point of discount would still be 0, not -1.
        assert_eq!(tech_cost(&state, &state.players[0], card("Masonry")), Some(0));
        state.players[0].one_time_discount.develop_science = 2;
        assert_eq!(tech_cost(&state, &state.players[0], card("Masonry")), Some(0));
    }

    /// A card with no develop cost is `None` before any discount is
    /// considered -- the discounts must never invent a develop cost for a
    /// starting technology.
    #[test]
    fn tech_cost_stays_none_for_an_undevelopable_card_even_with_discounts() {
        let mut state = state_with_science_pact();
        state.players[0].one_time_discount.develop_science = 1;
        assert_eq!(tech_cost(&state, &state.players[0], card("Agriculture")), None);
        assert_eq!(tech_cost(&state, &state.players[0], card("Despotism")), None);
    }

    #[test]
    fn tech_cost_bach_discounts_theaters_by_two() {
        let mut p = blank_player(0, card("Despotism"));
        p.leader = card("J. S. Bach");
        let state = one_player_state(p);
        assert_eq!(tech_cost(&state, &state.players[0], card("Drama")), Some(3 - 2));
    }

    #[test]
    fn tech_cost_shakespeare_cross_discounts_theater_and_library() {
        let mut p = blank_player(0, card("Despotism"));
        p.leader = card("William Shakespeare");
        p.techs.insert(card("Drama"), TechSlot { workers: 1, stored: 0 });
        let state = one_player_state(p);
        // Printing Press tech cost 3, minus 1 for Shakespeare + a theater in play.
        assert_eq!(tech_cost(&state, &state.players[0], card("Printing Press")), Some(2));
    }

    /// [`build_cost_for_shakespeare_ignores_a_library_that_is_developed_but_not_built`]'s
    /// twin for the develop-cost side of the same leader ability.
    #[test]
    fn tech_cost_shakespeare_ignores_a_theater_that_is_developed_but_not_built() {
        let mut p = blank_player(0, card("Despotism"));
        p.leader = card("William Shakespeare");
        p.techs.insert(card("Drama"), TechSlot { workers: 0, stored: 0 });
        let state = one_player_state(p);
        assert_eq!(tech_cost(&state, &state.players[0], card("Printing Press")), Some(3));
    }

    #[test]
    fn tech_cost_none_when_unbuildable() {
        let p = blank_player(0, card("Despotism"));
        let state = one_player_state(p);
        // Age-A starting techs print no techCost (already on the board).
        assert_eq!(tech_cost(&state, &state.players[0], card("Agriculture")), None);
    }

    // -------------------------------------------------------- upgrade_cost

    #[test]
    fn upgrade_cost_is_the_difference_floored_at_zero() {
        let p = blank_player(0, card("Despotism"));
        let state = one_player_state(p);
        // Agriculture's buildCost is 2, Irrigation's is 4: 4 - 2 = 2.
        assert_eq!(upgrade_cost(&state, &state.players[0], card("Agriculture"), card("Irrigation")), 2);
    }

    #[test]
    fn upgrade_cost_floors_at_zero_when_hi_is_cheaper_than_lo() {
        let p = blank_player(0, card("Despotism"));
        let state = one_player_state(p);
        assert_eq!(upgrade_cost(&state, &state.players[0], card("Irrigation"), card("Agriculture")), 0);
    }

    // ----------------------------------------------------- wonder_stage_cost

    #[test]
    fn wonder_stage_cost_sums_the_next_k_printed_stages() {
        // Pyramids: stages [3, 2, 1] (data/cards_wonders_leaders.json).
        let mut p = blank_player(0, card("Despotism"));
        p.wonder = card("Pyramids");
        let state = one_player_state(p);
        assert_eq!(wonder_stage_cost(&state, &state.players[0], 1), 3, "first stage only");
        assert_eq!(wonder_stage_cost(&state, &state.players[0], 2), 3 + 2, "first two stages");
        assert_eq!(wonder_stage_cost(&state, &state.players[0], 3), 3 + 2 + 1, "all three stages");
    }

    #[test]
    fn wonder_stage_cost_reads_from_the_current_progress() {
        let mut p = blank_player(0, card("Despotism"));
        p.wonder = card("Pyramids");
        p.wonder_steps = 1; // first stage (cost 3) already paid
        let state = one_player_state(p);
        assert_eq!(wonder_stage_cost(&state, &state.players[0], 1), 2, "second stage only");
    }

    #[test]
    fn wonder_stage_cost_clamps_k_past_the_last_stage() {
        // Mirrors Python's forgiving slice: asking for more stages than are
        // left sums only what is actually printed, rather than panicking.
        let mut p = blank_player(0, card("Despotism"));
        p.wonder = card("Pyramids");
        p.wonder_steps = 2; // only the last stage (cost 1) remains
        let state = one_player_state(p);
        assert_eq!(wonder_stage_cost(&state, &state.players[0], 5), 1);
    }

    // ------------------------------------------------------------- is_unit

    #[test]
    fn is_unit_true_for_units_false_for_everything_else() {
        assert!(is_unit(card("Swordsmen")));
        assert!(!is_unit(card("Bronze")));
        assert!(!is_unit(card("Despotism")));
    }

    // ------------------------------------------------------- *_net discount

    #[test]
    fn build_cost_net_spends_the_military_discount_pool() {
        let mut p = blank_player(0, card("Despotism"));
        p.mil_discount = 2;
        let state = one_player_state(p);
        assert_eq!(build_cost_net(&state, &state.players[0], card("Swordsmen")), Some(3 - 2));
    }

    #[test]
    fn build_cost_net_floors_at_zero_when_discount_exceeds_cost() {
        let mut p = blank_player(0, card("Despotism"));
        p.mil_discount = 100;
        let state = one_player_state(p);
        assert_eq!(build_cost_net(&state, &state.players[0], card("Swordsmen")), Some(0));
    }

    #[test]
    fn build_cost_net_ignores_the_pool_for_non_units() {
        let mut p = blank_player(0, card("Despotism"));
        p.mil_discount = 100;
        let state = one_player_state(p);
        assert_eq!(build_cost_net(&state, &state.players[0], card("Irrigation")), Some(4));
    }

    #[test]
    fn upgrade_cost_net_is_gated_on_the_lo_cards_type() {
        let mut p = blank_player(0, card("Despotism"));
        p.mil_discount = 1;
        let state = one_player_state(p);
        // Warriors -> Swordsmen is a unit upgrade; the discount applies.
        let base = upgrade_cost(&state, &state.players[0], card("Warriors"), card("Swordsmen"));
        assert_eq!(
            upgrade_cost_net(&state, &state.players[0], card("Warriors"), card("Swordsmen")),
            (base - 1).max(0)
        );
    }

    #[test]
    fn tech_cost_net_spends_the_military_science_pool_and_floors_at_zero() {
        let mut p = blank_player(0, card("Despotism"));
        p.mil_sci_discount = 100;
        let state = one_player_state(p);
        assert_eq!(tech_cost_net(&state, &state.players[0], card("Swordsmen")), Some(0));
    }

    // -------------------------------------------------------- spend_* pools

    #[test]
    fn spend_mil_discount_consumes_only_what_is_used() {
        let mut p = blank_player(0, card("Despotism"));
        p.mil_discount = 5;
        let owed = spend_mil_discount(&mut p, card("Swordsmen"), 3);
        assert_eq!(owed, 0, "pool covers the whole cost");
        assert_eq!(p.mil_discount, 2, "only 3 of the 5 spent");
    }

    #[test]
    fn spend_mil_discount_partial_when_pool_is_smaller_than_the_cost() {
        let mut p = blank_player(0, card("Despotism"));
        p.mil_discount = 2;
        let owed = spend_mil_discount(&mut p, card("Swordsmen"), 5);
        assert_eq!(owed, 3);
        assert_eq!(p.mil_discount, 0);
    }

    #[test]
    fn spend_mil_discount_is_a_noop_for_non_units_or_empty_pool() {
        let mut p = blank_player(0, card("Despotism"));
        p.mil_discount = 5;
        assert_eq!(spend_mil_discount(&mut p, card("Irrigation"), 4), 4, "not a unit");
        assert_eq!(p.mil_discount, 5, "pool untouched");

        let mut p2 = blank_player(0, card("Despotism"));
        p2.mil_discount = 0;
        assert_eq!(spend_mil_discount(&mut p2, card("Swordsmen"), 4), 4, "pool already empty");
    }

    #[test]
    fn spend_mil_sci_discount_mirrors_spend_mil_discount() {
        let mut p = blank_player(0, card("Despotism"));
        p.mil_sci_discount = 1;
        let owed = spend_mil_sci_discount(&mut p, card("Swordsmen"), 4);
        assert_eq!(owed, 3);
        assert_eq!(p.mil_sci_discount, 0);
    }
}
