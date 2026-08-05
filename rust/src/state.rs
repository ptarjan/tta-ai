//! Game state.
//!
//! DESIGN.md rule 3: flat, fixed-size, `Clone`-as-memcpy. The search clones
//! states constantly -- that clone is what `copy_state` costs in Python today,
//! and the Python profile is flat precisely because the cost is spread across
//! every dict copy and every dict lookup rather than concentrated anywhere a
//! profiler can point at.
//!
//! Everything here is owned. There is not a lifetime parameter in this file and
//! there must not be one: cross-references are `CardId` and small indices.

use crate::cards::{CardId, CardType, NUM_CARDS};

pub const MAX_PLAYERS: usize = 4;
/// §2.1 -- the civil card row is thirteen slots.
pub const ROW_SIZE: usize = 13;

/// Largest deck an age can hold, over all player counts. Measured from
/// `data/*.json` on 2026-08-05: civil peaks at 53 (4p, Ages II and III),
/// military at 50 (3p/4p, Age II). Sized with headroom and asserted on refill;
/// this is a base-game-only bound and the expansion is deliberately not ported.
pub const MAX_DECK: usize = 64;

/// Cards a hand can hold. The printed limit is government-dependent and rises
/// to 13 with card effects (§2.5); this is the array bound, not the rule.
pub const MAX_HAND: usize = 24;

/// Distinct cards one player's tableau can hold at once. Every card in play is
/// one slot: farms, mines, urban buildings, units, special techs, the current
/// government. Debug-asserted on insert.
pub const MAX_TABLEAU: usize = 48;

/// Age of the deck currently being drawn. Also the age index used by
/// `civil_discard` / `civil_removed`.
pub use crate::cards::Age;

/// A technology in a tableau: the card plus the tokens sitting on it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TechSlot {
    /// Yellow tokens on the card.
    pub workers: u8,
    /// Blue tokens on the card (stored food / resources).
    pub stored: u8,
}

/// One player's cards in play.
///
/// A dense/sparse set: `ids` + `slots` are the dense side in insertion order
/// (so iteration is a short contiguous scan, not 236 wasted probes), and
/// `index` is the sparse side mapping `CardId -> dense position` for O(1)
/// membership without hashing. Python spells this `dict[str, TechCard]`, which
/// costs a string hash on every one of the ~734k lookups a 60-game 4p batch
/// makes.
///
/// `index` is 236 bytes and dominates the struct, which is fine: cloning it is
/// one memcpy, whereas cloning the equivalent dict is 20-odd allocations.
#[derive(Clone, Debug)]
pub struct Tableau {
    ids: [CardId; MAX_TABLEAU],
    slots: [TechSlot; MAX_TABLEAU],
    index: [u8; NUM_CARDS],
    len: u8,
}

impl Tableau {
    const ABSENT: u8 = u8::MAX;

    pub fn new() -> Self {
        Tableau {
            ids: [CardId::NONE; MAX_TABLEAU],
            slots: [TechSlot { workers: 0, stored: 0 }; MAX_TABLEAU],
            index: [Self::ABSENT; NUM_CARDS],
            len: 0,
        }
    }

    #[inline]
    pub fn has(&self, id: CardId) -> bool {
        self.index[id.0 as usize] != Self::ABSENT
    }

    #[inline]
    pub fn get(&self, id: CardId) -> Option<&TechSlot> {
        let pos = self.index[id.0 as usize];
        if pos == Self::ABSENT {
            None
        } else {
            Some(&self.slots[pos as usize])
        }
    }

    #[inline]
    pub fn get_mut(&mut self, id: CardId) -> Option<&mut TechSlot> {
        let pos = self.index[id.0 as usize];
        if pos == Self::ABSENT {
            None
        } else {
            Some(&mut self.slots[pos as usize])
        }
    }

    /// Workers on a card, zero if the card is not in play. Mirrors
    /// `PlayerState.worker_count`.
    #[inline]
    pub fn workers(&self, id: CardId) -> u8 {
        self.get(id).map_or(0, |s| s.workers)
    }

    pub fn insert(&mut self, id: CardId, slot: TechSlot) {
        debug_assert!(!self.has(id), "{id:?} already in tableau");
        debug_assert!((self.len as usize) < MAX_TABLEAU, "tableau overflow");
        let pos = self.len;
        self.ids[pos as usize] = id;
        self.slots[pos as usize] = slot;
        self.index[id.0 as usize] = pos;
        self.len += 1;
    }

    /// Remove a card, closing the hole and PRESERVING insertion order.
    ///
    /// This was a swap-remove until the economy port found what that costs.
    /// Python's tableau is a dict, so it iterates in build order, and two
    /// things depend on that order in ways that change play:
    ///
    ///   * `economy.lose_population` takes the worker off the FIRST
    ///     worker-holding card it walks (`engine/economy.py:290`). Which card
    ///     shrinks is arbitrary as a rule, but it is not arbitrary as a
    ///     position -- losing a farm worker is not losing a mine worker.
    ///   * `legal_moves` enumerates in the same order, and the bots break ties
    ///     by index, so a reordered list silently changes the chosen move.
    ///
    /// A swap-remove would diverge from the Python the first time any card
    /// left a tableau (a destroyed wonder, an antiquated tech, a Ravages of
    /// Time flip) and every fixture replayed after that point would drift.
    /// Order-preserving removal costs a memmove bounded by `MAX_TABLEAU` (48)
    /// on an operation that happens a handful of times per game, against a
    /// correctness property every later module leans on. That trade is not
    /// close.
    pub fn remove(&mut self, id: CardId) -> Option<TechSlot> {
        let pos = self.index[id.0 as usize];
        if pos == Self::ABSENT {
            return None;
        }
        let pos = pos as usize;
        let n = self.len as usize;
        let out = self.slots[pos];
        self.ids.copy_within(pos + 1..n, pos);
        self.slots.copy_within(pos + 1..n, pos);
        // Everything after the hole shifted down one; reindex it.
        for i in pos..n - 1 {
            self.index[self.ids[i].0 as usize] = i as u8;
        }
        self.ids[n - 1] = CardId::NONE;
        self.slots[n - 1] = TechSlot::default();
        self.index[id.0 as usize] = Self::ABSENT;
        self.len -= 1;
        Some(out)
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len as usize
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = (CardId, &TechSlot)> + '_ {
        self.ids[..self.len as usize]
            .iter()
            .zip(&self.slots[..self.len as usize])
            .map(|(id, s)| (*id, s))
    }

    /// Iterate the cards of one type. A short contiguous scan with a branch --
    /// faster than the Python equivalent's dict walk plus per-name type lookup,
    /// and it is what `effects` does on every stats recomputation.
    #[inline]
    pub fn of_type(&self, kind: CardType) -> impl Iterator<Item = (CardId, &TechSlot)> + '_ {
        self.iter().filter(move |(id, _)| id.kind() == kind)
    }
}

impl Default for Tableau {
    fn default() -> Self {
        Self::new()
    }
}

/// One pact in play (§5.9).
///
/// A pact card is asymmetric: it prints an `A` block and a `B` block, and which
/// player gets which is decided when the pact is offered, independently of who
/// physically holds the card. So `owner`/`partner` (who holds it, who agreed)
/// and `a`/`b` (who takes which printed block) are four separate indices, not
/// two -- `engine/actions.py:1084` sets `a`/`b` in either order depending on
/// which side was offered. Collapsing them to "owner is A" would be right about
/// half the time and silently wrong the rest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pact {
    pub card: CardId,
    pub owner: u8,
    pub partner: u8,
    pub a: u8,
    pub b: u8,
}

impl Pact {
    /// The other party to this pact, seen from `idx`. Meaningless unless `idx`
    /// is a party, which every caller checks first.
    #[inline]
    pub fn partner_of(&self, idx: u8) -> u8 {
        if self.owner == idx {
            self.partner
        } else {
            self.owner
        }
    }

    #[inline]
    pub fn is_party(&self, idx: u8) -> bool {
        self.owner == idx || self.partner == idx
    }
}

/// Pacts held in one play area. Four is already more than the base game deals
/// out; the bound is asserted rather than assumed, per the deck bounds above.
pub const MAX_PACTS: usize = 8;

#[derive(Clone, Debug)]
pub struct PactList {
    items: [Pact; MAX_PACTS],
    len: u8,
}

impl PactList {
    pub const fn new() -> Self {
        PactList {
            items: [Pact { card: CardId::NONE, owner: 0, partner: 0, a: 0, b: 0 }; MAX_PACTS],
            len: 0,
        }
    }

    #[inline]
    pub fn push(&mut self, p: Pact) {
        debug_assert!((self.len as usize) < MAX_PACTS, "pact list overflow");
        self.items[self.len as usize] = p;
        self.len += 1;
    }

    #[inline]
    pub fn as_slice(&self) -> &[Pact] {
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

    /// Drop every pact matching `pred`, preserving the order of the rest --
    /// Python rebuilds the list with a comprehension (`effects.cancel_attack_pacts`,
    /// `drop_pacts_of`), so order survives there and must survive here.
    pub fn retain(&mut self, mut keep: impl FnMut(&Pact) -> bool) {
        let mut out = 0usize;
        for i in 0..self.len as usize {
            if keep(&self.items[i]) {
                self.items[out] = self.items[i];
                out += 1;
            }
        }
        self.len = out as u8;
    }
}

impl Default for PactList {
    fn default() -> Self {
        Self::new()
    }
}

/// A fixed-capacity list of cards: hands, decks, discards, event stacks.
///
/// Exists so none of those is a `Vec`. A `Vec` in the state means an allocation
/// per clone, and the search clones far more often than it pushes.
#[derive(Clone, Debug)]
pub struct CardList<const N: usize> {
    items: [CardId; N],
    len: u16,
}

impl<const N: usize> CardList<N> {
    pub const fn new() -> Self {
        CardList { items: [CardId::NONE; N], len: 0 }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len as usize
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline]
    pub fn as_slice(&self) -> &[CardId] {
        &self.items[..self.len as usize]
    }

    /// Exists so decks (civil/military/event stacks) can be shuffled in place.
    #[inline]
    pub fn as_mut_slice(&mut self) -> &mut [CardId] {
        &mut self.items[..self.len as usize]
    }

    #[inline]
    pub fn push(&mut self, id: CardId) {
        debug_assert!((self.len as usize) < N, "CardList<{N}> overflow");
        self.items[self.len as usize] = id;
        self.len += 1;
    }

    #[inline]
    pub fn pop(&mut self) -> Option<CardId> {
        if self.len == 0 {
            return None;
        }
        self.len -= 1;
        Some(self.items[self.len as usize])
    }

    /// Remove the first occurrence, preserving order. Order matters here in a
    /// way it does not in `Tableau`: decks are drawn from and hands are
    /// enumerated into move lists.
    pub fn remove_first(&mut self, id: CardId) -> bool {
        let n = self.len as usize;
        if let Some(i) = self.items[..n].iter().position(|&c| c == id) {
            self.items.copy_within(i + 1..n, i);
            self.len -= 1;
            self.items[self.len as usize] = CardId::NONE;
            true
        } else {
            false
        }
    }

    #[inline]
    pub fn contains(&self, id: CardId) -> bool {
        self.as_slice().contains(&id)
    }
}

impl<const N: usize> Default for CardList<N> {
    fn default() -> Self {
        Self::new()
    }
}

/// A player.
///
/// Field order mirrors `engine/state.py::PlayerState` so the two can be read
/// side by side during the port; grouping is by lifetime (permanent, per-turn,
/// bookkeeping) because that is what a reader needs.
#[derive(Clone, Debug)]
pub struct PlayerState {
    pub idx: u8,

    // ---- cards in play ----
    pub techs: Tableau,
    pub government: CardId,
    pub leader: CardId,
    pub used_leader_ability: bool,
    /// The wonder under construction and how many steps are paid for.
    pub wonder: CardId,
    pub wonder_steps: u8,
    pub completed_wonders: CardList<8>,
    /// Destroyed wonders still count toward the row take surcharge (§2.3).
    pub destroyed_wonders: u8,
    /// Wonder that Homer was tucked under.
    pub homer_wonder: CardId,
    pub tactic: CardId,
    /// Tactic still in my play area rather than public.
    pub tactic_exclusive: bool,
    pub colonies: CardList<8>,
    pub flipped_wonders: CardList<8>,
    /// Pacts sitting in MY play area (§5.9). A pact binds two players but is
    /// physically held by one, and both facts matter: `pacts_for` scans every
    /// player's list to find the ones an index is party to, while cancelling
    /// removes it from wherever it sits. See `PactList`.
    pub pacts: PactList,

    // ---- hands ----
    pub hand_civil: CardList<MAX_HAND>,
    pub hand_military: CardList<MAX_HAND>,
    /// Cards KNOWN to be in this hand whose identity is unknown. Always zero in
    /// self-play -- the engine deals every card by name. They exist for the app
    /// harness, which mirrors a rival whose hand SIZE is public (§2.6) but whose
    /// contents were not transcribed. Without them such a rival reads as an
    /// EMPTY hand, which is not "unknown", it is wrong.
    pub hidden_civil: u8,
    pub hidden_military: u8,

    // ---- pools ----
    /// Unborn population.
    pub yellow_bank: u8,
    /// Yellow tokens that entered this supply from outside it (grants and
    /// transfers). Bookkeeping only: lets tests assert nothing else ever
    /// creates a token (§12.2.4).
    pub yellow_granted: u8,
    pub workers_free: u8,
    /// Total blue tokens owned, bank plus cards.
    pub blue_total: u8,
    pub food: u16,
    pub resources: u16,
    pub science: u16,
    pub culture: u16,
    pub culture_rate_extra: i16,
    pub science_rate_extra: i16,
    pub strength_extra: i16,
    pub happy_extra: i16,

    // ---- this turn ----
    pub civil_actions: i8,
    pub military_actions: i8,
    pub politics_done: bool,
    /// At most one tactic play/copy per phase.
    pub tactic_action_used: bool,
    pub taken_this_turn: CardList<8>,
    /// Civil actions spent THIS TURN reaching into the card row (§2.3 slot cost
    /// plus the wonder surcharge / Hammurabi discount). Nothing in the rules
    /// reads it, but the evaluator cannot see how a civil action was spent, and
    /// "a CA spent grabbing from the row" and "a CA spent upgrading a worker"
    /// move in OPPOSITE directions as the game goes on -- so it needs them as
    /// two channels, not one `ca_left`.
    pub ca_spent_taking: u8,
    pub hammurabi_used: bool,
    pub churchill_used: bool,
    pub bach_upgrade_used: bool,
    pub ocean_liners_used: bool,
    pub caesar_double_politics_used: bool,
    pub skip_next_politics: bool,
    /// Rebellion.
    pub ca_penalty_next_turn: i8,
    /// Resource discount pool for military unit builds/upgrades this turn
    /// (Patriotism / Wave of Nationalism / Military Build-Up, §3.11).
    pub mil_discount: i16,
    /// The exact twin of `mil_discount`, for the same reason: Churchill's
    /// military option is "3 science usable only to develop military unit
    /// technologies", which is not 3 science.
    pub mil_sci_discount: i16,

    pub resigned: bool,
}

/// Whose turn phase it is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Politics,
    Actions,
    Done,
}

/// The whole game.
#[derive(Clone, Debug)]
pub struct GameState {
    pub num_players: u8,
    pub seed: u64,
    pub players: [PlayerState; MAX_PLAYERS],
    pub current: u8,
    /// 1-based player-turn counter.
    pub turn: u16,
    /// 1-based round counter.
    pub round: u16,
    pub start_player: u8,

    pub age_civil: Age,
    pub age_military: Age,
    pub civil_deck: CardList<MAX_DECK>,
    pub military_deck: CardList<MAX_DECK>,
    /// Thirteen slots; `CardId::NONE` is an empty slot.
    pub card_row: [CardId; ROW_SIZE],

    pub future_events: CardList<MAX_DECK>,
    pub current_events: CardList<MAX_DECK>,
    pub past_events: CardList<MAX_DECK>,
    pub current_events_age: Age,
    pub scoring_events: CardList<8>,
    pub available_tactics: CardList<16>,

    /// Cards that left play, by age. Records, not state: nothing in the rules
    /// or the turn loop reads them, so they cannot change play. They exist
    /// because otherwise the legal card count
    ///     unseen(age) = deck(age, n) - row - hands - tableaux - discard
    /// is uncomputable, and a human at the table sees every one of these go.
    ///
    /// Two lists rather than one because PROVENANCE is real information: a card
    /// swept off the left of the row (`civil_discard`) is not the same event as
    /// one antiquated out of a hand or a destroyed wonder (`civil_removed`).
    /// Everything computing "what is left" MUST read the UNION -- reading one
    /// without the other undercounts silently, which is the whole bug class
    /// these fields exist to close.
    pub civil_discard: [CardList<MAX_DECK>; 5],
    pub civil_removed: [CardList<MAX_DECK>; 5],
    pub discarded_military: [CardList<MAX_DECK>; 5],

    pub last_round: bool,
    /// Turn index after which the game ends.
    pub final_round_end: Option<u16>,
    pub game_over: bool,
    pub phase: Phase,
    /// Last player standing (§5.11).
    pub forced_winner: Option<u8>,
}

impl GameState {
    #[inline]
    pub fn me(&self) -> &PlayerState {
        &self.players[self.current as usize]
    }

    #[inline]
    pub fn me_mut(&mut self) -> &mut PlayerState {
        &mut self.players[self.current as usize]
    }

    #[inline]
    pub fn active(&self) -> impl Iterator<Item = &PlayerState> + '_ {
        self.players[..self.num_players as usize]
            .iter()
            .filter(|p| !p.resigned)
    }
}

impl PlayerState {
    /// Cards in hand, named or not. The quantity §2.5 actually counts.
    #[inline]
    pub fn hand_size_civil(&self) -> usize {
        self.hand_civil.len() + self.hidden_civil as usize
    }

    #[inline]
    pub fn hand_size_military(&self) -> usize {
        self.hand_military.len() + self.hidden_military as usize
    }

    #[inline]
    pub fn has(&self, id: CardId) -> bool {
        self.techs.has(id)
    }
}

/// Compile-time proof that a clone stays a memcpy of a few kilobytes rather
/// than growing a heap indirection. If this fires, something gained a `Vec`,
/// a `String` or a `Box` -- check DESIGN.md rule 3 before raising the bound.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_stays_flat_and_small() {
        assert!(
            core::mem::size_of::<GameState>() < 16 * 1024,
            "GameState is {} bytes",
            core::mem::size_of::<GameState>()
        );
    }

    #[test]
    fn tableau_insert_remove_round_trips() {
        let a = CardId(0);
        let b = CardId(1);
        let mut t = Tableau::new();
        t.insert(a, TechSlot { workers: 2, stored: 0 });
        t.insert(b, TechSlot { workers: 1, stored: 3 });
        assert!(t.has(a) && t.has(b));
        assert_eq!(t.workers(a), 2);
        // Removing the FIRST entry moves every survivor down one slot; each
        // must still be findable through the sparse index afterwards.
        assert_eq!(t.remove(a).unwrap().workers, 2);
        assert!(!t.has(a));
        assert!(t.has(b));
        assert_eq!(t.workers(b), 1);
        assert_eq!(t.get(b).unwrap().stored, 3);
        assert_eq!(t.len(), 1);
    }

    /// Build order is play-relevant: `economy.lose_population` takes a worker
    /// off the first worker-holding card in tableau order, and `legal_moves`
    /// enumerates in that order while the bots break ties by index. A
    /// swap-remove passes the round-trip test above and still fails this one,
    /// which is exactly why this test exists separately.
    #[test]
    fn tableau_remove_preserves_build_order() {
        let mut t = Tableau::new();
        for i in 0..6u16 {
            t.insert(CardId(i), TechSlot { workers: i as u8, stored: 0 });
        }
        // Take one out of the middle -- an antiquated tech, say.
        t.remove(CardId(2));
        let order: Vec<u16> = t.iter().map(|(id, _)| id.0).collect();
        assert_eq!(order, vec![0, 1, 3, 4, 5]);
        // ...and the sparse index must still agree with the dense order, or
        // the next removal corrupts a different card.
        for (id, slot) in t.iter() {
            assert_eq!(slot.workers, id.0 as u8, "slot followed the wrong id");
        }
        t.remove(CardId(0));
        let order: Vec<u16> = t.iter().map(|(id, _)| id.0).collect();
        assert_eq!(order, vec![1, 3, 4, 5]);
        for (id, slot) in t.iter() {
            assert_eq!(slot.workers, id.0 as u8);
        }
    }

    #[test]
    fn card_list_remove_first_preserves_order() {
        let mut l: CardList<8> = CardList::new();
        for i in 0..4 {
            l.push(CardId(i));
        }
        assert!(l.remove_first(CardId(1)));
        assert_eq!(l.as_slice(), &[CardId(0), CardId(2), CardId(3)]);
        assert!(!l.remove_first(CardId(1)));
    }
}
