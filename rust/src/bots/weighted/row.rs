//! `engine/bots/weighted.py` lines 3212-3556: what a rival will pay to take a
//! card off the row before my next turn, and what that costs the row's
//! owner.
//!
//! Ports `_rival_take_cost`, `rival_take_p`, `_rival_desire`, `row_pressure`,
//! `row_last_copy`, and the module constant `RIVAL_TAKE_SHARE`. See
//! [`row_pressure`]'s own doc comment for the fitted-prior story behind
//! `RIVAL_TAKE_SHARE`/[`WeightKey::RivalTakeShare`] -- restated here only
//! where the Rust shape earns its own note.
//!
//! ## `card_potential` is called directly, not injected
//!
//! An earlier revision of this file took `card_potential` (`weighted.py`
//! lines 1730-3211, owned by `cards.rs`) as an injected `&dyn Fn`, because
//! only its yield-plumbing layer had landed at the time -- DESIGN.md's "no DI
//! for its own sake" rule excuses a closure only for a dependency that is
//! genuinely not ported yet. `cards::card_potential` (the valuation layer)
//! exists now, so [`row_pressure`]/[`row_last_copy`]/[`rival_desire`] call it
//! directly like every other landed dependency in this file. Each owns ONE
//! `Vec<cards::CardYield>` scratch buffer for its own whole loop
//! (`cards::card_potential`'s own doc comment: it threads a caller-supplied
//! buffer through to `card_yields` rather than allocating per call, DESIGN.md
//! rule 4), reused -- not reallocated -- across every card it prices.
//!
//! ## Gating a `RivalView` needs its own legality check
//!
//! `costs::can_take_gated` takes a real `&PlayerState`, which a rival's own
//! slot of `state.players` IS -- but `rivals::RivalView` (now landed) is
//! deliberately NOT one: it is an owned snapshot of the four fields that
//! function reads (`building_wonder`, `hand_civil`, `techs`, `government`),
//! frozen at the root so a journalled search's mutate/undo of the CURRENT
//! player's own board can never alias into it (see `RivalView`'s own doc
//! comment). Python's `_can_take_gated` could run against either shape
//! because Python duck-types; Rust's cannot, so [`rival_view_can_take`]
//! below restates the identical rule against `RivalView`'s fields instead of
//! `PlayerState`'s. This is the one place this port's "a fact must not live
//! in two registries" principle bends: `costs::can_take_gated` is a shared,
//! actively-used file this port does not touch to make generic over both
//! shapes (a real fix, but a costs.rs change, not a row.rs one).
//! `tests::rival_view_gating_agrees_with_player_state_gating` below pins the
//! two never disagreeing.

use crate::bots::board_yields::Baseline;
use crate::bots::counting;
use crate::cards::{CardId, CardType};
use crate::costs::{self, TakeGate};
use crate::game;
use crate::state::{GameState, ROW_SIZE};

use super::cards::{self, CardYield};
use super::horizon;
use super::rivals::{RivalContext, RivalView};
use super::weights::{WeightKey, Weights};

/// P(a rival who CAN legally take a card I also want actually takes it)
/// before my next turn, USED to be one flat 0.25 for every rival, every card
/// and every board (see [`rival_take_p`]'s own doc comment for the case
/// against that and against replacing it with a fitted per-slot table
/// instead). `w.get(WeightKey::RivalTakeShare)` reads the live value --
/// [`WeightKey::RivalTakeShare`]'s own default (also 0.5) is this constant,
/// duplicated so `WeightKey::default_weight` (a `const fn`) does not need to
/// reach across module boundaries for one number; `weights.rs`'s own
/// differential test pins the two never drifting apart from Python's
/// `DEFAULT_WEIGHTS["rival_take_share"]`.
pub const RIVAL_TAKE_SHARE: f64 = 0.5;

// ------------------------------------------------------------- rival gating

/// [`costs::can_take_gated`], restated against a [`RivalView`] snapshot
/// instead of a live `&PlayerState` -- see this module's top doc comment for
/// why Rust needs a second copy of this rule where Python's duck typing
/// needed none. Kept byte-for-byte structurally identical to
/// `costs::can_take_gated` (same branch order, same clamps) precisely so a
/// future diff between the two is easy to read as a real drift, not noise.
fn rival_view_can_take(view: &RivalView, gate: &TakeGate, idx: usize, name: CardId) -> bool {
    let card = name.get();
    let mut cost = costs::row_cost(idx);
    if card.kind == CardType::Wonder {
        cost += gate.surcharge;
        if cost > gate.have {
            return false;
        }
        return !view.building_wonder;
    }
    if card.kind == CardType::Leader {
        cost -= gate.leader_discount;
    }
    if cost > gate.have {
        return false;
    }
    if gate.hand_full {
        return false;
    }
    if card.kind == CardType::Leader {
        return gate.taken_leader_ages & (1 << (card.age as u8)) == 0;
    }
    card.kind == CardType::Action
        || !(view.hand_civil.contains(name) || view.techs.has(name) || name == view.government)
}

// ------------------------------------------------------- take now vs slide

/// `_rival_take_cost`: what row slot `i` costs THAT rival, in their civil
/// actions -- `costs::row_cost` restated against a precomputed [`TakeGate`]
/// rather than against a live budget, because the surcharge and the discount
/// are already in the gate `rivals::rival_context` built (RULES_SPEC 2.3/
/// 2.5: +1 per completed or destroyed wonder, waived for Michelangelo; -1 on
/// a leader for Hammurabi; floored at 0).
fn rival_take_cost(name: CardId, i: usize, gate: &TakeGate) -> i32 {
    let mut cost = costs::row_cost(i);
    match name.kind() {
        CardType::Wonder => cost += gate.surcharge,
        CardType::Leader => cost -= gate.leader_discount,
        CardType::Farm | CardType::Mine | CardType::Lab | CardType::Temple | CardType::Library | CardType::Arena | CardType::Theater | CardType::Infantry | CardType::Cavalry | CardType::Artillery | CardType::Air | CardType::Government | CardType::SpecialTech | CardType::Action | CardType::Tactic | CardType::Aggression | CardType::War | CardType::Pact | CardType::Bonus | CardType::Territory | CardType::Event => {}
    }
    cost.max(0)
}

/// P(this rival takes THIS card before my next turn), from their board.
///
/// Every argument but `share` is exact public state:
///
/// * `cost` -- civil actions the card costs *that rival*, their surcharges
///   and discounts included;
/// * `budget` -- civil actions they will have next turn;
/// * `reach` -- how many row slots they could legally take at all (or, with
///   [`WeightKey::RivalDesire`] switched on, an effective competition figure
///   -- see [`rival_desire`]) -- i.e. how many ways that budget can be spent
///   on the row;
/// * `slack` -- how many more civil cards their hand can hold (0 when full,
///   in which case the answer is exactly 0 and no prior is consulted).
///
/// The model is a budget split: of `budget` civil actions, `share` go to the
/// row, buying `share*budget/cost` cards at this price, spread over the
/// `reach` slots competing for them, and capped by the hand. It is monotone
/// the way the board says it should be -- more of their actions or fewer
/// competing cards raises it, a dearer slot or a fuller hand lowers it -- and
/// it collapses to 0 exactly when the legality gate says they cannot take
/// the card at all.
pub fn rival_take_p(cost: i32, budget: i32, reach: f64, slack: i32, share: f64) -> f64 {
    if reach <= 0.0 || slack <= 0 {
        return 0.0;
    }
    let takes = if cost <= 0 {
        // free to them (Hammurabi on a 1-CA leader): only the hand limits it
        slack as f64
    } else {
        let t = share * budget as f64 / cost as f64;
        if t > slack as f64 {
            slack as f64
        } else {
            t
        }
    };
    let p = takes / reach;
    p.clamp(0.0, 1.0) // a clamped-negative `share` at the low end
}

/// Row slots visible AT THE ROOT of this search, gated by `root_row` -- the
/// forward-only cursor mask `row_pressure`'s own doc comment below explains
/// in full. `root_row: None` masks nothing (the degraded, leaky no-root
/// path Python's `ctx`-less call site takes); an empty slice masks
/// everything, because then every card present was dealt after the decision
/// began.
///
/// Shared between [`row_pressure`] and [`row_last_copy`], which Python
/// (deliberately, per its own comment on `row_last_copy`) does not share --
/// that comment is about NOT widening `row_pressure`'s 2-tuple return across
/// the ~40 call sites that depend on its shape, not about this masking loop.
/// Consolidating just the cursor arithmetic changes no number and removes a
/// second copy of it.
fn visible_row(row: &[CardId; ROW_SIZE], root_row: Option<&[CardId]>) -> Vec<(u8, CardId)> {
    let mut cursor = 0usize;
    let mut out = Vec::with_capacity(ROW_SIZE);
    for (i, &name) in row.iter().enumerate() {
        if name.is_none() {
            continue;
        }
        if let Some(root) = root_row {
            let mut k = cursor;
            while k < root.len() && root[k] != name {
                k += 1; // swept, or taken out from under the row
            }
            if k >= root.len() {
                // Dealt after the decision began, and -- because dealt cards
                // are always a suffix -- so is every slot to its right.
                break;
            }
            cursor = k + 1;
        }
        out.push((i as u8, name));
    }
    out
}

/// One rival's reach into the visible row, and (only when
/// [`WeightKey::RivalDesire`] is switched on) what they would actually want
/// from it. Combines Python's separate `reach` list comprehension and
/// `_rival_desire` loop into one pass per rival over `visible` -- both read
/// the exact same take-gate, so there is nothing to gain from walking
/// `visible` twice.
struct RivalDemand {
    /// Row slots this rival could legally take at all -- Python's `reach[k]`.
    reach: f64,
    /// `(slot, this rival's value for that slot, this rival's grand total
    /// positive demand across every slot they want)` -- Python's `want[k]`,
    /// a `{slot: (value, total)}` dict flattened to a `Vec` of triples: at
    /// most 13 entries, looked up by a linear scan (`desire_lookup`) exactly
    /// the way `bots::counting::lookup` treats its own short pair lists,
    /// rather than paying a `HashMap` for a handful of entries this never
    /// grows past.
    wants: Vec<(u8, f64, f64)>,
}

fn desire_lookup(wants: &[(u8, f64, f64)], slot: u8) -> (f64, f64) {
    wants.iter().find(|&&(j, _, _)| j == slot).map(|&(_, v, t)| (v, t)).unwrap_or((0.0, 0.0))
}

/// `_rival_desire`, one rival at a time -- see [`RivalDemand`] for why this
/// is not a straight port of the Python `list[dict]` shape.
///
/// WHAT THIS CLOSES. [`rival_take_p`] models a rival's CAPACITY exactly --
/// what the row costs them, what their civil actions buy, how full their
/// hand is -- and spreads that capacity UNIFORMLY over every slot they could
/// legally take. Uniform is wrong in the obvious direction: a rival with two
/// civil actions and a Philosophy in reach is not equally likely to spend
/// them on the Bronze next to it.
///
/// Their board is public, so it can be priced. This runs the SAME
/// `card_potential` the bot prices its own row with, from the rival's seat
/// -- which is the honest thing to run: it is a model of them built out of
/// the evaluator's own judgement applied to their visible position, not a
/// peek at their plan. Intentions stay hidden; only the board is read.
///
/// Only slots that rival can legally take are counted, and only positive
/// values, so `wants` is a genuine distribution over the cards they would
/// actually want. `want_values` gates the pricing pass off entirely when
/// [`WeightKey::RivalDesire`] is 0.0 -- `reach` alone is still computed,
/// because [`row_pressure`]'s uniform fallback needs it regardless.
#[allow(clippy::too_many_arguments)]
fn rival_desire(
    state: &GameState,
    w: &Weights,
    visible: &[(u8, CardId)],
    view: &RivalView,
    gate: &TakeGate,
    late: f64,
    want_values: bool,
    scratch: &mut Vec<CardYield>,
) -> RivalDemand {
    let mut reach = 0.0;
    let mut vals: Vec<(u8, f64)> = Vec::new();
    let mut total = 0.0;
    // ONE baseline for the whole row: `effects::compute` for this player does
    // not depend on which card is being priced, and it is the most expensive
    // function in the engine.
    let base = Baseline::at(state, view.idx);
    for &(j, nm) in visible {
        if !rival_view_can_take(view, gate, j as usize, nm) {
            continue;
        }
        reach += 1.0;
        if want_values {
            let v = cards::card_potential(nm, w, Some(&base), Some(late), scratch);
            if v > 0.0 {
                vals.push((j, v));
                total += v;
            }
        }
    }
    let wants = vals.into_iter().map(|(j, v)| (j, v, total)).collect();
    RivalDemand { reach, wants }
}

/// (row_urgency, row_bargain_forgone) for the civil row.
///
/// The slide is arithmetic, not a guess. `_replenish` runs at the START of
/// every player's turn, so between one of my turns and the next there are
/// exactly `live` replenishes of `sweep_count(live)` cards -- 6 slots at 2p,
/// 6 at 3p, 4 at 4p -- plus one more for every card to the left of it that
/// somebody takes. Ignoring those takes makes `next_slot` an UPPER bound on
/// what the card will cost me next turn, i.e. the bargain below is
/// understated, never overstated.
///
/// * **row_urgency** -- summed `card_potential` of the cards I could legally
///   take that the sweep destroys before I act again. Take it now or never.
/// * **row_bargain_forgone** -- summed civil actions I overpay by taking a
///   card now instead of next turn, discounted by the chance a rival takes
///   it first.
///
/// Both are evaluated on the POST-move state like every other feature, so
/// taking a doomed card lowers `row_urgency` and taking a card that was
/// about to get cheaper leaves `row_bargain_forgone` behind for the
/// candidate that did not. Cards whose `card_potential` is <= 0 are skipped
/// in both sums: the sweep destroying a card I do not want is not a loss,
/// and waiting for one is not a bargain.
///
/// Reads `state.card_row`, my own board, and `ctx.rival_views` -- the
/// public rival boards/civil hands (`rivals::rival_context`). The row is
/// public AT THE ROOT only: a trial that crossed a turn boundary has had the
/// real next cards dealt into it, so every slot is matched against
/// `ctx.root_row` -- the root row's names in ROW ORDER -- with a
/// forward-only cursor ([`visible_row`]), and cards that were not visible
/// when the decision started are SKIPPED. Without that check this function
/// is the mechanism of the `end_turn` information leak
/// (docs/ANALYSIS_HISTORY.md, INFORMATION_AUDIT.md verdict).
///
/// `ctx: None` (a caller that has not built one, or the degraded no-context
/// path) masks nothing and considers no rivals, as leaky as Python's
/// `ctx=None` call site; every real caller goes through `rivals::
/// rival_context`.
pub fn row_pressure(state: &GameState, idx: u8, w: &Weights, ctx: Option<&RivalContext>) -> (f64, f64) {
    let root_row = ctx.map(|c| c.root_row.as_slice());
    let visible = visible_row(&state.card_row, root_row);
    if visible.is_empty() {
        return (0.0, 0.0);
    }
    let p = &state.players[idx as usize];
    let n = horizon::live_count(state);
    let slide = (n * game::sweep_count(n)) as i32;
    let mine = costs::take_gate(state, p, Some(costs::ca_total(state, p)));
    let share = w.get(WeightKey::RivalTakeShare);
    let desire = w.get(WeightKey::RivalDesire).clamp(0.0, 1.0);
    let late = horizon::lateness(state);
    let rival_views: &[(RivalView, TakeGate)] = ctx.map(|c| c.rival_views.as_slice()).unwrap_or(&[]);

    // One scratch buffer for every `cards::card_potential` call this whole
    // decision makes -- see this module's top doc comment.
    let mut scratch: Vec<CardYield> = Vec::new();
    // Per-rival slot counts, built at most once -- see `rival_desire`'s own
    // doc comment for why this is lazy: a row with no card ever passing the
    // `saving > 0` gate below never needs it at all.
    let mut demand: Option<Vec<RivalDemand>> = None;
    let mut urgency = 0.0;
    let mut bargain = 0.0;
    // Doomed single-slot cards (leader, government) collapse into `urgency`
    // the same way `cards::hand_total` collapses the hand -- see that
    // function's doc comment. `card_potential` prices a leader/government as
    // a diff against the CURRENT slot when board-aware pricing is on, so
    // summing that over two leaders about to be swept asserts you get both
    // replacements, when only the better one ever will be (the rest is worth
    // `hand_swap_extra`, reused here rather than duplicated: it is the same
    // free parameter either way -- the incremental value of a spare
    // single-slot card beyond the one you would actually play, whether the
    // spare arrives via the row or the hand). index 0 = Leader, index 1 =
    // Government, matching `hand_total`'s fixed-size accumulator and its own
    // reasoning for why that beats a keyed collection here.
    let mut doomed_slots: [Option<(f64, f64)>; 2] = [None, None];

    // ONE baseline for the whole row -- see `rival_demand`.
    let base = Baseline::at(state, idx);
    for &(i, name) in &visible {
        if !costs::can_take_gated(state, p, i as usize, &mine, Some(name)) {
            continue;
        }
        let val = cards::card_potential(name, w, Some(&base), Some(late), &mut scratch);
        if val <= 0.0 {
            continue;
        }
        let nxt = i as i32 - slide;
        if nxt < 0 {
            match cards::swap_slot(name, w) {
                None => urgency += val,
                Some(typ) => {
                    let slot = match typ {
                        CardType::Leader => 0,
                        CardType::Government => 1,
                        CardType::Farm | CardType::Mine | CardType::Lab | CardType::Temple | CardType::Library | CardType::Arena | CardType::Theater | CardType::Infantry | CardType::Cavalry | CardType::Artillery | CardType::Air | CardType::SpecialTech | CardType::Wonder | CardType::Action | CardType::Tactic | CardType::Aggression | CardType::War | CardType::Pact | CardType::Bonus | CardType::Territory | CardType::Event => unreachable!("swap_slot only ever returns a single-slot CardType"),
                    };
                    match &mut doomed_slots[slot] {
                        None => doomed_slots[slot] = Some((val, val)),
                        Some((best, tot)) => {
                            if val > *best {
                                *best = val;
                            }
                            *tot += val;
                        }
                    }
                }
            }
            continue;
        }
        let saving = costs::row_cost(i as usize) - costs::row_cost(nxt as usize);
        if saving <= 0 {
            continue;
        }
        if demand.is_none() {
            let mut d = Vec::with_capacity(rival_views.len());
            for (v, g) in rival_views {
                d.push(rival_desire(state, w, &visible, v, g, late, desire > 0.0, &mut scratch));
            }
            demand = Some(d);
        }
        let mut survive = 1.0;
        for ((view, gate), dem) in rival_views.iter().zip(demand.as_ref().unwrap().iter()) {
            if !rival_view_can_take(view, gate, i as usize, name) {
                continue;
            }
            let mut compete = dem.reach;
            if desire > 0.0 {
                let (mine_i, total_i) = desire_lookup(&dem.wants, i);
                if mine_i > 0.0 && total_i > 0.0 {
                    // Effective competition: how many slots' worth of that
                    // rival's appetite this one slot is up against. Equal to
                    // `reach` exactly when they want every reachable slot
                    // the same amount, which is what the uniform model
                    // assumed; larger when this card is one they do not
                    // want, and as small as 1.0 when it is the only card on
                    // the row that interests them -- `total_i >= mine_i`
                    // always (see this module's top doc comment), so no
                    // floor is needed here the way Python's original did.
                    let eff = total_i / mine_i;
                    debug_assert!(eff >= 1.0 - 1e-9, "eff {eff} below 1.0: mine={mine_i} total={total_i}");
                    compete = (1.0 - desire) * dem.reach + desire * eff;
                }
            }
            let cost = rival_take_cost(name, i as usize, gate);
            survive *= 1.0 - rival_take_p(cost, gate.have, compete, view.hand_slack, share);
        }
        bargain += f64::from(saving) * survive;
    }
    let extra = w.get(WeightKey::HandSwapExtra);
    for (best, tot) in doomed_slots.into_iter().flatten() {
        urgency += best + extra * (tot - best);
    }
    (urgency, bargain)
}

/// Value sitting in the row that, if I pass it up, I will never see again.
///
/// THE FEATURE THE AUDIT WAS ACTUALLY ASKED FOR, in the project owner's own
/// words: *"know that a second selective breeding is near the end or not
/// and it has to build another farm since it won't see it"*. [`row_pressure`]
/// already knows a card is about to slide off the LEFT; it has never known
/// whether another copy is coming off the deck, so a last copy and a card
/// with three more behind it price identically.
///
/// Each takeable slot contributes `P(no further copy)`, and that probability
/// is `max(0, 1 - outlook)` where `outlook` is `counting::civil_outlook`'s
/// expected number of further copies. The clip is the whole model and it has
/// no fitted constant in it: at an outlook of 0 the deck provably holds no
/// more (an age's deck is always dealt out in full), so the probability is
/// exactly 1; at an outlook of 1 or more another copy is expected, so it is
/// 0; in between it interpolates.
///
/// UNITS FIX (was `card_potential * P(no further copy)`, summed, then that
/// sum multiplied AGAIN by `WeightKey::RowLastCopy`'s own coefficient in
/// `eval::evaluate`): `card_potential(..., w, ...)` is itself already a
/// composite the LIVE weight vector `w` prices (`*BoardCredit`/
/// `card_rate_credit`/every printed-yield key `sum_yields` dots against) --
/// feeding that composite into a second, outer coefficient made the outer
/// coefficient's fitted number mean something different every time ANY
/// OTHER weight in `w` moved, even though `row_last_copy`'s own coefficient
/// never did (`weights.rs`'s own `sign_intent` comment on this key: "its
/// defect is units, not sign" -- widening the clamp or flipping the sign
/// cannot fix a term that is not measuring a fixed quantity to begin with).
/// `card_potential` is still called -- it is still the right question ("do I
/// want this card at all?") -- but only as a `> 0.0` GATE, never as a
/// magnitude: what is actually summed is `gone` itself, a deterministic,
/// `w`-independent count of wanted slots about to vanish for good. That
/// makes `eval::evaluate`'s own `rlc * row_last_copy(...)` the ONLY weight
/// this reading depends on, so the same `rlc` means the same thing (price
/// per about-to-vanish wanted card) at every position, not a price that
/// rides whatever the board-credit weights happen to be this generation.
/// Pinned by `row_last_copy_depends_on_no_weight_but_its_own` below.
///
/// Deliberately NOT folded into `row_pressure`'s return -- see that
/// function's own doc comment on why -- so the masking loop is duplicated
/// via the shared [`visible_row`] rather than the return shape being
/// widened.
///
/// `ctx: None` recomputes the outlook from `state` via
/// `counting::civil_outlook`, correct at the search root and the same
/// degraded path `row_pressure` documents.
pub fn row_last_copy(state: &GameState, idx: u8, w: &Weights, ctx: Option<&RivalContext>) -> f64 {
    let root_row = ctx.map(|c| c.root_row.as_slice());
    let visible = visible_row(&state.card_row, root_row);
    if visible.is_empty() {
        return 0.0;
    }
    let owned;
    let outlook: &[(CardId, f64)] = match ctx {
        Some(c) => &c.civil_outlook,
        None => {
            owned = counting::civil_outlook(state, idx);
            &owned
        }
    };
    let p = &state.players[idx as usize];
    let mine = costs::take_gate(state, p, Some(costs::ca_total(state, p)));
    let late = horizon::lateness(state);
    let mut scratch: Vec<CardYield> = Vec::new();
    let mut total = 0.0;
    // ONE baseline for the whole row -- see `rival_demand`.
    let base = Baseline::at(state, idx);
    for &(i, name) in &visible {
        if !costs::can_take_gated(state, p, i as usize, &mine, Some(name)) {
            continue;
        }
        // `card_potential` is a GATE here, not a magnitude -- see this
        // function's own doc comment (UNITS FIX): summing its dollar value
        // would feed a composite already priced by `w` into `rlc`'s own
        // multiply below, a second weight on top of however many `w` already
        // spent inside `card_potential`.
        let val = cards::card_potential(name, w, Some(&base), Some(late), &mut scratch);
        if val <= 0.0 {
            continue;
        }
        let gone = 1.0 - counting::lookup(outlook, name, 0.0);
        if gone > 0.0 {
            total += gone;
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::rivals;
    use crate::game as G;
    use crate::state::PlayerState;

    fn card(name: &str) -> CardId {
        CardId::by_name(name).unwrap_or_else(|| panic!("no such card: {name}"))
    }

    // ------------------------------------------------------- rival_take_p

    #[test]
    fn a_full_hand_takes_nothing() {
        assert_eq!(rival_take_p(1, 4, 5.0, 0, 0.5), 0.0);
    }

    #[test]
    fn zero_reach_takes_nothing() {
        assert_eq!(rival_take_p(1, 4, 0.0, 3, 0.5), 0.0);
    }

    #[test]
    fn free_to_them_is_limited_only_by_the_hand() {
        // Hammurabi on a 1 CA leader: cost clamps to 0 before this is even
        // called, but the function itself must still behave for cost <= 0.
        // reach 2, slack 3: takes = 3 (capped by hand), p = 3/2 clamped to 1.0
        assert_eq!(rival_take_p(0, 4, 2.0, 3, 0.5), 1.0);
        // reach 4, slack 3: takes = 3 (capped by hand), p = 3/4
        assert!((rival_take_p(0, 4, 4.0, 3, 0.5) - 0.75).abs() < 1e-12);
    }

    #[test]
    fn it_is_always_a_probability() {
        for cost in [0, 1, 3, 9] {
            for budget in [0, 1, 20] {
                for reach in [0.0, 1.0, 13.0] {
                    for slack in [0, 1, 9] {
                        for share in [-1.0, 0.0, 0.5, 5.0] {
                            let v = rival_take_p(cost, budget, reach, slack, share);
                            assert!((0.0..=1.0).contains(&v), "{cost} {budget} {reach} {slack} {share} -> {v}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn the_estimate_moves_the_way_the_board_says() {
        let base = rival_take_p(2, 4, 6.0, 3, 0.5);
        assert!(rival_take_p(2, 8, 6.0, 3, 0.5) > base, "more CA");
        assert!(rival_take_p(3, 4, 6.0, 3, 0.5) < base, "dearer");
        assert!(rival_take_p(2, 4, 12.0, 3, 0.5) < base, "more competition");
        assert!(rival_take_p(2, 4, 6.0, 1, 0.5) <= base, "tighter hand");
    }

    // ---------------------------------------------------- rival_take_cost

    #[test]
    fn ordinary_cards_are_bare_row_cost() {
        let gate = TakeGate { have: 4, hand_full: false, surcharge: 2, leader_discount: 1, taken_leader_ages: 0 };
        assert_eq!(rival_take_cost(card("Bronze"), 6, &gate), costs::row_cost(6));
    }

    #[test]
    fn wonders_add_the_surcharge() {
        let gate = TakeGate { have: 4, hand_full: false, surcharge: 2, leader_discount: 0, taken_leader_ages: 0 };
        assert_eq!(rival_take_cost(card("Colossus"), 0, &gate), costs::row_cost(0) + 2);
    }

    #[test]
    fn leaders_subtract_the_discount_and_floor_at_zero() {
        let gate = TakeGate { have: 4, hand_full: false, surcharge: 0, leader_discount: 1, taken_leader_ages: 0 };
        assert_eq!(rival_take_cost(card("Napoleon Bonaparte"), 0, &gate), 0, "row_cost(0)=1, 1-1=0");
    }

    // ------------------------------------------------------- visible_row

    #[test]
    fn no_root_row_masks_nothing() {
        let mut row = [CardId::NONE; ROW_SIZE];
        row[0] = card("Bronze");
        row[5] = card("Alchemy");
        let visible = visible_row(&row, None);
        assert_eq!(visible, vec![(0, card("Bronze")), (5, card("Alchemy"))]);
    }

    #[test]
    fn a_card_dealt_after_the_decision_began_is_masked_out() {
        let mut row = [CardId::NONE; ROW_SIZE];
        row[0] = card("Bronze");
        row[1] = card("Theology"); // not in root_row: freshly dealt
        let root = [card("Bronze")];
        let visible = visible_row(&row, Some(&root));
        assert_eq!(visible, vec![(0, card("Bronze"))]);
    }

    #[test]
    fn a_root_entry_is_consumed_at_most_once() {
        // root_row held exactly one "Bronze". The live row now has TWO --
        // the original survivor plus a freshly dealt duplicate further
        // right. The forward-only cursor must match the root's one entry
        // against the first occurrence and mask the second as dealt-after-
        // the-decision-began, not double-count the root's single copy.
        let mut row = [CardId::NONE; ROW_SIZE];
        row[0] = card("Bronze"); // the survivor -- matches the root's one entry
        row[3] = card("Bronze"); // a freshly dealt duplicate -- must be masked
        let root = [card("Bronze")];
        let visible = visible_row(&row, Some(&root));
        assert_eq!(visible, vec![(0, card("Bronze"))], "only the first copy consumes the root's one entry");
    }

    #[test]
    fn empty_root_row_masks_everything() {
        let mut row = [CardId::NONE; ROW_SIZE];
        row[0] = card("Bronze");
        let visible = visible_row(&row, Some(&[]));
        assert!(visible.is_empty());
    }

    // -------------------------------------------------- eff floor removal

    /// See this module's top doc comment: Python's `_rival_desire` used to
    /// floor `eff = total_i / mine_i` at 1.0. This proves the floor never
    /// had anything to guard -- `total_i` is a sum of the SAME positive
    /// terms `mine_i` is drawn from, so it can never be smaller, in exact
    /// arithmetic or in `f64`.
    #[test]
    fn eff_is_never_below_one_for_any_positive_demand_set() {
        let cases: [&[f64]; 5] = [
            &[1e-300, 1e300],
            &[0.1, 0.2, 0.3],
            &[1.0],
            &[5.5, 5.5, 5.5, 5.5],
            &[1e-10, 1e10, 3.0],
        ];
        for vals in cases {
            let total: f64 = vals.iter().sum();
            for &mine in vals {
                assert!(total / mine >= 1.0, "vals={vals:?} mine={mine} total={total}");
            }
        }
    }

    // --------------------------------------------------- rival_view gating

    /// `rival_view_can_take` must agree with `costs::can_take_gated` for
    /// every slot on a real board -- the two are meant to be the same rule
    /// read off two different shapes (see this module's top doc comment).
    /// Built off the real `rivals::rival_context` (not a hand-rolled
    /// `RivalView`, which has no public constructor by design -- see that
    /// struct's own doc comment) so this doubles as an integration check.
    #[test]
    fn rival_view_gating_agrees_with_player_state_gating() {
        let state = G::new_game(3, 44);
        let ctx = rivals::rival_context(&state, 0, None, None);
        for (view, gate) in &ctx.rival_views {
            let p: &PlayerState = &state.players[view.idx as usize];
            for (i, &name) in state.card_row.iter().enumerate() {
                if name.is_none() {
                    continue;
                }
                let direct = costs::can_take_gated(&state, p, i, gate, Some(name));
                let via_view = rival_view_can_take(view, gate, i, name);
                assert_eq!(direct, via_view, "idx {} slot {i} {}", view.idx, name.name());
            }
        }
    }

    // ------------------------------------------------- row_pressure/last_copy

    /// A row with nothing takeable prices at exactly zero -- the early-return
    /// path, before `cards::card_potential` is ever reached.
    #[test]
    fn empty_row_is_zero_zero() {
        let mut state = G::new_game(2, 5);
        state.card_row = [CardId::NONE; ROW_SIZE];
        let w = Weights::default();
        assert_eq!(row_pressure(&state, 0, &w, None), (0.0, 0.0));
        assert_eq!(row_last_copy(&state, 0, &w, None), 0.0);
    }

    /// A card the sweep destroys before my next turn contributes to
    /// `row_urgency`; one that survives contributes zero. 2p: slide = 2 *
    /// sweep_count(2), so a card sitting inside the first `slide` slots is
    /// doomed and one past it survives.
    ///
    /// Uses a Wonder ("Colossus") under `Weights::default()`: `card_board_credit`
    /// and `wonder_board_credit` both default to 0.0, so `cards::card_potential`
    /// takes the static-table early return for it regardless of board --
    /// deterministic and known-positive (pinned by `cards::tests::
    /// wonders_pay_a_stage_cost_and_grant_a_wonders_point`'s sibling
    /// coverage) without needing a hand-zeroed weight vector, which would
    /// also zero `rival_take_share`/`rival_desire` and break the very
    /// rival-gating arithmetic these row.rs tests exist to check.
    #[test]
    fn urgency_counts_only_cards_the_sweep_destroys() {
        let mut state = G::new_game(2, 23);
        state.players[0].civil_actions = 6;
        let slide = 2 * game::sweep_count(2);
        state.card_row = [CardId::NONE; ROW_SIZE];
        state.card_row[0] = card("Colossus");
        let w = Weights::default();
        let (doomed, _) = row_pressure(&state, 0, &w, None);
        assert!(doomed > 0.0, "slot 0 is inside the slide");

        state.card_row = [CardId::NONE; ROW_SIZE];
        let safe_slot = slide.min(ROW_SIZE - 1);
        state.card_row[safe_slot] = card("Colossus");
        let (safe, _) = row_pressure(&state, 0, &w, None);
        assert_eq!(safe, 0.0, "slot {safe_slot} is not inside the slide, so it is priced as a bargain, not an urgency");
    }

    /// A rival with a full civil hand contributes `rival_take_p == 0.0`, so
    /// `survive` for that rival is exactly 1.0 -- letting a card wait costs
    /// nothing extra when no rival could actually take it.
    #[test]
    fn a_full_handed_rival_leaves_the_bargain_undiscounted() {
        let mut state = G::new_game(2, 29);
        state.players[0].civil_actions = 6;
        state.card_row = [CardId::NONE; ROW_SIZE];
        state.card_row[9] = card("Colossus"); // 3 CA now, slot 3 (1 CA) next turn
        let w = Weights::default();

        let ctx = rivals::rival_context(&state, 0, None, None);
        let (_, contested) = row_pressure(&state, 0, &w, Some(&ctx));

        // Fill the rival's civil hand past the limit so `hand_slack` is 0.
        for _ in 0..20 {
            state.players[1].hand_civil.push(card("Theology"));
        }
        let ctx = rivals::rival_context(&state, 0, None, None);
        let (_, safe) = row_pressure(&state, 0, &w, Some(&ctx));
        assert!(safe > contested, "a full-handed rival must not discount the bargain");
    }

    /// `row_urgency` collapses two leaders about to be swept off the row to
    /// "the better one, plus `hand_swap_extra` times the rest" -- the same
    /// bug `hand_total`'s doc comment describes (cards.rs), just for the row
    /// instead of the hand (docs/ANALYSIS_HISTORY.md, CARD_BLINDNESS.md verdict:
    /// "row_pressure's row_urgency -- yes, and NOT fixed here"). Reuses `hand_swap_extra`:
    /// the free parameter is the same question either way -- the incremental
    /// worth of a spare single-slot card beyond the one you would actually
    /// play, whether the spare arrives via the row or the hand. Built with
    /// the leader's ABSOLUTE board-credit key (`card_board_leader`) on --
    /// `card_board_credit` no longer offsets it after the 2026-08-28 decouple
    /// -- so `swap_slot` actually collapses, with two DIFFERENT leaders so
    /// their `card_potential` values are not forced equal by symmetry.
    #[test]
    fn urgency_collapses_two_doomed_leaders_to_the_better_one_plus_extra_times_the_rest() {
        let mut state = G::new_game(2, 46);
        state.players[0].civil_actions = 6;
        state.card_row = [CardId::NONE; ROW_SIZE];
        let a = card("Julius Caesar");
        let b = card("Napoleon Bonaparte");
        state.card_row[0] = a;
        state.card_row[1] = b;
        let mut w = Weights::default();
        w.set(WeightKey::CardBoardLeader, 1.0);
        w.set(WeightKey::HandSwapExtra, 0.0);
        let late = horizon::lateness(&state);
        let mut scratch = Vec::new();
        let va = cards::card_potential(a, &w, Some(&Baseline::at(&state, 0)), Some(late), &mut scratch);
        let vb = cards::card_potential(b, &w, Some(&Baseline::at(&state, 0)), Some(late), &mut scratch);
        assert!(va > 0.0 && vb > 0.0, "va={va} vb={vb}");
        let (urgency, _) = row_pressure(&state, 0, &w, None);
        assert_eq!(urgency, va.max(vb), "hand_swap_extra=0.0 must keep only the better doomed leader's value, not the sum of both");
    }

    /// `row_last_copy` weights each takeable card by `max(0, 1 - outlook)`
    /// -- UNITS FIX: `card_potential` gates which cards count (must be
    /// wanted, `> 0.0`) but no longer scales the sum, so the total is
    /// exactly `gone` summed over wanted cards, not `val * gone`. Rather
    /// than assume a fresh deal's real `civil_outlook` for "Colossus" (a
    /// fitted/measured number, not a constant this test should hardcode),
    /// this computes that same outlook independently via
    /// `counting::civil_outlook` and checks `row_last_copy` against the
    /// formula directly -- correct whichever side of 1.0 the real outlook
    /// happens to land on.
    #[test]
    fn row_last_copy_matches_the_gone_only_formula() {
        let mut state = G::new_game(2, 31);
        state.players[0].civil_actions = 6;
        state.card_row = [CardId::NONE; ROW_SIZE];
        let name = card("Colossus");
        state.card_row[0] = name;
        let w = Weights::default();
        let late = horizon::lateness(&state);
        let mut scratch = Vec::new();
        let val = cards::card_potential(name, &w, Some(&Baseline::at(&state, 0)), Some(late), &mut scratch);
        assert!(val > 0.0, "val={val}, must be wanted for this test to exercise the gate");
        let outlook = counting::lookup(&counting::civil_outlook(&state, 0), name, 0.0);
        let gone = (1.0 - outlook).max(0.0);
        let total = row_last_copy(&state, 0, &w, None);
        assert!((total - gone).abs() < 1e-9, "total={total} expected={gone}");
    }

    /// THE UNITS BUG ITSELF, pinned directly: before this fix, `row_last_copy`
    /// summed `card_potential(..., w, ...) * gone`, a composite the LIVE
    /// weight vector already prices -- so moving ANY other weight (say
    /// `card_rate_credit`, which scales every `Rate`-kind printed card
    /// yield) moved this reading too, even though `row_last_copy`'s own
    /// coefficient never did. `eval::evaluate` then multiplies this reading
    /// by `RowLastCopy`'s own coefficient AGAIN, so that outer coefficient's
    /// fitted number meant a different thing every time some unrelated
    /// weight drifted -- "the coefficient has no consistent meaning" is
    /// exactly this: not expressible as `price * f(state)` for any fixed
    /// price. Confirmed RED by reverting the fix (`total += val * gone`
    /// instead of `total += gone`): this loop found at least one weight
    /// whose change moved `row_last_copy`'s reading (see this test's own
    /// failure message for which one and by how much).
    #[test]
    fn row_last_copy_depends_on_no_weight_but_its_own() {
        let mut state = G::new_game(2, 31);
        state.players[0].civil_actions = 6;
        state.card_row = [CardId::NONE; ROW_SIZE];
        let name = card("Colossus");
        state.card_row[0] = name;
        let baseline = Weights::default();
        let base_total = row_last_copy(&state, 0, &baseline, None);
        assert!(base_total > 0.0, "must be pricing a wanted last copy to exercise anything, got {base_total}");

        let late = horizon::lateness(&state);
        let board = Baseline::at(&state, 0);
        let mut scratch = Vec::new();
        let mut checked = 0;
        for &k in WeightKey::ALL {
            if k == WeightKey::RowLastCopy {
                continue; // its own coefficient is applied OUTSIDE this function, by `eval::evaluate`
            }
            let mut w = baseline;
            w.set(k, w.get(k) + 7.0);
            // Flipping whether the card is wanted at all is `card_potential`'s
            // own legitimate `> 0.0` gate (every caller in this file relies on
            // it) -- not the bug under test. The property this test pins is
            // that, AS LONG AS the card stays wanted, the READING does not
            // scale with `k`'s magnitude the way the old `val * gone` formula
            // did; a gate flip is skipped, not asserted on.
            if cards::card_potential(name, &w, Some(&board), Some(late), &mut scratch) <= 0.0 {
                continue;
            }
            checked += 1;
            let total = row_last_copy(&state, 0, &w, None);
            assert_eq!(
                total,
                base_total,
                "row_last_copy moved from {base_total} to {total} when only {} changed (card still \
                 wanted under the new weight) -- it must depend on no weight but its own",
                k.name()
            );
        }
        assert!(checked > 0, "the sweep never left the card wanted after a perturbation, so it proved nothing");
    }
}
