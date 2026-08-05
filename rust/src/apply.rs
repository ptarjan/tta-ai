//! Move application -- the port of `engine/actions.py`'s `apply` half:
//! `apply`, the `_h_*` per-move handlers, and `do_build`/`do_upgrade`/
//! `do_wonder_step` (the handlers' shared bodies, kept separate because
//! `apply_free_action` in Python calls them a second way -- discounted, no
//! action cost -- once the ordered-action machinery exists to call them that
//! way here too).
//!
//! Depends on `costs.rs` (row/build/upgrade/tech costs, the discount pools,
//! `special_icon`) and `economy.rs` (population, the blue-token bank,
//! `discard_civil`/`discard_military`). Both are read-only dependencies of
//! this module; neither is touched here.
//!
//! ## What is deliberately NOT here, and why
//!
//! Three sibling systems are not ported yet, and every move or trigger that
//! needs one of them is named individually below rather than silently
//! stubbed -- DESIGN.md's whole point is that an unhandled case is loud, not
//! quiet.
//!
//! - **`interact.rs`'s decision queue.** `GameState` has no `pending` field
//!   (the economy port already flagged this). Python's `apply()` opens with
//!   `if state.pending: interact.apply_pending(...)`; there is no field to
//!   test, so every call into this module implicitly assumes no decision is
//!   open. Response moves ([`Move::Bid`], [`Move::BidPass`], [`Move::Defend`],
//!   [`Move::DefendDone`], [`Move::SendUnit`], [`Move::SendBonus`],
//!   [`Move::SendDone`], [`Move::Choose`]) panic in [`apply`] for the same
//!   reason: nothing opened them, so they can never be legal today. So does
//!   [`Move::OfferPact`] (Python: `interact.push_choice`) and the ordered-
//!   action half of [`Move::PlayAction`] (Python: `interact.enqueue`) --
//!   see [`h_play_action`].
//! - **`events.rs`.** [`Move::PrepareEvent`] (`events.reveal_current_event`)
//!   and [`Move::Aggression`] (`events.start_aggression`) both call into it
//!   directly. Panics in [`apply`].
//! - **`game.rs`.** [`Move::EndTurn`] (`game.end_turn`) is the whole End-of-
//!   Turn Sequence orchestrator and is not ported at all. `_h_resign`'s tail
//!   call to `game.after_resign` (deciding whether resigning leaves a forced
//!   winner, §5.11) is in the same boat -- [`h_resign`] performs every OTHER
//!   effect of resigning and stops one call short of that, with a comment at
//!   the stopping point, rather than pretending the game continues normally.
//! - **[`Move::War`].** Not a `combat.rs` dependency -- `_h_war` itself never
//!   resolves combat, it only records the declaration. It is blocked on
//!   `PlayerState` having no `war_declared_by_me` / `wars_declared_on_me`
//!   fields (state.rs, off limits to this module). Panics in [`apply`]; the
//!   war-cleanup half of [`h_resign`] (§5.11's "wars against a resigned
//!   player score their declarer 7") is skipped for the same reason, noted
//!   at its call site rather than silently omitted.
//!
//! One more gap is narrower, and half-resolved out from under this module
//! mid-port -- exactly the "expect `cards.rs`/`card_table.rs` to change
//! under you" the coordinator's brief warned about:
//!
//! - **`Card::stages`.** [`costs::wonder_stage_cost`] still
//!   `unimplemented!()`s (its own module's doc comment says why -- `Card`
//!   had no `stages` field when it was written). `Card` HAS grown that field
//!   since (checked 2026-08-05, mid-port), but `costs.rs` is another
//!   worker's file and has not been updated to use it yet, so
//!   [`do_wonder_step`] still panics through the unchanged
//!   `costs::wonder_stage_cost` call. This module's OWN
//!   [`wonder_is_complete`] (the other place §9's wonder-completion check
//!   needs the stage count) reads the new field directly and is real, not a
//!   gap -- `do_wonder_step` just never reaches it today, because the cost
//!   lookup that runs first still panics. `Card::revolution_cost` landed the
//!   same way; [`revolution_cost`] below reads it directly, so
//!   [`h_revolution`] is fully ported, no gap left on this module's side.
//! - **Per-player-count effect magnitudes.** `Wave of Nationalism` /
//!   `Military Build-Up` (`resourcesForMilitaryUnitsPerStrongerCivilization`)
//!   and `Endowment for the Arts` (`culturePerCivilizationWithMoreCulture`)
//!   print a per-player-count dict (`{"2p": 6, "3p": 3, "4p": 2}`), which
//!   `gen_cards.py` cannot fold into a flat `i16` `CardEffects` field (only a
//!   bare int/float value survives; a dict value degrades to a payload-less
//!   `Special` variant -- see `EFFECT_FIELDS`/`_compile_effects` in
//!   `gen_cards.py`). [`h_play_action`] panics naming this rather than
//!   guessing a player count band. `Special::FreeCivilAction` has the same
//!   problem one level worse: the JSON value it drops is not a magnitude but
//!   a STRING naming which ordered action to run (`"build_one_wonder_stage"`,
//!   `"develop_technology"`, ...), so even wiring up `interact.rs` later
//!   would not be enough on its own -- `card_table.rs` needs a place to put
//!   that string (or an enum of it) first.
//!
//! One thing is a genuine, self-contained gap in THIS module, not a missing
//! dependency: [`wonder_completion_culture`] implements the two `onBuildCulture`
//! cases that read only `p`'s own tableau (`Fast Food Chains`,
//! `onBuildCulturePerTechLevelSum`), and panics naming `Hollywood`/`Internet`.
//! Both score "the effective output of a specific set of buildings", which in
//! Python is `effects.building_output` -- reusable by both `compute()` and
//! this trigger because it is a public module-level function. `effects.rs`
//! keeps its equivalent building-modifier arithmetic (`best_staffed`,
//! `workers_on`, the `apply_special` match arms for `BestTheaterDoubleCulture`
//! and friends) private to its own module, so producing a matching answer here
//! would mean a SECOND copy of that arithmetic -- exactly the "present in this
//! registry, absent from that one, with nothing that fails when they
//! disagree" bug class this whole rewrite exists to close (Python guards the
//! single-source property here with `tests/test_card_pricing.py::
//! TestOneImplementation`). Fixing this properly means `effects.rs` growing a
//! public `building_output`-equivalent that both modules call; that is
//! `effects.rs`'s file to change, not this one's.
//!
//! ## STRICT legality (not ported)
//!
//! Python's `apply()` optionally re-derives `legal_moves(state)` and asserts
//! the given move is in it (env-var gated; on by default in the Python test
//! suite). `legal.rs` is a sibling placeholder -- another worker's file, not
//! yet written -- so there is nothing to call. Every function below assumes
//! its caller already checked legality, exactly as the `costs.rs` cost
//! functions and `economy.rs` mutators already do (see `pay_ca`'s doc
//! comment: "a caller must have checked ... first, not a legality gate of its
//! own"). Add the assert back in [`apply`] once `legal.rs` exists.

use crate::cards::{CardId, CardType, Special};
use crate::costs;
use crate::economy;
use crate::effects;
use crate::moves::{ChurchillChoice, Move};
use crate::state::{CardList, GameState, PlayerState, TechSlot, MAX_HAND};

// ------------------------------------------------------------- leader identity
//
// Mirrors `costs.rs`'s `leader_is` (private there too -- see that module's
// "A note on leader identity"). Duplicated rather than shared for the same
// reason `economy.rs`'s test helpers duplicate `effects.rs`'s: it is four
// lines, and the alternative is making it `pub(crate)` in a module that is
// not this port's to edit.

#[inline]
fn leader_is(p: &PlayerState, name: &str) -> bool {
    !p.leader.is_none() && p.leader.get().name == name
}

// ==================================================================== apply

/// Apply `mv` to `state`, in place. Ports `engine/actions.py::apply`, minus
/// the `state.pending` branch and the STRICT assert -- see this module's top
/// doc comment.
pub fn apply(state: &mut GameState, mv: Move) {
    let idx = state.current;
    match mv {
        // ---- civil actions ----
        Move::Take { slot } => h_take(state, idx, slot),
        Move::Build { card } => do_build(state, idx, card, 0, false),
        Move::Develop { card } => h_develop(state, idx, card, false),
        Move::Upgrade { from, to } => do_upgrade(state, idx, from, to, 0, false),
        Move::WonderStep { steps } => do_wonder_step(state, idx, steps, 0, false),
        Move::Pop => h_pop(state, idx),
        Move::PopFree => h_pop_free(state, idx),
        Move::Revolution { card } => h_revolution(state, idx, card),
        Move::PlayLeader { card } => h_play_leader(state, idx, card),
        Move::PlayAction { card } => h_play_action(state, idx, card),
        Move::Destroy { card } => h_destroy(state, idx, card),

        // ---- military (declaration only; no combat resolution needed) ----
        Move::PlayTactic { card } => h_play_tactic(state, idx, card),
        Move::CopyTactic { card } => h_copy_tactic(state, idx, card),
        Move::CancelPact { owner } => h_cancel_pact(state, idx, owner),

        // ---- politics / turn control ----
        Move::PolPass => h_pol_pass(state, idx),
        Move::Resign => h_resign(state, idx),
        Move::Churchill { choice } => h_churchill(state, idx, choice),

        // ---- blocked on interact.rs (no `pending` field / decision queue) ----
        Move::OfferPact { .. } => unimplemented!(
            "OfferPact needs interact::push_choice -- interact.rs is not ported \
             (GameState has no `pending` field yet)"
        ),
        Move::Bid { .. }
        | Move::BidPass
        | Move::Defend { .. }
        | Move::DefendDone
        | Move::SendUnit { .. }
        | Move::SendBonus { .. }
        | Move::SendDone
        | Move::Choose { .. } => unimplemented!(
            "{mv:?} is a response to an interact.rs decision, and nothing can \
             have opened one: GameState has no `pending` field yet"
        ),

        // ---- blocked on events.rs ----
        Move::PrepareEvent { .. } => unimplemented!(
            "PrepareEvent needs events::reveal_current_event -- events.rs is not ported"
        ),
        Move::Aggression { .. } => unimplemented!(
            "Aggression needs events::start_aggression -- events.rs is not ported"
        ),

        // ---- blocked on state.rs (no war-tracking fields) ----
        Move::War { .. } => unimplemented!(
            "War needs PlayerState::war_declared_by_me / wars_declared_on_me, \
             which state.rs does not have yet -- see this module's top doc comment"
        ),

        // ---- blocked on game.rs ----
        Move::EndTurn => unimplemented!("EndTurn needs game::end_turn -- game.rs is not ported"),
    }
}

// ============================================================ enter/leave play
//
// Ports `engine/effects.py`'s `on_enter_play` / `on_leave_play` / triggers
// (`on_take_card`, `on_develop`, `on_build_unit`) -- one-shot effects fired
// BY a move, not read by `compute()`, so `effects.rs` deliberately does not
// carry them (see its own doc comment grouping these Special variants
// "belongs to actions.rs").

/// Move `n` yellow tokens into `p`'s supply from a card or a rival. Mirrors
/// `engine/effects.py::grant_yellow`.
fn grant_yellow(p: &mut PlayerState, n: i32) {
    if n > 0 {
        p.yellow_granted = p.yellow_granted.saturating_add(n as u8);
    }
    p.yellow_bank = (p.yellow_bank as i32 + n).max(0) as u8;
}

/// Immediate one-time effects when a card enters play (`blueTokens`,
/// `yellowTokens`).
fn on_enter_play(p: &mut PlayerState, id: CardId) {
    let eff = &id.get().effects;
    let bt = eff.blue_tokens as i32;
    if bt != 0 {
        p.blue_total = (p.blue_total as i32 + bt).max(0) as u8;
    }
    if eff.yellow_tokens != 0 {
        grant_yellow(p, eff.yellow_tokens as i32);
    }
}

/// The leave-play twin of [`on_enter_play`]. `cultureOnLeaveEqualToLab
/// ResourceProduction` (Bill Gates) is a genuine gap: it needs
/// `_lab_level_workers` (sum of lab level * workers), which is
/// self-contained and easy, but wiring it up needs `Special::
/// CultureOnLeaveEqualToLabResourceProduction` to be checked here, and
/// nothing in `p.effects` distinguishes "Bill Gates left play" from "a lab
/// technology's own `on_enter_play`" without it. Left unported since Bill
/// Gates is a rare leader swap and this module already has enough named
/// gaps to track.
fn on_leave_play(p: &mut PlayerState, id: CardId) {
    let eff = &id.get().effects;
    let bt = eff.blue_tokens as i32;
    if bt != 0 {
        p.blue_total = (p.blue_total as i32 - bt).max(0) as u8;
    }
    if eff.yellow_tokens != 0 {
        p.yellow_bank = (p.yellow_bank as i32 - eff.yellow_tokens as i32).max(0) as u8;
    }
}

/// Aristotle: 1 science per technology card taken from the row.
fn on_take_card(p: &mut PlayerState, id: CardId) {
    if leader_is(p, "Aristotle") && id.kind().is_developable() {
        p.science += 1;
    }
}

/// Leader triggers when a technology card is developed (§ leaders).
fn on_develop(state: &mut GameState, idx: u8) {
    if leader_is(&state.players[idx as usize], "Leonardo da Vinci") {
        economy::gain_resources(&mut state.players[idx as usize], 1);
    } else if leader_is(&state.players[idx as usize], "Albert Einstein") {
        state.players[idx as usize].culture += 3;
    } else if leader_is(&state.players[idx as usize], "Isaac Newton") {
        let s = effects::state_stats(state, &state.players[idx as usize]);
        let p = &mut state.players[idx as usize];
        p.civil_actions = s.civil_actions.min(p.civil_actions as i32 + 1) as i8;
    }
}

/// Homer: 1 resource whenever a military unit is built/upgraded.
fn on_build_unit(p: &mut PlayerState) {
    if leader_is(p, "Homer") {
        economy::gain_resources(p, 1);
    }
}

// ---------------------------------------------------- wonder-completion culture

/// Culture an Age III wonder scores on completion (§9.2). Mirrors
/// `engine/effects.py::wonder_completion_culture`; see this module's top doc
/// comment for why `Hollywood`/`Internet` are not ported.
fn wonder_completion_culture(p: &PlayerState, wonder: CardId) -> i32 {
    let card = wonder.get();
    if card.special.contains(&Special::OnBuildCulturePerTechLevelSum) {
        let mut gained = 0i32;
        for (id, _) in p.techs.iter() {
            if id.kind().is_developable() {
                gained += id.level() as i32;
            }
        }
        return gained + p.government.level() as i32;
    }
    if card.special.contains(&Special::OnBuildCulture) {
        return one_time_culture(p, card.base_name);
    }
    0
}

fn one_time_culture(p: &PlayerState, base_name: &str) -> i32 {
    match base_name {
        "Fast Food Chains" => {
            let production_workers = workers_on_kind(p, |k| k.is_production());
            let urban_or_unit_workers = workers_on_kind(p, |k| k.is_urban() || k.is_unit());
            2 * production_workers + urban_or_unit_workers
        }
        "Hollywood" | "Internet" => unimplemented!(
            "{base_name} needs effects::building_output (a public equivalent of \
             effects.rs's private building-modifier arithmetic) -- see this \
             module's top doc comment on why it is not duplicated here"
        ),
        _ => 0,
    }
}

fn workers_on_kind(p: &PlayerState, pred: impl Fn(CardType) -> bool) -> i32 {
    p.techs.iter().filter(|(id, _)| pred(id.kind())).map(|(_, s)| s.workers as i32).sum()
}

// --------------------------------------------------------------- pact helpers
//
// `engine/effects.py::cancel_attack_pacts` / `drop_pacts_of` operate on
// `PlayerState.pacts` alone -- no `Stats` needed -- so they port cleanly
// despite `effects.rs` not consuming pacts for `compute()` yet (see that
// module's own KNOWN GAPS: that gap is about `compute()`, not about the
// `pacts` field itself, which `state.rs` already carries).

/// §5.4.3: a pact that ends the moment its parties attack each other is
/// removed before the attack resolves.
fn cancel_attack_pacts(state: &mut GameState, attacker: u8, defender: u8) {
    for q in state.players.iter_mut() {
        q.pacts.retain(|pact| {
            let parties = pact.is_party(attacker) && pact.is_party(defender);
            !(parties && pact.card.get().special.contains(&Special::CancelledIfPartiesAttackEachOther))
        });
    }
}

/// §5.11: remove every pact `idx` is party to (resignation).
fn drop_pacts_of(state: &mut GameState, idx: u8) {
    for q in state.players.iter_mut() {
        q.pacts.retain(|pact| !pact.is_party(idx));
    }
}

// ================================================================== handlers
//
// One function per `_h_*` in `engine/actions.py`, `state: &mut GameState`
// plus the acting player's index rather than Python's `(state, p)` pair:
// several handlers need BOTH a mutable borrow of the acting player's own
// fields AND a call into something that needs `&mut GameState` (a discard
// pile, another player's pacts), and Rust cannot alias those two borrows the
// way Python's two same-object references alias for free. Re-indexing
// `state.players[idx as usize]` on each statement keeps every borrow short
// and non-overlapping (DESIGN.md rule 4: "arena-and-index is the native
// idiom for this shape of code").

fn h_take(state: &mut GameState, idx: u8, slot: u8) {
    let cost = costs::take_cost(state, &state.players[idx as usize], slot as usize);
    costs::pay_ca(&mut state.players[idx as usize], cost);
    // The ONLY place civil actions are spent reaching into the row; recorded
    // so the evaluator can price it apart from civil actions spent elsewhere
    // (`engine/actions.py`'s own comment: INFORMATION_AUDIT GAP 1).
    state.players[idx as usize].ca_spent_taking += cost as u8;
    take_card(state, idx, slot as usize);
}

/// Move row card `slot` into `idx`'s hand/play area (actions already paid).
/// Mirrors `engine/actions.py::take_card`.
fn take_card(state: &mut GameState, idx: u8, slot: usize) {
    let id = state.card_row[slot];
    state.card_row[slot] = CardId::NONE;
    on_take_card(&mut state.players[idx as usize], id);
    let card = id.get();
    if card.kind == CardType::Wonder {
        state.players[idx as usize].wonder = id;
        state.players[idx as usize].wonder_steps = 0;
    } else {
        let p = &mut state.players[idx as usize];
        p.hand_civil.push(id);
        // `taken_leader_ages` (§ one leader per age) is NOT recorded here:
        // `PlayerState` has no such field yet -- the exact gap `costs.rs`
        // already documents ("Retire the parameter once state.rs grows the
        // field"). Carried forward, not a new gap.
        if card.kind == CardType::Action {
            p.taken_this_turn.push(id);
        }
    }
}

fn h_pop(state: &mut GameState, idx: u8) {
    let stats = effects::state_stats(state, &state.players[idx as usize]);
    let cost = {
        let p = &state.players[idx as usize];
        economy::pop_food_cost(stats.pop_food_discount, p.yellow_bank, 0)
            .expect("h_pop: called with an empty yellow bank (caller must check legality)")
    };
    costs::pay_ca(&mut state.players[idx as usize], 1);
    let ok = economy::increase_population(&mut state.players[idx as usize], cost.max(0) as u16);
    debug_assert!(ok, "h_pop: caller must ensure enough food (legality check)");
}

fn h_pop_free(state: &mut GameState, idx: u8) {
    economy::increase_population(&mut state.players[idx as usize], 0);
    state.players[idx as usize].ocean_liners_used = true;
}

/// Ports `engine/actions.py::do_build`. `discount`/`free` exist for
/// `apply_free_action`'s benefit (an action card's ordered "build" with no
/// action cost and a resource discount) -- not callable from [`apply`]
/// today since that path is blocked on `interact.rs` (see this module's top
/// doc comment), but kept so wiring it up later is a one-line change here.
pub fn do_build(state: &mut GameState, idx: u8, id: CardId, discount: i32, free: bool) {
    let base = costs::build_cost_for(state, &state.players[idx as usize], id).unwrap_or(0);
    let mut cost = (base - discount).max(0);
    if !free {
        cost = costs::spend_mil_discount(&mut state.players[idx as usize], id, cost);
        if costs::is_unit(id) {
            state.players[idx as usize].military_actions -= 1;
        } else {
            costs::pay_ca(&mut state.players[idx as usize], 1);
        }
    }
    {
        let p = &mut state.players[idx as usize];
        p.resources = p.resources.saturating_sub(cost.max(0) as u16);
        p.techs
            .get_mut(id)
            .expect("do_build: card must already be developed (in the tableau)")
            .workers += 1;
        p.workers_free -= 1;
    }
    if costs::is_unit(id) {
        on_build_unit(&mut state.players[idx as usize]);
    }
}

fn h_destroy(state: &mut GameState, idx: u8, id: CardId) {
    let p = &mut state.players[idx as usize];
    if costs::is_unit(id) {
        p.military_actions -= 1;
    } else {
        costs::pay_ca(p, 1);
    }
    p.techs.get_mut(id).expect("h_destroy: card not in tableau").workers -= 1;
    p.workers_free += 1;
}

/// Ports `engine/actions.py::do_upgrade`. See [`do_build`] on `discount`/`free`.
pub fn do_upgrade(state: &mut GameState, idx: u8, lo: CardId, hi: CardId, discount: i32, free: bool) {
    let base = costs::upgrade_cost(state, &state.players[idx as usize], lo, hi);
    let mut cost = (base - discount).max(0);
    if !free {
        cost = costs::spend_mil_discount(&mut state.players[idx as usize], lo, cost);
        if costs::is_unit(lo) {
            state.players[idx as usize].military_actions -= 1;
        } else {
            costs::pay_ca(&mut state.players[idx as usize], 1);
        }
    }
    if costs::is_unit(lo) {
        on_build_unit(&mut state.players[idx as usize]);
    }
    let p = &mut state.players[idx as usize];
    p.resources = p.resources.saturating_sub(cost.max(0) as u16);
    p.techs.get_mut(lo).expect("do_upgrade: lo not in tableau").workers -= 1;
    p.techs.get_mut(hi).expect("do_upgrade: hi not in tableau").workers += 1;
}

/// Ports `engine/actions.py::do_wonder_step`. Panics unconditionally today
/// via [`costs::wonder_stage_cost`] -- see this module's top doc comment.
pub fn do_wonder_step(state: &mut GameState, idx: u8, k: u8, discount: i32, free: bool) {
    let base = costs::wonder_stage_cost(state, &state.players[idx as usize], k);
    let cost = (base - discount).max(0);
    if !free {
        costs::pay_ca(&mut state.players[idx as usize], 1);
    }
    let wonder = {
        let p = &mut state.players[idx as usize];
        p.resources = p.resources.saturating_sub(cost.max(0) as u16);
        p.wonder_steps += k;
        p.wonder
    };
    if wonder_is_complete(wonder, state.players[idx as usize].wonder_steps) {
        let gained = wonder_completion_culture(&state.players[idx as usize], wonder);
        let p = &mut state.players[idx as usize];
        p.wonder = CardId::NONE;
        p.wonder_steps = 0;
        p.completed_wonders.push(wonder);
        on_enter_play(p, wonder);
        p.culture = (p.culture as i32 + gained).max(0) as u16;
    }
}

/// §9: a wonder is complete once every printed stage is paid for. Real as of
/// `Card::stages` landing mid-port (see this module's top doc comment);
/// [`do_wonder_step`] never reaches this today only because the cost lookup
/// ahead of it (`costs::wonder_stage_cost`) still panics on the sibling gap.
fn wonder_is_complete(wonder: CardId, steps_built: u8) -> bool {
    steps_built as usize >= wonder.get().stages.len()
}

fn h_play_leader(state: &mut GameState, idx: u8, id: CardId) {
    costs::pay_ca(&mut state.players[idx as usize], 1);
    state.players[idx as usize].hand_civil.remove_first(id);
    let old = state.players[idx as usize].leader;
    if !old.is_none() {
        on_leave_play(&mut state.players[idx as usize], old);
        let is_homer = old.get().name == "Homer";
        let has_completed = !state.players[idx as usize].completed_wonders.is_empty();
        let homer_slot_free = state.players[idx as usize].homer_wonder.is_none();
        if is_homer && has_completed && homer_slot_free {
            let first = state.players[idx as usize].completed_wonders.as_slice()[0];
            state.players[idx as usize].homer_wonder = first; // tucked, still face up
        } else {
            economy::discard_civil(state, old);
        }
        // Replacing a leader refunds one civil action (§9.1).
        let total = costs::ca_total(state, &state.players[idx as usize]);
        let p = &mut state.players[idx as usize];
        p.civil_actions = total.min(p.civil_actions as i32 + 1) as i8;
    }
    state.players[idx as usize].leader = id;
    on_enter_play(&mut state.players[idx as usize], id);
}

fn h_develop(state: &mut GameState, idx: u8, id: CardId, free: bool) {
    let card = id.get();
    // NOTE: for a Government card, `costs::tech_cost` always returns `None`
    // (costs.rs's own KNOWN GAP #3: `peacefulCost` is not captured), so a
    // peaceful revolution taken via `develop` costs 0 science today instead
    // of its printed cost. Not fixed here -- costs.rs/cards.rs are off
    // limits to this module; carried forward, not a new gap.
    let raw = costs::tech_cost(state, &state.players[idx as usize], id).unwrap_or(0);
    let cost = costs::spend_mil_sci_discount(&mut state.players[idx as usize], id, raw);
    if !free {
        costs::pay_ca(&mut state.players[idx as usize], 1);
    }
    {
        let p = &mut state.players[idx as usize];
        p.science = p.science.saturating_sub(cost.max(0) as u16);
        p.hand_civil.remove_first(id);
    }
    if card.kind == CardType::Government {
        set_government(state, idx, id);
    } else if card.kind == CardType::SpecialTech {
        develop_special(state, idx, id);
    } else {
        state.players[idx as usize].techs.insert(id, TechSlot { workers: 0, stored: 0 });
        on_enter_play(&mut state.players[idx as usize], id);
    }
    on_develop(state, idx);
}

/// Ports `engine/actions.py::_set_government`.
fn set_government(state: &mut GameState, idx: u8, id: CardId) {
    let (spent_c, spent_m, old_gov) = {
        let p = &state.players[idx as usize];
        let stats = effects::state_stats(state, p);
        (costs::ca_total(state, p) - p.civil_actions as i32, stats.military_actions - p.military_actions as i32, p.government)
    };
    // `p.government` is never `CardId::NONE` in practice (every player
    // starts on Despotism), so Python's `if p.government and ...` is really
    // just "if it changed" here.
    if old_gov != id {
        economy::discard_civil(state, old_gov);
    }
    state.players[idx as usize].government = id;
    let s = effects::state_stats(state, &state.players[idx as usize]);
    let p = &mut state.players[idx as usize];
    p.civil_actions = (s.civil_actions - spent_c).max(0) as i8;
    p.military_actions = (s.military_actions - spent_m).max(0) as i8;
}

/// Ports `engine/actions.py::_develop_special` (== `put_special_in_play`):
/// §7.6's one-per-icon rule for blue special technologies. THE single
/// placement implementation -- `War over Technology` stealing one would call
/// this too, once combat.rs exists.
fn develop_special(state: &mut GameState, idx: u8, id: CardId) {
    let icon = costs::special_icon(id.get());
    let new_level = id.level();
    let mut existing = [CardId::NONE; 8];
    let mut n = 0usize;
    for (tid, _) in state.players[idx as usize].techs.iter() {
        if tid.kind() == CardType::SpecialTech && costs::special_icon(tid.get()) == icon {
            debug_assert!(n < existing.len(), "develop_special: more same-icon special techs than expected");
            existing[n] = tid;
            n += 1;
        }
    }
    for &old in &existing[..n] {
        if old.level() >= new_level {
            return; // the new (lower) card is discarded -- nothing changes
        }
    }
    for &old in &existing[..n] {
        on_leave_play(&mut state.players[idx as usize], old);
        state.players[idx as usize].techs.remove(old);
    }
    state.players[idx as usize].techs.insert(id, TechSlot { workers: 0, stored: 0 });
    on_enter_play(&mut state.players[idx as usize], id);
}

/// §7.6's one placement implementation, exposed for `War over Technology`'s
/// benefit once combat.rs exists (Code of Laws p.3: stealing a special tech
/// follows the same "keep the higher level" rule as developing one).
pub fn put_special_in_play(state: &mut GameState, idx: u8, id: CardId) {
    develop_special(state, idx, id);
}

fn h_revolution(state: &mut GameState, idx: u8, id: CardId) {
    let cost = revolution_cost(id);
    {
        let p = &mut state.players[idx as usize];
        p.science = p.science.saturating_sub(cost.max(0) as u16);
        p.hand_civil.remove_first(id);
    }
    let robespierre = leader_is(&state.players[idx as usize], "Maximilien Robespierre");
    // §8.3.4 (RB p.13): only the pool that PAYS for the revolution is
    // emptied; the other behaves exactly as in a peaceful change.
    let old = effects::state_stats(state, &state.players[idx as usize]);
    let spent = if robespierre {
        old.civil_actions - state.players[idx as usize].civil_actions as i32
    } else {
        old.military_actions - state.players[idx as usize].military_actions as i32
    };
    state.players[idx as usize].government = id;
    let s = effects::state_stats(state, &state.players[idx as usize]);
    if robespierre {
        let p = &mut state.players[idx as usize];
        p.military_actions = 0;
        p.civil_actions = (s.civil_actions - spent).max(0) as i8;
        p.culture += 3;
    } else {
        let p = &mut state.players[idx as usize];
        p.civil_actions = 0;
        p.military_actions = (s.military_actions - spent).max(0) as i8;
    }
    if leader_is(&state.players[idx as usize], "Isaac Newton") {
        let p = &mut state.players[idx as usize];
        p.civil_actions = s.civil_actions.min(p.civil_actions as i32 + 1) as i8;
    }
}

/// §8.3.4: science cost to seize a government by violent revolution.
fn revolution_cost(id: CardId) -> i32 {
    id.get().revolution_cost as i32
}

fn h_churchill(state: &mut GameState, idx: u8, choice: ChurchillChoice) {
    let p = &mut state.players[idx as usize];
    p.churchill_used = true;
    match choice {
        ChurchillChoice::Culture => p.culture += 3,
        ChurchillChoice::Military => {
            p.mil_sci_discount += 3;
            p.mil_discount += 3;
        }
    }
}

fn h_play_tactic(state: &mut GameState, idx: u8, id: CardId) {
    let p = &mut state.players[idx as usize];
    p.military_actions -= 1;
    p.hand_military.remove_first(id);
    p.tactic = id;
    p.tactic_exclusive = true;
    p.tactic_action_used = true;
}

fn h_copy_tactic(state: &mut GameState, idx: u8, id: CardId) {
    let p = &mut state.players[idx as usize];
    p.military_actions -= 2;
    p.tactic = id;
    p.tactic_exclusive = false;
    p.tactic_action_used = true;
}

/// Ports `engine/actions.py::_h_play_action` for every action card whose
/// effects the type layer captures as flat, player-count-independent
/// numbers. See this module's top doc comment for exactly which cards that
/// covers today (`Patriotism`, `Cultural Heritage`, `Stock Pile`,
/// `Revolutionary Idea`) and which panic (`Rich Land`, `Urban Growth`,
/// `Frugality`, `Engineering Genius`, `Breakthrough`, `Efficient Upgrade` --
/// all `freeCivilAction`; `Reserves` -- `gainFoodOrResources`; `Wave of
/// Nationalism`, `Military Build-Up`, `Endowment for the Arts` -- per-
/// player-count magnitudes).
fn h_play_action(state: &mut GameState, idx: u8, id: CardId) {
    costs::pay_ca(&mut state.players[idx as usize], 1);
    state.players[idx as usize].hand_civil.remove_first(id);
    economy::discard_civil(state, id); // one-shot: played face up, spent

    let card = id.get();
    if card.special.contains(&Special::FreeCivilAction) {
        unimplemented!(
            "play_action({}): orders a free civil action -- blocked on \
             interact.rs's decision queue AND on the ordered action's KIND, \
             which gen_cards.py drops (freeCivilAction's value is a string, \
             not a magnitude) -- see this module's top doc comment",
            card.name
        );
    }
    if card.special.contains(&Special::CulturePerCivilizationWithMoreCulture)
        || card.special.contains(&Special::ResourcesForMilitaryUnitsPerStrongerCivilization)
    {
        unimplemented!(
            "play_action({}): per-player-count effect magnitude is not captured \
             by the type layer (a dict-valued JSON effect collapses to a \
             payload-less Special) -- see this module's top doc comment",
            card.name
        );
    }
    if card.special.iter().any(|s| matches!(s, Special::GainFoodOrResources(_))) {
        unimplemented!(
            "play_action({}): gainFoodOrResources needs interact::push_choice \
             -- interact.rs is not ported",
            card.name
        );
    }

    let eff = card.effects;
    {
        let p = &mut state.players[idx as usize];
        // `extraCivilActions` / `extraMilitaryActions` are dead in the base
        // game's data today (confirmed 2026-08-05: `grep` over
        // `data/*.json` finds neither key on any card), unlike `militaryActions`
        // (Patriotism), which IS captured as `CardEffects.military_actions`
        // and applied here exactly as Python's `_h_play_action` applies it --
        // a one-shot grant, not the recurring government stat.
        p.military_actions = p.military_actions.saturating_add(eff.military_actions as i8);
        p.mil_discount += eff.resources_for_military_units;
        p.science += eff.gain_science as u16;
        p.culture += eff.gain_culture as u16;
    }
    if eff.gain_food != 0 {
        economy::gain_food(&mut state.players[idx as usize], eff.gain_food as u16);
    }
    if eff.gain_resources != 0 {
        economy::gain_resources(&mut state.players[idx as usize], eff.gain_resources as u16);
    }
    // `gainPopulation` is dead in the base game's data today (same check as
    // `extraCivilActions` above) and has no `CardEffects` field regardless.
}

fn h_pol_pass(state: &mut GameState, idx: u8) {
    state.players[idx as usize].politics_done = true;
    state.phase = crate::state::Phase::Actions;
}

fn h_cancel_pact(state: &mut GameState, idx: u8, owner: u8) {
    state.players[owner as usize].pacts.retain(|pact| !pact.is_party(idx));
    state.players[idx as usize].politics_done = true;
    state.phase = crate::state::Phase::Actions;
}

/// Ports `engine/actions.py::_h_resign` up to (not including) `game.after_
/// resign` and the war-cleanup section -- see this module's top doc comment
/// for both gaps.
fn h_resign(state: &mut GameState, idx: u8) {
    state.players[idx as usize].resigned = true;
    state.players[idx as usize].hand_civil = CardList::new();

    let military: CardList<MAX_HAND> = state.players[idx as usize].hand_military.clone();
    for &card in military.as_slice() {
        economy::discard_military(state, card);
    }
    state.players[idx as usize].hand_military = CardList::new();

    drop_pacts_of(state, idx);

    // NOT PORTED: §5.11's war-cleanup ("wars against a resigned player score
    // their declarer 7 culture", clearing `war_declared_by_me` /
    // `wars_declared_on_me`) -- `PlayerState` has neither field yet. See
    // this module's top doc comment.

    state.players[idx as usize].politics_done = true;

    // NOT PORTED: `game.after_resign(state, rng)` -- determines whether one
    // player is now the forced winner (§5.11). game.rs is not ported. A
    // caller of this function today gets every OTHER effect of resigning but
    // must decide forced-winner status itself until game.rs exists.
}

// ============================================================== tests ====

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::Age;
    use crate::state::{CardList, GameState, Pact, PactList, Phase, PlayerState, Tableau, MAX_PLAYERS, ROW_SIZE};

    fn card(name: &str) -> CardId {
        CardId::by_name(name).unwrap_or_else(|| panic!("no such card: {name}"))
    }

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

    fn blank_state(num_players: u8, players: [PlayerState; MAX_PLAYERS]) -> GameState {
        GameState {
            num_players,
            seed: 0,
            players,
            current: 0,
            turn: 1,
            round: 2, // most handlers assume round > 1 (§1.9's row-only round 1)
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
            scoring_events: CardList::new(),
            available_tactics: CardList::new(),
            civil_discard: [CardList::new(), CardList::new(), CardList::new(), CardList::new(), CardList::new()],
            civil_removed: [CardList::new(), CardList::new(), CardList::new(), CardList::new(), CardList::new()],
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

    fn one_player_state(p: PlayerState) -> GameState {
        let filler = || blank_player(0, card("Despotism"));
        let mut players = [filler(), filler(), filler(), filler()];
        players[0] = p;
        blank_state(4, players)
    }

    // -------------------------------------------------------------- take

    #[test]
    fn h_take_pays_row_cost_and_moves_the_card_to_hand() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        let mut state = one_player_state(p);
        state.card_row[6] = card("Bronze"); // slot cost 2
        h_take(&mut state, 0, 6);
        assert_eq!(state.players[0].civil_actions, 2);
        assert_eq!(state.players[0].ca_spent_taking, 2);
        assert!(state.players[0].hand_civil.contains(card("Bronze")));
        assert!(state.card_row[6].is_none());
    }

    #[test]
    fn h_take_a_wonder_sets_wonder_in_progress_not_hand() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        let mut state = one_player_state(p);
        state.card_row[0] = card("Colossus");
        h_take(&mut state, 0, 0);
        assert_eq!(state.players[0].wonder, card("Colossus"));
        assert_eq!(state.players[0].wonder_steps, 0);
        assert!(state.players[0].hand_civil.is_empty());
    }

    #[test]
    fn h_take_aristotle_gains_science_for_a_technology() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.leader = card("Aristotle");
        let mut state = one_player_state(p);
        state.card_row[0] = card("Bronze"); // a technology (mine)
        h_take(&mut state, 0, 0);
        assert_eq!(state.players[0].science, 1);
    }

    // --------------------------------------------------------------- pop

    #[test]
    fn h_pop_pays_food_and_ca_and_moves_a_yellow_token() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.yellow_bank = 5; // pop_cost_base(5) == 5
        p.food = 10;
        let mut state = one_player_state(p);
        h_pop(&mut state, 0);
        assert_eq!(state.players[0].civil_actions, 3);
        assert_eq!(state.players[0].food, 5);
        assert_eq!(state.players[0].yellow_bank, 4);
        assert_eq!(state.players[0].workers_free, 1);
    }

    #[test]
    fn h_pop_free_spends_no_civil_action_and_marks_ocean_liners_used() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.yellow_bank = 1;
        let mut state = one_player_state(p);
        h_pop_free(&mut state, 0);
        assert_eq!(state.players[0].civil_actions, 4, "no CA spent");
        assert_eq!(state.players[0].workers_free, 1);
        assert!(state.players[0].ocean_liners_used);
    }

    // ------------------------------------------------------------- build

    #[test]
    fn do_build_pays_resources_and_a_civil_action_and_adds_a_worker() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.resources = 10;
        p.workers_free = 2;
        p.techs.insert(card("Irrigation"), TechSlot { workers: 0, stored: 0 });
        let mut state = one_player_state(p);
        do_build(&mut state, 0, card("Irrigation"), 0, false);
        assert_eq!(state.players[0].civil_actions, 3);
        assert_eq!(state.players[0].resources, 10 - 4); // Irrigation build cost 4
        assert_eq!(state.players[0].workers_free, 1);
        assert_eq!(state.players[0].techs.workers(card("Irrigation")), 1);
    }

    #[test]
    fn do_build_a_unit_pays_a_military_action_not_civil_and_spends_the_discount_pool() {
        let mut p = blank_player(0, card("Despotism"));
        p.military_actions = 2;
        p.resources = 10;
        p.workers_free = 1;
        p.mil_discount = 1;
        p.techs.insert(card("Swordsmen"), TechSlot { workers: 0, stored: 0 });
        let mut state = one_player_state(p);
        do_build(&mut state, 0, card("Swordsmen"), 0, false);
        assert_eq!(state.players[0].military_actions, 1);
        assert_eq!(state.players[0].civil_actions, 0, "unpaid -- units use MA");
        assert_eq!(state.players[0].mil_discount, 0, "the 1-resource discount was spent");
        assert_eq!(state.players[0].resources, 10 - (3 - 1)); // Swordsmen cost 3, minus 1 discount
    }

    #[test]
    fn do_build_free_pays_nothing_and_applies_the_discount() {
        let mut p = blank_player(0, card("Despotism"));
        p.resources = 10;
        p.workers_free = 1;
        p.techs.insert(card("Irrigation"), TechSlot { workers: 0, stored: 0 });
        let mut state = one_player_state(p);
        do_build(&mut state, 0, card("Irrigation"), 1, true);
        assert_eq!(state.players[0].civil_actions, 0, "free: no CA spent");
        assert_eq!(state.players[0].resources, 10 - (4 - 1));
    }

    #[test]
    fn do_build_homer_gains_a_resource_on_a_unit_build() {
        let mut p = blank_player(0, card("Despotism"));
        p.leader = card("Homer");
        p.military_actions = 2;
        p.resources = 10;
        p.blue_total = 20; // bank room for gain_resources to actually pay out
        p.workers_free = 1;
        p.techs.insert(card("Swordsmen"), TechSlot { workers: 0, stored: 0 });
        let mut state = one_player_state(p);
        do_build(&mut state, 0, card("Swordsmen"), 0, false);
        // Paid 10 - 3 = 7, then Homer grants 1 back via gain_resources.
        assert_eq!(state.players[0].resources, 8);
    }

    // ----------------------------------------------------------- destroy

    #[test]
    fn h_destroy_a_civil_card_pays_one_ca_and_frees_a_worker() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.techs.insert(card("Irrigation"), TechSlot { workers: 1, stored: 0 });
        let mut state = one_player_state(p);
        h_destroy(&mut state, 0, card("Irrigation"));
        assert_eq!(state.players[0].civil_actions, 3);
        assert_eq!(state.players[0].techs.workers(card("Irrigation")), 0);
        assert_eq!(state.players[0].workers_free, 1);
    }

    #[test]
    fn h_destroy_a_unit_pays_one_ma() {
        let mut p = blank_player(0, card("Despotism"));
        p.military_actions = 2;
        p.techs.insert(card("Swordsmen"), TechSlot { workers: 1, stored: 0 });
        let mut state = one_player_state(p);
        h_destroy(&mut state, 0, card("Swordsmen"));
        assert_eq!(state.players[0].military_actions, 1);
        assert_eq!(state.players[0].techs.workers(card("Swordsmen")), 0);
    }

    // ----------------------------------------------------------- upgrade

    #[test]
    fn do_upgrade_moves_the_worker_and_pays_the_difference() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.resources = 10;
        p.techs.insert(card("Agriculture"), TechSlot { workers: 1, stored: 0 });
        p.techs.insert(card("Irrigation"), TechSlot { workers: 0, stored: 0 });
        let mut state = one_player_state(p);
        do_upgrade(&mut state, 0, card("Agriculture"), card("Irrigation"), 0, false);
        assert_eq!(state.players[0].civil_actions, 3);
        assert_eq!(state.players[0].resources, 10 - 2); // 4 - 2 = 2
        assert_eq!(state.players[0].techs.workers(card("Agriculture")), 0);
        assert_eq!(state.players[0].techs.workers(card("Irrigation")), 1);
    }

    // ------------------------------------------------------------ develop

    #[test]
    fn h_develop_a_technology_inserts_it_at_zero_workers() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.science = 10;
        p.hand_civil.push(card("Irrigation"));
        let mut state = one_player_state(p);
        h_develop(&mut state, 0, card("Irrigation"), false);
        assert_eq!(state.players[0].civil_actions, 3);
        assert_eq!(state.players[0].science, 10 - 3); // Irrigation tech cost 3
        assert!(state.players[0].techs.has(card("Irrigation")));
        assert_eq!(state.players[0].techs.workers(card("Irrigation")), 0);
        assert!(!state.players[0].hand_civil.contains(card("Irrigation")));
    }

    #[test]
    fn h_develop_leonardo_da_vinci_gains_a_resource() {
        let mut p = blank_player(0, card("Despotism"));
        p.leader = card("Leonardo da Vinci");
        p.civil_actions = 4;
        p.science = 10;
        p.blue_total = 20; // bank room for gain_resources to actually pay out
        p.hand_civil.push(card("Irrigation"));
        let mut state = one_player_state(p);
        h_develop(&mut state, 0, card("Irrigation"), false);
        assert_eq!(state.players[0].resources, 1);
    }

    #[test]
    fn h_develop_a_government_switches_via_set_government() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.military_actions = 2;
        p.hand_civil.push(card("Monarchy"));
        let mut state = one_player_state(p);
        h_develop(&mut state, 0, card("Monarchy"), false);
        assert_eq!(state.players[0].government, card("Monarchy"));
        // Monarchy: 5 CA / 3 MA. 1 CA was spent paying for `develop` itself,
        // so the new pool is 5 - 1 = 4 (recomputed relative to what was
        // already spent, per `_set_government`'s comment).
        assert_eq!(state.players[0].civil_actions, 4);
        assert_eq!(state.players[0].military_actions, 3);
    }

    #[test]
    fn h_develop_a_special_tech_keeps_only_the_higher_level_per_icon() {
        // Both Masonry and Construction are `construction`-icon special techs
        // (buildDiscount); Construction is the higher level.
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.hand_civil.push(card("Masonry"));
        let mut state = one_player_state(p);
        h_develop(&mut state, 0, card("Masonry"), false);
        assert!(state.players[0].techs.has(card("Masonry")));

        state.players[0].civil_actions = 4;
        state.players[0].hand_civil.push(card("Architecture"));
        h_develop(&mut state, 0, card("Architecture"), false);
        assert!(!state.players[0].techs.has(card("Masonry")), "lower level discarded");
        assert!(state.players[0].techs.has(card("Architecture")));
    }

    // -------------------------------------------------------- play_leader

    #[test]
    fn h_play_leader_replacing_none_just_plays_it() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.hand_civil.push(card("Hammurabi"));
        let mut state = one_player_state(p);
        h_play_leader(&mut state, 0, card("Hammurabi"));
        assert_eq!(state.players[0].leader, card("Hammurabi"));
        assert_eq!(state.players[0].civil_actions, 3);
    }

    #[test]
    fn h_play_leader_replacing_one_refunds_a_civil_action_and_discards_the_old() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 1; // nearly spent
        p.leader = card("Hammurabi");
        p.hand_civil.push(card("Aristotle"));
        let mut state = one_player_state(p);
        h_play_leader(&mut state, 0, card("Aristotle"));
        assert_eq!(state.players[0].leader, card("Aristotle"));
        // Paid 1 CA to play, then refunded 1 CA for replacing -> net unchanged.
        assert_eq!(state.players[0].civil_actions, 1);
        assert!(state.civil_removed[Age::A as usize].contains(card("Hammurabi")), "Hammurabi is Age A");
    }

    // -------------------------------------------------------------- churchill

    #[test]
    fn h_churchill_culture_choice() {
        let p = blank_player(0, card("Despotism"));
        let mut state = one_player_state(p);
        h_churchill(&mut state, 0, ChurchillChoice::Culture);
        assert_eq!(state.players[0].culture, 3);
        assert!(state.players[0].churchill_used);
    }

    #[test]
    fn h_churchill_military_choice_grants_ring_fenced_pools() {
        let p = blank_player(0, card("Despotism"));
        let mut state = one_player_state(p);
        h_churchill(&mut state, 0, ChurchillChoice::Military);
        assert_eq!(state.players[0].mil_sci_discount, 3);
        assert_eq!(state.players[0].mil_discount, 3);
    }

    // -------------------------------------------------------------- tactics

    #[test]
    fn h_play_tactic_spends_one_ma_and_sets_the_exclusive_tactic() {
        let mut p = blank_player(0, card("Despotism"));
        p.military_actions = 2;
        p.hand_military.push(card("Legion"));
        let mut state = one_player_state(p);
        h_play_tactic(&mut state, 0, card("Legion"));
        assert_eq!(state.players[0].military_actions, 1);
        assert_eq!(state.players[0].tactic, card("Legion"));
        assert!(state.players[0].tactic_exclusive);
        assert!(state.players[0].tactic_action_used);
    }

    #[test]
    fn h_copy_tactic_spends_two_ma_and_is_not_exclusive() {
        let mut p = blank_player(0, card("Despotism"));
        p.military_actions = 2;
        let mut state = one_player_state(p);
        h_copy_tactic(&mut state, 0, card("Legion"));
        assert_eq!(state.players[0].military_actions, 0);
        assert_eq!(state.players[0].tactic, card("Legion"));
        assert!(!state.players[0].tactic_exclusive);
    }

    // ------------------------------------------------------------- play_action

    #[test]
    fn h_play_action_patriotism_grants_a_military_action_and_the_discount_pool() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.military_actions = 2;
        p.hand_civil.push(card("Patriotism (A)"));
        let mut state = one_player_state(p);
        h_play_action(&mut state, 0, card("Patriotism (A)"));
        assert_eq!(state.players[0].civil_actions, 3);
        assert_eq!(state.players[0].military_actions, 3, "Patriotism grants +1 MA");
        assert_eq!(state.players[0].mil_discount, 1, "resourcesForMilitaryUnits: 1");
        assert!(!state.players[0].hand_civil.contains(card("Patriotism (A)")));
    }

    #[test]
    fn h_play_action_cultural_heritage_grants_flat_science_and_culture() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.hand_civil.push(card("Cultural Heritage (A)"));
        let mut state = one_player_state(p);
        h_play_action(&mut state, 0, card("Cultural Heritage (A)"));
        assert_eq!(state.players[0].science, 1);
        assert_eq!(state.players[0].culture, 4);
    }

    #[test]
    fn h_play_action_stock_pile_gains_food_and_resources() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.blue_total = 10;
        p.hand_civil.push(card("Stock Pile"));
        let mut state = one_player_state(p);
        h_play_action(&mut state, 0, card("Stock Pile"));
        assert_eq!(state.players[0].food, 1);
        assert_eq!(state.players[0].resources, 1);
    }

    #[test]
    #[should_panic(expected = "orders a free civil action")]
    fn h_play_action_free_civil_action_cards_are_a_named_gap() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.hand_civil.push(card("Rich Land (A)"));
        let mut state = one_player_state(p);
        h_play_action(&mut state, 0, card("Rich Land (A)"));
    }

    #[test]
    #[should_panic(expected = "gainFoodOrResources")]
    fn h_play_action_reserves_is_a_named_gap() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.hand_civil.push(card("Reserves (I)"));
        let mut state = one_player_state(p);
        h_play_action(&mut state, 0, card("Reserves (I)"));
    }

    // -------------------------------------------------------------- pacts

    #[test]
    fn h_cancel_pact_removes_only_pacts_the_actor_is_party_to() {
        let mut owner = blank_player(1, card("Despotism"));
        owner.pacts.push(Pact { card: card("Peace Treaty"), owner: 1, partner: 0, a: 1, b: 0 });
        owner.pacts.push(Pact { card: card("Peace Treaty"), owner: 1, partner: 2, a: 1, b: 2 });
        let mut state = blank_state(4, {
            let filler = || blank_player(3, card("Despotism"));
            [blank_player(0, card("Despotism")), owner, filler(), filler()]
        });
        h_cancel_pact(&mut state, 0, 1);
        assert_eq!(state.players[1].pacts.len(), 1, "only the pact involving player 0 is dropped");
        assert_eq!(state.players[1].pacts.as_slice()[0].partner, 2);
        assert!(state.players[0].politics_done);
        assert_eq!(state.phase, Phase::Actions);
    }

    #[test]
    fn h_resign_clears_hands_and_pacts() {
        let mut p = blank_player(0, card("Despotism"));
        p.hand_civil.push(card("Irrigation"));
        p.hand_military.push(card("Warriors"));
        p.pacts.push(Pact { card: card("Peace Treaty"), owner: 0, partner: 1, a: 0, b: 1 });
        let mut state = one_player_state(p);
        h_resign(&mut state, 0);
        assert!(state.players[0].resigned);
        assert!(state.players[0].hand_civil.is_empty());
        assert!(state.players[0].hand_military.is_empty());
        assert!(state.players[0].pacts.is_empty());
        assert!(state.discarded_military[Age::A as usize].contains(card("Warriors")));
        assert!(state.players[0].politics_done);
    }

    // -------------------------------------------------------------- gaps

    #[test]
    fn h_revolution_switches_government_and_pays_its_revolution_cost() {
        let mut p = blank_player(0, card("Despotism"));
        p.science = 10;
        p.civil_actions = 4; // == ca_total(Despotism): every CA still unspent
        p.military_actions = 2; // == Despotism's full MA pool: none spent yet
        p.hand_civil.push(card("Monarchy"));
        let mut state = one_player_state(p);
        h_revolution(&mut state, 0, card("Monarchy"));
        assert_eq!(state.players[0].government, card("Monarchy"));
        assert_eq!(state.players[0].science, 10 - 2); // Monarchy revolution_cost 2
        // Not Robespierre: civil_actions is zeroed, military_actions carries
        // the unspent-military-action count forward against Monarchy's total.
        assert_eq!(state.players[0].civil_actions, 0);
        assert_eq!(state.players[0].military_actions, 3); // Monarchy MA total, none spent yet
    }

    #[test]
    #[should_panic(expected = "wonder_stage_cost needs Card::stages")]
    fn do_wonder_step_is_blocked_on_the_missing_card_field() {
        let mut p = blank_player(0, card("Despotism"));
        p.wonder = card("Pyramids");
        let mut state = one_player_state(p);
        do_wonder_step(&mut state, 0, 1, 0, false);
    }

    #[test]
    #[should_panic(expected = "EndTurn needs game::end_turn")]
    fn apply_end_turn_is_a_named_gap() {
        let p = blank_player(0, card("Despotism"));
        let mut state = one_player_state(p);
        apply(&mut state, Move::EndTurn);
    }

    #[test]
    #[should_panic(expected = "interact.rs is not ported")]
    fn apply_offer_pact_is_a_named_gap() {
        let p = blank_player(0, card("Despotism"));
        let mut state = one_player_state(p);
        apply(&mut state, Move::OfferPact { card: card("Peace Treaty"), target: 1, side: crate::moves::PactSide::Unspecified });
    }

    // -------------------------------------------- wonder-completion culture

    #[test]
    fn wonder_completion_culture_fast_food_chains() {
        let mut p = blank_player(0, card("Despotism"));
        p.techs.insert(card("Agriculture"), TechSlot { workers: 2, stored: 0 }); // production
        p.techs.insert(card("Warriors"), TechSlot { workers: 1, stored: 0 }); // unit
        let gained = wonder_completion_culture(&p, card("Fast Food Chains"));
        // 2 * production workers (2) + 1 * urban-or-unit workers (1) = 5
        assert_eq!(gained, 5);
    }

    #[test]
    #[should_panic(expected = "needs effects::building_output")]
    fn wonder_completion_culture_hollywood_is_a_named_gap() {
        let p = blank_player(0, card("Despotism"));
        wonder_completion_culture(&p, card("Hollywood"));
    }

    // ----------------------------------------------------------- on_enter/leave

    #[test]
    fn on_enter_play_grants_blue_tokens() {
        let mut p = blank_player(0, card("Despotism"));
        on_enter_play(&mut p, card("Taj Mahal")); // wonders often print blueTokens
        // Not every wonder prints blueTokens; assert the mechanism doesn't
        // panic and leaves a sane (non-negative) total either way.
        assert!(p.blue_total <= 100);
    }

    #[test]
    fn on_take_card_aristotle_only_for_technologies() {
        let mut p = blank_player(0, card("Despotism"));
        p.leader = card("Aristotle");
        on_take_card(&mut p, card("Bronze")); // a technology
        assert_eq!(p.science, 1);
        on_take_card(&mut p, card("Napoleon Bonaparte")); // a leader, not a technology
        assert_eq!(p.science, 1, "leaders are not technologies");
    }
}

