//! `engine/bots/weighted.py` lines 592-1055: what a rival's board is worth,
//! and the credit machinery `card_potential`/`tech_value`/etc. (unowned by
//! this module -- `cards.rs`) share for "what would a feature be worth if I
//! had this instead".
//!
//! Ported: `rival_board`, `_add_block`, `deferred_credit`, `_RivalView`,
//! `root_row_budget`, `rival_context`, `rival_strength`, `strength_marginal`,
//! `feature_marginal`, plus the module-scope tables `_YIELD_TO_FEATURE`/
//! `_NO_GAINS`/`_PHASE_SET` -- the last of which needs no Rust counterpart at
//! all (see [`feature_marginal`]'s own doc comment).
//!
//! ## Two very different call frequencies live in this one file
//!
//! [`rival_board`] and [`deferred_credit`] are read out of `features()`
//! (unowned, `eval.rs`/`cards.rs`'s eventual territory), which runs once PER
//! CANDIDATE move of a 1-ply search -- maybe 30 times a decision. Both are
//! therefore written with zero heap allocation: [`RivalBoard`] is a flat
//! struct of `f64` fields, [`Gains`] is a fixed `[f64; 16]` array indexed by
//! [`GainFeature`], and [`deferred_credit`]'s pact-block walk pushes into
//! that array through a callback rather than building a `Vec` of blocks
//! first.
//!
//! [`rival_context`] is the opposite: its own doc comment is explicit that it
//! runs ONCE per decision, at the root, and is threaded down unchanged --
//! `bots::counting`'s `civil_outlook`/`event_pool` already return `Vec`s for
//! exactly this "root-only" reason, so [`RivalContext`] follows suit rather
//! than inventing a fixed-capacity substitute nothing downstream needs.
//!
//! ## `_YIELD_TO_FEATURE`, restated
//!
//! Python's `_add_block` walks a raw, string-keyed effect dict and maps each
//! key through one shared table (`_YIELD_TO_FEATURE`) regardless of which
//! kind of block it came from. In Rust every one of those sources is already
//! a TYPED struct generated straight off the same card data
//! (`cards::PactBlock` for a pact's `A`/`B`/`bothPlayers` block,
//! `cards::CardEffects` for a territory's `permanentEffects`,
//! `cards::ImmediateEffects` for its `immediateEffects`) -- so the
//! "raw key" side of `_YIELD_TO_FEATURE` is simply which FIELD each
//! source-specific `add_*_effects`/`add_pact_block` function below reads, and
//! the "feature" side survives as [`GainFeature`]. Three small functions
//! instead of one generic dict walk, exactly the trade `effects.rs` already
//! made for the same underlying data (see that module's own "the effect
//! system, and why it is two things").
//!
//! Confirmed against the live `data/*.json` (2026-08-05) that the three
//! sources never share a raw key that this table would route differently
//! depending on context: a territory's `permanentEffects` only ever prints
//! `blueTokens`/`happiness`/`strength`/`yellowTokens`, its `immediateEffects`
//! only `culture`/`drawMilitaryCards`/`food`/`population`/`resources`/
//! `science`, and a pact's `A`/`B`/`bothPlayers` block only the `*Production`
//! spellings (never the bare `culture`/`food`/`resources` that would mean a
//! ONE-SHOT gain in a territory's `immediateEffects`) -- exactly the split
//! Python's own comment on `_YIELD_TO_FEATURE` documents ("pact effect blocks
//! / colony permanentEffects" vs. "territory immediateEffects").

use crate::bots::counting;
use crate::cards::{CardEffects, CardId, ImmediateEffects, PactBlock, Special};
use crate::costs::{self, TakeGate};
use crate::economy;
use crate::effects;
use crate::state::{CardList, GameState, Pending, PlayerState, Tableau, MAX_HAND, MAX_PLAYERS, ROW_SIZE};

use super::horizon;
use super::weights::{WeightKey, Weights, PHASE_KEYS};

// =========================================================== rival_board

/// The public rival board facts nothing scored, as a flat `f64` struct --
/// Python's `rival_board`, one field per dict key.
///
/// Every one of these is on the table in the physical 2015 base game -- the
/// stocks sit in front of their owner, the workers stand on the cards, the
/// colony and wonder cards are face up -- so all of it is legal under the
/// standing rule recorded in docs/EVALUATOR_HISTORY.md ("Military discard
/// pile legibility": "Card counting is legal. All public info can be used.").
///
/// `max` over rivals throughout, matching `rival_free_ca` and friends, so
/// each term means the same thing at 2p, 3p and 4p. The exception is
/// `rival_building_wonder`, which is a COUNT: "somebody is mid-wonder" and
/// "everybody is mid-wonder" are different rows to play into, and a max over
/// a boolean would flatten them to the same 1.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RivalBoard {
    pub rival_science_stock: f64,
    pub rival_food_stock: f64,
    pub rival_resource_stock: f64,
    pub rival_free_workers: f64,
    pub rival_yellow_bank: f64,
    pub rival_colonies: f64,
    pub rival_mil_actions: f64,
    pub rival_building_wonder: f64,
}

/// `rival_board(state, idx)`. Zero heap allocation: this runs once per
/// candidate move (see this module's top doc comment), so the max-fold below
/// is a manual loop over an `Option<RivalBoard>` seed rather than collecting
/// rivals into a `Vec` first.
pub fn rival_board(state: &GameState, idx: u8) -> RivalBoard {
    let mut out: Option<RivalBoard> = None;
    for q in state.players[..state.num_players as usize].iter() {
        if q.idx == idx || q.resigned {
            continue;
        }
        let wonder = if q.wonder.is_none() { 0.0 } else { 1.0 };
        match &mut out {
            None => {
                out = Some(RivalBoard {
                    rival_science_stock: q.science as f64,
                    rival_food_stock: q.food as f64,
                    rival_resource_stock: q.resources as f64,
                    rival_free_workers: q.workers_free as f64,
                    rival_yellow_bank: q.yellow_bank as f64,
                    rival_colonies: q.colonies.len() as f64,
                    rival_mil_actions: q.military_actions as f64,
                    rival_building_wonder: wonder,
                });
            }
            Some(b) => {
                b.rival_science_stock = b.rival_science_stock.max(q.science as f64);
                b.rival_food_stock = b.rival_food_stock.max(q.food as f64);
                b.rival_resource_stock = b.rival_resource_stock.max(q.resources as f64);
                b.rival_free_workers = b.rival_free_workers.max(q.workers_free as f64);
                b.rival_yellow_bank = b.rival_yellow_bank.max(q.yellow_bank as f64);
                b.rival_colonies = b.rival_colonies.max(q.colonies.len() as f64);
                b.rival_mil_actions = b.rival_mil_actions.max(q.military_actions as f64);
                b.rival_building_wonder += wonder;
            }
        }
    }
    out.unwrap_or_default()
}

// ======================================================= deferred gains

/// The sixteen evaluator-feature targets a deferred pact/auction gain can be
/// priced through -- Python's `_YIELD_TO_FEATURE` table, restated as an enum
/// (see this module's top doc comment for why the raw-JSON-key side of that
/// table needs no Rust representation at all).
///
/// `Happy` is deliberately NOT a [`WeightKey`]: `features()` (unowned by this
/// module) reads it straight into `s.happy`'s own margin term (`margin =
/// s.happy - happy_req + g("happy", 0.0)`), never through a weight lookup --
/// see Python's own comment on `_TERR_TO_FEATURE` (the sibling table
/// `card_potential` uses) for the one place conflating this with the REAL
/// weight `happy_margin` would be a bug.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(usize)]
pub enum GainFeature {
    CultureRate,
    ScienceRate,
    FoodRate,
    ResourceRate,
    Strength,
    Happy,
    CivilActions,
    MilitaryActions,
    YellowBank,
    BlueFree,
    Culture,
    Science,
    FoodStock,
    ResourceStock,
    FreeWorkers,
    HandMilitary,
}

impl GainFeature {
    /// Every variant, in declaration order -- for the (since-retired)
    /// differential test's own both-directions iteration
    /// (`rust/tests/weighted_rivals.rs`), the same role `WeightKey::ALL`
    /// plays for `weights.rs`.
    pub const ALL: &'static [GainFeature] = &[
        GainFeature::CultureRate,
        GainFeature::ScienceRate,
        GainFeature::FoodRate,
        GainFeature::ResourceRate,
        GainFeature::Strength,
        GainFeature::Happy,
        GainFeature::CivilActions,
        GainFeature::MilitaryActions,
        GainFeature::YellowBank,
        GainFeature::BlueFree,
        GainFeature::Culture,
        GainFeature::Science,
        GainFeature::FoodStock,
        GainFeature::ResourceStock,
        GainFeature::FreeWorkers,
        GainFeature::HandMilitary,
    ];

    /// The exact string Python's `_YIELD_TO_FEATURE` target / `gains` dict
    /// key uses for this feature. I/O boundary only (the differential test)
    /// -- nothing inside this crate needs it, mirroring `WeightKey::name`'s
    /// own doc comment.
    pub const fn name(self) -> &'static str {
        match self {
            GainFeature::CultureRate => "culture_rate",
            GainFeature::ScienceRate => "science_rate",
            GainFeature::FoodRate => "food_rate",
            GainFeature::ResourceRate => "resource_rate",
            GainFeature::Strength => "strength",
            GainFeature::Happy => "happy",
            GainFeature::CivilActions => "civil_actions",
            GainFeature::MilitaryActions => "military_actions",
            GainFeature::YellowBank => "yellow_bank",
            GainFeature::BlueFree => "blue_free",
            GainFeature::Culture => "culture",
            GainFeature::Science => "science",
            GainFeature::FoodStock => "food_stock",
            GainFeature::ResourceStock => "resource_stock",
            GainFeature::FreeWorkers => "free_workers",
            GainFeature::HandMilitary => "hand_military",
        }
    }
}

const N_GAIN_FEATURES: usize = 16;

/// A sparse accumulation of deferred feature gains -- Python's per-decision
/// `gains`/`partner_gains` dict (`deferred_credit`'s return value). `[f64;
/// 16]` indexed by `GainFeature as usize`, exactly `weights::Weights`'
/// own shape and for the same reason: this is rebuilt once per CANDIDATE
/// move (unlike `RivalContext`), so it must not allocate.
///
/// Python's shared empty-dict sentinel `_NO_GAINS` has no equivalent need
/// here: `_NO_GAINS` exists to let `gains = partner_gains = _NO_GAINS` share
/// ONE object instead of allocating two empty dicts on every call where
/// `state.pending` is empty. `Gains::default()` is a zeroed value type, not a
/// shared mutable reference -- there is nothing to alias and nothing to
/// allocate, so every caller can just build its own default.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gains([f64; N_GAIN_FEATURES]);

impl Gains {
    #[inline]
    pub fn get(&self, key: GainFeature) -> f64 {
        self.0[key as usize]
    }

    #[inline]
    fn add(&mut self, key: GainFeature, amount: f64) {
        self.0[key as usize] += amount;
    }
}

impl Default for Gains {
    fn default() -> Self {
        Gains([0.0; N_GAIN_FEATURES])
    }
}

/// Add one pact block's grants to `gains`, scaled by `scale` -- Python's
/// `_add_block` specialised to `cards::PactBlock`'s closed vocabulary.
/// `other` is the player whose completed-wonder count the
/// `cultureProductionPerCompletedWonderOfTheOtherParty` special case reads
/// (Python's `other` parameter).
///
/// `PactBlock`'s `culture`/`food`/`resources` fields are the pact data's
/// `cultureProduction`/`foodProduction`/`resourceProduction` keys, aliased
/// onto these field names at generation time (`gen_cards.py`, mirroring
/// `engine/effects.py::FLAT_KEYS`'s own aliasing) -- so they map to the RATE
/// features here, not the stock ones, exactly as `_YIELD_TO_FEATURE`'s
/// `"cultureProduction": "culture_rate"` entry says. `tech_discount`/
/// `war_immune`/`food_as_resource`/`resource_as_food`/
/// `other_party_pays_science`/`attacker_strength` have no entry in
/// `_YIELD_TO_FEATURE` and are correctly left unread here, same as Python's
/// generic walk silently drops any key `_YIELD_TO_FEATURE.get` misses.
fn add_pact_block(gains: &mut Gains, block: &PactBlock, scale: f64, other: &PlayerState) {
    if block.culture != 0 {
        gains.add(GainFeature::CultureRate, scale * block.culture as f64);
    }
    if block.food != 0 {
        gains.add(GainFeature::FoodRate, scale * block.food as f64);
    }
    if block.resources != 0 {
        gains.add(GainFeature::ResourceRate, scale * block.resources as f64);
    }
    if block.strength != 0 {
        gains.add(GainFeature::Strength, scale * block.strength as f64);
    }
    if block.military_actions != 0 {
        gains.add(GainFeature::MilitaryActions, scale * block.military_actions as f64);
    }
    if block.culture_per_wonder_of_other_party != 0 {
        let wonders = other.completed_wonders.len() as f64;
        gains.add(GainFeature::CultureRate, scale * block.culture_per_wonder_of_other_party as f64 * wonders);
    }
}

/// A territory's `permanentEffects`, applied to `gains` -- Python's
/// `_add_block` specialised to `cards::CardEffects` (the "pact effect blocks
/// / colony permanentEffects" half of `_YIELD_TO_FEATURE`). Every field read
/// here is already the ALIASED form of the raw JSON key (`gen_cards.py`
/// folds a territory's `permanentEffects` onto the same `CardEffects` a
/// regular card's `effects` dict uses -- see `effects.rs`'s own "KNOWN GAPS,
/// closed 2026-08-05" for the citation), so this reads `eff.culture` for what
/// Python's raw dict spelled `cultureProduction`, not `culture`.
fn add_permanent_effects(gains: &mut Gains, eff: &CardEffects, scale: f64) {
    if eff.culture != 0 {
        gains.add(GainFeature::CultureRate, scale * eff.culture as f64);
    }
    if eff.science != 0 {
        gains.add(GainFeature::ScienceRate, scale * eff.science as f64);
    }
    if eff.food != 0 {
        gains.add(GainFeature::FoodRate, scale * eff.food as f64);
    }
    if eff.resources != 0 {
        gains.add(GainFeature::ResourceRate, scale * eff.resources as f64);
    }
    if eff.strength != 0 {
        gains.add(GainFeature::Strength, scale * eff.strength as f64);
    }
    if eff.happy != 0 {
        gains.add(GainFeature::Happy, scale * eff.happy as f64);
    }
    if eff.civil_actions != 0 {
        gains.add(GainFeature::CivilActions, scale * eff.civil_actions as f64);
    }
    if eff.military_actions != 0 {
        gains.add(GainFeature::MilitaryActions, scale * eff.military_actions as f64);
    }
    if eff.yellow_tokens != 0 {
        gains.add(GainFeature::YellowBank, scale * eff.yellow_tokens as f64);
    }
    if eff.blue_tokens != 0 {
        gains.add(GainFeature::BlueFree, scale * eff.blue_tokens as f64);
    }
}

/// A territory's `immediateEffects`, applied to `gains` -- Python's
/// `_add_block` specialised to `cards::ImmediateEffects` (the "territory
/// immediateEffects" half of `_YIELD_TO_FEATURE`): the one-shot welcome gift
/// paid the moment a colonization auction is won, priced through the STOCK
/// features (`culture`/`science`/`food_stock`/`resource_stock`), not the rate
/// ones -- the whole reason `_YIELD_TO_FEATURE` keys the same word
/// (`culture`) two different ways depending on which block it came from.
fn add_immediate_effects(gains: &mut Gains, eff: &ImmediateEffects, scale: f64) {
    if eff.culture != 0 {
        gains.add(GainFeature::Culture, scale * eff.culture as f64);
    }
    if eff.science != 0 {
        gains.add(GainFeature::Science, scale * eff.science as f64);
    }
    if eff.food != 0 {
        gains.add(GainFeature::FoodStock, scale * eff.food as f64);
    }
    if eff.resources != 0 {
        gains.add(GainFeature::ResourceStock, scale * eff.resources as f64);
    }
    if eff.population != 0 {
        gains.add(GainFeature::FreeWorkers, scale * eff.population as f64);
    }
    if eff.draw_military_cards != 0 {
        gains.add(GainFeature::HandMilitary, scale * eff.draw_military_cards as f64);
    }
}

/// Every pact block `idx` would read off `card`'s printed `A`/`B`/
/// `bothPlayers` effects -- Python's `effects._pact_blocks(pact, idx)`,
/// restated as a callback instead of building a fresh `Vec` per call:
/// `deferred_credit` runs once per candidate move, so a 0-2-item allocation
/// here would be exactly the "small helper, hot path" shape this port's
/// house style forbids.
fn pact_blocks_for(card: CardId, a: u8, b: u8, idx: u8, mut f: impl FnMut(&PactBlock)) {
    for &sp in card.get().special {
        match sp {
            Special::BothPlayers(block) => f(&block),
            Special::A(block) if idx == a => f(&block),
            Special::B(block) if idx == b => f(&block),
            _ => {}
        }
    }
}

/// FITTED PRIOR (see docs/EVALUATOR_HISTORY.md): a pact you have OFFERED is not
/// a pact -- the partner may refuse. Credit it at this fraction of a live
/// pact (docs/AUDIT_HISTORY.md), without which a 1-ply search sees only
/// the card leaving your hand and `offer_pact` is strictly dominated by
/// `pol_pass` in every position.
pub const PACT_OFFER_CREDIT: f64 = 0.5;

/// Payoffs the 1-ply trial state cannot show yet, priced by their yield --
/// Python's `deferred_credit`. See that function's own docstring
/// (`engine/bots/weighted.py:679`) for the two moves it covers
/// (`offer_pact`, a live `bid`) and its documented third gap
/// (`interact.colonize`'s pending sacrifice), which Python leaves unhandled
/// on purpose. This port closes that third gap instead (the `Pending::
/// Colonize` arm below): a WON auction still needs its colony priced for
/// the one ply between `bid_pass` settling the winner and `colonize_auto`
/// resolving the sacrifice, and nothing else prices it in that window.
///
/// `state.pending` is read through [`crate::state::PendingStack::top`]
/// rather than a general loop: that type's own doc comment establishes the
/// invariant this relies on ("the depth is 0 or 1, never 2" -- structural,
/// not just measured, because a nested `push_choice` only ever runs after
/// `apply_pending` has already popped the one that triggered it), so "the
/// one open decision, if any" is exactly what Python's `for pend in
/// state.pending` sees in every reachable state.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DeferredCredit {
    pub pact_offers: f64,
    pub auction_committed: f64,
    pub auction_bid: f64,
    pub blocks_attack: f64,
    /// Deferred amounts for `idx` itself, keyed by [`GainFeature`].
    pub gains: Gains,
    /// The same, for the side of a pact a RIVAL would take -- indexed by
    /// player index (like `wars_declared_on_me` elsewhere in this crate),
    /// not a map: at most one entry is ever nonzero (`state.pending` holds at
    /// most one decision), and an unindexed rival simply reads
    /// `Gains::default()`, which is inert wherever it is added in.
    pub partner_gains: [Gains; MAX_PLAYERS],
}

pub fn deferred_credit(state: &GameState, idx: u8) -> DeferredCredit {
    let mut out = DeferredCredit::default();
    let Some(pend) = state.pending.top() else {
        return out;
    };
    match pend {
        Pending::Choice(c) => {
            use crate::state::ChoiceKind;
            let ChoiceKind::PactOffer { owner, card, a, b } = c.kind else {
                return out;
            };
            if owner != idx {
                return out;
            }
            out.pact_offers += PACT_OFFER_CREDIT;
            if card.get().special.contains(&Special::NoAttacksBetweenParties) {
                out.blocks_attack += PACT_OFFER_CREDIT;
            }
            // `_c_pact_offer`: the PARTNER (not the owner) is who must
            // accept or refuse, so `Choice.player` -- who this decision is
            // waiting on -- IS Python's `pend.get("player")`.
            let partner = c.player;
            pact_blocks_for(card, a, b, idx, |block| {
                add_pact_block(&mut out.gains, block, PACT_OFFER_CREDIT, &state.players[partner as usize]);
            });
            // THE OTHER HALF OF THE DEAL: the partner's own side of the SAME
            // pact, differenced rather than own-only for the reason
            // `event_scoring_margin` is -- see Python's docstring on this
            // branch. Fed into the `rival_*` maxima the evaluator already
            // prices, not a new coordinate.
            if partner != idx {
                pact_blocks_for(card, a, b, partner, |block| {
                    add_pact_block(
                        &mut out.partner_gains[partner as usize],
                        block,
                        PACT_OFFER_CREDIT,
                        &state.players[idx as usize],
                    );
                });
            }
        }
        Pending::Auction(auc) if auc.high == Some(idx) => {
            let rivals_still_bidding = auc.active().iter().filter(|&&p| p != idx).count();
            let share = 1.0 / (1.0 + rivals_still_bidding as f64);
            out.auction_committed += share;
            // The winner sacrifices units worth at least the bid (§11.3);
            // none of that is in the trial state either.
            out.auction_bid += share * auc.bid as f64;
            if !auc.card.is_none() {
                let card = auc.card.get();
                add_permanent_effects(&mut out.gains, &card.effects, share);
                add_immediate_effects(&mut out.gains, &card.immediate_effects, share);
            }
        }
        // `bid_pass` on the sole remaining bidder pops `Auction` and pushes
        // `Colonize` in the same move (`interact::end_auction_round`), so a
        // winning bid's credit vanishes for exactly one ply unless this arm
        // exists: the `Auction` guard above never sees it again, because the
        // pending decision it matched on is already gone. Share is 1.0, not
        // the `Auction` branch's bid-still-live fraction, because a
        // `Colonize` decision only ever exists AFTER the winner is settled
        // (`interact::colonize` is called with a single `player`, never a
        // set of contenders) -- there is no more "chance of winning" left to
        // discount by.
        Pending::Colonize(c) if c.player == idx => {
            out.auction_committed += 1.0;
            out.auction_bid += c.bid as f64;
            if !c.card.is_none() {
                let card = c.card.get();
                add_permanent_effects(&mut out.gains, &card.effects, 1.0);
                add_immediate_effects(&mut out.gains, &card.immediate_effects, 1.0);
            }
        }
        _ => {}
    }
    out
}

// ============================================================ rival view

/// An immutable snapshot of the parts of a rival's board
/// [`costs::can_take_gated`] reads off a real `PlayerState` -- Python's
/// `_RivalView`, a duck-typed shim that stands in for
/// `actions._can_take_gated`'s `p` parameter so the SAME row-take legality
/// rule can run against a rival whose real `PlayerState` will be mutated and
/// rolled back many times over the course of a search.
///
/// `rival_context` builds one PER LIVE RIVAL, ONCE, at the root of a
/// decision, and hands it unchanged to every candidate -- including on the
/// journalled search path, where the "root state" IS the very `GameState`
/// candidates mutate and undo. A `&PlayerState` borrow would alias straight
/// into that rollback; every field here is therefore an owned copy instead
/// (`Tableau`/`CardList` are `Clone`, not `Copy` -- cloning is one memcpy
/// each, paid once per rival per DECISION, not per candidate, unlike
/// [`RivalBoard`]/[`Gains`] above).
///
/// `taken_leader_ages` is deliberately NOT a field here, unlike Python's
/// `_RivalView`: Rust's [`TakeGate`] (unlike Python's bare 4-element `gate`
/// tuple) already carries it, so `rival_context` sets it on the paired
/// `TakeGate` instead of duplicating it on this struct too.
#[derive(Clone, Debug)]
pub struct RivalView {
    pub idx: u8,
    /// `can_take_gated`'s wonder branch only ever asks `p.wonder.is_none()`.
    pub building_wonder: bool,
    pub hand_civil: CardList<MAX_HAND>,
    pub techs: Tableau,
    pub government: CardId,
    /// How many more civil cards this rival's hand can hold (RULES_SPEC
    /// 2.5). `hand_size`, not `hand_civil.len()`, so the app harness's
    /// `hidden_civil` count is included; read by `row_pressure` (unowned).
    pub hand_slack: i32,
}

impl RivalView {
    /// Snapshot `q`'s legality-relevant fields. `pub`: `row.rs` (unowned)
    /// builds a `RivalView` directly off a real `&PlayerState` too, to prove
    /// its own `rival_view_can_take` agrees with `costs::can_take_gated` on
    /// the real thing before trusting it on a rolled-back snapshot.
    pub fn new(q: &PlayerState, hand_slack: i32) -> RivalView {
        RivalView {
            idx: q.idx,
            building_wonder: !q.wonder.is_none(),
            hand_civil: q.hand_civil.clone(),
            techs: q.techs.clone(),
            government: q.government,
            hand_slack,
        }
    }
}

// ======================================================== root row budget

/// The card-row names VISIBLE AT THE ROOT of a search, IN ROW ORDER --
/// Python's `root_row_budget`. See that function's own extensive docstring
/// (`engine/bots/weighted.py:800`) for why this must be an order-preserving
/// SEQUENCE and not a set or a bare multiset, and for the one documented
/// residual leak (11 of 1583 `end_turn` candidates at 3p) that order alone
/// does not close.
///
/// `state.card_row` entries are already `CardId`s, not printed names, so this
/// is a straight filter of `CardId::NONE` rather than Python's `getattr(c,
/// "name", c)` fallback chain.
pub fn root_row_budget(state: &GameState) -> CardList<ROW_SIZE> {
    let mut out = CardList::new();
    for &c in state.card_row.iter() {
        if !c.is_none() {
            out.push(c);
        }
    }
    out
}

// =========================================================== rival context

/// Rival aggregates that only change when THEY move -- Python's
/// `rival_context`, a dict there and a struct here for the same reason
/// `weights::Weights` is an array rather than a `dict[str, f64]`: every field
/// is read by name at a fixed set of call sites, never iterated by key.
///
/// Computed once per decision at the root and reused for every candidate
/// move (this module's top doc comment); see [`rival_context`] for the
/// override parameters that let a caller REBUILD one on a trial state
/// without leaking information a deeper search node must not see.
#[derive(Clone, Debug)]
pub struct RivalContext {
    pub rival_culture_rate: i32,
    pub rival_science_rate: i32,
    pub rival_strength: i32,
    /// One `(RivalView, TakeGate)` per live rival. A `Vec`, like
    /// `bots::counting`'s own root-only outputs: this whole struct is built
    /// once per decision, not once per candidate (this module's top doc
    /// comment), so there is no hot-path allocation to avoid here.
    pub rival_views: Vec<(RivalView, TakeGate)>,
    /// Per-rival `(culture, science, strength)`, indexed by player index --
    /// `(0, 0, 0)` for `idx` itself and for any seat that is resigned or
    /// unused beyond `state.num_players`. A fixed array keyed by the same
    /// index space `PlayerState::wars_declared_on_me` uses elsewhere in this
    /// crate, not a `HashMap`: `deferred_credit`'s partner-gains merge
    /// (unowned, `features()`) needs a SPECIFIC rival's rates, not just the
    /// max.
    pub rival_rates: [(i32, i32, i32); MAX_PLAYERS],
    pub root_row: CardList<ROW_SIZE>,
    pub civil_outlook: Vec<(CardId, f64)>,
    pub event_pool: (Vec<(CardId, u16)>, f64),
}

/// `rival_context(state, idx, root_row=None, root_counts=None)`.
///
/// `root_row`/`root_counts`, when `Some`, MUST be the values a previous call
/// at the search ROOT already returned (`ctx.root_row`, `(ctx.civil_outlook,
/// ctx.event_pool)`) -- every caller rebuilding this context on a trial state
/// mid-search passes both, for the reason Python's own docstring gives:
/// recomputing either from the trial state's own row/decks is exactly the
/// information leak `root_row_budget` and `bots::counting` exist to prevent.
pub fn rival_context(
    state: &GameState,
    idx: u8,
    root_row: Option<CardList<ROW_SIZE>>,
    root_counts: Option<(Vec<(CardId, f64)>, (Vec<(CardId, u16)>, f64))>,
) -> RivalContext {
    let (civil_outlook, event_pool) =
        root_counts.unwrap_or_else(|| (counting::civil_outlook(state, idx), counting::event_pool(state, idx)));

    let mut best_rate = 0;
    let mut best_sci = 0;
    let mut best_str = 0;
    let mut rival_rates = [(0i32, 0i32, 0i32); MAX_PLAYERS];
    let mut views = Vec::new();
    for q in state.players[..state.num_players as usize].iter() {
        if q.idx == idx || q.resigned {
            continue;
        }
        let s = effects::compute(state, q);
        best_rate = best_rate.max(s.culture);
        best_sci = best_sci.max(s.science);
        best_str = best_str.max(s.strength);
        // PER-RIVAL, not just the max: `deferred_credit`'s partner-gains
        // merge needs to know what a SPECIFIC rival already produces to work
        // out what offering them a pact would do to the maximum. Free here
        // -- `effects::compute` already ran -- and impossible to reconstruct
        // downstream without paying for it again on every candidate.
        rival_rates[q.idx as usize] = (s.culture, s.science, s.strength);
        // Budgeted at this rival's FULL civil-action allotment (`s.
        // civil_actions`), not what is left of their CURRENT turn
        // (`costs::spare_ca`) -- i.e. what they will be able to reach on
        // THEIR next turn. `costs::take_gate`'s `budget` override exists for
        // exactly this substitution, so this reuses it rather than
        // restating the surcharge/leader-discount arithmetic here.
        let gate = costs::take_gate(state, q, Some(s.civil_actions));
        let hand_slack = (s.civil_actions + s.civil_hand_limit - q.hand_size_civil() as i32).max(0);
        views.push((RivalView::new(q, hand_slack), gate));
    }
    RivalContext {
        rival_culture_rate: best_rate,
        rival_science_rate: best_sci,
        rival_strength: best_str,
        rival_views: views,
        rival_rates,
        root_row: root_row.unwrap_or_else(|| root_row_budget(state)),
        civil_outlook,
        event_pool,
    }
}

// ========================================================== rival_strength

/// `rival_context(state, idx)["rival_strength"]`, on its own -- Python's
/// `rival_strength`.
///
/// `card_potential` (unowned, `cards.rs`) is not handed a whole
/// [`RivalContext`] -- `hand_potential`/`_hand_total` carry no `ctx` -- and
/// building one per card priced would recompute every opponent's full
/// statistics several times per candidate move. This is the one field of it
/// [`strength_marginal`] needs, off [`effects::state_stats`] rather than a
/// bare `compute`.
///
/// A SECOND SPELLING OF ONE QUANTITY, so it is guarded like one:
/// `rival_strength_agrees_with_rival_context` below checks this against
/// `RivalContext::rival_strength` directly; the (since-retired) differential
/// test in `rust/tests/weighted_rivals.rs` used to check the same invariant
/// against real sampled states, the same device Python's
/// `TestRivalStrengthAgrees` uses.
pub fn rival_strength(state: &GameState, idx: u8) -> i32 {
    let mut best = 0;
    for q in state.players[..state.num_players as usize].iter() {
        if q.idx == idx || q.resigned {
            continue;
        }
        let s = effects::state_stats(state, q).strength;
        if s > best {
            best = s;
        }
    }
    best
}

/// The weakest live rival's player index, or `None` when `idx` has no rival
/// left (every other seat resigned). Strength is a public rating-track
/// marker (RULES_SPEC 1.2: "4 rating markers: science, culture, strength,
/// happiness"), so reading every rival's is exactly as legal as
/// [`rival_strength`] reading the STRONGEST one already is -- this is the
/// same quantity, minimised instead of maximised.
///
/// `cards::aggression_value`/`cards::war_hand_value` (docs/OPEN_ITEMS.md item
/// 2) both need the weakest rival specifically, for two different rules
/// reasons that happen to name the same target: RULES_SPEC 5.4.2 makes a
/// strictly-weaker rival the only LEGAL aggression target at all, and a war
/// (no legality gate, RULES_SPEC 5.6) is simply most profitable in
/// expectation against whoever the current margin already favours most.
pub fn weakest_rival(state: &GameState, idx: u8) -> Option<u8> {
    let mut best: Option<(u8, i32)> = None;
    for q in state.players[..state.num_players as usize].iter() {
        if q.idx == idx || q.resigned {
            continue;
        }
        let s = effects::state_stats(state, q).strength;
        if best.is_none_or(|(_, bs)| s < bs) {
            best = Some((q.idx, s));
        }
    }
    best.map(|(i, _)| i)
}

// ========================================================= marginal pricing

/// What ONE point of strength is worth to `evaluate` (unowned) on THIS
/// board -- Python's `strength_marginal`. d(`evaluate`)/d(`features()
/// ["strength"]`), evaluated exactly, not a proxy for it and not a constant:
/// the board expresses a point of strength through FOUR features
/// (`strength`, `strength_rel`, `strength_deficit`, `strength_lead`), and a
/// card pricer that only looks up `w[strength]` under-counts a unit's gain by
/// 2.3x-7x depending on the board (docs/ANALYSIS_HISTORY.md, CARD_BLINDNESS.md
/// verdict). See
/// Python's own docstring (`engine/bots/weighted.py:950`) for the exact
/// derivative of each of the four channels; not reproduced here.
///
/// This is a LOCAL derivative and is honest about being one: it does not see
/// the clamp at `s.strength = max(0, ...)`, and it treats the deficit/lead
/// kink at `rel == 0` as right-differentiable. Both are exact everywhere
/// except at the kink itself.
pub fn strength_marginal(state: &GameState, idx: u8, w: &Weights, ctx: Option<&RivalContext>) -> f64 {
    let rival = match ctx {
        Some(c) => c.rival_strength,
        None => rival_strength(state, idx),
    };
    let p = &state.players[idx as usize];
    let rel = effects::state_stats(state, p).strength - rival;
    let late = horizon::lateness(state);
    let mut total = w.get(WeightKey::Strength) + w.get(WeightKey::StrengthRel);
    total += (1.0 - late) * w.get(WeightKey::StrengthRel.early());
    total += late * w.get(WeightKey::StrengthRel.late());
    if rel < 0 {
        total -= w.get(WeightKey::StrengthDeficit);
    } else if rel < 6 {
        total += w.get(WeightKey::StrengthLead);
    }
    total
}

/// What ONE point of happiness is worth to `evaluate` on THIS board -- the
/// same clamp-aware treatment [`strength_marginal`] gives `strength_lead`,
/// applied to `features()`'s OWN clamp on `happy_margin` (`margin.min(3.0)`,
/// `features.rs`). Priced linearly through the bare `w[happy_margin]` (the
/// bug this function fixes), a card pricer would value the margin's 6th
/// point exactly as much as its 1st, even though `features()` cannot show
/// the 6th one at all -- once `margin >= 3` the feature is already pinned at
/// its ceiling and one more happy face buys `evaluate` nothing.
///
/// A LOCAL derivative, like `strength_marginal`, and right-differentiable at
/// the clamp for the same reason: `margin == 3.0` already reads as clamped
/// (`3.0.min(3.0) == 3.0`, so one more point still cannot move it), matching
/// `margin.min(3.0)`'s own tie-break.
pub fn happy_margin_marginal(state: &GameState, idx: u8, w: &Weights) -> f64 {
    let p = &state.players[idx as usize];
    let s = effects::state_stats(state, p);
    let dc = deferred_credit(state, idx);
    let margin = f64::from(s.happy) - f64::from(economy::happy_required(p.yellow_bank)) + dc.gains.get(GainFeature::Happy);
    if margin >= 3.0 {
        0.0
    } else {
        w.get(WeightKey::HappyMargin)
    }
}

/// What ONE unit of `features()[key]` (unowned) is worth to `evaluate` on
/// THIS board -- Python's `feature_marginal`, `strength_marginal`
/// generalised. `evaluate` prices a [`horizon::PHASE_KEYS`] feature at `w[k]
/// + (1 - L) * w[k_early] + L * w[k_late]`, so a card pricer that looked up
/// the bare `w[k]` (the historical bug this function's Python docstring
/// measures) read a card's yield through a number `evaluate` does not use.
///
/// `key` is a real [`WeightKey`], not a string: every call site in this
/// crate's `bots::weighted` tree prices a card/government/action through
/// `board_yields`-derived triples whose keys are already real weight names
/// (`_TERR_TO_FEATURE`/`_EFF_TO_FEATURE`/`_PROD_TO_FEATURE` in Python all
/// resolve to one before it ever reaches `feature_marginal`) -- `"happy"`,
/// the one [`GainFeature`] that is NOT a `WeightKey`, is read directly out of
/// [`Gains`] by `features()`, never routed through here. `WeightKey::Strength`
/// and `WeightKey::HappyMargin` are each delegated to their own clamp-aware
/// marginal ([`strength_marginal`], [`happy_margin_marginal`]) rather than
/// special-cased inline here.
///
/// `_PHASE_SET` needs no port: [`horizon::PHASE_KEYS`] (four elements) is
/// already the honest source of "which keys carry a phase pair", and a
/// four-element linear scan is not slower than hashing into a set would be.
pub fn feature_marginal(
    key: WeightKey,
    state: &GameState,
    idx: u8,
    w: &Weights,
    late: Option<f64>,
    ctx: Option<&RivalContext>,
) -> f64 {
    if key == WeightKey::Strength {
        return strength_marginal(state, idx, w, ctx);
    }
    if key == WeightKey::HappyMargin {
        return happy_margin_marginal(state, idx, w);
    }
    let mut m = w.get(key);
    if PHASE_KEYS.contains(&key) {
        let late = late.unwrap_or_else(|| horizon::lateness(state));
        m += (1.0 - late) * w.get(key.early());
        m += late * w.get(key.late());
    }
    // THE RATE HORIZON. `evaluate` prices a rate at `blend * horizon`, so its
    // marginal is too -- and because this function is the single definition
    // of "what one unit of this feature is worth", every card-pricing site
    // picks the horizon up for free and cannot disagree with `evaluate`
    // about it.
    if horizon::RATE_KEYS.contains(&key) {
        let n = horizon::live_count(state);
        let hz = horizon::rate_multiplier(state, w, n);
        if hz != 1.0 {
            m *= hz;
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game as G;
    use crate::state::Colonize;

    fn card(name: &str) -> CardId {
        CardId::by_name(name).unwrap_or_else(|| panic!("no such card: {name}"))
    }

    /// `rival_board` on a state with no rivals (single seat live) is all
    /// zero -- Python's own no-rivals early return.
    #[test]
    fn rival_board_is_zero_with_no_live_rivals() {
        let mut state = G::new_game(2, 21);
        state.players[1].resigned = true;
        let b = rival_board(&state, 0);
        assert_eq!(b, RivalBoard::default());
    }

    /// `deferred_credit` on a state with nothing pending returns every field
    /// at its zero default -- the early-return path.
    #[test]
    fn deferred_credit_is_default_with_nothing_pending() {
        let state = G::new_game(2, 22);
        assert!(state.pending.is_empty());
        let dc = deferred_credit(&state, 0);
        assert_eq!(dc, DeferredCredit::default());
    }

    /// `GainFeature::ALL` has no duplicate name and covers every variant --
    /// the invariant the (since-retired) `rust/tests/weighted_rivals.rs`
    /// relied on to compare Python's `gains` dict against `Gains` by name in
    /// both directions.
    #[test]
    fn gain_feature_all_names_are_unique_and_complete() {
        use std::collections::HashSet;
        let names: HashSet<&str> = GainFeature::ALL.iter().map(|k| k.name()).collect();
        assert_eq!(names.len(), GainFeature::ALL.len());
        assert_eq!(GainFeature::ALL.len(), N_GAIN_FEATURES);
    }

    /// `Gains` round-trips through `get`/`add` per key, and an unrelated key
    /// stays zero -- the array-index arithmetic `GainFeature as usize` relies
    /// on.
    #[test]
    fn gains_get_add_round_trips_and_is_isolated_per_key() {
        let mut g = Gains::default();
        g.add(GainFeature::CultureRate, 3.0);
        g.add(GainFeature::CultureRate, 1.5);
        assert_eq!(g.get(GainFeature::CultureRate), 4.5);
        assert_eq!(g.get(GainFeature::Happy), 0.0);
        assert_eq!(g.get(GainFeature::HandMilitary), 0.0);
    }

    /// `root_row_budget` preserves row order and drops empty slots -- built
    /// from a fresh deal, which is dense (no empty slots yet), so this pins
    /// the order-preservation half directly against `state.card_row`.
    #[test]
    fn root_row_budget_preserves_row_order() {
        let state = G::new_game(3, 23);
        let names: Vec<CardId> = root_row_budget(&state).as_slice().to_vec();
        let expected: Vec<CardId> = state.card_row.iter().copied().filter(|c| !c.is_none()).collect();
        assert_eq!(names, expected);
    }

    /// `rival_strength` (the standalone spelling) agrees with
    /// `RivalContext::rival_strength` (the batched spelling) on a real deal
    /// -- guards the "second spelling of one quantity" invariant this
    /// module's own doc comment on `rival_strength` calls out, the same
    /// invariant the (since-retired) `rust/tests/weighted_rivals.rs` used to
    /// check across sampled fixtures.
    #[test]
    fn rival_strength_agrees_with_rival_context() {
        for n in [2u8, 3, 4] {
            let state = G::new_game(n, 24);
            for idx in 0..n {
                let ctx = rival_context(&state, idx, None, None);
                assert_eq!(rival_strength(&state, idx), ctx.rival_strength, "{n}p idx {idx}");
            }
        }
    }

    /// `feature_marginal` on a non-phase, non-rate key is exactly `w.get`.
    #[test]
    fn feature_marginal_on_a_plain_key_is_the_bare_weight() {
        let state = G::new_game(2, 25);
        let mut w = Weights::default();
        w.set(WeightKey::Colonies, 7.0);
        assert_eq!(feature_marginal(WeightKey::Colonies, &state, 0, &w, None, None), 7.0);
    }

    /// `feature_marginal(WeightKey::Strength, ...)` delegates to
    /// `strength_marginal` rather than reading the bare `strength` weight.
    #[test]
    fn feature_marginal_on_strength_delegates() {
        let state = G::new_game(2, 26);
        let w = Weights::default();
        let direct = strength_marginal(&state, 0, &w, None);
        let via_marginal = feature_marginal(WeightKey::Strength, &state, 0, &w, None, None);
        assert_eq!(direct, via_marginal);
        assert_ne!(direct, w.get(WeightKey::Strength), "strength_marginal must fold in strength_rel and its phase pair too");
    }

    /// A `bid_pass` that settles the LAST bidder pops `Auction` and pushes
    /// `Colonize` in the same move (`interact::end_auction_round`) -- so once
    /// `Colonize` is on top of `state.pending`, the `Auction` arm above never
    /// sees this decision again, and unless a sibling arm exists for it, a bot
    /// that just won an auction reads its own winning `bid_pass` as though it
    /// bought nothing. Share is 1.0, not the `Auction` arm's
    /// `1 / (1 + rivals_still_bidding)`: `interact::colonize` is only ever
    /// called with the single settled winner, so there is no more "chance of
    /// winning" left to discount by.
    #[test]
    fn a_won_colonize_credits_the_colony_at_full_share() {
        let mut state = G::new_game(2, 30);
        let territory = card("Developed Territory (I)");
        state.pending.push(Pending::Colonize(Colonize {
            player: 0,
            card: territory,
            bid: 3,
            units: CardList::new(),
            bonuses: CardList::new(),
            discards: CardList::new(),
            pool: CardList::new(),
            bpool: CardList::new(),
            dpool: CardList::new(),
        }));
        let dc = deferred_credit(&state, 0);
        assert_eq!(dc.auction_committed, 1.0);
        assert_eq!(dc.auction_bid, 3.0);
        // "Developed Territory (I)": permanentEffects {yellowTokens: 1,
        // blueTokens: 1}, immediateEffects {science: 3} -- the same
        // `add_permanent_effects`/`add_immediate_effects` calls the `Auction`
        // arm makes, just at share 1.0 instead of a fraction.
        assert_eq!(dc.gains.get(GainFeature::YellowBank), 1.0);
        assert_eq!(dc.gains.get(GainFeature::BlueFree), 1.0);
        assert_eq!(dc.gains.get(GainFeature::Science), 3.0);
    }

    /// A rival's pending `Colonize` (or the idle-player early return) must
    /// not credit `idx` -- the `c.player == idx` guard on the new arm, the
    /// same ownership check the `Auction` arm makes with `auc.high ==
    /// Some(idx)`.
    #[test]
    fn a_rivals_pending_colonize_credits_nothing_to_idx() {
        let mut state = G::new_game(2, 31);
        state.pending.push(Pending::Colonize(Colonize {
            player: 1,
            card: card("Developed Territory (I)"),
            bid: 3,
            units: CardList::new(),
            bonuses: CardList::new(),
            discards: CardList::new(),
            pool: CardList::new(),
            bpool: CardList::new(),
            dpool: CardList::new(),
        }));
        let dc = deferred_credit(&state, 0);
        assert_eq!(dc, DeferredCredit::default());
    }

    /// `feature_marginal(WeightKey::HappyMargin, ...)` delegates to
    /// [`happy_margin_marginal`] rather than reading the bare `happy_margin`
    /// weight -- and once `margin` (`features()`'s own `s.happy -
    /// happy_required(..)`) is already at or past its `min(3.0)` ceiling, one
    /// more point of happiness cannot move the feature, so the marginal must
    /// read 0.0 even though the weight itself is nonzero.
    #[test]
    fn feature_marginal_on_happy_margin_is_zero_once_the_margin_is_already_clamped() {
        let mut state = G::new_game(2, 32);
        state.players[0].yellow_bank = 17; // happy_required(17) == 0
        state.players[0].happy_extra = 100; // clamps to s.happy == 8, margin == 8
        let mut w = Weights::default();
        w.set(WeightKey::HappyMargin, 1.2);
        let direct = happy_margin_marginal(&state, 0, &w);
        let via_marginal = feature_marginal(WeightKey::HappyMargin, &state, 0, &w, None, None);
        assert_eq!(direct, via_marginal);
        assert_eq!(direct, 0.0, "margin (8) is already past the min(3.0) ceiling `features()` reports");
    }

    /// Below the clamp, `happy_margin`'s marginal is exactly the bare weight
    /// -- the same "no phase pair, no rate horizon" shape `feature_marginal`
    /// gives any other plain key, confirming the new special case does not
    /// change pricing where the feature can still move.
    #[test]
    fn feature_marginal_on_happy_margin_is_the_bare_weight_below_the_clamp() {
        let mut state = G::new_game(2, 33);
        state.players[0].yellow_bank = 17; // happy_required(17) == 0
        state.players[0].happy_extra = -100; // clamps to s.happy == 0, margin == 0
        let mut w = Weights::default();
        w.set(WeightKey::HappyMargin, 1.2);
        assert_eq!(feature_marginal(WeightKey::HappyMargin, &state, 0, &w, None, None), 1.2);
    }
}
