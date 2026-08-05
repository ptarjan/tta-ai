//! Moves.
//!
//! DESIGN.md rule 5. Python spells a move as a small tagged tuple --
//! `("take", 3)`, `("upgrade", "Bronze", "Iron")` -- which means `apply()` must
//! validate the *shape* of every move before it can consider its *legality*,
//! and a typo in a tag is a runtime error at best. Here an ill-shaped move does
//! not exist, so `legal_moves` has exactly one job.
//!
//! The variant list is derived from every `("...",)` tuple the Python engine
//! constructs (`engine/actions.py`, `engine/interact.py`, `engine/game.py`).
//! It is closed: if a port needs a move that is not here, the move list and
//! this enum have diverged and one of them is wrong -- fix it here rather than
//! smuggling the case through an escape hatch.

use crate::cards::CardId;

/// Which side of a pact is being offered (§5.9). Empty string in Python.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PactSide {
    Unspecified,
    A,
    B,
}

/// Winston Churchill's once-per-turn choice.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChurchillChoice {
    Culture,
    Military,
}

/// A player index. Named so signatures like `Aggression { card, target }` read
/// unambiguously -- half the two-field moves here take a card and a player, and
/// a bare `(CardId, u8)` invites transposing them.
pub type PlayerIdx = u8;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Move {
    // ---- civil actions ----
    /// Take the card in row slot `slot` (0-based). Cost is by slot (§2.3).
    Take { slot: u8 },
    /// Build a wonder step, a unit, or an urban building.
    Build { card: CardId },
    /// Develop a technology.
    Develop { card: CardId },
    /// Upgrade a worker from one card to a better one of the same family.
    Upgrade { from: CardId, to: CardId },
    /// Pay for `steps` stages of the wonder under construction.
    WonderStep { steps: u8 },
    /// Increase population (pay food, move a yellow token out of the bank).
    Pop,
    /// The free population increase some cards grant.
    PopFree,
    /// Change government by revolution rather than by taking it as a tech.
    Revolution { card: CardId },
    PlayLeader { card: CardId },
    PlayAction { card: CardId },
    /// Destroy one of my own cards (events, and the Ravages of Time flip).
    Destroy { card: CardId },

    // ---- military ----
    PlayTactic { card: CardId },
    CopyTactic { card: CardId },
    Aggression { card: CardId, target: PlayerIdx },
    War { card: CardId, target: PlayerIdx },
    OfferPact { card: CardId, target: PlayerIdx, side: PactSide },
    CancelPact { owner: PlayerIdx },
    /// Politics phase: put an event into the future-events stack.
    PrepareEvent { card: CardId },

    // ---- responses to a decision somebody else opened (engine::interact) ----
    /// Commit `n` military strength to an auction or a colonisation bid.
    Bid { n: u8 },
    /// Commit `n` strength to defence.
    Defend { n: u8 },
    /// Stop committing units.
    SendDone,
    /// Pick option `n` of an open `choice` decision. The option list is
    /// generated with the decision and is positional, so this index is only
    /// meaningful against the state that opened it.
    Choose { n: u8 },
    Churchill { choice: ChurchillChoice },

    // ---- turn control ----
    Resign,
}

impl Move {
    /// The card this move names, if any. Used by the move-ordering contract:
    /// `legal_moves` output order is part of the differential test, because the
    /// bots break ties by index and a reordered list silently changes play.
    pub fn card(self) -> Option<CardId> {
        use Move::*;
        match self {
            Build { card }
            | Develop { card }
            | Revolution { card }
            | PlayLeader { card }
            | PlayAction { card }
            | Destroy { card }
            | PlayTactic { card }
            | CopyTactic { card }
            | Aggression { card, .. }
            | War { card, .. }
            | OfferPact { card, .. }
            | PrepareEvent { card } => Some(card),
            Upgrade { from: _, to } => Some(to),
            Take { .. } | WonderStep { .. } | Pop | PopFree | CancelPact { .. } | Bid { .. }
            | Defend { .. } | SendDone | Choose { .. } | Churchill { .. } | Resign => None,
        }
    }

    /// Whether this move spends a military action rather than a civil one.
    #[inline]
    pub fn is_military(self) -> bool {
        use Move::*;
        matches!(
            self,
            PlayTactic { .. }
                | CopyTactic { .. }
                | Aggression { .. }
                | War { .. }
                | OfferPact { .. }
                | Bid { .. }
                | Defend { .. }
                | SendDone
        )
    }
}

/// A legal-move list. Fixed capacity for the same reason the state is: this is
/// allocated on every decision, and there are ~372 decisions per 3p game.
///
/// The bound is asserted, not assumed. If it fires, measure the real maximum
/// before raising it -- a move list far larger than expected is usually a
/// generation bug, not a big position.
pub const MAX_MOVES: usize = 256;

#[derive(Clone, Debug)]
pub struct MoveList {
    items: [Move; MAX_MOVES],
    len: u16,
}

impl MoveList {
    pub fn new() -> Self {
        MoveList { items: [Move::Resign; MAX_MOVES], len: 0 }
    }

    #[inline]
    pub fn push(&mut self, m: Move) {
        debug_assert!((self.len as usize) < MAX_MOVES, "move list overflow");
        self.items[self.len as usize] = m;
        self.len += 1;
    }

    #[inline]
    pub fn as_slice(&self) -> &[Move] {
        &self.items[..self.len as usize]
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len as usize
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl Default for MoveList {
    fn default() -> Self {
        Self::new()
    }
}
