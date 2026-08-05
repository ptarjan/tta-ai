//! Through the Ages (base game, 2015 "A New Story of Civilization") engine.
//!
//! A native Rust rewrite of the Python engine in `../engine`, not a
//! transliteration of it -- see `DESIGN.md` for the five rules every module
//! here follows and for why the Python source is the spec for the *rules* and
//! never for the *representation*.
//!
//! Scope is the base game only. The expansion is deliberately out of scope.

#![forbid(unsafe_code)]
// The port is incremental and the type layer lands before the module bodies
// that consume it; without this the crate is a wall of warnings that hides the
// real ones. Delete this line once `effects` and `actions` are ported.
#![allow(dead_code)]

pub mod card_table;
pub mod cards;
pub mod moves;
pub mod state;

pub use cards::{Age, Card, CardEffects, CardId, CardType, Special, CARDS, NUM_CARDS};
pub use moves::{Move, MoveList};
pub use state::{GameState, PlayerState, Phase, Tableau, TechSlot};

#[cfg(test)]
mod tests {
    use super::*;

    /// The card table and the Python engine must agree on the card list. This
    /// is the project's recurring bug class -- a name in one registry and not
    /// the other, with nothing that fails when they disagree -- so it gets a
    /// test on the Rust side too, not just a generator that happened to run.
    #[test]
    fn the_table_has_every_base_game_card() {
        assert_eq!(NUM_CARDS, 236);
        assert_eq!(CARDS.len(), NUM_CARDS);
    }

    #[test]
    fn every_card_has_a_known_type_and_age() {
        for c in CARDS.iter() {
            assert!(!c.name.is_empty());
            // A card that can hold workers must be buildable; one that cannot,
            // must not carry a build cost it can never pay off.
            if c.kind.takes_workers() {
                assert!(
                    c.resource_cost > 0 || c.age == Age::A,
                    "{} takes workers but costs nothing to build",
                    c.name
                );
            }
        }
    }

    /// Deck sizes are the bound `state::MAX_DECK` is chosen against. If the
    /// card data grows, this fails before an array does.
    #[test]
    fn no_age_deck_exceeds_the_array_bound() {
        for n in 0..3usize {
            for age in [Age::A, Age::I, Age::II, Age::III, Age::IV] {
                for deck_is_civil in [true, false] {
                    let total: usize = CARDS
                        .iter()
                        .filter(|c| c.age == age && c.kind.is_civil_row() == deck_is_civil)
                        .map(|c| c.count[n] as usize)
                        .sum();
                    assert!(
                        total <= state::MAX_DECK,
                        "{:?} deck at {}p is {} cards, MAX_DECK is {}",
                        age,
                        n + 2,
                        total,
                        state::MAX_DECK
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod size_report {
    /// Not an assertion -- a number worth seeing. Run with `--nocapture`.
    #[test]
    fn print_sizes() {
        eprintln!(
            "GameState={}B PlayerState={}B Tableau={}B",
            core::mem::size_of::<crate::GameState>(),
            core::mem::size_of::<crate::PlayerState>(),
            core::mem::size_of::<crate::Tableau>(),
        );
    }
}
