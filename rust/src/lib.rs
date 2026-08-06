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

pub mod advisor;
pub mod apply;
pub mod arena;
pub mod bots;
pub mod card_table;
pub mod cards;
pub mod combat;
pub mod costs;
pub mod legal;
pub mod economy;
pub mod effects;
pub mod events;
pub mod fixtures;
pub mod game;
pub mod harness;
pub mod interact;
pub mod moves;
pub mod rng;
pub mod state;
pub mod stats;

pub use cards::{
    Age, Card, CardEffects, CardId, CardType, Composition, Production, Special, CARDS, NUM_CARDS,
};
pub use moves::{Move, MoveList};
pub use state::{GameState, Pact, PactList, PlayerState, Phase, Tableau, TechSlot};

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

    /// Every tactic forms an army and nothing else does. The Python reads a
    /// tactic's composition straight out of the card dict, so a tactic that
    /// lost its composition in transcription would silently be worth zero
    /// strength rather than failing -- which is why this is asserted over the
    /// whole table rather than spot-checked.
    #[test]
    fn tactics_and_only_tactics_form_armies() {
        for c in CARDS.iter() {
            let forms_army = !c.composition.is_empty();
            assert_eq!(
                forms_army,
                c.kind == CardType::Tactic,
                "{}: composition {:?} but kind {:?}",
                c.name,
                c.composition,
                c.kind
            );
            if forms_army {
                // An army worth nothing is a transcription failure, not a card.
                assert!(c.effects.strength > 0, "{} forms an army worth 0", c.name);
                // The reduced rate is never an increase (§10.4).
                assert!(
                    (c.obsolete_strength as i16) <= c.effects.strength,
                    "{}: obsolete strength exceeds fresh",
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
