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
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum PactSide {
    Unspecified,
    A,
    B,
}

/// Winston Churchill's once-per-turn choice.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ChurchillChoice {
    Culture,
    Military,
}

/// A player index. Named so signatures like `Aggression { card, target }` read
/// unambiguously -- half the two-field moves here take a card and a player, and
/// a bare `(CardId, u8)` invites transposing them.
pub type PlayerIdx = u8;

/// `Hash` (added alongside `bots::neural::action`) is what lets a bare
/// `Move` serve as [`crate::bots::neural::action::ActionId`] directly: every
/// field here is plain-old-data (`CardId` is an index, `PlayerIdx`/`u8`/the
/// two small enums above are already `Hash`), so two equal `Move`s hash
/// equal and two different ones are never forced together the way a
/// hand-rolled `u64` digest could be -- see that module's top doc comment.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
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
    /// Alexander the Great, as a political action: remove him from the game
    /// for 1 yellow token from the box (`engine/actions.py`
    /// `("remove_leader_yellow",)`; his ability is always available while he
    /// is in play, so this move carries no argument).
    RemoveLeaderYellow,
    /// Christopher Columbus, as a political action: remove him from the game
    /// to colonize `card` (a territory from hand) with no military
    /// sacrifice (`engine/actions.py` `("columbus_colonize", name)`). One
    /// move per territory, exactly as `PrepareEvent` is one move per card.
    ColumbusColonize { card: CardId },
    /// Frederick Barbarossa, as an action-phase action: spend 1 military
    /// action to increase population AND build `card` (a unit technology) at
    /// once, both halves discounted off the printed price (`engine/
    /// actions.py` `("barbarossa", name)`). One move per unit technology,
    /// for the same reason `ColumbusColonize` carries its territory: a bare
    /// declaration would price at what the population half alone is worth.
    Barbarossa { card: CardId },
    /// J. S. Bach, once per turn as a civil action: upgrade `from` (any
    /// staffed urban building) to `to` (a theater of the same or higher
    /// level), paying the resource cost difference as normal (`engine/
    /// actions.py` `("bach_theater", from_name, to_name)`). The only
    /// cross-type upgrade in the game -- see `legal::bach_moves`.
    BachTheater { from: CardId, to: CardId },
    /// Trade Routes Agreement, side A's half ("Civilization A can use 1 food
    /// as 1 resource during its turn"): convert 1 stored food into 1
    /// resource, subject to the blue-token bank same as any other gain. NOT
    /// folded into `Move::Build`/`Move::Upgrade`'s own cost the way an
    /// action-card discount is -- this is the player's OWN choice, spendable
    /// whenever they hold the live grant, not gated on the action it will
    /// eventually help pay for (`legal::action_moves`, `economy::
    /// trade_food_as_resource_remaining`/`trade_resource_as_food_remaining`'s
    /// own doc comments). No action-point cost (the printed text is not a
    /// civil/military action).
    TradeFoodAsResource,
    /// Trade Routes Agreement, side B's half ("Civilization B can use 1
    /// resource as 1 food during its turn"): the mirror image of
    /// [`Move::TradeFoodAsResource`], converting 1 stored resource into 1
    /// food.
    TradeResourceAsFood,

    // ---- responses to a decision somebody else opened (engine::interact) ----
    /// Commit `n` military strength to a colonization auction.
    Bid { n: u8 },
    /// Stop bidding in a colonization auction.
    BidPass,
    /// Commit one military card from hand as defense strength against an
    /// aggression (`engine/interact.py` `_defense_move`: the move is
    /// `("defend", card_name)`, not a raw strength number -- the strength is
    /// derived from the card once committed).
    Defend { card: CardId },
    /// Stop committing defenders.
    DefendDone,
    /// Colonization (§11.3): commit one unit from the auction-winner's pool.
    SendUnit { card: CardId },
    /// Colonization (§11.3): commit one bonus card from the pool.
    SendBonus { card: CardId },
    /// Colonization (§11.3): James Cook only -- discard one military card
    /// (never a bonus card, see `interact::cook_pool`) from hand for +1
    /// colony bonus, up to twice per colonization (`engine/interact.py`
    /// `("send_discard", card_name)`).
    SendDiscard { card: CardId },
    /// Stop committing units/bonuses to a colonization force.
    SendDone,
    /// Pick option `n` of an open `choice` decision. The option list is
    /// generated with the decision and is positional, so this index is only
    /// meaningful against the state that opened it.
    Choose { n: u8 },
    Churchill { choice: ChurchillChoice },

    // ---- turn control ----
    /// End the Actions phase (`engine/actions.py` `("end_turn",)`).
    EndTurn,
    /// Pass the (at most one) political action (`("pol_pass",)`).
    PolPass,
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
            | PrepareEvent { card }
            | ColumbusColonize { card }
            | Barbarossa { card }
            | Defend { card }
            | SendUnit { card }
            | SendBonus { card }
            | SendDiscard { card } => Some(card),
            Upgrade { from: _, to } | BachTheater { from: _, to } => Some(to),
            Take { .. } | WonderStep { .. } | Pop | PopFree | CancelPact { .. } | Bid { .. }
            | BidPass | DefendDone | SendDone | Choose { .. } | Churchill { .. } | EndTurn
            | PolPass | Resign | RemoveLeaderYellow | TradeFoodAsResource
            | TradeResourceAsFood => None,
        }
    }

    // There was an `is_military(self) -> bool` here. It had zero call sites and
    // its list disagreed with its own docstring ("spends a military action
    // rather than a civil one") in both directions:
    //
    //   * `Build`/`Upgrade`/`Destroy` of a UNIT each spend one military action
    //     (`actions.do_build`, `do_upgrade`, `_h_destroy` -- all branch on
    //     `is_unit(name)`), and none of the three were listed.
    //   * `OfferPact` spends no military action at all; `_h_offer_pact` calls
    //     `_end_politics`, so it costs the POLITICAL action, and it was listed.
    //   * `Bid`/`BidPass`/`Defend`/`DefendDone`/`Send*` are sub-decisions
    //     inside a resolution that has already been paid for. They spend
    //     nothing, and they were listed.
    //
    // Deleted 2026-08-05 rather than corrected: with no caller there is no fact
    // of the matter about which of "costs a military action" and "belongs to
    // the military phase" it was meant to mean, and the bot port is about to
    // want one of those. Whichever it is, derive it at the use site from the
    // handlers named above, not from a list nothing checks.
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
