//! The turn loop, age progression and game end -- the port of
//! `engine/game.py` (RULES_SPEC §1, §2, §5.0, §6.6, §12).
//!
//! This module owns the *sequence*: what happens between one player's
//! decision and the next. Everything it sequences lives elsewhere --
//! `legal::legal_moves` says what may be chosen, `apply::apply` performs the
//! choice, `economy::end_of_turn` runs §6.6, `combat` resolves a declared war
//! -- so what is written here is the skeleton and nothing else.
//!
//! ```text
//! Start-of-Turn : replenish the card row -> resolve a war I declared ->
//!                 my exclusive tactic goes public
//! Politics      : at most one political action (skipped in round 1)
//! Actions       : civil + military actions in any order, ends with EndTurn
//! End-of-Turn   : economy::end_of_turn (§6.6), then hand off
//! ```
//!
//! ## Randomness
//!
//! Python threads an optional `rng` through `apply` -> `_h_end_turn` ->
//! `game.end_turn`, and derives one from the state when that argument is
//! `None`: `random.Random(seed * 1000003 + turn * 97 + round)`
//! (`engine/game.py::_rng_for`). `apply::apply` in this port takes no rng
//! argument at all, so the derived path is the only one reachable and
//! [`rng_for`] is it. That is Python's `actions.apply(state, mv)` default and
//! is fully deterministic; the alternative Python path (a caller-supplied
//! persistent stream, which `game.play_game` uses) is unreachable from here
//! and is deliberately NOT reproduced -- a second entry point taking an
//! `&mut PyRandom` would be a second shuffle order for the same game, which
//! is precisely the "two registries that can disagree" bug class DESIGN.md
//! exists to close. One derived stream per `end_turn`, threaded through the
//! whole start-of-turn sequence exactly as Python threads its one object.
//!
//! [`rng_for`] is VERIFIED correct against CPython for that entry point:
//! `random.Random(2 * 1000003 + 2 * 97 + 1).shuffle(db.civil_deck("I", 2))`
//! reproduces this port's Age I deck at `2p_seed2` ply 1 card for card,
//! including the `list.pop()`-off-the-end deal direction. The pre-shuffle
//! list is byte-identical too (`gen_cards.py` walks `data/*.json` in the same
//! order `cards.CardDB._deck` does, and no card overrides its default
//! `deck` field), so both sides shuffle the same list with the same
//! generator. See KNOWN GAP 2 for why the differential fixtures still
//! disagree; it is not this.
//!
//! ## KNOWN GAPS (reported, not routed around)
//!
//! Two gaps this list used to carry are closed and deleted rather than left
//! as dead history: `events.rs` now exists (`apply_gains`, and, 2026-08-05,
//! §12.5.2 final scoring -- [`finish_game`] calls [`events::
//! evaluate_final_events`] unconditionally, no tripwire), and
//! `economy::end_of_turn` already calls [`crate::interact::
//! discard_excess_military`] directly (`economy.rs`'s own doc comment) --
//! both were stale before this pass touched either file.
//!
//! 1. `GameState` has no `final_scores`, `moves_played` or `move_cap_hit`
//!    field, and this module deliberately did NOT add one (`state.rs` is not
//!    this port's file). None is load-bearing: Python's `_finish_game` writes
//!    the same numbers into `p.culture` that it snapshots into
//!    `final_scores`, so [`scores`] reading `p.culture` after the game is over
//!    returns exactly what Python's `scores()` does; `moves_played` /
//!    `move_cap_hit` are `play_game` bookkeeping and are this port's return
//!    value instead (see [`Outcome`]).
//! 2. **The checked-in differential fixtures cannot be matched at an age
//!    transition, and the fault is not in this file.** `tools/
//!    dump_fixtures.py` builds ONE `random.Random(seed ^ 0x5EED)` per game
//!    and passes it to every `actions.apply` call (mirroring
//!    `game.play_game`), so `_rng_for(state, rng)` hands `_advance_age` a
//!    PERSISTENT stream whose position depends on everything drawn from it
//!    earlier in the game. This port derives a fresh stream per `end_turn`
//!    instead (see "Randomness" above), so every age transition in a fixture
//!    comes out as a different permutation of the same multiset -- 14 of
//!    them across the 9 files, and they are the only state divergences left.
//!    Verified both directions on `2p_seed2` ply 1: this port's Age I deck is
//!    exactly `Random(2 * 1000003 + 2 * 97 + 1)`'s shuffle, and the fixture's
//!    is exactly `Random(2 ^ 0x5EED)`'s.
//!
//!    A stateless port cannot follow that stream, and neither can a stateful
//!    one, for two independent reasons:
//!      * `tests/differential.rs` reloads the state from the fixture after
//!        EVERY ply, and the fixtures do not record the generator's position,
//!        so an rng carried in `GameState` would be reset every ply.
//!      * `events._recycle_future_events` also draws from that same stream
//!        (once per `prepare_event` that empties the current-events deck --
//!        5 to 8 times in a typical fixture, before the Age II and Age III
//!        transitions). Reproducing those draws needs `events.rs::
//!        reveal_current_event`/`_recycle_future_events` (§5.2 event
//!        revealing, NOT the §12.5.2 final scoring or §5.3/§5.4.6 gain-block
//!        interpreter that exist in `events.rs` today -- still unported), and
//!        the number of them that have already happened is NOT recoverable
//!        from a state snapshot (a revealed territory goes to an auction
//!        rather than to `past_events`, so nothing counts reveals).
//!
//!    The fix is to regenerate the fixtures through the entry point this port
//!    actually implements -- one derived stream per apply -- which was
//!    measured: 8 of the 9 games regenerated that way replay at
//!    `state_divergences=0`. Two things block doing it in `tools/`:
//!      * `actions.apply(state, mv)` -- the literal rng=None default this
//!        module's "Randomness" section names -- CRASHES. `apply` passes its
//!        `rng` straight to the handlers without the `_rng_for` backfill
//!        `game.start_turn`/`end_turn` do, so the first `prepare_event` dies
//!        in `events._recycle_future_events` on `None.shuffle`. Only
//!        `game.py`'s own entry points are None-safe. That is a Python bug,
//!        not a porting question: `_h_prepare_event` needs the same
//!        `_rng_for` backfill its siblings have.
//!      * the fixtures are stale against today's `engine/`: regenerating
//!        surfaces a `remove_leader_yellow` move tag `moves.rs` has no
//!        variant for, and two `PlayerState` fields (`caesar_second_politics`,
//!        `peeked_event`) that postdate the recording.

use crate::cards::{Age, CardId, CardType, Special, CARDS};
use crate::combat;
use crate::economy;
use crate::events;
use crate::legal;
use crate::moves::{Move, MoveList};
use crate::rng::{shuffle_cards, PyRandom};
use crate::state::{
    CardList, GameState, PactList, Phase, PlayerState, PendingStack, Queue, QueueItem, Tableau,
    TechSlot, MAX_DECK, MAX_PLAYERS, ROW_SIZE,
};

/// §1.4: the five technologies every civilization starts with, and the
/// workers printed on them. Order is load-bearing -- it is the tableau's
/// build order, and `economy::lose_population` takes a worker off the FIRST
/// worker-holding card it walks (see `Tableau::remove`'s doc comment). Python
/// spells it as a dict literal, which iterates in exactly this order.
const START_TECHS: [(&str, u8); 5] = [
    ("Warriors", 1),
    ("Agriculture", 2),
    ("Bronze", 2),
    ("Philosophy", 1),
    ("Religion", 0),
];

/// §2.1: civil cards swept off the left of the row at the start of a turn, by
/// live player count. Python's `SWEEP = {2: 3, 3: 2, 4: 1}`.
#[inline]
fn sweep_count(live: usize) -> usize {
    match live {
        2 => 3,
        3 => 2,
        _ => 1,
    }
}

/// The next age, or `None` past Age IV. Python indexes `C.AGES[level + 1]`
/// and would `IndexError` past the end; every caller here checks first, the
/// same way Python's `_advance_age` opens with `if ended == "IV": return`.
#[inline]
fn next_age(a: Age) -> Option<Age> {
    match a {
        Age::A => Some(Age::I),
        Age::I => Some(Age::II),
        Age::II => Some(Age::III),
        Age::III => Some(Age::IV),
        Age::IV => None,
    }
}

/// `engine/game.py::_rng_for` with `rng=None` -- see this module's
/// "Randomness" note for why that is the only reachable branch.
///
/// Checked arithmetic, not wrapping: Python's ints are unbounded, so a seed
/// big enough to overflow would draw a DIFFERENT MT19937 stream there than
/// any `i64` could here. That is a silent divergence, so it is asserted.
/// (`economy::deck_rng` makes the same call for the same reason.)
fn rng_for(state: &GameState) -> PyRandom {
    let s = i64::try_from(state.seed)
        .ok()
        .and_then(|s| s.checked_mul(1_000_003))
        .and_then(|s| s.checked_add(state.turn as i64 * 97))
        .and_then(|s| s.checked_add(state.round as i64))
        .expect(
            "seed * 1000003 + turn * 97 + round overflows i64; Python's unbounded ints would \
             seed a different MT19937 stream -- widen rng::PyRandom::new rather than wrapping",
        );
    PyRandom::new(s)
}

// ==================================================================== setup

/// A blank player, before §1.4's starting cards are dealt onto them.
///
/// Every field is spelled out rather than derived from a `Default` impl:
/// `PlayerState` deliberately has none (`state.rs`), because a state type
/// with a `Default` invites a partially-initialised player through a
/// `..Default::default()` tail, and the field that got silently defaulted is
/// the one nobody notices.
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
        yellow_bank: 18,
        yellow_granted: 0,
        workers_free: 1,
        blue_total: 16,
        food: 0,
        resources: 0,
        science: 0,
        culture: 0,
        culture_rate_extra: 0,
        science_rate_extra: 0,
        strength_extra: 0,
        happy_extra: 0,
        civil_actions: 4,
        military_actions: 2,
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

/// A card by printed name, or a panic naming it. Setup-only: five starting
/// techs and one starting government, resolved once per game. DESIGN.md rule
/// 1 forbids names as engine KEYS, not names in I/O, and §1.4's card list is
/// I/O -- the alternative is six magic `CardId` literals that silently point
/// at the wrong cards the next time `card_table.rs` is regenerated.
fn named(name: &str) -> CardId {
    CardId::by_name(name).unwrap_or_else(|| panic!("card_table.rs has no card named {name:?}"))
}

/// Every copy of every card in one age's deck, in `CARDS` order -- which is
/// `data/*.json` order, which is the order `engine/cards.py::_deck` builds
/// its list in, so the list handed to the shuffler is identical on both
/// sides and the shuffle lands on the same permutation.
///
/// `civil` selects the deck the way Python's `c["deck"]` default does:
/// `CardType::is_civil_row()` is `cards.CIVIL_ROW_TYPES` exactly.
/// Multiplicity comes from `Card::count`, indexed `[num_players - 2]`, so a
/// card printed 0 times at this player count (wonders, leaders, starting
/// techs, and the 3+/4-player-only military cards) is simply absent -- §13's
/// deck trimming is the same mechanism, not a separate filter.
fn build_deck(age: Age, civil: bool, num_players: usize) -> CardList<MAX_DECK> {
    let mut out = CardList::new();
    let slot = num_players - 2;
    for (i, c) in CARDS.iter().enumerate() {
        if c.age != age || c.kind.is_civil_row() != civil {
            continue;
        }
        for _ in 0..c.count[slot] {
            out.push(CardId(i as u16));
        }
    }
    out
}

/// Deal a fresh game (§1).
///
/// # Panics
///
/// Outside 2..=4 players -- Through the Ages is a 2-4 player game, and the
/// expansion (which is not in scope) is what changes that.
pub fn new_game(num_players: u8, seed: u64) -> GameState {
    assert!(
        (2..=MAX_PLAYERS as u8).contains(&num_players),
        "Through the Ages is a 2-4 player game, got {num_players}"
    );
    let n = num_players as usize;
    let mut rng = PyRandom::new(
        i64::try_from(seed).expect("game seed does not fit in i64; see rng::PyRandom::new"),
    );

    let despotism = named("Despotism");
    let mut players: [PlayerState; MAX_PLAYERS] =
        std::array::from_fn(|i| blank_player(i as u8, despotism));
    for (i, p) in players[..n].iter_mut().enumerate() {
        for (name, workers) in START_TECHS {
            p.techs.insert(named(name), TechSlot { workers, stored: 0 });
        }
        // §1.9: first-round civil actions are 1, 2, 3, 4 by seating order --
        // the compensation for moving later. Military actions are 0 in round
        // one for everybody (there is no politics phase to spend them in).
        p.civil_actions = i as i8 + 1;
        p.military_actions = 0;
    }

    let mut state = GameState {
        num_players,
        seed,
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
        scoring_events: CardList::new(),
        available_tactics: CardList::new(),
        civil_discard: std::array::from_fn(|_| CardList::new()),
        civil_removed: std::array::from_fn(|_| CardList::new()),
        discarded_military: std::array::from_fn(|_| CardList::new()),
        last_round: false,
        final_round_end: None,
        game_over: false,
        // §1.9: round one has no politics phase.
        phase: Phase::Actions,
        forced_winner: None,
        pending: PendingStack::new(),
        queue: Queue::new(),
    };

    // The thirteen row slots, dealt from the shuffled Age A civil deck. That
    // deck is 20 cards at every player count, so `deal` cannot exhaust it
    // here and cannot advance the age during setup -- but it is not written
    // as if that were guaranteed, because `deal` is the same function the
    // rest of the game uses.
    state.civil_deck = build_deck(Age::A, true, n);
    shuffle_cards(&mut rng, state.civil_deck.as_mut_slice());
    deal(&mut state, &mut rng);

    // §1.6: no military DECK in Age A -- the ten Age A military cards are
    // shuffled and the top `num_players + 2` become the starting current
    // events. The rest are simply not in the game. `military_deck` stays
    // empty until the first age advance builds the Age I deck.
    let mut age_a = build_deck(Age::A, false, n);
    shuffle_cards(&mut rng, age_a.as_mut_slice());
    for &card in &age_a.as_slice()[..(n + 2).min(age_a.len())] {
        state.current_events.push(card);
    }

    state
}

// ================================================================= card row

/// Player count for deck trimming and event tables (§13, and resignations).
/// Clamped to 2..=4: a one-player endgame still has to price a deck.
pub fn live_count(state: &GameState) -> usize {
    state.active().count().clamp(2, 4)
}

/// §2.1: discard the leftmost N cards, slide the rest left, deal from the
/// current deck.
fn replenish(state: &mut GameState, rng: &mut PyRandom) {
    // §1.10: the FIRST replenish ends Age A outright -- whatever is left of
    // the 20-card Age A deck is simply removed from the game, with no
    // antiquation and no yellow-token loss (that is what `advance_age`'s
    // `ended != A` guard buys). Note that those leftover cards are NOT
    // recorded in `civil_discard`/`civil_removed`: Python drops them the same
    // way, and the card census treats "never dealt" as unseen either way.
    if state.age_civil == Age::A {
        state.civil_deck = CardList::new();
        advance_age(state, rng);
    }

    let n = sweep_count(live_count(state)).min(ROW_SIZE);
    for i in 0..n {
        let card = state.card_row[i];
        if !card.is_none() {
            // The public record of what was destroyed (§2.1). Keyed by the
            // CARD's own age, not the row's -- a row can hold two ages at
            // once across an age boundary.
            state.civil_discard[card.get().age as usize].push(card);
        }
        state.card_row[i] = CardId::NONE;
    }
    // Slide left, preserving order: `[kept..., NONE...]`.
    let mut kept = [CardId::NONE; ROW_SIZE];
    let mut k = 0;
    for i in 0..ROW_SIZE {
        if !state.card_row[i].is_none() {
            kept[k] = state.card_row[i];
            k += 1;
        }
    }
    state.card_row = kept;
    deal(state, rng);
}

/// Fill empty row slots from the current civil deck (§2.1 step 3).
///
/// Python exports this as `game.deal_row(state, rng)` for exactly one caller:
/// `interact._finish_take_row`, which compacts the row after a free take and
/// then has to refill it. The rng is derived here rather than taken as an
/// argument, for the reason in this module's "Randomness" note.
pub fn deal_row(state: &mut GameState) {
    let mut rng = rng_for(state);
    deal(state, &mut rng);
}

///
/// §2.2: the age ends the MOMENT its last card is dealt, not when the row
/// next needs one -- so the deck can advance mid-deal and the remaining slots
/// come out of the new age. Getting that boundary wrong shows up as a row
/// that is one card short for exactly one turn per age.
fn deal(state: &mut GameState, rng: &mut PyRandom) {
    for i in 0..ROW_SIZE {
        if !state.card_row[i].is_none() {
            continue;
        }
        let Some(card) = state.civil_deck.pop() else { break };
        state.card_row[i] = card;
        if state.civil_deck.is_empty() {
            advance_age(state, rng);
            if state.civil_deck.is_empty() {
                break;
            }
        }
    }
}

// ========================================================= age progression

/// End the current age and make the next age's decks current (§12.2).
fn advance_age(state: &mut GameState, rng: &mut PyRandom) {
    let ended = state.age_civil;
    let Some(nxt) = next_age(ended) else { return };

    if ended != Age::A {
        antiquate(state, ended);
        // §12.2.4: two unborn population are removed from every supply at
        // every age change after the first.
        for p in state.players[..state.num_players as usize].iter_mut() {
            p.yellow_bank = p.yellow_bank.saturating_sub(2);
        }
    }

    state.age_civil = nxt;
    state.age_military = nxt;
    if nxt == Age::IV {
        state.civil_deck = CardList::new();
        state.military_deck = CardList::new();
        set_last_round(state);
    } else {
        // §13: FUTURE-age decks are trimmed for the surviving player count.
        // The civil deck is built and shuffled before the military one, and
        // both draw from the same stream -- swapping them is a different
        // game from the same seed.
        let n = live_count(state);
        state.civil_deck = build_deck(nxt, true, n);
        shuffle_cards(rng, state.civil_deck.as_mut_slice());
        state.military_deck = build_deck(nxt, false, n);
        shuffle_cards(rng, state.military_deck.as_mut_slice());
    }
    // Python calls `effects.invalidate(state)` here; there is no stats cache
    // in this port (see `economy::increase_population`), so there is nothing
    // to invalidate.
}

/// Remove cards of ages OLDER than the age that just ended (§12.2).
///
/// Everything culled is RECORDED (`economy::discard_civil` /
/// `discard_military`). They used to vanish on the Python side, and an age's
/// printed card count stopped adding up the moment antiquation touched it --
/// `engine/bots/counting`, which subtracts what it has seen from what the
/// rulebook prints, got a silent shortfall it could not tell from cards still
/// in a rival's hand. Same "in this list but not that one" shape as GAP 5,
/// one zone over. A human at the table sees this happen: the cull is public.
///
/// Nothing in the rules or the turn loop reads the records, so this cannot
/// change play -- but leaving them out cannot be detected, which is why it is
/// written down here rather than assumed.
fn antiquate(state: &mut GameState, ended: Age) {
    let cutoff = ended as u8;
    for idx in 0..state.num_players as usize {
        // Hands: cull first (so the record is written), then keep the rest.
        // `CardList` has no `retain`, and rebuilding preserves order, which
        // is what Python's list comprehension does.
        let hand = state.players[idx].hand_civil.clone();
        let mut keep: CardList<{ crate::state::MAX_HAND }> = CardList::new();
        for &card in hand.as_slice() {
            if card.get().age as u8 >= cutoff {
                keep.push(card);
            } else {
                economy::discard_civil(state, card);
            }
        }
        state.players[idx].hand_civil = keep;

        let hand = state.players[idx].hand_military.clone();
        let mut keep: CardList<{ crate::state::MAX_HAND }> = CardList::new();
        for &card in hand.as_slice() {
            if card.get().age as u8 >= cutoff {
                keep.push(card);
            } else {
                economy::discard_military(state, card);
            }
        }
        state.players[idx].hand_military = keep;

        let leader = state.players[idx].leader;
        if !leader.is_none() && (leader.get().age as u8) < cutoff {
            on_leave_play(&mut state.players[idx], leader);
            economy::discard_civil(state, leader);
            state.players[idx].leader = CardId::NONE;
        }

        let wonder = state.players[idx].wonder;
        if !wonder.is_none() && (wonder.get().age as u8) < cutoff {
            economy::discard_civil(state, wonder);
            // Python drops the whole `WonderInProgress`, which takes
            // `steps_built` with it; here the two are separate fields, so the
            // step count has to be cleared explicitly or the NEXT wonder
            // starts part-built. The blue tokens that were on it return to
            // the bank, which needs no bookkeeping: `blue_total` is unchanged
            // and `economy::blue_used` re-derives occupancy from what is in
            // play (see `economy::blue_available`).
            state.players[idx].wonder = CardId::NONE;
            state.players[idx].wonder_steps = 0;
        }

        // §12.2.2: antiquated PACTS leave play. Technologies, wonders,
        // colonies, tactics and declared wars all stay.
        state.players[idx]
            .pacts
            .retain(|pact| pact.card.get().age as u8 >= cutoff);
    }
}

/// The leave-play token bookkeeping, for the one caller [`antiquate`] has.
///
/// Duplicated from `apply.rs`'s private `on_leave_play` rather than shared,
/// exactly as `legal.rs`/`costs.rs`/`apply.rs` each keep their own four-line
/// `leader_is`: making it `pub(crate)` is an edit to a module this port does
/// not own. It is six lines and it is the whole of Python's
/// `effects.on_leave_play` for the non-`Special` half. (Bill Gates'
/// `cultureOnLeaveEqualToLabResourceProduction` is a gap `apply.rs` already
/// names and carries; it is not re-opened here.)
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

/// §12.3: Age IV begins -> this round or the next one is the last.
///
/// Which of the two depends on whose turn triggered it: if the player who
/// triggered it is the start player, the round they are in is the last one;
/// otherwise everyone gets to finish the round AND play one more, so that
/// nobody's civilization is scored a turn short of a rival's.
fn set_last_round(state: &mut GameState) {
    if state.final_round_end.is_some() {
        return;
    }
    let end = if state.current == state.start_player {
        state.round
    } else {
        state.round + 1
    };
    state.final_round_end = Some(end);
    state.last_round = state.round >= end;
}

// =============================================================== turn loop

/// Start-of-Turn sequence + politics phase entry (§5.0).
///
/// The three start-of-turn steps happen in this order and only from round two
/// on: the row is replenished, a war I declared LAST turn resolves, and a
/// tactic I have been keeping to myself becomes public. Replenishing first
/// matters -- it is what can end an age, and an age change can antiquate a
/// card out of the very tableau the war is about to be scored against.
fn start_turn(state: &mut GameState, rng: &mut PyRandom) {
    let idx = state.current;
    if state.round > 1 {
        replenish(state, rng);

        // §5.7: `events.resolve_war`, in its two ported halves. `None` means
        // either no war was declared or it was a dead tie -- both of which
        // Python returns from without spoils, having already discarded the
        // card and cleared the tracking fields.
        if let Some(outcome) = combat::resolve_war_outcome(state, idx) {
            combat::apply_war_spoils(state, &outcome);
        }

        // §10.2: a tactic played from my own hand is exclusive to me for one
        // turn; at the start of my NEXT turn it joins the common area and
        // anybody may copy it.
        if state.players[idx as usize].tactic_exclusive {
            let tactic = state.players[idx as usize].tactic;
            if !tactic.is_none() && !state.available_tactics.contains(tactic) {
                state.available_tactics.push(tactic);
            }
            state.players[idx as usize].tactic_exclusive = false;
        }
    }

    state.last_round = state
        .final_round_end
        .is_some_and(|end| state.round >= end);

    {
        let p = &mut state.players[idx as usize];
        p.politics_done = false;
        p.taken_this_turn = CardList::new();
        p.ca_spent_taking = 0;
    }

    if state.players[idx as usize].skip_next_politics {
        // International Agreement (CoL p.12).
        state.players[idx as usize].skip_next_politics = false;
        state.phase = Phase::Actions;
    } else if state.round > 1 && !state.game_over {
        // Python also gates on `state.has_military`, a card-DATABASE
        // completeness flag that is always true for the compiled-in base game
        // -- the same non-field `legal.rs::politics_moves` and
        // `economy::end_of_turn` both document.
        state.phase = Phase::Politics;
        if state.pending.is_empty() {
            auto_skip_politics(state);
        } else {
            // `resolve_war` above can leave a War over Technology's spoils
            // decision outstanding (§5.7), and stealing a blue technology can
            // hand the CURRENT player military actions -- Warfare +1,
            // Strategy +2, Military Theory +3 -- which is exactly what
            // `auto_skip_politics` reads to decide whether passing is the
            // only political option. Answering that before the spoils are
            // taken would deny the victor a politics phase it is owed, so the
            // test is deferred behind the decision. Nothing else can be
            // pending here: measured across the Python fingerprint's 33
            // games, `state.pending` was empty on all 3737 arrivals.
            state.queue.push_back(QueueItem::AutoSkipPolitics { player: idx });
        }
    } else {
        state.phase = Phase::Actions;
    }
}

/// Pass immediately when passing is the only political option.
///
/// Note that this practically never fires in the base game: `PolPass` and
/// `Resign` are both unconditional outside Age IV, so the list is two long.
/// It is ported anyway because it is the only thing that skips the phase
/// without spending the player's political action, and because in Age IV
/// (where `Resign` is not offered) it does fire.
pub fn auto_skip_politics(state: &mut GameState) {
    if legal::legal_moves(state).len() == 1 {
        state.me_mut().politics_done = true;
        state.phase = Phase::Actions;
    }
}

/// End-of-Turn sequence, then hand the turn to the next player (§6.6).
///
pub fn end_turn(state: &mut GameState) {
    resume_end_turn(state, state.current);
}

/// Run §6.6 from step 1, suspending if the discard step needs a decision.
///
/// §6.6 step 1 is the only end-of-turn step that asks the player anything,
/// and it is a real choice (RB p.20: "once you have decided which military
/// cards to discard, the rest of your turn is automatic"). When it opens that
/// choice the turn does NOT advance: the continuation is queued as a
/// [`QueueItem::EndOfTurn`], `interact::run_queue` drains it once the player
/// has chosen, and `interact::_q_end_of_turn` lands back here. Steps 2-5 and
/// the hand-off therefore stay strictly AFTER the discard, as the sequence
/// requires -- the next player may not start until the discarding is done.
///
/// `pub` because that resume path is interact.rs's call back in; it is the
/// same entry `engine/interact.py::_q_end_of_turn` uses.
///
/// The rng is derived HERE and not only in [`end_turn`]: the resume path
/// arrives from the queue with no rng at all, and [`rng_for`] reads
/// seed/turn/round, none of which a discard can change -- so deriving it at
/// resume time gives the same stream the unsuspended sequence would have
/// used.
pub fn resume_end_turn(state: &mut GameState, idx: u8) {
    let mut rng = rng_for(state);
    if !economy::end_of_turn(state, idx) {
        state.queue.push_back(QueueItem::EndOfTurn { player: idx });
        return;
    }
    advance_turn(state, &mut rng);
}

/// §5.11: a resigning player's turn ends at once, and the last one left wins.
///
/// Called by `apply::h_resign` after it has performed every other effect of
/// resigning (hands discarded, pacts dropped, wars against the resigner paid
/// out at 7 culture each).
pub fn after_resign(state: &mut GameState) {
    let mut rng = rng_for(state);
    let mut survivors = [0u8; MAX_PLAYERS];
    let mut n = 0usize;
    for p in state.active() {
        survivors[n] = p.idx;
        n += 1;
    }
    if n <= 1 {
        state.forced_winner = (n == 1).then(|| survivors[0]);
        finish_game(state);
        return;
    }
    advance_turn(state, &mut rng);
}

fn advance_turn(state: &mut GameState, rng: &mut PyRandom) {
    state.turn += 1;

    let Some(nxt) = next_player(state) else {
        // Everybody has resigned. `after_resign` handles the one-left case
        // before it ever gets here, so this is the nobody-left case.
        finish_game(state);
        return;
    };
    // A wrap is "the next seat is at or before mine in this round's turn
    // order" -- by SEAT, not by player index, because the start player is not
    // player 0 forever and `nxt <= current` on raw indices would miss the
    // wrap whenever the start player has moved.
    let wrapped = seat_index(state, nxt) <= seat_index(state, state.current);
    state.current = nxt;
    if wrapped {
        state.round += 1;
        if state.final_round_end.is_some_and(|end| state.round > end) {
            finish_game(state);
            return;
        }
    }
    start_turn(state, rng);
}

/// Position in the current round's turn order (the start player is 0).
fn seat_index(state: &GameState, idx: u8) -> u8 {
    (idx + state.num_players - state.start_player) % state.num_players
}

/// The next player still in the game, or `None` if there is nobody left.
/// Walks a full lap so that the CURRENT player is returned when they are the
/// only one who has not resigned.
fn next_player(state: &GameState) -> Option<u8> {
    let n = state.num_players;
    for step in 1..=n {
        let cand = (state.current + step) % n;
        if !state.players[cand as usize].resigned {
            return Some(cand);
        }
    }
    None
}

// ================================================================ game end

/// §12.5 final scoring. Mirrors `engine/game.py::_finish_game`.
fn finish_game(state: &mut GameState) {
    // §12.5.2: Age III events left in the current/future decks score at game
    // end -- `events::evaluate_final_events`. Python guards this call with
    // `if state.has_military:`; that flag is card-database-completeness, not
    // per-game state, and is always true for this port's always-complete
    // 236-card table (same reasoning `legal.rs::politics_moves` already
    // documents for the identical guard), so there is no `state.has_military`
    // field to read here and the call is unconditional.
    events::evaluate_final_events(state);

    for idx in 0..state.num_players as usize {
        let bonus = end_of_game_bonus(&state.players[idx]);
        let p = &mut state.players[idx];
        p.culture = (p.culture as i32 + bonus).max(0) as u16;
    }
    state.game_over = true;
    state.phase = Phase::Done;
}

/// §12.5.3: end-of-game culture a card pays for what it leaves behind.
///
/// Bill Gates is the only carrier in the base game: he scores the sum of
/// `lab level * workers`, the same quantity he pays out as resources every
/// turn. Python dispatches on the leader's NAME
/// (`effects.end_of_game_bonus`); this reads the card's own
/// `Special::CultureOnLeaveEqualToLabResourceProduction`, so a second carrier
/// would be scored rather than silently ignored.
fn end_of_game_bonus(p: &PlayerState) -> i32 {
    if p.leader.is_none() {
        return 0;
    }
    let carries = p
        .leader
        .get()
        .special
        .contains(&Special::CultureOnLeaveEqualToLabResourceProduction);
    if !carries {
        return 0;
    }
    p.techs
        .of_type(CardType::Lab)
        .map(|(id, slot)| id.level() as i32 * slot.workers as i32)
        .sum()
}

// ===================================================================== API

/// Index of the player who must choose the next move.
///
/// `engine/game.py::current_player` -- `state.decider()`, which is the owner
/// of the outstanding decision when there is one and `state.current`
/// otherwise. An aggression defense, a colonization auction and a pact offer
/// are all answered by somebody else while `current` stays put.
#[inline]
pub fn current_player(state: &GameState) -> u8 {
    state.decider()
}

#[inline]
pub fn is_over(state: &GameState) -> bool {
    state.game_over
}

/// Culture by player index. After the game is over these are the final
/// scores: `finish_game` writes the end-of-game bonuses into `p.culture`
/// itself, exactly as Python does before snapshotting them into
/// `final_scores` (see this module's KNOWN GAPS 1).
pub fn scores(state: &GameState) -> Vec<i32> {
    state.players[..state.num_players as usize]
        .iter()
        .map(|p| p.culture as i32)
        .collect()
}

/// Indices of the players with the most culture -- ties share the win.
///
/// A resigned player is scored `-1` rather than skipped, so that a game in
/// which everybody resigned still names somebody, and so that the returned
/// indices stay indices into `state.players`.
pub fn winners(state: &GameState) -> Vec<u8> {
    if let Some(w) = state.forced_winner {
        return vec![w]; // §5.11 last player standing
    }
    let sc: Vec<i32> = state.players[..state.num_players as usize]
        .iter()
        .map(|p| if p.resigned { -1 } else { p.culture as i32 })
        .collect();
    let best = sc.iter().copied().max().unwrap_or(0);
    sc.iter()
        .enumerate()
        .filter(|(_, &v)| v == best)
        .map(|(i, _)| i as u8)
        .collect()
}

// ================================================================== driver

/// Decisions a single game is allowed before it is declared stuck. A 3p
/// self-play game is ~372; this is two orders of magnitude of headroom, so
/// hitting it means a loop, not a long game.
pub const MOVE_CAP: usize = 20_000;

/// How a game ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// Decisions taken.
    pub moves_played: usize,
    /// The game was aborted at [`MOVE_CAP`] rather than reaching §12.5.
    /// Should never happen; Python carries the same flag on the state.
    pub move_cap_hit: bool,
}

/// Run a game to the end. `pick` chooses one move from the legal list for
/// whichever player is to act; it is handed the state and the list so that a
/// bot, a fixture replay and a random driver can all use the same loop.
///
/// Ports `engine/game.py::play_game`. The `bots[state.decider()]` indirection
/// is not reproduced: with no decision queue the decider is always
/// `state.current`, which `pick` can read off the state itself.
pub fn play_game(
    state: &mut GameState,
    move_cap: usize,
    mut pick: impl FnMut(&GameState, &MoveList) -> Move,
) -> Outcome {
    let mut moves = 0usize;
    while !state.game_over {
        if moves >= move_cap {
            finish_game(state);
            return Outcome { moves_played: moves, move_cap_hit: true };
        }
        let legal = legal::legal_moves(state);
        assert!(
            !legal.is_empty(),
            "no legal move for player {} in phase {:?} (turn {}, round {})",
            state.current,
            state.phase,
            state.turn,
            state.round
        );
        let mv = pick(state, &legal);
        step(state, mv);
        moves += 1;
    }
    Outcome { moves_played: moves, move_cap_hit: false }
}

/// Apply one chosen move. `engine/game.py` re-exports `actions.apply` under
/// this name; here it is a one-line forward to [`crate::apply::apply`], which
/// routes `Move::EndTurn` back into [`end_turn`] and `Move::Resign` into
/// [`after_resign`] (the two moves that are turn-loop control flow rather
/// than board actions -- `apply.rs` gained both call sites when this module
/// landed).
///
/// It exists so that a driver has one entry point that does not have to know
/// which module owns which move, which is what `play_game` and every test
/// here use.
#[inline]
pub fn step(state: &mut GameState, mv: Move) {
    crate::apply::apply(state, mv);
}

// ================================================================== tests

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_game_deals_the_row_and_the_starting_tableaux() {
        for n in 2..=4u8 {
            let s = new_game(n, 7);
            assert_eq!(s.num_players, n);
            assert_eq!(s.round, 1);
            assert_eq!(s.turn, 1);
            assert_eq!(s.phase, Phase::Actions, "§1.9: no politics phase in round 1");
            // 20 Age A civil cards, 13 into the row, 7 left in the deck.
            assert!(s.card_row.iter().all(|c| !c.is_none()), "all 13 slots dealt");
            assert_eq!(s.civil_deck.len(), 20 - ROW_SIZE);
            assert!(s.military_deck.is_empty(), "§1.6: no military deck in Age A");
            assert_eq!(s.current_events.len(), n as usize + 2);
            for (i, p) in s.players[..n as usize].iter().enumerate() {
                assert_eq!(p.civil_actions, i as i8 + 1, "§1.9 seating-order civil actions");
                assert_eq!(p.military_actions, 0);
                assert_eq!(p.yellow_bank, 18);
                assert_eq!(p.blue_total, 16);
                assert_eq!(p.workers_free, 1);
                assert_eq!(p.techs.len(), START_TECHS.len());
                assert_eq!(p.techs.workers(named("Agriculture")), 2);
                assert_eq!(p.techs.workers(named("Religion")), 0);
                assert_eq!(p.government, named("Despotism"));
            }
        }
    }

    /// Build order in the tableau is play-relevant (`Tableau::remove`'s doc
    /// comment: `economy::lose_population` walks it and takes the first
    /// worker it finds), so §1.4's order is asserted, not assumed.
    #[test]
    fn starting_techs_are_in_printed_order() {
        let s = new_game(3, 1);
        let order: Vec<&str> = s.players[0].techs.iter().map(|(id, _)| id.name()).collect();
        assert_eq!(order, START_TECHS.iter().map(|(n, _)| *n).collect::<Vec<_>>());
    }

    #[test]
    fn the_same_seed_deals_the_same_game() {
        let a = new_game(4, 12345);
        let b = new_game(4, 12345);
        assert_eq!(a.card_row, b.card_row);
        assert_eq!(a.civil_deck.as_slice(), b.civil_deck.as_slice());
        assert_eq!(a.current_events.as_slice(), b.current_events.as_slice());
        let c = new_game(4, 12346);
        assert_ne!(a.card_row, c.card_row);
    }

    #[test]
    fn deck_sizes_match_the_printed_counts() {
        // Measured from `data/*.json` via `engine.cards.CardDB` on
        // 2026-08-05; if these move, the card data changed.
        for (n, civil, military) in [(2usize, [20, 44, 44, 44, 0], [10, 43, 46, 41, 0]),
                                     (3, [20, 50, 50, 50, 0], [10, 45, 50, 45, 0]),
                                     (4, [20, 53, 53, 53, 0], [10, 45, 50, 45, 0])] {
            for (i, age) in [Age::A, Age::I, Age::II, Age::III, Age::IV].into_iter().enumerate() {
                assert_eq!(build_deck(age, true, n).len(), civil[i], "{n}p civil {age:?}");
                assert_eq!(build_deck(age, false, n).len(), military[i], "{n}p military {age:?}");
            }
        }
    }

    /// §2.1: sweep the leftmost N, slide the survivors left, refill from the
    /// right. The swept cards are recorded; the survivors keep their order.
    #[test]
    fn replenish_sweeps_slides_and_refills() {
        let mut s = new_game(3, 5); // 3p sweeps 2
        // Get out of Age A first, so the sweep is a plain sweep rather than
        // §1.10's age-ending special case.
        let mut rng = PyRandom::new(1);
        replenish(&mut s, &mut rng);
        assert_eq!(s.age_civil, Age::I, "§1.10: the first replenish ends Age A");

        let before = s.card_row;
        let swept: Vec<CardId> = before[..2].to_vec();
        replenish(&mut s, &mut rng);
        assert_eq!(&s.card_row[..ROW_SIZE - 2], &before[2..], "survivors slid left in order");
        assert!(s.card_row.iter().all(|c| !c.is_none()), "refilled to 13");
        for card in swept {
            assert!(
                s.civil_discard[card.get().age as usize].contains(card),
                "{card:?} was swept and must be recorded (§2.1)"
            );
        }
    }

    /// §2.2: the age ends the moment its last card is DEALT, and the rest of
    /// the row comes out of the new age in the same deal.
    #[test]
    fn an_exhausted_deck_advances_the_age_mid_deal() {
        let mut s = new_game(2, 3);
        let mut rng = PyRandom::new(1);
        replenish(&mut s, &mut rng); // out of Age A
        // Empty the row and leave one card in the deck.
        s.card_row = [CardId::NONE; ROW_SIZE];
        while s.civil_deck.len() > 1 {
            s.civil_deck.pop();
        }
        deal(&mut s, &mut rng);
        assert_eq!(s.age_civil, Age::II);
        assert!(s.card_row.iter().all(|c| !c.is_none()), "row filled across the age boundary");
        assert!(
            s.card_row.iter().any(|c| c.get().age == Age::II),
            "the second age's cards are in the row"
        );
    }

    /// §12.2: an age change culls older cards from hands, and §12.2.4 takes
    /// two unborn population from every supply.
    #[test]
    fn advancing_out_of_age_i_antiquates_and_taxes_the_bank() {
        let mut s = new_game(2, 11);
        let mut rng = PyRandom::new(1);
        replenish(&mut s, &mut rng); // A -> I, no antiquation, no tax
        assert_eq!(s.players[0].yellow_bank, 18);

        let age_a_card = named("Bronze");
        let age_i_card = named("Irrigation");
        s.players[0].hand_civil.push(age_a_card);
        s.players[0].hand_civil.push(age_i_card);
        s.age_civil = Age::I;
        advance_age(&mut s, &mut rng);

        assert_eq!(s.age_civil, Age::II);
        assert_eq!(s.players[0].hand_civil.as_slice(), &[age_i_card], "Age A card culled");
        assert!(s.civil_removed[Age::A as usize].contains(age_a_card), "and recorded");
        assert_eq!(s.players[0].yellow_bank, 16, "§12.2.4");
        assert_eq!(s.players[1].yellow_bank, 16);
    }

    /// §12.3: reaching Age IV as the start player makes THIS round the last;
    /// reaching it as anybody else gives everyone one more.
    #[test]
    fn age_iv_sets_the_last_round_from_the_seat_that_triggered_it() {
        let mut s = new_game(3, 2);
        s.round = 9;
        s.current = 0; // == start_player
        set_last_round(&mut s);
        assert_eq!(s.final_round_end, Some(9));
        assert!(s.last_round);

        let mut s = new_game(3, 2);
        s.round = 9;
        s.current = 2;
        set_last_round(&mut s);
        assert_eq!(s.final_round_end, Some(10));
        assert!(!s.last_round);

        // Idempotent: a second age-IV trigger must not move the deadline.
        s.round = 10;
        set_last_round(&mut s);
        assert_eq!(s.final_round_end, Some(10));
    }

    #[test]
    fn seat_index_is_relative_to_the_start_player() {
        let mut s = new_game(4, 0);
        s.start_player = 2;
        assert_eq!(seat_index(&s, 2), 0);
        assert_eq!(seat_index(&s, 3), 1);
        assert_eq!(seat_index(&s, 0), 2);
        assert_eq!(seat_index(&s, 1), 3);
    }

    #[test]
    fn next_player_skips_the_resigned_and_wraps() {
        let mut s = new_game(4, 0);
        s.current = 1;
        assert_eq!(next_player(&s), Some(2));
        s.players[2].resigned = true;
        s.players[3].resigned = true;
        assert_eq!(next_player(&s), Some(0));
        s.players[0].resigned = true;
        assert_eq!(next_player(&s), Some(1), "the last one left is their own successor");
        s.players[1].resigned = true;
        assert_eq!(next_player(&s), None);
    }

    /// §5.11: resigning down to one player ends the game at once and that
    /// player wins regardless of culture.
    #[test]
    fn resigning_to_one_player_forces_the_winner() {
        let mut s = new_game(2, 0);
        s.players[1].culture = 200;
        s.players[0].resigned = true;
        s.current = 0;
        after_resign(&mut s);
        assert!(s.game_over);
        assert_eq!(s.forced_winner, Some(1));
        assert_eq!(winners(&s), vec![1]);
    }

    #[test]
    fn winners_share_a_tie_and_never_include_the_resigned() {
        let mut s = new_game(3, 0);
        s.players[0].culture = 10;
        s.players[1].culture = 10;
        s.players[2].culture = 40;
        s.players[2].resigned = true;
        assert_eq!(winners(&s), vec![0, 1]);
    }

    /// §12.5.3: Bill Gates scores lab level x workers at game end. Read off
    /// the card's `Special`, not off his name.
    #[test]
    fn bill_gates_scores_his_labs_at_game_end() {
        let mut s = new_game(2, 0);
        s.players[0].leader = named("Bill Gates");
        s.players[0].techs.insert(named("Computers"), TechSlot { workers: 2, stored: 0 });
        s.players[0].culture = 5;
        finish_game(&mut s);
        assert!(s.game_over);
        assert_eq!(s.phase, Phase::Done);
        // Computers is an Age III lab: level 3 x 2 workers = 6.
        assert_eq!(scores(&s)[0], 5 + 6);
        assert_eq!(scores(&s)[1], 0);
    }

    #[test]
    fn a_round_advances_only_when_the_turn_order_wraps() {
        let mut s = new_game(3, 4);
        let mut rng = PyRandom::new(1);
        assert_eq!((s.round, s.current), (1, 0));
        advance_turn(&mut s, &mut rng);
        assert_eq!((s.round, s.current), (1, 1));
        advance_turn(&mut s, &mut rng);
        assert_eq!((s.round, s.current), (1, 2));
        advance_turn(&mut s, &mut rng);
        assert_eq!((s.round, s.current), (2, 0));
        assert_eq!(s.turn, 4);
    }

    /// The game must end when the round counter passes the deadline §12.3
    /// set, even though nothing else is exhausted.
    #[test]
    fn passing_the_final_round_ends_the_game() {
        let mut s = new_game(2, 4);
        let mut rng = PyRandom::new(1);
        s.final_round_end = Some(1);
        s.current = 1; // next advance wraps into round 2
        advance_turn(&mut s, &mut rng);
        assert!(s.game_over);
        assert_eq!(s.phase, Phase::Done);
    }
}
