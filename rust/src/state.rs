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

/// `GameState::seeded_by`'s "nobody" sentinel -- the same convention
/// `Tableau::ABSENT` uses (`u8::MAX`, never a real player index, which is
/// 0..=3).
pub const NOT_SEEDED: u8 = u8::MAX;

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
    /// `legal_moves` enumerates in tableau order and the bots break ties by
    /// index, so a reordered tableau silently changes the chosen move -- the
    /// order is part of the bot's behaviour, not a presentation detail.
    ///
    /// (A second reason used to be listed here, `economy::lose_population`
    /// taking the worker off the first card it walked. That function was dead
    /// and its rule was wrong -- FAQ p.15 makes it the loser's choice -- so it
    /// was deleted on 2026-08-15. `legal_moves` alone still settles this.)
    ///
    /// A swap-remove would reorder a tableau the first time any card left it
    /// (a destroyed wonder, an antiquated tech, a Ravages of Time flip) and
    /// every fixture replayed after that point would drift.
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
#[derive(Clone, Debug, PartialEq, Eq)]
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
        // A real `assert!`, not `debug_assert!` (2026-08-06, after the
        // `colonies: CardList<8>` overflow crashed the 4p league): the
        // `self.items[self.len as usize] = id` line below is safe Rust array
        // indexing, so release builds ALREADY bounds-check it -- that check
        // is what actually fired in production ("index out of bounds: the
        // len is 8 but the index is 8"), not this line, which compiles out
        // under `panic = "abort"` + no `debug-assertions`. So this `assert!`
        // adds no NEW safety net, only a message that names the struct
        // (`CardList<{N}> overflow`) instead of a bare index; measured via
        // `tests/suite/bench_playout.rs` (3x500 4p games, `assert!` vs
        // `debug_assert!` in an otherwise-identical release build): within
        // noise, ~475 games/s either way. Not worth losing the diagnosis
        // time back for.
        assert!((self.len as usize) < N, "CardList<{N}> overflow");
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
    /// The wonder under construction, how many of its stages are paid for,
    /// and WHICH positions those stages are. `wonder_stages_built` is a
    /// bitmask over the wonder's `Card::stages` array (position 0 = the
    /// leftmost printed stage), the source of truth; `wonder_steps` is its
    /// POPULATION COUNT (`count_ones()`), kept as a plain `u8` because it is
    /// read from a dozen places (the blue-token `economy::blue_used`
    /// accounting, the `bots::`/`advisor::`/`harness::` feature readers, the
    /// legal-move generators' `stages_left` bound) and re-deriving it on
    /// every read is wasteful for a value that is never written anywhere but
    /// the single place that owns it, `apply::do_wonder_step` (plus the
    /// reset-to-0 sites on take/antiquation/loss).
    ///
    /// Why positions, not just a count: BGO lets a player build ANY
    /// uncovered stage of the wonder, paying that stage's own printed cost,
    /// not always the next left-to-right one (game `7520718`: Colossus
    /// `[3, 3]` built 3-then-1 via Engineering Genius; `7521361`: Pyramids
    /// `[3, 2, 1]` built 3-then-2). A count alone cannot tell which stage
    /// the NEXT payment is for -- the price of stage 0 vs stage 1 vs stage
    /// 2 differ for most of the 16 base-game wonders, so a count-only model
    /// (the old `wonder_stage_cost` reading of `stages[done..done+k]`)
    /// charged the wrong price whenever the journal's order diverged from
    /// left-to-right, drifting the player's resources for the rest of the
    /// game. See `apply::do_wonder_step`'s own doc and `costs::
    /// wonder_stages_cost` for the two readers that actually PAY against
    /// the positions rather than a fixed offset.
    pub wonder: CardId,
    pub wonder_stages_built: u32,
    pub wonder_steps: u8,
    /// Append-only (see the doc on [`Self::destroyed_wonders`]) and unique per
    /// card -- `card_table.rs` prints exactly one copy of each of the 16 base-
    /// game wonders (`count: [1, 1, 1]` on every `CardType::Wonder` entry, all
    /// ages, checked 2026-08-06), so one player completing every wonder in the
    /// game (nobody else ever builds one) is the worst case, not 8 of them.
    /// `CardList<8>` overflowed here in the wild via the sibling field
    /// [`Self::colonies`] (see its own doc); raised alongside it rather than
    /// waiting for a second incident to prove the same math.
    pub completed_wonders: CardList<16>,
    /// Destroyed wonders still count toward the row take surcharge (§2.3).
    ///
    /// Always 0 in the 2015 base game, and that is correct, not a gap. Nothing
    /// here removes a wonder from [`Self::completed_wonders`] -- the only card
    /// that touches a finished wonder is `flipCompletedWonder` (Ravages of
    /// Time), which leaves it completed and adds it to
    /// [`Self::flipped_wonders`], so it keeps paying the surcharge through the
    /// `completed_wonders` term. §2.3 says "built or destroyed" because the
    /// EXPANSION can destroy one; the term is modelled so the rule reads as
    /// printed and so the expansion needs no new field. Checked 2026-08-05:
    /// `completed_wonders` is append-only in both engines.
    pub destroyed_wonders: u8,
    /// Wonder that Homer was tucked under.
    pub homer_wonder: CardId,
    pub tactic: CardId,
    /// Tactic still in my play area rather than public.
    pub tactic_exclusive: bool,
    /// Territory cards won at colonization auctions (§11.5), append-only --
    /// nothing here removes an entry (`Annex` moves one to a NEW owner via
    /// [`crate::interact::lose_colony`] on the LOSING side and a fresh
    /// `push` on the gaining side, it never shrinks the winner's own list).
    /// `card_table.rs` prints exactly 12 territory cards total for the whole
    /// game (6 Age I + 6 Age II, `count: [1, 1, 1]` each, checked 2026-08-06)
    /// -- there is no rule capping how many one player may hold, so the true
    /// worst case is one player winning every colonization auction in the
    /// game. CONFIRMED REACHABLE, not hypothetical: this is the field that
    /// overflowed `CardList<8>` in production (`interact::gain_colony`,
    /// 2026-08-06) -- the trained 4p champion's policy contests colonization
    /// auctions so rarely that a single player routinely wins 9+ of the 12
    /// against it. See `gain_colony`'s own test module for the regression
    /// test.
    pub colonies: CardList<12>,
    /// Always a subset of [`Self::completed_wonders`] (`interact.rs`'s
    /// `Special::FlipCompletedWonder` handler only pushes a wonder here that
    /// is already IN `completed_wonders`), so it shares that field's 16-wonder
    /// capacity rather than needing its own separate bound.
    pub flipped_wonders: CardList<16>,
    /// §2.5/§9.1: every age a leader has EVER been taken in, across the
    /// whole game -- bit `card.age as u8` set the moment a leader of that
    /// age is taken (`take_card`), never cleared, even once that leader is
    /// later replaced (`Age` has 5 values, ample headroom in a `u8`). Mirrors
    /// Python's `p.taken_leader_ages` list membership test as a bitmask
    /// instead, per DESIGN.md rule 3 (flat, fixed-size, no `Vec`). Was a
    /// caller-supplied parameter to `costs::take_gate`/`can_take` before this
    /// field existed; both now read it directly off `PlayerState`.
    pub taken_leader_ages: u8,
    /// §5.6: the war card THIS player currently has declared, `CardId::NONE`
    /// if none. At most one at a time -- `legal.rs`'s war move generation
    /// (once written) gates a second declaration on this being set, exactly
    /// as `engine/actions.py::_politics_moves` gates on `p.war_declared_by_me`
    /// being truthy.
    pub war_declared_by_me: CardId,
    /// Player index `war_declared_by_me` targets. Meaningless while
    /// `war_declared_by_me` is `CardId::NONE`.
    pub war_target: u8,
    /// §5.6: wars declared ON this player, indexed by the ATTACKER's player
    /// index (`CardId::NONE` = that attacker has no war declared on me).
    /// A flat array rather than Python's list: at most one active war per
    /// attacker at a time (their own `war_declared_by_me` gates a second),
    /// so the attacker's index is already a unique key -- no growable
    /// container needed (DESIGN.md rule 3).
    pub wars_declared_on_me: [CardId; MAX_PLAYERS],
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
    /// Printed cost (unrounded) of the urban buildings THIS Raid aggression
    /// has destroyed so far this use, accumulated across its per-age-tier
    /// brackets (`QueueItem::Raid`/`ChoiceKind::Raid`'s `is_last`) so the
    /// card's "half the resources needed to build THEM, rounded up" can
    /// round the TOTAL once, not each casualty separately -- see
    /// `interact.rs`'s `ChoiceKind::Raid` handler, which drains this back to
    /// 0 the moment the last bracket resolves (destroyed, declined, or
    /// invalid alike), so it never survives past the raid that filled it.
    pub raid_loot_pending: u16,
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
    /// Reset at the start of MY next turn (`game.rs`/`economy.rs`), so unlike
    /// [`Self::colonies`]/[`Self::completed_wonders`] this does NOT need a
    /// whole-game bound -- but 8 is still too tight for one accumulation
    /// window. Stackable civil-action sources that survive to the SAME
    /// window (checked against every nonzero `civil_actions` in
    /// `card_table.rs`, 2026-08-06): government max 7 (Republic/Communism/
    /// Democracy) + best same-icon SpecialTech +2 (Civil Service; Code of
    /// Laws/Justice System are the same Law icon so §7.6 keeps only one) +
    /// both civil-action wonders +2 (Pyramids, Kremlin -- wonders aren't
    /// icon-gated against each other) + Hammurabi's once-per-turn MA-as-CA
    /// conversion +1 (`costs::spare_ca`) = 12 in a single turn if every
    /// civil action that turn is spent taking an `Action`-kind card from the
    /// row. On top of that, International Agreement (`card_table.rs`, one
    /// copy in the whole game) grants the strongest player a ONE-TIME
    /// `optional_take_cards_with_civil_actions: 5` side pool
    /// (`interact::offer_take_row`) that is not charged against
    /// `civil_actions` at all, so it can land in the same window as a
    /// player's own maxed-out turn (it fires off another player's Politics
    /// Phase reveal, before this player's own next-turn reset). 12 + 5 = 17.
    pub taken_this_turn: CardList<17>,
    /// Civil actions spent THIS TURN reaching into the card row (§2.3 slot cost
    /// plus the wonder surcharge / Hammurabi discount). Nothing in the rules
    /// reads it, but the evaluator cannot see how a civil action was spent, and
    /// "a CA spent grabbing from the row" and "a CA spent upgrading a worker"
    /// move in OPPOSITE directions as the game goes on -- so it needs them as
    /// two channels, not one `ca_left`.
    pub ca_spent_taking: u8,
    pub hammurabi_used: bool,
    /// Hammurabi was in play during THIS turn and has since been replaced by
    /// a new leader (`apply::h_play_leader`). His once-per-turn MA-as-CA
    /// conversion survives that replacement for the rest of the turn:
    /// the rulebook is explicit that "you are allowed to use the benefit of a
    /// leader and then replace him or her on the same turn" (RB, "Replacing a
    /// Leader"), so the conversion was always reachable BEFORE the swap, and
    /// `costs::pay_ca` only reaches for it lazily -- once the civil-action
    /// pool runs dry, which is typically after the swap. Without this flag a
    /// player who replaces Hammurabi mid-turn silently forfeits an action the
    /// rules let them keep. Cleared with `hammurabi_used` in
    /// `economy::end_of_turn`.
    pub hammurabi_replaced_this_turn: bool,
    /// This turn's `Move::PlayAction { card: Breakthrough }` paid its own
    /// RB p.15 "1 CA" with a military action instead (Maximilien
    /// Robespierre's CoL p.12 exception, `legal::
    /// breakthrough_robespierre_ma_fundable`'s own doc comment has the full
    /// citation and the corroborating BGO games) because civil was already
    /// exhausted. Consumed by `apply::h_revolution`'s own `robespierre`
    /// branch the instant the revolution it funded actually resolves --
    /// that branch otherwise assumes Breakthrough's 1 CA came out of civil
    /// and backs 1 out of this turn's civil spend to avoid double-counting
    /// it; skipping that correction here (nothing was ever spent from civil
    /// to correct for) is the other half of the same fix. Reset at end of
    /// turn like every other once-per-turn flag on this struct, in case the
    /// order resolved to an ordinary Develop instead and left it unconsumed.
    pub breakthrough_ma_funded: bool,
    /// SOME leader was in play this turn and has since been replaced by a new
    /// one (`apply::h_play_leader`) -- the condition Taj Mahal's printed 2015
    /// text reads: "If you replaced your leader this turn, taking this wonder
    /// costs you 2 civil actions less." Strictly weaker than
    /// `hammurabi_replaced_this_turn` (which names WHO was replaced, because
    /// only Hammurabi's benefit survives him); both are per-turn and both are
    /// cleared in `economy::end_of_turn`. Playing a FIRST leader onto an empty
    /// slot is not a replacement and does not set this.
    pub replaced_leader_this_turn: bool,
    pub churchill_used: bool,
    pub bach_upgrade_used: bool,
    pub ocean_liners_used: bool,
    /// Homer's "extra 1 resource for building and upgrading military units"
    /// (`sources/bga_throughtheages_material.inc.php`'s own leader text,
    /// "On your turn, you have an extra 1 resource for building and
    /// upgrading military units") -- ONE resource per turn, not one per
    /// build/upgrade action. `costs::homer_unit_discount` previously applied
    /// unconditionally to every single unit build/upgrade in a turn (fixed
    /// once, from a post-payment gain to a pre-payment discount, in the
    /// commit `costs.rs`'s own doc comment on `homer_unit_discount`
    /// describes -- but that fix never added a per-turn cap, so a player
    /// with Homer who built/upgraded TWO units in one turn got the discount
    /// TWICE). Corpus-confirmed wrong: real BGO journals with Homer active
    /// and 2+ same-turn unit build/upgrade lines show the `"loses N
    /// military resource"` clause on AT MOST ONE of them (measured across
    /// the full 1,011-game corpus: 45 such turns, 0 with the clause on more
    /// than one action -- see `costs::spend_homer_unit_discount`'s own test
    /// for the minimal repro, real game `7521819` round 6: Orange upgrades
    /// Warrior->Swordsmen TWICE in the same turn, only the FIRST shows
    /// `"loses 1 military resource"`, the second pays full price with
    /// `"spends 1 resource"`). This is an ENGINE-facing bug, not a replayer
    /// one: `legal.rs`'s own affordability check reads
    /// `costs::homer_unit_discount` too, so an un-capped discount let the
    /// engine consider builds/upgrades legal that a real human opponent
    /// could not actually afford. Cleared alongside the other once-per-turn
    /// flags above in `economy::end_of_turn`.
    pub homer_used_this_turn: bool,
    pub caesar_double_politics_used: bool,
    /// Julius Caesar's once-per-game second political action: set while the
    /// FIRST political action of a turn has left the politics phase open for
    /// a second one (`apply::end_politics`, mirroring `engine/actions.py::
    /// _end_politics`); cleared the moment that second action resolves (and
    /// `caesar_double_politics_used` is set instead), or as a backstop at
    /// end of turn (`economy::end_of_turn`) / on resignation (`apply::
    /// h_resign`).
    pub caesar_second_politics: bool,
    pub skip_next_politics: bool,
    /// Joan of Arc: the name of the current-events card she looked at when
    /// this politics phase began (`CardId::NONE` if she is not the leader,
    /// or the deck was empty to look at). Bot-facing information only, no
    /// rule effect of its own (`game::start_turn`, mirroring `engine/
    /// events.py::peek_top_event`); cleared wherever `caesar_second_politics`
    /// is (same three places).
    pub peeked_event: CardId,
    /// Rebellion.
    pub ca_penalty_next_turn: i8,
    /// Resource discount pool for military unit builds/upgrades this turn
    /// (Patriotism / Wave of Nationalism / Military Build-Up, §3.11).
    pub mil_discount: i16,
    /// The exact twin of `mil_discount`, for the same reason: Churchill's
    /// military option is "3 science usable only to develop military unit
    /// technologies", which is not 3 science.
    pub mil_sci_discount: i16,
    /// Event-granted cost discount (§ "Development of Civil Life", the one
    /// Age A event that prints `oneTimeDiscount`). See [`OneTimeDiscount`]
    /// for the one thing a reader needs to know about it: it is a SINGLE
    /// mutually-exclusive one-time grant -- using ANY ONE of its three
    /// candidate discounts exhausts ALL THREE, via [`OneTimeDiscount::exhaust`].
    pub one_time_discount: OneTimeDiscount,

    /// Trade Routes Agreement (§5.9, `PactBlock::food_as_resource`/
    /// `resource_as_food`, `bga_throughtheages_material.inc.php`: "Civilization
    /// A can use 1 food as 1 resource during its turn" / "Civilization B can
    /// use 1 resource as 1 food during its turn"). These count conversions
    /// ALREADY SPENT this turn, one counter per direction because a player
    /// can hold both allowances at once (they are party to more than one
    /// pact -- see `docs/RULES_SPEC.md` §5.9: "You may be party to many
    /// pacts but have only one in your own area"). Legal only while `<
    /// effects::state_stats(..).food_as_resource`/`resource_as_food`, which
    /// sums every live pact's grant, so two copies of the same-direction
    /// grant (from two different partners) really do allow two conversions.
    /// Cleared in `economy::end_of_turn`, same as `replaced_leader_this_turn`.
    pub trade_food_as_resource_used_this_turn: u8,
    /// The `resource_as_food` twin of the field above (Trade Routes' OTHER
    /// direction: spend a resource to gain food).
    pub trade_resource_as_food_used_this_turn: u8,

    pub resigned: bool,

    /// Blue tokens actually placed on this player's Farm cards, grouped by
    /// denomination (printed food value) -- see [`TokenBank`]'s own doc.
    /// `economy::blue_used`/`gain_food`/`pay_food`/`credit_production`'s
    /// shared body maintains this; nothing else should write it directly.
    pub food_tokens: TokenBank,
    /// The Mine-card twin of [`Self::food_tokens`] (printed resource value).
    pub resource_tokens: TokenBank,
}

/// A player's blue tokens, bucketed by the denomination (printed farm food /
/// mine resource value) of whatever card they are placed on -- see
/// `economy.rs`'s module doc for the corpus bug this closes (RESOURCE_ORACLE
/// Group A, `analysis/worker_notes_2026-08-14/corrskip__CORRSKIP.txt`/
/// `corrskip2__CORRSKIP2.txt`).
///
/// RULES_SPEC §6.4 / Code of Laws p.11: a token is worth whatever card it is
/// ACTUALLY sitting on; tokens may move to a LOWER-value card ("making
/// change") but NEVER to a higher one. The engine used to derive `blue_used`
/// fresh every call from the scalar `p.food`/`p.resources` total, greedily
/// re-packed into whatever denominations exist RIGHT NOW -- which silently
/// assumes every player has always achieved perfect consolidation onto their
/// best available card, something the "never move up" rule makes impossible
/// once a higher card enters play after older stock was already placed.
/// `TokenBank` instead tracks the tokens actually placed, by value, so old
/// stock stays exactly where a real player's tokens would sit.
///
/// Per-DENOMINATION counts, not per-card identity: two cards printing the
/// same food/resource value are interchangeable for corruption purposes (only
/// the total occupied-token COUNT feeds the bank, `economy::corruption`),
/// and nothing in RULES_SPEC/Code of Laws/the FAQ caps how many tokens one
/// card may hold -- see `corrskip2__CORRSKIP2.txt` for the citation search.
/// Same fixed 8-slot capacity as `economy::Denoms` (DESIGN.md rule 3: no
/// `Vec`) for the same reason -- at most four farm/mine levels exist today.
///
/// `_len == 0` (the `Default`) also doubles as "never tracked" -- a
/// hand-built `PlayerState` in a unit test that sets `p.food`/`p.resources`
/// directly and never calls a real gain/pay path. `economy::blue_used`
/// reconciles that gap by treating any UNTRACKED amount (`p.food` above
/// `bank_total_value`) as if it needed fresh greedy placement right now --
/// i.e. exactly today's pre-fix formula -- so every test that never touches
/// this field keeps its old, already-asserted behavior unchanged. Real
/// gameplay (`replay`/self-play) always drives the full `gain_food`/
/// `pay_food`/`credit_production` chokepoints, so for those the tracked
/// total and `p.food` never drift apart and the reconciliation fallback is a
/// no-op.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct TokenBank {
    pub denoms: [u8; 8],
    pub counts: [u8; 8],
    pub len: u8,
}

/// The one-time cost discount `events::apply_extras` writes onto EVERY
/// qualifying player when the Age A event **Development of Civil Life**
/// resolves. Its printed payload is exactly `{"increasePopulation": {"food":
/// 1}, "build": {"resources": 1}, "developTechnology": {"science": 1}}` and
/// exactly one card in `data/*.json` writes it, so this is three scalars
/// rather than a dict: the schema is closed, and a `HashMap` here would be a
/// growable allocation inside a `Clone`-as-memcpy `GameState` (DESIGN.md
/// rule 3) bought with nothing.
///
/// **A single mutually-exclusive choice, NOT three independent discounts.**
/// The card text (`data/cards_military_actions.json`, "Development of Civil
/// Life"): *"Players may **either** increase its population; **or** build a
/// farm, mine or urban building; **or** develop a technology. It costs 1
/// food or 1 resource or 1 science less than usual."* (BGO's own journal
/// text for this event, verbatim -- see below) -- an "either/or/or" list is
/// one choice among three options, not a grant of all three. All three
/// scalar fields ARE still populated at once when the event resolves
/// (below), because it is not yet known which of the three the player will
/// eventually pick -- but the FIRST of the three that is actually spent by a
/// real action must exhaust the OTHER TWO as well, via
/// [`OneTimeDiscount::exhaust`], called from every consumption site
/// (`economy::increase_population`'s `consume_one_time` branch,
/// `apply::do_build`, `apply::h_develop`).
///
/// **Fixed twice, and the second fix reversed part of the first.** A
/// 2026-08-05 fix (previously documented here) established that the field
/// must be cleared AT ALL (before it, nothing ever cleared it, so it applied
/// forever) but wrongly modeled it as three INDEPENDENT one-time discounts,
/// each cleared only when ITS OWN field was spent -- a plausible-looking
/// reading of the JSON schema (three separate sub-keys) that turns out to
/// contradict the card's own English text once read as a list of
/// alternatives. **Confirmed wrong by replaying real BGO games**
/// (`docs/REPLAY.md`, fifth pass): in three separate sample games a human
/// spent ONE of the three discounts (e.g. developed a technology 1 science
/// under its printed cost) and later, the SAME turn or a later one, paid
/// FULL, undiscounted price for an action of a DIFFERENT type (e.g. built a
/// Mine at its full printed cost) that this engine's old independent-grants
/// model predicted should ALSO have been discounted -- direct evidence the
/// real rule exhausts all three the instant any one is used. Every one of
/// the 8/24 sample games that stopped on `docs/REPLAY.md`'s "build cost
/// mismatch (unmodeled discount)" category was this exact bug: the engine
/// computed a cost 1 LOWER than the human actually paid, because it still
/// thought an already-exhausted discount was live.
///
/// Each field is in the units of the thing discounted, and each is checked
/// by a DIFFERENT rule (`build_resources` only to `URBAN_OR_PRODUCTION`
/// cards, `develop_science` to every technology including governments,
/// `pop_food` to §6.1's population increase), which is why they are three
/// named fields and not one number -- but [`exhaust`](Self::exhaust) always
/// clears all three together.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OneTimeDiscount {
    /// `build.resources` -- off a farm/mine/urban building's build cost.
    pub build_resources: i16,
    /// `developTechnology.science` -- off any technology's develop cost.
    pub develop_science: i16,
    /// `increasePopulation.food` -- off the §6.1 population-increase cost.
    pub pop_food: i16,
}

impl OneTimeDiscount {
    /// Clears all three candidate discounts at once. Call this the instant
    /// ANY ONE of them is actually spent by a real action -- see this
    /// struct's own doc comment for why using one must exhaust the other
    /// two, not just its own field.
    pub fn exhaust(&mut self) {
        *self = OneTimeDiscount::default();
    }
}

// ==================================================== the decision queue ==
//
// Ports the STATE half of `engine/interact.py` (the behaviour half is
// `interact.rs`). Two containers, and the difference between them is the
// whole design:
//
//   * `GameState::pending` -- a STACK of open decisions. The top one owns the
//     move, and its `player` is `GameState::decider()`, which is not
//     necessarily `current`: a colonization auction, an aggression defense
//     and a pact offer are all answered by somebody other than the player
//     whose turn it is.
//   * `GameState::queue` -- a FIFO of DEFERRED sub-effects. `interact::
//     run_queue` drains it only while `pending` is empty, so a queued item
//     that opens a decision suspends the drain until that decision is
//     answered.
//
// Python spells both as lists of plain JSON dicts (so `GameState.to_dict()`
// stays serializable) and dispatches on a `"tag"` string. Here they are
// enums, for DESIGN.md rule 5's reason and one more: Python's dispatch
// tables (`interact._CHOICE`, `interact._QUEUE_ITEMS`) are `dict.get(tag)`
// with `if fn is None: return` -- a decision whose tag is not in the table
// is SILENTLY DROPPED. That is precisely this project's recurring bug class,
// and an exhaustive `match` over these enums makes it a compile error.

/// One option inside an event's `choose` block (§5.3, `engine/events.py::
/// _queue_decisions`).
///
/// The base game prints exactly ONE such block -- `Development of Markets`,
/// `{"choose": [{"food": 2}, {"resources": 2}]}` (verified over all of
/// `data/*.json`, 2026-08-05) -- so this is the closed vocabulary, not a
/// general gain block. `fixtures.rs` rejects any other key rather than
/// dropping it, so a future card that prints a third key fails loudly here
/// instead of silently gaining nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GainOption {
    pub food: i16,
    pub resources: i16,
}

/// The base game's only `choose` block offers two options; four is headroom.
pub const MAX_GAIN_OPTIONS: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GainOptions {
    items: [GainOption; MAX_GAIN_OPTIONS],
    len: u8,
}

impl GainOptions {
    pub const fn new() -> Self {
        GainOptions { items: [GainOption { food: 0, resources: 0 }; MAX_GAIN_OPTIONS], len: 0 }
    }

    #[inline]
    pub fn push(&mut self, o: GainOption) {
        debug_assert!((self.len as usize) < MAX_GAIN_OPTIONS, "gain option list overflow");
        self.items[self.len as usize] = o;
        self.len += 1;
    }

    #[inline]
    pub fn as_slice(&self) -> &[GainOption] {
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

impl Default for GainOptions {
    fn default() -> Self {
        Self::new()
    }
}

/// One option of an open `choice` decision.
///
/// Python's options are raw JSON -- a card name, a row index, a bare keyword
/// string, a `{"food": 2}` dict, or a move tuple -- all in the same list, and
/// each `_c_*` handler re-interprets its own. Here the five shapes are five
/// variants, so a handler that reaches for the wrong one does not compile.
///
/// The list is POSITIONAL and its order is part of the contract: `Move::
/// Choose { n }` is an index into it, every bot in this project breaks ties
/// to the lowest index, and `interact::WAR_TECH_SCIENCE_IDX` is a documented
/// dependency on option 0 specifically.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChoiceOption {
    /// A card: which one to discard / destroy / build / steal / lose.
    Card(CardId),
    /// A `card_row` slot index (`take_row` only).
    Slot(u8),
    /// A concrete move satisfying an action card's ordered action (§3.11).
    /// Python spells these `["build", "Philosophy"]`.
    Move(crate::moves::Move),
    /// One branch of an event's `choose` block.
    Gain(GainOption),
    /// One of the bare keyword options; see [`Keyword`].
    Word(Keyword),
}

/// The bare-string options Python mixes into an option list. Closed: these
/// are every non-card, non-index, non-dict option string `engine/interact.py`
/// ever puts in an `options` list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keyword {
    /// `take_row`: stop taking cards.
    Stop,
    /// `free_build`: decline the free build.
    Skip,
    /// `war_tech`: take the remaining advantage as science (`interact::
    /// WAR_TECH_SCIENCE_IDX` -- always option 0).
    Science,
    /// `food_or_res`.
    Food,
    /// `food_or_res`.
    Resources,
    /// `pact_offer`.
    Accept,
    /// `pact_offer`.
    Refuse,
    /// `infiltrate`: remove the rival's leader.
    Leader,
    /// `infiltrate`: remove the rival's unfinished wonder.
    Wonder,
}

/// Options one `choice` decision can offer.
///
/// Bound picked the way `MAX_DECK`/`MAX_TABLEAU` were: from the data, not
/// from a guess. The largest option list any producer in `interact.py` can
/// build is the tableau-derived one (`_q_destroy_own` / `_q_lose_pop`), which
/// offers one option per DISTINCT worker-holding card in the tableau; the
/// base game prints 34 such cards in total (4 farms, 4 mines, 4 labs, 3
/// temples, 3 libraries, 3 arenas, 3 theaters, 4 infantry, 3 cavalry, 2
/// artillery, 1 air -- counted from `data/*.json` on 2026-08-05), which no
/// player can exceed. The next largest are `take_row` at `ROW_SIZE + 1` = 14
/// and `discard_military`, bounded by `MAX_HAND` (24). 40 is that ceiling
/// plus headroom, and `push` debug-asserts it rather than assuming it. The
/// largest list actually observed across the 3383 fixture states is 14.
pub const MAX_OPTIONS: usize = 40;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OptionList {
    items: [ChoiceOption; MAX_OPTIONS],
    len: u8,
}

impl OptionList {
    pub const fn new() -> Self {
        OptionList { items: [ChoiceOption::Slot(0); MAX_OPTIONS], len: 0 }
    }

    #[inline]
    pub fn push(&mut self, o: ChoiceOption) {
        debug_assert!((self.len as usize) < MAX_OPTIONS, "choice option list overflow");
        self.items[self.len as usize] = o;
        self.len += 1;
    }

    #[inline]
    pub fn as_slice(&self) -> &[ChoiceOption] {
        &self.items[..self.len as usize]
    }

    #[inline]
    pub fn get(&self, i: usize) -> Option<ChoiceOption> {
        self.as_slice().get(i).copied()
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

impl Default for OptionList {
    fn default() -> Self {
        Self::new()
    }
}

/// What an open `choice` decision IS, plus the context its resolution needs.
///
/// One variant per key of Python's `interact._CHOICE` dispatch table -- the
/// tag and the `ctx` dict folded into one value, because they were never
/// independent: `ctx["victim"]` is meaningful for `raid`/`annex`/`infiltrate`/
/// `war_tech` and meaningless everywhere else, and a flat struct carrying
/// every key would let a handler read a field its own tag never set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChoiceKind {
    /// `_c_gain_block`: pick one branch of an event's `choose` block.
    GainBlock,
    /// `_c_free_civil`: perform an action card's ordered action (§3.11).
    FreeCivil { discount: i16 },
    /// `_c_food_or_res`: `gainFoodOrResources`, `n` of whichever is picked.
    FoodOrRes { n: i16 },
    /// `_c_free_build`: an event's free build, or decline.
    FreeBuild,
    /// `_c_destroy_own`: destroy one of your own worker-holding cards.
    DestroyOwn,
    /// `_c_lose_pop`: lose 1 population when no unused worker is available.
    LosePop,
    /// `_c_lose_colony`.
    LoseColony,
    /// `_c_flip_wonder`.
    FlipWonder,
    /// `_c_discard_military`: §6.6 step 1 / an event's discard.
    DiscardMilitary,
    /// `_c_raid`: attacker picks the urban building to destroy. `is_last`
    /// marks the final age-tier bracket of this raid use (Raid I has one,
    /// Raid II/III two) -- the card grants "half the resources needed to
    /// build THEM, rounded up" ONCE for the whole raid, so only the last
    /// bracket flushes `PlayerState::raid_loot_pending` into real resources.
    Raid { victim: u8, loot: bool, is_last: bool },
    /// `_c_annex`: attacker picks which colony changes hands.
    Annex { victim: u8 },
    /// `_c_infiltrate`: remove the rival's leader or unfinished wonder,
    /// `per` culture per level.
    Infiltrate { victim: u8, per: u8 },
    /// `_c_pact_offer`: the partner accepts or refuses (§5.9). `owner` holds
    /// the card, `a`/`b` are who takes which printed block -- four separate
    /// indices, exactly as [`Pact`] documents.
    PactOffer { owner: u8, card: CardId, a: u8, b: u8 },
    /// `_c_take_row`: International Agreement, spend up to `budget` civil
    /// actions taking cards.
    TakeRow { budget: i16 },
    /// `_c_war_tech`: War over Technology spoils (§5.7).
    WarTech { victim: u8, budget: i16 },
    /// Aggression: Plunder's "take up to N food and/or resources" (§5.4.6).
    /// FAQ p.7: the SPLIT between the two banks is the attacker's choice,
    /// not a fixed order -- `options` (built by `interact::
    /// plunder_split_options`) is every way to reach the maximum takeable
    /// total, one `ChoiceOption::Gain` per split. `victim` is who pays it.
    PlunderSplit { victim: u8 },
    /// Raiders'/Foray's `foodAndOrResources` gain/lose block (§5.3): a
    /// SELF-directed split, not an attacker's -- the targeted player picks
    /// their own food/resources split, clockwise from the revealer when
    /// more than one player is targeted (`QueueItem::FoodOrResSplit`'s own
    /// doc). `lose` is Raiders' direction (drain, floored at zero, capped
    /// by `interact::plunder_split_options` reused against the player's
    /// OWN banks); `!lose` is Foray's (gain, one `ChoiceOption::Gain` per
    /// split of the total with no cap at option-build time -- exactly
    /// `PlunderSplit`'s own attacker-gain precedent above: blue-token
    /// availability caps what actually lands, per bank, at resolution via
    /// `economy::gain_food`/`gain_resources`, not by filtering options).
    /// Distinct from `FoodOrRes` above: that one is Reserves' bare "N food
    /// OR N resources" (all-or-nothing, `gainFoodOrResources`), this one is
    /// a SPLIT of a fixed total between the two (`foodAndOrResources`).
    FoodOrResSplit { lose: bool },
}

/// An open `choice`: somebody must pick one of `options`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Choice {
    pub player: u8,
    pub kind: ChoiceKind,
    pub options: OptionList,
}

/// A colonization auction in progress (§11.1-11.2).
///
/// `active` is the still-bidding players in clockwise order from the
/// revealer, and `pos` indexes it; a pass REMOVES an entry, so the order is
/// load-bearing and the removal must preserve it (`_auction_move`'s
/// `del pend["active"][pend["pos"]]`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Auction {
    pub card: CardId,
    active: [u8; MAX_PLAYERS],
    n_active: u8,
    pub pos: u8,
    pub bid: u8,
    /// Highest bidder so far, `None` before any bid (Python's `high: None`).
    pub high: Option<u8>,
    /// Whose move it is -- always `active[pos]` while the auction runs.
    pub player: u8,
}

impl Auction {
    pub fn new(card: CardId, active: &[u8]) -> Self {
        debug_assert!(!active.is_empty(), "auction with no bidders");
        let mut a = [0u8; MAX_PLAYERS];
        a[..active.len()].copy_from_slice(active);
        Auction {
            card,
            active: a,
            n_active: active.len() as u8,
            pos: 0,
            bid: 0,
            high: None,
            player: active[0],
        }
    }

    /// Rebuild an auction from a dumped mid-auction state (`fixtures.rs`).
    /// `pos`/`bid`/`high`/`player` are whatever the dump recorded, not
    /// re-derived: a snapshot taken between two bids has all four already.
    pub fn restore(card: CardId, active: &[u8], pos: u8, bid: u8, high: Option<u8>, player: u8) -> Self {
        let mut a = [0u8; MAX_PLAYERS];
        a[..active.len()].copy_from_slice(active);
        Auction { card, active: a, n_active: active.len() as u8, pos, bid, high, player }
    }

    #[inline]
    pub fn active(&self) -> &[u8] {
        &self.active[..self.n_active as usize]
    }

    /// Drop the bidder at `i`, preserving the order of the rest.
    pub fn drop_at(&mut self, i: usize) {
        let n = self.n_active as usize;
        debug_assert!(i < n);
        self.active.copy_within(i + 1..n, i);
        self.n_active -= 1;
    }
}

/// An aggression's defense decision (§5.4.4).
///
/// `atk`/`dfn` are the two totals being compared; `dfn` starts at the
/// defender's printed strength and grows by [`crate::interact::
/// defense_points`] per committed card. `spent` counts committed cards
/// against `budget` military actions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Defense {
    pub player: u8,
    pub attacker: u8,
    pub card: CardId,
    pub atk: i32,
    pub dfn: i32,
    pub spent: u8,
    pub budget: u8,
}

/// Cards one colonization force can involve.
///
/// `unit_pool` puts one entry in per WORKER on a military unit, so the pool
/// is bounded by the yellow tokens a player owns (18 in the base game, and
/// only some of them are ever on units); `bonus_pool` is bounded by the
/// military hand, i.e. `MAX_HAND`. `MAX_HAND` covers both with room to
/// spare, and each of the four lists debug-asserts its own bound.
pub const MAX_FORCE: usize = MAX_HAND;

/// The auction winner building the force it sacrifices (§11.3-11.5).
///
/// Six lists, not four: `pool`/`bpool`/`dpool` are what is still AVAILABLE
/// and `units`/`bonuses`/`discards` are what has been COMMITTED, and both
/// halves are read -- `interact::colonize_moves` enumerates the pools,
/// `interact::force_value` scores the commitments. Each commitment is paid
/// the moment it is made (see `interact::colonize`'s doc comment for why
/// that is not an optimization).
///
/// `discards`/`dpool` are James Cook's addition (`engine/interact.py`'s
/// `pend["discards"]`/`pend["dpool"]`, ded32dd): empty for every other
/// leader, since `interact::cook_pool` only ever returns cards while Cook is
/// in play.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Colonize {
    pub player: u8,
    pub card: CardId,
    pub bid: u8,
    pub units: CardList<MAX_FORCE>,
    pub bonuses: CardList<MAX_FORCE>,
    pub discards: CardList<MAX_FORCE>,
    pub pool: CardList<MAX_FORCE>,
    pub bpool: CardList<MAX_FORCE>,
    pub dpool: CardList<MAX_FORCE>,
}

/// One open decision. Python's `state.pending[-1]`, with its `"kind"` string
/// made a variant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pending {
    Choice(Choice),
    Auction(Auction),
    Defense(Defense),
    Colonize(Colonize),
}

impl Pending {
    /// The player who must answer this decision. Python reads `pend["player"]`
    /// off whichever dict is on top; every kind carries one.
    #[inline]
    pub fn player(&self) -> u8 {
        match self {
            Pending::Choice(c) => c.player,
            Pending::Auction(a) => a.player,
            Pending::Defense(d) => d.player,
            Pending::Colonize(c) => c.player,
        }
    }
}

/// Depth of the decision stack.
///
/// Python's `state.pending` is a stack, but it can only ever hold one entry
/// in practice and the reason is structural, not incidental: `run_queue`
/// drains the deferred queue only `while not state.pending`, and every
/// `push_choice` that happens DURING a resolution (`_offer_war_tech`'s
/// re-offer, `_offer_take_row`'s re-offer) runs after `apply_pending` has
/// already popped. Measured: across all 3383 fixture states the depth is 0
/// or 1, never 2. Four is headroom, and `push` debug-asserts it rather than
/// trusting the argument.
pub const MAX_PENDING: usize = 4;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingStack {
    items: [Option<Pending>; MAX_PENDING],
    len: u8,
}

impl PendingStack {
    pub fn new() -> Self {
        PendingStack { items: [None, None, None, None], len: 0 }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len as usize
    }

    /// The decision that owns the next move (`interact.top`).
    #[inline]
    pub fn top(&self) -> Option<&Pending> {
        if self.len == 0 {
            None
        } else {
            self.items[self.len as usize - 1].as_ref()
        }
    }

    #[inline]
    pub fn top_mut(&mut self) -> Option<&mut Pending> {
        if self.len == 0 {
            None
        } else {
            self.items[self.len as usize - 1].as_mut()
        }
    }

    pub fn push(&mut self, p: Pending) {
        debug_assert!((self.len as usize) < MAX_PENDING, "pending stack overflow");
        self.items[self.len as usize] = Some(p);
        self.len += 1;
    }

    pub fn pop(&mut self) -> Option<Pending> {
        if self.len == 0 {
            return None;
        }
        self.len -= 1;
        self.items[self.len as usize].take()
    }
}

impl Default for PendingStack {
    fn default() -> Self {
        Self::new()
    }
}

/// An event's `freeBuild` spec (`engine/events.py::_queue_decisions`).
///
/// Both base-game printings name a specific card (`Development of Warfare` ->
/// Warriors, `Development of Religion` -> Temple age A), so `card` is always
/// set today; `age`/`kind`/`cost` are the other keys the spec may print and
/// are carried because `_q_free_build` filters on all of them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FreeBuildSpec {
    /// `CardId::NONE` when the spec names no specific card.
    pub card: CardId,
    pub age: Option<Age>,
    pub kind: Option<CardType>,
    pub cost: u16,
}

/// The gain half of a yellow action card, deferred behind its ordered action
/// (§3.11). Exactly Python's `_GAIN_KEYS`, in that order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CardGains {
    pub science: i16,
    pub culture: i16,
    pub food: i16,
    pub resources: i16,
    pub population: i16,
    /// `gainFoodOrResources`: opens a `FoodOrRes` choice rather than paying
    /// out directly. `0` means the card does not print the key at all, which
    /// is also what Python's `if "gainFoodOrResources" in eff` tests -- no
    /// base-game card prints an explicit zero here.
    pub food_or_resources: i16,
}

/// One deferred sub-effect. Python's `state.queue` entries, with the `"tag"`
/// string made a variant and the per-tag keys made its payload.
///
/// `player` is on every variant because `interact::_run_item` reads it before
/// dispatching (and skips the item entirely if that player has resigned).
///
/// Python registers a sixteenth handler, `"gains"` (`_q_gains` ->
/// `events.apply_gains`), that NOTHING enqueues -- `grep -rn '"tag": "gains"'
/// engine/` finds only the registry entry itself (2026-08-05). It is dead,
/// so it has no variant here; see `interact.rs`'s top doc comment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueItem {
    /// `_q_choose`: offer an event's `choose` block.
    Choose { player: u8, options: GainOptions },
    /// `_q_free_civil`: offer the moves satisfying an action card's order.
    FreeCivil { player: u8, kind: crate::card_table::FreeCivilActionValue, discount: i16, revolt_ok: bool },
    /// `_q_card_gains`: the gain half of an action card, behind its order.
    CardGains { player: u8, gains: CardGains },
    /// `_q_free_build`.
    FreeBuild { player: u8, spec: FreeBuildSpec },
    /// `_q_destroy_own`.
    DestroyOwn { player: u8 },
    /// `_q_lose_pop`: `n` losses, re-queued one at a time behind a decision.
    LosePop { player: u8, n: u8 },
    /// `_q_lose_colony`.
    LoseColony { player: u8 },
    /// `_q_flip_wonder`: `ages` is a bitmask over `Age as u8` (Python passes
    /// a list of age strings, default `["A", "I"]`).
    FlipWonder { player: u8, ages: u8 },
    /// `_q_discard_military`: `n` discards, re-queued like `LosePop`.
    DiscardMilitary { player: u8, n: u8 },
    /// `_q_end_of_turn`: resume §6.6 after the discard decision.
    EndOfTurn { player: u8 },
    /// `_q_raid`: destroy one of `victim`'s urban buildings, up to `max_age`.
    /// `is_last` mirrors `ChoiceKind::Raid`'s own field -- see its doc.
    Raid { player: u8, victim: u8, max_age: Age, no_loot: bool, is_last: bool },
    /// `_q_annex`.
    Annex { player: u8, victim: u8 },
    /// `_q_infiltrate`.
    Infiltrate { player: u8, victim: u8, per: u8 },
    /// `_q_take_row`.
    TakeRow { player: u8, budget: i16 },
    /// §5.3's `foodAndOrResources` gain/lose block (Raiders, Foray): the
    /// TARGETED player's own choice of how to split `amount` between food
    /// and resources, not `events::food_or_resources`'s old fixed
    /// "resources first" formula -- see `ChoiceKind::FoodOrResSplit`'s own
    /// doc for the full rationale. One of these is enqueued per targeted
    /// player, in the same clockwise-from-the-revealer order
    /// `resolve_count_targets` already calls `apply_gains_block` in, so
    /// RULES_SPEC.md §5.3's "multiple-player decisions resolve clockwise
    /// from the revealing player" falls out of that existing order rather
    /// than needing new ordering logic here.
    FoodOrResSplit { player: u8, amount: i16, lose: bool },
}

impl QueueItem {
    #[inline]
    pub fn player(&self) -> u8 {
        use QueueItem::*;
        match *self {
            Choose { player, .. }
            | FreeCivil { player, .. }
            | CardGains { player, .. }
            | FreeBuild { player, .. }
            | DestroyOwn { player }
            | FoodOrResSplit { player, .. }
            | LosePop { player, .. }
            | LoseColony { player }
            | FlipWonder { player, .. }
            | DiscardMilitary { player, .. }
            | EndOfTurn { player }
            | Raid { player, .. }
            | Annex { player, .. }
            | Infiltrate { player, .. }
            | TakeRow { player, .. } => player,
        }
    }
}

/// Deferred sub-effects outstanding at once.
///
/// The producers that enqueue more than one item are all bounded and small:
/// `destroyOneUrbanBuildingOfEachOpponent` enqueues one `Raid` per opponent
/// (at most `MAX_PLAYERS - 1` = 3), `destroyOwnBuilding`/`loseColony` enqueue
/// their printed count (1 in the base game's data), and an action card
/// enqueues exactly two (`FreeCivil` then `CardGains`). The deepest queue
/// observed across the 3383 fixture states is 3. Sixteen is that ceiling with
/// generous headroom, debug-asserted on push.
pub const MAX_QUEUE: usize = 16;

/// FIFO. Python appends and `pop(0)`s, and `_q_lose_pop`/`_q_discard_military`
/// additionally `insert(0, ...)` to put their own remainder at the FRONT --
/// so this needs both ends, and the order is play-affecting (§5.3 resolves
/// deferred effects in clockwise order). A ring buffer rather than a shifting
/// array so `push_front` is not a memmove.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Queue {
    items: [Option<QueueItem>; MAX_QUEUE],
    head: u8,
    len: u8,
}

impl Queue {
    pub const fn new() -> Self {
        Queue { items: [None; MAX_QUEUE], head: 0, len: 0 }
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
    fn slot(&self, i: usize) -> usize {
        (self.head as usize + i) % MAX_QUEUE
    }

    /// `list.append` -- the normal enqueue.
    pub fn push_back(&mut self, item: QueueItem) {
        debug_assert!((self.len as usize) < MAX_QUEUE, "deferred queue overflow");
        let s = self.slot(self.len as usize);
        self.items[s] = Some(item);
        self.len += 1;
    }

    /// `list.insert(0, ...)` -- a handler putting its own remainder back at
    /// the front, ahead of everything already queued.
    pub fn push_front(&mut self, item: QueueItem) {
        debug_assert!((self.len as usize) < MAX_QUEUE, "deferred queue overflow");
        self.head = ((self.head as usize + MAX_QUEUE - 1) % MAX_QUEUE) as u8;
        self.items[self.head as usize] = Some(item);
        self.len += 1;
    }

    /// `list.pop(0)`.
    pub fn pop_front(&mut self) -> Option<QueueItem> {
        if self.len == 0 {
            return None;
        }
        let out = self.items[self.head as usize].take();
        self.head = ((self.head as usize + 1) % MAX_QUEUE) as u8;
        self.len -= 1;
        out
    }

    /// Front-to-back, for tests and diffing. Not on any hot path.
    pub fn iter(&self) -> impl Iterator<Item = QueueItem> + '_ {
        (0..self.len as usize).filter_map(move |i| self.items[self.slot(i)])
    }
}

impl Default for Queue {
    fn default() -> Self {
        Self::new()
    }
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
    /// Event -> which player prepared it (`engine/state.py`'s `seeded_by`).
    /// Bot-evaluator bookkeeping only -- nothing in this file, `legal.rs`,
    /// `apply.rs`, `costs.rs`, `economy.rs` or `effects.rs` reads it, exactly
    /// like Python's original (`apply.rs::h_prepare_event`'s doc comment used
    /// to record this as a gap: "this port has no bot layer and `state.rs`
    /// has no field for it" -- closed now that `bots::counting::event_pool`
    /// is that reader). `[u8; NUM_CARDS]` rather than a map, dense/sparse
    /// exactly like `Tableau::index` above: [`NOT_SEEDED`] ("nobody", the
    /// overwhelming majority of entries) or a real player index. Write-once
    /// in both engines -- `h_prepare_event` sets an entry and nothing ever
    /// clears one (Python's `journal.py::touch` docstring shows a `del` as an
    /// example of the *mechanism*, but no call site in `actions.py`/
    /// `events.py` ever performs one on `seeded_by`) -- so a stale entry for
    /// an event that has since resolved into `past_events` is harmless: every
    /// reader only ever looks an entry up for a name still sitting in
    /// `current_events`/`future_events`.
    pub seeded_by: [u8; NUM_CARDS],
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

    /// Open decisions, innermost last (`engine/interact.py`). While this is
    /// non-empty the top entry owns the next move and [`GameState::decider`]
    /// -- NOT `current` -- says whose it is. See "the decision queue" above.
    pub pending: PendingStack,
    /// Deferred sub-effects, resolved FIFO once `pending` empties (§5.3).
    pub queue: Queue,

    /// Instrumentation only -- read by `replay_common.rs`'s culture-oracle
    /// checkpoint, nothing in this file, `legal.rs`, `apply.rs`, `costs.rs`
    /// or `effects.rs` reads it, exactly like `seeded_by` above. `idx`'s own
    /// `culture` total the INSTANT `economy::end_of_turn`'s production steps
    /// (2-5) finish for them, captured by `game::resume_end_turn` BEFORE the
    /// immediately-following `advance_turn` call can move it further -- e.g.
    /// a war that resolves at the START of the NEXT player's turn (§5.7),
    /// which can add to or subtract from a culture total that was already
    /// correct a few statements earlier. Without this, any checkpoint taken
    /// after `resume_end_turn` returns can read a value that has already
    /// been through a DIFFERENT player's entire start-of-turn sequence.
    pub last_end_of_turn_culture: [Option<u16>; MAX_PLAYERS],

    /// [`GameState::last_end_of_turn_culture`]'s twin for science -- same
    /// snapshot-before-`advance_turn` timing, same consumer
    /// (`replay_common.rs`'s science oracle, `SCIENCE_ORACLE`-gated, mirrors
    /// the culture oracle exactly). Instrumentation only.
    pub last_end_of_turn_science: [Option<u16>; MAX_PLAYERS],

    /// [`GameState::last_end_of_turn_culture`]'s twin for resources -- same
    /// snapshot-before-`advance_turn` timing, same consumer
    /// (`replay_common.rs`'s resource oracle, `RESOURCE_ORACLE`-gated).
    /// Instrumentation only.
    pub last_end_of_turn_resources: [Option<u16>; MAX_PLAYERS],
}

impl GameState {
    /// Did player `idx` place `card` face down in an event pile themselves?
    ///
    /// The one way to ask, because the event piles can hold [`CardId::NONE`]:
    /// a position restored from a save file knows how many cards are face down
    /// without knowing which, and indexing [`Self::seeded_by`] by that sentinel
    /// is an out-of-bounds panic. An unknown card is trivially not one you
    /// placed yourself, so the sentinel has exactly one right answer and this
    /// is where it lives -- two separate readers each guessing it is how the
    /// same crash turned up twice.
    #[inline]
    pub fn seeded_by_player(&self, card: CardId, idx: u8) -> bool {
        !card.is_none() && self.seeded_by[card.0 as usize] == idx
    }

    #[inline]
    pub fn me(&self) -> &PlayerState {
        &self.players[self.current as usize]
    }

    #[inline]
    pub fn me_mut(&mut self) -> &mut PlayerState {
        &mut self.players[self.current as usize]
    }

    /// Index of the player who must choose the next move (`engine/state.py::
    /// decider`). This, not `current`, is who `legal_moves` is generated for:
    /// an aggression defense, a colonization auction and a pact offer are all
    /// answered by somebody else while `current` stays put.
    #[inline]
    pub fn decider(&self) -> u8 {
        match self.pending.top() {
            Some(p) => p.player(),
            None => self.current,
        }
    }

    /// The player `decider` names (`engine/state.py::actor`).
    #[inline]
    pub fn actor(&self) -> &PlayerState {
        &self.players[self.decider() as usize]
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

    /// Build order is play-relevant: `legal_moves` enumerates in tableau
    /// order and the bots break ties by index, so the order picks moves. A
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
