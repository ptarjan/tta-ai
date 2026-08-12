//! Action encoding for the policy head (`docs/NEURAL.md`'s planned third
//! net): a fixed-width, dense feature vector for one legal [`Move`], plus a
//! stable action IDENTITY two different games can compare.
//!
//! ## The policy head scores actions, it does not classify into a fixed
//! vocabulary
//!
//! The legal-move set at a decision point is not a fixed-size, known-in-
//! advance list the way "which of 236 cards" is -- `Move::Aggression { card,
//! target }` alone has (card count) x (player count) shapes, and
//! `Move::Choose { n }`'s meaning depends on whichever `Choice` is
//! currently open. A softmax over a fixed action vocabulary the way an Atari
//! DQN's output layer works is therefore the wrong shape here; what this
//! module builds instead is a per-action feature vector meant to be scored
//! by a shared small head and soft-maxed over WHICHEVER actions are legal
//! right now (`docs/design decisions` on the calling task: "predicts a
//! PRIOR OVER LEGAL ACTIONS", not a second value net). [`encode_action`] is
//! that per-action feature vector; [`ActionId`] is how two encoded actions
//! from different decision points are told apart or matched up.
//!
//! ## Built on `encode::card_vec_array`, not a second per-card vector
//!
//! Every card-bearing field below reuses [`super::encode::card_vec_array`]
//! (made `pub(crate)` for this module) rather than inventing a second dense
//! card representation -- the same principle `encode.rs`'s own top doc
//! comment states for why it reuses `sweep_tableau`/`row_cost`/`pacts_for`.
//!
//! ## The action identity is `Move` itself
//!
//! [`Move`] already derives `PartialEq + Eq` over plain-old-data fields
//! (`CardId` is an index into the closed card table, not a name --
//! `cards.rs` rule 1), so two equal actions are equal BY CONSTRUCTION and
//! two different actions can never collide onto the same identity -- a
//! STRONGER guarantee than a hand-rolled `u64` digest, which could in
//! principle collide. `moves.rs` grew a `Hash` derive alongside this module
//! so `Move` can also serve as a `HashSet`/`HashMap` key. Minting a second
//! `ActionId` newtype around the same data would be exactly the "two
//! registries for one fact" bug class this project keeps finding elsewhere
//! (see `encode.rs`'s "No `lru_cache`" section for the same argument turned
//! on a different pair of registries), so [`ActionId`] is a type alias, not
//! a new type.
//!
//! ## No `GameState` parameter -- not merely undocumented, unreachable
//!
//! [`encode_action`] takes only the acting player's seat and the [`Move`]
//! itself, never a `&GameState`. Every field a `Move` can carry (a
//! [`CardId`], a row slot, a player index, a pact side, a Churchill choice,
//! a raw `u8` count) is already fully public once the move exists at all --
//! `legal_moves`/`pending_moves` only ever hand back MY OWN legal actions
//! (`state.rs`'s own doc comment on `decider`/`actor`), so a card named
//! inside one is a card in my own hand, my own tableau, or the (public)
//! card row, never a rival's hand. Because the signature carries no state
//! reference, there is no path inside this function that could reach a
//! rival's hand or the undealt deck order even by mistake -- the "read only
//! public information" rule (design decision 4 on the calling task) is
//! enforced by the type signature, not merely followed by convention.

use crate::cards::CardId;
use crate::moves::{ChurchillChoice, Move, PactSide, PlayerIdx};
use crate::state::{MAX_OPTIONS, MAX_PLAYERS, ROW_SIZE};

use super::encode::{card_vec_array, CARD_VEC_DIM};

/// The stable action identity two encoded actions are compared or matched
/// by -- see this module's top doc comment for why this is a type alias to
/// [`Move`] rather than a second minted type.
pub type ActionId = Move;

/// One slot per [`Move`] variant, in the enum's own declaration order.
/// [`move_kind_slot`]'s exhaustive `match` (no wildcard arm) is the only way
/// to reach a slot, so a variant `moves.rs` adds without a matching arm here
/// fails to compile rather than silently sharing slot 0 with `Take`.
pub const NUM_MOVE_KINDS: usize = 37;

#[inline]
fn move_kind_slot(mv: &Move) -> usize {
    match mv {
        Move::Take { .. } => 0,
        Move::Build { .. } => 1,
        Move::Develop { .. } => 2,
        Move::Upgrade { .. } => 3,
        Move::WonderStep { .. } => 4,
        Move::Pop => 5,
        Move::PopFree => 6,
        Move::Revolution { .. } => 7,
        Move::PlayLeader { .. } => 8,
        Move::PlayAction { .. } => 9,
        Move::Destroy { .. } => 10,
        Move::PlayTactic { .. } => 11,
        Move::CopyTactic { .. } => 12,
        Move::Aggression { .. } => 13,
        Move::War { .. } => 14,
        Move::OfferPact { .. } => 15,
        Move::CancelPact { .. } => 16,
        Move::PrepareEvent { .. } => 17,
        Move::RemoveLeaderYellow => 18,
        Move::ColumbusColonize { .. } => 19,
        Move::Barbarossa { .. } => 20,
        Move::BachTheater { .. } => 21,
        Move::TradeFoodAsResource => 22,
        Move::TradeResourceAsFood => 23,
        Move::Bid { .. } => 24,
        Move::BidPass => 25,
        Move::Defend { .. } => 26,
        Move::DefendDone => 27,
        Move::SendUnit { .. } => 28,
        Move::SendBonus { .. } => 29,
        Move::SendDiscard { .. } => 30,
        Move::SendDone => 31,
        Move::Choose { .. } => 32,
        Move::Churchill { .. } => 33,
        Move::EndTurn => 34,
        Move::PolPass => 35,
        Move::Resign => 36,
    }
}

/// The fields [`encode_action`] needs beyond the "primary card"
/// [`Move::card`] already extracts (`to`, not `from`, for the two
/// cross-card moves -- see that method's own doc comment). One exhaustive
/// `match` (no wildcard arm) rather than nine separate ones: most `Move`
/// variants carry none of these auxiliary fields, so grouping every such
/// variant onto one "all fields empty" arm keeps this readable while still
/// forcing a compile error -- once, not nine times over -- the moment
/// `moves.rs` grows a variant this match has not been told how to decode.
#[derive(Clone, Copy)]
struct Aux {
    /// `Upgrade`/`BachTheater`'s `from` -- the trade-in half of a two-card
    /// move, which [`Move::card`] does not expose (it returns `to`).
    secondary: CardId,
    target: Option<PlayerIdx>,
    side: Option<PactSide>,
    churchill: Option<ChurchillChoice>,
    /// `Take`'s row slot.
    slot: Option<u8>,
    /// `WonderStep`'s stage count.
    steps: Option<u8>,
    /// `Bid`'s committed strength.
    bid: Option<u8>,
    /// `Choose`'s positional option index.
    choose: Option<u8>,
}

impl Aux {
    const EMPTY: Aux = Aux {
        secondary: CardId::NONE,
        target: None,
        side: None,
        churchill: None,
        slot: None,
        steps: None,
        bid: None,
        choose: None,
    };
}

#[inline]
fn decode_aux(mv: Move) -> Aux {
    match mv {
        Move::Take { slot } => Aux { slot: Some(slot), ..Aux::EMPTY },
        Move::Upgrade { from, .. } => Aux { secondary: from, ..Aux::EMPTY },
        Move::WonderStep { steps } => Aux { steps: Some(steps), ..Aux::EMPTY },
        Move::Aggression { target, .. } => Aux { target: Some(target), ..Aux::EMPTY },
        Move::War { target, .. } => Aux { target: Some(target), ..Aux::EMPTY },
        Move::OfferPact { target, side, .. } => {
            Aux { target: Some(target), side: Some(side), ..Aux::EMPTY }
        }
        Move::CancelPact { owner } => Aux { target: Some(owner), ..Aux::EMPTY },
        Move::BachTheater { from, .. } => Aux { secondary: from, ..Aux::EMPTY },
        Move::Bid { n } => Aux { bid: Some(n), ..Aux::EMPTY },
        Move::Choose { n } => Aux { choose: Some(n), ..Aux::EMPTY },
        Move::Churchill { choice } => Aux { churchill: Some(choice), ..Aux::EMPTY },
        Move::Build { .. }
        | Move::Develop { .. }
        | Move::Pop
        | Move::PopFree
        | Move::Revolution { .. }
        | Move::PlayLeader { .. }
        | Move::PlayAction { .. }
        | Move::Destroy { .. }
        | Move::PlayTactic { .. }
        | Move::CopyTactic { .. }
        | Move::PrepareEvent { .. }
        | Move::RemoveLeaderYellow
        | Move::ColumbusColonize { .. }
        | Move::Barbarossa { .. }
        | Move::TradeFoodAsResource
        | Move::TradeResourceAsFood
        | Move::BidPass
        | Move::Defend { .. }
        | Move::DefendDone
        | Move::SendUnit { .. }
        | Move::SendBonus { .. }
        | Move::SendDiscard { .. }
        | Move::SendDone
        | Move::EndTurn
        | Move::PolPass
        | Move::Resign => Aux::EMPTY,
    }
}

#[inline]
fn pact_side_slot(side: PactSide) -> usize {
    match side {
        PactSide::Unspecified => 0,
        PactSide::A => 1,
        PactSide::B => 2,
    }
}

#[inline]
fn churchill_slot(choice: ChurchillChoice) -> usize {
    match choice {
        ChurchillChoice::Culture => 0,
        ChurchillChoice::Military => 1,
    }
}

/// Largest printed wonder `stages` array on the base-game card table (5,
/// checked against `card_table.rs` 2026-08-12: one 5-stage wonder, five
/// 4-stage, eight 3-stage, everything else 0 or 2) -- the ceiling
/// [`Move::WonderStep`]'s `steps` count can ever legally reach in one move.
const MAX_WONDER_STEPS: f64 = 5.0;

/// [`Move::Bid`]'s strength scale, matching `encode::player_block`'s own
/// `s.strength / 30.0` -- a colonization bid is denominated in the same
/// military-strength units, so reusing the scale keeps this feature on the
/// same footing as the state encoding's strength columns rather than
/// inventing an unrelated unit.
const BID_SCALE: f64 = 30.0;

/// Type one-hot + primary/secondary card identity + relative-target one-hot
/// + pact-side one-hot + Churchill one-hot + row-slot one-hot + three raw
/// scalars (`steps`, `bid`, `choose`, each normalised).
pub const ACTION_DIM: usize =
    NUM_MOVE_KINDS + 2 * CARD_VEC_DIM + MAX_PLAYERS + 3 + 2 + ROW_SIZE + 3;

/// A fixed-length, dense feature vector for `mv`, a legal action available
/// to `actor` (`state.decider()` at the decision point it came from). See
/// this module's top doc comment for why there is no `GameState` parameter.
///
/// Deterministic and pure: encoding the same `(actor, mv)` pair twice, in
/// two different games, produces bit-identical output, because every input
/// is plain-old-data and every step below is ordinary arithmetic on it (no
/// RNG, no clock, no hash-map iteration order).
pub fn encode_action(actor: PlayerIdx, mv: Move) -> Vec<f64> {
    let mut out = Vec::with_capacity(ACTION_DIM);

    let mut kind = [0.0f64; NUM_MOVE_KINDS];
    kind[move_kind_slot(&mv)] = 1.0;
    out.extend_from_slice(&kind);

    // Primary card: `Move::card()` already resolves `Upgrade`/`BachTheater`
    // to `to` (the card actually being built), not `from` -- see its own
    // doc comment.
    out.extend_from_slice(&card_vec_array(mv.card().unwrap_or(CardId::NONE)));

    let aux = decode_aux(mv);
    out.extend_from_slice(&card_vec_array(aux.secondary));

    // Target/owner, relative to `actor` rather than an absolute seat index:
    // "attack the next player" should encode the same regardless of which
    // absolute seats are playing, matching `encode::encode`'s own
    // self-then-rivals-in-relative-order ordering for the state side of the
    // vector. `MAX_PLAYERS` slots (not `MAX_PLAYERS - 1`) because a target
    // CAN equal the actor (`CancelPact`'s `owner` may be a pact the actor
    // itself holds), unlike `encode::encode`'s rival list which excludes
    // self by construction.
    let mut target = [0.0f64; MAX_PLAYERS];
    if let Some(t) = aux.target {
        let offset = (t as usize + MAX_PLAYERS - actor as usize) % MAX_PLAYERS;
        target[offset] = 1.0;
    }
    out.extend_from_slice(&target);

    let mut side = [0.0f64; 3];
    if let Some(s) = aux.side {
        side[pact_side_slot(s)] = 1.0;
    }
    out.extend_from_slice(&side);

    let mut churchill = [0.0f64; 2];
    if let Some(c) = aux.churchill {
        churchill[churchill_slot(c)] = 1.0;
    }
    out.extend_from_slice(&churchill);

    let mut slot = [0.0f64; ROW_SIZE];
    if let Some(s) = aux.slot {
        slot[s as usize] = 1.0;
    }
    out.extend_from_slice(&slot);

    out.push(aux.steps.map_or(0.0, |s| f64::from(s) / MAX_WONDER_STEPS));
    out.push(aux.bid.map_or(0.0, |n| f64::from(n) / BID_SCALE));
    out.push(aux.choose.map_or(0.0, |n| f64::from(n) / MAX_OPTIONS as f64));

    debug_assert_eq!(out.len(), ACTION_DIM, "encode_action() produced the wrong length");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::moves::PactSide;

    /// Every [`Move`] variant maps to a distinct slot in `0..NUM_MOVE_KINDS`
    /// -- the same injectivity guard `encode.rs` runs for `card_type_slot`,
    /// checked here so a copy-paste slot number collision is caught by a
    /// unit test rather than by two actions quietly sharing a one-hot.
    #[test]
    fn move_kind_slot_is_injective_and_in_range() {
        let samples = [
            Move::Take { slot: 0 },
            Move::Build { card: CardId::NONE },
            Move::Develop { card: CardId::NONE },
            Move::Upgrade { from: CardId::NONE, to: CardId::NONE },
            Move::WonderStep { steps: 1 },
            Move::Pop,
            Move::PopFree,
            Move::Revolution { card: CardId::NONE },
            Move::PlayLeader { card: CardId::NONE },
            Move::PlayAction { card: CardId::NONE },
            Move::Destroy { card: CardId::NONE },
            Move::PlayTactic { card: CardId::NONE },
            Move::CopyTactic { card: CardId::NONE },
            Move::Aggression { card: CardId::NONE, target: 0 },
            Move::War { card: CardId::NONE, target: 0 },
            Move::OfferPact { card: CardId::NONE, target: 0, side: PactSide::Unspecified },
            Move::CancelPact { owner: 0 },
            Move::PrepareEvent { card: CardId::NONE },
            Move::RemoveLeaderYellow,
            Move::ColumbusColonize { card: CardId::NONE },
            Move::Barbarossa { card: CardId::NONE },
            Move::BachTheater { from: CardId::NONE, to: CardId::NONE },
            Move::TradeFoodAsResource,
            Move::TradeResourceAsFood,
            Move::Bid { n: 0 },
            Move::BidPass,
            Move::Defend { card: CardId::NONE },
            Move::DefendDone,
            Move::SendUnit { card: CardId::NONE },
            Move::SendBonus { card: CardId::NONE },
            Move::SendDiscard { card: CardId::NONE },
            Move::SendDone,
            Move::Choose { n: 0 },
            Move::Churchill { choice: ChurchillChoice::Culture },
            Move::EndTurn,
            Move::PolPass,
            Move::Resign,
        ];
        assert_eq!(samples.len(), NUM_MOVE_KINDS, "one sample per variant, or this test itself is stale");
        let mut seen = [false; NUM_MOVE_KINDS];
        for mv in samples {
            let slot = move_kind_slot(&mv);
            assert!(slot < NUM_MOVE_KINDS, "{mv:?}: slot {slot} out of range");
            assert!(!seen[slot], "{mv:?}: slot {slot} collides with an earlier variant");
            seen[slot] = true;
        }
        assert!(seen.iter().all(|&b| b), "not every slot in 0..NUM_MOVE_KINDS was used");
    }

    /// Encoding the same `(actor, move)` pair twice -- standing in for "the
    /// same action in two different games" -- produces bit-identical
    /// vectors, since [`encode_action`] is pure arithmetic over plain data.
    #[test]
    fn encoding_is_deterministic_for_the_same_action() {
        let warriors = CardId::by_name("Warriors").unwrap();
        let mv = Move::Build { card: warriors };
        let a = encode_action(1, mv);
        let b = encode_action(1, mv);
        assert_eq!(a, b);
    }

    /// A sample of genuinely different actions -- different row slots,
    /// different cards, different targets, a card move vs. a control move --
    /// never collide onto the same encoded vector. This is the property a
    /// bug in the offset arithmetic (e.g. two fields sharing a slot range)
    /// would actually break, unlike identity collision (see this module's
    /// top doc comment: `Move` equality cannot collide by construction).
    #[test]
    fn genuinely_different_actions_encode_to_different_vectors() {
        let warriors = CardId::by_name("Warriors").unwrap();
        let irrigation = CardId::by_name("Irrigation").unwrap();
        let variants = [
            encode_action(0, Move::Take { slot: 0 }),
            encode_action(0, Move::Take { slot: 1 }),
            encode_action(0, Move::Build { card: warriors }),
            encode_action(0, Move::Build { card: irrigation }),
            encode_action(0, Move::Develop { card: warriors }),
            encode_action(0, Move::Aggression { card: warriors, target: 1 }),
            encode_action(0, Move::Aggression { card: warriors, target: 2 }),
            encode_action(0, Move::EndTurn),
            encode_action(0, Move::PolPass),
            encode_action(0, Move::Bid { n: 3 }),
            encode_action(0, Move::Bid { n: 6 }),
            encode_action(0, Move::Choose { n: 0 }),
            encode_action(0, Move::Choose { n: 1 }),
        ];
        for i in 0..variants.len() {
            for j in (i + 1)..variants.len() {
                assert_ne!(variants[i], variants[j], "sample {i} and {j} collided");
            }
        }
    }

    /// [`ActionId`] (== [`Move`]) equality is exact structural equality, so
    /// two `Move` values built from the same fields are the same identity
    /// regardless of which call site constructed them -- the property a
    /// training pipeline needs to match a chosen action back to one of the
    /// encoded candidates it dumped.
    #[test]
    fn identical_actions_share_one_identity() {
        let warriors = CardId::by_name("Warriors").unwrap();
        let a: ActionId = Move::Build { card: warriors };
        let b: ActionId = Move::Build { card: warriors };
        assert_eq!(a, b);
    }

    /// The relative-target encoding is seat-invariant: "aggress the player
    /// one seat ahead of me" encodes identically whether I am seat 0
    /// attacking seat 1, or seat 2 attacking seat 3 -- the same argument
    /// `encode::encode`'s own rival ordering makes for state features.
    #[test]
    fn target_encoding_is_relative_to_the_actor_not_absolute() {
        let card = CardId::by_name("Warriors").unwrap();
        let a = encode_action(0, Move::Aggression { card, target: 1 });
        let b = encode_action(2, Move::Aggression { card, target: 3 });
        assert_eq!(a, b, "both are \"attack the next seat\", so they must encode identically");
    }

    /// A target CAN equal the actor (`CancelPact { owner }` may name a pact
    /// the actor itself holds) -- offset 0 must set its own one-hot slot,
    /// not be folded into the "no target at all" all-zero block every
    /// non-targeted move (e.g. `EndTurn`) already uses. Checks the target
    /// sub-slice directly rather than comparing whole vectors against a
    /// different-kind move, which would pass on the KIND one-hot alone even
    /// if the target block itself were wrongly zeroed.
    #[test]
    fn a_target_equal_to_the_actor_sets_the_self_offset_slot() {
        let v = encode_action(1, Move::CancelPact { owner: 1 });
        let target_start = NUM_MOVE_KINDS + 2 * CARD_VEC_DIM;
        let target_block = &v[target_start..target_start + MAX_PLAYERS];
        assert_eq!(
            target_block,
            &[1.0, 0.0, 0.0, 0.0],
            "offset 0 (self) must be set, not treated as \"no target\""
        );
    }
}
