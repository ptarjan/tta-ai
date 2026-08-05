//! Event-gain application (§5.3/§5.4.6). Ports exactly `engine/events.py`'s
//! "gain blocks" section (that file's own header comment, lines 41-125):
//! `apply_gains` and its one directly-needed helper, `_food_or_resources`
//! (this module's [`food_or_resources`]).
//!
//! ## Scope: what this module is, and is not
//!
//! `apply_gains` is the shared interpreter Python uses BOTH to award an
//! event's effect block to a player (`_apply_player_block`, from
//! `resolve_event`) AND to pay an aggression's success gains to the attacker
//! (`finish_aggression`). Only the second caller is wired up by this pass --
//! `combat.rs::finish_aggression`'s SUCCESS branch, which used to
//! `unimplemented!` naming this module. Event RESOLUTION itself
//! (`resolve_event`, `_apply_player_block`, `_conditional_target`,
//! `_apply_extras`, `scoring_culture`, `pending_final_events`,
//! `final_event_awards`, `evaluate_final_events`, ...) is a separate, larger
//! job and is deliberately not touched here -- see "keys this module does
//! not implement" below for exactly where that leaves `apply_gains` short of
//! Python's version, and why that shortfall is inert today.
//!
//! `_draw_military` (`engine/events.py:117-124`) is NOT ported: nothing in
//! Python's `apply_gains` can reach it except through a `drawMilitaryCards`
//! key, and -- see below -- no card this port can construct a block from
//! ever carries one. Porting a function apply_gains cannot currently call
//! would be exactly the untested-dead-code shape this project's structural
//! guarantees exist to prevent (a compile-time-exhaustive `match` is only a
//! guarantee if every arm is reachable).
//!
//! ## What `apply_gains` operates on
//!
//! Python's `apply_gains(state, p, block, rng, sign)` takes an arbitrary
//! dict -- sometimes a whole card's `effects`, sometimes a nested sub-dict an
//! event prints under `allPlayers`/`weakestPlayer`/etc (§5.3). Every key
//! inside one of those NESTED sub-dicts is opaque to this port today:
//! `gen_cards.py`'s `DEFERRED_DICT_EFFECT_KEYS` still collapses `allPlayers`
//! and its seven siblings to a payload-less `Special` ("event targeting --
//! events.rs not ported"), so there is no Rust value yet that could stand in
//! for one of those nested dicts.
//!
//! The one shape IS ported: a CARD's own top-level `effects` dict, which
//! `gen_cards.py` already decodes unconditionally for every card (via
//! `CardEffects`'s recurring fields, plus whichever one-off `Special`
//! variants that card's other keys produced) regardless of whether
//! resolution is wired up. That is exactly what `combat.rs::
//! finish_aggression` passes today (Python: `apply_gains(state, attacker,
//! eff, rng)` where `eff = db.get(name).get("effects")`, `name` being the
//! AGGRESSION card itself). So [`apply_gains`] here takes a `CardId` and
//! reads `card.get().effects` / `card.get().special` directly, rather than a
//! generic block value -- there being no other value it is ever asked to
//! operate on by this port's one caller.
//!
//! Reusing `CardEffects` (rather than a dedicated struct, the way
//! `PactBlock` exists precisely to avoid overloading `CardEffects`) is safe
//! here for a structural reason, not a coincidental one: `effects::compute`
//! only ever reads a `CardEffects` off a `CardId` sitting in one of a
//! player's SLOTS (`p.techs`/`wonder`/`tactic`/`government`/`leader`).
//! Event/aggression/war cards are never placed in any of those slots -- they
//! resolve and are discarded -- so `compute` structurally cannot ever read
//! the very fields [`apply_gains`] is about to interpret as one-shot gains.
//! (Contrast a hypothetical territory card: its `CardEffects.food`/
//! `resources` ARE read recurringly, once colonies enter play, via
//! `permanentEffects` -- which is exactly why `apply_gains` is never called
//! with a territory `CardId` by anything in this port; colonization is its
//! own unported area.)
//!
//! ## Keys this module does not implement, and why
//!
//! FLAGGED, not routed around (this project's standing rule: reproduce a
//! real gap faithfully and say so, rather than silently drop it). Eight of
//! Python's `apply_gains` key branches have no reachable path through this
//! port's one call site, verified against a full top-level `effects`-key
//! census over all 236 base-2015 cards (2026-08-05):
//!
//!   * `loseScience`, `loseCulture`, `population`/`gainPopulation`,
//!     `increasePopulation`, a BARE (top-level) `yellowTokens`, a BARE
//!     `loseAllStoredFood`, a BARE `foodAndOrResources` -- printed by ZERO
//!     cards anywhere in the base game's data, at ANY nesting depth.
//!     `gen_cards.py` only emits a `Special` variant for a key it has
//!     actually seen (`card_table.rs`'s own doc comment: "one variant per
//!     distinct one-off effect key"), so there is no variant for any of
//!     these six and no card's `special` slice could ever carry one. A match
//!     arm against a variant that cannot exist would be dead code nothing
//!     exercises -- the opposite of "a card whose rule the engine cannot
//!     interpret is a compile error". If a future data revision (or the
//!     expansion, out of scope by standing decision) ever prints one,
//!     `gen_cards.py`'s exhaustive key census fails the build and names it,
//!     at which point it gets a real field/variant and a real arm here.
//!   * `drawMilitaryCards` -- printed exactly once (`Development of
//!     Politics`), but NESTED under `allPlayers`, the opaque-dict case
//!     above; the key exists, but no `CardId`-shaped value this module can
//!     read carries it.
//!   * `decreasePopulation`/`losePopulation` -- IS printed top-level
//!     (Barbarians: `Special::DecreasePopulation`), so it IS implemented
//!     below, but note it is never actually reached by Python's own
//!     `resolve_event` either: Barbarians has no `allPlayers` key, so its
//!     `decreasePopulation` is read directly by `_conditional_target`, not
//!     through `apply_gains` -- included here anyway because it is a real,
//!     data-backed branch, exercised by this module's own unit test rather
//!     than by any live caller today.
//!
//! `food`/`resources` (bare, i.e. not `gainFood`/`gainResources`) ARE
//! backed by a field (`CardEffects.food`/`resources`) and so are handled
//! below despite being unreachable by any base-game aggression card either
//! -- unlike the six above, a card COULD print them (territories do, via a
//! different JSON path merged into the same field by `gen_cards.py`; see
//! `cards.rs`'s doc comment on `CardEffects.food`), so there is a real
//! variant to dispatch on, just no aggression/event card that currently
//! does.

use crate::cards::{CardId, Special};
use crate::economy;
use crate::interact;
use crate::state::{GameState, PlayerState, QueueItem};

/// §5.3/§5.4.6: apply one card's own top-level gain effects to player `idx`.
/// Mirrors `engine/events.py::apply_gains` -- see this module's top doc
/// comment for exactly which of Python's key branches are implemented, why
/// the rest are not, and why `card: CardId` stands in for Python's
/// `block: dict`.
///
/// `sign = -1` inverts every gain into a loss (Python's own docstring:
/// "`sign=-1` inverts (lose blocks)"). `combat::finish_aggression` always
/// calls this with `sign = 1` (an aggression's own effects are gains to the
/// ATTACKER, never losses) -- the parameter exists anyway because it is the
/// one thing separating this from a second, near-identical copy of the
/// function the moment a lose-block caller exists, exactly as in Python.
pub fn apply_gains(state: &mut GameState, idx: u8, card: CardId, sign: i32) {
    let eff = card.get().effects;

    // science / gainScience (events.py:47-49) -- both keys apply
    // identically in Python, so both fields are walked the same way. Each
    // is its own statement (not summed first) so a card that somehow
    // printed both would clamp at zero exactly where Python's per-key dict
    // loop would, rather than only after combining them.
    for delta in [eff.science, eff.gain_science] {
        add_clamped(&mut state.players[idx as usize].science, sign * delta as i32);
    }
    // culture / gainCulture (events.py:53-55).
    for delta in [eff.culture, eff.gain_culture] {
        add_clamped(&mut state.players[idx as usize].culture, sign * delta as i32);
    }
    // food / gainFood (events.py:59-64). `produceFood` is the third key
    // spelling Python accepts here, but it is only ever printed as a
    // BOOLEAN flag in the base data (`_num` rejects bools -- see
    // `_apply_extras`, out of this port's scope, which is what actually
    // reads that flag), so it never contributes a magnitude through this
    // path regardless.
    for delta in [eff.food, eff.gain_food] {
        apply_food_delta(state, idx, delta, sign);
    }
    // resources / gainResources (events.py:65-70). Same `produceResources`
    // caveat as food above.
    for delta in [eff.resources, eff.gain_resources] {
        apply_resources_delta(state, idx, delta, sign);
    }
    // blueTokens (events.py:91-93).
    if eff.blue_tokens != 0 {
        let p = &mut state.players[idx as usize];
        p.blue_total = (p.blue_total as i32 + sign * eff.blue_tokens as i32).max(0) as u8;
    }
    // strength (events.py:94-96) -- a ONE-SHOT grant via `strength_extra`,
    // not the recurring per-turn `CardEffects.strength` a card sitting in a
    // player's tableau contributes through `effects::compute`. See this
    // module's top doc comment for why the two meanings cannot collide.
    if eff.strength != 0 {
        state.players[idx as usize].strength_extra += (sign as i16) * eff.strength;
    }
    // happiness / happy (events.py:97-99) -- same one-shot-via-`_extra`
    // reasoning as strength above.
    if eff.happy != 0 {
        state.players[idx as usize].happy_extra += (sign as i16) * eff.happy;
    }
    // decreasePopulation / losePopulation (events.py:82-87) -- §6.5/FAQ
    // p.15: the OWNER chooses which worker to lose, so this only enqueues
    // the decision. Only the `decreasePopulation` spelling is ever printed
    // top-level in the base data (Barbarians); `Special::DecreasePopulation`
    // is the int-shape variant `gen_cards.py` already emits for it.
    for &sp in card.get().special {
        if let Special::DecreasePopulation(n) = sp {
            if n != 0 {
                interact::enqueue(state, QueueItem::LosePop { player: idx, n: n as u8 });
            }
        }
    }
}

/// `p.X = max(0, p.X + delta)` (events.py's repeated `p.science = max(0,
/// p.science + sign * v)` idiom) -- a no-op when `delta` is zero, matching
/// Python's own `if v:` guard on every branch that uses it.
fn add_clamped(field: &mut u16, delta: i32) {
    if delta != 0 {
        *field = (*field as i32 + delta).max(0) as u16;
    }
}

/// One `food`/`gainFood` key application (events.py:59-64): the blue-token-
/// limited [`economy::gain_food`] when gaining, plain floored subtraction
/// when losing -- `sign` chooses which, `p.food` itself is never allowed
/// negative either way.
fn apply_food_delta(state: &mut GameState, idx: u8, delta: i16, sign: i32) {
    if delta == 0 {
        return;
    }
    if sign > 0 {
        economy::gain_food(&mut state.players[idx as usize], delta as u16);
    } else {
        let p = &mut state.players[idx as usize];
        p.food = p.food.saturating_sub(delta as u16);
    }
}

/// The `resources`/`gainResources` twin of [`apply_food_delta`]
/// (events.py:65-70).
fn apply_resources_delta(state: &mut GameState, idx: u8, delta: i16, sign: i32) {
    if delta == 0 {
        return;
    }
    if sign > 0 {
        economy::gain_resources(&mut state.players[idx as usize], delta as u16);
    } else {
        let p = &mut state.players[idx as usize];
        p.resources = p.resources.saturating_sub(delta as u16);
    }
}

/// §5.4.6/§11.5: move `amount` between food and resources, preferring
/// resources both ways -- gaining tops up resources first and spills the
/// remainder into food (blue-token limited, via [`economy::gain_resources`]/
/// [`economy::gain_food`]); losing drains resources first and only then
/// food (unlimited, floored at zero). Mirrors `engine/events.py::
/// _food_or_resources`.
///
/// This is [`apply_gains`]'s OWN helper for its (unreachable, see this
/// module's top doc comment) bare `foodAndOrResources` key -- but it is very
/// much live for real: `combat::finish_aggression`'s `takeFromOpponent.
/// foodAndOrResources` theft (`events.py:656-659`, Aggression: Plunder) calls
/// this exact private function too, so it is `pub(crate)` rather than
/// private, and there is one copy, not two drifting in and out of step.
pub(crate) fn food_or_resources(p: &mut PlayerState, amount: i32, sign: i32) {
    let amount = amount.max(0) as u16;
    if sign > 0 {
        let got = economy::gain_resources(p, amount);
        economy::gain_food(p, amount - got);
    } else {
        let take = p.resources.min(amount);
        p.resources -= take;
        p.food = p.food.saturating_sub(amount - take);
    }
}

// ============================================================== tests ====

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::CardId;
    use crate::state::{
        CardList, GameState, PactList, Phase, PlayerState, Tableau, MAX_PLAYERS, ROW_SIZE,
    };

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

    fn one_player_state(p0: PlayerState) -> GameState {
        let filler = || blank_player(1, card("Despotism"));
        let mut players = [filler(), filler(), filler(), filler()];
        players[0] = p0;
        GameState {
            num_players: 2,
            seed: 0,
            players,
            current: 0,
            turn: 1,
            round: 2,
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

    // ------------------------------------------------------------ apply_gains

    #[test]
    fn apply_gains_enslave_grants_food_and_resources_to_the_attacker() {
        // The one base-game aggression card whose top-level effects apply_gains
        // actually does anything with (see this module's top doc comment):
        // "Aggression: Enslave" prints `{gainFood: 2, gainResources: 2,
        // opponentDecreasesPopulation: 1}` -- the last key is combat.rs's job,
        // not apply_gains's.
        let mut p0 = blank_player(0, card("Despotism"));
        p0.blue_total = 10; // enough blue tokens for both gains to land.
        let mut state = one_player_state(p0);
        apply_gains(&mut state, 0, card("Aggression: Enslave"), 1);
        assert_eq!(state.players[0].food, 2);
        assert_eq!(state.players[0].resources, 2);
    }

    #[test]
    fn apply_gains_is_a_no_op_for_a_takefromopponent_only_card() {
        // "Aggression: Spy" prints only `takeFromOpponent`, which apply_gains
        // does not read at all (that dict is combat.rs's own theft loop).
        let p0 = blank_player(0, card("Despotism"));
        let mut state = one_player_state(p0);
        apply_gains(&mut state, 0, card("Aggression: Spy"), 1);
        let p = &state.players[0];
        assert_eq!((p.food, p.resources, p.science, p.culture), (0, 0, 0, 0));
    }

    #[test]
    fn apply_gains_negative_sign_inverts_and_floors_at_zero() {
        let mut p0 = blank_player(0, card("Despotism"));
        p0.food = 1;
        p0.resources = 1;
        let mut state = one_player_state(p0);
        apply_gains(&mut state, 0, card("Aggression: Enslave"), -1);
        // gainFood/gainResources are both 2; losing floors at 0, not -1.
        assert_eq!(state.players[0].food, 0);
        assert_eq!(state.players[0].resources, 0);
    }

    #[test]
    fn apply_gains_decrease_population_enqueues_lose_pop() {
        // "Barbarians" prints a top-level `decreasePopulation: 1` (never
        // actually reached through apply_gains by Python either -- see this
        // module's top doc comment -- but the branch is real and tested here
        // directly).
        let p0 = blank_player(0, card("Despotism"));
        let mut state = one_player_state(p0);
        apply_gains(&mut state, 0, card("Barbarians"), 1);
        assert_eq!(
            state.queue.pop_front(),
            Some(QueueItem::LosePop { player: 0, n: 1 })
        );
    }

    // ------------------------------------------------------- food_or_resources

    #[test]
    fn food_or_resources_gain_prefers_resources_when_blue_tokens_cover_it() {
        let mut p = blank_player(0, card("Despotism"));
        p.blue_total = 10;
        food_or_resources(&mut p, 5, 1);
        assert_eq!(p.resources, 5);
        assert_eq!(p.food, 0);
    }

    #[test]
    fn food_or_resources_gain_with_no_blue_tokens_grants_nothing() {
        // No blue tokens at all -- `gain_resources`/`gain_food` are each
        // capped by `blue_available`, which with `blue_total == 0` is 0, so
        // NEITHER can gain anything and the whole amount is dropped (both
        // draw from the SAME shared bank, so there is no separate "food
        // allowance" for the remainder to fall back on).
        let mut p = blank_player(0, card("Despotism"));
        food_or_resources(&mut p, 5, 1);
        assert_eq!(p.resources, 0);
        assert_eq!(p.food, 0);
    }

    #[test]
    fn food_or_resources_lose_drains_resources_before_food() {
        let mut p = blank_player(0, card("Despotism"));
        p.resources = 3;
        p.food = 3;
        food_or_resources(&mut p, 5, -1);
        assert_eq!(p.resources, 0);
        assert_eq!(p.food, 1); // 3 resources covers 3 of the 5, food covers 2.
    }

    #[test]
    fn food_or_resources_lose_floors_at_zero() {
        let mut p = blank_player(0, card("Despotism"));
        p.resources = 1;
        p.food = 1;
        food_or_resources(&mut p, 5, -1);
        assert_eq!(p.resources, 0);
        assert_eq!(p.food, 0);
    }
}
