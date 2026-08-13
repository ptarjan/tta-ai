//! Card counting: what a human at the table could work out about the decks.
//!
//! Ports `engine/bots/counting.py` (235 lines) -- read that file's own module
//! doc comment first; it is the design rationale for everything below and is
//! restated here only where the Rust shape earns its own note. Short version,
//! in the project owner's own words: *"Card counting is legal. All public
//! info can be used."* Three things pass the test of "could a human at the
//! table with the physical 2015 base game arrive at this" and are used below:
//!
//! * **Deck COMPOSITION is public.** The card counts per player number are
//!   printed in the rulebook. [`crate::game::build_deck`] returns exactly
//!   that list.
//! * **Deck HEIGHT is public.** `state.civil_deck.len()` is how many cards
//!   are left in the stack on the table. Anyone can count a stack.
//! * **Everything already seen is public.** The card row is face up,
//!   tableaux are face up, `state.civil_discard`/`state.civil_removed` are
//!   face-up piles beside the board, and a player's own hand is theirs.
//!
//! Deck ORDER and deck CONTENTS are not public, and nothing here reads
//! either: this module never iterates `state.civil_deck`/`state.military_deck`,
//! only ever takes their `len()`.
//!
//! ## One deliberate shape change from the Python
//!
//! **Composition comes from the SAME function that deals the game, not a
//! second one.** Python's `Db.civil_deck`/`Db.military_deck` are a separate
//! implementation of "what's in this age's deck" from `engine.game._deal`'s
//! actual dealing -- `tests/test_counting.py::TheDeckIsFullyDealt` exists
//! specifically to check the two never drift apart on real positions.
//! [`crate::game::build_deck`] (made `pub(crate)` for this module) IS the
//! function `game::new_game`/`game::deal`/`game::advance_age` call to build
//! every real deck, so this port has that guarantee by construction rather
//! than by a property test walking self-play positions -- the "present in
//! this registry, absent from that one" bug class DESIGN.md names is closed
//! here, not merely tested for.
//!
//! ## A registry gap this port had to close, not merely port around
//!
//! `event_pool` needs `state.seeded_by` (which player prepared which
//! face-down politics card) -- Python's `state.seeded_by`. **`state.rs` had
//! no field for it before this port**: `apply.rs::h_prepare_event`'s doc
//! comment used to record this in so many words ("this port has no bot layer
//! and `state.rs` -- another worker's file -- has no field for it"), and
//! `fixtures.rs::GameState::from_json` silently dropped the dumped fixtures'
//! real `seeded_by` data for the same reason. Both are fixed now:
//! `state.rs::GameState::seeded_by` is a real `[u8; NUM_CARDS]` field (dense,
//! `state::NOT_SEEDED` = "nobody", exactly `Tableau::index`'s sparse-array
//! idiom), `apply.rs::h_prepare_event` is its one writer, and
//! `fixtures.rs::from_json`/`GameState::diff` both read/compare it now. This
//! was not optional: without it, `event_pool` could only ever report `(∅,
//! 0.0)` on every loaded fixture, which is not a smaller version of the
//! function, it is a different, wrong one that happens to type-check.
//!
//! ## Representation notes
//!
//! `civil_outlook`/`event_pool` return `Vec<(CardId, f64)>`/
//! `Vec<(CardId, u16)>` rather than a `HashMap<CardId, _>` -- short-lived
//! scratch values, at most a few dozen entries (one age's civil deck, or one-
//! to-two ages' event cards), consumed by a linear scan ([`lookup`]) exactly
//! the way `board_yields.rs::merge` treats its own short triple lists. A
//! `HashMap` would cost an allocation and a hasher for a handful of entries
//! nothing here is ever going to grow past.
//!
//! ## CALLERS MUST ROOT-CACHE
//!
//! Every function here reads a zone that a trial `apply` mutates -- dealing
//! refills the row from the deck, revealing an event moves a face-down card
//! into `past_events`. Recomputing on a searched state would let a deeper
//! search *see the card it just drew*. A future `weighted.rs` port must
//! compute these once at the search root and thread them down, exactly as
//! `engine/bots/weighted.py::rival_context` does today.

use crate::cards::{Age, CardId, CardType, NUM_CARDS};
use crate::game;
use crate::state::{CardList, GameState, MAX_DECK};

/// Every age, oldest first. Mirrors `cards.AGES`; `Age::A as u8 ..= Age::IV
/// as u8` is the same sequence, this just spells it as values instead of a
/// range so callers can slice from an arbitrary starting age.
const ALL_AGES: [Age; 5] = [Age::A, Age::I, Age::II, Age::III, Age::IV];

/// `pairs.get(id, default)` -- the read side of every `Vec<(CardId, _)>`
/// this module returns. See this module's top doc comment for why a linear
/// scan over a short `Vec` is the right trade-off here.
pub fn lookup<T: Copy>(pairs: &[(CardId, T)], id: CardId, default: T) -> T {
    pairs.iter().find(|&&(c, _)| c == id).map_or(default, |&(_, v)| v)
}

/// `_live_count`: players who have not resigned. RULES_SPEC 13 trims the
/// future decks to this, so it is the right `n` for a composition lookup.
fn live_count(state: &GameState) -> usize {
    state.players[..state.num_players as usize].iter().filter(|q| !q.resigned).count()
}

/// `game::build_deck`, guarded for `n < 2`. Python's `_live_count` is not
/// clamped either, and `Db._deck`'s `count.get(f"{n}p", 0)` simply returns
/// nothing for an `"1p"` key the printed data never has -- an empty
/// composition, not a crash. `game::build_deck` instead computes `num_players
/// - 2` to index `Card::count`, which underflows for `n < 2`, so this wrapper
/// is the guard that keeps this module's behaviour identical to Python's for
/// the same (rare: a forced-win endgame with one player left) input, rather
/// than inheriting a panic that only a subtraction away.
fn deck_or_empty(age: Age, civil: bool, n: usize) -> CardList<MAX_DECK> {
    if !(2..=4).contains(&n) {
        return CardList::new();
    }
    game::build_deck(age, civil, n)
}

/// Copies of each card in `deck`, dense by [`CardId`]. `game::build_deck`
/// already returns one entry per physical copy (mirroring the shuffle-ready
/// list `engine/cards.py::_deck` builds), so this only tallies duplicates --
/// exactly `collections.Counter(...)` on Python's list.
fn tally(deck: &CardList<MAX_DECK>) -> [u16; NUM_CARDS] {
    let mut counts = [0u16; NUM_CARDS];
    for &id in deck.as_slice() {
        counts[id.0 as usize] += 1;
    }
    counts
}

/// "Homer" the leader card, resolved by name once per call -- an I/O-
/// adjacent fixed identity (mirrors `game.rs::named`'s setup-only card
/// lookups), not a per-turn hot path: this module's own top doc comment
/// requires callers to root-cache everything here, so this runs once per
/// search, not once per candidate.
///
/// # Panics
/// If `card_table.rs` has no card named "Homer" -- would mean the base game's
/// card data changed shape, the same failure mode `game.rs::named` has.
fn homer() -> CardId {
    CardId::by_name("Homer").unwrap_or_else(|| panic!("card_table.rs has no card named \"Homer\""))
}

/// `live_ages`: the ages whose cards can still be in somebody's hand or in a
/// deck.
///
/// `game::antiquate` discards every card of level BELOW the level of the age
/// that just ended, from hands and from the common area alike (RULES_SPEC
/// 12.2). So when age `a` is current, the cards still in play are those of
/// level `>= level(a) - 1`: the current age, and the one before it that
/// players are allowed to keep holding.
///
/// Returned oldest first. Age `IV` has no decks at all (`game::advance_age`
/// empties both), so it contributes nothing and is not special-cased here --
/// the composition lookups it feeds simply come back empty.
pub fn live_ages(state: &GameState) -> Vec<Age> {
    let lv = state.age_civil as i8;
    ALL_AGES.iter().copied().filter(|&a| lv - 1 <= a as i8 && a as i8 <= lv).collect()
}

/// `_seen_civil`: face-up civil cards of the ages `keep` accepts, plus my own
/// hand, dense by [`CardId`].
///
/// Every zone read here is one a human can point at. Rival hands are NOT in
/// here, deliberately -- their SIZE is public and their contents are not, and
/// the whole point of [`civil_outlook`] is to reason about that gap rather
/// than to look through it.
fn seen_civil(state: &GameState, idx: u8, keep: impl Fn(Age) -> bool) -> [u16; NUM_CARDS] {
    let mut seen = [0u16; NUM_CARDS];
    for &id in state.card_row.iter() {
        if !id.is_none() && keep(id.get().age) {
            seen[id.0 as usize] += 1;
        }
    }
    for age in ALL_AGES {
        if !keep(age) {
            continue;
        }
        // Both discard records: swept off the row, and left play from a hand
        // or a board. Separate fields for the encoder's sake (`state.rs`'s
        // own doc comment on `civil_discard`/`civil_removed`); the count
        // wants the union of them.
        for &id in state.civil_discard[age as usize].as_slice() {
            seen[id.0 as usize] += 1;
        }
        for &id in state.civil_removed[age as usize].as_slice() {
            seen[id.0 as usize] += 1;
        }
    }
    for q in &state.players[..state.num_players as usize] {
        // Every civil card a player has put on the table, in every zone it
        // can sit in. Missing ONE of these zones does not fail loudly -- it
        // just makes the subtraction read the shortfall as "still in a
        // rival's hand", which is exactly how the leader and wonder zones
        // were found in the Python port.
        for (id, _slot) in q.techs.iter() {
            if keep(id.get().age) {
                seen[id.0 as usize] += 1;
            }
        }
        if !q.government.is_none() && keep(q.government.get().age) {
            seen[q.government.0 as usize] += 1;
        }
        for &w in q.completed_wonders.as_slice() {
            if keep(w.get().age) {
                seen[w.0 as usize] += 1;
            }
        }
        if !q.wonder.is_none() && keep(q.wonder.get().age) {
            seen[q.wonder.0 as usize] += 1;
        }
        if !q.leader.is_none() && keep(q.leader.get().age) {
            seen[q.leader.0 as usize] += 1;
        }
        if !q.homer_wonder.is_none() {
            // Tucked under a wonder, still face up -- and it is "Homer" the
            // leader that is visible, not the wonder he is tucked under
            // (that wonder is already counted via `completed_wonders` above).
            let h = homer();
            if keep(h.get().age) {
                seen[h.0 as usize] += 1;
            }
        }
        if q.idx == idx {
            // My hand: mine to read.
            for &id in q.hand_civil.as_slice() {
                if keep(id.get().age) {
                    seen[id.0 as usize] += 1;
                }
            }
        }
    }
    seen
}

/// `civil_outlook`: `CardId -> EXPECTED further copies that will be dealt
/// into the row.`
///
/// This is the number the project owner asked for in the audit, in his own
/// example: *"know that a second selective breeding is near the end or not
/// and it has to build another farm since it won't see it"*. A card with an
/// outlook of `0.0` is one that will never appear again -- if a player wants
/// it, the copy in the row is the last one, and passing it up is permanent.
///
/// Three cases, and only one of them is an estimate:
///
/// * **A future age** -- the whole deck is still to come and, by the
///   exhaustion property `game::deal`'s own doc comment establishes (an
///   age's civil deck is always fully dealt before the age turns over),
///   every card of it *will* be dealt. Outlook is the printed count,
///   exactly.
/// * **An age already past** -- its deck was fully dealt, so nothing more is
///   coming. Not even present in the returned list (Python's dict omits it
///   too -- `ages` only ever runs from the current age forward).
/// * **The current age** -- `unaccounted` copies are those the printed count
///   says exist and that this player has not seen in the row, a tableau, the
///   discard or their own hand. Each is either still in the deck or in a
///   rival's hand, and the deck stack can be counted, so
///
///   ```text
///   outlook(X) = unaccounted(X) * len(deck) / total_unaccounted
///   ```
///
///   with no fitted constant anywhere in it. The multiplier is the fraction
///   of the unknown cards that are in the deck rather than in a hand, and
///   both ends of that ratio are things a human can count.
///
/// Uniformity across names is the one modelling assumption, and it is the
/// assumption-light one: a rival holding a card is *more* likely to be
/// holding a good one, so this slightly over-estimates the outlook of the
/// strongest cards. See `engine/bots/counting.py::civil_outlook`'s own doc
/// comment for why tightening that is tracked as a separate, harder problem.
pub fn civil_outlook(state: &GameState, idx: u8) -> Vec<(CardId, f64)> {
    let n = live_count(state);
    let cur = state.age_civil;
    let mut out: Vec<(CardId, f64)> = Vec::new();
    for &age in ALL_AGES.iter().filter(|&&a| a as u8 >= cur as u8) {
        let comp = tally(&deck_or_empty(age, true, n));
        if age != cur {
            // The whole deck is still to come.
            for i in 0..NUM_CARDS {
                if comp[i] > 0 {
                    out.push((CardId(i as u16), comp[i] as f64));
                }
            }
            continue;
        }
        let seen = seen_civil(state, idx, |a| a == age);
        let mut total: i64 = 0;
        let mut unaccounted = [0i32; NUM_CARDS];
        for i in 0..NUM_CARDS {
            if comp[i] == 0 {
                continue;
            }
            let left = comp[i] as i32 - seen[i] as i32;
            unaccounted[i] = left;
            if left > 0 {
                total += left as i64;
            }
        }
        if total <= 0 {
            for i in 0..NUM_CARDS {
                if comp[i] > 0 {
                    out.push((CardId(i as u16), 0.0));
                }
            }
            continue;
        }
        let share = state.civil_deck.len() as f64 / total as f64;
        for i in 0..NUM_CARDS {
            if comp[i] == 0 {
                continue;
            }
            out.push((CardId(i as u16), unaccounted[i].max(0) as f64 * share));
        }
    }
    out
}

/// `event_pool`: what could be in the face-down politics pile that this
/// player did not put there.
///
/// Returns `(unknown, p_in_pile)`: copies whose location is not known, and
/// the probability that any one such copy is sitting in one of the pile
/// slots this player did not seed.
///
/// THIS IS THE FUNCTION THAT REPLACES A LEAK in the Python original --
/// `events.pending_final_events` used to be walked by name, reading events
/// opponents placed face down. What a human really knows is:
///
/// * exactly which events *they* prepared, because they put them there
///   (`state.seeded_by`, see this module's top doc comment for why that
///   field had to be added to `state.rs` for this port to be correct);
/// * how many face-down cards are in the pile, because it is a stack;
/// * which events have already been revealed (`past_events`) and which were
///   discarded (`discarded_military`), because both happened in the open;
/// * the printed composition of each age's event deck.
///
/// From those, a copy whose location is unknown is in the draw deck, in an
/// opponent's hand, or in the pile, and the pile holds `k` of them out of
/// `total` unknown copies, so `p_in_pile = k / total`. Uniform over names for
/// the same reason [`civil_outlook`] is: modelling what an opponent *wants*
/// to seed is a harder, separate problem.
///
/// `p_in_pile` is only ever a probability over cards this player has NOT
/// seen -- their own seeds are certain, and a future caller building the
/// full picture (`weighted.py::my_seeds`, not yet ported) adds the two.
pub fn event_pool(state: &GameState, idx: u8) -> (Vec<(CardId, u16)>, f64) {
    let n = live_count(state);
    let ages = live_ages(state);
    let mut known = [0u16; NUM_CARDS];
    for &id in state.past_events.as_slice() {
        // A position restored from a file written before past events were saved
        // by name knows only how many resolved, not which. Counting an unknown
        // as "seen" would be indexing by a sentinel; leaving it out only makes
        // us slightly more pessimistic about what is still in the deck.
        if id.is_none() {
            continue;
        }
        known[id.0 as usize] += 1;
    }
    for &age in &ages {
        for &id in state.discarded_military[age as usize].as_slice() {
            known[id.0 as usize] += 1;
        }
    }
    let me = &state.players[idx as usize];
    for &id in me.hand_military.as_slice() {
        known[id.0 as usize] += 1;
    }
    let mut mine_in_pile: u32 = 0;
    for &id in state.current_events.as_slice().iter().chain(state.future_events.as_slice()) {
        if state.seeded_by_player(id, idx) {
            known[id.0 as usize] += 1; // I put it there
            mine_in_pile += 1;
        }
    }

    let mut unknown: Vec<(CardId, u16)> = Vec::new();
    let mut total: i64 = 0;
    for &age in &ages {
        let counts = tally(&deck_or_empty(age, false, n));
        for i in 0..NUM_CARDS {
            if counts[i] == 0 || CardId(i as u16).kind() != CardType::Event {
                continue;
            }
            let left = counts[i] as i32 - known[i] as i32;
            if left > 0 {
                unknown.push((CardId(i as u16), left as u16));
                total += left as i64;
            }
        }
    }
    let k = state.current_events.len() as i64 + state.future_events.len() as i64 - mine_in_pile as i64;
    if total <= 0 || k <= 0 {
        return (unknown, 0.0);
    }
    (unknown, (k as f64 / total as f64).min(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game;
    use crate::state::NOT_SEEDED;

    /// A guard against `NOT_SEEDED` accidentally colliding with a real
    /// player index -- `event_pool`'s `== idx` compares against it directly,
    /// so if this ever failed every seeded-event check would silently start
    /// misclassifying player 255 (which cannot exist, `MAX_PLAYERS == 4`) as
    /// the seeder of everything nobody seeded.
    #[test]
    fn not_seeded_is_not_a_real_player_index() {
        assert!(NOT_SEEDED as usize >= crate::state::MAX_PLAYERS);
    }

    /// `live_ages` at the very start of the game: only Age A is current, and
    /// there is nothing "before" it, so exactly one age comes back.
    #[test]
    fn live_ages_at_game_start_is_just_age_a() {
        let state = game::new_game(3, 1);
        assert_eq!(live_ages(&state), vec![Age::A]);
    }

    /// `civil_outlook` at the very start of the game prices every age from
    /// the current one (`A`) forward -- `ages = [a for a in AGES if
    /// level(a) >= level(cur)]` in Python is *every* age when `cur` is `A`,
    /// not just `A` itself. A FUTURE age's whole deck is certain (see this
    /// function's own doc comment: "Outlook is the printed count, exactly"),
    /// so this pins that against `game::build_deck` directly for Age I --
    /// the same function `civil_outlook` itself calls, so this is a
    /// self-consistency check on the "future age" branch, not a duplicate of
    /// it. Age IV never appears: its civil deck is empty in the base game
    /// (verified 2026-08-05 against `engine.cards.Db.civil_deck("IV", 3)`
    /// returning `[]`), so nothing is printed to price.
    #[test]
    fn civil_outlook_at_game_start_prices_every_age_but_iv() {
        let state = game::new_game(3, 1);
        let out = civil_outlook(&state, 0);
        assert!(!out.is_empty());
        for &(id, v) in &out {
            assert_ne!(id.get().age, Age::IV, "Age IV has no printed civil deck");
            assert!(v >= 0.0, "{:?}: outlook {v} is negative", id);
        }
        let age_1_deck = game::build_deck(Age::I, true, 3);
        let sample = age_1_deck.as_slice()[0];
        let printed = age_1_deck.as_slice().iter().filter(|&&c| c == sample).count() as f64;
        assert_eq!(
            lookup(&out, sample, -1.0),
            printed,
            "a FUTURE age's outlook must be exactly its printed count, not an estimate"
        );
    }

    /// Honesty: reordering the draw deck must not move a single outlook
    /// number -- this module reads only `len()`, never iterates the deck.
    #[test]
    fn reordering_the_civil_deck_changes_nothing() {
        let mut state = game::new_game(3, 1);
        let before = civil_outlook(&state, 0);
        state.civil_deck.as_mut_slice().reverse();
        let after = civil_outlook(&state, 0);
        assert_eq!(before, after, "civil_outlook moved when only the deck ORDER changed");
    }

    /// Honesty, the other side: MY hand must be visible to me. Give player 0
    /// a specific Age A card and confirm the printed count minus one copy
    /// (now in hand) is reflected for the current age.
    #[test]
    fn my_own_hand_is_visible_to_me() {
        let mut state = game::new_game(3, 1);
        // Farming (Age A) is guaranteed to exist and be developable this
        // early -- pull one copy out of the row (or the deck, deterministically
        // via civil_deck) into player 0's hand, then confirm the outlook
        // dropped relative to a state that never got the card.
        let target = state.card_row.iter().copied().find(|c| !c.is_none()).expect("row is dealt");
        let before = lookup(&civil_outlook(&state, 0), target, -1.0);
        assert!(before >= 0.0, "target card should already have an outlook entry");
        // Move it from the row into player 0's hand -- exactly what `take_card`
        // does, done directly here since this test only needs the state shape,
        // not a legal move.
        let slot = state.card_row.iter().position(|&c| c == target).unwrap();
        state.card_row[slot] = CardId::NONE;
        state.players[0].hand_civil.push(target);
        let after = lookup(&civil_outlook(&state, 0), target, -1.0);
        assert!(after >= 0.0);
        assert!(after <= before, "moving a copy into MY hand must not raise its own outlook");
    }
}
