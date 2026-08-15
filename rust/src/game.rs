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
//! 2. ~~**The checked-in differential fixtures cannot be matched at an age
//!    transition.**~~ **CLOSED 2026-08-05.** The fault was never in this
//!    file: `tools/dump_fixtures.py` used to build ONE `random.Random(seed ^
//!    0x5EED)` per game and pass it to every `actions.apply` call (mirroring
//!    `game.play_game`), so `_rng_for(state, rng)` handed `_advance_age` a
//!    PERSISTENT stream whose position depended on everything drawn from it
//!    earlier in the game, while this port derives a fresh stream per
//!    `end_turn` (see "Randomness" above) -- an unrecoverable divergence at
//!    every age transition, verified both directions on `2p_seed2` ply 1
//!    against CPython.
//!
//!    Fixed by regenerating the fixtures through the entry point this port
//!    actually implements -- `dump_fixtures.py` now derives `game._rng_for
//!    (state)` fresh per `actions.apply` call too, matching this module
//!    exactly, instead of threading one persistent stream. Two things blocked
//!    doing that and are both fixed:
//!      * `actions.apply(state, mv)` -- the literal rng=None default this
//!        module's "Randomness" section names -- used to CRASH: `apply`
//!        passed its `rng` straight to the handlers without the `_rng_for`
//!        backfill `game.start_turn`/`end_turn` do, so the first
//!        `prepare_event` died in `events._recycle_future_events` on
//!        `None.shuffle`. Fixed in `engine/actions.py::_h_prepare_event`
//!        (the same `_rng_for` backfill its siblings already had).
//!      * the fixtures were stale against `engine/`'s leader abilities
//!        (`ded32dd`): regenerating surfaced a `remove_leader_yellow` move
//!        tag and a `columbus_colonize` move tag `moves.rs` had no variant
//!        for (both added, with `apply.rs`/`legal.rs` support built on
//!        already-existing primitives -- `apply::grant_yellow`,
//!        `economy::discard_civil`, `interact::gain_colony`, no `events.rs`/
//!        `combat.rs`/`interact.rs` LOGIC touched), and two `PlayerState`
//!        fields (`caesar_second_politics`, `peeked_event`) that postdate the
//!        recording (both added; see `apply::end_politics` and
//!        `game::peek_top_event`, which also closes the "phase always ends
//!        after one action, for every leader" simplification `apply.rs`'s
//!        political handlers used to document -- Julius Caesar's once-per-
//!        game second political action is real rule effect, not fixture
//!        bookkeeping, and was a genuine, if rare, engine disagreement until
//!        now: it fired exactly once across the 9 checked-in fixtures,
//!        `4p_seed1.jsonl` ply 68).

use crate::cards::{Age, CardId, CardType, Special, CARDS};
use crate::combat;
use crate::economy;
use crate::effects;
use crate::events;
use crate::legal;
use crate::moves::{Move, MoveList};
use crate::rng::{shuffle_cards, LazyRandom, PyRandom};
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

// Mirrors `apply.rs`/`legal.rs`/`costs.rs`/`combat.rs`'s own `leader_is`
// (private in each -- see `apply.rs`'s "A note on leader identity" analogue).
#[inline]
fn leader_is(p: &PlayerState, name: &str) -> bool {
    !p.leader.is_none() && p.leader.get().name == name
}

/// Joan of Arc: *"When you begin your politics phase, you may look at the
/// top card of the current events deck."* Automatic, not a decision (`engine/
/// events.py::peek_top_event`'s doc comment: the "may" is a permission with
/// no cost and nothing to regret, not a branch worth a `MoveList` entry), and
/// purely informational -- `peeked_event` has no rule effect of its own
/// (`events.rs::reveal_current_event`'s doc comment). `current_events` is
/// only ever popped from the end (`events::reveal_current_event`), so its
/// last element is the "top" card. Mirrors `engine/events.py::
/// peek_top_event` exactly, including writing `CardId::NONE` for a
/// non-Joan leader (Python writes `None` unconditionally on that branch too,
/// not just leaving the field alone).
fn peek_top_event(state: &mut GameState, idx: u8) {
    let top = if leader_is(&state.players[idx as usize], "Joan of Arc") {
        state.current_events.as_slice().last().copied().unwrap_or(CardId::NONE)
    } else {
        CardId::NONE
    };
    state.players[idx as usize].peeked_event = top;
}

/// §2.1: civil cards swept off the left of the row at the start of a turn, by
/// live player count. Python's `SWEEP = {2: 3, 3: 2, 4: 1}`.
///
/// `pub(crate)` (not private) so `bots::weighted::horizon` -- which needs the
/// exact same number for its own deal-rate arithmetic (Python's
/// `weighted.py::_SWEEP`, a byte-for-byte duplicate of this table kept only
/// because `engine.game` importing `engine.actions`, which `weighted.py`
/// itself imports, would be a cycle) -- can call this directly instead of
/// keeping a second copy that could drift from it. Rust's module tree has no
/// such cycle, so there is no reason for the port to inherit Python's
/// duplication once the reason for it stops applying.
#[inline]
pub(crate) fn sweep_count(live: usize) -> usize {
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
/// Checked arithmetic in `i128`, not wrapping: Python's ints are unbounded,
/// so a seed big enough to overflow would draw a DIFFERENT MT19937 stream
/// there than any fixed-width Rust integer could here -- that is a silent
/// divergence, so it is asserted rather than wrapped. `i128` (not `i64`) is
/// the width: `state.seed` is a `u64`, which climb.rs's search folds a
/// generation counter into via `u64`-wrapping arithmetic before it ever
/// reaches a `GameState` -- so by the time it lands here it is already
/// capped at `u64::MAX` (~1.8e19), no matter how long a climb runs.
/// `u64::MAX * 1_000_003` is ~1.8e25, i.e. about 84 bits; `i128` has 127
/// bits of magnitude, so this has ~43 bits (about 13 decimal orders of
/// magnitude) of headroom that can never be exhausted by this formula. That
/// margin is why `i64` was the wrong width in production: it has only 63
/// bits, comfortably less than the ~84 this formula needs once `state.seed`
/// gets large, and it overflowed for real in a long-running climb (see
/// `git log` around this line for the incident). (`economy::deck_rng` makes
/// the same call, at a smaller multiplier, for the same reason.)
///
/// Returns a [`LazyRandom`], not a built [`PyRandom`]: every caller of this
/// function reaches it on a path that MIGHT reshuffle a deck or pile several
/// calls further down (`deal` -> `advance_age`, `replenish`, `start_turn`),
/// gated on that deck/pile actually being empty -- true on only a small
/// fraction of calls. A profiler run on a live `plan` bot found
/// `PyRandom::new` as the single hottest named symbol in the process; a call
/// count confirmed this function alone accounted for 99.66% of ~510k calls
/// across 8 games, almost all of them paying MT19937's ~1900-word
/// `init_by_array` for a stream nothing ever drew from. Deferring the build
/// to [`LazyRandom::get`] changes nothing observable -- same formula, same
/// algorithm, same fixture bytes once a shuffle actually happens -- it only
/// skips the work when nothing downstream ever asks for a word.
///
/// `pub(crate)`: `events::events_rng` calls this directly rather than
/// keeping its own copy of the formula -- see that function's doc comment
/// for why the two used to disagree and why that was ever an accepted gap.
pub(crate) fn rng_for(state: &GameState) -> LazyRandom {
    let s = i128::from(state.seed)
        .checked_mul(1_000_003)
        .and_then(|s| s.checked_add(state.turn as i128 * 97))
        .and_then(|s| s.checked_add(state.round as i128))
        .expect(
            "seed * 1000003 + turn * 97 + round overflows i128; Python's unbounded ints would \
             seed a different MT19937 stream -- widen rng::PyRandom::new rather than wrapping",
        );
    LazyRandom::new(s)
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
            hammurabi_replaced_this_turn: false,
            breakthrough_ma_funded: false,
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
///
/// `pub(crate)` (not private) so `bots::counting`'s composition lookups
/// (`engine/bots/counting.py`'s `db.civil_deck`/`db.military_deck`) can call
/// the SAME function that actually deals the game, rather than a second
/// filter over `CARDS` that could drift from this one -- the "two registries"
/// bug class DESIGN.md calls out, closed by construction for this fact.
/// `num_players` must be `2..=4`; callers below that (a forced-win endgame
/// with one player left, mirroring Python's `_live_count` being unclamped)
/// must not call this -- see `bots::counting`'s own guard.
pub(crate) fn build_deck(age: Age, civil: bool, num_players: usize) -> CardList<MAX_DECK> {
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
    // `i128::from(u64)` is total (never fails): `PyRandom::new` takes `i128`
    // precisely so this and every other `state.seed`-derived construction
    // never need a fallible conversion -- see `rng::PyRandom::new`'s doc
    // comment for why `i128` is permanent headroom for a `u64` seed.
    let mut rng = PyRandom::new(i128::from(seed));

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
        seeded_by: [crate::state::NOT_SEEDED; crate::cards::NUM_CARDS],
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
        last_end_of_turn_culture: [None; MAX_PLAYERS],
    };

    // The thirteen row slots, dealt from the shuffled Age A civil deck. That
    // deck is 20 cards at every player count, so `deal` cannot exhaust it
    // here and cannot advance the age during setup -- but it is not written
    // as if that were guaranteed, because `deal` is the same function the
    // rest of the game uses.
    state.civil_deck = build_deck(Age::A, true, n);
    shuffle_cards(&mut rng, state.civil_deck.as_mut_slice());
    // Wrapped rather than passed as a bare `&mut PyRandom`: `deal` takes a
    // `LazyRandom` everywhere else it is called from (see `rng_for`'s doc
    // comment), and `built` keeps this call on the SAME already-drawn-from
    // stream instead of starting a second one at the same seed.
    let mut lazy_rng = LazyRandom::built(rng);
    deal(&mut state, &mut lazy_rng);
    let rng = lazy_rng.get();

    // §1.6: no military DECK in Age A -- the ten Age A military cards are
    // shuffled and the top `num_players + 2` become the starting current
    // events. The rest are simply not in the game. `military_deck` stays
    // empty until the first age advance builds the Age I deck.
    let mut age_a = build_deck(Age::A, false, n);
    shuffle_cards(rng, age_a.as_mut_slice());
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
fn replenish(state: &mut GameState, rng: &mut LazyRandom) {
    // §1.10: whether THIS replenish is the one that ends Age A. Captured
    // before sweeping/dealing, and checked again below rather than assumed,
    // because `deal` can itself already advance the age (see the fallback
    // note below) -- in which case the check after `deal` must be a no-op,
    // not a second advance.
    let first_replenish = state.age_civil == Age::A;

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
    // §1.10: fill from whatever deck is current -- for the first replenish
    // that is the remaining (7-card) Age A civil deck, NOT Age I. `deal`
    // already falls through to the next age if the deck it is drawing from
    // runs dry mid-fill (its own doc comment, §2.2), which is exactly the
    // "if Age A civil cards run out while filling, continue filling from the
    // Age I deck" fallback -- nothing extra is needed here for that case. At
    // the current card counts this deck never actually runs dry here (7
    // available, `player_count + sweep_count` = 5 needed at every player
    // count), but the fallback is implemented anyway since a resignation or
    // future data change could reach it.
    deal(state, rng);

    // §1.10: the first replenish ends Age A outright, whether or not the
    // deck emptied while filling above -- unlike Ages I/II/III, Age A's
    // end is triggered by the replenish itself, not by deck exhaustion. If
    // `deal` already advanced the age above (the deck-ran-dry fallback),
    // `state.age_civil` is no longer `A` and this is a no-op guard. If it
    // didn't (the normal case), this call boxes whatever Age A cards are
    // still in `civil_deck` (overwritten by `advance_age`'s
    // `build_deck(nxt, ..)`, so not separately recorded -- same "never
    // dealt is unseen" reasoning as before) and installs the Age I civil
    // and military decks. `advance_age`'s `ended != Age::A` guard still
    // skips antiquation and the -2 yellow-token loss here, since `ended`
    // reads `state.age_civil`, which is still `A` at this point.
    if first_replenish && state.age_civil == Age::A {
        advance_age(state, rng);
    }
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
fn deal(state: &mut GameState, rng: &mut LazyRandom) {
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
fn advance_age(state: &mut GameState, rng: &mut LazyRandom) {
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
        shuffle_cards(rng.get(), state.civil_deck.as_mut_slice());
        state.military_deck = build_deck(nxt, false, n);
        shuffle_cards(rng.get(), state.military_deck.as_mut_slice());
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
    antiquate_hands(state, ended);
    antiquate_leader_wonder_and_pacts(state, ended);
}

/// The hand half of [`antiquate`], split out so [`antiquate_leader_wonder_and_pacts`]
/// can run on its own -- see that function's own doc for why.
fn antiquate_hands(state: &mut GameState, ended: Age) {
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
    }
}

/// The leader/wonder/pact half of [`antiquate`], split out so the BGO
/// journal replayer can run JUST this half early, ahead of [`antiquate_hands`]
/// and ahead of `advance_age`'s own §12.2.4 yellow-bank deduction and deck
/// rebuild -- see [`antiquate_leader_wonder_pacts_up_to`]'s own doc for why.
/// Hands stay OUT of this split deliberately: they are what the discard-phase
/// hand-size machinery (`interact::discard_options` and friends) reasons
/// about, and running their antiquation ahead of the deferred, fully-timed
/// `advance_age` call risks shifting a discard-phase decision this function
/// has no business touching. A leader/wonder/pact leaving play has no such
/// entanglement: `on_leave_play`'s own token bookkeeping is the only side
/// effect, and it is idempotent here the same way the rest of [`antiquate`]
/// is -- the card is simply gone by the time the deferred, full [`antiquate`]
/// call reaches the same `(state, ended)` pair, so that later call finds
/// nothing left to discard for it.
fn antiquate_leader_wonder_and_pacts(state: &mut GameState, ended: Age) {
    let cutoff = ended as u8;
    for idx in 0..state.num_players as usize {
        let leader = state.players[idx].leader;
        if !leader.is_none() && (leader.get().age as u8) < cutoff {
            // Snapshot/carry-over exactly like `apply::h_play_leader`'s own
            // replacement -- antiquation is a total DECREASE (the leader's
            // own flat CA/MA bonus drops out) exactly like §8.2's "if
            // decreased, return tokens (spent first)", so a flat subtract
            // (this file's `on_leave_play` alone) would wrongly claw back
            // actions the player had already spent from OTHER sources this
            // same turn once the antiquating leader's bonus is smaller than
            // what is left of it -- see `apply::carry_over_action_pool`'s
            // own doc comment for the citation and the leader-replacement
            // precedent (game `7522520`) this mirrors.
            let old_total_c = effects::state_stats(state, &state.players[idx]).civil_actions;
            let old_total_m = effects::state_stats(state, &state.players[idx]).military_actions;
            let old_remaining_c = state.players[idx].civil_actions as i32;
            let old_remaining_m = state.players[idx].military_actions as i32;
            let spent_c = old_total_c - old_remaining_c;
            let spent_m = old_total_m - old_remaining_m;
            on_leave_play(&mut state.players[idx], leader);
            economy::discard_civil(state, leader);
            state.players[idx].leader = CardId::NONE;
            let new_total_c = effects::state_stats(state, &state.players[idx]).civil_actions;
            let new_total_m = effects::state_stats(state, &state.players[idx]).military_actions;
            state.players[idx].civil_actions =
                crate::apply::carry_over_action_pool(old_total_c, new_total_c, spent_c, old_remaining_c);
            state.players[idx].military_actions =
                crate::apply::carry_over_action_pool(old_total_m, new_total_m, spent_m, old_remaining_m);
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

/// Run [`antiquate_leader_wonder_and_pacts`] for every age between
/// `state.age_civil` (exclusive lower bound handled by the loop starting
/// there) and `target` (exclusive), WITHOUT bumping `state.age_civil`,
/// touching `civil_deck`, or running `advance_age`'s §12.2.4 yellow-bank
/// deduction -- the narrow fix for the (-6,+6) War-over-Culture cluster
/// (`docs/REPLAY.md`, Napoleon Bonaparte bucket).
///
/// WHY THIS EXISTS: `combat::resolve_war_outcome` fires synchronously inside
/// `game::start_turn`, itself triggered as a side effect of applying the
/// BGO journal's own DEFENDER-side `EndTurn`/`Discard` line for the OLD
/// age -- there is no journal line boundary between that trailer and the
/// attacker's own synchronous start-of-turn cascade for the replayer to
/// hook into. `replay_common.rs`'s `catch_up_civil_age` cannot fire in time:
/// it is deliberately deferred past `EndTurn`/`Discard`/`WinWar` lines
/// (`is_trustworthy_age_line`'s own doc has the full history, including WHY
/// trusting a `WinWar` line's own age at ITS call site is unsafe -- game
/// `7523079`'s same-timestamp export collision). The result: an Age II
/// leader (Napoleon Bonaparte) that the real BGA game had already
/// antiquated out by the time a War over Culture resolved one age later is
/// still present in this reconstruction, still contributing his strength
/// bonus.
///
/// The fix runs ONLY the antiquation half early, at the point
/// `replay_common.rs`'s main loop detects (via `upcoming_confirmed_winwar_age`)
/// that the very next non-bridge line is a `WinWar` confirmation tagged an
/// age this reconstruction has not caught up to yet, with no older-tagged
/// line immediately following it (the discriminator that rules out the
/// `7523079` collision shape). Everything else -- `state.age_civil` itself,
/// `civil_deck`, `yellow_bank`'s §12.2.4 deduction, hand antiquation -- stays
/// on the EXACT same deferred schedule as before, so none of the existing
/// timing-sensitive tests around `catch_up_civil_age`/`is_trustworthy_age_line`
/// change behaviour. The later, deferred, full `antiquate` call (via the
/// normal `force_civil_age_at_least` path) still runs for the same
/// `(state, ended)` pairs; it is a no-op for whatever this function already
/// removed, since there is nothing left below `cutoff` to find.
pub(crate) fn antiquate_leader_wonder_pacts_up_to(state: &mut GameState, target: Age) {
    let mut ended = state.age_civil;
    while ended < target {
        antiquate_leader_wonder_and_pacts(state, ended);
        let Some(next) = next_age(ended) else { break };
        ended = next;
    }
}

/// The leave-play token bookkeeping, for the one caller [`antiquate`] has.
///
/// Duplicated from `apply.rs`'s private `on_leave_play` rather than shared,
/// exactly as `legal.rs`/`costs.rs`/`apply.rs` each keep their own four-line
/// `leader_is`: making it `pub(crate)` is an edit to a module this port does
/// not own. PLUS the one `Special` this port models here too:
/// `CultureOnLeaveEqualToLabResourceProduction` (Bill Gates) now fires on
/// THIS path as well (antiquation -- a leader too old for the current age is
/// discarded exactly like any other leave-play, so a Bill Gates who ages out
/// rather than being Iconoclasm'd owes the same culture; `apply.rs`'s own
/// twin carries the full citation/trace for the mechanism itself).
///
/// ENGINE BUG FIX (`IllegalMove: Revolution` bucket, game `7522515` round
/// 12): this duplicate used to stop at blue/yellow tokens, silently dropping
/// the `civil_actions`/`military_actions` giveback `apply.rs`'s own
/// `on_leave_play` already has (its own doc comment: "a leader carrying a
/// ... bonus leaving play mid-turn ... must give back the headroom it
/// added"). A leader antiquated out of play (RULES_SPEC line 244, CoL p.3/RB
/// p.21: "an Age I leader dies when Age II ends") is a leave-play exactly
/// like a replacement or Iconoclasm -- there is no rules basis for exempting
/// it from the same symmetric give-back `on_enter_play` grants on the way
/// in. Without this, a flat MA/CA bonus a since-antiquated leader printed
/// (Joan of Arc: +1 MA) survived as a ghost in the player's LIVE per-turn
/// pool forever; the next leader elected into the now-empty slot then had
/// `apply.rs::on_enter_play`'s own `p.military_actions += ma` ADD its bonus
/// ON TOP of that ghost (that branch assumes an empty slot means nothing to
/// net against), overcounting the total by exactly the antiquated leader's
/// bonus. `legal::revolt_pool_ok` requires the live pool to equal a FRESH
/// `effects::state_stats` recompute (which correctly excludes the
/// long-gone leader) before a revolution is legal, so the ghosted extra
/// action permanently failed that check for the rest of the game -- 7522515
/// round 12: Joan of Arc (Age I, +1 MA) antiquates when Age II ends,
/// leaving `military_actions` at a stale 3 instead of 2; Robespierre (+1
/// MA) is elected into the empty slot afterward and `on_enter_play` adds
/// its own +1 on top, landing on 4 where a fresh recompute says 3 -- the
/// exact mismatch this binary's own `try_apply` debug trace showed at the
/// human's real, BGA-accepted "Purple revolutions ... Republic" line.
fn on_leave_play(p: &mut PlayerState, id: CardId) {
    if id.get().special.contains(&Special::CultureOnLeaveEqualToLabResourceProduction) {
        let gained = effects::lab_level_workers(&p.techs);
        p.culture = (p.culture as i32 + gained).max(0) as u16;
    }
    let eff = &id.get().effects;
    let bt = eff.blue_tokens as i32;
    if bt != 0 {
        p.blue_total = (p.blue_total as i32 - bt).max(0) as u8;
    }
    if eff.yellow_tokens != 0 {
        p.yellow_bank = (p.yellow_bank as i32 - eff.yellow_tokens as i32).max(0) as u8;
    }
    let ca = eff.civil_actions as i32;
    if ca != 0 {
        p.civil_actions = (p.civil_actions as i32 - ca).max(0) as i8;
    }
    let ma = eff.military_actions as i32;
    if ma != 0 {
        p.military_actions = (p.military_actions as i32 - ma).max(0) as i8;
    }
}

/// §12.3: Age IV begins -> this round or the next one is the last.
///
/// Which of the two depends on whose turn triggered it: if the player who
/// triggered it is the start player, the round they are in is the last one;
/// otherwise everyone gets to finish the round AND play one more, so that
/// nobody's civilization is scored a turn short of a rival's.
///
/// `pub(crate)`, its only outside caller is `replay_common.rs`'s BGO journal
/// replayer -- `advance_age`'s own call above is normally the only trigger,
/// but the replayer forces the card row to match each observed "takes ... in
/// hand" line directly (`Replayer::ground_row_slot`) rather than drawing
/// through `civil_deck`/`deal`, so its Age III deck can go an entire replayed
/// game without ever emptying even when the real one did -- this rule would
/// then never fire and `state.game_over` could never become detectable on a
/// clean replay. BGO's OWN journal states the same §12.3 fact in-band, in
/// two lines with no leading actor colour ("Last turn Game ends at the end
/// of the starting round", one per surviving player) that `corpus::classify`
/// previously dropped as pure flavour text -- `replay_game` now calls this
/// directly when it sees that line, using its own (by then still accurate)
/// `state.current`/`state.round`/`state.start_player` to run the IDENTICAL
/// formula the engine itself would have, rather than re-deriving or
/// approximating it. This is reading an authoritative fact the journal
/// already states, not changing what the rule computes.
pub(crate) fn set_last_round(state: &mut GameState) {
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

/// Bring `state.age_civil` up to at least `target` (§12.2), running every
/// intervening [`advance_age`] exactly as a real deck-driven transition
/// would -- antiquation, the two-unborn-population deduction, and the deck
/// rebuild all included.
///
/// `pub(crate)`, its only outside caller is `replay_common.rs`'s BGO
/// journal replayer -- the SAME reason [`set_last_round`] exists (see its
/// own doc, directly above): `Replayer::ground_row_slot` forces row
/// identities to match each observed "takes ... in hand" line directly
/// rather than draining `civil_deck` through the ordinary `deal` path, so
/// this binary's own `civil_deck` can go an entire replayed turn -- or
/// several -- without emptying even once the true deck already had, one
/// undercounted refill at a time (a `TakeRow` free take, a `PutBack`
/// client-side undo, ... every place a real draw can happen without this
/// reconstruction popping `civil_deck` in lockstep). The result is
/// `advance_age`'s normal trigger firing LATE relative to the real game --
/// which means `antiquate` firing late too, so a hand can go on holding an
/// Age I card the real human's own hand lost several rounds (and one or
/// two age transitions) earlier, inflating this reconstruction's hand size
/// against a `civil_hand_limit` the real game already had it correctly
/// under. Traced against real games from the `IllegalMove: Take`/
/// `HandFull` bucket (`docs/REPLAY.md`): every rejected `Take` in a
/// 108-game sample held at least one card whose own age was more than one
/// age behind the journal's OWN age column at that exact line -- a card
/// `antiquate` should already have discarded.
///
/// BGO's journal states the true civil age in-band on every single line
/// (column 3, `Line::age`) -- reading that authoritative fact and running
/// the IDENTICAL formula the engine itself would have (exactly
/// `set_last_round`'s own precedent) is correct; approximating the true
/// deck's depletion timing from this reconstruction's own undercounted
/// draws is not. A bounded loop (not a single call): the journal can jump
/// more than one age between two consecutive lines this file actually
/// reads (an entire age with zero of its own cards ever named in a
/// "takes"/"discovers"/... line this parser stops on), and every
/// intervening age's own antiquation must still run, not just the final
/// one's.
pub(crate) fn force_civil_age_at_least(state: &mut GameState, target: Age) {
    let mut rng = rng_for(state);
    while state.age_civil < target {
        let before = state.age_civil;
        advance_age(state, &mut rng);
        if state.age_civil == before {
            break; // already at Age IV, `advance_age` is a no-op -- stop rather than loop forever.
        }
    }
}

// =============================================================== turn loop

/// Start-of-Turn sequence + politics phase entry (§5.0).
///
/// The three start-of-turn steps happen in this order and only from round two
/// on: the row is replenished, a war I declared LAST turn resolves, and a
/// tactic I have been keeping to myself becomes public. Replenishing first
/// matters -- it is what can end an age, and an age change can antiquate a
/// card out of the very tableau the war is about to be scored against.
fn start_turn(state: &mut GameState, rng: &mut LazyRandom) {
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
        p.caesar_second_politics = false;
        p.peeked_event = CardId::NONE;
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
        //
        // There USED to be an `auto_skip_politics` call here that silently
        // jumped straight to `Phase::Actions`, without ever consuming a
        // `Move::PolPass`, whenever `legal::legal_moves` had exactly one
        // option (only ever true in Age IV, where `Resign` is not offered --
        // `legal::politics_moves`'s own §5.11 comment). RULES_SPEC.md §5.0:
        // "In the Politics Phase you may perform AT MOST ONE political
        // action (OR SKIP)" -- skipping is the player's own move, not a
        // phase that can fail to exist. BGO's own journals confirm this: the
        // `IllegalMove: PolPass` bucket (`docs/REPLAY.md`'s 2026-08-14 note)
        // was 54 real human games logging an explicit "<Color> passes
        // Political Phase" at EXACTLY the turn this shortcut had already
        // silently closed -- proof the phase is never actually skipped
        // client-side, only ever explicitly passed. Removed; the phase now
        // always opens and waits for whatever `Move::PolPass` the caller
        // (bot or replay) submits, which is legal.rs's own only option
        // anyway, so self-play's behaviour is unchanged bit-for-bit.
        state.phase = Phase::Politics;
        peek_top_event(state, idx);
    } else {
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
    // Snapshot BEFORE `advance_turn` -- see `GameState::last_end_of_turn_
    // culture`'s own doc. `advance_turn` can run the NEXT player's own
    // `start_turn` synchronously, including a war resolving against (or
    // for) `idx` (§5.7), which would otherwise corrupt this exact value
    // before anything gets a chance to read it as "idx's total right after
    // idx's own turn ended".
    state.last_end_of_turn_culture[idx as usize] = Some(state.players[idx as usize].culture);
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

fn advance_turn(state: &mut GameState, rng: &mut LazyRandom) {
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
    if std::env::var("SCOREDIV_EVENT_DEBUG").is_ok() {
        eprintln!(
            "SCOREDIV_FINISH_GAME round={} current_events_len={} future_events_len={}",
            state.round,
            state.current_events.len(),
            state.future_events.len()
        );
    }
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

    /// ENGINE BUG FIX regression (see `on_leave_play`'s own doc comment for
    /// the full BGO trace, game `7522515` round 12): a leader antiquated out
    /// of play must give back any flat `military_actions`/`civil_actions`
    /// bonus it printed, the same way a mid-turn leader REPLACEMENT already
    /// does (`apply.rs::on_leave_play`) -- there is no rules basis for
    /// exempting antiquation from that symmetric give-back. RULES_SPEC line
    /// 244 (CoL p.3/RB p.21): "an Age I leader dies when Age II ends."
    /// Before this fix, `on_leave_play`'s antiquation-only duplicate in this
    /// file stopped at blue/yellow tokens, so Joan of Arc's printed +1 MA
    /// survived in the live per-turn pool as a permanent ghost.
    #[test]
    fn on_leave_play_gives_back_a_military_action_when_a_leader_antiquates() {
        let mut state = new_game(2, 1);
        state.players[0].leader = named("Joan of Arc"); // Age I, +1 MA
        state.players[0].military_actions = 3; // Despotism's 2 + Joan's +1, unspent
        antiquate(&mut state, Age::II); // Age II just ended -> Age I leaders die
        assert!(state.players[0].leader.is_none(), "Joan of Arc is Age I and must antiquate when Age II ends");
        assert_eq!(state.players[0].military_actions, 2, "her +1 MA must be given back, not left as a ghost");
    }

    /// The overflow that killed a live `climb` run for real: `climb.rs`
    /// folds its generation counter into `state.seed` (a `u64`) via
    /// `u64`-wrapping arithmetic, and once that folded value's magnitude
    /// crosses roughly `i64::MAX / 1_000_003`, `rng_for`'s old `i64`-only
    /// checked-arithmetic chain overflowed and `.expect()`-panicked --
    /// fatal, because the release profile is `panic = "abort"`.
    /// `i64::MAX` itself is exactly representable in `i64` (so the OLD code's
    /// `i64::try_from` step succeeded) but `* 1_000_003` overflows `i64` by
    /// three-plus orders of magnitude, so this seed reproduces the exact
    /// failing shape, not just an out-of-range one. See this test's sibling
    /// below for the even more extreme case, and see `rng_for`'s doc comment
    /// for why `i128` is permanent headroom rather than a bigger version of
    /// the same problem.
    #[test]
    fn rng_for_does_not_panic_when_seed_times_1_000_003_overflows_i64() {
        let s = new_game(2, i64::MAX as u64);
        let _ = rng_for(&s); // must not panic
    }

    /// The full extreme: `state.seed` is a `u64`, so `u64::MAX` is a value a
    /// caller can legitimately hand `new_game`/`rng_for` -- it does not even
    /// fit in an `i64` at all (the OLD code's `i64::try_from` step returned
    /// `None` immediately, before ever reaching the multiply). `i128` holds
    /// it with room to spare.
    #[test]
    fn rng_for_does_not_panic_at_the_largest_possible_u64_seed() {
        let s = new_game(2, u64::MAX);
        let _ = rng_for(&s); // must not panic
    }

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
        let mut rng = LazyRandom::new(1);
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

    /// Simulates §1.10's real precondition for the first replenish: by the
    /// time it fires, each of the `p` players has already taken one card out
    /// of the row (their first turn), so `p` slots are already empty before
    /// the sweep. The rightmost `p` slots are picked (not the leftmost) so
    /// they fall outside the sweep's `0..n` range and don't collide with it
    /// -- see this test's callers for why that makes the arithmetic land on
    /// exactly `p + sweep_count(p)` cards dealt.
    fn simulate_first_turns_taken(s: &mut GameState, p: usize) {
        for i in ROW_SIZE - p..ROW_SIZE {
            s.card_row[i] = CardId::NONE;
        }
    }

    /// §1.10: the first replenish fills from the remaining Age A civil deck
    /// (7 cards after the 13 dealt at setup), NOT the Age I deck the old
    /// code jumped to. Before the fix, `replenish` cleared `civil_deck` and
    /// called `advance_age` FIRST, so every card dealt here was already Age
    /// I; a real 2p game against the CGE digital edition (2026-08-12)
    /// dealt Frugality, Patriotism, Pyramids, Caesar, Stock Pile on this
    /// exact replenish, and Stock Pile/Pyramids/Caesar exist only in Age A.
    ///
    /// Checked by POSITION, not by scanning for card identity: `CardId` is
    /// an index into the shared card table, so two physical copies of the
    /// same printed card (e.g. a yellow action card with `count > 1`) share
    /// one `CardId` -- a plain `.contains()` on a card-value miscounts the
    /// moment the surviving row and the dealt cards happen to share a
    /// duplicated type. Position is unambiguous: `slide` packs survivors to
    /// `card_row[..kept]` in order (`replenish_sweeps_slides_and_refills`
    /// already pins that down), so `deal`'s empties are exactly the trailing
    /// `card_row[kept..]`, and `CardList::pop` (`state.rs`) takes from the
    /// END of the deck -- so the deck's LAST card deals first, into the
    /// leftmost of those trailing slots.
    #[test]
    fn the_first_replenish_deals_the_remaining_age_a_cards_not_age_i_cards() {
        for p in [2usize, 3, 4] {
            let mut s = new_game(p as u8, 5);
            let mut rng = LazyRandom::new(1);
            // The 7 cards left in the Age A deck after setup's 13-card deal.
            let deck_before: Vec<CardId> = s.civil_deck.as_slice().to_vec();
            assert_eq!(deck_before.len(), 7, "{p}p: Age A deck is 20 cards, 13 dealt at setup");

            simulate_first_turns_taken(&mut s, p);
            replenish(&mut s, &mut rng);

            assert!(s.card_row.iter().all(|c| !c.is_none()), "{p}p: row refilled to 13");
            assert!(
                s.card_row.iter().all(|c| c.get().age == Age::A),
                "{p}p: every row card is still Age A right after the first replenish"
            );
            // §1.10's own arithmetic: p players already took one card each,
            // the sweep removes sweep_count(p) more, and the deal must
            // refill both -- 5 at every player count, verified rather than
            // assumed.
            let dealt_count = p + sweep_count(p);
            assert_eq!(dealt_count, 5, "{p}p: player_count + sweep_count is 5 at every count");

            let dealt: Vec<CardId> = s.card_row[ROW_SIZE - dealt_count..].to_vec();
            let expected: Vec<CardId> =
                deck_before[deck_before.len() - dealt_count..].iter().rev().copied().collect();
            assert_eq!(
                dealt, expected,
                "{p}p: the last {dealt_count} row slots are the deck's top cards, in pop order"
            );
        }
    }

    /// §9: the Age A civil deck is 20 cards; 13 go to the row at setup and
    /// the first replenish deals `p + sweep_count(p)` (always 5) more --
    /// leaving exactly 2 that are boxed, unseen, when Age A ends. Derived
    /// positionally (see the previous test's doc comment for why): the two
    /// cards that were at the FRONT of the deck (indices `0..2`, since `pop`
    /// drains from the end) are the ones never reached by the fill.
    #[test]
    fn exactly_two_age_a_civil_cards_never_appear() {
        for p in [2usize, 3, 4] {
            let mut s = new_game(p as u8, 7);
            let mut rng = LazyRandom::new(1);
            let deck_before: Vec<CardId> = s.civil_deck.as_slice().to_vec();

            simulate_first_turns_taken(&mut s, p);
            replenish(&mut s, &mut rng);

            let dealt_count = p + sweep_count(p);
            // Same positional argument as the previous test: `pop` drains
            // the deck from the end, so the cards actually dealt are the
            // deck's LAST `dealt_count`, in reverse. Asserting this (not
            // just the arithmetic `7 - 5 == 2`) is what makes this test
            // depend on the fix rather than on a coincidence -- the old
            // buggy code also dealt 5 cards, just from the wrong (Age I)
            // deck, which would make a bare length check pass for the wrong
            // reason.
            let dealt: Vec<CardId> = s.card_row[ROW_SIZE - dealt_count..].to_vec();
            let expected: Vec<CardId> =
                deck_before[deck_before.len() - dealt_count..].iter().rev().copied().collect();
            assert_eq!(dealt, expected, "{p}p: the dealt cards are the Age A deck's own top cards");

            let boxed = &deck_before[..deck_before.len() - dealt_count];
            assert_eq!(
                boxed.len(),
                2,
                "{p}p: exactly 2 of the 7 remaining Age A cards are boxed, unseen"
            );
        }
    }

    /// §1.10's last sentence: no antiquation, no yellow-token loss at the
    /// A -> I transition, unlike every later age change. Regression guard
    /// for the fix to `replenish`: the new code explicitly calls
    /// `advance_age` with `state.age_civil` still `A`, so `ended != Age::A`
    /// must still be false here and the two side effects must still be
    /// skipped.
    #[test]
    fn the_age_a_to_age_i_transition_skips_antiquation_and_yellow_token_loss() {
        let mut s = new_game(2, 5);
        let mut rng = LazyRandom::new(1);
        let age_a_card = named("Bronze");
        s.players[0].hand_civil.push(age_a_card);

        replenish(&mut s, &mut rng);

        assert_eq!(s.age_civil, Age::I, "§1.10: the first replenish ends Age A");
        assert!(
            s.players[0].hand_civil.as_slice().contains(&age_a_card),
            "no antiquation at the A -> I transition"
        );
        assert!(
            !s.civil_removed[Age::A as usize].contains(age_a_card),
            "nothing culled, so nothing recorded as culled"
        );
        assert_eq!(s.players[0].yellow_bank, 18, "§12.2.4: no yellow-token loss at A -> I");
        assert_eq!(s.players[1].yellow_bank, 18);
    }

    /// §2.2: the age ends the moment its last card is DEALT, and the rest of
    /// the row comes out of the new age in the same deal.
    #[test]
    fn an_exhausted_deck_advances_the_age_mid_deal() {
        let mut s = new_game(2, 3);
        let mut rng = LazyRandom::new(1);
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

    /// `deal_row` builds its own [`LazyRandom`] from `state.seed`/`state.
    /// turn`/`state.round` on every call (`rng_for`'s doc comment: this is
    /// the function that used to pay MT19937's full init even when the deck
    /// never emptied). Deferred construction must not change the result:
    /// two clones of the same position, put through an age-boundary refill
    /// that DOES draw from the stream, must land on the identical row and
    /// deck order every time.
    #[test]
    fn deal_row_determinizes_identically_from_the_same_state_twice() {
        let build = || {
            let mut s = new_game(2, 9);
            s.card_row = [CardId::NONE; ROW_SIZE];
            while s.civil_deck.len() > 1 {
                s.civil_deck.pop();
            }
            s
        };
        let mut a = build();
        let mut b = build();
        deal_row(&mut a);
        deal_row(&mut b);
        assert_eq!(a.card_row, b.card_row, "same state must refill the row identically");
        assert_eq!(
            a.civil_deck.as_slice(),
            b.civil_deck.as_slice(),
            "same state must leave the deck in the identical order"
        );
    }

    /// A different game seed must reach a different determinization once
    /// the deck actually reshuffles -- otherwise `deal_row`'s per-call
    /// stream would not be sampling anything, just replaying one fixed
    /// order under a different name.
    #[test]
    fn deal_row_determinizes_differently_for_a_different_seed() {
        let build = |seed: u64| {
            let mut s = new_game(2, seed);
            s.card_row = [CardId::NONE; ROW_SIZE];
            while s.civil_deck.len() > 1 {
                s.civil_deck.pop();
            }
            s
        };
        let mut a = build(9);
        let mut b = build(10);
        deal_row(&mut a);
        deal_row(&mut b);
        assert_ne!(
            a.civil_deck.as_slice(),
            b.civil_deck.as_slice(),
            "different seeds must not reshuffle the new age's deck into the same order"
        );
    }

    /// §12.2: an age change culls older cards from hands, and §12.2.4 takes
    /// two unborn population from every supply.
    #[test]
    fn advancing_out_of_age_i_antiquates_and_taxes_the_bank() {
        let mut s = new_game(2, 11);
        let mut rng = LazyRandom::new(1);
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

    /// The bug `force_civil_age_at_least` exists to close (`docs/REPLAY.md`'s
    /// `HandFull` handoff): this reconstruction's own `civil_deck` can lag
    /// the true deck's depletion (`Replayer::ground_row_slot` forces row
    /// identities directly rather than draining `civil_deck` through the
    /// ordinary `deal` path in lockstep with every real draw), so
    /// `advance_age`'s normal civil_deck-empty trigger -- and with it
    /// `antiquate`'s hand cull -- can fire late relative to what the
    /// journal's own age column already proves happened. Catching the age
    /// up from the journal directly must run the SAME antiquation a real
    /// deck-driven transition would, not skip it: an Age A card sitting in
    /// hand while the journal has already reached Age III must be culled,
    /// exactly as if two ordinary `advance_age` calls had fired in
    /// sequence, not just the age counter moved.
    #[test]
    fn force_civil_age_at_least_antiquates_every_intervening_age_not_just_the_final_one() {
        let mut s = new_game(2, 11);
        s.age_civil = Age::A;
        let age_a_card = named("Bronze");
        let age_ii_card = named("Coal");
        s.players[0].hand_civil.push(age_a_card);
        s.players[0].hand_civil.push(age_ii_card);

        force_civil_age_at_least(&mut s, Age::III);

        assert_eq!(s.age_civil, Age::III, "catches up through every intervening age, not just one step");
        assert_eq!(
            s.players[0].hand_civil.as_slice(),
            &[age_ii_card],
            "the Age A card is culled (it is older than EVERY age that ended on the way to III); \
             the Age II card survives (never older than the age that just ended)"
        );
        assert!(s.civil_removed[Age::A as usize].contains(age_a_card), "the cull is recorded, same as an ordinary advance_age");
    }

    /// The (-6,+6) War-over-Culture cluster's own fix: an Age II leader
    /// (Napoleon Bonaparte) must leave play as soon as this function is
    /// asked to catch antiquation up to Age IV, WITHOUT touching
    /// `age_civil`, `civil_deck`, or the §12.2.4 yellow-bank deduction --
    /// those stay on the replayer's normal, deferred schedule. Reverting
    /// the fix (having `replay_common.rs` call nothing here) leaves
    /// Napoleon in play, which is the exact bug: his `StrengthPerUnitType`
    /// bonus still contributes to a war strength BGA's own game had already
    /// stopped crediting.
    #[test]
    fn antiquate_leader_wonder_pacts_up_to_discards_a_too_old_leader_without_moving_the_age() {
        let mut s = new_game(2, 11);
        s.age_civil = Age::III;
        s.players[0].leader = named("Napoleon Bonaparte");
        let yellow_before = s.players[0].yellow_bank;
        let deck_len_before = s.civil_deck.len();

        antiquate_leader_wonder_pacts_up_to(&mut s, Age::IV);

        assert!(s.players[0].leader.is_none(), "an Age II leader is more than one age behind once Age III ends");
        assert_eq!(s.age_civil, Age::III, "the age itself must not move -- that stays on the deferred schedule");
        assert_eq!(s.players[0].yellow_bank, yellow_before, "§12.2.4's deduction must not fire early");
        assert_eq!(s.civil_deck.len(), deck_len_before, "the deck rebuild must not fire early");
    }

    /// A leader still within one age of the age that just ended survives --
    /// this function must not over-cull just because it is being asked to
    /// run ahead of the normal schedule. Napoleon (Age II) is exactly one
    /// age behind Age II ending (cutoff 2, leader age 2 is not < 2), so
    /// catching antiquation up from II to III must leave him in play.
    #[test]
    fn antiquate_leader_wonder_pacts_up_to_keeps_a_leader_still_within_one_age() {
        let mut s = new_game(2, 11);
        s.age_civil = Age::II;
        let leader = named("Napoleon Bonaparte");
        s.players[0].leader = leader;

        antiquate_leader_wonder_pacts_up_to(&mut s, Age::III);

        assert_eq!(s.players[0].leader, leader, "one age behind is not YET antiquated -- CoL only discards MORE than one age behind");
        assert_eq!(s.age_civil, Age::II, "the age itself still must not move");
    }

    /// Running the early half must not double-discard once the deferred,
    /// full `antiquate` (via `force_civil_age_at_least`) later reaches the
    /// SAME `(state, ended)` pair -- the leader is simply already gone, so
    /// the later call is a no-op for it, exactly like re-running `antiquate`
    /// on an already-caught-up age is a no-op elsewhere in this module.
    #[test]
    fn antiquate_leader_wonder_pacts_up_to_is_idempotent_with_the_later_deferred_antiquate() {
        let mut s = new_game(2, 11);
        s.age_civil = Age::III;
        s.players[0].leader = named("Napoleon Bonaparte");

        antiquate_leader_wonder_pacts_up_to(&mut s, Age::IV);
        assert!(s.players[0].leader.is_none());

        force_civil_age_at_least(&mut s, Age::IV);
        assert_eq!(s.age_civil, Age::IV, "the deferred call still moves the age itself");
        assert!(s.players[0].leader.is_none(), "still gone -- the deferred antiquate found nothing left to discard");
    }

    /// A no-op when this reconstruction's own age already matches or leads
    /// the journal's -- the overwhelmingly common case (every line but the
    /// first one of a new age). Must not re-run antiquation (which would
    /// wrongly cull a card taken AFTER the transition already happened) or
    /// panic on an already-current age.
    #[test]
    fn force_civil_age_at_least_is_a_no_op_when_already_caught_up() {
        let mut s = new_game(2, 11);
        s.age_civil = Age::II;
        let card = named("Coal");
        s.players[0].hand_civil.push(card);

        force_civil_age_at_least(&mut s, Age::II);
        assert_eq!(s.age_civil, Age::II);
        assert_eq!(s.players[0].hand_civil.as_slice(), &[card], "nothing culled by a no-op catch-up");

        force_civil_age_at_least(&mut s, Age::I); // even a LOWER target: never runs backwards
        assert_eq!(s.age_civil, Age::II, "age never moves backwards");
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
        let mut rng = LazyRandom::new(1);
        assert_eq!((s.round, s.current), (1, 0));
        advance_turn(&mut s, &mut rng);
        assert_eq!((s.round, s.current), (1, 1));
        advance_turn(&mut s, &mut rng);
        assert_eq!((s.round, s.current), (1, 2));
        advance_turn(&mut s, &mut rng);
        assert_eq!((s.round, s.current), (2, 0));
        assert_eq!(s.turn, 4);
    }

    /// Age IV, empty military hand: `legal::legal_moves` returns exactly one
    /// move (`Move::PolPass` -- `Resign` is not offered in Age IV,
    /// `legal::politics_moves`'s own §5.11 comment). This USED to be exactly
    /// when the deleted `auto_skip_politics` fired, jumping straight to
    /// `Phase::Actions` without ever letting a real `Move::PolPass` land --
    /// which is what made 54 real BGO games' own logged "<Color> passes
    /// Political Phase" illegal against this engine (`docs/REPLAY.md`,
    /// 2026-08-14, the `IllegalMove: PolPass` bucket). RULES_SPEC.md §5.0:
    /// the Politics Phase always lets the player "perform AT MOST ONE
    /// political action (OR SKIP)" -- skipping is the player's own move, so
    /// the phase must stay open and wait for it, never vanish underneath
    /// them.
    #[test]
    fn start_turn_leaves_the_politics_phase_open_in_age_iv_even_when_passing_is_the_only_option() {
        let mut s = new_game(2, 7);
        s.age_civil = Age::IV;
        s.age_military = Age::IV;
        s.round = 2;
        s.civil_deck = CardList::new();
        s.military_deck = CardList::new();
        s.players[0].hand_military = CardList::new();
        let mut rng = LazyRandom::new(1);
        start_turn(&mut s, &mut rng);
        assert_eq!(s.phase, Phase::Politics, "the Politics Phase must stay open, not be silently skipped");
        let moves = legal::legal_moves(&s);
        assert_eq!(moves.as_slice().len(), 1, "passing must be the only legal move here");
        assert_eq!(moves.as_slice()[0], Move::PolPass);
        assert!(
            !s.players[0].politics_done,
            "politics_done must not be set until an actual PolPass move lands"
        );
    }

    /// The game must end when the round counter passes the deadline §12.3
    /// set, even though nothing else is exhausted.
    #[test]
    fn passing_the_final_round_ends_the_game() {
        let mut s = new_game(2, 4);
        let mut rng = LazyRandom::new(1);
        s.final_round_end = Some(1);
        s.current = 1; // next advance wraps into round 2
        advance_turn(&mut s, &mut rng);
        assert!(s.game_over);
        assert_eq!(s.phase, Phase::Done);
    }
}
