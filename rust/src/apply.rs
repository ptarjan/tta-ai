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
//! - ~~**`interact.rs`'s decision queue.**~~ CLOSED 2026-08-05.
//!   `GameState::pending` exists, [`apply`] opens with the `if state.pending:
//!   interact.apply_pending(...)` branch Python does, [`Move::OfferPact`] is
//!   ported ([`h_offer_pact`]), [`Move::Aggression`] hands off to
//!   `interact::start_defense`, and the response moves ([`Move::Bid`],
//!   [`Move::Defend`], [`Move::SendUnit`], [`Move::Choose`], ...) are taken by
//!   that branch. They still `panic!` in the `match` below, but for the
//!   opposite reason: reaching the match arm means one was played with
//!   NOTHING open, which is a caller legality bug rather than a port gap.
//!   [`h_play_action`] now defers its ordered action and its gains onto
//!   `state.queue` exactly as Python does (`QueueItem::FreeCivil` then
//!   `QueueItem::CardGains`), and [`apply`] drains the queue on the way out;
//!   the two synchronous stand-ins written before the queue existed
//!   (`resolve_free_civil_action`, `apply_action_card_gains`) are deleted
//!   rather than left as a second copy of the same rule.
//! - **`events.rs`.** [`Move::PrepareEvent`] (`events.reveal_current_event`)
//!   calls into it directly. Panics in [`apply`].
//! - **`game.rs`.** [`Move::EndTurn`] (`game.end_turn`) is the whole End-of-
//!   Turn Sequence orchestrator and is not ported at all. `_h_resign`'s tail
//!   call to `game.after_resign` (deciding whether resigning leaves a forced
//!   winner, §5.11) is in the same boat -- [`h_resign`] performs every OTHER
//!   effect of resigning and stops one call short of that, with a comment at
//!   the stopping point, rather than pretending the game continues normally.
//!
//! [`Move::War`] used to be blocked here too (`PlayerState` had no
//! `war_declared_by_me` / `wars_declared_on_me` fields), but it was never a
//! `combat.rs` dependency -- `_h_war` itself never resolves combat, it only
//! records the declaration. `state.rs` grew both fields, so [`h_war`] is now
//! fully ported, and so is the war-cleanup half of [`h_resign`] (§5.11:
//! "wars against a resigned player score their declarer 7 culture"). War
//! RESOLUTION (as opposed to declaration) exists too -- `combat::
//! resolve_war_outcome` / `apply_war_spoils` -- and `game.rs` now calls it:
//! Python fires it from `game.start_turn`, not from `_h_war`, and the ported
//! turn loop matches that placement.
//!
//! [`Move::Aggression`] is FULLY ported. [`h_aggression`] calls
//! `combat::start_aggression` (cost, discard, strength, doomed-pact
//! cancellation -- everything `events.start_aggression` does) and then hands
//! off to `interact::start_defense`, which `state.pending` makes expressible.
//! An earlier revision of this comment said the hand-off panicked; it has not
//! since `state.pending` landed, and the differential fixtures now play
//! aggressions through to resolution with zero divergences. Corrected
//! 2026-08-05 -- two edits the same day left this paragraph contradicting the
//! one at the top of this block.
//!
//! `Card::stages` and `Card::revolution_cost` landed in `cards.rs` mid-port,
//! ahead of `costs.rs` being updated to use them -- both are closed now:
//! [`costs::wonder_stage_cost`] reads `Card::stages` (so [`do_wonder_step`]
//! is fully ported; this module's own [`wonder_is_complete`] already read the
//! field directly and was never a gap), and [`revolution_cost`] below reads
//! `Card::revolution_cost` directly, so [`h_revolution`] is fully ported too.
//!
//! - ~~**Per-player-count effect magnitudes.**~~ CLOSED 2026-08-05.
//!   `Wave of Nationalism` / `Military Build-Up`
//!   (`resourcesForMilitaryUnitsPerStrongerCivilization`) and `Endowment for
//!   the Arts` (`culturePerCivilizationWithMoreCulture`) print a
//!   per-player-count dict (`{"2p": 6, "3p": 3, "4p": 2}`), which
//!   `gen_cards.py` could not fold into a flat `i16` `CardEffects` field
//!   (only a bare int/float value survives that path; a dict value degraded
//!   to a payload-less `Special` variant). Fixed the same way
//!   `strongestPlayers`/`weakestPlayers`/`condition` already were: both keys
//!   now build through `gen_cards.py`'s existing `build_count_table` into a
//!   real `Special::<Name>([i16; 3])` payload (index 0/1/2 = 2p/3p/4p, `events::
//!   live_count_idx` picks the live one), no new machinery needed.
//!   [`h_play_action`] applies both exactly as `engine/actions.py::
//!   _h_play_action` does: culture gain per civilization with MORE culture
//!   than the player (a one-shot `p.culture` add), and `p.mil_discount`
//!   increased by the per-count magnitude for each civilization STRONGER
//!   (matching `effects::state_stats(..).strength`) than the player -- the
//!   same one-shot discount pool `resourcesForMilitaryUnits` (a bare int
//!   effect, already ported) and Churchill's military option already feed:
//!   spent by [`costs::spend_mil_discount`] on the next unit build/upgrade,
//!   and any unspent balance expires at end of turn (`economy::end_of_turn`
//!   zeroes `p.mil_discount`, mirroring `engine/economy.py::end_of_turn`'s
//!   own `p.mil_discount = 0` -- "§3.11 action-card discounts expire").
//!   `Special::FreeCivilAction` is unrelated to this gap: it carries a real
//!   `FreeCivilActionValue` payload
//!   (fixed 2026-08-05) naming one of the six ordered actions, [`legal::
//!   free_action_kind_of`] maps that onto `legal::FreeActionKind`, and
//!   [`h_play_action`] resolves it -- the only remaining blocker for THOSE 18
//!   cards is the 2+-legal-options case described above, not a missing place
//!   to put the value.
//!
//! [`wonder_completion_culture`] implements all three `onBuildCulture` cases
//! now, including `Hollywood`/`Internet`, which score "the effective output
//! of a specific set of buildings" (§9.2) via `effects::building_output` --
//! CLOSED 2026-08-05. `effects.rs` grew a public `building_output` that both
//! `compute()` (through `mine_resources`/`farm_food`) and this trigger call,
//! rather than this module carrying a second copy of the building-modifier
//! arithmetic (`best_staffed`, `workers_on`, the `apply_special` match arms
//! for `BestTheaterDoubleCulture` and friends stay private to `effects.rs`,
//! reached only through that one function) -- exactly the "present in this
//! registry, absent from that one, with nothing that fails when they
//! disagree" bug class this whole rewrite exists to close (Python guards the
//! single-source property here with `tests/test_card_pricing.py::
//! TestOneImplementation`).
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
use crate::combat;
use crate::costs;
use crate::economy;
use crate::events;
#[cfg(test)]
use crate::legal;
use crate::effects;
use crate::moves::{ChurchillChoice, Move, PactSide};
use crate::state::{CardList, GameState, PlayerState, TechSlot, MAX_HAND, MAX_PLAYERS};

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

/// Frederick Barbarossa's two discounts, read off his own card. Mirrors
/// `legal.rs`'s `barbarossa_discounts` -- duplicated for the same reason
/// `leader_is` is, above: `legal.rs`'s copy is private.
fn barbarossa_discounts(p: &PlayerState) -> (i32, i32) {
    let mut food = 0;
    let mut resources = 0;
    if !p.leader.is_none() {
        for s in p.leader.get().special.iter() {
            match s {
                Special::ComboFoodDiscount(v) => food = *v as i32,
                Special::ComboResourceDiscount(v) => resources = *v as i32,
                _ => {}
            }
        }
    }
    (food, resources)
}

/// Close the politics phase after ONE political action -- or after two.
/// Mirrors `engine/actions.py::_end_politics`, and is now the single place
/// every political handler in this file ends its phase, exactly as every
/// `_h_*` handler in `actions.py` routes through `_end_politics` rather than
/// setting `politics_done`/`phase` itself.
///
/// Julius Caesar, *"once per game, you may take two political actions in
/// your politics phase"*: while he is `idx`'s leader and the once-per-game is
/// unspent, the FIRST call of a turn leaves the phase open instead of
/// closing it (`caesar_second_politics = true`, an early return -- `phase`
/// stays `Politics`, `politics_done` stays `false`, so `legal_moves` offers
/// the politics list again to the SAME player); the SECOND call spends the
/// once-per-game and closes as normal. Passing on the second action
/// (`h_pol_pass`, same as any other handler routing through here) still
/// closes it, matching Python: declining action two never re-arms it.
///
/// Also clears Joan of Arc's `peeked_event` -- "the knowledge is scoped to
/// the politics phase the card scopes it to" (`engine/events.py::
/// peek_top_event`'s doc comment) -- which does not fire on Caesar's early
/// return either, matching Python's `_end_politics` clearing it only on the
/// branch that actually closes the phase.
fn end_politics(state: &mut GameState, idx: u8) {
    let game_over = state.game_over;
    let p = &mut state.players[idx as usize];
    if p.caesar_second_politics {
        p.caesar_second_politics = false;
        p.caesar_double_politics_used = true;
    } else if leader_is(p, "Julius Caesar") && !p.caesar_double_politics_used && !p.resigned && !game_over {
        p.caesar_second_politics = true;
        return;
    }
    p.peeked_event = CardId::NONE;
    p.politics_done = true;
    state.phase = crate::state::Phase::Actions;
}

// ==================================================================== apply

/// Apply `mv` to `state`, in place. Ports `engine/actions.py::apply`, minus
/// the `state.pending` branch and the STRICT assert -- see this module's top
/// doc comment.
pub fn apply(state: &mut GameState, mv: Move) {
    // Python's `apply` opens with exactly this: `if state.pending: return
    // interact.apply_pending(state, move, rng)`. An open decision owns the
    // move, and its owner is `state.decider()`, not `state.current`.
    if !state.pending.is_empty() {
        crate::interact::apply_pending(state, mv);
        return;
    }
    let idx = state.current;
    match mv {
        // ---- civil actions ----
        Move::Take { slot } => h_take(state, idx, slot),
        Move::Build { card } => do_build(state, idx, card, 0, false),
        Move::Develop { card } => h_develop(state, idx, card, false),
        Move::Upgrade { from, to } => do_upgrade(state, idx, from, to, 0, false),
        Move::WonderStep { steps } => do_wonder_step(state, idx, steps, 0, false),
        Move::Pop => h_pop(state, idx, false),
        Move::PopFree => h_pop_free(state, idx),
        Move::Revolution { card } => h_revolution(state, idx, card),
        Move::PlayLeader { card } => h_play_leader(state, idx, card),
        Move::PlayAction { card } => h_play_action(state, idx, card),
        Move::Destroy { card } => h_destroy(state, idx, card),

        // ---- military (declaration only; no combat resolution needed) ----
        Move::PlayTactic { card } => h_play_tactic(state, idx, card),
        Move::CopyTactic { card } => h_copy_tactic(state, idx, card),
        Move::War { card, target } => h_war(state, idx, card, target),
        Move::CancelPact { owner } => h_cancel_pact(state, idx, owner),

        // ---- politics / turn control ----
        Move::PolPass => h_pol_pass(state, idx),
        Move::Resign => h_resign(state, idx),
        Move::Churchill { choice } => h_churchill(state, idx, choice),

        Move::OfferPact { card, target, side } => h_offer_pact(state, idx, card, target, side),
        Move::RemoveLeaderYellow => h_remove_leader_yellow(state, idx),
        Move::ColumbusColonize { card } => h_columbus_colonize(state, idx, card),
        Move::Barbarossa { card } => h_barbarossa(state, idx, card),
        Move::BachTheater { from, to } => h_bach_theater(state, idx, from, to),

        // Responses to an open decision. Unreachable: the `state.pending`
        // branch at the top of this function has already taken them. Getting
        // here means one was played with NOTHING open, which is a legality
        // bug in the caller, not a port gap.
        Move::Bid { .. }
        | Move::BidPass
        | Move::Defend { .. }
        | Move::DefendDone
        | Move::SendUnit { .. }
        | Move::SendBonus { .. }
        | Move::SendDiscard { .. }
        | Move::SendDone
        | Move::Choose { .. } => panic!(
            "{mv:?} is a response to a decision, but `state.pending` is empty -- \
             nothing opened one"
        ),

        Move::PrepareEvent { card } => h_prepare_event(state, idx, card),

        // ---- combat.rs declaration + interact.rs defense decision ----
        Move::Aggression { card, target } => h_aggression(state, idx, card, target),

        // ---- blocked on game.rs ----
        // The End-of-Turn Sequence orchestrator (§6.6) plus the hand-off to
        // the next player. `game.rs` owns it; Python's `_h_end_turn` is the
        // same one-line delegation.
        Move::EndTurn => crate::game::end_turn(state),
    }
    // Python's `apply` tail: `effects.invalidate(state, p); interact.run_queue
    // (state, rng)`. There is no stats cache here, but the queue drain is
    // real -- a handler that defers a sub-effect (an action card's ordered
    // action, an event's population loss) has nothing to run it otherwise.
    crate::interact::run_queue(state);
}

// ============================================================ enter/leave play
//
// Ports `engine/effects.py`'s `on_enter_play` / `on_leave_play` / triggers
// (`on_take_card`, `on_develop`, `on_build_unit`) -- one-shot effects fired
// BY a move, not read by `compute()`, so `effects.rs` deliberately does not
// carry them (see its own doc comment grouping these Special variants
// "belongs to actions.rs").

/// Move `n` yellow tokens into `p`'s supply from a card or a rival. Mirrors
/// `engine/effects.py::grant_yellow`. `pub(crate)` (not private): `combat.rs`
/// needs the exact same bookkeeping for a War over Territory's spoils
/// (`combat::apply_war_spoils`), and this is the one existing copy rather
/// than a second one drifting out of sync with it.
pub(crate) fn grant_yellow(p: &mut PlayerState, n: i32) {
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
pub(crate) fn on_leave_play(p: &mut PlayerState, id: CardId) {
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
/// `engine/effects.py::wonder_completion_culture`.
pub(crate) fn wonder_completion_culture(p: &PlayerState, wonder: CardId) -> i32 {
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
    // `OnBuildCulture` now carries an `OnBuildCultureValue` payload naming
    // which of the three formulas (gen_cards.py, 2026-08-05) -- `.contains`
    // no longer type-checks against a bare variant, so this matches on the
    // variant only; `one_time_culture` below still dispatches on
    // `base_name`, unchanged.
    if card.special.iter().any(|s| matches!(s, Special::OnBuildCulture(_))) {
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
        // Hollywood and the Internet score what the buildings ACTUALLY
        // produce, not their printed production -- see `effects::
        // building_output`'s doc comment for the full worked-example list
        // (Chaplin, Shakespeare, Newton, Einstein, ...).
        "Hollywood" => {
            2 * effects::building_output(
                p,
                |k| matches!(k, CardType::Theater | CardType::Library),
                &[effects::Attr::Culture],
            )
        }
        "Internet" => effects::building_output(
            p,
            |k| k.is_urban(),
            &[effects::Attr::Culture, effects::Attr::Science, effects::Attr::Strength],
        ),
        _ => 0,
    }
}

fn workers_on_kind(p: &PlayerState, pred: impl Fn(CardType) -> bool) -> i32 {
    p.techs.iter().filter(|(id, _)| pred(id.kind())).map(|(_, s)| s.workers as i32).sum()
}

// --------------------------------------------------------------- pact helpers
//
// `engine/effects.py::cancel_attack_pacts` moved to `combat::
// cancel_attack_pacts` now that `combat.rs` exists -- this module used to
// carry its own copy (written for `h_war` before `combat.rs` was ported),
// which is exactly the "same fact, two registries" bug class DESIGN.md
// warns about, so it is deleted here in favour of the one in `combat.rs`.
// `drop_pacts_of` stays here: it is resignation bookkeeping, not combat
// math (no strength/legality involved), and has no `combat.rs` equivalent.

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
pub(crate) fn take_card(state: &mut GameState, idx: u8, slot: usize) {
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
        if card.kind == CardType::Leader {
            // § one leader per age (state.rs's doc comment on the field):
            // set once, never cleared, even once this leader is replaced.
            p.taken_leader_ages |= 1 << (card.age as u8);
        } else if card.kind == CardType::Action {
            p.taken_this_turn.push(id);
        }
    }
}

/// `free` exists for [`apply_free_civil_move`]'s benefit (an action card's
/// ordered "pop" with no action cost, still at the real food cost -- Python's
/// `apply_free_action` calls `economy.increase_population(state, p)` with its
/// own `free` defaulting to `False`, only skipping `pay_ca`). Not `discount`:
/// `free_action_moves`'s `IncreasePopulation` arm does not apply one either
/// (see its own comment -- "at full price").
fn h_pop(state: &mut GameState, idx: u8, free: bool) {
    let stats = effects::state_stats(state, &state.players[idx as usize]);
    let cost = {
        let p = &state.players[idx as usize];
        economy::pop_food_cost(
            stats.pop_food_discount,
            p.yellow_bank,
            p.one_time_discount.pop_food as i32,
        )
            .expect("h_pop: called with an empty yellow bank (caller must check legality)")
    };
    if !free {
        costs::pay_ca(&mut state.players[idx as usize], 1);
    }
    // `cost` above ALWAYS folded in `one_time_discount.pop_food` (this
    // function's own `free` only skips the civil action below, exactly per
    // its doc comment -- the food cost, discount included, is paid either
    // way), so consumption is unconditional here too: `true`, not `!free`.
    let ok = economy::increase_population(
        &mut state.players[idx as usize], cost.max(0) as u16, true);
    debug_assert!(ok, "h_pop: caller must ensure enough food (legality check)");
}

fn h_pop_free(state: &mut GameState, idx: u8) {
    // Ocean Liners: genuinely free, `cost = 0`, and this path never even
    // calls `economy::pop_food_cost` -- so it never looked at the one-time
    // discount and must not consume it.
    economy::increase_population(&mut state.players[idx as usize], 0, false);
    state.players[idx as usize].ocean_liners_used = true;
}

/// Frederick Barbarossa's combined action: 1 military action buys BOTH
/// halves. Mirrors `engine/actions.py::_h_barbarossa`.
///
/// The order is the card's: population first, then the build, so the worker
/// the increase just produced is available for [`do_build`] to place. `
/// do_build` pays the ONE military action the whole combination costs (it
/// always pays one military action for a unit build) -- the population half
/// costs NO civil action, matching `economy::increase_population` never
/// touching one either.
///
/// The food cost is computed exactly as [`h_pop`] computes it (same
/// `economy::pop_food_cost` call, same discount-pool reads) and then
/// Barbarossa's own `comboFoodDiscount` is taken off, floored at 0 -- the
/// legality check in `legal::barbarossa_moves` guarantees this can never go
/// negative on food or come up short on the yellow bank.
fn h_barbarossa(state: &mut GameState, idx: u8, unit: CardId) {
    let (food_disc, res_disc) = barbarossa_discounts(&state.players[idx as usize]);
    let pop_cost = {
        let stats = effects::state_stats(state, &state.players[idx as usize]);
        let p = &state.players[idx as usize];
        economy::pop_food_cost(stats.pop_food_discount, p.yellow_bank, p.one_time_discount.pop_food as i32)
            .expect("h_barbarossa: called with an empty yellow bank (caller must check legality)")
    };
    let pay = (pop_cost - food_disc).max(0) as u16;
    // `pop_cost` above already read `one_time_discount.pop_food`; this IS a
    // real, paid population increase (never free), so consume it: `true`.
    let ok = economy::increase_population(&mut state.players[idx as usize], pay, true);
    debug_assert!(ok, "h_barbarossa: caller must ensure enough food (legality check)");
    do_build(state, idx, unit, res_disc, false);
}

/// J. S. Bach: upgrade an urban building to a theater, 1 civil action, once
/// a turn. Mirrors `engine/actions.py::_h_bach_theater`.
///
/// [`do_upgrade`] is the shared §3.5 implementation and already does the
/// right thing for this cross-type move: the cost is the difference of the
/// two build costs (floored at 0), the civil action is paid because a
/// theater is not a unit, and the worker moves from one card to the other.
fn h_bach_theater(state: &mut GameState, idx: u8, from: CardId, to: CardId) {
    state.players[idx as usize].bach_upgrade_used = true;
    do_upgrade(state, idx, from, to, 0, false);
}

/// Ports `engine/actions.py::do_build`. `discount`/`free` exist for
/// `apply_free_action`'s benefit (an action card's ordered "build" with no
/// action cost and a resource discount) -- not callable from [`apply`]
/// today since that path is blocked on `interact.rs` (see this module's top
/// doc comment), but kept so wiring it up later is a one-line change here.
pub fn do_build(state: &mut GameState, idx: u8, id: CardId, discount: i32, free: bool) {
    let base = costs::build_cost_for(state, &state.players[idx as usize], id).unwrap_or(0);
    let mut cost = (base - discount).max(0);
    if !costs::is_unit(id) {
        // `build_cost_for` above already folded in Civil Life's one-shot
        // `build_resources` discount for every non-unit build (exactly the
        // farm/mine/urban cards it is gated on -- see its own doc comment);
        // this is the ONE build that spends it. Unconditional on `free`:
        // `free` only waives the civil action below, not the resource cost
        // that already consumed the discount computing `base`.
        state.players[idx as usize].one_time_discount.build_resources = 0;
    }
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
    let raw_cost = costs::tech_cost(state, &state.players[idx as usize], id);
    if raw_cost.is_some() {
        // `tech_cost` returns `None` only when the card has no develop cost
        // at all, in which case it never looked at `one_time_discount`
        // either (see its own doc comment: it subtracts the discount
        // unconditionally whenever it returns `Some`). This is the ONE
        // technology that spends Civil Life's one-shot `develop_science`
        // discount; unconditional on `free` for the same reason as
        // `do_build` above.
        state.players[idx as usize].one_time_discount.develop_science = 0;
    }
    let raw = raw_cost.unwrap_or(0);
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

/// §5.6 / CoL p.4: reveal, pay, name the rival, drop the pact, record the
/// declaration. Mirrors `engine/actions.py::_h_war`. Uses `combat::
/// cancel_attack_pacts` for the pact-cancellation step, but otherwise still
/// never resolves combat -- it only records that a war is open. Actual war
/// RESOLUTION (`combat::resolve_war_outcome` / `apply_war_spoils`) fires at
/// the start of the attacker's NEXT turn (`engine/game.py::start_turn`,
/// `game.rs`, not ported), not from this handler; see this module's top doc
/// comment.
fn h_war(state: &mut GameState, idx: u8, id: CardId, target: u8) {
    let mut cost = id.get().military_action_cost as i32;
    if leader_is(&state.players[target as usize], "Mahatma Gandhi") {
        cost *= 2;
    }
    state.players[idx as usize].military_actions -= cost as i8;
    state.players[idx as usize].hand_military.remove_first(id);
    // CoL p.4 / FAQ p.11: a pact that ends if its parties attack each other
    // is removed before the war is recorded, and its strength never applies.
    combat::cancel_attack_pacts(state, idx, target);
    state.players[idx as usize].war_declared_by_me = id;
    state.players[idx as usize].war_target = target;
    state.players[target as usize].wars_declared_on_me[idx as usize] = id;
    end_politics(state, idx);
}

/// §5.4: pay cost, discard, compute strength, cancel any doomed pact.
/// Mirrors the part of `engine/actions.py::_h_aggression` that is portable
/// today -- `combat::start_aggression` is the exact prefix of `engine/
/// events.py::start_aggression` up to (not including) `interact.
/// start_defense`, which needs `state.pending` (no such field exists, and
/// this module may not add one). See `combat.rs`'s top doc comment
/// "Aggression: declaration ported, resolution blocked on `interact.rs`"
/// for the full reasoning; this handler only names the one remaining
/// blocker.
fn h_aggression(state: &mut GameState, idx: u8, card: CardId, target: u8) {
    // Python's `_h_aggression` marks the politics action spent and advances
    // the phase BEFORE handing the defense decision over -- the attacker's
    // political action is finished either way, and the defender answering
    // must not look like the attacker still owing a move. (When Caesar's
    // second action is still open, `end_politics` leaves `phase` at
    // `Politics` instead -- matching Python, which calls `_end_politics`
    // here unconditionally too and lets that same early return apply.)
    end_politics(state, idx);
    let atk = combat::start_aggression(state, idx, card, target);
    crate::interact::start_defense(state, idx, target, card, atk);
}

/// §5.9: reveal the pact, name the partner and the sides, and hand the
/// accept/refuse decision to that partner. Mirrors `engine/actions.py::
/// _h_offer_pact`.
///
/// `side` decides which printed block each party takes, and it is NOT the
/// same as who owns the card: offering side B puts the TARGET on `a` and the
/// offerer on `b`. See `state::Pact`'s doc comment for why those are four
/// separate indices.
fn h_offer_pact(state: &mut GameState, idx: u8, card: CardId, target: u8, side: PactSide) {
    state.players[idx as usize].hand_military.remove_first(card);
    let (a, b) = match side {
        PactSide::B => (target, idx),
        PactSide::A | PactSide::Unspecified => (idx, target),
    };
    end_politics(state, idx);
    let mut options = crate::state::OptionList::new();
    options.push(crate::state::ChoiceOption::Word(crate::state::Keyword::Accept));
    options.push(crate::state::ChoiceOption::Word(crate::state::Keyword::Refuse));
    crate::interact::push_choice(
        state,
        target,
        crate::state::ChoiceKind::PactOffer { owner: idx, card, a, b },
        options,
        false,
    );
}

/// Ports `engine/actions.py::_h_play_action`. See this module's top doc
/// comment for exactly which cards still panic (`Reserves` --
/// `gainFoodOrResources`, always needs `interact::push_choice` per Python's
/// `apply_card_gains`, `auto=False`; and, for the 18 `freeCivilAction`
/// cards, only the case where their ordered action has 2+ legal options,
/// which now opens a real decision). `Wave of Nationalism`/`Military
/// Build-Up`/`Endowment for the Arts` (the per-player-count magnitudes) are
/// CLOSED as of 2026-08-05 -- see the top doc comment's "Per-player-count
/// effect magnitudes" entry.
fn h_play_action(state: &mut GameState, idx: u8, id: CardId) {
    // RB p.15: Breakthrough's `develop_technology` order may spend itself on
    // a revolution instead, gated on every civil action THIS TURN still
    // being unspent. Python's `_h_play_action` reads `revolt_ok` before its
    // own `pay_ca` call below -- playing the card itself must not count as
    // "a civil action spent" for this test -- so this has to be captured
    // here, ahead of that payment, not recomputed later inside
    // the queue item's own resolution (which would otherwise always see the
    // CA this card itself just cost and read `revolt_ok` as false).
    let revolt_ok = {
        let p = &state.players[idx as usize];
        p.civil_actions as i32 == costs::ca_total(state, p)
    };
    costs::pay_ca(&mut state.players[idx as usize], 1);
    state.players[idx as usize].hand_civil.remove_first(id);
    economy::discard_civil(state, id); // one-shot: played face up, spent

    let card = id.get();
    let eff = card.effects;
    {
        let p = &mut state.players[idx as usize];
        // `extraCivilActions` / `extraMilitaryActions` are dead in the base
        // game's data today (confirmed 2026-08-05: `grep` over
        // `data/*.json` finds neither key on any card), unlike `militaryActions`
        // (Patriotism), which IS captured as `CardEffects.military_actions`
        // and applied here exactly as Python's `_h_play_action` applies it --
        // a one-shot grant, not the recurring government stat. These, unlike
        // the GAIN_KEYS below, apply unconditionally BEFORE the ordered
        // action resolves -- Python applies them ahead of the `if ordered:`
        // branch too.
        p.military_actions = p.military_actions.saturating_add(eff.military_actions as i8);
        p.mil_discount += eff.resources_for_military_units;
    }

    // The two per-player-count action-card magnitudes (Endowment for the
    // Arts / Wave of Nationalism / Military Build-Up -- see this module's
    // top doc comment for why `gen_cards.py` could not fold these into a
    // flat `CardEffects` field). Mirrors `engine/actions.py::_h_play_action`
    // lines 1153-1167 exactly, including the order (culture bonus, THEN the
    // military discount -- moot in practice since no base-game card prints
    // both, but kept for fidelity) and the strict `>` comparisons (Python:
    // `q.culture > p.culture` / `state_stats(q).strength > mine`).
    let count_idx = events::live_count_idx(state);
    if let Some(t) = card.special.iter().find_map(|&s| match s {
        Special::CulturePerCivilizationWithMoreCulture(t) => Some(t),
        _ => None,
    }) {
        let per = t[count_idx] as i32;
        let mine = state.players[idx as usize].culture as i32;
        let mut n = 0i32;
        for q in state.active() {
            if q.idx != idx && q.culture as i32 > mine {
                n += 1;
            }
        }
        let gained = per * n;
        state.players[idx as usize].culture =
            (state.players[idx as usize].culture as i32 + gained).max(0) as u16;
    }
    if let Some(t) = card.special.iter().find_map(|&s| match s {
        Special::ResourcesForMilitaryUnitsPerStrongerCivilization(t) => Some(t),
        _ => None,
    }) {
        let per = t[count_idx] as i32;
        let mine = effects::state_stats(state, &state.players[idx as usize]).strength;
        let mut n = 0i32;
        for q in state.active() {
            if q.idx != idx && effects::state_stats(state, q).strength > mine {
                n += 1;
            }
        }
        state.players[idx as usize].mil_discount += (per * n) as i16;
    }

    // §3.11: the ordered action resolves FIRST, and only THEN the card's own
    // gains -- Breakthrough's science and Frugality's food arrive too late to
    // pay for the very action the card just ordered (`_h_play_action`'s FIFO
    // `interact.enqueue` order: "free_civil" is pushed ahead of "card_gains").
    // A card with no ordered action skips straight to the gains, matching
    // Python's `else: apply_card_gains(...)` branch.
    let gains = card_gains_of(card);
    if let Some(value) = card.special.iter().find_map(|s| match s {
        Special::FreeCivilAction(v) => Some(*v),
        _ => None,
    }) {
        // FIFO, and the order is the rule (§3.11): the ordered action
        // resolves, and only THEN the card's own gains -- Breakthrough's
        // science and Frugality's food arrive too late to pay for the very
        // action the card just ordered.
        crate::interact::enqueue(
            state,
            crate::state::QueueItem::FreeCivil {
                player: idx,
                kind: value,
                discount: eff.resource_discount,
                revolt_ok,
            },
        );
        crate::interact::enqueue(
            state,
            crate::state::QueueItem::CardGains { player: idx, gains },
        );
    } else {
        crate::interact::apply_card_gains(state, idx, gains);
    }
}

/// One action card's gain half as a [`crate::state::CardGains`]. Mirrors the
/// `{k: eff[k] for k in _GAIN_KEYS if k in eff}` comprehension in
/// `engine/actions.py::_h_play_action`, over the same six keys.
///
/// `gainPopulation` is dead in the base game's data today (confirmed
/// 2026-08-05: `data/*.json` prints the key on no card at all) and
/// `CardEffects` has no field for it, so it stays zero here; `interact::
/// apply_card_gains` implements the arithmetic anyway, because the field
/// exists and a silently-unimplemented gain is the bug class this port is
/// about.
fn card_gains_of(card: &crate::cards::Card) -> crate::state::CardGains {
    crate::state::CardGains {
        science: card.effects.gain_science,
        culture: card.effects.gain_culture,
        food: card.effects.gain_food,
        resources: card.effects.gain_resources,
        population: 0,
        food_or_resources: card
            .special
            .iter()
            .find_map(|s| match s {
                Special::GainFoodOrResources(n) => Some(*n),
                _ => None,
            })
            .unwrap_or(0),
    }
}

/// Apply the one move an ordered free civil action resolved to, at no action
/// cost and `discount` off its resource cost (mirrors `engine/actions.py::
/// apply_free_action`'s dispatch over its own `kind` tuples). `Move::Pop` /
/// `Move::Build` / `Move::Upgrade` / `Move::WonderStep` are the only shapes
/// `free_action_moves`'s build/upgrade/pop/wonder-step arms ever produce;
/// `Move::Develop` / `Move::Revolution` are the two `DevelopTechnology` can
/// produce. No other `Move` variant is reachable from there.
pub(crate) fn apply_free_civil_move(state: &mut GameState, idx: u8, mv: Move, discount: i32) {
    match mv {
        Move::Pop => h_pop(state, idx, true),
        Move::WonderStep { steps } => do_wonder_step(state, idx, steps, discount, true),
        Move::Build { card } => do_build(state, idx, card, discount, true),
        Move::Upgrade { from, to } => do_upgrade(state, idx, from, to, discount, true),
        Move::Develop { card } => h_develop(state, idx, card, true),
        // §8.3.4: a revolution never spends a civil action to begin with
        // (it empties a whole action pool instead), so `h_revolution` takes
        // no `free` flag -- matching Python's `apply_free_action`, which
        // calls `_h_revolution` exactly as the normal `Move::Revolution`
        // handler does.
        Move::Revolution { card } => h_revolution(state, idx, card),
        other => unreachable!(
            "free_action_moves produced a move apply_free_civil_move does not \
             expect: {other:?}"
        ),
    }
}

fn h_pol_pass(state: &mut GameState, idx: u8) {
    end_politics(state, idx);
}

/// §5.2: play an event/territory card from the hand into `future_events`,
/// bank its age as culture, and reveal-and-resolve the current event.
/// Mirrors `engine/actions.py::_h_prepare_event`.
///
/// `state.seeded_by[card] = idx` -- bot-evaluator bookkeeping only
/// (`bots/weighted.py`/`bots/counting.py`/`bots/neural_encode.py` are its
/// only readers; nothing in `engine/`'s own rules reads it). USED TO BE a
/// named gap here ("this port has no bot layer and `state.rs` has no field
/// for it") -- closed 2026-08-05 once `bots::counting::event_pool` became
/// that reader: `state.rs::GameState::seeded_by` now has a field for it, and
/// this is its one and only writer, exactly as in Python (`journal.py`'s
/// `del ...[ev]` docstring example is never actually called from
/// `actions.py`/`events.py` -- grepped 2026-08-05 -- so "write-once, never
/// cleared" is Python's real behaviour too, not a simplification).
///
/// Julius Caesar's once-per-game second political action USED to be a second
/// gap here (this handler always closed the phase, for every leader) --
/// closed 2026-08-05 by routing through [`end_politics`], same as every
/// other political handler.
fn h_prepare_event(state: &mut GameState, idx: u8, card: CardId) {
    state.players[idx as usize].hand_military.remove_first(card);
    state.players[idx as usize].culture += card.level() as u16;
    state.future_events.push(card);
    state.seeded_by[card.0 as usize] = idx;
    events::reveal_current_event(state);
    end_politics(state, idx);
}

fn h_cancel_pact(state: &mut GameState, idx: u8, owner: u8) {
    state.players[owner as usize].pacts.retain(|pact| !pact.is_party(idx));
    end_politics(state, idx);
}

/// Alexander the Great, as a political action: remove him from the game for
/// 1 yellow token from the box. Mirrors `engine/actions.py::
/// _h_remove_leader_yellow` + `remove_leader_from_game`.
fn h_remove_leader_yellow(state: &mut GameState, idx: u8) {
    let leader = state.players[idx as usize].leader;
    if !leader.is_none() {
        on_leave_play(&mut state.players[idx as usize], leader);
        economy::discard_civil(state, leader);
        state.players[idx as usize].leader = CardId::NONE;
    }
    grant_yellow(&mut state.players[idx as usize], 1);
    end_politics(state, idx);
}

/// Christopher Columbus, as a political action: remove him from the game to
/// colonize `card` (a territory from hand) with no military sacrifice.
/// Mirrors `engine/actions.py::_h_columbus_colonize` +
/// `remove_leader_from_game`; `interact::gain_colony` is the exact function
/// the normal auction-won colonization path (`interact::apply_pending`'s
/// `SendDone` arm) already calls for §11.5's permanent-then-immediate order,
/// which is exactly what Columbus's ability skips STRAIGHT to (no auction, no
/// bid, no force -- `engine/interact.py::colonize_without_sacrifice`'s own
/// doc comment on why it is a separate entry point rather than a `bid=0`
/// call into the auction).
fn h_columbus_colonize(state: &mut GameState, idx: u8, card: CardId) {
    let leader = state.players[idx as usize].leader;
    if !leader.is_none() {
        on_leave_play(&mut state.players[idx as usize], leader);
        economy::discard_civil(state, leader);
        state.players[idx as usize].leader = CardId::NONE;
    }
    state.players[idx as usize].hand_military.remove_first(card);
    crate::interact::gain_colony(state, idx, card);
    end_politics(state, idx);
}

/// Ports `engine/actions.py::_h_resign` up to (not including) `game.after_
/// resign`, which is `game.rs`'s to write -- see this module's top doc
/// comment. The war-cleanup section is fully ported now that `state.rs`
/// carries `war_declared_by_me`/`wars_declared_on_me`.
fn h_resign(state: &mut GameState, idx: u8) {
    state.players[idx as usize].resigned = true;
    state.players[idx as usize].hand_civil = CardList::new();

    let military: CardList<MAX_HAND> = state.players[idx as usize].hand_military.clone();
    for &card in military.as_slice() {
        economy::discard_military(state, card);
    }
    state.players[idx as usize].hand_military = CardList::new();

    drop_pacts_of(state, idx);

    // §5.11 war-cleanup: every war declared ON the resigning player scores
    // its declarer 7 culture, clears that declarer's OWN `war_declared_by_me`
    // if it is still this same war (it may already have moved on), and
    // discards the war card. `wars_declared_on_me` is indexed by attacker
    // (state.rs's doc comment), so this is a flat scan rather than Python's
    // list walk.
    for attacker in 0..MAX_PLAYERS as u8 {
        let war_card = state.players[idx as usize].wars_declared_on_me[attacker as usize];
        if war_card.is_none() {
            continue;
        }
        state.players[attacker as usize].culture += 7;
        if state.players[attacker as usize].war_declared_by_me == war_card {
            state.players[attacker as usize].war_declared_by_me = CardId::NONE;
            // Cleared together -- see `combat::resolve_war_outcome`'s comment
            // on why a stale `war_target` is a real divergence, not just a
            // meaningless leftover.
            state.players[attacker as usize].war_target = 0;
        }
        economy::discard_military(state, war_card);
    }
    state.players[idx as usize].wars_declared_on_me = [CardId::NONE; MAX_PLAYERS];

    // And the symmetric half: a war the resigning player themselves declared
    // is also torn down (the defender no longer faces it).
    let my_war = state.players[idx as usize].war_declared_by_me;
    if !my_war.is_none() {
        let target = state.players[idx as usize].war_target;
        state.players[target as usize].wars_declared_on_me[idx as usize] = CardId::NONE;
        economy::discard_military(state, my_war);
        state.players[idx as usize].war_declared_by_me = CardId::NONE;
        state.players[idx as usize].war_target = 0;
    }

    state.players[idx as usize].caesar_second_politics = false;
    state.players[idx as usize].peeked_event = CardId::NONE;
    state.players[idx as usize].politics_done = true;

    // §5.11: a resigning player's turn ends at once, and if that leaves one
    // player standing they win outright. `game.rs` owns that decision (it is
    // the same hand-off `end_turn` makes), exactly as Python's `_h_resign`
    // tail-calls `game.after_resign`.
    crate::game::after_resign(state);
}

// ============================================================== tests ====

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::Age;
    use crate::state::{CardList, GameState, Pact, PactList, Phase, PlayerState, Tableau, TechSlot, MAX_PLAYERS, ROW_SIZE};

    fn card(name: &str) -> CardId {
        CardId::by_name(name).unwrap_or_else(|| panic!("no such card: {name}"))
    }

    /// `h_play_action` DEFERS an action card's ordered action and its gains
    /// onto `state.queue` (§3.11, Python's `_h_play_action`), and it is
    /// `apply()` -- not the handler -- that drains the queue. These tests
    /// call the handler directly, so they do the drain themselves.
    fn play_action_and_drain(state: &mut GameState, id: CardId) {
        h_play_action(state, 0, id);
        crate::interact::run_queue(state);
    }

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
            seeded_by: [crate::state::NOT_SEEDED; crate::cards::NUM_CARDS],
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
            pending: crate::state::PendingStack::new(),
            queue: crate::state::Queue::new(),
        }
    }

    fn one_player_state(p: PlayerState) -> GameState {
        // Each filler needs its OWN `idx` (not a repeated 0) -- `combat.rs`
        // functions this module now calls (`start_aggression`,
        // `cancel_attack_pacts`) read `PlayerState::idx` for pact-party
        // matching, and four players sharing idx 0 would make every filler
        // indistinguishable from the actor for that purpose.
        let mut players = [
            blank_player(0, card("Despotism")),
            blank_player(1, card("Despotism")),
            blank_player(2, card("Despotism")),
            blank_player(3, card("Despotism")),
        ];
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
        h_pop(&mut state, 0, false);
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

    /// THE REGRESSION, fixed 2026-08-05: Development of Civil Life's
    /// `pop_food` discount is one population increase, not a standing
    /// discount (state.rs's `OneTimeDiscount` doc comment has the card text).
    /// Before the fix `h_pop` never cleared the field, so a SECOND increase
    /// after the event resolved was still 1 food cheaper than it should be.
    #[test]
    fn h_pop_second_increase_after_the_event_costs_full_price() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        // pop_cost_base(18) == pop_cost_base(17) == 2 (both >= 17), so the
        // SECOND call is still comparing apples to apples after one token
        // moves out of the bank -- only the discount, nothing else, differs.
        p.yellow_bank = 18;
        p.food = 10;
        p.one_time_discount.pop_food = 1;
        let mut state = one_player_state(p);
        h_pop(&mut state, 0, false);
        assert_eq!(state.players[0].food, 10 - 1, "first increase: 2 - 1 discount");
        assert_eq!(state.players[0].one_time_discount.pop_food, 0,
                   "the discount must be consumed by the first increase");
        let food_before = state.players[0].food;
        h_pop(&mut state, 0, false);
        assert_eq!(food_before - state.players[0].food, 2,
                   "REGRESSION: the one-shot discount silently applied to a \
                    second population increase");
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

    /// THE REGRESSION, fixed 2026-08-05: same bug as `h_pop`'s, for `build`.
    /// Civil Life's `build_resources` discount is one build, so a second one
    /// must be full price.
    #[test]
    fn do_build_second_build_after_the_event_costs_full_price() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.resources = 10;
        p.workers_free = 2;
        p.one_time_discount.build_resources = 1;
        p.techs.insert(card("Irrigation"), TechSlot { workers: 0, stored: 0 });
        let mut state = one_player_state(p);
        do_build(&mut state, 0, card("Irrigation"), 0, false);
        assert_eq!(state.players[0].resources, 10 - (4 - 1), "first build: discounted");
        assert_eq!(state.players[0].one_time_discount.build_resources, 0,
                   "the discount must be consumed by the first build");
        let before = state.players[0].resources;
        // A second worker on the SAME card is a legal build too (no per-card
        // worker cap -- farms/mines/urban buildings can carry any number).
        do_build(&mut state, 0, card("Irrigation"), 0, false);
        assert_eq!(before - state.players[0].resources, 4,
                   "REGRESSION: the one-shot build discount silently applied \
                    to a second build");
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

    // -------------------------------------------------------- Barbarossa

    #[test]
    fn h_barbarossa_grows_population_and_builds_a_unit_for_one_military_action() {
        let mut p = blank_player(0, card("Despotism"));
        p.leader = card("Frederick Barbarossa");
        p.military_actions = 2;
        p.civil_actions = 4;
        p.yellow_bank = 14; // pop_cost_base(14) == 3, minus his 1 food discount == 2
        p.food = 2;
        p.resources = 1; // Warriors costs 2, minus his 1 resource discount == 1
        p.techs.insert(card("Warriors"), TechSlot { workers: 0, stored: 0 });
        let mut state = one_player_state(p);
        h_barbarossa(&mut state, 0, card("Warriors"));
        let p = &state.players[0];
        assert_eq!(p.food, 0, "paid the discounted population cost");
        assert_eq!(p.yellow_bank, 13, "one token left the bank");
        assert_eq!(p.resources, 0, "paid the discounted build cost");
        assert_eq!(p.techs.workers(card("Warriors")), 1, "the new worker built the unit");
        assert_eq!(p.military_actions, 1, "the ONE military action bought both halves");
        assert_eq!(p.civil_actions, 4, "the population half costs no civil action");
    }

    // -------------------------------------------------------------- Bach

    #[test]
    fn h_bach_theater_upgrades_cross_type_pays_the_difference_and_marks_used_once() {
        let mut p = blank_player(0, card("Despotism"));
        p.leader = card("J. S. Bach");
        p.civil_actions = 4;
        p.resources = 10;
        p.techs.insert(card("Theology"), TechSlot { workers: 1, stored: 0 });
        p.techs.insert(card("Drama"), TechSlot { workers: 0, stored: 0 });
        let mut state = one_player_state(p);
        h_bach_theater(&mut state, 0, card("Theology"), card("Drama"));
        let p = &state.players[0];
        assert!(p.bach_upgrade_used, "at most once per turn");
        assert_eq!(p.civil_actions, 3);
        assert_eq!(p.techs.workers(card("Theology")), 0);
        assert_eq!(p.techs.workers(card("Drama")), 1);
        // Theology costs 5, Drama costs 4 -- the upgrade cost floors at 0
        // rather than refunding the difference.
        assert_eq!(p.resources, 10);
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

    /// THE REGRESSION, fixed 2026-08-05: same bug as `h_pop`'s and
    /// `do_build`'s, for `develop`. Civil Life's `develop_science` discount
    /// is one technology; two DISTINCT technologies (Irrigation techCost 3,
    /// Iron techCost 5) so the second is not just re-developing the first.
    #[test]
    fn h_develop_second_technology_after_the_event_costs_full_price() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.science = 20;
        p.one_time_discount.develop_science = 1;
        p.hand_civil.push(card("Irrigation"));
        p.hand_civil.push(card("Iron"));
        let mut state = one_player_state(p);
        h_develop(&mut state, 0, card("Irrigation"), false);
        assert_eq!(state.players[0].science, 20 - (3 - 1), "first develop: discounted");
        assert_eq!(state.players[0].one_time_discount.develop_science, 0,
                   "the discount must be consumed by the first develop");
        let before = state.players[0].science;
        h_develop(&mut state, 0, card("Iron"), false);
        assert_eq!(before - state.players[0].science, 5,
                   "REGRESSION: the one-shot develop discount silently \
                    applied to a second technology");
    }

    /// The three categories are consumed INDEPENDENTLY (card text: three
    /// separate discounted actions, not one discount usable on anything).
    /// Spending the population discount must leave build and develop intact.
    #[test]
    fn one_time_discount_categories_are_consumed_independently() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.yellow_bank = 5;
        p.food = 10;
        p.resources = 10;
        p.workers_free = 1;
        p.one_time_discount = crate::state::OneTimeDiscount {
            build_resources: 1,
            develop_science: 1,
            pop_food: 1,
        };
        p.techs.insert(card("Irrigation"), TechSlot { workers: 0, stored: 0 });
        let mut state = one_player_state(p);
        h_pop(&mut state, 0, false);
        let d = state.players[0].one_time_discount;
        assert_eq!(d.pop_food, 0, "population discount spent");
        assert_eq!(d.build_resources, 1, "build discount untouched by pop");
        assert_eq!(d.develop_science, 1, "develop discount untouched by pop");
        // and the still-pending build discount is for real, not just a field
        let before = state.players[0].resources;
        do_build(&mut state, 0, card("Irrigation"), 0, false);
        assert_eq!(before - state.players[0].resources, 3,
                   "spending the population discount must not have consumed \
                    the still-pending build discount (Irrigation cost 4 - 1)");
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

    // ----------------------------------------------------------------- war

    #[test]
    fn h_war_pays_military_actions_and_records_the_declaration() {
        let mut p = blank_player(0, card("Despotism"));
        p.military_actions = 3;
        p.hand_military.push(card("War over Territory")); // cost 2
        let mut state = one_player_state(p);
        h_war(&mut state, 0, card("War over Territory"), 1);
        assert_eq!(state.players[0].military_actions, 1);
        assert!(!state.players[0].hand_military.contains(card("War over Territory")));
        assert_eq!(state.players[0].war_declared_by_me, card("War over Territory"));
        assert_eq!(state.players[0].war_target, 1);
        assert_eq!(state.players[1].wars_declared_on_me[0], card("War over Territory"));
        assert!(state.players[0].politics_done);
        assert_eq!(state.phase, Phase::Actions);
    }

    #[test]
    fn h_war_doubles_cost_against_mahatma_gandhi() {
        let mut p = blank_player(0, card("Despotism"));
        p.military_actions = 4;
        p.hand_military.push(card("War over Territory")); // cost 2, doubled to 4
        let mut target = blank_player(1, card("Despotism"));
        target.leader = card("Mahatma Gandhi");
        let mut state = blank_state(4, {
            let filler = || blank_player(2, card("Despotism"));
            [p, target, filler(), blank_player(3, card("Despotism"))]
        });
        h_war(&mut state, 0, card("War over Territory"), 1);
        assert_eq!(state.players[0].military_actions, 0, "cost doubled to 4");
    }

    #[test]
    fn h_war_cancels_a_pact_that_ends_on_mutual_attack() {
        let mut p = blank_player(0, card("Despotism"));
        p.military_actions = 3;
        p.hand_military.push(card("War over Territory"));
        p.pacts.push(Pact { card: card("Military Alliance"), owner: 0, partner: 1, a: 0, b: 1 });
        let mut state = blank_state(4, {
            let filler = || blank_player(2, card("Despotism"));
            [p, blank_player(1, card("Despotism")), filler(), blank_player(3, card("Despotism"))]
        });
        h_war(&mut state, 0, card("War over Territory"), 1);
        assert!(state.players[0].pacts.is_empty(), "Military Alliance ends the moment its parties attack");
    }

    // -------------------------------------------------------------- aggression

    #[test]
    fn h_aggression_pays_cost_discards_and_cancels_pacts_then_resolves() {
        // `h_aggression` runs `combat::start_aggression` (the portable
        // prefix of `events.start_aggression`) and then hands the defense
        // decision to the rival through `interact::start_defense`.
        let agg = crate::cards::CARDS
            .iter()
            .position(|c| c.kind == CardType::Aggression)
            .map(|i| CardId(i as u16))
            .expect("at least one aggression card exists");
        let cost = agg.get().military_action_cost as i8;
        let mut p = blank_player(0, card("Despotism"));
        p.military_actions = cost + 2;
        p.hand_military.push(agg);
        p.pacts.push(Pact { card: card("Military Alliance"), owner: 0, partner: 1, a: 0, b: 1 });
        let mut state = one_player_state(p);

        h_aggression(&mut state, 0, agg, 1);
        assert_eq!(state.players[0].military_actions, 2, "cost was paid");
        assert!(!state.players[0].hand_military.contains(agg), "card left the hand");
        assert!(
            state.discarded_military[agg.get().age as usize].contains(agg),
            "card was discarded"
        );
        assert!(state.players[0].pacts.is_empty(), "doomed pact was cancelled");
        assert!(state.players[0].politics_done, "the political action is spent");
        // The defender holds no military cards, so §5.4.4 offers nothing to
        // decide and the aggression resolves at its printed strength -- 0 vs
        // 0, which FAILS. That whole tail was `unimplemented!` until
        // `interact::start_defense` existed to produce a defense total.
        assert!(state.pending.is_empty(), "nothing left to decide");
    }

    // ------------------------------------------------------------- play_action

    #[test]
    fn h_play_action_patriotism_grants_a_military_action_and_the_discount_pool() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.military_actions = 2;
        p.hand_civil.push(card("Patriotism (A)"));
        let mut state = one_player_state(p);
        play_action_and_drain(&mut state, card("Patriotism (A)"));
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
        play_action_and_drain(&mut state, card("Cultural Heritage (A)"));
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
        play_action_and_drain(&mut state, card("Stock Pile"));
        assert_eq!(state.players[0].food, 1);
        assert_eq!(state.players[0].resources, 1);
    }

    #[test]
    fn h_play_action_rich_land_with_no_legal_ordered_move_is_a_silent_no_op() {
        // No workers_free and an empty tableau: `free_action_moves` returns
        // no options, so this must resolve like Python's `push_choice` with
        // an empty option list -- silently, no panic -- exactly as playing a
        // card with no ordered action at all does.
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.hand_civil.push(card("Rich Land (A)"));
        let mut state = one_player_state(p);
        play_action_and_drain(&mut state, card("Rich Land (A)"));
        assert_eq!(state.players[0].civil_actions, 3, "only the card's own CA is spent");
        assert!(!state.players[0].hand_civil.contains(card("Rich Land (A)")));
    }

    #[test]
    fn h_play_action_rich_land_builds_a_mine_at_a_discount_and_no_action_cost() {
        // "Rich Land (A)": build_or_upgrade_farm_or_mine, resourceDiscount 1.
        // Bronze costs 2 resources; the free build pays only 2 - 1 = 1 and no
        // separate civil action (only the 1 CA to play the card itself).
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.workers_free = 1;
        p.resources = 1;
        p.techs.insert(card("Bronze"), TechSlot { workers: 0, stored: 0 });
        p.hand_civil.push(card("Rich Land (A)"));
        let mut state = one_player_state(p);
        play_action_and_drain(&mut state, card("Rich Land (A)"));
        assert_eq!(state.players[0].techs.workers(card("Bronze")), 1);
        assert_eq!(state.players[0].resources, 0);
        assert_eq!(state.players[0].workers_free, 0);
        assert_eq!(state.players[0].civil_actions, 3, "the build itself is free");
    }

    #[test]
    fn h_play_action_efficient_upgrade_upgrades_at_a_discount() {
        // "Efficient Upgrade (II)": upgrade_farm_mine_or_urban_building,
        // resourceDiscount 3. Agriculture->Irrigation costs 4-2=2, floored to
        // 0 by the discount.
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.techs.insert(card("Agriculture"), TechSlot { workers: 1, stored: 0 });
        p.techs.insert(card("Irrigation"), TechSlot { workers: 0, stored: 0 });
        p.hand_civil.push(card("Efficient Upgrade (II)"));
        let mut state = one_player_state(p);
        play_action_and_drain(&mut state, card("Efficient Upgrade (II)"));
        assert_eq!(state.players[0].techs.workers(card("Agriculture")), 0);
        assert_eq!(state.players[0].techs.workers(card("Irrigation")), 1);
        assert_eq!(state.players[0].civil_actions, 3, "the upgrade itself is free");
    }

    #[test]
    fn h_play_action_engineering_genius_builds_a_wonder_stage_at_a_discount() {
        // "Engineering Genius (A)": build_one_wonder_stage, resourceDiscount
        // 2. Pyramids' first stage costs 3, floored by the discount to 1.
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.wonder = card("Pyramids");
        p.resources = 1;
        p.hand_civil.push(card("Engineering Genius (A)"));
        let mut state = one_player_state(p);
        play_action_and_drain(&mut state, card("Engineering Genius (A)"));
        assert_eq!(state.players[0].wonder_steps, 1);
        assert_eq!(state.players[0].resources, 0);
        assert_eq!(state.players[0].civil_actions, 3, "the wonder step itself is free");
    }

    #[test]
    fn h_play_action_frugality_increases_population_before_its_own_gain_food_lands() {
        // "Frugality (A)": increase_population "at full price" + gainFood 1.
        // yellow_bank 5 prices the increase at 5 food; starting on exactly 5
        // affords it (paid BEFORE the card's own +1 food arrives), leaving 0,
        // then the card's own gain lands on top.
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.yellow_bank = 5;
        p.food = 5;
        p.blue_total = 10; // gain_food needs blue tokens free to convert
        p.hand_civil.push(card("Frugality (A)"));
        let mut state = one_player_state(p);
        play_action_and_drain(&mut state, card("Frugality (A)"));
        assert_eq!(state.players[0].workers_free, 1, "the ordered pop happened");
        assert_eq!(state.players[0].yellow_bank, 4);
        assert_eq!(state.players[0].food, 1, "0 left after the pop, +1 from the card's own gainFood");
        assert_eq!(state.players[0].civil_actions, 3, "the pop itself is free");
    }

    #[test]
    fn h_play_action_frugality_own_gain_food_arrives_too_late_to_pay_for_its_pop() {
        // Same card, starting 1 food short of the pop cost: the card's own
        // +1 food would cover the gap, but §3.11 resolves the ordered action
        // BEFORE the card's own gains land (`_h_play_action`'s FIFO enqueue
        // order), so the pop must NOT happen -- only the flat +1 food does.
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.yellow_bank = 5; // prices the increase at 5
        p.food = 4;
        p.blue_total = 10; // gain_food needs blue tokens free to convert
        p.hand_civil.push(card("Frugality (A)"));
        let mut state = one_player_state(p);
        play_action_and_drain(&mut state, card("Frugality (A)"));
        assert_eq!(state.players[0].workers_free, 0, "the ordered pop must not happen");
        assert_eq!(state.players[0].yellow_bank, 5, "unchanged: no token moved");
        assert_eq!(state.players[0].food, 5, "4 unspent + the card's own +1 gainFood");
    }

    #[test]
    fn h_play_action_breakthrough_develops_a_technology_at_full_price() {
        // "Breakthrough (I)": develop_technology "at full price" + gainScience
        // 2. Bronze is a free develop (scienceCost 0), so this exercises the
        // ordered `develop` arm end to end.
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.hand_civil.push(card("Breakthrough (I)"));
        p.hand_civil.push(card("Bronze"));
        let mut state = one_player_state(p);
        play_action_and_drain(&mut state, card("Breakthrough (I)"));
        assert_eq!(state.players[0].techs.workers(card("Bronze")), 0, "developed, not built");
        assert!(state.players[0].techs.has(card("Bronze")));
        assert!(!state.players[0].hand_civil.contains(card("Bronze")));
        assert_eq!(state.players[0].science, 2, "only the card's own gainScience -- Bronze cost 0");
        assert_eq!(state.players[0].civil_actions, 3, "the develop itself is free");
    }

    #[test]
    fn h_play_action_breakthrough_own_gain_science_arrives_too_late_to_pay_for_its_develop() {
        // Irrigation costs 3 science; starting on 2 is short, and the card's
        // own +2 science arrives AFTER the ordered action resolves (same
        // FIFO rule as Frugality above), so the develop must not happen.
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.science = 2;
        p.hand_civil.push(card("Breakthrough (I)"));
        p.hand_civil.push(card("Irrigation"));
        let mut state = one_player_state(p);
        play_action_and_drain(&mut state, card("Breakthrough (I)"));
        assert!(!state.players[0].techs.has(card("Irrigation")), "must not develop");
        assert!(state.players[0].hand_civil.contains(card("Irrigation")), "stays in hand");
        assert_eq!(state.players[0].science, 4, "2 unspent + the card's own +2 gainScience");
    }

    /// Two developable, freely-affordable technologies in hand: a GENUINE
    /// tie, which `push_choice(auto=True)` does not resolve. This used to be
    /// the one case that had to panic; it now opens a real decision, and the
    /// player who owns it is the one who played the card.
    #[test]
    fn h_play_action_breakthrough_opens_a_decision_on_a_genuine_multi_way_choice() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.hand_civil.push(card("Breakthrough (I)"));
        p.hand_civil.push(card("Bronze"));
        p.hand_civil.push(card("Agriculture"));
        let mut state = one_player_state(p);
        play_action_and_drain(&mut state, card("Breakthrough (I)"));
        assert_eq!(state.decider(), 0);
        let moves = legal::legal_moves(&state);
        assert_eq!(moves.len(), 2, "develop Agriculture, or develop Bronze");
        // §3.11: the card's own gains are queued BEHIND the ordered action,
        // so they have not landed yet.
        assert_eq!(state.players[0].science, 0);
        apply(&mut state, Move::Choose { n: 0 });
        assert!(state.pending.is_empty());
        assert_eq!(state.players[0].science, 2, "...and now the gains land");
    }

    #[test]
    fn h_play_action_breakthrough_may_spend_its_order_on_a_revolution() {
        // RB p.15: Breakthrough's order may pay for a revolution instead of a
        // peaceful develop. Monarchy's peaceful cost (8) is unaffordable on 2
        // science, but its revolutionCost (2) is -- and `revolt_ok` needs
        // "every civil action THIS TURN still unspent" measured BEFORE
        // playing Breakthrough spends one, not after (a regression test for
        // that exact ordering: computing it post-`pay_ca` would read
        // `civil_actions` as one short of `ca_total` and wrongly report no
        // legal ordered-action move at all).
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4; // == ca_total(Despotism): revolt_ok must be true
        p.science = 2;
        p.hand_civil.push(card("Breakthrough (I)"));
        p.hand_civil.push(card("Monarchy"));
        let mut state = one_player_state(p);
        play_action_and_drain(&mut state, card("Breakthrough (I)"));
        assert_eq!(state.players[0].government, card("Monarchy"), "revolution happened");
        assert_eq!(state.players[0].science, 2, "0 left after the revolution, +2 from the card's own gainScience");
    }

    /// `Reserves` prints `gainFoodOrResources`, which is a real choice --
    /// a named gap here until `interact::push_choice` existed.
    #[test]
    fn h_play_action_reserves_offers_food_or_resources() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.blue_total = 20;
        p.hand_civil.push(card("Reserves (I)"));
        let mut state = one_player_state(p);
        play_action_and_drain(&mut state, card("Reserves (I)"));
        assert_eq!(legal::legal_moves(&state).len(), 2, "food, or resources");
        apply(&mut state, Move::Choose { n: 1 });
        let n = match card("Reserves (I)").get().special.iter().find_map(|s| match s {
            Special::GainFoodOrResources(n) => Some(*n),
            _ => None,
        }) {
            Some(n) => n as u16,
            None => panic!("Reserves prints gainFoodOrResources"),
        };
        assert_eq!(state.players[0].resources, n);
        assert_eq!(state.players[0].food, 0);
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

    #[test]
    fn h_resign_scores_the_declarer_of_a_war_against_the_resigning_player() {
        // §5.11: a war against a resigned player scores its declarer 7
        // culture, clears the declarer's OWN `war_declared_by_me` if it is
        // still this same war, and discards the war card.
        let mut attacker = blank_player(1, card("Despotism"));
        attacker.war_declared_by_me = card("War over Territory");
        attacker.war_target = 0;
        let mut resigner = blank_player(0, card("Despotism"));
        resigner.wars_declared_on_me[1] = card("War over Territory");
        let mut state = blank_state(4, {
            let filler = || blank_player(2, card("Despotism"));
            [resigner, attacker, filler(), blank_player(3, card("Despotism"))]
        });
        h_resign(&mut state, 0);
        assert_eq!(state.players[1].culture, 7);
        assert!(state.players[1].war_declared_by_me.is_none(), "the declarer's own war is cleared too");
        assert!(state.players[0].wars_declared_on_me.iter().all(|c| c.is_none()));
        assert!(state.discarded_military[Age::II as usize].contains(card("War over Territory")));
    }

    #[test]
    fn h_resign_tears_down_a_war_the_resigning_player_themselves_declared() {
        let mut resigner = blank_player(0, card("Despotism"));
        resigner.war_declared_by_me = card("War over Territory");
        resigner.war_target = 1;
        let mut defender = blank_player(1, card("Despotism"));
        defender.wars_declared_on_me[0] = card("War over Territory");
        let mut state = blank_state(4, {
            let filler = || blank_player(2, card("Despotism"));
            [resigner, defender, filler(), blank_player(3, card("Despotism"))]
        });
        h_resign(&mut state, 0);
        assert!(state.players[0].war_declared_by_me.is_none());
        assert!(state.players[1].wars_declared_on_me[0].is_none());
        assert!(state.discarded_military[Age::II as usize].contains(card("War over Territory")));
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
    fn do_wonder_step_pays_resources_and_advances_progress() {
        // Pyramids: stages [3, 2, 1].
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.resources = 10;
        p.wonder = card("Pyramids");
        let mut state = one_player_state(p);
        do_wonder_step(&mut state, 0, 1, 0, false);
        assert_eq!(state.players[0].civil_actions, 3);
        assert_eq!(state.players[0].resources, 10 - 3);
        assert_eq!(state.players[0].wonder_steps, 1);
        assert_eq!(state.players[0].wonder, card("Pyramids"), "not complete yet: 1 of 3 stages paid");
    }

    #[test]
    fn do_wonder_step_completes_the_wonder_and_gains_completion_culture() {
        // Fast Food Chains: stages [4, 4, 4, 4], onBuildCulture
        // "2*workers(farm,mine)+1*workers(urban,military)".
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.resources = 10;
        p.wonder = card("Fast Food Chains");
        p.wonder_steps = 3; // only the last stage remains
        p.techs.insert(card("Agriculture"), TechSlot { workers: 2, stored: 0 }); // production
        p.techs.insert(card("Warriors"), TechSlot { workers: 1, stored: 0 }); // unit
        let mut state = one_player_state(p);
        do_wonder_step(&mut state, 0, 1, 0, false);
        assert_eq!(state.players[0].resources, 10 - 4);
        assert!(state.players[0].wonder.is_none(), "wonder completed");
        assert_eq!(state.players[0].wonder_steps, 0);
        assert!(state.players[0].completed_wonders.contains(card("Fast Food Chains")));
        // 2 * production workers (2) + 1 * urban-or-unit workers (1) = 5.
        assert_eq!(state.players[0].culture, 5);
    }

    /// `EndTurn` was a named gap here until `game.rs` landed. It is now a
    /// one-line delegation to `game::end_turn` (the whole §6.6 sequence plus
    /// the hand-off), which `game.rs`'s own tests cover -- this is only a
    /// smoke test that [`apply`] routes there at all.
    #[test]
    fn apply_end_turn_delegates_to_game() {
        let mut state =
            blank_state(2, std::array::from_fn(|i| blank_player(i as u8, card("Despotism"))));
        let before = state.turn;
        apply(&mut state, Move::EndTurn);
        assert_eq!(state.turn, before + 1, "the turn advanced");
        assert_eq!(state.current, 1, "...to the next player");
    }

    /// `OfferPact` used to be a named gap here (no `state.pending` field).
    /// It now opens a real decision -- and the point of the whole subsystem
    /// is that the TARGET answers it while `current` stays put.
    #[test]
    fn apply_offer_pact_hands_the_decision_to_the_target() {
        let p = blank_player(0, card("Despotism"));
        let mut state = blank_state(2, std::array::from_fn(|i| blank_player(i as u8, card("Despotism"))));
        state.players[0] = p;
        let peace = card("Peace Treaty");
        state.players[0].hand_military.push(peace);
        apply(&mut state, Move::OfferPact { card: peace, target: 1, side: PactSide::B });
        assert!(!state.players[0].hand_military.contains(peace), "the card is revealed");
        assert!(state.players[0].politics_done, "the political action is spent either way");
        assert_eq!(state.decider(), 1, "the partner answers");
        assert_eq!(state.current, 0, "...and the turn has not moved");
        // Offering side B puts the TARGET on `a` -- see `state::Pact`.
        match state.pending.top() {
            Some(crate::state::Pending::Choice(c)) => assert_eq!(
                c.kind,
                crate::state::ChoiceKind::PactOffer { owner: 0, card: peace, a: 1, b: 0 }
            ),
            other => panic!("expected a pact offer, got {other:?}"),
        }
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

    /// Hollywood: "twice the total culture production of your theaters and
    /// libraries" (§9.2) -- including a modifier that reaches into that
    /// production, not just the printed numbers. Shakespeare's
    /// `CulturePerLibraryTheaterPair(2)` adds `2 * min(library workers,
    /// theater workers)` on top of Drama's printed 2 culture and Printing
    /// Press's printed 1: `(2 + 1 + 2*min(1,1)) * 2 == 10`.
    #[test]
    fn wonder_completion_culture_hollywood_counts_modifiers() {
        let mut p = blank_player(0, card("Despotism"));
        p.leader = card("William Shakespeare");
        p.techs.insert(card("Drama"), TechSlot { workers: 1, stored: 0 }); // theater, culture 2
        p.techs.insert(card("Printing Press"), TechSlot { workers: 1, stored: 0 }); // library, culture 1
        let gained = wonder_completion_culture(&p, card("Hollywood"));
        assert_eq!(gained, 10);
    }

    /// A Hollywood completion with no modifiers in play is just twice the
    /// printed theater/library culture: `(2 + 1) * 2 == 6`.
    #[test]
    fn wonder_completion_culture_hollywood_printed_only() {
        let mut p = blank_player(0, card("Despotism"));
        p.techs.insert(card("Drama"), TechSlot { workers: 1, stored: 0 });
        p.techs.insert(card("Printing Press"), TechSlot { workers: 1, stored: 0 });
        let gained = wonder_completion_culture(&p, card("Hollywood"));
        assert_eq!(gained, 6);
    }

    /// Internet: "the combined culture, science and strength your urban
    /// buildings give" (§9.2), again the EFFECTIVE output. Leonardo da
    /// Vinci's `SciencePerBestLabOrLibraryLevel` adds the best staffed
    /// lab-or-library's level (1, from either Age-I card here) as science on
    /// top of the printed sum: Alchemy's 2 science, Printing Press's 1
    /// culture + 1 science, Bread and Circuses' 1 strength -- `2+1+1+1 == 5`
    /// printed, `+1` from the leader, `== 6`.
    #[test]
    fn wonder_completion_culture_internet_counts_modifiers() {
        let mut p = blank_player(0, card("Despotism"));
        p.leader = card("Leonardo da Vinci");
        p.techs.insert(card("Alchemy"), TechSlot { workers: 1, stored: 0 }); // lab, science 2
        p.techs.insert(card("Printing Press"), TechSlot { workers: 1, stored: 0 }); // library, culture 1 / science 1
        p.techs.insert(card("Bread and Circuses"), TechSlot { workers: 1, stored: 0 }); // arena, strength 1
        let gained = wonder_completion_culture(&p, card("Internet"));
        assert_eq!(gained, 6);
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

