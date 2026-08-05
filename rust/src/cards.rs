//! Card identity and the static card table.
//!
//! DESIGN.md rule 1: a card is a `CardId`, which is an index. Names exist at
//! the I/O boundary and nowhere else. Every lookup in this module is an array
//! index, which is the difference between this engine and the Python one --
//! `dict.get` is the single largest entry in the Python profile at 9%, and the
//! profile is otherwise flat, so there is no hot loop to fix instead.

use core::fmt;

/// A card, as an index into [`CARDS`].
///
/// `u16` rather than `u8` because there are 236 cards today and the base game
/// is fixed, but the expansion is not being ported and a future `u8` overflow
/// would be a silent wrap. `u16` also keeps `Tableau::ids` aligned sanely.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct CardId(pub u16);

impl CardId {
    /// Sentinel for "no card". `Option<CardId>` is the same size (niche-less
    /// `u16`), so the explicit sentinel keeps the fixed-size arrays in
    /// `state.rs` plain `Copy` data with no `Option` unwrapping in hot loops.
    pub const NONE: CardId = CardId(u16::MAX);

    #[inline]
    pub fn is_none(self) -> bool {
        self == CardId::NONE
    }

    #[inline]
    pub fn get(self) -> &'static Card {
        &CARDS[self.0 as usize]
    }

    #[inline]
    pub fn name(self) -> &'static str {
        self.get().name
    }

    #[inline]
    pub fn kind(self) -> CardType {
        self.get().kind
    }

    /// Age as a number: A=0, I=1, II=2, III=3, IV=4. Mirrors `cards.level`.
    #[inline]
    pub fn level(self) -> u8 {
        self.get().age as u8
    }

    /// Parse a printed name. **I/O only** -- fixture loading, transcripts,
    /// tests. Linear scan is deliberate: making this fast would mean a
    /// `HashMap<String, CardId>`, and a fast path here would invite callers to
    /// use names in the engine, which rule 1 exists to prevent.
    pub fn by_name(name: &str) -> Option<CardId> {
        CARDS
            .iter()
            .position(|c| c.name == name)
            .map(|i| CardId(i as u16))
    }
}

impl fmt::Debug for CardId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_none() {
            f.write_str("CardId::NONE")
        } else {
            write!(f, "{}", self.name())
        }
    }
}

/// Which age a card belongs to. `Age::A` is the starting-tableau age.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[repr(u8)]
pub enum Age {
    A = 0,
    I = 1,
    II = 2,
    III = 3,
    IV = 4,
}

/// The 23 card types in the base game, exactly as `data/*.json` spells them.
///
/// Exhaustive by construction: the generator fails if `data/*.json` grows a
/// type not listed here, rather than defaulting it to something plausible.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum CardType {
    // production
    Farm,
    Mine,
    // urban
    Lab,
    Temple,
    Library,
    Arena,
    Theater,
    // military units
    Infantry,
    Cavalry,
    Artillery,
    Air,
    // other civil
    Government,
    SpecialTech,
    Wonder,
    Leader,
    Action,
    // military deck
    Tactic,
    Aggression,
    War,
    Pact,
    Bonus,
    Territory,
    Event,
}

impl CardType {
    #[inline]
    pub fn is_urban(self) -> bool {
        use CardType::*;
        matches!(self, Lab | Temple | Library | Arena | Theater)
    }

    #[inline]
    pub fn is_unit(self) -> bool {
        use CardType::*;
        matches!(self, Infantry | Cavalry | Artillery | Air)
    }

    #[inline]
    pub fn is_production(self) -> bool {
        matches!(self, CardType::Farm | CardType::Mine)
    }

    /// Types that can hold yellow tokens. Mirrors `cards.WORKER_TYPES`.
    #[inline]
    pub fn takes_workers(self) -> bool {
        self.is_urban() || self.is_unit() || self.is_production()
    }

    /// Mirrors `cards.DEVELOPABLE_TYPES`.
    #[inline]
    pub fn is_developable(self) -> bool {
        self.takes_workers() || matches!(self, CardType::SpecialTech | CardType::Government)
    }

    /// Mirrors `cards.CIVIL_ROW_TYPES` -- what appears in the 13-slot row.
    #[inline]
    pub fn is_civil_row(self) -> bool {
        use CardType::*;
        self.takes_workers()
            || matches!(self, Government | SpecialTech | Wonder | Leader | Action)
    }
}

/// The numeric effects that RECUR across cards.
///
/// DESIGN.md: 113 effect keys exist, but 60 of them appear exactly once. The
/// recurring ones live here as fields -- read on every stats recomputation, so
/// they must be a field load. The one-offs are [`Special`], which is code.
///
/// Signed because several cards subtract (Despotism's civil actions, the
/// obsolete-tactic penalty). Sixteen bits throughout: nothing in the base game
/// approaches 32k, and keeping the struct small keeps `CARDS` in cache.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CardEffects {
    /// Per-turn yield while this card is in play.
    pub culture: i16,
    pub science: i16,
    pub strength: i16,
    pub happy: i16,
    pub civil_actions: i16,
    pub military_actions: i16,

    /// One-shot gains when the card resolves (action cards, events).
    pub gain_culture: i16,
    pub gain_science: i16,
    pub gain_food: i16,
    pub gain_resources: i16,

    /// Discounts, in the units of the thing discounted.
    pub build_discount: i16,
    pub resource_discount: i16,
    pub resources_for_military_units: i16,

    /// Tactic scoring. `tactic_bonus_obsolete` is the reduced value once the
    /// tactic is superseded (§4.2) -- two fields because they are two numbers
    /// on the printed card, not one number with a modifier.
    pub tactic_bonus: i16,
    pub tactic_bonus_obsolete: i16,

    pub defense_bonus: i16,
    pub colonization_bonus: i16,
    pub colonize_bonus: i16,
    pub blue_tokens: i16,
    pub on_build_culture: i16,
    pub wonder_stages_per_action: i16,
    pub civil_hand_limit: i16,
    pub military_hand_limit: i16,
    pub free_civil_action: i16,
}

/// One card's static data. Immutable for the process lifetime.
#[derive(Clone, Copy, Debug)]
pub struct Card {
    pub name: &'static str,
    /// Printed name, before `_disambiguate` appended an age suffix to the
    /// handful of military cards that share a name across ages. Rules that key
    /// on the printed name (war spoils, one-per-name) read THIS, not `name`.
    pub base_name: &'static str,
    pub kind: CardType,
    pub age: Age,
    /// Science to develop / resources to build, from the printed card.
    pub science_cost: u8,
    pub resource_cost: u8,
    /// Copies in the deck at 2 / 3 / 4 players. Zero means "not in the deck"
    /// (wonders, leaders and starting techs are dealt by another route).
    pub count: [u8; 3],
    /// Food per turn (farms) or resources per turn (mines); the blue-token
    /// denomination for §6.4. Zero for everything else.
    pub production: u8,
    pub effects: CardEffects,
    /// The card's unique rules. Empty for the majority whose whole behaviour is
    /// `effects`. A slice rather than one value because several cards carry two
    /// or three unrelated one-offs, and because events additionally carry their
    /// targeting this way until the events port gives targeting its own type.
    pub special: &'static [Special],
}

/// A rule that belongs to exactly one card (or a handful).
///
/// This is the enum that makes the port's central guarantee: the `match` on it
/// in `effects.rs` is exhaustive, so **a card whose rule the engine cannot
/// interpret is a compile error**. Python's equivalent is name-dispatch over a
/// `"text"` field, where an unhandled card silently does nothing -- the exact
/// bug class ("in this registry, not in that one, and nothing fails when they
/// disagree") that this rewrite exists to close.
///
/// Variants are GENERATED into `card_table.rs` by `tools/gen_cards.py`, one per
/// distinct one-off effect key, so this enum cannot drift from the card data.
/// Do not hand-edit the variant list; edit the generator.
pub use crate::card_table::Special;

pub use crate::card_table::{CARDS, NUM_CARDS};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_through_names() {
        for (i, c) in CARDS.iter().enumerate() {
            assert_eq!(CardId::by_name(c.name), Some(CardId(i as u16)));
        }
    }

    #[test]
    fn none_is_not_a_real_card() {
        assert!(NUM_CARDS < CardId::NONE.0 as usize);
    }
}
